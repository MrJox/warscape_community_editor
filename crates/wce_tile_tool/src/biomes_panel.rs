//! The biomes' window (docs/plans/auto_detail.md, C; `wce_tile::biomes`, `tile_document::biomes`): opened from the Tile panel. The biome
//! presets (the built-in ones and the projects' files) and the chosen one's values (its trees, undergrowth and rocks as a set or keys,
//! their densities, the edge band, its texture rules), Save, Save as, Delete; **Draw an area** of it (an exclusion polygon with
//! `wce_biome`), or **Brush** (a left drag adds to the area under the mouse, or makes a new one; Ctrl + drag takes away); the selected
//! area's biome and seed.

use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_foundation::math::Float3;
use wce_scene::render_scene::{Overlay, overlay_colour};
use wce_tile::biomes::{BIOME_ATTR, BUILTIN_FOLDER, Biome, Props, SEED_ATTR, presets_root};
use wce_tile::texture_rules::Range;
use wce_tile::vegetation_sets::{SetKind, VegetationSet};
use wce_tile_editor::tile_document::Draft;

use crate::gesture::Gesture;
use crate::styled;
use crate::texture_rules_panel::RuleView;
use crate::tool::{TileMode, TileTool, document, document_id, primary};

/// The brush's radius at first, metres.
pub const DEFAULT_RADIUS: f32 = 12.0;

/// The window's state.
#[derive(Default)]
pub struct BiomesUi {
    pub open: bool,

    /// The biome being worked with: a copy of a preset's (or the tile's), its values as edited.
    pub draft: Option<Biome>,
    save_as: Option<String>,
    delete_ask: bool,

    /// Brush mode, its radius (0: the default), the stroke under way.
    pub brush: bool,
    pub radius: f32,
    pub(crate) stroke: Option<BiomeStroke>,

    /// Every number (advanced) or the plain choices.
    pub advanced: bool,
}

/// A brush stroke: taking away or adding, the area it works on (none: a new one), the dabs so far (world metres).
pub(crate) struct BiomeStroke {
    erase: bool,
    area: Option<usize>,
    dabs: Vec<(f32, f32)>,
}

impl TileTool {
    /// Opens the window (Tile panel, "Biomes...").
    pub fn open_biomes(&mut self) {
        self.biomes.open = true;
    }

    /// Whether a left drag in the viewport is the biome brush's: the window is open in Brush mode and nothing else is being placed or
    /// drawn (Select mode).
    pub(crate) fn biome_brushing(&self, e: &Editor) -> bool {
        self.biomes.open
            && self.biomes.brush
            && self.mode == TileMode::Select
            && self.placing.is_none()
            && !self.drawing
            && document_id(e).is_some()
    }

    pub(crate) fn biome_radius(&self) -> f32 {
        if self.biomes.radius > 0.0 { self.biomes.radius } else { DEFAULT_RADIUS }
    }

    /// The biome being worked with: the draft, else the first preset.
    fn biome_draft(&mut self, e: &mut Editor) -> Biome {
        if self.biomes.draft.is_none() {
            self.biomes.draft = self.biome_presets(e).first().cloned();
        }

        self.biomes.draft.clone().unwrap_or_default()
    }

    /// A stroke begins at the cursor: on the area under it (Ctrl: taking away from it), else a new area of the draft's biome.
    pub(crate) fn begin_biome_stroke(&mut self, e: &mut Editor, erase: bool) {
        let at = (self.cursor.x, self.cursor.z);
        let area = document(e).and_then(|d| {
            let areas = d.tile().biome_areas();
            areas
                .iter()
                .find(|a| wce_tile::biomes::inside(&a.points, at.0, at.1))
                .or_else(|| {
                    areas.iter().filter(|a| wce_tile::biomes::boundary_distance(&a.points, at.0, at.1) < self.biome_radius()).min_by(
                        |a, b| {
                            let d = |x: &&wce_tile_editor::tile_document::biomes::BiomeArea| {
                                wce_tile::biomes::boundary_distance(&x.points, at.0, at.1)
                            };
                            d(a).total_cmp(&d(b))
                        },
                    )
                })
                .map(|a| a.elem)
        });
        if erase && area.is_none() {
            self.status = "no biome area under the brush to take away from".to_owned();
            return;
        }

        self.biomes.stroke = Some(BiomeStroke { erase, area, dabs: vec![at] });
        self.gesture = Some(Gesture::BiomeStroke);
    }

