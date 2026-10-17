//! The UI tool: the canvas, the raw XML pane, the fields of a component, the menu and the commands. Everything edits the document's one
//! model, so the canvas, the XML and the fields always show the same thing.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::path::PathBuf;

use dear_imgui_sys as sys;
use wce_editor::editor::Editor;
use wce_editor::gui::{self, Vec2, col32, v2};
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_scene::render_scene::{DocumentId, SourceRef};
use wce_ui_model::draw::{DrawKind, DrawOptions, pieces};
use wce_ui_model::fonts::FontLibrary;
use wce_ui_model::DrawItem;
use wce_ui_model::images;
use wce_ui_model::related::{self, RelKind, Relation, RelationIndex};
use wce_ui_model::skins::{self, Skin};
use wce_ui_model::{Entry, EntryKind, UiModel};

use crate::document::{Source, UiDocument, index_of, item_of};

#[derive(Clone, Copy, Debug)]
struct View {
    zoom: f32,
    /// Layout position at the canvas's top left corner.
    ox: f32,
    oy: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Move,
    /// 0..8: TL, T, TR, R, BR, B, BL, L.
    Resize(u8),
}

#[derive(Clone, Debug)]
struct Drag {
    kind: DragKind,
    entry: usize,
    start_mouse: (f32, f32),
    /// Offset in the parent and size when the gesture began.
    start: (i32, i32, i32, i32),
    key: u64,
    last: (i32, i32, i32, i32),
}

#[derive(Default)]
struct XmlPane {
    text: String,
    shown: (DocumentId, u64, Option<u64>, bool),
    edited: bool,
    last_edit: f64,
    error: String,
    whole: bool,
}

#[derive(Default)]
pub struct UiTool {
    view: Option<View>,
    fitted: DocumentId,
    show_outlines: bool,
    hide_images: bool,
    hide_text: bool,
    drag: Option<Drag>,
    gesture: u64,
    xml: XmlPane,
    missing: HashSet<String>,
    loads: u32,
    draw_opts: DrawOptions,
    fonts: FontLibrary,
    skin: String,
    skins: Option<Vec<Skin>>,
    pack_path: String,
    pack_inside: String,
    pack_message: String,
    open_pack_popup: bool,
    anim: Option<AnimPlay>,
    related_on: bool,
    related_generic: bool,
    /// Paths of related layouts the user switched off.
    related_off: HashSet<String>,
    rel_index: Option<Rc<RelationIndex>>,
    rel_list: Option<(String, Vec<Relation>)>,
    rel_models: HashMap<String, Option<Rc<UiModel>>>,
    rel_layers: Option<(String, Rc<Layers>)>,
}

/// Reference layers: drawn behind the layout (parents) and over it (children).
#[derive(Default)]
struct Layers {
    below: Vec<DrawItem>,
    above: Vec<DrawItem>,
}

/// An animation being previewed.
#[derive(Clone, Debug)]
struct AnimPlay {
    /// The component that owns it.
    path: Vec<usize>,
    name: String,
    /// Milliseconds since it started.
    t: f32,
    /// Milliseconds until it and what it triggers are done.
    len: f32,
    playing: bool,
    looping: bool,
}

/// Screen sizes to preview the layout at.
const SCREEN_SIZES: [(i32, i32); 7] = [(1024, 768), (1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160)];

const HANDLES: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];

fn rgba(c: [u8; 4]) -> u32 {
    col32(c[0], c[1], c[2], c[3])
}

/// The editor's active UI document, else the first one.
fn active_doc(e: &Editor) -> Option<DocumentId> {
    let a = e.docs.active_id();
    if e.docs.find_as::<UiDocument>(a).is_some() {
        return Some(a);
    }

    e.docs.order().iter().copied().find(|&i| e.docs.find_as::<UiDocument>(i).is_some())
}

fn selected(e: &Editor, doc: DocumentId) -> Option<usize> {
    e.selection.items().iter().find(|s| s.doc == doc).and_then(|s| index_of(s.item))
}

fn select(e: &mut Editor, doc: DocumentId, entry: usize) {
    e.selection.set(SourceRef { doc, item: item_of(entry) });
}

fn ui_doc(e: &mut Editor, id: DocumentId) -> Option<&mut UiDocument> {
    e.docs.find_as_mut::<UiDocument>(id)
}

impl UiTool {
    // ---- images

    /// The key a picture is stored under: the skin and the path as the layout spells it.
    fn image_key(&self, path: &str) -> String {
        format!("{}|{}", self.skin, path)
    }

    fn want_image(&mut self, e: &mut Editor, path: &str) {
        let key = self.image_key(path);
        if self.loads >= 6 || self.missing.contains(&key) || e.images.knows(&key) {
            return;
        }

        self.loads += 1;
        let Some(vfs) = e.vfs().cloned() else {
            self.missing.insert(key);
            return;
        };

        let skin = (!self.skin.is_empty()).then_some(self.skin.as_str());
        for cand in skins::candidates(path, skin) {
            if let Ok(bytes) = vfs.read(&cand) {
                if let Ok(img) = images::decode(&cand, &bytes) {
                    e.images.submit(&key, img.w, img.h, img.px);
                    return;
                }
            }
        }

        self.missing.insert(key);
    }

    /// The skin folders, read once.
    fn load_skins(&mut self, e: &Editor) {
        if self.skins.is_none() {
            if let Some(vfs) = e.vfs() {
                self.skins = Some(skins::list(vfs));
            }
        }
    }

    // ---- what is drawn: state, skin, visibility

    fn options_row(&mut self, e: &mut Editor, _id: DocumentId) {
        self.load_skins(e);
        let mut hidden = !self.draw_opts.show_hidden;
        if gui::checkbox("Honour hidden", &mut hidden) {
            self.draw_opts.show_hidden = !hidden;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Components the layout hides when it loads (tab pages, cards, timers) are left out, as in the game.");
        }

        gui::same_line();
        let mut templates = !self.draw_opts.show_templates;
        if gui::checkbox("Skip templates", &mut templates) {
            self.draw_opts.show_templates = !templates;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Components whose names end in 'template' are filled in by the game (list items, player slots): not drawn in place.");
        }

        // screen size: the root is sized to it and docked components follow (the game's SizeToScreen)
        gui::same_line();
        gui::set_next_item_width(150.0);
        let cur = match self.draw_opts.screen {
            Some((w, h)) => format!("{w} x {h}"),
            None => "layout size".to_owned(),
        };

        if gui::begin_combo("Screen##screen", &cur) {
            if gui::selectable("layout size", self.draw_opts.screen.is_none(), 0) {
                self.draw_opts.screen = None;
                self.fitted = DocumentId::default();
            }

            for (w, h) in SCREEN_SIZES {
                if gui::selectable(&format!("{w} x {h}"), self.draw_opts.screen == Some((w, h)), 0) {
                    self.draw_opts.screen = Some((w, h));
                    self.fitted = DocumentId::default();
                }
            }

            gui::end_combo();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The game sizes the root of a layout to the screen. Components docked to a side or the centre move with it; the others stay.
Only the preview: the file is not changed.");
        }

        gui::same_line();
        gui::checkbox("Related", &mut self.related_on);
        if gui::is_item_hovered() {
            gui::set_tooltip("Draw the layouts this one is used with, faded, as reference only (not editable). Choose which in the UI Related panel.");
        }

        if self.anim.is_some() {
            gui::same_line();
            if gui::small_button("Stop animation") {
                self.anim = None;
            }
        }

        // skin
        gui::same_line();
        gui::set_next_item_width(140.0);
        let skin_label = if self.skin.is_empty() { "default".to_owned() } else { self.skin.clone() };
        if gui::begin_combo("Skin##skin", &skin_label) {
            let list = self.skins.clone().unwrap_or_default();
            for s in &list {
                let label = format!("{}  ({})", s.name, s.files);
                let on = (self.skin.is_empty() && s.name == "default") || self.skin == s.name;
                if gui::selectable(&label, on, 0) {
                    self.skin = if s.name == "default" { String::new() } else { s.name.clone() };
                }
            }

            gui::end_combo();
        }
    }

