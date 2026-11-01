//! The UI tool: the canvas, the raw XML pane, the fields of a component, the menu and the commands. Everything edits the document's one
//! model, so the canvas, the XML and the fields always show the same thing.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::path::PathBuf;

use crate::settings::{SETTINGS_ID, UiSettings};

use dear_imgui_sys as sys;
use wce_editor::editor::Editor;
use wce_editor::gui::{self, Vec2, col32, v2};
use wce_editor::theme;
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
use wce_ui_model::props::{Control, Prop, PropEdit};
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
    /// Show the converter's own XML instead of the one under the game's names.
    raw: bool,
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
    /// A component being renamed in the Layers panel: (its number, the text, frames since it started).
    rename: Option<(usize, String, u8)>,
    new_skin_popup: bool,
    new_skin_name: String,
    new_skin_msg: String,
    /// What the skin list was built from: the mounted files and the loose skin folders.
    skin_sig: (usize, Vec<String>),
    export_popup: bool,
    export_path: String,
    export_msg: String,
    /// Pictures read from the project folder, with the file's time: a change reloads them.
    loose_imgs: HashMap<String, (PathBuf, std::time::SystemTime, [u8; 4])>,
    /// `ui_mark_count`: the component count to compare with.
    count_mark: usize,
    watch_acc: f32,
    /// The disk time each open layout had when last seen.
    doc_stamps: HashMap<DocumentId, std::time::SystemTime>,
    warned: HashSet<DocumentId>,
    show_adv: bool,
    /// Which state the Fields panel shows for a component (index), when it is not the default one.
    state_pick: HashMap<Vec<usize>, usize>,
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

    /// For the scripted checks: loads a picture the way the canvas would.
    pub(crate) fn load_picture(&mut self, e: &mut Editor, path: &str) {
        self.loads = 0;
        self.want_image(e, path);
    }

    /// For the scripted checks: the first pixel of a picture read from the project folder, if it is loaded from there.
    pub(crate) fn loose_pixel(&self, path: &str) -> Option<[u8; 4]> {
        self.loose_imgs.get(&self.image_key(path)).map(|(_, _, px)| *px)
    }

    /// For the scripted checks: makes the next frame look for changed files at once.
    pub(crate) fn watch_now(&mut self) {
        self.watch_acc = 1.0;
    }

    pub(crate) fn mark_count(&mut self, n: usize) {
        self.count_mark = n;
    }

    pub(crate) fn marked_count(&self) -> usize {
        self.count_mark
    }

    /// (loose, packed) of a skin of the list; the list is made if it was not.
    pub(crate) fn skin_info(&mut self, e: &Editor, name: &str) -> Option<(bool, bool)> {
        self.load_skins(e);
        self.skins.as_ref()?.iter().find(|s| s.name == name).map(|s| (s.loose, s.packed))
    }

    pub(crate) fn skin_selected(&self) -> &str {
        &self.skin
    }

    fn want_image(&mut self, e: &mut Editor, path: &str) {
        let key = self.image_key(path);
        if self.loads >= 6 || self.missing.contains(&key) || e.images.knows(&key) {
            return;
        }

        self.loads += 1;
        let skin = (!self.skin.is_empty()).then_some(self.skin.clone());
        // the loose folder is read from disk each time, so a picture saved a moment ago shows
        {
            let pd = e.loose_dir();
            for cand in skins::candidates(path, skin.as_deref()) {
                let f = pd.join(&cand);
                if let Ok(bytes) = std::fs::read(&f) {
                    if let Ok(img) = images::decode(&cand, &bytes) {
                        let stamp = std::fs::metadata(&f).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                        let px0 = img.px.get(..4).map(|p| [p[0], p[1], p[2], p[3]]).unwrap_or([0; 4]);
                        self.loose_imgs.insert(key.clone(), (f, stamp, px0));
                        e.images.submit(&key, img.w, img.h, img.px);
                        return;
                    }
                }
            }
        }

        let Some(vfs) = e.vfs().cloned() else {
            self.missing.insert(key);
            return;
        };

        let skin = skin.as_deref();
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
                let pd = e.loose_dir();
                let list = skins::list(vfs, Some(&pd));
                if !self.skin.is_empty() && !list.iter().any(|s| s.name == self.skin) {
                    self.skin.clear();
                }

                self.skins = Some(list);
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

        gui::same_line();
        let changed = active_doc(e).and_then(|id| ui_doc_ref(e, id)).is_some_and(|d| d.modified());
        gui::begin_disabled(!changed);
        if gui::small_button("Reset all changes") {
            self.reset_all(e);
        }

        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Puts the layout back as it was when opened or last saved. Ctrl+Z brings the changes back.");
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
        let list = self.skins.clone().unwrap_or_default();
        let cur = list.iter().find(|s| (self.skin.is_empty() && s.name == "default") || s.name == self.skin).cloned();
        let cur_label = cur.as_ref().map(skin_label).unwrap_or_else(|| "default".to_owned());
        gui::set_next_item_width(300.0);
        if gui::begin_combo("Skin##skin", &cur_label) {
            for s in &list {
                let on = (self.skin.is_empty() && s.name == "default") || self.skin == s.name;
                if gui::selectable(&skin_label(s), on, 0) {
                    self.skin = if s.name == "default" { String::new() } else { s.name.clone() };
                }
            }

            gui::end_combo();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("[loose] is a skin of your own, a folder on disk. Its pictures are read before the game's packs: a picture you do not provide comes from the pack's skin of the same name, if there is one, else from the default skin.");
        }

        gui::same_line();
        if gui::small_button("New skin...") {
            self.new_skin_popup = true;
        }

        if cur.as_ref().is_some_and(|s| s.loose) {
            gui::same_line();
            if gui::small_button("Open skin folder") {
                if let Err(m) = self.open_skin_folder(e) {
                    e.status(&m, true);
                }
            }
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
                use wce_editor::shortcuts::pressed;
                let nudge = [
                    ("ui.nudge_left", -1, 0), ("ui.nudge_right", 1, 0), ("ui.nudge_up", 0, -1), ("ui.nudge_down", 0, 1),
                    ("ui.nudge_left_10", -10, 0), ("ui.nudge_right_10", 10, 0), ("ui.nudge_up_10", 0, -10), ("ui.nudge_down_10", 0, 10),
                ];
                for (key, dx, dy) in nudge {
                    if pressed(key, true) {
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

                if pressed("ui.delete", false) {
                    self.delete_selected(e);
                }

                if pressed("ui.duplicate", false) {
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

    /// Throws away every change since the layout was opened or saved (one undo step).
    fn reset_all(&mut self, e: &mut Editor) {
        let Some(id) = active_doc(e) else { return };
        let r = ui_doc(e, id).map(|d| d.reset_all());
        match r {
            Some(Ok(())) => e.status("all changes undone (Ctrl+Z brings them back)", false),
            Some(Err(m)) => e.status(&m, true),
            None => {}
        }
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
            self.xml.text = match (self.xml.whole, self.xml.raw) {
                (true, true) => d.xml().as_ref().clone(),
                (true, false) => d.model().to_named_xml(),
                (false, raw) => sel
                    .and_then(|i| d.entries().get(i).and_then(|en| if raw { d.model().dom.at(&en.path).map(|el| wce_ui_model::Dom { root: el.clone() }.to_xml()) } else { d.model().entry_to_named_xml(&en.path) }))
                    .unwrap_or_default(),
            };
            self.xml.shown = key;
            self.xml.error.clear();
        }

        if gui::checkbox("Whole file", &mut self.xml.whole) {
            self.xml.edited = false;
        }

        gui::same_line();
        if gui::checkbox("Converter names", &mut self.xml.raw) {
            self.xml.edited = false;
            self.xml.shown = (0, 0, None, false);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Off: the fields are named as in the Fields panel (offset_x, visible, ...). On: the converter's own tags and guesses (<i>, <yes />). Either form can be edited.");
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
            gui::text_colored(theme::error(), &format!("not applied: {}", self.xml.error));
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

        let entry = {
            let d = ui_doc_ref(e, id).unwrap();
            let Some(en) = d.entries().get(i).cloned() else {
                gui::end();
                return;
            };

            en
        };

        gui::text(&format!("{}   [{}]", entry.name, i));
        if entry.kind == EntryKind::Entry {
            self.entry_controls(e, id, &entry);
            self.animation_controls(e, id, &entry);
        }
        gui::checkbox("Advanced", &mut self.show_adv);
        if gui::is_item_hovered() {
            gui::set_tooltip("Also show the rarely needed properties. The whole file, field by field, is in the UI XML panel.");
        }

        let mut edit: Option<(Vec<usize>, usize, PropEdit, String)> = None;
        if gui::begin_tab_bar("##fields_tabs") {
            if gui::begin_tab_item("Component", 0) {
                edit = self.props_table(e, id, &entry.path, "c");
                gui::end_tab_item();
            }

            if gui::begin_tab_item("State", 0) {
                let st = self.state_tab(e, id, &entry);
                if st.is_some() {
                    edit = st;
                }

                gui::end_tab_item();
            }

            gui::end_tab_bar();
        }

        if let Some((path, idx, change, name)) = edit {
            // dragging a number or a colour is one undo step per property
            let key = if matches!(change, PropEdit::Colour(_) | PropEdit::Drag(_)) {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (&path, idx).hash(&mut h);
                h.finish() | 1
            } else {
                0
            };
            if let Some(d) = ui_doc(e, id) {
                if let Err(msg) = d.edit(&name, key, |m| m.edit_prop(&path, idx, change)) {
                    e.status(&msg, true);
                }
            }
        }

        gui::end();
    }

    /// The fields of the component, state, picture or mouse action at `path` under the game's names, in groups.
    fn props_table(&self, e: &Editor, id: DocumentId, path: &[usize], salt: &str) -> Option<(Vec<usize>, usize, PropEdit, String)> {
        let props = ui_doc_ref(e, id)?.model().properties(path);
        let mut out = None;
        for g in props.groups {
            let rows: Vec<&Prop> = props.items.iter().filter(|p| p.group == *g && (self.show_adv || !p.advanced)).collect();
            if rows.is_empty() {
                continue;
            }

            gui::separator_text(g);
            if gui::begin_table(&format!("##pt{salt}{}{g}", path.len()), 2, gui::TABLE_ROW_BG, v2(0.0, 0.0)) {
                gui::table_setup_column("Property", gui::COLUMN_WIDTH_STRETCH, 1.35);
                gui::table_setup_column("Value", gui::COLUMN_WIDTH_STRETCH, 1.0);
                for p in rows {
                    gui::table_next_row();
                    gui::table_next_column();
                    gui::text(&p.name);
                    if gui::is_item_hovered() {
                        let mut tip = p.help.to_string();
                        if !p.label.is_empty() {
                            tip.push_str(&format!("{}Converter name: {}", if tip.is_empty() { "" } else { "\n\n" }, p.label));
                        }

                        if !tip.is_empty() {
                            gui::set_tooltip(&tip);
                        }
                    }

                    gui::table_next_column();
                    gui::push_id(&format!("{salt}{}{}{}", path.len(), p.idx, p.name));
                    gui::set_next_item_width(-1.0);
                    if let Some(change) = prop_control(p) {
                        out = Some((path.to_vec(), p.idx, change, format!("Set {}", p.name)));
                    }

                    gui::pop_id();
                }

                gui::end_table();
            }
        }

        out
    }

    /// One state of the component (default: the default state), then its pictures and mouse actions.
    fn state_tab(&mut self, e: &Editor, id: DocumentId, entry: &Entry) -> Option<(Vec<usize>, usize, PropEdit, String)> {
        let (states, def_path) = {
            let m = ui_doc_ref(e, id)?.model();
            (m.state_paths(&entry.path), m.default_state_path(&entry.path))
        };

        if states.is_empty() {
            gui::text_disabled("(this component has no states)");
            return None;
        }

        let def_i = states.iter().position(|(p, _)| Some(p) == def_path.as_ref()).unwrap_or(0);
        let sel = self.state_pick.get(&entry.path).copied().filter(|k| *k < states.len()).unwrap_or(def_i);
        let label = |i: usize| format!("{}{}", states[i].1, if i == def_i { "  (default)" } else { "" });
        gui::set_next_item_width(220.0);
        if gui::begin_combo("State", &label(sel)) {
            for i in 0..states.len() {
                if gui::selectable(&label(i), i == sel, 0) {
                    self.state_pick.insert(entry.path.clone(), i);
                }
            }

            gui::end_combo();
        }

        let sp = states[sel].0.clone();
        let mut out = self.props_table(e, id, &sp, "s");

        let m = ui_doc_ref(e, id)?.model();
        let pics = m.state_children(&sp, "image_use");
        if !pics.is_empty() && gui::collapsing_header(&format!("Pictures ({})##pics", pics.len()), 0) {
            for (n, p) in pics.iter().enumerate() {
                if gui::tree_node_id(&format!("pic{n}"), &format!("{}  {}", n + 1, m.picture_label(p))) {
                    if let Some(x) = self.props_table(e, id, p, &format!("p{n}")) {
                        out = Some(x);
                    }

                    gui::tree_pop();
                }
            }
        }

        let acts = m.state_children(&sp, "mouse_state");
        if !acts.is_empty() && gui::collapsing_header(&format!("Mouse actions ({})##acts", acts.len()), 0) {
            for (n, p) in acts.iter().enumerate() {
                if gui::tree_node_id(&format!("act{n}"), &format!("{}  {}", n + 1, m.mouse_label(p))) {
                    if let Some(x) = self.props_table(e, id, p, &format!("a{n}")) {
                        out = Some(x);
                    }

                    gui::tree_pop();
                }
            }
        }

        out
    }

    // ---- project folder, export, hot reload

    /// Makes `folder` the game's projects folder: layouts are saved in its `loose/`, pictures put there show at once, and Export packs it.
    pub(crate) fn set_project(&mut self, e: &mut Editor, folder: &str) -> Result<(), String> {
        let f = folder.trim();
        if f.is_empty() {
            return Err("give a folder".into());
        }

        let loose = PathBuf::from(f).join("loose");
        std::fs::create_dir_all(loose.join("ui").join("skins")).map_err(|er| format!("cannot create {}: {er}", loose.display()))?;
        e.settings().game_mut().projects = Some(PathBuf::from(f));
        e.save_settings();
        e.remount_vfs()?;
        self.skins = None;
        self.missing.clear();
        self.loose_imgs.clear();
        e.status(&format!("project folder: {f}"), false);
        Ok(())
    }

    /// Saves the layouts with unsaved changes into the project folder, then writes every file of the folder into a new mod pack.
    pub(crate) fn export_pack(&mut self, e: &mut Editor, pack: &str) -> Result<usize, String> {
        let project = e.loose_dir();
        if !pack.to_lowercase().ends_with(".pack") {
            return Err("the pack file must end in .pack".into());
        }

        let ids: Vec<DocumentId> = e.docs.order().iter().copied().filter(|&i| ui_doc_ref(e, i).is_some()).collect();
        for id in ids {
            let Some(d) = ui_doc(e, id) else { continue };
            if d.modified() && !d.source.vfs_path.is_empty() {
                let to = project.join(&d.source.vfs_path);
                d.save(&wce_editor_core::document::SaveTarget { path: to })?;
            }
        }

        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        collect_files(&project, &project, &mut files)?;
        if files.is_empty() {
            return Err("the project folder has no files yet".into());
        }

        wce_ui_model::pack::write_pack_many(e.game(), std::path::Path::new(pack), &files)?;
        e.update_tool_settings(SETTINGS_ID, |s: &mut UiSettings| s.export_pack = pack.to_owned());
        e.status(&format!("exported {} files to {pack}", files.len()), false);
        Ok(files.len())
    }

    /// About once a second: reloads pictures and layouts whose files changed on disk, and notices pictures that appeared.
    fn watch_tick(&mut self, e: &mut Editor) {
        self.watch_acc += gui::io().DeltaTime;
        if self.watch_acc < 1.0 {
            return;
        }

        self.watch_acc = 0.0;
        // the skin list follows the mounted files and the skin folders on disk
        let sig = (e.vfs().map(|v| std::sync::Arc::as_ptr(v) as *const u8 as usize).unwrap_or(0), skins::loose_names(&e.loose_dir()));
        if sig != self.skin_sig {
            self.skin_sig = sig;
            self.skins = None;
            self.missing.clear();
        }

        let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let changed: Vec<String> = self.loose_imgs.iter().filter(|(_, (p, t, _))| mtime(p) != Some(*t)).map(|(k, _)| k.clone()).collect();
        for k in changed {
            self.loose_imgs.remove(&k);
            self.missing.remove(&k);
            e.images.forget(&k);
        }

        {
            let pd = e.loose_dir();
            let appeared: Vec<String> = self
                .missing
                .iter()
                .filter(|k| {
                    let (skin, path) = k.split_once('|').unwrap_or(("", k));
                    let skin = (!skin.is_empty()).then_some(skin);
                    skins::candidates(path, skin).iter().any(|c| pd.join(c).is_file())
                })
                .cloned()
                .collect();
            for k in appeared {
                self.missing.remove(&k);
            }
        }

        let project = e.loose_dir();
        let ids: Vec<DocumentId> = e.docs.order().iter().copied().filter(|&i| ui_doc_ref(e, i).is_some()).collect();
        for id in ids {
            let (path, modified) = {
                let d = ui_doc_ref(e, id).unwrap();
                let p = if d.source.vfs_path.is_empty() { Some(d.source.disk.clone()) } else { Some(project.join(&d.source.vfs_path)) };
                (p, d.modified())
            };

            let Some(path) = path.filter(|p| p.is_file()) else { continue };
            let Some(now) = mtime(&path) else { continue };
            let Some(&before) = self.doc_stamps.get(&id) else {
                self.doc_stamps.insert(id, now);
                continue;
            };

            if now == before {
                continue;
            }

            self.doc_stamps.insert(id, now);
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let model = if wce_ui_format::looks_like_ui(&bytes) { UiModel::from_bytes(&bytes) } else { std::str::from_utf8(&bytes).map_err(|_| "not UTF-8".to_owned()).and_then(UiModel::from_xml) };
            let Ok(model) = model else { continue };
            let same = ui_doc_ref(e, id).and_then(|d| d.bytes().ok()).is_some_and(|b| b == bytes);
            if same {
                continue;
            }

            if modified {
                if self.warned.insert(id) {
                    e.status(&format!("{} changed on disk, but it has unsaved changes here; File > Revert loads the disk version", path.display()), true);
                }
            } else if let Some(d) = ui_doc(e, id) {
                self.warned.remove(&id);
                match d.reload(model) {
                    Ok(()) => e.status(&format!("reloaded {} from disk", path.display()), false),
                    Err(m) => e.status(&m, true),
                }
            }
        }
    }

    /// Makes a skin of your own: a folder `ui/skins/<name>` in the project folder (made on first use), selected in the preview.
    pub(crate) fn new_skin(&mut self, e: &mut Editor, name: &str) -> Result<String, String> {
        let n: String = name.trim().to_lowercase().chars().map(|c| if c == ' ' { '_' } else { c }).collect();
        if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err("a skin name is letters, digits, - and _".into());
        }

        if n == "default" {
            return Err("\"default\" is the game's own skin: pick another name".into());
        }

        let root = e.loose_dir();

        let dir = root.join("ui").join("skins").join(&n);
        let existed = dir.is_dir();
        std::fs::create_dir_all(&dir).map_err(|er| format!("cannot create {}: {er}", dir.display()))?;
        self.skin = n.clone();
        self.skins = None;
        Ok(if existed { format!("skin {n} already existed: selected it ({})", dir.display()) } else { format!("skin {n} created: put your pictures in {}", dir.display()) })
    }

    /// Opens the folder of the selected loose skin in the file explorer.
    fn open_skin_folder(&self, e: &Editor) -> Result<(), String> {
        let root = e.loose_dir();
        let dir = root.join("ui").join("skins").join(&self.skin);
        if !dir.is_dir() {
            return Err(format!("{} does not exist", dir.display()));
        }

        let shown = dir.display().to_string().replace('/', "\\");
        std::process::Command::new("explorer").arg(shown).spawn().map(|_| ()).map_err(|er| format!("cannot open the file explorer: {er}"))
    }

    fn project_popups(&mut self, e: &mut Editor) {
        if self.new_skin_popup {
            self.new_skin_popup = false;
            self.new_skin_name.clear();
            self.new_skin_msg.clear();
            gui::open_popup("New skin");
        }

        if gui::begin_popup_modal("New skin", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            let root = e.loose_dir();
            gui::text_wrapped("Name your skin. The editor makes a folder with that name; put your PNG pictures in it (same names as the game's pictures in ui/skins/default) and the preview updates as you save them.");
            gui::set_next_item_width(300.0);
            gui::input_text("Name", Some("my_skin"), &mut self.new_skin_name, 64, 0);
            let n = self.new_skin_name.trim().to_lowercase().replace(' ', "_");
            if !n.is_empty() {
                gui::text_disabled(&format!("Folder: {}", root.join("ui").join("skins").join(&n).display()));
                let in_packs = self.skins.as_ref().is_some_and(|l| l.iter().any(|s| s.name == n && s.packed));
                if in_packs {
                    gui::push_text_wrap_pos(520.0);
                    gui::text_colored(theme::warning(), "The game's packs already have a skin with this name. Your pictures replace the pack's pictures of the same name in the editor; pictures you do not provide still come from the pack.");
                    gui::pop_text_wrap_pos();
                }
            }

            if !self.new_skin_msg.is_empty() {
                gui::text_colored(theme::error(), &self.new_skin_msg);
            }

            if gui::button("Create") {
                let name = self.new_skin_name.clone();
                match self.new_skin(e, &name) {
                    Ok(m) => {
                        e.status(&m, false);
                        gui::close_current_popup();
                    }
                    Err(m) => self.new_skin_msg = m,
                }
            }

            gui::same_line();
            if gui::button("Cancel") {
                gui::close_current_popup();
            }

            gui::end_popup();
        }

        if self.export_popup {
            self.export_popup = false;
            self.export_msg.clear();
            if self.export_path.is_empty() {
                self.export_path = Some(e.tool_settings::<UiSettings>(SETTINGS_ID).export_pack).filter(|p| !p.is_empty()).or_else(|| Some(format!("{}.pack", e.loose_dir().display()))).unwrap_or_default();
            }

            gui::open_popup("Export mod pack");
        }

        if gui::begin_popup_modal("Export mod pack", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            gui::text_wrapped("Writes everything in the project folder (layouts you saved, your pictures) into one mod pack you can share. Layouts with unsaved changes are saved into the folder first. The game's own packs are never touched.");
            gui::set_next_item_width(560.0);
            gui::input_text("Pack file", Some("C:\\Mods\\my_ui_skin.pack"), &mut self.export_path, 1024, 0);
            if !self.export_msg.is_empty() {
                gui::text_wrapped(&self.export_msg);
            }

            if gui::button("Export") {
                let p = self.export_path.clone();
                self.export_msg = match self.export_pack(e, &p) {
                    Ok(n) => format!("Done: {n} files written."),
                    Err(m) => m,
                };
            }

            gui::same_line();
            if gui::button("Close") {
                gui::close_current_popup();
            }

            gui::end_popup();
        }
    }

    // ---- layers: make, remove, reorder and reparent components

    /// Moves component `src` next to or into `target` (numbers in the layout's list), as a drop in the Layers panel does.
    pub(crate) fn drop_layer(&mut self, e: &mut Editor, id: DocumentId, src: usize, target: usize, place: wce_ui_model::model::Place) -> Result<(), String> {
        let ents = Self::entries_of(e, id);
        let sp = ents.get(src).map(|en| en.path.clone()).ok_or("no such component")?;
        let tp = ents.get(target).map(|en| en.path.clone()).ok_or("no such target")?;
        let new = ui_doc(e, id).ok_or("no UI layout is open")?.edit("Move component", 0, |m| m.move_to(&sp, &tp, place, true))?;
        self.select_path(e, id, &new);
        Ok(())
    }

    pub(crate) fn rename_entry(&mut self, e: &mut Editor, id: DocumentId, idx: usize, name: &str) -> Result<(), String> {
        let path = Self::entries_of(e, id).get(idx).map(|en| en.path.clone()).ok_or("no such component")?;
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err("a component needs a name".into());
        }

        ui_doc(e, id).ok_or("no UI layout is open")?.edit("Rename component", 0, |m| m.set_name(&path, &name))
    }

    fn layers_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(300.0, 480.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("UI Layers", None, 0) {
            gui::end();
            return;
        }

        let Some(id) = active_doc(e) else {
            gui::text_disabled("No UI layout is open.");
            gui::end();
            return;
        };

        let sel = selected(e, id);
        if self.rename.as_ref().is_some_and(|r| Some(r.0) != sel) {
            self.rename = None;
        }

        // the buttons: one per thing a modder does to the structure
        let mut run: Option<&str> = None;
        let button = |label: &str, tip: &str, run: &mut Option<&str>, what: &'static str, enabled: bool| {
            gui::begin_disabled(!enabled);
            if gui::small_button(label) {
                *run = Some(what);
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(tip);
            }
        };

        let has = sel.is_some();
        button("Add", "Add a new component inside the selected one (or inside the root when nothing is selected)", &mut run, "add", true);
        gui::same_line();
        button("Duplicate", "Copy the selected component, with everything inside it, right after it (Ctrl+D)", &mut run, "dup", has);
        gui::same_line();
        button("Delete", "Remove the selected component and everything inside it (Del)", &mut run, "del", has);
        button("Up", "Earlier among its siblings: drawn earlier, so behind the ones after it", &mut run, "up", has);
        gui::same_line();
        button("Down", "Later among its siblings: drawn later, so in front of the ones before it", &mut run, "down", has);
        gui::same_line();
        button("In", "Make it a child of the sibling just above it; it stays where it is on screen", &mut run, "in", has);
        gui::same_line();
        button("Out", "Make it a sibling of its parent, right where it is on screen", &mut run, "out", has);
        let r = match run {
            Some("add") => {
                if !has {
                    select(e, id, 0);
                }

                let r = self.add_child(e, "new_component");
                if r.is_ok() {
                    if let Some(i) = selected(e, id) {
                        self.rename = Some((i, "new_component".to_owned(), 0));
                    }
                }

                r
            }
            Some("dup") => self.with_selected(e, "Duplicate component", |m, p| m.duplicate(p)),
            Some("del") => {
                self.delete_selected(e);
                Ok(())
            }
            Some("up") => self.move_in_order(e, -1),
            Some("down") => self.move_in_order(e, 1),
            Some("in") => self.move_into_previous(e),
            Some("out") => self.move_out(e),
            _ => Ok(()),
        };

        if let Err(m) = r {
            e.status(&m, true);
        }

        gui::separator();
        let Some(d) = ui_doc_ref(e, id) else {
            gui::end();
            return;
        };

        let ents = d.entries();
        let mut kids: Vec<Vec<usize>> = vec![vec![]; ents.len()];
        for (i, en) in ents.iter().enumerate() {
            if let Some(p) = en.parent {
                kids[p].push(i);
            }
        }

        let sel = selected(e, id);
        let mut f = LayerFrame { sel, ..Default::default() };
        if !ents.is_empty() {
            layer_row(&mut self.rename, &mut f, &ents, &kids, 0, 0);
        }

        gui::separator();
        gui::text_wrapped("Drag a row onto another: the middle makes it a child, the top or bottom edge puts it before or after. Double-click renames. Right-click for more.");

        if let Some(i) = f.select {
            select(e, id, i);
        }

        if let Some(i) = f.rename_start {
            let name = ents.get(i).map(|en| en.name.clone()).unwrap_or_default();
            select(e, id, i);
            self.rename = Some((i, name, 0));
        }

        if let Some((i, name)) = f.rename_commit.take() {
            self.rename = None;
            if let Err(m) = self.rename_entry(e, id, i, &name) {
                e.status(&m, true);
            }
        }

        if let Some(c) = f.menu {
            select(e, id, c.0);
            let r = match c.1 {
                "add" => self.add_child(e, "new_component"),
                "dup" => self.with_selected(e, "Duplicate component", |m, p| m.duplicate(p)),
                "del" => {
                    self.delete_selected(e);
                    Ok(())
                }
                _ => Ok(()),
            };

            if let Err(m) = r {
                e.status(&m, true);
            }
        }

        if let Some((src, target, place)) = f.drop {
            if let Err(m) = self.drop_layer(e, id, src, target, place) {
                e.status(&m, true);
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
                self.pack_path = e.tool_settings::<UiSettings>(SETTINGS_ID).mod_pack;
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
                gui::text_colored(theme::error(), &self.pack_message);
            }

            if gui::button("Save") {
                let game = e.game();
                let r = active_doc(e).ok_or_else(|| "no UI layout is open".to_owned()).and_then(|id| {
                    let d = ui_doc_ref(e, id).ok_or("no UI layout is open")?;
                    if !self.pack_path.to_lowercase().ends_with(".pack") {
                        return Err("the pack file must end in .pack".to_owned());
                    }

                    d.save_to_pack(game, std::path::Path::new(&self.pack_path), &self.pack_inside)
                });
                match r {
                    Ok(()) => {
                        let pack = self.pack_path.clone();
                        e.update_tool_settings(SETTINGS_ID, |s: &mut UiSettings| s.mod_pack = pack);
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

/// The control for one property; returns the change when the user made one this frame.
fn prop_control(p: &Prop) -> Option<PropEdit> {
    let text_edit = |value: &str, width: f32| -> Option<String> {
        gui::set_next_item_width(width);
        let mut v = value.to_owned();
        let enter = gui::input_text("##v", None, &mut v, 1024, gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL);
        ((enter || gui::is_item_deactivated_after_edit()) && v != value).then_some(v)
    };

    match &p.control {
        Control::Check => {
            let mut on = p.value == "yes";
            gui::checkbox("##v", &mut on).then(|| PropEdit::Value(if on { "yes" } else { "no" }.into()))
        }
        Control::Bit { mask, byte } => {
            let w = p.value.trim().parse::<i64>().unwrap_or(0) as u32;
            let mut on = if *byte { w & mask != 0 } else { w & mask == *mask };
            gui::checkbox("##v", &mut on).then_some(PropEdit::Bit { mask: *mask, byte: *byte, on })
        }
        Control::Choice(list) => {
            let cur: i64 = p.value.trim().parse().unwrap_or(i64::MIN);
            let preview = list.iter().find(|(n, _)| *n == cur).map(|(_, s)| s.clone()).unwrap_or_else(|| format!("Other ({})", p.value));
            let mut out = None;
            if gui::begin_combo("##v", &preview) {
                for (n, s) in list {
                    if gui::selectable(s, *n == cur, 0) {
                        out = Some(PropEdit::Value(n.to_string()));
                    }
                }

                gui::end_combo();
            }

            out
        }
        Control::Suggest(list) => {
            let mut out = text_edit(&p.value, -86.0).map(PropEdit::Value);
            gui::same_line();
            gui::set_next_item_width(80.0);
            if gui::begin_combo("##sug", "pick") {
                for s in list.iter() {
                    if gui::selectable(s, *s == p.value, 0) {
                        out = Some(PropEdit::Value((*s).to_owned()));
                    }
                }

                gui::end_combo();
            }

            out
        }
        Control::Text => text_edit(&p.value, -1.0).map(PropEdit::Value),
        Control::Int => {
            let mut n: i64 = p.value.trim().parse().unwrap_or(0);
            let mut out = None;
            gui::set_next_item_width(-52.0);
            if gui::drag_i64("##v", &mut n, 1.0) {
                out = Some(PropEdit::Drag(n.to_string()));
            }

            gui::same_line();
            if gui::small_button("-") {
                out = Some(PropEdit::Value((n - 1).to_string()));
            }

            gui::same_line();
            if gui::small_button("+") {
                out = Some(PropEdit::Value((n + 1).to_string()));
            }

            out
        }
        Control::Float => {
            let mut f: f32 = p.value.trim().parse().unwrap_or(0.0);
            let mut out = None;
            gui::set_next_item_width(-52.0);
            if gui::drag_f32("##v", &mut f, 0.01) {
                out = Some(PropEdit::Drag(format!("{:?}", f)));
            }

            gui::same_line();
            if gui::small_button("-") {
                out = Some(PropEdit::Value(format!("{:?}", f - 0.1)));
            }

            gui::same_line();
            if gui::small_button("+") {
                out = Some(PropEdit::Value(format!("{:?}", f + 0.1)));
            }

            out
        }
        Control::Colour => {
            let h = p.value.trim_start_matches('#');
            let ch = |i: usize| u8::from_str_radix(h.get(i * 2..i * 2 + 2).unwrap_or("ff"), 16).unwrap_or(255);
            let mut c = [ch(0) as f32 / 255.0, ch(1) as f32 / 255.0, ch(2) as f32 / 255.0, ch(3) as f32 / 255.0];
            gui::color_edit4("##v", &mut c, 0).then(|| PropEdit::Colour([(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8, (c[3] * 255.0).round() as u8]))
        }
        Control::Info => {
            gui::text_disabled(&p.value);
            None
        }
    }
}

/// Every file under `dir` as (path relative to `root`, lower case with `/`, bytes); hidden files and packs are left out.
fn collect_files(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    for ent in std::fs::read_dir(dir).map_err(|er| format!("cannot read {}: {er}", dir.display()))? {
        let ent = ent.map_err(|er| er.to_string())?;
        let p = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.to_lowercase().ends_with(".pack") {
            continue;
        }

        if p.is_dir() {
            collect_files(root, &p, out)?;
        } else if let Ok(rel) = p.strip_prefix(root) {
            let inside = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>().join("/");
            out.push((inside, std::fs::read(&p).map_err(|er| format!("cannot read {}: {er}", p.display()))?));
        }
    }

    Ok(())
}

/// What the rows of the Layers panel asked for in one frame.
#[derive(Default)]
struct LayerFrame {
    sel: Option<usize>,
    select: Option<usize>,
    rename_start: Option<usize>,
    rename_commit: Option<(usize, String)>,
    menu: Option<(usize, &'static str)>,
    drop: Option<(usize, usize, wce_ui_model::model::Place)>,
}

/// One row of the Layers panel, and the rows below it when it is open.
fn layer_row(rename: &mut Option<(usize, String, u8)>, f: &mut LayerFrame, ents: &[Entry], kids: &[Vec<usize>], i: usize, depth: usize) {
    use wce_ui_model::model::Place;
    let en = &ents[i];
    gui::push_id_int(i as i32);
    let leaf = kids[i].is_empty();
    let mut flags = gui::TREE_OPEN_ON_ARROW | gui::TREE_SPAN_AVAIL_WIDTH;
    if leaf {
        flags |= gui::TREE_LEAF | gui::TREE_NO_PUSH_ON_OPEN;
    } else if depth < 3 {
        flags |= gui::TREE_DEFAULT_OPEN;
    }

    if f.sel == Some(i) {
        flags |= gui::TREE_SELECTED;
    }

    let renaming = rename.as_ref().is_some_and(|r| r.0 == i);
    let mut open = false;
    if renaming {
        let (_, text, age) = rename.as_mut().unwrap();
        if *age == 0 {
            gui::set_keyboard_focus_here(0);
        }

        gui::set_next_item_width(-1.0);
        let enter = gui::input_text("##rename", None, text, 128, gui::INPUT_ENTER_RETURNS_TRUE | gui::INPUT_AUTO_SELECT_ALL);
        let active = gui::is_item_active();
        *age = age.saturating_add(1);
        if enter || gui::is_item_deactivated_after_edit() {
            f.rename_commit = Some((i, text.clone()));
        } else if gui::is_key_pressed(gui::KEY_ESCAPE, false) || (*age > 2 && !active) {
            *rename = None;
        }
    } else {
        let mut label = if en.kind == EntryKind::Template { format!("template {}", en.name) } else { en.name.clone() };
        if !en.class.is_empty() && en.kind == EntryKind::Entry {
            label.push_str(&format!("  ({})", en.class));
        }

        if !en.visible {
            label.push_str("  [hidden]");
        }

        open = gui::tree_node_ex(&label, flags);
        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
            f.select = Some(i);
        }

        if gui::is_item_hovered() && gui::is_mouse_double_clicked(0) && !leaf_toggled() {
            f.rename_start = Some(i);
        }

        if gui::begin_popup_context_item("##layer_menu") {
            if gui::menu_item("Add child", None, false, true) {
                f.menu = Some((i, "add"));
            }

            if gui::menu_item("Duplicate", None, false, i != 0) {
                f.menu = Some((i, "dup"));
            }

            if gui::menu_item("Delete", None, false, i != 0) {
                f.menu = Some((i, "del"));
            }

            if gui::menu_item("Rename", None, false, true) {
                f.rename_start = Some(i);
            }

            gui::end_popup();
        }

        if i != 0 && gui::begin_drag_drop_source() {
            gui::set_drag_drop_payload("UILAYER", &[i as i32]);
            gui::text(&en.name);
            gui::end_drag_drop_source();
        }

        if gui::begin_drag_drop_target() {
            if let Some((data, delivered)) = gui::accept_drag_drop_payload("UILAYER") {
                let (mn, mx) = gui::item_rect();
                let h = mx.y - mn.y;
                let y = gui::mouse_pos().y;
                let place = if i == 0 || (y >= mn.y + h * 0.25 && y <= mx.y - h * 0.25) {
                    Place::Inside
                } else if y < mn.y + h * 0.25 {
                    Place::Before
                } else {
                    Place::After
                };

                let dl = gui::window_draw_list();
                let col = theme::u32(theme::palette().highlight);
                match place {
                    Place::Before => dl.line(v2(mn.x, mn.y), v2(mx.x, mn.y), col, 2.0),
                    Place::After => dl.line(v2(mn.x, mx.y), v2(mx.x, mx.y), col, 2.0),
                    Place::Inside => dl.rect(mn, mx, col, 0.0, 2.0),
                }

                if delivered {
                    if let Some(&src) = data.first() {
                        f.drop = Some((src as usize, i, place));
                    }
                }
            }

            gui::end_drag_drop_target();
        }
    }

    if open && !leaf {
        for &c in &kids[i] {
            layer_row(rename, f, ents, kids, c, depth + 1);
        }

        gui::tree_pop();
    }

    gui::pop_id();
}

/// True while the mouse is on the arrow that opens a row (a double-click there only toggles it).
fn leaf_toggled() -> bool {
    gui::is_item_toggled_open()
}

/// A skin in the list: its name, how many pictures, and where it comes from.
fn skin_label(s: &skins::Skin) -> String {
    let origin = match (s.loose, s.packed) {
        (true, true) => "  [loose, overrides game packs]",
        (true, false) => "  [loose]",
        _ => "",
    };
    format!("{}  ({}){}", s.name, s.files, origin)
}

fn ui_doc_ref(e: &Editor, id: DocumentId) -> Option<&UiDocument> {
    e.docs.find_as::<UiDocument>(id)
}

/// The path of the entry that holds the entry at `path` (the `children` list sits between them).
fn parent_entry_path(path: &[usize]) -> Vec<usize> {
    path[..path.len().saturating_sub(2)].to_vec()
}

/// The UI tool's keys (Settings > Shortcuts).
const SHORTCUTS: &[wce_editor::shortcuts::Action] = {
    use wce_editor::shortcuts::{Action, Scope};
    const fn a(id: &'static str, label: &'static str, defaults: &'static [&'static str]) -> Action {
        Action { id, scope: Scope::Tool("ui"), group: "UI", label, defaults, held: false }
    }

    &[
        a("ui.delete", "Delete", &["Delete"]),
        a("ui.duplicate", "Duplicate", &["Ctrl+D"]),
        a("ui.nudge_left", "Nudge left (1 px)", &["LeftArrow"]),
        a("ui.nudge_right", "Nudge right (1 px)", &["RightArrow"]),
        a("ui.nudge_up", "Nudge up (1 px)", &["UpArrow"]),
        a("ui.nudge_down", "Nudge down (1 px)", &["DownArrow"]),
        a("ui.nudge_left_10", "Nudge left (10 px)", &["Shift+LeftArrow"]),
        a("ui.nudge_right_10", "Nudge right (10 px)", &["Shift+RightArrow"]),
        a("ui.nudge_up_10", "Nudge up (10 px)", &["Shift+UpArrow"]),
        a("ui.nudge_down_10", "Nudge down (10 px)", &["Shift+DownArrow"]),
    ]
};

impl Tool for UiTool {
    fn id(&self) -> &str {
        "ui"
    }

    fn game_settings_fields(&self) -> &'static [&'static str] {
        crate::settings::GAME_FIELDS
    }

    fn shortcuts(&self) -> &'static [wce_editor::shortcuts::Action] {
        SHORTCUTS
    }

    fn title(&self) -> &str {
        "UI"
    }

    fn icon(&self) -> &str {
        wce_editor::icons::LAYOUT_PANEL_TOP
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
        gui::dock_builder_dock_window("UI Layers", left);
        gui::dock_builder_dock_window("UI Fields", right);
        gui::dock_builder_dock_window("UI Related", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
        gui::dock_builder_dock_window("UI XML", bottom);
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        e.input_origin = Some(v2(0.0, 0.0));
        self.watch_tick(e);
        self.canvas(e);
        self.xml_pane(e);
        self.layers_panel(e);
        self.fields_panel(e);
        self.related_panel(e);
        self.pack_popup(e);
        self.project_popups(e);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if gui::begin_menu("UI") {
            let has = active_doc(e).is_some();
            let changed = active_doc(e).and_then(|id| ui_doc_ref(e, id)).is_some_and(|d| d.modified());
            if gui::menu_item("Reset all changes", None, false, changed) {
                self.reset_all(e);
            }

            let sel = has && active_doc(e).and_then(|id| selected(e, id)).is_some();
            gui::begin_disabled(!sel);
            if gui::menu_item("Add child", None, false, true) {
                if let Err(m) = self.add_child(e, "new_component") {
                    e.status(&m, true);
                }
            }

            if gui::menu_item("Duplicate", wce_editor::shortcuts::label("ui.duplicate").as_deref(), false, true) {
                self.duplicate_selected(e);
            }

            if gui::menu_item("Delete", wce_editor::shortcuts::label("ui.delete").as_deref(), false, true) {
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

            gui::end_disabled();
            gui::separator();
            if gui::menu_item("New skin...", None, false, true) {
                self.new_skin_popup = true;
            }

            if gui::menu_item("Export mod pack...", None, false, true) {
                self.export_popup = true;
            }

            gui::begin_disabled(!has);

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
        let out = e.loose_dir();
        Ok(Box::new(UiDocument::new(source, model, out)))
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
