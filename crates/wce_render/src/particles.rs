//! GPU particles with the game's particle_vfx_*.hlsl shaders.
//!
//! Per frame: simulation (a ping-pong of the particle state arrays) -> sort keys -> bitonic sort -> spawn (CPU-made points written into
//! free slots taken from the end of the sorted list) -> draw (the VS over the sorted list, GS quads). The CPU side (emission rate, spawn
//! positions and velocities) follows the XML semantics; the exact game code is not fully reverse-engineered yet, see the ASSUMPTION notes.

use std::collections::{BTreeMap, VecDeque};
use std::f32::consts::PI;

use wce_assets::vfx_constants::{VFX_CURVE_WIDTH, VFX_DISTORTION_VIEW_ID, VFX_MAIN_VIEW_ID, vfx_bits};
use wce_assets::weather_vfx::{Effect, VfxReader, WeatherVfx, load_effect_with, load_weather_vfx};
use wce_assets::vfx_def::VfxFile;
use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, length, normalize, rotation_x, rotation_y, rotation_z};
use windows_core::Interface;

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
    dev.create_buffer(&desc, Some(data)).ok()
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
        let mut td = first.srv.as_ref()?.resource().ok()?.cast::<ID3D11Texture2D>().ok()?.desc();
        let mips = td.MipLevels;
        td.ArraySize = self.textures.len() as u32;
        td.Usage = D3D11_USAGE_DEFAULT;
        td.MiscFlags = 0;
        let array = dev.create_texture_2d(&td, None).ok()?;
        for (s, texture) in self.textures.iter().enumerate() {
            let resource = texture.srv.as_ref()?.resource().ok()?;
            for m in 0..mips {
                ctx.copy_subresource_region(&array, m + s as u32 * mips, [0; 3], &resource, m, None);
            }
        }

        let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: first.format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
        sd.Anonymous.Texture2DArray.MipLevels = mips;
        sd.Anonymous.Texture2DArray.ArraySize = td.ArraySize;
        dev.create_shader_resource_view(&array, Some(&sd)).ok()
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
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpawnVertex {
    /// Emitter instance, random seed, pre-simulation sub-steps (0.2 s each), 0.
    pub id_seed_steps: [f32; 4],

    /// Position, spawn time.
    pub pos_time: [f32; 4],

    /// Velocity, lifetime.
    pub vel_life: [f32; 4],
}

/// A point on a box's surface (size `v`, centred on the origin) without its bottom face (the ground), faces chosen by their area, and the
/// face's normal; `r` three randoms in [0, 1).
fn box_surface_point(v: [f32; 3], r: [f32; 3]) -> (Float3, Float3) {
    let h = v.map(|x| 0.5 * x.max(0.0));

    // The top, then the four sides (+x, -x, +z, -z).
    let areas = [v[0] * v[2], v[1] * v[2], v[1] * v[2], v[0] * v[1], v[0] * v[1]].map(|a| a.max(0.0));
    let total = areas.iter().sum::<f32>();
    if total <= 0.0 {
        return (Float3::default(), Float3::new(0.0, 1.0, 0.0));
    }

    let mut pick = r[0] * total;
    let face = areas.iter().position(|&a| {
        pick -= a;
        pick < 0.0
    }).unwrap_or(0);

    let (u, w) = (2.0 * r[1] - 1.0, 2.0 * r[2] - 1.0);
    match face {
        0 => (Float3::new(u * h[0], h[1], w * h[2]), Float3::new(0.0, 1.0, 0.0)),
        1 => (Float3::new(h[0], u * h[1], w * h[2]), Float3::new(1.0, 0.0, 0.0)),
        2 => (Float3::new(-h[0], u * h[1], w * h[2]), Float3::new(-1.0, 0.0, 0.0)),
        3 => (Float3::new(u * h[0], w * h[1], h[2]), Float3::new(0.0, 0.0, 1.0)),
        _ => (Float3::new(u * h[0], w * h[1], -h[2]), Float3::new(0.0, 0.0, -1.0)),
    }
}

/// A placed effect (a tile's VFXInstance): `vfx/<effect>.xml` at a world transform (rotation and position; scale is `scale`).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldEffect {
    /// Stable across frames (the entity id): an effect keeps its particles and emitter state while it stays.
    pub key: u64,
    pub effect: String,
    pub world: Float4x4,

    /// The effect's scale: the tile's `scale` / unit_scale, as BOB writes it into particle_emitter_marker.markers.
    pub scale: f32,

    /// The box (size, metres, centred on the effect) its mesh emitters spawn over (render_scene::VfxEmitter::volume).
    pub volume: Option<[f32; 3]>,
}

/// One emitter of a placed effect: an emitter instance after the weather's.
#[derive(Clone, Debug)]
struct WorldInstance {
    key: u64,

    /// Into `world_effects`, and the emitter there.
    effect: usize,
    emitter: usize,
    pos: Float3,

    /// The rotation (no translation) and its quaternion (x, y, z, w).
    rot: Float4x4,
    quat: [f32; 4],

    /// The total scale: the effect's times the reference's (drawn in its range).
    scale: f32,
    volume: Option<[f32; 3]>,
    tint: [f32; 4],

    /// The instance's own generator (game: emitter instance +0xc0, 0x343fd / 0x269ec3).
    rng: u32,

    /// The spawn area's min -> max blend (+0xd8) and the particle lifetime bias (+0xa0), drawn once.
    area_blend: f32,
    lifetime_bias: f32,

    /// Added to the emitter's LifeTime (the reference's emitter_lifetime_bias draw).
    emitter_life_bias: f32,

    /// The clock when it started.
    start: f32,

    /// The last spawn time (fn 0x1188e240 spreads a frame's particles evenly from it).
    last_spawn: f32,
    st: EmitterState,
}

/// A `lighting/lights.xml` POINT_LIGHT (read by fn 0x11900a40): the light a rigid model's StaticPointLight mesh names.
#[derive(Clone, Debug, PartialEq)]
struct LightDef {
    colour: [f32; 3],
    radius: f32,

    /// 0 none, 1 ANIMATION_RADIUS_SIN, 2 ANIMATION_RADIUS_SIN_SIN (they modulate the colour).
    animation: u8,
    speed: [f32; 2],
    colour_scale: f32,
    colour_min: f32,

    /// `falloff`: smooth unless it says linear.
    smooth: bool,
}

/// The bursts of a rigid emitter (fn 0x11895130): a burst at T spawns round(lerp(RigidsPerBurst)) rigids spread over its length
/// (lerp(BurstIntervalLength)), each living lerp(RigidLifeTime); the next burst is at T + lerp(BurstInterval) + length; Bursts of them
/// unless InfiniteBursts.
#[derive(Clone, Debug, Default)]
struct RigidBursts {
    next: f32,
    left: i32,
    started: bool,

