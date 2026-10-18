//! One tile as a document of the shared editor: tile.ted (`TedScene`, lossless) + height_map_0.png (`HeightField`) of a `TileProject`.
//!
//! What it shows (render scene layer): the terrain from the height map, every BUILDING_INSTANCE as the real model pieces of
//! rigidmodels/buildings/<key>/ (a wire box when the key has no drawable model, reported in the log), every other instance as a small marker
//! batched per type, polygon outlines, and a highlight overlay for the selection and the active tool's cursor. Items of the document are
//! element indices + 1 (item 0 is the terrain). Edits are commands: attribute edits on the TedScene (moves, rotations), text splice records for
//! structural edits (duplicate, delete, layers, polygons), and brush strokes on the height field that merge into one entry per stroke.
//!
//! World space is scene space divided by the unit scale on every axis: x = scene x / unit_scale, y = scene y / unit_scale, z = scene z /
//! unit_scale, models and heights included, so the world has TEd's proportions (docs/formats/tile_ted.md; the legacy Studio divided x and z
//! only, which drew the terrain and every height twice as tall). The height map covers [-128, extent + 128] world units, see `TileProject`.
//!
//! The model the commands edit is [`Tile`] (everything but the undo stack); [`TileDocument`] is the stack around it. Where the C++ document kept
//! a pointer to the render scene it publishes into, the scene is passed in (`set_terrain_look`, `diagnostics`), and the document remembers
//! which scene it published into by its address only.

mod commands;
mod edits;
mod index;
mod pick;
mod view;

use std::any::Any;
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use wce_editor_core::command::{CommandStack, Dirty, History};
use wce_editor_core::document::{Document, DocumentCore, OutlinerNode, SaveTarget};
use wce_editor_core::inspector::Inspector;
use wce_formats::tile_climate::ClimateList;
use wce_foundation::math::{Double3, Float3, Float4x4};
use wce_foundation::log;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Overlay, RenderScene, SourceRef, TerrainMaterial, TerrainSkin};
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::WorldTransform;
use wce_vfs::files::Files;

use crate::terrain_bake::{BakedColour, TerrainLook};

