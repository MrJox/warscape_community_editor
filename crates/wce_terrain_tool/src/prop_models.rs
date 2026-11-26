//! A campaign map's props drawn as their models near the camera (the `.terry`'s `ECPropMesh` entities: `ECMesh model_path`; without a
//! `.terry`, the packs' `prop_marker.markers`), each with
//! the transform the export writes for it (`wce_terrain_io::attila::campaign::matrix`, the kit's `ECTransform`: its rotation and scale in
//! `.terry` units, so a model is scaled by 1 / the frame's unit into the view) and standing on the drawn ground. Drawn again when the
//! scene changes or the camera moves on; pickable (a click selects the prop). The overlays stay (their crosses mark the props too far
//! for a model).
//!
//! With them the campaign's forests, as the game draws them: `campaign_maps/<map>/display/trees/trees.campaign_tree_list` in the packs
//! (BOB's; the kit has no source for it): u32 version (3), f32 bounds x0 z0 x1 z1 (the map: x east, z north, 678.5 x 556.1 on Attila's
//! main map, not square to its cells), u32 models, per model a u16-length path and a u32 count of 15-byte trees (f32 x, y, z; u8 0 / 1,
//! u8 0 .. 5, u8 15). Mapped by the bounds, all but 6 of 17 576 sampled trees stand on land (`campaign_tree_fit`; by the 0.32 a cell the
//! `.terry` frame once took, a third in the sea or off the map). A tree stands at its y (the list's units, as a prop at its marker's y;
//! 2026-11-16: on the ground instead, the rocks and cliff pieces sunk into the hills by their markers floated over them).
//! ASSUMPTIONS: the second byte is the turn in sixths (its six values equally common); the first byte (1 on 5582) and the last (15: the
//! seasons?) are not read.

use std::collections::HashMap;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{
    DEFAULT_INSTANCE_PARAMS, DocumentId, EntityId, FactionColours, LayerId, MeshInstance, Payload, SourceRef, VfxEmitter,
};
use wce_terrain::terry::EntityKind;
use wce_terrain_editor::MapDocument;
use wce_tile_editor::tile_document::context::Frame;

/// A campaign's tree list (`trees.campaign_tree_list`).
pub struct CampaignTrees {
    pub models: Vec<String>,

    /// (model, cell x, cell y (down), turn in sixths, the file's y).
    pub trees: Vec<(u16, f32, f32, u8, f32)>,

    /// Cells a unit of the list (its models' too: the game's vertex shader puts a model's vertices at the list's position plus 0.4 times
    /// theirs, `rigidcampaignvegetation.hlsl`; the 0.4 is the model cache's), across.
    pub cells_per_unit: f32,

    /// The trees' indices by square of BUCKET cells (x, y).
    buckets: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

/// The side of a tree bucket (cells).
const BUCKET: f32 = 16.0;

/// The key bit of a settlement's building among the drawn models.
const CITY_KEY: u64 = 1 << 62;

/// With [`CITY_KEY`], an aqueduct's.
const AQUEDUCT_KEY: u64 = 1 << 61;

/// With [`CITY_KEY`], a sprawl piece's.
const SPRAWL_KEY: u64 = 1 << 60;

impl CampaignTrees {
    /// The indices of the trees within `r` cells of a cell.
    fn near(&self, cx: f32, cy: f32, r: f32) -> impl Iterator<Item = u32> + '_ {
        let (b0, b1) = (((cx - r) / BUCKET).floor() as i32, ((cx + r) / BUCKET).floor() as i32);
        let (c0, c1) = (((cy - r) / BUCKET).floor() as i32, ((cy + r) / BUCKET).floor() as i32);
        (c0..=c1).flat_map(move |by| (b0..=b1).map(move |bx| (bx, by))).filter_map(|b| self.buckets.get(&b)).flatten().copied().filter(
            move |&i| {
                let t = &self.trees[i as usize];
                (t.1 - cx).hypot(t.2 - cy) <= r
            },
        )
    }

    /// The indices of the trees within `r` cells of `at` and further than `r` from `not` (a disc's that its neighbour leaves out: what a
    /// move of the disc brings in, or with the two swapped what it leaves). The buckets wholly inside the other disc are not looked into.
    fn near_not(&self, at: (f32, f32), not: (f32, f32), r: f32) -> impl Iterator<Item = u32> + '_ {
        let (b0, b1) = (((at.0 - r) / BUCKET).floor() as i32, ((at.0 + r) / BUCKET).floor() as i32);
        let (c0, c1) = (((at.1 - r) / BUCKET).floor() as i32, ((at.1 + r) / BUCKET).floor() as i32);
        let inside = move |x: f32, y: f32| (x - not.0).hypot(y - not.1) <= r;
        (c0..=c1)
            .flat_map(move |by| (b0..=b1).map(move |bx| (bx, by)))
            .filter(move |&(bx, by)| {
                let (x0, y0, x1, y1) = (bx as f32 * BUCKET, by as f32 * BUCKET, (bx + 1) as f32 * BUCKET, (by + 1) as f32 * BUCKET);
                !(inside(x0, y0) && inside(x1, y0) && inside(x0, y1) && inside(x1, y1))
            })
            .filter_map(|b| self.buckets.get(&b))
            .flatten()
            .copied()
            .filter(move |&i| {
                let t = &self.trees[i as usize];
                (t.1 - at.0).hypot(t.2 - at.1) <= r && !inside(t.1, t.2)
            })
    }

    /// The trees of a file on a map `w` x `h` cells; none for a file that does not read.
    pub fn parse(b: &[u8], w: u32, h: u32) -> Option<Self> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        let f32_at = |o: usize| u32_at(o).map(f32::from_bits);
        let (x1, z1) = (f32_at(12)?, f32_at(16)?);
        if u32_at(0)? != 3 || x1 <= 0.0 || z1 <= 0.0 {
            return None;
        }

        let (mut models, mut trees, mut at) = (vec![], vec![], 24);
        for m in 0..u32_at(20)? {
            let n = usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]));
            models.push(String::from_utf8_lossy(b.get(at + 2..at + 2 + n)?).to_lowercase().replace('\\', "/"));
            at += 2 + n;
            let count = u32_at(at)? as usize;
            at += 4;
            for k in 0..count {
                let o = at + 15 * k;
                let (x, z) = (f32_at(o)?, f32_at(o + 8)?);
                trees.push((m as u16, x / x1 * w as f32, (1.0 - z / z1) * h as f32, *b.get(o + 13)?, f32_at(o + 4)?));
            }

            at += 15 * count;
        }

        let mut buckets: std::collections::HashMap<(i32, i32), Vec<u32>> = std::collections::HashMap::new();
        for (i, t) in trees.iter().enumerate() {
            buckets.entry(((t.1 / BUCKET).floor() as i32, (t.2 / BUCKET).floor() as i32)).or_default().push(i as u32);
        }

        Some(Self { models, trees, cells_per_unit: w as f32 / x1, buckets })
    }
}

