//! The shared render and editor scene: a flat set of entities with stable handles.
//!
//! Documents own the truth and publish what they show into it (one layer per open document), the renderer draws it, and picking and
//! selection map back to the document through [`SourceRef`]. It is not serialized and holds no GPU objects: models and textures are
//! named by VFS path and resolved by the renderer's caches.
//!
//! Draw order is insertion order. [`RenderScene::revision`] changes whenever entities, layers or visibility change, so the renderer
//! rebuilds its draw lists only then.

use std::sync::Arc;

use wce_foundation::math::{Float3, Float4x4};

use crate::environment::{EnvironmentParams, blend_environments};

pub type DocumentId = u32;
pub type LayerId = u32;

/// 0 is "none".
pub type EntityId = u32;

/// The element of a document an entity shows (picking -> document element).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SourceRef {
    pub doc: DocumentId,
    pub item: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A rigid model, drawn through the game's rigid shaders.
    MeshInstance,

    /// Declared for the unit editor (bone palette, weighted vertex formats); not built yet.
    SkinnedInstance,

    /// A regular height grid.
    Terrain,

    /// A particle emitter (the weather precipitation stays with the environment for now).
    VfxEmitter,

    /// Lines and triangles: markers, polygons, boxes, grid, gizmos.
    Overlay,

    /// Declared for the cinematic editor.
    CameraRig,

    /// River ribbons and sea / lake planes, drawn by the renderer's water pass.
    Water,

    /// A projected decal, drawn by the renderer's decal pass.
    Decal,

    /// A grass model's patches, drawn by the renderer's grass pass.
    Grass,
}

/// One grass model's patches (rigidgrass, drawn instanced near the camera). Each instance is the rows of its 3x4 world matrix; the entity's
/// own matrix is not used.
#[derive(Clone, Debug, PartialEq)]
pub struct Grass {
    /// VFS path of the .rigid_model_v2.
    pub model: String,
    pub instances: std::sync::Arc<Vec<[f32; 12]>>,
}

/// A projected decal: the entity's world matrix maps the unit box (-0.5 .. 0.5) onto the volume it paints, projected along its y axis. The
/// model (rigidmodels/decals/...: that box) names the textures, `<model without extension>_diffuse.dds` and `_normal.dds` / `_parallax.dds`.
#[derive(Clone, Debug, PartialEq)]
pub struct Decal {
    /// VFS path of the .rigid_model_v2.
    pub model: String,

    /// The textures repeat over the decal (by its size) instead of stretching once over it.
    pub tiled: bool,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshInstance {
    /// VFS path of the .rigid_model_v2.
    pub model: String,
    pub casts_shadow: bool,

    /// Use the file's LODs by camera distance and skip it when it is too small to see (the sample map keeps LOD 0 always).
    pub lod: bool,

    /// Per-instance pixel shader inputs of rigidcommon (POSITION4..7); [`DEFAULT_INSTANCE_PARAMS`] is the engine's default.
    pub p4: [f32; 4],
    pub p5: [f32; 4],
    pub p6: [f32; 4],
    pub p7: [f32; 4],
}

/// POSITION4..7 of an instance with nothing special: zero, except the opacity in POSITION4.w. rigidstandard's gbuffer_alpha_test multiplies
/// the diffuse alpha by floor(w) * 0.001 before testing it against 0.5, so 0 there discards every pixel of alpha-tested meshes; 1000 is opaque.
pub const DEFAULT_INSTANCE_PARAMS: [f32; 16] = [0.0, 0.0, 0.0, 1000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

impl Default for MeshInstance {
    fn default() -> Self {
        let p = |i: usize| std::array::from_fn(|k| DEFAULT_INSTANCE_PARAMS[i * 4 + k]);
        Self { model: String::new(), casts_shadow: true, lod: false, p4: p(0), p5: p(1), p6: p(2), p7: p(3) }
    }
}

/// A colour image over the terrain (the tile tool's baked ground colour): it replaces the tiled diffuse. It covers the world rectangle
/// `[x0, x0 + w_m] x [z0, z0 + h_m]` (before the entity's transform), sRGB RGBA8, rows along +z. `id` changes with the content.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainSkin {
    pub x0: f64,
    pub z0: f64,
    pub w_m: f64,
    pub h_m: f64,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub id: u64,
}

impl Default for TerrainSkin {
    fn default() -> Self {
        Self { x0: 0.0, z0: 0.0, w_m: 1.0, h_m: 1.0, w: 0, h: 0, rgba: vec![], id: 0 }
    }
}

/// An RGBA8 image of the terrain material, row 0 at the rectangle's lowest z (u along +x).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainImage {
    pub w: u32,
    pub h: u32,