    /// State and visibility of the selected component, for the Fields panel.
    fn entry_controls(&mut self, e: &mut Editor, id: DocumentId, entry: &Entry) {
        let (names, def, own_shown) = {
            let d = ui_doc_ref(e, id).unwrap();
            let names = d.model().entry_state_names(&entry.path);
            let def = d.model().default_state_index(&entry.path);
            let ents = d.entries();
            let sh = d.model().shown(&ents, &self.draw_opts);
            let idx = ents.iter().position(|en| en.path == entry.path).unwrap_or(0);
            (names, def, sh.get(idx).copied().unwrap_or(true))
        };

        let cur = self.draw_opts.entry_state.get(&entry.path).copied().unwrap_or(def);
        gui::set_next_item_width(160.0);
        if gui::begin_combo("Preview state", names.get(cur).map(String::as_str).unwrap_or("-")) {
            for (i, n) in names.iter().enumerate() {
                if gui::selectable(&format!("{n}{}", if i == def { "  (default)" } else { "" }), i == cur, 0) {
                    if i == def {
                        self.draw_opts.entry_state.remove(&entry.path);
                    } else {
                        self.draw_opts.entry_state.insert(entry.path.clone(), i);
                    }
                }
            }

            gui::end_combo();
        }

        gui::same_line();
        let mut vis = own_shown;
        if gui::checkbox("Shown in preview", &mut vis) {
            if vis == entry.visible && !self.draw_opts.show_hidden {
                self.draw_opts.visibility.remove(&entry.path);
            } else {
                self.draw_opts.visibility.insert(entry.path.clone(), vis);
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Only the preview: the layout file is not changed. The seventh flag below is what the game reads.");
        }
    }

    // ---- animation

    /// Advances the running animation and sets the pose the canvas draws.
    fn update_pose(&mut self, e: &Editor, id: DocumentId) {
        let Some(mut a) = self.anim.take() else {
            self.draw_opts.pose = None;
            return;
        };

        let Some(d) = ui_doc_ref(e, id) else {
            self.anim = Some(a);
            return;
        };

        let entries = d.entries();
        let Some(ei) = entries.iter().position(|en| en.path == a.path) else {
            self.draw_opts.pose = None;
            return;
        };

        a.len = d.model().animation_length(&entries, ei, &a.name) as f32;
        if a.playing {
            a.t += gui::io().DeltaTime * 1000.0;
            if a.t >= a.len {
                if a.looping {
                    a.t = 0.0;
                } else {
                    a.t = a.len;
                    a.playing = false;
                }
            }
        }

        a.t = a.t.clamp(0.0, a.len.max(0.0));
        self.draw_opts.pose = Some(Rc::new(d.model().pose_at(&entries, ei, &a.name, a.t as u32)));
        self.anim = Some(a);
    }

    /// The animations of the selected component: pick one, play, pause, scrub.
    fn animation_controls(&mut self, e: &Editor, id: DocumentId, entry: &Entry) {
        let anims = ui_doc_ref(e, id).map(|d| d.model().animations(&entry.path)).unwrap_or_default();
        if anims.is_empty() {
            return;
        }

        gui::separator_text("Animation");
        let mine = self.anim.as_ref().is_some_and(|a| a.path == entry.path);
        let cur = if mine { self.anim.as_ref().map(|a| a.name.clone()) } else { None };
        gui::set_next_item_width(200.0);
        if gui::begin_combo("##anim", cur.as_deref().unwrap_or("(none)")) {
            if gui::selectable("(none)", cur.is_none(), 0) && mine {
                self.anim = None;
            }

            for a in &anims {
                let label = format!("{}  ({} ms, {} frame{})", a.name, a.duration(), a.frames.len(), if a.frames.len() == 1 { "" } else { "s" });
                if gui::selectable(&label, cur.as_deref() == Some(a.name.as_str()), 0) {
                    self.anim = Some(AnimPlay { path: entry.path.clone(), name: a.name.clone(), t: 0.0, len: a.duration() as f32, playing: true, looping: false });
                }
            }

            gui::end_combo();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Plays the animation on the canvas as the game would: frames one after another, each easing to its target.\nThe layout file is not changed.");
        }

        let mut stop = false;
        if let Some(a) = self.anim.as_mut().filter(|a| a.path == entry.path) {
            gui::same_line();
            if gui::button(if a.playing { "Pause" } else { "Play" }) {
                if !a.playing && a.t >= a.len {
                    a.t = 0.0;
                }

                a.playing = !a.playing;
            }

            gui::same_line();
            stop = gui::button("Stop");
            gui::same_line();
            gui::checkbox("Loop", &mut a.looping);
            gui::set_next_item_width(-1.0);
            let mut t = a.t;
            if gui::slider_float("##animt", &mut t, 0.0, a.len.max(1.0), "%.0f ms") {
                a.t = t;
                a.playing = false;
            }

            if let Some(an) = anims.iter().find(|x| x.name == a.name) {
                gui::text_disabled(&format!(
                    "{} frame(s); {}{}",
                    an.frames.len(),
                    if an.propagate { "reaches descendants; " } else { "" },
                    "rotation, picture cross-fade and shader values are not drawn"
                ));
            }
        }

        if stop {
            self.anim = None;
        }
    }

    // ---- related layouts

    /// The layouts that go with the open one (found by name), read once per layout.
    fn related_for(&mut self, e: &Editor, id: DocumentId) -> Vec<Relation> {
        let Some(d) = ui_doc_ref(e, id) else { return vec![] };
        let path = d.source.vfs_path.clone();
        if path.is_empty() {
            return vec![];
        }

        if let Some((p, list)) = &self.rel_list {
            if *p == path {
                return list.clone();
            }
        }

        if self.rel_index.is_none() {
            let Some(vfs) = e.vfs() else { return vec![] };
            self.rel_index = Some(Rc::new(RelationIndex::build(vfs)));
        }

        let list = self.rel_index.as_ref().map(|i| i.relations(&path)).unwrap_or_default();
        self.rel_list = Some((path, list.clone()));
        list
    }

    fn related_model(&mut self, e: &Editor, path: &str) -> Option<Rc<UiModel>> {
        if let Some(m) = self.rel_models.get(path) {
            return m.clone();
        }

        let m = e.vfs().and_then(|v| v.read(path).ok()).and_then(|b| UiModel::from_bytes(&b).ok()).map(Rc::new);
        self.rel_models.insert(path.to_owned(), m.clone());
        m
    }

    /// The reference layers to draw now (None when switched off).
    fn related_layers(&mut self, e: &Editor, id: DocumentId) -> Option<Rc<Layers>> {
        if !self.related_on {
            return None;
        }

        let rels: Vec<Relation> = self.related_for(e, id).into_iter().filter(|r| !self.related_off.contains(&r.path) && (self.related_generic || !r.generic)).collect();
        let d = ui_doc_ref(e, id)?;
        let mut off: Vec<&String> = self.related_off.iter().collect();
        off.sort();
        let key = format!("{id:?}|{}|{:?}|{:?}|{}|{}", d.version(), self.draw_opts.screen, off, self.related_generic, self.anim.as_ref().map(|a| a.t as i64).unwrap_or(-1));
        if let Some((k, l)) = &self.rel_layers {
            if *k == key {
                return Some(l.clone());
            }
        }

        let main = d.model().clone();
        let mut layers = Layers::default();
        for r in rels {
            let Some(other) = self.related_model(e, &r.path) else { continue };
            if let Some(items) = related::layer(&main, &self.draw_opts, &r, &other, 0.5) {
                match r.kind {
                    RelKind::Child => layers.above.extend(items),
                    RelKind::Parent => layers.below.extend(items),
                }
            }
        }

        let layers = Rc::new(layers);
        self.rel_layers = Some((key, layers.clone()));
        Some(layers)
    }

    /// The panel that lists related layouts and lets each be switched off.
    fn related_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(420.0, 260.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("UI Related", None, 0) {
            gui::end();
            return;
        }

        let Some(id) = active_doc(e) else {
            gui::text_disabled("No UI layout is open.");
            gui::end();
            return;
        };

        gui::checkbox("Show related layouts", &mut self.related_on);
        gui::same_line();
        gui::checkbox("Include common names", &mut self.related_generic);
        gui::text_wrapped("Reference only: faded and not editable. The layout format does not store which layout is loaded into which, so these are found by name: a child is a layout named after a component of this one, a parent has a component named after this layout.");
        gui::separator();
        // Finding them reads every layout of the packs (a few seconds): only when asked for, not when a layout opens.
        if self.rel_index.is_none() && !self.related_on {
            if gui::button("Find related layouts") {
                self.related_on = true;
            }

            gui::text_disabled("Reads all layouts once (a few seconds).");
            gui::end();
            return;
        }

        let rels = self.related_for(e, id);
        if rels.is_empty() {
            gui::text_disabled("Nothing found (the layout must come from the packs).");
        }

        let mut open: Option<String> = None;
        for r in &rels {
            let mut on = !self.related_off.contains(&r.path);
            gui::push_id(&format!("{}{:?}", r.path, r.kind));
            if gui::checkbox("##on", &mut on) {
                if on {
                    self.related_off.remove(&r.path);
                } else {
                    self.related_off.insert(r.path.clone());
                }
            }

            gui::same_line();
            let what = match r.kind {
                RelKind::Child => "child ",
                RelKind::Parent => "parent",
            };

            let name = r.path.rsplit('/').next().unwrap_or(&r.path);
            let label = format!("{what} {name}{}", if r.generic { "  [common name]" } else { "" });
            if r.generic {
                gui::text_disabled(&label);
            } else {
                gui::text(&label);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}
linked by the component name '{}'", r.path, r.via));
            }

            gui::same_line();
            if gui::small_button("Open") {
                open = Some(r.path.clone());
            }

            gui::pop_id();
        }

        if let Some(p) = open {
            // the tool is lent out while its panels draw, so it opens the file itself
            match e.open_path_from(self, &p, true, true) {
                Ok(m) => e.status(&m, false),
                Err(m) => e.status(&m, true),
            }
        }

        gui::end();
    }

