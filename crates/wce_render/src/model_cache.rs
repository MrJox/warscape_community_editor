//! GPU-side resources the renderer builds from scene payloads: textures by VFS path, rigid models by VFS path, terrain meshes from
//! height grids. Entities name their assets; these caches resolve them (an asset that fails to load is reported once and skipped).
//!
//! Loading is thread safe and can run in the background: [`ModelCache::try_get`] queues a load on the io pool (priority = how close
//! the model is) and gives nothing until it is ready; [`ModelCache::model`] blocks (fixed-step runs, tools that need the bounds now).
//! D3D11 creates buffers and textures from any thread, so the whole job (read, decode, buffers, textures) runs in the worker.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, RwLock};

use rayon::prelude::*;
use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::materials::{MaterialType, TextureType};
use rpfm_lib::files::rigidmodel::vertices::VertexFormat;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_d3d11::*;
use wce_foundation::jobs::{Handle, JobPool, io_pool};
use wce_foundation::log::{self, LogLevel};
use wce_foundation::math::{Float3, Float4x4, normalize};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, Payload, RenderScene, Terrain};

use crate::texture::{Texture, load_dds, load_dds_tail, make_rgba_texture, make_solid_texture, plan_dds_stream};

/// The bytes of an in-pack file, from any thread.
pub type SharedReader = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// A texture the cache may replace later (the full mips of a streamed texture arrive in the background).
pub type SharedTexture = Arc<RwLock<Texture>>;

/// The device, shared with the loading threads.
///
/// D3D11 devices are free-threaded (unless created single-threaded, which the editor never does), and so are the device children
/// (buffers, textures, views), whose reference counts are atomic. Only the immediate context is bound to one thread, and it is never
/// shared through these types.
#[derive(Clone, Debug)]
pub struct SharedDevice(pub ID3D11Device);

impl SharedDevice {

    /// The device. Closures must reach it through this method: a closure that names the field captures the bare device, not this wrapper.
    pub fn device(&self) -> &ID3D11Device {
        &self.0
    }
}

// SAFETY: see the type's documentation.
unsafe impl Send for SharedDevice {}
// SAFETY: see the type's documentation.
unsafe impl Sync for SharedDevice {}
// SAFETY: a texture is a free-threaded view and plain data.
unsafe impl Send for Texture {}
// SAFETY: a texture is a free-threaded view and plain data.
unsafe impl Sync for Texture {}

/// The texture slots of the rigid materials, in the order of [`GpuMesh::tex`]: the five of the standard material, then the tiled dirt map's
/// two, then the ambient map's one.
pub const TEX_DIFFUSE: usize = 0;
pub const TEX_SPECULAR: usize = 1;
pub const TEX_GLOSS: usize = 2;
pub const TEX_COLOUR_MASK: usize = 3;
pub const TEX_NORMAL: usize = 4;
pub const TEX_DETAIL: usize = 5;
pub const TEX_ALPHA_MASK: usize = 6;
pub const TEX_AMBIENT: usize = 7;
pub const TEX_SLOTS: usize = 8;

const TEX_NAMES: [&str; TEX_SLOTS] =
    ["s_diffuse_map", "s_specular_colour_map", "s_gloss_map", "s_colour_mask_map", "s_normal_map", "s_detail_map", "s_alpha_mask", "s_ambient_map"];

impl MeshKind {
    pub fn is_tree(self) -> bool {
        matches!(self, Self::TreeBranch | Self::TreeLeaf | Self::TreeBillboard)
    }
}

/// The Grass vertex format: position (f16 x 4), uv (f32 x 2), normal, tangent and colour (u8 x 4 each; the colour's alpha is how much the
/// vertex sways and sinks with distance: 0 at the roots).
pub const GRASS_STRIDE: u32 = 28;

/// The Tree vertex format: position, LOD position, normal, tangent, bitangent (f16 x 4 each), uv, LOD weights, an unused pair (f16 x 2
/// each), colour and wind weight (f16 x 4). rigidbranchcommon_v3 / rigidleafcommon_v3 draw weight.x * position + weight.y * LOD position.
pub const TREE_STRIDE: u32 = 60;

/// The Billboard vertex format: the quad's corner (f16 x 4: x across, y up, in metres) and uv (f16 x 2).
pub const BILLBOARD_STRIDE: u32 = 12;

/// The Uk8 vertex format of water meshes: position (f16 x 4) and uv (f16 x 2).
pub const WATER_STRIDE: u32 = 12;

/// Which of the game's rigid shaders draw a mesh (its material type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MeshKind {
    /// DefaultMaterial: rigidstandard.
    #[default]
    Standard,

    /// TiledDirtmap: rigidtileddirtmapmesh, rigidstandard plus a tiled dirt map (`s_detail_map` at uv * `g_uv2_tile_interval`) faded by an
    /// alpha mask (`s_alpha_mask` at the second uv).
    TiledDirtmap,

    /// TerrainBlend (a building's ground skirt): rigidterrainblend.
    TerrainBlend,

    /// Water (vertex format Uk8, 12 bytes: f16 position, f16 uv): the water planes of lakes and the water of fountains and pools. Drawn by
    /// the water pass with the sea plane's waterplane shaders (they read only the position: the ripples follow world x / z), not as a mesh.
    Water,

    /// Dirtmap (statues, desert rocks): rigidwithdirtmesh, rigidstandard plus a dirt map (`s_decal_dirt_map`, its xy a normal offset and w
    /// the dirt, at uv * `g_uv2_tile_interval` + the instance's POSITION5.yz) faded by `s_decal_dirt_mask` at the uv: darkens the colour
    /// towards (0.03, 0.025, 0.02), dulls the specular and bends the normal.
    Dirtmap,

    /// ShipAmbientmap (many buildings, despite the name): rigidambientmapmesh, rigidstandard with the diffuse multiplied by a baked ambient
    /// occlusion map (`s_ambient_map`) at the second uv.
    AmbientMap,

    /// Tree / TreeLeaf meshes (vertex format Tree, [`TREE_STRIDE`] bytes): rigidbranch_v3 / rigidleaf_v3, with their own vertex shaders.
    TreeBranch,
    TreeLeaf,

    /// Grass (vertex format Grass, [`GRASS_STRIDE`] bytes): rigidgrass, drawn by the grass pass from the scene's grass entities.
    Grass,

    /// TreeBillboardMaterial (vertex format Billboard, [`BILLBOARD_STRIDE`] bytes): a tree's farthest LOD, a quad turned to the camera
    /// showing one of four views (rigidtreebillboard); its diffuse in the diffuse slot, the normal and displacement map in the normal slot.
    TreeBillboard,
}

/// The shader name of a texture slot (`s_diffuse_map`...); empty for other slots.
pub fn gpu_texture_name(slot: usize) -> &'static str {
    TEX_NAMES.get(slot).copied().unwrap_or_default()
}

/// The slot of a material texture's shader name.
pub fn gpu_texture_slot(name: &str) -> Option<usize> {
    TEX_NAMES.iter().position(|n| *n == name)
}

/// A mesh on the GPU.
#[derive(Clone, Debug)]
pub struct GpuMesh {
    pub vb: Option<ID3D11Buffer>,
    pub ib: Option<ID3D11Buffer>,
    pub index_count: u32,
    pub alpha_mode: i32,

