//! The automatic ground coverage's window (experimental; `wce_tile::ground_coverage`, `TileDocument::apply_coverage`): opened from the Tile
//! panel, a tab per kind of terrain (inland cliffs, coast, riverbanks), each with its presets (`wce_tile::ground_coverage_presets`: the
//! parameters, a rock set of buildings and a plant set of trees; saved, saved as, deleted here), and a seed. Preview finds the differentials and draws them in the viewport (the foot bands green, their cells under the water
//! cyan, the faces orange, what is left alone red, a mark where each rock and plant would go, the exclusion polygons blue; hovering one says
//! why); Generate puts the result into the kind's group of `(AUTOGEN) Ground Coverage`, replacing what the group held but its brushed
//! strokes. A tile whose kind's group has the settings of its last run starts from those, else from the kind's last preset (the Tile
//! settings remember it).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::math::Float3;
use wce_scene::render_scene::{DocumentId, Overlay, OverlayVertex, overlay_colour};
use wce_tile::ground_coverage::{Blocker, Context, Differential, Grid, Input, Kind, Params, RockSize, Seat, State, generate};
use wce_tile::ground_coverage_presets::{Preset, of_kind, presets_root, scan};
use wce_tile::vegetation_sets::{SetKind, VegetationSet};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::ground_coverage::{COVERAGE_LAYER, CoverageSettings, GRID_CELL, kind_group_name};

use crate::settings::{SETTINGS_ID, TileSettings};
use crate::styled;
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

    /// Around buildings: the clutter rules.
    pub clutter: Vec<wce_tile::ground_coverage::ClutterDef>,

    /// The preset the values came from (they may have been changed since): `Default` for the kind's built-in one; empty: none known
    /// (a run of the first version's).
    pub preset: String,

    /// The presets' files, read on first use and again after a change.
    presets: Option<Arc<Vec<Preset>>>,

    /// Save as: the name being typed. Delete: asked whether to.
    save_as: Option<String>,
    delete_ask: bool,

    /// The kind changed from elsewhere (a script): the tab bar selects its tab on the next frame.
    select_tab: bool,

    /// Every number (advanced) or the plain choices.
    pub advanced: bool,

    /// Brush mode (else Fill the tile), the brush's radius (metres; 0: the default) and the factor on the preset's densities (0: 1).
    pub brush: bool,
    pub brush_radius: f32,
    pub brush_density: f32,

    /// What the brush's dabs need of the tile, and the stroke under way (`ground_coverage_brush.rs`).
    pub(crate) base: Option<Box<crate::ground_coverage_brush::BrushBase>>,
    pub(crate) stroke: Option<crate::ground_coverage_brush::CoverageStroke>,

    /// The tile the values were read for (its kind's group's settings, else the kind's last preset).
    loaded_for: Option<DocumentId>,

    pub preview: Option<Preview>,

    /// The buildings' categories from the packs' tables (the mods' too), read on the first run, and why they could not be; again when the
    /// game, its mounted packs or the schema change (`CategoriesSource`).
    categories: Option<(CategoriesSource, std::sync::Arc<std::collections::BTreeMap<String, String>>)>,
    categories_note: String,

    /// Generate asked to replace what the layer holds (the question opens on the next frame).
    confirm: bool,

    /// The tile's water and no-go field on the generator's grid, for the tile and its version they were worked out for.
    pub(crate) context: Option<((DocumentId, u64), std::sync::Arc<Context>)>,
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
pub(crate) const UNKNOWN_ROCK: RockSize = RockSize { radius: 1.0, height: 1.0 };

impl TileTool {
    /// Opens the window (Tile panel, "Ground coverage...").
    pub fn open_coverage(&mut self) {
        self.coverage.open = true;
    }

