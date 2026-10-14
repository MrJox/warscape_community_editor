//! The edits of a tile document (each one undo entry), the inspector, the gizmo, height strokes and saving.
//!
//! Placement (docs/formats/tile_ted.md, "Placement modes, measured in TEd"): a Landscape instance (`surface_mode="MSM_LANDSCAPE"`) stands on
//! the terrain; a Freeform one has any y. `lf_offset` is not used: it is 0 in every TEd-written tile and its meaning in the older tiles (where
//! it equals the y) is unknown. As in TEd: moving a landscape instance in x / z puts it on the terrain at the new place and a y that is asked
//! for is ignored; freeform y is free; Landscape -> Freeform keeps y, Freeform -> Landscape snaps it; a height brush stroke carries the
//! landscape instances under the brush with the terrain (the same undo entry as the stroke). Items without a `surface_mode` behave as freeform.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use wce_editor_core::command::History;
use wce_editor_core::document::{Document, SaveTarget};
use wce_editor_core::inspector::Inspector;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::SourceRef;
use wce_tile::height_field::Brush;
use wce_tile::ted_scene::{TedScene, Vec3};
use wce_tile::world_transform::{
    Mat3, WorldTransform, euler_from_matrix, mat_mul, mat_transpose, rotate_vec, rotate_world_yaw, set_world_position, set_world_rotation, world_transform,
};

use super::commands::{BrushStepCommand, PlacementItem, TedFlagCommand, TedMoveCommand, TedPlacementCommand, TedPointCommand, TedSpliceCommand};
use super::{BrushKind, BrushSettings, Follow, OffTerrain, Placement, PlacementCounts, TERRAIN, Tile, TileDocument, elem_of, item_of};

fn degrees(r: f64) -> f32 {
    (r * 180.0 / std::f64::consts::PI) as f32
}

fn radians(d: f32) -> f64 {
    f64::from(d) * std::f64::consts::PI / 180.0
}

fn parent_transform(sc: &TedScene, elem: usize) -> WorldTransform {
    sc.doc().elements()[elem].parent.map(|p| world_transform(sc, p)).unwrap_or_default()
}

fn close(a: Vec3, b: Vec3) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 2e-4)
}

/// The elements of `elems` that are not below another element of it, in document order.
fn top_level(sc: &TedScene, mut elems: Vec<usize>) -> Vec<usize> {
    elems.sort_unstable();
    elems.dedup();
    let els = sc.doc().elements();
    elems.iter().copied().filter(|&e| {
        let mut p = els[e].parent;
        while let Some(q) = p {
            if elems.binary_search(&q).is_ok() {
                return false;
            }

            p = els[q].parent;
        }

        true
    }).collect()
}

impl Tile {

    /// Local polygon point of a world position.
    pub fn to_local(&self, elem: usize, world: Float3) -> Vec3 {
        let w = world_transform(self.tscene(), elem);
        let u = self.unit();
        let d = [f64::from(world.x) * u - w.position[0], f64::from(world.y) - w.position[1], f64::from(world.z) * u - w.position[2]];
        rotate_vec(&mat_transpose(&w.rotation), d)
    }

    /// Local position of `elem` with its world y replaced (x, z kept).
    pub(crate) fn local_for_world_y(&self, elem: usize, world_y: f64) -> Vec3 {
        let pw = parent_transform(self.tscene(), elem);
        let mut w = world_transform(self.tscene(), elem);
        w.position[1] = world_y;
        let d = [w.position[0] - pw.position[0], w.position[1] - pw.position[1], w.position[2] - pw.position[2]];
        rotate_vec(&mat_transpose(&pw.rotation), d)
    }

    /// The instances among `elems` and below them (layers are walked, not returned).
    pub fn instances_below(&self, elems: &[usize]) -> Vec<usize> {
        let mut out = vec![];
        let mut seen = BTreeSet::new();
        fn walk(t: &Tile, e: usize, seen: &mut BTreeSet<usize>, out: &mut Vec<usize>) {
            if !seen.insert(e) {
                return;
            }

            if t.is_instance(e) {
                out.push(e);
            }

            for &c in &t.tscene().doc().elements()[e].children {
                walk(t, c, seen, out);
            }
        }

        for &e in elems {
            if e < self.tscene().doc().elements().len() {
                walk(self, e, &mut seen, &mut out);
            }
        }

        out
    }

