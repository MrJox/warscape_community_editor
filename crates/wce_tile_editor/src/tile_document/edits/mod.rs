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
use wce_tile::ground_types::{GROUND_COLOURS, NO_GROUND_TYPE};
use wce_tile::height_field::{Brush, HeightField};
use wce_tile::properties::{
    Access, Kind, Prop, RuleContext, format_bool, format_colour, format_float, format_seasons, format_vec2, format_vec3, parse_bool,
    parse_colour, parse_float, parse_int, parse_seasons, parse_vec2, parse_vec3_text,
};
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::{
    WorldTransform, euler_from_matrix, euler_matrix, rotate_world_yaw, set_world_position, set_world_rotation, world_transform,
};

use super::commands::{
    BrushStepCommand, GroundFillCommand, HeightFillCommand, PlacementItem, SpliceEffect, TedAttrCommand, TedAttrsCommand, TedFlagCommand,
    TedMoveCommand, TedPlacementCommand, TedPointAttrsCommand, TedPointCommand, TedSpliceCommand, TextureGroupCommand,
};
use super::shift::merged_spans;
use super::{
    AlignTo, BrushKind, BrushSettings, CopiedItem, Create, Follow, OffTerrain, Placement, PlacementCounts, TERRAIN, Tile, TileDocument,
    elem_of, item_of, piece_of,
};

mod inspector;
mod new_items;
mod placement;
mod prefabs;
mod save;
mod splines;
mod strokes;
mod structure;
mod variations;

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

/// A point's coordinates as an array (x, y, z), to pick one by axis.
trait ToArr {
    fn to_arr(self) -> [f32; 3];
}

impl ToArr for Float3 {
    fn to_arr(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

pub(crate) fn parent_transform(sc: &TedScene, elem: usize) -> WorldTransform {
    sc.doc().elements()[elem].parent.map(|p| world_transform(sc, p)).unwrap_or_default()
}

fn close(a: Double3, b: Double3) -> bool {
    a.max_abs_diff(b) < 2e-4
}

/// A world position in scene units (`unit` per metre horizontally).
pub(crate) fn scene_point(p: Float3, unit: f64) -> Double3 {
    Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit)
}

/// A position in scene units in metres (`scene_point`'s inverse).
fn scene_metres(p: Double3, unit: f64) -> Float3 {
    Float3::new((p.x / unit) as f32, (p.y / unit) as f32, (p.z / unit) as f32)
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
    elems
        .iter()
        .copied()
        .filter(|&e| {
            let mut p = els[e].parent;
            while let Some(q) = p {
                if elems.binary_search(&q).is_ok() {
                    return false;
                }

                p = els[q].parent;
            }

            true
        })
        .collect()
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

    /// Whether an element stands on the terrain, so that its height against it means something. Polygons and lines have the landscape mode
    /// too, but what they are is their points, flat on the ground in the game: the height of their pivot says nothing (39 m on a shipped
    /// tile). The same goes for what only holds them: a siege AI edge is its two boundaries, at the tile's origin.
    pub(crate) fn on_the_terrain(&self, e: usize) -> bool {
        if self.placement(e) != Placement::Landscape || self.is_polygon(e) {
            return false;
        }

        let els = self.tscene().doc().elements();
        let inside = els[e].children.iter().filter(|&&c| els[c].name == "children").flat_map(|&c| els[c].children.iter());
        !inside.into_iter().any(|&c| self.is_polygon(c))
    }

    /// Landscape instances further from the terrain than tolerance, worst first.
    pub fn off_terrain(&self, tolerance: f32, among: Option<&[usize]>) -> Vec<OffTerrain> {
        let list = among.unwrap_or(&self.instances);
        let mut out = list
            .iter()
            .copied()
            .filter(|&e| self.on_the_terrain(e))
            .filter_map(|e| {
                // The file's height: a landscape item is shown on the terrain whatever its file says.
                let p = self.world_position(e);
                let dy = self.file_world_y(e) - self.landscape_y(e, p.x, p.z);
                (dy.abs() > tolerance).then_some(OffTerrain { elem: e, dy })
            })
            .collect::<Vec<_>>();

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
        let t = self.tscene().get(layer, "export_type").unwrap_or_else(|| "LayerExportType_Normal".to_owned());
        t.strip_prefix("LayerExportType_").map(str::to_owned).unwrap_or(t)
    }

    // ---- height strokes

    fn apply_brush(&mut self, kind: BrushKind, x: f32, z: f32, s: &BrushSettings, dt: f32) {
        if let BrushKind::Paint(channel) = kind {
            self.apply_paint(usize::from(channel), x, z, s, dt);
            return;
        }

        if let BrushKind::Ground(ground) = kind {
            self.apply_ground(ground, x, z, s);
            return;
        }

        let p = &mut self.project;
        let (cx, cy, mpp, unit) = (p.pixel_x(f64::from(x)), p.pixel_z(f64::from(z)), p.mpp_x(), p.unit_scale);
        let Some(hf) = &mut p.height else { return };
        if !self.stroke_open {
            let _ = hf.begin_edit();
            self.stroke_open = true;
        }

        let mut b = Brush {
            cx,
            cy,
            radius: (f64::from(s.radius_m) / mpp).max(1.0),
            falloff: f64::from(s.falloff),
            curve: s.curve,
            image: s.brush_image(),
            ..Default::default()
        };
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

            BrushKind::Stamp => {
                b.strength = 1.0;
                hf.raise(&b, f64::from(s.stamp_height_m) * unit)
            }

            BrushKind::Noise => {
                b.strength = 1.0;
                hf.noise(&b, f64::from(s.strength * dt) * unit, f64::from(s.noise_scale_m) / mpp)
            }

            BrushKind::Flatten => {
                b.strength = soft;
                hf.flatten(&b, f64::from(s.flatten_height) * unit)
            }

            BrushKind::Level => {
                b.strength = soft;
                hf.set_level_under(&b)
            }

            // `apply_paint`, `apply_ground`.
            BrushKind::Paint(_) | BrushKind::Ground(_) => Ok(()),
        };

        p.height_dirty = true;
    }

    /// Reads the blend map, if not yet.
    ///
    /// # Errors
    ///
    /// The tile has none, or it does not read.
    pub fn load_blend(&mut self) -> Result<(), String> {
        self.project.load_blend().map(|_| ())
    }

