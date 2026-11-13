//! A battle map as an editor document: where it comes from, its model (a kit map's folder read into `wce_terrain::MapModel`; a map of the
//! packs has only its built files, shown read-only), the placements it shows (BOB's fill of a kit map, the packs' tile map) and its battle
//! locations. What the map looks like is not published by the document: the Terrain tool streams it around the camera on its own layer
//! (`wce_map_view::MapView`).

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wce_editor_core::command::{CommandStack, History};
use wce_editor_core::document::{Document, DocumentCore, SaveTarget};
use wce_scene::render_scene::{LayerId, RenderScene};
use wce_terrain::MapModel;
use wce_tile::battle_map::context::PlacedMap;
use wce_tile::battle_map::locations::BattleLocations;
use wce_tile::battle_map::tile_db::TileDatabase;

/// Where a map comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapSource {
    /// A kit layout's map folder (`raw_data/terrain/battles/<map>`): the projects' or an Assembly Kit's.
    Kit(PathBuf),

    /// A folder of the game's packs (`terrain/battles/<map>`): its built files, read-only.
    Packs(String),
}

pub struct MapDocument {
    core: DocumentCore,
    history: CommandStack<Option<MapModel>>,

    /// The map's name as the map list shows it.
    pub name: String,
    pub source: MapSource,
    pub placed: Arc<PlacedMap>,
    pub locations: Option<BattleLocations>,

    /// The tile database the map was filled with (its sets', tiles' and climates' colours name the cells).
    pub db: Arc<TileDatabase>,

    /// Where Save writes the map's folder (the projects' kit layout: `<projects>/assembly_kit/raw_data/terrain/battles/<map>`, the first
    /// place every loader looks), and what the last save said.
    pub save_dir: Option<PathBuf>,
    message: Option<String>,

    /// The cells its layer edits changed, for the views.
    pub cells: Arc<crate::cells::CellWatch>,

    /// BOB's messages of the last fill ("Failed to find tile for point x x y": a cell no tile fits).
    pub fill_messages: Vec<String>,

    /// The `.terry` scene: its revision (moves with every edit), the view's metres a scene unit, and the gizmo's gesture (its id, the
    /// items' positions when it started, the move so far: metres).
    pub terry_revision: crate::terry::TerryRevision,
    /// How the `.terry`'s coordinates sit on the view's frame.
    pub terry_frame: TerryFrame,
    gesture: Option<Gesture>,

    /// A campaign's roads at its start (the levels its regions' buildings give them): the view draws the road tiles upgraded so
    /// ([`Self::shown_placed`]); `placed`, what is edited and saved, keeps the kit's.
    pub view_roads: Option<Arc<wce_terrain_io::attila::roads::CampaignRoads>>,

    /// Whether the view draws the roads at the start's levels (else the kit's tiles).
    pub roads_at_start: bool,
}

/// How a map's `.terry` coordinates sit on the view's frame (scene units, 128 a cell, the map's centre at 0): `frame = (terry - origin)
/// / unit` in x and z; `cell`: a map cell in `.terry` units. A battle map's `.terry` is in metres from the map's centre, 256 a cell
/// (docs/re/terry.md); a campaign map's from its bottom-left corner, z north, 0.32 a cell (the campaign database's `unit_scale` 0.2 x
/// 1.6: its props' x reach 649.9 on a map 2032 cells wide, 650.24; the game's `prop_marker.markers` has the same numbers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerryFrame {
    pub unit: f64,
    pub origin: [f64; 2],
    pub cell: f64,

    /// The `.terry`'s y is not the view's height (the campaign's): entities stand on the LF.
    pub y_from_lf: bool,
}

impl TerryFrame {
    pub const BATTLE: Self = Self { unit: 2.0, origin: [0.0, 0.0], cell: TERRY_CELL, y_from_lf: false };

    /// A campaign map `w` x `h` cells.
    pub fn campaign(w: u32, h: u32) -> Self {
        let cell = 0.32;
        Self { unit: cell / 128.0, origin: [f64::from(w) / 2.0 * cell, f64::from(h) / 2.0 * cell], cell, y_from_lf: true }
    }

