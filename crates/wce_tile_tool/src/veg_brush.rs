//! The vegetation brush: while a tree is being placed, Brush (a chip beside the placement ones) paints them instead of placing one per click.
//! A dab tries points at random in the circle and keeps one when it is at least `spacing` from every tree (the tile's and the stroke's) and
//! `clearance` from the other things' footprints (their models' bounds); `fill` says how many it tries for the room there is. The stroke's
//! trees are previewed while it lasts and put into the working layer at its end, one undo entry. Ctrl + drag takes the manual trees in the
//! circle away (one entry too). The settings are in the Inspector while the brush is on, kept in the Tile settings.

use std::collections::BTreeSet;

use wce_editor::editor::Editor;
use wce_editor_core::inspector::Inspector;
use wce_foundation::math::Float3;
use wce_tile_editor::tile_document::brush::Footprint;
use wce_tile_editor::tile_document::{turned, with_root_attr};

use crate::settings::{SETTINGS_ID, TileSettings, VegBrush};
use crate::tool::{TileTool, document};

/// A painted tree: where (world metres), its key, scale and turn (degrees).
#[derive(Clone, Debug, PartialEq)]
pub struct Planted {
    pub at: Float3,
    pub key: String,
    pub scale: f32,
    pub yaw: f32,
}

/// One stroke of the brush.
#[derive(Default)]
pub struct Stroke {
    pub erase: bool,
    pub planted: Vec<Planted>,
    pub erased: BTreeSet<usize>,

    /// The tile's footprints when the stroke began.
    pub footprints: Vec<Footprint>,
    last: Option<Float3>,
}

/// A small random sequence (xorshift), seeded from the clock.
pub struct Random(u64);

impl Random {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9e37_79b9, |d| d.as_nanos() as u64);
        Self(seed | 1)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

impl TileTool {
    /// The brush's settings, read from the settings the first time.
    pub(crate) fn veg_brush(&mut self, e: &Editor) -> VegBrush {
        *self.veg.get_or_insert_with(|| e.tool_settings::<TileSettings>(SETTINGS_ID).veg_brush)
    }

