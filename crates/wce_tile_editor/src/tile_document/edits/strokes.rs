//! Height strokes: the terrain brushes' strokes as undoable edits.

use super::*;

impl TileDocument {
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

        if !self.tile().stroke_open {
            match kind {
                BrushKind::Paint(_) => self.paint_over_texture_rules(super::super::layers::LayerType::Blend),
                BrushKind::Ground(_) => self.paint_over_texture_rules(super::super::layers::LayerType::Ground),
                _ => {}
            }
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
            before_top: 0,
            recorded: None,
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
        self.history.push(Box::new(super::super::commands::LinksCommand {
            name: name.to_owned(),
            before,
            after: (targets, links),
            gesture,
        }));
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
        };
        if before.as_deref().unwrap_or_default() == value {
            return Ok(false);
        }

        self.end_stroke();
        self.history.push(Box::new(super::super::commands::EntryAttrCommand {
            variation: v,
            attr: attr.to_owned(),
            before,
            after: value.to_owned(),
            gesture,
            strength_before: None,
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
        self.tile_mut().project.load_blend()?;
        self.paint_over_texture_rules(super::super::layers::LayerType::Blend);
        let t = self.tile_mut();
        let depth = t.project.blend.as_ref().map_or(0, wce_tile::blend_field::BlendField::edit_depth);
        t.paint_blend(usize::from(channel), None, settings, f64::from(settings.opacity).clamp(0.0, 1.0));
        t.end_stroke_edit();
        if t.project.blend.as_ref().map_or(0, wce_tile::blend_field::BlendField::edit_depth) == depth {
            return Ok(false);
        }

        self.history.push(Box::new(super::super::commands::BlendFillCommand { name: "Paint the whole tile".to_owned(), first: true }));
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

    pub(super) fn stroke_along_with(
        &mut self,
        kind: BrushKind,
        elem: usize,
        settings: &BrushSettings,
        follow_height: bool,
    ) -> Result<usize, String> {
        const STEPS: usize = 16;
        let t = self.tile();
        if !t.is_polygon(elem) {
            return Err("select a spline, polygon or line to stroke along".to_owned());
        }

        let spline = self.is_spline(elem);
        let sc = t.tscene();
        let closed =
            if spline { sc.get(elem, "closed").is_some_and(|v| v == "1") } else { sc.get(elem, "polygon_closed").is_none_or(|v| v != "0") };
        let mut path = if spline { t.spline_world_curve(elem) } else { t.polygon_world_points(elem) };

        // The points' widths (metres), when they have them, at every sample of the curve.
        let points = sc.polygon_points(elem);
        let widths = points.iter().map(|&p| sc.get(p, "width").and_then(|v| wce_formats::c_num::strtod(&v))).collect::<Vec<_>>();
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

        let dabs = dabs_along(&path, &radii);
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

    /// A straight stroke of the brush from `(x0, z0)` to `(x1, z1)` (metres), as a press there dragged straight to here would paint it
    /// (Shift + click with a brush, as in Substance Painter): a dab every quarter radius, each with the most a frame gives, one undo entry;
    /// returns the dabs (the first at the start, the last at the end). The brush's own radius all along.
    pub fn stroke_line(&mut self, kind: BrushKind, (x0, z0): (f32, f32), (x1, z1): (f32, f32), settings: &BrushSettings) -> usize {
        let path = [Float3::new(x0, 0.0, z0), Float3::new(x1, 0.0, z1)];
        let r = f64::from(settings.radius_m);
        let mut dabs = dabs_along(&path, &[r, r]);
        // Up to the click itself.
        if dabs.last().is_some_and(|&(x, z, _, _)| (x - x1).hypot(z - z1) > 1e-3) {
            dabs.push((x1, z1, r, 0.0));
        }

        self.end_stroke();
        for &(x, z, _, _) in &dabs {
            self.brush_step(kind, x, z, settings.clone(), 0.05);
        }

        self.end_stroke();
        dabs.len()
    }

    pub fn end_stroke(&mut self) {
        if !self.tile().stroke_open {
            return;
        }

        self.tile_mut().end_stroke_edit();
        self.history.break_merge();
    }
}

/// The dabs of a stroke along `path` (its radius at each point, `radii`): the first point, then one every quarter radius (x, z, radius,
/// the path's height there).
fn dabs_along(path: &[Float3], radii: &[f64]) -> Vec<(f32, f32, f64, f32)> {
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

    dabs
}