    /// A `.terry` point (x, z) in the frame.
    pub fn to_frame(&self, x: f64, z: f64) -> (f64, f64) {
        ((x - self.origin[0]) / self.unit, (z - self.origin[1]) / self.unit)
    }

    /// A frame point (x, z) in the `.terry`'s coordinates.
    pub fn from_frame(&self, x: f64, z: f64) -> (f64, f64) {
        (x * self.unit + self.origin[0], z * self.unit + self.origin[1])
    }

    /// A `.terry` point's map cell (x right, y down) on a map `w` x `h` cells (its centre at `w / 2`, `h / 2`, whole cells).
    pub fn cell_of(&self, x: f64, z: f64, w: u32, h: u32) -> (f64, f64) {
        ((x - self.origin[0]) / self.cell + f64::from(w / 2), f64::from(h / 2) - (z - self.origin[1]) / self.cell)
    }

    /// A map cell's point in the `.terry`'s coordinates.
    pub fn of_cell(&self, cx: f64, cy: f64, w: u32, h: u32) -> (f64, f64) {
        ((cx - f64::from(w / 2)) * self.cell + self.origin[0], (f64::from(h / 2) - cy) * self.cell + self.origin[1])
    }

    /// A coordinate snapped to the cell grid (`k`: 0 x, 1 z).
    pub fn snap(&self, v: f64, k: usize) -> f64 {
        ((v - self.origin[k]) / self.cell).round() * self.cell + self.origin[k]
    }
}

/// A gizmo gesture on `.terry` entities: its id, the items' positions when it started, the move so far (metres).
type Gesture = (u64, std::collections::HashMap<u64, [f64; 3]>, [f64; 3]);

/// Cells a side of the map's grid in `.terry` metres (docs/re/terry.md).
pub const TERRY_CELL: f64 = 256.0;

/// The values offered for an enumerated `.terry` attribute (the file's own values are added to them).
/// Placements as the view draws them: with a campaign's roads ([`MapDocument::view_roads`]) their road tiles upgraded to their level's.
pub fn shown_placed(placed: &Arc<PlacedMap>, roads: Option<&wce_terrain_io::attila::roads::CampaignRoads>) -> Arc<PlacedMap> {
    match roads {
        Some(r) => Arc::new(r.apply(placed).0),
        None => placed.clone(),
    }
}

fn choices_of(attr: &str) -> &'static [&'static str] {
    match attr {
        "catchment_type" => &[
            "land_normal",
            "land_ambush",
            "land_bridge",
            "coastal_battle",
            "naval_normal",
            "settlement_standard",
            "settlement_unfortified",
            "port_assault",
            "unfortified_port",
        ],
        "deployment_category" => &["DZC_RIVER", "DZC_COAST"],
        "deployment_type" => &["DZT_1V1"],
        "region_type" => &["DZRT_ADDITIVE", "DZRT_SUBTRACTIVE"],
        _ => &[],
    }
}

impl MapDocument {
    pub fn new(
        name: String,
        source: MapSource,
        model: Option<MapModel>,
        placed: Arc<PlacedMap>,
        locations: Option<BattleLocations>,
        db: Arc<TileDatabase>,
    ) -> Self {
        Self {
            core: DocumentCore::new(),
            history: CommandStack::new(model),
            name,
            source,
            placed,
            locations,
            db,
            save_dir: None,
            message: None,
            cells: Arc::default(),
            fill_messages: vec![],
            terry_revision: Arc::default(),
            terry_frame: TerryFrame::BATTLE,
            gesture: None,
            view_roads: None,
            roads_at_start: true,
        }
    }

    /// The placements as the view draws them: a campaign's road tiles at its start's levels (`view_roads`), else `placed`.
    pub fn shown_placed(&self) -> Arc<PlacedMap> {
        shown_placed(&self.placed, self.start_roads().as_deref())
    }

    /// The campaign's roads at its start when the view draws them.
    pub fn start_roads(&self) -> Option<Arc<wce_terrain_io::attila::roads::CampaignRoads>> {
        self.view_roads.clone().filter(|_| self.roads_at_start)
    }

