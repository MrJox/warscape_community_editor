//! The Prefab tool (docs/plans/prefab_tool.md): prefabs edited on a stage of their own, as the Weather tool edits weathers. It is a Tile
//! tool in its prefab profile (`Profile::Prefab`: no Tile panel, no tile browser, no minimap, no texture painting) over a flat stage tile
//! (`wce_tile::prefab_stage`) holding the prefab's group; the prefab's height layer is the stage's `Prefab ground` layer, and the Ground
//! brush paints its footprint. Saving the stage (Ctrl+S, the Prefab menu) writes the prefab into the projects' `art/prefabs` (`.wce`, with
//! its height layer), never the stage's tile.
//!
//! The stage is a document of this tool: the Tile tool's commands reach this tool's Tile tool while it is the active one (`tool::with_tile`),
//! and each tool sees its own tiles (`tool::document_id`). Activated, it shows its stage and hides the tiles (they are where the stage is); the
//! Tile tool, activated, puts them back.

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wce_editor::editor::Editor;
use wce_editor::tool::{Tool, ViewportEvent};
use wce_editor::{gui, icons};
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_scene::render_scene::{DocumentId, SourceRef};
use wce_tile::prefab_stage::{STAGE_ATTR, StageSettings};
use wce_tile::prefab_terrain::DEFAULT_FEATHER;
use wce_tile::prefabs::{self, PrefabLibrary};
use wce_tile_editor::tile_document::TileDocument;

use crate::tool::{Profile, TILE_TOOL_ID, TileTool};

/// The tool's id: the owner of its stages.
pub const PREFAB_TOOL_ID: &str = "prefab";

/// The stage group's attributes for the prefab's height layer: whether it normalizes the ground, and its feather.
pub const NORMALIZE_ATTR: &str = "wce_stage_normalize";
pub const FEATHER_ATTR: &str = "wce_stage_feather";

/// What the stage's ground's size was found for: the stage, its history's and terrain's versions, the flag and the feather's bits.
type SizeKey = (DocumentId, u64, bool, u32);

/// The prefab open on the stage.
struct Open {
    doc: DocumentId,
    key: String,

    /// Set by the stage's save: the libraries are scanned again, and the open tiles' copies follow.
    saved: Arc<AtomicBool>,

    /// Its contents as the file had them (references filled in) when opened or last saved: what the tiles' copies differ from.
    old: Vec<u8>,
}

pub struct PrefabTool {
    /// The Tile tool the stage is edited with.
    pub(crate) tile: TileTool,
    stage: StageSettings,
    open: Option<Open>,

    /// The tiles hidden while the stage shows (the Tile tool shows them again).
    pub(crate) hidden: Vec<DocumentId>,

    /// The stage's ground's size, and what it was found for (the stage, its history's and terrain's versions, the flag and feather).
    ground_size: (SizeKey, Option<(f64, f64)>),

    // The Prefabs panel.
    filter: String,
    new_name: String,
    new_folder: String,
    status: String,
}

impl Default for PrefabTool {
    fn default() -> Self {
        let mut tile = TileTool::default();
        tile.profile = Profile::Prefab;
        Self { tile, stage: StageSettings::default(), open: None, hidden: vec![], ground_size: ((0, 0, false, 0), None), filter: String::new(), new_name: String::new(), new_folder: String::new(), status: String::new() }
    }
}

/// A group with nothing in it, as a new prefab's.
fn empty_group(name: &str) -> String {
    let name = String::from_utf8_lossy(&wce_formats::xml_doc::encode_entities(name.as_bytes(), b'"')).into_owned();
    format!("<UserInstanceGroup flags=\" OIF_VISIBLE|OIF_ACTIVE\" position=\"vector(0,0,0)\" name=\"{name}\" orientation=\"euler(0,0,0)\">\n    <children>\n    </children>\n</UserInstanceGroup>\n")
}

impl PrefabTool {
    /// The projects' prefab folder, where every save goes.
    fn projects_prefabs(e: &mut Editor) -> PathBuf {
        prefabs::prefab_root(&TileTool::workspace_dir(e).join("raw_data"))
    }