    /// Landscape instances further from the terrain than tolerance, worst first.
    pub fn off_terrain(&self, tolerance: f32, among: Option<&[usize]>) -> Vec<OffTerrain> {
        let list = among.unwrap_or(&self.instances);
        let mut out = list.iter().copied().filter(|&e| self.placement(e) == Placement::Landscape).filter_map(|e| {
            let p = self.world_position(e);
            let dy = p.y - self.landscape_y(e, p.x, p.z);
            (dy.abs() > tolerance).then_some(OffTerrain { elem: e, dy })
        }).collect::<Vec<_>>();

        out.sort_by(|a, b| b.dy.abs().partial_cmp(&a.dy.abs()).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    pub fn placement_counts(&self) -> PlacementCounts {
        let mut c = PlacementCounts::default();
        for &e in &self.instances {
            match self.placement(e) {
                Placement::Landscape => c.landscape += 1,
                Placement::Freeform => c.freeform += 1,
                Placement::None => c.other += 1,
            }
        }

        c
    }

    /// "Walls/Standard_Walls": names from the scene graph down.
    pub fn find_layer(&self, path: &str) -> Option<usize> {
        let sc = self.tscene();
        let mut cur = sc.scene_graph()?;
        for part in path.split('/') {
            if part.is_empty() || part == "root" {
                continue;
            }

            cur = sc.children(cur).into_iter().find(|&c| sc.is_layer(c) && sc.name(c) == part)?;
        }

        Some(cur)
    }

    /// LayerExportType_Normal | Escalation1..3, without the prefix.
    pub fn export_type(&self, layer: usize) -> String {
        let t = self.tscene().doc().get(layer, "export_type").unwrap_or_else(|| "LayerExportType_Normal".to_owned());
        t.strip_prefix("LayerExportType_").map(str::to_owned).unwrap_or(t)
    }

    // ---- height strokes

    fn apply_brush(&mut self, kind: BrushKind, x: f32, z: f32, s: &BrushSettings, dt: f32) {
        let p = &mut self.project;
        let (cx, cy, mpp) = (p.pixel_x(f64::from(x)), p.pixel_z(f64::from(z)), p.mpp_x());
        let Some(hf) = &mut p.height else { return };
        if !self.stroke_open {
            let _ = hf.begin_edit();
            self.stroke_open = true;
        }

        let mut b = Brush { cx, cy, radius: (f64::from(s.radius_m) / mpp).max(1.0), falloff: f64::from(s.falloff), ..Default::default() };
        let dt = dt.min(0.05);
        let soft = (f64::from(s.strength) * f64::from(dt) * 0.5).min(1.0);
        let _ = match kind {
            BrushKind::Raise => {
                b.strength = 1.0;
                hf.raise(&b, f64::from(s.strength * dt))
            }

            BrushKind::Lower => {
                b.strength = 1.0;
                hf.raise(&b, f64::from(-s.strength * dt))
            }

            BrushKind::Smooth => {
                b.strength = soft;
                hf.smooth(&b)
            }

            BrushKind::Flatten => {
                b.strength = soft;
                hf.flatten(&b, f64::from(s.flatten_height))
            }

            BrushKind::Level => {
                b.strength = soft;
                hf.set_level_under(&b)
            }
        };

        p.height_dirty = true;
    }

    /// The brush step of a stroke: the landscape instances under the brush move with the terrain (y += the change of the height under each).
    pub(crate) fn apply_brush_following(&mut self, kind: BrushKind, x: f32, z: f32, s: &BrushSettings, dt: f32, moved: &mut Vec<Follow>) {
        let r = s.radius_m + 2.0 * self.project.mpp_x() as f32 + 1.0;
        let cands = self.instances.iter().copied().filter(|&e| self.placement(e) == Placement::Landscape).filter_map(|e| {
            let p = self.world_position(e);
            ((p.x - x).abs() <= r && (p.z - z).abs() <= r).then(|| (e, p.x, p.z, self.terrain_height(p.x, p.z)))
        }).collect::<Vec<_>>();

        self.apply_brush(kind, x, z, s, dt);
        for (e, cx, cz, h) in cands {
            let dh = self.terrain_height(cx, cz) - h;
            if dh.abs() < 1e-6 {
                continue;
            }

            let Some(before) = self.tscene().position(e) else { continue };
            let after = self.local_for_world_y(e, f64::from(self.world_position(e).y) + f64::from(dh));
            let before_text = self.tscene().doc().get(e, "position");
            self.project.scene.set_position(e, after);
            moved.push(Follow { elem: e, before, after, before_text });
        }

        for f in moved.iter() {
            self.index_edited(f.elem);
        }
    }

    pub(crate) fn put_positions(&mut self, moved: &[Follow], after: bool) {
        for f in moved {
            match &f.before_text {
                Some(text) if !after && !text.is_empty() => self.project.scene.set_attr(f.elem, "position", text),
                _ => self.project.scene.set_position(f.elem, if after { f.after } else { f.before }),
            }

            self.index_edited(f.elem);
        }
    }

    /// Closes the open stroke's height record (the history's merge is closed by its caller).
    pub(crate) fn end_stroke_edit(&mut self) {
        if !self.stroke_open {
            return;
        }

        if let Some(h) = &mut self.project.height {
            h.end_edit();
        }

        self.stroke_open = false;
    }

    fn height_modified(&self) -> bool {
        self.project.height.as_ref().is_some_and(|h| h.edit_depth() != self.saved_height_depth)
    }

    fn save_scene_needed(&self) -> bool {
        self.tscene().modified() || self.ted_on_disk_edited
    }
}

impl TileDocument {
    fn tscene_mut(&mut self) -> &mut TedScene {
        &mut self.tile_mut().project.scene
    }

    /// A structural edit is recorded as the text splices it made (`TedScene::begin_edit` / `end_edit`): `mark` is what `begin_edit` returned.
    fn push_splice(&mut self, name: String, mark: usize, sel_before: Vec<u64>, sel_after: Vec<u64>) {
        let record = self.tscene_mut().end_edit(mark).unwrap_or_default();

        // Element indices moved: the index is rebuilt by the republish that follows.
        self.tile_mut().idx.valid = false;
        self.history.push(Box::new(TedSpliceCommand { name, record, sel_before, sel_after, first: true }));
    }

    fn begin(&mut self) -> Result<usize, String> {
        self.tscene_mut().begin_edit()
    }

    /// Scene units / radians, local to the parent.
    pub fn set_local(&mut self, elem: usize, position: Option<Vec3>, euler: Option<Vec3>, gesture: u64) -> bool {
        let t = self.tile();
        if !t.is_instance(elem) {
            return false;
        }

        let sc = t.tscene();
        let old_pos = position.and(sc.position(elem));
        let old_euler = euler.and(sc.euler(elem));
        let (new_pos, new_euler) = (position.filter(|_| old_pos.is_some()), euler.filter(|_| old_euler.is_some()));
        if new_pos.is_none() && new_euler.is_none() {
            return false;
        }

        let n = sc.name(elem);
        let name = format!("Move {}", if n.is_empty() { sc.type_of(elem).to_owned() } else { n });
        let old_pos_text = old_pos.and_then(|_| sc.doc().get(elem, "position"));
        let old_euler_text = old_euler.and_then(|_| sc.doc().get(elem, "orientation"));
        self.history.push(Box::new(TedMoveCommand { name, elem, old_pos, new_pos, old_euler, new_euler, old_pos_text, old_euler_text, gesture }));
        true
    }

    /// Metres in the world; keeps the orientation.
    pub fn set_world_metres(&mut self, elem: usize, p_in: Float3, gesture: u64) -> bool {
        let t = self.tile();
        if !t.is_instance(elem) {
            return false;
        }

        let mut p = p_in;

        // TEd: x / z move a landscape instance over the terrain, a y that is typed or dragged is ignored.
        if t.placement(elem) == Placement::Landscape {
            let cur = t.world_position(elem);
            p.y = if (p.x - cur.x).abs() < 1e-4 && (p.z - cur.z).abs() < 1e-4 { cur.y } else { t.landscape_y(elem, p.x, p.z) };
        }

        let pw = parent_transform(t.tscene(), elem);
        let u = t.unit();
        let d = [f64::from(p.x) * u - pw.position[0], f64::from(p.y) - pw.position[1], f64::from(p.z) * u - pw.position[2]];
        let lp = rotate_vec(&mat_transpose(&pw.rotation), d);

        // Nothing to do: no empty undo entry.
        if t.tscene().position(elem).is_some_and(|cur| close(cur, lp)) {
            return false;
        }

        self.set_local(elem, Some(lp), None, gesture)
    }

    /// About the world Y axis. Without an orientation attribute the turn would add one outside the history (the command cannot take it back):
    /// such an instance is not turned.
    pub fn rotate(&mut self, elem: usize, deg: f32) -> bool {
        if !self.tile().is_instance(elem) {
            return false;
        }

        let sc = self.tscene_mut();
        let Some(old_euler) = sc.euler(elem) else { return false };
        rotate_world_yaw(sc, elem, radians(deg));
        let new_euler = sc.euler(elem).unwrap_or(old_euler);

        // The command applies it.
        sc.set_euler(elem, old_euler);
        self.set_local(elem, None, Some(new_euler), 0)
    }

    /// A copy 4 m to the side; returns its element.
    ///
    /// # Errors
    ///
    /// Nothing to duplicate.
    pub fn duplicate(&mut self, elem: usize) -> Result<usize, String> {
        let t = self.tile();
        if elem >= t.tscene().doc().elements().len() || (!t.is_instance(elem) && !t.tscene().is_layer(elem)) {
            return Err("nothing to duplicate".to_owned());
        }

        let mark = self.begin()?;
        let copy = self.tscene_mut().duplicate(elem).map_err(|_| "cannot duplicate this element".to_owned())?;
        let t = self.tile();
        if t.is_instance_slow(copy) {
            let w = world_transform(t.tscene(), copy);
            let u = t.unit();
            let mut y = w.position[1];

            // The copy stands on the terrain.
            if t.placement_of_attr(copy) == Placement::Landscape {
                y = f64::from(t.landscape_y(copy, ((w.position[0] + 4.0 * u) / u) as f32, (w.position[2] / u) as f32));
            }

            set_world_position(self.tscene_mut(), copy, [w.position[0] + 4.0 * u, y, w.position[2]]);
        }

        let name = format!("Duplicate {}", self.tile().tscene().type_of(copy));
        self.push_splice(name, mark, vec![item_of(elem)], vec![item_of(copy)]);
        Ok(copy)
    }

    pub fn remove(&mut self, elem: usize) -> bool {
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        let Ok(mark) = self.begin() else { return false };
        let label = self.tile().tscene().type_of(elem).to_owned();
        let _ = self.tscene_mut().remove(elem);
        self.push_splice(format!("Delete {label}"), mark, vec![item_of(elem)], vec![]);
        true
    }

    /// A new building copied from a template building (or the first one of the tile). `mode` None: it has the template's placement; a
    /// landscape one is put on the terrain at `at`.
    ///
    /// # Errors
    ///
    /// No key, or no building to copy.
    pub fn add_building(&mut self, key: &str, template_elem: Option<usize>, at: Option<Float3>, mode: Placement) -> Result<usize, String> {
        if key.is_empty() {
            return Err(String::new());
        }

        let sc = self.tile().tscene();
        let tmpl = template_elem.filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").or_else(|| sc.find_all("BUILDING_INSTANCE").first().copied())
            .ok_or("no BUILDING_INSTANCE to use as a template in this tile")?;
        let mark = self.begin()?;
        let copy = self.tscene_mut().duplicate(tmpl)?;
        let sc = self.tscene_mut();
        sc.set_attr(copy, "key", key);
        sc.set_attr(copy, "name", key);
        if mode != Placement::None && sc.doc().get(copy, "surface_mode").is_some() {
            sc.set_attr(copy, "surface_mode", if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" });
        }

        if let Some(at) = at {
            let t = self.tile();
            let y = if t.placement_of_attr(copy) == Placement::Landscape { t.landscape_y(copy, at.x, at.z) } else { at.y };
            let u = t.unit();
            set_world_position(self.tscene_mut(), copy, [f64::from(at.x) * u, f64::from(y), f64::from(at.z) * u]);
        }

        self.push_splice(format!("Add {key}"), mark, vec![template_elem.map_or(0, item_of)], vec![item_of(copy)]);
        Ok(copy)
    }

    /// A polygon from world points in `layer` (`None`: the scene graph).
    ///
    /// # Errors
    ///
    /// Fewer than 2 points, or no layer to put it in.
    pub fn add_polygon(&mut self, layer: Option<usize>, kind: &str, hint: &str, pts: &[Float3]) -> Result<usize, String> {
        if pts.len() < 2 {
            return Err("a polygon needs at least 2 points".to_owned());
        }

        let t = self.tile();
        let layer = layer.or(t.tscene().scene_graph()).ok_or("this tile has no SCENE_GRAPH, so there is no layer to add a polygon to")?;
        let local = pts.iter().map(|&p| t.to_local(layer, p)).collect::<Vec<_>>();
        let mut extra = BTreeMap::new();
        if kind == "AI_HINT_POLYGON_INSTANCE" && !hint.is_empty() {
            extra.insert("hint_type".to_owned(), hint.to_owned());
        }

        let mark = self.begin()?;
        let name = if kind == "AI_HINT_POLYGON_INSTANCE" { "AI hint polygon" } else { "Polygon" };
        let e = self.tscene_mut().add_polygon(Some(layer), kind, name, &local, &extra)?;
        self.push_splice(format!("Add {kind}"), mark, vec![], vec![item_of(e)]);
        Ok(e)
    }

    /// move | insert | remove a point of a polygon.
    ///
    /// # Errors
    ///
    /// Not a polygon, no such point, or an unknown operation.
    pub fn polygon_point(&mut self, elem: usize, op: &str, index: usize, world: Float3, gesture: u64) -> Result<(), String> {
        let t = self.tile();
        if !t.is_polygon(elem) {
            return Err("not a polygon".to_owned());
        }

        let pts = t.tscene().polygon_points(elem);
        if op == "move" {
            let &p = pts.get(index).ok_or("no such point")?;
            let before = t.tscene().point_position(p).unwrap_or([0.0; 3]);
            let after = t.to_local(elem, world);
            let (before_text, area_text) = (t.tscene().doc().get(p, "position"), t.tscene().doc().get(elem, "area"));
            self.history.push(Box::new(TedPointCommand { elem, index, before, after, before_text, area_text, gesture }));
            return Ok(());
        }

        let mark = self.begin()?;
        match op {
            "insert" => {
                if index >= pts.len() {
                    return Err("no such point".to_owned());
                }

                let local = self.tile().to_local(elem, world);
                self.tscene_mut().insert_polygon_point(elem, index, local)?;
                self.push_splice("Insert polygon point".to_owned(), mark, vec![item_of(elem)], vec![item_of(elem)]);
                Ok(())
            }

            "remove" => {
                if !self.tscene_mut().remove_polygon_point(elem, index)? {
                    return Err("a polygon needs at least 2 points".to_owned());
                }

                self.push_splice("Remove polygon point".to_owned(), mark, vec![item_of(elem)], vec![item_of(elem)]);
                Ok(())
            }

            _ => Err(format!("unknown point operation '{op}'")),
        }
    }

    /// # Errors
    ///
    /// No parent layer (a tile without SCENE_GRAPH).
    pub fn add_layer(&mut self, parent: Option<usize>, name: &str) -> Result<usize, String> {
        let parent = parent.ok_or("this tile has no SCENE_GRAPH, so layers cannot be added")?;
        let mark = self.begin()?;
        let layer = self.tscene_mut().add_layer(Some(parent), if name.is_empty() { "New layer" } else { name })?;
        self.push_splice("Add layer".to_owned(), mark, vec![], vec![item_of(layer)]);
        Ok(layer)
    }

    /// Keeps the world position and orientation.
    ///
    /// # Errors
    ///
    /// A layer moved into itself, or a move the text does not allow.
    pub fn move_to_layer(&mut self, elem: usize, layer: usize) -> Result<bool, String> {
        if elem == layer {
            return Ok(false);
        }

        if self.tile().tscene().is_descendant(layer, elem) {
            return Err("cannot move a layer into itself".to_owned());
        }

        let mark = self.begin()?;
        let sc = self.tile().tscene();
        let w = world_transform(sc, elem);
        let has_pos = sc.position(elem).is_some();
        let moved = self.tscene_mut().move_to_layer(elem, layer)?;
        let sc = self.tscene_mut();
        if has_pos {
            set_world_position(sc, moved, w.position);
        }

        if sc.euler(moved).is_some() {
            set_world_rotation(sc, moved, &w.rotation);
        }

        self.push_splice("Move to layer".to_owned(), mark, vec![item_of(elem)], vec![item_of(moved)]);
        Ok(true)
    }

    pub fn rename(&mut self, elem: usize, name: &str) -> bool {
        self.set_attribute(elem, "name", name)
    }

    pub fn set_attribute(&mut self, elem: usize, attr: &str, value: &str) -> bool {
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        if self.tile().tscene().doc().get(elem, attr).as_deref() == Some(value) {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        self.tscene_mut().set_attr(elem, attr, value);
        self.push_splice(format!("Set {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)]);
        true
    }

    pub fn remove_layer(&mut self, layer: usize) -> bool {
        let sc = self.tile().tscene();
        if layer >= sc.doc().elements().len() || !sc.is_layer(layer) || sc.type_of(layer) == "SCENE_GRAPH" {
            return false;
        }

        self.remove(layer)
    }

    // ---- layer flags and structure

    /// Layer flags (docs/formats/tile_ted.md, from toggling them in TEd): the eye is OIF_VISIBLE, the cursor arrow OIF_ACTIVE (selectable). A
    /// hidden layer hides everything below it (not drawn, not picked, still exported); an inactive one cannot be picked or moved with the
    /// gizmo. One undo entry; elements without a flags attribute are left alone.
    pub fn set_flags(&mut self, elems: &[usize], flag: &str, on: bool) -> bool {
        let t = self.tile();
        // Only elements with a flags attribute (layers and instances; TEd writes none on the scene graph): own_flag reads a missing attribute
        // as every flag set, which set_flag cannot write, and an added attribute is text the attribute edits of the command cannot take back.
        let todo = elems.iter().copied()
            .filter(|&e| e < t.tscene().doc().elements().len() && t.tscene().doc().get(e, "flags").is_some() && t.own_flag(e, flag) != on)
            .collect::<Vec<_>>();
        if todo.is_empty() {
            return false;
        }

        let eye = flag == "OIF_VISIBLE";
        let sc = t.tscene();
        let mut n = if todo.len() == 1 { sc.name(todo[0]) } else { format!("{} items", todo.len()) };
        if n.is_empty() && todo.len() == 1 {
            n = sc.type_of(todo[0]).to_owned();
        }

        let verb = match (eye, on) {
            (true, true) => "Show ",
            (true, false) => "Hide ",
            (false, true) => "Unlock ",
            (false, false) => "Lock ",
        };

        let name = format!("{verb}{n}");

        // The new flags text of every element; the stack applies it.
        let sc = self.tscene_mut();
        let (mut old, mut new) = (vec![], vec![]);
        for &e in &todo {
            let before = sc.doc().get(e, "flags");
            sc.set_flag(e, flag, on);
            new.push(sc.doc().get(e, "flags"));
            sc.set_attr(e, "flags", before.as_deref().unwrap_or(""));
            old.push(before);
        }

        self.history.push(Box::new(TedFlagCommand { name, elems: todo, old, new }));
        true
    }

    pub fn set_flag(&mut self, elem: usize, flag: &str, on: bool) -> bool {
        self.set_flags(&[elem], flag, on)
    }

    /// The moved elements keep their place in the world (their parents changed).
    fn keep_worlds(&mut self, moved: &[usize], worlds: &[WorldTransform]) {
        let sc = self.tscene_mut();
        for (&m, w) in moved.iter().zip(worlds) {
            if sc.position(m).is_some() {
                set_world_position(sc, m, w.position);
            }

            if sc.euler(m).is_some() {
                set_world_rotation(sc, m, &w.rotation);
            }
        }
    }

    /// Into a layer, in front of `before` (`None`: the end). One undo entry; the selection afterwards is the moved items.
    ///
    /// # Errors
    ///
    /// Nothing to move, a target that is not a layer, or a move the text does not allow.
    pub fn move_items(&mut self, elems: &[usize], layer: usize, before: Option<usize>) -> Result<(), String> {
        let sc = self.tile().tscene();
        if elems.is_empty() || layer >= sc.doc().elements().len() || !sc.is_layer(layer) {
            return Err("nothing to move, or the target is not a layer".to_owned());
        }

        if elems.iter().any(|&e| e >= sc.doc().elements().len() || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot move this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let moved = match self.tscene_mut().move_elements(top, layer, before) {
            Ok(moved) => moved,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        if moved.is_empty() {
            let _ = self.tscene_mut().end_edit(mark);
            return Err(String::new());
        }

        self.keep_worlds(&moved, &worlds);
        let sel_after = moved.iter().map(|&m| item_of(m)).collect();
        let name = if moved.len() == 1 { "Move to layer".to_owned() } else { format!("Move {} items to layer", moved.len()) };
        self.push_splice(name, mark, sel_before, sel_after);
        Ok(())
    }

    /// A new layer next to the first item holding all of them; returns it.
    ///
    /// # Errors
    ///
    /// Nothing selected, an element that can't be grouped, or a selection outside a layer.
    pub fn group_items(&mut self, elems: &[usize], name: &str) -> Result<usize, String> {
        let sc = self.tile().tscene();
        if elems.is_empty() {
            return Err("nothing selected".to_owned());
        }

        if elems.iter().any(|&e| e >= sc.doc().elements().len() || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot group this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        let els = sc.doc().elements();
        let mut parent = None;
        let mut p = els[top[0]].parent;
        while let Some(q) = p {
            if sc.is_layer(q) {
                parent = Some(q);
                break;
            }

            p = els[q].parent;
        }

        let parent = parent.ok_or("the selection is not inside a layer")?;
        if top.iter().any(|&e| e == parent || sc.is_descendant(parent, e)) {
            return Err("cannot group a layer into a layer inside itself".to_owned());
        }

        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<Vec<usize>, String> {
            let sc = self.tscene_mut();

            // Appended to the parent: the elements after it shift by its size.
            let layer = sc.add_layer(Some(parent), if name.is_empty() { "New layer" } else { name })?;
            let shift = sc.doc().subtree_size(layer);
            let shifted = top.iter().map(|&e| if e >= layer { e + shift } else { e }).collect();
            sc.move_elements(shifted, layer, None)
        })();

        let moved = match result {
            Ok(moved) => moved,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        self.keep_worlds(&moved, &worlds);

        // moved[0] sits in <children> of the new layer.
        let els = self.tile().tscene().doc().elements();
        let new_layer = els[els[moved[0]].parent.expect("a children wrapper")].parent.expect("the new layer");
        self.push_splice("Group into layer".to_owned(), mark, sel_before, vec![item_of(new_layer)]);
        Ok(new_layer)
    }

    /// One undo entry; the selection afterwards is empty.
    pub fn remove_items(&mut self, elems: &[usize]) -> bool {
        let sc = self.tile().tscene();
        let valid = elems.iter().copied().filter(|&e| e < sc.doc().elements().len() && sc.type_of(e) != "SCENE_GRAPH").collect();
        let top = top_level(sc, valid);
        if top.is_empty() {
            return false;
        }

        let name = if top.len() == 1 { format!("Delete {}", sc.type_of(top[0])) } else { format!("Delete {} items", top.len()) };
        let sel_before = top.iter().map(|&e| item_of(e)).collect();
        let Ok(mark) = self.begin() else { return false };

        // From the last one: the indices of the others stay.
        for &e in top.iter().rev() {
            let _ = self.tscene_mut().remove(e);
        }

        self.push_splice(name, mark, sel_before, vec![]);
        true
    }

    /// # Errors
    ///
    /// Not a layer, or a layer that can't be copied.
    pub fn duplicate_layer(&mut self, layer: usize) -> Result<usize, String> {
        let sc = self.tile().tscene();
        if layer >= sc.doc().elements().len() || !sc.is_layer(layer) || sc.type_of(layer) == "SCENE_GRAPH" {
            return Err("not a layer".to_owned());
        }

        let mark = self.begin()?;
        let copy = self.tscene_mut().duplicate(layer).map_err(|_| "cannot duplicate this layer".to_owned())?;
        let name = format!("{} copy", self.tile().tscene().name(layer));
        self.tscene_mut().set_attr(copy, "name", &name);
        self.push_splice("Duplicate layer".to_owned(), mark, vec![item_of(layer)], vec![item_of(copy)]);
        Ok(copy)
    }

    // ---- placement

    /// Sets the mode of the instances among `elems` (layers: everything below); Landscape snaps y. One undo entry; returns how many instances
    /// changed.
    pub fn set_placement(&mut self, elems: &[usize], mode: Placement) -> usize {
        if mode == Placement::None {
            return 0;
        }

        let t = self.tile();
        let text = if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" };
        let mut items = vec![];
        for e in t.instances_below(elems) {
            if t.tscene().doc().get(e, "surface_mode").is_none() {
                continue;
            }

            let cur = t.placement(e);
            let code = |p: Placement| match p {
                Placement::Landscape => 1,
                Placement::Freeform => 2,
                Placement::None => 0,
            };

            let mut it = PlacementItem { elem: e, mode_old: None, mode_new: None, pos_old: None, pos_new: None, idx_old: code(cur), idx_new: code(mode), pos_old_text: None };
            if cur != mode {
                it.mode_old = t.tscene().doc().get(e, "surface_mode");
                it.mode_new = Some(text.to_owned());
            }

            let file_y = world_transform(t.tscene(), e).position[1] as f32;
            if mode == Placement::Landscape {
                // Freeform -> Landscape snaps (TEd writes the terrain height); a landscape item is shown on the terrain even when its file is not.
                let p = t.world_position(e);
                let y = t.landscape_y(e, p.x, p.z);
                if cur != mode || (y - file_y).abs() > 1e-5 {
                    it.pos_old = t.tscene().position(e);
                    it.pos_old_text = t.tscene().doc().get(e, "position");
                    it.pos_new = Some(t.local_for_world_y(e, f64::from(y)));
                }
            } else if cur == Placement::Landscape {
                // Landscape -> Freeform keeps the height it is shown at (the file's y may be a stale one: see snap_landscape_world).
                let y = t.world_position(e).y;
                if (y - file_y).abs() > 1e-5 {
                    it.pos_old = t.tscene().position(e);
                    it.pos_old_text = t.tscene().doc().get(e, "position");
                    it.pos_new = Some(t.local_for_world_y(e, f64::from(y)));
                }
            }

            if it.mode_new.is_some() || it.pos_new.is_some() {
                items.push(it);
            }
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = format!("{}{}", if mode == Placement::Landscape { "Landscape placement" } else { "Freeform placement" }, if n == 1 { String::new() } else { format!(" ({n} items)") });
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    /// y = the terrain height; returns how many moved.
    pub fn snap_to_terrain(&mut self, elems: &[usize], include_freeform: bool) -> usize {
        let t = self.tile();
        let mut items = vec![];
        for e in t.instances_below(elems) {
            let cur = t.placement(e);
            if cur != Placement::Landscape && !(include_freeform && cur != Placement::None) {
                continue;
            }

            let p = t.world_position(e);
            let y = t.landscape_y(e, p.x, p.z);

            // The file's own y (see snap_landscape_world): this writes the shown height into it.
            if (y - world_transform(t.tscene(), e).position[1] as f32).abs() < 1e-5 {
                continue;
            }

            let code = if cur == Placement::Landscape { 1 } else { 2 };
            items.push(PlacementItem {
                elem: e, mode_old: None, mode_new: None, pos_old: t.tscene().position(e), pos_new: Some(t.local_for_world_y(e, f64::from(y))), idx_old: code, idx_new: code,
                pos_old_text: t.tscene().doc().get(e, "position"),
            });
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = format!("Snap to terrain{}", if n == 1 { String::new() } else { format!(" ({n} items)") });
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    // ---- height strokes

    /// A step of a height stroke: begins the stroke on the first step; `end_stroke` when the mouse goes up (also done by undo / redo and save).
    pub fn brush_step(&mut self, kind: BrushKind, x: f32, z: f32, settings: BrushSettings, dt: f32) {
        if !self.tile().project.has_height() {
            return;
        }

        // A stroke goes on only while its entry is the last change: after another edit the next step would be a new entry sharing the height
        // field's open record with the old one, and undo would take back both strokes' heights with the newer entry.
        if self.tile().stroke_open && self.history.version() != self.tile().stroke_version {
            self.end_stroke();
        }

        let t = self.tile_mut();
        if !t.stroke_open {
            t.stroke_id += 1;
        }

        let stroke = t.stroke_id;
        self.history.push(Box::new(BrushStepCommand { kind, x, z, settings, dt, stroke, first: true, moved: vec![], where_: HashMap::new() }));
        let version = self.history.version();
        self.tile_mut().stroke_version = version;
    }

    pub fn end_stroke(&mut self) {
        if !self.tile().stroke_open {
            return;
        }

        self.tile_mut().end_stroke_edit();
        self.history.break_merge();
    }

    // ---- save

    /// Saves tile.ted and the height map (in place, or into the folder of `target`); the message says what was written.
    pub(crate) fn save_tile(&mut self, target: &SaveTarget) -> Result<String, String> {
        self.end_stroke();
        let elsewhere = !target.path.as_os_str().is_empty();
        let t = self.tile();
        let mut out = if elsewhere { target.path.clone() } else { t.project.dir.clone() };

        // "Save as..." hands over a file name: the tile goes into its folder.
        if elsewhere && out.extension().is_some() {
            out = out.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }

        let height_dirty = t.height_modified() || t.png_on_disk_edited || (elsewhere && t.project.has_height());
        let force_scene = t.save_scene_needed() || elsewhere;
        let t = self.tile_mut();
        t.project.height_dirty = height_dirty;
        let (wrote, msg) = t.project.save(&out, force_scene, false)?;
        if !elsewhere {
            t.ted_on_disk_edited = t.tscene().modified();
            t.png_on_disk_edited = t.height_modified();
            t.saved_height_depth = t.project.height.as_ref().map_or(0, |h| h.edit_depth());
            t.project.height_dirty = false;
            self.history.mark_saved();
        }

        Ok(if wrote { format!("{msg}({})", out.display()) } else { msg })
    }

    // ---- inspector and gizmo

    pub(crate) fn inspect_item(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        let Some(e) = elem_of(item.item) else {
            self.inspect_terrain(ins);
            return;
        };

        if e >= self.tile().tscene().doc().elements().len() {
            return;
        }

        let t = self.tile();
        let sc = t.tscene();
        ins.section(sc.type_of(e));
        ins.text("name", "name", &sc.name(e));
        if let Some(k) = sc.doc().get(e, "key") {
            let model = match t.building_model(&k) {
                Some(m) if m.ok => format!("  ({} pieces)", m.pieces.len()),
                Some(_) => "  (no model: box)".to_owned(),
                None => String::new(),
            };

            ins.text("key", "key", &format!("{k}{model}"));
        }

        ins.text("layer", "layer", &sc.layer_path(e));

        // TEd's eye and cursor arrow of the Scene tab.
        if sc.doc().get(e, "flags").is_some() {
            let (mut vis, mut act) = (t.own_flag(e, "OIF_VISIBLE"), t.own_flag(e, "OIF_ACTIVE"));
            let (shown, selectable) = (t.shown(e), t.selectable(e));
            if ins.edit_bool("flag_visible", "visible (eye)", &mut vis) {
                self.set_flag(e, "OIF_VISIBLE", vis);
            }

            if ins.edit_bool("flag_active", "selectable (cursor)", &mut act) {
                self.set_flag(e, "OIF_ACTIVE", act);
            }

            if !shown {
                ins.text("hidden", "", "hidden by a layer above");
            } else if !selectable {
                ins.text("locked", "", "locked by a layer above");
            }
        }

        let t = self.tile();
        let sc = t.tscene();
        if sc.is_layer(e) && sc.type_of(e) != "SCENE_GRAPH" && sc.doc().get(e, "export_type").is_some() {
            let kinds = ["Normal", "Escalation1", "Escalation2", "Escalation3"].map(str::to_owned);
            let cur = t.export_type(e);
            let mut idx = kinds.iter().position(|k| *k == cur).unwrap_or(0) as i32;
            if ins.edit_choice("export_type", "export type", &mut idx, &kinds) && kinds[idx as usize] != cur {
                self.set_attribute(e, "export_type", &format!("LayerExportType_{}", kinds[idx as usize]));
            }
        }

        let base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x31;

        // TEd's X / Y / Z fields: the world position in metres.
        if self.tile().is_instance(e) {
            let w = self.tile().world_position(e);
            let mut v = [w.x, w.y, w.z];
            if ins.edit_float3("position_world", "position (world, m)", &mut v, 0.05) {
                self.set_world_metres(e, Float3::new(v[0], v[1], v[2]), base + 2);
            }

            if ins.gesture_ended() {
                self.history.break_merge();
            }
        }

        // TEd's surface_mode combo ("Landscape: Above" / "Freeform").
        if self.tile().is_instance(e) && self.tile().tscene().doc().get(e, "surface_mode").is_some() {
            let modes = ["Landscape: Above", "Freeform"].map(str::to_owned);
            let mut idx = i32::from(self.tile().placement(e) == Placement::Freeform);
            if ins.edit_choice("surface_mode", "placement", &mut idx, &modes) {
                self.set_placement(&[e], if idx == 0 { Placement::Landscape } else { Placement::Freeform });
            }

            let t = self.tile();
            if t.placement(e) == Placement::Landscape {
                let w = t.world_position(e);
                let dy = w.y - t.landscape_y(e, w.x, w.z);
                if dy.abs() > 0.005 {
                    ins.text("off_terrain", "off the terrain", &format!("{dy:+.3} m from the terrain"));
                }
            }
        }

        if let Some(p) = self.tile().tscene().position(e) {
            let mut v = [p[0] as f32, p[1] as f32, p[2] as f32];
            if ins.edit_float3("position", "position (scene)", &mut v, 0.5) {
                self.set_local(e, Some([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]), None, base);
            }

            if ins.gesture_ended() {
                self.history.break_merge();
            }
        }

        if let Some(eu) = self.tile().tscene().euler(e) {
            let mut v = [degrees(eu[0]), degrees(eu[1]), degrees(eu[2])];
            if ins.edit_float3("euler", "euler (deg)", &mut v, 0.5) {
                self.set_local(e, None, Some([radians(v[0]), radians(v[1]), radians(v[2])]), base + 1);
            }

            if ins.gesture_ended() {
                self.history.break_merge();
            }
        }

        if self.tile().is_polygon(e) {
            let sc = self.tile().tscene();
            ins.text("points", "points", &sc.polygon_points(e).len().to_string());
            ins.text("area", "area", &format!("{:.1}", sc.polygon_area(e)));
            if sc.type_of(e) == "AI_HINT_POLYGON_INSTANCE" {
                let hints = [
                    "AIH_FORT_APPROACH", "AIH_DEFENSIVE_HILL", "AIH_FORT_PERIMETER", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_MELEE_LINE", "AIH_AMBUSH_MISSILE_LINE",
                    "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_FIREBALL_LINE", "AIH_BEACH_LANDING",
                    "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_RIVER_EDGE", "AIH_BRIDGE_CROSSING", "AIH_COVER_LINE", "AIH_BEACH_LANDING_FORT_APPROACH",
                ].map(str::to_owned);

                let cur = sc.doc().get(e, "hint_type").unwrap_or_default();
                let mut idx = hints.iter().position(|h| *h == cur).unwrap_or(0) as i32;
                if ins.edit_choice("hint_type", "hint type", &mut idx, &hints) && hints[idx as usize] != cur {
                    self.set_attribute(e, "hint_type", &hints[idx as usize]);
                }
            }
        }

        ins.section("Attributes");
        let sc = self.tile().tscene();
        for (i, a) in sc.doc().elements()[e].attrs.iter().enumerate() {
            ins.text(&format!("attr_{}", a.name), &a.name, &String::from_utf8_lossy(sc.doc().attr_raw(e, i)));
        }
    }

    fn inspect_terrain(&self, ins: &mut dyn Inspector) {
        let t = self.tile();
        let p = &t.project;
        ins.section("Terrain");
        match &p.height {
            Some(h) => {
                ins.text("height_map", "height map", &format!("{} x {} px", h.width(), h.height()));
                let found = if p.info.found { "" } else { "  [tile database entry not found]" };
                let range = format!("{:.2} .. {:.2} m (min {:.3} scale {:.3}){found}", h.min_height(), h.min_height() + h.scale(), p.info.min_height, p.info.scale);
                ins.text("range", "heights", &range);
                ins.text("scale", "resolution", &format!("{:.3} m/px, border {:.0} m", p.mpp_x(), p.border_m()));
            }

            None => ins.text("height_map", "height map", "none (no height_map_0.png)"),
        }

        let tile = format!("{} x {} cells, {} x {} m", p.info.cells_x, p.info.cells_y, t.extent_x() as i32, t.extent_z() as i32);
        ins.text("tile", "tile", &tile);
        ins.text("ted", "tile.ted", &format!("version {}, {} instances", t.tscene().project_version(), t.instances.len()));
    }

    /// The gizmo: moves (and turns) the items by a world-space delta.
    pub(crate) fn transform_selected(&mut self, items: &[SourceRef], delta: &Float4x4, gesture: u64) {
        for &r in items {
            let t = self.tile();
            let Some(before) = t.item_transform(r).filter(|_| r.doc == self.id()) else { continue };
            let Some(e) = elem_of(r.item) else { continue };
            let now = *delta * before;
            let pw = parent_transform(t.tscene(), e);
            let u = t.unit();

            // The new world position in scene units, local to the parent.
            let mut wp = [f64::from(now.m[0][3]) * u, f64::from(now.m[1][3]), f64::from(now.m[2][3]) * u];

            // As in TEd: x / z move a landscape instance over the terrain, y is the terrain's (a y handle does nothing). The float32 noise of a
            // y-axis drag at 1000 m is 6e-5.
            if t.placement(e) == Placement::Landscape {
                let same_xz = (now.m[0][3] - before.m[0][3]).abs() < 2e-4 && (now.m[2][3] - before.m[2][3]).abs() < 2e-4;
                wp[1] = if same_xz { f64::from(before.m[1][3]) } else { f64::from(t.landscape_y(e, now.m[0][3], now.m[2][3])) };
            }

            let d = [wp[0] - pw.position[0], wp[1] - pw.position[1], wp[2] - pw.position[2]];
            let lp = rotate_vec(&mat_transpose(&pw.rotation), d);
            let (mut wr, mut br): (Mat3, Mat3) = ([0.0; 9], [0.0; 9]);
            for a in 0..3 {
                for b in 0..3 {
                    wr[a * 3 + b] = f64::from(now.m[a][b]);
                    br[a * 3 + b] = f64::from(before.m[a][b]);
                }
            }

            let dr = wr.iter().zip(&br).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
            let le = (dr > 1e-5).then(|| euler_from_matrix(&mat_mul(&mat_transpose(&pw.rotation), &wr)));

            // A y drag on a landscape instance: nothing changed.
            if le.is_none() && t.tscene().position(e).is_some_and(|cur| close(cur, lp)) {
                continue;
            }

            self.set_local(e, Some(lp), le, gesture);
        }
    }
}

impl Tile {
    /// Every element's own flag, for the outliner (`TERRAIN` has none).
    pub fn item_flag(&self, item: u64, flag: &str) -> bool {
        elem_of(item).is_none_or(|e| item == TERRAIN || self.own_flag(e, flag))
    }
}
