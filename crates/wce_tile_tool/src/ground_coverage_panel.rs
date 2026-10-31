//! The automatic ground coverage's window (experimental; `wce_tile::ground_coverage`, `TileDocument::apply_coverage`): opened from the Tile
//! panel, it takes the kind of terrain (inland cliffs, coast, riverbanks), the parameters, a rock set (of buildings) and a plant set (of
//! trees), and a seed. Preview finds the differentials and draws them in the viewport (the foot bands green, their cells under the water
//! cyan, the faces orange, what is left alone red, a mark where each rock and plant would go, the exclusion polygons blue; hovering one says
//! why); Generate puts the result into `(AUTOGEN) Ground Coverage`, replacing what it held.
//! The last values are kept in the Tile settings; a tile whose layer has the settings of its last run starts from those.

use std::collections::{HashMap, HashSet};

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{DocumentId, Overlay, OverlayVertex, overlay_colour};
use wce_tile::ground_coverage::{Context, Differential, Grid, Input, Kind, Params, RockSize, Seat, State, generate};
use wce_tile::vegetation_sets::{SetKind, VegetationSet};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::ground_coverage::{COVERAGE_LAYER, CoverageSettings, GRID_CELL};

use crate::settings::{GroundCoverage, SETTINGS_ID, TileSettings};
use crate::tool::{TileTool, document, document_id};

/// The window's state.
#[derive(Default)]
pub struct CoverageUi {
    pub open: bool,
    pub kind: Kind,
    pub params: Params,
    pub seed: u64,
    pub rocks: String,
    pub plants: String,

    /// The tile the values were read for (the layer's settings, else the Tile settings).
    loaded_for: Option<DocumentId>,

    pub preview: Option<Preview>,

    /// The buildings' categories from the packs' tables (the mods' too), read on the first run, and why they could not be; again when the
    /// game, its mounted packs or the schema change (`CategoriesSource`).
    categories: Option<(CategoriesSource, std::sync::Arc<std::collections::BTreeMap<String, String>>)>,
    categories_note: String,

    /// Generate asked to replace what the layer holds (the question opens on the next frame).
    confirm: bool,

    /// The tile's water and no-go field on the generator's grid, for the tile and its version they were worked out for.
    context: Option<((DocumentId, u64), std::sync::Arc<Context>)>,
}

/// What Preview found, for one tile: the result, its lines, which differential each grid cell is in, and a summary.
pub struct Preview {
    doc: DocumentId,
    pub result: Vec<Differential>,
    lines: Vec<OverlayVertex>,
    grid: Grid,
    owner: Vec<u32>,
    pub summary: String,
}

/// What the buildings' categories were read from: the game, its mounted packs (their `Files`) and rpfm's schema.
type CategoriesSource = (String, Option<usize>, Option<std::path::PathBuf>);

const NONE: u32 = u32::MAX;

/// A rock whose model was not found counts as this big.
const UNKNOWN_ROCK: RockSize = RockSize { radius: 1.0, height: 1.0 };

impl TileTool {
    /// Opens the window (Tile panel, "Ground coverage...").
    pub fn open_coverage(&mut self) {
        self.coverage.open = true;
    }

    /// The window from a script: its sets (`-`: none), seed (else the tile's) and kind (else the window's; another one starts from its
    /// defaults), then Preview (the summary) or Generate (without asking).
    ///
    /// # Errors
    ///
    /// Not `preview` or `generate`, no set, or the generation failed.
    pub fn coverage_for_script(&mut self, e: &mut Editor, what: &str, rocks: &str, plants: &str, seed: Option<u64>, kind: Option<Kind>) -> Result<String, String> {
        self.open_coverage();
        self.load_coverage_values(e);
        if let Some(k) = kind.filter(|&k| k != self.coverage.kind) {
            self.set_coverage_kind(k);
        }

        let name = |s: &str| if s == "-" { String::new() } else { s.to_owned() };
        (self.coverage.rocks, self.coverage.plants) = (name(rocks), name(plants));
        if let Some(s) = seed {
            self.coverage.seed = s;
        }

        match what {
            "preview" => {
                let p = self.compute_coverage(e)?;
                let summary = p.summary.clone();
                self.coverage.preview = Some(p);
                Ok(summary)
            }

            "generate" => self.generate_coverage(e),

            _ => Err("ground_coverage: preview or generate".to_owned()),
        }
    }

