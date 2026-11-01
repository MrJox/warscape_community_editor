//! A tool is a mode of the editor (Weather, Tile, and the future Unit, VFX, Terrain and Cinematic editors). It decides which documents it opens,
//! which panels and menus it adds, what the viewport does with clicks, and the default dock layout. The shell (docking, outliner, inspector,
//! asset browser, log, history, viewport, gizmos, commands, layouts) is shared.
//!
//! The editor owns its tools; while it calls one, the tool is lent out of the editor, so the tool and the editor can both be changed.

use std::any::Any;

use wce_editor_core::command_registry::CommandRegistry;
use wce_editor_core::document::Document;
use wce_foundation::settings::Settings;
use wce_scene::render_scene::{EntityId, SourceRef};

use crate::editor::Editor;
use crate::gui;

/// What the viewport tells the active tool about the mouse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewportEventKind {
    /// A press and release without moving (the default selects).
    Click,

    /// The left button, every frame it is held; a tool that returns true from Press owns the gesture (no camera rotation, no click).
    Press,
    Drag,
    Release,

    /// Every frame the cursor is over the viewport.
    #[default]
    Hover,

    /// A right click (no drag): what is under the cursor is in `hit` / `hit_source`.
    ContextMenu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewportEvent {
    pub kind: ViewportEventKind,

    /// Pixel in the viewport image.
    pub x: i32,
    pub y: i32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,

    /// What is under the cursor (clicks only), 0 for nothing.
    pub hit: EntityId,

    /// The document element behind `hit`.
    pub hit_source: SourceRef,

    /// 0 left, 1 right, 2 middle.
    pub button: i32,
}

/// The standard layout every tool starts from: outliner left, inspector right, assets / log / history below, the viewport in the middle; a
/// tool's main panel, when it has one named like the tool, tabs with the inspector.
pub fn default_layout(dockspace: gui::ImGuiID, title: &str) {
    let center = dockspace;
    let (left, center) = gui::dock_builder_split(center, gui::DIR_LEFT, 0.20);
    let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.25);
    let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.25);
    gui::dock_builder_dock_window("Viewport", center);
    gui::dock_builder_dock_window("Outliner", left);
    gui::dock_builder_dock_window("Inspector", right);
    gui::dock_builder_dock_window("Assets", bottom);
    gui::dock_builder_dock_window("Log", bottom);
    gui::dock_builder_dock_window("History", bottom);
    gui::dock_builder_dock_window(title, right);
}

pub trait Tool: Any {

    /// "weather", "tile", ...
    fn id(&self) -> &str;
    fn title(&self) -> &str;

    /// The tool's icon in the switcher (one of `icons`); none by default.
    fn icon(&self) -> &str {
        ""
    }

    /// File extensions (with the dot, lower case) this tool opens from the packs or from disk; the asset browser filters by them.
    fn opens(&self) -> Vec<String> {
        vec![]
    }

    /// False for a tool that has its own canvas and no use for the 3D viewport.
    fn shows_viewport(&self) -> bool {
        true
    }

    /// Builds the first-run layout in the dockspace. The default is the standard layout.
    fn default_layout(&self, dockspace: gui::ImGuiID) {
        default_layout(dockspace, self.title());
    }