    /// The window from a script: the kind, its preset (by name, `Default` the built-in one; `-`: the values the window starts from for the
    /// tile) and the seed (else the values'), then Preview (the summary) or Generate (without asking).
    ///
    /// # Errors
    ///
    /// Not `preview` or `generate`, no such preset, no set, or the generation failed.
    pub fn coverage_for_script(
        &mut self,
        e: &mut Editor,
        what: &str,
        kind: Kind,
        preset: &str,
        seed: Option<u64>,
    ) -> Result<String, String> {
        self.open_coverage();
        self.load_coverage_values(e);
        if kind != self.coverage.kind {
            self.set_coverage_kind(e, kind);
            self.coverage.select_tab = true;
        }

        if preset != "-" {
            let p = self.find_preset(e, kind, preset).ok_or_else(|| format!("ground coverage: no {} preset '{preset}'", kind.name()))?;
            self.use_preset(e, &p);
        }

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

    /// The values for a tile, once per tile ([`Self::load_kind_values`]).
    fn load_coverage_values(&mut self, e: &mut Editor) {
        let id = document_id(e);
        if self.coverage.loaded_for == id {
            return;
        }

        self.coverage.loaded_for = id;
        self.load_kind_values(e);
    }

    /// The kind's values for the tile: its last run (its group's settings), else its last preset (the built-in one when there is none or
    /// it went) with a new seed. No preview.
    fn load_kind_values(&mut self, e: &mut Editor) {
        let kind = self.coverage.kind;
        self.coverage.preview = None;
        (self.coverage.save_as, self.coverage.delete_ask) = (None, false);
        if let Some(s) = document(e).and_then(|d| d.coverage_settings(kind)) {
            let c = &mut self.coverage;
            (c.params, c.seed, c.rocks, c.plants, c.preset, c.clutter) = (s.params, s.seed, s.rocks, s.plants, s.preset, s.clutter);
            return;
        }

        let last = e.tool_settings::<TileSettings>(SETTINGS_ID).ground_coverage.last_preset.get(kind.name()).cloned().unwrap_or_default();
        let p = self.find_preset(e, kind, &last).unwrap_or_else(|| Preset::builtin(kind));
        self.use_preset(e, &p);
        self.coverage.seed = u64::from((self.random.unit() * 1.0e6) as u32);
    }

    /// Another kind, with its values for the tile ([`Self::load_kind_values`]).
    fn set_coverage_kind(&mut self, e: &mut Editor, kind: Kind) {
        self.coverage.kind = kind;
        self.load_kind_values(e);
    }

    /// The folder of the presets: the projects'.
    fn presets_folder(e: &mut Editor) -> std::path::PathBuf {
        presets_root(&Self::workspace_dir(e).join("raw_data"))
    }

    /// The presets' files, read on first use and again after a change.
    fn coverage_presets(&mut self, e: &mut Editor) -> Arc<Vec<Preset>> {
        if let Some(p) = &self.coverage.presets {
            return p.clone();
        }

        let p = Arc::new(scan(&Self::presets_folder(e)));
        self.coverage.presets = Some(p.clone());
        p
    }

    /// A kind's preset by its name (`Default`: the built-in one).
    fn find_preset(&mut self, e: &mut Editor, kind: Kind, name: &str) -> Option<Preset> {
        of_kind(&self.coverage_presets(e), kind).into_iter().find(|p| p.name == name)
    }

    /// The preset's values (not the seed), remembered as the kind's last. No preview.
    fn use_preset(&mut self, e: &mut Editor, p: &Preset) {
        let c = &mut self.coverage;
        (c.params, c.rocks, c.plants, c.preset, c.clutter) =
            (p.params, p.rocks.clone(), p.plants.clone(), p.name.clone(), p.clutter.clone());
        c.preview = None;
        self.remember_preset(e);
    }

    /// The preset in use is the kind's last.
    fn remember_preset(&self, e: &mut Editor) {
        if self.coverage.preset.is_empty() {
            return;
        }

        let (kind, name) = (self.coverage.kind.name().to_owned(), self.coverage.preset.clone());
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.ground_coverage.last_preset.insert(kind, name);
        });
    }

    /// Writes the values to the preset in use (a file), or to a new one named `name`; then it is the one in use.
    fn save_preset(&mut self, e: &mut Editor, name: Option<&str>) -> Result<String, String> {
        let kind = self.coverage.kind;
        let path = match name {
            Some(n) => {
                let n = crate::veg_sets::set_file_name(n)?;
                let path = Self::presets_folder(e).join(format!("{n}.wce"));
                if path.exists() {
                    return Err(format!("there is a preset '{n}' already: pick it and Save"));
                }

                path
            }

            None => self
                .find_preset(e, kind, &self.coverage.preset.clone())
                .and_then(|p| p.path)
                .ok_or("the built-in preset can't be saved: Save as")?,
        };

        let c = &self.coverage;
        let p = Preset {
            name: String::new(),
            folder: String::new(),
            path: Some(path.clone()),
            kind,
            params: c.params,
            rocks: c.rocks.clone(),
            plants: c.plants.clone(),
            clutter: c.clutter.clone(),
        };
        p.save()?;
        self.coverage.presets = None;
        self.coverage.preset = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.remember_preset(e);
        Ok(format!("ground coverage: preset saved ({})", path.display()))
    }

    /// Deletes the preset in use (a file); the built-in one is used then.
    fn delete_preset(&mut self, e: &mut Editor) -> Result<String, String> {
        let kind = self.coverage.kind;
        let path =
            self.find_preset(e, kind, &self.coverage.preset.clone()).and_then(|p| p.path).ok_or("the built-in preset can't be deleted")?;
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        self.coverage.presets = None;
        self.use_preset(e, &Preset::builtin(kind));
        Ok(format!("ground coverage: preset deleted ({})", path.display()))
    }

    /// Closes the window, and its preview with it (the viewport would go on drawing it); a stroke under way is dropped.
    fn close_coverage(&mut self) {
        self.coverage.open = false;
        self.coverage.preview = None;
        self.coverage.stroke = None;
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
             vegetation exclusion polygon around each clump of plants. It goes into the kind's group in the layer \"{COVERAGE_LAYER}\", \
             replacing what the group holds but its brushed strokes.",
        ));
        gui::spacing();

        // The tabs follow the kind (a script may set it), and set it when clicked.
        let mut picked = None;
        if gui::begin_tab_bar("##coveragekinds") {
            for k in Kind::ALL {
                let select = self.coverage.select_tab && k == self.coverage.kind;
                if gui::begin_tab_item(kind_title(k), if select { gui::TAB_SET_SELECTED } else { 0 }) {
                    picked = Some(k);
                    gui::end_tab_item();
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip(kind_tip(k));
                }
            }

            gui::end_tab_bar();
        }

        match picked {
            Some(k) if self.coverage.select_tab && k == self.coverage.kind => self.coverage.select_tab = false,
            Some(k) if !self.coverage.select_tab && k != self.coverage.kind => self.set_coverage_kind(e, k),
            _ => {}
        }

        let row = styled::row("Show", "The plain choices, or every number");
        if let Some(i) = styled::segmented("mode", &["Simple", "Advanced"], Some(usize::from(self.coverage.advanced)), row.width) {
            self.coverage.advanced = i == 1;
        }

        self.preset_rows(e);
        let sets = self.vegetation_sets(e);
        let mut changed = false;
        if self.coverage.kind == Kind::Buildings {
            let mut clutter = std::mem::take(&mut self.coverage.clutter);
            changed |= self.clutter_rows(e, &mut clutter, &sets);
            self.coverage.clutter = clutter;
        } else {
            gui::separator_text("What it lays");
            set_row("Rocks", "a set of buildings", &sets, SetKind::Building, &mut self.coverage.rocks);
            set_row("Plants", "a set of trees", &sets, SetKind::Vegetation, &mut self.coverage.plants);
            if !sets.iter().any(|s| s.kind == SetKind::Building) {
                note("No set of buildings yet: select some rocks and \"New set from the selected trees or buildings\".");
            }
        }

        changed |= params_rows(self.coverage.kind, &mut self.coverage.params, self.coverage.advanced);

        gui::separator_text("Lay it");
        let row = styled::row("Where", "The whole tile at once, or where you brush");
        if let Some(i) = styled::segmented("where", &["Fill the tile", "Brush"], Some(usize::from(self.coverage.brush)), row.width)
            && (i == 1) != self.coverage.brush
        {
            self.coverage.brush = i == 1;
            self.coverage.preview = None;
        }

        if self.coverage.brush {
            note(
                "Left drag in the viewport (Select mode) puts the kind's rocks and plants on its terrain under the circle; Ctrl + drag takes the kind's coverage away. The bands show around the mouse once a stroke worked them out.",
            );
            let row = styled::row("Radius", "The brush's radius (Ctrl + click to type)");
            let mut radius = self.coverage_radius();
            gui::set_next_item_width(row.width);
            if gui::slider_float("##radius", &mut radius, 1.0, 100.0, "%.1f m") {
                self.coverage.brush_radius = radius;
            }

            let row = styled::row("Density", "A factor on the preset's densities, for the brush only");
            let mut density = if self.coverage.brush_density > 0.0 { self.coverage.brush_density } else { 1.0 };
            gui::set_next_item_width(row.width);
            if gui::slider_float("##density", &mut density, 0.05, 4.0, "x %.2f") {
                self.coverage.brush_density = density;
            }
        }

        // The field holds an i32: a bigger seed (a script's) is shown capped, and kept unless edited.
        let row = styled::row("Seed", "Another seed, another layout of the same values");
        let mut seed = self.coverage.seed.min(i32::MAX as u64) as i32;
        gui::set_next_item_width(row.width - gui::button_width(icons::REFRESH_CW) - gui::item_spacing().x);
        let mut reseeded = gui::input_int("##seed", &mut seed);
        gui::same_line();
        if styled::icon_button(icons::REFRESH_CW, "reroll", "Another seed") {
            seed = (self.random.unit() * 1.0e6) as i32;
            reseeded = true;
        }

        if reseeded {
            self.coverage.seed = seed.max(0) as u64;
        }

        let (mut preview, mut generate_now, mut clear) = (false, false, false);
        if !self.coverage.brush {
            gui::spacing();
            let n = if self.coverage.preview.is_some() { 3.0 } else { 2.0 };
            let w = (gui::content_region_avail().x - gui::item_spacing().x * (n - 1.0)) / n;
            if gui::button_sized(&format!("{}  Preview", icons::EYE), gui::v2(w, 0.0)) {
                preview = true;
            }

            gui::same_line();
            if gui::button_sized(&format!("{}  Generate", icons::SPARKLES), gui::v2(w, 0.0)) {
                generate_now = true;
            }

            if self.coverage.preview.is_some() {
                gui::same_line();
                clear = gui::button_sized("Clear preview", gui::v2(w, 0.0));
            }
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
            self.remember_preset(e);
            match self.compute_coverage(e) {
                Ok(p) => self.coverage.preview = Some(p),
                Err(err) => self.status = err,
            }
        }

        if generate_now {
            self.remember_preset(e);
            if document(e).is_some_and(|d| d.coverage_items(self.coverage.kind) > 0) {
                self.coverage.confirm = true;
            } else {
                let _ = self.generate_coverage(e);
            }
        }

        self.draw_coverage_confirm(e);
    }

    /// The preset in use (and whether the values are still its), the others to pick, and Save, Save as, Delete.
    fn preset_rows(&mut self, e: &mut Editor) {
        let kind = self.coverage.kind;
        let presets = of_kind(&self.coverage_presets(e), kind);
        let current = presets.iter().find(|p| p.name == self.coverage.preset).cloned();
        let c = &self.coverage;
        let label = |p: &Preset| {
            if p.is_builtin() {
                format!("{} (built in)", p.name)
            } else if p.folder.is_empty() {
                p.name.clone()
            } else {
                format!("{}   {}", p.name, p.folder)
            }
        };
        let shown = match &current {
            Some(p) if p.same_values(&c.params, &c.rocks, &c.plants, &c.clutter) => label(p),
            Some(p) => format!("{} (changed)", label(p)),
            None if c.preset.is_empty() => "none (the tile's last run)".to_owned(),
            None => format!("{} (not found)", c.preset),
        };

        let mut pick = None;
        let row = styled::row("Preset", "The built-in preset of the kind and the projects' (raw_data/art/ground_coverage_presets)");
        let buttons =
            [icons::SAVE, icons::FILE_PLUS, icons::TRASH_2].iter().map(|i| gui::button_width(i)).sum::<f32>() + 3.0 * gui::item_spacing().x;
        gui::set_next_item_width((row.width - buttons).max(100.0));
        if gui::begin_combo("##preset", &shown) {
            for p in &presets {
                if gui::selectable(&label(p), p.name == c.preset, 0) {
                    pick = Some(p.clone());
                }
            }

            gui::end_combo();
        }

        let file = current.as_ref().is_some_and(|p| !p.is_builtin());
        let mut result = None;
        if self.coverage.save_as.is_none() && !self.coverage.delete_ask {
            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##savepreset", icons::SAVE)) {
                result = Some(self.save_preset(e, None));
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Save the values into their preset (a built-in one has no file: Save as)");
            }

            gui::same_line();
            if gui::button(&format!("{}##saveas", icons::FILE_PLUS)) {
                self.coverage.save_as = Some(String::new());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Save as a new preset...");
            }

            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##deletepreset", icons::TRASH_2)) {
                self.coverage.delete_ask = true;
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Delete the preset's file");
            }
        }

        if let Some(p) = pick {
            self.use_preset(e, &p);
        }

        if let Some(mut name) = self.coverage.save_as.take() {
            gui::set_next_item_width(gui::content_region_avail().x - gui::button_width("Save") - gui::button_width("Cancel") - 24.0);
            let enter = gui::input_text("##presetname", Some("the new preset's name"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
            gui::same_line();
            let ok = gui::small_button("Save") || enter;
            gui::same_line();
            if ok && !name.trim().is_empty() {
                result = Some(self.save_preset(e, Some(&name)));
            } else if !gui::small_button("Cancel") {
                self.coverage.save_as = Some(name);
            }
        } else if self.coverage.delete_ask {
            gui::text_colored(theme::warning(), &format!("Delete the preset '{}'?", self.coverage.preset));
            gui::same_line();
            if gui::small_button("Delete") {
                result = Some(self.delete_preset(e));
                self.coverage.delete_ask = false;
            }

            gui::same_line();
            if gui::small_button("Cancel") {
                self.coverage.delete_ask = false;
            }
        }

        if let Some(r) = result {
            self.status = r.unwrap_or_else(|err| err);
        }
    }

    fn draw_coverage_confirm(&mut self, e: &mut Editor) {
        if std::mem::take(&mut self.coverage.confirm) {
            gui::open_popup("Replace the ground coverage?");
        }

        if !gui::begin_popup_modal("Replace the ground coverage?", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let n = document(e).map_or(0, |d| d.coverage_items(self.coverage.kind));
        let group = kind_group_name(self.coverage.kind);
        gui::text_wrapped(&format!(
            "\"{COVERAGE_LAYER}\" > \"{group}\" holds {n} item(s) from the last run. They are replaced, edits made to them by hand too (Ctrl+Z \
             undoes it); the brushed strokes stay.",
        ));
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
    pub(crate) fn coverage_sets(&mut self, e: &mut Editor) -> Result<(VegetationSet, VegetationSet), String> {
        let sets = self.vegetation_sets(e);
        let find = |name: &str, kind: SetKind| sets.iter().find(|s| s.name == name && s.kind == kind).cloned().unwrap_or_default();
        let (rocks, plants) = (find(&self.coverage.rocks, SetKind::Building), find(&self.coverage.plants, SetKind::Vegetation));
        if self.coverage.kind == Kind::Buildings {
            return Ok((rocks, plants));
        }

        if rocks.entries.is_empty() && plants.entries.is_empty() {
            return Err("ground coverage: choose a set of rocks or of plants (or both)".to_owned());
        }

        Ok((rocks, plants))
    }

    /// Around buildings: the window's clutter rules with their props (a building set by name, else the keys inline); empty for the other
    /// kinds.
    ///
    /// # Errors
    ///
    /// A rule names a set that is not there, or no rule has any props.
    pub(crate) fn clutter_rules(&mut self, e: &mut Editor) -> Result<Vec<wce_tile::ground_coverage::ClutterRule>, String> {
        if self.coverage.kind != Kind::Buildings {
            return Ok(vec![]);
        }

        let sets = self.vegetation_sets(e);
        let mut out = vec![];
        for d in &self.coverage.clutter {
            let set = if d.set.is_empty() {
                VegetationSet::from_keys(&d.name, d.keys.iter().map(String::as_str))
            } else {
                sets.iter()
                    .find(|s| s.name == d.set && s.kind == SetKind::Building)
                    .cloned()
                    .ok_or_else(|| format!("around buildings: the rule '{}' names the set '{}', which is not there", d.name, d.set))?
            };
            out.push(wce_tile::ground_coverage::ClutterRule {
                name: d.name.clone(),
                patterns: d.patterns.clone(),
                except: d.except.clone(),
                categories: d.categories.clone(),
                set,
            });
        }

        if out.iter().all(|r| r.set.entries.is_empty()) {
            return Err("around buildings: no rule has any props".to_owned());
        }

        Ok(out)
    }

    /// The buildings' categories of the packs' `battlefield_buildings` tables (read once for the game, its packs and the schema; empty
    /// without rpfm's schema, and the kit's tables are what is known then).
    pub(crate) fn building_categories(&mut self, e: &Editor) -> std::sync::Arc<std::collections::BTreeMap<String, String>> {
        let (files, schema) = (e.files(), crate::tool::walls_schema_path(e).ok());
        let source = (e.game().key.to_owned(), files.as_ref().map(|f| std::sync::Arc::as_ptr(f).cast::<()>() as usize), schema.clone());
        if let Some((_, c)) = self.coverage.categories.as_ref().filter(|(s, _)| *s == source) {
            return c.clone();
        }

        self.coverage.categories_note.clear();
        let mut out = std::collections::BTreeMap::new();
        match (files, schema) {
            (Some(files), Some(schema)) => {
                let tables =
                    files.list("db/battlefield_buildings_tables/", "").into_iter().filter_map(|p| files.read(&p).ok()).collect::<Vec<_>>();
                match wce_tile_io::attila::game_data::building_categories(&tables, &schema) {
                    Ok(c) => out = c,
                    Err(err) => self.coverage.categories_note = err,
                }
            }

            (_, None) => {
                self.coverage.categories_note = "no rpfm schema (schema_att.ron): only the kit's building categories are known".to_owned()
            }
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
    pub(crate) fn coverage_context(&mut self, e: &mut Editor) -> Result<std::sync::Arc<Context>, String> {
        let id = document_id(e).ok_or("no tile is open")?;
        let key = (id, document(e).ok_or("no tile is open")?.history().version());
        if let Some((_, c)) = self.coverage.context.as_ref().filter(|(k, _)| *k == key) {
            return Ok(c.clone());
        }

        let game = self.exported_game(e).ok();
        let d = document(e).ok_or("no tile is open")?;
        let started = std::time::Instant::now();
        let (f, raw) = crate::water_view::stage_b_fields(d, game.as_deref())
            .map_err(|err| format!("ground coverage: the tile's water and no-go areas: {err}"))?;
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
            started.elapsed().as_secs_f64(),
            m2(no_go),
            m2(wet),
            if c.has_sea { "by the sea" } else { "no sea" },
        ));

        let c = std::sync::Arc::new(c);
        self.coverage.context = Some((key, c.clone()));
        Ok(c)
    }

    /// Finds the differentials and what goes on them, with the lines that show it.
    fn compute_coverage(&mut self, e: &mut Editor) -> Result<Preview, String> {
        let (rocks, plants) = self.coverage_sets(e)?;
        let clutter = self.clutter_rules(e)?;
        let categories = self.building_categories(e);
        let context = self.coverage_context(e)?;
        let doc = document_id(e).ok_or("no tile is open")?;
        let d = document(e).ok_or("no tile is open")?;
        let (result, grid, unknown) =
            run(d, self.coverage.kind, &context, &rocks, &plants, &clutter, &categories, self.coverage.params, self.coverage.seed);
        let lines = preview_lines(d, &grid, &result);
        let owner = owners(&grid, &result);
        let mut summary = if self.coverage.kind == Kind::Buildings { buildings_summary(&result, &clutter) } else { summary_of(&result) };
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
        let clutter = self.clutter_rules(e)?;
        let c = &self.coverage;
        let settings = CoverageSettings {
            params: c.params,
            seed: c.seed,
            rocks: c.rocks.clone(),
            plants: c.plants.clone(),
            preset: c.preset.clone(),
            clutter: c.clutter.clone(),
        };
        let categories = self.building_categories(e);
        let context = self.coverage_context(e)?;
        let id = document_id(e).ok_or("ground coverage: no tile is open")?;
        let d = document(e).ok_or("ground coverage: no tile is open")?;
        let (result, _, _) = run(d, self.coverage.kind, &context, &rocks, &plants, &clutter, &categories, settings.params, settings.seed);
        let before = d.history().version();
        let c = d.apply_coverage(self.coverage.kind, &result, &settings).map_err(|err| format!("ground coverage failed: {err}"))?;
        let after = d.history().version();
        self.adopt_coverage_version(id, before, after);

        // What is in the tile now is the result: the preview would draw over it.
        self.coverage.preview = None;
        e.selection.clear();
        let group = kind_group_name(self.coverage.kind);
        if self.coverage.kind == Kind::Buildings {
            return Ok(format!(
                "ground coverage: {} prop(s) around {} building(s) in \"{COVERAGE_LAYER}\" > \"{group}\"",
                c.rocks, c.slopes
            ));
        }

        Ok(format!(
            "ground coverage: {} slope(s), {} rock(s), {} plant(s), {} exclusion polygon(s) in \"{COVERAGE_LAYER}\" > \"{group}\"",
            c.slopes, c.rocks, c.plants, c.exclusions
        ))
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
                    "Slope {}: rise {:.1} m, {:.0} m2; {} rock(s), {} plant(s){}",
                    d.number,
                    d.rise,
                    d.area,
                    d.rocks.len(),
                    d.plants.len(),
                    if d.avoided == 0 { String::new() } else { format!("; goes around {} building(s)", d.avoided) },
                ),
                Some(State::TooLow(r)) => format!("Slope {}: left alone, too low ({r:.1} m, the least is {:.1} m)", d.number, pr.min_rise),
                Some(State::TooSmall(a)) => {
                    format!("Slope {}: left alone, too small ({a:.0} m2, the least is {:.0} m2)", d.number, pr.min_area)
                }
                Some(State::ByWater) => format!("Slope {}: left alone, its foot is at the sea or in a river (Coast, Riverbanks)", d.number),
                None => String::new(),
            });
        }
    }
}