    /// The blend map's pixel at a point of the tile (metres), once the map is read (`None` before, or outside it).
    pub fn blend_pixel_at(&self, x: f32, z: f32) -> Option<[u16; 16]> {
        let p = &self.project;
        let blend = p.blend.as_ref()?;
        let (b, ex, ez) = (p.border_m(), p.extent_x(), p.extent_z());
        let px = ((f64::from(x) + b) * f64::from(blend.width()) / (ex + 2.0 * b)).floor();
        let pz = ((f64::from(z) + b) * f64::from(blend.height()) / (ez + 2.0 * b)).floor();
        if px < 0.0 || pz < 0.0 || px >= f64::from(blend.width()) || pz >= f64::from(blend.height()) {
            return None;
        }

        blend.pixel(px as u32, pz as u32).try_into().ok()
    }

    /// A step of a paint stroke on the blend map, which covers the bordered tile.
    fn apply_paint(&mut self, channel: usize, x: f32, z: f32, s: &BrushSettings, dt: f32) {
        // At strength 1 a second of painting lays 98 % (1 - e^-4).
        let amount = (4.0 * f64::from(s.opacity) * f64::from(dt.min(0.05))).min(1.0);
        self.paint_blend(channel, Some((x, z)), s, amount);
    }

    /// A dab of paint on the blend map with `amount` at its middle: at (x, z) with the brush's radius, falloff and image, or (`None`) over
    /// the whole map, border included, the same at every pixel (the filters and the mode still apply).
    pub(crate) fn paint_blend(&mut self, channel: usize, at: Option<(f32, f32)>, s: &BrushSettings, amount: f64) {
        let p = &mut self.project;
        let (b, ex, ez, mpp_x, mpp_z, unit) = (p.border_m(), p.extent_x(), p.extent_z(), p.mpp_x(), p.mpp_z(), p.unit_scale);
        if p.load_blend().is_err() {
            return;
        }

        let (Some(blend), height) = (p.blend.as_mut(), p.height.as_ref()) else { return };
        if !self.stroke_open {
            let _ = blend.begin_edit();
            self.stroke_open = true;
        }

        let (ppx, ppz) = (f64::from(blend.width()) / (ex + 2.0 * b), f64::from(blend.height()) / (ez + 2.0 * b));
        let brush = match at {
            Some((x, z)) => Brush {
                cx: (f64::from(x) + b) * ppx - 0.5,
                cy: (f64::from(z) + b) * ppz - 0.5,
                radius: (f64::from(s.radius_m) * ppx).max(1.0),
                strength: amount,
                falloff: f64::from(s.falloff),
                curve: s.curve,
                image: s.brush_image(),
            },

            None => {
                let (w, h) = (f64::from(blend.width()), f64::from(blend.height()));
                Brush { cx: w * 0.5, cy: h * 0.5, radius: w.hypot(h), strength: amount, falloff: 0.0, curve: None, image: None }
            }
        };

        // The terrain's height and slope (degrees) at a pixel's centre, in the editor's metres: TEd's Blendmap Filters and Rock-painting.
        let terrain = |px: u32, py: u32| -> Option<(f64, f64)> {
            let hf = height?;
            let (hx, hz) = (((f64::from(px) + 0.5) / ppx) / mpp_x, ((f64::from(py) + 0.5) / ppz) / mpp_z);
            let at = |dx: f64, dz: f64| hf.sample(hx + dx, hz + dz) / unit;
            let (gx, gz) = ((at(1.0, 0.0) - at(-1.0, 0.0)) / (2.0 * mpp_x), (at(0.0, 1.0) - at(0.0, -1.0)) / (2.0 * mpp_z));
            Some((at(0.0, 0.0), gx.hypot(gz).atan().to_degrees()))
        };

        let filter = s.filter.filter(|f| f.is_active());
        let mask = |px: u32, py: u32| -> bool {
            let Some(f) = filter else { return true };
            terrain(px, py).is_none_or(|(h, slope)| f.passes(h, slope))
        };

        let slope = |px: u32, py: u32| terrain(px, py).map_or(0.0, |t| t.1);
        let _ = blend.paint_mode(&brush, channel, s.paint_mode, &mask, &slope);
    }

    /// A step of a ground type stroke on ground_type_map.png, which covers the bordered tile: hard values under the brush.
    fn apply_ground(&mut self, ground: u8, x: f32, z: f32, s: &BrushSettings) {
        let p = &mut self.project;
        let (b, ex, ez) = (p.border_m(), p.extent_x(), p.extent_z());
        let Ok(map) = p.load_ground() else { return };
        if !self.stroke_open {
            let _ = map.begin_edit();
            self.stroke_open = true;
        }

        let rgb = GROUND_COLOURS.get(usize::from(ground)).copied().unwrap_or(NO_GROUND_TYPE);
        let (ppx, ppz) = (f64::from(map.width()) / (ex + 2.0 * b), f64::from(map.height()) / (ez + 2.0 * b));
        let brush = Brush {
            cx: (f64::from(x) + b) * ppx - 0.5,
            cy: (f64::from(z) + b) * ppz - 0.5,
            radius: (f64::from(s.radius_m) * ppx).max(0.5),
            ..Default::default()
        };
        let _ = map.paint(&brush, rgb);
    }

    /// The ground type map's colour at a point of the tile (metres), once the map is read (`None` before, or outside it).
    pub fn ground_rgb_at(&self, x: f32, z: f32) -> Option<[u8; 3]> {
        let p = &self.project;
        let map = p.ground.as_ref()?;
        let (b, ex, ez) = (p.border_m(), p.extent_x(), p.extent_z());
        let px = ((f64::from(x) + b) * f64::from(map.width()) / (ex + 2.0 * b)).floor();
        let pz = ((f64::from(z) + b) * f64::from(map.height()) / (ez + 2.0 * b)).floor();
        if px < 0.0 || pz < 0.0 || px >= f64::from(map.width()) || pz >= f64::from(map.height()) {
            return None;
        }

        Some(map.rgb(px as u32, pz as u32))
    }

    /// Reads the ground type map, if not yet.
    ///
    /// # Errors
    ///
    /// The tile has none, or it does not read.
    pub fn load_ground(&mut self) -> Result<(), String> {
        self.project.load_ground().map(|_| ())
    }

