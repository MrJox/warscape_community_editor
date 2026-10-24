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
use wce_foundation::math::{Double3, Double3x3, Float3, Float4x4};
use wce_scene::render_scene::SourceRef;
use wce_tile::height_field::Brush;
use wce_tile::properties::{
    Access, Kind, Prop, RuleContext, format_bool, format_colour, format_float, format_seasons, format_vec2, format_vec3, parse_bool, parse_colour, parse_float,
    parse_int, parse_seasons, parse_vec2, parse_vec3_text,
};
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::{WorldTransform, euler_from_matrix, rotate_world_yaw, set_world_position, set_world_rotation, world_transform};

use super::commands::{BrushStepCommand, PlacementItem, TedAttrCommand, TedFlagCommand, TedMoveCommand, TedPlacementCommand, TedPointAttrsCommand, TedPointCommand, TedSpliceCommand};
use super::{Create, BrushKind, BrushSettings, Follow, OffTerrain, Placement, PlacementCounts, TERRAIN, Tile, TileDocument, elem_of, item_of, piece_of};

fn degrees(r: f64) -> f32 {
    (r * 180.0 / std::f64::consts::PI) as f32
}

fn radians(d: f32) -> f64 {
    f64::from(d) * std::f64::consts::PI / 180.0
}

/// A number from an attribute name, to tell the gestures of different properties apart.
fn attr_hash(attr: &str) -> u64 {
    attr.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) & 0xff_ffff
}

fn parent_transform(sc: &TedScene, elem: usize) -> WorldTransform {
    sc.doc().elements()[elem].parent.map(|p| world_transform(sc, p)).unwrap_or_default()
}

fn close(a: Double3, b: Double3) -> bool {
    a.max_abs_diff(b) < 2e-4
}

/// A world position in scene units (`unit` per metre horizontally).
pub(crate) fn scene_point(p: Float3, unit: f64) -> Double3 {
    Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit)
}

/// The rotation (upper 3x3) of a world matrix.
fn rotation_of(m: &Float4x4) -> Double3x3 {
    Double3x3 { m: std::array::from_fn(|a| std::array::from_fn(|b| f64::from(m.m[a][b]))) }
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
    pub fn to_local(&self, elem: usize, world: Float3) -> Double3 {
        let w = world_transform(self.tscene(), elem);
        w.rotation.transpose() * (scene_point(world, self.unit()) - w.position)
    }

    /// Local position of `elem` with its world y replaced (x, z kept).
    pub(crate) fn local_for_world_y(&self, elem: usize, world_y: f64) -> Double3 {
        let pw = parent_transform(self.tscene(), elem);
        let mut w = world_transform(self.tscene(), elem);
        w.position.y = world_y * self.unit();
        pw.rotation.transpose() * (w.position - pw.position)
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
        let (cx, cy, mpp, unit) = (p.pixel_x(f64::from(x)), p.pixel_z(f64::from(z)), p.mpp_x(), p.unit_scale);
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
                // Heights are world units here, scene units in the height map.
                hf.raise(&b, f64::from(s.strength * dt) * unit)
            }

            BrushKind::Lower => {
                b.strength = 1.0;
                hf.raise(&b, f64::from(-s.strength * dt) * unit)
            }

            BrushKind::Smooth => {
                b.strength = soft;
                hf.smooth(&b)
            }

            BrushKind::Flatten => {
                b.strength = soft;
                hf.flatten(&b, f64::from(s.flatten_height) * unit)
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

        // The walls' layouts follow the ground.
        self.polygons_dirty |= self.has_walls();
    }

    fn height_modified(&self) -> bool {
        self.project.height.as_ref().is_some_and(|h| h.edit_depth() != self.saved_height_depth)
    }

    fn save_scene_needed(&self) -> bool {
        self.tscene().modified() || self.ted_on_disk_edited
    }
}

impl TileDocument {
    pub(crate) fn tscene_mut(&mut self) -> &mut TedScene {
        &mut self.tile_mut().project.scene
    }

    /// A structural edit is recorded as the text splices it made (`TedScene::begin_edit` / `end_edit`): `mark` is what `begin_edit` returned.
    pub(crate) fn push_splice(&mut self, name: String, mark: usize, sel_before: Vec<u64>, sel_after: Vec<u64>) {
        let record = self.tscene_mut().end_edit(mark).unwrap_or_default();

        // Element indices moved: the index is rebuilt by the republish that follows.
        self.tile_mut().idx.valid = false;
        self.history.push(Box::new(TedSpliceCommand { name, record, sel_before, sel_after, first: true }));
    }

    pub(crate) fn begin(&mut self) -> Result<usize, String> {
        self.tscene_mut().begin_edit()
    }

    /// Scene units / radians, local to the parent.
    pub fn set_local(&mut self, elem: usize, position: Option<Double3>, euler: Option<Double3>, gesture: u64) -> bool {
        let t = self.tile();

        // A layer or a group moves everything below it: TEd's grid has its position (and a group's orientation) too.
        let whole = !t.is_instance(elem) && t.tscene().is_layer(elem) && t.tscene().type_of(elem) != "SCENE_GRAPH";
        if !t.is_instance(elem) && !whole {
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
        self.history.push(Box::new(TedMoveCommand { name, elem, old_pos, new_pos, old_euler, new_euler, old_pos_text, old_euler_text, gesture, whole }));
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
        let lp = pw.rotation.transpose() * (scene_point(p, t.unit()) - pw.position);

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
        let old_text = sc.doc().get(elem, "orientation");
        rotate_world_yaw(sc, elem, radians(deg));
        let new_euler = sc.euler(elem).unwrap_or(old_euler);

        // The command applies it; the file's text goes back first, so that undo writes it again as it was.
        match old_text {
            Some(t) => sc.set_attr(elem, "orientation", &t),
            None => sc.set_euler(elem, old_euler),
        }

        self.set_local(elem, None, Some(new_euler), 0)
    }

    /// A copy 4 m to the side; returns its element.
    ///
    /// # Errors
    ///
    /// Nothing to duplicate.
    pub fn duplicate(&mut self, elem: usize) -> Result<usize, String> {
        self.check_structure(&[elem], None)?;
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
            let mut y = w.position.y;

            // The copy stands on the terrain.
            if t.placement_of_attr(copy) == Placement::Landscape {
                y = f64::from(t.landscape_y(copy, ((w.position.x + 4.0 * u) / u) as f32, (w.position.z / u) as f32)) * u;
            }

            set_world_position(self.tscene_mut(), copy, Double3::new(w.position.x + 4.0 * u, y, w.position.z));
        }

        let name = format!("Duplicate {}", self.tile().tscene().type_of(copy));
        self.push_splice(name, mark, vec![item_of(elem)], vec![item_of(copy)]);
        Ok(copy)
    }