/// What the drawn models are of: document, `.terry` revision, the cell the camera's target was in.
type Built = (DocumentId, u64, (i64, i64));

/// How many tiles coming into memory build the models again (`PropModels::update`).
const LOADED_STEP: usize = 16;

/// How far from the camera's target the forests are drawn (world units): the game draws its campaign trees over the whole view, as
/// far as its land goes in it (RenderDoc, the G-buffer's `rigidcampaignvegetation` instances: 236 units from the eye in capture A, 236
/// in C, 186 in F; about 21 000 trees in each; no count or distance cap found), where the props' radius (96 cells, 64 units on
/// `main_attila_map`) left the distant land without its forests (capture F's far ground plain and darker). All around the target, not
/// only ahead: the trees ahead taken by the view's sector (eight of them) came and went by the tens of thousands each time a turning
/// camera crossed into the next, a 40 - 90 ms frame every 45 degrees; the whole circle costs nothing a steady frame shows.
const TREE_REACH: f32 = 240.0;

/// A prop: its key (a `.terry` entity's id, else MARKER_KEY | its record), `.terry` position, model and transform (`campaign::matrix`'s
/// layout), and the item a click selects (u64::MAX: none).
struct Prop {
    key: u64,
    at: [f64; 3],
    model: String,
    matrix: [f32; 16],
    item: u64,

    /// The seasons it shows in (`ECSeasonMask`, the markers' `seasons`: spring 1, summer 2, autumn 4, winter 8): main_attila_map's
    /// fallen leaves are autumn's alone, many of its animals spring's (the game's spring frame draws no leaves).
    seasons: u32,
}

/// The key of a prop of the packs' markers.
const MARKER_KEY: u64 = 1 << 59;

/// The key of a placed effect (its record in `particle_emitter_marker.markers`).
const EFFECT_KEY: u64 = 1 << 58;

/// The placed effects within this many campaign world units of the camera's target are handed to the renderer, which plays those in
/// view within 80 units of the eye (`wce_render::particles::CAMPAIGN_EFFECT_REACH`): the eye is up to some 50 units from the target at
/// the campaign's zooms.
const EFFECT_RADIUS: f64 = 130.0;

/// A campaign map's placed effect (`particle_emitter_marker.markers`: the packs', or what BOB makes of the `.terry`'s `ECVFX`): its
/// effect (`vfx/<effect>.xml`), position and transform in the `.terry`'s frame, scale, seasons (spring 1, summer 2, autumn 4, winter 8).
struct PlacedEffect {
    key: u64,
    at: [f64; 3],
    effect: String,
    matrix: [f32; 16],
    scale: f32,
    seasons: u32,

    /// `visible_inside_snow_zone`, `visible_outside_snow_zone`.
    snow: [bool; 2],
}

/// A campaign snow mask (`snow_mask_*.dds`: L8, row 0 the map's north edge) for the placed effects' snow zone.
struct SnowGrid {
    width: usize,
    height: usize,
    values: Vec<u8>,
}

impl SnowGrid {
    /// An uncompressed 8-bit DDS (the masks' format); None for another.
    fn parse(b: &[u8]) -> Option<Self> {
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        if b.get(..4)? != b"DDS " || u32_at(88)? != 8 {
            return None;
        }

        let (height, width) = (u32_at(12)? as usize, u32_at(16)? as usize);
        let values = b.get(128..128 + width * height)?.to_vec();
        Some(Self { width, height, values })
    }

    /// The mask at a `.terry` point (x east, z north from the map's south-west corner) over a map `size` wide: the weather node's reading
    /// (row from the north; RenderDoc: only the flipped reading fits the captures' snow and rain spots).
    fn at(&self, x: f64, z: f64, size: f64) -> u8 {
        let cell = size / self.width as f64;
        let col = (x / cell).floor();
        let row = self.height as f64 - 1.0 - (z / cell).floor();
        if col < 0.0 || row < 0.0 || col >= self.width as f64 || row >= self.height as f64 {
            return 0;
        }

        self.values[row as usize * self.width + col as usize]
    }
}

/// A placed effect is in the snow zone where the season's mask reaches this (ASSUMPTION: the game's test not traced; in the captures
/// a rain spot at 255 was off and a snow spot at 0 off, none at the values between).
const SNOW_ZONE: u8 = 128;