    /// Shared: a height stroke only replaces the normal map.
    pub rgba: Option<Arc<Vec<u8>>>,
}

/// The game's own terrain colour pipeline: the per-tile maps and the texture set its shaders (rigidterrain ps30_blend and
/// ps30_terrain_to_gbuffer_ss) read, in the layout of BOB's blend.dds / index.dds / normal.dds. Texture slot = 4 * group + variation
/// of the tile entry's TEXTURE_GROUP (red, green, blue, alpha).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMaterial {
    /// The rectangle the maps cover, in world metres: the tile with its border.
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,

    /// The vertical extent of the box that draws it.
    pub y0: f64,
    pub y1: f64,

    /// r g b a = the weights of the four groups (sum 1).
    pub blend: TerrainImage,

    /// r g b a = per group the selector of its four variations: (weighted mean variation) / 3.
    pub index: TerrainImage,

    /// .a = x, .g = z of the surface normal * 0.5 + 0.5 (the two-channel normal the shader reads as .wy); follows the heights.
    pub normal: TerrainImage,

    /// Changes when `normal` does (the rest of the material is reused then).
    pub normal_id: u64,

    /// Pack paths per slot; empty is a neutral slice.
    pub diffuse: [String; 16],
    pub normal_tex: [String; 16],

    /// Metres per repeat per group (g_blend_pixel_scale_array[].x), its .y, and .z (mid_distance_strength).
    pub pixel_scale: [f32; 4],
    pub outfield_scale: [f32; 4],
    pub mid_strength: [f32; 4],

    /// s_light_params per slot.
    pub smoothness: [f32; 16],
    pub reflectivity: [f32; 16],

    /// The climate's medium_detail_map_colour / _normal.
    pub detail_colour: String,
    pub detail_normal: String,

    /// Changes with the content.
    pub id: u64,
}

impl Default for TerrainMaterial {
    fn default() -> Self {
        Self {
            x0: 0.0,
            z0: 0.0,
            x1: 1.0,
            z1: 1.0,
            y0: -100.0,
            y1: 1000.0,
            blend: TerrainImage::default(),
            index: TerrainImage::default(),
            normal: TerrainImage::default(),
            normal_id: 0,
            diffuse: Default::default(),
            normal_tex: Default::default(),
            pixel_scale: [40.1; 4],
            outfield_scale: [1200.1; 4],
            mid_strength: [1.0; 4],
            smoothness: [0.0; 16],
            reflectivity: [0.0; 16],
            detail_colour: String::new(),
            detail_normal: String::new(),
            id: 0,
        }
    }
}

/// A regular grid of heights: vertex (i, j) is at x = origin_x + i * step, z = origin_z + j * step, y = heights[j * nx + i] (before the
/// entity's world transform). The interim material tiles one diffuse texture every `tile_metres` and shades with the game's rigid
/// G-buffer shader.
#[derive(Clone, Debug, PartialEq)]
pub struct Terrain {
    pub nx: i32,
    pub nz: i32,
    pub origin_x: f32,
    pub origin_z: f32,
    pub step: f32,

    /// nx * nz; empty is flat at 0.
    pub heights: Vec<f32>,
    pub diffuse: String,
    pub tile_metres: f32,