    /// The heights from a picture (`BrushImage`: grey or colour PNG, 8 or 16 bit, or DDS; its brightness 0..1) stretched over the whole height
    /// map (the bordered tile, `height_map_0.png`'s layout: an edited copy of it comes back where it was): black at `low_m`, white at
    /// `high_m` (metres as the editor shows them), replacing the heights or added to them. The landscape instances move with the ground. One
    /// undo entry; returns the samples changed.
    pub(crate) fn import_heights(
        &mut self,
        image: &wce_tile::brush_image::BrushImage,
        low_m: f32,
        high_m: f32,
        add: bool,
    ) -> Result<(usize, Vec<Follow>), String> {
        let unit = self.project.unit_scale;
        let cands = self
            .instances
            .iter()
            .copied()
            .filter(|&e| self.placement(e) == Placement::Landscape)
            .map(|e| {
                let p = self.world_position(e);
                (e, p.x, p.z, self.terrain_height(p.x, p.z))
            })
            .collect::<Vec<_>>();

        let hf = self.project.height.as_mut().ok_or("the tile has no height map")?;
        let (w, h) = (hf.width(), hf.height());
        hf.begin_edit()?;
        let mut changed = 0;
        for y in 0..h {
            for x in 0..w {
                let g = image.sample((f64::from(x) + 0.5) / f64::from(w), (f64::from(y) + 0.5) / f64::from(h));
                let m = (f64::from(low_m) + g * f64::from(high_m - low_m)) * unit;
                let target = if add { hf.metres(x, y) + m } else { m };
                if (target - hf.metres(x, y)).abs() > 1e-9 {
                    hf.set_metres(x, y, target)?;
                    changed += 1;
                }
            }
        }

        hf.end_edit();
        self.project.height_dirty = true;
        let mut moved = vec![];
        for (e, cx, cz, before) in cands {
            let dh = self.terrain_height(cx, cz) - before;
            if dh.abs() < 1e-6 {
                continue;
            }

            let Some(old) = self.tscene().position(e) else { continue };
            let after = self.local_for_world_y(e, f64::from(self.world_position(e).y) + f64::from(dh));
            let before_text = self.tscene().get(e, "position");
            self.project.scene.set_position(e, after);
            moved.push(Follow { elem: e, before: old, after, before_text });
        }

        for f in &moved {
            self.index_edited(f.elem);
        }

        Ok((changed, moved))
    }