    /// A dab at the cursor, a quarter of the radius from the last one at least.
    pub(crate) fn biome_dab(&mut self) {
        let r = self.biome_radius();
        let at = (self.cursor.x, self.cursor.z);
        if let Some(s) = self.biomes.stroke.as_mut()
            && s.dabs.last().is_none_or(|l| (l.0 - at.0).hypot(l.1 - at.1) >= r * 0.25)
        {
            s.dabs.push(at);
        }
    }

    /// The stroke into the tile (`TileDocument::biome_stroke`, one undo entry); a new area's biome taken in first.
    ///
    /// # Errors
    ///
    /// No tile, or the stroke made nothing.
    pub(crate) fn end_biome_stroke(&mut self, e: &mut Editor) -> Result<String, String> {
        let Some(stroke) = self.biomes.stroke.take() else { return Ok(String::new()) };
        let biome = self.biome_draft(e);
        let radius = self.biome_radius();
        let d = document(e).ok_or("no tile is open")?;
        let into = self.new_items_layer(d).ok_or("no layer to put a new area in")?;
        if stroke.area.is_none() {
            d.adopt_biome(&biome);
        }

        let areas = d.biome_stroke(stroke.area, into, &biome.name, &stroke.dabs, radius, stroke.erase)?;
        e.selection.clear();
        Ok(match (stroke.erase, stroke.area, areas.len()) {
            (true, _, 0) => "biome brush: the area went".to_owned(),
            (true, _, n) => format!("biome brush: taken away ({n} area(s) left of it)"),
            (false, None, _) => format!("biome brush: a new area of {}", biome.name),
            (false, Some(_), _) => "biome brush: the area grown".to_owned(),
        })
    }

    /// The stroke's dabs as circles while it lasts.
    pub(crate) fn biome_brush_overlay(&self, d: &wce_tile_editor::tile_document::TileDocument, o: &mut Overlay) {
        let Some(s) = &self.biomes.stroke else { return };
        let colour = if s.erase { overlay_colour(1.0, 0.3, 0.2, 1.0) } else { overlay_colour(0.3, 1.0, 0.4, 1.0) };
        let r = self.biome_radius();
        for &(x, z) in &s.dabs {
            let mut prev = None;
            for i in 0..=24 {
                let a = i as f32 * std::f32::consts::TAU / 24.0;
                let (px, pz) = (x + a.cos() * r, z + a.sin() * r);
                let p = Float3::new(px, d.tile().terrain_height(px, pz) + 0.4, pz);
                if let Some(q) = prev {
                    o.line(q, p, colour);
                }

                prev = Some(p);
            }
        }
    }

