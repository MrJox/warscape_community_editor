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
use wce_foundation::{log, wce_prof};
use wce_imgui::renderer::D3D11Renderer;
use wce_render::files::Files;
use wce_render::renderer::Renderer;
use wce_scene::camera::Camera;
use wce_scene::environment::read_graphics_options;
use wce_scene::render_scene::{DocumentId, EnvironmentSlot, RenderScene};
use wce_vfs::{Config, GameProfile, Vfs, normalize};

use crate::gui;
use crate::panels::PanelState as PanelData;
use crate::perf::PerfStats;
use crate::steam_detect::{detect_attila_folder, file_dialog, pick_folder};
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

/// What the shell keeps of a tool while it is lent out.
struct ToolInfo {
    id: String,
    title: String,
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
    pub(crate) commands: CommandRegistry<Editor>,
    pub(crate) script: ScriptRunner,
    pub(crate) script_lines: Vec<String>,
    pub viewport: Viewport,
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
    pub(crate) preview_doc: DocumentId,
    pub(crate) cam_mark: Option<Camera>,
    output_gamma: f32,
    output_brightness: f32,
    prompt: PromptKind,
    prompt_docs: Vec<DocumentId>,
    prompt_close: Vec<DocumentId>,
    prompt_error: String,
    mouse_queue: VecDeque<MouseState>,
    pub(crate) mouse_cur: MouseState,
    mouse_active: bool,

    /// Keys the `key` command holds down for a couple of frames.
    pub(crate) test_keys: Vec<(i32, i32)>,
    first_run_game: String,
    first_run_message: String,

    /// Frame times (the app loop records them).
    pub perf: PerfStats,
    pub(crate) wait_loaded_start: Option<Instant>,