    /// The map's files as data (a kit map; none for a map of the packs).
    pub fn model(&self) -> Option<&MapModel> {
        self.history.model().as_ref()
    }

    /// A field of the model to paint on (a kit map's LF; none for a map of the packs). The dabs of a stroke change it directly (the
    /// file is marked edited); the finished stroke goes on the history with `push_stroke`.
    pub fn lf_field_mut(&mut self, role: wce_terrain::LfRole) -> Option<&mut wce_terrain::paged::PagedU16> {
        self.history.model_mut().as_mut().and_then(|m| m.lf_mut(role)).map(|item| item.edit())
    }

    pub fn lf_field(&self, role: wce_terrain::LfRole) -> Option<&wce_terrain::paged::PagedU16> {
        self.model().and_then(|m| m.lf(role)).map(|item| item.get())
    }

    /// The `.terry` scene (a kit map's).
    pub fn project(&self) -> Option<&wce_terrain::terry::TerryProject> {
        self.model().and_then(|m| m.project.as_ref()).map(|p| p.get())
    }

    /// The `.terry` scene to change (the file is marked edited); a structural edit goes on the history with `push_terry` after.
    pub fn project_mut(&mut self) -> Option<&mut wce_terrain::terry::TerryProject> {
        self.history.model_mut().as_mut().and_then(|m| m.project.as_mut()).map(|p| p.edit())
    }

    /// A `.terry` edit onto the history (applied by pushing it, or already done: `TerrySpliceCommand::done`).
    pub fn push_terry(&mut self, cmd: Box<dyn wce_editor_core::command::Command<Option<MapModel>>>) {
        self.history.push(cmd);
        self.terry_revision.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// An entity's position in the view's frame (scene units).
    pub fn frame_position(&self, id: u64) -> Option<wce_foundation::math::Float3> {
        let p = self.project()?;
        p.entity(id)?;
        let w = p.world_position(id);
        let t = self.terry_frame;
        let (x, z) = t.to_frame(w[0], w[2]);
        // A campaign map's entities on its LF (its metres over 0.2: `wce_tile::tile_project::height_unit`); a battle map's `.terry` y.
        let y = if t.y_from_lf { self.lf_metres_at(w[0], w[2]) / 0.2 } else { w[1] / t.unit.max(1e-9) };
        Some(wce_foundation::math::Float3::new(x as f32, y as f32, z as f32))
    }

    /// The LF height (metres) under a `.terry` point (metres from the map's centre).
    pub fn lf_metres_at(&self, x: f64, z: f64) -> f64 {
        let (cx, cy) = self.terry_frame.cell_of(x, z, self.placed.width, self.placed.height);
        self.placed.lf_metres(cx, cy)
    }

    /// The `.terry` y of a new entity standing at a point: a battle map's the LF's metres; a campaign map's as the kit's props have
    /// theirs, 0.0025 an LF metre (the lowest on their ground rise 0.0022 a metre: not the drawn terrain's scale, which is 5 times that;
    /// the game stands `ECLandscapeAbove` props on the ground). ASSUMPTION.
    pub fn terry_y_at(&self, x: f64, z: f64) -> f64 {
        let m = self.lf_metres_at(x, z);
        if !self.terry_frame.y_from_lf {
            return m;
        }

        m * self.terry_frame.unit
    }

    /// A cell layer of the model to paint on (its index in `MapModel::layers`; the file is marked edited); the finished stroke goes on the
    /// history with `push_cells`.
    pub fn cell_layer_mut(&mut self, k: usize) -> Option<&mut wce_terrain::cells::CellLayer> {
        self.history.model_mut().as_mut().and_then(|m| m.layers.get_mut(k)).map(|(_, item)| item.edit())
    }

    pub fn cell_layer(&self, k: usize) -> Option<&wce_terrain::cells::CellLayer> {
        self.model().and_then(|m| m.layers.get(k)).map(|(_, item)| item.get())
    }

    /// A finished cell stroke onto the history (in the layer already).
    pub fn push_cells(&mut self, cmd: crate::cells::CellStrokeCommand) {
        use wce_editor_core::command::History;
        self.history.push(Box::new(cmd));
        self.history.break_merge();
    }

    /// An edit of the explicit tiles onto the history (applied by pushing it).
    pub fn push_explicit(&mut self, cmd: crate::cells::ExplicitCommand) {
        use wce_editor_core::command::History;
        self.history.push(Box::new(cmd));
        self.history.break_merge();
    }

    /// An edit of a built map's placements onto the history (applied).
    pub fn push_tile_list(&mut self, cmd: crate::cells::TileListCommand) {
        use wce_editor_core::command::History;
        self.history.push(Box::new(cmd));
        self.history.break_merge();
    }

    /// A finished stroke onto the history (it is in the field already: applying it again changes nothing).
    pub fn push_stroke(&mut self, cmd: crate::lf::LfStrokeCommand) {
        use wce_editor_core::command::History;
        self.history.push(Box::new(cmd));
        self.history.break_merge();
    }

    /// A line for the panel: the map's size and what it holds.
    pub fn summary(&self) -> String {
        let m = &self.placed;
        let mut s = format!("{} x {} cells, {} placements", m.width, m.height, m.placements.len());
        if let Some(lf) = &m.lf {
            s += &format!(", LF {} x {}", lf.w, lf.h);
        }

        if let Some(l) = &self.locations {
            s += &format!(", {} battle locations", l.types.iter().map(|(_, v)| v.len()).sum::<usize>());
        }

        if let Some(p) = self.model().and_then(|m| m.project.as_ref()) {
            s += &format!(", {} .terry entities", p.get().entities.len());
        }

        s
    }
}

impl Document for MapDocument {
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
        self.name.clone()
    }