    /// The values for a tile: its layer's last run, else the Tile settings with a new seed.
    fn load_coverage_values(&mut self, e: &mut Editor) {
        let id = document_id(e);
        if self.coverage.loaded_for == id {
            return;
        }

        self.coverage.loaded_for = id;
        self.coverage.preview = None;
        if let Some(s) = document(e).and_then(|d| d.coverage_settings()) {
            (self.coverage.params, self.coverage.seed, self.coverage.rocks, self.coverage.plants) = (s.params, s.seed, s.rocks, s.plants);
            return;
        }

        let s = e.tool_settings::<TileSettings>(SETTINGS_ID).ground_coverage;
        self.coverage.params = if s.params.is_empty() { Params::default() } else { Params::from_text(&s.params) };
        self.coverage.rocks = s.rocks;
        self.coverage.plants = s.plants;
        self.coverage.seed = u64::from((self.random.unit() * 1.0e6) as u32);
    }

    /// Another kind: its default parameters, no preview.
    fn set_coverage_kind(&mut self, kind: Kind) {
        self.coverage.kind = kind;
        self.coverage.params = Params::for_kind(kind);
        self.coverage.preview = None;
    }

    /// Closes the window, and its preview with it (the viewport would go on drawing it).
    fn close_coverage(&mut self) {
        self.coverage.open = false;
        self.coverage.preview = None;
    }