    /// Receiver only.
    pub casts_shadow: bool,

    /// Patches of 64 x 64 cells switch to coarser index buffers with distance.
    pub lod: bool,

    /// When set, each patch gets its piece of this image instead of the tiled diffuse (the debug views of the tile maps).
    pub skin: Option<Arc<TerrainSkin>>,

    /// When set, the ground is drawn by the game's terrain shaders (the legacy patches stay for picking, shadows and the environment cube).
    pub material: Option<Arc<TerrainMaterial>>,

    /// Change it when the heights or the texture change: the renderer rebuilds the mesh.
    pub version: u64,
}

impl Default for Terrain {
    fn default() -> Self {
        Self {
            nx: 2,
            nz: 2,
            origin_x: -1.0,
            origin_z: -1.0,
            step: 2.0,
            heights: vec![],
            diffuse: "terrain/textures/battle/default/grass/0_su_diffuse.dds".to_owned(),
            tile_metres: 8.0,
            casts_shadow: false,
            lod: false,
            skin: None,
            material: None,
            version: 0,
        }
    }
}

impl Terrain {

    /// A flat square of two triangles.
    pub fn flat(half_size: f32, diffuse: &str, tile_metres: f32) -> Self {
        Self { origin_x: -half_size, origin_z: -half_size, step: half_size * 2.0, diffuse: diffuse.to_owned(), tile_metres, ..Default::default() }
    }

    pub fn height_at(&self, i: i32, j: i32) -> f32 {
        if self.heights.is_empty() { 0.0 } else { self.heights[j as usize * self.nx as usize + i as usize] }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VfxEmitter {
    /// The vfx definition name; not drawn yet.
    pub effect: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayVertex {
    pub p: Float3,

    /// 0xAABBGGRR as a little-endian u32, i.e. bytes r, g, b, a.
    pub rgba: u32,
}

/// An overlay colour from 0..1 channels.
pub fn overlay_colour(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let q = |v: f32| if v <= 0.0 { 0 } else if v >= 1.0 { 255 } else { (v * 255.0 + 0.5) as u32 };
    q(r) | (q(g) << 8) | (q(b) << 16) | (q(a) << 24)
}

/// An immediate-style batch: `lines` are vertex pairs (1 px wide), `triangles` vertex triples (alpha blended).
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub lines: Vec<OverlayVertex>,
    pub triangles: Vec<OverlayVertex>,

    /// Against the scene depth; false draws on top.
    pub depth_test: bool,

    /// Picked in screen space (distance to a line, inside a triangle).
    pub pickable: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self { lines: vec![], triangles: vec![], depth_test: true, pickable: true }
    }
}

impl Overlay {

    pub fn line(&mut self, a: Float3, b: Float3, rgba: u32) {
        self.lines.push(OverlayVertex { p: a, rgba });
        self.lines.push(OverlayVertex { p: b, rgba });
    }

    pub fn triangle(&mut self, a: Float3, b: Float3, c: Float3, rgba: u32) {
        self.triangles.extend([a, b, c].map(|p| OverlayVertex { p, rgba }));
    }

    pub fn box_wire(&mut self, lo: Float3, hi: Float3, rgba: u32) {
        self.box_wire_transformed(&Float4x4::IDENTITY, lo, hi, rgba);
    }

    /// An oriented box: its corners go through `m`.
    pub fn box_wire_transformed(&mut self, m: &Float4x4, lo: Float3, hi: Float3, rgba: u32) {
        let c = [
            Float3::new(lo.x, lo.y, lo.z), Float3::new(hi.x, lo.y, lo.z), Float3::new(hi.x, lo.y, hi.z), Float3::new(lo.x, lo.y, hi.z),
            Float3::new(lo.x, hi.y, lo.z), Float3::new(hi.x, hi.y, lo.z), Float3::new(hi.x, hi.y, hi.z), Float3::new(lo.x, hi.y, hi.z),
        ].map(|p| m.transform_point(p));

        for i in 0..4 {
            self.line(c[i], c[(i + 1) % 4], rgba);
            self.line(c[4 + i], c[4 + (i + 1) % 4], rgba);
            self.line(c[i], c[4 + i], rgba);
        }
    }

