//! The `.terry` scene of the shown kit map in the 3D view (docs/plans/terrain_editor.md, Phase 5): each entity near the camera an overlay
//! on the LF (rectangles, polylines; pickable, so selection, the Outliner, the Inspector and the gizmo reach it through the map
//! document), drawn again when the scene changes or the camera moves on; and Terry's creation modes for battle maps: a battle catchment area
//! (a click: a 2048 m rectangle on the cell grid, with its `Catchment Region`), a deployment zone (clicks for its region's points); for
//! campaign maps a prop, a VFX and an environment volume (a click each, as the kit's are); and delete and duplicate of the selected
//! entities. Each is one step on the map's history.

use std::collections::HashMap;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{
    DocumentId, EntityId, LayerId, Overlay, OverlayVertex, Payload, RenderScene, SourceRef,
    overlay_colour,
};
use wce_terrain::terry::{EntityKind, TerryProject};
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::document::{TERRY_CELL, TerryFrame};
use wce_terrain_editor::terry::TerrySpliceCommand;
use wce_tile_editor::tile_document::context::Frame;

/// What the drawn set is of: document, `.terry` revision, filters (kinds, season), radius, the point it is centred on.
type Built = (DocumentId, u64, ([bool; 4], usize), f32, [f64; 2]);

#[cfg(test)]
mod removal_tests {
    use super::*;
    use wce_editor_core::selection::Selection;
    use wce_scene::render_scene::StructureChange;

    #[test]
    fn batch_removes_only_shown_overlays_and_keeps_source_selection() {
        let mut scene = RenderScene::default();
        let mut view = TerryView::default();
        view.layer = scene.add_layer(7, "terry entities");
        let other_layer = scene.add_layer(8, "unrelated");
        let source = SourceRef { doc: 7, item: 101 };
        let mut keep = vec![];
        for i in 0..6 {
            let id = scene.add(
                view.layer,
                Float4x4::IDENTITY,
                Payload::Overlay(Overlay::default()),
                SourceRef {
                    item: source.item + i,
                    ..source
                },
            );
            view.shown.insert(source.item + i, id);
            // Interleaving unrelated ids catches accidental removal of a whole layer or a contiguous enclosing range.
            keep.push(scene.add(
                other_layer,
                Float4x4::IDENTITY,
                Payload::Overlay(Overlay::default()),
                SourceRef { doc: 8, item: i },
            ));
        }
        view.draft = scene.add(
            view.layer,
            Float4x4::IDENTITY,
            Payload::Overlay(Overlay::default()),
            SourceRef {
                doc: 7,
                item: u64::MAX,
            },
        );
        keep.push(view.draft);
        let retained: Vec<_> = keep
            .iter()
            .map(|&id| scene.find(id).unwrap().clone())
            .collect();
        let layers = scene.layers().to_vec();
        let terrain_revision = scene.terrain_revision();
        let mut cursor = 0;
        scene.structure_since(&mut cursor);
        let mut selection = Selection::default();
        selection.set(source);
        view.remove_shown(&mut scene);
        assert!(view.shown.is_empty());
        assert_eq!(scene.entities(), retained);
        assert_eq!(scene.layers(), layers);
        assert_eq!(scene.terrain_revision(), terrain_revision);
        let changes = scene.structure_since(&mut cursor).unwrap();
        assert_eq!(changes.len(), 6);
        assert!(
            changes
                .iter()
                .all(|c| matches!(c, StructureChange::Removed(_)))
        );
        view.remove_shown(&mut scene);
        assert!(scene.structure_since(&mut cursor).unwrap().is_empty());
        assert_eq!(scene.entities(), retained);
        let rebuilt = scene.add(
            view.layer,
            Float4x4::IDENTITY,
            Payload::Overlay(Overlay::default()),
            source,
        );
        assert_eq!(selection.items(), &[source]);
        assert_eq!(selection.entities(&scene), vec![rebuilt]);
        assert!(scene.pickable(scene.find(rebuilt).unwrap()));
        assert_eq!(scene.find(rebuilt).unwrap().source, source);
    }

    #[test]
    #[ignore = "explicit campaign-scale performance comparison"]
    fn campaign_overlay_removal_timing() {
        let count = 14_644;
        for filler in [0, 60_000] {
            for batch in [false, true] {
                let mut times = vec![];
                for _ in 0..3 {
                    let mut scene = RenderScene::default();
                    let mut view = TerryView::default();
                    view.layer = scene.add_layer(1, "benchmark");
                    for i in 0..filler {
                        scene.add(
                            view.layer,
                            Float4x4::IDENTITY,
                            Payload::Overlay(Overlay::default()),
                            SourceRef { doc: 2, item: i },
                        );
                    }
                    for i in 0..count {
                        let id = scene.add(
                            view.layer,
                            Float4x4::IDENTITY,
                            Payload::Overlay(Overlay::default()),
                            SourceRef { doc: 1, item: i },
                        );
                        view.shown.insert(i, id);
                    }
                    let start = std::time::Instant::now();
                    if batch {
                        view.remove_shown(&mut scene);
                    } else {
                        for (_, id) in view.shown.drain() {
                            scene.remove(id);
                        }
                    }
                    times.push(start.elapsed().as_secs_f64() * 1000.);
                    assert_eq!(scene.entities().len(), filler as usize);
                }
                times.sort_by(f64::total_cmp);
                println!(
                    "overlays={count} unrelated={filler} batch={batch} median_ms={:.3} samples_ms={times:?}",
                    times[1]
                );
            }
        }
    }
}

/// The seasons of the season filter (`ECSeasonMask`'s attributes), after "every season".
pub const SEASONS: [&str; 4] = ["spring", "summer", "autumn", "winter"];

/// Whether an entity shows in a season (`season` 1 .. 4 of [`SEASONS`]; 0: every season): Terry's `SeasonManager`, an `ECSeasonMask`
/// without the season hides it; no mask, every season.
pub fn in_season(p: &TerryProject, e: &wce_terrain::terry::TerryEntity, season: usize) -> bool {
    let Some(name) = season.checked_sub(1).and_then(|k| SEASONS.get(k)) else { return true };
    p.component(e, "ECSeasonMask").is_none_or(|c| p.doc.get(c, name).as_deref() != Some("false"))
}