    /// The brush step of a stroke: the landscape instances under the brush move with the terrain (y += the change of the height under each).
    pub(crate) fn apply_brush_following(&mut self, kind: BrushKind, x: f32, z: f32, s: &BrushSettings, dt: f32, moved: &mut Vec<Follow>) {
        if matches!(kind, BrushKind::Paint(_) | BrushKind::Ground(_)) {
            self.apply_brush(kind, x, z, s, dt);
            return;
        }

        let r = s.radius_m + 2.0 * self.project.mpp_x() as f32 + 1.0;
        let cands = self
            .instances
            .iter()
            .copied()
            .filter(|&e| self.placement(e) == Placement::Landscape)
            .filter_map(|e| {
                let p = self.world_position(e);
                ((p.x - x).abs() <= r && (p.z - z).abs() <= r).then(|| (e, p.x, p.z, self.terrain_height(p.x, p.z)))
            })
            .collect::<Vec<_>>();

        self.apply_brush(kind, x, z, s, dt);
        for (e, cx, cz, h) in cands {
            let dh = self.terrain_height(cx, cz) - h;
            if dh.abs() < 1e-6 {
                continue;
            }

            let Some(before) = self.tscene().position(e) else { continue };
            let after = self.local_for_world_y(e, f64::from(self.world_position(e).y) + f64::from(dh));
            let before_text = self.tscene().get(e, "position");
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

        let heights = self.project.height.as_ref().is_some_and(HeightField::is_editing);
        if let Some(h) = &mut self.project.height {
            h.end_edit();
        }

        if let Some(b) = &mut self.project.blend {
            b.end_edit();
        }

        if let Some(g) = &mut self.project.ground {
            g.end_edit();
        }

        self.stroke_open = false;

        // The walls' layouts and the polygons' covers follow the ground.
        self.polygons_dirty |= heights;
    }

    fn height_modified(&self) -> bool {
        self.project.height.as_ref().is_some_and(|h| h.top_id() != self.saved_height_top)
    }

    fn blend_modified(&self) -> bool {
        self.project.blend.as_ref().is_some_and(|b| b.top_id() != self.saved_blend_top)
    }

    fn ground_modified(&self) -> bool {
        self.project.ground.as_ref().is_some_and(|g| g.top_id() != self.saved_ground_top)
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
        self.push_splice_as(name, mark, sel_before, sel_after, SpliceEffect::Structure);
    }

    fn push_splice_as(&mut self, name: String, mark: usize, sel_before: Vec<u64>, sel_after: Vec<u64>, effect: SpliceEffect) {
        let record = self.tscene_mut().end_edit(mark).unwrap_or_default();

        // Element indices moved: the index is rebuilt by the republish that follows.
        if !matches!(effect, SpliceEffect::Touched { .. }) {
            self.tile_mut().idx.valid = false;
        }

        self.history.push(Box::new(TedSpliceCommand { name, record, sel_before, sel_after, first: true, effect }));
    }

    /// `push_splice` of an edit of attributes of `elems` alone: the elements keep their indices, only they are published again.
    pub(crate) fn push_splice_touching(&mut self, name: String, mark: usize, sel_before: Vec<u64>, sel_after: Vec<u64>, elems: Vec<usize>) {
        self.push_splice_as(name, mark, sel_before, sel_after, SpliceEffect::Touched { elems, tree: false });
    }

    /// `push_splice` of an edit of `attrs` of `elems`, by what they are: what the tile's structure hangs on, what is drawn, or neither.
    fn push_splice_attrs(
        &mut self,
        name: String,
        mark: usize,
        sel_before: Vec<u64>,
        sel_after: Vec<u64>,
        elems: Vec<usize>,
        attrs: &[&str],
    ) {
        use wce_tile::project_files::{PREFAB_ATTR, ROTATION_ATTR, SURFACE_ATTR};
        let structure = |a: &&str| matches!(*a, "flags" | "position" | "orientation" | "surface_mode" | "export_type");
        let noted = |a: &&str| *a == "name" || *a == SURFACE_ATTR || *a == ROTATION_ATTR || *a == PREFAB_ATTR;
        let effect = if attrs.iter().any(structure) {
            SpliceEffect::Structure
        } else if attrs.iter().all(noted) {
            SpliceEffect::Touched { elems, tree: true }
        } else {
            SpliceEffect::Remade(elems)
        };

        self.push_splice_as(name, mark, sel_before, sel_after, effect);
    }

    /// `push_splice` of an edit that added or took away one point of `elem` (`before`: its points then), and of its attributes.
    fn push_splice_point(&mut self, name: String, mark: usize, elem: usize, before: &[usize]) {
        let after = self.tile().tscene().polygon_points(elem);
        let differs = |a: &[usize], b: &[usize]| a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
        let (span, added) = if after.len() == before.len() + 1 {
            (after[differs(&after, before)], true)
        } else if after.len() + 1 == before.len() {
            (before[differs(before, &after)], false)
        } else {
            self.push_splice(name, mark, vec![item_of(elem)], vec![item_of(elem)]);
            return;
        };

        let t = self.tile();
        let (known, now) = (t.idx.type_.len(), t.tscene().doc().elements().len());
        let fits = t.idx.valid && if added { known + 1 == now } else { known == now + 1 };
        let effect =
            if fits { SpliceEffect::Shift { spans: vec![(span, 1)], added, touched: vec![elem] } } else { SpliceEffect::Structure };
        self.push_splice_as(name, mark, vec![item_of(elem)], vec![item_of(elem)], effect);
    }

    /// `push_splice` of an edit that added `roots` with what they hold and changed no other element's number: the scene follows the
    /// renumbering (shift.rs). When the list did not grow by just that, everything is built again.
    pub(crate) fn push_splice_added(&mut self, name: String, mark: usize, sel_before: Vec<u64>, sel_after: Vec<u64>, roots: &[usize]) {
        let spans = self.subtree_spans(roots);
        let t = self.tile();
        let fits = t.idx.valid && t.idx.type_.len() + spans.iter().map(|s| s.1).sum::<usize>() == t.tscene().doc().elements().len();
        let effect = if fits { SpliceEffect::Shift { spans, added: true, touched: vec![] } } else { SpliceEffect::Structure };
        self.push_splice_as(name, mark, sel_before, sel_after, effect);
    }

    /// `push_splice` of an edit that removed `spans` (`subtree_spans`, read before it) and nothing else: as `push_splice_added`.
    fn push_splice_removed(&mut self, name: String, mark: usize, sel_before: Vec<u64>, spans: Vec<(usize, usize)>) {
        let t = self.tile();
        let fits = t.idx.valid && t.idx.type_.len() == t.tscene().doc().elements().len() + spans.iter().map(|s| s.1).sum::<usize>();
        let effect = if fits { SpliceEffect::Shift { spans, added: false, touched: vec![] } } else { SpliceEffect::Structure };
        self.push_splice_as(name, mark, sel_before, vec![], effect);
    }

    /// The spans of the list `roots` and all they hold take.
    fn subtree_spans(&self, roots: &[usize]) -> Vec<(usize, usize)> {
        let doc = self.tile().tscene().doc();
        merged_spans(roots.iter().map(|&r| (r, doc.subtree_size(r))).collect())
    }

    pub(crate) fn begin(&mut self) -> Result<usize, String> {
        self.tscene_mut().begin_edit()
    }

    /// An edit opened at `mark` (`begin`) that failed part way: what it changed is taken back (the scene as before, no undo entry, the
    /// journal's mark closed) and the error comes back.
    pub(crate) fn rolled_back<T>(&mut self, mark: usize, error: impl ToString) -> Result<T, String> {
        let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
        let _ = self.tscene_mut().unapply(&record);
        Err(error.to_string())
    }

    /// Scene units / radians, local to the parent.
    pub fn set_local(&mut self, elem: usize, position: Option<Double3>, euler: Option<Double3>, gesture: u64) -> bool {
        // An element number from before an edit that took elements away (a script's, a stale selection's): nothing to move.
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        let t = self.tile();

        // A layer or a group moves everything below it: TEd's grid has its position (and a group's orientation) too.
        let whole = !t.is_instance(elem) && t.tscene().is_layer(elem) && t.tscene().type_of(elem) != "SCENE_GRAPH";
        if !t.is_instance(elem) && !whole {
            return false;
        }

        // So does a holder (a siege AI node or edge, a deployment zone, a capture location): its shapes are local to it, as in TEd's
        // files (every vanilla tile), and are published again with it.
        let holder = wce_tile::parents::child_kind(t.tscene().type_of(elem)).is_some();
        let below = (whole || holder).then(|| t.items_below(elem));

        let sc = t.tscene();
        let old_pos = position.and(sc.position(elem));
        let old_euler = euler.and(sc.euler(elem));
        let (new_pos, new_euler) = (position.filter(|_| old_pos.is_some()), euler.filter(|_| old_euler.is_some()));
        if new_pos.is_none() && new_euler.is_none() {
            return false;
        }

        let n = sc.name(elem);
        let name = format!("Move {}", if n.is_empty() { sc.type_of(elem).to_owned() } else { n });
        let old_pos_text = old_pos.and_then(|_| sc.get(elem, "position"));
        let old_euler_text = old_euler.and_then(|_| sc.get(elem, "orientation"));
        let cmd = TedMoveCommand {
            name,
            elem,
            old_pos,
            new_pos,
            old_euler,
            new_euler,
            old_pos_text,
            old_euler_text,
            gesture,
            below,
            grounded: vec![],
        };
        match &mut self.move_batch {
            // Moved with the others: applied with them (`transform_selected`).
            Some(batch) => batch.push(cmd),
            None => self.history.push(Box::new(cmd)),
        }

        true
    }

    /// Puts an instance at a world transform (scene units; `Tile::snap_piece`'s): its local position and orientation in one undo entry, the
    /// steps of a drag (one `gesture`) merging.
    pub fn set_world_transform(&mut self, elem: usize, w: &WorldTransform, gesture: u64) -> bool {
        // An element number from before an edit that took elements away (a script's, a stale selection's): nothing to move.
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        let pw = parent_transform(self.tile().tscene(), elem);
        let lp = pw.rotation.transpose() * (w.position - pw.position);
        let euler = euler_from_matrix(&(pw.rotation.transpose() * w.rotation));
        self.set_local(elem, Some(lp), Some(euler), gesture)
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
        let old_text = sc.get(elem, "orientation");
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
        let copy = match self.tscene_mut().duplicate(elem) {
            Ok(c) => c,
            Err(_) => return self.rolled_back(mark, "cannot duplicate this element"),
        };
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
        self.push_splice_added(name, mark, vec![item_of(elem)], vec![item_of(copy)], &[copy]);
        Ok(copy)
    }

    /// Copies elements (instances, layers, groups; one inside another copied goes with it): each one's text and where it is in the world
    /// (metres, its world rotation), for `paste` in this tile or another.
    pub fn copy_items(&self, elems: &[usize]) -> Vec<CopiedItem> {
        let t = self.tile();
        let sc = t.tscene();
        let n = sc.doc().elements().len();
        let wanted = elems
            .iter()
            .copied()
            .filter(|&e| e < n && (t.is_instance(e) || (sc.is_layer(e) && sc.type_of(e) != "SCENE_GRAPH")))
            .collect::<Vec<_>>();
        wanted
            .iter()
            .copied()
            .filter(|&e| !wanted.iter().any(|&a| a != e && sc.is_descendant(e, a)))
            .map(|e| {
                let w = world_transform(sc, e);
                let u = t.unit();
                let at = Float3::new((w.position.x / u) as f32, (w.position.y / u) as f32, (w.position.z / u) as f32);
                CopiedItem { text: sc.element_text(e), at, rotation: w.rotation, landscape: t.placement_of_attr(e) == Placement::Landscape }
            })
            .collect()
    }

    /// Pastes copied elements into `into`, each moved by `offset` (metres; a landscape one stands on the terrain where it lands), in one
    /// undo entry; the new elements.
    ///
    /// # Errors
    ///
    /// Nothing to paste, or a text is not an element.
    pub fn paste(&mut self, items: &[CopiedItem], into: usize, offset: Float3) -> Result<Vec<usize>, String> {
        if items.is_empty() {
            return Err("nothing to paste".to_owned());
        }

        self.check_structure(&[], Some(into))?;
        self.check_fits(items.iter().map(|it| text_kind(&it.text)), into)?;
        let mark = self.begin()?;
        let result = self.paste_elements(items, into, offset);
        match result {
            Ok(new) => {
                let name = if new.len() == 1 {
                    format!("Paste {}", self.tile().tscene().type_of(new[0]))
                } else {
                    format!("Paste {} items", new.len())
                };
                self.push_splice_added(name, mark, vec![], new.iter().map(|&g| item_of(g)).collect(), &new);
                Ok(new)
            }

            Err(e) => self.rolled_back(mark, e),
        }
    }

    /// The copied elements into `into`, inside an edit that is open.
    pub(crate) fn paste_elements(&mut self, items: &[CopiedItem], into: usize, offset: Float3) -> Result<Vec<usize>, String> {
        let mut out = vec![];
        for it in items {
            let g = self.tscene_mut().insert_element_text(into, &it.text)?;
            let t = self.tile();
            let u = t.unit();
            let (x, z) = (it.at.x + offset.x, it.at.z + offset.z);
            let y = if it.landscape { t.terrain_height(x, z) } else { it.at.y + offset.y };
            let before = t.tscene().position(g);
            set_world_position(self.tscene_mut(), g, scene_point(Float3::new(x, y, z), u));
            set_world_rotation(self.tscene_mut(), g, &it.rotation);
            self.carry_points(g, before);
            out.push(g);
        }

        Ok(out)
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
        let spans = self.subtree_spans(&[elem]);
        let _ = self.tscene_mut().remove(elem);
        self.push_splice_removed(format!("Delete {label}"), mark, vec![item_of(elem)], spans);
        true
    }

    /// A new building copied from a template building (or the first one of the tile; a tile without any gets TEd's defaults, as the Create
    /// panel places it). `mode` None: it has the template's placement; a landscape one is put on the terrain at `at`.
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
    pub fn add_building_in(
        &mut self,
        key: &str,
        template_elem: Option<usize>,
        at: Option<Float3>,
        mode: Placement,
        into: Option<usize>,
    ) -> Result<usize, String> {
        if key.is_empty() {
            return Err(String::new());
        }

        let sc = self.tile().tscene();
        let Some(tmpl) =
            template_elem.filter(|&e| sc.type_of(e) == "BUILDING_INSTANCE").or_else(|| sc.find_all("BUILDING_INSTANCE").first().copied())
        else {
            return self.add_building_from_defaults(key, at, mode, into);
        };

        let mark = self.begin()?;
        let before = self.tile().tscene().doc().elements().len();
        let mut copy = match self.tscene_mut().duplicate(tmpl) {
            Ok(c) => c,
            Err(e) => return self.rolled_back(mark, e),
        };

        // The copy went in after the template: a layer from the copy on moved down by as many elements as it has.
        let added = self.tile().tscene().doc().elements().len() - before;
        let into = into.map(|l| if l >= copy { l + added } else { l });
        if let Some(into) = into.filter(|&l| self.tile().layer_of(copy) != Some(l)) {
            let world = world_transform(self.tile().tscene(), copy);
            copy = match self.tscene_mut().move_element(copy, into, None) {
                Ok(c) => c,
                Err(e) => return self.rolled_back(mark, e),
            };
            self.keep_worlds(&[copy], &[world]);
        }

        let sc = self.tscene_mut();
        sc.set_attr(copy, "key", key);
        sc.set_attr(copy, "name", key);
        if mode != Placement::None && sc.get(copy, "surface_mode").is_some() {
            sc.set_attr(copy, "surface_mode", if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" });
        }

        if let Some(at) = at {
            let t = self.tile();
            let y = if t.placement_of_attr(copy) == Placement::Landscape { t.landscape_y(copy, at.x, at.z) } else { at.y };
            let p = scene_point(Float3::new(at.x, y, at.z), t.unit());
            set_world_position(self.tscene_mut(), copy, p);
        }

        self.push_splice_added(format!("Add {key}"), mark, vec![template_elem.map_or(0, item_of)], vec![item_of(copy)], &[copy]);
        Ok(copy)
    }

    /// A new building with TEd's defaults (`instance_templates`) in `into` (`None`: the scene graph), at `at` (the origin by default).
    ///
    /// # Errors
    ///
    /// No scene graph to put it in.
    fn add_building_from_defaults(&mut self, key: &str, at: Option<Float3>, mode: Placement, into: Option<usize>) -> Result<usize, String> {
        let mut d = super::draft::Draft::new("BUILDING_INSTANCE", key);
        if mode != Placement::None {
            d.attrs
                .insert("surface_mode".to_owned(), if mode == Placement::Landscape { "MSM_LANDSCAPE" } else { "MSM_FREEFORM" }.to_owned());
        }

        let text = self.draft_text(&d).ok_or("no building key")?;
        let into =
            into.or(self.tile().tscene().scene_graph()).ok_or("this tile has no SCENE_GRAPH, so there is no layer to add a building to")?;
        let on_terrain = mode != Placement::Freeform;
        self.place_text(&format!("Add {key}"), &text, into, at.unwrap_or(Float3::new(0.0, 0.0, 0.0)), None, on_terrain)
    }

    /// A polygon from world points in `layer` (`None`: the scene graph).
    /// `hint`: an AI hint polygon's hint type, a wall's (`WceWallSpline`) base wall.
    ///
    /// # Errors
    ///
    /// Fewer than 2 points, or no layer to put it in.
    pub fn add_polygon(&mut self, layer: Option<usize>, kind: &str, hint: &str, pts: &[Float3]) -> Result<usize, String> {
        if pts.len() < 2 {
            return Err("a polygon needs at least 2 points".to_owned());
        }

        if kind == wce_tile::new_instances::ANGLE_FINDER && pts.len() != 3 {
            return Err(ANGLE_POINTS.to_owned());
        }

        let t = self.tile();
        let layer = layer.or(t.tscene().scene_graph()).ok_or("this tile has no SCENE_GRAPH, so there is no layer to add a polygon to")?;
        self.check_fits([kind], layer)?;
        let t = self.tile();

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

        if kind == super::ROAD_SPLINE {
            extra.insert("road_id".to_owned(), t.next_road_id().to_string());
        }

        if kind == super::WALL_SPLINE {
            extra.insert(wce_tile::new_instances::WALL_ID.to_owned(), t.next_wall_id().to_string());
        }

        // A wall's `hint` is its base wall.
        if kind == super::WALL_SPLINE && !hint.is_empty() {
            extra.insert("wall_base".to_owned(), hint.to_owned());
        }

        let mark = self.begin()?;
        let name = match kind {
            "AI_HINT_POLYGON_INSTANCE" => "AI hint polygon",
            super::WALL_SPLINE => "Wall",
            super::ROAD_SPLINE => "Road",
            super::BUILDING_SPLINE => "Building spline",
            wce_tile::new_instances::MEASURE_LINE => "Tape measure",
            wce_tile::new_instances::ANGLE_FINDER => "Angle finder",
            _ => "Polygon",
        };
        let e = match self.tscene_mut().add_polygon(Some(layer), kind, name, &local, &extra) {
            Ok(e) => e,
            Err(err) => return self.rolled_back(mark, err),
        };
        self.push_splice_added(format!("Add {kind}"), mark, vec![], vec![item_of(e)], &[e]);
        Ok(e)
    }

    /// Tilts an instance so that its up is `normal` (world), keeping its heading (where its +z points on the ground). The steps of one
    /// gesture merge.
    pub fn tilt_to_surface(&mut self, elem: usize, normal: Double3, gesture: u64) -> bool {
        // An element number from before an edit that took elements away (a script's, a stale selection's): nothing to move.
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

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

    /// Stands an instance upright on the slope under it (the *Follow slope* rotation mode, `Tile::slope_rotation`): its heading kept, pitched
    /// along its long axis only, as the building spline lays a building that does not tilt across. The steps of one gesture merge.
    pub fn tilt_along_slope(&mut self, elem: usize, gesture: u64) -> bool {
        if elem >= self.tile().tscene().doc().elements().len() {
            return false;
        }

        let sc = self.tile().tscene();
        let Some(euler) = sc.euler(elem) else { return false };
        let world = world_transform(sc, elem).rotation;
        let parent = world * wce_tile::world_transform::euler_matrix(euler).transpose();
        let key = (sc.type_of(elem) == "BUILDING_INSTANCE").then(|| sc.get(elem, "key")).flatten();
        let p = self.tile().world_position(elem);
        let turned = self.tile().slope_rotation(world, [f64::from(p.x), f64::from(p.z)], key.as_deref());
        let local = euler_from_matrix(&(parent.transpose() * turned));
        self.set_local(elem, None, Some(local), gesture)
    }

    /// An instance that follows the ground under it (`wce_rotation`: `surface`, tilted to `normal`; `slope`, upright on the terrain's slope)
    /// tilted again as its mode says; false for one that does not follow it.
    pub fn follow_ground(&mut self, elem: usize, normal: Double3, gesture: u64) -> bool {
        match self.tile().tscene().get(elem, wce_tile::project_files::ROTATION_ATTR).as_deref() {
            Some("surface") => self.tilt_to_surface(elem, normal, gesture),
            Some("slope") => self.tilt_along_slope(elem, gesture),
            _ => false,
        }
    }

    /// move | insert | remove a point of a polygon.
    ///
    /// # Errors
    ///
    /// Not a polygon, no such point, or an unknown operation.
    pub fn polygon_point(&mut self, elem: usize, op: &str, index: usize, world: Float3, gesture: u64) -> Result<(), String> {
        let t = self.tile();
        if self.is_spline(elem) || (matches!(op, "in" | "out") && self.has_handles(elem)) {
            return self.spline_point(elem, op, index, world, gesture);
        }

        if !t.is_polygon(elem) {
            return Err("not a polygon".to_owned());
        }

        if op != "move" && t.is_angle_finder(elem) {
            return Err(ANGLE_POINTS.to_owned());
        }

        let pts = t.tscene().polygon_points(elem);
        if op == "move" {
            let &p = pts.get(index).ok_or("no such point")?;

            // A wall's end goes onto a fort building's end near it.
            let world = if t.is_wall(elem) && (index == 0 || index + 1 == pts.len()) { t.snap_wall_end(world) } else { world };

            // A procedural spline's point with handles written: they go with it.
            if self.has_handles(elem)
                && wce_tile::splines::written(t.tscene(), elem).get(index).is_some_and(|h| h.iter().any(Option::is_some))
            {
                return self.spline_point(elem, "move", index, world, gesture);
            }

            let before = t.tscene().point_position(p).unwrap_or_default();
            let after = t.to_local(elem, world);
            let (before_text, area_text) = (t.tscene().get(p, "position"), t.tscene().get(elem, "area"));
            self.history.push(Box::new(TedPointCommand { elem, index, before, after, before_text, area_text, gesture }));
            return Ok(());
        }

        let mark = self.begin()?;
        match op {
            "insert" => {
                if index >= pts.len() {
                    return self.rolled_back(mark, "no such point");
                }

                let local = self.tile().to_local(elem, world);
                let own = self.has_handles(elem);
                let sc = self.tscene_mut();
                if let Err(e) = sc.insert_polygon_point(elem, index, local) {
                    return self.rolled_back(mark, e);
                }

                // The new point is a copy of the one before it: not its handles (made from its neighbours) nor its corner mark.
                if own && let Some(&p) = sc.polygon_points(elem).get(index + 1) {
                    for attr in ["controlIn", "controlOut", "sharp"] {
                        if sc.get(p, attr).is_some_and(|v| !v.is_empty()) {
                            sc.set_attr(p, attr, "");
                        }
                    }
                }

                self.push_splice_point("Insert polygon point".to_owned(), mark, elem, &pts);
                Ok(())
            }

            "remove" => {
                if !self.tscene_mut().remove_polygon_point(elem, index)? {
                    return Err("a polygon needs at least 2 points".to_owned());
                }

                self.push_splice_point("Remove polygon point".to_owned(), mark, elem, &pts);
                Ok(())
            }

            _ => Err(format!("unknown point operation '{op}'")),
        }
    }
}

impl Tile {
    /// Every element's own flag, for the Scene panel (`TERRAIN` has none).
    pub fn item_flag(&self, item: u64, flag: &str) -> bool {
        elem_of(item).is_none_or(|e| item == TERRAIN || self.own_flag(e, flag))
    }
}

/// A polygon of TEd's shape: its area, open or closed, its points.
/// A capture location's new boundary as a preset, around its capture location's origin (scene units `r`: a radius or a square's half
/// side; `turn` degrees, 0: a semicircle's round side and a square's top towards +z), as TEd writes the shape (`set_boundary_shape`).
pub(crate) fn preset_boundary(kind: BoundaryKind, r: f64, turn: f64) -> wce_tile::new_instances::NewInstance {
    let (sn, cs) = turn.to_radians().sin_cos();
    let around = |x: f64, z: f64| Double3::new(x * cs + z * sn, 0.0, -x * sn + z * cs);
    let ring = |a: f64| around(r * a.to_radians().sin(), r * a.to_radians().cos());
    let (shape, points): (&str, Vec<Double3>) = match kind {
        BoundaryKind::Circle | BoundaryKind::Polygon => ("PS_CIRCLE", wce_tile::new_instances::circle_points(r)),
        BoundaryKind::Semicircle => ("PS_SEMICIRCLE", (0..21).map(|i| ring(-90.0 + f64::from(i) * 9.0)).collect()),
        BoundaryKind::Square => ("PS_RECTANGLE", [(-r, r), (r, r), (r, -r), (-r, -r)].into_iter().map(|(x, z)| around(x, z)).collect()),
    };

    let mut n = new_polygon("CaptureLocationPolygonInstance", points, true).attr("polygon_shape", shape).attr("polygon_closed", "1");
    if shape != "PS_RECTANGLE" {
        n = n.attr("radius", wce_tile::ted_scene::format_number(r));
    }

    n
}

/// A capture location draft's boundary preset (`Draft::attrs`, not the element's): its shape (`Circle`, `Semicircle`, `Square`), its
/// size (metres) and its turn (degrees).
pub const DRAFT_BOUNDARY: [&str; 3] = ["wce_boundary_shape", "wce_boundary_size", "wce_boundary_turn"];

/// The preset shapes of a capture location's boundary (`TileDocument::set_boundary_shape`), TEd's, and a free polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    Circle,
    Semicircle,
    Square,
    Polygon,
}

impl BoundaryKind {
    pub const ALL: [BoundaryKind; 4] = [BoundaryKind::Circle, BoundaryKind::Semicircle, BoundaryKind::Square, BoundaryKind::Polygon];

    pub fn label(self) -> &'static str {
        match self {
            BoundaryKind::Circle => "Circle",
            BoundaryKind::Semicircle => "Semicircle",
            BoundaryKind::Square => "Square",
            BoundaryKind::Polygon => "Polygon",
        }
    }

    /// Whether the shape faces a way (its turn matters).
    pub fn turns(self) -> bool {
        matches!(self, BoundaryKind::Semicircle | BoundaryKind::Square)
    }
}

/// A capture location's boundary as a preset (`TileDocument::boundary_shape`): its kind, its centre on the ground (world metres), its
/// size (metres: a radius, a square's half side) and its turn (degrees; 0: a semicircle's round side and a square's top towards +z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryShape {
    pub kind: BoundaryKind,
    pub centre: Float3,
    pub size: f32,
    pub turn: f32,
}

