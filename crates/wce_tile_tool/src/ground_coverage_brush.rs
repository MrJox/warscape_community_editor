//! The ground coverage's brush (the window's Brush mode, `ground_coverage_panel.rs`): a left drag puts the kind's rocks and plants on its
//! terrain under the circle (`ground_coverage::scatter`: up to the preset's densities, counting what is there), Ctrl + drag takes the kind's
//! coverage away (filled or brushed). A stroke goes into the kind's `Brushed` group as `Stroke N`, with the exclusion polygons around its
//! plants, at its end (one undo entry); a taking away redoes the polygons of the groups it touched.
//!
//! What a dab needs of the tile (the grid, the water and the no-go field, the buildings, the rocks' sizes and the kind's bands over the whole
//! tile) is worked out at a stroke's start when the tile or the values changed since (about a second on a big tile), and kept: the brush's
//! own edits don't make it stale.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{DocumentId, Overlay, overlay_colour};
use wce_tile::ground_coverage::{Band, Blocker, Classified, Context, Grid, Input, Kind, Params, Placed, RockSize, classify, exclusions, scatter};
use wce_tile::vegetation_sets::VegetationSet;
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{TileMode, TileTool, document, document_id};

/// The brush's radius and its densities' factor at first.
pub const DEFAULT_RADIUS: f32 = 15.0;

/// What a dab needs of the tile, for the values it was worked out with.
pub(crate) struct BrushBase {
    key: BaseKey,
    kind: Kind,
    grid: Grid,
    context: Arc<Context>,
    blockers: Vec<Blocker>,
    rocks: VegetationSet,
    plants: VegetationSet,
    sizes: HashMap<String, RockSize>,
    params: Params,
    classified: Classified,
}

/// The tile and its version, the kind, the parameters' text and the sets' names a base was worked out for.
type BaseKey = (DocumentId, u64, Kind, String, String, String);

impl BrushBase {
    /// The generator's input over the base (a dab's seed is its own).
    fn input<'a>(&'a self, size: &'a dyn Fn(&str) -> RockSize) -> Input<'a> {
        Input {
            kind: self.kind,
            grid: &self.grid,
            context: Some(&self.context),
            blockers: &self.blockers,
            keep_off: &[],
            rocks: &self.rocks,
            plants: &self.plants,
            rock_size: size,
            params: self.params,
            seed: 0,
        }
    }

    fn size(&self, key: &str) -> RockSize {
        self.sizes.get(key).copied().unwrap_or(crate::ground_coverage_panel::UNKNOWN_ROCK)
    }
}

/// A stroke: painting or taking away, where the last dab was, what it put (painting) or takes (taking away), and what was there.
#[derive(Default)]
pub(crate) struct CoverageStroke {
    erase: bool,
    last: Option<(f32, f32)>,
    rocks: Vec<Placed>,
    plants: Vec<Placed>,
    erased: HashSet<usize>,

    /// The kind's coverage at the start: its rocks and plants as footprints (the element, where, its radius, whether a plant).
    there: Vec<wce_tile_editor::tile_document::brush::Footprint>,
}

impl TileTool {
    /// Whether a left drag in the viewport is the coverage brush's: the window is open in Brush mode and nothing else is being placed or
    /// drawn (Select mode).
    pub(crate) fn coverage_brushing(&self, e: &Editor) -> bool {
        self.coverage.open && self.coverage.brush && self.mode == TileMode::Select && self.placing.is_none() && !self.drawing && document_id(e).is_some()
    }

    /// The brush's radius (metres).
    pub(crate) fn coverage_radius(&self) -> f32 {
        if self.coverage.brush_radius > 0.0 { self.coverage.brush_radius } else { DEFAULT_RADIUS }
    }

    /// The base for the window's values, worked out again when the tile or they changed.
    fn brush_base(&mut self, e: &mut Editor) -> Result<(), String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let version = document(e).ok_or("no tile is open")?.history().version();
        let c = &self.coverage;
        let key = (id, version, c.kind, c.params.to_text(), c.rocks.clone(), c.plants.clone());
        if self.coverage.base.as_ref().is_some_and(|b| b.key == key) {
            return Ok(());
        }