/// What a click does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Select,
    NewBca,
    NewDeploymentZone,

    /// The selected polyline's points: a click picks a point, the next moves it there; Shift+click on a side adds one; Ctrl+click
    /// removes one.
    EditPoints,

    /// Campaign maps: a prop mesh, a VFX, an environment volume at the click.
    NewProp,
    NewVfx,
    NewEnvironmentVolume,
}

/// The modes of a battle map's and of a campaign map's `.terry`, with their labels.
const BATTLE_MODES: [(Mode, &str); 4] = [
    (Mode::Select, "select (the gizmo moves)"), (Mode::NewBca, "new battle catchment area"), (Mode::NewDeploymentZone, "new deployment zone"),
    (Mode::EditPoints, "edit the selected polyline's points"),
];
const CAMPAIGN_MODES: [(Mode, &str); 4] = [
    (Mode::Select, "select (the gizmo moves)"), (Mode::NewProp, "new prop"), (Mode::NewVfx, "new VFX"), (Mode::NewEnvironmentVolume, "new environment volume"),
];

/// What a campaign creation makes: the prop's model and scale, the VFX's effect, the environment volume's lighting and radii (`.terry`
/// units). Empty ones take the map's most used when the section is drawn.
#[derive(Clone, Debug, Default)]
pub struct NewCampaignEntity {
    pub model: String,
    pub scale: f32,
    pub vfx: String,
    pub lighting: String,
    pub radii: [f32; 2],
}

pub struct TerryView {
    /// Which kinds are drawn: battle catchment areas, their catchment regions, deployment zones' regions, the others.
    pub show: [bool; 4],

    /// Entities within this of the camera's target are drawn (metres).
    pub radius_m: f32,

    /// The season shown (1 .. 4 of [`SEASONS`]; 0: every season).
    pub season: usize,
    pub mode: Mode,

    /// The deployment zone being drawn: its points (metres on the map).
    pub points: Vec<[f64; 2]>,

    /// The point picked to move (entity, index).
    pub picked: Option<(u64, usize)>,

    layer: LayerId,
    shown: HashMap<u64, EntityId>,

    /// What the drawn set is of: document, `.terry` revision, filters, the point it is centred on (metres).
    built: Option<Built>,
    draft: EntityId,

    /// The campaign creations' settings.
    pub new: NewCampaignEntity,
}

impl Default for TerryView {
    fn default() -> Self {
        Self {
            show: [true, true, true, true], radius_m: 40_000.0, season: 0, mode: Mode::Select, points: vec![], picked: None, layer: 0, shown: HashMap::new(), built: None, draft: 0,
            new: NewCampaignEntity { scale: 0.3, radii: [100.0, 150.0], ..Default::default() },
        }
    }
}

/// A kind's filter slot and colour.
fn style(p: &TerryProject, id: u64, kind: &EntityKind) -> (usize, [f32; 4]) {
    match kind {
        EntityKind::Bca(t) if t.starts_with("settlement") => (0, [1.0, 0.3, 0.25, 1.0]),
        EntityKind::Bca(t) if t.contains("port") || t.contains("naval") || t.contains("coast") => (0, [0.3, 0.65, 1.0, 1.0]),
        EntityKind::Bca(_) => (0, [1.0, 0.65, 0.15, 1.0]),
        EntityKind::CatchmentRegion => (1, [0.75, 0.75, 0.75, 0.6]),
        EntityKind::DeploymentRegion => {
            let alliance = p.parent_of(id).and_then(|z| p.entity(z)).and_then(|z| p.component(z, "ECDeploymentZone")).and_then(|c| p.doc.get(c, "alliance_id"));
            (2, if alliance.as_deref() == Some("1") { [0.9, 0.35, 0.85, 1.0] } else { [0.35, 0.9, 0.4, 1.0] })
        }

        EntityKind::DeploymentZone => (2, [0.35, 0.9, 0.4, 1.0]),
        EntityKind::Prop(_) => (3, [0.85, 0.75, 0.55, 0.9]),
        EntityKind::Vfx(_) => (3, [0.4, 0.9, 1.0, 0.9]),
        EntityKind::EnvironmentVolume(_) => (3, [1.0, 0.9, 0.3, 0.9]),
        EntityKind::Other => (3, [1.0, 1.0, 1.0, 0.8]),
    }
}

