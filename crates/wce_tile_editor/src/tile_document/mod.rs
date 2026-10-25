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

mod collapse;
mod columns;
mod commands;
mod draft;
pub mod context;
mod edits;
mod index;
pub mod kit;
mod pick;
mod preview;
mod grass;
mod vegetation;
mod view;

use std::any::Any;
use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wce_editor_core::command::{CommandStack, Dirty, History};
use wce_editor_core::document::{Document, DocumentCore, OutlinerNode, SaveTarget};
use wce_editor_core::inspector::Inspector;
use wce_formats::tile_climate::ClimateList;
use wce_foundation::math::{Double3, Float3, Float4x4};
use wce_foundation::log;
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, LfMap, Overlay, RenderScene, SourceRef, TerrainMaterial, TerrainSkin};
use wce_tile::building_db::BuildingDb;
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::WorldTransform;
use wce_vfs::files::Files;

use crate::terrain_bake::{BakedColour, TerrainLook};

pub use self::draft::{Draft, tilt_to, tilted, turned, with_root_attr};
pub use self::edits::prefab_reference_text;
pub use self::index::style_of;
pub use self::vegetation::{DEFAULT_TREE_LIST, GENERATED_PREFIX};

/// What a new instance is made from (`TileDocument::create`): a point on the ground (world metres), or drawn points.
#[derive(Clone, Debug, PartialEq)]
pub enum Create {
    Point(Float3),
    Line(Vec<Float3>),
}

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

    /// River ribbons (the water pass); drawn whatever the spline layers' eye says.
    pub show_water: bool,

    /// TEd's sea: a plane at 0 m over every tile, drawn for context (not an entity of the tile). The map context draws its own sea while it
    /// is shown; this hides both.
    pub show_sea: bool,

    /// The procedural trees drawn: the name of one of the tile's tree lists (`default`, a climate key like `rome_med`), empty for none.
    pub tree_list: String,

    /// The grass of BOB's `default.grass_list.bin`, near the camera.
    pub show_grass: bool,

    /// Generate the grass from the seed (as BOB would) even when the tile has a packed list.
    pub generate_grass: bool,
}

impl Default for TileDisplay {
    fn default() -> Self {
        Self {
            hidden_types: BTreeSet::new(), show_polygons: true, lod: true, veg_distance: 450.0, decal_distance: 300.0, other_distance: 3000.0, show_border: true,
            show_links: false, show_water: true, show_sea: true, tree_list: DEFAULT_TREE_LIST.to_owned(), show_grass: true, generate_grass: false,
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

    /// The number of destruction levels of the building (the highest `destructNN` of its piece files): `destruction_level` of an instance goes
    /// from 0 to this - 1 (TEd: `building->num_destruction_levels()`). 0 when it is not known.
    pub levels: u32,

    /// Per piece, its own number of levels (a building's pieces don't all have the same: roman_fort_straight's have 2 or 3).
    pub piece_levels: Vec<u32>,
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

    /// Per element: its index among the instances of its type in document order, hidden ones included (BOB's export order).
    pub(crate) type_ordinal: Vec<u32>,
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

    /// The selection outline's mask overlay: solid shapes for the selected instances without meshes of their own (markers), which the
    /// renderer's outline goes around (`Overlay::selection_mask`).
    pub(crate) mask_entity: EntityId,
    pub(crate) last_mask: Option<Overlay>,

    /// The selected elements that have meshes of their own (their outline is the meshes'), for this selection at this scene structure
    /// revision: found again only when one of them changes.
    pub(crate) meshed_for: (Vec<usize>, u64),
    pub(crate) meshed: HashSet<u64>,

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

    /// The walls' bridging columns (columns.rs).
    pub(crate) columns: columns::ColumnState,

    /// The buildings' pieces collapsing between two levels (collapse.rs).
    pub(crate) collapses: collapse::CollapseState,
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

    /// The kit's `battlefield_buildings` (categories), read on first use.
    pub(crate) building_db: OnceCell<BuildingDb>,

    /// The projects folder in the kit's layout tiles are saved into (kit.rs); none: saved where they are.
    pub(crate) projects_root: Option<std::path::PathBuf>,

    /// What the tool is about to place, following the mouse (preview.rs).
    pub(crate) preview: preview::Preview,

    /// Text typed into a line edit of the Property Editor and not applied yet, by (element, attribute): it is applied when the typing ends.
    pub(crate) drafts: HashMap<(usize, String), String>,

    /// The edited pieces of buildings by (building element, piece), and the pieces whose entities still have to follow their properties.
    pub(crate) piece_props: HashMap<(usize, usize), PieceProps>,
    pub(crate) piece_pending: Vec<(usize, usize)>,
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

    /// The decoded tree list of the display's `tree_list`.
    pub(crate) trees: vegetation::TreeList,
    pub(crate) grass: grass::GrassState,

    /// Draw the terrain (and its material) over the tile's own cells only, without the height map's border: the map context's neighbours
    /// are drawn there.
    pub(crate) clip_to_tile: bool,

    /// The material id of the clipped maps (0: none yet).
    pub(crate) material_clip_id: u64,

    /// The map context's LF map: the terrain is drawn on it.
    pub(crate) context_lf: Option<Arc<LfMap>>,
}

pub(crate) fn scene_key(scene: &RenderScene) -> usize {
    scene as *const RenderScene as usize
}

/// Ids of ground skins, materials and normal maps. The renderer keeps GPU copies by these ids, so they must differ between tiles: a counter per
/// tile would give every tile's first material the id 1, and the second tile opened would draw with the first one's maps and textures.
pub(crate) static SKIN_IDS: AtomicU64 = AtomicU64::new(0);
pub(crate) static MATERIAL_IDS: AtomicU64 = AtomicU64::new(0);
pub(crate) static NORMAL_IDS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_id(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed) + 1
}

pub struct TileDocument {
    core: DocumentCore,
    pub(crate) history: CommandStack<Tile>,
}

/// The item of an element; item 0 is the terrain.
pub fn item_of(elem: usize) -> u64 {
    elem as u64 + 1
}

/// The element of an item; `None` for the terrain and for the pieces of a building.
pub fn elem_of(item: u64) -> Option<usize> {
    if item & PIECE_ITEM != 0 {
        return None;
    }

    item.checked_sub(1).map(|e| e as usize)
}

/// The pieces of a building are children of it in TEd's Scene tab ("building piece", one node per mesh). Their items have this bit, the
/// building's element in bits 16..50 and the number of the piece in the low 16 bits.
pub const PIECE_ITEM: u64 = 1 << 50;

pub fn piece_item(elem: usize, piece: usize) -> u64 {
    PIECE_ITEM | ((elem as u64) << 16) | (piece as u64 & 0xffff)
}

/// (building element, piece number) of a piece item.
pub fn piece_of(item: u64) -> Option<(usize, usize)> {
    (item & PIECE_ITEM != 0).then_some((((item & (PIECE_ITEM - 1)) >> 16) as usize, (item & 0xffff) as usize))
}

/// The properties of one building piece (`BUILDING_PIECE_INSTANCE`). TEd makes the pieces when it loads a building and does not write them to
/// the tile (a BUILDING_INSTANCE has no children in any tile.ted): what is edited here lasts for the session. The values a new piece has are
/// what TEd shows for one.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceProps {
    pub visible: bool,
    pub active: bool,
    pub draggable: bool,
    pub freeform: bool,
    pub position: [f32; 3],