    /// The stage's folder: the editor's own, in the projects.
    fn stage_root(e: &mut Editor) -> PathBuf {
        TileTool::workspace_dir(e).join(".prefab_stage")
    }

    fn library(&mut self, e: &mut Editor) -> Arc<PrefabLibrary> {
        self.tile.prefab_library(e)
    }

    /// The stage document, when one is open.
    fn stage<'a>(&self, e: &'a mut Editor) -> Option<&'a mut TileDocument> {
        let id = self.open.as_ref()?.doc;
        e.docs.find_as_mut::<TileDocument>(id)
    }

    /// The key of the prefab open on the stage.
    #[must_use]
    pub fn open_key(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.key.as_str())
    }

    /// Opens a prefab on the stage (the one there goes, if it has nothing unsaved).
    ///
    /// # Errors
    ///
    /// The one there has unsaved changes, no such prefab, or the stage can't be made.
    pub fn open_prefab(&mut self, e: &mut Editor, key: &str) -> Result<String, String> {
        self.close_stage(e, false)?;
        let lib = self.library(e);
        let entry = lib.get(key).ok_or_else(|| format!("no prefab '{key}'"))?.clone();
        let prefab = lib.load(&entry.key)?;
        let terrain = lib.terrain(&entry.key)?;

        let mut project = wce_tile::prefab_stage::make(&Self::stage_root(e), &self.stage, &prefab.group_text)?;
        let group = project.scene.find_all("UserInstanceGroup").into_iter().find(|&g| project.scene.doc().get(g, STAGE_ATTR).is_some()).ok_or("the stage lost its group")?;
        let (normalize, feather) = terrain.as_ref().map_or((false, DEFAULT_FEATHER), |t| (t.normalize, t.feather));
        project.scene.set_attr(group, NORMALIZE_ATTR, if normalize { "true" } else { "false" });
        project.scene.set_attr(group, FEATHER_ATTR, &wce_tile::ted_scene::format_number(feather));

        let mut doc = self.tile.make_document(e, project)?;
        doc.begin_stage(terrain.as_ref())?;
        let saved = Arc::new(AtomicBool::new(false));
        let projects = Self::projects_prefabs(e);
        let (save_key, flag) = (entry.key.clone(), saved.clone());
        doc.set_stage(&format!("Prefab {}", entry.key), Box::new(move |d: &mut TileDocument| {
            let group = d.stage_group().ok_or("the stage has no prefab group")?;
            let doc = d.tile().tscene().doc();
            let normalize = doc.get(group, NORMALIZE_ATTR).is_some_and(|v| v == "true");
            let feather = doc.get(group, FEATHER_ATTR).and_then(|v| v.parse::<f64>().ok()).unwrap_or(DEFAULT_FEATHER).max(0.0);
            let terrain = d.stage_terrain(normalize, feather);
            let path = lib.save_with_terrain(&save_key, &projects, &d.children_text(group), terrain.as_ref())?;
            flag.store(true, Ordering::Relaxed);
            Ok(format!("prefab saved to {}{}", path.display(), if terrain.is_some() { " with its ground" } else { "" }))
        }));

        let id = e.docs.add(&mut e.scene, Box::new(doc), PREFAB_TOOL_ID);
        e.docs.set_active(id);
        self.open = Some(Open { doc: id, key: entry.key.clone(), saved, old: prefab.children_text });
        self.show_stage(e);
        e.pending_focus = Some("Scene".to_owned());
        Ok(format!("{} on the stage{}", entry.key, if entry.wce { "" } else { " (from the Assembly Kit: a save makes the projects' copy)" }))
    }

    /// Writes a new empty prefab into the projects (`folder` below `art/prefabs`) and opens it.
    ///
    /// # Errors
    ///
    /// No name, a prefab of that key exists, or it can't be written or opened.
    pub fn new_prefab(&mut self, e: &mut Editor, name: &str, folder: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', '.']) {
            return Err("a prefab's name: no slashes or dots".to_owned());
        }

        if self.library(e).get(name).is_some() {
            return Err(format!("a prefab '{name}' exists"));
        }

        let root = Self::projects_prefabs(e);
        let folder = folder.trim().trim_matches(['/', '\\']);
        let path = if folder.is_empty() { root.join(format!("{name}.wce")) } else { root.join(folder).join(format!("{name}.wce")) };
        prefabs::write(&path, empty_group(name).as_bytes(), None)?;
        self.rescan(e);
        self.open_prefab(e, name).map(|m| format!("new prefab {name}: {m}"))
    }

    /// Closes the stage. Refused with unsaved changes unless `discard`.
    ///
    /// # Errors
    ///
    /// It has unsaved changes.
    pub fn close_stage(&mut self, e: &mut Editor, discard: bool) -> Result<(), String> {
        let Some(open) = &self.open else { return Ok(()) };
        if let Some(d) = e.docs.find(open.doc) {
            if d.modified() && !discard {
                return Err(format!("{} has unsaved changes: save it (Ctrl+S) or revert it first", open.key));
            }

            e.docs.close(&mut e.scene, open.doc);
        }

        self.open = None;
        Ok(())
    }

    /// The prefab as its file has it again.
    ///
    /// # Errors
    ///
    /// None open, or it can't be read.
    pub fn revert(&mut self, e: &mut Editor) -> Result<String, String> {
        let key = self.open_key().ok_or("no prefab on the stage")?.to_owned();
        self.close_stage(e, true)?;
        self.open_prefab(e, &key).map(|_| format!("{key} reverted"))
    }

    /// Saves the stage's prefab.
    ///
    /// # Errors
    ///
    /// None open, or the save failed.
    pub fn save(&mut self, e: &mut Editor) -> Result<String, String> {
        let id = self.open.as_ref().ok_or("no prefab on the stage")?.doc;
        let r = e.docs.save_one(id);
        self.after_save(e);
        r
    }

    /// Saves the stage's prefab for the Assembly Kit (as TEd reads it: no ground).
    ///
    /// # Errors
    ///
    /// None open, unsaved changes, or the save failed.
    pub fn save_for_kit(&mut self, e: &mut Editor) -> Result<String, String> {
        let key = self.open_key().ok_or("no prefab on the stage")?.to_owned();
        if self.stage(e).is_some_and(|d| d.modified()) {
            return Err(format!("save {key} first"));
        }

        self.tile.save_prefab_for_kit(e, &key, None)
    }

    /// The libraries (this tool's and the Tile tool's) read again, so that the tiles' references follow a save.
    fn rescan(&mut self, e: &mut Editor) {
        self.tile.rescan_prefabs(e);
        e.with_tool::<TileTool, _>(|t, e| {
            t.rescan_prefabs(e);
        });
    }

    fn after_save(&mut self, e: &mut Editor) {
        let Some(open) = self.open.as_ref().filter(|o| o.saved.swap(false, Ordering::Relaxed)) else { return };
        let (key, old) = (open.key.clone(), open.old.clone());
        self.rescan(e);
        let n = e.with_tool::<TileTool, _>(|t, e| t.refresh_copies(e, &key, &old)).unwrap_or(0);
        if n != 0 {
            e.status(&format!("{key} saved: {n} cop{} in the open tiles updated", if n == 1 { "y" } else { "ies" }), false);
        }

        let new = self.library(e).contents(&key).unwrap_or_default();
        if let Some(open) = &mut self.open {
            open.old = new;
        }
    }

    /// The stage shows, the tiles don't (they would be where it is).
    fn show_stage(&mut self, e: &mut Editor) {
        for id in e.docs.order().to_vec() {
            let tool = e.docs.tool_of(id);
            if tool == TILE_TOOL_ID && e.docs.visible(&e.scene, id) {
                e.docs.set_visible(&mut e.scene, id, false);
                self.hidden.push(id);
            } else if tool == PREFAB_TOOL_ID {
                e.docs.set_visible(&mut e.scene, id, true);
            }
        }

        if let Some(open) = &self.open {
            e.docs.set_active(open.doc);
        }
    }

    // ---- panels

    fn draw_prefabs_panel(&mut self, e: &mut Editor) {
        if !gui::begin("Prefabs", None, 0) {
            gui::end();
            return;
        }

        let lib = self.library(e);
        if let Some(key) = self.open_key().map(str::to_owned) {
            gui::text(&format!("{}  {key}", icons::BOX));
            if gui::button(&format!("{}  Save", icons::SAVE)) {
                let r = self.save(e);
                self.report(e, r);
            }

            gui::same_line();
            if gui::button("Revert") {
                let r = self.revert(e);
                self.report(e, r);
            }

            gui::same_line();
            if gui::button("Save for Assembly Kit") {
                let r = self.save_for_kit(e);
                self.report(e, r);
            }

            self.draw_properties(e);
            gui::separator();
        }

        // New.
        gui::set_next_item_width(140.0);
        gui::input_text("##new_name", Some("new prefab"), &mut self.new_name, 128, 0);
        gui::same_line();
        gui::set_next_item_width(100.0);
        gui::input_text("##new_folder", Some("folder"), &mut self.new_folder, 256, 0);
        gui::same_line();
        if gui::button(&format!("{}##new", icons::PACKAGE_PLUS)) {
            let (name, folder) = (self.new_name.clone(), self.new_folder.clone());
            let r = self.new_prefab(e, &name, &folder);
            if r.is_ok() {
                self.new_name.clear();
            }

            self.report(e, r);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("A new empty prefab in the projects' art/prefabs (the folder below it, optional)");
        }

        if !self.status.is_empty() {
            gui::text_wrapped(&self.status);
        }

        gui::set_next_item_width(-1.0);
        gui::input_text("##prefab_filter", Some("filter"), &mut self.filter, 256, 0);
        gui::begin_child("prefab_list", gui::v2(0.0, 0.0), 0, 0);
        let needle = self.filter.to_lowercase();
        let mut folder = None;
        let mut open = None;
        for p in lib.entries() {
            let path = if p.folder.is_empty() { p.key.clone() } else { format!("{}/{}", p.folder, p.key) };
            if !needle.is_empty() && !path.to_lowercase().contains(&needle) {
                continue;
            }

            if folder.as_ref() != Some(&p.folder) {
                folder = Some(p.folder.clone());
                gui::text_disabled(&format!("{}  {}", icons::FOLDER, if p.folder.is_empty() { "(top)" } else { &p.folder }));
            }

            let label = format!("    {}{}##{}", p.key, if p.wce { "" } else { "  [kit]" }, path);
            if gui::selectable(&label, self.open_key() == Some(p.key.as_str()), gui::SELECTABLE_ALLOW_DOUBLE_CLICK) && gui::is_mouse_double_clicked(0) {
                open = Some(p.key.clone());
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}\nDouble-click: open it on the stage", p.path.display()));
            }
        }

        gui::end_child();
        gui::end();
        if let Some(key) = open {
            let r = self.open_prefab(e, &key);
            self.report(e, r);
        }
    }

    /// The prefab's own settings: its height layer's.
    fn draw_properties(&mut self, e: &mut Editor) {
        let Some(d) = self.stage(e) else { return };
        let Some(group) = d.stage_group() else { return };
        let doc = d.tile().tscene().doc();
        let mut normalize = doc.get(group, NORMALIZE_ATTR).is_some_and(|v| v == "true");
        let mut feather = doc.get(group, FEATHER_ATTR).and_then(|v| v.parse::<f32>().ok()).unwrap_or(DEFAULT_FEATHER as f32);
        if gui::checkbox("Normalize ground", &mut normalize) {
            d.set_attribute(group, NORMALIZE_ATTR, if normalize { "true" } else { "false" });
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("On: placed in a tile, the ground under the prefab's footprint is taken as this stage's plain (at the height under its origin) and its \
                ground goes over it, so it looks as it does here. Off: its ground adds to whatever ground it is put on.");
        }

        if normalize {
            gui::set_next_item_width(120.0);
            if gui::drag_float("feather", &mut feather, 0.1, 0.0, 100.0, "%.1f") {
                d.set_attribute(group, FEATHER_ATTR, &wce_tile::ted_scene::format_number(f64::from(feather.max(0.0))));
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("How far past the footprint the levelled ground blends into the tile's");
            }
        }

        // Found again after an edit only.
        let key = (d.id(), d.history().version() ^ d.tile().terrain_version().rotate_left(32), normalize, feather.to_bits());
        if self.ground_size.0 != key {
            let size = d.stage_terrain(normalize, f64::from(feather)).map(|t| (f64::from(t.width) * t.spacing, f64::from(t.height) * t.spacing));
            self.ground_size = (key, size);
        }

        gui::text_disabled(&match self.ground_size.1 {
            Some((w, h)) => format!("ground: {w:.0} x {h:.0}"),
            None => "no ground: sculpt it, or paint its footprint with the Ground brush".to_owned(),
        });
    }

    fn report(&mut self, e: &mut Editor, r: Result<String, String>) {
        match r {
            Ok(m) => {
                e.status(&m, false);
                self.status = m;
            }

            Err(m) => {
                e.status(&m, true);
                self.status = m;
            }
        }
    }
}