/// What the generator needs of the tile for a rock set: its grid, the buildings it goes around (of the avoided categories, by `categories`:
/// the packs' tables, else the kit's), the rocks' sizes and the rock keys whose model was not found.
pub(crate) fn tile_base(
    d: &TileDocument,
    rocks: &VegetationSet,
    categories: &std::collections::BTreeMap<String, String>,
) -> (Grid, Vec<Blocker>, HashMap<String, RockSize>, Vec<String>) {
    let grid = d.coverage_grid();
    let blockers = d.coverage_blockers(&|k: &str| categories.get(&k.to_lowercase()).cloned());
    let mut unknown = vec![];
    let sizes = rocks
        .entries
        .iter()
        .map(|en| {
            let s = d.rock_size(&en.key).unwrap_or_else(|| {
                unknown.push(en.key.clone());
                UNKNOWN_ROCK
            });
            (en.key.clone(), s)
        })
        .collect::<HashMap<_, _>>();

    (grid, blockers, sizes, unknown)
}

/// Runs the generator on the tile (around the buildings and the kind's brushed rocks and plants): the result, the grid, and the rock keys
/// whose model was not found.
#[allow(clippy::too_many_arguments)]
fn run(
    d: &TileDocument,
    kind: Kind,
    context: &Context,
    rocks: &VegetationSet,
    plants: &VegetationSet,
    clutter: &[wce_tile::ground_coverage::ClutterRule],
    categories: &std::collections::BTreeMap<String, String>,
    params: Params,
    seed: u64,
) -> (Vec<Differential>, Grid, Vec<String>) {
    let (grid, blockers, sizes, unknown) = tile_base(d, &with_clutter(rocks, clutter), categories);
    let targets = if kind == Kind::Buildings { d.coverage_targets(&|k: &str| categories.get(&k.to_lowercase()).cloned()) } else { vec![] };
    let keep_off = d
        .coverage_footprints(kind)
        .into_iter()
        .filter(|(_, brushed)| *brushed)
        .map(|(f, _)| Blocker { x: f.x, z: f.z, radius: if f.tree { params.plant_radius } else { f.radius } })
        .collect::<Vec<_>>();
    let size = |k: &str| sizes.get(k).copied().unwrap_or(UNKNOWN_ROCK);
    let result = generate(&Input {
        kind,
        grid: &grid,
        context: Some(context),
        blockers: &blockers,
        keep_off: &keep_off,
        rocks,
        plants,
        rock_size: &size,
        params,
        seed,
        targets: &targets,
        clutter,
    });
    (result, grid, unknown)
}