pub use self::index::style_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Level,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub radius_m: f32,
    pub strength: f32,
    pub falloff: f32,
    pub flatten_height: f32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self { radius_m: 12.0, strength: 6.0, falloff: 0.7, flatten_height: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TileDisplay {
    pub hidden_types: BTreeSet<String>,
    pub show_polygons: bool,

    /// Distance culling of the markers.
    pub lod: bool,
    pub veg_distance: f32,
    pub decal_distance: f32,
    pub other_distance: f32,

    /// The tile's own rectangle (inside the bordered height map).
    pub show_border: bool,

    /// The tile database entry's link rules and targets on the edge cells (read-only view).
    pub show_links: bool,

    /// River ribbons and the sea / lake plane (the water pass); drawn whatever the spline layers' eye says.
    pub show_water: bool,
}

impl Default for TileDisplay {
    fn default() -> Self {
        Self {
            hidden_types: BTreeSet::new(), show_polygons: true, lod: true, veg_distance: 450.0, decal_distance: 300.0, other_distance: 3000.0, show_border: true,
            show_links: false, show_water: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildingModel {
    /// Paths of the rigid models (intact state of every piece).
    pub pieces: Vec<String>,

    /// Union of the pieces' bounds once they have loaded.
    pub bmin: Float3,
    pub bmax: Float3,

    /// At least one drawable piece (assumed while the pieces load in the background).
    pub ok: bool,

    /// Every piece has loaded or failed.
    pub resolved: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PieceStatus {
    #[default]
    Loading,
    Ready,
    Failed,
}

/// What the renderer's model cache says about one piece (the tool fills it in; the document does not know the renderer).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PieceState {
    pub status: PieceStatus,
    pub bmin: Float3,
    pub bmax: Float3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileTypeStyle {
    pub color: u32,
    pub size: f32,

    /// Many small instances: three lines instead of a box.
    pub cross: bool,
}

/// The surface_mode of an element, see "Placement" on [`TileDocument`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    None,
    Landscape,
    Freeform,
}

/// What the ground is drawn with: the game's terrain shaders (the blend / index / normal maps and the climate's textures) when true.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroundInfo {
    pub game_shaders: bool,
    pub layers: i32,
    pub climate: String,
    pub season: String,
}

/// A cell of the tile database entry's link view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkCell {
    /// The entry's cell (x = -1 or width: the neighbour column).
    pub x: i32,
    pub y: i32,
    pub set: String,
    pub text: String,
    pub equals: bool,
    pub target: bool,
    pub base_x: i32,
    pub base_y: i32,
}

/// A landscape instance further from the terrain than a tolerance: dy = y - the terrain height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffTerrain {
    pub elem: usize,
    pub dy: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlacementCounts {
    pub landscape: usize,
    pub freeform: usize,
    pub other: usize,
}

/// A landscape instance that follows a height stroke: its local position before and after.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Follow {
    pub elem: usize,
    pub before: Double3,
    pub after: Double3,

    /// The position attribute as it was: undo puts these bytes back.
    pub before_text: Option<String>,
}

pub(crate) const LAYER_BIT: u8 = 1;
pub(crate) const INSTANCE_BIT: u8 = 2;
pub(crate) const POLYGON_BIT: u8 = 4;

/// Structure of arrays over the elements of the scene (element index = array index): what the document asks about every instance (is it one,
/// where is it, which building). Built in one parallel pass over the XML elements plus one sequential pass for the parent chains, instead of
/// walking and re-parsing the ancestors of every instance; rebuilt after structural edits, patched for one element after an attribute edit.
#[derive(Clone, Debug, Default)]
pub(crate) struct Index {
    /// Into type_names.
    pub type_: Vec<u16>,

    /// LAYER_BIT | INSTANCE_BIT | POLYGON_BIT.
    pub kind: Vec<u8>,

    /// Into world: the nearest ancestor-or-self that has a position or an orientation.
    pub slot: Vec<u32>,

    /// Dense; world[0] is the identity.
    pub world: Vec<WorldTransform>,
    pub scale: Vec<[f32; 3]>,

    /// Building key id (into keys) of BUILDING_INSTANCE elements.
    pub key: Vec<Option<u32>>,

    /// surface_mode of instances: 0 none, 1 landscape, 2 freeform.
    pub mode: Vec<u8>,

    /// Per element: bit 0 shown (OIF_VISIBLE here and in every ancestor), bit 1 selectable (OIF_ACTIVE likewise).
    pub state: Vec<u8>,
    pub type_names: Vec<String>,
    pub keys: Vec<String>,

    /// Per type id.
    pub style: Vec<TileTypeStyle>,
    pub valid: bool,
}

/// The markers (instances without a model), grouped by type: group t is [group_begin[t], group_begin[t + 1]) of the parallel arrays.
#[derive(Clone, Debug, Default)]
pub(crate) struct Markers {
    pub elem: Vec<usize>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub z: Vec<f32>,
    pub group_begin: Vec<u32>,

    /// Per element: index into the arrays, `None` when it has no marker.
    pub slot: Vec<Option<u32>>,
}

/// The instances in cells of the ground plane, for the ray test of a click or a hover.
#[derive(Clone, Debug, Default)]
pub(crate) struct PickGrid {
    pub valid: bool,
    pub cell: f32,
    pub x0: f32,
    pub z0: f32,
    pub nx: i32,
    pub nz: i32,

    /// nx * nz + 1.
    pub start: Vec<u32>,

    /// Indices into the arrays below.
    pub items: Vec<u32>,
    pub elem: Vec<usize>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub z: Vec<f32>,
    pub r: Vec<f32>,
    pub type_: Vec<u16>,
    pub stamp: Vec<u32>,
    pub stamp_counter: u32,
}

/// Everything of a tile document but its undo stack: what its commands edit.
pub struct Tile {
    pub(crate) id: DocumentId,
    pub(crate) project: TileProject,
    pub(crate) files: Option<Arc<dyn Files>>,
    pub(crate) display: TileDisplay,

    // Display state: the scene published into (its address) and its entities.
    pub(crate) scene_key: usize,
    pub(crate) layer: LayerId,
    pub(crate) terrain_entity: EntityId,
    pub(crate) overlay_entity: EntityId,
    pub(crate) terrain_version: u64,
    pub(crate) terrain_stride: i32,
    pub(crate) instances: Vec<usize>,
    pub(crate) markers: Markers,
    pub(crate) type_counts: BTreeMap<String, i32>,

    /// Per type id: the overlay entity of that type's markers.
    pub(crate) batch_entity: Vec<EntityId>,

    /// Per type id: rebuild the overlay at the next update.
    pub(crate) dirty_type: Vec<u8>,

    /// Per element: its first model piece entity (the pieces have consecutive ids), 0 for none.
    pub(crate) first_entity: Vec<EntityId>,
    pub(crate) entity_count: Vec<u8>,

    /// Polygon outlines by element.
    pub(crate) outline_entity: HashMap<usize, EntityId>,
    pub(crate) models: BTreeMap<String, BuildingModel>,

    /// Key ids whose pieces are still loading.
    pub(crate) unresolved: Vec<usize>,
    pub(crate) fallbacks: Vec<String>,
    pub(crate) markers_dirty: bool,
    pub(crate) polygons_dirty: bool,
    pub(crate) hidden_signature: u64,
    pub(crate) lod_eye: Float3,
    pub(crate) grid: RefCell<PickGrid>,
    pub(crate) grid_dirty: std::cell::Cell<bool>,
    pub(crate) last_highlight: Option<Overlay>,

    // Edit state.
    pub(crate) gesture: u64,
    pub(crate) structure_version: u64,
    pub(crate) hint: Option<Vec<u64>>,
    pub(crate) stroke_open: bool,
    pub(crate) stroke_id: u64,

    /// The history's version after the stroke's last step.
    pub(crate) stroke_version: u64,
    pub(crate) saved_height_depth: usize,
    pub(crate) ted_on_disk_edited: bool,
    pub(crate) png_on_disk_edited: bool,

    // Water entities of the scene (rivers, sea plane), rebuilt when the terrain or the display changed.
    pub(crate) water_entities: Vec<EntityId>,
    pub(crate) links_entity: EntityId,
    pub(crate) links_dirty: bool,
    pub(crate) water_dirty: bool,
    pub(crate) water_version: u64,
    pub(crate) water_note: String,

    // Ground look.
    pub(crate) look: TerrainLook,
    pub(crate) skin_look: TerrainLook,
    pub(crate) skin_baked: Option<Arc<BakedColour>>,
    pub(crate) skin: Option<Arc<TerrainSkin>>,
    pub(crate) skin_id: u64,
    pub(crate) terrain_note: String,
    pub(crate) climates: OnceCell<ClimateList>,
    pub(crate) material_base: Option<Arc<TerrainMaterial>>,
    pub(crate) material_look: TerrainLook,
    pub(crate) material_id: u64,
    pub(crate) normal_id: u64,
    pub(crate) ground_info: GroundInfo,
    pub(crate) ground_info_base: GroundInfo,

    pub last_message: String,

    /// How long opening took, by phase.
    pub load_info: String,
    pub(crate) idx: Index,
}

pub(crate) fn scene_key(scene: &RenderScene) -> usize {
    scene as *const RenderScene as usize
}

pub struct TileDocument {
    core: DocumentCore,
    pub(crate) history: CommandStack<Tile>,
}

/// The item of an element; item 0 is the terrain.
pub fn item_of(elem: usize) -> u64 {
    elem as u64 + 1
}

/// The element of an item; `None` for the terrain.
pub fn elem_of(item: u64) -> Option<usize> {
    item.checked_sub(1).map(|e| e as usize)
}

pub const TERRAIN: u64 = 0;

impl TileDocument {

    /// A tile opened for editing; `files` are the game's files (the building models, the climate's textures), `None` without them.
    pub fn new(project: TileProject, files: Option<Arc<dyn Files>>) -> Self {
        let core = DocumentCore::new();
        let t0 = std::time::Instant::now();
        let mut tile = Tile {
            id: core.id(), project, files, display: TileDisplay::default(), scene_key: 0, layer: 0, terrain_entity: 0, overlay_entity: 0, terrain_version: 1,
            terrain_stride: 1, instances: vec![], markers: Markers::default(), type_counts: BTreeMap::new(), batch_entity: vec![], dirty_type: vec![],
            first_entity: vec![], entity_count: vec![], outline_entity: HashMap::new(), models: BTreeMap::new(), unresolved: vec![], fallbacks: vec![],
            markers_dirty: true, polygons_dirty: false, hidden_signature: 0, lod_eye: Float3::new(1e30, 1e30, 1e30), grid: RefCell::default(),
            grid_dirty: std::cell::Cell::new(true), last_highlight: None, gesture: 0x100, structure_version: 1, hint: None, stroke_open: false,
            stroke_id: 0x5000, stroke_version: 0, saved_height_depth: 0, ted_on_disk_edited: false, png_on_disk_edited: false, water_entities: vec![], links_entity: 0,
            links_dirty: true, water_dirty: true, water_version: 0, water_note: String::new(), look: TerrainLook::default(), skin_look: TerrainLook::default(),
            skin_baked: None, skin: None, skin_id: 0, terrain_note: String::new(), climates: OnceCell::new(), material_base: None,
            material_look: TerrainLook::default(), material_id: 0, normal_id: 0, ground_info: GroundInfo::default(), ground_info_base: GroundInfo::default(),
            last_message: String::new(), load_info: String::new(), idx: Index::default(),
        };

        tile.build_index();
        let t_index = t0.elapsed().as_secs_f64();
        tile.resolve_buildings();
        let t_models = t0.elapsed().as_secs_f64() - t_index;

        // The markers: everything that is not a building with a model, and the buildings that have none.
        tile.rebuild_items(false);
        if let Some(h) = &tile.project.height {
            tile.saved_height_depth = h.edit_depth();
        }

        let total = t0.elapsed().as_secs_f64();
        tile.load_info = format!(
            "tile loaded: {} instances in {total:.2} s (element index {t_index:.2}, building models {t_models:.2}, markers {:.2})", tile.instances.len(), total - t_index - t_models
        );
        log::info(tile.load_info.clone());
        Self { core, history: CommandStack::new(tile) }
    }

    pub fn tile(&self) -> &Tile {
        self.history.model()
    }

    pub fn tile_mut(&mut self) -> &mut Tile {
        self.history.model_mut()
    }

    pub fn project(&self) -> &TileProject {
        &self.tile().project
    }

    /// After a structural command the element indices moved: the selection the command wants (undo gives the selection before it).
    pub fn take_selection_hint(&mut self) -> Option<Vec<u64>> {
        self.tile_mut().hint.take()
    }

    pub fn next_gesture(&mut self) -> u64 {
        let t = self.tile_mut();
        t.gesture += 1;
        t.gesture
    }

    pub fn ref_of(&self, elem: usize) -> SourceRef {
        SourceRef { doc: self.id(), item: item_of(elem) }
    }
}

impl Document for TileDocument {
    fn core(&self) -> &DocumentCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut DocumentCore {
        &mut self.core
    }

    fn history(&self) -> &dyn History {
        &self.history
    }

    fn history_mut(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn title(&self) -> String {
        self.tile().title()
    }

    fn kind(&self) -> String {
        "tile".to_owned()
    }

    fn shows_scene(&self) -> bool {
        true
    }

    fn can_save(&self) -> bool {
        true
    }

    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        self.save_tile(target).map(|message| self.tile_mut().last_message = message)
    }

    fn publish(&mut self, scene: &mut RenderScene, layer: LayerId) {
        self.tile_mut().publish_items(scene, layer, false);
    }

    fn republish(&mut self, scene: &mut RenderScene, layer: LayerId, dirty: &Dirty) {
        let t = self.tile_mut();
        if dirty.all || t.scene_key != scene_key(scene) {
            t.rebuild_items(true);
            t.publish_items(scene, layer, true);
            return;
        }

        for &item in &dirty.items {
            match elem_of(item) {
                None => t.update_terrain(scene, false),
                Some(e) => t.refresh_item(scene, e),
            }
        }
    }

    fn outline(&self) -> OutlinerNode {
        self.tile().outline()
    }

    fn outline_stamp(&self) -> u64 {
        self.tile().structure_version
    }

    fn inspect(&mut self, item: SourceRef, inspector: &mut dyn Inspector) {
        self.inspect_item(item, inspector);
    }

    fn describe(&self, item: SourceRef) -> String {
        self.tile().describe(item)
    }

    fn source_key(&self) -> String {
        self.tile().project.dir.to_string_lossy().replace('\\', "/").to_lowercase()
    }

    fn item_transform(&self, item: SourceRef) -> Option<Float4x4> {
        self.tile().item_transform(item)
    }

    fn transform_items(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        self.transform_selected(items, delta, gesture);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