/// Runs `f` on the Prefab tool.
fn with_prefab(e: &mut Editor, f: impl FnOnce(&mut PrefabTool, &mut Editor) -> Result<String, String>) -> CommandResult {
    match e.with_tool::<PrefabTool, _>(f) {
        Some(Ok(m)) => CommandResult::success(m),
        Some(Err(m)) => CommandResult::fail(m),
        None => CommandResult::fail("the prefab tool is not available"),
    }
}

fn register(c: &mut CommandRegistry<Editor>) {
    c.add("prefab_tool_open", "prefab_tool_open <key>", "open a prefab on the Prefab tool's stage", |e, a| {
        let Some(key) = a.first().cloned() else { return CommandResult::fail("usage: prefab_tool_open <key>") };
        with_prefab(e, |p, e| p.open_prefab(e, &key))
    });

    c.add("prefab_tool_new", "prefab_tool_new <name> [folder]", "a new empty prefab in the projects, opened on the stage", |e, a| {
        let Some(name) = a.first().cloned() else { return CommandResult::fail("usage: prefab_tool_new <name> [folder]") };
        let folder = a.get(1).cloned().unwrap_or_default();
        with_prefab(e, |p, e| p.new_prefab(e, &name, &folder))
    });

    c.add("prefab_tool_save", "prefab_tool_save", "save the stage's prefab (its contents and its ground) into the projects", |e, _| with_prefab(e, PrefabTool::save));
    c.add("prefab_tool_revert", "prefab_tool_revert", "open the stage's prefab again as its file has it", |e, _| with_prefab(e, PrefabTool::revert));
    c.add("prefab_tool_save_kit", "prefab_tool_save_kit", "save the stage's prefab for the Assembly Kit (TEd's prefabs have no ground)", |e, _| with_prefab(e, PrefabTool::save_for_kit));
    c.add("prefab_tool_close", "prefab_tool_close [discard]", "close the stage", |e, a| {
        let discard = a.first().is_some_and(|v| v == "discard");
        with_prefab(e, |p, e| p.close_stage(e, discard).map(|()| "stage closed".to_owned()))
    });
}

