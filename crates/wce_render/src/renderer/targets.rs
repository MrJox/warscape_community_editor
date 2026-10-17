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
}

impl PostTargets {

    /// (Re)creates the targets for a view size; the SSAO result is kept.
    pub fn resize(&mut self, dev: &ID3D11Device, w: u32, h: u32) {
        self.final_ = make_target(dev, w, h, DXGI_FORMAT_R8G8B8A8_UNORM);

        // SSAOX_RESOLVE / BLUR_X / BLUR_Y RTs: TTF_R16F (fn 0x11766de0); the blur at half resolution (dump).
        self.ao = make_target(dev, w, h, DXGI_FORMAT_R16_FLOAT);
        self.ao_tmp = make_target(dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);
        self.ao_blur = make_target(dev, (w / 2).max(1), (h / 2).max(1), DXGI_FORMAT_R16_FLOAT);

        let (qw, qh) = (w.div_ceil(4), h.div_ceil(4));
        self.quarter = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);  // rgb + luma in .w
        self.bloom_a = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.bloom_b = sized_target(dev, qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
        self.ldr = sized_target(dev, w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);        // tone-mapped, linear
        self.stats.clear();
        let (mut sw, mut sh) = (qw, qh);
        loop {
            sw = sw.div_ceil(4);
            sh = sh.div_ceil(4);
            self.stats.push(sized_target(dev, sw, sh, DXGI_FORMAT_R32G32B32A32_FLOAT));  // min, max, avg, max
            if sw <= 1 && sh <= 1 {
                break;
            }
        }
    }
}

/// The cascaded shadow atlas (SHADOW_SIZE).
#[derive(Clone, Debug, Default)]
pub(crate) struct ShadowMap {
    pub atlas: Target,
    pub dsv: Option<ID3D11DepthStencilView>,
}

impl ShadowMap {
    pub fn new(dev: &ID3D11Device) -> Self {
        Self {
            atlas: make_target(dev, SHADOW_SIZE, SHADOW_SIZE, DXGI_FORMAT_R32_FLOAT),
            dsv: depth_target(dev, SHADOW_SIZE, SHADOW_SIZE, DXGI_FORMAT_D32_FLOAT),
        }
    }
}