/// The effects of a `particle_emitter_marker.markers` (their group's key the effect).
fn marker_effects(bytes: &[u8]) -> Vec<PlacedEffect> {
    use wce_formats::markers::{MarkersFile, Value};
    let Ok(f) = MarkersFile::parse(bytes) else { return vec![] };
    let mut effect_of = vec![String::new(); f.records.len()];
    for g in f.groups.iter().flatten() {
        let key = group_key(&g.key);
        for &i in &g.indices {
            if let Some(m) = effect_of.get_mut(i as usize) {
                m.clone_from(&key);
            }
        }
    }

    effect_of
        .into_iter()
        .enumerate()
        .filter(|(_, n)| !n.is_empty())
        .filter_map(|(i, effect)| {
            let fields = f.fields(i).ok()?;
            let get = |name: &str| fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v);
            let Some(Value::Matrix(matrix)) = get("transform") else { return None };
            let scale = match get("scale") {
                Some(Value::F32(s)) => *s,
                _ => 1.0,
            };
            let seasons = match get("seasons") {
                Some(Value::U32(s)) => *s,
                _ => 15,
            };
            let flag = |name: &str| !matches!(get(name), Some(Value::U8(0)));
            let snow = [flag("visible_inside_snow_zone"), flag("visible_outside_snow_zone")];
            let at = [f64::from(matrix[12]), f64::from(matrix[13]), f64::from(matrix[14])];
            Some(PlacedEffect { key: EFFECT_KEY | i as u64, at, effect, matrix: *matrix, scale, seasons, snow })
        })
        .collect()
}

/// A markers group key's text.
fn group_key(key: &[wce_formats::markers::KeyPart]) -> String {
    use wce_formats::markers::KeyPart;
    key.iter()
        .find_map(|k| match k {
            KeyPart::Wide(w) => Some(String::from_utf16_lossy(w)),
            KeyPart::Narrow(n) => Some(String::from_utf8_lossy(n).into_owned()),
            KeyPart::U32(_) => None,
        })
        .unwrap_or_default()
}

/// The props of a campaign map's `prop_marker.markers` in the packs (records with a `transform`, their group's key the model).
fn marker_props(bytes: &[u8]) -> Vec<Prop> {
    use wce_formats::markers::{MarkersFile, Value};
    let Ok(f) = MarkersFile::parse(bytes) else { return vec![] };
    let mut model_of = vec![String::new(); f.records.len()];
    for g in f.groups.iter().flatten() {
        let key = group_key(&g.key);
        for &i in &g.indices {
            if let Some(m) = model_of.get_mut(i as usize) {
                m.clone_from(&key);
            }
        }
    }

    model_of
        .into_iter()
        .enumerate()
        .filter(|(_, m)| !m.is_empty())
        .filter_map(|(i, model)| {
            let fields = f.fields(i).ok()?;
            let Some(Value::Matrix(matrix)) = fields.iter().find(|(n, _)| *n == "transform").map(|(_, v)| v.clone()) else { return None };
            let at = [f64::from(matrix[12]), f64::from(matrix[13]), f64::from(matrix[14])];
            let seasons = fields
                .iter()
                .find(|(n, _)| *n == "seasons")
                .and_then(|(_, v)| match v {
                    Value::U32(s) => Some(*s),
                    Value::U8(s) => Some(u32::from(*s)),
                    _ => None,
                })
                .unwrap_or(15);
            Some(Prop { key: MARKER_KEY | i as u64, at, model: model.to_lowercase().replace('\\', "/"), matrix, item: u64::MAX, seasons })
        })
        .collect()
}

pub struct PropModels {
    pub on: bool,

    /// Props within this many cells of the camera's target get their model.
    pub radius_cells: f64,
    layer: LayerId,

    /// The drawn models by key (a prop's `.terry` id, an effect's, a settlement building's; the trees: `trees_drawn`): a camera move adds
    /// and removes only the difference.
    drawn: wce_foundation::id_hash::IdMap<u64, EntityId>,
    built: Option<Built>,

    /// What the models were last drawn for besides the camera: (document, `.terry` revision and season), which draws them all again,
    /// and the ground (tiles loaded, the settlements read), which stands the settlements' buildings again (the only ones on it: the
    /// props, effects and trees stand at their own heights). Drawn all again as tiles came, a pan's rebuilds took 25 - 40 ms.
    built_full: Option<(DocumentId, u64)>,
    built_ground: Option<u64>,

    /// The trees drawn by their index in the list, and the disc they were drawn for (centre cell, radius in cells): a move adds and
    /// removes those of the two discs' difference (looking at every tree of the disc each rebuild was most of its time).
    trees_drawn: wce_foundation::id_hash::IdMap<u32, EntityId>,
    tree_disc: Option<(f32, f32, f32)>,
    pub count: usize,

    /// The shown map's props (document, `.terry` revision; per prop its key, `.terry` position, model and transform), made again when the
    /// `.terry` changes: walking its 14 644 entities for their world positions at every camera step cost up to 100 ms. A map without a
    /// `.terry` (Rome 2's built campaign maps) has the packs' `prop_marker.markers` (the same frame and transforms: what BOB makes of a
    /// `.terry`); its props are not pickable.
    props: Option<(DocumentId, u64, Vec<Prop>)>,

    /// The shown campaign's trees (its document; none: no list).
    trees: Option<(DocumentId, Option<Arc<CampaignTrees>>)>,

    /// The shown campaign's settlements at its start (`wce_terrain_io::attila::cities`), read on a thread (the startpos and the db
    /// tables take seconds): none until they are, or when they can't be.
    cities: Option<(DocumentId, Arc<std::sync::OnceLock<Option<Arc<Settlements>>>>)>,
    pub city_count: usize,
    pub tree_count: usize,

    /// The shown map's placed effects (document, `.terry` revision), and the view's season as a bit of their `seasons` (spring 1,
    /// summer 2, autumn 4, winter 8; the tool sets it); those of the season near the camera are drawn.
    effects: Option<(DocumentId, u64, Vec<PlacedEffect>)>,
    pub season_bit: u32,

    /// The scene's snow mask (its path) read for the effects' snow zone; none: no mask (nothing in the snow zone).
    snow: Option<(String, Option<SnowGrid>)>,
    pub effect_count: usize,

    /// The placed effects handed over, by effect (for `terrain_props`).
    pub effect_names: std::collections::BTreeMap<String, usize>,

    /// The drawn effect nearest the camera's target: its effect and position in the view (`terrain_props`).
    pub effect_near: Option<(String, [f32; 3])>,

