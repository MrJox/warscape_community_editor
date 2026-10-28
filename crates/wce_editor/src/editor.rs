//! The editor shell: owns the packs, the renderer, the shared scene and the open documents, draws the dockspace, menus, toolbar and panels,
//! switches tools (with a layout file per tool), and runs commands and scripts. The app creates one editor, registers the tools and calls
//! `pre_frame` / `inject_input` before Dear ImGui's frame and `frame` inside it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use wce_d3d11::{ID3D11Device, ID3D11DeviceContext};
use wce_editor_core::command_registry::{CommandHost, CommandRegistry, ScriptRunner};
use wce_editor_core::document::Document;
use wce_editor_core::document_manager::DocumentManager;
use wce_editor_core::selection::Selection;
use wce_foundation::clock::Clock;
use wce_foundation::settings::Settings;
pub use wce_foundation::settings::DEFAULT_FRAME_CAP;
use wce_foundation::{log, wce_prof};
use wce_imgui::renderer::D3D11Renderer;
use wce_render::files::Files;
use wce_render::renderer::Renderer;
use wce_formats::xml_doc::XmlDoc;
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, read_environment, read_graphics_options};
use wce_scene::render_scene::{DocumentId, EnvironmentSlot, RenderScene};
use wce_vfs::{Config, GameProfile, Vfs, normalize};

use crate::{gui, icons};
use crate::panels::PanelState as PanelData;
use crate::perf::PerfStats;
use crate::steam_detect::{detect_game_folder, file_dialog, pick_folder};
use crate::tool::Tool;
use crate::viewport::Viewport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PromptKind {
    #[default]
    None,
    Close,
    Quit,
}

/// The scripted mouse: positions are pixels of the viewport image.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseState {
    pub x: f32,
    pub y: f32,
    pub down: [bool; 3],
    pub wheel: f32,
}

/// Which shared panels are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    pub viewport: bool,
    pub outliner: bool,
    pub inspector: bool,
    pub assets: bool,
    pub log: bool,
    pub history: bool,
    pub commands: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self { viewport: true, outliner: true, inspector: true, assets: true, log: true, history: true, commands: false }
    }
}

/// A weather file lighting the scene outside the weather tool ("Preview this weather on this tile").
pub(crate) struct WeatherPreview {
    /// The file's path in the packs, normalized (a document's `source_key`).
    path: String,

    /// The file as the packs have it.
    params: EnvironmentParams,
}

/// What the shell keeps of a tool while it is lent out.
struct ToolInfo {
    id: String,
    title: String,
    icon: String,
    opens: Vec<String>,
}

pub struct Editor {
    pub(crate) dev: ID3D11Device,
    pub(crate) ctx: ID3D11DeviceContext,
    settings: Settings,
    vfs: Option<Arc<Vfs>>,
    pub renderer: Option<Renderer>,
    pub scene: RenderScene,
    pub docs: DocumentManager,
    pub selection: Selection,

    /// The selection's outlined entities (`selection_entities`), for the selection and the scene structure they were found for.
    outline_cache: Option<(Vec<wce_scene::render_scene::SourceRef>, u64, Vec<wce_scene::render_scene::EntityId>)>,

    /// The active document's tint (`Document::tint`) as entities: its document, key and the scene's structure revision it was found for.
    tint_cache: Option<(DocumentId, u64, u64, Vec<wce_scene::render_scene::EntityId>, [f32; 4])>,
    pub(crate) commands: CommandRegistry<Editor>,
    pub(crate) script: ScriptRunner,
    pub(crate) script_lines: Vec<String>,
    pub viewport: Viewport,

    /// A window to bring to the front on the next frame (`window_focus`, for scripts).
    pub pending_focus: Option<String>,
    pub clock: Clock,
    tools: Vec<Option<Box<dyn Tool>>>,
    tool_info: Vec<ToolInfo>,
    active_tool: usize,
    pending_tool: Option<usize>,
    layout_pending_default: bool,
    layout_loaded: bool,
    dockspace: gui::ImGuiID,
    quit: bool,
    exit_code: i32,
    frame: u64,
    status: String,
    status_error: bool,
    first_run: bool,
    pub(crate) weather_preview: Option<WeatherPreview>,
    pub(crate) cam_mark: Option<Camera>,
    output_gamma: f32,
    output_brightness: f32,
    prompt: PromptKind,
    prompt_docs: Vec<DocumentId>,

    /// The quit prompt's work held by the tools that no document saves (`Tool::unsaved_work`).
    prompt_work: Vec<String>,
    prompt_close: Vec<DocumentId>,
    prompt_error: String,

    /// The game the close prompt switches to once the documents are closed.
    prompt_game: Option<&'static GameProfile>,
    mouse_queue: VecDeque<MouseState>,
    pub(crate) mouse_cur: MouseState,
    mouse_active: bool,

    /// Keys the `key` command holds down for a couple of frames.
    pub(crate) test_keys: Vec<(i32, i32)>,
    first_run_game: String,
    first_run_message: String,
    mod_checks: Vec<(bool, String)>,
    mod_checks_for: String,
    mods_popup: bool,

    /// Frame times (the app loop records them).
    pub perf: PerfStats,
    pub(crate) wait_loaded_start: Option<Instant>,

    /// The app loop presents with it (command `vsync`, environment WCE_VSYNC=0).
    pub vsync: bool,

    /// Frames per second an interactive session is held to (0: no limit; settings `frame_cap`, environment WCE_FRAME_CAP). A laptop GPU
    /// rendering the scene as fast as a fast display refreshes heats up and throttles itself to a fraction of its clock.
    frame_cap: u32,
    pub panels: Panels,
    pub(crate) panel_data: PanelData,

    pub(crate) settings_window: crate::settings_window::SettingsWindow,

    /// An interactive session: the active tool is kept in the settings (`restore_last_tool`).
    remember_tool: bool,

    /// The display's scale the theme's sizes are multiplied by (`set_display_scale`).
    display_scale: f32,

    /// The appearance the style was built from: `pre_frame` builds it again when the settings' differs.
    applied_appearance: Option<wce_foundation::settings::Appearance>,

    /// Pictures tools draw with (see `images`).
    pub images: crate::images::ImageTextures,

    /// A tool without a 3D viewport sets this every frame so that the scripted mouse (`mouse ...`) is in window pixels from this origin.
    pub input_origin: Option<gui::Vec2>,

    /// The file the next `shot` writes, handled by the app loop.
    pub pending_shot: Option<PathBuf>,
}

impl CommandHost for Editor {
    fn commands(&self) -> &CommandRegistry<Self> {
        &self.commands
    }

    fn take_script_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.script_lines)
    }
}