    /// Bytes per vertex: 32 (the default static layout), [`TREE_STRIDE`] for trees.
    pub stride: u32,

    pub kind: MeshKind,

    /// The tiled dirt map's repeats per uv unit (the material's floats 0 and 1, `rigid_config_PS.g_uv2_tile_interval`); TiledDirtmap only.
    pub uv2_tile: [f32; 2],

    /// The mesh-local origin: a model mesh's pivot (its material's `v_pivot`: the vertices are stored around it), a terrain patch's corner
    /// (half-float positions stay small).
    pub offset: Float3,
    pub has_offset: bool,
    pub tex: [SharedTexture; TEX_SLOTS],

    /// The VFS path the diffuse texture was loaded from (empty for none): the grass pass derives the seasonal variants from it.
    pub diffuse_path: String,

    /// The bounds of the vertices (model space, before `offset`).
    pub bmin: Float3,
    pub bmax: Float3,
}

#[derive(Clone, Debug, Default)]
pub struct GpuLod {
    pub meshes: Vec<GpuMesh>,

    /// Used up to this camera distance.
    pub distance: f32,
}

#[derive(Clone, Debug, Default)]
pub struct GpuModel {
    pub path: String,

    /// LOD 0.
    pub meshes: Vec<GpuMesh>,

    /// LOD 0 is used up to this distance (0: always).
    pub distance: f32,

    /// The coarser LODs 1.. (models always have at least one LOD; terrain patches have several index buffers on one vertex buffer).
    pub lods: Vec<GpuLod>,

    /// Of the LOD 0 meshes, with their offsets.
    pub bmin: Float3,
    pub bmax: Float3,

    /// The projected decals of LOD 0 (ProjectedDecal* meshes): drawn by the decal pass, not as meshes.
    pub decals: Vec<ModelDecal>,
}

/// A projected decal mesh of a model: an 8 corner box (ProjectedDecalV3 boxes are turned about y) and the decal's textures by name.
#[derive(Clone, Debug, Default)]
pub struct ModelDecal {
    /// `<texture>_diffuse.dds`, `_normal.dds` / `_parallax.dds` (the material's name, `rigidmodels/buildings/textures/rubble_1`).
    pub texture: String,

    /// Model space: the unit box (-0.5 .. 0.5) onto the decal's box; its axes are the box's edges (columns), y the projection axis.
    pub matrix: Float4x4,
}

/// The box of a decal mesh from its corners. Building models order them 0..3 the top face (0 -> 3 along x, 0 -> 1 along z) and 4..7 the
/// same corners at the bottom (all 176 decal meshes of rigidmodels/buildings/); other orders (the standalone decal models count the corners
/// in binary) give the corners' axis-aligned bounds.
fn decal_box(corners: &[[f32; 3]]) -> Option<Float4x4> {
    if corners.len() != 8 {
        return None;
    }

    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut axes = [sub(corners[3], corners[0]), sub(corners[0], corners[4]), sub(corners[1], corners[0])];
    let len = axes.map(|v| dot(v, v).sqrt());
    let square = (0..3).all(|i| (i + 1..3).all(|j| dot(axes[i], axes[j]).abs() <= 1e-2 * len[i] * len[j]));
    if !square || axes[1][1] <= 0.0 || len.contains(&0.0) {
        let (lo, hi) = corners.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), c| (std::array::from_fn(|k| lo[k].min(c[k])), std::array::from_fn(|k| hi[k].max(c[k]))));
        axes = [[hi[0] - lo[0], 0.0, 0.0], [0.0, hi[1] - lo[1], 0.0], [0.0, 0.0, hi[2] - lo[2]]];
    }

    let mut m = Float4x4::IDENTITY;
    for r in 0..3 {
        let c = corners.iter().map(|v| v[r]).sum::<f32>() / 8.0;
        m.m[r] = [axes[0][r], axes[1][r], axes[2][r], c];
    }

    Some(m)
}

// SAFETY: free-threaded buffers and shared textures, and plain data.
unsafe impl Send for GpuMesh {}
// SAFETY: free-threaded buffers and shared textures, and plain data.
unsafe impl Sync for GpuMesh {}

/// Per-instance vertex data, POSITION1..7 of rigidcommon.hlsl vs30_main.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RigidInstance {
    /// POSITION1..3: the rows of the 3x4 world matrix.
    pub world: [[f32; 4]; 3],

    /// POSITION4 -> PS v6 (.y also -> v9.w).
    pub p4: [f32; 4],

    /// POSITION5 -> PS v7 (.w > 0: snow layer).
    pub p5: [f32; 4],

    /// POSITION6 -> PS v8 (.y: damage brighten, .z: desaturate).
    pub p6: [f32; 4],

    /// POSITION7 -> PS v11 (faction colour override, .w = which colour).
    pub p7: [f32; 4],
}

/// An immutable buffer of some bytes.
fn immutable_buffer(dev: &ID3D11Device, bytes: &[u8], bind: D3D11_BIND_FLAG) -> Option<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC { ByteWidth: bytes.len() as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: bind as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
    dev.create_buffer(&desc, Some(bytes)).ok()
}

// ------------------------------------------------------------------------------------------------ textures

struct TextureInner {
    dev: SharedDevice,
    read: SharedReader,
    cache: Mutex<HashMap<String, Arc<OnceLock<Option<SharedTexture>>>>>,
    errors: Mutex<Vec<String>>,

    /// What went to the log already: each problem is logged once.
    logged: Mutex<HashSet<String>>,
    loaded: AtomicUsize,
    streaming: AtomicBool,
    upgrades: Mutex<Vec<(SharedTexture, Texture)>>,
    upgrade_jobs: Mutex<Vec<Handle>>,
    upgrades_inflight: AtomicUsize,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl TextureInner {
    fn error(&self, level: LogLevel, message: String) {
        if lock(&self.logged).insert(message.clone()) {
            log::add(level, format!("texture {message}"));
        }

        let mut errors = lock(&self.errors);
        if errors.len() < 50 {
            errors.push(message);
        }
    }
}

/// Textures by VFS path.
pub struct TextureCache {
    inner: Arc<TextureInner>,
    white: SharedTexture,
    black: SharedTexture,
    flat_normal: SharedTexture,
}

impl TextureCache {

    pub fn new(dev: &ID3D11Device, read: SharedReader) -> Self {
        let solid = |rgba| Arc::new(RwLock::new(make_solid_texture(dev, rgba)));
        Self {
            inner: Arc::new(TextureInner {
                dev: SharedDevice(dev.clone()),
                read,
                cache: Mutex::new(HashMap::new()),
                errors: Mutex::new(vec![]),
                logged: Mutex::new(HashSet::new()),
                loaded: AtomicUsize::new(0),
                streaming: AtomicBool::new(false),
                upgrades: Mutex::new(vec![]),
                upgrade_jobs: Mutex::new(vec![]),
                upgrades_inflight: AtomicUsize::new(0),
            }),
            white: solid(0xFFFF_FFFF),
            black: solid(0),

            // DXT5nm style: x in .a, y in .g, both 0.5 is a normal pointing straight up (z is rebuilt).
            flat_normal: solid(0x7FFF_7F7F),
        }
    }