    pub fn polygon_outline(&mut self, points: &[Float3], closed: bool, rgba: u32) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], rgba);
        }

        if closed && points.len() > 2 {
            self.line(points[points.len() - 1], points[0], rgba);
        }
    }
}

/// Placeholder for the cinematic editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRig {}

/// A water vertex: the river vertex shader's input, P0 = (x, y, z, across), P1 = (u, v, flow, fade); the sea plane's vertex shader
/// reads only the position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,

    /// River: metres from the centre line (the shader's P0.w); sea: 0.
    pub across: f32,

    /// River: u = metres across (centre 0), v = metres along the spline; sea: world x, z.
    pub u: f32,
    pub v: f32,

    /// The river's flow speed there (the spline point's flow_speed).
    pub flow: f32,

    /// alpha_fade of the river's ends (0..1), 1 for the sea; the game's pixel shader doesn't read it.
    pub fade: f32,
}

/// A water surface as a triangle mesh in world coordinates (the entity's world matrix is applied on top), drawn with the game's
/// shaders (rigidriver for a river, waterplane for the sea or lake plane).
#[derive(Clone, Debug, PartialEq)]
pub struct Water {
    pub verts: Vec<WaterVertex>,

    /// A triangle list.
    pub indices: Vec<u32>,

    /// False: the sea / lake plane.
    pub river: bool,

    /// The river points' foam_amount, averaged (kept for the editor; the game's river shader has no foam).
    pub foam: f32,

    /// Change it when the mesh changes.
    pub version: u64,
}