impl Editor {
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext) -> Self {
        let mut e = Self {
            dev: dev.clone(), ctx: ctx.clone(), settings: Settings::load_default(), vfs: None, renderer: None, scene: RenderScene::default(),
            docs: DocumentManager::default(), selection: Selection::default(), commands: CommandRegistry::default(), script: ScriptRunner::default(), script_lines: vec![],
            viewport: Viewport::default(), pending_focus: None, clock: Clock::live(), tools: vec![], tool_info: vec![], active_tool: 0, pending_tool: None, layout_pending_default: false,
            layout_loaded: false, dockspace: 0, quit: false, exit_code: 0, frame: 0, status: String::new(), status_error: false, first_run: false, weather_preview: None,
            cam_mark: None, output_gamma: 2.2, output_brightness: 1.0, prompt: PromptKind::None, prompt_docs: vec![], prompt_work: vec![], outline_cache: None, tint_cache: None, prompt_close: vec![], prompt_error: String::new(), prompt_game: None,
            mouse_queue: VecDeque::new(), mouse_cur: MouseState::default(), mouse_active: false, test_keys: vec![], first_run_game: String::new(),
            first_run_message: String::new(), mod_checks: vec![], mod_checks_for: String::new(), mods_popup: false, perf: PerfStats::default(), wait_loaded_start: None, vsync: true, frame_cap: DEFAULT_FRAME_CAP, panels: Panels::default(),
            panel_data: PanelData::default(), settings_window: Default::default(), remember_tool: false, display_scale: 1.0, applied_appearance: None, images: Default::default(), input_origin: None, pending_shot: None,
        };

        e.frame_cap = e.settings.frame_cap();
        e.viewport.gizmo_points = e.settings.appearance.gizmo_size;
        crate::commands::register_builtin(&mut e.commands);
        e
    }

    /// The display's scale (1 for a high pixel density window, whose fonts Dear ImGui already rasterizes at its density).
    pub fn set_display_scale(&mut self, dpi: f32) {
        self.display_scale = dpi;
        self.viewport.gizmo_points = self.settings.appearance.gizmo_size * dpi;
        self.applied_appearance = None;
    }

    /// The selection outline's colour (the theme's highlight) and width: 2 px at 100 % (the display's scale, and the pixel density of a high
    /// density window). Scripted runs keep 2 px, so that shots do not depend on the machine.
    /// The entities the selection is outlined and framed by: each item's, a group's being the items below it (`Document::outline_items`).
    /// Found again when the selection or the scene's structure changes.
    pub fn selection_entities(&mut self) -> Vec<wce_scene::render_scene::EntityId> {
        let items = self.selection.items().to_vec();
        let rev = self.scene.structure_revision();
        if let Some((i, r, ents)) = &self.outline_cache
            && *i == items
            && *r == rev
        {
            return ents.clone();
        }

        let mut expanded = vec![];
        for &it in &items {
            match self.docs.find(it.doc) {
                Some(d) => expanded.extend(d.outline_items(it)),
                None => expanded.push(it),
            }
        }

        let ents = match expanded.as_slice() {
            [] => vec![],
            &[r] => self.scene.entities_of(r),
            many => {
                let wanted = many.iter().copied().collect::<std::collections::HashSet<_>>();
                self.scene.entities().iter().filter(|e| wanted.contains(&e.source)).map(|e| e.id).collect()
            }
        };

        self.outline_cache = Some((items, rev, ents.clone()));
        ents
    }

    /// The entities the active document tints in the viewport and the colour (`Document::tint`), none when it tints nothing.
    pub fn tint_entities(&mut self) -> (Vec<wce_scene::render_scene::EntityId>, [f32; 4]) {
        let Some(d) = self.docs.active() else { return (vec![], [0.0; 4]) };
        let (doc, key, rev) = (d.id(), d.tint_key(), self.scene.structure_revision());
        if key == 0 {
            return (vec![], [0.0; 4]);
        }

        if let Some((cd, ck, cr, ents, colour)) = &self.tint_cache
            && (*cd, *ck, *cr) == (doc, key, rev)
        {
            return (ents.clone(), *colour);
        }

        let (items, colour) = d.tint();
        let wanted = items.into_iter().collect::<std::collections::HashSet<_>>();
        let ents = self.scene.entities().iter().filter(|e| e.source.doc == doc && wanted.contains(&e.source.item)).map(|e| e.id).collect::<Vec<_>>();
        self.tint_cache = Some((doc, key, rev, ents.clone(), colour));
        (ents, colour)
    }

    pub fn selection_outline_style(&self) -> ([f32; 4], f32) {
        let density = if self.scripted_input() { 1.0 } else { self.display_scale * gui::io().DisplayFramebufferScale.x.max(1.0) };
        (crate::theme::palette().highlight, 2.0 * density)
    }

    pub fn frame_cap(&self) -> u32 {
        self.frame_cap
    }

    /// Sets the frame cap (0: no limit) and keeps it in the settings.
    /// The gizmo's handles' length in points (before the display's scale), kept in the settings.
    pub fn set_gizmo_size(&mut self, points: f32) {
        let points = points.clamp(20.0, 300.0);
        self.settings.appearance.gizmo_size = points;
        self.viewport.gizmo_points = points * self.display_scale;
        self.save_settings();
    }

    pub fn set_frame_cap(&mut self, fps: u32) {
        self.frame_cap = fps;
        self.settings.frame_cap = fps;
        self.save_settings();
    }

    /// Sets the frame cap of this session only.
    pub(crate) fn set_frame_cap_live(&mut self, fps: u32) {
        self.frame_cap = fps;
    }

    // ---- setup

    pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
        tool.register_commands(&mut self.commands);
        self.settings.set_game_fields(tool.id(), tool.game_settings_fields());
        self.tool_info.push(ToolInfo { id: tool.id().to_owned(), title: tool.title().to_owned(), icon: tool.icon().to_owned(), opens: tool.opens() });
        self.tools.push(Some(tool));
    }

    /// Runs `f` on tool `i`, lent out of the editor meanwhile; `None` when it is already lent (a tool calling into itself).
    pub fn with_tool_at<R>(&mut self, i: usize, f: impl FnOnce(&mut dyn Tool, &mut Editor) -> R) -> Option<R> {
        let mut tool = self.tools.get_mut(i)?.take()?;
        let r = f(tool.as_mut(), self);
        self.tools[i] = Some(tool);
        Some(r)
    }

    /// Runs `f` on the tool of type `T` (a tool's commands reach their tool this way).
    pub fn with_tool<T: Tool, R>(&mut self, f: impl FnOnce(&mut T, &mut Editor) -> R) -> Option<R> {
        let i = self.tools.iter_mut().position(|t| t.as_mut().is_some_and(|t| t.as_any_mut().is::<T>()))?;
        self.with_tool_at(i, |t, e| f(t.as_any_mut().downcast_mut::<T>().expect("the tool of that type"), e))
    }

    pub fn set_fixed_step(&mut self, step: f64) {
        self.clock = Clock::fixed(step);
    }

    pub fn status(&mut self, text: &str, error: bool) {
        self.status = text.to_owned();
        self.status_error = error;
        if error {
            log::error(text);
        } else {
            log::info(text);
        }
    }

    pub fn status_text(&self) -> &str {
        &self.status
    }

    pub fn device(&self) -> &ID3D11Device {
        &self.dev
    }

    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.ctx
    }

    /// The game being edited.
    pub fn game(&self) -> &'static GameProfile {
        self.settings.game_profile()
    }

    /// Switches to another game: closes every document (the unsaved-changes prompt asks first if one has changes), then mounts the
    /// game's packs and creates the renderer again. A game that is not set up yet opens the first-run dialog.
    ///
    /// # Errors
    ///
    /// The editor doesn't support the game.
    pub fn switch_game(&mut self, key: &str) -> Result<(), String> {
        let game = GameProfile::by_key(key).ok_or_else(|| format!("the editor doesn't support the game '{key}'"))?;
        if game.key == self.game().key && self.is_mounted() {
            return Ok(());
        }

        let all = self.docs.order().to_vec();
        self.prompt_docs = self.docs.unsaved();
        if self.prompt_docs.is_empty() {
            for id in all {
                self.close_document(id);
            }

            self.enter_game(game);
            return Ok(());
        }

        self.prompt = PromptKind::Close;
        self.prompt_error.clear();
        self.prompt_close = all;
        self.prompt_game = Some(game);
        Ok(())
    }

    /// Makes `game` the current one (its documents closed already): the tools drop what was the old game's, then the game's files are
    /// mounted and the tools activated.
    fn enter_game(&mut self, game: &'static GameProfile) {
        self.settings.game = game.key.to_owned();
        self.save_settings();
        self.mod_checks_reset();
        self.images.clear();
        for i in 0..self.tools.len() {
            self.with_tool_at(i, |t, e| t.game_changed(e));
        }

        self.first_run_game.clear();
        self.first_run_message.clear();
        if self.start() {
            self.activate_all_tools();
            self.status(&format!("{}: {}", game.display_name, self.status_text().to_owned()), false);
        }
    }

    pub fn vfs(&self) -> Option<&Arc<Vfs>> {
        self.vfs.as_ref()
    }

    /// The game's files, for the renderer and the tools.
    pub fn files(&self) -> Option<Arc<dyn Files>> {
        self.vfs.clone().map(|v| v as Arc<dyn Files>)
    }

    pub fn renderer(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }

    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
    }

    /// The settings, to change them; `save_settings` writes them.
    pub fn settings(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn settings_ref(&self) -> &Settings {
        &self.settings
    }

    /// Writes the settings file; a failure is logged.
    pub fn save_settings(&mut self) {
        if let Err(err) = self.settings.save() {
            log::error(format!("Cannot write {}: {err}", self.settings.file().display()));
        }
    }

    /// A tool's settings (its `[tools.<id>]` table), defaults where unset.
    pub fn tool_settings<T: serde::de::DeserializeOwned + Default>(&self, id: &str) -> T {
        self.settings.tool(id)
    }

    /// Changes a tool's settings and writes the file.
    pub fn update_tool_settings<T: serde::de::DeserializeOwned + serde::Serialize + Default>(&mut self, id: &str, f: impl FnOnce(&mut T)) {
        let mut value: T = self.settings.tool(id);
        f(&mut value);
        self.settings.set_tool(id, &value);
        self.save_settings();
    }

    pub fn commands(&self) -> &CommandRegistry<Editor> {
        &self.commands
    }

    pub fn script(&mut self) -> &mut ScriptRunner {
        &mut self.script
    }

    pub fn wants_quit(&self) -> bool {
        self.quit
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn frame_number(&self) -> u64 {
        self.frame
    }

    pub fn request_quit(&mut self, exit_code: i32) {
        self.quit = true;
        self.exit_code = exit_code;
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn tool_ids(&self) -> Vec<String> {
        self.tool_info.iter().map(|t| t.id.clone()).collect()
    }

    pub fn tool_title(&self, id: &str) -> Option<&str> {
        self.tool_info.iter().find(|t| t.id == id).map(|t| t.title.as_str())
    }

    /// Runs `f` on the active tool.
    pub fn with_active_tool<R>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut Editor) -> R) -> Option<R> {
        self.with_tool_at(self.active_tool, f)
    }

    pub fn active_tool_id(&self) -> &str {
        self.tool_info.get(self.active_tool).map_or("", |t| t.id.as_str())
    }

    pub fn active_tool_opens(&self) -> &[String] {
        self.tool_info.get(self.active_tool).map_or(&[], |t| t.opens.as_slice())
    }

    /// The projects' loose files (`<projects>/loose`): the UI tool's skins, saved layouts and pictures live there, read before every pack.
    pub fn loose_dir(&self) -> PathBuf {
        self.settings.loose_dir()
    }

    /// What to mount: the game's release packs, the mod packs the user chose, and the loose folders above them. The launcher's list of
    /// enabled mods is not used.
    fn vfs_config(&self, game_dir: PathBuf) -> Config {
        let mut cfg = Config::new(game_dir, self.game().clone());
        cfg.use_mods = false;
        cfg.mod_packs = Some(self.selected_mod_packs());
        cfg.overlay_folders = self.overlay_folders();
        cfg
    }

    /// The mod packs the user chose (the current game's `mod_packs` in the settings: file names in the data folder).
    pub fn selected_mod_packs(&self) -> Vec<String> {
        self.settings.mod_packs()
    }

    /// Chooses the mod packs and mounts the files again.
    ///
    /// # Errors
    ///
    /// The files cannot be mounted.
    pub fn set_mod_packs(&mut self, names: &[String]) -> Result<(), String> {
        self.settings.game_mut().mod_packs = names.to_vec();
        self.save_settings();
        self.remount_vfs()
    }

    /// The mod packs found in the game's data folder.
    pub fn available_mod_packs(&self) -> Vec<String> {
        let Some(dir) = self.settings.game_dir() else { return vec![] };
        self.game().available_mod_packs(&dir)
    }

    /// The check list of mod packs (first run and File > Mod packs).
    fn mod_pack_checks(&mut self, game: &str) {
        let (profile, chosen) = (self.game(), self.selected_mod_packs());
        self.mod_pack_checks_at(game, profile, &chosen, gui::cursor_pos_x());
    }

    /// Builds the check list again from the settings the next time it is drawn.
    pub(crate) fn mod_checks_reset(&mut self) {
        self.mod_checks_for.clear();
    }

    /// The ticked mod packs, if the check list is the one of `profile` in the folder `game`.
    pub(crate) fn mod_checks_for_game(&self, profile: &GameProfile, game: &Option<PathBuf>) -> Option<Vec<String>> {
        let game = game.as_ref().map(|g| g.display().to_string()).unwrap_or_default();
        (self.mod_checks_for == format!("{}|{game}", profile.key)).then(|| self.checked_mod_packs())
    }

    /// The check list at a cursor x: the packs of `profile`'s data folder in the game folder `game`, `chosen` ticked (the Settings window
    /// passes the game it shows, which needn't be the current one).
    pub(crate) fn mod_pack_checks_at(&mut self, game: &str, profile: &GameProfile, chosen: &[String], x: f32) {
        // The list is that game's: another game's choice is not carried over (it would be saved as this one's).
        let key = format!("{}|{game}", profile.key);
        if self.mod_checks_for != key {
            self.mod_checks_for = key;
            let mut names = profile.available_mod_packs(Path::new(game));
            for c in chosen {
                if !names.iter().any(|n| n.eq_ignore_ascii_case(c)) {
                    names.push(c.clone());
                }
            }

            self.mod_checks = names.into_iter().map(|n| (chosen.iter().any(|c| c.eq_ignore_ascii_case(&n)), n)).collect();
        }

        gui::set_cursor_pos_x(x);
        if self.mod_checks.is_empty() {
            gui::text_disabled("No mod packs found in the game's data folder.");
            return;
        }

        if gui::begin_child("##modlist", gui::v2(560.0, 180.0), 1, 0) {
            for (on, name) in self.mod_checks.iter_mut() {
                gui::checkbox(name, on);
            }
        }

        gui::end_child();
    }

    fn checked_mod_packs(&self) -> Vec<String> {
        self.mod_checks.iter().filter(|(on, _)| *on).map(|(_, n)| n.clone()).collect()
    }

    fn draw_mods_popup(&mut self) {
        if self.mods_popup {
            self.mods_popup = false;
            self.mod_checks_for.clear();
            gui::open_popup("Mod packs");
        }

        if gui::begin_popup_modal("Mod packs", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::text_wrapped("The editor reads the game's own (release) packs and, on top of them, only the mod packs ticked here. Your own loose skin folder is always read before all of them.");
            let game = self.settings.game_dir().map(|d| d.display().to_string()).unwrap_or_default();
            self.mod_pack_checks(&game);
            if gui::button("Apply") {
                let names = self.checked_mod_packs();
                match self.set_mod_packs(&names) {
                    Ok(()) => {
                        self.activate_all_tools();
                        self.status(&format!("{} mod pack(s) selected", names.len()), false);
                    }
                    Err(m) => self.status(&m, true),
                }

                gui::close_current_popup();
            }

            gui::same_line();
            if gui::button("Cancel") {
                gui::close_current_popup();
            }

            gui::end_popup();
        }
    }

    /// Loose folders read before every pack, highest priority first: the projects' loose files, then the current game's `overlay_folders`.
    fn overlay_folders(&self) -> Vec<PathBuf> {
        let mut v = vec![self.loose_dir()];
        v.extend(self.settings.overlay_folders());
        v
    }

    /// Mounts the files again (the project folder changed). The renderer keeps the files it was created with.
    ///
    /// # Errors
    ///
    /// The game folder is not set or cannot be read.
    pub fn remount_vfs(&mut self) -> Result<(), String> {
        let dir = self.settings.game_dir().ok_or("the game folder is not set")?;
        let vfs = Vfs::mount(&self.vfs_config(dir)).map_err(|e| e.to_string())?;
        self.vfs = Some(Arc::new(vfs));
        self.images.clear();
        Ok(())
    }

    /// Mounts the packs and creates the renderer. False (with a message in the log) when the game is not configured: the first-run dialog opens.
    pub fn start(&mut self) -> bool {
        let game_dir = self.settings.game_dir();
        log::info(format!("Data paths: game={}", game_dir.as_ref().map_or_else(|| "(unset)".to_owned(), |d| d.display().to_string())));
        // Another game's folder would mount (the data folders look alike) and show that game as this one.
        let game = self.game();
        let mounted = game_dir.ok_or_else(|| "the game folder is not set".to_owned()).and_then(|dir| match game.folder_problem(&dir) {
            Some(problem) => Err(problem),
            None => Vfs::mount(&self.vfs_config(dir)).map_err(|e| e.to_string()),
        });

        match mounted.and_then(|vfs| {
            let vfs = Arc::new(vfs);
            let renderer = Renderer::new(&self.dev, &self.ctx, vfs.clone()).map_err(|e| e.0)?;
            Ok((vfs, renderer))
        }) {
            Ok((vfs, mut renderer)) => {
                let go = read_graphics_options(self.game());
                renderer.settings.apply_graphics_options(&go);
                self.output_gamma = go.gamma;
                self.output_brightness = go.brightness;
                self.first_run = false;
                let mut msg = format!("Mounted {} packs, {} files.", vfs.packs().len(), vfs.files().len());
                for m in vfs.missing_mods() {
                    msg += &format!(" Missing mod: {m}.");
                }

                self.vfs = Some(vfs);
                self.renderer = Some(renderer);
                self.status(&msg, false);
                true
            }

            Err(e) => {
                self.vfs = None;
                self.renderer = None;
                self.first_run = true;
                log::error(format!("Start failed: {e}"));
                self.first_run_message = e;
                if self.first_run_game.is_empty() {
                    self.first_run_game = self.first_run_folder(self.game());
                }

                false
            }
        }
    }

    // ---- layouts

    pub fn layout_file(&self, tool_id: &str) -> PathBuf {
        self.settings.paths().layouts_dir().join(format!("{tool_id}.ini"))
    }

    /// Where "Save" writes loose files (under their in-pack path): `output_dir` in the settings, else <Documents>/AttilaWeatherEditor/out as
    /// the old weather editor did for Attila, <Documents>/WCE/<game>/out for the other games.
    pub fn output_dir(&self) -> PathBuf {
        self.settings.output_dir().unwrap_or_else(|| self.default_output_dir())
    }

    /// The loose copy of a game file in the output folder (Save writes files there, under their path in the packs), if there is one.
    pub fn output_copy(&self, path: &str) -> Option<PathBuf> {
        let p = self.output_dir().join(normalize(path));
        p.is_file().then_some(p)
    }

    /// Copies a game file from the packs into the output folder, unless a copy is there already; returns the copy.
    ///
    /// # Errors
    ///
    /// The packs are not mounted, the file is not in them, or the copy can't be written.
    pub fn copy_to_output(&self, path: &str) -> Result<PathBuf, String> {
        if let Some(copy) = self.output_copy(path) {
            return Ok(copy);
        }

        let path = normalize(path);
        let bytes = self.vfs.as_ref().ok_or("the game packs are not mounted")?.read(&path).map_err(|e| format!("{path}: {e}"))?;
        let to = self.output_dir().join(&path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }

        std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        Ok(to)
    }

    /// The output folder when the settings name none.
    pub fn default_output_dir(&self) -> PathBuf {
        default_output_dir_for(self.game())
    }

    pub fn set_output_dir(&mut self, dir: &Path, persist: bool) {
        self.settings.game_mut().output_dir = Some(dir.to_path_buf());
        if persist {
            self.save_settings();
        }
    }

    pub fn save_layout(&self) {
        let Some(t) = self.tool_info.get(self.active_tool) else { return };
        let p = self.layout_file(&t.id);
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        gui::save_ini(&p);
    }

    pub fn reset_layout(&mut self) {
        let Some(t) = self.tool_info.get(self.active_tool) else { return };
        let _ = std::fs::remove_file(self.layout_file(&t.id));
        self.layout_pending_default = true;
    }

    /// Takes effect in the next `pre_frame`.
    pub fn set_tool(&mut self, id: &str) -> bool {
        match self.tool_info.iter().position(|t| t.id == id) {
            Some(i) => {
                self.pending_tool = Some(i);
                true
            }

            None => false,
        }
    }

    /// Starts in the tool the last interactive session was in, and keeps the active tool in the settings from now on (interactive runs; a
    /// tool asked for on the command line or a scripted run starts as it says).
    pub fn restore_last_tool(&mut self) {
        self.remember_tool = true;
        let last = self.settings.last_tool.clone();
        if !last.is_empty() && !self.set_tool(&last) {
            log::warn(format!("the last tool used, '{last}', is not in this build"));
        }
    }

    fn apply_tool_switch(&mut self) {
        match self.pending_tool.take() {
            Some(i) if i != self.active_tool && i < self.tools.len() => {
                // A layout not loaded yet (the first frame) is not the tool's: the file keeps the saved one.
                if self.layout_loaded {
                    self.save_layout();
                }

                self.active_tool = i;
                self.layout_loaded = false;
                if self.remember_tool && self.settings.last_tool != self.tool_info[i].id {
                    self.settings.last_tool.clone_from(&self.tool_info[i].id);
                    self.save_settings();
                }
            }

            _ => {}
        }
    }

    fn release_test_keys(&mut self) {
        for k in &mut self.test_keys {
            k.1 -= 1;
            if k.1 == 0 {
                gui::add_key_event(k.0, false);
            }
        }

        self.test_keys.retain(|k| k.1 > 0);
    }

    pub fn queue_mouse(&mut self, s: MouseState) {
        self.mouse_queue.push_back(s);
        self.mouse_active = true;
    }

    pub fn park_mouse(&mut self) {
        self.mouse_queue.clear();
        self.mouse_active = false;
        self.mouse_cur = MouseState::default();
    }

    /// Scripted (fixed-step) runs ignore the real mouse; the `mouse` commands drive the same Dear ImGui events.
    pub fn scripted_input(&self) -> bool {
        self.clock.fixed_step()
    }

    /// Between the platform's frame setup and Dear ImGui's frame: replays the scripted mouse (or parks the cursor) in fixed-step runs.
    pub fn inject_input(&mut self) {
        if !self.clock.fixed_step() {
            return;
        }

        if !self.mouse_active {
            // The real cursor is ignored.
            gui::add_mouse_pos_event(-f32::MAX, -f32::MAX);
            for b in 0..3 {
                gui::add_mouse_button_event(b, false);
            }

            return;
        }

        if let Some(m) = self.mouse_queue.pop_front() {
            self.mouse_cur = m;
        }

        // The scripted positions are pixels of the viewport image, which has the window's pixel density.
        let o = self.input_origin.unwrap_or_else(|| self.viewport.image_origin());
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        gui::add_mouse_pos_event(o.x + self.mouse_cur.x / density, o.y + self.mouse_cur.y / density);
        for b in 0..3 {
            gui::add_mouse_button_event(b, self.mouse_cur.down[b as usize]);
        }

        if self.mouse_cur.wheel != 0.0 {
            gui::add_mouse_wheel_event(0.0, self.mouse_cur.wheel);
            self.mouse_cur.wheel = 0.0;
        }
    }

    /// Before Dear ImGui's frame: releases the keys of the `key` command and applies a pending tool switch (its layout file).
    pub fn pre_frame(&mut self) {
        self.release_test_keys();
        if self.applied_appearance.as_ref() != Some(self.shown_appearance()) {
            let appearance = self.shown_appearance().clone();
            // SAFETY: the shell calls pre_frame outside a frame, on the current context.
            unsafe { crate::theme::apply(&appearance, self.display_scale) };
            self.applied_appearance = Some(appearance);
        }

        if self.tools.is_empty() {
            return;
        }

        self.apply_tool_switch();
        if !self.layout_loaded {
            let p = self.layout_file(&self.tool_info[self.active_tool].id);
            if p.exists() {
                gui::load_ini(&p);
                self.layout_pending_default = false;
            } else {
                gui::load_ini_empty();
                self.layout_pending_default = true;
            }

            self.layout_loaded = true;
            if self.renderer.is_some() {
                self.with_tool_at(self.active_tool, |t, e| t.activated(e));
            }
        }
    }

    // ---- documents

    /// Routed to the tool that claims the extension; that tool becomes the active one unless `switch_tool` is false (opening a weather file to
    /// preview it on the tile being edited).
    ///
    /// # Errors
    ///
    /// No tool opens the file, or the tool can't.
    /// True when a tool opens files like `path` (by its extension).
    pub fn can_open(&self, path: &str) -> bool {
        let ext = path.rfind('.').map(|d| path[d..].to_lowercase()).unwrap_or_default();
        self.tool_info.iter().any(|t| t.opens.contains(&ext))
    }

    pub fn open_path(&mut self, path: &str, from_packs: bool, switch_tool: bool) -> Result<String, String> {
        self.open_path_with(None, path, from_packs, switch_tool)
    }

    /// `open_path` for a tool that opens a file while the editor calls it (a double-click in its own panel): the tool is lent out then, so
    /// it passes itself, and opens the file when it is the tool that claims it.
    ///
    /// # Errors
    ///
    /// No tool opens the file, or the tool can't.
    pub fn open_path_from(&mut self, me: &mut dyn Tool, path: &str, from_packs: bool, switch_tool: bool) -> Result<String, String> {
        self.open_path_with(Some(me), path, from_packs, switch_tool)
    }

    fn open_path_with(&mut self, me: Option<&mut dyn Tool>, path: &str, from_packs: bool, switch_tool: bool) -> Result<String, String> {
        let ext = path.rfind('.').map(|d| path[d..].to_lowercase()).unwrap_or_default();
        let claims = |t: &ToolInfo| t.opens.contains(&ext);
        let tool = if self.tool_info.get(self.active_tool).is_some_and(claims) { Some(self.active_tool) } else { self.tool_info.iter().position(claims) };
        let Some(tool) = tool else { return Err(format!("no tool opens '{ext}' files")) };

        // Already open: just show it.
        if from_packs {
            let key = normalize(path);
            if let Some(&id) = self.docs.order().iter().find(|&&id| self.docs.find(id).is_some_and(|d| d.source_key() == key)) {
                self.docs.set_active(id);
                return Ok(format!("already open: {path}"));
            }
        } else if let Some(&id) = self.docs.order().iter().find(|&&id| self.docs.find(id).is_some_and(|d| d.opened_from(path))) {
            // A file on disk: the document it is (another of its files, another path to it) is shown, not opened again.
            self.docs.set_active(id);
            if switch_tool && tool != self.active_tool {
                let tid = self.tool_info[tool].id.clone();
                self.set_tool(&tid);
            }

            return Ok(format!("already open: {path}"));
        }

        let title = self.tool_info[tool].title.clone();
        let tid = self.tool_info[tool].id.clone();
        let opened = match me {
            Some(me) if self.tools[tool].is_none() && me.id() == tid => Some(me.open(self, path, from_packs)),
            _ => self.with_tool_at(tool, |t, e| t.open(e, path, from_packs)),
        };

        let doc = opened.unwrap_or_else(|| Err(format!("{title} is busy"))).map_err(|e| format!("cannot open {path}: {e}"))?;
        self.docs.add(&mut self.scene, doc, &tid);
        if switch_tool && tool != self.active_tool {
            self.set_tool(&tid);
        }

        Ok(format!("opened {path}"))
    }

    // ---- cross-tool composition

    /// The scene is lit by the document the weather tool edits while the weather tool is active. In every other tool, `path` (an .environment
    /// file of the packs, `None` for the default light) lights it: "Preview this weather on this tile". The file is read from the packs; while a
    /// document of the same file is open (the weather tool's), the preview is that document, live while it is edited.
    ///
    /// # Errors
    ///
    /// The packs are not mounted, or the file is not in them or is not an environment.
    pub fn set_weather_preview(&mut self, path: Option<&str>) -> Result<String, String> {
        let Some(path) = path.map(normalize).filter(|p| !p.is_empty()) else {
            self.weather_preview = None;
            return Ok("weather preview off".to_owned());
        };

        if self.weather_preview.as_ref().is_some_and(|w| w.path == path) {
            return Ok(format!("previewing {path}"));
        }

        if !path.ends_with(".environment") && !path.ends_with(".lighting") {
            return Err(format!("{path} is not an .environment or .lighting file"));
        }

        // A copy in the output folder (one being edited) before the packs.
        let bytes = match self.output_copy(&path) {
            Some(copy) => std::fs::read(&copy).map_err(|e| format!("{}: {e}", copy.display()))?,
            None => self.vfs.as_ref().ok_or("the game packs are not mounted")?.read(&path).map_err(|e| format!("{path}: {e}"))?,
        };

        let doc = XmlDoc::parse(bytes).map_err(|e| format!("{path}: {e}"))?;

        self.weather_preview = Some(WeatherPreview { params: read_environment(&doc), path: path.clone() });
        Ok(format!("previewing {path}"))
    }

    /// The previewed weather file (normalized), if any.
    pub fn weather_preview(&self) -> Option<&str> {
        self.weather_preview.as_ref().map(|w| w.path.as_str())
    }

    /// The open document of the previewed weather file, if there is one: the preview follows its edits.
    pub fn weather_preview_document(&self) -> Option<DocumentId> {
        let path = self.weather_preview()?;
        self.docs.order().iter().copied().find(|&id| self.docs.find(id).is_some_and(|d| d.environment().is_some() && d.source_key() == path))
    }

    /// The scene's lighting outside the weather tool: the previewed weather (its open document if there is one), else the default environment.
    fn update_lighting(&mut self) {
        if self.with_tool_at(self.active_tool, |t, _| t.lights_scene()).unwrap_or(true) {
            return;
        }

        let live = self.weather_preview_document().and_then(|id| self.docs.find(id)).and_then(|d| d.environment().map(|p| (d.id(), p)));
        match live.or_else(|| self.weather_preview.as_ref().map(|w| (0, w.params.clone()))) {
            Some((owner, mut p)) => {
                p.output_gamma = self.output_gamma;
                p.output_brightness = self.output_brightness;
                let slot = &mut self.scene.environment;
                slot.active = Some(p);
                slot.blend = None;
                slot.blend_t = 0.0;
                slot.owner = owner;
            }

            None => self.scene.environment = EnvironmentSlot::default(),
        }
    }

    // ---- closing and quitting

    /// Closes a document without asking (the selection drops its items too).
    pub fn close_document(&mut self, id: DocumentId) {
        self.selection.remove_document(id);

        self.docs.close(&mut self.scene, id);
    }

    /// Closes at once when nothing is unsaved; otherwise the prompt opens.
    pub fn ask_close(&mut self, id: DocumentId) {
        let Some(d) = self.docs.find(id) else { return };
        if !d.modified() {
            self.close_document(id);
            return;
        }

        self.prompt = PromptKind::Close;
        self.prompt_error.clear();
        self.prompt_close = vec![id];
        self.prompt_docs = vec![id];
    }

    pub fn ask_close_all(&mut self) {
        let all = self.docs.order().to_vec();
        self.prompt_docs = self.docs.unsaved();
        if self.prompt_docs.is_empty() {
            for id in all {
                self.close_document(id);
            }

            return;
        }

        self.prompt = PromptKind::Close;
        self.prompt_error.clear();
        self.prompt_close = all;
    }

    /// The tools' work that no document saves (`Tool::unsaved_work`).
    fn tools_unsaved_work(&self) -> Vec<String> {
        self.tools.iter().flatten().flat_map(|t| t.unsaved_work()).collect()
    }

    /// The `quit` command leaves at once, as scripts expect; this one asks about unsaved changes (and the tools' unsaved work).
    pub fn ask_quit(&mut self) {
        self.prompt_docs = self.docs.unsaved();
        self.prompt_work = self.tools_unsaved_work();
        if self.prompt_docs.is_empty() && self.prompt_work.is_empty() {
            self.request_quit(0);
            return;
        }

        self.prompt = PromptKind::Quit;
        self.prompt_error.clear();
        self.prompt_close.clear();
    }

    /// The window's close button: true when the prompt took over (there are unsaved changes), false to let the window close.
    pub fn window_close_requested(&mut self) -> bool {
        if self.docs.unsaved().is_empty() && self.tools_unsaved_work().is_empty() {
            return false;
        }

        self.ask_quit();
        true
    }

    pub fn prompt_kind(&self) -> PromptKind {
        self.prompt
    }

    pub fn prompt_documents(&self) -> &[DocumentId] {
        &self.prompt_docs
    }

    /// Applies the answer to the open prompt (Save: save the documents it lists, then close / quit; if one cannot be saved nothing closes).
    ///
    /// # Errors
    ///
    /// No prompt is open, or a document could not be saved.
    pub fn answer_prompt(&mut self, a: Answer) -> Result<String, String> {
        if self.prompt == PromptKind::None {
            return Err("no prompt is open".to_owned());
        }

        let kind = self.prompt;
        self.prompt_error.clear();
        let message = match a {
            Answer::Cancel => {
                self.prompt = PromptKind::None;
                self.prompt_game = None;
                return Ok("cancelled".to_owned());
            }

            Answer::Save => {
                let mut saved = vec![];
                for id in self.prompt_docs.clone() {
                    match self.docs.save_one(id) {
                        Ok(m) => saved.push(m),
                        Err(m) => {
                            // Nothing closes; the prompt stays so that the user can discard or cancel.
                            let title = self.docs.find(id).map_or_else(|| "a document".to_owned(), |d| d.title());
                            self.prompt_error = format!("cannot save {title}: {m}");
                            log::error(self.prompt_error.clone());
                            return Err(self.prompt_error.clone());
                        }
                    }
                }

                saved.join(", ")
            }

            Answer::Discard => format!("discarded {} unsaved document(s)", self.prompt_docs.len()),
        };

        self.prompt = PromptKind::None;
        if kind == PromptKind::Close {
            for id in std::mem::take(&mut self.prompt_close) {
                self.close_document(id);
            }

            if let Some(game) = self.prompt_game.take() {
                self.enter_game(game);
            }
        } else {
            self.request_quit(0);
        }

        self.prompt_docs.clear();
        self.prompt_work.clear();
        self.prompt_close.clear();
        Ok(message)
    }

    fn draw_prompt(&mut self) {
        if self.prompt == PromptKind::None {
            return;
        }

        let quit = self.prompt == PromptKind::Quit;
        gui::open_popup("Unsaved changes");
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_ALWAYS, gui::v2(0.5, 0.5));
        let mut open = true;
        if !gui::begin_popup_modal("Unsaved changes", Some(&mut open), gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let docs = !self.prompt_docs.is_empty();
        if docs {
            gui::text(if quit { "These documents have unsaved changes:" } else { "This document has unsaved changes:" });
        }

        for &id in &self.prompt_docs {
            if let Some(d) = self.docs.find(id) {
                gui::bullet_text(&format!("{}  ({})", d.title(), d.kind()));
            }
        }

        // The tools' work: saving the documents does not keep it.
        if quit && !self.prompt_work.is_empty() {
            gui::text("Not saved anywhere yet (lost on quitting):");
            for w in &self.prompt_work {
                gui::bullet_text(w);
            }
        }

        if !self.prompt_error.is_empty() {
            gui::text_colored(crate::theme::error(), &self.prompt_error.clone());
        }

        gui::spacing();
        if docs {
            if gui::button(if quit { "Save all and quit" } else { "Save and close" }) {
                let _ = self.answer_prompt(Answer::Save);
            }

            gui::same_line();
        }

        if gui::button(if !docs { "Quit anyway" } else if quit { "Quit without saving" } else { "Discard changes" }) {
            let _ = self.answer_prompt(Answer::Discard);
        }

        gui::same_line();
        if gui::button("Cancel") || !open || gui::is_key_pressed(gui::KEY_ESCAPE, false) {
            gui::close_current_popup();
            let _ = self.answer_prompt(Answer::Cancel);
        }

        if self.prompt == PromptKind::None {
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    // ---- frame

    /// Between Dear ImGui's NewFrame and Render: the script, the documents, the tool, the dockspace, menus, toolbar and panels. `textures`
    /// shows the viewport's image.
    pub fn frame(&mut self, textures: &mut D3D11Renderer) {
        wce_prof!("editor.frame");
        self.frame += 1;
        self.images.flush(&self.dev, textures);
        self.input_origin = None;

        // Lines of a `script` command run outside a script (the command line, the console).
        if !self.script_lines.is_empty() {
            let lines = std::mem::take(&mut self.script_lines);
            self.script.include_lines(lines);
        }

        if !self.script.done() && !self.script.failed() {
            let mut script = std::mem::take(&mut self.script);
            let ok = script.step(self);
            self.script = script;
            if !ok {
                self.request_quit(1);
            }
        } else if self.script.failed() {
            self.request_quit(1);
        }

        {
            wce_prof!("editor.docs_update");
            self.docs.update(&mut self.scene);
        }

        if self.tools.is_empty() {
            return;
        }

        if self.renderer.is_some() {
            wce_prof!("editor.tool_update");
            self.with_tool_at(self.active_tool, |t, e| t.update(e));
            self.update_lighting();
        }

        let vp = gui::main_viewport();
        gui::set_next_window_pos(vp.WorkPos, 0, gui::v2(0.0, 0.0));
        gui::set_next_window_size(vp.WorkSize, 0);
        gui::push_style_var_f(gui::VAR_WINDOW_ROUNDING, 0.0);
        gui::push_style_var_f(gui::VAR_WINDOW_BORDER_SIZE, 0.0);
        gui::push_style_var_v(gui::VAR_WINDOW_PADDING, gui::v2(0.0, 0.0));
        gui::begin(
            "##host", None,
            gui::WINDOW_MENU_BAR | gui::WINDOW_NO_TITLE_BAR | gui::WINDOW_NO_COLLAPSE | gui::WINDOW_NO_RESIZE | gui::WINDOW_NO_MOVE | gui::WINDOW_NO_BRING_TO_FRONT
                | gui::WINDOW_NO_NAV_FOCUS | gui::WINDOW_NO_DOCKING | gui::WINDOW_NO_SAVED_SETTINGS,
        );
        gui::pop_style_var(3);
        self.draw_menu_bar();
        self.draw_toolbar();
        self.dockspace = gui::get_id("wce_dockspace");
        let avail = gui::content_region_avail();
        gui::dock_space(self.dockspace, avail);
        if self.layout_pending_default {
            gui::dock_builder_reset(self.dockspace, avail);
            let dockspace = self.dockspace;
            self.with_tool_at(self.active_tool, |t, _| t.default_layout(dockspace));
            gui::dock_builder_finish(self.dockspace);
            self.layout_pending_default = false;
        }

        gui::end();
        self.draw_mods_popup();
        self.draw_settings_window();
        if self.first_run {
            self.draw_first_run();
            return;
        }

        // Save works from anywhere, a text field too; the other shortcuts not while typing into a field.
        let io = gui::io();
        if io.KeyCtrl && gui::is_key_pressed(gui::KEY_S, false) {
            let _ = CommandRegistry::run(self, if io.KeyShift { "save_all" } else { "save" });
        }

        let io = gui::io();
        if !io.WantTextInput {
            if io.KeyCtrl {
                for i in 0..self.tool_info.len().min(9) {
                    if gui::is_key_pressed(gui::KEY_1 + i as i32, false) {
                        let id = self.tool_info[i].id.clone();
                        self.set_tool(&id);
                    }
                }
            }

            if io.KeyCtrl && gui::is_key_pressed(gui::KEY_Z, true) {
                let _ = CommandRegistry::run(self, if io.KeyShift { "redo" } else { "undo" });
            }

            if io.KeyCtrl && gui::is_key_pressed(gui::KEY_Y, true) {
                let _ = CommandRegistry::run(self, "redo");
            }

            // Not while the tool has the left button (placing, drawing, a brush): its keys are its own then.
            let claimed = self.with_tool_at(self.active_tool, |t, e| t.claims_left_button(e)).unwrap_or(false);
            if !io.KeyCtrl && self.viewport.hovered && !claimed {
                let fly = self.viewport.is_fly();
                for (key, mode) in [
                    (gui::KEY_W, crate::viewport::GizmoMode::Translate), (gui::KEY_E, crate::viewport::GizmoMode::Rotate), (gui::KEY_R, crate::viewport::GizmoMode::Scale),
                    (gui::KEY_Q, crate::viewport::GizmoMode::Off),
                ] {
                    if gui::is_key_pressed(key, false) && !fly && !gui::is_mouse_down(1) {
                        self.viewport.gizmo = mode;
                    }
                }

                if gui::is_key_pressed(gui::KEY_F, false) {
                    self.focus_selection();
                }
            }
        }

        {
            wce_prof!("editor.draw_panels");
            self.draw_panels(textures);
        }

        self.draw_prompt();
    }

    pub fn focus_selection(&mut self) {
        crate::viewport::focus_selection(self);
    }

    fn draw_panels(&mut self, textures: &mut D3D11Renderer) {
        if let Some(name) = self.pending_focus.take() {
            gui::set_window_focus_named(&name);
        }

        let shows_viewport = self.with_tool_at(self.active_tool, |t, _| t.shows_viewport()).unwrap_or(true);
        if self.panels.viewport && shows_viewport {
            wce_prof!("panel.viewport");
            crate::viewport::draw(self, textures);
        }

        if self.panels.outliner {
            wce_prof!("panel.outliner");
            crate::panels::outliner(self);
        }

        if self.panels.inspector {
            wce_prof!("panel.inspector");
            crate::panels::inspector(self);
        }

        if self.panels.assets {
            wce_prof!("panel.assets");
            crate::panels::assets(self);
        }

        if self.panels.log {
            wce_prof!("panel.log");
            crate::panels::log(self);
        }

        if self.panels.history {
            wce_prof!("panel.history");
            crate::panels::history(self);
        }

        if self.panels.commands {
            crate::panels::commands(self);
        }

        wce_prof!("panel.tool");
        self.with_tool_at(self.active_tool, |t, e| t.draw_panels(e));
    }

    fn draw_toolbar(&mut self) {
        gui::set_cursor_pos_x(gui::cursor_pos_x() + 6.0);
        gui::align_text_to_frame_padding();

        // The game, switched from here as from File > Game (the documents close, asking about unsaved ones).
        let current = self.game();
        gui::set_next_item_width(gui::calc_text_size(current.display_name).x + 60.0);
        if gui::begin_combo("##game", &format!("{}  {}", icons::SWORDS, current.display_name)) {
            for game in GameProfile::ALL {
                if gui::selectable(game.display_name, game.key == current.key, 0)
                    && game.key != current.key
                    && let Err(m) = self.switch_game(game.key)
                {
                    self.status(&m, true);
                }
            }

            gui::end_combo();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The game the editor works on (switching closes the open documents)");
        }

        gui::same_line();
        gui::text_disabled("|");
        gui::same_line();
        for i in 0..self.tool_info.len() {
            let active = i == self.active_tool;
            if active {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            let info = &self.tool_info[i];
            let label = if info.icon.is_empty() { info.title.clone() } else { format!("{}  {}", info.icon, info.title) };
            if gui::button(&format!("{label}##tool{i}")) {
                let id = self.tool_info[i].id.clone();
                self.set_tool(&id);
            }

            if i < 9 && gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}  (Ctrl+{})", self.tool_info[i].title, i + 1));
            }

            if active {
                gui::pop_style_color(1);
            }

            gui::same_line();
        }

        gui::text_disabled("|");
        gui::same_line();
        for (g, (icon, name)) in [(icons::MOUSE_POINTER_2, "Select"), (icons::MOVE, "Move"), (icons::ROTATE_3D, "Rotate"), (icons::SCALE_3D, "Scale")].iter().enumerate() {
            let mode = crate::viewport::GizmoMode::from_index(g);
            let on = self.viewport.gizmo == mode;
            if on {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            if gui::small_button(&format!("{icon}  {name}")) {
                self.viewport.gizmo = mode;
            }

            if on {
                gui::pop_style_color(1);
            }

            gui::same_line();
        }

        self.with_tool_at(self.active_tool, |t, e| t.toolbar(e));
        gui::text_disabled("|");
        gui::same_line();
        gui::push_style_color(gui::COL_TEXT, if self.status_error { crate::theme::v4(crate::theme::error()) } else { gui::style_color(gui::COL_TEXT_DISABLED) });
        gui::text(&self.status.clone());
        gui::pop_style_color(1);
        gui::separator();
    }

    /// "Files this editor opens" and "All files" for the file dialogs.
    fn open_filters(&self) -> Vec<(String, String)> {
        let mut exts: Vec<String> = vec![];
        for e in self.tool_info.iter().flat_map(|t| t.opens.iter()) {
            let p = format!("*{e}");
            if !exts.contains(&p) {
                exts.push(p);
            }
        }

        let mine = if exts.is_empty() { "*.*".to_owned() } else { exts.join(" ") };
        vec![("Files this editor opens".to_owned(), mine), ("All files".to_owned(), "*.*".to_owned())]
    }

    fn draw_menu_bar(&mut self) {
        if !gui::begin_menu_bar() {
            return;
        }

        let active = self.docs.active_id();
        let (can_save, modified, has_doc) = self.docs.active().map_or((false, false, false), |d| (d.can_save(), d.modified() || d.save_wanted(), true));
        if gui::begin_menu("File") {
            if gui::menu_item(&format!("{}  Open from the game packs...", icons::PACKAGE), None, false, true) {
                self.panels.assets = true;
            }

            if gui::menu_item(&format!("{}  Open file...", icons::FOLDER_OPEN), None, false, true)
                && let Some(p) = file_dialog(false, &self.output_dir().join("x"), &self.open_filters())
            {
                match self.open_path(&p.display().to_string(), false, true) {
                    Ok(msg) => self.status(&msg, false),
                    Err(msg) => self.status(&format!("open failed: {msg}"), false),
                }
            }

            if gui::begin_menu(&format!("{}  Game", icons::SWORDS)) {
                let current = self.game().key;
                for game in GameProfile::ALL {
                    if gui::menu_item(game.display_name, None, game.key == current, true)
                        && let Err(m) = self.switch_game(game.key)
                    {
                        self.status(&m, true);
                    }
                }

                gui::end_menu();
            }

            if gui::menu_item(&format!("{}  Mod packs...", icons::LAYERS), None, false, true) {
                self.mods_popup = true;
            }

            if gui::menu_item(&format!("{}  Save", icons::SAVE), Some("Ctrl+S"), false, can_save && modified) {
                let _ = CommandRegistry::run(self, "save");
            }

            if gui::menu_item(&format!("{}  Save as...", icons::FILE), None, false, can_save) {
                let title = self.docs.active().map(|d| d.title()).unwrap_or_default();
                if let Some(p) = file_dialog(true, &self.output_dir().join(title), &[("All files".to_owned(), "*.*".to_owned())])
                    && let Some(d) = self.docs.find_mut(active)
                {
                    let result = d.save(&wce_editor_core::document::SaveTarget { path: p });
                    if result.is_ok() {
                        d.history_mut().mark_saved();
                    }

                    let msg = result.map_or_else(|e| e, |()| "saved".to_owned());
                    self.status(&msg, false);
                }
            }

            let unsaved = self.docs.unsaved().len();
            let label = if unsaved != 0 { format!("{}  Save all ({unsaved})", icons::SAVE_ALL) } else { format!("{}  Save all", icons::SAVE_ALL) };
            if gui::menu_item(&label, Some("Ctrl+Shift+S"), false, unsaved > 0) {
                let _ = CommandRegistry::run(self, "save_all");
            }

            if gui::menu_item(&format!("{}  Close document", icons::X), None, false, has_doc) {
                self.ask_close(active);
            }

            if gui::menu_item("Close all documents", None, false, !self.docs.order().is_empty()) {
                self.ask_close_all();
            }

            if gui::menu_item(&format!("{}  Remount packs", icons::REFRESH_CW), None, false, true) {
                let _ = CommandRegistry::run(self, "remount");
            }

            gui::separator();
            if gui::menu_item(&format!("{}  Save layout", icons::LAYOUT_GRID), None, false, true) {
                self.save_layout();
            }

            if gui::menu_item("Reset layout", None, false, true) {
                self.reset_layout();
            }

            gui::separator();
            if gui::menu_item(&format!("{}  Settings...", icons::SETTINGS), None, false, true) {
                self.show_settings_window(true);
            }

            gui::separator();
            if gui::menu_item(&format!("{}  Quit", icons::LOG_OUT), None, false, true) {
                self.ask_quit();
            }

            gui::end_menu();
        }

        if gui::begin_menu("Edit") {
            let h = self.docs.active().map(|d| (d.history().can_undo(), d.history().can_redo(), d.history().position()));
            let name = |i: usize| self.docs.active().map(|d| d.history().name(i)).unwrap_or_default();
            let undo = match h {
                Some((true, _, p)) => format!("{}  Undo {}", icons::UNDO_2, name(p - 1)),
                _ => format!("{}  Undo", icons::UNDO_2),
            };

            let redo = match h {
                Some((_, true, p)) => format!("{}  Redo {}", icons::REDO_2, name(p)),
                _ => format!("{}  Redo", icons::REDO_2),
            };

            if gui::menu_item(&undo, Some("Ctrl+Z"), false, h.is_some_and(|h| h.0)) {
                let _ = CommandRegistry::run(self, "undo");
            }

            if gui::menu_item(&redo, Some("Ctrl+Y"), false, h.is_some_and(|h| h.1)) {
                let _ = CommandRegistry::run(self, "redo");
            }

            gui::separator();
            if gui::menu_item("Select none", None, false, !self.selection.is_empty()) {
                self.selection.clear();
            }

            self.with_tool_at(self.active_tool, |t, e| t.edit_menu(e));

            gui::end_menu();
        }

        if gui::begin_menu("View") {
            gui::menu_item_toggle("Viewport", &mut self.panels.viewport);
            gui::menu_item_toggle("Outliner", &mut self.panels.outliner);
            gui::menu_item_toggle("Inspector", &mut self.panels.inspector);
            gui::menu_item_toggle("Assets", &mut self.panels.assets);
            gui::menu_item_toggle("Log", &mut self.panels.log);
            gui::menu_item_toggle("History", &mut self.panels.history);
            gui::separator();
            if gui::menu_item("Orbit camera", None, !self.viewport.is_fly(), true) {
                self.viewport.use_orbit();
            }

            if gui::menu_item("Fly camera", None, self.viewport.is_fly(), true) {
                self.viewport.use_fly();
            }

            if gui::menu_item(&format!("{}  Focus selection", icons::FOCUS), Some("F"), false, !self.selection.is_empty()) {
                self.focus_selection();
            }

            gui::menu_item_toggle("Grid", &mut self.viewport.show_grid);
            gui::menu_item_toggle("Compass", &mut self.viewport.show_compass);
            gui::menu_item_toggle("Coordinates", &mut self.viewport.show_coords);
            if let Some(r) = self.renderer.as_mut() {
                gui::menu_item_toggle("Fog", &mut r.settings.fog);
            }

            gui::menu_item_toggle("Snap", &mut self.viewport.snap);
            if gui::begin_menu("Gizmo size") {
                for (size, label) in [(45.0, "Small"), (wce_foundation::settings::DEFAULT_GIZMO_SIZE, "Medium"), (90.0, "Large"), (120.0, "Extra large")] {
                    if gui::menu_item(label, None, self.settings.appearance.gizmo_size == size, true) {
                        self.set_gizmo_size(size);
                    }
                }

                gui::end_menu();
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("How long the move / rotate / scale gizmo's handles are on screen: always the same, however far the camera is or however big the object.");
            }

            gui::separator();
            if gui::begin_menu(&format!("{}  Frame rate cap", icons::GAUGE)) {
                for (fps, label) in [(30, "30 fps"), (60, "60 fps"), (120, "120 fps"), (144, "144 fps"), (0, "Unlimited")] {
                    if gui::menu_item(label, None, self.frame_cap == fps, true) {
                        self.set_frame_cap(fps);
                    }
                }

                gui::end_menu();
            }

            gui::end_menu();
        }

        self.with_tool_at(self.active_tool, |t, e| t.draw_menus(e));
        if gui::begin_menu("Help") {
            gui::menu_item_toggle(&format!("{}  Commands", icons::TERMINAL), &mut self.panels.commands);
            gui::end_menu();
        }

        // Frame rate readout, right aligned (not in scripted runs: their shots must not depend on the machine's speed).
        if self.perf.count() > 10 && !self.clock.fixed_step() {
            let ms = self.perf.mean_frame_ms(60);
            let b = format!("{:.0} fps  {ms:.1} ms", if ms > 0.0 { 1000.0 / ms } else { 0.0 });
            gui::same_line_at(gui::window_width() - gui::calc_text_width(&b) - 16.0);
            gui::text_disabled(&b);
        }

        gui::end_menu_bar();
    }

    /// Stores the game folder in the settings and mounts: what the first-run dialog's button does, also the `setup` command.
    ///
    /// # Errors
    ///
    /// The packs can't be mounted.
    pub fn setup(&mut self, game_dir: &str) -> Result<String, String> {
        self.settings.game_mut().dir = Some(PathBuf::from(game_dir));
        match self.settings.save() {
            Ok(()) => log::info(format!("Saved {}", self.settings.file().display())),
            Err(err) => log::error(format!("Cannot write {}: {err}", self.settings.file().display())),
        }

        // The environment may still override game_dir.
        if !self.start() {
            return Err(self.first_run_message.clone());
        }

        self.with_tool_at(self.active_tool, |t, e| t.activated(e));
        Ok(format!("mounted {} packs", self.vfs.as_ref().map_or(0, |v| v.packs().len())))
    }

    /// The folder the first-run dialog offers for `game`: the settings' if it is that game's, else the one in the Steam libraries.
    fn first_run_folder(&self, game: &GameProfile) -> String {
        let set = self.settings.game_dir().filter(|d| game.folder_problem(d).is_none());
        set.or_else(|| detect_game_folder(game)).map(|p| p.display().to_string()).unwrap_or_default()
    }

    /// The first-run dialog's choice of game: choosing one shows its folder (the settings', else the one found in the Steam libraries).
    fn game_combo(&mut self) {
        let current = self.game();
        gui::set_next_item_width(470.0);
        if gui::begin_combo("Game", current.display_name) {
            for game in GameProfile::ALL {
                if gui::selectable(game.display_name, game.key == current.key, 0) && game.key != current.key {
                    self.settings.game = game.key.to_owned();
                    self.first_run_message.clear();
                    self.mod_checks_reset();
                    self.first_run_game = self.first_run_folder(game);
                }
            }

            gui::end_combo();
        }
    }

    fn draw_first_run(&mut self) {
        gui::open_popup("First run");
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_ALWAYS, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(640.0, 0.0), 0);
        if !gui::begin_popup_modal("First run", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let game = self.game();
        gui::text_wrapped(&format!("Warscape Community Editor needs {}. Nothing from the game is copied: files are read from your installation.", game.display_name));
        self.game_combo();
        gui::spacing();
        gui::set_next_item_width(470.0);
        gui::input_text("Game folder", None, &mut self.first_run_game, 1024, 0);
        gui::same_line();
        if gui::button("Browse##Game folder")
            && let Some(p) = pick_folder(&format!("{} folder", game.display_name))
        {
            self.first_run_game = p.display().to_string();
        }

        if gui::button("Detect")
            && let Some(g) = detect_game_folder(self.game())
        {
            self.first_run_game = g.display().to_string();
        }

        gui::same_line();
        let problem = if self.first_run_game.is_empty() { Some("no game folder".to_owned()) } else { self.game().folder_problem(Path::new(&self.first_run_game)) };
        let game_ok = problem.is_none();
        if let Some(problem) = problem {
            gui::text_colored(crate::theme::error(), &problem);
        }

        if !self.first_run_message.is_empty() {
            gui::push_text_wrap_pos(600.0);
            gui::text_disabled(&self.first_run_message.clone());
            gui::pop_text_wrap_pos();
        }

        if game_ok {
            gui::spacing();
            gui::text_wrapped("Mod packs to read besides the game's own (optional; you can change this later in File > Mod packs):");
            let game = self.first_run_game.clone();
            self.mod_pack_checks(&game);
        }

        gui::spacing();
        gui::begin_disabled(!game_ok);
        if gui::button("Save and continue") {
            gui::close_current_popup();
            self.settings.game_mut().mod_packs = self.checked_mod_packs();
            let game = self.first_run_game.clone();
            if let Err(msg) = self.setup(&game) {
                self.first_run_message = msg;
            }
        }

        gui::end_disabled();
        gui::end_popup();
    }

    pub fn dockspace_id(&self) -> gui::ImGuiID {
        self.dockspace
    }

    pub fn is_mounted(&self) -> bool {
        self.vfs.is_some()
    }

    pub fn active_document(&mut self) -> Option<&mut dyn Document> {
        self.docs.active_mut()
    }

    /// Re-activates every tool (after the packs were mounted again).
    pub fn activate_all_tools(&mut self) {
        for i in 0..self.tools.len() {
            self.with_tool_at(i, |t, e| t.activated(e));
        }
    }

    pub(crate) fn tool_titles(&self) -> Vec<(String, String)> {
        self.tool_info.iter().map(|t| (t.id.clone(), t.title.clone())).collect()
    }

    pub(crate) fn viewport_and_tool_claims(&mut self) -> bool {
        self.with_tool_at(self.active_tool, |t, e| t.claims_left_button(e)).unwrap_or(false)
    }

    pub(crate) fn tool_viewport_input(&mut self, ev: &crate::tool::ViewportEvent) -> bool {
        self.with_tool_at(self.active_tool, |t, e| t.viewport_input(e, ev)).unwrap_or(false)
    }

    pub fn has_active_tool(&self) -> bool {
        !self.tools.is_empty()
    }
}

/// A game's output folder when the settings name none: <Documents>/AttilaWeatherEditor/out for Attila (the old weather editor's),
/// <Documents>/WCE/<game>/out for the other games.
pub fn default_output_dir_for(game: &GameProfile) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/tmp" }), PathBuf::from);
    match game.key {
        "attila" => home.join("Documents").join("AttilaWeatherEditor").join("out"),
        key => home.join("Documents").join("WCE").join(key).join("out"),
    }
}