    pub(crate) fn draw_biomes_window(&mut self, e: &mut Editor) {
        if !self.biomes.open {
            return;
        }

        if document_id(e).is_none() {
            self.biomes.open = false;
            return;
        }

        gui::set_next_window_size(gui::v2(460.0, 700.0), gui::COND_FIRST_USE_EVER);
        let mut open = true;
        if !gui::begin(&format!("{}  Biomes (experimental)###biomes", icons::SHRUB), Some(&mut open), 0) {
            gui::end();
            self.biomes.open = open;
            return;
        }

        gui::text_wrapped(
            "A biome is trees, undergrowth, rocks and a texture laid in areas. An area is a vegetation exclusion polygon: the game's own trees \
             give way to the biome's there. Its contents are made again whenever it, its biome or the ground changes.",
        );
        gui::spacing();

        let row = styled::row("Show", "The plain choices, or every number");
        if let Some(i) = styled::segmented("mode", &["Simple", "Advanced"], Some(usize::from(self.biomes.advanced)), row.width) {
            self.biomes.advanced = i == 1;
        }

        self.biome_preset_rows(e);
        let mut draft = self.biome_draft(e);
        let sets = self.vegetation_sets(e);
        let swatches = self.ground_swatches(e);
        let (groups, heights) =
            document(e).map(|d| (d.texture_group_slots(), crate::texture_rules_panel::tile_heights(d))).unwrap_or((vec![], (0.0, 10.0)));
        let view = RuleView { groups: &groups, swatches: &swatches, heights, advanced: self.biomes.advanced };
        let changed = biome_fields(&mut draft, &sets, &view);
        if changed {
            self.biomes.draft = Some(draft.clone());
        }

        gui::separator_text("Lay it");
        let half = (gui::content_region_avail().x - gui::item_spacing().x) * 0.5;
        if gui::button_sized(&format!("{}  Draw an area", icons::PLUS), gui::v2(half, 0.0)) {
            if let Some(d) = document(e) {
                d.adopt_biome(&draft);
            }

            let kind = wce_tile::biomes::AREA_TYPE;
            let mut d = Draft::new(kind, "");
            d.attrs.insert(BIOME_ATTR.to_owned(), draft.name.clone());
            d.attrs.insert(SEED_ATTR.to_owned(), ((self.random.unit() * 1.0e6) as u32).to_string());
            d.attrs.insert("name".to_owned(), draft.name.clone());
            self.draft = Some(d);
            self.start_drawing();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Click its points on the ground, Enter or a double click to close it");
        }

        gui::same_line();
        if styled::toggle_chip(
            "brush",
            "Brush",
            self.biomes.brush,
            half,
            "Left drag in the viewport (Select mode): adds to the area under the mouse, or makes a new one; Ctrl + drag takes away",
        ) {
            self.biomes.brush = !self.biomes.brush;
        }

        if self.biomes.brush {
            let row = styled::row("Radius", "The brush's radius (Ctrl + click to type)");
            let mut r = self.biome_radius();
            gui::set_next_item_width(row.width);
            if gui::slider_float("##radius", &mut r, 1.0, 100.0, "%.1f m") {
                self.biomes.radius = r;
            }
        }

        self.selected_area_rows(e, &draft);

        if let Some(d) = document(e) {
            let (trees, rocks) = d.tile().biome_item_counts();
            let areas = d.tile().biome_areas().len();
            gui::spacing();
            gui::text_disabled(&format!("{areas} area(s): {trees} tree(s) and plant(s), {rocks} rock(s)"));
        }

        gui::end();
        self.biomes.open = open;
    }

    /// The presets: a combo, Save, Save as, Delete.
    fn biome_preset_rows(&mut self, e: &mut Editor) {
        let presets = self.biome_presets(e);
        let draft = self.biome_draft(e);
        let label = |b: &Biome| {
            if b.folder.is_empty() || b.folder == BUILTIN_FOLDER { b.name.clone() } else { format!("{}   {}", b.name, b.folder) }
        };
        let current = presets.iter().find(|b| b.name == draft.name).cloned();
        let shown = match &current {
            Some(b) if same_values(b, &draft) => label(b),
            Some(b) => format!("{} (changed)", label(b)),
            None => format!("{} (the tile's)", draft.name),
        };

        let mut pick = None;
        let row = styled::row("Biome", "The built-in biomes and the projects' (raw_data/art/biomes)");
        let buttons =
            [icons::SAVE, icons::FILE_PLUS, icons::TRASH_2].iter().map(|i| gui::button_width(i)).sum::<f32>() + 3.0 * gui::item_spacing().x;
        gui::set_next_item_width((row.width - buttons).max(100.0));
        if gui::begin_combo("##biome", &shown) {
            for b in presets.iter() {
                let text = if b.folder == BUILTIN_FOLDER { format!("{} (built in)", b.name) } else { label(b) };
                if gui::selectable(&text, b.name == draft.name, 0) {
                    pick = Some(b.clone());
                }
            }

            gui::end_combo();
        }

        let file = current.as_ref().is_some_and(|b| b.path.is_some());
        let mut result: Option<Result<String, String>> = None;
        if self.biomes.save_as.is_none() && !self.biomes.delete_ask {
            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##savebiome", icons::SAVE)) {
                result = Some(self.save_biome(e, None));
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Save the biome into its file (a built-in one has none: Save as)");
            }

            gui::same_line();
            if gui::button(&format!("{}##saveasbiome", icons::FILE_PLUS)) {
                self.biomes.save_as = Some(String::new());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Save as a new biome...");
            }

            gui::same_line();
            gui::begin_disabled(!file);
            if gui::button(&format!("{}##deletebiome", icons::TRASH_2)) {
                self.biomes.delete_ask = true;
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip("Delete the biome's file");
            }
        }

