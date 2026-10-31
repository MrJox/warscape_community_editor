//! The editor's render scale (no game has it): the scene drawn smaller is enlarged to the target with NVIDIA Image Scaling (NIS, SDK
//! v1.0.3, MIT; the shader files are `third_party/nis`, the configuration `NVScalerUpdateConfig` of `NIS_Config.h` ported here, its
//! coefficients in `nis_coef.rs`). NIS scales by 1 to 2 per axis: the render scale is 0.5 .. 1.
//!
//! The scaler is a compute shader: it writes its own texture (the targets are render targets only), copied into the target after.

use wce_d3d11::*;

use super::nis_coef::{COEF_SCALE, COEF_USM};
use crate::RenderError;
use crate::render_target::RenderTarget;

/// The thread group's block of output pixels and its size (`NISOptimizer` for AMD / Intel: 32 x 24, 256; NVIDIA's 128 threads would do
/// the same work in half groups).
const BLOCK_WIDTH: u32 = 32;
const BLOCK_HEIGHT: u32 = 24;
const THREAD_GROUP_SIZE: u32 = 256;

/// The scaler, made on first use.
#[derive(Default)]
pub(crate) struct Nis {
    cs: Option<ID3D11ComputeShader>,
    cb: Option<ID3D11Buffer>,
    coef_scaler: Option<ID3D11ShaderResourceView>,
    coef_usm: Option<ID3D11ShaderResourceView>,
    sampler: Option<ID3D11SamplerState>,

    /// The output (RGBA8 with unordered access) and its size.
    out: Option<(ID3D11Texture2D, ID3D11UnorderedAccessView)>,
    out_size: (u32, u32),

    /// Made and failed: not tried again (the reason is logged once by the caller).
    failed: bool,
}

/// NIS's constants (`cb` of NIS_Main.hlsl), SDR, without viewports.
fn config(sharpness: f32, (sw, sh): (u32, u32), (dw, dh): (u32, u32)) -> [u32; 28] {
    let sharpness = sharpness.clamp(0.0, 1.0);
    let slider = sharpness - 0.5;
    let (max_scale, min_scale, limit_scale) = if slider >= 0.0 { (1.25, 1.25, 1.25) } else { (1.75, 1.0, 1.0) };
    let (min_contrast, max_contrast) = (2.0f32, 10.0f32);
    let (sharp_start_y, sharp_end_y) = (0.45f32, 0.9f32);
    let strength_min = (0.4 + slider * min_scale * 1.2f32).max(0.0);
    let strength_max = 1.6 + slider * max_scale * 1.8f32;
    let limit_min = (0.14 + slider * limit_scale * 0.32f32).max(0.1);
    let limit_max = 0.5 + slider * limit_scale * 0.6f32;
    let f = [
        2.0 * 1127.0 / 1024.0,
        64.0 / 1024.0,
        min_contrast,
        1.0 / (max_contrast - min_contrast),
        1.0,
        1.0 / 255.0,
        sharp_start_y,
        1.0 / (sharp_end_y - sharp_start_y),
        strength_min,
        strength_max - strength_min,
        limit_min,
        limit_max - limit_min,
        sw as f32 / dw as f32,
        sh as f32 / dh as f32,
        1.0 / dw as f32,
        1.0 / dh as f32,
        1.0 / sw as f32,
        1.0 / sh as f32,
    ];

    let mut out = [0u32; 28];
    for (o, v) in out.iter_mut().zip(f) {
        *o = v.to_bits();
    }

    // The viewports: the whole input, the whole output.
    out[18..26].copy_from_slice(&[0, 0, sw, sh, 0, 0, dw, dh]);
    out
}

/// A coefficient table as a 2 x 64 RGBA32F texture.
fn coef_texture(dev: &ID3D11Device, table: &[[f32; 8]; 64]) -> Option<ID3D11ShaderResourceView> {
    let bytes: Vec<u8> = table.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    let desc = D3D11_TEXTURE2D_DESC {
        Width: 2, Height: 64, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32G32B32A32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };
    let tex = dev.create_texture_2d(&desc, Some(&[InitData::new(&bytes, 2 * 16)])).ok()?;
    dev.create_shader_resource_view(&tex, None).ok()
}

