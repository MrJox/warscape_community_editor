//! The renderer's fixed pipeline states: samplers, depth-stencil, rasterizer and blend states, created once.

use wce_d3d11::*;

use crate::particles::blend_state;

#[derive(Clone, Debug, Default)]
pub(crate) struct States {
    pub s_wrap_aniso: Option<ID3D11SamplerState>,
    pub s_wrap_linear: Option<ID3D11SamplerState>,
    pub s_wrap_point: Option<ID3D11SamplerState>,
    pub s_clamp_linear: Option<ID3D11SamplerState>,
    pub s_clamp_point: Option<ID3D11SamplerState>,
    pub s_shadow_cmp: Option<ID3D11SamplerState>,
    pub ds_gbuffer: Option<ID3D11DepthStencilState>,
    pub ds_sky: Option<ID3D11DepthStencilState>,
    pub ds_light: Option<ID3D11DepthStencilState>,
    pub ds_off: Option<ID3D11DepthStencilState>,
    pub rs_back: Option<ID3D11RasterizerState>,
    pub rs_none: Option<ID3D11RasterizerState>,
    pub rs_shadow: Option<ID3D11RasterizerState>,
    pub bs_opaque: Option<ID3D11BlendState>,
    pub bs_alpha: Option<ID3D11BlendState>,
    pub bs_additive_alpha: Option<ID3D11BlendState>,
    pub bs_add_one: Option<ID3D11BlendState>,
    pub bs_sun: Option<ID3D11BlendState>,
}

impl States {
    pub fn new(dev: &ID3D11Device) -> Self {
        let mut st = Self::default();
        let mut s = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_ANISOTROPIC, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MaxAnisotropy: 8, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
        };

        st.s_wrap_aniso = dev.create_sampler_state(&s).ok();

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
        st.ds_light = dev.create_depth_stencil_state(&d).ok();
        d.StencilEnable = false.into();
        st.ds_off = dev.create_depth_stencil_state(&d).ok();

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

        // The lens flare sprites (fn 0x116de600, blend object +0x6da): ONE / ONE add on colour and alpha.
        st.bs_add_one = blend_state(dev, D3D11_RENDER_TARGET_BLEND_DESC {
            SrcBlend: D3D11_BLEND_ONE, DestBlend: D3D11_BLEND_ONE, SrcBlendAlpha: D3D11_BLEND_ONE, DestBlendAlpha: D3D11_BLEND_ONE, ..alpha
        });

        // Verified (rigid shader fn 0x116e5130, param_6 = 1 for rigidstandard: rasterizer +0x1a60 with cull field = 1): CULL_NONE, solid,
        // depth clip on, no depth bias (the engine's state objects hold D3D11 enums).
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
        st.rs_shadow = dev.create_rasterizer_state(&rd).ok();
        let sm = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_POINT, AddressU: D3D11_TEXTURE_ADDRESS_WRAP, AddressV: D3D11_TEXTURE_ADDRESS_WRAP, AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
        };

        st.s_wrap_point = dev.create_sampler_state(&sm).ok();
        st
    }
}
