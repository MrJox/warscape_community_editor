//! The renderer's render targets: the G-buffer set a view is drawn into, the post-processing chain, and the shadow atlas.

use wce_d3d11::*;

use super::SHADOW_SIZE;

/// A texture with its render target and shader resource views.
#[derive(Clone, Debug, Default)]
pub(crate) struct Target {
    pub tex: Option<ID3D11Texture2D>,
    pub rtv: Option<ID3D11RenderTargetView>,
    pub srv: Option<ID3D11ShaderResourceView>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SizedTarget {
    pub t: Target,
    pub w: u32,
    pub h: u32,
}

/// A target of a size and format.
pub(crate) fn make_target(dev: &ID3D11Device, w: u32, h: u32, fmt: DXGI_FORMAT) -> Target {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let Ok(tex) = dev.create_texture_2d(&td, None) else { return Target::default() };
    Target { rtv: dev.create_render_target_view(&tex, None).ok(), srv: dev.create_shader_resource_view(&tex, None).ok(), tex: Some(tex) }
}

/// A target whose size is kept with it (never 0).
fn sized_target(dev: &ID3D11Device, w: u32, h: u32, fmt: DXGI_FORMAT) -> SizedTarget {
    let (w, h) = (w.max(1), h.max(1));
    SizedTarget { t: make_target(dev, w, h, fmt), w, h }
}

/// A depth-stencil texture's view.
pub(crate) fn depth_target(dev: &ID3D11Device, w: u32, h: u32, fmt: DXGI_FORMAT) -> Option<ID3D11DepthStencilView> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_DEPTH_STENCIL as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    dev.create_texture_2d(&td, None).and_then(|tex| dev.create_depth_stencil_view(&tex, None)).ok()
}

/// What a view of the scene is drawn into: the G-buffer channels, the HDR light accumulation and the depth-stencil buffer, at one size.
/// The main view has one at the target's size, the environment cube one at the cube's (the passes draw into whichever is current).
#[derive(Clone, Debug, Default)]
pub(crate) struct GBuffer {
    pub w: u32,
    pub h: u32,

    /// Channels 1..4 + the per-instance id.
    pub channels: [Target; 5],
    pub hdr: Target,
    pub dsv: Option<ID3D11DepthStencilView>,
}

impl GBuffer {

    /// Verified formats (the engine's TTF_* enum = the order of its name strings): G-buffer channels 1-3 TTF_A8R8G8B8 (fn 0x118f4b90), the
    /// depth channel TTF_R32F (R16F on low settings, fn 0x118f62b0), HDR light accumulation TTF_A16B16G16R16F (fns 0x118f4a80, 0x117659b0).
    /// The id channel is editor-only.
    pub fn new(dev: &ID3D11Device, w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            channels: [
                make_target(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // normal (cube-normalised, *0.5+0.5), gloss
                make_target(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // sqrt(albedo), .w mask
                make_target(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM),  // specular colour, .w material id / 255
                make_target(dev, w, h, DXGI_FORMAT_R32_FLOAT),       // z / w
                make_target(dev, w, h, DXGI_FORMAT_R32_UINT),        // per-instance value (uint output, debug layer)
            ],
            hdr: make_target(dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT),
            dsv: depth_target(dev, w, h, DXGI_FORMAT_D24_UNORM_S8_UINT),
        }
    }

    /// The colour targets of the geometry pass.
    pub fn rtvs(&self) -> Vec<Option<ID3D11RenderTargetView>> {
        self.channels.iter().map(|t| t.rtv.clone()).collect()
    }
}

/// The targets after lighting, at the main view's size: SSAO, the tone mapping chain and the final image.
#[derive(Clone, Debug, Default)]
pub(crate) struct PostTargets {
    /// hdr_to_screen's output (its alpha is the refraction mask).
    pub final_: Target,

    /// ao: the full-resolution resolve; ao_tmp / ao_blur: the half-resolution blur.
    pub ao: Target,
    pub ao_tmp: Target,
    pub ao_blur: Target,

    /// What the lighting reads as SSAO this frame (the blur, the cleared resolve when SSAO is off, white for the environment cube).
    pub ao_result: Option<ID3D11ShaderResourceView>,

    // The tone mapping chain (hdr_tone_mapper.hlsl).
    pub quarter: SizedTarget,
    pub bloom_a: SizedTarget,
    pub bloom_b: SizedTarget,
    pub ldr: SizedTarget,

    /// The luma stats pyramid; the last is 1x1.
    pub stats: Vec<SizedTarget>,