/// The rock set with every clutter rule's props: the keys whose sizes the generator asks for.
pub(crate) fn with_clutter(rocks: &VegetationSet, clutter: &[wce_tile::ground_coverage::ClutterRule]) -> VegetationSet {
    let mut all = rocks.clone();
    for e in clutter.iter().flat_map(|r| &r.set.entries) {
        if !all.entries.iter().any(|a| a.key == e.key) {
            all.entries.push(e.clone());
        }
    }

    all
}

/// Around buildings: "251 building(s) matched (Barbarian houses 200, Markets 3, ...): 242 with 1068 prop(s), 9 with no room; the most
/// placed: hay01 210, barrel 190, ...".
fn buildings_summary(result: &[Differential], clutter: &[wce_tile::ground_coverage::ClutterRule]) -> String {
    let mut by_rule = std::collections::BTreeMap::<usize, usize>::new();
    let mut by_key = HashMap::<&str, usize>::new();
    for d in result {
        *by_rule.entry(d.rule.unwrap_or(usize::MAX)).or_default() += 1;
        for r in &d.rocks {
            *by_key.entry(r.key.as_str()).or_default() += 1;
        }
    }

    let rules =
        by_rule.iter().map(|(&r, &n)| format!("{} {n}", clutter.get(r).map_or("?", |c| c.name.as_str()))).collect::<Vec<_>>().join(", ");
    let mut keys = by_key.into_iter().collect::<Vec<_>>();
    keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let keys = keys.iter().take(8).map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ");
    let kept = result.iter().filter(|d| d.kept() && !d.rocks.is_empty()).count();
    let props = result.iter().map(|d| d.rocks.len()).sum::<usize>();
    format!(
        "{} building(s) matched ({rules}): {kept} with {props} prop(s), {} with none (no room, or the rule has no props); the most placed: \
         {keys}.",
        result.len(),
        result.len() - kept
    )
}