    pub fn remove(&mut self, elem: usize) -> bool {
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        if let Err(err) = self.check_structure(&[elem], None) {
            wce_foundation::log::warn(err);
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
        self.add_building_in(key, template_elem, at, mode, None)
    }

    /// [`Self::add_building`] into the layer `into` (the working layer, or the scene graph); `None`: next to the template.
    ///
    /// # Errors
    ///
    /// No key, or no building to copy.
    pub fn add_building_in(&mut self, key: &str, template_elem: Option<usize>, at: Option<Float3>, mode: Placement, into: Option<usize>) -> Result<usize, String> {
        if key.is_empty() {
            return Err(String::new());
        }

        let sc = self.tile().tscene();
        let tmpl = template_elem.filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").or_else(|| sc.find_all("BUILDING_INSTANCE").first().copied())
            .ok_or("no BUILDING_INSTANCE to use as a template in this tile")?;
        let mark = self.begin()?;
        let mut copy = self.tscene_mut().duplicate(tmpl)?;
        if let Some(into) = into.filter(|&l| self.tile().layer_of(copy) != Some(l)) {
            let world = world_transform(self.tile().tscene(), copy);
            copy = self.tscene_mut().move_element(copy, into, None)?;
            self.keep_worlds(&[copy], &[world]);
        }

        let sc = self.tscene_mut();
        sc.set_attr(copy, "key", key);
        sc.set_attr(copy, "name", key);
        if mode != Placement::None && sc.doc().get(copy, "surface_mode").is_some() {
            sc.set_attr(copy, "surface_mode", if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" });
        }

        if let Some(at) = at {
            let t = self.tile();
            let y = if t.placement_of_attr(copy) == Placement::Landscape { t.landscape_y(copy, at.x, at.z) } else { at.y };
            let p = scene_point(Float3::new(at.x, y, at.z), t.unit());
            set_world_position(self.tscene_mut(), copy, p);
        }

        self.push_splice(format!("Add {key}"), mark, vec![template_elem.map_or(0, item_of)], vec![item_of(copy)]);
        Ok(copy)
    }

    /// A polygon from world points in `layer` (`None`: the scene graph).
    /// `hint`: an AI hint polygon's hint type, a wall's (`WALL_SPLINE`) base wall.
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

        // A wall's ends go onto the fort buildings' ends near them.
        let mut pts = pts.to_vec();
        if kind == super::WALL_SPLINE {
            let last = pts.len() - 1;
            pts[0] = t.snap_wall_end(pts[0]);
            pts[last] = t.snap_wall_end(pts[last]);
        }

        let local = pts.iter().map(|&p| t.to_local(layer, p)).collect::<Vec<_>>();
        let mut extra = BTreeMap::new();
        if kind == "AI_HINT_POLYGON_INSTANCE" && !hint.is_empty() {
            extra.insert("hint_type".to_owned(), hint.to_owned());
        }

        // A wall's `hint` is its base wall.
        if kind == super::WALL_SPLINE && !hint.is_empty() {
            extra.insert("wall_base".to_owned(), hint.to_owned());
        }

        let mark = self.begin()?;
        let name = match kind {
            "AI_HINT_POLYGON_INSTANCE" => "AI hint polygon",
            super::WALL_SPLINE => "Wall",
            _ => "Polygon",
        };
        let e = self.tscene_mut().add_polygon(Some(layer), kind, name, &local, &extra)?;
        self.push_splice(format!("Add {kind}"), mark, vec![], vec![item_of(e)]);
        Ok(e)
    }

    /// Tilts an instance so that its up is `normal` (world), keeping its heading (where its +z points on the ground). The steps of one
    /// gesture merge.
    pub fn tilt_to_surface(&mut self, elem: usize, normal: Double3, gesture: u64) -> bool {
        let sc = self.tile().tscene();
        let Some(euler) = sc.euler(elem) else { return false };
        let world = world_transform(sc, elem).rotation;
        let parent = world * wce_tile::world_transform::euler_matrix(euler).transpose();
        let forward = world * Double3::new(0.0, 0.0, 1.0);
        let yaw = if forward.x.hypot(forward.z) > 1e-6 { forward.x.atan2(forward.z) } else { 0.0 };
        let tilted = super::draft::tilt_to(normal) * Double3x3::rotation_y(yaw);
        let local = euler_from_matrix(&(parent.transpose() * tilted));
        self.set_local(elem, None, Some(local), gesture)
    }

    /// move | insert | remove a point of a polygon.
    ///
    /// # Errors
    ///
    /// Not a polygon, no such point, or an unknown operation.
    pub fn polygon_point(&mut self, elem: usize, op: &str, index: usize, world: Float3, gesture: u64) -> Result<(), String> {
        let t = self.tile();
        if self.is_spline(elem) {
            return self.spline_point(elem, op, index, world, gesture);
        }

        if !t.is_polygon(elem) {
            return Err("not a polygon".to_owned());
        }

        let pts = t.tscene().polygon_points(elem);
        if op == "move" {
            let &p = pts.get(index).ok_or("no such point")?;

            // A wall's end goes onto a fort building's end near it.
            let world = if t.is_wall(elem) && (index == 0 || index + 1 == pts.len()) { t.snap_wall_end(world) } else { world };
            let before = t.tscene().point_position(p).unwrap_or_default();
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

    // ---- splines (wce_tile::splines): rivers and roads

    pub fn is_spline(&self, elem: usize) -> bool {
        let sc = self.tile().tscene();
        elem < sc.doc().elements().len() && wce_tile::splines::is_spline(sc.type_of(elem))
    }

    /// A spline point edit, `world` the cursor:
    /// - `move`: the point, its handles with it;
    /// - `in` / `out`: its `controlIn` / `controlOut`, at the point's height, the other handle mirrored;
    /// - `insert`: a new point after `index` (after the last one: the spline grows), its handles made from its neighbours;
    /// - `prepend`: a new first point;
    /// - `remove`: the point (a spline keeps 2).
    ///
    /// The handles no segment uses (the first point's `controlIn`, the last one's `controlOut`) stay `0,0,0`, as TEd writes them.
    ///
    /// # Errors
    ///
    /// Not a spline, no such point, or an unknown operation.
    pub fn spline_point(&mut self, elem: usize, op: &str, index: usize, world: Float3, gesture: u64) -> Result<(), String> {
        use wce_tile::new_instances::point_text;
        use wce_tile::splines::{auto_handles, mirror, read};

        if !self.is_spline(elem) {
            return Err("not a spline".to_owned());
        }

        let t = self.tile();
        let sc = t.tscene();
        let pts = read(sc, elem);
        let n = pts.len();
        let elems = sc.polygon_points(elem);
        if index >= n.max(1) {
            return Err("no such point".to_owned());
        }

        let local = t.to_local(elem, world);
        let text = |i: usize, attr: &str| sc.doc().get(elems[i], attr).unwrap_or_default();
        let step = |i: usize, attr: &str, v: Double3| (i, attr.to_owned(), text(i, attr), point_text(v));
        let (first, last) = (index == 0, index + 1 == n);
        let attrs = match op {
            "move" => {
                let p = pts[index];
                let d = local - p.position;
                let mut steps = vec![step(index, "position", local)];
                if !first {
                    steps.push(step(index, "controlIn", p.control_in + d));
                }

                if !last {
                    steps.push(step(index, "controlOut", p.control_out + d));
                }

                Some(("Move spline point", steps))
            }

            "in" | "out" => {
                let p = pts[index];
                let h = Double3::new(local.x, p.position.y, local.z);
                let (this, other, used) = if op == "in" { ("controlIn", "controlOut", !last) } else { ("controlOut", "controlIn", !first) };
                if (op == "in" && first) || (op == "out" && last) {
                    return Err("an end's outer handle is not used".to_owned());
                }

                let mut steps = vec![step(index, this, h)];
                if used {
                    steps.push(step(index, other, mirror(p.position, h)));
                }

                Some(("Move spline handle", steps))
            }

            _ => None,
        };

        if let Some((name, steps)) = attrs {
            let steps = steps.into_iter().filter(|s| s.2 != s.3).collect::<Vec<_>>();
            if !steps.is_empty() {
                self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem, steps, gesture }));
            }

            return Ok(());
        }

        let set = |sc: &mut TedScene, i: usize, attr: &str, v: Double3| {
            let p = sc.polygon_points(elem)[i];
            sc.set_attr(p, attr, &point_text(v));
        };

        let mark = self.begin()?;
        let result = match op {
            "insert" => {
                let mut positions = pts.iter().map(|p| p.position).collect::<Vec<_>>();
                positions.insert(index + 1, local);
                let (cin, cout) = auto_handles(&positions, index + 1);
                let sc = self.tscene_mut();
                sc.insert_polygon_point(elem, index, local)?;
                set(sc, index + 1, "controlIn", cin);
                set(sc, index + 1, "controlOut", cout);
                if last {
                    // The old end is inside now: its outer handle mirrors its inner one (or is made, for a spline of one point).
                    let p = pts[index];
                    let out = if n > 1 { mirror(p.position, p.control_in) } else { auto_handles(&positions, index).1 };
                    set(sc, index, "controlOut", out);
                }

                Ok("Insert spline point")
            }

            "prepend" => {
                let mut positions = pts.iter().map(|p| p.position).collect::<Vec<_>>();
                positions.insert(0, local);
                let (_, cout) = auto_handles(&positions, 0);
                let p = pts[0];
                let sc = self.tscene_mut();

                // The copy after the first point keeps its values; the first one becomes the new point.
                sc.insert_polygon_point(elem, 0, p.position)?;
                set(sc, 0, "position", local);
                set(sc, 0, "controlIn", Double3::ZERO);
                set(sc, 0, "controlOut", cout);
                let cin = if n > 1 { mirror(p.position, p.control_out) } else { auto_handles(&positions, 1).0 };
                set(sc, 1, "controlIn", cin);
                Ok("Insert spline point")
            }

            "remove" => {
                let sc = self.tscene_mut();
                if sc.remove_polygon_point(elem, index)? {
                    if first {
                        set(sc, 0, "controlIn", Double3::ZERO);
                    } else if last {
                        set(sc, index - 1, "controlOut", Double3::ZERO);
                    }

                    Ok("Remove spline point")
                } else {
                    Err("a spline needs at least 2 points".to_owned())
                }
            }

            _ => Err(format!("unknown point operation '{op}'")),
        };

        match result {
            Ok(name) => {
                self.push_splice(name.to_owned(), mark, vec![item_of(elem)], vec![item_of(elem)]);
                Ok(())
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }

    /// A value of spline points (`width`, `foam_amount`, `alpha_fade`, `flow_speed`, `terrain_offset`): one undo entry, the steps of one
    /// gesture merge.
    pub fn set_spline_value(&mut self, elem: usize, points: &[usize], attr: &str, value: &str, gesture: u64) -> bool {
        if !self.is_spline(elem) {
            return false;
        }

        let sc = self.tile().tscene();
        let elems = sc.polygon_points(elem);
        let steps = points.iter().filter_map(|&i| {
            let before = sc.doc().get(*elems.get(i)?, attr).unwrap_or_default();
            Some((i, attr.to_owned(), before, value.to_owned()))
        }).collect::<Vec<_>>();
        if steps.is_empty() {
            return false;
        }

        self.history.push(Box::new(TedPointAttrsCommand { name: format!("Set {attr}"), elem, steps, gesture }));
        true
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

        self.check_structure(&[elem], Some(layer))?;

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

    /// Sets an attribute of a polygon's point (a wall's `sharp`), the polygon staying selected.
    pub fn set_point_attribute(&mut self, elem: usize, index: usize, attr: &str, value: &str) -> bool {
        let Some(&p) = self.tile().tscene().polygon_points(elem).get(index) else { return false };
        if self.tile().tscene().doc().get(p, attr).as_deref() == Some(value) {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        self.tscene_mut().set_attr(p, attr, value);
        self.push_splice(format!("Set point {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)]);
        true
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
        if let Err(err) = self.check_structure(&[layer], None) {
            wce_foundation::log::warn(err);
            return false;
        }

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
    pub(super) fn keep_worlds(&mut self, moved: &[usize], worlds: &[WorldTransform]) {
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
        self.check_structure(elems, Some(layer))?;
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
        self.check_structure(elems, None)?;
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

    /// Groups items into a new instance group (TEd's `UserInstanceGroup`) in the layer `into` (the working layer, or the scene graph), at the
    /// centre of the items, which keep their place in the world. One undo entry; the selection afterwards is the group.
    ///
    /// # Errors
    ///
    /// Nothing to group, the scene graph, or `into` inside one of the items.
    pub fn group_instances(&mut self, elems: &[usize], into: usize, name: &str) -> Result<usize, String> {
        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        if elems.is_empty() || into >= n || !sc.is_layer(into) {
            return Err("nothing to group, or no layer to put the group in".to_owned());
        }

        if elems.iter().any(|&e| e >= n || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot group this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        if top.iter().any(|&e| e == into || sc.is_descendant(into, e)) {
            return Err("the working layer is inside the selection".to_owned());
        }

        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let k = worlds.len() as f64;
        let centre = worlds.iter().fold(Double3::default(), |a, w| a + w.position) * (1.0 / k);
        let pw = world_transform(sc, into);
        let local = pw.rotation.transpose() * (centre - pw.position);
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<(usize, Vec<usize>), String> {
            let sc = self.tscene_mut();

            // Appended to `into`: the elements after it shift by its size.
            let group = sc.add_group(Some(into), if name.is_empty() { "Instance Group" } else { name }, local)?;
            let shift = sc.doc().subtree_size(group);
            let shifted = top.iter().map(|&e| if e >= group { e + shift } else { e }).collect();
            let moved = sc.move_elements(shifted, group, None)?;
            Ok((group, moved))
        })();

        let (group, moved) = match result {
            Ok(r) => r,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        self.keep_worlds(&moved, &worlds);

        // The group sits before its moved contents when it came after them.
        let els = self.tile().tscene().doc().elements();
        let group = els[els[moved[0]].parent.expect("a children wrapper")].parent.unwrap_or(group);
        self.push_splice("Group".to_owned(), mark, sel_before, vec![item_of(group)]);
        Ok(group)
    }

    /// Takes instance groups apart: their contents go into the layer `into` (the working layer, or the scene graph), keeping their place in
    /// the world, and the groups are deleted. Only instance groups (`UserInstanceGroup`, `INSTANCE_GROUP`) among `elems` are taken apart. One
    /// undo entry; the selection afterwards is what they held.
    ///
    /// # Errors
    ///
    /// No group among the elements, or `into` inside one of them.
    pub fn ungroup(&mut self, elems: &[usize], into: usize) -> Result<Vec<usize>, String> {
        self.check_structure(elems, Some(into))?;
        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        let groups = top_level(sc, elems.iter().copied().filter(|&e| e < n && matches!(sc.type_of(e), "UserInstanceGroup" | "INSTANCE_GROUP")).collect());
        if groups.is_empty() || into >= n || !sc.is_layer(into) {
            return Err("no instance group selected".to_owned());
        }

        if groups.iter().any(|&g| g == into || sc.is_descendant(into, g)) {
            return Err("the working layer is inside a selected group".to_owned());
        }

        let sel_before = groups.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let group_worlds = groups.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<Vec<usize>, String> {
            // 1. The groups move into `into` together (they keep their place): they are its last children now.
            let moved_groups = self.tscene_mut().move_elements(groups.clone(), into, None)?;
            self.keep_worlds(&moved_groups, &group_worlds);

            // 2. From the last group to the first: its contents go to the end of `into` and the empty group goes. Every change is after the
            // groups still to do, so their indices hold.
            let mut freed: Vec<usize> = vec![];
            for &g in moved_groups.iter().rev() {
                let sc = self.tile().tscene();
                let kids = sc.children(g);
                let worlds = kids.iter().map(|&k| world_transform(sc, k)).collect::<Vec<_>>();
                if !kids.is_empty() {
                    // The contents freed before sit after this group: cutting its contents moves them down by their size.
                    let cut = kids.iter().map(|&k| sc.doc().subtree_size(k)).sum::<usize>();
                    let moved = self.tscene_mut().move_elements(kids, into, None)?;
                    self.keep_worlds(&moved, &worlds);
                    for f in &mut freed {
                        *f -= cut;
                    }

                    freed.extend(moved);
                }

                // Removing the (now empty) group moves what comes after it down by its size.
                let size = self.tile().tscene().doc().subtree_size(g);
                self.tscene_mut().remove(g)?;
                for f in &mut freed {
                    if *f > g {
                        *f -= size;
                    }
                }
            }

            Ok(freed)
        })();

        match result {
            Ok(freed) => {
                self.push_splice(if groups.len() == 1 { "Ungroup".to_owned() } else { format!("Ungroup {} groups", groups.len()) }, mark, sel_before, freed.iter().map(|&m| item_of(m)).collect());
                Ok(freed)
            }

            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
        }
    }

    // ---- new instances of any type (wce_tile::new_instances, with the kit's defaults)

    /// Makes a new instance of `kind` in the layer `into`: a point type (`Create::Point`, on the ground at a world point) or a drawn type
    /// (`Create::Line`, world points on the ground). `key` is the model, decal, tree or effect where the type has one. The types made of
    /// several elements are made whole: a capture location with its 20 m boundary circle, a siege AI node with a 20 m square area, a
    /// deployment zone group with a 64 m square polygon (a drawn deployment polygon goes into the selected group, `into`, or into a new
    /// group), a siege AI edge with its drawn boundary and a second one 10 m to its right. One undo entry; the selection afterwards is it.
    ///
    /// # Errors
    ///
    /// A type that can't be made this way, too few points, or no such layer.
    pub fn create(&mut self, kind: &str, into: usize, shape: &Create, key: &str) -> Result<usize, String> {
        self.create_with(kind, into, shape, key, &BTreeMap::new())
    }

    /// [`Self::create`] with these attributes over the type's defaults (a draft's values, `Draft::attrs`).
    ///
    /// # Errors
    ///
    /// As `create`.
    pub fn create_with(&mut self, kind: &str, into: usize, shape: &Create, key: &str, attrs: &BTreeMap<String, String>) -> Result<usize, String> {
        self.check_structure(&[], Some(into))?;
        use wce_tile::new_instances::{NewInstance, NewPoint, element_text};

        let t = self.tile();
        let unit = t.unit();
        let local = |p: Float3| t.to_local(into, Float3::new(p.x, p.y, p.z));

        // A drawn shape: its centre (the element's position) and the points around it.
        let line = |pts: &[Float3]| -> Result<(Double3, Vec<Double3>), String> {
            if pts.len() < 2 {
                return Err("draw at least 2 points".to_owned());
            }

            let local = pts.iter().map(|&p| local(p)).collect::<Vec<_>>();
            let c = local.iter().fold(Double3::default(), |a, &p| a + p) * (1.0 / local.len() as f64);
            Ok((c, local.into_iter().map(|p| p - c).collect()))
        };

        let into_type = t.tscene().type_of(into).to_owned();
        let element = match (kind, shape) {
            (_, Create::Point(p)) => point_instance(kind, local(*p), key, unit)?,
            (
                "ZONE_POLYGON_INSTANCE" | "CivilianDeploymentZone" | "LiteBuildingZone" | "AI_HINT_POLYGON_INSTANCE" | "NOGO_REGION_POLYGON_INSTANCE"
                | "GO_REGION_POLYGON_INSTANCE" | "VegetationExclusionPolygonInstance" | "SIEGE_AI_POLYGON_INSTANCE",
                Create::Line(pts),
            ) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, true).position(c)
            }

            (super::WALL_SPLINE, Create::Line(pts)) => {
                // A wall's ends go onto the fort buildings' ends near them.
                let mut pts = pts.clone();
                if let [first, .., last] = pts.as_mut_slice() {
                    *first = t.snap_wall_end(*first);
                    *last = t.snap_wall_end(*last);
                }

                let (c, points) = line(&pts)?;
                let mut n = new_polygon(kind, points, false).position(c);
                for (k, v) in super::walls::wall_defaults() {
                    n.attrs.entry(k).or_insert(v);
                }

                n
            }

            ("AISeparatorInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c)
            }

            ("DEPLOYMENT_ZONE_POLYGON_INSTANCE", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                if into_type == "DeploymentZonePolygonInstanceGroup" {
                    new_polygon(kind, points, true).position(c)
                } else {
                    // A new group around it.
                    let mut g = NewInstance::new("DeploymentZonePolygonInstanceGroup").position(c);
                    g.children.push(element_text(&new_polygon(kind, points, true))?);
                    g
                }
            }

            ("SiegeAIEdgeInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;

                // The second boundary: the first moved 10 m to the right of its direction.
                let (a, b) = (points[0], points[points.len() - 1]);
                let d = b - a;
                let len = (d.x * d.x + d.z * d.z).sqrt().max(1e-9);
                let off = Double3::new(-d.z / len, 0.0, d.x / len) * (10.0 * unit);
                let other = points.iter().map(|&p| p + off).collect::<Vec<_>>();
                let mut n = NewInstance::new(kind).position(c);
                for b in [points, other] {
                    n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", b, false))?);
                }

                n
            }

            ("SplineRiverInstance" | "SEGMENTED_SPLINE_INSTANCE", Create::Line(pts)) => {
                // TEd's layout: the instance at the first point, the points from it.
                if pts.len() < 2 {
                    return Err("draw at least 2 points".to_owned());
                }

                let at = local(pts[0]);
                let positions = pts.iter().map(|&p| local(p) - at).collect::<Vec<_>>();
                let mut n = NewInstance::new(kind).position(at).attr("lf_offset", wce_tile::ted_scene::format_number(at.y));
                n.points = wce_tile::splines::new_points(&positions).into_iter().map(|p| {
                    let mut q = NewPoint::at(p.position);
                    q.attrs.insert("controlIn".to_owned(), wce_tile::new_instances::point_text(p.control_in));
                    q.attrs.insert("controlOut".to_owned(), wce_tile::new_instances::point_text(p.control_out));
                    q
                }).collect();
                n
            }

            (_, Create::Line(_)) => return Err(format!("{kind} is placed, not drawn")),
        };

        let mut element = element;
        if element.kind == kind {
            element.attrs.extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        let text = element_text(&element)?;
        let mark = self.begin()?;
        match self.tscene_mut().insert_element_text(into, &text) {
            Ok(e) => {
                self.push_splice(format!("Add {kind}"), mark, vec![], vec![item_of(e)]);
                Ok(e)
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }

    /// The text of what a click of the Create section places (a type placed by a click, at the origin), for the preview.
    pub fn point_instance_text(&self, kind: &str, key: &str) -> Option<Vec<u8>> {
        let n = point_instance(kind, Double3::ZERO, key, self.tile().unit()).ok()?;
        wce_tile::new_instances::element_text(&n).ok()
    }

    // ---- prefabs (wce_tile::prefabs): a copy is an instance group named after the prefab's key

    /// The copies of a prefab in the tile: the references to it (case aside), in document order.
    pub fn prefab_copies(&self, key: &str) -> Vec<usize> {
        let sc = self.tile().tscene();
        sc.find_all("UserInstanceGroup").into_iter().filter(|&g| self.prefab_key(g).is_some_and(|k| k.eq_ignore_ascii_case(key))).collect()
    }

    /// The prefab an instance group is a reference to (`wce_prefab`). A group that is only named after a prefab is a plain group: the
    /// ones TEd copied become references when the tile opens from the Assembly Kit, or with `prefab_refs::recognise` (whether there is such
    /// a prefab is the caller's to check).
    pub fn prefab_key(&self, group: usize) -> Option<String> {
        let sc = self.tile().tscene();
        if sc.type_of(group) != "UserInstanceGroup" {
            return None;
        }

        sc.doc().get(group, wce_tile::project_files::PREFAB_ATTR).filter(|k| !k.is_empty())
    }

    /// Whether an instance group is a reference to a prefab (placed by the editor), not a copy TEd made.
    pub fn is_prefab_reference(&self, group: usize) -> bool {
        self.tile().tscene().doc().get(group, wce_tile::project_files::PREFAB_ATTR).is_some_and(|k| !k.is_empty())
    }

    /// Makes a copy a plain group: its reference goes (an empty `wce_prefab`, which tile.wce and TEd's file leave out), and its name.
    pub fn unpack_prefab_copy(&mut self, group: usize, name: &str) -> bool {
        if self.tile().tscene().doc().get(group, wce_tile::project_files::PREFAB_ATTR).is_some() {
            self.set_attribute(group, wce_tile::project_files::PREFAB_ATTR, "");
        }

        self.rename(group, name)
    }

    /// The copy being edited (Edit prefab), whose contents may change in structure too (`check_structure`); the tool says which.
    pub fn set_unlocked_reference(&mut self, copy: Option<usize>) {
        self.tile_mut().unlocked_reference = copy;
    }

    /// A structural edit (adding, taking away, moving or grouping items) of items inside a prefab reference, or into one, is refused: what
    /// changes in a reference's items is kept as overrides, values only. Not inside the copy being edited.
    ///
    /// # Errors
    ///
    /// An item (or `into`) is inside a reference: says which.
    pub fn check_structure(&self, elems: &[usize], into: Option<usize>) -> Result<(), String> {
        let doc = self.tile().tscene().doc();
        let els = doc.elements();
        let unlocked = self.tile().unlocked_reference;
        let locked = |e: usize, itself: bool| -> Option<usize> {
            let mut chain = std::iter::successors(if itself { Some(e) } else { els.get(e)?.parent }, |&p| els.get(p).and_then(|x| x.parent));
            let r = chain.find(|&p| self.is_prefab_reference(p))?;
            let open = std::iter::successors(Some(e), |&p| els.get(p).and_then(|x| x.parent)).any(|p| Some(p) == unlocked);
            (!open).then_some(r)
        };

        let hit = elems.iter().find_map(|&e| locked(e, false)).or_else(|| into.and_then(|i| locked(i, true)));
        match hit {
            Some(r) => Err(format!(
                "that is part of the prefab {}: a reference's items change only in their values (overrides); Edit prefab changes the prefab, Unpack prefab makes it a plain group",
                self.prefab_key(r).unwrap_or_default()
            )),

            None => Ok(()),
        }
    }

    /// The outermost prefab reference an element is inside (not the element itself), and the element's path in its contents.
    pub fn reference_above(&self, elem: usize) -> Option<(usize, Vec<usize>)> {
        let doc = self.tile().tscene().doc();
        let els = doc.elements();
        let r = std::iter::successors(els.get(elem)?.parent, |&p| els[p].parent).filter(|&p| self.is_prefab_reference(p)).last()?;
        Some((r, wce_tile::prefab_refs::path_in(doc, r, elem)?))
    }

    /// A prefab's contents to compare its references with, from the project's prefab source (`TileProject::prefab_base`), read again when
    /// they change.
    fn base_contents(&mut self, key: &str) -> Option<(Vec<u8>, std::sync::Arc<wce_tile::prefab_refs::BaseContents>)> {
        let src = self.project().prefab_base.clone()?;
        let bytes = (src.0)(key)?;
        if let Some((b, c)) = self.tile().prefab_cache.get(key)
            && *b == bytes
        {
            return Some((bytes, c.clone()));
        }

        let c = std::sync::Arc::new(wce_tile::prefab_refs::BaseContents::parse(&bytes).ok()?);
        self.tile_mut().prefab_cache.insert(key.to_owned(), (bytes.clone(), c.clone()));
        Some((bytes, c))
    }

    /// An item of a prefab reference's contents: the reference's key and the attributes it overrides, each with the prefab's value (`None`:
    /// the prefab's item has no such attribute). `None` when it is not in a reference, or the prefab has no such item there.
    pub fn overrides_of(&mut self, elem: usize) -> Option<(String, wce_tile::prefab_refs::Differences)> {
        let (r, path) = self.reference_above(elem)?;
        let key = self.prefab_key(r)?;
        let (_, base) = self.base_contents(&key)?;
        let diffs = base.differences(self.tile().tscene().doc(), elem, &path)?;
        Some((key, diffs))
    }

    /// The items of a prefab reference that override values of the prefab's (once each, in document order); empty when none do, or its
    /// contents differ by more than values.
    pub fn overridden_items(&self, reference: usize) -> Vec<usize> {
        let Some(key) = self.prefab_key(reference) else { return vec![] };
        let Some(base) = self.project().prefab_base.as_ref().and_then(|src| (src.0)(&key)) else { return vec![] };
        let sc = self.tile().tscene();
        let Ok(ovs) = wce_tile::prefab_refs::overrides(&sc.children_text(reference), &base) else { return vec![] };
        let mut out = ovs.iter().filter_map(|o| wce_tile::prefab_refs::element_at(sc.doc(), reference, &o.path)).collect::<Vec<_>>();
        out.dedup();
        out
    }

    /// How many values a prefab reference's contents override; `None` when they differ by more than values (or there is no prefab).
    pub fn override_count(&mut self, reference: usize) -> Option<usize> {
        let key = self.prefab_key(reference).filter(|_| self.is_prefab_reference(reference))?;
        let (bytes, _) = self.base_contents(&key)?;
        wce_tile::prefab_refs::overrides(&self.tile().tscene().children_text(reference), &bytes).ok().map(|o| o.len())
    }

    /// Gives an item of a prefab reference its prefab's values back (or, for the reference, all its contents): one undo entry. Returns how
    /// many values went back (a reference: 1 when its contents did).
    pub fn revert_overrides(&mut self, elem: usize) -> usize {
        if self.is_prefab_reference(elem) {
            let Some(key) = self.prefab_key(elem) else { return 0 };
            let Some((bytes, _)) = self.base_contents(&key) else { return 0 };
            return self.set_groups_children(&[elem], &bytes, &format!("Revert the overrides of {key}")).unwrap_or(0);
        }

        let Some((key, diffs)) = self.overrides_of(elem) else { return 0 };
        let back = diffs.into_iter().filter_map(|(a, v)| v.map(|v| (a, v))).collect::<Vec<_>>();
        if back.is_empty() {
            return 0;
        }

        let Ok(mark) = self.begin() else { return 0 };
        for (a, v) in &back {
            self.tscene_mut().set_attr(elem, a, v);
        }

        self.push_splice(format!("Revert the overrides of an item of {key}"), mark, vec![item_of(elem)], vec![item_of(elem)]);
        back.len()
    }

    /// The Inspector's rows of a prefab reference's overrides: on one of its items, the values it overrides (marked) and reverting them; on
    /// the reference, how many and reverting them all. The revert buttons are always there, greyed out when there is nothing to revert.
    fn override_rows(&mut self, e: usize, ins: &mut dyn Inspector) {
        if self.is_prefab_reference(e) {
            let key = self.prefab_key(e).unwrap_or_default();
            ins.text("prefab", "prefab", &key);
            let count = self.override_count(e);
            match count {
                Some(0) => ins.text("overrides", "overrides", "none"),
                Some(n) => {
                    ins.mark_fields(vec![("overrides".to_owned(), "Values of its items that are not the prefab's".to_owned())]);
                    ins.text("overrides", "overrides", &format!("{n} value(s) in its items"));
                }

                None => ins.text("overrides", "overrides", "its items differ by more than values (kept as they are)"),
            }

            if ins.button("revert_all", "Revert all overrides", count.is_some_and(|n| n > 0)) {
                self.revert_overrides(e);
            }

            return;
        }

        let Some((key, diffs)) = self.overrides_of(e) else { return };
        ins.text("prefab", "in the prefab", &key);
        ins.text("overrides", "overrides", &if diffs.is_empty() { "none".to_owned() } else { format!("{} (marked)", diffs.len()) });
        let revertible = diffs.iter().any(|(_, v)| v.is_some());
        if ins.button("revert", "Revert to the prefab's values", revertible) {
            self.revert_overrides(e);
        }

        let marks = diffs.into_iter().map(|(a, v)| {
            let note = match v {
                Some(v) => format!("Override: the prefab has {v}"),
                None => "Override: the prefab's item has no such value".to_owned(),
            };

            (a, note)
        }).collect::<Vec<_>>();
        if !marks.is_empty() {
            ins.mark_fields(marks);
        }
    }

    /// Makes instance groups references to prefabs (each with its key; the copies `prefab_refs::recognise` found), one undo entry. Returns
    /// how many.
    pub fn make_references(&mut self, groups: &[(usize, String)], what: &str) -> usize {
        let n = self.tile().tscene().doc().elements().len();
        let groups = groups.iter().filter(|(g, _)| *g < n).collect::<Vec<_>>();
        if groups.is_empty() {
            return 0;
        }

        let Ok(mark) = self.begin() else { return 0 };
        for (g, key) in &groups {
            self.tscene_mut().set_attr(*g, wce_tile::project_files::PREFAB_ATTR, key);
        }

        self.push_splice(what.to_owned(), mark, vec![], vec![]);
        groups.len()
    }

    /// Makes an instance group a reference to the prefab `key` (after "Save as prefab").
    pub fn make_prefab_reference(&mut self, group: usize, key: &str) -> bool {
        self.rename(group, key) && self.set_attribute(group, wce_tile::project_files::PREFAB_ATTR, key)
    }

    /// The text of an element (at column 0) and of a container's contents, as a prefab file holds them.
    pub fn element_text(&self, elem: usize) -> Vec<u8> {
        self.tile().tscene().element_text(elem)
    }

    pub fn children_text(&self, elem: usize) -> Vec<u8> {
        self.tile().tscene().children_text(elem)
    }

    /// Places a copy of a prefab (its group's text) in the layer `into`, named after the key, standing on the ground at `at` (world metres).
    /// One undo entry; the selection afterwards is the copy.
    ///
    /// # Errors
    ///
    /// No such layer, or the text is not one element.
    pub fn place_prefab(&mut self, key: &str, group_text: &[u8], into: usize, at: Float3) -> Result<usize, String> {
        let text = prefab_reference_text(group_text, key);
        self.place_text(&format!("Place {key}"), &text, into, at, Some(key), true)
    }

    /// Gives every group of `groups` these contents (text at column 0; their positions are relative to the group, so each copy keeps its
    /// place). One undo entry named `what`; the selection afterwards is empty. Returns how many groups changed.
    ///
    /// # Errors
    ///
    /// A group without a children element.
    pub fn set_groups_children(&mut self, groups: &[usize], children: &[u8], what: &str) -> Result<usize, String> {
        let mut groups = groups.to_vec();
        groups.sort_unstable();
        groups.dedup();

        // From the last one: an edit moves only the indices after it.
        let changed = groups.iter().filter(|&&g| self.tile().tscene().children_text(g) != children).copied().collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(0);
        }

        let mark = self.begin()?;
        for &g in changed.iter().rev() {
            if let Err(e) = self.tscene_mut().set_children_text(g, children) {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        }

        self.push_splice(what.to_owned(), mark, vec![], vec![]);
        Ok(changed.len())
    }

    /// Gives each instance group its own contents (`(group, contents)`), one undo entry. Returns how many changed.
    ///
    /// # Errors
    ///
    /// A group's contents do not read (nothing changes).
    pub fn set_each_group_children(&mut self, groups: &[(usize, Vec<u8>)], what: &str) -> Result<usize, String> {
        let mut groups = groups.iter().filter(|(g, c)| self.tile().tscene().children_text(*g) != *c).collect::<Vec<_>>();
        groups.sort_unstable_by_key(|(g, _)| *g);
        groups.dedup_by_key(|(g, _)| *g);
        if groups.is_empty() {
            return Ok(0);
        }

        // From the last one: an edit moves only the indices after it.
        let mark = self.begin()?;
        for (g, c) in groups.iter().rev() {
            if let Err(e) = self.tscene_mut().set_children_text(*g, c) {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        }

        self.push_splice(what.to_owned(), mark, vec![], vec![]);
        Ok(groups.len())
    }

    /// One undo entry; the selection afterwards is empty.
    pub fn remove_items(&mut self, elems: &[usize]) -> bool {
        if let Err(err) = self.check_structure(elems, None) {
            wce_foundation::log::warn(err);
            return false;
        }

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
        self.check_structure(&[layer], None)?;
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

            let file_y = (world_transform(t.tscene(), e).position.y / t.unit()) as f32;
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
            if (y - (world_transform(t.tscene(), e).position.y / t.unit()) as f32).abs() < 1e-5 {
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

    /// Saves tile.wce and the height map (in place, or into the folder of `target`); the message says what was written. A tile opened
    /// from a kit is saved into its project (kit.rs), which the document is from then on.
    pub(crate) fn save_tile(&mut self, target: &SaveTarget) -> Result<String, String> {
        self.end_stroke();
        let mut elsewhere = !target.path.as_os_str().is_empty();
        let mut into_project = None;
        if !elsewhere && !self.is_project() {
            into_project = Some(self.make_project()?);
            elsewhere = true;
        }

        let t = self.tile();
        let mut out = into_project.clone().unwrap_or_else(|| if elsewhere { target.path.clone() } else { t.project.dir.clone() });

        // "Save as..." hands over a file name: the tile goes into its folder.
        if into_project.is_none() && elsewhere && out.extension().is_some() {
            out = out.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        }

        let height_dirty = t.height_modified() || t.png_on_disk_edited || (elsewhere && t.project.has_height());
        let force_scene = t.save_scene_needed() || elsewhere;
        let t = self.tile_mut();
        t.project.height_dirty = height_dirty;
        let (wrote, msg) = t.project.save(&out, force_scene, false)?;
        if let Some(dir) = into_project {
            self.moved_to_project(dir);
            elsewhere = false;
        }

        let t = self.tile_mut();
        if !elsewhere {
            t.ted_on_disk_edited = t.tscene().modified();
            t.png_on_disk_edited = t.height_modified();
            t.saved_height_depth = t.project.height.as_ref().map_or(0, |h| h.edit_depth());
            t.project.height_dirty = false;
            t.project.river_texts = t.project.current_river_texts();
            self.history.mark_saved();
        }

        let msg = if wrote { format!("{msg}({})", out.display()) } else { msg };

        // What the game will not have, said at every save.
        Ok(match self.exclusion_warning() {
            Some(w) => {
                wce_foundation::log::warn(format!("{}: {w}", self.title()));
                format!("WARNING: {w}. {msg}")
            }

            None => msg,
        })
    }

    // ---- inspector and gizmo

    pub(crate) fn inspect_item(&mut self, item: SourceRef, ins: &mut dyn Inspector) {
        if let Some((e, k)) = piece_of(item.item) {
            self.inspect_piece(e, k, ins);
            return;
        }

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
        self.override_rows(e, ins);
        let t = self.tile();
        let sc = t.tscene();
        let before = sc.name(e);
        if sc.doc().get(e, "name").is_some() {
            // TEd renames in the Scene tab; the name is not one of the properties.
            let drafts_key = (e, "name".to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| before.clone());
            if ins.edit_string("name", "name", &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended() && let Some(v) = self.tile_mut().drafts.remove(&drafts_key) && v != before {
                self.rename(e, &v);
            }
        } else {
            ins.text("name", "name", &before);
        }

        let t = self.tile();
        let sc = t.tscene();
        if let Some(k) = sc.doc().get(e, "key").filter(|_| sc.type_of(e) == "BUILDING_INSTANCE") {
            let model = match t.building_model(&k) {
                Some(m) if m.ok => format!("{} pieces, {} destruction levels", m.pieces.len(), m.levels.max(1)),
                Some(_) => "no model: box".to_owned(),
                None => String::new(),
            };

            if let Some(c) = t.building_category(&k) {
                ins.text("category", "category", &c);
            }

            ins.text("model", "model", &model);
        }

        ins.text("layer", "layer", &sc.layer_path(e));

        let tag = sc.type_of(e).to_owned();
        let Some(schema) = wce_tile::properties::schema(&tag) else {
            self.inspect_unknown(e, ins);
            return;
        };

        // The rows of TEd's Property Editor, in its order.
        ins.section("Properties");
        // The reference's key is the prefab row above.
        let mut shown = vec!["name", wce_tile::project_files::PREFAB_ATTR];
        for prop in schema {
            shown.push(prop.attr);
            if let Some(twin) = prop.twin {
                shown.push(twin);
            }

            self.property_row(e, prop, ins);
        }

        // What the file has and the grid does not show.
        let sc = self.tile().tscene();
        let rest = sc.doc().elements()[e].attrs.iter().enumerate().filter(|(_, a)| !shown.contains(&a.name.as_str())).collect::<Vec<_>>();
        if !rest.is_empty() {
            ins.section("Other attributes");
            for (i, a) in rest {
                ins.text(&format!("attr_{}", a.name), &a.name, &String::from_utf8_lossy(sc.doc().attr_raw(e, i)));
            }
        }

        // Polygons: the point count (their points are edited in the viewport).
        if self.tile().is_polygon(e) {
            let sc = self.tile().tscene();
            ins.text("points", "points", &sc.polygon_points(e).len().to_string());
        }
    }

    /// The Property Editor of a building piece (TEd: `BUILDING_PIECE_INSTANCE`). TEd does not write the pieces to the tile, so these values live
    /// for the session; the position, orientation, scale, visibility and destruction level are shown in the viewport.
    fn inspect_piece(&mut self, e: usize, k: usize, ins: &mut dyn Inspector) {
        let t = self.tile();
        let Some(model) = t.model_of(e).filter(|m| k < m.pieces.len()) else { return };
        let levels = i64::from(model.levels.max(1));
        let file = model.pieces[k].rsplit('/').next().unwrap_or_default().to_owned();
        let owner = t.tscene().name(e);
        ins.section("building piece");
        ins.text("owner", "building", &owner);
        ins.text("mesh", "mesh", &file);

        let mut p = self.tile().piece_props.get(&(e, k)).cloned().unwrap_or_default();
        let before = p.clone();

        let mut flags = [p.visible, p.active, p.draggable];
        if ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut flags) {
            (p.visible, p.active, p.draggable) = (flags[0], flags[1], flags[2]);
        }

        let modes = ["Landscape: Above", "Freeform"].map(str::to_owned);
        let mut idx = i32::from(p.freeform);
        if ins.edit_choice("surface_mode", "surface_mode", &mut idx, &modes) {
            p.freeform = idx == 1;
        }

        ins.edit_float3("position", "position", &mut p.position, 0.05);
        let mut deg = p.euler.map(f32::to_degrees);
        if ins.edit_float3("orientation", "orientation", &mut deg, 0.5) {
            p.euler = deg.map(f32::to_radians);
        }

        ins.edit_float3("scale", "scale", &mut p.scale, 0.01);
        ins.edit_slider("opacity", "opacity", &mut p.opacity, 0.0, 1.0);
        ins.edit_bool("burn", "burn", &mut p.burn);
        ins.edit_slider("burn_amount", "burn_amount", &mut p.burn_amount, 0.0, 1.0);
        ins.edit_slider("glow_amount", "glow_amount", &mut p.glow_amount, 0.0, 1.0);
        ins.edit_slider("dirt_level", "dirt_level", &mut p.dirt_level, 0.0, 1.0);
        ins.edit_color4("bounds_colour", "bounds_colour", &mut p.bounds_colour);
        ins.edit_bool("show_collision", "show_collision", &mut p.show_collision);
        ins.edit_checks("seasons", "seasons", &["Winter", "Spring", "Summer", "Autumn"], &mut p.seasons);
        let mut level = p.destruction_level.clamp(0, levels - 1);
        if ins.edit_int_slider("destruction_level", "destruction_level", &mut level, 0, levels - 1) {
            p.destruction_level = level;
        }

        if p != before {
            let t = self.tile_mut();
            t.piece_props.insert((e, k), p);
            t.piece_pending.push((e, k));
        }
    }

    /// An element the property schema does not know: its attributes, read-only.
    fn inspect_unknown(&mut self, e: usize, ins: &mut dyn Inspector) {
        let base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x31;
        if self.tile().is_instance(e) {
            self.position_rows(e, ins, base);
        }

        ins.section("Attributes");
        let sc = self.tile().tscene();
        for (i, a) in sc.doc().elements()[e].attrs.iter().enumerate() {
            ins.text(&format!("attr_{}", a.name), &a.name, &String::from_utf8_lossy(sc.doc().attr_raw(e, i)));
        }
    }

    /// TEd's X / Y / Z fields: the world position in metres (an extra row; the grid has the scene position) and the position as the file has it.
    fn position_rows(&mut self, e: usize, ins: &mut dyn Inspector, base: u64) {
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

        if let Some(p) = self.tile().tscene().position(e) {
            let mut v = p.to_array().map(|c| c as f32);
            if ins.edit_float3("position", "position", &mut v, 0.5) {
                self.set_local(e, Some(v.map(f64::from).into()), None, base);
            }

            if ins.gesture_ended() {
                self.history.break_merge();
            }
        }
    }

    /// One row of the Property Editor.
    fn property_row(&mut self, e: usize, prop: &Prop, ins: &mut dyn Inspector) {
        let base = (u64::from(self.id()) << 40) ^ ((e as u64) << 8) ^ 0x31;
        let t = self.tile();
        let sc = t.tscene();
        let tag = sc.type_of(e).to_owned();
        let text = sc.doc().get(e, prop.attr);

        // The rules that need to know more than the property: a building's category, a polygon's shape.
        let key = sc.doc().get(e, "key").unwrap_or_default();
        let category = if tag == "BUILDING_INSTANCE" { t.building_category(&key) } else { None };
        let shape = sc.doc().get(e, "polygon_shape");
        let ctx = RuleContext { category: category.as_deref(), shape: shape.as_deref(), present: text.is_some() };
        let access = wce_tile::properties::access(prop, &ctx);
        if access == Access::Hidden {
            return;
        }

        // A property the instance lacks is not in the file (TEd writes only some of them when they have their default), but every instance
        // has all of them in the grid.
        let cur = text.clone().unwrap_or_default();
        let editable = access == Access::Edit;
        let label = prop.attr;
        let field = prop.attr;

        // The ones that have their own handling.
        match prop.kind {
            Kind::Flags => {
                let (mut vis, mut act, mut drag) = (self.tile().own_flag(e, "OIF_VISIBLE"), self.tile().own_flag(e, "OIF_ACTIVE"), self.tile().own_flag(e, "OIF_DRAGGABLE"));
                if self.tile().tscene().doc().get(e, "flags").is_none() {
                    return;
                }

                let (shown, selectable) = (self.tile().shown(e), self.tile().selectable(e));
                let mut on = [vis, act, drag];
                if editable && ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut on) {
                    if on[0] != vis {
                        vis = on[0];
                        self.set_flag(e, "OIF_VISIBLE", vis);
                    }

                    if on[1] != act {
                        act = on[1];
                        self.set_flag(e, "OIF_ACTIVE", act);
                    }

                    if on[2] != drag {
                        drag = on[2];
                        self.set_flag(e, "OIF_DRAGGABLE", drag);
                    }
                }

                let _ = (vis, act, drag);
                if !shown {
                    ins.text("hidden", "", "hidden by a layer above");
                } else if !selectable {
                    ins.text("locked", "", "locked by a layer above");
                }

                return;
            }

            Kind::Placement => {
                if self.tile().is_instance(e) && self.tile().tscene().doc().get(e, "surface_mode").is_some() {
                    // TEd's two modes and the editor's Entity: Above (freeform, marked: `project_files::SURFACE_ATTR`).
                    use wce_tile::project_files::{ROTATION_ATTR, SURFACE_ATTR};
                    let doc = self.tile().tscene().doc();
                    let on_entity = doc.get(e, SURFACE_ATTR).is_some_and(|v| v == "entity");
                    let modes = ["Landscape: Above", "Freeform", "Entity: Above"].map(str::to_owned);
                    let mut idx = if on_entity { 2 } else { i32::from(self.tile().placement(e) == Placement::Freeform) };
                    let before = idx;
                    if ins.edit_choice("surface_mode", "surface_mode", &mut idx, &modes) && editable && idx != before {
                        self.set_placement(&[e], if idx == 0 { Placement::Landscape } else { Placement::Freeform });
                        if idx == 2 {
                            self.set_attribute(e, SURFACE_ATTR, "entity");
                        } else if on_entity {
                            self.set_attribute(e, SURFACE_ATTR, "");
                        }
                    }

                    if idx == 2 {
                        ins.text("surface_note", "", "kept at its height; G puts it on what is under the mouse");
                    }

                    // The rotation: free (as given), or following the surface under it (tilted to it now; a grab tilts it again).
                    let follow = self.tile().tscene().doc().get(e, ROTATION_ATTR).is_some_and(|v| v == "surface");
                    let rotations = ["Free", "Follow surface"].map(str::to_owned);
                    let mut r = i32::from(follow);
                    if ins.edit_choice("rotation_mode", "rotation", &mut r, &rotations) && editable && (r == 1) != follow {
                        if r == 1 {
                            let p = self.tile().world_position(e);
                            let n = self.tile().terrain_normal(p.x, p.z);
                            self.tilt_to_surface(e, n, base + 5);
                            self.history.break_merge();
                            self.set_attribute(e, ROTATION_ATTR, "surface");
                        } else {
                            self.set_attribute(e, ROTATION_ATTR, "");
                        }
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

                return;
            }

            Kind::Position => {
                self.position_rows(e, ins, base);
                return;
            }

            Kind::Orientation => {
                if let Some(eu) = self.tile().tscene().euler(e) {
                    if !editable {
                        let d = eu.to_array().map(degrees);
                        ins.text("euler", "orientation", &format!("{:.1}°  {:.1}°  {:.1}°", d[0], d[1], d[2]));
                        return;
                    }

                    let mut v = eu.to_array().map(degrees);
                    if ins.edit_float3("euler", "orientation", &mut v, 0.5) {
                        self.set_local(e, None, Some(v.map(radians).into()), base + 1);
                    }

                    if ins.gesture_ended() {
                        self.history.break_merge();
                    }
                }

                return;
            }

            _ => {}
        }

        if !editable {
            ins.text(field, label, &cur);
            return;
        }

        let gesture = base ^ (attr_hash(prop.attr) << 4) ^ 0x7;
        let mut new = edit_value(prop, &cur, text.is_some(), i64::from(self.tile().destruction_levels(&key)), ins);
        if prop.kind == Kind::Text {
            // Typed text is applied when the typing ends (a key edit loads a model).
            let drafts_key = (e, prop.attr.to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| cur.clone());
            if ins.edit_string(field, label, &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended() && let Some(v) = self.tile_mut().drafts.remove(&drafts_key) {
                new = Some(v);
            }
        }

        if let Some(v) = new {
            self.set_property(e, prop, &v, gesture);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// Sets a property's attribute (and its twin): one undo entry, the steps of a drag merge. An attribute the element lacks is added by a text
    /// splice (that entry does not merge).
    pub fn set_property(&mut self, elem: usize, prop: &Prop, value: &str, gesture: u64) -> bool {
        let t = self.tile();
        if elem >= t.tscene().doc().elements().len() {
            return false;
        }

        let sc = t.tscene();
        let tag = sc.type_of(elem).to_owned();
        let mut steps = vec![];
        let mut missing = vec![];
        for attr in std::iter::once(prop.attr).chain(prop.twin) {
            match sc.doc().get(elem, attr) {
                Some(old) if old == value => {}
                Some(old) => steps.push((attr.to_owned(), old, value.to_owned())),
                None => missing.push(attr),
            }
        }

        // An empty optional text that is not in the file stays out of it (TEd writes `capture_location` only when it has one).
        missing.retain(|_| !(value.is_empty() && matches!(prop.kind, Kind::Text)));
        for attr in missing {
            self.set_attribute(elem, attr, value);
        }

        if steps.is_empty() {
            return true;
        }

        let n = self.tile().tscene().name(elem);
        let name = format!("Set {} of {}", prop.attr, if n.is_empty() { tag.clone() } else { n });
        let rebuild = tag == "BUILDING_INSTANCE" && prop.attr == "key";
        self.history.push(Box::new(TedAttrCommand { name, elem, steps, gesture, rebuild }));
        true
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
            let mut wp = Double3::new(f64::from(now.m[0][3]) * u, f64::from(now.m[1][3]) * u, f64::from(now.m[2][3]) * u);

            // As in TEd: x / z move a landscape instance over the terrain, y is the terrain's (a y handle does nothing). The float32 noise of a
            // y-axis drag at 1000 m is 6e-5.
            if t.placement(e) == Placement::Landscape {
                let same_xz = (now.m[0][3] - before.m[0][3]).abs() < 2e-4 && (now.m[2][3] - before.m[2][3]).abs() < 2e-4;
                wp.y = u * if same_xz { f64::from(before.m[1][3]) } else { f64::from(t.landscape_y(e, now.m[0][3], now.m[2][3])) };
            }

            let lp = pw.rotation.transpose() * (wp - pw.position);
            let (wr, br) = (rotation_of(&now), rotation_of(&before));
            let dr = wr.data().iter().zip(br.data()).fold(0.0f64, |m, (a, b)| m.max((a - b).abs()));
            let le = (dr > 1e-5).then(|| euler_from_matrix(&(pw.rotation.transpose() * wr)));

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

/// A polygon of TEd's shape: its area, open or closed, its points.
fn new_polygon(kind: &str, points: Vec<Double3>, closed: bool) -> wce_tile::new_instances::NewInstance {
    use wce_tile::new_instances::{NewInstance, NewPoint, polygon_area};

    let mut n = NewInstance::new(kind).attr("area", wce_tile::ted_scene::format_number(polygon_area(&points)));
    if !closed {
        n = n.attr("polygon_closed", "0");
    }

    n.points = points.into_iter().map(NewPoint::at).collect();
    n
}

/// A type placed by a click (`TileDocument::create`), at `at` (local to where it goes), with its parts.
pub(crate) fn point_instance(kind: &str, at: Double3, key: &str, unit: f64) -> Result<wce_tile::new_instances::NewInstance, String> {
    use wce_tile::new_instances::{NewInstance, circle_points, element_text, vector};

    let square = |half_m: f64| {
        let h = half_m * unit;
        vec![Double3::new(-h, 0.0, -h), Double3::new(h, 0.0, -h), Double3::new(h, 0.0, h), Double3::new(-h, 0.0, h)]
    };

    Ok(match kind {
        "ProjectedDecalInstance" | "VEGETATION_INSTANCE" | "SIMPLE_MESH_INSTANCE" => {
            let n = NewInstance::new(kind).position(at);
            if key.is_empty() { n } else { n.attr("key", key) }
        }

        // An effect at its own most common scale in the kit (the type's default is a torch's flame), else 1.
        "VFXInstance" => {
            let n = NewInstance::new(kind).position(at);
            if key.is_empty() { n } else { n.attr("vfx", key).attr("scale", wce_tile::instance_templates::vfx_scale(key).unwrap_or("1")) }
        }

        "POINT_LIGHT_INSTANCE" => NewInstance::new(kind).position(at),
        "CivilianShelter" => NewInstance::new(kind).position(at).attr("target", vector(at + Double3::new(0.0, 0.0, 10.0 * unit))),
        "CaptureLocationInstance" => {
            let boundary = new_polygon("CaptureLocationPolygonInstance", circle_points(20.0 * unit), true).attr("radius", wce_tile::ted_scene::format_number(20.0 * unit));
            let mut n = NewInstance::new(kind).position(at);
            n.children.push(element_text(&boundary)?);
            n
        }

        "SiegeAINodeInstance" => {
            let mut n = NewInstance::new(kind).position(at);
            n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", square(10.0), true).attr("polygon_closed", "1"))?);
            n
        }

        "DeploymentZonePolygonInstanceGroup" => {
            let mut n = NewInstance::new(kind).position(at);
            n.children.push(element_text(&new_polygon("DEPLOYMENT_ZONE_POLYGON_INSTANCE", square(32.0), true))?);
            n
        }

        _ => return Err(format!("{kind} is drawn, not placed")),
    })
}

/// The row of a property with a plain value (not the flags, placement, position or orientation, nor typed text): the new value when it was
/// edited. `present`: the element has the attribute; `levels`: a building's destruction levels.
pub(crate) fn edit_value(prop: &Prop, cur: &str, present: bool, levels: i64, ins: &mut dyn Inspector) -> Option<String> {
    let (field, label) = (prop.attr, prop.attr);
    let mut new: Option<String> = None;
    match prop.kind {
        Kind::Bool => {
            let mut v = parse_bool(cur);
            if ins.edit_bool(field, label, &mut v) {
                new = Some(format_bool(v).to_owned());
            }
        }

        Kind::Slider(lo, hi) => {
            let mut v = parse_float(cur).unwrap_or(lo).clamp(lo, hi);
            if ins.edit_slider(field, label, &mut v, lo, hi) {
                new = Some(format_float(v.clamp(lo, hi)));
            }
        }

        Kind::Float => {
            let mut v = parse_float(cur).unwrap_or(0.0);
            if ins.edit_float(field, label, &mut v, 0.01, -1.0e9, 1.0e9) {
                new = Some(format_float(v));
            }
        }

        Kind::Vec3 => {
            let mut v = parse_vec3_text(cur).unwrap_or([1.0; 3]);
            if ins.edit_float3(field, label, &mut v, 0.05) {
                new = Some(format_vec3(v));
            }
        }

        Kind::Vec2 => {
            let mut v = parse_vec2(cur).unwrap_or([0.0; 2]);
            if ins.edit_float2(field, label, &mut v, 0.05) {
                new = Some(format_vec2(v));
            }
        }

        Kind::Angle(max) => {
            let mut v = parse_float(cur).unwrap_or(0.0);
            if ins.edit_angle(field, label, &mut v, max) {
                new = Some(format_float(v.clamp(0.0, max)));
            }
        }

        Kind::Colour => {
            let mut v = parse_colour(cur).unwrap_or([1.0; 4]);
            if ins.edit_color4(field, label, &mut v) {
                new = Some(format_colour(v));
            }
        }

        Kind::Enum(table) => {
            let mut choices = table.iter().map(|(_, d)| (*d).to_owned()).collect::<Vec<_>>();
            let mut idx = table.iter().position(|(f, _)| *f == cur).map_or(-1, |i| i as i32);
            if idx < 0 && !cur.is_empty() {
                // A value TEd's table does not have: shown as it is, until another is chosen.
                choices.push(cur.to_owned());
                idx = choices.len() as i32 - 1;
            }

            if ins.edit_choice(field, label, &mut idx, &choices) && let Some((f, _)) = usize::try_from(idx).ok().and_then(|i| table.get(i)) {
                new = Some((*f).to_owned());
            }
        }

        // The caller applies typed text when the typing ends (`text_row`).
        Kind::Text => {}

        Kind::Int(lo, hi) => {
            let mut v = parse_int(cur).unwrap_or(i64::from(lo)).clamp(i64::from(lo), i64::from(hi));
            if ins.edit_int_slider(field, label, &mut v, i64::from(lo), i64::from(hi)) {
                new = Some(v.clamp(i64::from(lo), i64::from(hi)).to_string());
            }
        }

        Kind::DestructionLevel => {
            let mut v = parse_int(cur).unwrap_or(0).clamp(0, levels - 1);
            if ins.edit_int_slider(field, label, &mut v, 0, levels - 1) {
                new = Some(v.clamp(0, levels - 1).to_string());
            }
        }

        Kind::ReadOnlyFloat => ins.text(field, label, cur),
        Kind::Flags | Kind::Seasons | Kind::Placement | Kind::Position | Kind::Orientation => {}
    }

    if prop.kind == Kind::Seasons {
        let mut on = if present { parse_seasons(cur) } else { [true; 4] };
        if ins.edit_checks(field, label, &["Winter", "Spring", "Summer", "Autumn"], &mut on) {
            new = Some(format_seasons(on));
        }
    }

    new
}

/// A prefab's group text as a placed reference: `wce_prefab="<key>"` on its start tag (replacing one it has).
pub fn prefab_reference_text(group_text: &[u8], key: &str) -> Vec<u8> {
    super::draft::with_root_attr(group_text, wce_tile::project_files::PREFAB_ATTR, key)
}