    /// Extra panels of the tool, every frame while it is active.
    fn draw_panels(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// Extra menus of the tool (inside the menu bar), every frame while it is active.
    fn draw_menus(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// Buttons of the tool in the toolbar, after the gizmo's.
    fn toolbar(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// Items of the tool at the end of the Edit menu.
    fn edit_menu(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// Widgets over the viewport image (inside the viewport window, after the image): `origin` and `size` are the image's, in screen
    /// pixels. A child window there takes the mouse before the viewport.
    fn viewport_overlay(&mut self, editor: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        let _ = (editor, origin, size);
    }

    /// The tool's part of the viewport's camera chip's popup (crate::chips), after the editor's camera options.
    fn camera_chip(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// The tool's part of the viewport's snap chip's popup (crate::chips), after the gizmo's steps.
    fn snap_chip(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// What of the tool's snapping is on, for the snap chip's tooltip (it is lit then); None when nothing is.
    fn snapping(&mut self, editor: &mut Editor) -> Option<String> {
        let _ = editor;
        None
    }

    /// The Inspector's contents instead of the selection's (what the tool is about to make, say): true when it filled it.
    fn inspect(&mut self, editor: &mut Editor, inspector: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        let _ = (editor, inspector);
        false
    }

    /// What the tool holds that no document saves (a new vegetation set not saved yet, say), one line each: quitting asks first and lists it
    /// as lost.
    fn unsaved_work(&self) -> Vec<String> {
        vec![]
    }

    /// True when the event was handled (the default click-to-select is then skipped).
    fn viewport_input(&mut self, editor: &mut Editor, event: &ViewportEvent) -> bool {
        let _ = (editor, event);
        false
    }

    /// True while the left mouse button belongs to the tool (a brush is active, a polygon is being drawn): Press always goes to the tool, the
    /// left button does not rotate the camera and a click does not select.
    fn claims_left_button(&self, editor: &Editor) -> bool {
        let _ = editor;
        false
    }

    /// Whether a left drag that starts off the gizmo (and off what the tool takes the press for) draws a selection box instead of turning
    /// the camera (`box_drag`); a click without a drag still selects. The camera keeps the right and middle buttons.
    fn left_drag_selects(&self, editor: &Editor) -> bool {
        let _ = editor;
        false
    }

    /// The selection box of a left drag (`left_drag_selects`): from the press to the mouse, image pixels; `done` on the release, which
    /// selects what is inside (Shift adds, Ctrl takes away).
    fn box_drag(&mut self, editor: &mut Editor, from: (i32, i32), to: (i32, i32), done: bool) {
        let _ = (editor, from, to, done);
    }

    /// Where the translate gizmo puts `item` (world metres) when its drag asks for `to` (it was at `from`): the tool's snapping. An axis
    /// that did not move (`to` equals `from` there) is the tool's to leave alone. The default does not snap.
    fn snap_gizmo_move(&self, editor: &Editor, item: SourceRef, from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
        let _ = (editor, item, from);
        to
    }

    /// Where the rotate gizmo leaves `item` (its world matrix `moved`, from `frame` at the drag's start): the tool's snapping. The default
    /// leaves it.
    fn snap_gizmo_rotate(&self, editor: &Editor, item: SourceRef, frame: &wce_foundation::math::Float4x4, moved: wce_foundation::math::Float4x4) -> wce_foundation::math::Float4x4 {
        let _ = (editor, item, frame);
        moved
    }

    /// The tool's commands; they reach the tool through `Editor::with_tool`.
    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        let _ = commands;
    }

    /// True when `update` puts the lighting into the scene's environment slot itself (the weather tool). For every other tool the editor
    /// lights the scene: with the previewed weather document if there is one, the default environment otherwise.
    fn lights_scene(&self) -> bool {
        false
    }

    /// Once per frame while the tool is active, after the documents were re-published and before the viewport renders.
    fn update(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// The editor switched to another game: drop what belongs to the old one. Its documents are closed already; once the new game's
    /// files are mounted every tool is activated again.
    fn game_changed(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// The tool became the active one (open a default document, set the camera, ...).
    fn activated(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    /// Opens a file of the packs or a disk file this tool claims.
    ///
    /// # Errors
    ///
    /// Why it can't be opened.
    fn open(&mut self, editor: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        let _ = (editor, from_packs);
        Err(format!("{} does not open {path}", self.title()))
    }

    /// True when the tool has a page in the Settings window (`draw_settings`).
    fn has_settings(&self) -> bool {
        false
    }

    /// The fields of the tool's settings table that are about the current game's files (recent files, per-file choices): each game keeps
    /// its own (`Settings::set_game_fields`). The others are the editor's, the same in every game.
    fn game_settings_fields(&self) -> &'static [&'static str] {
        &[]
    }

    /// The actions its keys do (`shortcuts`: rebindable in Settings > Shortcuts; read with `shortcuts::pressed`).
    fn shortcuts(&self) -> &'static [crate::shortcuts::Action] {
        &[]
    }

    /// The tool's page in the Settings window. It edits `draft`, the window's copy of the settings (the tool's table: `Settings::tool` and
    /// `Settings::set_tool`); OK or Apply put it in place and call `settings_applied`.
    fn draw_settings(&mut self, draft: &mut Settings) {
        let _ = draft;
    }

    /// The settings window applied new settings: take up what changed.
    fn settings_applied(&mut self, editor: &mut Editor) {
        let _ = editor;
    }

    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A registered tool with no features yet: a panel that says what is planned. Keeps the switcher, layout files and document routing
/// exercised from the first day.
pub struct PlaceholderTool {
    id: String,
    title: String,
    icon: &'static str,
    plan: String,
}

impl PlaceholderTool {
    pub fn new(id: &str, title: &str, icon: &'static str, plan: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), icon, plan: plan.to_owned() }
    }
}

impl Tool for PlaceholderTool {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn icon(&self) -> &str {
        self.icon
    }

    fn draw_panels(&mut self, _: &mut Editor) {
        gui::set_next_window_size(gui::v2(420.0, 160.0), gui::COND_FIRST_USE_EVER);
        if gui::begin(&self.title, None, 0) {
            gui::text_wrapped(&self.plan);
            gui::separator();
            gui::text_disabled("Registered so that the tool switcher, the layout files and document routing are exercised. No features yet.");
        }

        gui::end();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
