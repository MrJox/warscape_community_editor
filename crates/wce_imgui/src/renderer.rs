//! The D3D11 renderer backend: the pipeline of Dear ImGui's `imgui_impl_dx11.cpp` (orthographic vertex shader, one texture, alpha blending,
//! scissor clipping), with managed textures (the font atlas and anything else Dear ImGui creates and updates) and user textures (a render
//! target's view shown with `Ui::image`).

use std::collections::HashMap;

use dear_imgui_rs::Context;
use dear_imgui_rs::render::{DrawCmd, DrawData, DrawIdx, DrawVert, PendingFrame, SynchronousRendererConsumer, TextureFeedback, TextureOp};
use dear_imgui_rs::texture::{TextureFormat, TextureId};
use dear_imgui_rs::BackendFlags;
use wce_d3d11::*;

const SHADERS: &str = r#"
cbuffer vertexBuffer : register(b0) { float4x4 ProjectionMatrix; };
struct VS_INPUT { float2 pos : POSITION; float4 col : COLOR0; float2 uv : TEXCOORD0; };
struct PS_INPUT { float4 pos : SV_POSITION; float4 col : COLOR0; float2 uv : TEXCOORD0; };
PS_INPUT vs(VS_INPUT input) {
    PS_INPUT output;
    output.pos = mul(ProjectionMatrix, float4(input.pos.xy, 0.f, 1.f));
    output.col = input.col;
    output.uv = input.uv;
    return output;
}
sampler sampler0;
Texture2D texture0;
float4 ps(PS_INPUT input) : SV_Target { return input.col * texture0.Sample(sampler0, input.uv); }
"#;

/// An error of the renderer backend, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererError(pub String);

impl std::fmt::Display for RendererError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RendererError {}

impl From<windows_core::Error> for RendererError {
    fn from(error: windows_core::Error) -> Self {
        Self(error.to_string())
    }
}

fn compile(entry: &str, target: &str) -> Result<Vec<u8>, RendererError> {
    wce_d3d11::safe::compile(SHADERS, "imgui", entry, target).map_err(RendererError)
}

/// The bytes of vertices: `DrawVert` is `#[repr(C)]` floats and a u32, without padding.
fn vertex_bytes(v: &[DrawVert]) -> &[u8] {
    const _: () = assert!(size_of::<DrawVert>() == 20);
    // SAFETY: plain data without padding, read as bytes for the slice's lifetime.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), size_of_val(v)) }
}

/// A texture the renderer owns (a managed one) or shows (a user one).
struct Texture {
    tex: Option<ID3D11Texture2D>,
    srv: ID3D11ShaderResourceView,
}

pub struct D3D11Renderer {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    consumer: SynchronousRendererConsumer,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    cb: ID3D11Buffer,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    depth: ID3D11DepthStencilState,
    linear: ID3D11SamplerState,
    nearest: ID3D11SamplerState,
    vb: Option<ID3D11Buffer>,
    ib: Option<ID3D11Buffer>,
    vb_size: usize,
    ib_size: usize,
    textures: HashMap<u64, Texture>,

    /// Dear ImGui's managed textures by the id of their requests.
    managed: HashMap<dear_imgui_rs::render::SnapshotTextureId, u64>,
    next_id: u64,
}

impl D3D11Renderer {