    /// Per drawn tree within the props' radius: the file's y and the ground's height in the list's units (the view's over k)
    /// (`terrain_props`).
    pub heights: Vec<(f32, f32)>,

    /// The props near the camera that move (a model with its own clip: the fauna, `wce_render::animated_prop`): key, world, model;
    /// posed every frame (`pose_animated`), not drawn as models.
    animated: Vec<(u64, Float4x4, String)>,

    /// Each model's clip (none: it has none, or it does not load), and whether a model has one.
    clips: std::collections::HashMap<String, Option<Arc<wce_render::animated_prop::AnimatedProp>>>,
    has_clip: std::collections::HashMap<String, bool>,
    animated_drawn: std::collections::HashMap<u64, EntityId>,
    pub animated_count: usize,
}

impl Default for PropModels {
    fn default() -> Self {
        Self {
            on: true,
            radius_cells: 96.0,
            layer: 0,
            drawn: Default::default(),
            built_full: None,
            built_ground: None,
            trees_drawn: Default::default(),
            tree_disc: None,
            built: None,
            count: 0,
            props: None,
            trees: None,
            tree_count: 0,
            effects: None,
            snow: None,
            season_bit: 2,
            effect_count: 0,
            effect_names: std::collections::BTreeMap::new(),
            effect_near: None,
            cities: None,
            city_count: 0,
            heights: vec![],
            animated: vec![],
            clips: Default::default(),
            has_clip: Default::default(),
            animated_drawn: Default::default(),
            animated_count: 0,
        }
    }
}