/// "12 slopes: 9 covered (...), 3 left alone (...)".
fn summary_of(result: &[Differential]) -> String {
    let kept = result.iter().filter(|d| d.kept()).collect::<Vec<_>>();
    let count = |f: fn(&State) -> bool| result.iter().filter(|d| d.state.as_ref().is_some_and(f)).count();
    let (low, small) = (count(|s| matches!(s, State::TooLow(_))), count(|s| matches!(s, State::TooSmall(_))));

    // How low the too low ones are (what "rise at least" would have to come down to), from the lowest to the highest.
    let mut rises = result
        .iter()
        .filter_map(|d| match d.state {
            Some(State::TooLow(r)) => Some(r),
            _ => None,
        })
        .collect::<Vec<_>>();
    rises.sort_unstable_by(f32::total_cmp);
    let low = match (rises.first(), rises.get(rises.len() / 2), rises.last()) {
        (Some(a), Some(m), Some(b)) if rises.len() > 1 => format!("{low} too low ({a:.1} .. {b:.1} m, half under {m:.1} m)"),
        (Some(a), _, _) => format!("{low} too low ({a:.1} m)"),
        _ => format!("{low} too low"),
    };

    let water = count(|s| matches!(s, State::ByWater));
    let by_water = if water == 0 { String::new() } else { format!(", {water} by the water (Coast, Riverbanks)") };
    let around = kept.iter().filter(|d| d.avoided != 0).count();
    format!(
        "{} slope(s): {} covered with {} rock(s), {} plant(s) and {} exclusion polygon(s), {around} of them around buildings; left alone: {low}, \
         {small} too small{by_water}.",
        result.len(),
        kept.len(),
        kept.iter().map(|d| d.rocks.len()).sum::<usize>(),
        kept.iter().map(|d| d.plants.len()).sum::<usize>(),
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
    let (green, orange, red) =
        (overlay_colour(0.3, 0.9, 0.3, 1.0), overlay_colour(1.0, 0.6, 0.15, 1.0), overlay_colour(0.95, 0.2, 0.2, 1.0));
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
pub(crate) fn outline(g: &Grid, cells: &[usize], colour: u32, o: &mut Overlay) {
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

/// A kind's tab.
fn kind_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Cliffs => "Inland cliffs",
        Kind::Coast => "Coast",
        Kind::Riverbank => "Riverbanks",
        Kind::Buildings => "Around buildings",
    }
}

/// What a kind of terrain is, for its tooltip.
fn kind_tip(kind: Kind) -> &'static str {
    match kind {
        Kind::Cliffs => "Inland slopes and cliffs steeper than the angle, whose foot is over the sea and out of the rivers",
        Kind::Coast => {
            "The ground by the sea (under 0 m, reached from the tile's edge) that units can't walk; its bands measured from the sea"
        }
        Kind::Riverbank => "The ground by a river or a water plane that units can't walk; its bands measured from the water's surface",
        Kind::Buildings => "A ring around each building a clutter rule matches (by its key or category), with that rule's props",
    }
}

