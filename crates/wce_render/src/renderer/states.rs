//! The renderer's fixed pipeline states: samplers, depth-stencil, rasterizer and blend states, created once.

use wce_d3d11::*;

use crate::particles::blend_state;

#[derive(Clone, Debug, Default)]
pub(crate) struct States {
    /// The "quality" samplers (wrap and clamp): their filter follows gfx_texture_filtering ([`Self::set_filtering`]).
    pub s_wrap_aniso: Option<ID3D11SamplerState>,
    pub s_clamp_aniso: Option<ID3D11SamplerState>,
    pub s_wrap_linear: Option<ID3D11SamplerState>,
    pub s_wrap_point: Option<ID3D11SamplerState>,
    pub s_clamp_linear: Option<ID3D11SamplerState>,
    pub s_clamp_point: Option<ID3D11SamplerState>,
    pub s_shadow_cmp: Option<ID3D11SamplerState>,

    /// The texture filtering the quality samplers were made for.
    pub filtering: i32,
    pub ds_gbuffer: Option<ID3D11DepthStencilState>,
    pub ds_sky: Option<ID3D11DepthStencilState>,
    pub ds_light: Option<ID3D11DepthStencilState>,
    pub ds_off: Option<ID3D11DepthStencilState>,
    /// Depth written whatever is there (the MSAA depth resolve).
    pub ds_depth_write: Option<ID3D11DepthStencilState>,
    pub rs_back: Option<ID3D11RasterizerState>,
    pub rs_none: Option<ID3D11RasterizerState>,
    pub rs_shadow: Option<ID3D11RasterizerState>,
    pub rs_shadow_terrain: Option<ID3D11RasterizerState>,
    /// Rome 2's (fn 0x11456f70): DepthBias 1, SlopeScaledDepthBias 2.
    pub rs_shadow_rome2: Option<ID3D11RasterizerState>,
    /// Rome 2's baked shadow map (shadow_baked.rs): no constant bias (its offset is in the projection), a slope-scaled one for sloped
    /// roofs, whose higher neighbour texels shaded them in stripes; not clipped above the map's height range.
    pub rs_shadow_baked: Option<ID3D11RasterizerState>,
    pub rs_shadow_terrain_rome2: Option<ID3D11RasterizerState>,
    pub bs_opaque: Option<ID3D11BlendState>,
    pub bs_alpha: Option<ID3D11BlendState>,
    pub bs_additive_alpha: Option<ID3D11BlendState>,
    pub bs_add_one: Option<ID3D11BlendState>,
    pub bs_sun: Option<ID3D11BlendState>,
    pub bs_premultiplied: Option<ID3D11BlendState>,
}

impl States {
    /// Makes the quality samplers for a gfx_texture_filtering value: 0 trilinear, n anisotropic 2^n (the engine's texture-quality sampler
    /// class, fn 0x1177f410: anisotropic 2 / 4 / 8 / 16, else trilinear; docs/re/terrain.md).
    pub fn set_filtering(&mut self, dev: &ID3D11Device, filtering: i32) {
        let filtering = filtering.clamp(0, 4);
        let s = D3D11_SAMPLER_DESC {
            Filter: if filtering == 0 { D3D11_FILTER_MIN_MAG_MIP_LINEAR } else { D3D11_FILTER_ANISOTROPIC },
            AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MaxAnisotropy: 1 << filtering.max(1), MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
        };

        self.s_wrap_aniso = dev.create_sampler_state(&s).ok();
        self.s_clamp_aniso = dev.create_sampler_state(&D3D11_SAMPLER_DESC {
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP, AddressV: D3D11_TEXTURE_ADDRESS_CLAMP, AddressW: D3D11_TEXTURE_ADDRESS_CLAMP, ..s
        }).ok();
        self.filtering = filtering;
    }

    pub fn new(dev: &ID3D11Device) -> Self {
        let mut st = Self::default();
        let mut s = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_ANISOTROPIC, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MaxAnisotropy: 8, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
        };

        st.set_filtering(dev, 3);

        // The engine's sampler objects store D3D11 enums (filter 21 = MIN_MAG_MIP_LINEAR, address 1 = WRAP, 3 = CLAMP).
        s.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
        st.s_wrap_linear = dev.create_sampler_state(&s).ok();
        s.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
        s.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
        s.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
        st.s_clamp_linear = dev.create_sampler_state(&s).ok();
        s.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
        st.s_clamp_point = dev.create_sampler_state(&s).ok();
        s.Filter = D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT;
        s.ComparisonFunc = D3D11_COMPARISON_LESS_EQUAL;
        st.s_shadow_cmp = dev.create_sampler_state(&s).ok();