impl PropModels {
    /// Waits up to `max` for the settlements' read (scripts: `terrain_settle`). False: still reading.
    pub fn wait_cities(&self, max: std::time::Duration) -> bool {
        let Some((_, cell)) = &self.cities else { return true };
        let t0 = std::time::Instant::now();
        while cell.get().is_none() {
            if t0.elapsed() >= max {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    /// `ground`: the drawn ground at a frame point (the LF and the tiles' own heights, `MapView::ground_at`), which the props, trees and
    /// buildings stand on; `loaded`: the view's tiles in memory, which build the models again as more come (each brings its ground).
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<Arc<Frame>>, ground: &dyn Fn(f64, f64) -> f32, loaded: usize) {
        let Some(frame) = frame.filter(|_| self.on) else { return self.clear(e) };
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        // A map without a `.terry` (Rome 2's built campaign maps) still has its settlements and forests.
        let p = d.project();
        let tf = d.terry_frame;
        if !tf.y_from_lf {
            // Battle maps: their props come with the tiles.
            return self.clear(e);
        }

        let cam = e.viewport.camera();
        let (tx, tz) = tf.from_frame(f64::from(cam.target.x), f64::from(cam.target.z));
        let quarter = (self.radius_cells / 4.0).max(1.0) * tf.cell;
        // The settlements arriving (read on a thread) build the models again.
        let cities_ready = self.cities.as_ref().is_some_and(|(id, c)| *id == doc && c.get().is_some());
        let full = d.terry_revision.load(std::sync::atomic::Ordering::Relaxed) * 16 + u64::from(self.season_bit & 15);
        // The settlements' buildings again as tiles come (in steps: each brings the ground they stand on).
        let ground_rev = (loaded / LOADED_STEP) as u64 * 2 + u64::from(cities_ready);
        let revision = full * 1_000_003 + ground_rev;
        // The season's snow mask (the scene's), which decides the effects' snow zone.
        let mask = e.scene.campaign_snow.as_ref().map(|s| s.mask.clone());
        if self.snow.as_ref().map(|s| &s.0) != mask.as_ref() {
            self.snow = mask.map(|m| {
                let grid = e.files().and_then(|f| f.read(&m).ok()).and_then(|b| SnowGrid::parse(&b));
                (m, grid)
            });
            self.built = None;
        }

        let key = (doc, revision, ((tx / quarter).floor() as i64, (tz / quarter).floor() as i64));
        if self.built == Some(key) {
            return;
        }

        // The settlements: read once a map, away from the UI (its campaign's startpos: `wce_terrain_io::attila::start`).
        if self.cities.as_ref().is_none_or(|(id, _)| *id != doc) {
            let cell = Arc::new(std::sync::OnceLock::new());
            let (_, name) = crate::tool::map_kind(&d.name);
            let name = name.to_owned();
            let game = e.game();
            let (files, dir) = (e.files(), e.settings_ref().game_dir());
            let out = cell.clone();
            std::thread::spawn(move || {
                let cities = (|| {
                    let files = files?;
                    let schema = rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(game)?, None).ok()?;
                    let list = |p: &str| files.list(p, "");
                    let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
                    let tables = wce_terrain_io::attila::cities::DisplayTables::read(&schema, &list, &read);
                    let map_data = files.read(&format!("campaign_maps/{name}/map_data.esf")).ok()?;
                    let reader = crate::tool::startpos_reader(&files, dir.as_deref());
                    let start = wce_terrain_io::attila::start::find(&schema, &name, &list, &read, &reader).ok()?;
                    let cities = wce_terrain_io::attila::cities::read(&map_data, &start.startpos, &tables).ok()?;
                    let flags = flag_colours(&schema, &list, &read);
                    Some(Arc::new(Settlements { cities, flags }))
                })();
                let _ = out.set(cities);
            });
            self.cities = Some((doc, cell));
        }

        let cities = self.cities.as_ref().and_then(|(_, c)| c.get().cloned().flatten());

        if self.trees.as_ref().is_none_or(|(id, _)| *id != doc) {
            wce_foundation::wce_prof!("props.tree_list");
            let (_, name) = crate::tool::map_kind(&d.name);
            let list = e
                .files()
                .and_then(|f| f.read(&format!("campaign_maps/{name}/display/trees/trees.campaign_tree_list")).ok())
                .and_then(|b| CampaignTrees::parse(&b, frame.map.width, frame.map.height))
                .map(Arc::new);
            self.trees = Some((doc, list));
        }

        if self.props.as_ref().is_none_or(|(id, rev, _)| *id != doc || *rev != key.1) {
            wce_foundation::wce_prof!("props.index");
            let list = match p {
                Some(p) => p
                    .entities
                    .iter()
                    .filter_map(|x| match p.kind(x) {
                        EntityKind::Prop(model) if !model.is_empty() => Some(Prop {
                            key: x.id,
                            at: p.world_position(x.id),
                            model: model.to_lowercase().replace('\\', "/"),
                            matrix: wce_terrain_io::attila::campaign::matrix(p, x),
                            item: x.id,
                            seasons: wce_terrain_io::attila::campaign::seasons(p, x),
                        }),
                        _ => None,
                    })
                    .collect(),
                None => {
                    let (_, name) = crate::tool::map_kind(&d.name);
                    e.files()
                        .and_then(|f| f.read(&format!("terrain/campaigns/{name}/prop_marker.markers")).ok())
                        .map_or_else(Vec::new, |b| marker_props(&b))
                }
            };

            self.props = Some((doc, key.1, list));
        }

        // The placed effects: the `.terry`'s `ECVFX` as BOB exports them (`campaign::particle_markers`), else the packs' file.
        let terry_rev = d.terry_revision.load(std::sync::atomic::Ordering::Relaxed);
        if self.effects.as_ref().is_none_or(|(id, rev, _)| *id != doc || *rev != terry_rev) {
            let (_, name) = crate::tool::map_kind(&d.name);
            let bytes = match p {
                Some(p) => wce_terrain_io::attila::campaign::particle_markers(p).ok(),
                None => e
                    .files()
                    .and_then(|f| f.read(&format!("terrain/campaigns/{name}/particle_emitter_marker.markers")).ok())
                    .map(|b| b.to_vec()),
            };
            let mut list = bytes.map_or_else(Vec::new, |b| marker_effects(&b));
            // Effects the packs do not have (TDD names `gaul_campaign_vfx_blossom_vfx_autumn_leaves`): the game finds nothing and shows
            // nothing; left out, listed once.
            if let Some(files) = e.files() {
                let mut missing: Vec<String> = vec![];
                list.retain(|fx| {
                    let found = files.exists(&format!("vfx/{}.xml", fx.effect));
                    if !found && !missing.contains(&fx.effect) {
                        missing.push(fx.effect.clone());
                    }

                    found
                });
                if !missing.is_empty() {
                    wce_foundation::log::info(format!(
                        "terrain {name}: placed effects naming no effect in the packs, not drawn (as in the game): {}",
                        missing.join(", ")
                    ));
                }
            }

            self.effects = Some((doc, terry_rev, list));
        }

        wce_foundation::wce_prof!("props.build");
        let r = self.radius_cells * tf.cell;
        let k = (1.0 / tf.unit) as f32;
        // (key, world, payload, the item a click selects); no payload: already in the scene as it is (a tree kept from the last build).
        let mut out: Vec<(u64, Float4x4, Option<Payload>, u64)> = vec![];
        // A `.terry` edit (or another document) draws every model again (below); otherwise those drawn stay.
        let again = self.built.is_none() || self.built_full != Some((doc, full));
        let ground_again = self.built_ground != Some(ground_rev);
        let mut animated: Vec<(u64, Float4x4, String)> = vec![];
        let files = e.files();
        let props = self.props.as_ref().map_or(&[][..], |(_, _, l)| l.as_slice());
        for prop in props.iter().filter(|p| p.seasons & self.season_bit != 0) {
            let (w, m) = (prop.at, prop.matrix);
            if (w[0] - tx).hypot(w[2] - tz) > r {
                continue;
            }

            let (fx, fz) = tf.to_frame(w[0], w[2]);
            let mut world = Float4x4::IDENTITY;
            for row in 0..3 {
                for col in 0..3 {
                    // The marker's row `col` is the transform's column `col`.
                    world.m[row][col] = m[col * 4 + row] * k;
                }
            }

            world.m[0][3] = fx as f32;
            // At the marker's own height: most stand on the LF (the ground less the marker's height, 25 / 50 / 75 %: 1 / 10 / 33
            // frame units near Emyn Muil), but the rocks and cliff pieces are sunk into it (`cliff_med02` at 1.748, 480 units under
            // the LF there; 10 % of the props over 105): stood on the ground they floated over the hills.
            world.m[1][3] = (w[1] / tf.unit) as f32;
            world.m[2][3] = fz as f32;
            // A model with its own clip moves (the fauna): posed every frame instead.
            let moves = *self.has_clip.entry(prop.model.clone()).or_insert_with(|| {
                wce_render::animated_prop::clip_path(&prop.model).is_some_and(|c| files.as_ref().is_some_and(|f| f.exists(&c)))
            });
            if moves {
                animated.push((prop.key, world, prop.model.clone()));
                continue;
            }
            let p16 = DEFAULT_INSTANCE_PARAMS;
            let mesh = MeshInstance {
                model: prop.model.clone(),
                casts_shadow: true,
                lod: true,
                p4: [p16[0], p16[1], p16[2], p16[3]],
                p5: [p16[4], p16[5], p16[6], p16[7]],
                p6: [p16[8], p16[9], p16[10], p16[11]],
                p7: [p16[12], p16[13], p16[14], p16[15]],
                burning: false,
                faction_colours: None,
            };
            out.push((prop.key, world, Some(Payload::MeshInstance(mesh)), prop.item));
        }

        // The placed effects of the season near the camera, in the props' frame; their scale in the shaders' units (the renderer's
        // `scale * units`) is the marker's, as the game's instances (RenderDoc: the instance's scale the marker's). The destruction zone's
        // flags are not read: every vanilla marker shows inside and outside it.
        self.effect_count = 0;
        self.effect_names.clear();
        self.effect_near = None;
        let mut near = f64::MAX;
        let effects = self.effects.as_ref().map_or(&[][..], |(_, _, l)| l.as_slice());
        for fx in effects {
            let w = fx.at;
            if fx.seasons & self.season_bit == 0 || (w[0] - tx).hypot(w[2] - tz) > EFFECT_RADIUS {
                continue;
            }

            // The snow zone: the season's mask at the marker (fn 0x10b053d0's flag bits 2 / 4; no mask: outside).
            let inside = self
                .snow
                .as_ref()
                .and_then(|s| s.1.as_ref())
                .is_some_and(|g| g.at(w[0], w[2], f64::from(frame.map.width) * tf.cell) >= SNOW_ZONE);
            if !fx.snow[usize::from(!inside)] {
                continue;
            }

            let (fx_x, fx_z) = tf.to_frame(w[0], w[2]);
            let mut world = Float4x4::IDENTITY;
            for row in 0..3 {
                for col in 0..3 {
                    world.m[row][col] = fx.matrix[col * 4 + row] * k;
                }
            }

            (world.m[0][3], world.m[1][3], world.m[2][3]) = (fx_x as f32, (w[1] / tf.unit) as f32, fx_z as f32);
            let vfx = VfxEmitter { effect: fx.effect.clone(), scale: fx.scale * k, mesh: None };
            let dist = (w[0] - tx).hypot(w[2] - tz);
            if dist < near {
                near = dist;
                self.effect_near = Some((fx.effect.clone(), [world.m[0][3], world.m[1][3], world.m[2][3]]));
            }

            out.push((fx.key, world, Some(Payload::VfxEmitter(vfx)), u64::MAX));
            self.effect_count += 1;
            *self.effect_names.entry(fx.effect.clone()).or_default() += 1;
        }

        // The forests near the camera: by cells (the trees' own frame), within the same radius. Those of the new disc not drawn come, those
        // of the old one outside the new go; all again for a `.terry` edit or another radius.
        let trees_scope = wce_foundation::profile::Scope::new(TREES_SLOT.get_or_init(|| wce_foundation::profile::slot("props.trees")));
        self.heights.clear();
        let mut trees_new: Vec<(u32, Float4x4, Payload)> = vec![];
        let mut trees_gone: Vec<u32> = vec![];
        let mut trees_all = again;
        if let Some((_, Some(trees))) = &self.trees {
            let (cx, cy) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            let rc = self.radius_cells as f32;
            let (a, b) = (frame.from_map.apply(0.0, 0.0), frame.from_map.apply(1.0, 0.0));
            // Drawn by the game's `rigidcampaignvegetation` (the renderer's `MeshKind::CampaignVegetation`): its instance is the list's
            // position (plus 0.4 x the mesh's pivot, the draw set's) and the turn, its vertices 0.4 of the world's units a model unit, as
            // the game's (RenderDoc, capture A: docs/re/terry.md "Forests"). The matrix's scale is that, for the culling's bounds; its
            // turn about y is what the draw set reads as the angle (the list's byte times 60 degrees).
            let k = (b.0 - a.0).hypot(b.1 - a.1) as f32;
            let unit = trees.cells_per_unit * k;
            let size = 0.4 / e.scene.game_units.max(1e-9);
            let (cx, cy) = (cx as f32, cy as f32);
            let reach = (TREE_REACH * trees.cells_per_unit).max(rc);
            let tree = |i: u32| {
                let (m, x, y, turn, file_y) = trees.trees[i as usize];
                let (fx, fz) = frame.from_map.apply(f64::from(x), f64::from(y));
                let (s, c) = (f32::from(turn) * std::f32::consts::FRAC_PI_3).sin_cos();
                let mut world = Float4x4::IDENTITY;
                world.m[0] = [c * size, 0.0, s * size, fx as f32];
                // At the list's own height, as the props at their markers'.
                world.m[1] = [0.0, size, 0.0, file_y * unit];
                world.m[2] = [-s * size, 0.0, c * size, fz as f32];
                let p16 = DEFAULT_INSTANCE_PARAMS;
                let mesh = MeshInstance {
                    model: trees.models[usize::from(m)].clone(),
                    casts_shadow: true,
                    lod: true,
                    p4: [p16[0], p16[1], p16[2], p16[3]],
                    p5: [p16[4], p16[5], p16[6], p16[7]],
                    p6: [p16[8], p16[9], p16[10], p16[11]],
                    p7: [p16[12], p16[13], p16[14], p16[15]],
                    burning: false,
                    faction_colours: None,
                };
                (i, world, Payload::MeshInstance(mesh))
            };

            match self.tree_disc.filter(|d| !trees_all && d.2 == reach) {
                Some((ox, oy, _)) if (ox, oy) == (cx, cy) => {}
                Some((ox, oy, _)) => {
                    trees_new.extend(trees.near_not((cx, cy), (ox, oy), reach).filter(|i| !self.trees_drawn.contains_key(i)).map(tree));
                    trees_gone.extend(trees.near_not((ox, oy), (cx, cy), reach));
                }
                None => {
                    trees_all = true;
                    trees_new.extend(trees.near(cx, cy, reach).map(tree));
                }
            }

            self.tree_disc = Some((cx, cy, reach));
            // The diagnostic's sample: those near the target (the LF read under all of them was a third of a rebuild).
            for i in trees.near(cx, cy, rc) {
                let (_, x, y, _, file_y) = trees.trees[i as usize];
                let (fx, fz) = frame.from_map.apply(f64::from(x), f64::from(y));
                self.heights.push((file_y, frame.lift(fx, fz) / unit));
            }
        } else {
            trees_all = true;
            self.tree_disc = None;
        }

        drop(trees_scope);

        // The settlements' buildings near the camera at their cell's centre, standing on the ground, at the models' scale
        // (`Cities::model_scale`).
        self.city_count = 0;
        if let Some(Settlements { cities: c, flags }) = cities.as_deref() {
            let (w, h) = (f64::from(frame.map.width), f64::from(frame.map.height));
            let (a, b) = (frame.from_map.apply(0.0, 0.0), frame.from_map.apply(1.0, 0.0));
            // Frame units a model unit: the frame's per world unit times the models' world units a unit (their footprint decal, 0.405).
            let k = ((b.0 - a.0).hypot(b.1 - a.1) * w / c.world.0 * c.model_scale) as f32;
            let (cx, cy) = frame.cell_of(f64::from(cam.target.x), f64::from(cam.target.z));
            for (i, slot) in c.slots.iter().enumerate() {
                let Some(model) = &slot.model else { continue };
                let (x, y) = (slot.world.0 / c.world.0 * w, (1.0 - slot.world.1 / c.world.1) * h);
                if (x - cx).hypot(y - cy) > self.radius_cells {
                    continue;
                }

                // Turned as the sprawl is, by its sixths (a port's from its coast; the others 0 until their rule is known).
                let (fx, fz) = frame.from_map.apply(x, y);
                let (s, co) = (slot.turn as f32 * std::f32::consts::FRAC_PI_3).sin_cos();
                // A port stands on the sea's level (capture A: Salona's at y 0, its cell 35 m under the sea): on the sea bed it sank.
                let y = if slot.kind == "port" { ground(fx, fz).max(frame.sea_y()) } else { ground(fx, fz) };
                let mut world = Float4x4::IDENTITY;
                world.m[0] = [co * k, 0.0, s * k, fx as f32];
                world.m[1] = [0.0, k, 0.0, y];
                world.m[2] = [-s * k, 0.0, co * k, fz as f32];
                let p16 = DEFAULT_INSTANCE_PARAMS;
                let mesh = MeshInstance {
                    model: model.clone(),
                    casts_shadow: true,
                    lod: true,
                    p4: [p16[0], p16[1], p16[2], p16[3]],
                    p5: [p16[4], p16[5], p16[6], p16[7]],
                    p6: [p16[8], p16[9], p16[10], p16[11]],
                    p7: [p16[12], p16[13], p16[14], p16[15]],
                    burning: false,
                    faction_colours: c.owners.get(&slot.region).and_then(|o| flags.get(o)).copied(),
                };
                out.push((CITY_KEY | i as u64, world, Some(Payload::MeshInstance(mesh)), u64::MAX));
                self.city_count += 1;
            }

            // The aqueducts of the regions with an aqueduct building, turned clockwise (seen from above) by their rotation, on the ground.
            for (i, aq) in c.aqueducts.iter().enumerate() {
                let (x, y) = (aq.world.0 / c.world.0 * w, (1.0 - aq.world.1 / c.world.1) * h);
                if (x - cx).hypot(y - cy) > self.radius_cells {
                    continue;
                }

                let (fx, fz) = frame.from_map.apply(x, y);
                let (s, co) = (aq.rotation as f32).sin_cos();
                let mut world = Float4x4::IDENTITY;
                world.m[0] = [co * k, 0.0, s * k, fx as f32];
                world.m[1] = [0.0, k, 0.0, ground(fx, fz)];
                world.m[2] = [-s * k, 0.0, co * k, fz as f32];
                let p16 = DEFAULT_INSTANCE_PARAMS;
                let mesh = MeshInstance {
                    model: aq.model.clone(),
                    casts_shadow: true,
                    lod: true,
                    p4: [p16[0], p16[1], p16[2], p16[3]],
                    p5: [p16[4], p16[5], p16[6], p16[7]],
                    p6: [p16[8], p16[9], p16[10], p16[11]],
                    p7: [p16[12], p16[13], p16[14], p16[15]],
                    burning: false,
                    faction_colours: None,
                };
                out.push((CITY_KEY | AQUEDUCT_KEY | i as u64, world, Some(Payload::MeshInstance(mesh)), u64::MAX));
                self.city_count += 1;
            }

            // The sprawl pieces, turned clockwise (seen from above) by their sixths (the hex's neighbours go clockwise from north,
            // RPFM's `coords.rs`; ASSUMPTION: a piece's model has its first edge north).
            for (i, sp) in c.sprawl.iter().enumerate() {
                let Some(model) = &sp.model else { continue };
                let (x, y) = (sp.world.0 / c.world.0 * w, (1.0 - sp.world.1 / c.world.1) * h);
                if (x - cx).hypot(y - cy) > self.radius_cells {
                    continue;
                }

                let (fx, fz) = frame.from_map.apply(x, y);
                let (s, co) = (sp.turn as f32 * std::f32::consts::FRAC_PI_3).sin_cos();
                let mut world = Float4x4::IDENTITY;
                world.m[0] = [co * k, 0.0, s * k, fx as f32];
                world.m[1] = [0.0, k, 0.0, ground(fx, fz)];
                world.m[2] = [-s * k, 0.0, co * k, fz as f32];
                let p16 = DEFAULT_INSTANCE_PARAMS;
                let mesh = MeshInstance {
                    model: model.clone(),
                    casts_shadow: true,
                    lod: true,
                    p4: [p16[0], p16[1], p16[2], p16[3]],
                    p5: [p16[4], p16[5], p16[6], p16[7]],
                    p6: [p16[8], p16[9], p16[10], p16[11]],
                    p7: [p16[12], p16[13], p16[14], p16[15]],
                    burning: false,
                    faction_colours: c.owners.get(&sp.region).and_then(|o| flags.get(o)).copied(),
                };
                out.push((CITY_KEY | SPRAWL_KEY | i as u64, world, Some(Payload::MeshInstance(mesh)), u64::MAX));
                self.city_count += 1;
            }
        }

        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.layer = e.scene.add_layer(doc, "prop models");
            self.drawn.clear();
            self.trees_drawn.clear();
        }

        wce_foundation::wce_prof!("props.scene");
        // A `.terry` edit (or another document): every model again; a camera move: only the difference.
        // Removed in one pass (the scene's `remove` moves its whole list each time: 55 ms a rebuild).
        let mut gone: Vec<std::ops::Range<EntityId>> = vec![];
        if again {
            gone.extend(self.drawn.drain().map(|(_, ent)| ent..ent + 1));
        } else if ground_again {
            // The settlements' buildings on the ground as it is now.
            self.drawn.retain(|k, ent| {
                let city = k & CITY_KEY != 0;
                if city {
                    gone.push(*ent..*ent + 1);
                }

                !city
            });
        }

        if trees_all {
            gone.extend(self.trees_drawn.drain().map(|(_, ent)| ent..ent + 1));
        }

        for i in trees_gone {
            gone.extend(self.trees_drawn.remove(&i).map(|ent| ent..ent + 1));
        }

        let keep: wce_foundation::id_hash::IdSet<u64> = out.iter().map(|o| o.0).collect();
        self.drawn.retain(|k, ent| {
            let stay = keep.contains(k);
            if !stay {
                gone.push(*ent..*ent + 1);
            }

            stay
        });
        e.scene.remove_ranges(&gone);

        self.count = out.len() - self.city_count - self.effect_count;
        for (k, world, payload, item) in out {
            let Some(payload) = payload.filter(|_| !self.drawn.contains_key(&k)) else { continue };

            let ent = e.scene.add(self.layer, world, payload, SourceRef { doc, item });
            // The trees can't be picked (nothing of the document's to select).
            if item == u64::MAX
                && let Some(en) = e.scene.find_mut(ent)
            {
                en.locked = true;
            }

            self.drawn.insert(k, ent);
        }

        // The trees can't be picked (nothing of the document's to select).
        for (i, world, payload) in trees_new {
            let ent = e.scene.add(self.layer, world, payload, SourceRef { doc, item: u64::MAX });
            if let Some(en) = e.scene.find_mut(ent) {
                en.locked = true;
            }

            self.trees_drawn.insert(i, ent);
        }

        self.tree_count = self.trees_drawn.len();
        self.animated = animated;
        self.built = Some(key);
        (self.built_full, self.built_ground) = (Some((doc, full)), Some(ground_rev));
    }