    /// (birth, life) of the live rigids.
    rigids: Vec<(f32, f32)>,
    rng: u32,
}

impl RigidBursts {
    fn update(&mut self, d: &wce_assets::vfx_def::VfxEmitterDef, clock: f32, start: f32) {
        let range = |name: &str, lo: f32, hi: f32| d.range(name, lo, hi);
        if !self.started {
            self.started = true;
            self.next = start;
            let (b0, b1) = range("Bursts", 1.0, 1.0);
            self.left = lerp(b0, b1, game_rand(&mut self.rng)).round() as i32;
        }

        let infinite = d.b("InfiniteBursts", false);
        let mut guard = 0;
        while self.next <= clock && (infinite || self.left > 0) && guard < 64 {
            guard += 1;
            let (i0, i1) = range("BurstInterval", 0.0, 0.0);
            let (l0, l1) = range("BurstIntervalLength", 0.0, 0.0);
            let (n0, n1) = range("RigidsPerBurst", 1.0, 1.0);
            let (r0, r1) = range("RigidLifeTime", 2.0, 2.0);
            let interval = lerp(i0, i1, game_rand(&mut self.rng));
            let length = lerp(l0, l1, game_rand(&mut self.rng));
            let n = lerp(n0, n1, game_rand(&mut self.rng)).round().max(0.0) as u32;
            for i in 0..n {
                let birth = self.next + length * i as f32 / n as f32;
                let life = lerp(r0, r1, game_rand(&mut self.rng)).max(1e-3);
                self.rigids.push((birth, life));
            }

            self.left -= 1;
            self.next += (interval + length).max(1e-3);
        }

        self.rigids.retain(|&(birth, life)| birth + life > clock);
    }
}

/// A light of a placed effect: a rigid emitter whose model is a point light.
#[derive(Clone, Debug)]
struct WorldLight {
    key: u64,

    /// Into `world_effects[effect].lights`.
    effect: usize,
    light: usize,
    pos: Float3,
    rot: Float4x4,
    scale: f32,
    start: f32,
    bursts: RigidBursts,
}

/// A point light of the placed effects this frame, for the light pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectPointLight {
    pub pos: Float3,

    /// The colour times its level this frame (before the HDR scale).
    pub colour: [f32; 3],
    pub radius: f32,
    pub smooth: bool,
}

/// The StaticPointLight meshes of a rigid model: (light name, pivot).
fn model_lights(bytes: &[u8]) -> Vec<(String, Float3)> {
    use rpfm_lib::files::rigidmodel::RigidModel;
    use rpfm_lib::files::rigidmodel::materials::MaterialType;
    use rpfm_lib::files::{Decodeable, DecodeableExtraData};
    let Ok(model) = RigidModel::decode(&mut std::io::Cursor::new(bytes), &Some(DecodeableExtraData::default())) else { return vec![] };
    let Some(lod) = model.lods().first() else { return vec![] };
    lod.mesh_blocks().iter()
        .filter(|b| *b.mesh().material_type() == MaterialType::StaticPointLight)
        .map(|b| {
            let p = b.material().v_pivot();
            (b.material().name().to_string(), Float3::new(p.x, p.y, p.z))
        })
        .collect()
}

/// `lighting/lights.xml`.
fn parse_light_defs(bytes: &[u8]) -> BTreeMap<String, LightDef> {
    let mut out = BTreeMap::new();
    let Ok(doc) = wce_formats::xml_doc::XmlDoc::parse(bytes.to_vec()) else { return out };
    for e in 0..doc.elements().len() {
        if doc.elements()[e].name != "POINT_LIGHT" {
            continue;
        }

        let n = |name: &str, d: f32| doc.get(e, name).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
        let animation = match doc.get(e, "animation").as_deref() {
            Some("ANIMATION_RADIUS_SIN") => 1,
            Some("ANIMATION_RADIUS_SIN_SIN") => 2,
            _ => 0,
        };

        out.insert(doc.get(e, "name").unwrap_or_default(), LightDef {
            colour: [n("red", 0.0), n("green", 0.0), n("blue", 0.0)],
            radius: n("radius", 0.0),
            animation,
            speed: [n("animation_x", 0.0), n("animation_y", 0.0)],
            colour_scale: n("colour_scale", 1.0),
            colour_min: n("colour_min", 0.0),
            smooth: !doc.get(e, "falloff").is_some_and(|v| v.to_lowercase().contains("linear")),
        });
    }

    out
}

/// The rotation part of a matrix applied to a vector.
fn rotate(m: &Float4x4, v: Float3) -> Float3 {
    Float3::new(
        m.m[0][0] * v.x + m.m[0][1] * v.y + m.m[0][2] * v.z,
        m.m[1][0] * v.x + m.m[1][1] * v.y + m.m[1][2] * v.z,
        m.m[2][0] * v.x + m.m[2][1] * v.y + m.m[2][2] * v.z,
    )
}

/// A world matrix's rotation without its scale, and the quaternion (x, y, z, w) of it.
fn rotation_of(w: &Float4x4) -> (Float4x4, [f32; 4]) {
    let mut r = Float4x4::IDENTITY;
    for c in 0..3 {
        let len = (w.m[0][c] * w.m[0][c] + w.m[1][c] * w.m[1][c] + w.m[2][c] * w.m[2][c]).sqrt().max(1e-12);
        for row in 0..3 {
            r.m[row][c] = w.m[row][c] / len;
        }
    }

    let m = &r.m;
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    };

    (r, q)
}