impl Nis {
    fn create(&mut self, dev: &ID3D11Device) -> Result<(), RenderError> {
        let main = include_str!("../../../../third_party/nis/NIS_Main.hlsl").replace("#include \"NIS_Scaler.h\"", include_str!("../../../../third_party/nis/NIS_Scaler.h"));
        let source = format!("#define NIS_BLOCK_WIDTH {BLOCK_WIDTH}\n#define NIS_BLOCK_HEIGHT {BLOCK_HEIGHT}\n#define NIS_THREAD_GROUP_SIZE {THREAD_GROUP_SIZE}\n{main}");
        let code = super::compile(&source, "NIS_Main.hlsl", "main", "cs_5_0")?;
        self.cs = Some(dev.create_compute_shader(&code).map_err(|e| RenderError(format!("NIS: {e}")))?);
        self.cb = dev
            .create_buffer(&D3D11_BUFFER_DESC { ByteWidth: 28 * 4, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, ..Default::default() }, None)
            .ok();
        self.coef_scaler = coef_texture(dev, &COEF_SCALE);
        self.coef_usm = coef_texture(dev, &COEF_USM);
        self.sampler = dev
            .create_sampler_state(&D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR, AddressU: D3D11_TEXTURE_ADDRESS_CLAMP, AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default()
            })
            .ok();
        if self.cb.is_none() || self.coef_scaler.is_none() || self.coef_usm.is_none() || self.sampler.is_none() {
            return Err(RenderError("NIS: its resources can't be made".to_owned()));
        }

        Ok(())
    }

    /// Enlarges `src` (`size`, 0.5 to 1 times the target's) into the target. False when NIS can't run (then nothing is written).
    ///
    /// # Errors
    ///
    /// The shader or its resources can't be made (once; later calls return false).
    pub(crate) fn upscale(&mut self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext, src: &ID3D11ShaderResourceView, size: (u32, u32), target: &RenderTarget, sharpness: f32) -> Result<bool, RenderError> {
        if self.failed {
            return Ok(false);
        }

        if self.cs.is_none()
            && let Err(e) = self.create(dev)
        {
            self.failed = true;
            return Err(e);
        }

        let dst = (target.width(), target.height());
        let Some(target_tex) = target.texture() else { return Ok(false) };
        if self.out.is_none() || self.out_size != dst {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: dst.0, Height: dst.1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_UNORDERED_ACCESS as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };
            self.out = dev.create_texture_2d(&desc, None).ok().and_then(|tex| {
                let uav = dev.create_unordered_access_view(&tex, None).ok()?;
                Some((tex, uav))
            });
            self.out_size = dst;
        }

        let (Some((out_tex, uav)), Some(cb)) = (self.out.clone(), self.cb.clone()) else { return Ok(false) };
        let words = config(sharpness, size, dst);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let _ = ctx.update_subresource(&cb, 0, None, &bytes, 0, 0);
        ctx.cs_set_shader(self.cs.as_ref());
        ctx.cs_set_constant_buffers(0, &[Some(cb)]);
        ctx.cs_set_samplers(0, &[self.sampler.clone()]);
        ctx.cs_set_shader_resources(0, &[Some(src.clone()), self.coef_scaler.clone(), self.coef_usm.clone()]);
        ctx.cs_set_unordered_access_views(0, &[Some(uav)]);
        ctx.dispatch(dst.0.div_ceil(BLOCK_WIDTH), dst.1.div_ceil(BLOCK_HEIGHT), 1);
        ctx.cs_set_unordered_access_views(0, &[None]);
        ctx.cs_set_shader_resources(0, &[None, None, None]);
        ctx.cs_set_shader(None);
        ctx.copy_resource(target_tex, &out_tex);
        Ok(true)
    }
}
