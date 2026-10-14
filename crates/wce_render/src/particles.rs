//! GPU particles with the game's particle_vfx_*.hlsl shaders.
//!
//! Per frame: simulation (a ping-pong of the particle state arrays) -> sort keys -> bitonic sort -> spawn (CPU-made points written into
//! free slots taken from the end of the sorted list) -> draw (the VS over the sorted list, GS quads). The CPU side (emission rate, spawn
//! positions and velocities) follows the XML semantics; the exact game code is not fully reverse-engineered yet, see the ASSUMPTION notes.

use std::collections::{BTreeMap, VecDeque};
use std::f32::consts::PI;

use wce_assets::vfx_constants::{VFX_CURVE_WIDTH, VFX_DISTORTION_VIEW_ID, VFX_MAIN_VIEW_ID, vfx_bits};
use wce_assets::weather_vfx::{VfxReader, WeatherVfx, load_weather_vfx};
use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, rotation_x, rotation_y, rotation_z};
use windows_core::{Interface, PCSTR};

use crate::cbuffers::ConstantBuffers;
use crate::shader_lib::{FileReader, GameShader, ShaderLibrary, Stage, bind_sampler, bind_shader, bind_srv};
use crate::texture::{Texture, load_dds};

const SIM_VS: &str = "dx11_sm_4_0_particle_vfx_simulation.hlsl_vs_vs40_main";
const SIM_PS: &str = "dx11_sm_4_0_particle_vfx_simulation.hlsl_ps_ps_main_simulation";
const KEYS_PS: &str = "dx11_sm_4_0_particle_vfx_simulation.hlsl_ps_ps_main";
const SORT_VS: &str = "dx11_sm_4_0_particle_vfx_gpu_sort.hlsl_vs_vs_main";
const SORT_PS: &str = "dx11_sm_4_0_particle_vfx_gpu_sort.hlsl_ps_ps_main";
const SPAWN_VS: &str = "dx11_sm_4_0_particle_vfx_spawn.hlsl_vs_vs40_main";
const SPAWN_PS: &str = "dx11_sm_4_0_particle_vfx_spawn.hlsl_ps_ps_main";
const MAIN_VS: &str = "dx11_sm_4_0_particle_vfx_main.hlsl_vs_vs40_main";
const MAIN_GS: &str = "dx11_sm_4_0_particle_vfx_main.hlsl_gs_gs_main";
const MAIN_PS: &str = "dx11_sm_4_0_particle_vfx_main.hlsl_ps_ps_main";
const DISTORTION_PS: &str = "dx11_sm_4_0_particle_vfx_main.hlsl_ps_ps_distortion";

/// Particle state and sorted-data textures: WIDTH^2 slots.
const WIDTH: u32 = 512;

/// g_emitter_instance_tex_width (dump).
const EMITTER_WIDTH: usize = 256;
const EMITTER_LAYERS: usize = 6;
const MAX_EMITTERS: usize = 512;

/// The spawn shader's pre-simulation step.
const SUB_STEP: f32 = 0.2;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// One float4 per emitter in a [512] array at `offset` bytes into a cbuffer image.
fn put(cb: &mut [u8], offset: usize, index: usize, v: &[u8; 16]) {
    cb[offset + 16 * index..offset + 16 * index + 16].copy_from_slice(v);
}

fn f4(v: &[f32; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (k, f) in v.iter().enumerate() {
        out[k * 4..k * 4 + 4].copy_from_slice(&f.to_le_bytes());
    }

    out
}

fn u4(v: &[u32; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (k, u) in v.iter().enumerate() {
        out[k * 4..k * 4 + 4].copy_from_slice(&u.to_le_bytes());
    }

    out
}

fn make_cb(dev: &ID3D11Device, data: &[u8]) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: data.len() as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
    let init = D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr().cast(), SysMemPitch: 0, SysMemSlicePitch: 0 };
    let mut buffer = None;
    // SAFETY: the data covers the buffer and lives during the call.
    let _ = unsafe { dev.CreateBuffer(&desc, Some(&init), Some(&mut buffer)) };
    buffer
}

/// Particle maps of one kind in one 2D array (behaviour_mask.y = diffuse slice, .z = normal slice), as the game's pixel shaders expect.
/// All particle maps seen are 1024^2 DXT5 with 11 mips, so the slices are plain copies.
struct TextureArrayBuilder {
    srgb: bool,
    slices: BTreeMap<String, u32>,
    textures: Vec<Texture>,
}

impl TextureArrayBuilder {

    /// The slice of a path, loading it on first use; `None` if missing or incompatible with slice 0.
    fn add(&mut self, dev: &ID3D11Device, read: &VfxReader, cache: &mut BTreeMap<String, Texture>, path: &str, errors: &mut Vec<String>) -> Option<u32> {
        if path.is_empty() {
            return None;
        }

        if let Some(&slice) = self.slices.get(path) {
            return Some(slice);
        }

        let key = format!("{path}{}", if self.srgb { "|srgb" } else { "" });
        if !cache.contains_key(&key) {
            let loaded = read(path).and_then(|bytes| load_dds(dev, &bytes, self.srgb).map_err(|error| error.to_string()));
            match loaded {
                Ok(texture) => {
                    cache.insert(key.clone(), texture);
                }

                Err(error) => {
                    errors.push(format!("{path}: {error}"));
                    return None;
                }
            }
        }

        let t = cache[&key].clone();
        if let Some(first) = self.textures.first()
            && (t.width != first.width || t.height != first.height || t.format != first.format || t.mips != first.mips)
        {
            errors.push(format!("{path}: size/format differs from the other particle maps, not drawn"));
            return None;
        }

        let slice = self.textures.len() as u32;
        self.slices.insert(path.to_owned(), slice);
        self.textures.push(t);
        Some(slice)
    }

    fn build(&self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext) -> Option<ID3D11ShaderResourceView> {
        let first = self.textures.first()?;
        // SAFETY: copies between textures of this device; descriptions are locals.
        unsafe {
            let t0 = first.srv.as_ref()?.GetResource().ok()?.cast::<ID3D11Texture2D>().ok()?;
            let mut td = D3D11_TEXTURE2D_DESC::default();
            t0.GetDesc(&mut td);
            let mips = td.MipLevels;
            td.ArraySize = self.textures.len() as u32;
            td.Usage = D3D11_USAGE_DEFAULT;
            td.MiscFlags = 0;
            let mut array = None;
            dev.CreateTexture2D(&td, None, Some(&mut array)).ok().ok()?;
            let array = array?;
            for (s, texture) in self.textures.iter().enumerate() {
                let resource = texture.srv.as_ref()?.GetResource().ok()?;
                for m in 0..mips {
                    ctx.CopySubresourceRegion(&array, m + s as u32 * mips, 0, 0, 0, &resource, m, None);
                }
            }

            let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: first.format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
            sd.Anonymous.Texture2DArray.MipLevels = mips;
            sd.Anonymous.Texture2DArray.ArraySize = td.ArraySize;
            let mut srv = None;
            dev.CreateShaderResourceView(&array, Some(&sd), Some(&mut srv)).ok().ok()?;
            srv
        }
    }
}

/// A target of the particle state: a texture with its view and a render target per slice.
#[derive(Clone, Debug, Default)]
struct Target {
    srv: Option<ID3D11ShaderResourceView>,
    rtv: [Option<ID3D11RenderTargetView>; 3],
}