/// The game's generator step (0x343fd / 0x269ec3) and its [0, 1] number ((state >> 16) / 65535).
fn game_rand(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
    (*state >> 16) as f32 * 1.525_902_2e-5
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

    /// The placed effects' definitions (loaded once per name; their emitters' constants rows follow the weather's) and emitter instances.
    world_effects: Vec<Effect>,
    world_names: BTreeMap<String, usize>,
    world_libs: BTreeMap<String, VfxFile>,
    world: Vec<WorldInstance>,
    world_lights: Vec<WorldLight>,

    /// The weather's rigid lights (at the camera) and their bursts.
    weather_lights: Vec<RigidBursts>,

    /// `lighting/lights.xml`, and per light model its StaticPointLight meshes, loaded on first use.
    light_defs: Option<BTreeMap<String, LightDef>>,
    light_models: BTreeMap<String, Vec<(String, Float3)>>,

    /// The game's shared generator (DAT_11da7408: the spawn box's three numbers).
    global_rng: u32,

    /// The emitter instance texture's rows (256 instances each).
    emitter_rows: usize,
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
    dev.create_blend_state(&desc).ok()
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
            world_effects: vec![],
            world_names: BTreeMap::new(),
            world_libs: BTreeMap::new(),
            world: vec![],
            world_lights: vec![],
            weather_lights: vec![],
            light_defs: None,
            light_models: BTreeMap::new(),
            global_rng: 0x1234_5678,
            emitter_rows: 1,
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
            wce_foundation::log::error(format!("vfx {e}"));
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

            let Ok(tex) = dev.create_texture_2d(&td, None) else { return t };
            let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: fmt, ..Default::default() };
            if slices > 1 {
                sd.ViewDimension = D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY);
                sd.Anonymous.Texture2DArray.MipLevels = 1;
                sd.Anonymous.Texture2DArray.ArraySize = slices;
            } else {
                sd.ViewDimension = D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2D);
                sd.Anonymous.Texture2D.MipLevels = 1;
            }

            t.srv = dev.create_shader_resource_view(&tex, Some(&sd)).ok();
            for s in 0..slices {
                let mut rd = D3D11_RENDER_TARGET_VIEW_DESC { Format: fmt, ..Default::default() };
                if slices > 1 {
                    rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2DARRAY;
                    rd.Anonymous.Texture2DArray.FirstArraySlice = s;
                    rd.Anonymous.Texture2DArray.ArraySize = 1;
                } else {
                    rd.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
                }

                t.rtv[s as usize] = dev.create_render_target_view(&tex, Some(&rd)).ok();
            }

            t
        };

        self.particles = [make_target(DXGI_FORMAT_R32G32B32A32_FLOAT, 3), make_target(DXGI_FORMAT_R32G32B32A32_FLOAT, 3)];
        self.keys = [make_target(DXGI_FORMAT_R32G32_FLOAT, 1), make_target(DXGI_FORMAT_R32G32_FLOAT, 1)];

        self.create_emitter_texture(1);

        // A flat normal (1 slice) and a still sea surface (2 slices; the game logic's collision is off for weather).
        let flat = 0xFFFF_8080u32.to_le_bytes();
        let td = D3D11_TEXTURE2D_DESC {
            Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let mut vd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: td.Format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
        vd.Anonymous.Texture2DArray.MipLevels = 1;
        vd.Anonymous.Texture2DArray.ArraySize = 1;
        self.flat_normal_srv = dev.create_texture_2d(&td, Some(&[InitData { data: &flat, row_pitch: 4, slice_pitch: 4 }]))
            .and_then(|t| dev.create_shader_resource_view(&t, Some(&vd)))
            .ok();

        self.normal_srv = self.flat_normal_srv.clone();
        let zero = [0u8; 16];
        let sdsc = D3D11_TEXTURE2D_DESC {
            Width: 1, Height: 1, MipLevels: 1, ArraySize: 2, Format: DXGI_FORMAT_R32G32B32A32_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let ss = [InitData { data: &zero, row_pitch: 16, slice_pitch: 16 }; 2];
        self.sea_srv = dev.create_texture_2d(&sdsc, Some(&ss)).and_then(|st| dev.create_shader_resource_view(&st, None)).ok();

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

                let sd = [InitData { data: &mid.to_le_bytes(), row_pitch: 4, slice_pitch: 4 }];
                self.vector_field_srv = dev.create_texture_3d(&vd, Some(&sd)).and_then(|t| dev.create_shader_resource_view(&t, None)).ok();
            }
        }

        let sampler = |filter: D3D11_FILTER, address: D3D11_TEXTURE_ADDRESS_MODE| {
            let sd = D3D11_SAMPLER_DESC { Filter: filter, AddressU: address, AddressV: address, AddressW: address, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default() };
            dev.create_sampler_state(&sd).ok()
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
        self.ds_test = dev.create_depth_stencil_state(&dd).ok();
        dd.DepthEnable = false.into();
        self.ds_off = dev.create_depth_stencil_state(&dd).ok();
        self.rs_none = dev.create_rasterizer_state(&rd).ok();
    }

    /// The emitter instance texture (g_emitter_instances: 6 layers of 256 x `rows`; the shaders address instance i at (i % 256, i / 256)).
    fn create_emitter_texture(&mut self, rows: usize) {
        let ed = D3D11_TEXTURE2D_DESC {
            Width: EMITTER_WIDTH as u32, Height: rows as u32, MipLevels: 1, ArraySize: EMITTER_LAYERS as u32, Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        self.emitter_tex = self.dev.create_texture_2d(&ed, None).ok();
        self.emitter_srv = self.emitter_tex.as_ref().and_then(|tex| self.dev.create_shader_resource_view(tex, None).ok());
        self.emitter_rows = rows;
    }

    /// The placed effects of this frame. Effects are loaded on first use (their emitters become constants rows after the weather's);
    /// an effect that stays (same key and name) keeps its emitter state, so its particles carry on.
    pub fn set_world_effects(&mut self, effects: &[WorldEffect]) {
        let mut loaded_new = false;
        for fx in effects {
            if !self.world_names.contains_key(&fx.effect) {
                let effect = load_effect_with(&*self.reader, &fx.effect, &mut self.world_libs);
                for note in &effect.notes {
                    self.error(format!("{}: {note}", fx.effect));
                }

                self.world_names.insert(fx.effect.clone(), self.world_effects.len());
                self.world_effects.push(effect);
                loaded_new = true;
            }
        }

        if loaded_new {
            let rows = self.effect.emitters.len() + self.world_effects.iter().map(|e| e.emitters.len()).sum::<usize>();
            if rows > MAX_EMITTERS {
                self.error(format!("{rows} emitter definitions, more than the {MAX_EMITTERS} the shaders' constant arrays hold: the rest are not drawn"));
            }

            self.build_emitter_gpu();
        }

        // Keep the state of the instances that stay.
        let mut old = BTreeMap::<(u64, usize), WorldInstance>::new();
        for inst in self.world.drain(..) {
            old.insert((inst.key, inst.emitter), inst);
        }

        let clock = self.clock;
        let mut world = Vec::new();
        for fx in effects {
            let e = self.world_names[&fx.effect];
            let (rot, quat) = rotation_of(&fx.world);
            let origin = Float3::new(fx.world.m[0][3], fx.world.m[1][3], fx.world.m[2][3]);
            for (k, em) in self.world_effects[e].emitters.iter().enumerate() {
                let reference = Float3::new(em.pos[0], em.pos[1], em.pos[2]);
                let inst = match old.remove(&(fx.key, k)).filter(|inst| inst.effect == e) {
                    Some(mut inst) => {
                        inst.pos = origin + rotate(&rot, reference * fx.scale);
                        inst.rot = rot;
                        inst.quat = quat;
                        inst.volume = fx.volume;
                        inst
                    }

                    None => {
                        // Per reference, one set of draws shared by its emitters (fn 0x1181a930 / 0x117f1860, from the shared generator):
                        // scale lerp(scale_min, max), then one blend for the tint's four channels, the particle lifetime bias and the
                        // emitter lifetime bias. The generators are seeded from the key: every vanilla emitter has RandomSeed -1 (random).
                        let mut shared = (fx.key as u32).wrapping_mul(0x9E37_79B9) ^ em.instance.bytes().fold(0x2545_F491u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
                        let ref_scale = lerp(em.scale_range[0], em.scale_range[1], game_rand(&mut shared));
                        let (t_tint, t_life, t_emitter) = (game_rand(&mut shared), game_rand(&mut shared), game_rand(&mut shared));
                        let mut rng = shared ^ (k as u32).wrapping_mul(0x85EB_CA6B);
                        let area_blend = game_rand(&mut rng);
                        let tint = std::array::from_fn(|c| lerp(em.tint_min[c], em.tint_max[c], t_tint));
                        WorldInstance {
                            key: fx.key, effect: e, emitter: k, pos: origin + rotate(&rot, reference * fx.scale), rot, quat, scale: fx.scale * ref_scale, volume: fx.volume, tint, rng,
                            area_blend, lifetime_bias: lerp(em.particle_lifetime_bias[0], em.particle_lifetime_bias[1], t_life),
                            emitter_life_bias: lerp(em.emitter_lifetime_bias[0], em.emitter_lifetime_bias[1], t_emitter),
                            start: clock, last_spawn: clock, st: EmitterState::default(),
                        }
                    }
                };

                world.push(inst);
            }
        }

        self.world = world;

        // The effects' lights (their rigid point light models, read once).
        let old_lights = std::mem::take(&mut self.world_lights).into_iter().map(|l| ((l.key, l.light), l)).collect::<BTreeMap<_, _>>();
        for fx in effects {
            let e = self.world_names[&fx.effect];
            let (rot, _) = rotation_of(&fx.world);
            let origin = Float3::new(fx.world.m[0][3], fx.world.m[1][3], fx.world.m[2][3]);
            for (k, light) in self.world_effects[e].lights.iter().enumerate() {
                if !self.light_models.contains_key(&light.model) {
                    let lights = (self.reader)(&light.model).map(|bytes| model_lights(&bytes)).unwrap_or_default();
                    self.light_models.insert(light.model.clone(), lights);
                }

                let reference = Float3::new(light.pos[0], light.pos[1], light.pos[2]);
                let scale = fx.scale * 0.5 * (light.scale_range[0] + light.scale_range[1]);
                let (start, bursts) = old_lights.get(&(fx.key, k)).map_or_else(
                    || (clock, RigidBursts { rng: (fx.key as u32).wrapping_mul(0x9E37_79B9) ^ 0x6C8E_9CF5 ^ k as u32, ..Default::default() }),
                    |l| (l.start, l.bursts.clone()),
                );
                self.world_lights.push(WorldLight { key: fx.key, effect: e, light: k, pos: origin + rotate(&rot, reference * fx.scale), rot, scale, start, bursts });
            }
        }

        let rows = (self.effect.emitters.len() + self.world.len()).div_ceil(EMITTER_WIDTH).max(1);
        if rows > self.emitter_rows {
            self.create_emitter_texture(rows.next_power_of_two());
        }
    }

    /// The point lights of the placed effects and the weather now. A rigid point light emitter makes rigids in bursts ([`RigidBursts`];
    /// the fires keep one alive: BurstInterval and RigidLifeTime 64 s; the storms' lightning flashes 1-3 for 0.1 s every 30-60 s): the model's StaticPointLight is submitted every frame (fn 0x117f9800) at the rigid's matrix times
    /// the mesh pivot, with radius = the definition's * the matrix's scale (the rigid's ParticleScale curve, start -> mid -> end over its
    /// life, times the effect's scale), and the definition's colour, colour_scale, colour_min and animation (as the tile's lights, see
    /// renderer/lights.rs). Lights beyond the emitter's CullDistances.w are skipped. ASSUMPTION: the per-instance phase (render context
    /// +0x30) is 0; the rigid's position is SpawnPosition * scale (the fire lights have no spawn area).
    pub fn effect_lights(&mut self, eye: Float3) -> Vec<EffectPointLight> {
        if (self.world_lights.is_empty() && self.effect.lights.is_empty()) || !self.enabled {
            return vec![];
        }

        if self.light_defs.is_none() {
            let defs = (self.reader)("lighting/lights.xml").map(|bytes| parse_light_defs(&bytes)).unwrap_or_default();
            if defs.is_empty() {
                self.error("lighting/lights.xml: no point light definitions".to_owned());
            }

            self.light_defs = Some(defs);
        }

        // The light models of the weather's lights (the placed effects' are read when they are placed).
        for light in &self.effect.lights {
            if !self.light_models.contains_key(&light.model) {
                let lights = (self.reader)(&light.model).map(|bytes| model_lights(&bytes)).unwrap_or_default();
                self.light_models.insert(light.model.clone(), lights);
            }
        }

        if self.weather_lights.len() != self.effect.lights.len() {
            self.weather_lights = (0..self.effect.lights.len()).map(|k| RigidBursts { rng: 0x5EED_0000 ^ k as u32, ..Default::default() }).collect();
        }

        let defs = self.light_defs.take().unwrap_or_default();
        let clock = self.clock;
        let mut out = vec![];
        let mut emit = |light: &wce_assets::weather_vfx::EffectLight, pos: Float3, rot: &Float4x4, scale: f32, bursts: &mut RigidBursts, start: f32, models: &BTreeMap<String, Vec<(String, Float3)>>| {
            let d = &light.def;
            let mut cull = [0.0f32; 4];
            d.vec("CullDistances", &mut cull);
            let mut sp = [0.0f32; 3];
            d.vec("SpawnPosition", &mut sp);
            let base = pos + rotate(rot, Float3::new(sp[0], sp[1], sp[2]) * scale);
            bursts.update(d, clock, start);
            if cull[3] > 0.0 && length(eye - base) > cull[3] {
                return;
            }

            let (mut s0, mut s1, mut s2) = ([1.0f32; 2], [1.0f32; 2], [1.0f32; 2]);
            d.vec("ParticleScaleStart", &mut s0);
            d.vec("ParticleScaleMid", &mut s1);
            d.vec("ParticleScaleEnd", &mut s2);
            let mid = d.f("ParticleScaleMidPointTime", 0.5).clamp(1e-4, 1.0 - 1e-4);
            for &(birth, life) in &bursts.rigids {
                if birth > clock {
                    continue;
                }

                let t = ((clock - birth) / life).clamp(0.0, 1.0);
                let curve = if t < mid { lerp(s0[0], s1[0], t / mid) } else { lerp(s1[0], s2[0], (t - mid) / (1.0 - mid)) };
                let rigid_scale = curve * scale;
                for (name, pivot) in models.get(&light.model).map_or(&[][..], |v| &v[..]) {
                    let Some(def) = defs.get(name) else { continue };
                    let wave = |speed: f32| (clock * speed).sin().abs();
                    let level = match def.animation {
                        1 => def.colour_scale * wave(def.speed[0]) + def.colour_min,
                        2 => def.colour_scale * (wave(def.speed[0]) * wave(def.speed[1])) + def.colour_min,
                        _ => def.colour_scale + def.colour_min,
                    };

                    out.push(EffectPointLight {
                        pos: base + rotate(rot, *pivot * rigid_scale),
                        colour: def.colour.map(|c| c * level),
                        radius: def.radius * rigid_scale,
                        smooth: def.smooth,
                    });
                }
            }
        };

        for wl in &mut self.world_lights {
            let light = &self.world_effects[wl.effect].lights[wl.light];
            emit(light, wl.pos, &wl.rot, wl.scale, &mut wl.bursts, wl.start, &self.light_models);
        }

        // The weather's: camera-following, at the reference's position, scale = the middle of its range.
        for (light, bursts) in self.effect.lights.iter().zip(self.weather_lights.iter_mut()) {
            let pos = eye + Float3::new(light.pos[0], light.pos[1], light.pos[2]);
            let scale = 0.5 * (light.scale_range[0] + light.scale_range[1]);
            emit(light, pos, &Float4x4::IDENTITY, scale, bursts, 0.0, &self.light_models);
        }

        self.light_defs = Some(defs);
        out
    }

    /// The constants row of a world instance.
    fn world_row(&self, inst: &WorldInstance) -> usize {
        self.effect.emitters.len() + self.world_effects[..inst.effect].iter().map(|e| e.emitters.len()).sum::<usize>() + inst.emitter
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
        self.weather_lights = (0..self.effect.lights.len()).map(|k| RigidBursts { rng: 0x5EED_0000 ^ k as u32, ..Default::default() }).collect();
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
        // The rows: the weather's emitters, then each placed effect's.
        let mut sources = (0..self.effect.emitters.len()).map(|e| (usize::MAX, e)).collect::<Vec<_>>();
        for (fx, effect) in self.world_effects.iter().enumerate() {
            sources.extend((0..effect.emitters.len()).map(|e| (fx, e)));
        }

        sources.truncate(MAX_EMITTERS);
        let n = sources.len();
        for (e, &(fx, i)) in sources.iter().enumerate() {
            let k = if fx == usize::MAX { &self.effect.emitters[i].k } else { &self.world_effects[fx].emitters[i].k };
            let (dpath, npath) = (k.diffuse.clone(), k.normal.clone());
            let ds = diffuse.add(&self.dev, &*self.reader, &mut self.texture_cache, &dpath, &mut tex_errors);
            let ns = normal.add(&self.dev, &*self.reader, &mut self.texture_cache, &npath, &mut tex_errors);
            let k = if fx == usize::MAX { &mut self.effect.emitters[i].k } else { &mut self.world_effects[fx].emitters[i].k };
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
        for (chunk, &(fx, i)) in texels.chunks_mut(slice).zip(&sources) {
            let k = if fx == usize::MAX { &self.effect.emitters[i].k } else { &self.world_effects[fx].emitters[i].k };
            chunk.copy_from_slice(k.curves.as_flattened().as_flattened());
        }

        let subs = texels.chunks(slice)
            .map(|chunk| InitData { data: bytemuck::cast_slice(chunk), row_pitch: (VFX_CURVE_WIDTH * 16) as u32, slice_pitch: (VFX_CURVE_WIDTH * 16 * 8) as u32 })
            .collect::<Vec<_>>();
        let td = D3D11_TEXTURE2D_DESC {
            Width: VFX_CURVE_WIDTH as u32, Height: 8, MipLevels: 1, ArraySize: layers as u32, Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: td.Format, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2DARRAY), ..Default::default() };
        sd.Anonymous.Texture2DArray.MipLevels = 1;
        sd.Anonymous.Texture2DArray.ArraySize = layers as u32;
        self.curves_srv = self.dev.create_texture_2d(&td, Some(&subs)).and_then(|t| self.dev.create_shader_resource_view(&t, Some(&sd))).ok();

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
            match s.stage {
                Stage::Vs => self.ctx.vs_set_constant_buffers(slot, &buffers),
                Stage::Gs => self.ctx.gs_set_constant_buffers(slot, &buffers),
                Stage::Ps => self.ctx.ps_set_constant_buffers(slot, &buffers),
                _ => {}
            }
        }
    }

    fn fullscreen(&self, rtvs: &[Option<ID3D11RenderTargetView>], w: u32, h: u32) {
        self.ctx.set_render_targets(rtvs, None);
        self.ctx.set_viewports(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.draw(3, 0);
    }

    fn unbind(&self) {
        let none: [Option<ID3D11ShaderResourceView>; 8] = Default::default();
        self.ctx.set_render_targets(&[], None);
        self.ctx.vs_set_shader_resources(0, &none);
        self.ctx.ps_set_shader_resources(0, &none);
        self.ctx.gs_set_shader_resources(0, &none);
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

        self.spawn_world(f, &mut out, &mut free_slots);
        out
    }

    /// The placed effects' particles, as the game's continuous emitter makes them (fn 0x1188e240 -> fn 0x1181bdc0):
    /// - emission: accumulate min(dt, 0.1) * rate (lerp(min, max curve, a per-instance blend), times the owner's multipliers +0xb4 and
    ///   +0x78^2), spawn floor(gate * floor(accum)) spread evenly from the last spawn time, the burst gate as for the weather;
    /// - lifetime (fn 0x11835b50): lerp(ParticleLifeTime) + the instance's lifetime bias;
    /// - position (fn 0x1183f4a0): a ring (angle lerp(SpawnArc) degrees, radius lerp(inner, outer, random) where inner and outer are the
    ///   SpawnAreaCurve min -> max lerped by the instance's area blend) plus a box (SpawnAreaBox size per axis, the same blend, times
    ///   (random - 0.5) from the shared generator), times the scale; turned by EmitterRotation, plus SpawnPosition * scale, turned by the
    ///   instance's rotation, plus its position;
    /// - direction (fn 0x1183f1a0): SpreadMethod true: elevation lerp(SpawnSpread) degrees above the horizon, azimuth random over the
    ///   circle, dir = (cos az cos el, sin el, -sin az cos el); false: away from SpawnPosition; both turned by the emitter and the instance;
    /// - velocity: dir * lerp(SpawnSpeed) * scale.
    ///
    /// The owner's scale (+0x70) is the effect's and its emission multipliers (+0xb4, +0x78) are 1, as for an effect placed at a transform
    /// (fn 0x1046c320; the marker path was not followed to the setters). Verified: the ring's x is the cosine (the sincos call,
    /// fn 0x119eb652, returns (sin, cos); x takes lane 1).
    fn spawn_world(&mut self, f: &Frame, out: &mut Vec<SpawnVertex>, free_slots: &mut u32) {
        let base = self.effect.emitters.len();
        for j in 0..self.world.len() {
            let row = self.world_row(&self.world[j]);
            if row >= MAX_EMITTERS {
                continue;
            }

            let (e, k) = (self.world[j].effect, self.world[j].emitter);
            let a = &self.world_effects[e].emitters[k];
            let d = a.def.clone();
            let infinite = a.infinite_life;
            let inst = &mut self.world[j];

            // The emitter's culling distance (CullDistances.w): no spawning beyond it.
            let mut cull = [0.0f32; 4];
            d.vec("CullDistances", &mut cull);
            let dist = length(f.eye - inst.pos);
            if cull[3] > 0.0 && dist > cull[3] {
                inst.st.rate_accum = 0.0;
                inst.last_spawn = self.clock;
                continue;
            }

            let (life_lo, life_hi) = d.range("ParticleLifeTime", 1.0, 1.0);
            let (start_lo, start_hi) = d.range("StartTime", 0.0, 0.0);
            let (emitter_life_lo, emitter_life_hi) = d.range("LifeTime", 50.0, 50.0);
            let emitter_life = (0.5 * (emitter_life_lo + emitter_life_hi) + inst.emitter_life_bias).max(0.001);
            let pre = (-0.5 * (start_lo + start_hi)).max(0.0);
            let (mut window_start, mut window) = (inst.last_spawn, (self.clock - inst.last_spawn).max(0.0));
            if !inst.st.started {
                inst.st.started = true;
                window_start = self.clock - pre;
                window = pre + f.dt;
            }

            let age = self.clock - inst.start + pre;
            if !infinite && !d.b("InfiniteLife", false) && age > emitter_life {
                continue;
            }

            let loop_rate = d.b("EmissionLoopAnim", false);
            let t_rate = if loop_rate { (age / emitter_life) % 1.0 } else { (age / emitter_life).min(1.0) };
            let loop_area = d.b("SpawnAreaLoopAnim", true);
            let t_area = if loop_area { (age / emitter_life) % 1.0 } else { (age / emitter_life).min(1.0) };
            if inst.st.rate_blend < 0.0 {
                inst.st.rate_blend = game_rand(&mut inst.rng);
            }

            let curve = |name: &str, sub: &str, t: f32| d.curve(name, sub).map_or(0.0, |c| c.eval(t));
            let rate = lerp(curve("EmissionRateCurve", "emission_min", t_rate), curve("EmissionRateCurve", "emission_max", t_rate), inst.st.rate_blend).max(0.0);

            // The burst gate (fn 0x1180c680 / 0x1180bf70).
            let (bi_lo, bi_hi) = d.range("BurstInterval", 0.0, 0.0);
            let (bl_lo, bl_hi) = d.range("BurstIntervalLength", 0.0, 0.0);
            if inst.st.burst_interval < 0.0 {
                inst.st.burst_interval = lerp(bi_lo, bi_hi, game_rand(&mut inst.rng));
                inst.st.burst_length = lerp(bl_lo, bl_hi, game_rand(&mut inst.rng));
            }

            let mut gate = 1.0f32;
            if inst.st.burst_interval > 0.0 {
                gate = if inst.st.burst_timer <= inst.st.burst_length { 1.0 } else { 0.0 };
                if inst.st.burst_length + inst.st.burst_interval <= inst.st.burst_timer {
                    inst.st.burst_interval = lerp(bi_lo, bi_hi, game_rand(&mut inst.rng));
                    inst.st.burst_length = lerp(bl_lo, bl_hi, game_rand(&mut inst.rng));
                    inst.st.burst_timer = 0.0;
                } else {
                    inst.st.burst_timer += f.dt;
                }
            }

            inst.st.rate_accum += rate * if window <= f.dt + 1e-6 { window.min(0.1) } else { window };
            let whole = inst.st.rate_accum.floor();
            let count = (gate * whole).floor() as u32;
            inst.st.rate_accum -= whole;
            inst.last_spawn = self.clock;
            if count == 0 {
                continue;
            }

            let blend = inst.area_blend;
            let ring = |sub: [&str; 2]| lerp(curve("SpawnAreaCurve", sub[0], t_area), curve("SpawnAreaCurve", sub[1], t_area), blend);
            let (inner, outer) = (ring(["inner_radius_min", "inner_radius_max"]), ring(["outer_radius_min", "outer_radius_max"]));
            let size = [["x_min", "x_max"], ["y_min", "y_max"], ["z_min", "z_max"]]
                .map(|[lo, hi]| lerp(curve("SpawnAreaBox", lo, t_area), curve("SpawnAreaBox", hi, t_area), blend));
            let mut arc = [0.0f32, 360.0];
            d.vec("SpawnArc", &mut arc);
            let mut sp = [0.0f32; 3];
            d.vec("SpawnPosition", &mut sp);
            let spawn_pos = Float3::new(sp[0], sp[1], sp[2]);
            let (speed_lo, speed_hi) = d.range("SpawnSpeed", 0.0, 0.0);
            let (spread_lo, spread_hi) = d.range("SpawnSpread", 90.0, 90.0);
            let cone = d.b("SpreadMethod", true);
            let mut er = [0.0f32; 3];
            d.vec("EmitterRotation", &mut er);
            let emitter_rot = rotation_z(er[2] * PI / 180.0) * rotation_y(er[1] * PI / 180.0) * rotation_x(er[0] * PI / 180.0);
            let scale = inst.scale;

            // A spawn-mesh emitter of an effect placed with a volume (a building's VOLUMETRIC_EFFECT): over the box's surface, along its
            // normal (UseSurfaceNormal). ASSUMPTION: the engine emits from a mesh it makes of the volume, or from the collapsing building's own
            // meshes (not traced); without a volume these emitters spawn as their spawn area says (zero for the destruction dust: one point).
            let volume = inst.volume.filter(|_| d.b("AllowEmitFromMesh", false));
            let surface_normal = d.b("UseSurfaceNormal", false);

            let mut batch = 0u32;
            let mut latest_death = 0.0f32;
            for p in 0..count {
                if *free_slots == 0 {
                    break;
                }

                let spawn_t = window_start + window * (p + 1) as f32 / count as f32;
                let life = (lerp(life_lo, life_hi, game_rand(&mut inst.rng)) + inst.lifetime_bias).max(0.01);
                if spawn_t + life <= self.clock {
                    continue;
                }

                let angle = lerp(arc[0], arc[1], game_rand(&mut inst.rng)) * PI / 180.0;
                let r = lerp(inner, outer, game_rand(&mut inst.rng));
                let b = [game_rand(&mut self.global_rng), game_rand(&mut self.global_rng), game_rand(&mut self.global_rng)];
                let local = Float3::new(angle.cos() * r + size[0] * (b[2] - 0.5), size[1] * (b[1] - 0.5), angle.sin() * r + size[2] * (b[0] - 0.5)) * scale;
                let (offset, normal) = match volume {
                    Some(v) => {
                        let (at, n) = box_surface_point(v, [game_rand(&mut inst.rng), game_rand(&mut inst.rng), game_rand(&mut inst.rng)]);
                        (at + spawn_pos * scale, Some(n))
                    }

                    None => (rotate(&emitter_rot, local) + spawn_pos * scale, None),
                };

                let pos = inst.pos + rotate(&inst.rot, offset);
                let dir = if let Some(n) = normal.filter(|_| surface_normal) {
                    n
                } else if cone {
                    let az = game_rand(&mut inst.rng) * 2.0 * PI;
                    let el = lerp(spread_lo, spread_hi, game_rand(&mut inst.rng)) * PI / 180.0;
                    Float3::new(az.cos() * el.cos(), el.sin(), -(az.sin() * el.cos()))
                } else {
                    let away = offset - spawn_pos * scale;
                    if length(away) > 0.0 { normalize(away) } else { Float3::new(0.0, 1.0, 0.0) }
                };

                let speed = lerp(speed_lo, speed_hi, game_rand(&mut inst.rng)) * scale;
                // The box's points and normals are in the effect's frame: the emitter's rotation does not turn them.
                let vel = rotate(&inst.rot, if volume.is_some() { dir } else { rotate(&emitter_rot, dir) }) * speed;
                let steps = if spawn_t < self.clock - SUB_STEP { ((self.clock - spawn_t) / SUB_STEP).floor() } else { 0.0 };
                let seed = game_rand(&mut self.global_rng);
                out.push(SpawnVertex {
                    id_seed_steps: [(base + j) as f32, seed, steps, 0.0],
                    pos_time: [pos.x, pos.y, pos.z, spawn_t],
                    vel_life: [vel.x, vel.y, vel.z, life],
                });

                latest_death = latest_death.max(spawn_t + life);
                batch += 1;
                *free_slots -= 1;
            }

            if batch > 0 {
                inst.st.deaths.push_back((latest_death, batch));
            }
        }
    }

    /// The CPU step of a frame before the GPU passes: advances the clock (dt capped at 0.1 s) and updates the alive estimate (particles
    /// whose batch has not fully expired: an upper bound, dead slots are skipped by the VS). Returns the dt used.
    pub fn advance(&mut self, dt: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.1);
        self.clock += dt;
        self.alive = 0;
        for st in self.state.iter_mut().chain(self.world.iter_mut().map(|inst| &mut inst.st)) {
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
        if !self.enabled || (self.effect.emitters.is_empty() && self.world.is_empty()) {
            return;
        }

        let Some(sh) = self.shaders(lib, read, cbs) else { return };
        let dt = self.advance(f.dt);

        // The emitter instances (g_emitter_instances, 6 layers).
        let per_layer = EMITTER_WIDTH * self.emitter_rows;
        let mut layers = vec![0.0f32; EMITTER_LAYERS * per_layer * 4];
        let at = |layer: usize, e: usize| (layer * per_layer + e) * 4;
        for i in 0..self.effect.emitters.len().min(self.state.len()) {
            let a = &self.effect.emitters[i];
            let st = &mut self.state[i];
            let pos = if a.camera_following { f.eye } else { Float3::default() } + Float3::new(a.pos[0], a.pos[1], a.pos[2]);
            let delta = if st.started { pos - st.last_pos } else { Float3::default() };
            st.last_pos = pos;
            layers[at(0, i)..at(0, i) + 4].copy_from_slice(&[pos.x, pos.y, pos.z, self.clock]);
            layers[at(1, i)..at(1, i) + 4].copy_from_slice(&[delta.x, delta.y, delta.z, dt]);

            // The weather's tint: the middle of the reference's range (the game draws one blend per reference, fn 0x117f1860; every weather
            // reference has equal bounds).
            for c in 0..4 {
                layers[at(2, i) + c] = 0.5 * (a.tint_min[c] + a.tint_max[c]);
            }

            layers[at(3, i)] = i as f32;
            layers[at(3, i) + 1] = a.scale;

            // The rotation quaternion, and the kill plane: never.
            layers[at(4, i) + 3] = 1.0;
            layers[at(5, i) + 3] = 1.0;
        }

        // The placed effects' emitters follow the weather's.
        let base = self.effect.emitters.len();
        for j in 0..self.world.len() {
            let row = self.world_row(&self.world[j]);
            let i = base + j;
            if i >= per_layer || row >= MAX_EMITTERS {
                continue;
            }

            let inst = &mut self.world[j];
            let delta = if inst.st.started { inst.pos - inst.st.last_pos } else { Float3::default() };
            inst.st.last_pos = inst.pos;
            layers[at(0, i)..at(0, i) + 4].copy_from_slice(&[inst.pos.x, inst.pos.y, inst.pos.z, self.clock]);
            layers[at(1, i)..at(1, i) + 4].copy_from_slice(&[delta.x, delta.y, delta.z, dt]);
            layers[at(2, i)..at(2, i) + 4].copy_from_slice(&inst.tint);
            layers[at(3, i)] = row as f32;
            layers[at(3, i) + 1] = inst.scale;
            layers[at(4, i)..at(4, i) + 4].copy_from_slice(&inst.quat);
            layers[at(5, i) + 3] = 1.0;
        }

        if let Some(tex) = &self.emitter_tex {
            for l in 0..EMITTER_LAYERS {
                let _ = self.ctx.update_subresource(tex, l as u32, None, bytemuck::cast_slice(&layers[at(l, 0)..at(l + 1, 0)]), (EMITTER_WIDTH * 16) as u32, (per_layer * 16) as u32);
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
        self.ctx.set_blend_state(self.bs_opaque.as_ref());
        self.ctx.set_depth_stencil_state(self.ds_off.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.rs_none.as_ref());
        self.ctx.gs_set_shader(None);
        if self.clear {
            for p in &self.particles {
                let clears = [[-1.0, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4]];
                for (rtv, colour) in p.rtv.iter().zip(clears) {
                    if let Some(rtv) = rtv {
                        self.ctx.clear_render_target_view(rtv, colour);
                    }
                }
            }

            self.clear = false;
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
                    self.ctx.clear_render_target_view(&rtv, neutral);
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
            self.ctx.set_blend_state(None);
        }
    }

    fn create_distortion_target(&mut self, w: u32, h: u32) {
        self.dist_rtv = None;
        self.dist_srv = None;
        let td = D3D11_TEXTURE2D_DESC {
            Width: w, Height: h, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        if let Ok(tex) = self.dev.create_texture_2d(&td, None) {
            self.dist_rtv = self.dev.create_render_target_view(&tex, None).ok();
            self.dist_srv = self.dev.create_shader_resource_view(&tex, None).ok();
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

            self.spawn_vb = self.dev.create_buffer(&desc, None).ok();
        }

        let Some(vb) = self.spawn_vb.clone() else { return };
        let elements = [0u32, 16, 32].map(|offset| InputElement::new(c"COLOR", offset / 16, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, offset, false));

        let Ok(layout) = lib.layout(&sh.spawn_vs, "vfx_spawn", &elements) else { return };
        let written = match self.ctx.map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0) {
            Ok(mut ms) => ms.write(0, verts),
            Err(_) => false,
        };

        if !written {
            return;
        }

        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_vertex_buffers(0, &[Some(vb)], &[size_of::<SpawnVertex>() as u32], &[0]);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);
        bind_shader(&self.ctx, &sh.spawn_vs, cbs);
        bind_srv(&self.ctx, &sh.spawn_vs, "g_tex_sorted_data", self.keys[self.sorted].srv.as_ref());
        bind_shader(&self.ctx, &sh.spawn_ps, cbs);
        self.bind_emitter_cbs(&sh.spawn_ps, true);
        bind_srv(&self.ctx, &sh.spawn_ps, "g_emitter_instances", self.emitter_srv.as_ref());
        bind_srv(&self.ctx, &sh.spawn_ps, "g_tex_sea_surface_anim", self.sea_srv.as_ref());
        bind_srv(&self.ctx, &sh.spawn_ps, "g_tex_vector_field", self.vector_field_srv.as_ref());
        bind_sampler(&self.ctx, &sh.spawn_ps, "g_sam_terrain_surface", self.s_linear_clamp.as_ref());
        bind_sampler(&self.ctx, &sh.spawn_ps, "g_sam_vector_field", self.s_linear_wrap.as_ref());
        self.ctx.set_render_targets(&self.particles[self.cur].rtv, None);
        self.ctx.set_viewports(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: WIDTH as f32, Height: WIDTH as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
        self.ctx.draw(verts.len() as u32, 0);
        self.unbind();
        self.ctx.set_vertex_buffers(0, &[None], &[0], &[0]);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, f: &Frame, sh: &Shaders, pixel: &GameShader, rtv: &ID3D11RenderTargetView, view_id: u32, blend: Option<ID3D11BlendState>, cbs: &mut ConstantBuffers) {
        cbs.set_i("cb_vfx", "g_view_id", view_id as i32);
        let _ = cbs.upload(&self.dev, &self.ctx);
        self.ctx.set_render_targets(&[Some(rtv.clone())], f.depth.as_ref());
        self.ctx.set_viewports(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: f.width as f32, Height: f.height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
        self.ctx.set_blend_state(blend.as_ref());
        self.ctx.set_depth_stencil_state(if f.depth.is_some() { self.ds_test.as_ref() } else { self.ds_off.as_ref() }, 0);
        self.ctx.set_input_layout(None);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_POINTLIST);

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
        self.ctx.draw(self.alive + 1, 0);
        self.ctx.gs_set_shader(None);

        self.unbind();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter(props: &str) -> wce_assets::vfx_def::VfxEmitterDef {
        let xml = format!(r#"<vfx><emitters><emitter type="burst_rigid" id="e">{props}</emitter></emitters></vfx>"#);
        let doc = wce_formats::xml_doc::XmlDoc::parse(xml.into_bytes()).unwrap();
        wce_assets::vfx_def::parse_vfx(&doc).emitters.remove(0)
    }

    #[test]
    fn lightning_flashes_in_bursts() {
        // library_vfx_weather_rain's lightning_point_light.
        let d = emitter(r#"<ranged_property name="Bursts" min="1" max="1"/><property name="InfiniteBursts" value="true"/>
            <ranged_property name="BurstInterval" min="30" max="60"/><ranged_property name="BurstIntervalLength" min="0.2" max="0.4"/>
            <ranged_property name="RigidLifeTime" min="0.1" max="0.15"/><ranged_property name="RigidsPerBurst" min="1" max="3"/>"#);
        let mut b = RigidBursts { rng: 7, ..Default::default() };
        let (mut lit, mut flashes, mut was_lit) = (0, 0, false);
        let mut t = 0.0;
        while t < 600.0 {
            b.update(&d, t, 0.0);
            let now_lit = b.rigids.iter().any(|&(birth, life)| birth <= t && t < birth + life);
            lit += i32::from(now_lit);
            flashes += i32::from(now_lit && !was_lit);
            was_lit = now_lit;
            t += 0.01;
        }

        // About one burst every 45 s for 600 s, each lit for a fraction of a second.
        assert!((10..=25).contains(&flashes), "{flashes} flashes");
        assert!(lit < 600, "lit {lit} hundredths");
    }

    #[test]
    fn fire_light_stays_on() {
        // library_vfx_fire_smoke_loop's fire_point_light.
        let d = emitter(r#"<ranged_property name="Bursts" min="1" max="1"/><property name="InfiniteBursts" value="true"/>
            <ranged_property name="BurstInterval" min="64" max="64"/><ranged_property name="BurstIntervalLength" min="0" max="0"/>
            <ranged_property name="RigidLifeTime" min="64" max="64"/><ranged_property name="RigidsPerBurst" min="1" max="1"/>"#);
        let mut b = RigidBursts::default();
        let mut t = 0.0;
        while t < 300.0 {
            b.update(&d, t, 0.0);
            assert_eq!(b.rigids.iter().filter(|&&(birth, life)| birth <= t && t < birth + life).count(), 1, "at {t}");
            t += 0.25;
        }
    }
}