        if let Some(b) = pick {
            self.biomes.draft = Some(b);
        }

        if let Some(mut name) = self.biomes.save_as.take() {
            gui::set_next_item_width(gui::content_region_avail().x - gui::button_width("Save") - gui::button_width("Cancel") - 24.0);
            let enter = gui::input_text("##biomename", Some("the new biome's name"), &mut name, 128, gui::INPUT_ENTER_RETURNS_TRUE);
            gui::same_line();
            let ok = gui::small_button("Save") || enter;
            gui::same_line();
            if ok && !name.trim().is_empty() {
                result = Some(self.save_biome(e, Some(name.trim())));
            } else if !gui::small_button("Cancel") {
                self.biomes.save_as = Some(name);
            }
        } else if self.biomes.delete_ask {
            gui::text_colored(theme::warning(), &format!("Delete the biome '{}'?", draft.name));
            gui::same_line();
            if gui::small_button("Delete") {
                result = Some(self.delete_biome(e, &draft));
                self.biomes.delete_ask = false;
            }

            gui::same_line();
            if gui::small_button("Cancel") {
                self.biomes.delete_ask = false;
            }
        }

        if let Some(r) = result {
            self.status = r.unwrap_or_else(|err| err);
        }
    }

    /// The draft into a preset's file: its own (`None`) or a new one.
    fn save_biome(&mut self, e: &mut Editor, new_name: Option<&str>) -> Result<String, String> {
        let root = presets_root(&Self::workspace_dir(e).join("raw_data"));
        let mut b = self.biome_draft(e);
        if let Some(name) = new_name {
            if name.contains(['/', '\\']) || wce_tile::biomes::builtin().iter().any(|x| x.name == name) {
                return Err(format!("'{name}' can't be a biome's name"));
            }

            b.name = name.to_owned();
            b.folder.clear();
            b.path = Some(root.join(format!("{name}.wce")));
        }

        b.save()?;
        self.biome_presets = None;
        self.biomes.draft = Some(b.clone());
        Ok(format!("biome '{}' saved", b.name))
    }

    fn delete_biome(&mut self, e: &mut Editor, b: &Biome) -> Result<String, String> {
        let path = b.path.clone().ok_or("a built-in biome can't be deleted")?;
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        self.biome_presets = None;
        self.biomes.draft = self.biome_presets(e).first().cloned();
        Ok(format!("biome '{}' deleted", b.name))
    }

    /// The selected area's biome (the draft given to it, or the tile's copy updated from the draft) and seed.
    fn selected_area_rows(&mut self, e: &mut Editor, draft: &Biome) {
        let Some(sel) = primary(e) else { return };
        let Some(d) = document(e) else { return };
        let Some(area) = d.tile().biome_areas().into_iter().find(|a| a.elem == sel) else { return };
        gui::separator_text("The selected area");
        gui::text(&format!("{}, seed {}", area.biome, area.seed));
        let differs = d.tile().biome_named(&area.biome).is_none_or(|b| !same_values(&b, draft) || b.name != draft.name);
        gui::begin_disabled(!differs);
        if gui::button(&format!("Give it {}", draft.name)) {
            d.adopt_biome(draft);
            d.set_attribute(sel, BIOME_ATTR, &draft.name);
        }

        gui::end_disabled();
        if gui::is_item_hovered() {
            gui::set_tooltip("The area takes the biome as the window has it (every area of that biome in the tile follows)");
        }

        gui::same_line();
        if gui::button(&format!("{}  Reroll", icons::REFRESH_CW)) {
            let seed = ((self.random.unit() * 1.0e6) as u32).to_string();
            if let Some(d) = document(e) {
                d.set_attribute(sel, SEED_ATTR, &seed);
            }
        }
    }
}