#[derive(Clone, Debug)]
struct EmitterState {
    rate_accum: f32,

    /// The emission min -> max blend, random once per emitter instance (game: instance +0x100).
    rate_blend: f32,

    /// The burst gate (fn 0x1180c680, instance +0x104 timer, +0x108 interval, +0x10c length, +0xf4 gate): with a BurstInterval > 0 the
    /// emitter only spawns while timer <= length, then waits `interval`; both re-rolled from BurstInterval / BurstIntervalLength each
    /// cycle (fn 0x1180bf70).
    burst_timer: f32,
    burst_interval: f32,
    burst_length: f32,

    /// (latest death time, count) per spawned batch.
    deaths: VecDeque<(f32, u32)>,
    last_pos: Float3,
    started: bool,
}

impl Default for EmitterState {
    fn default() -> Self {
        Self { rate_accum: 0.0, rate_blend: -1.0, burst_timer: 0.0, burst_interval: -1.0, burst_length: 0.0, deaths: VecDeque::new(), last_pos: Float3::default(), started: false }
    }
}

/// A spawned particle as the spawn shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpawnVertex {
    /// Emitter instance, random seed, pre-simulation sub-steps (0.2 s each), 0.
    pub id_seed_steps: [f32; 4],

    /// Position, spawn time.
    pub pos_time: [f32; 4],

    /// Velocity, lifetime.
    pub vel_life: [f32; 4],
}

/// What a frame gives the particles.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub eye: Float3,

    /// Rows 0..2: camera right, up, forward.
    pub view: Float4x4,
    pub time: f32,
    pub dt: f32,

    /// cb_vfx_game_simulation_data (see the wind).
    pub wind_velocity: Float3,
    pub wind_variance: Float3,
    pub target: Option<ID3D11RenderTargetView>,
    pub depth: Option<ID3D11DepthStencilView>,

    /// G-buffer channel 4 (z / w).
    pub depth_zw: Option<ID3D11ShaderResourceView>,

    /// The sky cube.
    pub sky: Option<ID3D11ShaderResourceView>,
    pub width: u32,
    pub height: u32,
}

/// The weather particles.
pub struct ParticleSystem {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    errors: Vec<String>,

    definition: String,
    revision: u64,
    reader: Box<VfxReader<'static>>,

    /// path|srgb -> the loaded DDS.
    texture_cache: BTreeMap<String, Texture>,
    loaded: bool,
    effect: WeatherVfx,
    state: Vec<EmitterState>,
    clock: f32,
    alive: u32,
    rng: u32,
    clear: bool,

    /// 3-slice arrays: (id, seed, steps), (pos, spawn time), (velocity, lifetime).
    particles: [Target; 2],
    cur: usize,

    /// (distance, index) sort keys, ping-pong.
    keys: [Target; 2],
    sorted: usize,
    emitter_tex: Option<ID3D11Texture2D>,
    emitter_srv: Option<ID3D11ShaderResourceView>,
    curves_srv: Option<ID3D11ShaderResourceView>,
    diffuse_srv: Option<ID3D11ShaderResourceView>,
    normal_srv: Option<ID3D11ShaderResourceView>,
    flat_normal_srv: Option<ID3D11ShaderResourceView>,
    vector_field_srv: Option<ID3D11ShaderResourceView>,
    sea_srv: Option<ID3D11ShaderResourceView>,
    has_distortion: bool,
    cb_common: Option<ID3D11Buffer>,
    cb_gs: Option<ID3D11Buffer>,
    cb_vs: Option<ID3D11Buffer>,
    cb_ps_render: Option<ID3D11Buffer>,
    cb_ps_sim: Option<ID3D11Buffer>,
    spawn_vb: Option<ID3D11Buffer>,
    spawn_capacity: usize,
    s_linear_clamp: Option<ID3D11SamplerState>,
    s_linear_wrap: Option<ID3D11SamplerState>,
    s_point_clamp: Option<ID3D11SamplerState>,
    bs_premultiplied: Option<ID3D11BlendState>,
    bs_opaque: Option<ID3D11BlendState>,
    bs_alpha: Option<ID3D11BlendState>,
    dist_rtv: Option<ID3D11RenderTargetView>,
    dist_srv: Option<ID3D11ShaderResourceView>,
    dist_w: u32,
    dist_h: u32,
    distortion_active: bool,
    ds_test: Option<ID3D11DepthStencilState>,
    ds_off: Option<ID3D11DepthStencilState>,
    rs_none: Option<ID3D11RasterizerState>,

    pub enabled: bool,
}

/// The game shaders a frame of particles uses.
struct Shaders {
    sim_vs: std::rc::Rc<GameShader>,
    sim_ps: std::rc::Rc<GameShader>,
    keys_ps: std::rc::Rc<GameShader>,
    sort_vs: std::rc::Rc<GameShader>,
    sort_ps: std::rc::Rc<GameShader>,
    spawn_vs: std::rc::Rc<GameShader>,
    spawn_ps: std::rc::Rc<GameShader>,
    vs: std::rc::Rc<GameShader>,
    gs: std::rc::Rc<GameShader>,
    ps: std::rc::Rc<GameShader>,
}

/// A blend state of one render target description.
pub(crate) fn blend_state(dev: &ID3D11Device, rt: D3D11_RENDER_TARGET_BLEND_DESC) -> Option<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0] = rt;
    let mut state = None;
    // SAFETY: a plain creation call.
    let _ = unsafe { dev.CreateBlendState(&desc, Some(&mut state)) };
    state
}

impl ParticleSystem {

    /// The particle system; `read` gives the files of the packs (the editor passes one that prefers open documents).
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, read: Box<VfxReader<'static>>) -> Self {
        let mut system = Self {
            dev: dev.clone(),
            ctx: ctx.clone(),
            errors: vec![],
            definition: String::new(),
            revision: 0,
            reader: read,
            texture_cache: BTreeMap::new(),
            loaded: false,
            effect: WeatherVfx::default(),
            state: vec![],
            clock: 0.0,
            alive: 0,
            rng: 0x123_4567,
            clear: true,
            particles: Default::default(),
            cur: 0,
            keys: Default::default(),
            sorted: 0,
            emitter_tex: None,
            emitter_srv: None,
            curves_srv: None,
            diffuse_srv: None,
            normal_srv: None,
            flat_normal_srv: None,
            vector_field_srv: None,
            sea_srv: None,
            has_distortion: false,
            cb_common: None,
            cb_gs: None,
            cb_vs: None,
            cb_ps_render: None,
            cb_ps_sim: None,
            spawn_vb: None,
            spawn_capacity: 0,
            s_linear_clamp: None,
            s_linear_wrap: None,
            s_point_clamp: None,
            bs_premultiplied: None,
            bs_opaque: None,
            bs_alpha: None,
            dist_rtv: None,
            dist_srv: None,
            dist_w: 0,
            dist_h: 0,
            distortion_active: false,
            ds_test: None,
            ds_off: None,
            rs_none: None,
            enabled: true,
        };