    /// A texture (thread safe; two threads asking for the same file wait for one load); `None` if missing or unreadable.
    ///
    /// While streaming is on, a large DDS file first loads its small mips only; the full texture follows in the background and replaces
    /// the view at the next [`Self::pump`].
    pub fn get(&self, path: &str, srgb: bool) -> Option<SharedTexture> {
        self.get_or_load(path, srgb, false)
    }

    /// [`Self::get`] for a caller with somewhere else to look: a file that isn't there is not logged (the caller reports it with
    /// [`Self::report_missing`] when nothing it tried was there). Files that are there and don't load still are.
    pub fn get_quiet(&self, path: &str, srgb: bool) -> Option<SharedTexture> {
        self.get_or_load(path, srgb, true)
    }

    /// Logs (once per texture) that a texture something needs is in no pack, and what is drawn instead.
    pub fn report_missing(&self, path: &str, needed_by: &str, instead: &str) {
        let path = wce_vfs::normalize(path);
        if lock(&self.inner.logged).insert(path.clone()) {
            log::warn(format!("texture {path}: in no pack (needed by {needed_by}, drawn with {instead})"));
        }
    }

    fn get_or_load(&self, path: &str, srgb: bool, quiet: bool) -> Option<SharedTexture> {
        let key = format!("{}|{}", wce_vfs::normalize(path), if srgb { "s" } else { "l" });
        let slot = lock(&self.inner.cache).entry(key).or_default().clone();
        slot.get_or_init(|| self.load(path, srgb, quiet)).clone()
    }

    fn load(&self, path: &str, srgb: bool, quiet: bool) -> Option<SharedTexture> {
        wce_prof!("texture.load");
        let inner = &self.inner;
        let bytes = match (inner.read)(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if !quiet {
                    inner.error(LogLevel::Warning, format!("{path}: {error}"));
                }

                return None;
            }
        };

        // Stage one: only the small mips, from the tail of the file. When they don't load, the whole file is tried below.
        let header = &bytes[..bytes.len().min(148)];
        let plan = plan_dds_stream(header, bytes.len(), 256);
        let first = (inner.streaming.load(Ordering::Relaxed) && plan.ok)
            .then(|| load_dds_tail(inner.dev.device(), header, &bytes[plan.tail_offset..], &plan, srgb))
            .and_then(|tail| tail.map_err(|error| inner.error(LogLevel::Warning, format!("{path}: streamed mips: {error} (loaded whole)"))).ok());
        if let Some(first) = first {
            let first = Arc::new(RwLock::new(first));
            inner.loaded.fetch_add(1, Ordering::Relaxed);
            inner.upgrades_inflight.fetch_add(1, Ordering::AcqRel);
            let (job_inner, target, path) = (inner.clone(), first.clone(), path.to_owned());

            // After every model load that is queued.
            let handle = io_pool().post(move || {
                wce_prof!("texture.upgrade");
                match load_dds(job_inner.dev.device(), &bytes, srgb) {
                    Ok(full) => lock(&job_inner.upgrades).push((target, full)),
                    Err(error) => job_inner.error(LogLevel::Error, format!("{path}: {error} (stays at its small mips)")),
                }

                job_inner.upgrades_inflight.fetch_sub(1, Ordering::AcqRel);
            }, JobPool::current_priority() + 1.0e6);

            lock(&inner.upgrade_jobs).push(handle);
            return Some(first);
        }

        wce_prof!("texture.create");
        match load_dds(inner.dev.device(), &bytes, srgb) {
            Ok(texture) => {
                inner.loaded.fetch_add(1, Ordering::Relaxed);
                Some(Arc::new(RwLock::new(texture)))
            }

            Err(error) => {
                inner.error(LogLevel::Error, format!("{path}: {error}"));
                None
            }
        }
    }

    pub fn set_streaming(&self, on: bool) {
        self.inner.streaming.store(on, Ordering::Relaxed);
    }

    /// Swaps in the full textures that arrived. Call it on the thread that draws, once per frame.
    pub fn pump(&self) {
        let ready = std::mem::take(&mut *lock(&self.inner.upgrades));
        for (target, full) in ready {
            let mut target = target.write().unwrap_or_else(|poisoned| poisoned.into_inner());
            target.srv = full.srv;
            target.width = full.width;
            target.height = full.height;
            target.mips = full.mips;
        }
    }

    pub fn upgrades_pending(&self) -> usize {
        self.inner.upgrades_inflight.load(Ordering::Acquire)
    }

    pub fn white(&self) -> SharedTexture {
        self.white.clone()
    }

    pub fn black(&self) -> SharedTexture {
        self.black.clone()
    }

    pub fn flat_normal(&self) -> SharedTexture {
        self.flat_normal.clone()
    }

    /// What a fallback texture is, in words.
    fn solid_name(&self, t: &SharedTexture) -> &'static str {
        if Arc::ptr_eq(t, &self.white) {
            "white"
        } else if Arc::ptr_eq(t, &self.black) {
            "black"
        } else if Arc::ptr_eq(t, &self.flat_normal) {
            "a flat normal"
        } else {
            "a fallback"
        }
    }

    pub fn errors(&self) -> Vec<String> {
        lock(&self.inner.errors).clone()
    }

    pub fn loaded(&self) -> usize {
        self.inner.loaded.load(Ordering::Relaxed)
    }
}