/// A row of the sets of a kind (and none) to pick from.
fn set_row(label: &str, hint: &str, sets: &[VegetationSet], kind: SetKind, chosen: &mut String) {
    let row = styled::row(label, "");
    let missing = !chosen.is_empty() && !sets.iter().any(|s| s.kind == kind && s.name == *chosen);
    let preview = if chosen.is_empty() {
        format!("none ({hint})")
    } else if missing {
        format!("{chosen} (not found)")
    } else {
        chosen.clone()
    };
    gui::set_next_item_width(row.width);
    if gui::begin_combo(&format!("##{label}"), &preview) {
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
}

/// Plain choices of a value: names and values.
type Choices = [(&'static str, f32); 4];
const SLOPES: Choices = [("Gentle", 25.0), ("Steep", 35.0), ("Very steep", 45.0), ("Cliffs", 60.0)];
const ROCKS_FOOT: Choices = [("None", 0.0), ("Few", 1.0 / 25.0), ("Some", 1.0 / 10.0), ("Many", 1.0 / 5.0)];
const ROCKS_FACE: Choices = [("None", 0.0), ("Few", 1.0 / 50.0), ("Some", 1.0 / 25.0), ("Many", 1.0 / 12.0)];
const PLANTS: Choices = [("None", 0.0), ("Few", 1.0 / 30.0), ("Some", 1.0 / 15.0), ("Many", 1.0 / 8.0)];
const PROPS: Choices = [("Few", 1.0 / 25.0), ("Some", 1.0 / 12.0), ("Many", 1.0 / 6.0), ("Packed", 1.0 / 3.0)];
const RINGS: Choices = [("Tight", 1.5), ("Medium", 3.0), ("Wide", 6.0), ("Very wide", 10.0)];

/// A value's row: its plain choices, or (advanced) a slider. `per100`: a density per square metre, shown per 100.
#[allow(clippy::too_many_arguments)]
fn value_row(
    id: &str,
    label: &str,
    tip: &str,
    v: &mut f32,
    choices: Option<&Choices>,
    range: (f32, f32),
    format: &str,
    advanced: bool,
) -> bool {
    let per100 = format.contains("100 m2");
    let row = styled::row(label, tip);
    match choices.filter(|_| !advanced) {
        Some(c) => {
            let names = c.map(|c| c.0);
            let current = c.iter().position(|c| (c.1 - *v).abs() < 1e-5);
            match styled::segmented(id, &names, current, row.width) {
                Some(i) => {
                    *v = c[i].1;
                    true
                }
                None => false,
            }
        }

        None => {
            let mut shown = if per100 { *v * 100.0 } else { *v };
            gui::set_next_item_width(row.width);
            let changed = gui::slider_float(&format!("##{id}"), &mut shown, range.0, range.1, format);
            if changed {
                *v = if per100 { shown / 100.0 } else { shown };
            }

            changed
        }
    }
}

/// A size range (smallest to largest) as a bar with two ends.
fn size_row(id: &str, label: &str, tip: &str, v: &mut (f32, f32)) -> bool {
    let row = styled::row(label, tip);
    let bar = styled::Bar { domain: (0.2, 3.0), step: 0.05, unit: "x", open_ends: false, fade: false };
    let mut r = wce_tile::texture_rules::Range { lo: Some(v.0), hi: Some(v.1), fade: 0.0 };
    let (changed, _) = styled::range_bar(id, &mut r, &bar, row.width, true);
    if changed {
        *v = (r.lo.unwrap_or(v.0), r.hi.unwrap_or(v.1));
    }

    changed
}

/// A note under a section, in the value column's width.
fn note(text: &str) {
    gui::push_text_wrap_pos(0.0);
    gui::text_disabled(text);
    gui::pop_text_wrap_pos();
}

/// The parameters' rows (plain choices, or every number when advanced); whether one changed.
fn params_rows(kind: Kind, p: &mut Params, advanced: bool) -> bool {
    let mut changed = false;
    if kind == Kind::Buildings {
        gui::separator_text("Ring");
        changed |= value_row(
            "ringwidth",
            "Width",
            "How wide the ring of props is",
            &mut p.ring_width,
            Some(&RINGS),
            (0.5, 30.0),
            "%.1f m",
            advanced,
        );
        changed |= value_row(
            "props",
            "Props",
            "Props per 100 square metres of a ring",
            &mut p.rock_density_foot,
            Some(&PROPS),
            (0.0, 100.0),
            "%.2f / 100 m2",
            advanced,
        );
        let row = styled::row("Most a building", "No building gets more props than this");
        let mut cap = p.max_per_building as i32;
        gui::set_next_item_width(row.width);
        if gui::slider_int("##cap", &mut cap, 0, 40) {
            p.max_per_building = cap.max(0) as f32;
            changed = true;
        }

        if advanced {
            changed |= value_row(
                "gap",
                "Gap",
                "Metres between a building's footprint and its ring; also kept from every other building",
                &mut p.ring_gap,
                None,
                (0.0, 10.0),
                "%.1f m",
                true,
            );
        }

        return changed;
    }

    gui::separator_text("Slopes");
    if kind.by_water() {
        note("Where units can't walk (the game's no-go areas, steeper than 30 deg), by the water.");
        if advanced {
            changed |= value_row(
                "reach",
                "From the water",
                "How far from the water the no-go ground may be to count",
                &mut p.water_reach,
                None,
                (0.0, 30.0),
                "%.1f m",
                true,
            );
            changed |= value_row(
                "below",
                "Under the water",
                "How deep into the water the waterline band goes (rocks only)",
                &mut p.water_below,
                None,
                (0.0, 5.0),
                "%.1f m",
                true,
            );
        }
    } else {
        changed |= value_row(
            "slope",
            "Steeper than",
            "A slope steeper than this is a differential",
            &mut p.slope_deg,
            Some(&SLOPES),
            (5.0, 85.0),
            "%.1f deg",
            advanced,
        );
        if advanced {
            changed |= value_row(
                "seamargin",
                "Foot over the sea",
                "A slope whose foot is lower is the coast's (and one whose foot is in a river, the banks')",
                &mut p.sea_margin,
                None,
                (0.0, 20.0),
                "%.1f m",
                true,
            );
        }
    }

    if advanced {
        changed |= value_row(
            "rise",
            "Rise at least",
            "A lower one (a ramp, a bump) is left alone",
            &mut p.min_rise,
            None,
            (0.0, 30.0),
            "%.1f m",
            true,
        );
        changed |= value_row("area", "Area at least", "A smaller one is left alone", &mut p.min_area, None, (0.0, 1000.0), "%.0f m2", true);

        gui::separator_text("Bands");
        changed |= value_row(
            "footup",
            "Foot: up the slope",
            "How far up the slope the foot band goes (rocks and plants)",
            &mut p.foot_up,
            None,
            (0.0, 20.0),
            "%.1f m",
            true,
        );
        changed |= value_row(
            "footout",
            "Foot: out on the flat",
            "How far out from the slope the foot band goes",
            &mut p.foot_out,
            None,
            (0.0, 20.0),
            "%.1f m",
            true,
        );
        changed |= value_row(
            "top",
            "Face: under the top",
            "The face band (rocks only) stops this far under the top",
            &mut p.top_margin,
            None,
            (0.0, 20.0),
            "%.1f m",
            true,
        );
    }

    gui::separator_text("Rocks");
    changed |= value_row(
        "rocksfoot",
        "At the foot",
        "Rocks per 100 square metres of the foot band",
        &mut p.rock_density_foot,
        Some(&ROCKS_FOOT),
        (0.0, 50.0),
        "%.2f / 100 m2",
        advanced,
    );
    changed |= value_row(
        "rocksface",
        "On the face",
        "Rocks per 100 square metres of the face band",
        &mut p.rock_density_face,
        Some(&ROCKS_FACE),
        (0.0, 50.0),
        "%.2f / 100 m2",
        advanced,
    );
    let row = styled::row("Scale", "");
    let mut scale = p.scale_rocks;
    if styled::toggle_chip(
        "scalerocks",
        "Scale the rocks",
        scale,
        row.width,
        "The game ignores a building's scale unless it is exported as a prop, so a scaled rock is marked Export as prop: plain models, \
         with no collision. Off: every rock a building at scale 1.",
    ) {
        scale = !scale;
        p.scale_rocks = scale;
        changed = true;
    }

    if p.scale_rocks {
        changed |= size_row("scalefoot", "Sizes at the foot", "The rocks' sizes at the foot, smallest to largest", &mut p.rock_scale_foot);
        changed |= size_row("scaleface", "Sizes on the face", "The rocks' sizes on the face, smallest to largest", &mut p.rock_scale_face);
        let row = styled::row("", "");
        gui::set_cursor_pos_x(row.x);
        gui::text_colored(theme::warning(), "Experimental: the game may show them at scale 1.");
    }

    if advanced {
        let row = styled::row(
            "Stands on",
            "The ground a rock's base is put on. The lowest never lets its downhill side float, but buries big rocks on steep faces.",
        );
        let names = ["Centre", "Mean", "Lowest"];
        let current = Seat::ALL.iter().position(|&s| s == p.seat);
        if let Some(i) = styled::segmented("seat", &names, current, row.width) {
            p.seat = Seat::ALL[i];
            changed = true;
        }

        let mut sink = p.sink * 100.0;
        changed |= value_row(
            "sink",
            "Sunk",
            "How much of each rock goes under the ground it stands on",
            &mut sink,
            None,
            (0.0, 90.0),
            "%.0f %% of its height",
            true,
        );
        p.sink = sink / 100.0;
        let mut visible = p.min_visible * 100.0;
        changed |= value_row(
            "visible",
            "At least visible",
            "How much of each rock stays over the ground at its centre, whatever the rest says",
            &mut visible,
            None,
            (0.0, 100.0),
            "%.0f %% of its height",
            true,
        );
        p.min_visible = visible / 100.0;
    }

    gui::separator_text("Plants");
    changed |= value_row(
        "plants",
        "At the foot",
        "Plants per 100 square metres of the foot band",
        &mut p.plant_density,
        Some(&PLANTS),
        (0.0, 50.0),
        "%.2f / 100 m2",
        advanced,
    );
    changed |= size_row("plantscale", "Sizes", "The plants' sizes, smallest to largest", &mut p.plant_scale);
    if advanced {
        changed |=
            value_row("apart", "Apart", "The least distance between two plants", &mut p.plant_spacing, None, (0.1, 20.0), "%.1f m", true);

        gui::separator_text("Buildings");
        changed |= value_row(
            "clearance",
            "Clearance",
            "What every rock and plant keeps from a building's footprint. Gone around: houses, fort walls, towers, gates, bridges; not \
             rocks, props or ground pieces.",
            &mut p.building_clearance,
            None,
            (0.0, 20.0),
            "%.1f m",
            true,
        );

        gui::separator_text("Vegetation exclusion");
        changed |= value_row(
            "clump",
            "Clump: plants within",
            "Plants this near are one clump, with one polygon around it",
            &mut p.cluster_distance,
            None,
            (0.0, 50.0),
            "%.1f m",
            true,
        );
        changed |= value_row(
            "clumpmax",
            "Clump: at most",
            "No clump gets wider (its polygon would cover the land between)",
            &mut p.max_cluster,
            None,
            (1.0, 200.0),
            "%.0f m across",
            true,
        );
        changed |= value_row(
            "margin",
            "Margin",
            "Around each plant (and its own radius) in the polygon",
            &mut p.exclusion_margin,
            None,
            (0.0, 20.0),
            "%.1f m",
            true,
        );
    }

    changed
}

/// Around buildings: the clutter rules, each a collapsing row (name, the buildings it matches, its props: a building set or keys); whether
/// any changed.
impl TileTool {
    fn clutter_rows(&mut self, e: &mut Editor, defs: &mut Vec<wce_tile::ground_coverage::ClutterDef>, sets: &[VegetationSet]) -> bool {
        let mut changed = false;
        gui::separator_text("Rules (the first that matches a building is its)");
        note(
            "A rule matches ordinary buildings by their key (patterns with *, several separated by |), less its exceptions, or the buildings \
         of its categories; its props go all around them.",
        );
        let (mut remove, mut up, mut down) = (None, None, None);
        let n = defs.len();
        for (i, d) in defs.iter_mut().enumerate() {
            gui::push_id_int(i as i32);
            let title = format!("{}###rule", if d.name.is_empty() { "(no name)" } else { &d.name });
            if gui::collapsing_header(&title, 0) {
                let mut text = |label: &str, id: &str, v: &mut String, hint: &str, tip: &str| {
                    let row = styled::row(label, tip);
                    gui::set_next_item_width(row.width);
                    changed |= gui::input_text(&format!("##{id}"), Some(hint), v, 256, 0);
                };
                text("Name", "name", &mut d.name, "", "");
                text(
                    "Keys like",
                    "patterns",
                    &mut d.patterns,
                    "*villa*|*farm*",
                    "Building keys with * for any run, several separated by |",
                );
                text("Except", "except", &mut d.except, "*_wall*|*tower*", "Keys left out, as above");
                text(
                    "Categories",
                    "categories",
                    &mut d.categories,
                    "generic (none: ordinary buildings)",
                    "The categories the rule looks among (gate|fort_tower ...); none: ordinary buildings, not walls, towers, gates or bridges",
                );

                let row = styled::row("Props", "A set of buildings of the projects, or keys of their own");
                let preview = if d.set.is_empty() { format!("keys ({})", d.keys.len()) } else { d.set.clone() };
                gui::set_next_item_width(row.width);
                if gui::begin_combo("##props", &preview) {
                    if gui::selectable("keys", d.set.is_empty(), 0) {
                        d.set.clear();
                        changed = true;
                    }

                    for s in sets.iter().filter(|s| s.kind == SetKind::Building) {
                        if gui::selectable(&s.name, d.set == s.name, 0) {
                            d.set.clone_from(&s.name);
                            changed = true;
                        }
                    }

                    gui::end_combo();
                }

                if d.set.is_empty() {
                    row.next_line();
                    changed |= self.key_list(e, "props", "BUILDING_INSTANCE", &mut d.keys, row.width, "incidental");
                }

                let row = styled::row("", "");
                gui::set_cursor_pos_x(row.x);
                gui::begin_disabled(i == 0);
                if styled::icon_button(icons::ARROW_BIG_UP, "up", "Earlier: tried before the rules after it") {
                    up = Some(i);
                }

                gui::end_disabled();
                gui::same_line();
                gui::begin_disabled(i + 1 == n);
                if styled::icon_button(icons::ARROW_BIG_DOWN, "down", "Later: tried after the rules before it") {
                    down = Some(i);
                }

                gui::end_disabled();
                gui::same_line();
                if styled::icon_button(icons::TRASH_2, "remove", "Remove the rule") {
                    remove = Some(i);
                }
            }

            gui::pop_id();
        }

        if let Some(i) = up.filter(|&i| i > 0) {
            defs.swap(i, i - 1);
            changed = true;
        }

        if let Some(i) = down.filter(|&i| i + 1 < defs.len()) {
            defs.swap(i, i + 1);
            changed = true;
        }

        if let Some(i) = remove {
            defs.remove(i);
            changed = true;
        }

        if gui::small_button(&format!("{}  Add a rule", icons::PLUS)) {
            defs.push(wce_tile::ground_coverage::ClutterDef { name: "New rule".to_owned(), ..Default::default() });
            changed = true;
        }

        changed
    }
}