    pub(crate) fn set_veg_brush(&mut self, e: &mut Editor, b: VegBrush) {
        self.veg = Some(b);
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.veg_brush = b);
    }

    /// Whether a tree is being placed with the brush on.
    pub(crate) fn painting_trees(&mut self, e: &Editor) -> bool {
        self.placing == Some(crate::tool::Placing::Draft) && self.draft.as_ref().is_some_and(|d| d.kind == "VEGETATION_INSTANCE") && self.veg_brush(e).on
    }

    /// A stroke begins at the mouse (`erase`: Ctrl held).
    pub(crate) fn begin_stroke(&mut self, e: &mut Editor, erase: bool) {
        let Some(d) = document(e) else { return };
        self.stroke = Some(Stroke { erase, footprints: d.footprints(), ..Default::default() });
        self.dab(e);
    }

    /// A dab at the mouse, when it moved far enough from the last one (a quarter of the radius).
    pub(crate) fn dab(&mut self, e: &mut Editor) {
        let b = self.veg_brush(e);
        let at = self.cursor;
        let set = self.set_in_use(e);
        let Some(stroke) = self.stroke.as_mut() else { return };
        if stroke.last.is_some_and(|l| (l.x - at.x).hypot(l.z - at.z) < b.radius * 0.25) {
            return;
        }

        stroke.last = Some(at);
        let r2 = b.radius * b.radius;
        if stroke.erase {
            for f in stroke.footprints.iter().filter(|f| f.tree && (f.x - at.x).powi(2) + (f.z - at.z).powi(2) <= r2) {
                stroke.erased.insert(f.elem);
            }

            stroke.planted.retain(|p| (p.at.x - at.x).powi(2) + (p.at.z - at.z).powi(2) > r2);
            return;
        }

        let Some(d) = document(e) else { return };
        let key = self.draft.as_ref().map(|d| d.key.clone()).unwrap_or_default();
        if set.as_ref().is_some_and(|s| s.entries.is_empty()) {
            return;
        }

        let spacing = b.spacing.max(0.25);

        // As many tries as trees would fit at that spacing, times the fill.
        let tries = ((std::f32::consts::PI * r2 / (spacing * spacing)) * b.fill.clamp(0.0, 1.0)).ceil().min(256.0) as usize;
        let near = stroke.footprints.iter().filter(|f| (f.x - at.x).hypot(f.z - at.z) <= b.radius + f.radius + b.clearance.max(spacing)).copied().collect::<Vec<_>>();
        for _ in 0..tries {
            let (a, r) = (self.random.unit() * std::f32::consts::TAU, b.radius * self.random.unit().sqrt());
            let (x, z) = (at.x + a.cos() * r, at.z + a.sin() * r);
            let free = near.iter().all(|f| {
                let dist = (f.x - x).hypot(f.z - z);
                if f.tree { dist >= spacing } else { dist >= f.radius + b.clearance }
            }) && stroke.planted.iter().all(|p| (p.at.x - x).hypot(p.at.z - z) >= spacing);
            if !free {
                continue;
            }

            // A set's tree is picked by the weights, with its own scale range if it has one.
            let picked = set.as_ref().and_then(|s| s.pick(self.random.unit()));
            let key = picked.map_or_else(|| key.clone(), |en| en.key.clone());
            let (lo, hi) = picked.and_then(|en| en.scale).unwrap_or((b.scale_min, b.scale_max));
            let scale = lo + (hi - lo).max(0.0) * self.random.unit();
            let yaw = if b.random_yaw { self.random.unit() * 360.0 } else { 0.0 };
            stroke.planted.push(Planted { at: Float3::new(x, d.tile().terrain_height(x, z), z), key, scale, yaw });
        }
    }

    /// The text of a painted tree (at the origin): the draft's tree with its key, scale and turn.
    fn planted_text(&self, d: &wce_tile_editor::tile_document::TileDocument, p: &Planted) -> Option<Vec<u8>> {
        let mut draft = self.draft.clone()?;
        draft.key.clone_from(&p.key);
        draft.yaw = 0.0;
        let text = d.draft_text(&draft)?;
        let text = with_root_attr(&text, "scale", &wce_tile::ted_scene::format_number(f64::from(p.scale)));
        Some(turned(&self.apply_placement_modes(d, text), p.yaw))
    }

    /// The stroke ends: its trees go into the working layer (one undo entry), or the trees it took away go.
    pub(crate) fn end_stroke_veg(&mut self, e: &mut Editor) -> String {
        let Some(stroke) = self.stroke.take() else { return String::new() };
        let Some(d) = document(e) else { return String::new() };
        if stroke.erase {
            let elems = stroke.erased.into_iter().collect::<Vec<_>>();
            let n = elems.len();
            if n != 0 {
                d.remove_items(&elems);

                // What was selected may be gone, and the rest moved: nothing is.
                e.selection.clear();
            }

            return format!("{n} tree(s) taken away");
        }

        let items = stroke.planted.iter().filter_map(|p| self.planted_text(d, p).map(|t| (t, p.at))).collect::<Vec<_>>();
        let Some(into) = self.new_items_layer(d) else { return "this tile has no scene graph".to_owned() };
        match d.add_painted(&format!("Paint {} tree(s)", items.len()), into, &items) {
            Ok(added) => format!("{} tree(s) painted", added.len()),
            Err(err) => err,
        }
    }

    /// The preview of the stroke's trees: one group of them around the origin (the preview puts landscape ones on the terrain).
    pub(crate) fn stroke_preview(&self, e: &mut Editor) -> Option<Vec<u8>> {
        let stroke = self.stroke.as_ref()?;
        let d = document(e)?;
        let unit = d.tile().unit();
        let mut out = b"<UserInstanceGroup name=\"stroke\" position=\"vector(0,0,0)\" orientation=\"euler(0,0,0)\">\n    <children>\n".to_vec();
        for p in &stroke.planted {
            let Some(text) = self.planted_text(d, p) else { continue };
            let pos = format!("vector({},{},{})", f64::from(p.at.x) * unit, 0, f64::from(p.at.z) * unit);
            let text = with_root_attr(&text, "position", &pos);
            for line in text.split_inclusive(|&c| c == b'\n') {
                out.extend_from_slice(b"        ");
                out.extend_from_slice(line);
            }
        }

        out.extend_from_slice(b"    </children>\n</UserInstanceGroup>\n");
        Some(out)
    }

    /// The trees the erasing stroke takes, for the overlay.
    pub(crate) fn erased_points(&self, d: &wce_tile_editor::tile_document::TileDocument) -> Vec<Float3> {
        let Some(s) = self.stroke.as_ref().filter(|s| s.erase) else { return vec![] };
        s.erased.iter().map(|&el| d.tile().world_position(el)).collect()
    }

    /// The brush's settings in the Inspector (while it is on).
    pub(crate) fn inspect_veg_brush(&mut self, e: &mut Editor, ins: &mut dyn Inspector) {
        let mut b = self.veg_brush(e);
        let before = b;
        ins.section("Vegetation brush (Ctrl + drag takes trees away)");
        ins.edit_slider("veg_radius", "radius (m)", &mut b.radius, 1.0, 100.0);
        ins.edit_slider("veg_spacing", "spacing (m)", &mut b.spacing, 0.5, 40.0);
        ins.edit_slider("veg_clearance", "clearance (m)", &mut b.clearance, 0.0, 20.0);
        ins.edit_slider("veg_fill", "fill", &mut b.fill, 0.0, 1.0);
        ins.edit_slider("veg_scale_min", "scale from", &mut b.scale_min, 0.1, 3.0);
        ins.edit_slider("veg_scale_max", "scale to", &mut b.scale_max, 0.1, 3.0);
        ins.edit_bool("veg_yaw", "random turn", &mut b.random_yaw);
        if b != before {
            self.set_veg_brush(e, b);
        }
    }
}