impl Drop for TextureCache {
    fn drop(&mut self) {
        for handle in lock(&self.inner.upgrade_jobs).drain(..) {
            if io_pool().cancel(handle) {
                self.inner.upgrades_inflight.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ models

/// The load state of a model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum State {
    #[default]
    Absent,
    Queued,
    Loading,
    Ready,
    Failed,
}

#[derive(Default)]
struct Entry {
    state: State,
    model: Option<Arc<GpuModel>>,
    handle: Handle,
}

struct ModelInner {
    dev: SharedDevice,
    read: SharedReader,
    textures: Arc<TextureCache>,
    models: Mutex<HashMap<String, Entry>>,
    done: Condvar,
    pending: AtomicUsize,
    revision: AtomicU64,
    errors: Mutex<Vec<String>>,
}

// SAFETY: the texture cache only holds free-threaded GPU objects behind locks.
unsafe impl Send for TextureCache {}
// SAFETY: the texture cache only holds free-threaded GPU objects behind locks.
unsafe impl Sync for TextureCache {}

/// The rigid models of the packs, and the terrain meshes of the scene.
pub struct ModelCache {
    inner: Arc<ModelInner>,
    terrain: BTreeMap<EntityId, TerrainGpu>,
}

/// The texture a material gives a slot, or a fallback.
/// The path a material texture loads from: where the material says, else under the same name next to the model.
/// The engine's stand-ins for the art tools' placeholder textures (the material resolver fn 0x1186c880, first match wins, `\name` or
/// `/name` anywhere in the path): test_gray -> default_grey, test_black / test_mask / test_gloss -> default_black, test_white ->
/// default_white (with `_srgb` for a colour slot), flatnormal -> default_normal. The placeholders' own files are not what the game draws
/// (flatnormal.dds is an RGB normal, which the shaders' .wy decode turns sideways).
fn engine_default(path: &str, srgb: bool) -> Option<String> {
    let lower = path.to_lowercase();
    let has = |name: &str| lower.contains(&format!("/{name}")) || lower.contains(&format!("\\{name}"));
    let named = |base: &str| if srgb { format!("commontextures/{base}_srgb.dds") } else { format!("commontextures/{base}.dds") };
    if has("test_gray") {
        Some(named("default_grey"))
    } else if has("test_black") || has("test_mask") {
        Some(named("default_black"))
    } else if has("test_white") {
        Some(named("default_white"))
    } else if has("test_gloss") {
        Some(named("default_black"))
    } else if has("flatnormal") {
        Some("commontextures/default_normal.dds".to_owned())
    } else {
        None
    }
}

/// A material texture's path, the engine's stand-in for a placeholder (when it loads) taking its place.
fn material_path(textures: &TextureCache, material: &rpfm_lib::files::rigidmodel::materials::Material, kind: TextureType, srgb: bool) -> Option<String> {
    let path = material.textures().iter().find(|t| *t.tex_type() == kind).map(|t| t.path().replace('\\', "/"))?;
    Some(engine_default(&path, srgb).filter(|d| textures.get_quiet(d, srgb).is_some()).unwrap_or(path))
}

fn material_texture_path(textures: &TextureCache, model: &str, material: &rpfm_lib::files::rigidmodel::materials::Material, kind: TextureType) -> Option<String> {
    let path = material_path(textures, material, kind, true)?;
    if textures.get_quiet(&path, true).is_some() {
        return Some(path);
    }

    let (dir, name) = (model.rsplit_once('/')?.0, path.rsplit_once('/').map_or(path.as_str(), |(_, n)| n));
    let local = format!("{dir}/{name}");
    textures.get_quiet(&local, true).is_some().then_some(local)
}

/// A material texture; one that isn't where the material says is looked for under the same name next to the model (the `default` grass
/// models name the rome_atlantic folder's textures, which are only in their own folder).
fn material_texture(textures: &TextureCache, model: &str, material: &rpfm_lib::files::rigidmodel::materials::Material, kind: TextureType, srgb: bool, fallback: SharedTexture) -> SharedTexture {
    let Some(path) = material_path(textures, material, kind, srgb) else { return fallback };
    textures.get_quiet(&path, srgb)
        .or_else(|| {
            let (dir, name) = (model.rsplit_once('/')?.0, path.rsplit_once('/').map_or(path.as_str(), |(_, n)| n));
            textures.get_quiet(&format!("{dir}/{name}"), srgb)
        })
        .unwrap_or_else(|| {
            textures.report_missing(&path, &format!("{model}, {kind:?}"), textures.solid_name(&fallback));
            fallback
        })
}

impl ModelInner {

    /// Builds a model on any thread; `None` when it has no drawable standard meshes at LOD 0.
    fn build(&self, path: &str) -> Result<Option<GpuModel>, String> {
        wce_prof!("model.build");
        let bytes = {
            wce_prof!("model.read");
            (self.read)(path)?
        };

        let model = {
            wce_prof!("model.parse");
            let extra = DecodeableExtraData::default();
            RigidModel::decode(&mut Cursor::new(&bytes), &Some(extra)).map_err(|error| format!("rmv2: {error}"))?
        };

        // Version 5 (some grass) is read into the same vertices, written back as 6.
        if !matches!(*model.version(), 5 | 6) {
            return Err(format!("rmv2: version {} (expected 5 or 6)", model.version()));
        }

        if model.lods().is_empty() {
            return Err("no LODs".to_owned());
        }

        let textures = &self.textures;
        let make_mesh = |block: &rpfm_lib::files::rigidmodel::MeshBlock| -> Option<GpuMesh> {
            let (mesh, material) = (block.mesh(), block.material());
            let (kind, format) = match (mesh.material_type(), material.vertex_format()) {
                (MaterialType::DefaultMaterial, VertexFormat::Static) => (MeshKind::Standard, VertexFormat::Static),
                (MaterialType::TiledDirtmap, VertexFormat::Static) => (MeshKind::TiledDirtmap, VertexFormat::Static),
                (MaterialType::TerrainBlend, VertexFormat::Static) => (MeshKind::TerrainBlend, VertexFormat::Static),
                (MaterialType::Dirtmap, VertexFormat::Static) => (MeshKind::Dirtmap, VertexFormat::Static),
                (MaterialType::ShipAmbientmap, VertexFormat::Static) => (MeshKind::AmbientMap, VertexFormat::Static),
                (MaterialType::Tree, VertexFormat::Tree) => (MeshKind::TreeBranch, VertexFormat::Tree),
                (MaterialType::TreeLeaf, VertexFormat::Tree) => (MeshKind::TreeLeaf, VertexFormat::Tree),
                (MaterialType::TreeBillboardMaterial, VertexFormat::Billboard) => (MeshKind::TreeBillboard, VertexFormat::Billboard),
                (MaterialType::Grass, VertexFormat::Grass) => (MeshKind::Grass, VertexFormat::Grass),
                (MaterialType::Water, VertexFormat::Uk8) => (MeshKind::Water, VertexFormat::Uk8),
                _ => return None,
            };

            if mesh.vertices().is_empty() || mesh.indices().is_empty() {
                return None;
            }

            let pivot = Float3::new(material.v_pivot().x, material.v_pivot().y, material.v_pivot().z);

            // The static material types share the default layout; trees have theirs. Written back as rpfm_lib reads them: the file's bytes.
            let stride = match format {
                VertexFormat::Tree => TREE_STRIDE,
                VertexFormat::Billboard => BILLBOARD_STRIDE,
                VertexFormat::Grass => GRASS_STRIDE,
                VertexFormat::Uk8 => WATER_STRIDE,
                _ => 32,
            };

            let mut vertices = Vec::with_capacity(mesh.vertices().len() * stride as usize);
            for vertex in mesh.vertices() {
                let mtype = if format == VertexFormat::Static { MaterialType::DefaultMaterial } else { *mesh.material_type() };
                vertex.write(&mut vertices, 6, format, mtype).ok()?;
            }

            if vertices.len() != mesh.vertices().len() * stride as usize {
                return None;
            }

            let indices = mesh.indices().iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>();
            let vb = immutable_buffer(self.dev.device(), &vertices, D3D11_BIND_VERTEX_BUFFER)?;
            let ib = immutable_buffer(self.dev.device(), &indices, D3D11_BIND_INDEX_BUFFER)?;

            // The bounds come from the vertices: the file's are not trusted (a culling sphere that is too small drops visible geometry).
            // A tree vertex is drawn somewhere between its two positions (f16 at bytes 0 and 8): both count.
            let (mut lo, mut hi) = ([1e30f32; 3], [-1e30f32; 3]);
            let half = |v: &[u8], at: usize| half_to_float(u16::from_le_bytes([v[at], v[at + 1]]));
            let mut grow = |p: [f32; 3]| {
                for (a, v) in p.into_iter().enumerate() {
                    lo[a] = lo[a].min(v);
                    hi[a] = hi[a].max(v);
                }
            };

            if format == VertexFormat::Tree {
                for v in vertices.chunks_exact(stride as usize) {
                    grow([half(v, 0), half(v, 2), half(v, 4)]);
                    grow([half(v, 8), half(v, 10), half(v, 12)]);
                }
            } else if format == VertexFormat::Billboard {
                // The quad turns around the vertical axis: as far in z as in x.
                for v in vertices.chunks_exact(stride as usize) {
                    let (x, y) = (half(v, 0), half(v, 2));
                    grow([x, y, x]);
                    grow([-x, y, -x]);
                }
            } else {
                for vertex in mesh.vertices() {
                    let p = vertex.position();
                    grow([p.x, p.y, p.z]);
                }
            }

            // A billboard's normal and displacement map is named after its diffuse.
            let normal = if kind == MeshKind::TreeBillboard {
                material.textures().iter().find(|t| *t.tex_type() == TextureType::Diffuse)
                    .and_then(|t| t.path().strip_suffix("_billboard_diffuse.dds").map(|base| format!("{base}_billboard_normal_and_displacement.dds")))
                    .and_then(|normal| textures.get_quiet(&normal, false).or_else(|| {
                        textures.report_missing(&normal, &format!("{path}, billboard normal"), "a flat normal");
                        None
                    }))
                    .unwrap_or_else(|| textures.flat_normal())
            } else {
                material_texture(textures, path, material, TextureType::Normal, false, textures.flat_normal())
            };

            Some(GpuMesh {
                vb: Some(vb),
                ib: Some(ib),
                index_count: mesh.indices().len() as u32,
                alpha_mode: material.params_i32().iter().find(|(id, _)| *id == 0).map_or(0, |(_, v)| *v),
                stride,
                kind,
                uv2_tile: if matches!(kind, MeshKind::TiledDirtmap | MeshKind::Dirtmap) { [0, 1].map(|id| material.params_f32().iter().find(|(i, _)| *i == id).map_or(1.0, |(_, v)| *v)) } else { [0.0; 2] },
                offset: pivot,
                has_offset: pivot != Float3::default(),
                tex: [
                    material_texture(textures, path, material, TextureType::Diffuse, true, textures.white()),
                    material_texture(textures, path, material, TextureType::Specular, true, textures.black()),
                    material_texture(textures, path, material, TextureType::GlossMap, false, textures.black()),
                    material_texture(textures, path, material, TextureType::Mask, false, textures.black()),
                    normal,
                    // TEX_DETAIL / TEX_ALPHA_MASK: the tiled dirt map and its mask, or a Dirtmap mesh's dirt map (linear: a normal offset and
                    // the dirt amount) and dirt mask.
                    if kind == MeshKind::Dirtmap {
                        material_texture(textures, path, material, TextureType::DecalDirtmap, false, textures.black())
                    } else {
                        material_texture(textures, path, material, TextureType::TilingDirtUV2, true, textures.white())
                    },
                    if kind == MeshKind::Dirtmap {
                        material_texture(textures, path, material, TextureType::DecalDirtmask, false, textures.black())
                    } else {
                        material_texture(textures, path, material, TextureType::DirtAlphaMask, false, textures.white())
                    },
                    material_texture(textures, path, material, TextureType::AmbientOcclusion, false, textures.white()),
                ],
                diffuse_path: material_texture_path(textures, path, material, TextureType::Diffuse).unwrap_or_default(),
                bmin: Float3::new(lo[0], lo[1], lo[2]),
                bmax: Float3::new(hi[0], hi[1], hi[2]),
            })
        };

        let mut gm = GpuModel {
            path: path.to_owned(),
            bmin: Float3::new(1e30, 1e30, 1e30),
            bmax: Float3::new(-1e30, -1e30, -1e30),
            ..Default::default()
        };

        // LOD 0's projected decals.
        if let Some(lod) = model.lods().first() {
            for block in lod.mesh_blocks() {
                let decal = matches!(
                    block.mesh().material_type(),
                    MaterialType::ProjectedDecal | MaterialType::ProjectedDecalV2 | MaterialType::ProjectedDecalV3 | MaterialType::ProjectedDecalV4
                );
                if !decal {
                    continue;
                }

                let corners: Vec<[f32; 3]> = block.mesh().vertices().iter().map(|v| { let p = v.position(); [p.x, p.y, p.z] }).collect();
                if let Some(matrix) = decal_box(&corners) {
                    gm.decals.push(ModelDecal { texture: block.material().name().to_owned(), matrix });
                }
            }
        }

        for (l, lod) in model.lods().iter().enumerate() {
            let meshes = lod.mesh_blocks().iter().filter_map(make_mesh).collect::<Vec<_>>();
            if meshes.is_empty() {
                // A model of decals only (its boxes give the bounds).
                if l == 0 && !gm.decals.is_empty() {
                    for d in &gm.decals {
                        let r = (0..3).map(|c| (0..3).map(|k| d.matrix.m[k][c] * d.matrix.m[k][c]).sum::<f32>().sqrt()).sum::<f32>() * 0.5;
                        let c = Float3::new(d.matrix.m[0][3], d.matrix.m[1][3], d.matrix.m[2][3]);
                        gm.bmin = Float3::new(gm.bmin.x.min(c.x - r), gm.bmin.y.min(c.y - r), gm.bmin.z.min(c.z - r));
                        gm.bmax = Float3::new(gm.bmax.x.max(c.x + r), gm.bmax.y.max(c.y + r), gm.bmax.z.max(c.z + r));
                    }

                    continue;
                }

                if l == 0 {
                    return Ok(None);
                }

                continue;
            }

            if l == 0 {
                for g in &meshes {
                    let (lo, hi) = (g.bmin + g.offset, g.bmax + g.offset);
                    gm.bmin = Float3::new(gm.bmin.x.min(lo.x), gm.bmin.y.min(lo.y), gm.bmin.z.min(lo.z));
                    gm.bmax = Float3::new(gm.bmax.x.max(hi.x), gm.bmax.y.max(hi.y), gm.bmax.z.max(hi.z));
                }

                gm.meshes = meshes;
                gm.distance = *lod.visibility_distance();
            } else {
                gm.lods.push(GpuLod { meshes, distance: *lod.visibility_distance() });
            }
        }

        Ok(Some(gm))
    }

    fn finish(&self, path: &str, model: Option<GpuModel>) {
        let mut models = lock(&self.models);
        let entry = models.entry(path.to_owned()).or_default();
        entry.state = if model.is_some() { State::Ready } else { State::Failed };
        entry.model = model.map(Arc::new);
        self.pending.fetch_sub(1, Ordering::AcqRel);
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.done.notify_all();
    }

    fn build_and_finish(&self, path: &str) {
        let model = match self.build(path) {
            Ok(model) => model,
            Err(error) => {
                log::error(format!("model {path}: {error}"));
                lock(&self.errors).push(format!("{path}: {error}"));
                None
            }
        };

        self.finish(path, model);
    }

    /// The load job.
    fn run(&self, path: &str) {
        {
            let mut models = lock(&self.models);
            let Some(entry) = models.get_mut(path) else { return };

            // A blocking model() call took it.
            if entry.state != State::Queued {
                return;
            }

            entry.state = State::Loading;
        }

        self.build_and_finish(path);
    }
}

impl ModelCache {

    pub fn new(dev: &ID3D11Device, read: SharedReader, textures: Arc<TextureCache>) -> Self {
        Self {
            inner: Arc::new(ModelInner {
                dev: SharedDevice(dev.clone()),
                read,
                textures,
                models: Mutex::new(HashMap::new()),
                done: Condvar::new(),
                pending: AtomicUsize::new(0),
                revision: AtomicU64::new(1),
                errors: Mutex::new(vec![]),
            }),
            terrain: BTreeMap::new(),
        }
    }

    pub fn textures(&self) -> &Arc<TextureCache> {
        &self.inner.textures
    }

    /// Blocking: the model (loading it now, or waiting for the load in flight); `None` when the file has no drawable standard meshes
    /// or can't be read.
    pub fn model(&self, path: &str) -> Option<Arc<GpuModel>> {
        let inner = &self.inner;
        let mut models = lock(&inner.models);
        loop {
            let entry = models.entry(path.to_owned()).or_default();
            match entry.state {
                State::Ready => return entry.model.clone(),
                State::Failed => return None,
                State::Loading => {
                    models = inner.done.wait(models).unwrap_or_else(|poisoned| poisoned.into_inner());
                }

                State::Absent | State::Queued => {
                    if entry.state == State::Queued {
                        io_pool().cancel(entry.handle);
                    } else {
                        inner.pending.fetch_add(1, Ordering::AcqRel);
                    }

                    entry.state = State::Loading;
                    drop(models);
                    inner.build_and_finish(path);
                    models = lock(&inner.models);
                }
            }
        }
    }

    /// Not blocking: the model when it is ready; otherwise queues the load (smaller priorities load first) and gives nothing.
    pub fn try_get(&self, path: &str, priority: f32) -> Option<Arc<GpuModel>> {
        let mut models = lock(&self.inner.models);
        let entry = models.entry(path.to_owned()).or_default();
        match entry.state {
            State::Ready => entry.model.clone(),
            State::Absent => {
                entry.state = State::Queued;
                self.inner.pending.fetch_add(1, Ordering::AcqRel);
                let (inner, job_path) = (self.inner.clone(), path.to_owned());
                entry.handle = io_pool().post(move || inner.run(&job_path), priority);
                None
            }

            State::Queued => {
                io_pool().set_priority(entry.handle, priority);
                None
            }

            State::Loading | State::Failed => None,
        }
    }

    pub fn state(&self, path: &str) -> State {
        lock(&self.inner.models).get(path).map_or(State::Absent, |entry| entry.state)
    }

    /// The model if it is ready, without queueing anything, and its state (Absent counts as still to come).
    pub fn peek(&self, path: &str) -> (Option<Arc<GpuModel>>, State) {
        lock(&self.inner.models).get(path).map_or((None, State::Absent), |entry| (entry.model.clone().filter(|_| entry.state == State::Ready), entry.state))
    }

    /// Re-ranks a queued load.
    pub fn set_priority(&self, path: &str, priority: f32) {
        if let Some(entry) = lock(&self.inner.models).get(path)
            && entry.state == State::Queued
        {
            io_pool().set_priority(entry.handle, priority);
        }
    }

    /// Queued and loading models.
    pub fn pending(&self) -> usize {
        self.inner.pending.load(Ordering::Acquire)
    }

    /// Changes whenever a load finishes.
    pub fn revision(&self) -> u64 {
        self.inner.revision.load(Ordering::Acquire)
    }

    /// Blocks until nothing is pending.
    pub fn wait_all(&self) {
        let mut models = lock(&self.inner.models);
        while self.inner.pending.load(Ordering::Acquire) != 0 {
            models = self.inner.done.wait(models).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub fn errors(&self) -> Vec<String> {
        lock(&self.inner.errors).clone()
    }

    /// The terrain patches of a terrain entity; the patches whose heights changed are rebuilt when the payload's version moves on.
    pub fn terrain(&mut self, id: EntityId, t: &Terrain) -> Option<&TerrainGpu> {
        wce_prof!("terrain.update");
        let layout = layout_hash(t);
        let valid = t.nx >= 2 && t.nz >= 2 && (t.heights.is_empty() || t.heights.len() == t.nx as usize * t.nz as usize) && t.step > 0.0;
        if self.terrain.get(&id).is_some_and(|slot| slot.layout == layout && slot.version == t.version) {
            return self.terrain.get(&id);
        }

        if !valid {
            self.terrain.remove(&id);
            return None;
        }

        let (px, pz) = ((t.nx - 1 + CELLS - 1) / CELLS, (t.nz - 1 + CELLS - 1) / CELLS);
        let same_layout = self.terrain.get(&id).is_some_and(|slot| slot.layout == layout && slot.patches_x == px && slot.patches_z == pz);
        if !same_layout {
            let count = (px * pz) as usize;
            self.terrain.insert(id, TerrainGpu { patches_x: px, patches_z: pz, patches: (0..count).map(|_| Arc::new(GpuModel::default())).collect(), skins: vec![None; count], ..Default::default() });
        }

        let diffuse = self.inner.textures.get(&t.diffuse, true);
        let textures = self.inner.textures.clone();
        let dev = self.inner.dev.clone();
        let tg = self.terrain.get_mut(&id)?;
        if tg.skins.len() != tg.patches.len() {
            tg.skins.resize(tg.patches.len(), None);
        }

        // The patches that changed: all on a new layout, otherwise the ones whose heights (plus the margin the normals read) differ.
        let dirty = (0..tg.patches.len()).into_par_iter().map(|p| {
            if !same_layout || t.heights.is_empty() || tg.heights.len() != t.heights.len() {
                return true;
            }

            let (pi, pj) = ((p % px as usize) as i32, (p / px as usize) as i32);
            let (i0, i1) = ((pi * CELLS - 1).max(0) as usize, ((pi + 1) * CELLS + 1).min(t.nx - 1) as usize);
            let (j0, j1) = ((pj * CELLS - 1).max(0) as usize, ((pj + 1) * CELLS + 1).min(t.nz - 1) as usize);
            let nx = t.nx as usize;
            (j0..=j1).any(|j| t.heights[j * nx + i0..=j * nx + i1].iter().zip(&tg.heights[j * nx + i0..=j * nx + i1]).any(|(a, b)| a.to_bits() != b.to_bits()))
        }).collect::<Vec<_>>();

        let built = (0..tg.patches.len()).into_par_iter().filter(|&p| dirty[p]).map(|p| {
            let (pi, pj) = ((p % px as usize) as i32, (p / px as usize) as i32);
            (p, build_patch(dev.device(), t, diffuse.clone(), &textures, pi, pj))
        }).collect::<Vec<_>>();

        let mut failed = false;
        for (p, result) in built {
            match result {
                Some((model, skin)) => {
                    tg.patches[p] = Arc::new(model);
                    tg.skins[p] = skin;
                }

                None => failed = true,
            }
        }

        tg.heights = t.heights.clone();
        tg.layout = layout;
        tg.version = t.version;
        if failed {
            log::error("terrain: a patch could not be created");
            lock(&self.inner.errors).push("terrain: a patch could not be created".to_owned());
        }

        self.terrain.get(&id)
    }

    /// Drops the terrain meshes of entities that no longer exist.
    pub fn prune_terrain(&mut self, scene: &RenderScene) {
        self.terrain.retain(|id, _| scene.find(*id).is_some_and(|e| matches!(e.payload, Payload::Terrain(_))));
    }
}

impl Drop for ModelCache {
    fn drop(&mut self) {
        let mut models = lock(&self.inner.models);
        for entry in models.values_mut().filter(|entry| entry.state == State::Queued) {
            if io_pool().cancel(entry.handle) {
                entry.state = State::Failed;
                self.inner.pending.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ terrain

/// Patch size in cells: local half-float positions and UVs stay small.
const CELLS: i32 = 64;

/// One terrain entity on the GPU: patches of 64 x 64 cells, each a model of its own (own bounds, LODs that share the vertex buffer),
/// so they are culled and detailed one by one.
#[derive(Clone, Debug, Default)]
pub struct TerrainGpu {
    pub patches: Vec<Arc<GpuModel>>,

    /// The patches' pieces of the payload's skin (the meshes use them).
    pub skins: Vec<Option<SharedTexture>>,

    // What the patches were built from.
    pub layout: u64,
    pub version: u64,
    pub patches_x: i32,
    pub patches_z: i32,

    /// A copy of the payload's heights, to find the patches a new payload changed.
    pub heights: Vec<f32>,
}

fn mix64(h: u64, v: u64) -> u64 {
    h ^ (v.wrapping_add(0x9E37_79B9_7F4A_7C15).wrapping_add(h << 6).wrapping_add(h >> 2))
}

fn layout_hash(t: &Terrain) -> u64 {
    let mut h = t.diffuse.bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3));
    h = mix64(h, (t.nx as u64).wrapping_mul(65537).wrapping_add(t.nz as u64));
    for f in [t.origin_x, t.origin_z, t.step, t.tile_metres] {
        h = mix64(h, u64::from(f.to_bits()));
    }

    h = mix64(h, t.heights.len() as u64);
    h = mix64(h, u64::from(t.lod));
    mix64(h, t.skin.as_ref().map_or(0, |skin| skin.id + 1))
}

/// A half float as a float (exact).
pub fn half_to_float(h: u16) -> f32 {
    let (s, e, m) = (u32::from(h >> 15) & 1, u32::from(h >> 10) & 31, u32::from(h) & 1023);
    let bits = match e {
        0 if m == 0 => s << 31,
        0 => return if s != 0 { -1.0 } else { 1.0 } * m as f32 * (1.0 / 16_777_216.0),
        31 => (s << 31) | 0x7F80_0000 | (m << 13),
        _ => (s << 31) | ((e + 112) << 23) | (m << 13),
    };

    f32::from_bits(bits)
}

/// A float as a half float, truncating, flushing what would be subnormal to zero, as the C++ terrain mesh did.
fn to_half(f: f32) -> u16 {
    let x = f.to_bits();
    let (sign, exp, man) = (((x >> 16) & 0x8000) as u16, (x >> 23) & 0xFF, x & 0x7F_FFFF);
    if exp == 0 {
        return sign;
    }

    let e = exp as i32 - 127 + 15;
    if e >= 31 {
        return sign | 0x7C00;
    }

    if e <= 0 {
        return sign;
    }

    sign | ((e as u16) << 10) | (man >> 13) as u16
}

/// STANDARD_RIGID_MESH_VERTEX: half4 pos, half4 uv, BGRA8 normal / tangent / bitangent, BGRA8 colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Vertex0 {
    pos: [u16; 4],
    uv: [u16; 4],
    normal: [u8; 4],
    tangent: [u8; 4],
    bitangent: [u8; 4],
    colour: [u8; 4],
}

/// Memory order B, G, R, A -> shader x = R, y = G, z = B.
fn pack_vec(x: f32, y: f32, z: f32) -> [u8; 4] {
    let q = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
    [q(z), q(y), q(x), 255]
}

fn axis_positions(cells: i32, stride: i32) -> Vec<i32> {
    let mut p = (0..cells).step_by(stride as usize).collect::<Vec<_>>();
    p.push(cells);
    p
}

/// One patch: a vertex buffer of (cells + 1)^2 vertices plus a skirt around it, and an index buffer per LOD (stride 1, 2, 4...).
fn build_patch(dev: &ID3D11Device, t: &Terrain, diffuse: Option<SharedTexture>, tex: &TextureCache, pi: i32, pj: i32) -> Option<(GpuModel, Option<SharedTexture>)> {
    let tile = if t.tile_metres > 0.0 { t.tile_metres } else { 8.0 };
    let height = |i: i32, j: i32| t.height_at(i.clamp(0, t.nx - 1), j.clamp(0, t.nz - 1));
    let (ci0, cj0) = (pi * CELLS, pj * CELLS);
    let (ci1, cj1) = ((ci0 + CELLS).min(t.nx - 1), (cj0 + CELLS).min(t.nz - 1));
    let (w, h) = (ci1 - ci0 + 1, cj1 - cj0 + 1);
    let (mut miny, mut maxy) = (1e30f32, -1e30f32);
    for j in cj0..=cj1 {
        for i in ci0..=ci1 {
            let y = height(i, j);
            miny = miny.min(y);
            maxy = maxy.max(y);
        }
    }

    let (x0, z0) = (t.origin_x + ci0 as f32 * t.step, t.origin_z + cj0 as f32 * t.step);
    let (u0, v0) = (x0 / tile - (x0 / tile).floor(), z0 / tile - (z0 / tile).floor());

    // The skin: this patch's rectangle of the image (one texel more on every side, edge texels repeated) becomes the patch's own
    // texture, uv 0..1 across it.
    let (mut sk_i0, mut sk_j0, mut sk_w, mut sk_h) = (0i32, 0i32, 0i32, 0i32);
    let (mut sk_dx, mut sk_dz) = (1.0f64, 1.0f64);
    let mut skin_out = None;
    if let Some(s) = t.skin.as_ref().filter(|s| s.w > 1 && s.h > 1) {
        sk_dx = s.w_m / f64::from(s.w);
        sk_dz = s.h_m / f64::from(s.h);
        let (px0, pz0) = (f64::from(x0), f64::from(z0));
        let px1 = f64::from(x0) + f64::from(w - 1) * f64::from(t.step);
        let pz1 = f64::from(z0) + f64::from(h - 1) * f64::from(t.step);
        sk_i0 = ((px0 - s.x0) / sk_dx).floor() as i32 - 1;
        sk_j0 = ((pz0 - s.z0) / sk_dz).floor() as i32 - 1;
        let i1 = ((px1 - s.x0) / sk_dx).ceil() as i32 + 1;
        let j1 = ((pz1 - s.z0) / sk_dz).ceil() as i32 + 1;
        sk_w = (i1 - sk_i0).max(2);
        sk_h = (j1 - sk_j0).max(2);
        let mut crop = vec![0u8; sk_w as usize * sk_h as usize * 4];
        for j in 0..sk_h {
            let sj = (sk_j0 + j).clamp(0, s.h as i32 - 1) as usize;
            for i in 0..sk_w {
                let si = (sk_i0 + i).clamp(0, s.w as i32 - 1) as usize;
                let at = (j as usize * sk_w as usize + i as usize) * 4;
                let from = (sj * s.w as usize + si) * 4;
                crop[at..at + 4].copy_from_slice(&s.rgba[from..from + 4]);
            }
        }

        let texture = make_rgba_texture(dev, sk_w as u32, sk_h as u32, &crop, true);
        if texture.srv.is_some() {
            skin_out = Some(Arc::new(RwLock::new(texture)));
        }
    }

    let (wu, hu) = (w as usize, h as usize);
    let grid = wu * hu;
    let mut v = vec![Vertex0::default(); grid + 2 * wu + 2 * hu];
    for j in 0..h {
        for i in 0..w {
            let (gi, gj) = (ci0 + i, cj0 + j);
            let (xl, zl) = (i as f32 * t.step, j as f32 * t.step);
            let vx = &mut v[j as usize * wu + i as usize];
            vx.pos = [to_half(xl), to_half(height(gi, gj) - miny), to_half(zl), to_half(1.0)];
            match (&skin_out, &t.skin) {
                (Some(_), Some(skin)) => {
                    let (xw, zw) = (f64::from(x0) + f64::from(xl), f64::from(z0) + f64::from(zl));
                    vx.uv[0] = to_half(((xw - skin.x0 - f64::from(sk_i0) * sk_dx) / (f64::from(sk_w) * sk_dx)) as f32);
                    vx.uv[1] = to_half(((zw - skin.z0 - f64::from(sk_j0) * sk_dz) / (f64::from(sk_h) * sk_dz)) as f32);
                }

                _ => {
                    vx.uv[0] = to_half(u0 + xl / tile);
                    vx.uv[1] = to_half(v0 + zl / tile);
                }
            }

            let dhdx = (height(gi + 1, gj) - height(gi - 1, gj)) / (2.0 * t.step);
            let dhdz = (height(gi, gj + 1) - height(gi, gj - 1)) / (2.0 * t.step);
            let n = normalize(Float3::new(-dhdx, 1.0, -dhdz));
            let tg = normalize(Float3::new(1.0, dhdx, 0.0));
            let bt = normalize(Float3::new(0.0, dhdz, 1.0));
            vx.normal = pack_vec(n.x, n.y, n.z);
            vx.tangent = pack_vec(tg.x, tg.y, tg.z);
            vx.bitangent = pack_vec(bt.x, bt.y, bt.z);
            vx.colour = [255; 4];
        }
    }

    // Skirts hang under the border vertices (south, north, west, east), so the cracks between patches of different detail stay closed.
    let depth = (t.step * 4.0).max(1.0);
    let south = grid as u32;
    let (north, west) = (south + w as u32, south + 2 * w as u32);
    let east = west + h as u32;
    let mut lowered = |from: usize, to: usize| {
        v[to] = v[from];
        v[to].pos[1] = to_half(half_to_float(v[from].pos[1]) - depth);
    };

    for i in 0..wu {
        lowered(i, south as usize + i);
        lowered((hu - 1) * wu + i, north as usize + i);
    }

    for j in 0..hu {
        lowered(j * wu, west as usize + j);
        lowered(j * wu + wu - 1, east as usize + j);
    }

    let bytes = v.iter().flat_map(|vx| {
        let mut out = [0u8; 32];
        for (k, p) in vx.pos.iter().chain(&vx.uv).enumerate() {
            out[k * 2..k * 2 + 2].copy_from_slice(&p.to_le_bytes());
        }

        out[16..20].copy_from_slice(&vx.normal);
        out[20..24].copy_from_slice(&vx.tangent);
        out[24..28].copy_from_slice(&vx.bitangent);
        out[28..32].copy_from_slice(&vx.colour);
        out
    }).collect::<Vec<_>>();

    let vb = immutable_buffer(dev, &bytes, D3D11_BIND_VERTEX_BUFFER)?;
    let diffuse = skin_out.clone().or(diffuse).unwrap_or_else(|| tex.white());
    let base = GpuMesh {
        vb: Some(vb),
        ib: None,
        index_count: 0,
        alpha_mode: 0,
        stride: 32,
        kind: MeshKind::Standard,
        uv2_tile: [0.0; 2],
        offset: Float3::new(x0, miny, z0),
        has_offset: x0 != 0.0 || miny != 0.0 || z0 != 0.0,
        tex: [diffuse, tex.black(), tex.black(), tex.black(), tex.flat_normal(), tex.white(), tex.white(), tex.white()],
        diffuse_path: String::new(),
        bmin: Float3::default(),
        bmax: Float3::new((w - 1) as f32 * t.step, maxy - miny, (h - 1) as f32 * t.step),
    };

    let cells = (ci1 - ci0).max(cj1 - cj0);
    let max_lods = if t.lod { 5 } else { 1 };
    let patch_m = CELLS as f32 * t.step;
    let mut out = GpuModel { path: "(terrain patch)".to_owned(), ..Default::default() };
    let (mut lod, mut stride) = (0, 1);
    while lod < max_lods && (lod == 0 || stride < cells) {
        let (xs, zs) = (axis_positions(w - 1, stride), axis_positions(h - 1, stride));
        let mut idx = Vec::<u16>::with_capacity((xs.len() - 1) * (zs.len() - 1) * 6 + (xs.len() + zs.len()) * 12);
        for j in 0..zs.len() - 1 {
            for i in 0..xs.len() - 1 {
                let a = (zs[j] * w + xs[i]) as u16;
                let b = (zs[j + 1] * w + xs[i]) as u16;
                let c = (zs[j + 1] * w + xs[i + 1]) as u16;
                let e = (zs[j] * w + xs[i + 1]) as u16;
                idx.extend([a, b, c, a, c, e]);
            }
        }

        // Both windings: no back-face culling decision for the skirt.
        let mut strip = |pos: &[i32], border: &dyn Fn(i32) -> i32, skirt_base: u32| {
            for k in 0..pos.len() - 1 {
                let (b0, b1) = (border(pos[k]) as u16, border(pos[k + 1]) as u16);
                let (s0, s1) = ((skirt_base + pos[k] as u32) as u16, (skirt_base + pos[k + 1] as u32) as u16);
                idx.extend([b0, b1, s1, b0, s1, s0, b0, s1, b1, b0, s0, s1]);
            }
        };

        strip(&xs, &|i| i, south);
        strip(&xs, &|i| (h - 1) * w + i, north);
        strip(&zs, &|j| j * w, west);
        strip(&zs, &|j| j * w + (w - 1), east);

        let ib = immutable_buffer(dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER)?;
        let mesh = GpuMesh { ib: Some(ib), index_count: idx.len() as u32, ..base.clone() };
        let last = lod + 1 >= max_lods || stride * 2 >= cells;
        let distance = if last { 0.0 } else { patch_m * 1.5 * (1 << lod) as f32 };
        if lod == 0 {
            out.meshes.push(mesh);
            out.distance = distance;
        } else {
            out.lods.push(GpuLod { meshes: vec![mesh], distance });
        }

        if last {
            break;
        }

        lod += 1;
        stride *= 2;
    }

    out.bmin = Float3::new(x0, miny, z0);
    out.bmax = Float3::new(x0 + (w - 1) as f32 * t.step, maxy, z0 + (h - 1) as f32 * t.step);
    Some((out, skin_out))
}