/// A line along the ground (frame units): the points lifted onto the LF, every segment cut into pieces a cell long at most.
fn on_ground(frame: &Frame, pts: &[[f64; 2]], closed: bool, tf: TerryFrame, colour: u32, out: &mut Vec<OverlayVertex>) {
    let n = pts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let step = tf.cell / 2.0;
    for k in 0..segments {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let pieces = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as usize;
        let at = |t: f64| {
            let (x, z) = tf.to_frame(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
            Float3::new(x as f32, frame.lift(x, z) + 1.0, z as f32)
        };

        for i in 0..pieces {
            out.push(OverlayVertex { p: at(i as f64 / pieces as f64), rgba: colour });
            out.push(OverlayVertex { p: at((i + 1) as f64 / pieces as f64), rgba: colour });
        }
    }
}

/// An entity's overlay, or none (a kind not shown, nothing to draw).
fn overlay(d: &MapDocument, frame: &Frame, id: u64, show: [bool; 4], season: usize) -> Option<Overlay> {
    let p = d.project()?;
    let e = p.entity(id)?;
    let kind = p.kind(e);
    let (slot, c) = style(p, id, &kind);
    if !show[slot] || !in_season(p, e, season) {
        return None;
    }

    let colour = overlay_colour(c[0], c[1], c[2], c[3]);
    let w = p.world_position(id);
    let mut lines = vec![];
    if let Some([rw, rh]) = p.rectangle(e) {
        let (x0, x1, z0, z1) = (w[0] - rw / 2.0, w[0] + rw / 2.0, w[2] - rh / 2.0, w[2] + rh / 2.0);
        on_ground(frame, &[[x0, z0], [x1, z0], [x1, z1], [x0, z1]], true, d.terry_frame, colour, &mut lines);
    }

    if let Some((pts, closed)) = p.polyline(e) {
        let pts: Vec<[f64; 2]> = pts.iter().map(|q| [w[0] + q[0], w[2] + q[1]]).collect();
        on_ground(frame, &pts, closed, d.terry_frame, colour, &mut lines);
    }

    // An environment volume: its inner and outer radius.
    if let Some(c) = p.component(e, "ECEnvironmentVolume") {
        for attr in ["inner_radius", "outer_radius"] {
            let r: f64 = p.doc.get(c, attr).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
            if r > 0.0 {
                let ring: Vec<[f64; 2]> = (0..48).map(|k| {
                    let a = f64::from(k) / 48.0 * std::f64::consts::TAU;
                    [w[0] + r * a.cos(), w[2] + r * a.sin()]
                }).collect();
                on_ground(frame, &ring, true, d.terry_frame, colour, &mut lines);
            }
        }
    }

    if lines.is_empty() {
        // A spot: a small cross.
        let r = d.terry_frame.cell / 4.0;
        on_ground(frame, &[[w[0] - r, w[2]], [w[0] + r, w[2]]], false, d.terry_frame, colour, &mut lines);
        on_ground(frame, &[[w[0], w[2] - r], [w[0], w[2] + r]], false, d.terry_frame, colour, &mut lines);
    }

    Some(Overlay { lines, depth_test: false, ..Default::default() })
}

impl TerryView {
    fn remove_shown(&mut self, scene: &mut RenderScene) {
        let ids: Vec<_> = self.shown.drain().map(|(_, ent)| ent).collect();
        scene.remove_ids(&ids);
    }

    /// The entities near the camera drawn again when the scene, the filters or the camera's place changed enough.
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<std::sync::Arc<Frame>>) {
        let Some(frame) = frame else { return self.clear(e) };
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        let Some(p) = d.project() else { return self.clear(e) };
        let cam = e.viewport.camera();
        let at = { let (x, z) = d.terry_frame.from_frame(f64::from(cam.target.x), f64::from(cam.target.z)); [x, z] };
        let revision = d.terry_revision.load(std::sync::atomic::Ordering::Relaxed);
        let fresh = self.built.is_some_and(|(bd, br, bs, brad, bc)| {
            bd == doc && br == revision && bs == (self.show, self.season) && brad == self.radius_m && (bc[0] - at[0]).hypot(bc[1] - at[1]) < f64::from(self.radius_m) / 4.0
        });

        if !fresh {
            let r = f64::from(self.radius_m);
            let wanted: Vec<u64> = p.entities.iter().map(|x| x.id).filter(|&id| {
                let w = p.world_position(id);
                (w[0] - at[0]).hypot(w[2] - at[1]) <= r
            }).collect();

            let overlays: Vec<(u64, Overlay)> = wanted.into_iter().filter_map(|id| Some((id, overlay(d, &frame, id, self.show, self.season)?))).collect();
            if self.layer == 0 || e.scene.layer(self.layer).is_none() {
                self.layer = e.scene.add_layer(doc, "terry entities");
                self.shown.clear();
                self.draft = 0;
            } else {
                self.remove_shown(&mut e.scene);
            }

            for (id, o) in overlays {
                let ent = e.scene.add(self.layer, Float4x4::IDENTITY, Payload::Overlay(o), SourceRef { doc, item: id });
                self.shown.insert(id, ent);
            }

            self.built = Some((doc, revision, (self.show, self.season), self.radius_m, at));
        }

        // The deployment zone being drawn, or the edited polyline's points.
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let draft = (self.mode == Mode::NewDeploymentZone && !self.points.is_empty()).then(|| {
            let mut lines = vec![];
            on_ground(&frame, &self.points, false, d.terry_frame, overlay_colour(1.0, 1.0, 0.3, 1.0), &mut lines);
            Overlay { lines, depth_test: false, pickable: false, ..Default::default() }
        });

        let selected = e.selection.items().iter().filter(|s| s.doc == doc).map(|s| s.item).collect::<Vec<_>>();
        let draft = draft.or_else(|| {
            if self.mode != Mode::EditPoints {
                return None;
            }

            let (id, pts) = selected.iter().find_map(|&id| polyline_world(d, id).map(|p| (id, p)))?;
            let mut lines = vec![];
            for (k, q) in pts.iter().enumerate() {
                let picked = self.picked == Some((id, k));
                let cell = d.terry_frame.cell;
                let (r, c) = if picked { (cell / 3.0, overlay_colour(1.0, 0.25, 0.25, 1.0)) } else { (cell / 6.0, overlay_colour(1.0, 1.0, 0.3, 1.0)) };
                on_ground(&frame, &[[q[0] - r, q[1] - r], [q[0] + r, q[1] - r], [q[0] + r, q[1] + r], [q[0] - r, q[1] + r]], true, d.terry_frame, c, &mut lines);
            }

            Some(Overlay { lines, depth_test: false, pickable: false, ..Default::default() })
        });

        if self.draft != 0 {
            e.scene.remove(self.draft);
            self.draft = 0;
        }

        if let Some(o) = draft {
            self.draft = e.scene.add(self.layer, Float4x4::IDENTITY, Payload::Overlay(o), SourceRef { doc, item: u64::MAX });
        }
    }

    /// Nothing drawn (no kit map shown).
    pub fn clear(&mut self, e: &mut Editor) {
        if self.layer != 0 {
            e.scene.remove_layer(self.layer);
        }

        (self.layer, self.draft, self.built) = (0, 0, None);
        self.shown.clear();
    }

    /// A click on the map at `at` (metres from the map's centre) in a creation mode. What happened, for the status line.
    pub fn click(&mut self, e: &mut Editor, doc: DocumentId, at: [f64; 2]) -> Option<String> {
        match self.mode {
            Mode::Select => None,
            Mode::EditPoints => Some(self.edit_point(e, doc, at).unwrap_or_else(|err| err)),
            Mode::NewBca => Some(new_bca(e, doc, at).unwrap_or_else(|err| err)),
            Mode::NewProp => Some(new_prop(e, doc, at, &self.new.model, self.new.scale).unwrap_or_else(|err| err)),
            Mode::NewVfx => Some(new_vfx(e, doc, at, &self.new.vfx).unwrap_or_else(|err| err)),
            Mode::NewEnvironmentVolume => Some(new_environment_volume(e, doc, at, &self.new.lighting, self.new.radii).unwrap_or_else(|err| err)),
            Mode::NewDeploymentZone => {
                // Next to the first point: the zone closed and made.
                let close = self.points.len() >= 3 && (self.points[0][0] - at[0]).hypot(self.points[0][1] - at[1]) < TERRY_CELL / 2.0;
                if close {
                    return Some(self.finish_zone(e, doc));
                }

                self.points.push(at);
                Some(format!("deployment zone: point {} (click the first point again, or Finish, to make it)", self.points.len()))
            }
        }
    }

    /// The deployment zone drawn so far made: a zone with one additive region whose polyline the points are.
    pub fn finish_zone(&mut self, e: &mut Editor, doc: DocumentId) -> String {
        let pts = std::mem::take(&mut self.points);
        new_zone(e, doc, &pts).unwrap_or_else(|err| err)
    }

    pub fn draw_section(&mut self, e: &mut Editor, doc: DocumentId) {
        if !gui::collapsing_header("Terry entities", 0) {
            return;
        }

        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let Some(p) = d.project() else {
            gui::text_disabled(if d.model().is_some_and(|m| m.built) {
                "This map has no battle locations (Rome 2's campaign battle maps, main_*, have them)."
            } else {
                "Open a kit map with a .terry project to edit its entities."
            });
            self.mode = Mode::Select;
            return;
        };

        gui::text_disabled(&format!("{} entities; {} drawn near the camera", p.entities.len(), self.shown.len()));
        for (k, name) in ["battle catchment areas", "catchment regions", "deployment zones", "others"].iter().enumerate() {
            if k > 0 {
                gui::same_line();
            }

            gui::checkbox(name, &mut self.show[k]);
        }

        gui::slider_float("drawn within (m)", &mut self.radius_m, 2_000.0, 200_000.0, "%.0f");
        let campaign = d.terry_frame.y_from_lf;
        let modes = if campaign { &CAMPAIGN_MODES } else { &BATTLE_MODES };
        if !modes.iter().any(|m| m.0 == self.mode) {
            self.mode = Mode::Select;
        }

        let mut m = modes.iter().position(|x| x.0 == self.mode).unwrap_or(0) as i32;
        let labels: Vec<&str> = modes.iter().map(|x| x.1).collect();
        if gui::combo("left button", &mut m, &labels) {
            self.mode = modes[m.clamp(0, 3) as usize].0;
            self.points.clear();
            self.picked = None;
        }

        if campaign {
            let mut k = self.season as i32;
            if gui::combo("season", &mut k, &["every season", "spring", "summer", "autumn", "winter"]) {
                self.season = k.clamp(0, 4) as usize;
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Terry's season: the entities whose season mask leaves the season out are hidden");
            }

            let selected: Vec<u64> = e.selection.items().iter().filter(|s| s.doc == doc).map(|s| s.item).collect();
            self.new_entity_settings(p, &selected);
        }

        if self.mode == Mode::EditPoints {
            gui::text_disabled("click a point, then where it goes; Shift+click a side: a point there; Ctrl+click a point: removed");
        }

        if self.mode == Mode::NewDeploymentZone {
            gui::text_disabled(&format!("{} points", self.points.len()));
            if gui::button("Finish the zone") {
                let msg = self.finish_zone(e, doc);
                e.status(&msg, false);
            }

            gui::same_line();
            if gui::button("Cancel") {
                self.points.clear();
            }
        }

        let selected: Vec<u64> = e.selection.items().iter().filter(|s| s.doc == doc).map(|s| s.item).collect();
        gui::begin_disabled(selected.is_empty());
        if gui::button("Delete selected") {
            let m = delete(e, doc, &selected).unwrap_or_else(|err| err);
            e.selection.clear();
            e.status(&m, false);
        }

        gui::same_line();
        if gui::button("Duplicate selected") {
            let m = duplicate(e, doc, &selected).unwrap_or_else(|err| err);
            e.status(&m, false);
        }

        gui::end_disabled();
        if gui::button("Generate battle catchment areas") {
            let m = generate_bcas(e, doc).unwrap_or_else(|err| err);
            e.status(&m, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Terry's generator: the procedural BCAs replaced by those bca_generation_spec.xml makes from the map's tiles, with their deployment zones");
        }
    }

    pub fn claims_left_button(&self) -> bool {
        self.mode != Mode::Select
    }

    /// The settings of the campaign creation mode chosen: a text field each, a list of the map's values (most used first), "From the
    /// selected".
    fn new_entity_settings(&mut self, p: &TerryProject, selected: &[u64]) {
        let used = |f: &dyn Fn(&EntityKind) -> Option<String>| -> Vec<String> {
            let mut c: HashMap<String, usize> = HashMap::new();
            for x in &p.entities {
                if let Some(v) = f(&p.kind(x)) {
                    *c.entry(v).or_default() += 1;
                }
            }

            let mut v: Vec<(String, usize)> = c.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.into_iter().map(|x| x.0).collect()
        };

        let pick = |label: &str, value: &mut String, list: &[String]| {
            if value.is_empty() && let Some(first) = list.first() {
                value.clone_from(first);
            }

            gui::input_text(label, None, value, 512, 0);
            let mut k = list.iter().position(|v| v == value).map_or(-1, |k| k as i32);
            let names: Vec<&str> = list.iter().map(String::as_str).collect();
            if gui::combo(&format!("used in the map##{label}"), &mut k, &names) && let Some(v) = list.get(k.max(0) as usize) {
                value.clone_from(v);
            }
        };

        let first_selected = selected.iter().find_map(|&id| p.entity(id));
        match self.mode {
            Mode::NewProp => {
                pick("model", &mut self.new.model, &used(&|k| if let EntityKind::Prop(m) = k { Some(m.clone()) } else { None }));
                gui::drag_float("scale", &mut self.new.scale, 0.01, 0.01, 100.0, "%.3f");
                if let Some(x) = first_selected && let EntityKind::Prop(m) = p.kind(x) && gui::button("From the selected") {
                    self.new.model = m;
                    let s: Vec<f32> = p.component(x, "ECTransform").and_then(|c| p.doc.get(c, "scale"))
                        .map(|v| v.split_whitespace().filter_map(|t| t.parse().ok()).collect()).unwrap_or_default();
                    self.new.scale = s.first().copied().unwrap_or(1.0);
                }

                gui::text_disabled("a click: the prop there, every season; its y from the LF (ASSUMPTION: the kit's follow the tiles)");
            }

            Mode::NewVfx => {
                pick("effect", &mut self.new.vfx, &used(&|k| if let EntityKind::Vfx(v) = k { Some(v.clone()) } else { None }));
                if let Some(x) = first_selected && let EntityKind::Vfx(v) = p.kind(x) && gui::button("From the selected") {
                    self.new.vfx = v;
                }
            }

            Mode::NewEnvironmentVolume => {
                pick("lighting", &mut self.new.lighting, &used(&|k| if let EntityKind::EnvironmentVolume(l) = k { Some(l.clone()) } else { None }));
                gui::drag_float2("inner, outer radius", &mut self.new.radii, 1.0, "%.1f");
                gui::text_disabled("an outer radius of 0 is the map's global lighting");
            }

            _ => {}
        }
    }
}

/// A structural edit of the document's `.terry` onto its history.
fn structural(e: &mut Editor, doc: DocumentId, name: &str, f: impl FnOnce(&mut TerryProject) -> Result<Vec<wce_formats::xml_doc::XmlSplice>, String>) -> Result<(), String> {
    let d = e.docs.find_as_mut::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project_mut().ok_or("this map has no .terry project")?;
    let splices = f(p)?;
    let revision = d.terry_revision.clone();
    d.push_terry(Box::new(TerrySpliceCommand { name: name.to_owned(), splices, done: true, revision }));
    Ok(())
}

/// A battle catchment area at the cell under `at`: a 2048 m rectangle on the cell grid on the LF, with a 2304 m `Catchment Region`, as
/// Terry's (and the kit's) are.
pub fn new_bca(e: &mut Editor, doc: DocumentId, at: [f64; 2]) -> Result<String, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    if d.terry_frame.y_from_lf {
        return Err("battle catchment areas are a battle map's (a campaign map's .terry has props, VFX and environment volumes)".to_owned());
    }

    let (x, z) = ((at[0] / TERRY_CELL).round() * TERRY_CELL, (at[1] / TERRY_CELL).round() * TERRY_CELL);
    let y = d.lf_metres_at(x, z);
    let (id, cid) = (p.next_id(), p.next_id() + 1);
    let n = |v: f64| wce_terrain::terry::number_text(v);
    let name = format!("BCA {}", p.with_component("ECBattleCatchmentArea").count() + 1);
    let xml = format!(
        "<entity id=\"{id:x}\" name=\"{name}\">\n{{i}}<components>\n{{i}}{{i}}<ECRectangle width=\"2048\" height=\"2048\" active=\"true\"/>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} {} {}\">\n{{i}}{{i}}{{i}}<children>\n\
{{i}}{{i}}{{i}}{{i}}<child id=\"{cid:x}\"/>\n{{i}}{{i}}{{i}}</children>\n{{i}}{{i}}</ECTransform>\n{{i}}{{i}}<ECLandscapeAbove active=\"true\"/>\n\
{{i}}{{i}}<ECTileAlignment active=\"true\"/>\n{{i}}{{i}}<ECBattleCatchmentArea catchment_type=\"land_normal\" active=\"true\"/>\n{{i}}</components>\n\
{{i}}<children>\n{{i}}{{i}}<child id=\"{cid:x}\"/>\n{{i}}</children>\n</entity>\n<entity id=\"{cid:x}\" name=\"Catchment Region\">\n{{i}}<components>\n\
{{i}}{{i}}<ECRectangle width=\"2304\" height=\"2304\" active=\"true\"/>\n{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"0 0 0\"/>\n\
{{i}}{{i}}<ECLandscapeAbove active=\"true\"/>\n{{i}}</components>\n{{i}}<children/>\n</entity>",
        n(x), n(y), n(z)
    );

    structural(e, doc, "new battle catchment area", |p| p.insert_entities(&xml).map_err(|err| err.to_string()))?;
    Ok(format!("{name} at {}, {} (land_normal; its type in the Inspector)", n(x), n(z)))
}