    /// The environment specular pass (environment_specular_light, reflections_blur): the reflection, its blur parameters (radius, weight),
    /// the x blur, and the stand-in for the depth-stencil the game's pass reads (the stencil in .y; the pass runs where its low bits are 1).
    pub reflection: Target,
    pub reflection_params: Target,
    pub reflection_blur: Target,

    /// The reflection targets' divisor of the view size (1 full, 2 half: gfx_screen_space_reflections 1).
    pub reflection_div: u32,
    pub stencil: Target,

    /// Thrones of Britannia's god rays (gfx_god_rays): HDR_GOD_RAYS at half the view's size, its x and y blurs at the view's (fn
    /// 0x117a88a0); made when first drawn.
    pub god_rays: SizedTarget,
    pub god_rays_x: SizedTarget,
    pub god_rays_y: SizedTarget,

    /// MLAA (gfx_aa 1; fns 0x11765d60, 0x11765e60): the separating lines' mask and the line lengths (horizontal, vertical; twice, the
    /// iterations alternate), R8_UINT at the view's size; the blended image, copied back into `final_`. Made when first drawn.
    pub mlaa_edges: SizedTarget,
    pub mlaa_counts: [SizedTarget; 4],
    pub mlaa_out: SizedTarget,

    /// The order-independent transparency's buffers (gfx_alpha_blend). Made when first drawn.
    pub oit: OitBuffers,
}

/// The side of the tone mapper's downsampled image (luma statistics, bloom), whatever the view's size.
pub(crate) const TONEMAP_SIZE: u32 = 256;

impl PostTargets {

