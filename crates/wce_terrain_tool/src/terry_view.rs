//! The `.terry` scene of the shown kit map in the 3D view (docs/plans/terrain_editor.md, Phase 5): each entity near the camera an overlay
//! on the LF (rectangles, polylines; pickable, so selection, the Outliner, the Inspector and the gizmo reach it through the map
//! document), drawn again when the scene changes or the camera moves on; and Terry's creation modes for battle maps: a battle catchment area
//! (a click: a 2048 m rectangle on the cell grid, with its `Catchment Region`), a deployment zone (clicks for its region's points), and
//! delete and duplicate of the selected entities. Each is one step on the map's history.

use std::collections::HashMap;

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{DocumentId, EntityId, LayerId, Overlay, OverlayVertex, Payload, SourceRef, overlay_colour};
use wce_terrain::terry::{EntityKind, TerryProject};
use wce_terrain_editor::MapDocument;
use wce_terrain_editor::document::TERRY_CELL;
use wce_terrain_editor::terry::TerrySpliceCommand;
use wce_tile_editor::tile_document::context::Frame;

/// What the drawn set is of: document, `.terry` revision, filters, radius, the point it is centred on.
type Built = (DocumentId, u64, [bool; 4], f32, [f64; 2]);

/// What a click does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Select,
    NewBca,
    NewDeploymentZone,
}

pub struct TerryView {
    /// Which kinds are drawn: battle catchment areas, their catchment regions, deployment zones' regions, the others.
    pub show: [bool; 4],

    /// Entities within this of the camera's target are drawn (metres).
    pub radius_m: f32,
    pub mode: Mode,

    /// The deployment zone being drawn: its points (metres on the map).
    pub points: Vec<[f64; 2]>,

    layer: LayerId,
    shown: HashMap<u64, EntityId>,

    /// What the drawn set is of: document, `.terry` revision, filters, the point it is centred on (metres).
    built: Option<Built>,
    draft: EntityId,
}

impl Default for TerryView {
    fn default() -> Self {
        Self { show: [true, true, true, true], radius_m: 40_000.0, mode: Mode::Select, points: vec![], layer: 0, shown: HashMap::new(), built: None, draft: 0 }
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
        EntityKind::Other => (3, [1.0, 1.0, 1.0, 0.8]),
    }
}

/// A line along the ground (frame units): the points lifted onto the LF, every segment cut into pieces a cell long at most.
fn on_ground(frame: &Frame, pts: &[[f64; 2]], closed: bool, unit: f64, colour: u32, out: &mut Vec<OverlayVertex>) {
    let n = pts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let step = TERRY_CELL / 2.0;
    for k in 0..segments {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let pieces = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as usize;
        let at = |t: f64| {
            let (x, z) = ((a[0] + (b[0] - a[0]) * t) / unit, (a[1] + (b[1] - a[1]) * t) / unit);
            Float3::new(x as f32, frame.lift(x, z) + 1.0, z as f32)
        };

        for i in 0..pieces {
            out.push(OverlayVertex { p: at(i as f64 / pieces as f64), rgba: colour });
            out.push(OverlayVertex { p: at((i + 1) as f64 / pieces as f64), rgba: colour });
        }
    }
}

/// An entity's overlay, or none (a kind not shown, nothing to draw).
fn overlay(d: &MapDocument, frame: &Frame, id: u64, show: [bool; 4]) -> Option<Overlay> {
    let p = d.project()?;
    let e = p.entity(id)?;
    let kind = p.kind(e);
    let (slot, c) = style(p, id, &kind);
    if !show[slot] {
        return None;
    }

    let colour = overlay_colour(c[0], c[1], c[2], c[3]);
    let w = p.world_position(id);
    let mut lines = vec![];
    if let Some([rw, rh]) = p.rectangle(e) {
        let (x0, x1, z0, z1) = (w[0] - rw / 2.0, w[0] + rw / 2.0, w[2] - rh / 2.0, w[2] + rh / 2.0);
        on_ground(frame, &[[x0, z0], [x1, z0], [x1, z1], [x0, z1]], true, d.unit, colour, &mut lines);
    }

    if let Some((pts, closed)) = p.polyline(e) {
        let pts: Vec<[f64; 2]> = pts.iter().map(|q| [w[0] + q[0], w[2] + q[1]]).collect();
        on_ground(frame, &pts, closed, d.unit, colour, &mut lines);
    }

    if lines.is_empty() {
        // A spot: a small cross.
        let r = TERRY_CELL / 4.0;
        on_ground(frame, &[[w[0] - r, w[2]], [w[0] + r, w[2]]], false, d.unit, colour, &mut lines);
        on_ground(frame, &[[w[0], w[2] - r], [w[0], w[2] + r]], false, d.unit, colour, &mut lines);
    }

    Some(Overlay { lines, depth_test: false, ..Default::default() })
}

impl TerryView {
    /// The entities near the camera drawn again when the scene, the filters or the camera's place changed enough.
    pub fn update(&mut self, e: &mut Editor, doc: DocumentId, frame: Option<std::sync::Arc<Frame>>) {
        let Some(frame) = frame else { return self.clear(e) };
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return self.clear(e) };
        let Some(p) = d.project() else { return self.clear(e) };
        let cam = e.viewport.camera();
        let at = [f64::from(cam.target.x) * d.unit, f64::from(cam.target.z) * d.unit];
        let revision = d.terry_revision.load(std::sync::atomic::Ordering::Relaxed);
        let fresh = self.built.is_some_and(|(bd, br, bs, brad, bc)| {
            bd == doc && br == revision && bs == self.show && brad == self.radius_m && (bc[0] - at[0]).hypot(bc[1] - at[1]) < f64::from(self.radius_m) / 4.0
        });