impl Default for Water {
    fn default() -> Self {
        Self { verts: vec![], indices: vec![], river: true, foam: 0.1, version: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    MeshInstance(MeshInstance),
    Terrain(Terrain),
    VfxEmitter(VfxEmitter),
    Overlay(Overlay),
    CameraRig(CameraRig),
    Water(Water),
    Decal(Decal),
    Grass(Grass),
}

impl Payload {
    pub fn kind(&self) -> Kind {
        match self {
            Self::MeshInstance(_) => Kind::MeshInstance,
            Self::Terrain(_) => Kind::Terrain,
            Self::VfxEmitter(_) => Kind::VfxEmitter,
            Self::Overlay(_) => Kind::Overlay,
            Self::CameraRig(_) => Kind::CameraRig,
            Self::Water(_) => Kind::Water,
            Self::Decal(_) => Kind::Decal,
            Self::Grass(_) => Kind::Grass,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub layer: LayerId,
    pub world: Float4x4,
    pub source: SourceRef,
    pub visible: bool,

    /// Not pickable (an inactive layer's instances).
    pub locked: bool,
    pub payload: Payload,
}

impl Entity {
    pub fn kind(&self) -> Kind {
        self.payload.kind()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub id: LayerId,
    pub doc: DocumentId,
    pub name: String,
    pub visible: bool,

    /// Not pickable.
    pub locked: bool,
    pub order: i32,
}

/// Which environment lights the scene. The active weather document's environment wins; otherwise the tile tool's picker result
/// applies; otherwise the game profile's default (all handled by whoever fills the slot).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnvironmentSlot {
    pub active: Option<EnvironmentParams>,

    /// The cross-fade partner.
    pub blend: Option<EnvironmentParams>,

    /// 0: active, 1: blend.
    pub blend_t: f32,
    pub owner: DocumentId,
}

impl EnvironmentSlot {

    /// The active environment (or the defaults), cross-faded with the blend partner when blend_t > 0.
    pub fn resolved(&self) -> EnvironmentParams {
        let a = self.active.clone().unwrap_or_default();
        match (&self.active, &self.blend) {
            (Some(_), Some(blend)) if self.blend_t > 0.0 => blend_environments(&a, blend, self.blend_t),
            _ => a,
        }
    }
}

/// An index entry of an entity id without an entity.
const NONE: u32 = u32::MAX;

/// Moved-entity log entries kept while nobody follows the log.
const MOVED_LOG_LIMIT: usize = 1 << 20;

/// The scene.
#[derive(Clone, Debug)]
pub struct RenderScene {
    layers: Vec<Layer>,
    entities: Vec<Entity>,

    /// Entity id -> position in `entities` (ids are handed out in sequence and never reused).
    index: Vec<u32>,
    next_layer: LayerId,
    next_entity: EntityId,
    revision: u64,
    structure: u64,
    terrain_rev: u64,
    moved_log: Vec<EntityId>,

    /// The log position of moved_log[0].
    moved_base: u64,

    pub environment: EnvironmentSlot,
}

impl Default for RenderScene {
    fn default() -> Self {
        Self {
            layers: vec![],
            entities: vec![],
            index: vec![],
            next_layer: 1,
            next_entity: 1,
            revision: 1,
            structure: 1,
            terrain_rev: 1,
            moved_log: vec![],
            moved_base: 0,
            environment: EnvironmentSlot::default(),
        }
    }
}

impl RenderScene {

    pub fn add_layer(&mut self, doc: DocumentId, name: &str) -> LayerId {
        let id = self.next_layer;
        self.next_layer += 1;
        self.layers.push(Layer { id, doc, name: name.to_owned(), visible: true, locked: false, order: self.layers.len() as i32 });
        self.touch();
        id
    }

    /// Removes a layer and every entity in it.
    pub fn remove_layer(&mut self, id: LayerId) {
        self.clear_layer(id);
        self.layers.retain(|l| l.id != id);
        self.touch();
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) {
        if let Some(l) = self.layer_mut(id)
            && l.visible != visible
        {
            l.visible = visible;
            self.touch();
        }
    }

    pub fn reserve(&mut self, n: usize) {
        self.entities.reserve(n);
        self.index.reserve(n);
    }

    /// The id the next added entity gets.
    pub fn next_id(&self) -> EntityId {
        self.next_entity
    }

    pub fn add(&mut self, layer: LayerId, world: Float4x4, payload: Payload, source: SourceRef) -> EntityId {
        let id = self.next_entity;
        self.next_entity += 1;
        if self.index.len() <= id as usize {
            self.index.resize(id as usize + 1, NONE);
        }

        self.index[id as usize] = self.entities.len() as u32;
        self.entities.push(Entity { id, layer, world, source, visible: true, locked: false, payload });
        self.touch();
        id
    }

    pub fn find(&self, id: EntityId) -> Option<&Entity> {
        self.index.get(id as usize).filter(|&&at| at != NONE).map(|&at| &self.entities[at as usize])
    }

    /// An entity to edit in place; call [`Self::touch`] afterwards (`update_*` do both).
    pub fn find_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.index.get(id as usize).filter(|&&at| at != NONE).map(|&at| &mut self.entities[at as usize])
    }

    /// Moves an entity: logged for [`Self::moved_since`], no structural change.
    pub fn update_world(&mut self, id: EntityId, world: Float4x4) {
        let Some(e) = self.find_mut(id) else { return };
        e.world = world;
        self.revision += 1;

        // Nobody is following: forget the old entries.
        if self.moved_log.len() > MOVED_LOG_LIMIT {
            self.moved_base += self.moved_log.len() as u64;
            self.moved_log.clear();
        }

        self.moved_log.push(id);
    }

    /// Replaces an entity's payload. An overlay replacing an overlay changes nothing the renderer's draw set keeps, and a terrain
    /// replacing a terrain only changes the terrain revision.
    pub fn update_payload(&mut self, id: EntityId, payload: Payload) {
        let Some(e) = self.find_mut(id) else { return };
        let overlay_only = matches!((&e.payload, &payload), (Payload::Overlay(_), Payload::Overlay(_)));
        let terrain_only = matches!((&e.payload, &payload), (Payload::Terrain(_), Payload::Terrain(_)));
        e.payload = payload;
        if overlay_only {
            self.revision += 1;
        } else if terrain_only {
            self.revision += 1;
            self.terrain_rev += 1;
        } else {
            self.touch();
        }
    }

    /// The entities whose world matrix changed since `cursor` (start from 0, pass the same variable again); `None` when the log no
    /// longer reaches back that far (rebuild everything then). Afterwards `cursor` is the log's end.
    pub fn moved_since(&self, cursor: &mut u64) -> Option<Vec<EntityId>> {
        let end = self.moved_base + self.moved_log.len() as u64;
        let start = *cursor;
        *cursor = end;
        if start < self.moved_base || start > end {
            return None;
        }

        Some(self.moved_log[(start - self.moved_base) as usize..].to_vec())
    }

    fn reindex(&mut self) {
        self.index.fill(NONE);
        for (i, e) in self.entities.iter().enumerate() {
            if self.index.len() <= e.id as usize {
                self.index.resize(e.id as usize + 1, NONE);
            }

            self.index[e.id as usize] = i as u32;
        }
    }

    pub fn remove(&mut self, id: EntityId) {
        let Some(&at) = self.index.get(id as usize).filter(|&&at| at != NONE) else { return };
        self.entities.remove(at as usize);
        self.reindex();
        self.touch();
    }

    /// Removes the entities with ids in `ids` (one pass, for many entities added in a row).
    pub fn remove_range(&mut self, ids: std::ops::Range<EntityId>) {
        self.retain_entities(|e| !ids.contains(&e.id));
    }

    /// Keeps the layer, drops its entities.
    pub fn clear_layer(&mut self, id: LayerId) {
        self.retain_entities(|e| e.layer != id);
    }

    /// [`Self::clear_layer`], but one entity stays (a terrain whose meshes are expensive to rebuild).
    pub fn clear_layer_except(&mut self, id: LayerId, keep: EntityId) {
        self.retain_entities(|e| e.layer != id || e.id == keep);
    }

    fn retain_entities(&mut self, keep: impl Fn(&Entity) -> bool) {
        let before = self.entities.len();
        self.entities.retain(keep);
        if self.entities.len() != before {
            self.reindex();
            self.touch();
        }
    }

    pub fn clear(&mut self) {
        self.entities.clear();
        self.index.clear();
        self.layers.clear();
        self.touch();
    }

    /// Every entity in draw order (insertion order); hidden ones included, see [`Self::drawable`].
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The entities that show one element of a document (a building has one entity per model piece), in draw order.
    pub fn entities_of(&self, source: SourceRef) -> Vec<EntityId> {
        self.entities.iter().filter(|e| e.source == source).map(|e| e.id).collect()
    }

    /// A visible entity in a visible layer.
    pub fn drawable(&self, e: &Entity) -> bool {
        e.visible && self.layer(e.layer).is_some_and(|l| l.visible)
    }

    /// An entity of a visible layer that picking may hit.
    pub fn pickable(&self, e: &Entity) -> bool {
        e.visible && !e.locked && self.layer(e.layer).is_some_and(|l| l.visible && !l.locked)
    }

    /// Changes with everything.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Changes only when entities, layers, visibility or a payload change (not for a moved entity), so a consumer that keeps derived
    /// data (the renderer's draw set) can patch moved entities instead of rebuilding.
    pub fn structure_revision(&self) -> u64 {
        self.structure
    }

    /// Changes when the payload of a terrain entity (its heights) is replaced: the renderer rebuilds only its terrain draw items then.
    pub fn terrain_revision(&self) -> u64 {
        self.terrain_rev
    }

    pub fn touch(&mut self) {
        self.revision += 1;
        self.structure += 1;
    }
}
