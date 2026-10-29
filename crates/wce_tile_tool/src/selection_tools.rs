//! Selection tools (Phase 5g of docs/plans/tile_ted_parity.md): box select (TEd's Frustum selection: a left drag in Select mode, or B then a
//! drag in any mode; Shift adds, Ctrl takes away), selecting by the primary's type or key, everything, the inverse, and selections saved by name for the session.

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_editor::{gui, icons, theme};
use wce_scene::render_scene::{DocumentId, SourceRef};
use wce_tile_editor::tile_document::{TileDocument, item_of};

use crate::tool::{TileTool, document, document_id, primary, selected};

/// The box select: armed by B, the rectangle once the drag started (image pixels: the press and where the mouse is).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BoxSelect {
    pub armed: bool,
    pub from: Option<(i32, i32)>,
    pub to: (i32, i32),
}

/// A selection saved by name: the tile's elements as they were numbered then (an edit of the tile's structure since makes it stale).
#[derive(Clone, Debug)]
pub(crate) struct SavedSelection {
    pub name: String,
    pub doc: DocumentId,
    pub elems: Vec<usize>,
    pub structure: u64,
}

/// Replaces the selection with these elements of the open tile.
pub(crate) fn set_selection(e: &mut Editor, elems: &[usize]) {
    let Some(id) = document_id(e) else { return };
    e.selection.clear();
    for &el in elems {
        e.selection.add(SourceRef { doc: id, item: item_of(el) });
    }
}

impl TileTool {
    /// The shown, selectable instances of the open tile with `keep`.
    fn instances_where(e: &Editor, keep: impl Fn(&TileDocument, usize) -> bool) -> Vec<usize> {
        let Some(d) = document_id(e).and_then(|id| e.docs.find_as::<TileDocument>(id)) else { return vec![] };
        let t = d.tile();
        t.instances().iter().copied().filter(|&el| t.shown(el) && t.selectable(el) && keep(d, el)).collect()
    }

    /// Selects every instance of the primary's type (`same_key`: and key).
    pub(crate) fn select_like_primary(&mut self, e: &mut Editor, same_key: bool) {
        let Some(p) = primary(e) else { return };
        let Some(d) = document(e) else { return };
        let sc = d.tile().tscene();
        let (ty, key) = (sc.type_of(p).to_owned(), sc.doc().get(p, "key"));
        let found = Self::instances_where(e, |d, el| {
            let sc = d.tile().tscene();
            sc.type_of(el) == ty && (!same_key || sc.doc().get(el, "key") == key)
        });
        set_selection(e, &found);
        self.status = format!("{} selected", found.len());
    }

    pub(crate) fn select_everything(&mut self, e: &mut Editor) {
        let found = Self::instances_where(e, |_, _| true);
        set_selection(e, &found);
        self.status = format!("{} selected", found.len());
    }

    /// Selects the shown, selectable instances that are not selected.
    pub(crate) fn invert_selection(&mut self, e: &mut Editor) {
        let now = selected(e);
        let found = Self::instances_where(e, |_, el| !now.contains(&el));
        set_selection(e, &found);
        self.status = format!("{} selected", found.len());
    }

    /// Saves the selection under a name (one of that name is replaced).
    pub(crate) fn save_selection(&mut self, e: &mut Editor, name: &str) -> bool {
        let elems = selected(e);
        let Some(d) = document(e) else { return false };
        if elems.is_empty() {
            self.status = "nothing selected to save".to_owned();
            return false;
        }

        let s = SavedSelection { name: name.to_owned(), doc: d.id(), elems, structure: d.tile().structure_version() };
        self.saved_selections.retain(|x| !(x.doc == s.doc && x.name == name));
        self.saved_selections.push(s);
        self.status = format!("selection '{name}' saved");
        true
    }

    /// Selects a saved selection of the open tile; false when it is gone or stale (the tile's structure changed since).
    pub(crate) fn restore_selection(&mut self, e: &mut Editor, name: &str) -> bool {
        let Some(d) = document(e) else { return false };
        let (id, structure) = (d.id(), d.tile().structure_version());
        let Some(s) = self.saved_selections.iter().find(|s| s.doc == id && s.name == name).cloned() else { return false };
        if s.structure != structure {
            self.status = format!("selection '{name}' is out of date (things were added or removed since)");
            return false;
        }

        set_selection(e, &s.elems);
        self.status = format!("selection '{name}': {} item(s)", s.elems.len());
        true
    }