/// One campaign entity (the kit's components, in its order) at `at`, named `name`: one step on the history.
fn new_campaign_entity(e: &mut Editor, doc: DocumentId, at: [f64; 2], y: Option<f64>, name: &str, what: &str, components: &str) -> Result<String, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    if !d.terry_frame.y_from_lf {
        return Err(format!("{what}: a campaign map's (a battle map's props and effects come with its tiles)"));
    }

    let id = p.next_id();
    let n = |v: f64| wce_terrain::terry::number_text(v);
    let y = y.unwrap_or_else(|| d.terry_y_at(at[0], at[1]));
    let components = components.replace("{pos}", &format!("{} {} {}", n(at[0]), n(y), n(at[1])));
    let xml = format!("<entity id=\"{id:x}\" name=\"{}\">\n{{i}}<components>\n{components}{{i}}</components>\n{{i}}<children/>\n</entity>", xml_attr(name));
    structural(e, doc, what, |p| p.insert_entities(&xml).map_err(|err| err.to_string()))?;
    Ok(format!("{what}: {name} ({id:x}) at {}, {}, {}", n(at[0]), n(y), n(at[1])))
}

/// Text in an XML attribute.
fn xml_attr(v: &str) -> String {
    v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A campaign prop: `ECMesh` of `model`, the transform (no turn, `scale` on every axis), on the landscape, every season, visible
/// everywhere but its destruction zone flags as the kit's defaults; named after the model's file.
pub fn new_prop(e: &mut Editor, doc: DocumentId, at: [f64; 2], model: &str, scale: f32) -> Result<String, String> {
    if model.trim().is_empty() {
        return Err("a prop needs a model (RigidModels/campaign/...rigid_model_v2)".to_owned());
    }

    let s = wce_terrain::terry::number_text(f64::from(scale));
    let name = model.rsplit(['/', '\\']).next().unwrap_or(model).trim_end_matches(".rigid_model_v2").to_owned();
    let c = format!(
        "{{i}}{{i}}<ECMesh model_path=\"{}\" opacity=\"1\" glow_amount=\"0\" active=\"true\" burn=\"false\" burn_amount=\"0\"/>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"{s} {s} {s}\" active=\"true\" position=\"{{pos}}\"/>\n{{i}}{{i}}<ECLandscapeAbove active=\"true\"/>\n\
{{i}}{{i}}<ECPropMesh visible_outside_snow_zone=\"true\" active=\"true\" visible_inside_destruction_zone=\"true\" visible_inside_snow_zone=\"true\" visible_outside_destruction_zone=\"true\"/>\n\
{{i}}{{i}}<ECSeasonMask autumn=\"true\" winter=\"true\" active=\"true\" spring=\"true\" summer=\"true\"/>\n",
        xml_attr(model.trim())
    );

    new_campaign_entity(e, doc, at, None, &name, "new prop", &c)
}

/// A campaign VFX: `ECVFX` of `vfx` (autoplay, visible everywhere, scale 1), the transform, every season; named after the effect.
pub fn new_vfx(e: &mut Editor, doc: DocumentId, at: [f64; 2], vfx: &str) -> Result<String, String> {
    let vfx = vfx.trim();
    if vfx.is_empty() {
        return Err("a VFX needs an effect".to_owned());
    }

    let c = format!(
        "{{i}}{{i}}<ECVFX vfx=\"{v}\" visible_outside_snow_zone=\"true\" instance_name=\"\" scale=\"1\" active=\"true\" visible_inside_destruction_zone=\"true\" visible_inside_snow_zone=\"true\" visible_outside_destruction_zone=\"true\" export_in_tile_marker_file=\"false\" autoplay=\"true\"/>\n\
{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{{pos}}\"/>\n\
{{i}}{{i}}<ECSeasonMask autumn=\"true\" winter=\"true\" active=\"true\" spring=\"true\" summer=\"true\"/>\n",
        v = xml_attr(vfx)
    );

    new_campaign_entity(e, doc, at, None, vfx, "new VFX", &c)
}

/// A campaign environment volume: the transform, then `ECEnvironmentVolume` (lighting, radii: `[inner, outer]`); named `EV <n>`.
pub fn new_environment_volume(e: &mut Editor, doc: DocumentId, at: [f64; 2], lighting: &str, radii: [f32; 2]) -> Result<String, String> {
    if lighting.trim().is_empty() {
        return Err("an environment volume needs a lighting file".to_owned());
    }

    let count = e.docs.find_as::<MapDocument>(doc).and_then(|d| d.project()).map_or(0, |p| p.with_component("ECEnvironmentVolume").count());
    let n = |v: f32| wce_terrain::terry::number_text(f64::from(v));
    let c = format!(
        "{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{{pos}}\"/>\n\
{{i}}{{i}}<ECEnvironmentVolume outer_radius=\"{}\" lighting_file=\"{}\" active=\"true\" inner_radius=\"{}\"/>\n",
        n(radii[1]), xml_attr(lighting.trim()), n(radii[0])
    );

    new_campaign_entity(e, doc, at, None, &format!("EV {}", count + 1), "new environment volume", &c)
}

/// A deployment zone of one additive region through `pts` (metres on the map; three at least). Zones come in pairs, the two alliances'
/// halves of one deployment ("DZ k/0" then "DZ k/1", as the kit's 883 pairs; BOB writes a deployment file per pair): a new zone is the
/// alliance its predecessor leaves open. A zone belongs to a battle catchment area (an entity child of it: BOB writes the deployments of
/// a BCA's zones only): the selected one, else the one whose rectangle holds the first point. Its region is placed on the map (no
/// transform parent), as the kit's are.
pub fn new_zone(e: &mut Editor, doc: DocumentId, pts: &[[f64; 2]]) -> Result<String, String> {
    if pts.len() < 3 {
        return Err("a deployment zone needs three points at least".to_owned());
    }

    let selected: Vec<u64> = e.selection.items().iter().filter(|s| s.doc == doc).map(|s| s.item).collect();
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    if d.terry_frame.y_from_lf {
        return Err("deployment zones are a battle map's".to_owned());
    }

    let is_bca = |id: &u64| p.entity(*id).is_some_and(|x| matches!(p.kind(x), EntityKind::Bca(_)));
    let holds = |id: &u64| p.entity(*id).and_then(|x| p.rectangle(x)).is_some_and(|r| {
        let w = p.world_position(*id);
        (pts[0][0] - w[0]).abs() <= r[0] / 2.0 && (pts[0][1] - w[2]).abs() <= r[1] / 2.0
    });

    let bca = selected.iter().copied().find(is_bca).or_else(|| p.entities.iter().map(|x| x.id).filter(is_bca).find(holds))
        .ok_or("a deployment zone belongs to a battle catchment area: select one, or start the zone inside one")?;
    let (id, rid) = (p.next_id(), p.next_id() + 1);
    let zones = p.entity(bca).map_or(0, |b| b.children.iter().filter(|c| p.entity(**c).is_some_and(|x| p.component(x, "ECDeploymentZone").is_some())).count());
    let (pair, alliance) = (zones / 2, zones % 2);
    let n = |v: f64| wce_terrain::terry::number_text(v);
    let o = pts[0];
    let y = d.lf_metres_at(o[0], o[1]);
    let points: String = pts.iter().map(|q| format!("{{i}}{{i}}{{i}}{{i}}<point x=\"{}\" y=\"{}\"/>\n", n(q[0] - o[0]), n(q[1] - o[1]))).collect();
    let xml = format!(
        "<entity id=\"{id:x}\" name=\"DZ {pair}/{alliance}\">\n{{i}}<components>\n\
{{i}}{{i}}<ECDeploymentZone deployment_category=\"DZC_RIVER\" deployment_type=\"DZT_1V1\" active=\"true\" facing_direction=\"0\" deployment_zone_id=\"0\" alliance_id=\"{alliance}\" configuration=\"0\"/>\n\
{{i}}</components>\n{{i}}<children>\n{{i}}{{i}}<child id=\"{rid:x}\"/>\n{{i}}</children>\n</entity>\n<entity id=\"{rid:x}\" name=\"DZR 0\">\n{{i}}<components>\n\
{{i}}{{i}}<ECDeploymentZoneRegion active=\"true\" region_type=\"DZRT_ADDITIVE\"/>\n{{i}}{{i}}<ECPolyline active=\"true\">\n{{i}}{{i}}{{i}}<polyline closed=\"true\">\n\
{points}{{i}}{{i}}{{i}}</polyline>\n{{i}}{{i}}</ECPolyline>\n{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} {} {}\"/>\n\
{{i}}</components>\n{{i}}<children/>\n</entity>",
        n(o[0]), n(y), n(o[1])
    );

    structural(e, doc, "new deployment zone", |p| {
        let mut s = p.insert_entities(&xml).map_err(|err| err.to_string())?;
        s.extend(p.add_child(bca, id).map_err(|err| err.to_string())?);
        Ok(s)
    })?;
    Ok(format!("deployment zone DZ {pair}/{alliance} (alliance {alliance}) of {} points, of BCA {bca:x}; its region {rid:x}", pts.len()))
}

/// An entity's polyline on the map (metres from the map's centre: x, z).
fn polyline_world(d: &MapDocument, id: u64) -> Option<Vec<[f64; 2]>> {
    let p = d.project()?;
    let (pts, _) = p.polyline(p.entity(id)?)?;
    let w = p.world_position(id);
    Some(pts.iter().map(|q| [w[0] + q[0], w[2] + q[1]]).collect())
}

impl TerryView {
    /// A click of the point editing mode (see [`Mode::EditPoints`]).
    fn edit_point(&mut self, e: &mut Editor, doc: DocumentId, at: [f64; 2]) -> Result<String, String> {
        let selected: Vec<u64> = e.selection.items().iter().filter(|s| s.doc == doc).map(|s| s.item).collect();
        let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
        let (id, mut pts) = selected.iter().find_map(|&id| polyline_world(d, id).map(|p| (id, p))).ok_or("select an entity with a polyline (a deployment zone region)")?;
        let near = TERRY_CELL / 2.0;
        let vertex = pts.iter().enumerate().map(|(k, q)| (k, (q[0] - at[0]).hypot(q[1] - at[1]))).filter(|v| v.1 <= near)
            .min_by(|a, b| a.1.total_cmp(&b.1)).map(|v| v.0);
        let what;
        if gui::is_key_down(gui::MOD_CTRL) {
            let k = vertex.ok_or("Ctrl+click on a point to remove it")?;
            if pts.len() <= 3 {
                return Err("a region keeps three points at least".to_owned());
            }

            pts.remove(k);
            self.picked = None;
            what = format!("point {k} removed");
        } else if gui::is_key_down(gui::MOD_SHIFT) {
            // The side nearest the click: its point nearest the click on it.
            let n = pts.len();
            let mut best: Option<(usize, f64, [f64; 2])> = None;
            for k in 0..n {
                let (a, b) = (pts[k], pts[(k + 1) % n]);
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let len2 = dx * dx + dz * dz;
                let t = if len2 > 0.0 { (((at[0] - a[0]) * dx + (at[1] - a[1]) * dz) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let q = [a[0] + dx * t, a[1] + dz * t];
                let dist = (q[0] - at[0]).hypot(q[1] - at[1]);
                if dist <= near && best.is_none_or(|b| dist < b.1) {
                    best = Some((k, dist, q));
                }
            }

            let (k, _, q) = best.ok_or("Shift+click on a side to add a point there")?;
            pts.insert(k + 1, q);
            self.picked = Some((id, k + 1));
            what = format!("point added after {k}");
        } else if let Some((pid, k)) = self.picked.filter(|p| p.0 == id && p.1 < pts.len()) {
            if vertex == Some(k) {
                self.picked = None;
                return Ok(format!("point {k} let go"));
            }

            pts[k] = at;
            self.picked = None;
            let _ = pid;
            what = format!("point {k} moved");
        } else {
            let k = vertex.ok_or("click on a point of the selected polyline")?;
            self.picked = Some((id, k));
            self.built = None;
            return Ok(format!("point {k} picked: click where it goes"));
        }

        set_points(e, doc, id, &pts)?;
        Ok(what)
    }
}

/// An entity's polyline set to points on the map (metres from the map's centre), as offsets from its position: one step on the history.
pub fn set_points(e: &mut Editor, doc: DocumentId, id: u64, pts: &[[f64; 2]]) -> Result<(), String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    let w = p.world_position(id);
    let offsets: Vec<[f64; 2]> = pts.iter().map(|q| [q[0] - w[0], q[1] - w[2]]).collect();
    structural(e, doc, "edit polyline points", |p| p.set_polyline(id, &offsets).map_err(|err| err.to_string()))
}

/// Removes entities (their descendants and the references to them).
pub fn delete(e: &mut Editor, doc: DocumentId, ids: &[u64]) -> Result<String, String> {
    structural(e, doc, "delete entities", |p| p.remove_entities(ids).map_err(|err| err.to_string()))?;
    Ok(format!("{} entities deleted (with their children)", ids.len()))
}

/// Copies entities (with their descendants: new ids, the references among them renamed) a cell to the east.
pub fn duplicate(e: &mut Editor, doc: DocumentId, ids: &[u64]) -> Result<String, String> {
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    let roots: Vec<u64> = ids.iter().copied().filter(|&id| p.parent_of(id).is_none_or(|q| !ids.contains(&q))).collect();
    let mut next = p.next_id();
    let mut xml = String::new();
    for &root in &roots {
        let texts = p.entity_text(root);
        let map: HashMap<u64, u64> = texts.iter().map(|(old, _)| {
            next += 1;
            (*old, next - 1)
        }).collect();

        for (old, text) in texts {
            let mut t = text;
            for (a, b) in &map {
                t = t.replace(&format!("id=\"{a:x}\""), &format!("id=\"{b:x}\""));
            }

            // A cell to the east: each copied entity placed on its own (the root, a BCA's zones' regions: no transform parent in the copy).
            if p.transform_parent_of(old).is_none_or(|q| !map.contains_key(&q))
                && let Some(pos) = p.entity(old).and_then(|x| p.local_position(x))
            {
                let n = |v: f64| wce_terrain::terry::number_text(v);
                t = t.replacen(&format!("position=\"{} {} {}\"", n(pos[0]), n(pos[1]), n(pos[2])), &format!("position=\"{} {} {}\"", n(pos[0] + TERRY_CELL), n(pos[1]), n(pos[2])), 1);
            }

            // The text's own lines lose the indent of the scene's entities (insert_entities puts it back).
            let lines: Vec<&str> = t.lines().collect();
            let indent = lines.iter().skip(1).filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
            for (k, l) in lines.iter().enumerate() {
                let l = l.trim_end_matches('\r');
                xml += if k == 0 { l } else { l.get(indent..).unwrap_or(l.trim_start()) };
                xml.push('\n');
            }
        }
    }

    let xml = xml.trim_end().to_owned();
    structural(e, doc, "duplicate entities", |p| p.insert_entities(&xml).map_err(|err| err.to_string()))?;
    Ok(format!("{} entities duplicated a cell to the east", roots.len()))
}

/// The battle field of a placed map as the generator takes it: its placements as tile database indices (in the map's order: the battle
/// field's); the ones the database doesn't know are left out.
fn battle_field(db: &wce_tile::battle_map::tile_db::TileDatabase, placed: &wce_tile::battle_map::context::PlacedMap) -> Vec<wce_tile::battle_map::fill::Placement> {
    let mut by_name = HashMap::new();
    for (t, tile) in db.tiles.iter().enumerate() {
        for v in 0..tile.variations.len() {
            by_name.insert(db.tile_location_of(t, v).replace('/', "\\").to_lowercase(), (t, v));
        }
    }

    placed.placements.iter().filter_map(|p| {
        let key = format!("{}\\{}", p.location.replace('/', "\\").trim_matches('\\'), p.variation).to_lowercase();
        let &(tile, variation) = by_name.get(&key)?;
        Some(wce_tile::battle_map::fill::Placement {
            tile, variation, x: p.x as i16, y: p.y as i16, flags: (p.rotation & 0xf0) | if p.flipped { 4 } else { 0 }, climate: p.climate, lf: [0.0, 0.0],
        })
    }).collect()
}

/// Terry's Generate Battle Catchment Areas (docs/re/terry.md, "The BCA generator"): the procedural BCAs (and their regions and zones)
/// replaced by the ones `bca_generation_spec.xml` makes from the shown map's battle field, deployment zones included; with the spec's
/// `delete_invalid` the BCAs with errors are left out. The spec and the tiles' deployment masks are looked for in the projects' kit layout,
/// then the Assembly Kits. One step on the history.
pub fn generate_bcas(e: &mut Editor, doc: DocumentId) -> Result<String, String> {
    let roots = crate::tool::kit_roots(e);
    let spec_path = roots.iter().filter_map(|r| r.parent()).map(|k| k.join("editor_working_data/Terry/bca_generation_spec.xml")).find(|p| p.is_file())
        .ok_or("no editor_working_data/Terry/bca_generation_spec.xml in the projects' kit layout or an Assembly Kit")?;
    let spec = wce_terrain::bca::Spec::parse(&std::fs::read(&spec_path).map_err(|err| format!("{}: {err}", spec_path.display()))?);
    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    let (w, h) = (d.placed.width, d.placed.height);
    let instances = battle_field(&d.db, &d.placed);
    let masks = wce_terrain::bca::load_masks(&d.db, &instances, &roots);
    let all = wce_terrain::bca::generate(&spec, &d.db, w, h, &instances, &masks);
    let kept: Vec<&wce_terrain::bca::Bca> = all.iter().filter(|b| !(spec.delete_invalid && !b.errors.is_empty())).collect();
    let old = wce_terrain::bca::procedural(p);
    let first = p.next_id();
    let (xml, _) = wce_terrain::bca::terry_xml(&spec, &kept, w, h, first, &|x, z| d.lf_metres_at(x, z));
    let zones: usize = kept.iter().map(|b| b.zones.len()).sum();
    let errors: Vec<String> = all.iter().flat_map(|b| b.errors.iter().map(move |m| format!("{}: {m}", b.name))).collect();
    if kept.is_empty() && old.is_empty() {
        return Ok(format!("no battle catchment areas: {} made, {} with errors (a map's {} cells by its edges make none)", all.len(), errors.len(), spec.map_border_for_outfield));
    }

    structural(e, doc, "generate battle catchment areas", |p| {
        let mut s = p.remove_entities(&old).map_err(|err| err.to_string())?;
        s.extend(p.insert_entities(&xml).map_err(|err| err.to_string())?);
        Ok(s)
    })?;

    let mut msg = format!(
        "{} battle catchment areas with {zones} deployment zones ({} made, {} with errors{}; {} procedural ones replaced; {} deployment masks)",
        kept.len(), all.len(), all.iter().filter(|b| !b.errors.is_empty()).count(), if spec.delete_invalid { ", left out" } else { "" }, old.len(),
        masks.len()
    );

    if !errors.is_empty() {
        msg += &format!("\n{}", errors.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
        if errors.len() > 20 {
            msg += &format!("\n... {} more", errors.len() - 20);
        }
    }

    Ok(msg)
}