/// Whether two biomes have the same values (their files aside).
fn same_values(a: &Biome, b: &Biome) -> bool {
    Biome { folder: String::new(), path: None, ..a.clone() } == Biome { folder: String::new(), path: None, ..b.clone() }
}

/// A density's plain choices, per 100 square metres.
type Choices = [(&'static str, f32); 4];
const TREES: Choices = [("Scattered", 0.5), ("Open", 1.25), ("Wood", 2.0), ("Dense", 3.0)];
const UNDERGROWTH: Choices = [("Little", 1.0), ("Some", 3.0), ("Lots", 6.0), ("Thick", 15.0)];
const ROCKS: Choices = [("None", 0.0), ("Few", 0.1), ("Some", 0.4), ("Many", 1.0)];

/// The edge band's plain choices, metres.
const EDGES: Choices = [("Hard", 0.0), ("Narrow", 6.0), ("Medium", 12.0), ("Wide", 24.0)];

/// A density row: plain choices, or (advanced) a slider; per square metre, shown per 100.
fn density_row(id: &str, label: &str, tip: &str, v: &mut f32, choices: &Choices, advanced: bool) -> bool {
    let row = styled::row(label, tip);
    let mut per100 = *v * 100.0;
    if advanced {
        gui::set_next_item_width(row.width);
        if gui::slider_float(&format!("##{id}"), &mut per100, 0.0, 20.0, "%.2f / 100 m2") {
            *v = per100 / 100.0;
            return true;
        }

        return false;
    }

    let names = choices.map(|c| c.0);
    let current = choices.iter().position(|c| (c.1 - per100).abs() < 1e-4);
    if let Some(i) = styled::segmented(id, &names, current, row.width) {
        *v = choices[i].1 / 100.0;
        return true;
    }

    false
}

/// A slider row of the advanced view.
fn slider_row(id: &str, label: &str, tip: &str, v: &mut f32, lo: f32, hi: f32, format: &str) -> bool {
    let row = styled::row(label, tip);
    gui::set_next_item_width(row.width);
    gui::slider_float(&format!("##{id}"), v, lo, hi, format)
}

/// A size range (smallest to largest) as a bar with two ends.
fn size_row(id: &str, label: &str, tip: &str, v: &mut (f32, f32)) -> bool {
    let row = styled::row(label, tip);
    let bar = styled::Bar { domain: (0.3, 2.0), step: 0.05, unit: "x", open_ends: false, fade: false };
    let mut r = Range { lo: Some(v.0), hi: Some(v.1), fade: 0.0 };
    let (changed, _) = styled::range_bar(id, &mut r, &bar, row.width, true);
    if changed {
        *v = (r.lo.unwrap_or(v.0), r.hi.unwrap_or(v.1));
    }

    changed
}

/// A biome's fields in rows; whether any changed.
fn biome_fields(b: &mut Biome, sets: &[VegetationSet], view: &RuleView) -> bool {
    let mut changed = false;
    let advanced = view.advanced;
    gui::separator_text("What grows");
    changed |= props_row("Trees", &mut b.trees, sets, SetKind::Vegetation);
    changed |= props_row("Undergrowth", &mut b.undergrowth, sets, SetKind::Vegetation);
    changed |= props_row("Rocks", &mut b.rocks, sets, SetKind::Building);

    gui::separator_text("How much");
    changed |=
        density_row("treesd", "Trees", "Trees per 100 square metres, inside past the edge band", &mut b.tree_density, &TREES, advanced);
    changed |= density_row(
        "undergrowthd",
        "Undergrowth",
        "Plants of the undergrowth per 100 square metres, inside",
        &mut b.undergrowth_density,
        &UNDERGROWTH,
        advanced,
    );
    changed |= density_row("rocksd", "Rocks", "Rocks per 100 square metres", &mut b.rock_density, &ROCKS, advanced);
    changed |= size_row("treesize", "Tree sizes", "The trees' sizes, smallest to largest (times the model's)", &mut b.tree_scale);
    if advanced {
        changed |= size_row("undersize", "Plant sizes", "The undergrowth's sizes, smallest to largest", &mut b.undergrowth_scale);
        changed |=
            slider_row("spacing", "Tree spacing", "How far apart the trees stand at least", &mut b.tree_spacing, 0.0, 30.0, "%.1f m");
    }

    gui::separator_text("Edge");
    let tip = "How far in from the area's boundary the edge reaches: smaller, fewer trees and more undergrowth there; the texture fades \
               over it too";
    if advanced {
        changed |= slider_row("edge", "Edge band", tip, &mut b.edge, 0.0, 100.0, "%.1f m");
        changed |= slider_row(
            "edgesize",
            "Trees' size there",
            "At the boundary the trees are this much of their size",
            &mut b.edge_tree_scale,
            0.05,
            2.0,
            "x %.2f",
        );
        changed |= slider_row(
            "edgetrees",
            "Trees there",
            "At the boundary there are this much as many trees",
            &mut b.edge_tree_density,
            0.0,
            1.0,
            "x %.2f",
        );
        changed |= slider_row(
            "edgeunder",
            "Undergrowth there",
            "At the boundary the undergrowth is this much as dense",
            &mut b.edge_undergrowth,
            0.0,
            10.0,
            "x %.2f",
        );
    } else {
        let row = styled::row("Edge band", tip);
        let names = EDGES.map(|c| c.0);
        let current = EDGES.iter().position(|c| (c.1 - b.edge).abs() < 1e-4);
        if let Some(i) = styled::segmented("edge", &names, current, row.width) {
            b.edge = EDGES[i].1;
            changed = true;
        }
    }

    gui::separator_text("Texture");
    let mut remove = None;
    for (i, r) in b.rules.iter_mut().enumerate() {
        gui::push_id_int(i as i32);
        let title = if r.name.is_empty() { "(rule)".to_owned() } else { r.name.clone() };
        if gui::collapsing_header(&format!("{title}###rule"), 0) {
            changed |= crate::texture_rules_panel::rule_fields(&format!("rule{i}"), r, view);
            if gui::small_button(&format!("{}  Remove this rule", icons::TRASH_2)) {
                remove = Some(i);
            }
        }

        gui::pop_id();
    }

    if let Some(i) = remove {
        b.rules.remove(i);
        changed = true;
    }

    if gui::small_button(&format!("{}  Add a texture rule", icons::PLUS)) {
        b.rules.push(wce_tile::texture_rules::Rule {
            name: "Ground".to_owned(),
            group: view.groups.first().cloned().unwrap_or_default(),
            ..Default::default()
        });
        changed = true;
    }

    changed
}

/// What grows of one sort: a set (of its kind) or keys (on a line of their own).
fn props_row(label: &str, p: &mut Props, sets: &[VegetationSet], kind: SetKind) -> bool {
    let mut changed = false;
    gui::push_id(label);
    let row = styled::row(label, "A vegetation set of the projects, or keys of their own");
    let preview = if p.set.is_empty() { format!("keys ({})", p.keys.len()) } else { p.set.clone() };
    gui::set_next_item_width(row.width);
    if gui::begin_combo("##set", &preview) {
        if gui::selectable("keys", p.set.is_empty(), 0) {
            p.set.clear();
            changed = true;
        }

        for s in sets.iter().filter(|s| s.kind == kind) {
            if gui::selectable(&s.name, p.set == s.name, 0) {
                p.set.clone_from(&s.name);
                changed = true;
            }
        }

        gui::end_combo();
    }

    if p.set.is_empty() {
        row.next_line();
        let mut keys = p.keys.join(" ");
        gui::set_next_item_width(row.width);
        if gui::input_text("##keys", Some("oak.oak_g beech.beech_e"), &mut keys, 4096, 0) {
            p.keys = keys.split_whitespace().map(str::to_owned).collect();
            changed = true;
        }
    }

    gui::pop_id();
    changed
}

/// The sets for the document (each frame: the areas' named props are looked for in them).
pub(crate) fn feed_sets(tool: &mut TileTool, e: &mut Editor) {
    let sets: Arc<Vec<VegetationSet>> = tool.vegetation_sets(e);
    if let Some(d) = document(e) {
        d.set_biome_sets(sets);
    }
}
