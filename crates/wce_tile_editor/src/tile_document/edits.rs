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
                let p = self.world_position(e);
                let dy = p.y - self.landscape_y(e, p.x, p.z);
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
        let t = self.tscene().doc().get(layer, "export_type").unwrap_or_else(|| "LayerExportType_Normal".to_owned());
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
            let before_text = self.tscene().doc().get(e, "position");
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
        self.project.height.as_ref().is_some_and(|h| h.edit_depth() != self.saved_height_depth)
    }

    fn blend_modified(&self) -> bool {
        self.project.blend.as_ref().is_some_and(|b| b.edit_depth() != self.saved_blend_depth)
    }

    fn ground_modified(&self) -> bool {
        self.project.ground.as_ref().is_some_and(|g| g.edit_depth() != self.saved_ground_depth)
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

    /// Scene units / radians, local to the parent.
    pub fn set_local(&mut self, elem: usize, position: Option<Double3>, euler: Option<Double3>, gesture: u64) -> bool {
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
        let old_pos_text = old_pos.and_then(|_| sc.doc().get(elem, "position"));
        let old_euler_text = old_euler.and_then(|_| sc.doc().get(elem, "orientation"));
        self.history.push(Box::new(TedMoveCommand {
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
        }));
        true
    }

    /// Puts an instance at a world transform (scene units; `Tile::snap_piece`'s): its local position and orientation in one undo entry, the
    /// steps of a drag (one `gesture`) merging.
    pub fn set_world_transform(&mut self, elem: usize, w: &WorldTransform, gesture: u64) -> bool {
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

            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(e)
            }
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
        let mut copy = self.tscene_mut().duplicate(tmpl)?;

        // The copy went in after the template: a layer from the copy on moved down by as many elements as it has.
        let added = self.tile().tscene().doc().elements().len() - before;
        let into = into.map(|l| if l >= copy { l + added } else { l });
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
            wce_tile::new_instances::MEASURE_LINE => "Tape measure",
            wce_tile::new_instances::ANGLE_FINDER => "Angle finder",
            _ => "Polygon",
        };
        let e = self.tscene_mut().add_polygon(Some(layer), kind, name, &local, &extra)?;
        self.push_splice_added(format!("Add {kind}"), mark, vec![], vec![item_of(e)], &[e]);
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
                let own = self.has_handles(elem);
                let sc = self.tscene_mut();
                sc.insert_polygon_point(elem, index, local)?;

                // The new point is a copy of the one before it: not its handles (made from its neighbours) nor its corner mark.
                if own && let Some(&p) = sc.polygon_points(elem).get(index + 1) {
                    for attr in ["controlIn", "controlOut", "sharp"] {
                        if sc.doc().get(p, attr).is_some_and(|v| !v.is_empty()) {
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

    // ---- splines (wce_tile::splines): rivers and roads

    pub fn is_spline(&self, elem: usize) -> bool {
        let sc = self.tile().tscene();
        elem < sc.doc().elements().len() && wce_tile::splines::is_spline(sc.type_of(elem))
    }

    /// Whether its points have handles: TEd's splines and the editor's own (procedural roads and walls, `splines::OWN_KINDS`).
    pub fn has_handles(&self, elem: usize) -> bool {
        let sc = self.tile().tscene();
        elem < sc.doc().elements().len() && wce_tile::splines::has_handles(sc.type_of(elem))
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

        if !self.has_handles(elem) {
            return Err("not a spline".to_owned());
        }

        let t = self.tile();
        let sc = t.tscene();
        let pts = read(sc, elem);
        // The editor's own splines: their handles not written are made from the points (and go on being): a point moved takes along only
        // those written.
        let own = !wce_tile::splines::is_spline(sc.type_of(elem));
        let written = wce_tile::splines::written(sc, elem);
        let keeps = |i: usize, side: usize| !own || written.get(i).is_some_and(|h| h[side].is_some());
        let n = pts.len();
        let elems = sc.polygon_points(elem);
        if index >= n.max(1) {
            return Err("no such point".to_owned());
        }

        let local = t.to_local(elem, world);
        // An attribute not written (a procedural spline's handle made from the points) is as it is drawn now.
        let text = |i: usize, attr: &str| {
            sc.doc().get(elems[i], attr).filter(|v| !v.is_empty()).unwrap_or_else(|| match attr {
                "controlIn" => point_text(pts[i].control_in),
                "controlOut" => point_text(pts[i].control_out),
                _ => String::new(),
            })
        };
        let step = |i: usize, attr: &str, v: Double3| (i, attr.to_owned(), text(i, attr), point_text(v));
        let (first, last) = (index == 0, index + 1 == n);
        let attrs = match op {
            "move" => {
                let p = pts[index];
                let d = local - p.position;
                let mut steps = vec![step(index, "position", local)];
                if !first && keeps(index, 0) {
                    steps.push(step(index, "controlIn", p.control_in + d));
                }

                if !last && keeps(index, 1) {
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
            if steps.is_empty() {
                return Ok(());
            }

            // A handle the point has not written yet: written first as it is, in a text splice (its undo takes the attribute away
            // again, which setting it back cannot), then edited as the others.
            let missing = steps
                .iter()
                .filter(|s| self.tile().tscene().doc().get(elems[s.0], &s.1).is_none_or(|v| v.is_empty()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                let mark = self.begin()?;
                let before = self.tile().tscene().polygon_points(elem);
                let sc = self.tscene_mut();
                for (i, attr, was, _) in &missing {
                    sc.set_attr(elems[*i], attr, was);
                }

                self.push_splice_point("Write spline handles".to_owned(), mark, elem, &before);
            }

            self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem, steps, gesture }));
            return Ok(());
        }

        let set = |sc: &mut TedScene, i: usize, attr: &str, v: Double3| {
            let p = sc.polygon_points(elem)[i];
            sc.set_attr(p, attr, &point_text(v));
        };

        let mark = self.begin()?;
        let before = self.tile().tscene().polygon_points(elem);
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
                self.push_splice_point(name.to_owned(), mark, elem, &before);
                Ok(())
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }

    /// Reverses a spline's direction (a river's flow, a road's way): the points in the other order, each with its own values (`width`,
    /// `foam_amount`, ...) and its `controlIn` and `controlOut` swapped. Only the points' attribute texts are permuted (no number is
    /// computed again), so the spline stays exactly where it is and flipping twice gives the file's bytes back. The instance does not move:
    /// the first point is no longer at its origin, as TEd puts it (a habit, not a rule: 10 of the kit's 132 rivers have it off zero; the
    /// river files carry world positions). One undo entry. TEd has no such command (wce's).
    ///
    /// # Errors
    ///
    /// Not a spline, or fewer than 2 points.
    pub fn flip_spline(&mut self, elem: usize) -> Result<(), String> {
        if !self.is_spline(elem) {
            return Err("not a spline".to_owned());
        }

        let sc = self.tile().tscene();
        let elems = sc.polygon_points(elem);
        let n = elems.len();
        if n < 2 {
            return Err("a spline needs at least 2 points".to_owned());
        }

        // Every point's attributes as written, the handles under each other's name.
        let doc = sc.doc();
        let texts = elems
            .iter()
            .map(|&p| {
                doc.elements()[p]
                    .attrs
                    .iter()
                    .map(|a| {
                        let name = match a.name.as_str() {
                            "controlIn" => "controlOut",
                            "controlOut" => "controlIn",
                            other => other,
                        };

                        (name.to_owned(), doc.get(p, &a.name).unwrap_or_default())
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let mark = self.begin()?;
        let sc = self.tscene_mut();
        for (i, &dst) in elems.iter().enumerate() {
            for (k, v) in &texts[n - 1 - i] {
                if sc.doc().get(dst, k).as_deref() != Some(v.as_str()) {
                    sc.set_attr(dst, k, v);
                }
            }
        }

        self.push_splice_touching("Flip spline".to_owned(), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem]);
        Ok(())
    }

    /// A value of spline points (`width`, `foam_amount`, `alpha_fade`, `flow_speed`, `terrain_offset`): one undo entry, the steps of one
    /// gesture merge.
    pub fn set_spline_value(&mut self, elem: usize, points: &[usize], attr: &str, value: &str, gesture: u64) -> bool {
        if !self.is_spline(elem) {
            return false;
        }

        let sc = self.tile().tscene();
        let elems = sc.polygon_points(elem);
        let steps = points
            .iter()
            .filter_map(|&i| {
                let before = sc.doc().get(*elems.get(i)?, attr).unwrap_or_default();
                Some((i, attr.to_owned(), before, value.to_owned()))
            })
            .collect::<Vec<_>>();
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
        self.push_splice_added("Add layer".to_owned(), mark, vec![], vec![item_of(layer)], &[layer]);
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
        self.check_fits([self.tile().tscene().type_of(elem)], layer)?;

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
        self.push_splice_touching(format!("Set point {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem]);
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
        self.push_splice_attrs(format!("Set {attr}"), mark, vec![item_of(elem)], vec![item_of(elem)], vec![elem], &[attr]);
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
        let todo = elems
            .iter()
            .copied()
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
        let holder = layer < sc.doc().elements().len() && wce_tile::parents::child_kind(sc.type_of(layer)).is_some();
        if elems.is_empty() || layer >= sc.doc().elements().len() || !(sc.is_layer(layer) || holder) {
            return Err("nothing to move, or the target is not a layer".to_owned());
        }

        let n = sc.doc().elements().len();
        self.check_fits(top_level(sc, elems.iter().copied().filter(|&e| e < n).collect()).into_iter().map(|e| sc.type_of(e)), layer)?;

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
        top.iter().try_for_each(|&e| wce_tile::parents::fits(sc.type_of(e), "UserInstanceGroup"))?;
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
        let groups = top_level(
            sc,
            elems.iter().copied().filter(|&e| e < n && matches!(sc.type_of(e), "UserInstanceGroup" | "INSTANCE_GROUP")).collect(),
        );
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
                self.push_splice(
                    if groups.len() == 1 { "Ungroup".to_owned() } else { format!("Ungroup {} groups", groups.len()) },
                    mark,
                    sel_before,
                    freed.iter().map(|&m| item_of(m)).collect(),
                );
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
    /// several elements are made whole: a capture location with its 20 m boundary circle, a siege AI node with its 20 m square Bounding Box
    /// and a 24 m Boundary, a deployment zone group with a 64 m square polygon (a drawn deployment polygon goes into the selected group,
    /// `into`, or into a new group), a siege AI edge with its drawn Street and, a street edge, a Street boundary 8 m to each side. A siege AI
    /// shape or a capture location boundary goes only into its holder (`into`), in a role of it (`shape_role`). One undo entry; the
    /// selection afterwards is it (a holder's shape: the holder).
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
    pub fn create_with(
        &mut self,
        kind: &str,
        into: usize,
        shape: &Create,
        key: &str,
        attrs: &BTreeMap<String, String>,
    ) -> Result<usize, String> {
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
                "ZONE_POLYGON_INSTANCE"
                | "CivilianDeploymentZone"
                | "LiteBuildingZone"
                | "AI_HINT_POLYGON_INSTANCE"
                | "NOGO_REGION_POLYGON_INSTANCE"
                | "GO_REGION_POLYGON_INSTANCE"
                | "VegetationExclusionPolygonInstance",
                Create::Line(pts),
            ) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, true).position(c)
            }

            // A shape of a siege AI node or edge or of a capture location: in the role named (a draft's `name`), else the first one the
            // holder lacks; it replaces the holder's one of that role when it has only one.
            ("SIEGE_AI_POLYGON_INSTANCE" | "CaptureLocationPolygonInstance", Create::Line(pts)) => {
                let role = self.shape_role(into, attrs.get("name").map(String::as_str))?;
                let (c, mut points) = line(pts)?;
                if role.name == "Bounding Box" {
                    points = bounding_rectangle(points);
                }

                let mut n = new_polygon(kind, points, role.closed).position(c).attr("name", role.name);
                if role.closed {
                    n = n.attr("polygon_closed", "1");
                }

                if kind == "CaptureLocationPolygonInstance" {
                    n = n.attr("polygon_shape", "PS_POLYGON");
                }

                n
            }

            (super::WALL_SPLINE, Create::Line(pts)) => {
                // A wall's ends go onto the fort buildings' ends near them.
                let mut pts = pts.clone();
                if let [first, .., last] = pts.as_mut_slice() {
                    *first = t.snap_wall_end(*first);
                    *last = t.snap_wall_end(*last);
                }

                let (c, points) = line(&pts)?;
                new_polygon(kind, points, false).position(c).attr(wce_tile::new_instances::WALL_ID, t.next_wall_id().to_string())
            }

            (super::ROAD_SPLINE, Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c).attr("road_id", t.next_road_id().to_string())
            }

            ("AISeparatorInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                new_polygon(kind, points, false).position(c)
            }

            // A line: no area. An angle finder: an arm's end, the corner, the other arm's end.
            (wce_tile::new_instances::MEASURE_LINE | wce_tile::new_instances::ANGLE_FINDER, Create::Line(pts)) => {
                if kind == wce_tile::new_instances::ANGLE_FINDER && pts.len() != 3 {
                    return Err(ANGLE_POINTS.to_owned());
                }

                let (c, points) = line(pts)?;
                let mut n = new_polygon(kind, points, false).position(c);
                n.attrs.remove("area");
                n
            }

            ("DEPLOYMENT_ZONE_POLYGON_INSTANCE", Create::Line(pts)) => {
                let (c, points) = line(pts)?;
                if into_type == "DeploymentZonePolygonInstanceGroup" {
                    new_polygon(kind, points, true).position(c)
                } else {
                    // A new group around it (scripts: `add_polygon`; the editor draws regions in a zone), the polygon with the values asked
                    // for it (they were left out here before).
                    let mut g =
                        NewInstance::new("DeploymentZonePolygonInstanceGroup").position(c).attr("deployment_category", NEW_ZONE_CATEGORY);
                    let mut p = new_polygon(kind, points, true);
                    p.attrs.extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
                    g.children.push(element_text(&p)?);
                    g
                }
            }

            ("SiegeAIEdgeInstance", Create::Line(pts)) => {
                let (c, points) = line(pts)?;

                // TEd's shapes: the drawn line is the Street; a street edge has a Street boundary to each side of it, 8 m away (CA's
                // corridors are about 16 m wide).
                let mut n = NewInstance::new(kind).position(c);
                let ty = attrs.get("type").map_or("SAET_STREET", String::as_str);
                let sides = if ty == "SAET_STREET" { side_lines(&points, 8.0 * unit) } else { vec![] };
                n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", points, false).attr("name", "Street"))?);
                for side in sides {
                    n.children.push(element_text(&new_polygon("SIEGE_AI_POLYGON_INSTANCE", side, false).attr("name", "Street boundary"))?);
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
                if kind == "SplineRiverInstance" && self.tile().rivers_keep_heights() {
                    n = n.attr(wce_tile::river_files::RIVER_HEIGHT_ATTR, "game");
                }

                n.points = wce_tile::splines::new_points(&positions)
                    .into_iter()
                    .map(|p| {
                        let mut q = NewPoint::at(p.position);
                        q.attrs.insert("controlIn".to_owned(), wce_tile::new_instances::point_text(p.control_in));
                        q.attrs.insert("controlOut".to_owned(), wce_tile::new_instances::point_text(p.control_out));
                        q
                    })
                    .collect();
                n
            }

            (_, Create::Line(_)) => return Err(format!("{kind} is placed, not drawn")),
        };

        let mut element = element;
        if element.kind == kind {
            element.attrs.extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
        }

        self.check_fits([element.kind.as_str()], into)?;

        // A holder's shape: in its role's place among the holder's shapes (TEd's order), in place of the one it replaces.
        let placed = if wce_tile::parents::holders(&element.kind).is_empty() { None } else { Some(self.shape_place(into, &element)) };
        let text = element_text(&element)?;
        let mark = self.begin()?;
        let inserted = self.tscene_mut().insert_element_text(into, &text).and_then(|e| match placed {
            Some((before, replaced)) => self.put_shape(e, into, before, replaced),
            None => Ok(e),
        });

        // A holder's new shape leaves the holder selected (its shapes are picked only to be edited by hand); it is before the shape,
        // so the insert did not move it.
        let holder = placed.is_some() && wce_tile::parents::child_kind(self.tile().tscene().type_of(into)).is_some();
        let selected = |e: usize| vec![item_of(if holder { into } else { e })];
        match inserted {
            Ok(e) if placed.is_some_and(|p| p != (None, None)) => {
                self.push_splice(format!("Add {kind}"), mark, vec![], selected(e));
                Ok(e)
            }

            Ok(e) => {
                self.push_splice_added(format!("Add {kind}"), mark, vec![], selected(e), &[e]);
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
        let key = self.prefab_key(group);
        let place = self.prefab_ground_place(group);
        if self.tile().tscene().doc().get(group, wce_tile::project_files::PREFAB_ATTR).is_some() {
            self.set_attribute(group, wce_tile::project_files::PREFAB_ATTR, "");
        }

        let renamed = self.rename(group, name);

        // Its ground stays: the layer it owned (still there until the ground is looked at again) becomes the user's.
        if let (Some(key), Some(n)) = (key, place) {
            self.keep_prefab_ground(n, &key);
        }

        renamed
    }

    /// A structural edit (adding, taking away, moving or grouping items) of items inside a prefab reference, or into one, is refused: what
    /// changes in a reference's items is kept as overrides, values only.
    ///
    /// # Errors
    ///
    /// An item (or `into`) is inside a reference: says which.
    /// Whether elements of these types may go into `into`: a deployment region only into a deployment zone, a siege AI shape only into a
    /// node or an edge, a capture location's boundary only into one, and those holders hold nothing else (`wce_tile::parents`).
    ///
    /// # Errors
    ///
    /// One may not: says where it goes.
    pub fn check_fits<'a>(&self, kinds: impl IntoIterator<Item = &'a str>, into: usize) -> Result<(), String> {
        let sc = self.tile().tscene();
        if into >= sc.doc().elements().len() {
            return Ok(());
        }

        let into_kind = sc.type_of(into);
        kinds.into_iter().try_for_each(|k| wce_tile::parents::fits(k, into_kind))
    }

    /// The nearest of `elem` and the elements above it that can hold a `kind` (`wce_tile::parents::holders`).
    pub fn holder_for(&self, kind: &str, elem: usize) -> Option<usize> {
        let sc = self.tile().tscene();
        let els = sc.doc().elements();
        let holders = wce_tile::parents::holders(kind);
        std::iter::successors(Some(elem).filter(|&e| e < els.len()), |&x| els[x].parent).find(|&x| holders.contains(&sc.type_of(x)))
    }

    /// The shapes a holder has (`wce_tile::parents::roles`), by its type.
    pub fn holder_roles(&self, holder: usize) -> &'static [wce_tile::parents::Role] {
        let sc = self.tile().tscene();
        if holder >= sc.doc().elements().len() {
            return &[];
        }

        wce_tile::parents::roles(sc.type_of(holder), &sc.doc().get(holder, "type").unwrap_or_default())
    }

    /// A holder's shapes of each of its roles.
    fn role_shapes(&self, holder: usize) -> Vec<(wce_tile::parents::Role, Vec<usize>)> {
        let sc = self.tile().tscene();
        let kids = sc.children(holder);
        self.holder_roles(holder).iter().map(|&r| (r, kids.iter().copied().filter(|&c| sc.name(c) == r.name).collect())).collect()
    }

    /// The role a new shape takes in a holder: `named` (one of its roles), else the first one it has fewer of than it may have, else the
    /// first (which it replaces).
    ///
    /// # Errors
    ///
    /// Not a holder, a name that is not one of its roles, or a role it has all of that it may have more than one of (a street edge's two
    /// boundaries).
    pub fn shape_role(&self, holder: usize, named: Option<&str>) -> Result<wce_tile::parents::Role, String> {
        let shapes = self.role_shapes(holder);
        let Some(first) = shapes.first().map(|c| c.0) else {
            return Err("draw it inside a siege AI node or edge, a deployment zone or a capture location".to_owned());
        };

        let role =
            match named.filter(|n| !n.is_empty()) {
                Some(n) => shapes.iter().find(|c| c.0.name == n).map(|c| c.0).ok_or_else(|| {
                    format!("a {} has no shape named {n}", wce_tile::parents::label(self.tile().tscene().type_of(holder)))
                })?,

                None => shapes.iter().find(|c| c.0.most == 0 || c.1.len() < c.0.most).map_or(first, |c| c.0),
            };

        let have = shapes.iter().find(|c| c.0 == role).map_or(0, |c| c.1.len());
        if role.most > 1 && have >= role.most {
            return Err(format!("it has its {} {}s already: delete one first", role.most, role.name.to_lowercase()));
        }

        Ok(role)
    }

    /// Where a new shape (its role from its `name`) goes among its holder's: in place of the one it replaces (its role's, when the holder
    /// has only one), else in front of the first shape of a later role; and the one it replaces.
    fn shape_place(&self, holder: usize, shape: &wce_tile::new_instances::NewInstance) -> (Option<usize>, Option<usize>) {
        let name = shape.attrs.get("name").map(String::as_str).unwrap_or_default();
        let shapes = self.role_shapes(holder);
        let Some(at) = shapes.iter().position(|c| c.0.name == name) else { return (None, None) };
        let replaced = if shapes[at].0.most == 1 { shapes[at].1.first().copied() } else { None };
        let later = shapes[at + 1..].iter().flat_map(|c| c.1.iter().copied()).min();
        (replaced.or(later), replaced)
    }

    /// A shape just put at the end of its holder moved in front of `before`, and `replaced` (which is before it) taken away; returns where
    /// the shape is then. Inside an edit.
    fn put_shape(&mut self, shape: usize, holder: usize, before: Option<usize>, replaced: Option<usize>) -> Result<usize, String> {
        let Some(before) = before else { return Ok(shape) };
        let moved = self.tscene_mut().move_elements(vec![shape], holder, Some(before))?;
        let at = moved.first().copied().unwrap_or(shape);
        let Some(r) = replaced else { return Ok(at) };

        // In front of the one it replaces: that one comes after it now.
        let r = if r >= at { r + self.tile().tscene().doc().subtree_size(at) } else { r };
        self.tscene_mut().remove(r)?;
        Ok(at)
    }

    /// A capture location's boundary as a preset shape (`BoundaryShape`): its kind, its size (metres: a circle's or a semicircle's
    /// radius, a square's half side; a polygon's: its farthest point from its centre) and its turn (degrees: which way a semicircle's
    /// round side, a square's sides face; 0: +z), read back from what it is (TEd keeps no more: a semicircle's turn is in its points).
    pub fn boundary_shape(&self, boundary: usize) -> Option<BoundaryShape> {
        let t = self.tile();
        let sc = t.tscene();
        if boundary >= sc.doc().elements().len() || sc.type_of(boundary) != "CaptureLocationPolygonInstance" {
            return None;
        }

        let u = t.unit() as f32;
        let pts = t.polygon_world_points(boundary);
        let at = world_transform(sc, boundary).position;
        let position = Float3::new((at.x / f64::from(u)) as f32, (at.y / f64::from(u)) as f32, (at.z / f64::from(u)) as f32);
        let radius = || sc.doc().get(boundary, "radius").and_then(|r| r.trim().parse::<f32>().ok()).map(|r| r / u);
        let mean = || {
            let n = pts.len().max(1) as f32;
            pts.iter().fold(Float3::default(), |a, &p| a + p) * (1.0 / n)
        };

        // The angle of a direction on the ground (x = sin, z = cos: 0 is +z, 90 is +x).
        let angle = |dx: f32, dz: f32| dx.atan2(dz).to_degrees();
        Some(match sc.doc().get(boundary, "polygon_shape").as_deref() {
            Some("PS_CIRCLE") => BoundaryShape { kind: BoundaryKind::Circle, centre: position, size: radius().unwrap_or(20.0), turn: 0.0 },
            Some("PS_SEMICIRCLE") => {
                let mid = pts.get(pts.len() / 2).copied().unwrap_or(position);
                BoundaryShape {
                    kind: BoundaryKind::Semicircle,
                    centre: position,
                    size: radius().unwrap_or(20.0),
                    turn: angle(mid.x - position.x, mid.z - position.z),
                }
            }

            Some("PS_RECTANGLE") if pts.len() >= 2 => {
                let (a, b) = (pts[0], pts[1]);
                let turn = (-(b.z - a.z)).atan2(b.x - a.x).to_degrees();
                BoundaryShape { kind: BoundaryKind::Square, centre: mean(), size: (b.x - a.x).hypot(b.z - a.z) * 0.5, turn }
            }

            _ => {
                let c = mean();
                let size = pts.iter().map(|p| (p.x - c.x).hypot(p.z - c.z)).fold(0.0, f32::max);
                BoundaryShape { kind: BoundaryKind::Polygon, centre: c, size, turn: 0.0 }
            }
        })
    }

    /// A capture location's boundary made a preset shape (TEd's own: `PS_CIRCLE` with its 20 points, `PS_SEMICIRCLE` with 21 over half a
    /// turn, `PS_RECTANGLE` with 4 corners), at `shape.centre`, `shape.size` and `shape.turn`; it keeps its own attributes (name, flags,
    /// colours...) and its place among its capture location's children. A polygon: its points as they are, its shape `PS_POLYGON`
    /// (freely edited). One undo entry; the selection afterwards is `select` (the capture location, or the new boundary: `None`).
    ///
    /// # Errors
    ///
    /// Not a capture location's boundary.
    pub fn set_boundary_shape(&mut self, boundary: usize, shape: BoundaryShape, select: Option<usize>) -> Result<usize, String> {
        let t = self.tile();
        let sc = t.tscene();
        if boundary >= sc.doc().elements().len() || sc.type_of(boundary) != "CaptureLocationPolygonInstance" {
            return Err("not a capture location's boundary".to_owned());
        }

        let holder = self
            .holder_for("CaptureLocationPolygonInstance", sc.doc().elements()[boundary].parent.unwrap_or(boundary))
            .ok_or("the boundary is not in a capture location")?;
        let (r, turn) = (shape.size.max(0.5), shape.turn.to_radians());

        // On the ground, around the centre (metres): the canonical shape (a semicircle's round side and a square's top on +z) turned.
        let c = shape.centre;
        let around = |x: f32, z: f32| {
            let (sn, cs) = turn.sin_cos();
            Float3::new(c.x + x * cs + z * sn, c.y, c.z - x * sn + z * cs)
        };
        let ring = |a_deg: f32| {
            let a = a_deg.to_radians();
            around(r * a.sin(), r * a.cos())
        };
        let (kind, world): (&str, Vec<Float3>) = match shape.kind {
            BoundaryKind::Circle => ("PS_CIRCLE", (0..20).map(|i| ring(i as f32 * 18.0)).collect()),
            BoundaryKind::Semicircle => ("PS_SEMICIRCLE", (0..21).map(|i| ring(-90.0 + i as f32 * 9.0)).collect()),
            BoundaryKind::Square => ("PS_RECTANGLE", [(-r, r), (r, r), (r, -r), (-r, -r)].into_iter().map(|(x, z)| around(x, z)).collect()),
            BoundaryKind::Polygon => ("PS_POLYGON", t.polygon_world_points(boundary)),
        };

        // Local to the capture location; the boundary at the centre (a circle's and a semicircle's points are around it, as TEd's).
        let centre = t.to_local(holder, c);
        let local = world.iter().map(|&p| t.to_local(holder, p) - centre).collect::<Vec<_>>();
        let mut n = new_polygon("CaptureLocationPolygonInstance", local, true)
            .position(centre)
            .attr("polygon_shape", kind)
            .attr("polygon_closed", "1");
        if matches!(shape.kind, BoundaryKind::Circle | BoundaryKind::Semicircle) {
            n = n.attr("radius", wce_tile::ted_scene::format_number(f64::from(r) * t.unit()));
        }

        // Its own attributes kept (not what the shape decides).
        const SHAPED: [&str; 7] = ["position", "orientation", "polygon_shape", "radius", "area", "polygon_closed", "points"];
        for a in &sc.doc().elements()[boundary].attrs {
            if !SHAPED.contains(&a.name.as_str())
                && !n.attrs.contains_key(&a.name)
                && let Some(v) = sc.doc().get(boundary, &a.name)
            {
                n.attrs.insert(a.name.clone(), v);
            }
        }

        let text = wce_tile::new_instances::element_text(&n)?;
        let mark = self.begin()?;
        let result =
            self.tscene_mut().insert_element_text(holder, &text).and_then(|e| self.put_shape(e, holder, Some(boundary), Some(boundary)));
        match result {
            Ok(e) => {
                let after = select.unwrap_or(e);
                self.push_splice(format!("Boundary: {}", shape.kind.label()), mark, vec![item_of(after)], vec![item_of(after)]);
                Ok(e)
            }

            Err(err) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                Err(err)
            }
        }
    }

    /// A boundary as another preset (`kind`), keeping its centre and size; a semicircle or a square made from what had no turn (a circle, a
    /// polygon) faces the way its capture location's flag does (`flag_facing`).
    pub fn boundary_preset(&self, boundary: usize, kind: BoundaryKind) -> Option<BoundaryShape> {
        let mut shape = self.boundary_shape(boundary)?;
        if kind.turns() && !shape.kind.turns() {
            let sc = self.tile().tscene();
            let location = self.holder_for("CaptureLocationPolygonInstance", boundary);
            let facing = location.and_then(|l| sc.doc().get(l, "flag_facing")).and_then(|f| f.trim().parse::<f32>().ok());
            shape.turn = facing.map_or(0.0, f32::to_degrees);
        }

        shape.kind = kind;
        Some(shape)
    }

    /// The boundary of a capture location (its first `CaptureLocationPolygonInstance`).
    pub fn capture_boundary(&self, location: usize) -> Option<usize> {
        let sc = self.tile().tscene();
        (location < sc.doc().elements().len())
            .then(|| sc.children(location).into_iter().find(|&c| sc.type_of(c) == "CaptureLocationPolygonInstance"))
            .flatten()
    }

    /// The Inspector's rows of a capture location's boundary in place of TEd's shape and radius: its shape (Circle, Semicircle, Square,
    /// Polygon) and, for the presets, its size and turn, any change making its points again (`set_boundary_shape`; a drag at its end).
    fn boundary_shape_rows(&mut self, e: usize, ins: &mut dyn Inspector) {
        let Some(shape) = self.boundary_shape(e) else { return };
        let kinds = BoundaryKind::ALL.map(|k| k.label().to_owned());
        let mut idx = BoundaryKind::ALL.iter().position(|&k| k == shape.kind).unwrap_or(0) as i32;
        let mut apply = None;
        ins.hint("The boundary's shape; choosing Circle, Semicircle or Square makes its points again.");
        if ins.edit_choice("wce_boundary_shape", "shape", &mut idx, &kinds) {
            apply = self.boundary_preset(e, BoundaryKind::ALL[idx.clamp(0, 3) as usize]);
        }

        if shape.kind != BoundaryKind::Polygon {
            for (field, label, value, speed) in [
                ("wce_boundary_size", "size (m: radius or half side)", shape.size, 0.1),
                ("wce_boundary_turn", "turn (deg; 0: +z)", shape.turn, 1.0),
            ] {
                if field == "wce_boundary_turn" && !shape.kind.turns() {
                    continue;
                }

                let key = (e, field.to_owned());
                let mut v = self.tile().drafts.get(&key).and_then(|d| d.parse::<f32>().ok()).unwrap_or(value);
                ins.hint(if field == "wce_boundary_size" {
                    "The boundary's size in metres: a circle's radius, half a square's side. Applied when the drag ends."
                } else {
                    "Which way the boundary faces, in degrees (0: +z). Applied when the drag ends."
                });
                if ins.edit_float(
                    field,
                    label,
                    &mut v,
                    speed,
                    if field == "wce_boundary_size" { 0.5 } else { -360.0 },
                    if field == "wce_boundary_size" { 500.0 } else { 360.0 },
                ) {
                    self.tile_mut().drafts.insert(key.clone(), v.to_string());
                }

                if ins.gesture_ended()
                    && let Some(d) = self.tile_mut().drafts.remove(&key).and_then(|d| d.parse::<f32>().ok())
                    && (d - value).abs() > 1e-4
                {
                    let mut s = shape;
                    if field == "wce_boundary_size" {
                        s.size = d
                    } else {
                        s.turn = d
                    }
                    apply = Some(s);
                }
            }
        }

        if let Some(s) = apply
            && let Err(err) = self.set_boundary_shape(e, s, None)
        {
            wce_foundation::log::warn(err);
        }
    }

    pub fn check_structure(&self, elems: &[usize], into: Option<usize>) -> Result<(), String> {
        let doc = self.tile().tscene().doc();
        let els = doc.elements();
        let locked = |e: usize, itself: bool| -> Option<usize> {
            let mut chain =
                std::iter::successors(if itself { Some(e) } else { els.get(e)?.parent }, |&p| els.get(p).and_then(|x| x.parent));
            chain.find(|&p| self.is_prefab_reference(p))
        };

        let hit = elems.iter().find_map(|&e| locked(e, false)).or_else(|| into.and_then(|i| locked(i, true)));
        match hit {
            Some(r) => Err(format!(
                "that is part of the prefab {}: a reference's items change only in their values (overrides); Open in Prefab tool changes the prefab, Unpack prefab makes it a plain group",
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

        let attrs = back.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>();
        self.push_splice_attrs(
            format!("Revert the overrides of an item of {key}"),
            mark,
            vec![item_of(elem)],
            vec![item_of(elem)],
            vec![elem],
            &attrs,
        );
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

            ins.hint("Gives all the reference's items the prefab's values back (one undo step).");
            if ins.button("revert_all", "Revert all overrides", count.is_some_and(|n| n > 0)) {
                self.revert_overrides(e);
            }

            if self.tile().prefab_tool_ready {
                ins.hint("Opens the prefab itself in the Prefab tool, to change it for every reference.");
            }

            if self.tile().prefab_tool_ready && ins.button("open_in_prefab_tool", "Open in Prefab tool", true) {
                self.tile_mut().prefab_open_request = Some(key.clone());
            }

            self.prefab_ground_rows(e, ins);
            return;
        }

        let Some((key, diffs)) = self.overrides_of(e) else { return };
        ins.text("prefab", "in the prefab", &key);
        ins.text("overrides", "overrides", &if diffs.is_empty() { "none".to_owned() } else { format!("{} (marked)", diffs.len()) });
        let revertible = diffs.iter().any(|(_, v)| v.is_some());
        ins.hint("Gives this item the prefab's values back (one undo step).");
        if ins.button("revert", "Revert to the prefab's values", revertible) {
            self.revert_overrides(e);
        }

        let marks = diffs
            .into_iter()
            .map(|(a, v)| {
                let note = match v {
                    Some(v) => format!("Override: the prefab has {v}"),
                    None => "Override: the prefab's item has no such value".to_owned(),
                };

                (a, note)
            })
            .collect::<Vec<_>>();
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

        let elems = groups.iter().map(|(g, _)| *g).collect();
        self.push_splice_attrs(what.to_owned(), mark, vec![], vec![], elems, &[wce_tile::project_files::PREFAB_ATTR]);
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
        let spans = self.subtree_spans(&top);
        let Ok(mark) = self.begin() else { return false };

        // From the last one: the indices of the others stay.
        for &e in top.iter().rev() {
            let _ = self.tscene_mut().remove(e);
        }

        self.push_splice_removed(name, mark, sel_before, spans);
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
        self.push_splice_added("Duplicate layer".to_owned(), mark, vec![item_of(layer)], vec![item_of(copy)], &[copy]);
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

            let mut it = PlacementItem {
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: None,
                pos_new: None,
                idx_old: code(cur),
                idx_new: code(mode),
                pos_old_text: None,
            };
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
        let name = format!(
            "{}{}",
            if mode == Placement::Landscape { "Landscape placement" } else { "Freeform placement" },
            if n == 1 { String::new() } else { format!(" ({n} items)") }
        );
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
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: t.tscene().position(e),
                pos_new: Some(t.local_for_world_y(e, f64::from(y))),
                idx_old: code,
                idx_new: code,
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

    // ---- align and distribute

    /// Moves instances to world points (metres) in one undo entry named `name`: a landscape instance takes the terrain's height there.
    /// Returns how many moved.
    pub fn move_instances(&mut self, name: &str, moves: &[(usize, Float3)]) -> usize {
        let t = self.tile();
        let mut items = vec![];
        for &(e, p) in moves {
            if !t.is_instance(e) {
                continue;
            }

            let cur = t.placement(e);
            let y = if cur == Placement::Landscape { t.landscape_y(e, p.x, p.z) } else { p.y };
            let pw = parent_transform(t.tscene(), e);
            let lp = pw.rotation.transpose() * (scene_point(Float3::new(p.x, y, p.z), t.unit()) - pw.position);
            if t.tscene().position(e).is_some_and(|old| close(old, lp)) {
                continue;
            }

            let code = match cur {
                Placement::Landscape => 1,
                Placement::Freeform => 2,
                Placement::None => 0,
            };
            items.push(PlacementItem {
                elem: e,
                mode_old: None,
                mode_new: None,
                pos_old: t.tscene().position(e),
                pos_new: Some(lp),
                idx_old: code,
                idx_new: code,
                pos_old_text: t.tscene().doc().get(e, "position"),
            });
        }

        if items.is_empty() {
            return 0;
        }

        let n = items.len();
        let name = if n == 1 { name.to_owned() } else { format!("{name} ({n} items)") };
        self.history.push(Box::new(TedPlacementCommand { name, items }));
        n
    }

    /// Lines the instances up on an axis (0 x, 1 height, 2 z) at `to`: their lowest, middle or highest value there, or the first one's. The
    /// height lines up the freeform ones only (a landscape instance stands on the terrain). One undo entry; how many moved.
    pub fn align(&mut self, elems: &[usize], axis: usize, to: AlignTo) -> usize {
        let t = self.tile();
        let items = elems
            .iter()
            .copied()
            .filter(|&e| t.is_instance(e) && !(axis == 1 && t.placement(e) == Placement::Landscape))
            .collect::<Vec<_>>();
        if items.len() < 2 {
            return 0;
        }

        let at = |e: usize| t.world_position(e).to_arr()[axis];
        let values = items.iter().map(|&e| at(e)).collect::<Vec<_>>();
        let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let v = match to {
            AlignTo::Min => lo,
            AlignTo::Centre => (lo + hi) * 0.5,
            AlignTo::Max => hi,
            AlignTo::First => values[0],
        };

        let moves = items
            .iter()
            .map(|&e| {
                let mut p = t.world_position(e).to_arr();
                p[axis] = v;
                (e, Float3::new(p[0], p[1], p[2]))
            })
            .collect::<Vec<_>>();
        self.move_instances(&format!("Align {}", ["x", "height", "z"][axis]), &moves)
    }

    /// Spaces the instances evenly between the two furthest apart on an axis (0 x, 2 z), in their order along it; with `axis` None, along
    /// the line from the first to the last of `elems` (the others in their order along it, put on the line). One undo entry.
    pub fn distribute(&mut self, elems: &[usize], axis: Option<usize>) -> usize {
        let t = self.tile();
        let items = elems.iter().copied().filter(|&e| t.is_instance(e)).collect::<Vec<_>>();
        if items.len() < 3 {
            return 0;
        }

        let pos = |e: usize| t.world_position(e);
        let moves = match axis {
            Some(axis) => {
                let mut by = items.iter().map(|&e| (pos(e).to_arr()[axis], e)).collect::<Vec<_>>();
                by.sort_by(|a, b| a.0.total_cmp(&b.0));
                let (lo, hi) = (by[0].0, by[by.len() - 1].0);
                let n = (by.len() - 1) as f32;
                by.iter()
                    .enumerate()
                    .map(|(i, &(_, e))| {
                        let mut p = pos(e).to_arr();
                        p[axis] = lo + (hi - lo) * i as f32 / n;
                        (e, Float3::new(p[0], p[1], p[2]))
                    })
                    .collect::<Vec<_>>()
            }

            None => {
                let (a, b) = (pos(items[0]), pos(items[items.len() - 1]));
                let d = b - a;
                let len2 = (d.x * d.x + d.z * d.z).max(1e-9);
                let mut by = items.iter().map(|&e| (((pos(e).x - a.x) * d.x + (pos(e).z - a.z) * d.z) / len2, e)).collect::<Vec<_>>();
                by.sort_by(|x, y| x.0.total_cmp(&y.0));
                let n = (by.len() - 1) as f32;
                by.iter().enumerate().map(|(i, &(_, e))| (e, a + d * (i as f32 / n))).collect::<Vec<_>>()
            }
        };

        self.move_instances(if axis.is_some() { "Distribute" } else { "Distribute along a line" }, &moves)
    }

    // ---- height strokes

    /// A step of a height stroke: begins the stroke on the first step; `end_stroke` when the mouse goes up (also done by undo / redo and save).
    pub fn brush_step(&mut self, kind: BrushKind, x: f32, z: f32, settings: BrushSettings, dt: f32) {
        let ready = match kind {
            BrushKind::Paint(_) => self.tile_mut().project.load_blend().is_ok(),
            BrushKind::Ground(_) => self.tile_mut().project.load_ground().is_ok(),
            _ => self.tile().project.has_height(),
        };

        if !ready {
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
        self.history.push(Box::new(BrushStepCommand {
            kind,
            x,
            z,
            settings,
            dt,
            stroke,
            first: true,
            moved: vec![],
            where_: HashMap::new(),
        }));
        let version = self.history.version();
        self.tile_mut().stroke_version = version;
    }

    /// Sets one of the entry's four texture groups (0..4 = red, green, blue, alpha), one undo entry; saved into the entry next to the tile.
    ///
    /// # Errors
    ///
    /// The tile has no entry, or no such slot.
    /// Replaces the tile's links (its database entry's link targets and rules, Phase 6a of docs/plans/tile_ted_parity.md): one undo
    /// entry named `name` (the steps of one `gesture` merge); nothing when they are the same. Written to `TILES/<tile>.xml` on save (`tile_database.bin` is compiled from the
    /// entries at export).
    ///
    /// # Errors
    ///
    /// The tile has no database entry.
    pub fn set_links(
        &mut self,
        name: &str,
        targets: Vec<wce_formats::tile_entry::TileLinkTarget>,
        links: Vec<wce_formats::tile_entry::TileLink>,
        gesture: u64,
    ) -> Result<bool, String> {
        let Some(entry) = self.tile().project.entry.as_ref() else { return Err("the tile has no database entry".to_owned()) };
        let before = (entry.link_targets(), entry.links());
        let strip_t = |v: &[wce_formats::tile_entry::TileLinkTarget]| {
            v.iter().map(|t| (t.serialise_version, t.tile_set.clone(), t.x, t.y)).collect::<Vec<_>>()
        };
        let strip_l = |v: &[wce_formats::tile_entry::TileLink]| {
            v.iter()
                .map(|l| {
                    (
                        l.serialise_version,
                        l.link_set.clone(),
                        l.x,
                        l.y,
                        l.base_x,
                        l.base_y,
                        l.is_entry,
                        l.blend_size,
                        l.no_offline_blend,
                        l.test.clone(),
                        l.has_blend_quad,
                    )
                })
                .collect::<Vec<_>>()
        };
        if strip_t(&before.0) == strip_t(&targets) && strip_l(&before.1) == strip_l(&links) {
            return Ok(false);
        }

        self.end_stroke();
        self.history.push(Box::new(super::commands::LinksCommand { name: name.to_owned(), before, after: (targets, links), gesture }));
        Ok(true)
    }

    /// Sets an attribute of the tile's database entry (Tile properties): of its `<TILE>` element, or of its variation with `variation`; one
    /// undo entry (the steps of one `gesture` merge). Written to the entry beside the tile on save.
    ///
    /// # Errors
    ///
    /// The tile has no database entry.
    pub fn set_entry_attr(&mut self, variation: bool, attr: &str, value: &str, gesture: u64) -> Result<bool, String> {
        let Some(entry) = self.tile().project.entry.as_ref() else { return Err("the tile has no database entry".to_owned()) };
        let v = variation.then(|| self.tile().project.variation.unwrap_or(0));
        let before = match v {
            Some(v) => entry.variation_attr(v, attr),
            None => entry.tile_attr(attr),
        }
        .unwrap_or_default();
        if before == value {
            return Ok(false);
        }

        self.end_stroke();
        self.history.push(Box::new(super::commands::EntryAttrCommand {
            variation: v,
            attr: attr.to_owned(),
            before,
            after: value.to_owned(),
            gesture,
        }));
        Ok(true)
    }

    pub fn set_texture_group(&mut self, slot: usize, name: &str) -> Result<(), String> {
        let Some(entry) = self.tile().project.entry.as_ref() else { return Err("the tile has no database entry".to_owned()) };
        if slot >= 4 {
            return Err(format!("no texture group {slot} (0..3 = red, green, blue, alpha)"));
        }

        let g = entry.texture_group();
        let before = [g.red, g.green, g.blue, g.alpha][slot].clone();
        if before == name {
            return Ok(());
        }

        self.end_stroke();
        self.history.push(Box::new(TextureGroupCommand { slot, before, after: name.to_owned() }));
        Ok(())
    }

    /// Paints the water ground types from the water depth view's raster (`Tile::water_depth`): shallow water where the water is
    /// `shallow_m` .. `deep_m` deep, deep water from `deep_m`; a pixel painted as water where the water is shallower than `shallow_m` (or
    /// there is none) is cleared; other ground types are left where there is no water. One undo entry; returns the pixels changed.
    /// The kit's river tiles (`water_depth_check`): painted deep water lies 4.4 .. 5.7 m deep (quartiles), shallow 0.2 .. 1.25 m, the
    /// water left unpainted at its banks 1.25 m under the ground .. 0.8 m deep.
    ///
    /// # Errors
    ///
    /// No water depth worked out yet, or no ground type map.
    pub fn paint_water_ground_types(&mut self, shallow_m: f32, deep_m: f32) -> Result<usize, String> {
        use wce_tile::ground_types::{GROUND_COLOURS, NO_GROUND_TYPE};
        const DEEP: usize = 6;
        const SHALLOW: usize = 7;

        let water = self.tile().water_depth.clone().ok_or("the water depth is not worked out yet: show the water depth view")?;
        self.end_stroke();
        let t = self.tile_mut();
        let g = t.project.load_ground()?;
        let border = (g.width() as usize).saturating_sub(water.w.saturating_sub(1)) / 2;
        g.begin_edit()?;
        let mut changed = 0;
        for j in 0..water.h {
            for i in 0..water.w {
                let (x, y) = ((i + border) as u32, (j + border) as u32);
                if x >= g.width() || y >= g.height() {
                    continue;
                }

                let d = water.depth[j * water.w + i];
                let now = g.rgb(x, y);
                let is_water = now == GROUND_COLOURS[DEEP] || now == GROUND_COLOURS[SHALLOW];
                let want = if d.is_finite() && d >= deep_m {
                    GROUND_COLOURS[DEEP]
                } else if d.is_finite() && d >= shallow_m {
                    GROUND_COLOURS[SHALLOW]
                } else if is_water {
                    NO_GROUND_TYPE
                } else {
                    continue;
                };

                changed += usize::from(g.set_rgb(x, y, want));
            }
        }

        g.end_edit();
        if changed == 0 {
            return Ok(0);
        }

        self.history.push(Box::new(GroundFillCommand { name: "Paint water ground types".to_owned(), first: true }));
        Ok(changed)
    }

    /// Paints the whole tile at once (the texture painter's Paint the whole tile): one dab of the paint brush over the whole blend map, border
    /// included, at the brush's strength with no falloff, in its mode and through its filters: at strength 1, normal mode and no filter,
    /// every pixel becomes the channel's texture. Goes into the working terrain layer as a stroke does. One undo entry; false when nothing
    /// changed.
    ///
    /// # Errors
    ///
    /// The tile has no blend map, or the settings are not the paint brush's.
    pub fn paint_whole_tile(&mut self, kind: BrushKind, settings: &BrushSettings) -> Result<bool, String> {
        let BrushKind::Paint(channel) = kind else { return Err("not a paint brush".to_owned()) };
        self.end_stroke();
        let t = self.tile_mut();
        t.project.load_blend()?;
        let depth = t.project.blend.as_ref().map_or(0, wce_tile::blend_field::BlendField::edit_depth);
        t.paint_blend(usize::from(channel), None, settings, f64::from(settings.opacity).clamp(0.0, 1.0));
        t.end_stroke_edit();
        if t.project.blend.as_ref().map_or(0, wce_tile::blend_field::BlendField::edit_depth) == depth {
            return Ok(false);
        }

        self.history.push(Box::new(super::commands::BlendFillCommand { name: "Paint the whole tile".to_owned(), first: true }));
        Ok(true)
    }

    /// The tile's lowest and highest heights (metres as the editor shows them), `None` without a height map.
    pub fn height_span(&self) -> Option<(f32, f32)> {
        let p = &self.tile().project;
        let hf = p.height.as_ref()?;
        let (lo, scale) = hf.fit_range();
        Some(((lo / p.unit_scale) as f32, ((lo + scale) / p.unit_scale) as f32))
    }

    /// Imports a height map picture (`Tile::import_heights`): one undo entry; returns the samples changed.
    ///
    /// # Errors
    ///
    /// The picture does not read, or the tile has no height map.
    pub fn import_height_map(&mut self, path: &std::path::Path, low_m: f32, high_m: f32, add: bool) -> Result<usize, String> {
        let image = wce_tile::brush_image::BrushImage::read(path)?;
        self.end_stroke();
        let (changed, moved) = self.tile_mut().import_heights(&image, low_m, high_m, add)?;
        if changed == 0 && moved.is_empty() {
            return Ok(0);
        }

        let name = format!("Import height map {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        self.history.push(Box::new(HeightFillCommand { name, first: true, moved }));
        Ok(changed)
    }

    /// The game's texture groups: every climate's (the default climate has them all), sorted; a campaign database's also `climate` (the
    /// cell climate's own `texture_set`).
    pub fn texture_group_names(&self) -> Vec<String> {
        let climates = self.tile().climates();
        let mut names = climates.climates.iter().flat_map(|c| c.textures.iter().map(|t| t.name.clone())).collect::<Vec<_>>();
        if climates.kind == "campaign" {
            names.push("climate".to_owned());
        }

        names.sort();
        names.dedup();
        names
    }

    /// The brush stroked along a spline, polygon or line (TEd's Spline Paint Stroke, and its Spline splat's width): dabs a quarter of the
    /// radius apart along its curve (a closed polygon round to its first point), one undo entry; returns the dabs. A spline whose points
    /// have a `width` (rivers, roads) is painted as wide as it is there (the radius half the width, the points' widths blended along each
    /// segment, as TEd's Spline splat lays its channel); other lines with the brush's radius.
    ///
    /// # Errors
    ///
    /// The element is not a spline, polygon or line, or has fewer than two points.
    pub fn stroke_along(&mut self, kind: BrushKind, elem: usize, settings: &BrushSettings) -> Result<usize, String> {
        self.stroke_along_with(kind, elem, settings, false)
    }

    /// TEd's Spline Road (the Height Map module's `Spline Road` operation): the ground under the selected line levelled to the line's own
    /// height, as wide as the spline is at each point (else the brush's radius), the brush's falloff making the banks. TEd moves each height
    /// inside the road's strip towards the road's height there (`h + (road - h) * w`, its `apply` fn 0x00485160); here a flatten stroke
    /// along the line whose target at each dab is the line's height there, at full strength. One undo entry; returns the dabs.
    ///
    /// # Errors
    ///
    /// As [`Self::stroke_along`].
    pub fn level_along(&mut self, elem: usize, settings: &BrushSettings) -> Result<usize, String> {
        let s = BrushSettings { strength: 40.0, ..settings.clone() };
        self.stroke_along_with(BrushKind::Flatten, elem, &s, true)
    }

    fn stroke_along_with(&mut self, kind: BrushKind, elem: usize, settings: &BrushSettings, follow_height: bool) -> Result<usize, String> {
        const STEPS: usize = 16;
        let t = self.tile();
        if !t.is_polygon(elem) {
            return Err("select a spline, polygon or line to stroke along".to_owned());
        }

        let spline = self.is_spline(elem);
        let sc = t.tscene();
        let closed = if spline {
            sc.doc().get(elem, "closed").is_some_and(|v| v == "1")
        } else {
            sc.doc().get(elem, "polygon_closed").is_none_or(|v| v != "0")
        };
        let mut path = if spline { t.spline_world_curve(elem) } else { t.polygon_world_points(elem) };

        // The points' widths (metres), when they have them, at every sample of the curve.
        let points = sc.polygon_points(elem);
        let widths = points.iter().map(|&p| sc.doc().get(p, "width").and_then(|v| wce_formats::c_num::strtod(&v))).collect::<Vec<_>>();
        let mut radii = if spline && widths.iter().any(Option::is_some) && widths.len() >= 2 {
            let w = widths.iter().map(|w| w.unwrap_or(0.0) * 0.5).collect::<Vec<_>>();
            let n = w.len();
            let segments = if closed && n > 2 { n } else { n - 1 };
            let mut r = vec![w[0]];
            for i in 0..segments {
                let (a, b) = (w[i], w[(i + 1) % n]);
                r.extend((1..=STEPS).map(|k| a + (b - a) * k as f64 / STEPS as f64));
            }

            r
        } else {
            vec![f64::from(settings.radius_m); path.len()]
        };

        if !spline && closed && path.len() > 2 {
            path.push(path[0]);
            radii.push(radii[0]);
        }

        if path.len() < 2 || radii.len() != path.len() {
            return Err("the line has fewer than two points".to_owned());
        }

        // A dab every quarter radius, each with the most a frame gives (`apply_brush` takes at most 0.05 s).
        let spacing = |r: f64| (r * 0.25).max(0.25);
        let mut dabs = vec![(path[0].x, path[0].z, radii[0], path[0].y)];
        let mut left = spacing(radii[0]);
        for (w, r) in path.windows(2).zip(radii.windows(2)) {
            let (a, b) = (w[0], w[1]);
            let len = f64::from((b.x - a.x).hypot(b.z - a.z));
            let mut at = 0.0;
            while len - at >= left {
                at += left;
                let f = at / len;
                let radius = r[0] + (r[1] - r[0]) * f;
                dabs.push((a.x + (b.x - a.x) * f as f32, a.z + (b.z - a.z) * f as f32, radius, a.y + (b.y - a.y) * f as f32));
                left = spacing(radius);
            }

            left -= len - at;
        }

        self.end_stroke();
        for &(x, z, radius, y) in &dabs {
            let mut s = BrushSettings { radius_m: radius.max(0.25) as f32, ..settings.clone() };
            if follow_height {
                s.flatten_height = y;
            }

            self.brush_step(kind, x, z, s, 0.05);
        }

        self.end_stroke();
        Ok(dabs.len())
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

        // Or marked by what wrote to the maps outside the history (the roads' terrain, `take_road_terrain`; user 2026-11-08: recomputed
        // here from the history alone, the mark was lost and the height map never saved after the roads were made again).
        let height_dirty = t.project.height_dirty || t.height_modified() || t.png_on_disk_edited || (elsewhere && t.project.has_height());
        // A splice of attribute edits alone is baked into the scene's text without counting as a modification of it: the history says.
        let force_scene = t.save_scene_needed() || elsewhere || self.history.modified();
        let blend_dirty = t.project.blend_dirty || t.blend_modified() || t.blend_on_disk_edited;
        let ground_dirty = t.project.ground_dirty || t.ground_modified() || t.ground_on_disk_edited;
        let t = self.tile_mut();
        t.project.height_dirty = height_dirty;
        t.project.blend_dirty = blend_dirty;
        t.project.ground_dirty = ground_dirty;
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
            t.blend_on_disk_edited = t.blend_modified();
            t.saved_blend_depth = t.project.blend.as_ref().map_or(0, |b| b.edit_depth());
            t.project.blend_dirty = false;
            t.ground_on_disk_edited = t.ground_modified();
            t.saved_ground_depth = t.project.ground.as_ref().map_or(0, |g| g.edit_depth());
            t.project.ground_dirty = false;
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
            ins.hint("The element's name, as the Scene panel lists it. Applied when the typing ends.");
            if ins.edit_string("name", "name", &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
                && v != before
            {
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

        let sc = self.tile().tscene();
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

        // A reference's ground has its own rows, above.
        shown.extend([
            super::prefab_ground::TERRAIN_ATTR,
            super::prefab_ground::TERRAIN_OPACITY_ATTR,
            super::prefab_ground::TERRAIN_FEATHER_ATTR,
        ]);

        // A stage's group: the Prefab tool's rows (its ground's settings), below.
        shown.extend([wce_tile::prefab_stage::STAGE_ATTR, wce_tile::prefab_stage::NORMALIZE_ATTR, wce_tile::prefab_stage::FEATHER_ATTR]);

        // A reference's own are the tool's rows, below.
        if matches!(tag.as_str(), super::REF_HUMAN | super::REF_UNIT | super::REF_SHIP) {
            shown.extend(wce_tile::new_instances::REF_TOOL_ATTRS);
        }

        if wce_tile::new_instances::MEASURE_KINDS.contains(&tag.as_str()) {
            shown.extend(wce_tile::new_instances::MEASURE_HIDDEN_ATTRS);
        }

        for prop in schema {
            shown.push(prop.attr);
            if let Some(twin) = prop.twin {
                shown.push(twin);
            }

            // A capture location's boundary: its preset shape, size and turn in place of TEd's shape and radius rows.
            if tag == "CaptureLocationPolygonInstance" && matches!(prop.attr, "polygon_shape" | "radius") {
                if prop.attr == "polygon_shape" {
                    self.boundary_shape_rows(e, ins);
                }

                continue;
            }

            self.property_row(e, prop, ins);
            if prop.attr == "burn" && self.tile().tscene().type_of(e) == "BUILDING_INSTANCE" {
                self.fire_controls(e, ins);
            }
        }

        // A kit wall: made into the buildings of its pieces (kit_walls.rs).
        if tag == super::WALL_SPLINE && !wce_walls::scene::is_procedural(self.tile().tscene(), e) {
            let can = self.can_convert_wall(e);
            ins.hint(&match &can {
                Ok(n) => format!("Makes the wall's {n} pieces buildings of their own where they are (one undo). The wall is kept, hidden and not exported, \
                    in a layer of its own inside its layer, and its ground becomes a height layer of yours"),
                Err(why) => format!("Not now: {why}"),
            });
            if ins.button("convert_to_buildings", "Convert to buildings", can.is_ok()) {
                match self.convert_wall_to_buildings(e) {
                    Ok(msg) => {
                        wce_foundation::log::info(&msg);
                        self.tile_mut().last_message = msg;
                    }

                    Err(why) => wce_foundation::log::warn(format!("Convert to buildings: {why}")),
                }

                return;
            }
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

    /// A building's fire preview (fire.rs): where the sequence is, and playing or stopping it. Not an edit: the file keeps its values.
    /// The burning sequence's preview, on the `burn` row: play, stop and where it is.
    fn fire_controls(&mut self, e: usize, ins: &mut dyn Inspector) {
        let time = self.tile().fire_time(e);
        let state = match time {
            Some(t) => {
                let (amount, stage) = super::fire::fire_at(t);
                let what = if stage == 1 {
                    "flames"
                } else if t < super::fire::STAGE_ONE_SECONDS + super::fire::STAGE_TWO_SECONDS {
                    "collapse"
                } else {
                    "burnt"
                };
                format!("{t:.0} s: {what} {amount:.2}")
            }

            None => "not burning".to_owned(),
        };

        // Lucide's play and square (`wce_editor::icons`).
        match ins.row_buttons(
            &[("fire_play", "\u{e13c}", "Play the fire", true), ("fire_stop", "\u{e167}", "Stop the fire", time.is_some())],
            &state,
        ) {
            Some(0) => {
                let now = self.tile().fire_now();
                self.tile_mut().fire_start(e, now);
            }

            Some(_) => self.tile_mut().fire_stop(Some(e)),
            None => {}
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

        let mut p = self.tile().piece_props_or_building(e, k);
        let before = p.clone();

        let mut flags = [p.visible, p.active, p.draggable];
        ins.hint("Visible: the piece is drawn. Active and draggable are only kept, as in TEd.");
        if ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut flags) {
            (p.visible, p.active, p.draggable) = (flags[0], flags[1], flags[2]);
        }

        let modes = ["Landscape: Above", "Freeform"].map(str::to_owned);
        let mut idx = i32::from(p.freeform);
        ins.hint("Only kept: it changes nothing in the viewport, as in TEd.");
        if ins.edit_choice("surface_mode", "surface_mode", &mut idx, &modes) {
            p.freeform = idx == 1;
        }

        ins.hint("The piece's position in its building.");
        ins.edit_float3("position", "position", &mut p.position, 0.05);
        let mut deg = p.euler.map(f32::to_degrees);
        ins.hint("The piece's turn in its building, in degrees.");
        if ins.edit_float3("orientation", "orientation", &mut deg, 0.5) {
            p.euler = deg.map(f32::to_radians);
        }

        ins.hint("The piece's scale along x, y and z.");
        ins.edit_float3("scale", "scale", &mut p.scale, 0.01);
        ins.hint("How opaque the piece is drawn (0 to 1).");
        ins.edit_slider("opacity", "opacity", &mut p.opacity, 0.0, 1.0);
        ins.edit_bool("burn", "burn", &mut p.burn);
        ins.edit_slider("burn_amount", "burn_amount", &mut p.burn_amount, 0.0, 1.0);
        ins.edit_color4("bounds_colour", "bounds_colour", &mut p.bounds_colour);
        ins.hint("Draws the building's collision, even when the collision view is off.");
        ins.edit_bool("show_collision", "show_collision", &mut p.show_collision);
        ins.hint("Only kept: it changes nothing in the viewport, as in TEd.");
        ins.edit_checks("seasons", "seasons", &["Winter", "Spring", "Summer", "Autumn"], &mut p.seasons);
        let mut level = p.destruction_level.clamp(0, levels - 1);
        ins.hint("Which destruction level's model the piece shows.");
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

    /// The position, in the Inspector's coordinate system (`Inspector::local_coordinates`, the toolbar's Global / Local): relative to the
    /// parent as the file has it (scene units), or in the world in metres (TEd's X / Y / Z fields).
    fn position_rows(&mut self, e: usize, ins: &mut dyn Inspector, base: u64) {
        let pos = self.tile().tscene().position(e);
        if ins.local_coordinates() {
            let Some(p) = pos else { return };
            let mut v = p.to_array().map(|c| c as f32);
            ins.hint("Relative to its parent, in the file's scene units. The toolbar's Global / Local (L) switches.");
            if ins.edit_float3("position", "position (local)", &mut v, 0.5) {
                self.set_local(e, Some(v.map(f64::from).into()), None, base);
            }
        } else {
            let t = self.tile();
            if pos.is_none() && !t.is_instance(e) {
                return;
            }

            let w = if t.is_instance(e) { t.world_position(e) } else { scene_metres(world_transform(t.tscene(), e).position, t.unit()) };
            let mut v = [w.x, w.y, w.z];
            ins.hint("In the world, in metres. The toolbar's Global / Local (L) switches.");
            if ins.edit_float3("position_world", "position (world, m)", &mut v, 0.05) {
                let p = Float3::new(v[0], v[1], v[2]);
                if self.tile().is_instance(e) {
                    self.set_world_metres(e, p, base + 2);
                } else {
                    // A layer or a group: its local position for that world one.
                    let pw = parent_transform(self.tile().tscene(), e);
                    let lp = pw.rotation.transpose() * (scene_point(p, self.tile().unit()) - pw.position);
                    self.set_local(e, Some(lp), None, base + 2);
                }
            }
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// The orientation (degrees), in the Inspector's coordinate system: relative to the parent as the file has it, or in the world.
    fn orientation_row(&mut self, e: usize, ins: &mut dyn Inspector, base: u64, editable: bool) {
        let Some(local) = self.tile().tscene().euler(e) else { return };
        let world = !ins.local_coordinates();
        let pw = parent_transform(self.tile().tscene(), e);
        let shown = if world { euler_from_matrix(&(pw.rotation * euler_matrix(local))) } else { local };
        let (field, label) = if world { ("euler_world", "orientation (world)") } else { ("euler", "orientation (local)") };
        let mut v = shown.to_array().map(degrees);
        if !editable {
            ins.text(field, label, &format!("{:.1}°  {:.1}°  {:.1}°", v[0], v[1], v[2]));
            return;
        }

        ins.hint(if world {
            "Its turn in the world, in degrees. The toolbar's Global / Local (L) switches."
        } else {
            "Its turn relative to its parent, in degrees. The toolbar's Global / Local (L) switches."
        });
        if ins.edit_float3(field, label, &mut v, 0.5) {
            let typed: Double3 = v.map(radians).into();
            let euler = if world { euler_from_matrix(&(pw.rotation.transpose() * euler_matrix(typed))) } else { typed };
            self.set_local(e, None, Some(euler), base + 1);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
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
        let capture_location = wce_tile::export_as_prop::capture_location_set(sc.doc().get(e, "capture_location").as_deref());
        let ctx = RuleContext {
            category: category.as_deref(),
            shape: shape.as_deref(),
            present: text.is_some(),
            capture_location,
            export_as_prop: t.exports_as_prop(e),
        };
        let access = wce_tile::properties::access(prop, &ctx);
        if access == Access::Hidden {
            return;
        }

        // What a wall's row does (its tooltip).
        if tag == super::WALL_SPLINE
            && let Some(tip) = wce_tile::properties::wall_tip(prop.attr)
        {
            ins.hint(tip);
        }

        // A property the instance lacks is not in the file (TEd writes only some of them when they have their default), but every instance
        // has all of them in the grid.
        let cur = text.clone().unwrap_or_default();
        let editable = access == Access::Edit;
        let label = prop.label();
        let field = prop.attr;

        // The ones that have their own handling.
        match prop.kind {
            Kind::Flags => {
                let (mut vis, mut act, mut drag) = (
                    self.tile().own_flag(e, "OIF_VISIBLE"),
                    self.tile().own_flag(e, "OIF_ACTIVE"),
                    self.tile().own_flag(e, "OIF_DRAGGABLE"),
                );
                if self.tile().tscene().doc().get(e, "flags").is_none() {
                    return;
                }

                let (shown, selectable) = (self.tile().shown(e), self.tile().selectable(e));
                let mut on = [vis, act, drag];
                if editable {
                    ins.hint(FLAGS_TIP);
                }

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
                    let tips = SURFACE_TIPS.map(str::to_owned);
                    if ins.edit_choice_tips("surface_mode", "surface_mode", &mut idx, &modes, &tips) && editable && idx != before {
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
                    let tips = ["Turned as given.", "Tilted to the surface under it, now and again after each grab."].map(str::to_owned);
                    if ins.edit_choice_tips("rotation_mode", "rotation", &mut r, &rotations, &tips) && editable && (r == 1) != follow {
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
                    if t.on_the_terrain(e) {
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
                self.orientation_row(e, ins, base, editable);
                return;
            }

            _ => {}
        }

        if prop.rule == wce_tile::properties::Rule::NoCaptureLocation && !editable {
            greyed_export_as_prop(prop, ins);
            return;
        }

        if prop.rule == wce_tile::properties::Rule::WhenExportedAsProp && !editable {
            ins.hint(wce_tile::export_as_prop::SCALE_TIP);
            ins.disabled_float3(field, label, parse_vec3_text(&cur).unwrap_or([1.0; 3]));
            return;
        }

        if !editable {
            ins.text(field, label, &cur);
            return;
        }

        let gesture = base ^ (attr_hash(prop.attr) << 4) ^ 0x7;
        let prop = &self.tile().game_choices(prop);
        let mut new = edit_value(prop, &cur, text.is_some(), i64::from(self.tile().destruction_levels(&key)), ins);
        if prop.kind == Kind::Text {
            // Typed text is applied when the typing ends (a key edit loads a model).
            let drafts_key = (e, prop.attr.to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| cur.clone());
            if ins.edit_string(field, label, &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
            {
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

    /// Whether the Inspector shows a property of an element, and edits it (the rules that need its building's category or polygon's shape).
    fn prop_access(&self, e: usize, prop: &Prop) -> Access {
        let t = self.tile();
        let doc = t.tscene().doc();
        let key = doc.get(e, "key").unwrap_or_default();
        let category = if t.tscene().type_of(e) == "BUILDING_INSTANCE" { t.building_category(&key) } else { None };
        let shape = doc.get(e, "polygon_shape");
        let capture_location = wce_tile::export_as_prop::capture_location_set(doc.get(e, "capture_location").as_deref());
        let ctx = RuleContext {
            category: category.as_deref(),
            shape: shape.as_deref(),
            present: doc.get(e, prop.attr).is_some(),
            capture_location,
            export_as_prop: t.exports_as_prop(e),
        };
        wce_tile::properties::access(prop, &ctx)
    }

    /// The Inspector with several elements selected: the properties all of them have (the same attribute of the same kind, shown for each),
    /// but their place (position, orientation, scale) and name; a value they do not share is marked, and shows the first one's. An edit sets
    /// it on all of them, one undo entry (a drag merges). False when fewer than two of the items are elements.
    pub(crate) fn inspect_elements(&mut self, items: &[SourceRef], ins: &mut dyn Inspector) -> bool {
        let n = self.tile().tscene().doc().elements().len();
        let mut elems = items
            .iter()
            .filter(|r| r.doc == self.id() && piece_of(r.item).is_none())
            .filter_map(|r| elem_of(r.item))
            .filter(|&e| e < n)
            .collect::<Vec<_>>();
        elems.dedup();
        if elems.len() < 2 {
            return false;
        }

        // What is selected, by type.
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        for &e in &elems {
            *types.entry(self.tile().tscene().type_of(e).to_owned()).or_default() += 1;
        }

        ins.section(&format!("{} items", elems.len()));
        ins.text("types", "types", &types.iter().map(|(t, n)| format!("{t} x{n}")).collect::<Vec<_>>().join(", "));

        // The properties shown for every one of them, in the first one's order.
        let excluded = |p: &Prop| matches!(p.kind, Kind::Position | Kind::Orientation) || p.attr == "scale" || p.attr == "name";
        let mut common: Option<Vec<(Prop, bool)>> = None;
        for &e in &elems {
            let Some(schema) = wce_tile::properties::schema(self.tile().tscene().type_of(e)) else {
                common = Some(vec![]);
                break;
            };

            let mine = schema
                .iter()
                .filter(|p| !excluded(p))
                .filter_map(|p| {
                    let access = self.prop_access(e, p);
                    (access != Access::Hidden).then_some((*p, access == Access::Edit))
                })
                .collect::<Vec<_>>();

            common = Some(match common {
                None => mine,
                Some(c) => c
                    .into_iter()
                    .filter_map(|(p, edit)| mine.iter().find(|(m, _)| m.attr == p.attr && m.kind == p.kind).map(|(_, e2)| (p, edit && *e2)))
                    .collect(),
            });
        }

        // Flags and placement need the attribute in every element (an added one is text the attribute edits cannot take back).
        let doc = self.tile().tscene().doc();
        let common = common
            .unwrap_or_default()
            .into_iter()
            .filter(|(p, _)| match p.kind {
                Kind::Flags => elems.iter().all(|&e| doc.get(e, "flags").is_some()),
                Kind::Placement => elems.iter().all(|&e| self.tile().is_instance(e) && doc.get(e, "surface_mode").is_some()),
                _ => true,
            })
            .collect::<Vec<_>>();

        if common.is_empty() {
            ins.text("none", "", "no property in common");
            return true;
        }

        // The values they do not share.
        let mut marks = vec![];
        for (p, _) in &common {
            let first = doc.get(elems[0], p.attr);
            if elems[1..].iter().any(|&e| doc.get(e, p.attr) != first) {
                marks.push((p.attr.to_owned(), "differs between the selected items: the first one's is shown".to_owned()));
            }
        }

        ins.section("Common properties");
        ins.mark_fields(marks);
        for (prop, editable) in common {
            self.common_row(&elems, &prop, editable, ins);
        }

        true
    }

    /// A row of [`Self::inspect_elements`].
    fn common_row(&mut self, elems: &[usize], prop: &Prop, editable: bool, ins: &mut dyn Inspector) {
        let first = elems[0];
        let base = (u64::from(self.id()) << 40) ^ 0x5151_0000 ^ elems.len() as u64;
        let text = self.tile().tscene().doc().get(first, prop.attr);
        let cur = text.clone().unwrap_or_default();
        match prop.kind {
            Kind::Flags => {
                let t = self.tile();
                let names = ["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"];
                let before = names.map(|f| t.own_flag(first, f));
                let mut on = before;
                if editable {
                    ins.hint(FLAGS_TIP);
                }

                if editable && ins.edit_checks("flags", "flags", &["visible", "active", "draggable"], &mut on) {
                    for (i, flag) in names.iter().enumerate() {
                        if on[i] != before[i] {
                            self.set_flags(elems, flag, on[i]);
                        }
                    }
                }

                return;
            }

            Kind::Placement => {
                use wce_tile::project_files::SURFACE_ATTR;
                let doc = self.tile().tscene().doc();
                let on_entity = doc.get(first, SURFACE_ATTR).is_some_and(|v| v == "entity");
                let modes = ["Landscape: Above", "Freeform", "Entity: Above"].map(str::to_owned);
                let mut idx = if on_entity { 2 } else { i32::from(self.tile().placement(first) == Placement::Freeform) };
                let tips = SURFACE_TIPS.map(str::to_owned);
                if ins.edit_choice_tips("surface_mode", "surface_mode", &mut idx, &modes, &tips) && editable {
                    self.set_placement(elems, if idx == 0 { Placement::Landscape } else { Placement::Freeform });
                    self.history.break_merge();
                    self.set_attribute_all(elems, SURFACE_ATTR, if idx == 2 { "entity" } else { "" });
                }

                return;
            }

            // Each season on its own: the others keep each element's.
            Kind::Seasons => {
                let present = text.is_some();
                let before = if present { parse_seasons(&cur) } else { [true; 4] };
                let mut on = before;
                if editable {
                    ins.hint("A tick changes only that season, on each selected item; the others keep their own.");
                }

                if editable && ins.edit_checks(prop.attr, prop.attr, &["Winter", "Spring", "Summer", "Autumn"], &mut on) {
                    let doc = self.tile().tscene().doc();
                    let values = elems
                        .iter()
                        .map(|&e| {
                            let mut s = doc.get(e, prop.attr).map_or([true; 4], |v| parse_seasons(&v));
                            for i in 0..4 {
                                if on[i] != before[i] {
                                    s[i] = on[i];
                                }
                            }

                            format_seasons(s)
                        })
                        .collect::<Vec<_>>();
                    self.set_property_each(elems, prop, &values, 0);
                } else if !editable {
                    ins.text(prop.attr, prop.attr, &cur);
                }

                return;
            }

            _ => {}
        }

        if prop.rule == wce_tile::properties::Rule::NoCaptureLocation && !editable {
            greyed_export_as_prop(prop, ins);
            return;
        }

        if !editable {
            ins.text(prop.attr, prop.attr, &cur);
            return;
        }

        let gesture = base ^ (attr_hash(prop.attr) << 4) ^ 0x9;
        let prop = &self.tile().game_choices(prop);
        let levels = elems
            .iter()
            .map(|&e| {
                let key = self.tile().tscene().doc().get(e, "key").unwrap_or_default();
                i64::from(self.tile().destruction_levels(&key))
            })
            .min()
            .unwrap_or(1);
        let mut new = edit_value(prop, &cur, text.is_some(), levels, ins);
        if prop.kind == Kind::Text {
            // Typed text is applied when the typing ends (a key edit loads models).
            let drafts_key = (usize::MAX, prop.attr.to_owned());
            let mut shown = self.tile().drafts.get(&drafts_key).cloned().unwrap_or_else(|| cur.clone());
            if ins.edit_string(prop.attr, prop.attr, &mut shown) {
                self.tile_mut().drafts.insert(drafts_key.clone(), shown);
            }

            if ins.gesture_ended()
                && let Some(v) = self.tile_mut().drafts.remove(&drafts_key)
            {
                new = Some(v);
            }
        }

        if let Some(v) = new {
            self.set_property_each(elems, prop, &vec![v; elems.len()], gesture);
        }

        if ins.gesture_ended() {
            self.history.break_merge();
        }
    }

    /// Sets a property every one of the elements has and can edit, as the Inspector does with them selected (a script); how many changed.
    ///
    /// # Errors
    ///
    /// One of them does not have the property, or it is read-only for it.
    pub fn set_common(&mut self, elems: &[usize], attr: &str, value: &str) -> Result<usize, String> {
        let first = *elems.first().ok_or("nothing selected")?;
        let prop = wce_tile::properties::schema(self.tile().tscene().type_of(first))
            .and_then(|s| s.iter().find(|p| p.attr == attr))
            .copied()
            .ok_or_else(|| format!("{} has no property {attr}", self.tile().tscene().type_of(first)))?;
        for &e in elems {
            let has = wce_tile::properties::schema(self.tile().tscene().type_of(e))
                .is_some_and(|s| s.iter().any(|p| p.attr == attr && p.kind == prop.kind));
            if !has || self.prop_access(e, &prop) != Access::Edit {
                return Err(format!("element {e} ({}) cannot edit {attr}", self.tile().tscene().type_of(e)));
            }
        }

        let doc = self.tile().tscene().doc();
        let changed = elems.iter().filter(|&&e| doc.get(e, attr).as_deref() != Some(value)).count();
        self.set_property_each(elems, &prop, &vec![value.to_owned(); elems.len()], 0);
        Ok(changed)
    }

    /// Sets an attribute of several elements, one undo entry (a text splice; empty: as [`Self::set_attribute`] does with it).
    pub fn set_attribute_all(&mut self, elems: &[usize], attr: &str, value: &str) -> bool {
        let n = self.tile().tscene().doc().elements().len();
        let todo = elems
            .iter()
            .copied()
            .filter(|&e| e < n && self.tile().tscene().doc().get(e, attr).as_deref() != Some(value))
            .collect::<Vec<_>>();
        if todo.is_empty() {
            return true;
        }

        let Ok(mark) = self.begin() else { return false };
        for &e in &todo {
            self.tscene_mut().set_attr(e, attr, value);
        }

        let items = todo.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        self.push_splice_attrs(format!("Set {attr} of {} items", todo.len()), mark, items.clone(), items, todo, &[attr]);
        true
    }

    /// Sets a property (and its twin) of several elements, each to its value: one undo entry, the steps of a drag (`gesture`) merge. Elements
    /// that lack the attribute get it first, by one text splice.
    pub fn set_property_each(&mut self, elems: &[usize], prop: &Prop, values: &[String], gesture: u64) -> bool {
        let attrs = std::iter::once(prop.attr).chain(prop.twin).collect::<Vec<_>>();
        let n = self.tile().tscene().doc().elements().len();
        let pairs = elems.iter().copied().zip(values.iter().cloned()).filter(|(e, _)| *e < n).collect::<Vec<_>>();

        // The attributes missing (an empty optional text stays out of the file, as `set_property` keeps it).
        let doc = self.tile().tscene().doc();
        let missing = pairs
            .iter()
            .flat_map(|(e, v)| {
                attrs
                    .iter()
                    .filter(move |a| doc.get(*e, a).is_none() && !(v.is_empty() && prop.kind == Kind::Text))
                    .map(move |a| (*e, *a, v.clone()))
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let Ok(mark) = self.begin() else { return false };
            for (e, a, v) in &missing {
                self.tscene_mut().set_attr(*e, a, v);
            }

            let items = missing.iter().map(|(e, _, _)| item_of(*e)).collect::<Vec<_>>();
            let mut touched = missing.iter().map(|(e, _, _)| *e).collect::<Vec<_>>();
            touched.dedup();
            self.push_splice_attrs(
                format!("Set {} of {} items", prop.attr, pairs.len()),
                mark,
                items.clone(),
                items,
                touched,
                &[prop.attr],
            );
            self.history.break_merge();
        }

        let doc = self.tile().tscene().doc();
        let edits = pairs
            .iter()
            .filter_map(|(e, v)| {
                let tag = self.tile().tscene().type_of(*e);
                let steps = attrs
                    .iter()
                    .filter_map(|a| doc.get(*e, a).filter(|old| old != v).map(|old| ((*a).to_owned(), old, v.clone())))
                    .collect::<Vec<_>>();
                (!steps.is_empty()).then(|| TedAttrCommand {
                    name: String::new(),
                    elem: *e,
                    steps,
                    gesture,
                    rebuild: tag == "BUILDING_INSTANCE" && prop.attr == "key",
                })
            })
            .collect::<Vec<_>>();

        if edits.is_empty() {
            return true;
        }

        let name = format!("Set {} of {} items", prop.attr, edits.len());
        self.history.push(Box::new(TedAttrsCommand { name, edits, gesture }));
        true
    }

    /// Moves a civilian shelter's target to a world point (TEd drags it over the ground), as the Inspector's `target` row writes it; the
    /// steps of a drag (one `gesture`) merge into one undo entry.
    pub fn set_shelter_target(&mut self, elem: usize, at: Float3, gesture: u64) -> bool {
        if self.tile().tscene().type_of(elem) != "CivilianShelter" {
            return false;
        }

        let Some(prop) = wce_tile::properties::schema("CivilianShelter").and_then(|s| s.iter().find(|p| p.attr == "target")) else {
            return false;
        };
        let pw = parent_transform(self.tile().tscene(), elem);
        let v = pw.rotation.transpose() * (scene_point(at, self.tile().unit()) - pw.position);
        self.set_property(elem, prop, &wce_tile::properties::format_vec3([v.x as f32, v.y as f32, v.z as f32]), gesture)
    }

    /// Sets a point light's radius as the Inspector's slider does (clamped to its range, `[0, 1000]`); the steps of a drag (one `gesture`)
    /// merge into one undo entry. False for anything but a point light.
    pub fn set_light_radius(&mut self, elem: usize, radius: f32, gesture: u64) -> bool {
        if self.tile().tscene().type_of(elem) != "POINT_LIGHT_INSTANCE" {
            return false;
        }

        let Some(prop) = wce_tile::properties::schema("POINT_LIGHT_INSTANCE").and_then(|s| s.iter().find(|p| p.attr == "radius")) else {
            return false;
        };
        let (lo, hi) = match prop.kind {
            Kind::Slider(lo, hi) => (lo, hi),
            _ => (0.0, f32::MAX),
        };

        // A hundredth of a metre is as fine as a drag goes.
        let r = ((radius.clamp(lo, hi) * 100.0).round() / 100.0).clamp(lo, hi);
        self.set_property(elem, prop, &wce_tile::properties::format_float(r), gesture)
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
                let range = format!(
                    "{:.2} .. {:.2} m (min {:.3} scale {:.3}){found}",
                    h.min_height(),
                    h.min_height() + h.scale(),
                    p.info.min_height,
                    p.info.scale
                );
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
    /// The `scale` property of an element the Scale gizmo works on: every type that has one, while the Inspector can edit it (a building only
    /// as a prop).
    fn scale_prop(&self, e: usize) -> Option<Prop> {
        let schema = wce_tile::properties::schema(self.tile().tscene().type_of(e))?;
        let prop = *schema.iter().find(|p| p.attr == "scale")?;
        (self.prop_access(e, &prop) == Access::Edit).then_some(prop)
    }

    /// The Scale gizmo on the selected items that have a scale, one undo entry: each from the scale it had when the drag began (`factor` is
    /// the scale since then). A vector takes the axes (a decal's negative z stays negative); a single number takes the axis that moved most,
    /// within its range.
    pub(crate) fn scale_selected(&mut self, items: &[SourceRef], factor: [f32; 3], gesture: u64) {
        if self.tile().scale_drag.as_ref().is_none_or(|(g, _)| *g != gesture) {
            let t = self.tile();
            let base = items
                .iter()
                .filter(|r| r.doc == self.id() && piece_of(r.item).is_none())
                .filter_map(|r| elem_of(r.item))
                .filter(|&e| self.scale_prop(e).is_some())
                .map(|e| {
                    let s = t.tscene().scale(e).map_or([1.0; 3], |s| [s.x as f32, s.y as f32, s.z as f32]);
                    (e, s)
                })
                .collect();
            self.tile_mut().scale_drag = Some((gesture, base));
        }

        let Some((_, base)) = self.tile().scale_drag.clone() else { return };
        let uniform = factor.into_iter().max_by(|a, b| (a - 1.0).abs().total_cmp(&(b - 1.0).abs())).unwrap_or(1.0);
        let scaled = base
            .iter()
            .filter_map(|(&e, s)| {
                let prop = self.scale_prop(e)?;
                let value = match prop.kind {
                    Kind::Vec3 => format_vec3([s[0] * factor[0], s[1] * factor[1], s[2] * factor[2]]),
                    Kind::Slider(lo, hi) => format_float((s[0] * uniform).clamp(lo, hi)),
                    _ => format_float(s[0] * uniform),
                };

                Some((e, prop, value))
            })
            .collect::<Vec<_>>();

        // One attribute: the types' props differ only in how the value is written, done above.
        let Some(&(_, prop, _)) = scaled.first() else { return };
        let elems = scaled.iter().map(|s| s.0).collect::<Vec<_>>();
        let values = scaled.into_iter().map(|s| s.2).collect::<Vec<_>>();
        self.set_property_each(&elems, &prop, &values, gesture);
    }

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
