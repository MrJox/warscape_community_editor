//! The D3D11 renderer backend: the pipeline of Dear ImGui's `imgui_impl_dx11.cpp` (orthographic vertex shader, one texture, alpha blending,
//! scissor clipping), with managed textures (the font atlas and anything else Dear ImGui creates and updates) and user textures (a render
//! target's view shown with `Ui::image`).

use std::collections::HashMap;
use std::ffi::CString;

use dear_imgui_rs::Context;
use dear_imgui_rs::render::{DrawCmd, DrawData, DrawIdx, DrawVert, PendingFrame, SynchronousRendererConsumer, TextureFeedback, TextureOp};
use dear_imgui_rs::texture::{TextureFormat, TextureId};
use dear_imgui_rs::BackendFlags;
use wce_d3d11::*;
use windows_core::PCSTR;

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
    let (entry_c, target_c) = (CString::new(entry).unwrap_or_default(), CString::new(target).unwrap_or_default());
    let (mut code, mut errors) = (None, None);
    // SAFETY: the source, names and outputs are alive during the call.
    let result = unsafe {
        D3DCompile(SHADERS.as_ptr().cast(), SHADERS.len(), PCSTR(c"imgui".as_ptr().cast()), None, None, PCSTR(entry_c.as_ptr().cast()),
            PCSTR(target_c.as_ptr().cast()), 0, 0, &raw mut code, &raw mut errors)
    };

    let bytes = |blob: &ID3D10Blob| {
        // SAFETY: the blob's buffer is valid for its size while the blob lives.
        unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize()).to_vec() }
    };

    match (result.ok(), code) {
        (Ok(()), Some(code)) => Ok(bytes(&code)),
        _ => Err(RendererError(errors.as_ref().map(|e| String::from_utf8_lossy(&bytes(e)).into_owned()).unwrap_or_else(|| format!("{entry}: compile failed")))),
    }
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
        let element = |name: &'static std::ffi::CStr, format, offset| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(name.as_ptr().cast()), SemanticIndex: 0, Format: format, InputSlot: 0, AlignedByteOffset: offset,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA, InstanceDataStepRate: 0,
        };

        let elements = [element(c"POSITION", DXGI_FORMAT_R32G32_FLOAT, 0), element(c"TEXCOORD", DXGI_FORMAT_R32G32_FLOAT, 8), element(c"COLOR", DXGI_FORMAT_R8G8B8A8_UNORM, 16)];
        let (mut vs, mut ps, mut layout, mut cb, mut blend, mut raster, mut depth, mut linear, mut nearest) = (None, None, None, None, None, None, None, None, None);
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
        // SAFETY: creation calls with valid descriptions and code.
        unsafe {
            dev.CreateVertexShader(&vs_code, None, Some(&mut vs)).ok()?;
            dev.CreatePixelShader(&ps_code, None, Some(&mut ps)).ok()?;
            dev.CreateInputLayout(&elements, &vs_code, Some(&mut layout)).ok()?;
            dev.CreateBuffer(&cb_desc, None, Some(&mut cb)).ok()?;
            dev.CreateBlendState(&blend_desc, Some(&mut blend)).ok()?;
            dev.CreateRasterizerState(&raster_desc, Some(&mut raster)).ok()?;
            dev.CreateDepthStencilState(&depth_desc, Some(&mut depth)).ok()?;
            dev.CreateSamplerState(&sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR), Some(&mut linear)).ok()?;
            dev.CreateSamplerState(&sampler(D3D11_FILTER_MIN_MAG_MIP_POINT), Some(&mut nearest)).ok()?;
        }

        let missing = || RendererError("a pipeline object was not created".to_owned());
        Ok(Self {
            dev: dev.clone(), ctx: ctx.clone(), consumer, vs: vs.ok_or_else(missing)?, ps: ps.ok_or_else(missing)?, layout: layout.ok_or_else(missing)?, cb: cb.ok_or_else(missing)?,
            blend: blend.ok_or_else(missing)?, raster: raster.ok_or_else(missing)?, depth: depth.ok_or_else(missing)?, linear: linear.ok_or_else(missing)?,
            nearest: nearest.ok_or_else(missing)?, vb: None, ib: None, vb_size: 0, ib_size: 0, textures: HashMap::new(), managed: HashMap::new(), next_id: 1,
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

        let init = D3D11_SUBRESOURCE_DATA { pSysMem: rgba.as_ptr().cast(), SysMemPitch: width * 4, SysMemSlicePitch: 0 };
        let (mut tex, mut srv) = (None, None);
        // SAFETY: creation calls; the pixels outlive the call.
        unsafe {
            self.dev.CreateTexture2D(&desc, Some(&init), Some(&mut tex)).ok()?;
            let t = tex.as_ref().ok_or_else(|| RendererError("no texture".to_owned()))?;
            self.dev.CreateShaderResourceView(t, None, Some(&mut srv)).ok()?;
        }

        let id = self.next_id;
        self.next_id += 1;
        self.textures.insert(id, Texture { tex, srv: srv.ok_or_else(|| RendererError("no view".to_owned()))? });
        Ok(id)
    }

    fn update_texture(&mut self, id: u64, format: TextureFormat, rects: &[dear_imgui_rs::render::TextureUploadRect]) {
        let Some(tex) = self.textures.get(&id).and_then(|t| t.tex.clone()) else { return };
        for r in rects {
            let rgba = to_rgba(format, &r.data);
            let b = D3D11_BOX { left: u32::from(r.rect.x), top: u32::from(r.rect.y), front: 0, right: u32::from(r.rect.x + r.rect.w), bottom: u32::from(r.rect.y + r.rect.h), back: 1 };
            // SAFETY: the box is inside the texture (Dear ImGui's rectangle) and the pixels cover it.
            unsafe { self.ctx.UpdateSubresource(&tex, 0, Some(&b), rgba.as_ptr().cast(), u32::from(r.rect.w) * 4, 0) };
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
            let mut b = None;
            // SAFETY: a plain creation call.
            unsafe { dev.CreateBuffer(&desc, None, Some(&mut b)).ok()? };
            Ok(b)
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
        // SAFETY: state calls with objects of this device; the constant buffer is written while mapped.
        unsafe {
            let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
            if self.ctx.Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut ms)).is_ok() {
                std::ptr::copy_nonoverlapping(mvp.as_ptr(), ms.pData.cast::<f32>(), 16);
                self.ctx.Unmap(&self.cb, 0);
            }

            self.ctx.RSSetViewports(Some(&[viewport]));
            self.ctx.IASetInputLayout(&self.layout);
            self.ctx.IASetVertexBuffers(0, 1, Some(&Some(vb.clone())), Some(&(size_of::<DrawVert>() as u32)), Some(&0));
            self.ctx.IASetIndexBuffer(ib, index_format, 0);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.VSSetShader(&self.vs, None);
            self.ctx.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            self.ctx.PSSetShader(&self.ps, None);
            self.ctx.PSSetSamplers(0, Some(&[Some(self.linear.clone())]));
            self.ctx.GSSetShader(None, None);
            self.ctx.HSSetShader(None, None);
            self.ctx.DSSetShader(None, None);
            self.ctx.CSSetShader(None, None);
            self.ctx.OMSetBlendState(&self.blend, Some(&[0.0; 4]), 0xffff_ffff);
            self.ctx.OMSetDepthStencilState(&self.depth, 0);
            self.ctx.RSSetState(&self.raster);
        }
    }

    fn draw(&mut self, dd: &DrawData) -> Result<(), RendererError> {
        let [w, h] = dd.display_size();
        if w <= 0.0 || h <= 0.0 || dd.total_vtx_count() == 0 {
            return Ok(());
        }

        self.ensure_buffers(dd.total_vtx_count(), dd.total_idx_count())?;
        let (Some(vb), Some(ib)) = (self.vb.clone(), self.ib.clone()) else { return Ok(()) };

        // Every list into one vertex and one index buffer.
        // SAFETY: the buffers hold the totals (ensure_buffers) and are written while mapped.
        unsafe {
            let (mut vm, mut im) = (D3D11_MAPPED_SUBRESOURCE::default(), D3D11_MAPPED_SUBRESOURCE::default());
            self.ctx.Map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut vm)).ok()?;
            self.ctx.Map(&ib, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut im)).ok()?;
            let (mut vdst, mut idst) = (vm.pData.cast::<DrawVert>(), im.pData.cast::<DrawIdx>());
            for list in dd.draw_lists() {
                let (v, i) = (list.vtx_buffer(), list.idx_buffer());
                std::ptr::copy_nonoverlapping(v.as_ptr(), vdst, v.len());
                std::ptr::copy_nonoverlapping(i.as_ptr(), idst, i.len());
                vdst = vdst.add(v.len());
                idst = idst.add(i.len());
            }

            self.ctx.Unmap(&vb, 0);
            self.ctx.Unmap(&ib, 0);
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
                        // SAFETY: drawing calls within the buffers filled above.
                        unsafe {
                            self.ctx.RSSetScissorRects(Some(&[scissor]));
                            self.ctx.PSSetShaderResources(0, Some(&[srv]));
                            self.ctx.DrawIndexed(count as u32, (cmd_params.idx_offset + idx_base) as u32, (cmd_params.vtx_offset + vtx_base) as i32);
                        }
                    }

                    DrawCmd::ResetRenderState => self.setup_state(dd, &vb, &ib),
                    // SAFETY: a state call.
                    DrawCmd::SetSamplerLinear => unsafe { self.ctx.PSSetSamplers(0, Some(&[Some(self.linear.clone())])) },
                    // SAFETY: a state call.
                    DrawCmd::SetSamplerNearest => unsafe { self.ctx.PSSetSamplers(0, Some(&[Some(self.nearest.clone())])) },
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