        if !fresh {
            let r = f64::from(self.radius_m);
            let wanted: Vec<u64> = p.entities.iter().map(|x| x.id).filter(|&id| {
                let w = p.world_position(id);
                (w[0] - at[0]).hypot(w[2] - at[1]) <= r
            }).collect();

            let overlays: Vec<(u64, Overlay)> = wanted.into_iter().filter_map(|id| Some((id, overlay(d, &frame, id, self.show)?))).collect();
            if self.layer == 0 || e.scene.layer(self.layer).is_none() {
                self.layer = e.scene.add_layer(doc, "terry entities");
                self.shown.clear();
                self.draft = 0;
            } else {
                for (_, ent) in self.shown.drain() {
                    e.scene.remove(ent);
                }
            }

            for (id, o) in overlays {
                let ent = e.scene.add(self.layer, Float4x4::IDENTITY, Payload::Overlay(o), SourceRef { doc, item: id });
                self.shown.insert(id, ent);
            }

            self.built = Some((doc, revision, self.show, self.radius_m, at));
        }

        // The deployment zone being drawn.
        let Some(d) = e.docs.find_as::<MapDocument>(doc) else { return };
        let draft = (self.mode == Mode::NewDeploymentZone && !self.points.is_empty()).then(|| {
            let mut lines = vec![];
            on_ground(&frame, &self.points, false, d.unit, overlay_colour(1.0, 1.0, 0.3, 1.0), &mut lines);
            Overlay { lines, depth_test: false, pickable: false, ..Default::default() }
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
            Mode::NewBca => Some(new_bca(e, doc, at).unwrap_or_else(|err| err)),
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
            gui::text_disabled("Open a kit map with a .terry project to edit its entities.");
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
        let mut m = match self.mode {
            Mode::Select => 0,
            Mode::NewBca => 1,
            Mode::NewDeploymentZone => 2,
        };

        if gui::combo("left button", &mut m, &["select (the gizmo moves)", "new battle catchment area", "new deployment zone"]) {
            self.mode = [Mode::Select, Mode::NewBca, Mode::NewDeploymentZone][m.clamp(0, 2) as usize];
            self.points.clear();
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
    }

    pub fn claims_left_button(&self) -> bool {
        self.mode != Mode::Select
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

/// A deployment zone of one additive region through `pts` (metres on the map; three at least).
pub fn new_zone(e: &mut Editor, doc: DocumentId, pts: &[[f64; 2]]) -> Result<String, String> {
    if pts.len() < 3 {
        return Err("a deployment zone needs three points at least".to_owned());
    }

    let d = e.docs.find_as::<MapDocument>(doc).ok_or("no map shown")?;
    let p = d.project().ok_or("this map has no .terry project")?;
    let (id, rid) = (p.next_id(), p.next_id() + 1);
    let zones = p.with_component("ECDeploymentZone").count();
    let n = |v: f64| wce_terrain::terry::number_text(v);
    let o = pts[0];
    let y = d.lf_metres_at(o[0], o[1]);
    let points: String = pts.iter().map(|q| format!("{{i}}{{i}}{{i}}{{i}}<point x=\"{}\" y=\"{}\"/>\n", n(q[0] - o[0]), n(q[1] - o[1]))).collect();
    let xml = format!(
        "<entity id=\"{id:x}\" name=\"DZ {zones}/0\">\n{{i}}<components>\n\
{{i}}{{i}}<ECDeploymentZone deployment_category=\"DZC_RIVER\" deployment_type=\"DZT_1V1\" active=\"true\" facing_direction=\"0\" deployment_zone_id=\"0\" alliance_id=\"0\" configuration=\"0\"/>\n\
{{i}}</components>\n{{i}}<children>\n{{i}}{{i}}<child id=\"{rid:x}\"/>\n{{i}}</children>\n</entity>\n<entity id=\"{rid:x}\" name=\"DZR 0\">\n{{i}}<components>\n\
{{i}}{{i}}<ECDeploymentZoneRegion active=\"true\" region_type=\"DZRT_ADDITIVE\"/>\n{{i}}{{i}}<ECPolyline active=\"true\">\n{{i}}{{i}}{{i}}<polyline closed=\"true\">\n\
{points}{{i}}{{i}}{{i}}</polyline>\n{{i}}{{i}}</ECPolyline>\n{{i}}{{i}}<ECTransform rotation=\"0 0 0\" scale=\"1 1 1\" active=\"true\" position=\"{} {} {}\"/>\n\
{{i}}</components>\n{{i}}<children/>\n</entity>",
        n(o[0]), n(y), n(o[1])
    );

    structural(e, doc, "new deployment zone", |p| p.insert_entities(&xml).map_err(|err| err.to_string()))?;
    Ok(format!("deployment zone DZ {zones}/0 of {} points", pts.len()))
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

            // The root a cell to the east.
            if old == root
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