    fn keep_coverage_values(&self, e: &mut Editor) {
        let c = GroundCoverage { params: self.coverage.params.to_text(), rocks: self.coverage.rocks.clone(), plants: self.coverage.plants.clone() };
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| s.ground_coverage = c);
    }

    /// The window.
    pub(crate) fn draw_coverage_window(&mut self, e: &mut Editor) {
        if !self.coverage.open {
            return;
        }

        if document_id(e).is_none() {
            self.close_coverage();
            return;
        }

        self.load_coverage_values(e);
        gui::set_next_window_size(gui::v2(430.0, 640.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        if !gui::begin(&format!("{}  Ground coverage (experimental)###groundcoverage", icons::MOUNTAIN), Some(&mut open), 0) {
            gui::end();
            if !open {
                self.close_coverage();
            }

            return;
        }

        gui::text_wrapped(&format!(
            "Finds the kind's terrain and puts rocks and plants along its foot (by the water, its waterline) and rocks up its faces, with a \
             vegetation exclusion polygon around each clump of plants. It all goes into the layer \"{COVERAGE_LAYER}\", replacing what that \
             layer holds.",
        ));
        gui::spacing();

        let mut kind = Kind::ALL.iter().position(|&k| k == self.coverage.kind).unwrap_or(0) as i32;
        gui::set_next_item_width(260.0);
        if gui::combo("kind", &mut kind, &["Inland cliffs", "Coast", "Riverbanks"]) {
            self.set_coverage_kind(Kind::ALL[kind as usize]);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(kind_tip(self.coverage.kind));
        }

        let sets = self.vegetation_sets(e);
        set_combo("rocks", "a set of buildings", &sets, SetKind::Building, &mut self.coverage.rocks);
        set_combo("plants", "a set of trees", &sets, SetKind::Vegetation, &mut self.coverage.plants);
        if !sets.iter().any(|s| s.kind == SetKind::Building) {
            gui::text_disabled("No set of buildings yet: select some rocks and \"New set from the selected trees or buildings\".");
        }

        let changed = params_rows(self.coverage.kind, &mut self.coverage.params);

        gui::separator_text("Seed");
        // The field holds an i32: a bigger seed (a script's) is shown capped, and kept unless edited.
        let mut seed = self.coverage.seed.min(i32::MAX as u64) as i32;
        gui::set_next_item_width(140.0);
        let mut reseeded = gui::input_int("##seed", &mut seed);
        gui::same_line();
        if gui::button(&format!("{}  Reroll", icons::REFRESH_CW)) {
            seed = (self.random.unit() * 1.0e6) as i32;
            reseeded = true;
        }

        if reseeded {
            self.coverage.seed = seed.max(0) as u64;
        }

        gui::separator();
        let (mut preview, mut generate_now, mut clear) = (false, false, false);
        if gui::button(&format!("{}  Preview", icons::EYE)) {
            preview = true;
        }

        gui::same_line();
        if gui::button(&format!("{}  Generate", icons::SPARKLES)) {
            generate_now = true;
        }

        if self.coverage.preview.is_some() {
            gui::same_line();
            clear = gui::button("Clear preview");
        }

        gui::same_line();
        if gui::button("Defaults") {
            self.coverage.params = Params::default();
            self.coverage.preview = None;
        }

        // A preview of other values is not what Generate would make: it goes.
        if (changed || reseeded) && self.coverage.preview.is_some() {
            self.coverage.preview = None;
        }

        if let Some(p) = &self.coverage.preview {
            gui::spacing();
            gui::text_wrapped(&p.summary);
        }

        gui::end();
        if !open {
            self.close_coverage();
        }

        if clear {
            self.coverage.preview = None;
        }

        if preview {
            self.keep_coverage_values(e);
            match self.compute_coverage(e) {
                Ok(p) => self.coverage.preview = Some(p),
                Err(err) => self.status = err,
            }
        }

        if generate_now {
            self.keep_coverage_values(e);
            if document(e).is_some_and(|d| d.coverage_items() > 0) {
                self.coverage.confirm = true;
            } else {
                let _ = self.generate_coverage(e);
            }
        }

        self.draw_coverage_confirm(e);
    }

    fn draw_coverage_confirm(&mut self, e: &mut Editor) {
        if std::mem::take(&mut self.coverage.confirm) {
            gui::open_popup("Replace the ground coverage?");
        }

        if !gui::begin_popup_modal("Replace the ground coverage?", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let n = document(e).map_or(0, |d| d.coverage_items());
        gui::text_wrapped(&format!("\"{COVERAGE_LAYER}\" holds {n} item(s). They are replaced, edits made to them by hand too (Ctrl+Z undoes it)."));
        if gui::button("Replace") {
            gui::close_current_popup();
            let _ = self.generate_coverage(e);
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    /// The chosen sets (either may be missing, not both).
    fn coverage_sets(&mut self, e: &mut Editor) -> Result<(VegetationSet, VegetationSet), String> {
        let sets = self.vegetation_sets(e);
        let find = |name: &str, kind: SetKind| sets.iter().find(|s| s.name == name && s.kind == kind).cloned().unwrap_or_default();
        let (rocks, plants) = (find(&self.coverage.rocks, SetKind::Building), find(&self.coverage.plants, SetKind::Vegetation));
        if rocks.entries.is_empty() && plants.entries.is_empty() {
            return Err("ground coverage: choose a set of rocks or of plants (or both)".to_owned());
        }

        Ok((rocks, plants))
    }

    /// The buildings' categories of the packs' `battlefield_buildings` tables (read once for the game, its packs and the schema; empty
    /// without rpfm's schema, and the kit's tables are what is known then).
    fn building_categories(&mut self, e: &Editor) -> std::sync::Arc<std::collections::BTreeMap<String, String>> {
        let (files, schema) = (e.files(), crate::tool::walls_schema_path(e).ok());
        let source = (e.game().key.to_owned(), files.as_ref().map(|f| std::sync::Arc::as_ptr(f).cast::<()>() as usize), schema.clone());
        if let Some((_, c)) = self.coverage.categories.as_ref().filter(|(s, _)| *s == source) {
            return c.clone();
        }

        self.coverage.categories_note.clear();
        let mut out = std::collections::BTreeMap::new();
        match (files, schema) {
            (Some(files), Some(schema)) => {
                let tables = files.list("db/battlefield_buildings_tables/", "").into_iter().filter_map(|p| files.read(&p).ok()).collect::<Vec<_>>();
                match wce_tile_io::attila::game_data::building_categories(&tables, &schema) {
                    Ok(c) => out = c,
                    Err(err) => self.coverage.categories_note = err,
                }
            }

            (_, None) => self.coverage.categories_note = "no rpfm schema (schema_att.ron): only the kit's building categories are known".to_owned(),
            (None, _) => self.coverage.categories_note = "no game files: only the kit's building categories are known".to_owned(),
        }

        let c = std::sync::Arc::new(out);
        self.coverage.categories = Some((source, c.clone()));
        c
    }

    /// The tile's water and no-go field on the generator's grid (stage B's, as the water depth view's): worked out again when the tile
    /// changed (about a second on a big tile).
    ///
    /// # Errors
    ///
    /// No tile is open, or stage B fails (no height map, a linking tile's heights missing).
    fn coverage_context(&mut self, e: &mut Editor) -> Result<std::sync::Arc<Context>, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let key = (id, document(e).ok_or("no tile is open")?.history().version());
        if let Some((_, c)) = self.coverage.context.as_ref().filter(|(k, _)| *k == key) {
            return Ok(c.clone());
        }

        let game = self.exported_game(e).ok();
        let d = document(e).ok_or("no tile is open")?;
        let started = std::time::Instant::now();
        let (f, raw) = crate::water_view::stage_b_fields(d, game.as_deref()).map_err(|err| format!("ground coverage: the tile's water and no-go areas: {err}"))?;
        let (sx, sz) = crate::water_view::vertex_spacing(d, &f);
        // Stage B's heights, so its water's depth, are in scene units (`unit` per metre).
        let unit = d.tile().unit() as f32;
        let depth = crate::water_view::water_depth(&f).iter().map(|w| w / unit).collect::<Vec<_>>();
        let mut c = Context::resample(&d.coverage_grid(), f.w, f.h, sx, sz, &depth, &f.no_go_field(&raw));
        c.has_sea = d.project().entry.as_ref().is_some_and(|en| en.links().iter().any(|l| l.link_set == "sea" && l.test == "TLT_EQUALS"));
        let m2 = |n: usize| n as f32 * GRID_CELL * GRID_CELL;
        let (no_go, wet) = (c.no_go.iter().filter(|&&b| b).count(), c.water_depth.iter().filter(|&&d| d > 0.0).count());
        wce_foundation::log::info(format!(
            "ground coverage: the tile's water and no-go areas worked out in {:.2} s ({:.0} m2 no-go, {:.0} m2 under rivers and water planes, {})",
            started.elapsed().as_secs_f64(), m2(no_go), m2(wet), if c.has_sea { "by the sea" } else { "no sea" },
        ));

        let c = std::sync::Arc::new(c);
        self.coverage.context = Some((key, c.clone()));
        Ok(c)
    }

    /// Finds the differentials and what goes on them, with the lines that show it.
    fn compute_coverage(&mut self, e: &mut Editor) -> Result<Preview, String> {
        let (rocks, plants) = self.coverage_sets(e)?;
        let categories = self.building_categories(e);
        let context = self.coverage_context(e)?;
        let doc = document_id(e).ok_or("no tile is open")?;
        let d = document(e).ok_or("no tile is open")?;
        let (result, grid, unknown) = run(d, self.coverage.kind, &context, &rocks, &plants, &categories, self.coverage.params, self.coverage.seed);
        let lines = preview_lines(d, &grid, &result);
        let owner = owners(&grid, &result);
        let mut summary = summary_of(&result);
        if !self.coverage.categories_note.is_empty() {
            summary += &format!("\n{}", self.coverage.categories_note);
        }

        if !unknown.is_empty() {
            summary += &format!("\nNo model found for {} (counted 1 m across, 1 m high).", unknown.join(", "));
        }

        Ok(Preview { doc, result, lines, grid, owner, summary })
    }

    /// Generates and puts the result into the layer; what it did, also the status.
    ///
    /// # Errors
    ///
    /// No set, no tile, or the layer can't take the result.
    fn generate_coverage(&mut self, e: &mut Editor) -> Result<String, String> {
        let done = self.generate_coverage_into(e);
        self.status = done.clone().unwrap_or_else(|err| err);
        done
    }

    fn generate_coverage_into(&mut self, e: &mut Editor) -> Result<String, String> {
        let (rocks, plants) = self.coverage_sets(e)?;
        let settings = CoverageSettings { params: self.coverage.params, seed: self.coverage.seed, rocks: self.coverage.rocks.clone(), plants: self.coverage.plants.clone() };
        let categories = self.building_categories(e);
        let context = self.coverage_context(e)?;
        let d = document(e).ok_or("ground coverage: no tile is open")?;
        let (result, _, _) = run(d, self.coverage.kind, &context, &rocks, &plants, &categories, settings.params, settings.seed);
        let c = d.apply_coverage(&result, &settings).map_err(|err| format!("ground coverage failed: {err}"))?;

        // What is in the tile now is the result: the preview would draw over it.
        self.coverage.preview = None;
        e.selection.clear();
        Ok(format!("ground coverage: {} slope(s), {} rock(s), {} plant(s), {} exclusion polygon(s) in \"{COVERAGE_LAYER}\"", c.slopes, c.rocks, c.plants, c.exclusions))
    }

    /// The preview's lines, into the tool's overlay (only over the tile it was made for).
    pub(crate) fn coverage_overlay(&self, d: &TileDocument, o: &mut Overlay) {
        if let Some(p) = self.coverage.preview.as_ref().filter(|p| p.doc == d.id()) {
            o.lines.extend_from_slice(&p.lines);
        }
    }

    /// What the preview says of the differential under the mouse, as a tooltip.
    pub(crate) fn coverage_tooltip(&self, e: &Editor) {
        let Some(p) = &self.coverage.preview else { return };
        if !self.terrain_hit || !e.viewport.hovered || document_id(e) != Some(p.doc) {
            return;
        }

        let g = &p.grid;
        let (x, z) = (((self.cursor.x - g.x0) / g.cell).round(), ((self.cursor.z - g.z0) / g.cell).round());
        if x < 0.0 || z < 0.0 || x as usize >= g.nx || z as usize >= g.nz {
            return;
        }

        let k = p.owner[z as usize * g.nx + x as usize];
        if let Some(d) = p.result.get(k as usize) {
            let pr = &self.coverage.params;
            gui::set_tooltip(&match d.state {
                Some(State::Kept) => format!(
                    "Slope {}: rise {:.1} m, {:.0} m2; {} rock(s), {} plant(s){}", d.number, d.rise, d.area, d.rocks.len(), d.plants.len(),
                    if d.avoided == 0 { String::new() } else { format!("; goes around {} building(s)", d.avoided) },
                ),
                Some(State::TooLow(r)) => format!("Slope {}: left alone, too low ({r:.1} m, the least is {:.1} m)", d.number, pr.min_rise),
                Some(State::TooSmall(a)) => format!("Slope {}: left alone, too small ({a:.0} m2, the least is {:.0} m2)", d.number, pr.min_area),
                Some(State::ByWater) => format!("Slope {}: left alone, its foot is at the sea or in a river (Coast, Riverbanks)", d.number),
                None => String::new(),
            });
        }
    }
}

/// Runs the generator on the tile (around the buildings of the avoided categories): the result, the grid, and the rock keys whose model was
/// not found.
#[allow(clippy::too_many_arguments)]
fn run(
    d: &TileDocument, kind: Kind, context: &Context, rocks: &VegetationSet, plants: &VegetationSet, categories: &std::collections::BTreeMap<String, String>, params: Params, seed: u64,
) -> (Vec<Differential>, Grid, Vec<String>) {
    let grid = d.coverage_grid();
    let blockers = d.coverage_blockers(&|k: &str| categories.get(&k.to_lowercase()).cloned());
    let mut unknown = vec![];
    let sizes = rocks.entries.iter().map(|en| {
        let s = d.rock_size(&en.key).unwrap_or_else(|| {
            unknown.push(en.key.clone());
            UNKNOWN_ROCK
        });
        (en.key.clone(), s)
    }).collect::<HashMap<_, _>>();

    let size = |k: &str| sizes.get(k).copied().unwrap_or(UNKNOWN_ROCK);
    let result = generate(&Input { kind, grid: &grid, context: Some(context), blockers: &blockers, rocks, plants, rock_size: &size, params, seed });
    (result, grid, unknown)
}

/// "12 slopes: 9 covered (...), 3 left alone (...)".
fn summary_of(result: &[Differential]) -> String {
    let kept = result.iter().filter(|d| d.kept()).collect::<Vec<_>>();
    let count = |f: fn(&State) -> bool| result.iter().filter(|d| d.state.as_ref().is_some_and(f)).count();
    let (low, small) = (count(|s| matches!(s, State::TooLow(_))), count(|s| matches!(s, State::TooSmall(_))));
    let water = count(|s| matches!(s, State::ByWater));
    let by_water = if water == 0 { String::new() } else { format!(", {water} by the water (Coast, Riverbanks)") };
    let around = kept.iter().filter(|d| d.avoided != 0).count();
    format!(
        "{} slope(s): {} covered with {} rock(s), {} plant(s) and {} exclusion polygon(s), {around} of them around buildings; left alone: {low} too \
         low, {small} too small{by_water}.",
        result.len(), kept.len(), kept.iter().map(|d| d.rocks.len()).sum::<usize>(), kept.iter().map(|d| d.plants.len()).sum::<usize>(),
        kept.iter().map(|d| d.exclusions.len()).sum::<usize>(),
    )
}

/// Which differential each grid cell is in (its cells and its bands).
fn owners(g: &Grid, result: &[Differential]) -> Vec<u32> {
    let mut owner = vec![NONE; g.nx * g.nz];
    for (k, d) in result.iter().enumerate() {
        for &i in d.cells.iter().chain(&d.foot) {
            owner[i] = k as u32;
        }
    }

    owner
}

/// The preview's lines: the outlines of the foot bands (green), their cells under the water (cyan) and faces (orange) of the kept
/// differentials and of the ones left alone (red), a cross where each rock (grey) and plant (green) goes, and the exclusion polygons (blue).
fn preview_lines(d: &TileDocument, g: &Grid, result: &[Differential]) -> Vec<OverlayVertex> {
    let mut o = Overlay::default();
    let (green, orange, red) = (overlay_colour(0.3, 0.9, 0.3, 1.0), overlay_colour(1.0, 0.6, 0.15, 1.0), overlay_colour(0.95, 0.2, 0.2, 1.0));
    let (grey, blue, cyan) = (overlay_colour(0.8, 0.8, 0.8, 1.0), overlay_colour(0.3, 0.6, 1.0, 1.0), overlay_colour(0.3, 0.95, 0.95, 1.0));
    let t = d.tile();
    let up = 0.3;
    for dif in result {
        if dif.kept() {
            outline(g, &dif.foot, green, &mut o);
            outline(g, &dif.face, orange, &mut o);
            outline(g, &dif.wet, cyan, &mut o);
        } else {
            outline(g, &dif.cells, red, &mut o);
        }

        for (items, c) in [(&dif.rocks, grey), (&dif.plants, green)] {
            for p in items {
                let (y, r) = (t.terrain_height(p.x, p.z) + up, 0.5);
                o.line(Float3::new(p.x - r, y, p.z), Float3::new(p.x + r, y, p.z), c);
                o.line(Float3::new(p.x, y, p.z - r), Float3::new(p.x, y, p.z + r), c);
            }
        }

        for poly in &dif.exclusions {
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                let steps = ((b.0 - a.0).hypot(b.1 - a.1) / 2.0).ceil().max(1.0) as usize;
                let at = |s: usize| {
                    let f = s as f32 / steps as f32;
                    let (x, z) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
                    Float3::new(x, t.terrain_height(x, z) + up, z)
                };

                for s in 0..steps {
                    o.line(at(s), at(s + 1), blue);
                }
            }
        }
    }

    o.lines
}

/// The outline of a set of grid cells (each a square around its sample), on the terrain.
fn outline(g: &Grid, cells: &[usize], colour: u32, o: &mut Overlay) {
    let set = cells.iter().copied().collect::<HashSet<_>>();
    let h = g.cell * 0.5;
    let at = |x: f32, z: f32| Float3::new(x, g.height(x, z) + 0.3, z);
    for &i in cells {
        let (cx, cz) = g.at(i);
        let (x, z) = (i % g.nx, i / g.nx);
        let out = |dx: isize, dz: isize| {
            let (nx, nz) = (x as isize + dx, z as isize + dz);
            nx < 0 || nz < 0 || nx as usize >= g.nx || nz as usize >= g.nz || !set.contains(&(nz as usize * g.nx + nx as usize))
        };

        if out(1, 0) {
            o.line(at(cx + h, cz - h), at(cx + h, cz + h), colour);
        }

        if out(-1, 0) {
            o.line(at(cx - h, cz - h), at(cx - h, cz + h), colour);
        }

        if out(0, 1) {
            o.line(at(cx - h, cz + h), at(cx + h, cz + h), colour);
        }

        if out(0, -1) {
            o.line(at(cx - h, cz - h), at(cx + h, cz - h), colour);
        }
    }
}

/// What a kind of terrain is, for its tooltip.
fn kind_tip(kind: Kind) -> &'static str {
    match kind {
        Kind::Cliffs => "Inland slopes and cliffs steeper than the angle, whose foot is over the sea and out of the rivers",
        Kind::Coast => "The ground by the sea (under 0 m, reached from the tile's edge) that units can't walk; its bands measured from the sea",
        Kind::Riverbank => "The ground by a river or a water plane that units can't walk; its bands measured from the water's surface",
    }
}

/// A combo of the sets of a kind (and none).
fn set_combo(label: &str, hint: &str, sets: &[VegetationSet], kind: SetKind, chosen: &mut String) {
    let preview = if chosen.is_empty() { format!("none ({hint})") } else { chosen.clone() };
    gui::set_next_item_width(260.0);
    if gui::begin_combo(label, &preview) {
        if gui::selectable("none", chosen.is_empty(), 0) {
            chosen.clear();
        }

        for s in sets.iter().filter(|s| s.kind == kind) {
            let name = if s.folder.is_empty() { s.name.clone() } else { format!("{}   {}", s.name, s.folder) };
            if gui::selectable(&name, *chosen == s.name, 0) {
                chosen.clone_from(&s.name);
            }
        }

        gui::end_combo();
    }

    if !chosen.is_empty() && !sets.iter().any(|s| s.kind == kind && s.name == *chosen) {
        gui::same_line();
        gui::text_colored(theme::warning(), "(not found)");
    }
}

/// The parameters' rows; whether one changed.
fn params_rows(kind: Kind, p: &mut Params) -> bool {
    let mut changed = false;
    let mut f = |label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &str, tip: &str| {
        gui::set_next_item_width(160.0);
        changed |= gui::drag_float(label, v, speed, lo, hi, format);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    };

    gui::separator_text("Slopes");
    if kind.by_water() {
        gui::text_disabled("Where units can't walk (the game's no-go areas, steeper than 30 deg), by the water.");
        f("from the water", &mut p.water_reach, 0.1, 0.0, 50.0, "%.1f m", "How far from the water the no-go ground may be to count");
        f("under the water", &mut p.water_below, 0.05, 0.0, 10.0, "%.1f m", "How deep into the water the waterline band goes (rocks only)");
    } else {
        f("steeper than", &mut p.slope_deg, 0.2, 5.0, 85.0, "%.1f deg", "A slope steeper than this is a differential");
        f("foot over the sea", &mut p.sea_margin, 0.05, 0.0, 50.0, "%.1f m", "A slope whose foot is lower is the coast's (and one whose foot is in a river, the banks')");
    }

    f("rise at least", &mut p.min_rise, 0.05, 0.0, 100.0, "%.1f m", "A lower one (a ramp, a bump) is left alone");
    f("area at least", &mut p.min_area, 1.0, 0.0, 10000.0, "%.0f m2", "A smaller one is left alone");

    gui::separator_text("Bands");
    f("foot: up the slope", &mut p.foot_up, 0.05, 0.0, 50.0, "%.1f m", "How far up the slope the foot band goes (rocks and plants)");
    f("foot: out on the flat", &mut p.foot_out, 0.05, 0.0, 50.0, "%.1f m", "How far out from the slope the foot band goes");
    f("face: under the top", &mut p.top_margin, 0.05, 0.0, 50.0, "%.1f m", "The face band (rocks only) stops this far under the top");

    gui::separator_text("Rocks");
    let mut per100 = |label: &str, v: &mut f32, tip: &str| {
        let mut d = *v * 100.0;
        gui::set_next_item_width(160.0);
        if gui::drag_float(label, &mut d, 0.05, 0.0, 100.0, "%.2f / 100 m2") {
            *v = d / 100.0;
            changed = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    };

    per100("at the foot", &mut p.rock_density_foot, "Rocks per 100 square metres of the foot band");
    per100("on the face", &mut p.rock_density_face, "Rocks per 100 square metres of the face band");
    changed |= gui::checkbox("scale the rocks", &mut p.scale_rocks);
    if gui::is_item_hovered() {
        gui::set_tooltip("A building's scale is the editor's: TEd has none, and whether Attila honours it is not known. Off: every rock at scale 1.");
    }

    if p.scale_rocks {
        changed |= range_row("scale at the foot", &mut p.rock_scale_foot);
        changed |= range_row("scale on the face", &mut p.rock_scale_face);
        gui::text_colored(theme::warning(), "Experimental: the game may show them at scale 1.");
    }

    let mut seat = Seat::ALL.iter().position(|&s| s == p.seat).unwrap_or(1) as i32;
    gui::set_next_item_width(160.0);
    if gui::combo("stands on", &mut seat, &["the ground at its centre", "the mean ground under it", "the lowest ground under it"]) {
        p.seat = Seat::ALL[seat as usize];
        changed = true;
    }

    if gui::is_item_hovered() {
        gui::set_tooltip("The ground a rock's base is put on. The lowest never lets its downhill side float, but buries big rocks on steep faces.");
    }

    let mut f = |label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &str, tip: &str| {
        gui::set_next_item_width(160.0);
        changed |= gui::drag_float(label, v, speed, lo, hi, format);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    };

    let mut sink = p.sink * 100.0;
    f("sunk", &mut sink, 0.5, 0.0, 90.0, "%.0f %% of its height", "How much of each rock goes under the ground it stands on");
    p.sink = sink / 100.0;
    let mut visible = p.min_visible * 100.0;
    f("at least visible", &mut visible, 0.5, 0.0, 100.0, "%.0f %% of its height", "How much of each rock stays over the ground at its centre, whatever the rest says");
    p.min_visible = visible / 100.0;

    gui::separator_text("Plants");
    let mut density = p.plant_density * 100.0;
    f("at the foot##plants", &mut density, 0.05, 0.0, 100.0, "%.2f / 100 m2", "Plants per 100 square metres of the foot band");
    p.plant_density = density / 100.0;
    f("apart", &mut p.plant_spacing, 0.05, 0.1, 50.0, "%.1f m", "The least distance between two plants");
    changed |= range_row("scale##plants", &mut p.plant_scale);

    gui::separator_text("Buildings");
    let mut f = |label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &str, tip: &str| {
        gui::set_next_item_width(160.0);
        changed |= gui::drag_float(label, v, speed, lo, hi, format);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    };

    f("clearance", &mut p.building_clearance, 0.05, 0.0, 50.0, "%.1f m", "What every rock and plant keeps from a building's footprint");
    gui::text_disabled("Gone around: houses, fort walls, towers, gates, bridges. Not rocks, props or ground pieces.");

    gui::separator_text("Vegetation exclusion");
    let mut f = |label: &str, v: &mut f32, speed: f32, lo: f32, hi: f32, format: &str, tip: &str| {
        gui::set_next_item_width(160.0);
        changed |= gui::drag_float(label, v, speed, lo, hi, format);
        if gui::is_item_hovered() {
            gui::set_tooltip(tip);
        }
    };

    f("clump: plants within", &mut p.cluster_distance, 0.1, 0.0, 100.0, "%.1f m", "Plants this near are one clump, with one polygon around it");
    f("clump: at most", &mut p.max_cluster, 0.5, 1.0, 500.0, "%.0f m across", "No clump gets wider (its polygon would cover the land between)");
    f("margin", &mut p.exclusion_margin, 0.05, 0.0, 50.0, "%.1f m", "Around each plant (and its own radius) in the polygon");
    changed
}

fn range_row(label: &str, r: &mut (f32, f32)) -> bool {
    let mut v = [r.0, r.1];
    gui::set_next_item_width(160.0);
    let changed = gui::drag_float2(label, &mut v, 0.01, "%.2f");
    if changed {
        (r.0, r.1) = (v[0].clamp(0.05, 20.0), v[1].clamp(0.05, 20.0));
    }

    changed
}