    /// (Re)creates the reflection targets at the view size divided by `div`.
    pub fn resize_reflection(&mut self, dev: &ID3D11Device, w: u32, h: u32, div: u32) {
        let (w, h) = (w.div_ceil(div).max(1), h.div_ceil(div).max(1));
        self.reflection = make_target(dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.reflection_params = make_target(dev, w, h, DXGI_FORMAT_R8G8_UNORM);
        self.reflection_blur = make_target(dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.reflection_div = div;
    }

    /// (Re)creates the targets for a view size; the SSAO result is kept.
    pub fn resize(&mut self, dev: &ID3D11Device, w: u32, h: u32) {
        self.final_ = make_target(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM);

        // SSAOX_RESOLVE / BLUR_X / BLUR_Y RTs: TTF_R16F (fn 0x11766de0); the blur at half resolution (dump).
        self.ao = make_target(dev, w, h, DXGI_FORMAT_R16_FLOAT);
        self.ao_tmp = make_target(dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);
        self.ao_blur = make_target(dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);

        // The game's "quarter" image is a fixed 256 x 256 (a RenderDoc capture at 1920 x 1080: the luma statistics and the bloom read it),
        // halved down to 1 x 1 by the statistics, whose four point taps at +-1/4 of an output texel cover a 2 x 2 block: a pyramid that
        // shrank faster skipped texels and missed the highlights (the white point stayed at the environment's minimum).
        let (qw, qh) = (TONEMAP_SIZE, TONEMAP_SIZE);
        self.quarter = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);  // rgb + luma in .w
        self.bloom_a = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.bloom_b = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.ldr = sized_target(dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);        // tone-mapped, linear

        // The game's: R11G11B10 reflection and x blur, R8G8 parameters (a RenderDoc capture, docs/re/renderer.md).
        self.resize_reflection(dev, w, h, self.reflection_div.max(1));
        self.stencil = make_target(dev, w, h, DXGI_FORMAT_R8G8_UINT);
        self.stats.clear();
        let (mut sw, mut sh) = (qw, qh);
        loop {
            sw = sw.div_ceil(2);
            sh = sh.div_ceil(2);
            self.stats.push(sized_target(dev, sw, sh, DXGI_FORMAT_R32G32B32A32_FLOAT));  // min, max, avg, max
            if sw <= 1 && sh <= 1 {
                break;
            }
        }
    }
}

/// The cascaded shadow atlas (SHADOW_SIZE), depth only as the game's DX11 path has it (fn 0x117665e0: "SHADOW_MAP_VIEW DB" in
/// TTF_R32_TYPELESS, no colour target; the light pass samples the depth, fn 0x1178ae60).
#[derive(Clone, Debug, Default)]
pub(crate) struct ShadowMap {
    pub dsv: Option<ID3D11DepthStencilView>,
    pub srv: Option<ID3D11ShaderResourceView>,
}

impl ShadowMap {
    pub fn new(dev: &ID3D11Device) -> Self {
        let td = D3D11_TEXTURE2D_DESC {
            Width: SHADOW_SIZE, Height: SHADOW_SIZE, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_TYPELESS, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let Ok(tex) = dev.create_texture_2d(&td, None) else { return Self::default() };
        let dd = D3D11_DEPTH_STENCIL_VIEW_DESC { Format: DXGI_FORMAT_D32_FLOAT, ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D, ..Default::default() };
        let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: DXGI_FORMAT_R32_FLOAT, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2D), ..Default::default() };
        sd.Anonymous.Texture2D.MipLevels = 1;
        Self { dsv: dev.create_depth_stencil_view(&tex, Some(&dd)).ok(), srv: dev.create_shader_resource_view(&tex, Some(&sd)).ok() }
    }
}

/// The order-independent transparency's buffers (gfx_alpha_blend; fn 0x11766000): per pixel the index of its last fragment (a raw buffer,
/// cleared to 0xffffffff, the list's end), and the fragments (12 bytes: packed colour, alpha and depth, the next's index) with an append
/// counter, `OIT_FRAGMENTS_PER_PIXEL` per pixel.
#[derive(Clone, Debug, Default)]
pub(crate) struct OitBuffers {
    pub head_uav: Option<ID3D11UnorderedAccessView>,
    pub head_srv: Option<ID3D11ShaderResourceView>,
    pub fragments_uav: Option<ID3D11UnorderedAccessView>,
    pub fragments_srv: Option<ID3D11ShaderResourceView>,
    pub w: u32,
    pub h: u32,
}

/// D3D11_BUFFER_UAV_FLAG_RAW, D3D11_BUFFER_UAV_FLAG_COUNTER (not in the generated bindings).
const UAV_FLAG_RAW: u32 = 1;
const UAV_FLAG_COUNTER: u32 = 4;
const FRAGMENT_STRIDE: u32 = 12;

impl OitBuffers {
    /// The buffers for a view size; none (all `None`) when the device refuses them.
    pub fn new(dev: &ID3D11Device, w: u32, h: u32, per_pixel: u32) -> Self {
        let pixels = w * h;
        let bind = (D3D11_BIND_UNORDERED_ACCESS | D3D11_BIND_SHADER_RESOURCE) as u32;
        let head = dev.create_buffer(&D3D11_BUFFER_DESC {
            ByteWidth: pixels * 4, Usage: D3D11_USAGE_DEFAULT, BindFlags: bind, MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS as u32, ..Default::default()
        }, None);
        let fragments = dev.create_buffer(&D3D11_BUFFER_DESC {
            ByteWidth: pixels * per_pixel * FRAGMENT_STRIDE, Usage: D3D11_USAGE_DEFAULT, BindFlags: bind,
            MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED as u32, StructureByteStride: FRAGMENT_STRIDE, ..Default::default()
        }, None);
        // Without them the size is kept (not asked for again every frame) and `ready` is false.
        let (Ok(head), Ok(fragments)) = (head, fragments) else { return Self { w, h, ..Self::default() } };

        let uav = |format, n, flags| D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: format, ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 { Buffer: D3D11_BUFFER_UAV { FirstElement: 0, NumElements: n, Flags: flags } },
        };
        let srv = |format, n| {
            let mut d = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_BUFFER), ..Default::default() };
            d.Anonymous.Buffer = D3D11_BUFFER_SRV { Anonymous: D3D11_BUFFER_SRV_0 { FirstElement: 0 }, Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: n } };
            d
        };

        // The pixel shaders write the head through a raw view; the resolve reads it as a Buffer<uint>.
        Self {
            head_uav: dev.create_unordered_access_view(&head, Some(&uav(DXGI_FORMAT_R32_TYPELESS, pixels, UAV_FLAG_RAW))).ok(),
            head_srv: dev.create_shader_resource_view(&head, Some(&srv(DXGI_FORMAT_R32_UINT, pixels))).ok(),
            fragments_uav: dev.create_unordered_access_view(&fragments, Some(&uav(DXGI_FORMAT_UNKNOWN, pixels * per_pixel, UAV_FLAG_COUNTER))).ok(),
            fragments_srv: dev.create_shader_resource_view(&fragments, Some(&srv(DXGI_FORMAT_UNKNOWN, pixels * per_pixel))).ok(),
            w,
            h,
        }
    }

    pub fn ready(&self) -> bool {
        self.head_uav.is_some() && self.head_srv.is_some() && self.fragments_uav.is_some() && self.fragments_srv.is_some()
    }
}