    fn kind(&self) -> String {
        "battle_map".to_owned()
    }

    /// Nothing of its own: the tool's map view is on the scene.
    fn publish(&mut self, _scene: &mut RenderScene, _layer: LayerId) {}

    fn can_save(&self) -> bool {
        self.model().is_some() && self.save_dir.is_some()
    }

    fn save_message(&self) -> Option<String> {
        self.message.clone()
    }

    /// The map's folder into the projects' kit layout: the files made again from the model where they were edited (the others as they
    /// were: copied the first time, then left when the folder has them at the same size).
    fn save(&mut self, target: &SaveTarget) -> Result<(), String> {
        let dir = if target.path.as_os_str().is_empty() {
            self.save_dir.clone().ok_or("no projects folder to save into")?
        } else {
            target.path.clone()
        };
        let model = self.model().ok_or("a map of the game's packs is read-only")?;
        let files = if model.built { wce_terrain_io::built::write(model, Some(&self.db)) } else { wce_terrain_io::attila::write(model) }
            .map_err(|e| e.to_string())?;
        let (mut written, mut kept) = (0, 0);
        for (rel, blob) in &files.folder {
            let to = dir.join(rel);
            if let wce_tile_model::Blob::File(src) = blob {
                let same_size = std::fs::metadata(src).ok().zip(std::fs::metadata(&to).ok()).is_some_and(|(a, b)| a.len() == b.len());
                if *src == to || same_size {
                    kept += 1;
                    continue;
                }
            }

            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }

            // An LF whose samples are a view of this very file (a map opened from the projects folder, saved back): its samples into
            // memory first (a file with a view of it can't be rewritten on Windows; on Unix the write would reach the view).
            for (_, item) in &model.lf {
                if matches!(&item.origin, wce_tile_model::Blob::File(src) if *src == to) {
                    item.get().grid().detach();
                }
            }

            // Renamed over the file, never written in place: another document's view of it (the map opened twice) keeps the old file on Unix,
            // where truncating it would kill the process when that document reads its LF. Windows refuses, as it refuses a write.
            let temp = to.with_added_extension("wce_tmp");
            match blob {
                wce_tile_model::Blob::File(src) => std::fs::copy(src, &temp).map(|_| ()),
                wce_tile_model::Blob::Bytes(b) => std::fs::write(&temp, b),
            }
            .and_then(|()| std::fs::rename(&temp, &to))
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                format!("{}: {e}", to.display())
            })?;
            written += 1;
        }

        // A copy elsewhere (Save as) leaves the projects' folder without the edits: the map stays unsaved.
        if self.save_dir.as_deref().is_some_and(|own| wce_editor_core::document::same_path(own, &dir)) {
            self.history.mark_saved();
        }

        self.message = Some(format!("{}: {written} files written, {kept} already there", dir.display()));
        Ok(())
    }

    fn source_key(&self) -> String {
        match &self.source {
            MapSource::Packs(folder) => folder.to_lowercase(),
            MapSource::Kit(_) => String::new(),
        }
    }

    fn opened_from(&self, path: &str) -> bool {
        let MapSource::Kit(dir) = &self.source else { return false };
        let p = Path::new(path);
        p == dir || p.parent() == Some(dir.as_path())
    }

    fn shows_scene(&self) -> bool {
        true
    }

    fn describe(&self, item: wce_scene::render_scene::SourceRef) -> String {
        let Some(p) = self.project() else { return String::new() };
        p.entity(item.item).map_or_else(String::new, |e| format!("{} ({})", e.name, p.kind(e).name()))
    }

    /// An entity's name and its components' attributes: numbers, vectors, switches, the enumerated ones as choices.
    /// Several entities: the components they all have, each attribute they all have (the first's value, with how many differ); an edit
    /// sets it on all of them, one step on the history. Three-number attributes (positions, rotations) are shown, not edited.
    fn inspect_many(&mut self, items: &[wce_scene::render_scene::SourceRef], ins: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        use crate::terry::{TerryAttrCommand, TerryAttrsCommand};
        let Some(p) = self.project() else { return false };
        let ents: Vec<wce_terrain::terry::TerryEntity> = items.iter().filter_map(|s| p.entity(s.item).cloned()).collect();
        if ents.len() < 2 {
            return false;
        }

        let revision = self.terry_revision.clone();
        ins.section(&format!("{} entities", ents.len()));
        let mut kinds: Vec<String> = ents.iter().map(|e| p.kind(e).name()).collect();
        kinds.sort();
        kinds.dedup();
        ins.text("kinds", "kinds", &kinds.join(", "));
        let mut edit: Option<(String, String, String)> = None;
        for (cname, c) in &ents[0].components {
            let all: Vec<usize> = ents.iter().filter_map(|e| p.component(e, cname)).collect();
            if all.len() != ents.len() {
                continue;
            }

            ins.section(cname);
            let attrs: Vec<String> = p.doc.elements()[*c].attrs.iter().map(|a| a.name.clone()).collect();
            for a in attrs {
                let values: Vec<Option<String>> = all.iter().map(|&k| p.doc.get(k, &a)).collect();
                if values.iter().any(Option::is_none) {
                    continue;
                }

                let values: Vec<String> = values.into_iter().flatten().collect();
                let v = values[0].clone();
                let differ = values.iter().filter(|x| **x != v).count();
                let label = if differ == 0 { a.clone() } else { format!("{a} ({differ} differ)") };
                let field = format!("many.{cname}.{a}");
                let nums: Vec<f64> = v.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                let all_numbers = !v.is_empty() && nums.len() == v.split_whitespace().count();
                let mut set = |after: String| edit = Some((cname.clone(), a.clone(), after));
                if v == "true" || v == "false" {
                    let mut b = v == "true";
                    if ins.edit_bool(&field, &label, &mut b) {
                        set(b.to_string());
                    }
                } else if !choices_of(&a).is_empty() {
                    let mut list: Vec<String> = choices_of(&a).iter().map(|s| (*s).to_owned()).collect();
                    if !list.contains(&v) {
                        list.push(v.clone());
                    }

                    let mut k = list.iter().position(|x| *x == v).unwrap_or(0) as i32;
                    if ins.edit_choice(&field, &label, &mut k, &list) {
                        set(list[k.max(0) as usize].clone());
                    }
                } else if all_numbers && nums.len() == 1 {
                    let mut f = nums[0] as f32;
                    if ins.edit_float(&field, &label, &mut f, 1.0, -1.0e9, 1.0e9) {
                        set(wce_terrain::terry::number_text(f64::from(f)));
                    }
                } else if all_numbers {
                    ins.text(&field, &label, &v);
                } else {
                    let mut t = v.clone();
                    if ins.edit_string(&field, &label, &mut t) {
                        set(t);
                    }
                }
            }
        }

        let ended = ins.gesture_ended();
        if let Some((component, attr, after)) = edit {
            let mut key: u64 = 0xcbf2_9ce4_8422_2325;
            for b in component.bytes().chain(attr.bytes()).chain(ents.iter().flat_map(|e| e.id.to_le_bytes())) {
                key = (key ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }

            let edits = ents
                .iter()
                .filter_map(|e| {
                    let c = p.component(e, &component)?;
                    let before = p.doc.get(c, &attr)?;
                    Some(TerryAttrCommand {
                        id: e.id,
                        component: component.clone(),
                        attr: attr.clone(),
                        before,
                        after: after.clone(),
                        key: 0,
                        revision: revision.clone(),
                    })
                })
                .collect();
            self.push_terry(Box::new(TerryAttrsCommand { edits, key: key | 1 }));
        }

        if ended {
            self.history.break_merge();
        }

        true
    }

    fn inspect(&mut self, item: wce_scene::render_scene::SourceRef, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        use crate::terry::TerryAttrCommand;
        let Some(p) = self.project() else { return };
        let Some(e) = p.entity(item.item).cloned() else { return };
        let revision = self.terry_revision.clone();
        let key_of = |c: &str, a: &str| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ item.item;
            for b in c.bytes().chain(a.bytes()) {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }

            h | 1
        };

        let mut edits: Vec<TerryAttrCommand> = vec![];
        ins.section(&format!("{} ({})", e.name, p.kind(&e).name()));
        let mut name = e.name.clone();
        if ins.edit_string("name", "name", &mut name) {
            edits.push(TerryAttrCommand {
                id: e.id,
                component: String::new(),
                attr: "name".to_owned(),
                before: e.name.clone(),
                after: name,
                key: key_of("", "name"),
                revision: revision.clone(),
            });
        }

        ins.text("id", "id", &format!("{:x}", e.id));
        let w = p.world_position(e.id);
        ins.text("world", "on the map (m)", &format!("{:.1}, {:.1}, {:.1}", w[0], w[1], w[2]));
        for (cname, c) in &e.components {
            ins.section(cname);
            let attrs: Vec<(String, String)> =
                p.doc.elements()[*c].attrs.iter().map(|a| (a.name.clone(), p.doc.get(*c, &a.name).unwrap_or_default())).collect();
            for (a, v) in attrs {
                let field = format!("{cname}.{a}");
                let mut push = |after: String| {
                    edits.push(TerryAttrCommand {
                        id: e.id,
                        component: cname.clone(),
                        attr: a.clone(),
                        before: v.clone(),
                        after,
                        key: key_of(cname, &a),
                        revision: revision.clone(),
                    })
                };
                let nums: Vec<f64> = v.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                let all_numbers = !v.is_empty() && nums.len() == v.split_whitespace().count();
                if v == "true" || v == "false" {
                    let mut b = v == "true";
                    if ins.edit_bool(&field, &a, &mut b) {
                        push(b.to_string());
                    }
                } else if !choices_of(&a).is_empty() {
                    let mut list: Vec<String> = choices_of(&a).iter().map(|s| (*s).to_owned()).collect();
                    if !list.contains(&v) {
                        list.push(v.clone());
                    }

                    let mut k = list.iter().position(|x| *x == v).unwrap_or(0) as i32;
                    if ins.edit_choice(&field, &a, &mut k, &list) {
                        push(list[k.max(0) as usize].clone());
                    }
                } else if all_numbers && nums.len() == 3 {
                    let mut f = [nums[0] as f32, nums[1] as f32, nums[2] as f32];
                    if ins.edit_float3(&field, &a, &mut f, 1.0) {
                        push(f.iter().map(|&x| wce_terrain::terry::number_text(f64::from(x))).collect::<Vec<_>>().join(" "));
                    }
                } else if all_numbers && nums.len() == 1 {
                    let mut f = nums[0] as f32;
                    if ins.edit_float(&field, &a, &mut f, 1.0, -1.0e9, 1.0e9) {
                        push(wce_terrain::terry::number_text(f64::from(f)));
                    }
                } else {
                    let mut t = v.clone();
                    if ins.edit_string(&field, &a, &mut t) {
                        push(t);
                    }
                }
            }

            if cname == "ECPolyline"
                && let Some((pts, closed)) = p.polyline(&e)
            {
                ins.text(&format!("{cname}.points"), "points", &format!("{} ({})", pts.len(), if closed { "closed" } else { "open" }));
            }
        }

        let ended = ins.gesture_ended();
        for cmd in edits {
            self.push_terry(Box::new(cmd));
        }

        if ended {
            self.history.break_merge();
        }
    }

    fn item_transform(&self, item: wce_scene::render_scene::SourceRef) -> Option<wce_foundation::math::Float4x4> {
        let p = self.frame_position(item.item)?;
        let mut m = wce_foundation::math::Float4x4::IDENTITY;
        (m.m[0][3], m.m[1][3], m.m[2][3]) = (p.x, p.y, p.z);
        Some(m)
    }

    /// Moves entities by the gizmo's delta: an `ECTileAlignment` one snapped to the cell grid (256 m), a root `ECLandscapeAbove` one put on
    /// the LF; one undo step a gesture.
    fn transform_items(&mut self, items: &[wce_scene::render_scene::SourceRef], delta: &wce_foundation::math::Float4x4, gesture: u64) {
        use crate::terry::TerryAttrCommand;
        let u = self.terry_frame.unit;
        let d = [f64::from(delta.m[0][3]) * u, f64::from(delta.m[1][3]) * u, f64::from(delta.m[2][3]) * u];
        if self.gesture.as_ref().is_none_or(|g| g.0 != gesture) {
            let starts = self
                .project()
                .map(|p| items.iter().filter_map(|i| Some((i.item, p.local_position(p.entity(i.item)?)?))).collect())
                .unwrap_or_default();
            self.gesture = Some((gesture, starts, [0.0; 3]));
        }

        let Some(g) = self.gesture.as_mut() else { return };
        g.2 = [g.2[0] + d[0], g.2[1] + d[1], g.2[2] + d[2]];
        let (acc, starts) = (g.2, g.1.clone());
        let Some(p) = self.project() else { return };
        let mut cmds = vec![];
        for (id, start) in starts {
            let Some(e) = p.entity(id) else { continue };
            let parent = p.transform_parent_of(id).map_or([0.0; 3], |q| p.world_position(q));
            let mut l = [start[0] + acc[0], start[1] + acc[1], start[2] + acc[2]];
            if p.component(e, "ECTileAlignment").is_some() {
                for k in [0, 2] {
                    l[k] = self.terry_frame.snap(parent[k] + l[k], k / 2) - parent[k];
                }
            }

            if p.component(e, "ECLandscapeAbove").is_some() && p.transform_parent_of(id).is_none() {
                l[1] = self.lf_metres_at(l[0], l[2]);
            }

            let Some(t) = p.component(e, "ECTransform") else { continue };
            let before = p.doc.get(t, "position").unwrap_or_default();
            let after = l.iter().map(|&v| wce_terrain::terry::number_text(v)).collect::<Vec<_>>().join(" ");
            if after != before {
                cmds.push(TerryAttrCommand {
                    id,
                    component: "ECTransform".to_owned(),
                    attr: "position".to_owned(),
                    before,
                    after,
                    key: gesture ^ id.rotate_left(17) | 1,
                    revision: self.terry_revision.clone(),
                });
            }
        }

        for c in cmds {
            self.push_terry(Box::new(c));
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