    /// The selection menu's "Select" submenu.
    pub(crate) fn select_menu(&mut self, e: &mut Editor) {
        let some = primary(e).is_some();
        if !gui::begin_menu("Select") {
            return;
        }

        if gui::menu_item("Box select", Some("B"), false, true) {
            self.box_select = BoxSelect { armed: true, ..Default::default() };
            self.status = "box select: drag a rectangle (Shift adds, Ctrl takes away; Esc cancels)".to_owned();
        }

        if gui::menu_item("All of this type", None, false, some) {
            self.select_like_primary(e, false);
        }

        if gui::menu_item("All with this key", None, false, some) {
            self.select_like_primary(e, true);
        }

        if gui::menu_item("Everything", None, false, true) {
            self.select_everything(e);
        }

        if gui::menu_item("Invert", None, false, true) {
            self.invert_selection(e);
        }

        gui::separator();
        let id = document_id(e);
        let names = self.saved_selections.iter().filter(|s| Some(s.doc) == id).map(|s| s.name.clone()).collect::<Vec<_>>();
        for n in names {
            if gui::menu_item(&format!("{}  {n}", icons::HISTORY), None, false, true) {
                self.restore_selection(e, &n);
            }
        }

        gui::set_next_item_width(140.0);
        let enter = gui::input_text("##selname", Some("name"), &mut self.selection_name, 64, gui::INPUT_ENTER_RETURNS_TRUE);
        gui::same_line();
        if (gui::small_button("Save the selection") || enter) && !self.selection_name.trim().is_empty() {
            let n = self.selection_name.trim().to_owned();
            self.save_selection(e, &n);
            self.selection_name.clear();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Kept for this session; adding or removing things in the tile makes it out of date.");
        }

        gui::end_menu();
    }

    /// The box select's mouse: the press starts the rectangle, the drag stretches it, the release selects what is inside. True while it
    /// has the mouse.
    pub(crate) fn box_select_input(&mut self, e: &mut Editor, kind: wce_editor::tool::ViewportEventKind, x: i32, y: i32) -> bool {
        use wce_editor::tool::ViewportEventKind as K;
        if !self.box_select.armed {
            return false;
        }

        match kind {
            K::Press => {
                if self.box_select.from.is_none() {
                    self.box_select.from = Some((x, y));
                }

                self.box_select.to = (x, y);
                true
            }

            K::Drag => {
                self.box_select.to = (x, y);
                true
            }

            K::Release => {
                let Some(from) = self.box_select.from else { return false };
                let to = (x, y);
                self.box_select = BoxSelect::default();
                let (lo, hi) = ((from.0.min(to.0) as f32, from.1.min(to.1) as f32), (from.0.max(to.0) as f32, from.1.max(to.1) as f32));
                let io = gui::io();
                let inside = Self::instances_where(e, |d, el| {
                    e.viewport.project(d.tile().world_position(el)).is_some_and(|(px, py)| px >= lo.0 && px <= hi.0 && py >= lo.1 && py <= hi.1)
                });

                let mut now = if io.KeyShift || io.KeyCtrl { selected(e) } else { vec![] };
                if io.KeyCtrl {
                    now.retain(|el| !inside.contains(el));
                } else {
                    for el in inside {
                        if !now.contains(&el) {
                            now.push(el);
                        }
                    }
                }

                set_selection(e, &now);
                self.status = format!("{} selected", now.len());
                true
            }

            _ => false,
        }
    }

    /// A left drag's selection box (`Tool::box_drag`, Select mode): drawn as the B box is, selecting on the release.
    pub(crate) fn drag_box(&mut self, e: &mut Editor, from: (i32, i32), to: (i32, i32), done: bool) {
        self.box_select = BoxSelect { armed: true, from: Some(from), to };
        if done {
            self.box_select_input(e, wce_editor::tool::ViewportEventKind::Release, to.0, to.1);
        }
    }

    /// The box select's rectangle over the viewport image.
    pub(crate) fn draw_box_select(&self, origin: gui::Vec2) {
        let Some(from) = self.box_select.from.filter(|_| self.box_select.armed) else { return };
        let density = gui::io().DisplayFramebufferScale.x.max(1e-3);
        let at = |p: (i32, i32)| gui::v2(origin.x + p.0 as f32 / density, origin.y + p.1 as f32 / density);
        let (a, b) = (at(from), at(self.box_select.to));
        let (lo, hi) = (gui::v2(a.x.min(b.x), a.y.min(b.y)), gui::v2(a.x.max(b.x), a.y.max(b.y)));
        let dl = gui::window_draw_list();
        let h = theme::palette().highlight;
        dl.rect_filled(lo, hi, gui::col32((h[0] * 255.0) as u8, (h[1] * 255.0) as u8, (h[2] * 255.0) as u8, 40), 0.0);
        dl.rect(lo, hi, theme::u32(h), 0.0, 1.0);
    }
}