    /// The props that move, posed for the editor's clock: their clips loaded as they come (once a model), each at a phase of its own.
    pub fn pose_animated(&mut self, e: &mut Editor, doc: DocumentId) {
        if self.layer == 0 || e.scene.layer(self.layer).is_none() {
            self.animated_drawn.clear();
            self.animated_count = 0;
            return;
        }
        let files = e.files();
        let read = |p: &str| files.as_ref().and_then(|f| f.read(p).ok()).map(|b| b.to_vec());
        let keep: std::collections::HashSet<u64> = self.animated.iter().map(|a| a.0).collect();
        let gone: Vec<std::ops::Range<EntityId>> =
            self.animated_drawn.iter().filter(|(k, _)| !keep.contains(k)).map(|(_, &ent)| ent..ent + 1).collect();
        self.animated_drawn.retain(|k, _| keep.contains(k));
        e.scene.remove_ranges(&gone);
        let time = e.clock.time() as f32;
        let mut count = 0;
        for (key, world, model) in &self.animated {
            let Some(clip) = self
                .clips
                .entry(model.clone())
                .or_insert_with(|| wce_render::animated_prop::AnimatedProp::load(&read, model).map(Arc::new))
                .clone()
            else {
                continue;
            };
            let ent = *self.animated_drawn.entry(*key).or_insert_with(|| {
                let ent =
                    e.scene.add(self.layer, Float4x4::IDENTITY, Payload::CameraRig(Default::default()), SourceRef { doc, item: u64::MAX });
                if let Some(en) = e.scene.find_mut(ent) {
                    en.locked = true;
                }
                ent
            });
            // Not in step: a phase from the key.
            let phase = (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32 * clip.duration();
            e.scene.update_pose(ent, *world, clip.pose(time + phase, true));
            count += 1;
        }
        self.animated_count = count;
    }

    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }

        (self.layer, self.built, self.count) = (0, None, 0);
        (self.built_full, self.built_ground, self.tree_disc) = (None, None, None);
        self.drawn.clear();
        self.trees_drawn.clear();
        self.animated_drawn.clear();
        self.animated.clear();
    }
}

/// A campaign's settlements and their owners' flag colours.
struct Settlements {
    cities: wce_terrain_io::attila::cities::Cities,

    /// By faction: what its settlements' flags take (`GpuMesh::flag`, the walls' and gatehouses' `Cloth` blocks): its `faction_banners`
    /// primary, secondary and tertiary, linear, and black (capture V2: Gwined's FCFCFC / A91400 / 000000 / 0, Northymbre's, ...).
    flags: HashMap<String, FactionColours>,
}

/// The factions' flag colours (`Settlements::flags`).
fn flag_colours(
    schema: &rpfm_lib::schema::Schema,
    list: &dyn Fn(&str) -> Vec<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> HashMap<String, FactionColours> {
    let linear = |hex: Option<&String>| -> [f32; 3] {
        let hex = hex.map_or("", |h| h.trim().trim_start_matches('#'));
        std::array::from_fn(|i| {
            let c = hex.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map_or(0.0, |b| f32::from(b) / 255.0);
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        })
    };
    let mut out = HashMap::new();
    for r in wce_terrain_io::attila::cities::table_rows(schema, "faction_banners_tables", list, read) {
        let Some(key) = r.get("key") else { continue };
        let colours = [linear(r.get("primary_hex")), linear(r.get("secondary_hex")), linear(r.get("tertiary_hex")), [0.0; 3]];
        out.entry(key.clone()).or_insert(FactionColours { colours, flags_only: true });
    }
    out
}

/// The trees' profile slot (a scope ended before the function's end).
static TREES_SLOT: std::sync::OnceLock<&'static wce_foundation::profile::Slot> = std::sync::OnceLock::new();