/// A rectangle from drawn points (a siege AI node's Bounding Box: the game reads its first 4 points as one, its centre their mean, its
/// width |p1 - p0|, its height |p2 - p1|, docs/re/siege_ai.md). From 3 or 4 points: the first side as drawn (its direction and width),
/// the depth the third point's distance from it, on its side; from 2 or more than 4: the box around them on the ground.
fn bounding_rectangle(points: Vec<Double3>) -> Vec<Double3> {
    if let [a, b, c, ..] = points[..]
        && (3..=4).contains(&points.len())
    {
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let len = (dx * dx + dz * dz).sqrt();
        let (nx, nz) = (-dz / len.max(1e-9), dx / len.max(1e-9));
        let depth = (c.x - b.x) * nx + (c.z - b.z) * nz;
        if len > 1e-6 && depth.abs() > 1e-6 {
            let off = Double3::new(nx * depth, 0.0, nz * depth);
            let (c, d) = (b + off, a + off);
            return vec![
                a,
                b,
                Double3::new(c.x, (b.y + points[2].y) / 2.0, c.z),
                Double3::new(d.x, (a.y + points[points.len() - 1].y) / 2.0, d.z),
            ];
        }
    }

    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in &points {
        lo = (lo.0.min(p.x), lo.1.min(p.z));
        hi = (hi.0.max(p.x), hi.1.max(p.z));
    }

    let y = points.iter().map(|p| p.y).sum::<f64>() / points.len().max(1) as f64;
    vec![Double3::new(lo.0, y, lo.1), Double3::new(hi.0, y, lo.1), Double3::new(hi.0, y, hi.1), Double3::new(lo.0, y, hi.1)]
}