    /// Radians.
    pub euler: [f32; 3],
    pub scale: [f32; 3],
    pub opacity: f32,
    pub burn: bool,
    pub burn_amount: f32,
    pub glow_amount: f32,
    pub dirt_level: f32,
    pub bounds_colour: [f32; 4],
    pub show_collision: bool,
    pub seasons: [bool; 4],

    /// The destruction level; -1 is the building's highest one (the intact piece, `destruct01`: every new building has `levels - 1`, and so do
    /// most instances of the kit's tiles).
    pub destruction_level: i64,
}

impl Default for PieceProps {
    fn default() -> Self {
        Self {
            visible: true, active: true, draggable: false, freeform: true, position: [0.0; 3], euler: [0.0; 3], scale: [1.0; 3], opacity: 1.0, burn: false,
            burn_amount: 0.0, glow_amount: 0.0, dirt_level: 0.0, bounds_colour: [1.0; 4], show_collision: false, seasons: [true; 4], destruction_level: -1,
        }
    }
}

pub const TERRAIN: u64 = 0;

impl TileDocument {

    /// A tile opened for editing; `files` are the game's files (the building models, the climate's textures), `None` without them.
    pub fn new(project: TileProject, files: Option<Arc<dyn Files>>) -> Self {
        let core = DocumentCore::new();
        let t0 = std::time::Instant::now();
        let mut tile = Tile {
            id: core.id(), project, files, display: TileDisplay::default(), scene_key: 0, layer: 0, terrain_entity: 0, overlay_entity: 0, terrain_version: 1,
            terrain_stride: 1, instances: vec![], type_ordinal: vec![], markers: Markers::default(), type_counts: BTreeMap::new(), batch_entity: vec![], dirty_type: vec![],
            first_entity: vec![], entity_count: vec![], outline_entity: HashMap::new(), models: BTreeMap::new(), unresolved: vec![], fallbacks: vec![],
            markers_dirty: true, polygons_dirty: false, hidden_signature: 0, lod_eye: Float3::new(1e30, 1e30, 1e30), grid: RefCell::default(),
            grid_dirty: std::cell::Cell::new(true), last_highlight: None, mask_entity: 0, last_mask: None, meshed_for: (vec![], u64::MAX), meshed: HashSet::new(), gesture: 0x100, structure_version: 1, hint: None, stroke_open: false,
            stroke_id: 0x5000, stroke_version: 0, saved_height_depth: 0, ted_on_disk_edited: false, png_on_disk_edited: false, water_entities: vec![], columns: columns::ColumnState::default(), collapses: collapse::CollapseState::default(), links_entity: 0,
            links_dirty: true, water_dirty: true, water_version: 0, water_note: String::new(), look: TerrainLook::default(), skin_look: TerrainLook::default(),
            skin_baked: None, skin: None, skin_id: 0, terrain_note: String::new(), climates: OnceCell::new(), building_db: OnceCell::new(), preview: preview::Preview::default(), projects_root: None, drafts: HashMap::new(), piece_props: HashMap::new(), piece_pending: vec![], material_base: None,
            material_look: TerrainLook::default(), material_id: 0, normal_id: 0, ground_info: GroundInfo::default(), ground_info_base: GroundInfo::default(),
            last_message: String::new(), load_info: String::new(), idx: Index::default(), trees: Default::default(), grass: Default::default(),
            clip_to_tile: false, material_clip_id: 0, context_lf: None,
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

    fn save_message(&self) -> Option<String> {
        Some(self.tile().last_message.clone())
    }

    fn save_wanted(&self) -> bool {
        !self.is_project()
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