        system.create_resources();
        system.build_emitter_gpu();
        system
    }

    pub fn set_reader(&mut self, read: Box<VfxReader<'static>>) {
        self.reader = read;
    }

    pub fn effect(&self) -> &WeatherVfx {
        &self.effect
    }

    pub fn alive(&self) -> u32 {
        self.alive
    }

    /// An upper bound of the live particles of one emitter (the same estimate as [`Self::alive`]).
    pub fn alive_of(&self, emitter: usize) -> u32 {
        self.state.get(emitter).map_or(0, |state| state.deaths.iter().map(|(_, n)| n).sum())
    }

    /// The state of the spawner's random numbers (for comparisons with the C++ editor).
    pub fn rng_state(&self) -> u32 {
        self.rng
    }

    pub fn capacity(&self) -> u32 {
        WIDTH * WIDTH
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// The distortion buffer of this frame (heat haze, lens drops); `None` when no such emitter drew.
    pub fn distortion(&self) -> Option<&ID3D11ShaderResourceView> {
        self.dist_srv.as_ref().filter(|_| self.distortion_active)
    }

    fn error(&mut self, e: String) {
        if !self.errors.contains(&e) {
            self.errors.push(e);
        }
    }

    fn rand01(&mut self) -> f32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rng >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    fn create_resources(&mut self) {
        let dev = self.dev.clone();
        let make_target = |fmt: DXGI_FORMAT, slices: u32| -> Target {
            let mut t = Target::default();
            let td = D3D11_TEXTURE2D_DESC {
                Width: WIDTH, Height: WIDTH, MipLevels: 1, ArraySize: slices, Format: fmt, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let mut tex = None;
            // SAFETY: creation calls with valid descriptions.
            unsafe {
                if dev.CreateTexture2D(&td, None, Some(&mut tex)).is_err() {
                    return t;
                }

                let Some(tex) = tex else { return t };
                let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: fmt, ..Default::default() };
                if slices > 1 {
                    sd.ViewDimension = D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY);
                    sd.Anonymous.Texture2DArray.MipLevels = 1;
                    sd.Anonymous.Texture2DArray.ArraySize = slices;
                } else {
                    sd.ViewDimension = D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2D);
                    sd.Anonymous.Texture2D.MipLevels = 1;
                }

                let _ = dev.CreateShaderResourceView(&tex, Some(&sd), Some(&mut t.srv));
                for s in 0..slices {
                    let mut rd = D3D11_RENDER_TARGET_VIEW_DESC { Format: fmt, ..Default::default() };
                    if slices > 1 {
                        rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
                        rd.Anonymous.Texture2DArray.FirstArraySlice = s;
                        rd.Anonymous.Texture2DArray.ArraySize = 1;
                    } else {
                        rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
                    }

                    let _ = dev.CreateRenderTargetView(&tex, Some(&rd), Some(&mut t.rtv[s as usize]));
                }
            }

            t
        };

        self.particles = [make_target(DXGI_FORMAT_R32G32B32A32_FLOAT, 3), make_target(DXGI_FORMAT_R32G32B32A32_FLOAT, 3)];
        self.keys = [make_target(DXGI_FORMAT_R32G32_FLOAT, 1), make_target(DXGI_FORMAT_R32G32_FLOAT, 1)];

        // SAFETY: creation calls with valid descriptions and data that lives during them.
        unsafe {
            let ed = D3D11_TEXTURE2D_DESC {
                Width: EMITTER_WIDTH as u32, Height: 1, MipLevels: 1, ArraySize: EMITTER_LAYERS as u32, Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let _ = dev.CreateTexture2D(&ed, None, Some(&mut self.emitter_tex));
            if let Some(tex) = &self.emitter_tex {
                let _ = dev.CreateShaderResourceView(tex, None, Some(&mut self.emitter_srv));
            }

            // A flat normal (1 slice) and a still sea surface (2 slices; the game logic's collision is off for weather).
            let flat = 0xFFFF_8080u32;
            let td = D3D11_TEXTURE2D_DESC {
                Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let sd = D3D11_SUBRESOURCE_DATA { pSysMem: (&raw const flat).cast(), SysMemPitch: 4, SysMemSlicePitch: 4 };
            let mut t = None;
            let _ = dev.CreateTexture2D(&td, Some(&sd), Some(&mut t));
            if let Some(t) = t {
                let mut vd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: td.Format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
                vd.Anonymous.Texture2DArray.MipLevels = 1;
                vd.Anonymous.Texture2DArray.ArraySize = 1;
                let _ = dev.CreateShaderResourceView(&t, Some(&vd), Some(&mut self.flat_normal_srv));
            }

            self.normal_srv = self.flat_normal_srv.clone();
            let zero = [[0f32; 4]; 2];
            let sdsc = D3D11_TEXTURE2D_DESC {
                Width: 1, Height: 1, MipLevels: 1, ArraySize: 2, Format: DXGI_FORMAT_R32G32B32A32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let ss = [
                D3D11_SUBRESOURCE_DATA { pSysMem: zero[0].as_ptr().cast(), SysMemPitch: 16, SysMemSlicePitch: 16 },
                D3D11_SUBRESOURCE_DATA { pSysMem: zero[1].as_ptr().cast(), SysMemPitch: 16, SysMemSlicePitch: 16 },
            ];

            let mut st = None;
            let _ = dev.CreateTexture2D(&sdsc, Some(ss.as_ptr()), Some(&mut st));
            if let Some(st) = st {
                let _ = dev.CreateShaderResourceView(&st, None, Some(&mut self.sea_srv));
            }
        }

        // The 3D vector field the turbulence samples (g_tex_vector_field, the string next to it in the DLL).
        match (self.reader)("vfx/textures/turbulence.dds").and_then(|bytes| load_dds(&dev, &bytes, false).map_err(|error| error.to_string())) {
            Ok(texture) => self.vector_field_srv = texture.srv,
            Err(error) => {
                self.error(format!("vfx/textures/turbulence.dds: {error}"));
                let mid = 0xFF80_8080u32;
                let vd = D3D11_TEXTURE3D_DESC {
                    Width: 1, Height: 1, Depth: 1, MipLevels: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, Usage: D3D11_USAGE_IMMUTABLE,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
                };

                let sd = D3D11_SUBRESOURCE_DATA { pSysMem: (&raw const mid).cast(), SysMemPitch: 4, SysMemSlicePitch: 4 };
                let mut t = None;
                // SAFETY: creation calls with data that lives during them.
                unsafe {
                    let _ = dev.CreateTexture3D(&vd, Some(&sd), Some(&mut t));
                    if let Some(t) = t {
                        let _ = dev.CreateShaderResourceView(&t, None, Some(&mut self.vector_field_srv));
                    }
                }
            }
        }

        let sampler = |filter: D3D11_FILTER, address: D3D11_TEXTURE_ADDRESS_MODE| {
            let sd = D3D11_SAMPLER_DESC { Filter: filter, AddressU: address, AddressV: address, AddressW: address, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default() };
            let mut state = None;
            // SAFETY: a plain creation call.
            let _ = unsafe { dev.CreateSamplerState(&sd, Some(&mut state)) };
            state
        };

        self.s_linear_clamp = sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP);
        self.s_linear_wrap = sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_WRAP);
        self.s_point_clamp = sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP);

        let write_all = D3D11_COLOR_WRITE_ENABLE_ALL as u8;
        self.bs_opaque = blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC { RenderTargetWriteMask: write_all, ..Default::default() });

        // The pixel shader outputs premultiplied colour (its fog lerps towards fog colour * alpha).
        let premultiplied = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: write_all,
        };

        self.bs_premultiplied = blend_state(&dev, premultiplied);

        // The distortion buffer: lerp towards the particle's offset.
        self.bs_alpha = blend_state(&dev, D3D11_RENDER_TARGET_BLEND_DESC { SrcBlend: D3D11_BLEND_SRC_ALPHA, ..premultiplied });

        let mut dd = D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, ..Default::default() };
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
        // SAFETY: plain creation calls.
        unsafe {
            let _ = dev.CreateDepthStencilState(&dd, Some(&mut self.ds_test));
            dd.DepthEnable = false.into();
            let _ = dev.CreateDepthStencilState(&dd, Some(&mut self.ds_off));
            let _ = dev.CreateRasterizerState(&rd, Some(&mut self.rs_none));
        }
    }

    /// Loads the effect of a weather definition, restarting the simulation. It reloads when the name or `revision` (the editor's edit
    /// counter of open vfx / weather XML) changes; live particles are kept when the reload gives the same emitters.
    pub fn set_weather(&mut self, definition_name: &str, revision: u64) {
        if self.loaded && definition_name == self.definition && revision == self.revision {
            return;
        }

        let same_definition = self.loaded && definition_name == self.definition;
        self.loaded = true;
        self.definition = definition_name.to_owned();
        self.revision = revision;
        let before = self.effect.emitters.iter().map(|e| format!("{}/{}", e.instance, e.name)).collect::<Vec<_>>();
        self.errors.clear();
        match load_weather_vfx(&*self.reader, definition_name) {
            Ok(effect) => self.effect = effect,
            Err(error) => {
                self.effect = WeatherVfx::default();
                self.error(format!("weather vfx '{definition_name}': {error}"));
            }
        }

        self.effect.emitters.truncate(MAX_EMITTERS.min(EMITTER_WIDTH));
        self.build_emitter_gpu();
        let after = self.effect.emitters.iter().map(|e| format!("{}/{}", e.instance, e.name)).collect::<Vec<_>>();

        // An edited value keeps the particles in flight.
        if !same_definition || before != after {
            self.restart();
        }
    }

    pub fn restart(&mut self) {
        self.state = vec![EmitterState::default(); self.effect.emitters.len()];
        self.clock = 0.0;
        self.alive = 0;
        self.clear = true;
    }

    /// The emitter constant buffers, the curve texture array and the diffuse texture array of the current effect.
    fn build_emitter_gpu(&mut self) {
        let (mut common, mut gs, mut vs, mut ps_render, mut ps_sim) = (vec![0u8; 8192], vec![0u8; 16384], vec![0u8; 57344], vec![0u8; 57344], vec![0u8; 40960]);
        let mut diffuse = TextureArrayBuilder { srgb: true, slices: BTreeMap::new(), textures: vec![] };
        let mut normal = TextureArrayBuilder { srgb: false, slices: BTreeMap::new(), textures: vec![] };
        let mut tex_errors = vec![];
        self.has_distortion = false;
        let n = self.effect.emitters.len();
        for e in 0..n {
            let (dpath, npath) = (self.effect.emitters[e].k.diffuse.clone(), self.effect.emitters[e].k.normal.clone());
            let ds = diffuse.add(&self.dev, &*self.reader, &mut self.texture_cache, &dpath, &mut tex_errors);
            let ns = normal.add(&self.dev, &*self.reader, &mut self.texture_cache, &npath, &mut tex_errors);
            let k = &mut self.effect.emitters[e].k;
            k.behaviour_mask[1] = ds.unwrap_or(0);
            k.behaviour_mask[2] = ns.unwrap_or(0);
            if ds.is_none() {
                k.behaviour_mask[0] &= !vfx_bits::PS_TEXTURE;
            }

            if ns.is_none() {
                k.behaviour_mask[0] &= !vfx_bits::PS_NORMAL;
            }

            if k.behaviour_mask[3] == VFX_DISTORTION_VIEW_ID {
                self.has_distortion = true;
            }

            put(&mut common, 0, e, &u4(&k.behaviour_mask));
            put(&mut gs, 0, e, &f4(&k.vs_translation_params));
            put(&mut gs, 8192, e, &f4(&k.vs_translation_params1));
            put(&mut ps_sim, 0, e, &f4(&k.ps_sim_game_logic_params));
            put(&mut ps_sim, 8192, e, &f4(&k.ps_sim_gravity_params));
            put(&mut ps_sim, 16384, e, &f4(&k.ps_sim_deceleration_params));
            put(&mut ps_sim, 24576, e, &f4(&k.ps_sim_following_emitter_params));
            put(&mut ps_sim, 32768, e, &f4(&k.ps_sim_turbulence_params));
            put(&mut ps_render, 0, e, &f4(&k.ps_specular_params));
            put(&mut ps_render, 8192, e, &f4(&k.ps_specular_params1));
            put(&mut ps_render, 16384, e, &f4(&k.ps_receive_shadows_high_quality_params));
            put(&mut ps_render, 24576, e, &f4(&k.ps_lighting_legacy_params));
            put(&mut ps_render, 32768, e, &f4(&k.ps_lighting_params));
            put(&mut ps_render, 40960, e, &f4(&k.ps_alpha_crush_params));
            put(&mut ps_render, 49152, e, &f4(&k.ps_soft_edge_params));
            put(&mut vs, 0, e, &f4(&k.vs_culling_distances));
            put(&mut vs, 8192, e, &f4(&k.vs_rotation_params));
            put(&mut vs, 16384, e, &f4(&k.vs_scroll_uvs_params));
            put(&mut vs, 24576, e, &f4(&k.vs_atlas_texture_params));
            put(&mut vs, 32768, e, &u4(&k.vs_cell_texture_params));
            put(&mut vs, 40960, e, &u4(&k.vs_animated_texture_params));
            put(&mut vs, 49152, e, &f4(&k.vs_animated_texture_params1));
        }

        self.cb_common = make_cb(&self.dev, &common);
        self.cb_gs = make_cb(&self.dev, &gs);
        self.cb_vs = make_cb(&self.dev, &vs);
        self.cb_ps_render = make_cb(&self.dev, &ps_render);
        self.cb_ps_sim = make_cb(&self.dev, &ps_sim);

        // The curves: one 2D array slice per emitter, 8 rows.
        let layers = n.max(1);
        let slice = 8 * VFX_CURVE_WIDTH * 4;
        let mut texels = vec![1.0f32; layers * slice];
        for (l, chunk) in texels.chunks_mut(slice).enumerate().take(n) {
            chunk.copy_from_slice(self.effect.emitters[l].k.curves.as_flattened().as_flattened());
        }

        let subs = texels.chunks(slice)
            .map(|chunk| D3D11_SUBRESOURCE_DATA { pSysMem: chunk.as_ptr().cast(), SysMemPitch: (VFX_CURVE_WIDTH * 16) as u32, SysMemSlicePitch: (VFX_CURVE_WIDTH * 16 * 8) as u32 })
            .collect::<Vec<_>>();
        let td = D3D11_TEXTURE2D_DESC {
            Width: VFX_CURVE_WIDTH as u32, Height: 8, MipLevels: 1, ArraySize: layers as u32, Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        self.curves_srv = None;
        // SAFETY: the subresources point into `texels`, alive during the calls.
        unsafe {
            let mut t = None;
            let _ = self.dev.CreateTexture2D(&td, Some(subs.as_ptr()), Some(&mut t));
            if let Some(t) = t {
                let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: td.Format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
                sd.Anonymous.Texture2DArray.MipLevels = 1;
                sd.Anonymous.Texture2DArray.ArraySize = layers as u32;
                let _ = self.dev.CreateShaderResourceView(&t, Some(&sd), Some(&mut self.curves_srv));
            }
        }

        for e in tex_errors {
            self.error(e);
        }

        self.diffuse_srv = diffuse.build(&self.dev, &self.ctx);
        self.normal_srv = normal.build(&self.dev, &self.ctx).or_else(|| self.flat_normal_srv.clone());
    }

    fn bind_emitter_cbs(&self, s: &GameShader, sim_layout: bool) {
        for (name, &slot) in &s.cbuffers {
            let buffer = match name.as_str() {
                "cb_vfx_emitter_constants_common" => &self.cb_common,
                "cb_vfx_emitter_constants_gs" => &self.cb_gs,
                "cb_vfx_emitter_constants_vs" => &self.cb_vs,
                "cb_vfx_emitter_constants_ps" => if sim_layout { &self.cb_ps_sim } else { &self.cb_ps_render },
                _ => continue,
            };

            let Some(buffer) = buffer else { continue };
            let buffers = [Some(buffer.clone())];
            // SAFETY: binding calls with objects of this device.
            unsafe {
                match s.stage {
                    Stage::Vs => self.ctx.VSSetConstantBuffers(slot, Some(&buffers)),
                    Stage::Gs => self.ctx.GSSetConstantBuffers(slot, Some(&buffers)),
                    Stage::Ps => self.ctx.PSSetConstantBuffers(slot, Some(&buffers)),
                    _ => {}
                }
            }
        }
    }

    fn fullscreen(&self, rtvs: &[Option<ID3D11RenderTargetView>], w: u32, h: u32) {
        // SAFETY: drawing calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(rtvs), None);
            self.ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            self.ctx.IASetInputLayout(None);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
            self.ctx.Draw(3, 0);
        }
    }

    fn unbind(&self) {
        let none: [Option<ID3D11ShaderResourceView>; 8] = Default::default();
        // SAFETY: unbinding calls.
        unsafe {
            self.ctx.OMSetRenderTargets(None, None);
            self.ctx.VSSetShaderResources(0, Some(&none));
            self.ctx.PSSetShaderResources(0, Some(&none));
            self.ctx.GSSetShaderResources(0, Some(&none));
        }
    }

    /// CPU emission. The spawn code is fn 0x1895130; the shapes below are still approximations of it:
    /// - rate = lerp(emission_min, emission_max, random) of EmissionRateCurve at the emitter's normalised age (the alive counts in the
    ///   dumps match rate * ParticleLifeTime per reference);
    /// - position: SpawnAreaBox gives the box size per axis (centred), else SpawnAreaCurve an annulus radius;
    /// - velocity: SpawnSpeed along elevation SpawnSpread (degrees), azimuth in SpawnArc;
    /// - StartTime < 0 pre-warms the emitter with the spawn shader's 0.2 s sub-steps.
    pub fn spawn(&mut self, f: &Frame) -> Vec<SpawnVertex> {
        let mut out = vec![];
        let mut free_slots = self.capacity().saturating_sub(self.alive);
        for i in 0..self.effect.emitters.len().min(self.state.len()) {
            let a = self.effect.emitters[i].clone();
            let d = &a.def;
            let mut origin = if a.camera_following { f.eye } else { Float3::default() } + Float3::new(a.pos[0], a.pos[1], a.pos[2]);
            let mut sp = [0.0f32; 3];
            d.vec("SpawnPosition", &mut sp);
            origin = origin + Float3::new(sp[0], sp[1], sp[2]) * a.scale;

            let (life_lo, life_hi) = d.range("ParticleLifeTime", 1.0, 1.0);
            let (start_lo, start_hi) = d.range("StartTime", 0.0, 0.0);
            let (emitter_life_lo, emitter_life_hi) = d.range("LifeTime", 50.0, 50.0);
            let emitter_life = (0.5 * (emitter_life_lo + emitter_life_hi)).max(0.001);
            let pre = (-0.5 * (start_lo + start_hi)).max(0.0);
            let (mut window_start, mut window) = (self.clock - f.dt, f.dt);
            if !self.state[i].started {
                self.state[i].started = true;
                window_start = self.clock - pre;
                window = pre + f.dt;
            }

            let age = self.clock + pre;
            let t_norm = if d.b("EmissionLoopAnim", false) { (age / emitter_life) % 1.0 } else { (age / emitter_life).min(1.0) };
            if !d.b("InfiniteLife", false) && age > emitter_life {
                continue;
            }

            let emin = d.curve("EmissionRateCurve", "emission_min");
            let emax = d.curve("EmissionRateCurve", "emission_max");

            // Verified (fn 0x11829530 / 0x1188e240, snow dump): rate = lerp(min(t), max(t), f) clamped >= 0, where f is drawn once per
            // emitter instance (live snowflake emitters: 0.71 and 0.95); per frame dt is capped at 0.1 s.
            if self.state[i].rate_blend < 0.0 {
                self.state[i].rate_blend = self.rand01();
            }

            let rate = lerp(emin.map_or(0.0, |c| c.eval(t_norm)), emax.map_or(0.0, |c| c.eval(t_norm)), self.state[i].rate_blend).max(0.0);

            // The burst gate (fn 0x1180c680 / 0x1180bf70).
            let (bi_lo, bi_hi) = d.range("BurstInterval", 0.0, 0.0);
            let (bl_lo, bl_hi) = d.range("BurstIntervalLength", 0.0, 0.0);
            let roll_burst = |system: &mut Self| {
                let interval = lerp(bi_lo, bi_hi, system.rand01());
                let length = lerp(bl_lo, bl_hi, system.rand01());
                system.state[i].burst_interval = interval;
                system.state[i].burst_length = length;
            };

            if self.state[i].burst_interval < 0.0 {
                roll_burst(self);
            }

            let mut gate = 1.0f32;
            if self.state[i].burst_interval > 0.0 {
                let st = &self.state[i];
                gate = if st.burst_timer <= st.burst_length { 1.0 } else { 0.0 };
                if st.burst_length + st.burst_interval <= st.burst_timer {
                    roll_burst(self);
                    self.state[i].burst_timer = 0.0;
                } else {
                    self.state[i].burst_timer += f.dt;
                }
            }

            // Accumulate rate * time (dt capped at 0.1 s, fn 0x1188e240), spawn floor(gate * floor(accum)) and drop floor(accum): while
            // the burst gate is closed the accumulated particles are discarded, as in the game. NOTE: fn 0x1188e240 either accumulates
            // or spawns per call; that alternation is not modelled because the rain dump's live count (8426) matches accumulating every frame.
            let st = &mut self.state[i];
            st.rate_accum += rate * if st.started && window <= f.dt { window.min(0.1) } else { window };
            let whole = st.rate_accum.floor();
            let count = (gate * whole).floor() as u32;
            st.rate_accum -= whole;

            let box_curve = |n: &str| d.curve("SpawnAreaBox", n).map_or(0.0, |c| c.eval(t_norm));
            let box_lo = [box_curve("x_min"), box_curve("y_min"), box_curve("z_min")];
            let box_hi = [box_curve("x_max"), box_curve("y_max"), box_curve("z_max")];
            let use_box = box_hi.iter().any(|&v| v > 0.0);
            let area_curve = |n: &str| d.curve("SpawnAreaCurve", n).map_or(0.0, |c| c.eval(t_norm));
            let (r_in_lo, r_in_hi) = (area_curve("inner_radius_min"), area_curve("inner_radius_max"));
            let (r_out_lo, r_out_hi) = (area_curve("outer_radius_min"), area_curve("outer_radius_max"));
            let (lb_lo, lb_hi) = (a.particle_lifetime_bias[0], a.particle_lifetime_bias[1]);
            let (speed_lo, speed_hi) = d.range("SpawnSpeed", 0.0, 0.0);
            let (spread_lo, spread_hi) = d.range("SpawnSpread", 90.0, 90.0);
            let mut arc = [0.0f32, 360.0];
            d.vec("SpawnArc", &mut arc);

            // EmitterRotation (degrees, x then y then z) turns the spawn area and directions; the lens snowflakes use x = 90 to put their
            // disk into the screen plane. Verified (fn 0x1180d730): q = qz * qy * qx from half-angles, i.e. X first, then Y, then Z.
            let mut rot = [0.0f32; 3];
            d.vec("EmitterRotation", &mut rot);
            let emitter_rot = rotation_z(rot[2] * PI / 180.0) * rotation_y(rot[1] * PI / 180.0) * rotation_x(rot[0] * PI / 180.0);

            let mut batch = 0u32;
            let mut latest_death = 0.0f32;
            let mut p = 0;
            while p < count && free_slots > 0 {
                let spawn_t = window_start + window * (p as f32 + self.rand01()) / count.max(1) as f32;
                let life_a = self.rand01();
                let life_b = self.rand01();
                let life = (lerp(life_lo, life_hi, life_a) + lerp(lb_lo, lb_hi, life_b)).max(0.01);
                p += 1;

                // Died during the pre-warm.
                if spawn_t + life <= self.clock {
                    continue;
                }

                let offset = if use_box {
                    let axis = |k: usize, system: &mut Self| {
                        let r = system.rand01();
                        let s = system.rand01();
                        (r - 0.5) * lerp(box_lo[k], box_hi[k], s) * a.scale
                    };

                    let x = axis(0, self);
                    let y = axis(1, self);
                    let z = axis(2, self);
                    Float3::new(x, y, z)
                } else {
                    // In the C++ editor's order: the blend, then the outer radius, then the inner one.
                    let (r_blend, r_outer, r_inner) = (self.rand01(), self.rand01(), self.rand01());
                    let r = lerp(lerp(r_in_lo, r_in_hi, r_inner), lerp(r_out_lo, r_out_hi, r_outer), r_blend) * a.scale;
                    let az = lerp(arc[0], arc[1], self.rand01()) * PI / 180.0;
                    Float3::new(az.cos(), 0.0, az.sin()) * r
                };

                let el = lerp(spread_lo, spread_hi, self.rand01()) * PI / 180.0;
                let az = lerp(arc[0], arc[1], self.rand01()) * PI / 180.0;
                let speed = lerp(speed_lo, speed_hi, self.rand01()) * a.scale;
                let vel = Float3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin()) * speed;
                let pos = origin + emitter_rot.transform_point(offset);
                let vel = emitter_rot.transform_point(vel);
                let steps = if spawn_t < self.clock - SUB_STEP { ((self.clock - spawn_t) / SUB_STEP).floor() } else { 0.0 };
                let seed = self.rand01();
                out.push(SpawnVertex { id_seed_steps: [i as f32, seed, steps, 0.0], pos_time: [pos.x, pos.y, pos.z, spawn_t], vel_life: [vel.x, vel.y, vel.z, life] });
                latest_death = latest_death.max(spawn_t + life);
                batch += 1;
                free_slots -= 1;
            }

            if batch > 0 {
                self.state[i].deaths.push_back((latest_death, batch));
            }
        }

        out
    }

    /// The CPU step of a frame before the GPU passes: advances the clock (dt capped at 0.1 s) and updates the alive estimate (particles
    /// whose batch has not fully expired: an upper bound, dead slots are skipped by the VS). Returns the dt used.
    pub fn advance(&mut self, dt: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.1);
        self.clock += dt;
        self.alive = 0;
        for st in &mut self.state {
            while st.deaths.front().is_some_and(|(death, _)| *death <= self.clock) {
                st.deaths.pop_front();
            }

            self.alive += st.deaths.iter().map(|(_, n)| n).sum::<u32>();
        }

        self.alive = self.alive.min(self.capacity());
        dt
    }

    fn shaders(&mut self, lib: &mut ShaderLibrary, read: &FileReader, cbs: &mut ConstantBuffers) -> Option<Shaders> {
        let mut get = |name: &str| lib.get(read, cbs, name);
        let shaders = (|| Ok::<_, crate::RenderError>(Shaders {
            sim_vs: get(SIM_VS)?,
            sim_ps: get(SIM_PS)?,
            keys_ps: get(KEYS_PS)?,
            sort_vs: get(SORT_VS)?,
            sort_ps: get(SORT_PS)?,
            spawn_vs: get(SPAWN_VS)?,
            spawn_ps: get(SPAWN_PS)?,
            vs: get(MAIN_VS)?,
            gs: get(MAIN_GS)?,
            ps: get(MAIN_PS)?,
        }))();

        match shaders {
            Ok(shaders) => Some(shaders),
            Err(error) => {
                self.error(error.0);
                None
            }
        }
    }

    /// Simulates, sorts, spawns and draws one frame.
    pub fn render(&mut self, f: &Frame, lib: &mut ShaderLibrary, read: &FileReader, cbs: &mut ConstantBuffers) {
        if !self.enabled || self.effect.emitters.is_empty() {
            return;
        }

        let Some(sh) = self.shaders(lib, read, cbs) else { return };
        let dt = self.advance(f.dt);

        // The emitter instances (g_emitter_instances, 6 layers).
        let mut layers = vec![0.0f32; EMITTER_LAYERS * EMITTER_WIDTH * 4];
        let at = |layer: usize, e: usize| (layer * EMITTER_WIDTH + e) * 4;
        for i in 0..self.effect.emitters.len().min(self.state.len()) {
            let a = &self.effect.emitters[i];
            let st = &mut self.state[i];
            let pos = if a.camera_following { f.eye } else { Float3::default() } + Float3::new(a.pos[0], a.pos[1], a.pos[2]);
            let delta = if st.started { pos - st.last_pos } else { Float3::default() };
            st.last_pos = pos;
            layers[at(0, i)..at(0, i) + 4].copy_from_slice(&[pos.x, pos.y, pos.z, self.clock]);
            layers[at(1, i)..at(1, i) + 4].copy_from_slice(&[delta.x, delta.y, delta.z, dt]);

            // ASSUMPTION: the tint is the middle of the reference's tint range.
            for c in 0..4 {
                layers[at(2, i) + c] = 0.5 * (a.tint_min[c] + a.tint_max[c]);
            }

            layers[at(3, i)] = i as f32;
            layers[at(3, i) + 1] = a.scale;

            // The rotation quaternion, and the kill plane: never.
            layers[at(4, i) + 3] = 1.0;
            layers[at(5, i) + 3] = 1.0;
        }

        // SAFETY: each layer is EMITTER_WIDTH texels of the CPU array.
        unsafe {
            if let Some(tex) = &self.emitter_tex {
                for l in 0..EMITTER_LAYERS {
                    self.ctx.UpdateSubresource(tex, l as u32, None, layers[at(l, 0)..].as_ptr().cast(), (EMITTER_WIDTH * 16) as u32, 0);
                }
            }
        }

        // The shared cbuffers.
        let w = f.wind_velocity;
        cbs.set_f("cb_vfx_shared", "g_emitter_instance_tex_width", &[EMITTER_WIDTH as f32]);
        cbs.set_f("cb_vfx_shared", "g_particle_instance_tex_width", &[WIDTH as f32]);
        cbs.set_f("cb_vfx_shared", "g_sorted_data_tex_width", &[WIDTH as f32]);
        cbs.set_f("cb_vfx_shared", "g_num_alive_particles", &[self.alive as f32]);
        cbs.set_f("cb_vfx_game_simulation_data", "wind_velocity", &[w.x, w.y, w.z]);
        cbs.set_f("cb_vfx_game_simulation_data", "wind_variance_dir", &[f.wind_variance.x, f.wind_variance.y, f.wind_variance.z]);
        cbs.set_f("cb_vfx", "g_camera_aligned_x_axis", &f.view.m[0][..3]);
        cbs.set_f("cb_vfx", "g_camera_aligned_y_axis", &f.view.m[1][..3]);
        cbs.set_f("cb_vfx", "g_camera_aligned_z_axis", &f.view.m[2][..3]);
        cbs.set_i("cb_vfx", "g_view_id", VFX_MAIN_VIEW_ID as i32);
        let _ = cbs.upload(&self.dev, &self.ctx);

        self.unbind();
        // SAFETY: state calls with objects of this device.
        unsafe {
            self.ctx.OMSetBlendState(self.bs_opaque.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.OMSetDepthStencilState(self.ds_off.as_ref(), 0);
            self.ctx.RSSetState(self.rs_none.as_ref());
            self.ctx.GSSetShader(None, None);
            if self.clear {
                for p in &self.particles {
                    let clears = [[-1.0, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4]];
                    for (rtv, colour) in p.rtv.iter().zip(clears) {
                        if let Some(rtv) = rtv {
                            self.ctx.ClearRenderTargetView(rtv, &colour);
                        }
                    }
                }

                self.clear = false;
            }
        }

        // 1. The simulation: particles[cur] -> particles[1 - cur].
        let (src, dst) = (self.cur, 1 - self.cur);
        bind_shader(&self.ctx, &sh.sim_vs, cbs);
        bind_shader(&self.ctx, &sh.sim_ps, cbs);
        self.bind_emitter_cbs(&sh.sim_ps, true);
        bind_srv(&self.ctx, &sh.sim_ps, "g_emitter_instances", self.emitter_srv.as_ref());
        bind_srv(&self.ctx, &sh.sim_ps, "g_particle_instances", self.particles[src].srv.as_ref());
        bind_srv(&self.ctx, &sh.sim_ps, "g_tex_sea_surface_anim", self.sea_srv.as_ref());
        bind_srv(&self.ctx, &sh.sim_ps, "g_tex_vector_field", self.vector_field_srv.as_ref());
        bind_sampler(&self.ctx, &sh.sim_ps, "g_sam_terrain_surface", self.s_linear_clamp.as_ref());
        bind_sampler(&self.ctx, &sh.sim_ps, "g_sam_vector_field", self.s_linear_wrap.as_ref());
        self.fullscreen(&self.particles[dst].rtv.clone(), WIDTH, WIDTH);
        self.unbind();
        self.cur = dst;

        // 2. The sort keys (distance to the camera, 1e6 for dead slots) and 3. the bitonic sort, ascending.
        bind_shader(&self.ctx, &sh.keys_ps, cbs);
        bind_srv(&self.ctx, &sh.keys_ps, "g_particle_instances", self.particles[self.cur].srv.as_ref());
        self.fullscreen(&[self.keys[0].rtv[0].clone()], WIDTH, WIDTH);
        self.unbind();
        let mut k_src = 0;
        bind_shader(&self.ctx, &sh.sort_vs, cbs);
        let n = WIDTH * WIDTH;
        let mut size = 2;
        while size <= n {
            let mut j = size >> 1;
            while j > 0 {
                cbs.set_f("cb_vfx_bitonicsort", "g_compop_direction_swap", &[(size >> 1) as f32]);
                cbs.set_f("cb_vfx_bitonicsort", "g_compop_dist", &[j as f32]);
                cbs.set_f("cb_vfx_bitonicsort", "g_compop_dist_coord2", &[(j % WIDTH) as f32, (j / WIDTH) as f32]);
                cbs.set_f("cb_vfx_bitonicsort", "g_tex_width", &[WIDTH as f32]);
                cbs.set_f("cb_vfx_bitonicsort", "g_ascending_order", &[1.0]);
                let _ = cbs.upload(&self.dev, &self.ctx);
                bind_shader(&self.ctx, &sh.sort_ps, cbs);
                bind_srv(&self.ctx, &sh.sort_ps, "g_tex_packeddata", self.keys[k_src].srv.as_ref());
                self.fullscreen(&[self.keys[1 - k_src].rtv[0].clone()], WIDTH, WIDTH);
                self.unbind();
                k_src = 1 - k_src;
                j >>= 1;
            }

            size <<= 1;
        }

        self.sorted = k_src;

        // 4. Spawn into free slots (the tail of the sorted list).
        let verts = self.spawn(f);
        if !verts.is_empty() {
            self.draw_spawn(&verts, &sh, lib, cbs);
        }

        // 5. Draw, sorted far to near (the VS reads entry num_alive - vertex_id). The VS drops particles whose behaviour_mask.w differs
        // from g_view_id, so the same sorted list is drawn once per view.
        self.distortion_active = false;
        if self.alive > 0
            && let Some(target) = f.target.clone()
        {
            self.draw(f, &sh, &sh.ps, &target, VFX_MAIN_VIEW_ID, self.bs_premultiplied.clone(), cbs);
            if self.has_distortion {
                // The distortion buffer: RGBA8, xy = screen offset * 0.5 + 0.5 (neutral 127 / 255), blended with the particle alpha;
                // hdr_to_screen (the enable_distortion variant) offsets its uv by (xy - 0.498) * 0.02.
                if self.dist_w != f.width || self.dist_h != f.height {
                    self.create_distortion_target(f.width, f.height);
                }

                if let Some(rtv) = self.dist_rtv.clone() {
                    let neutral = [127.0 / 255.0, 127.0 / 255.0, 0.0, 0.0];
                    // SAFETY: a clear of a view of this device.
                    unsafe { self.ctx.ClearRenderTargetView(&rtv, &neutral) };
                    match lib.get(read, cbs, DISTORTION_PS) {
                        Ok(ps) => {
                            self.draw(f, &sh, &ps, &rtv, VFX_DISTORTION_VIEW_ID, self.bs_alpha.clone(), cbs);
                            self.distortion_active = true;
                        }

                        Err(error) => self.error(error.0),
                    }
                }
            }

            cbs.set_i("cb_vfx", "g_view_id", VFX_MAIN_VIEW_ID as i32);
            // SAFETY: a state call.
            unsafe { self.ctx.OMSetBlendState(None, None, 0xFFFF_FFFF) };
        }
    }

    fn create_distortion_target(&mut self, w: u32, h: u32) {
        self.dist_rtv = None;
        self.dist_srv = None;
        let td = D3D11_TEXTURE2D_DESC {
            Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let mut tex = None;
        // SAFETY: creation calls with a valid description.
        unsafe {
            let _ = self.dev.CreateTexture2D(&td, None, Some(&mut tex));
            if let Some(tex) = tex {
                let _ = self.dev.CreateRenderTargetView(&tex, None, Some(&mut self.dist_rtv));
                let _ = self.dev.CreateShaderResourceView(&tex, None, Some(&mut self.dist_srv));
            }
        }

        self.dist_w = w;
        self.dist_h = h;
    }

    fn draw_spawn(&mut self, verts: &[SpawnVertex], sh: &Shaders, lib: &mut ShaderLibrary, cbs: &ConstantBuffers) {
        if verts.len() > self.spawn_capacity {
            self.spawn_capacity = verts.len() + 4096;
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: (self.spawn_capacity * size_of::<SpawnVertex>()) as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_VERTEX_BUFFER as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, MiscFlags: 0, StructureByteStride: 0,
            };

            self.spawn_vb = None;
            // SAFETY: a plain creation call.
            let _ = unsafe { self.dev.CreateBuffer(&desc, None, Some(&mut self.spawn_vb)) };
        }

        let Some(vb) = self.spawn_vb.clone() else { return };
        let elements = [0u32, 16, 32].map(|offset| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(c"COLOR".as_ptr().cast()),
            SemanticIndex: offset / 16,
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: offset,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        });

        let Ok(layout) = lib.layout(&sh.spawn_vs, "vfx_spawn", &elements) else { return };
        // SAFETY: the vertex buffer is written within its size while mapped; drawing calls with objects of this device.
        unsafe {
            let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
            if self.ctx.Map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut ms)).is_err() {
                return;
            }

            std::ptr::copy_nonoverlapping(verts.as_ptr().cast::<u8>(), ms.pData.cast::<u8>(), std::mem::size_of_val(verts));
            self.ctx.Unmap(&vb, 0);
            self.ctx.IASetInputLayout(&layout);
            let stride = size_of::<SpawnVertex>() as u32;
            let offset = 0u32;
            self.ctx.IASetVertexBuffers(0, 1, Some(&Some(vb)), Some(&stride), Some(&offset));
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST));
            bind_shader(&self.ctx, &sh.spawn_vs, cbs);
            bind_srv(&self.ctx, &sh.spawn_vs, "g_tex_sorted_data", self.keys[self.sorted].srv.as_ref());
            bind_shader(&self.ctx, &sh.spawn_ps, cbs);
            self.bind_emitter_cbs(&sh.spawn_ps, true);
            bind_srv(&self.ctx, &sh.spawn_ps, "g_emitter_instances", self.emitter_srv.as_ref());
            bind_srv(&self.ctx, &sh.spawn_ps, "g_tex_sea_surface_anim", self.sea_srv.as_ref());
            bind_srv(&self.ctx, &sh.spawn_ps, "g_tex_vector_field", self.vector_field_srv.as_ref());
            bind_sampler(&self.ctx, &sh.spawn_ps, "g_sam_terrain_surface", self.s_linear_clamp.as_ref());
            bind_sampler(&self.ctx, &sh.spawn_ps, "g_sam_vector_field", self.s_linear_wrap.as_ref());
            self.ctx.OMSetRenderTargets(Some(&self.particles[self.cur].rtv), None);
            self.ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: WIDTH as f32, Height: WIDTH as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            self.ctx.Draw(verts.len() as u32, 0);
        }

        self.unbind();
        let zero = 0u32;
        // SAFETY: unbinding the vertex buffer.
        unsafe { self.ctx.IASetVertexBuffers(0, 1, Some(&None), Some(&zero), Some(&zero)) };
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, f: &Frame, sh: &Shaders, pixel: &GameShader, rtv: &ID3D11RenderTargetView, view_id: u32, blend: Option<ID3D11BlendState>, cbs: &mut ConstantBuffers) {
        cbs.set_i("cb_vfx", "g_view_id", view_id as i32);
        let _ = cbs.upload(&self.dev, &self.ctx);
        // SAFETY: drawing calls with objects of this device.
        unsafe {
            self.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), f.depth.as_ref());
            self.ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: f.width as f32, Height: f.height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            self.ctx.OMSetBlendState(blend.as_ref(), None, 0xFFFF_FFFF);
            self.ctx.OMSetDepthStencilState(if f.depth.is_some() { self.ds_test.as_ref() } else { self.ds_off.as_ref() }, 0);
            self.ctx.IASetInputLayout(None);
            self.ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST));
        }

        bind_shader(&self.ctx, &sh.vs, cbs);
        self.bind_emitter_cbs(&sh.vs, false);
        bind_srv(&self.ctx, &sh.vs, "g_emitter_instances", self.emitter_srv.as_ref());
        bind_srv(&self.ctx, &sh.vs, "g_particle_instances", self.particles[self.cur].srv.as_ref());
        bind_srv(&self.ctx, &sh.vs, "g_tex_sorted_data", self.keys[self.sorted].srv.as_ref());
        bind_srv(&self.ctx, &sh.vs, "g_tex_emitter_constants", self.curves_srv.as_ref());
        bind_sampler(&self.ctx, &sh.vs, "g_sam_emitter_constants", self.s_linear_clamp.as_ref());
        bind_shader(&self.ctx, &sh.gs, cbs);
        self.bind_emitter_cbs(&sh.gs, false);
        bind_shader(&self.ctx, pixel, cbs);
        self.bind_emitter_cbs(pixel, false);
        bind_srv(&self.ctx, pixel, "s_environment", f.sky.as_ref());
        bind_srv(&self.ctx, pixel, "s_sky", f.sky.as_ref());
        bind_srv(&self.ctx, pixel, "gbuffer_channel_4_sampler", f.depth_zw.as_ref());
        bind_srv(&self.ctx, pixel, "g_tex_diffuse", self.diffuse_srv.as_ref());
        bind_srv(&self.ctx, pixel, "g_tex_normal", self.normal_srv.as_ref());
        bind_sampler(&self.ctx, pixel, "s_environment", self.s_linear_clamp.as_ref());
        bind_sampler(&self.ctx, pixel, "s_sky", self.s_linear_clamp.as_ref());
        bind_sampler(&self.ctx, pixel, "gbuffer_channel_4_sampler", self.s_point_clamp.as_ref());
        bind_sampler(&self.ctx, pixel, "g_sam_diffuse", self.s_linear_wrap.as_ref());
        bind_sampler(&self.ctx, pixel, "g_sam_normal", self.s_linear_wrap.as_ref());
        // SAFETY: drawing calls.
        unsafe {
            self.ctx.Draw(self.alive + 1, 0);
            self.ctx.GSSetShader(None, None);
        }

        self.unbind();
    }
}