/// The two lines `off` to each side of a line: each point moved along the mean of its segments' normals, on the ground.
fn side_lines(points: &[Double3], off: f64) -> Vec<Vec<Double3>> {
    let n = points.len();
    if n < 2 {
        return vec![];
    }

    // The normal of segment s (s in 0..n - 1).
    let normal = |s: usize| {
        let (a, b) = (points[s], points[s + 1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let len = (dx * dx + dz * dz).sqrt().max(1e-9);
        (-dz / len, dx / len)
    };

    let at = |i: usize| {
        let (a, b) = (normal(i.saturating_sub(1)), normal(i.min(n - 2)));
        let (sx, sz) = (a.0 + b.0, a.1 + b.1);
        let len = (sx * sx + sz * sz).sqrt();
        if len > 1e-6 { (sx / len, sz / len) } else { b }
    };

    [1.0, -1.0]
        .iter()
        .map(|&s| {
            (0..n)
                .map(|i| {
                    let (nx, nz) = at(i);
                    points[i] + Double3::new(nx * off * s, 0.0, nz * off * s)
                })
                .collect()
        })
        .collect()
}

/// The type of an element's text: its start tag's name.
pub fn text_kind(text: &[u8]) -> &str {
    let start = text.iter().position(|&c| c == b'<').map_or(text.len(), |i| i + 1);
    let rest = &text[start..];
    let end = rest.iter().position(|&c| c.is_ascii_whitespace() || c == b'/' || c == b'>').unwrap_or(rest.len());
    std::str::from_utf8(&rest[..end]).unwrap_or_default()
}

pub(crate) fn new_polygon(kind: &str, points: Vec<Double3>, closed: bool) -> wce_tile::new_instances::NewInstance {
    use wce_tile::new_instances::{NewInstance, NewPoint, polygon_area};

    let mut n = NewInstance::new(kind).attr("area", wce_tile::ted_scene::format_number(polygon_area(&points)));
    if !closed {
        n = n.attr("polygon_closed", "0");
    }

    n.points = points.into_iter().map(NewPoint::at).collect();
    n
}

/// Why an angle finder is not made or changed with another number of points.
const ANGLE_POINTS: &str = "an angle finder has 3 points: an arm's end, the corner, the other arm's end";

/// A new deployment zone's category: Default (TEd's template has Ambush, which a new zone is rarely meant to be).
const NEW_ZONE_CATEGORY: &str = "DAC_DEFAULT";

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

        "POINT_LIGHT_INSTANCE" | "WceRefHuman" | "WceRefUnit" | "WceRefShip" => NewInstance::new(kind).position(at),
        "CivilianShelter" => NewInstance::new(kind).position(at).attr("target", vector(at + Double3::new(0.0, 0.0, 10.0 * unit))),
        "CaptureLocationInstance" => {
            let boundary = new_polygon("CaptureLocationPolygonInstance", circle_points(20.0 * unit), true)
                .attr("radius", wce_tile::ted_scene::format_number(20.0 * unit));
            let mut n = NewInstance::new(kind).position(at);
            n.children.push(element_text(&boundary)?);
            n
        }

        "SiegeAINodeInstance" => {
            // TEd's shapes, by name: the Bounding Box (the occupiable rectangle) and the Boundary (the walkable outline) around it.
            let mut n = NewInstance::new(kind).position(at);
            for (name, half) in [("Bounding Box", 10.0), ("Boundary", 12.0)] {
                n.children.push(element_text(
                    &new_polygon("SIEGE_AI_POLYGON_INSTANCE", square(half), true).attr("polygon_closed", "1").attr("name", name),
                )?);
            }

            n
        }

        "DeploymentZonePolygonInstanceGroup" => {
            let mut n = NewInstance::new(kind).position(at).attr("deployment_category", NEW_ZONE_CATEGORY);
            n.children.push(element_text(&new_polygon("DEPLOYMENT_ZONE_POLYGON_INSTANCE", square(32.0), true))?);
            n
        }

        _ => return Err(format!("{kind} is drawn, not placed")),
    })
}