        let (rocks, plants) = self.coverage_sets(e)?;
        let categories = self.building_categories(e);
        let context = self.coverage_context(e)?;
        let d = document(e).ok_or("no tile is open")?;
        let started = std::time::Instant::now();
        let (grid, blockers, sizes, _) = crate::ground_coverage_panel::tile_base(d, &rocks, &categories);
        let mut base = BrushBase { key, kind: self.coverage.kind, grid, context, blockers, rocks, plants, sizes, params: self.coverage.params, classified: Classified::default() };
        let size = |k: &str| base.size(k);
        let classified = classify(&base.input(&size));
        base.classified = classified;
        wce_foundation::log::info(format!("ground coverage: the brush's bands worked out in {:.2} s", started.elapsed().as_secs_f64()));
        self.coverage.base = Some(Box::new(base));
        Ok(())
    }

    /// The tile's version moved from `before` by the coverage's own edit: what was worked out for `before` holds for it too (the coverage
    /// is no building the coverage goes around, and the terrain did not change).
    pub(crate) fn adopt_coverage_version(&mut self, id: DocumentId, before: u64, after: u64) {
        if let Some(b) = self.coverage.base.as_mut().filter(|b| b.key.0 == id && b.key.1 == before) {
            b.key.1 = after;
        }

        if let Some(((doc, v), _)) = self.coverage.context.as_mut()
            && *doc == id
            && *v == before
        {
            *v = after;
        }
    }

    /// A stroke begins at the mouse (`erase`: Ctrl held).
    pub(crate) fn begin_coverage_stroke(&mut self, e: &mut Editor, erase: bool) {
        if !erase && let Err(err) = self.brush_base(e) {
            self.status = err;
            return;
        }

        let Some(d) = document(e) else { return };
        let there = d.coverage_footprints(self.coverage.kind).into_iter().map(|(f, _)| f).collect();
        self.coverage.stroke = Some(CoverageStroke { erase, there, ..Default::default() });
        self.coverage_dab();
    }

    /// A dab at the mouse, when it moved far enough from the last one (a quarter of the radius).
    pub(crate) fn coverage_dab(&mut self) {
        let radius = self.coverage_radius();
        let at = (self.cursor.x, self.cursor.z);
        let seed = u64::from((self.random.unit() * u32::MAX as f32) as u32);
        let density = if self.coverage.brush_density > 0.0 { self.coverage.brush_density } else { 1.0 };
        let Some(stroke) = self.coverage.stroke.as_mut() else { return };
        if stroke.last.is_some_and(|l| (l.0 - at.0).hypot(l.1 - at.1) < radius * 0.25) {
            return;
        }

        stroke.last = Some(at);
        let inside = |x: f32, z: f32| (x - at.0).hypot(z - at.1) <= radius;
        if stroke.erase {
            stroke.erased.extend(stroke.there.iter().filter(|f| inside(f.x, f.z)).map(|f| f.elem));
            return;
        }

        let Some(base) = self.coverage.base.as_ref() else { return };
        let blocker = |x: f32, z: f32, radius: f32| Blocker { x, z, radius };
        let mut rocks = stroke.there.iter().filter(|f| !f.tree).map(|f| blocker(f.x, f.z, f.radius)).collect::<Vec<_>>();
        rocks.extend(stroke.rocks.iter().map(|r| blocker(r.x, r.z, base.size(&r.key).radius * r.scale)));
        let mut plants = stroke.there.iter().filter(|f| f.tree).map(|f| blocker(f.x, f.z, base.params.plant_radius)).collect::<Vec<_>>();
        plants.extend(stroke.plants.iter().map(|p| blocker(p.x, p.z, base.params.plant_radius)));
        let size = |k: &str| base.size(k);
        let (r, p) = scatter(&base.input(&size), &base.classified, at, radius, density, &rocks, &plants, seed);
        stroke.rocks.extend(r);
        stroke.plants.extend(p);
    }

    /// The stroke ends: its rocks and plants go into the kind's `Brushed` group, or what it takes away goes. What it did.
    ///
    /// # Errors
    ///
    /// No stroke or tile, or the edit failed (nothing changes).
    pub(crate) fn end_coverage_stroke(&mut self, e: &mut Editor) -> Result<String, String> {
        let stroke = self.coverage.stroke.take().ok_or("ground coverage: no stroke")?;
        let (id, kind) = (document_id(e).ok_or("no tile is open")?, self.coverage.kind);
        let d = e.docs.find_as_mut::<TileDocument>(id).ok_or("no tile is open")?;
        let before = d.history().version();
        let done = if stroke.erase {
            let elems = stroke.erased.into_iter().collect::<Vec<_>>();
            let (grid, params) = (d.coverage_grid(), self.coverage.params);
            let n = d.erase_coverage(kind, &elems, &|plants: &[Placed]| exclusions(&grid, plants, &params));
            if n.as_ref().is_ok_and(|&n| n > 0) {
                e.selection.clear();
            }

            n.map(|n| format!("ground coverage: {n} rock(s) and plant(s) taken away"))
        } else {
            let base = self.coverage.base.as_ref().ok_or("ground coverage: the brush has nothing worked out")?;
            let polygons = exclusions(&base.grid, &stroke.plants, &base.params);
            d.add_coverage_stroke(kind, stroke.rocks, stroke.plants, polygons)
                .map(|c| format!("ground coverage: {} rock(s), {} plant(s), {} exclusion polygon(s) brushed", c.rocks, c.plants, c.exclusions))
        };

        let after = e.docs.find_as::<TileDocument>(id).map_or(before, |d| d.history().version());
        self.adopt_coverage_version(id, before, after);
        done.map_err(|err| format!("ground coverage: {err}"))
    }

    /// The brush's lines: the kind's bands near the mouse (foot green, under the water cyan, face orange) while it is over the tile, and
    /// the stroke's rocks (grey) and plants (green) as crosses, or what it takes away (red).
    pub(crate) fn coverage_brush_overlay(&self, d: &TileDocument, hovered: bool, o: &mut Overlay) {
        if !self.coverage.open || !self.coverage.brush {
            return;
        }

        let t = d.tile();
        let (green, orange, cyan) = (overlay_colour(0.3, 0.9, 0.3, 1.0), overlay_colour(1.0, 0.6, 0.15, 1.0), overlay_colour(0.3, 0.95, 0.95, 1.0));
        let (grey, red) = (overlay_colour(0.8, 0.8, 0.8, 1.0), overlay_colour(1.0, 0.25, 0.2, 1.0));
        let cross = |o: &mut Overlay, x: f32, y: f32, z: f32, c: u32| {
            o.line(Float3::new(x - 0.5, y, z), Float3::new(x + 0.5, y, z), c);
            o.line(Float3::new(x, y, z - 0.5), Float3::new(x, y, z + 0.5), c);
        };

        // The bands around the mouse, a bit further than the circle (the base's, when it is this tile's).
        if let Some(b) = self.coverage.base.as_ref().filter(|b| b.key.0 == d.id() && hovered && self.terrain_hit) {
            let g = &b.grid;
            let reach = self.coverage_radius() * 1.5;
            let (x0, x1) = (g.index_x(self.cursor.x - reach), g.index_x(self.cursor.x + reach));
            let (z0, z1) = (g.index_z(self.cursor.z - reach), g.index_z(self.cursor.z + reach));
            let mut by_band: [Vec<usize>; 3] = Default::default();
            for i in (z0..=z1).flat_map(|z| (x0..=x1).map(move |x| z * g.nx + x)) {
                let (x, z) = g.at(i);
                if (x - self.cursor.x).hypot(z - self.cursor.z) > reach {
                    continue;
                }

                match b.classified.band_at(g, x, z) {
                    Band::Foot => by_band[0].push(i),
                    Band::Wet => by_band[1].push(i),
                    Band::Face => by_band[2].push(i),
                    Band::None => {}
                }
            }

            for (cells, c) in by_band.iter().zip([green, cyan, orange]) {
                crate::ground_coverage_panel::outline(g, cells, c, o);
            }
        }

        let Some(s) = self.coverage.stroke.as_ref() else { return };
        for (items, c) in [(&s.rocks, grey), (&s.plants, green)] {
            for p in items {
                cross(o, p.x, t.terrain_height(p.x, p.z) + 0.3, p.z, c);
            }
        }

        for f in s.there.iter().filter(|f| s.erased.contains(&f.elem)) {
            cross(o, f.x, t.terrain_height(f.x, f.z) + 0.5, f.z, red);
        }
    }

    /// A stroke from a script through ground points (a dab at each; `erase`: taking away), with the window's kind and values.
    ///
    /// # Errors
    ///
    /// No tile, no set, or the stroke failed.
    pub fn coverage_stroke_for_script(&mut self, e: &mut Editor, erase: bool, radius: f32, points: &[(f32, f32)]) -> Result<String, String> {
        self.open_coverage();
        if !self.coverage.brush {
            // As the window's Brush button: a preview of a fill goes.
            self.coverage.brush = true;
            self.coverage.preview = None;
        }

        self.coverage.brush_radius = radius;
        for (i, &(x, z)) in points.iter().enumerate() {
            let d = document(e).ok_or("no tile is open")?;
            self.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
            if i == 0 {
                self.begin_coverage_stroke(e, erase);
                if self.coverage.stroke.is_none() {
                    return Err(self.status.clone());
                }
            } else {
                self.coverage_dab();
            }
        }

        self.end_coverage_stroke(e)
    }
}