        let replace = D3D11_DEPTH_STENCILOP_DESC { StencilFailOp: D3D11_STENCIL_OP_KEEP, StencilDepthFailOp: D3D11_STENCIL_OP_KEEP, StencilPassOp: D3D11_STENCIL_OP_REPLACE, StencilFunc: D3D11_COMPARISON_ALWAYS };
        let mut d = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, StencilEnable: true.into(),
            StencilReadMask: 0xFF, StencilWriteMask: 0xFF, FrontFace: replace, BackFace: replace,
        };

        // Geometry marks stencil = 1.
        st.ds_gbuffer = dev.create_depth_stencil_state(&d).ok();

        // The sky at z = w only where nothing was drawn.
        d.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
        d.StencilEnable = false.into();
        st.ds_sky = dev.create_depth_stencil_state(&d).ok();

        // Lights only on geometry.
        d.DepthEnable = false.into();
        d.StencilEnable = true.into();
        let equal = D3D11_DEPTH_STENCILOP_DESC { StencilPassOp: D3D11_STENCIL_OP_KEEP, StencilFunc: D3D11_COMPARISON_EQUAL, ..replace };
        d.FrontFace = equal;
        d.BackFace = equal;
        d.StencilReadMask = super::STENCIL_LOW;
        st.ds_light = dev.create_depth_stencil_state(&d).ok();
        d.StencilReadMask = 0xFF;
        d.StencilEnable = false.into();
        st.ds_off = dev.create_depth_stencil_state(&d).ok();
        st.ds_depth_write = dev.create_depth_stencil_state(&D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL, DepthFunc: D3D11_COMPARISON_ALWAYS, ..Default::default()
        }).ok();

        let mut r = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_BACK, DepthClipEnable: true.into(), ..Default::default() };
        st.rs_back = dev.create_rasterizer_state(&r).ok();
        r.CullMode = D3D11_CULL_NONE;
        st.rs_none = dev.create_rasterizer_state(&r).ok();

        let all = D3D11_COLOR_WRITE_ENABLE_ALL as u8;
        st.bs_opaque = blend_state(dev, D3D11_RENDER_TARGET_BLEND_DESC { RenderTargetWriteMask: all, ..Default::default() });

        // Verified (BATTLE_FOG_SHADER fn 0x116c96c0 blend object +0x1cc; vignette +0x71f): SRC_ALPHA / INV_SRC_ALPHA, add, on colour and alpha.
        let alpha = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(), SrcBlend: D3D11_BLEND_SRC_ALPHA, DestBlend: D3D11_BLEND_INV_SRC_ALPHA, BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_SRC_ALPHA, DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA, BlendOpAlpha: D3D11_BLEND_OP_ADD, RenderTargetWriteMask: all,
        };

        st.bs_alpha = blend_state(dev, alpha);
        st.bs_additive_alpha = blend_state(dev, D3D11_RENDER_TARGET_BLEND_DESC {
            SrcBlendAlpha: D3D11_BLEND_ZERO, DestBlendAlpha: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, ..alpha
        });

        st.bs_sun = blend_state(dev, alpha);

        // The OIT resolve (fn 0x116e2690, its blend object at +0x338): ONE / INV_SRC_ALPHA, add, on colour and alpha (premultiplied colour).
        st.bs_premultiplied = blend_state(dev, D3D11_RENDER_TARGET_BLEND_DESC { SrcBlend: D3D11_BLEND_ONE, SrcBlendAlpha: D3D11_BLEND_ONE, ..alpha });

        // The lens flare sprites (fn 0x116de600, blend object +0x6da): ONE / ONE add on colour and alpha.
        st.bs_add_one = blend_state(dev, D3D11_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, SrcBlendAlpha: D3D11_BLEND_ONE, DestBlendAlpha: D3D11_BLEND_ONE, ..alpha
        });

        // Verified (rigid shader fn 0x116e5130, param_6 = 1 for rigidstandard: rasterizer +0x1a60 with cull field = 1): CULL_NONE, solid,
        // depth clip on (the engine's state objects hold D3D11 enums). Every shadow rasterizer (class vtable 0x11c8bb30) gets its bias when
        // it is created (fns 0x11767500 -> 0x11784690 -> 0x117846d0): DepthBias = g_new_depth_bias (2500), SlopeScaledDepthBias =
        // g_new_slope_scale_depth_bias (1.8), no clamp; the presets some shaders write first (terrain's 1000 / -2) are overwritten.
        let rd = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthBias: 2500, SlopeScaledDepthBias: 1.8, DepthClipEnable: true.into(), ..Default::default()
        };

        st.rs_shadow = dev.create_rasterizer_state(&rd).ok();

        // The terrain's shadow technique (shadow_30_new) keeps the base description's cull (fn 0x116e4af0: CULL_BACK; terrain +0x9720).
        st.rs_shadow_terrain = dev.create_rasterizer_state(&D3D11_RASTERIZER_DESC { CullMode: D3D11_CULL_BACK, ..rd }).ok();
        let r2 = D3D11_RASTERIZER_DESC { DepthBias: 1, SlopeScaledDepthBias: 2.0, ..rd };
        st.rs_shadow_rome2 = dev.create_rasterizer_state(&r2).ok();
        st.rs_shadow_baked = dev.create_rasterizer_state(&D3D11_RASTERIZER_DESC { CullMode: D3D11_CULL_BACK, DepthBias: 0, SlopeScaledDepthBias: 4.0, DepthClipEnable: false.into(), ..rd }).ok();
        st.rs_shadow_terrain_rome2 = dev.create_rasterizer_state(&D3D11_RASTERIZER_DESC { CullMode: D3D11_CULL_BACK, ..r2 }).ok();
        let sm = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_POINT, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
        };

        st.s_wrap_point = dev.create_sampler_state(&sm).ok();
        st
    }
}