impl Tool for PrefabTool {
    fn id(&self) -> &str {
        PREFAB_TOOL_ID
    }

    fn title(&self) -> &str {
        "Prefab"
    }

    fn icon(&self) -> &str {
        icons::BOX
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.30);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.18);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Prefabs", left);
        gui::dock_builder_dock_window("Create", left);
        gui::dock_builder_dock_window("Scene", left);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Weather", right);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.draw_prefabs_panel(e);
        self.tile.draw_panels(e);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if gui::begin_menu("Prefab") {
            let open = self.open.is_some();
            let mut r = None;
            if gui::menu_item("Save", None, false, open) {
                r = Some(self.save(e));
            }

            if gui::menu_item("Revert", None, false, open) {
                r = Some(self.revert(e));
            }

            if gui::menu_item("Save for Assembly Kit", None, false, open) {
                r = Some(self.save_for_kit(e));
            }

            gui::separator();
            if gui::menu_item("Close the stage", None, false, open) {
                r = Some(self.close_stage(e, false).map(|()| "stage closed".to_owned()));
            }

            gui::end_menu();
            if let Some(r) = r {
                self.report(e, r);
            }
        }
    }

    fn toolbar(&mut self, e: &mut Editor) {
        self.tile.toolbar(e);
    }

    fn edit_menu(&mut self, e: &mut Editor) {
        self.tile.edit_menu(e);
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        self.tile.viewport_overlay(e, origin, size);
    }

    fn camera_chip(&mut self, e: &mut Editor) {
        self.tile.camera_chip(e);
    }

    fn snap_chip(&mut self, e: &mut Editor) {
        self.tile.snap_chip(e);
    }

    fn snapping(&mut self, e: &mut Editor) -> Option<String> {
        self.tile.snapping(e)
    }

    fn inspect(&mut self, e: &mut Editor, ins: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        self.tile.inspect(e, ins)
    }

    fn inspect_below(&mut self, e: &mut Editor) {
        self.tile.inspect_below(e);
    }

    fn unsaved_work(&self) -> Vec<String> {
        self.tile.unsaved_work()
    }

    fn viewport_input(&mut self, e: &mut Editor, event: &ViewportEvent) -> bool {
        self.tile.viewport_input(e, event)
    }

    fn claims_left_button(&self, e: &Editor) -> bool {
        self.tile.claims_left_button(e)
    }

    fn tool_gizmo(&self, e: &Editor, dragging: bool) -> Option<wce_foundation::math::Float4x4> {
        self.tile.tool_gizmo(e, dragging)
    }

    fn tool_gizmo_moved(&mut self, e: &mut Editor, frame: &wce_foundation::math::Float4x4) {
        self.tile.tool_gizmo_moved(e, frame);
    }

    fn left_drag_selects(&self, e: &Editor) -> bool {
        self.tile.left_drag_selects(e)
    }

    fn box_drag(&mut self, e: &mut Editor, from: (i32, i32), to: (i32, i32), done: bool) {
        self.tile.box_drag(e, from, to, done);
    }

    fn snap_gizmo_move(&self, e: &Editor, item: SourceRef, from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
        self.tile.snap_gizmo_move(e, item, from, to)
    }

    fn snap_gizmo_rotate(&self, e: &Editor, item: SourceRef, frame: &wce_foundation::math::Float4x4, moved: wce_foundation::math::Float4x4) -> wce_foundation::math::Float4x4 {
        self.tile.snap_gizmo_rotate(e, item, frame, moved)
    }

    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        register(commands);
    }

    fn lights_scene(&self) -> bool {
        self.tile.lights_scene()
    }

    fn update(&mut self, e: &mut Editor) {
        // A stage closed elsewhere (the Documents panel) is gone.
        if let Some(open) = &self.open
            && e.docs.find(open.doc).is_none()
        {
            self.open = None;
        }

        self.after_save(e);
        self.tile.update(e);
    }

    fn game_changed(&mut self, e: &mut Editor) {
        let _ = self.close_stage(e, true);
        self.tile.game_changed(e);
    }

    fn activated(&mut self, e: &mut Editor) {
        self.show_stage(e);
        self.tile.activated(e);
    }

    fn settings_applied(&mut self, e: &mut Editor) {
        self.tile.settings_applied(e);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl TileTool {
    /// The Tile tool activated: the tiles the Prefab tool hid show again, its stage does not.
    pub(crate) fn show_tiles(e: &mut Editor) {
        let hidden = e.with_tool::<PrefabTool, _>(|p, _| std::mem::take(&mut p.hidden)).unwrap_or_default();
        for id in e.docs.order().to_vec() {
            if e.docs.tool_of(id) == PREFAB_TOOL_ID {
                e.docs.set_visible(&mut e.scene, id, false);
            } else if hidden.contains(&id) {
                e.docs.set_visible(&mut e.scene, id, true);
            }
        }

        if let Some(id) = e.docs.order().iter().copied().find(|&id| e.docs.tool_of(id) == TILE_TOOL_ID) {
            e.docs.set_active(id);
        }
    }
}