    // ---- canvas

    fn fit(&mut self, e: &Editor, doc: DocumentId, size: Vec2) {
        let (w, h) = match self.draw_opts.screen {
            Some((sw, sh)) => (sw as f32, sh as f32),
            None => ui_doc_ref(e, doc).and_then(|d| d.entries().first().map(|r| (r.w.max(1) as f32, r.h.max(1) as f32))).unwrap_or((1280.0, 1024.0)),
        };
        let zoom = ((size.x - 24.0) / w).min((size.y - 24.0) / h).clamp(0.05, 8.0);
        self.view = Some(View { zoom, ox: -(size.x / zoom - w) / 2.0, oy: -(size.y / zoom - h) / 2.0 });
    }

    fn canvas(&mut self, e: &mut Editor) {
        self.loads = 0;
        gui::set_next_window_size(v2(960.0, 680.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("UI Canvas", None, gui::WINDOW_NO_SCROLLBAR | gui::WINDOW_NO_SCROLL_WITH_MOUSE) {
            gui::end();
            return;
        }

        let Some(id) = active_doc(e) else {
            gui::text_disabled("Open a UI layout: double-click one in Assets (files under ui/ without an extension), or use the command ui_open <path>.");
            gui::end();
            return;
        };

        // toolbar
        if gui::small_button("-") {
            if let Some(v) = &mut self.view {
                v.zoom = (v.zoom / 1.25).max(0.05);
            }
        }

        gui::same_line();
        if gui::small_button("+") {
            if let Some(v) = &mut self.view {
                v.zoom = (v.zoom * 1.25).min(16.0);
            }
        }

        gui::same_line();
        let fit_now = gui::small_button("Fit");
        gui::same_line();
        if gui::small_button("100%") {
            if let Some(v) = &mut self.view {
                v.zoom = 1.0;
            }
        }

        gui::same_line();
        gui::checkbox("Outlines", &mut self.show_outlines);
        gui::same_line();
        let mut images_on = !self.hide_images;
        if gui::checkbox("Pictures", &mut images_on) {
            self.hide_images = !images_on;
        }

        gui::same_line();
        let mut text_on = !self.hide_text;
        if gui::checkbox("Text", &mut text_on) {
            self.hide_text = !text_on;
        }

        self.options_row(e, id);

        let size = gui::content_region_avail();
        if size.x < 16.0 || size.y < 16.0 {
            gui::end();
            return;
        }

        if self.fitted != id || fit_now || self.view.is_none() {
            self.fit(e, id, size);
            self.fitted = id;
        }

        let mut view = self.view.unwrap();
        let p0 = gui::cursor_screen_pos();
        gui::invisible_button("##ui_canvas", size, gui::BUTTON_LEFT | gui::BUTTON_RIGHT | gui::BUTTON_MIDDLE);
        let hovered = gui::is_item_hovered();
        let canvas_active = gui::is_item_active();
        let m = gui::mouse_pos();
        let io = gui::io();

        // zoom about the cursor, pan
        if hovered && io.MouseWheel != 0.0 {
            let before = ((m.x - p0.x) / view.zoom + view.ox, (m.y - p0.y) / view.zoom + view.oy);
            view.zoom = (view.zoom * 1.12f32.powf(io.MouseWheel)).clamp(0.05, 16.0);
            view.ox = before.0 - (m.x - p0.x) / view.zoom;
            view.oy = before.1 - (m.y - p0.y) / view.zoom;
        }

        let space = gui::is_key_down(sys::ImGuiKey_Space as i32);
        let panning = (canvas_active && gui::is_mouse_down(2)) || (canvas_active && space && gui::is_mouse_down(0));
        if panning {
            view.ox -= io.MouseDelta.x / view.zoom;
            view.oy -= io.MouseDelta.y / view.zoom;
        }

        let to_screen = |lx: f32, ly: f32| v2(p0.x + (lx - view.ox) * view.zoom, p0.y + (ly - view.oy) * view.zoom);
        let to_layout = |sx: f32, sy: f32| ((sx - p0.x) / view.zoom + view.ox, (sy - p0.y) / view.zoom + view.oy);

        self.update_pose(e, id);
        let layers = self.related_layers(e, id);
        let (items, entries, shown, rects) = {
            let d = ui_doc_ref(e, id).unwrap();
            let entries = d.entries();
            let shown = d.model().shown(&entries, &self.draw_opts);
            let rects = d.model().geometry(&entries, &self.draw_opts);
            (std::rc::Rc::new(d.model().draw_list_with(&self.draw_opts)), entries, shown, rects)
        };

        let dl = gui::window_draw_list();
        let (cmin, cmax) = (p0, v2(p0.x + size.x, p0.y + size.y));
        dl.push_clip_rect(cmin, cmax);
        dl.rect_filled(cmin, cmax, col32(30, 31, 36, 255), 0.0);
        if let Some(root) = rects.first() {
            dl.rect_filled(to_screen(root.x, root.y), to_screen(root.x + root.w, root.y + root.h), col32(18, 18, 22, 255), 0.0);
        }

        let empty = Layers::default();
        let layer = layers.as_deref().unwrap_or(&empty);
        for it in layer.below.iter().chain(items.iter()).chain(layer.above.iter()) {
            let (a, b) = (to_screen(it.x, it.y), to_screen(it.x + it.w, it.y + it.h));
            if b.x < cmin.x || a.x > cmax.x || b.y < cmin.y || a.y > cmax.y {
                continue;
            }

            match &it.kind {
                DrawKind::Image { path, tiled, flip_x, flip_y, tint, margins } => {
                    if self.hide_images {
                        continue;
                    }

                    let key = self.image_key(path);
                    match e.images.get(&key) {
                        Some(img) => {
                            let tint = rgba(*tint);
                            for p in pieces(it.x, it.y, it.w, it.h, img.w as f32, img.h as f32, *margins, *tiled) {
                                let (tl, br) = (to_screen(p.x, p.y), to_screen(p.x + p.w, p.y + p.h));
                                let (u0, u1) = if *flip_x { (p.u1, p.u0) } else { (p.u0, p.u1) };
                                let (v0, v1) = if *flip_y { (p.v1, p.v0) } else { (p.v0, p.v1) };
                                dl.image(img.id, tl, br, v2(u0, v0), v2(u1, v1), tint);
                            }
                        }

                        None => {
                            if self.missing.contains(&key) {
                                dl.rect(a, b, col32(200, 60, 200, 140), 0.0, 1.0);
                            } else {
                                self.want_image(e, path);
                            }
                        }
                    }
                }

                DrawKind::Text { text, color, size: fs, halign, valign, font, category, inset } => {
                    if self.hide_text {
                        continue;
                    }

                    // the huge Unicode font (tens of thousands of glyphs) would need an atlas bigger than a texture: its text uses the editor's font
                    let resolved = e.vfs().cloned().and_then(|v| self.fonts.resolve(&v, font, *fs as i32, category)).filter(|r| r.cuf.glyph_count() < 5000);
                    match resolved {
                        Some(r) => {
                            let akey = format!("font:{}", r.key);
                            if !e.images.knows(&akey) {
                                let at = r.cuf.atlas();
                                e.images.submit(&akey, at.w, at.h, at.rgba);
                            }

                            if let Some(tex) = e.images.get(&akey) {
                                let mut base = r.color.unwrap_or(*color);
                                base[3] = base[3].min(color[3]);
                                let col = rgba(base);
                                let at_origin = r.cuf.atlas_origins();
                                let (bw, bh) = (it.w as i32 - inset[0] - inset[2], it.h as i32 - inset[1] - inset[3]);
                                for p in r.cuf.layout(text, bw, bh, *halign, *valign) {
                                    let m = r.cuf.metric(p.glyph);
                                    let (ox, oy) = at_origin[p.glyph];
                                    let (gx, gy) = (it.x + inset[0] as f32 + p.x as f32, it.y + inset[1] as f32 + p.y as f32);
                                    let (tl, br) = (to_screen(gx, gy), to_screen(gx + m.width as f32, gy + m.height as f32));
                                    let (tw, th) = (tex.w as f32, tex.h as f32);
                                    dl.image(tex.id, tl, br, v2(ox as f32 / tw, oy as f32 / th), v2((ox + m.width as u32) as f32 / tw, (oy + m.height as u32) as f32 / th), col);
                                }
                            }
                        }

                        None => {
                            let px = fs * view.zoom;
                            if px >= 5.0 {
                                dl.text_sized(px, a, rgba(*color), text);
                            }
                        }
                    }
                }
            }
        }

        // outlines, hover, selection
        let (mx, my) = to_layout(m.x, m.y);
        let sel = selected(e, id);
        if self.show_outlines {
            for (k, en) in entries.iter().enumerate().filter(|(k, en)| shown[*k] && en.kind == EntryKind::Entry && rects[*k].w > 0.0 && rects[*k].h > 0.0) {
                let _ = en;
                let r = rects[k];
                dl.rect(to_screen(r.x, r.y), to_screen(r.x + r.w, r.y + r.h), col32(80, 160, 255, 70), 0.0, 1.0);
            }
        }

        let pick = |mx: f32, my: f32| entries.iter().enumerate().rev().find(|(k, en)| shown[*k] && en.kind == EntryKind::Entry && rects[*k].w > 0.0 && rects[*k].h > 0.0 && mx >= rects[*k].x && my >= rects[*k].y && mx < rects[*k].x + rects[*k].w && my < rects[*k].y + rects[*k].h).map(|(i, _)| i);
        if hovered && !panning && self.drag.is_none() {
            if let Some(i) = pick(mx, my) {
                let r = rects[i];
                dl.rect(to_screen(r.x, r.y), to_screen(r.x + r.w, r.y + r.h), col32(255, 255, 255, 120), 0.0, 1.0);
            }
        }

        let mut handle_rects: Vec<(u8, Vec2, Vec2)> = vec![];
        if let Some((en, r)) = sel.and_then(|i| entries.get(i).zip(rects.get(i))) {
            let (a, b) = (to_screen(r.x, r.y), to_screen(r.x + r.w, r.y + r.h));
            dl.rect(a, b, col32(255, 200, 0, 255), 0.0, 2.0);
            if en.kind == EntryKind::Entry {
                for (k, (hx, hy)) in HANDLES.iter().enumerate() {
                    let cx = a.x + (b.x - a.x) * (*hx + 1) as f32 / 2.0;
                    let cy = a.y + (b.y - a.y) * (*hy + 1) as f32 / 2.0;
                    let (lo, hi) = (v2(cx - 4.0, cy - 4.0), v2(cx + 4.0, cy + 4.0));
                    dl.rect_filled(lo, hi, col32(255, 200, 0, 255), 0.0);
                    handle_rects.push((k as u8, lo, hi));
                }
            }
        }

        dl.pop_clip_rect();

        // mouse
        if hovered && !panning && gui::is_mouse_clicked(0) && !space {
            let hit_handle = handle_rects.iter().find(|(_, lo, hi)| m.x >= lo.x - 2.0 && m.x <= hi.x + 2.0 && m.y >= lo.y - 2.0 && m.y <= hi.y + 2.0).map(|(k, _, _)| *k);
            let target = match (hit_handle, sel) {
                (Some(k), Some(i)) => Some((DragKind::Resize(k), i)),
                _ => match pick(mx, my) {
                    Some(i) => {
                        select(e, id, i);
                        Some((DragKind::Move, i))
                    }
                    None => {
                        e.selection.clear();
                        None
                    }
                },
            };

            if let Some((kind, i)) = target {
                if let Some(en) = entries.get(i).filter(|en| en.kind == EntryKind::Entry) {
                    self.gesture += 1;
                    let key = ((u64::from(id) << 40) ^ (self.gesture << 8) ^ 0x61) | 1;
                    self.drag = Some(Drag { kind, entry: i, start_mouse: (m.x, m.y), start: (en.x, en.y, en.w, en.h), key, last: (en.x, en.y, en.w, en.h) });
                }
            }
        }

        if let Some(mut d) = self.drag.take() {
            if gui::is_mouse_down(0) {
                let dx = ((m.x - d.start_mouse.0) / view.zoom).round() as i32;
                let dy = ((m.y - d.start_mouse.1) / view.zoom).round() as i32;
                let (mut x, mut y, mut w, mut h) = d.start;
                match d.kind {
                    DragKind::Move => {
                        x += dx;
                        y += dy;
                    }
                    DragKind::Resize(k) => {
                        let (hx, hy) = HANDLES[k as usize];
                        if hx == -1 {
                            x += dx;
                            w -= dx;
                        } else if hx == 1 {
                            w += dx;
                        }

                        if hy == -1 {
                            y += dy;
                            h -= dy;
                        } else if hy == 1 {
                            h += dy;
                        }
                    }
                }

                if w < 0 {
                    w = 0;
                }

                if h < 0 {
                    h = 0;
                }

                if (x, y, w, h) != d.last {
                    if let (Some(en), Some(doc)) = (entries.get(d.entry), ui_doc(e, id)) {
                        let path = en.path.clone();
                        let resized = matches!(d.kind, DragKind::Resize(_));
                        let name = if resized { "Resize" } else { "Move" };
                        let _ = doc.edit(name, d.key, |mm| {
                            // size first: a docked entry's dock offset is worked out for the size it ends with
                            if resized {
                                mm.set_size(&path, w, h)?;
                            }

                            mm.set_offset(&path, x, y)
                        });
                    }

                    d.last = (x, y, w, h);
                }

                self.drag = Some(d);
            } else if let Some(doc) = ui_doc(e, id) {
                doc.break_merge();
            }
        }

        // keys
        if hovered && self.drag.is_none() {
            if let Some(i) = sel {
                let step = if io.KeyShift { 10 } else { 1 };
                let nudge = [(sys::ImGuiKey_LeftArrow, -step, 0), (sys::ImGuiKey_RightArrow, step, 0), (sys::ImGuiKey_UpArrow, 0, -step), (sys::ImGuiKey_DownArrow, 0, step)];
                for (key, dx, dy) in nudge {
                    if gui::is_key_pressed(key as i32, true) {
                        if let Some(en) = entries.get(i).filter(|en| en.kind == EntryKind::Entry) {
                            let path = en.path.clone();
                            let (x, y) = (en.x + dx, en.y + dy);
                            let k = ((u64::from(id) << 40) ^ ((i as u64) << 8) ^ 0x62) | 1;
                            if let Some(doc) = ui_doc(e, id) {
                                let _ = doc.edit("Nudge", k, |mm| mm.set_offset(&path, x, y));
                            }
                        }
                    }
                }

                if gui::is_key_pressed(gui::KEY_DELETE, false) {
                    self.delete_selected(e);
                }

                if io.KeyCtrl && gui::is_key_pressed(gui::KEY_D, false) {
                    self.duplicate_selected(e);
                }
            }
        }

        if let Some(doc) = ui_doc(e, id) {
            if !gui::is_mouse_down(0) {
                // key nudges merge until the keys stop
                if !(gui::is_key_down(sys::ImGuiKey_LeftArrow as i32) || gui::is_key_down(sys::ImGuiKey_RightArrow as i32) || gui::is_key_down(sys::ImGuiKey_UpArrow as i32) || gui::is_key_down(sys::ImGuiKey_DownArrow as i32)) {
                    doc.break_merge();
                }
            }
        }

        self.view = Some(view);
        if hovered {
            gui::set_cursor_screen_pos(v2(p0.x + 6.0, p0.y + size.y - 18.0));
            gui::text_disabled(&format!("{:.0}%   layout {:.0}, {:.0}", view.zoom * 100.0, mx, my));
        }

        gui::end();
    }

    // ---- operations (menu, keys, commands)

    fn with_selected(&mut self, e: &mut Editor, what: &str, op: impl FnOnce(&mut UiModel, &[usize]) -> Result<Vec<usize>, String>) -> Result<(), String> {
        let id = active_doc(e).ok_or("no UI layout is open")?;
        let i = selected(e, id).ok_or("no component is selected")?;
        let path = ui_doc_ref(e, id).and_then(|d| d.entries().get(i).map(|en| en.path.clone())).ok_or("the selection is gone")?;
        let new_path = ui_doc(e, id).ok_or("no UI layout is open")?.edit(what, 0, |m| op(m, &path))?;
        self.select_path(e, id, &new_path);
        Ok(())
    }

    fn select_path(&mut self, e: &mut Editor, id: DocumentId, path: &[usize]) {
        let found = ui_doc_ref(e, id).and_then(|d| d.entries().iter().position(|en| en.path == path));
        match found {
            Some(i) => select(e, id, i),
            None => e.selection.clear(),
        }
    }

    fn delete_selected(&mut self, e: &mut Editor) {
        let r = self.with_selected(e, "Delete component", |m, p| {
            m.delete(p)?;
            // the parent entry
            Ok(parent_entry_path(p))
        });
        if let Err(msg) = r {
            e.status(&msg, true);
        }
    }

    fn duplicate_selected(&mut self, e: &mut Editor) {
        if let Err(msg) = self.with_selected(e, "Duplicate component", |m, p| m.duplicate(p)) {
            e.status(&msg, true);
        }
    }

    fn add_child(&mut self, e: &mut Editor, name: &str) -> Result<(), String> {
        let name = name.to_owned();
        self.with_selected(e, "Add component", move |m, p| m.add_child(p, None, &name))
    }

    fn move_in_order(&mut self, e: &mut Editor, delta: i32) -> Result<(), String> {
        self.with_selected(e, "Change paint order", move |m, p| m.move_sibling(p, delta))
    }

    fn move_into_previous(&mut self, e: &mut Editor) -> Result<(), String> {
        self.with_selected(e, "Move into previous sibling", |m, p| {
            let ents = m.entries();
            let me = ents.iter().position(|en| en.path == p).ok_or("no such entry")?;
            let sibs: Vec<usize> = (0..ents.len()).filter(|&k| ents[k].parent == ents[me].parent).collect();
            let at = sibs.iter().position(|&k| k == me).unwrap();
            if at == 0 {
                return Err("there is no previous sibling".into());
            }

            let target = ents[sibs[at - 1]].path.clone();
            m.reparent(p, &target, None, true)
        })
    }

    fn move_out(&mut self, e: &mut Editor) -> Result<(), String> {
        self.with_selected(e, "Move out of parent", |m, p| {
            let ents = m.entries();
            let me = ents.iter().position(|en| en.path == p).ok_or("no such entry")?;
            let parent = ents[me].parent.ok_or("the root has no parent")?;
            let grand = ents[parent].parent.ok_or("the parent is the root")?;
            let target = ents[grand].path.clone();
            m.reparent(p, &target, None, true)
        })
    }

    // ---- XML pane

    fn xml_pane(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(760.0, 300.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("UI XML", None, 0) {
            gui::end();
            return;
        }

        let Some(id) = active_doc(e) else {
            gui::text_disabled("No UI layout is open.");
            gui::end();
            return;
        };

        let sel = selected(e, id);
        let scope = if self.xml.whole { None } else { sel.map(item_of) };
        let version = ui_doc_ref(e, id).unwrap().version();
        let key = (id, version, scope, self.xml.whole);
        if key != self.xml.shown && !self.xml.edited {
            let d = ui_doc_ref(e, id).unwrap();
            self.xml.text = if self.xml.whole {
                d.xml().as_ref().clone()
            } else {
                sel.and_then(|i| d.entries().get(i).and_then(|en| d.model().dom.at(&en.path).map(|el| wce_ui_model::Dom { root: el.clone() }.to_xml()))).unwrap_or_default()
            };
            self.xml.shown = key;
            self.xml.error.clear();
        }

        if gui::checkbox("Whole file", &mut self.xml.whole) {
            self.xml.edited = false;
        }

        gui::same_line();
        if gui::small_button("Revert") {
            self.xml.edited = false;
            self.xml.shown = (0, 0, None, false);
        }

        gui::same_line();
        let n = self.xml.text.len();
        if self.xml.error.is_empty() {
            gui::text_disabled(&if self.xml.edited { format!("{n} bytes, editing (applies when you pause)") } else { format!("{n} bytes, in sync") });
        } else {
            gui::text_colored([1.0, 0.45, 0.4, 1.0], &format!("not applied: {}", self.xml.error));
        }

        if !self.xml.whole && sel.is_none() {
            gui::text_disabled("Select a component to see its XML, or tick Whole file.");
            gui::end();
            return;
        }

        if self.xml.text.len() > 3_000_000 {
            gui::text_wrapped("The XML is too large for the text box: select a component, or edit the file with an external editor and open the XML.");
            gui::end();
            return;
        }

        let size = gui::content_region_avail();
        if gui::input_text_multiline("##xml", &mut self.xml.text, v2(size.x, size.y), 0) {
            self.xml.edited = true;
            self.xml.last_edit = gui::time();
        }

        if self.xml.edited && gui::time() - self.xml.last_edit > 0.35 {
            let text = self.xml.text.clone();
            let gesture_key = ((u64::from(id) << 40) ^ 0x7878) | 1;
            let whole = self.xml.whole;
            let path = sel.and_then(|i| ui_doc_ref(e, id).and_then(|d| d.entries().get(i).map(|en| en.path.clone())));
            let r = match ui_doc(e, id) {
                Some(d) if whole => d.set_xml(&text, gesture_key),
                Some(d) => match path {
                    Some(p) => d.set_entry_xml(&p, &text, gesture_key),
                    None => Err("no component is selected".into()),
                },
                None => Err("no UI layout is open".into()),
            };

            match r {
                Ok(()) => {
                    self.xml.edited = false;
                    self.xml.error.clear();
                    let d = ui_doc(e, id).unwrap();
                    let v = d.version();
                    d.break_merge();
                    self.xml.shown = (id, v, scope, whole);
                }

                Err(msg) => {
                    self.xml.error = msg;
                    self.xml.last_edit = f64::MAX / 2.0;
                }
            }
        }

        gui::end();
    }

    // ---- fields

    fn fields_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(420.0, 420.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("UI Fields", None, 0) {
            gui::end();
            return;
        }

        let Some(id) = active_doc(e) else {
            gui::text_disabled("No UI layout is open.");
            gui::end();
            return;
        };

        let Some(i) = selected(e, id) else {
            gui::text_disabled("Select a component.");
            gui::end();
            return;
        };

        let (entry, state_path) = {
            let d = ui_doc_ref(e, id).unwrap();
            let Some(en) = d.entries().get(i).cloned() else {
                gui::end();
                return;
            };

            let sp = d.model().default_state_path(&en.path);
            (en, sp)
        };

        gui::text(&format!("{}   [{}]", entry.name, i));
        if entry.kind == EntryKind::Entry {
            self.entry_controls(e, id, &entry);
            self.animation_controls(e, id, &entry);
        }
        let mut edit: Option<(Vec<usize>, usize, String, &'static str)> = None;
        if gui::begin_tab_bar("##fields_tabs") {
            let scopes: [(&str, Option<Vec<usize>>); 2] = [("Component", Some(entry.path.clone())), ("Default state", state_path)];
            for (label, path) in scopes {
                if gui::begin_tab_item(label, 0) {
                    match path {
                        None => gui::text_disabled("(none)"),
                        Some(path) => {
                            let fields = ui_doc_ref(e, id).unwrap().model().fields(&path);
                            if gui::begin_table("##ft", 4, gui::TABLE_BORDERS | gui::TABLE_ROW_BG | gui::TABLE_SCROLL_Y | gui::TABLE_RESIZABLE, v2(0.0, 0.0)) {
                                gui::table_setup_column("#", gui::COLUMN_WIDTH_FIXED, 30.0);
                                gui::table_setup_column("Field", gui::COLUMN_WIDTH_STRETCH, 1.7);
                                gui::table_setup_column("Type", gui::COLUMN_WIDTH_FIXED, 52.0);
                                gui::table_setup_column("Value", gui::COLUMN_WIDTH_STRETCH, 1.3);
                                gui::table_setup_scroll_freeze(0, 1);
                                gui::table_headers_row();
                                for f in &fields {
                                    gui::table_next_row();
                                    gui::table_next_column();
                                    gui::text(&f.idx.to_string());
                                    gui::table_next_column();
                                    gui::text(if f.label.is_empty() { "-" } else { &f.label });
                                    gui::table_next_column();
                                    gui::text_disabled(&f.tag);
                                    gui::table_next_column();
                                    gui::push_id(&format!("{}{}", label, f.idx));
                                    gui::set_next_item_width(-1.0);
                                    if f.tag == "yes" || f.tag == "no" {
                                        let mut on = f.tag == "yes";
                                        if gui::checkbox("##v", &mut on) {
                                            edit = Some((path.clone(), f.idx, if on { "yes" } else { "no" }.to_owned(), "Toggle field"));
                                        }
                                    } else {
                                        let mut v = f.value.clone();
                                        let enter = gui::input_text("##v", None, &mut v, 1024, gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL);
                                        if (enter || gui::is_item_deactivated_after_edit()) && v != f.value {
                                            edit = Some((path.clone(), f.idx, v, "Set field"));
                                        }
                                    }

                                    gui::pop_id();
                                }

                                gui::end_table();
                            }
                        }
                    }

                    gui::end_tab_item();
                }
            }

            gui::end_tab_bar();
        }

        if let Some((path, idx, value, name)) = edit {
            if let Some(d) = ui_doc(e, id) {
                if let Err(msg) = d.edit(name, 0, |m| m.set_field(&path, idx, &value)) {
                    e.status(&msg, true);
                }
            }
        }

        gui::end();
    }

    // ---- save into a pack

    fn pack_popup(&mut self, e: &mut Editor) {
        if self.open_pack_popup {
            self.open_pack_popup = false;
            if let Some(id) = active_doc(e) {
                let d = ui_doc_ref(e, id).unwrap();
                self.pack_inside = if d.source.vfs_path.is_empty() { String::new() } else { d.source.vfs_path.clone() };
            }

            if self.pack_path.is_empty() {
                self.pack_path = e.settings().get("ui_mod_pack", None).unwrap_or_default();
            }

            gui::open_popup("Save into a mod pack");
        }

        if gui::begin_popup_modal("Save into a mod pack", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::text_wrapped("Writes the binary layout into a mod pack (created if it does not exist). The game's own packs are never written.");
            gui::set_next_item_width(560.0);
            gui::input_text("Pack file", Some("C:\\...\\my_ui_mod.pack"), &mut self.pack_path, 1024, 0);
            gui::set_next_item_width(560.0);
            gui::input_text("Path inside", Some("ui/campaign ui/settlement_panel"), &mut self.pack_inside, 1024, 0);
            if !self.pack_message.is_empty() {
                gui::text_colored([1.0, 0.6, 0.4, 1.0], &self.pack_message);
            }

            if gui::button("Save") {
                let r = active_doc(e).ok_or_else(|| "no UI layout is open".to_owned()).and_then(|id| {
                    let d = ui_doc_ref(e, id).ok_or("no UI layout is open")?;
                    if !self.pack_path.to_lowercase().ends_with(".pack") {
                        return Err("the pack file must end in .pack".to_owned());
                    }

                    d.save_to_pack(std::path::Path::new(&self.pack_path), &self.pack_inside)
                });
                match r {
                    Ok(()) => {
                        e.settings().set("ui_mod_pack", &self.pack_path);
                        let _ = e.settings().save();
                        e.status(&format!("saved {} into {}", self.pack_inside, self.pack_path), false);
                        self.pack_message.clear();
                        gui::close_current_popup();
                    }
                    Err(m) => self.pack_message = m,
                }
            }

            gui::same_line();
            if gui::button("Cancel") {
                self.pack_message.clear();
                gui::close_current_popup();
            }

            gui::end_popup();
        }
    }
}

fn ui_doc_ref(e: &Editor, id: DocumentId) -> Option<&UiDocument> {
    e.docs.find_as::<UiDocument>(id)
}

/// The path of the entry that holds the entry at `path` (the `children` list sits between them).
fn parent_entry_path(path: &[usize]) -> Vec<usize> {
    path[..path.len().saturating_sub(2)].to_vec()
}

impl Tool for UiTool {
    fn id(&self) -> &str {
        "ui"
    }

    fn title(&self) -> &str {
        "UI"
    }

    /// Layouts have no extension in the packs.
    fn opens(&self) -> Vec<String> {
        vec![String::new(), ".cml".into(), ".fc".into(), ".uixml".into()]
    }

    fn shows_viewport(&self) -> bool {
        false
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.20);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.28);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.30);
        gui::dock_builder_dock_window("UI Canvas", center);
        gui::dock_builder_dock_window("Outliner", left);
        gui::dock_builder_dock_window("Assets", left);
        gui::dock_builder_dock_window("UI Fields", right);
        gui::dock_builder_dock_window("UI Related", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
        gui::dock_builder_dock_window("UI XML", bottom);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        e.input_origin = Some(v2(0.0, 0.0));
        self.canvas(e);
        self.xml_pane(e);
        self.fields_panel(e);
        self.related_panel(e);
        self.pack_popup(e);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if gui::begin_menu("UI") {
            let has = active_doc(e).is_some();
            let sel = has && active_doc(e).and_then(|id| selected(e, id)).is_some();
            gui::begin_disabled(!sel);
            if gui::menu_item("Add child", None, false, true) {
                if let Err(m) = self.add_child(e, "new_component") {
                    e.status(&m, true);
                }
            }

            if gui::menu_item("Duplicate", Some("Ctrl+D"), false, true) {
                self.duplicate_selected(e);
            }

            if gui::menu_item("Delete", Some("Del"), false, true) {
                self.delete_selected(e);
            }

            gui::separator();
            for (label, d) in [("Bring forward (draw later)", 1), ("Send backward (draw earlier)", -1)] {
                if gui::menu_item(label, None, false, true) {
                    if let Err(m) = self.move_in_order(e, d) {
                        e.status(&m, true);
                    }
                }
            }

            if gui::menu_item("Move into previous sibling", None, false, true) {
                if let Err(m) = self.move_into_previous(e) {
                    e.status(&m, true);
                }
            }

            if gui::menu_item("Move out of parent", None, false, true) {
                if let Err(m) = self.move_out(e) {
                    e.status(&m, true);
                }
            }

            gui::end_disabled();
            gui::separator();
            gui::begin_disabled(!has);
            if gui::menu_item("Save into a mod pack...", None, false, true) {
                self.open_pack_popup = true;
            }

            if gui::menu_item("Export XML...", None, false, true) {
                if let Some(id) = active_doc(e) {
                    let d = ui_doc_ref(e, id).unwrap();
                    let name = format!("{}.uixml", d.title());
                    let path = e.output_dir().join("ui_xml").join(name);
                    let xml = d.xml();
                    if let Some(p) = path.parent() {
                        let _ = std::fs::create_dir_all(p);
                    }

                    match std::fs::write(&path, xml.as_bytes()) {
                        Ok(()) => e.status(&format!("wrote {}", path.display()), false),
                        Err(err) => e.status(&format!("cannot write {}: {err}", path.display()), true),
                    }
                }
            }

            gui::end_disabled();
            gui::end_menu();
        }
    }

    fn register_commands(&self, c: &mut CommandRegistry<Editor>) {
        crate::commands::register(c);
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        let (source, bytes) = if from_packs {
            let vfs = e.vfs().ok_or("the game packs are not mounted")?;
            let p = wce_vfs::normalize(path);
            let owner = vfs.owner(&p).ok_or("not found in the packs")?;
            let bytes = vfs.read(&p).map_err(|e| e.to_string())?;
            let pack = &vfs.packs()[owner];
            (Source { vfs_path: p, disk: pack.path.clone(), label: pack.name.clone() }, bytes)
        } else {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            (Source { vfs_path: String::new(), disk: PathBuf::from(path), label: "loose".to_owned() }, bytes)
        };

        let model = if wce_ui_format::looks_like_ui(&bytes) {
            UiModel::from_bytes(&bytes)?
        } else {
            UiModel::from_xml(std::str::from_utf8(&bytes).map_err(|_| "not a UI layout and not UTF-8 XML")?)?
        };

        log::info(format!("UI layout {path}: version {}, {} components", model.version, model.entries().len()));
        Ok(Box::new(UiDocument::new(source, model, e.output_dir())))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub(crate) fn with_ui(e: &mut Editor, f: impl FnOnce(&mut UiTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<UiTool, _>(f).unwrap_or_else(|| CommandResult::fail("the UI tool is not available"))
}

/// Used by the commands: the active document, the selected entry.
pub(crate) fn current(e: &Editor) -> Option<(DocumentId, Option<usize>)> {
    active_doc(e).map(|id| (id, selected(e, id)))
}

pub(crate) fn tool_select(e: &mut Editor, doc: DocumentId, entry: usize) {
    select(e, doc, entry);
}

pub(crate) fn tool_doc(e: &mut Editor, id: DocumentId) -> Option<&mut UiDocument> {
    ui_doc(e, id)
}

impl UiTool {
    pub(crate) fn op_add(&mut self, e: &mut Editor, name: &str) -> Result<(), String> {
        self.add_child(e, name)
    }

    pub(crate) fn op_delete(&mut self, e: &mut Editor) {
        self.delete_selected(e);
    }

    pub(crate) fn op_duplicate(&mut self, e: &mut Editor) {
        self.duplicate_selected(e);
    }

    pub(crate) fn op_reorder(&mut self, e: &mut Editor, d: i32) -> Result<(), String> {
        self.move_in_order(e, d)
    }

    pub(crate) fn op_into_previous(&mut self, e: &mut Editor) -> Result<(), String> {
        self.move_into_previous(e)
    }

    pub(crate) fn op_move_out(&mut self, e: &mut Editor) -> Result<(), String> {
        self.move_out(e)
    }

    pub(crate) fn op_zoom(&mut self, z: f32) {
        if let Some(v) = &mut self.view {
            v.zoom = z.clamp(0.05, 16.0);
        }
    }

    pub(crate) fn op_fit(&mut self) {
        self.view = None;
    }

    pub(crate) fn op_outlines(&mut self, on: bool) {
        self.show_outlines = on;
    }

    pub(crate) fn op_skin(&mut self, e: &Editor, name: &str) -> Result<String, String> {
        self.load_skins(e);
        if name.is_empty() || name == "default" {
            self.skin.clear();
            return Ok("default".into());
        }

        if self.skins.as_ref().is_some_and(|l| l.iter().any(|s| s.name == name)) {
            self.skin = name.to_owned();
            return Ok(name.to_owned());
        }

        Err(format!("no skin folder 'ui/skins/{name}'"))
    }

    /// Where the game puts a component with the current preview options (screen size, animation), in layout pixels.
    pub(crate) fn rect_of(&self, e: &Editor, id: DocumentId, i: usize) -> Option<wce_ui_model::layout::Rect> {
        let d = ui_doc_ref(e, id)?;
        let ents = d.entries();
        d.model().geometry(&ents, &self.draw_opts).get(i).copied()
    }

    /// Sizes the root to a screen (None: the layout's own size).
    pub(crate) fn op_screen(&mut self, size: Option<(i32, i32)>) {
        self.draw_opts.screen = size;
        self.view = None;
    }

    pub(crate) fn op_related(&mut self, on: bool, generic: bool) {
        self.related_on = on;
        self.related_generic = generic;
    }

    /// How many reference layers are drawn (for scripted checks).
    pub(crate) fn related_count(&mut self, e: &Editor, id: DocumentId) -> usize {
        self.related_layers(e, id).map(|l| l.below.len() + l.above.len()).unwrap_or(0)
    }

    /// Previews an animation at a moment (None: stop).
    pub(crate) fn op_animate(&mut self, e: &Editor, id: DocumentId, entry: Option<(Vec<usize>, String, f32)>) -> Result<String, String> {
        let Some((path, name, t)) = entry else {
            self.anim = None;
            self.draw_opts.pose = None;
            return Ok("stopped".into());
        };

        let d = ui_doc_ref(e, id).ok_or("no UI layout is open")?;
        let ents = d.entries();
        let ei = ents.iter().position(|en| en.path == path).ok_or("no such component")?;
        if !d.model().animations(&path).iter().any(|a| a.name == name) {
            return Err(format!("{} has no animation '{name}'", ents[ei].name));
        }

        let len = d.model().animation_length(&ents, ei, &name) as f32;
        self.anim = Some(AnimPlay { path, name, t: t.clamp(0.0, len), len, playing: false, looping: false });
        self.update_pose(e, id);
        Ok(format!("{len} ms long"))
    }

    pub(crate) fn op_state(&mut self, name: Option<String>) {
        self.draw_opts.state = name;
    }

    pub(crate) fn op_show_hidden(&mut self, on: bool) {
        self.draw_opts.show_hidden = on;
    }

    pub(crate) fn op_show_templates(&mut self, on: bool) {
        self.draw_opts.show_templates = on;
    }

    /// Visibility in the preview of one component (None: back to what the layout says).
    pub(crate) fn op_visibility(&mut self, path: Vec<usize>, on: Option<bool>) {
        match on {
            Some(v) => {
                self.draw_opts.visibility.insert(path, v);
            }
            None => {
                self.draw_opts.visibility.remove(&path);
            }
        }
    }

    pub(crate) fn op_entry_state(&mut self, path: Vec<usize>, idx: Option<usize>) {
        match idx {
            Some(i) => {
                self.draw_opts.entry_state.insert(path, i);
            }
            None => {
                self.draw_opts.entry_state.remove(&path);
            }
        }
    }

    /// How many components are drawn with the current options (for scripted checks).
    pub(crate) fn shown_count(&self, e: &Editor, id: DocumentId) -> usize {
        ui_doc_ref(e, id).map(|d| {
            let ents = d.entries();
            d.model().shown(&ents, &self.draw_opts).iter().filter(|s| **s).count()
        }).unwrap_or(0)
    }

    pub(crate) fn entries_of(e: &Editor, id: DocumentId) -> Vec<Entry> {
        ui_doc_ref(e, id).map(|d| d.entries().as_ref().clone()).unwrap_or_default()
    }
}