    /// The app loop presents with it (command `vsync`, environment WCE_VSYNC=0).
    pub vsync: bool,
    pub panels: Panels,
    pub(crate) panel_data: PanelData,

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
            dev: dev.clone(), ctx: ctx.clone(), settings: Settings::load(&Settings::default_dir()), vfs: None, renderer: None, scene: RenderScene::default(),
            docs: DocumentManager::default(), selection: Selection::default(), commands: CommandRegistry::default(), script: ScriptRunner::default(), script_lines: vec![],
            viewport: Viewport::default(), clock: Clock::live(), tools: vec![], tool_info: vec![], active_tool: 0, pending_tool: None, layout_pending_default: false,
            layout_loaded: false, dockspace: 0, quit: false, exit_code: 0, frame: 0, status: String::new(), status_error: false, first_run: false, preview_doc: 0,
            cam_mark: None, output_gamma: 2.2, output_brightness: 1.0, prompt: PromptKind::None, prompt_docs: vec![], prompt_close: vec![], prompt_error: String::new(),
            mouse_queue: VecDeque::new(), mouse_cur: MouseState::default(), mouse_active: false, test_keys: vec![], first_run_game: String::new(),
            first_run_message: String::new(), perf: PerfStats::default(), wait_loaded_start: None, vsync: true, panels: Panels::default(),
            panel_data: PanelData::default(), images: Default::default(), input_origin: None, pending_shot: None,
        };

        crate::commands::register_builtin(&mut e.commands);
        e
    }

    // ---- setup

    pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
        tool.register_commands(&mut self.commands);
        self.tool_info.push(ToolInfo { id: tool.id().to_owned(), title: tool.title().to_owned(), opens: tool.opens() });
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

    pub fn settings(&mut self) -> &mut Settings {
        &mut self.settings
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

    pub fn tool_ids(&self) -> Vec<String> {
        self.tool_info.iter().map(|t| t.id.clone()).collect()
    }

    pub fn tool_title(&self, id: &str) -> Option<&str> {
        self.tool_info.iter().find(|t| t.id == id).map(|t| t.title.as_str())
    }

    pub fn active_tool_id(&self) -> &str {
        self.tool_info.get(self.active_tool).map_or("", |t| t.id.as_str())
    }

    pub fn active_tool_opens(&self) -> &[String] {
        self.tool_info.get(self.active_tool).map_or(&[], |t| t.opens.as_slice())
    }

    /// Mounts the packs and creates the renderer. False (with a message in the log) when the game is not configured: the first-run dialog opens.
    pub fn start(&mut self) -> bool {
        let game_dir = self.settings.get("game_dir", Some("WCE_GAME")).or_else(|| std::env::var("ATTILA_DIR").ok());
        log::info(format!("Data paths: game={}", game_dir.as_deref().unwrap_or("(unset)")));
        let mounted = game_dir.ok_or_else(|| "the game folder is not set".to_owned()).and_then(|dir| {
            Vfs::mount(&Config::new(PathBuf::from(dir), GameProfile::ATTILA)).map_err(|e| e.to_string())
        });

        match mounted.and_then(|vfs| {
            let vfs = Arc::new(vfs);
            let renderer = Renderer::new(&self.dev, &self.ctx, vfs.clone()).map_err(|e| e.0)?;
            Ok((vfs, renderer))
        }) {
            Ok((vfs, mut renderer)) => {
                let go = read_graphics_options();
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
                    self.first_run_game = self.settings.get("game_dir", Some("WCE_GAME"))
                        .or_else(|| detect_attila_folder().map(|p| p.display().to_string())).unwrap_or_default();
                }

                false
            }
        }
    }

    // ---- layouts

    pub fn layout_file(&self, tool_id: &str) -> PathBuf {
        Settings::default_dir().join("layouts").join(format!("{tool_id}.ini"))
    }

    /// Where "Save" writes loose files (under their in-pack path): output_dir in wce.ini, else <Documents>/AttilaWeatherEditor/out as the old
    /// weather editor did.
    pub fn output_dir(&self) -> PathBuf {
        if let Some(v) = self.settings.get("output_dir", Some("WCE_OUTPUT_DIR")) {
            return PathBuf::from(v);
        }

        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/tmp" }), PathBuf::from);
        home.join("Documents").join("AttilaWeatherEditor").join("out")
    }

    pub fn set_output_dir(&mut self, dir: &Path, persist: bool) {
        self.settings.set("output_dir", &dir.display().to_string());
        if persist {
            let _ = self.settings.save();
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

    fn apply_tool_switch(&mut self) {
        match self.pending_tool.take() {
            Some(i) if i != self.active_tool && i < self.tools.len() => {
                self.save_layout();
                self.active_tool = i;
                self.layout_loaded = false;
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

    /// The scene is lit by the document the weather tool edits while the weather tool is active. In every other tool, `id` (an .environment or
    /// .lighting document, 0 for none) lights it: "Preview this weather on this tile". The preview follows the document's edits live and ends
    /// when it is closed.
    ///
    /// # Errors
    ///
    /// No such document, or one that does not light the scene.
    pub fn set_weather_preview(&mut self, id: DocumentId) -> Result<String, String> {
        if id == 0 {
            self.preview_doc = 0;
            return Ok("weather preview off".to_owned());
        }

        let d = self.docs.find(id).ok_or("no such document")?;
        if d.environment().is_none() {
            return Err(format!("{} does not light the scene", d.title()));
        }

        self.preview_doc = id;
        Ok(format!("previewing {}", d.title()))
    }

    pub fn weather_preview(&self) -> DocumentId {
        self.preview_doc
    }

    /// The scene's lighting outside the weather tool: the previewed weather document, else the default environment.
    fn update_lighting(&mut self) {
        if self.with_tool_at(self.active_tool, |t, _| t.lights_scene()).unwrap_or(true) {
            return;
        }

        let previewed = (self.preview_doc != 0).then(|| self.docs.find(self.preview_doc)).flatten();
        match previewed.and_then(|d| d.environment().map(|p| (d.id(), p))) {
            Some((owner, mut p)) => {
                p.output_gamma = self.output_gamma;
                p.output_brightness = self.output_brightness;
                let slot = &mut self.scene.environment;
                slot.active = Some(p);
                slot.blend = None;
                slot.blend_t = 0.0;
                slot.owner = owner;
            }

            None => {
                // The previewed document was closed.
                if self.preview_doc != 0 && previewed.is_none() {
                    self.preview_doc = 0;
                }

                self.scene.environment = EnvironmentSlot::default();
            }
        }
    }

    // ---- closing and quitting

    /// Closes a document without asking (the selection drops its items too).
    pub fn close_document(&mut self, id: DocumentId) {
        self.selection.remove_document(id);
        if id == self.preview_doc {
            self.preview_doc = 0;
        }

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

    /// The `quit` command leaves at once, as scripts expect; this one asks about unsaved changes.
    pub fn ask_quit(&mut self) {
        self.prompt_docs = self.docs.unsaved();
        if self.prompt_docs.is_empty() {
            self.request_quit(0);
            return;
        }

        self.prompt = PromptKind::Quit;
        self.prompt_error.clear();
        self.prompt_close.clear();
    }

    /// The window's close button: true when the prompt took over (there are unsaved changes), false to let the window close.
    pub fn window_close_requested(&mut self) -> bool {
        if self.docs.unsaved().is_empty() {
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
        } else {
            self.request_quit(0);
        }

        self.prompt_docs.clear();
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

        gui::text(if quit { "These documents have unsaved changes:" } else { "This document has unsaved changes:" });
        for &id in &self.prompt_docs {
            if let Some(d) = self.docs.find(id) {
                gui::bullet_text(&format!("{}  ({})", d.title(), d.kind()));
            }
        }

        if !self.prompt_error.is_empty() {
            gui::text_colored([1.0, 0.45, 0.4, 1.0], &self.prompt_error.clone());
        }

        gui::spacing();
        if gui::button(if quit { "Save all and quit" } else { "Save and close" }) {
            let _ = self.answer_prompt(Answer::Save);
        }

        gui::same_line();
        if gui::button(if quit { "Quit without saving" } else { "Discard changes" }) {
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
        if self.first_run {
            self.draw_first_run();
            return;
        }

        // Shortcuts (not while typing into a field).
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

            if io.KeyCtrl && gui::is_key_pressed(gui::KEY_S, false) {
                let _ = CommandRegistry::run(self, if io.KeyShift { "save_all" } else { "save" });
            }

            if !io.KeyCtrl && self.viewport.hovered {
                let fly = self.viewport.is_fly();
                for (key, mode) in [
                    (gui::KEY_W, crate::viewport::GizmoMode::Translate), (gui::KEY_E, crate::viewport::GizmoMode::Rotate), (gui::KEY_R, crate::viewport::GizmoMode::Scale),
                    (gui::KEY_Q, crate::viewport::GizmoMode::Off),
                ] {
                    if gui::is_key_pressed(key, false) && !fly {
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
        for i in 0..self.tool_info.len() {
            let active = i == self.active_tool;
            if active {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            let mut label = self.tool_info[i].title.clone();
            if i < 9 {
                label += &format!("  Ctrl+{}", i + 1);
            }

            if gui::button(&format!("{label}##tool{i}")) {
                let id = self.tool_info[i].id.clone();
                self.set_tool(&id);
            }

            if active {
                gui::pop_style_color(1);
            }

            gui::same_line();
        }

        gui::text_disabled("|");
        gui::same_line();
        for (g, name) in ["None", "Move", "Rotate", "Scale"].iter().enumerate() {
            let mode = crate::viewport::GizmoMode::from_index(g);
            let on = self.viewport.gizmo == mode;
            if on {
                gui::push_style_color(gui::COL_BUTTON, gui::style_color(gui::COL_BUTTON_ACTIVE));
            }

            if gui::small_button(name) {
                self.viewport.gizmo = mode;
            }

            if on {
                gui::pop_style_color(1);
            }

            gui::same_line();
        }

        gui::text_disabled("|");
        gui::same_line();
        gui::push_style_color(gui::COL_TEXT, if self.status_error { gui::v4(1.0, 0.45, 0.4, 1.0) } else { gui::style_color(gui::COL_TEXT_DISABLED) });
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
        let (can_save, modified, has_doc) = self.docs.active().map_or((false, false, false), |d| (d.can_save(), d.modified(), true));
        if gui::begin_menu("File") {
            if gui::menu_item("Open from the game packs...", None, false, true) {
                self.panels.assets = true;
            }

            if gui::menu_item("Open file...", None, false, true)
                && let Some(p) = file_dialog(false, &self.output_dir().join("x"), &self.open_filters())
            {
                match self.open_path(&p.display().to_string(), false, true) {
                    Ok(msg) => self.status(&msg, false),
                    Err(msg) => self.status(&format!("open failed: {msg}"), false),
                }
            }

            if gui::menu_item("Save", Some("Ctrl+S"), false, can_save && modified) {
                let _ = CommandRegistry::run(self, "save");
            }

            if gui::menu_item("Save as...", None, false, can_save) {
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
            let label = if unsaved != 0 { format!("Save all ({unsaved})") } else { "Save all".to_owned() };
            if gui::menu_item(&label, Some("Ctrl+Shift+S"), false, unsaved > 0) {
                let _ = CommandRegistry::run(self, "save_all");
            }

            if gui::menu_item("Close document", None, false, has_doc) {
                self.ask_close(active);
            }

            if gui::menu_item("Close all documents", None, false, !self.docs.order().is_empty()) {
                self.ask_close_all();
            }

            if gui::menu_item("Remount packs", None, false, true) {
                let _ = CommandRegistry::run(self, "remount");
            }

            gui::separator();
            if gui::menu_item("Save layout", None, false, true) {
                self.save_layout();
            }

            if gui::menu_item("Reset layout", None, false, true) {
                self.reset_layout();
            }

            gui::separator();
            if gui::menu_item("Quit", None, false, true) {
                self.ask_quit();
            }

            gui::end_menu();
        }

        if gui::begin_menu("Edit") {
            let h = self.docs.active().map(|d| (d.history().can_undo(), d.history().can_redo(), d.history().position()));
            let name = |i: usize| self.docs.active().map(|d| d.history().name(i)).unwrap_or_default();
            let undo = match h {
                Some((true, _, p)) => format!("Undo {}", name(p - 1)),
                _ => "Undo".to_owned(),
            };

            let redo = match h {
                Some((_, true, p)) => format!("Redo {}", name(p)),
                _ => "Redo".to_owned(),
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

            if gui::menu_item("Focus selection", Some("F"), false, !self.selection.is_empty()) {
                self.focus_selection();
            }

            gui::menu_item_toggle("Grid", &mut self.viewport.show_grid);
            gui::menu_item_toggle("Snap", &mut self.viewport.snap);
            gui::end_menu();
        }

        self.with_tool_at(self.active_tool, |t, e| t.draw_menus(e));
        if gui::begin_menu("Help") {
            gui::menu_item_toggle("Commands", &mut self.panels.commands);
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

    /// Stores the game folder in wce.ini and mounts: what the first-run dialog's button does, also the `setup` command.
    ///
    /// # Errors
    ///
    /// The packs can't be mounted.
    pub fn setup(&mut self, game_dir: &str) -> Result<String, String> {
        self.settings.set("game_dir", game_dir);
        match self.settings.save() {
            Ok(()) => log::info(format!("Saved {}", self.settings.file().display())),
            Err(_) => log::error(format!("Cannot write {}", self.settings.file().display())),
        }

        // The environment may still override game_dir.
        if !self.start() {
            return Err(self.first_run_message.clone());
        }

        self.with_tool_at(self.active_tool, |t, e| t.activated(e));
        Ok(format!("mounted {} packs", self.vfs.as_ref().map_or(0, |v| v.packs().len())))
    }

    fn draw_first_run(&mut self) {
        gui::open_popup("First run");
        let vp = gui::main_viewport();
        gui::set_next_window_pos(gui::v2(vp.Pos.x + vp.Size.x * 0.5, vp.Pos.y + vp.Size.y * 0.5), gui::COND_ALWAYS, gui::v2(0.5, 0.5));
        gui::set_next_window_size(gui::v2(640.0, 0.0), 0);
        if !gui::begin_popup_modal("First run", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        gui::text_wrapped("Warscape Community Editor needs Total War: Attila. Nothing from the game is copied: files are read from your installation.");
        gui::spacing();
        gui::set_next_item_width(470.0);
        gui::input_text("Game folder", None, &mut self.first_run_game, 1024, 0);
        gui::same_line();
        if gui::button("Browse##Game folder")
            && let Some(p) = pick_folder("Total War Attila folder")
        {
            self.first_run_game = p.display().to_string();
        }

        if gui::button("Detect")
            && let Some(g) = detect_attila_folder()
        {
            self.first_run_game = g.display().to_string();
        }

        gui::same_line();
        let game_ok = !self.first_run_game.is_empty() && Path::new(&self.first_run_game).join("data").exists();
        if !game_ok {
            gui::text_colored([1.0, 0.5, 0.4, 1.0], "no data folder in the game folder");
        }

        if !self.first_run_message.is_empty() {
            gui::push_text_wrap_pos(600.0);
            gui::text_disabled(&self.first_run_message.clone());
            gui::pop_text_wrap_pos();
        }

        gui::spacing();
        gui::begin_disabled(!game_ok);
        if gui::button("Save and continue") {
            gui::close_current_popup();
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