/// The row of a property with a plain value (not the flags, placement, position or orientation, nor typed text): the new value when it was
/// edited. `present`: the element has the attribute; `levels`: a building's destruction levels.
impl Tile {
    /// A property with the choices the game's files have: the procedural walls' bases whose pieces are all there.
    pub(crate) fn game_choices(&self, prop: &Prop) -> Prop {
        match (prop.attr, prop.kind, self.files.as_deref()) {
            ("wall_base", Kind::Enum(_), Some(files)) => Prop { kind: Kind::Enum(wce_walls::profile::available_bases(files)), ..*prop },
            ("wall_base", Kind::Enum(_), None) => Prop { kind: Kind::Enum(wce_walls::profile::choices()), ..*prop },
            ("road_bridge", Kind::Enum(_), _) => Prop { kind: Kind::Enum(wce_walls::bridge::choices()), ..*prop },
            ("road_type", Kind::Enum(_), _) => Prop { kind: Kind::Enum(self.road_types()), ..*prop },
            ("road_fence_type", Kind::Enum(_), _) => Prop { kind: Kind::Enum(self.road_fence_types(None)), ..*prop },
            ("road_fence_joiners", Kind::Enum(_), _) => Prop { kind: Kind::Enum(self.road_fence_joiner_choices()), ..*prop },
            _ => *prop,
        }
    }
}

/// The flags row's tooltip.
const FLAGS_TIP: &str = "visible: it is shown\nactive: it can be selected\ndraggable: the gizmo and the grab (G) move it; off, it can be selected without moving it by mistake";

/// The tooltips of the surface_mode choices (Landscape: Above, Freeform, Entity: Above).
pub(super) const SURFACE_TIPS: [&str; 3] =
    ["Stands on the terrain and follows it.", "Kept at its height.", "Kept at its height; G puts it on what is under the mouse."];

/// The Export as prop row of a building that cannot be one: off, whatever is stored, as that is what the export does.
fn greyed_export_as_prop(prop: &Prop, ins: &mut dyn Inspector) {
    ins.hint(wce_tile::export_as_prop::CAPTURE_LOCATION_TIP);
    ins.disabled_bool(prop.attr, prop.label(), false);
}

pub(crate) fn edit_value(prop: &Prop, cur: &str, present: bool, levels: i64, ins: &mut dyn Inspector) -> Option<String> {
    let (field, label) = (prop.attr, prop.label());
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

            if ins.edit_choice(field, label, &mut idx, &choices)
                && let Some((f, _)) = usize::try_from(idx).ok().and_then(|i| table.get(i))
            {
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