    /// The renderer on a device; tells the context that it creates and updates textures itself.
    ///
    /// # Errors
    ///
    /// The shaders or the pipeline states can't be created.
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, imgui: &mut Context) -> Result<Self, RendererError> {
        let io = imgui.io_mut();
        io.set_backend_flags(io.backend_flags() | BackendFlags::RENDERER_HAS_TEXTURES | BackendFlags::RENDERER_HAS_VTX_OFFSET);
        let _ = imgui.set_renderer_name(Some("wce_d3d11"));
        let consumer = imgui.create_synchronous_renderer_consumer().map_err(|e| RendererError(e.to_string()))?;
        let (vs_code, ps_code) = (compile("vs", "vs_4_0")?, compile("ps", "ps_4_0")?);
        let element = |name, format, offset| InputElement::new(name, 0, format, 0, offset, false);

        let elements = [element(c"POSITION", DXGI_FORMAT_R32G32_FLOAT, 0), element(c"TEXCOORD", DXGI_FORMAT_R32G32_FLOAT, 8), element(c"COLOR", DXGI_FORMAT_R8G8B8A8_UNORM, 16)];
        let blend_desc = D3D11_BLEND_DESC {
            AlphaToCoverageEnable: false.into(), IndependentBlendEnable: false.into(),
            RenderTarget: [D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD, SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
            }; 8],
        };

        let raster_desc = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, ScissorEnable: true.into(), DepthClipEnable: true.into(), ..Default::default() };
        let depth_desc = D3D11_DEPTH_STENCIL_DESC { DepthEnable: false.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_ALWAYS, ..Default::default() };
        let sampler = |filter| D3D11_SAMPLER_DESC {
            Filter: filter, AddressU: D3D11_TEXTURE_ADDRESS_CLAMP, AddressV: D3D11_TEXTURE_ADDRESS_CLAMP, AddressW: D3D11_TEXTURE_ADDRESS_CLAMP, ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            ..Default::default()
        };

        let cb_desc = D3D11_BUFFER_DESC { ByteWidth: 64, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, ..Default::default() };
        Ok(Self {
            dev: dev.clone(), ctx: ctx.clone(), consumer, vs: dev.create_vertex_shader(&vs_code)?, ps: dev.create_pixel_shader(&ps_code)?,
            layout: dev.create_input_layout(&elements, &vs_code)?, cb: dev.create_buffer(&cb_desc, None)?, blend: dev.create_blend_state(&blend_desc)?,
            raster: dev.create_rasterizer_state(&raster_desc)?, depth: dev.create_depth_stencil_state(&depth_desc)?,
            linear: dev.create_sampler_state(&sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR))?, nearest: dev.create_sampler_state(&sampler(D3D11_FILTER_MIN_MAG_MIP_POINT))?,
            vb: None, ib: None, vb_size: 0, ib_size: 0, textures: HashMap::new(), managed: HashMap::new(), next_id: 1,
        })
    }

    /// The consumer to render the context's frames with.
    pub fn consumer(&self) -> &SynchronousRendererConsumer {
        &self.consumer
    }

    /// A shader resource view to show with `Ui::image` (a render target's); the id stays valid until `unregister`.
    pub fn register(&mut self, srv: &ID3D11ShaderResourceView) -> TextureId {
        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, Texture { tex: None, srv: srv.clone() });
        TextureId::new(id)
    }

    /// Points a registered id at another view (the render target was recreated).
    pub fn replace(&mut self, id: TextureId, srv: &ID3D11ShaderResourceView) {
        self.textures.insert(id.id(), Texture { tex: None, srv: srv.clone() });
    }

    pub fn unregister(&mut self, id: TextureId) {
        self.textures.remove(&id.id());
    }

    fn create_texture(&mut self, format: TextureFormat, width: u32, height: u32, pixels: &[u8]) -> Result<u64, RendererError> {
        let rgba = to_rgba(format, pixels);
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width, Height: height, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let tex = self.dev.create_texture_2d(&desc, Some(&[InitData::new(&rgba, width * 4)]))?;
        let srv = self.dev.create_shader_resource_view(&tex, None)?;
        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, Texture { tex: Some(tex), srv });
        Ok(id)
    }

    fn update_texture(&mut self, id: u64, format: TextureFormat, rects: &[dear_imgui_rs::render::TextureUploadRect]) {
        let Some(tex) = self.textures.get(&id).and_then(|t| t.tex.clone()) else { return };
        for r in rects {
            let rgba = to_rgba(format, &r.data);
            let b = D3D11_BOX { left: u32::from(r.rect.x), top: u32::from(r.rect.y), front: 0, right: u32::from(r.rect.x + r.rect.w), bottom: u32::from(r.rect.y + r.rect.h), back: 1 };
            let _ = self.ctx.update_subresource(&tex, 0, Some(&b), &rgba, u32::from(r.rect.w) * 4, 0);
        }
    }

    /// Answers the frame's texture requests, then draws it into the bound render target (the caller binds it and sets nothing else).
    ///
    /// # Errors
    ///
    /// A texture or buffer can't be created, or the frame is rejected by its context.
    pub fn render(&mut self, frame: PendingFrame<'_>) -> Result<(), RendererError> {
        let mut feedback: Vec<TextureFeedback> = vec![];
        for request in frame.texture_requests() {
            let answer = match request.operation() {
                TextureOp::Create { format, width, height, pixels, .. } => {
                    let id = self.create_texture(*format, *width, *height, pixels)?;
                    if let Some(old) = self.managed.insert(request.texture(), id) {
                        self.textures.remove(&old);
                    }

                    request.uploaded(TextureId::new(id))
                }

                TextureOp::Update { format, rects, .. } => match self.managed.get(&request.texture()).copied() {
                    Some(id) => {
                        self.update_texture(id, *format, rects);
                        request.uploaded(TextureId::new(id))
                    }

                    None => Ok(request.retry()),
                },

                TextureOp::Destroy => {
                    if let Some(id) = self.managed.remove(&request.texture()) {
                        self.textures.remove(&id);
                    }

                    request.destroyed()
                }
            };

            feedback.push(answer.map_err(|e| RendererError(format!("{e:?}")))?);
        }

        let reconciled = frame.reconcile_texture_feedback(feedback).map_err(|e| RendererError(e.to_string()))?;
        self.draw(reconciled.draw_data())
    }

    fn ensure_buffers(&mut self, vtx: usize, idx: usize) -> Result<(), RendererError> {
        let make = |dev: &ID3D11Device, bytes: usize, bind: i32| -> Result<Option<ID3D11Buffer>, RendererError> {
            let desc = D3D11_BUFFER_DESC { ByteWidth: bytes as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: bind as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, ..Default::default() };
            Ok(Some(dev.create_buffer(&desc, None)?))
        };

        if self.vb.is_none() || self.vb_size < vtx {
            self.vb_size = vtx + 5000;
            self.vb = make(&self.dev, self.vb_size * size_of::<DrawVert>(), D3D11_BIND_VERTEX_BUFFER)?;
        }

        if self.ib.is_none() || self.ib_size < idx {
            self.ib_size = idx + 10000;
            self.ib = make(&self.dev, self.ib_size * size_of::<DrawIdx>(), D3D11_BIND_INDEX_BUFFER)?;
        }

        Ok(())
    }

    fn setup_state(&self, dd: &DrawData, vb: &ID3D11Buffer, ib: &ID3D11Buffer) {
        let [w, h] = dd.display_size();
        let [l, t] = dd.display_pos();
        let (r, b) = (l + w, t + h);
        let mvp: [f32; 16] = [2.0 / (r - l), 0.0, 0.0, 0.0, 0.0, 2.0 / (t - b), 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, (r + l) / (l - r), (t + b) / (b - t), 0.5, 1.0];
        let viewport = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w * dd.framebuffer_scale()[0], Height: h * dd.framebuffer_scale()[1], MinDepth: 0.0, MaxDepth: 1.0 };
        let index_format = if size_of::<DrawIdx>() == 2 { DXGI_FORMAT_R16_UINT } else { DXGI_FORMAT_R32_UINT };
        if let Ok(mut ms) = self.ctx.map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0) {
            ms.write(0, &mvp);
        }

        self.ctx.set_viewports(&[viewport]);
        self.ctx.set_input_layout(Some(&self.layout));
        self.ctx.set_vertex_buffers(0, &[Some(vb.clone())], &[size_of::<DrawVert>() as u32], &[0]);
        self.ctx.set_index_buffer(Some(ib), index_format, 0);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.vs_set_shader(Some(&self.vs));
        self.ctx.vs_set_constant_buffers(0, &[Some(self.cb.clone())]);
        self.ctx.ps_set_shader(Some(&self.ps));
        self.ctx.ps_set_samplers(0, &[Some(self.linear.clone())]);
        self.ctx.gs_set_shader(None);
        self.ctx.hs_set_shader(None);
        self.ctx.ds_set_shader(None);
        self.ctx.cs_set_shader(None);
        self.ctx.set_blend_state_factor(Some(&self.blend), [0.0; 4], 0xffff_ffff);
        self.ctx.set_depth_stencil_state(Some(&self.depth), 0);
        self.ctx.set_rasterizer_state(Some(&self.raster));
    }

    fn draw(&mut self, dd: &DrawData) -> Result<(), RendererError> {
        let [w, h] = dd.display_size();
        if w <= 0.0 || h <= 0.0 || dd.total_vtx_count() == 0 {
            return Ok(());
        }

        self.ensure_buffers(dd.total_vtx_count(), dd.total_idx_count())?;
        let (Some(vb), Some(ib)) = (self.vb.clone(), self.ib.clone()) else { return Ok(()) };

        // Every list into one vertex and one index buffer (they hold the totals: ensure_buffers).
        {
            let (mut vm, mut im) = (self.ctx.map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0)?, self.ctx.map(&ib, 0, D3D11_MAP_WRITE_DISCARD, 0)?);
            let (mut vat, mut iat) = (0, 0);
            for list in dd.draw_lists() {
                let (v, i) = (vertex_bytes(list.vtx_buffer()), list.idx_buffer());
                if !vm.write(vat, v) || !im.write(iat, i) {
                    return Err(RendererError("the draw lists outgrew their buffers".to_owned()));
                }

                vat += v.len();
                iat += size_of_val(i);
            }
        }

        self.setup_state(dd, &vb, &ib);
        let [ox, oy] = dd.display_pos();
        let [sx, sy] = dd.framebuffer_scale();
        let (mut vtx_base, mut idx_base) = (0usize, 0usize);
        for list in dd.draw_lists() {
            for cmd in list.commands() {
                match cmd {
                    DrawCmd::Elements { count, cmd_params } => {
                        let c = cmd_params.clip_rect;
                        let (x0, y0, x1, y1) = ((c[0] - ox) * sx, (c[1] - oy) * sy, (c[2] - ox) * sx, (c[3] - oy) * sy);
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }

                        let scissor = RECT { left: x0 as i32, top: y0 as i32, right: x1 as i32, bottom: y1 as i32 };
                        let srv = self.textures.get(&cmd_params.texture_id.id()).map(|t| t.srv.clone());
                        self.ctx.set_scissor_rects(&[scissor]);
                        self.ctx.ps_set_shader_resources(0, &[srv]);
                        self.ctx.draw_indexed(count as u32, (cmd_params.idx_offset + idx_base) as u32, (cmd_params.vtx_offset + vtx_base) as i32);
                    }

                    DrawCmd::ResetRenderState => self.setup_state(dd, &vb, &ib),
                    DrawCmd::SetSamplerLinear => self.ctx.ps_set_samplers(0, &[Some(self.linear.clone())]),
                    DrawCmd::SetSamplerNearest => self.ctx.ps_set_samplers(0, &[Some(self.nearest.clone())]),
                    // The editor installs no draw callbacks: one is skipped.
                    DrawCmd::RawCallback(raw) => drop(raw),
                }
            }

            vtx_base += list.vtx_buffer().len();
            idx_base += list.idx_buffer().len();
        }

        Ok(())
    }
}

/// Alpha8 textures go up as white with that alpha.
fn to_rgba(format: TextureFormat, pixels: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match format {
        TextureFormat::RGBA32 => std::borrow::Cow::Borrowed(pixels),
        TextureFormat::Alpha8 => std::borrow::Cow::Owned(pixels.iter().flat_map(|&a| [255, 255, 255, a]).collect()),
    }
}
