//! Prefabs in the Tile tool (`wce_tile::prefabs`): the library of TEd's saved groups (`<kit raw_data>/art/prefabs` of the kits and of the
//! tile), the Prefabs panel to place them, and the copies in the tile: a reference to a prefab (`wce_prefab`) is a copy, shown as a
//! Prefab row (its items listed when it is opened in the tree; a click on them in the viewport selects the copy). "Edit prefab" opens a copy for editing; Apply writes the
//! prefab file and gives every other copy in the tile the new contents (one undo entry), Cancel gives the copy the file's contents back.
//! "Unpack prefab" renames a copy so that it is a plain group; "Save as prefab" writes a group as a new prefab and names it after it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_foundation::math::Float3;
use wce_scene::render_scene::SourceRef;
use wce_tile::prefab_refs::ContentIndex;
use wce_tile::prefabs::{self, PrefabLibrary};
use wce_tile::tile_project::PrefabSource;
use wce_tile_editor::tile_document::{TileDocument, item_of};

use crate::scene_panel::{layer_by_key, layer_key};
use crate::tool::{TileTool, document, document_id, selected};

/// The library in use and the prefab contents read from it (each once; dropped with the library), shared with the open tiles' projects.
#[derive(Default)]
pub(crate) struct PrefabBase {
    lib: Option<Arc<PrefabLibrary>>,
    contents: HashMap<String, Option<Arc<Vec<u8>>>>,
    index: Option<Arc<ContentIndex>>,
}

impl PrefabBase {
    /// A key's contents, as a copy holds them (`PrefabLibrary::contents`).
    pub(crate) fn contents(&mut self, key: &str) -> Option<Arc<Vec<u8>>> {
        let k = key.to_lowercase();
        if let Some(c) = self.contents.get(&k) {
            return c.clone();
        }

        let c = self.lib.as_ref()?.contents(key).map(Arc::new);
        self.contents.insert(k, c.clone());
        c
    }
}

pub(crate) type SharedBase = Arc<Mutex<PrefabBase>>;

/// A key's contents from the shared base.
pub(crate) fn base_contents(base: &SharedBase, key: &str) -> Option<Arc<Vec<u8>>> {
    base.lock().ok()?.contents(key)
}

impl TileTool {
    /// The source of the prefabs' contents for a tile's project (`TileProject::prefab_base`), following the library in use.
    pub(crate) fn prefab_source(&self) -> PrefabSource {
        let base = self.prefab_base.clone();
        PrefabSource(Arc::new(move |k: &str| base_contents(&base, k).map(|c| (*c).clone())))
    }

    /// The index of the prefabs' contents (to find the groups that are copies of one), made once per library.
    pub(crate) fn content_index(&mut self, e: &mut Editor) -> Arc<ContentIndex> {
        let lib = self.prefab_library(e);
        let Ok(mut base) = self.prefab_base.lock() else { return Arc::default() };
        if let Some(i) = &base.index {
            return i.clone();
        }

        let keys = lib.entries().map(|p| p.key.clone()).collect::<Vec<_>>();
        let all = keys.iter().filter_map(|k| base.contents(k).map(|c| (k.as_str(), (*c).clone()))).collect::<Vec<_>>();
        let index = Arc::new(ContentIndex::new(all));
        base.index = Some(index.clone());
        index
    }

    /// Makes the instance groups that are exact copies of a prefab references to it (one undo entry), the ones inside those left as their
    /// contents. Returns what was done.
    ///
    /// # Errors
    ///
    /// No tile open.
    pub fn recognise_prefabs(&mut self, e: &mut Editor) -> Result<String, String> {
        let index = self.content_index(e);
        let lib = self.prefab_library(e);
        let d = document(e).ok_or("no tile open")?;
        let found = wce_tile::prefab_refs::recognise(d.tile().tscene(), &index, &|n| lib.get(n).is_some());
        let n = d.make_references(&found.copies, "Turn copies of prefabs into references");
        self.prefab_version += 1;
        Ok(recognised_message(n, found.named_but_different))
    }

    /// The Tile panel's Prefabs: the references in the tile, and turning the groups that are copies of a prefab into references (done when
    /// a tile opens from the Assembly Kit; this does it again).
    pub(crate) fn draw_prefabs_section(&mut self, e: &mut Editor, id: wce_scene::render_scene::DocumentId) {
        if !gui::collapsing_header("Prefabs", 0) {
            return;
        }

        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let sc = d.tile().tscene();
        let refs = sc.find_all("UserInstanceGroup").into_iter().filter(|&g| d.is_prefab_reference(g)).count();
        gui::text(&format!("{refs} prefab reference(s)"));
        gui::text_wrapped("A reference's contents are its prefab's; select it, then one of its items (click it again, or in the Scene tree), and what you edit is an override, marked in the Inspector.");
        if gui::button(&format!("{}  Turn copies of prefabs into references", icons::BOXES)) {
            self.status = self.recognise_prefabs(e).unwrap_or_else(|err| err);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Every instance group whose contents are exactly a prefab's becomes a reference to it (one undo entry). Done when a tile opens from the Assembly Kit.");
        }
    }

    /// The folders the library is read from: the projects' `raw_data/art/prefabs` (new prefabs go there), then `art/prefabs` of the open
    /// tile's kit and of the kits the Tiles panel lists (the Assembly Kit).
    pub(crate) fn prefab_folders(&self, e: &mut Editor) -> Vec<std::path::PathBuf> {
        let mut folders = vec![prefabs::prefab_root(&Self::workspace_dir(e).join("raw_data"))];
        let mut kits = self.kit_roots(e);
        if let Some(raw) = document(e).and_then(|d| d.project().raw_root.clone()) {
            kits.insert(0, raw);
        }

        for k in kits {
            let f = prefabs::prefab_root(&k);
            if !folders.contains(&f) {
                folders.push(f);
            }
        }

        folders
    }

    /// The prefab library (`prefab_folders`), scanned on first use and again for another tile.
    pub fn prefab_library(&mut self, e: &mut Editor) -> Arc<PrefabLibrary> {
        if let Some(lib) = &self.prefabs {
            return lib.clone();
        }

        let folders = self.prefab_folders(e);
        let lib = Arc::new(PrefabLibrary::scan_folders(&folders));
        self.prefabs = Some(lib.clone());
        if let Ok(mut base) = self.prefab_base.lock() {
            *base = PrefabBase { lib: Some(lib.clone()), ..Default::default() };
        }

        self.prefab_version += 1;
        lib
    }

    /// Scans the prefab folders again.
    pub fn rescan_prefabs(&mut self, e: &mut Editor) -> usize {
        self.prefabs = None;
        self.prefab_library(e).len()
    }

    /// The prefab key of an element when it is a reference to a prefab of the library.
    pub(crate) fn prefab_of(&self, d: &TileDocument, elem: usize) -> Option<String> {
        let key = d.prefab_key(elem)?;
        self.prefabs.as_ref()?.get(&key).map(|p| p.key.clone())
    }

    /// The copy being edited in this tile, if there is one.
    pub(crate) fn edited_copy(&self, d: &TileDocument) -> Option<usize> {
        let (doc, key, _) = self.prefab_edit.as_ref().filter(|(doc, _, _)| *doc == d.id())?;
        let _ = doc;
        layer_by_key(d.tile().tscene(), key)
    }

    /// The copy opened for its items (a click on one of the items of a selected copy; for a reference, what is edited in them is an
    /// override), if there is one in this tile.
    pub(crate) fn opened_copy(&self, d: &TileDocument) -> Option<usize> {
        let (doc, key) = self.prefab_open.as_ref()?;
        if *doc != d.id() {
            return None;
        }

        layer_by_key(d.tile().tscene(), key)
    }

    /// A copy whose contents the Scene tree does not list and whose contents a click does not select: every copy but the edited one and the
    /// opened one (and the copies inside those).
    pub(crate) fn closed_prefab(&self, d: &TileDocument, elem: usize) -> bool {
        if self.prefab_of(d, elem).is_none() {
            return false;
        }

        let open = [self.edited_copy(d), self.opened_copy(d)];
        let els = d.tile().tscene().doc().elements();
        !std::iter::successors(Some(elem), |&x| els.get(x).and_then(|el| el.parent)).any(|x| open.contains(&Some(x)))
    }

    /// The selection with the contents of closed copies replaced by the copies (once per frame, after the clicks). A click on an item of a
    /// copy that was selected (in the viewport), or on an item's row in the Scene tree, opens the copy: its items are selected and edited (a
    /// reference's edits are its overrides) until the selection leaves it.
    pub(crate) fn select_whole_prefabs(&mut self, e: &mut Editor) {
        let Some(id) = document_id(e) else { return };

        // The edited copy may change in structure.
        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            let copy = self.edited_copy(d);
            d.set_unlocked_reference(copy);
        }

        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if self.prefabs.is_none() {
            return;
        }

        let previous = std::mem::take(&mut self.prefab_sel_before);
        let from_tree = std::mem::take(&mut self.tree_picked);
        let sc = d.tile().tscene();
        let els = sc.doc().elements();
        let sel = selected(e);

        // An item of a copy that was selected the frame before: the copy opens.
        let opening = sel.iter().find_map(|&el| {
            let mut copy = None;
            let mut p = els.get(el).and_then(|x| x.parent);
            while let Some(q) = p {
                if self.closed_prefab(d, q) {
                    copy = Some(q);
                }

                p = els[q].parent;
            }

            copy.filter(|c| from_tree || previous.contains(c))
        });

        if let Some(c) = opening && let Some(key) = layer_key(sc, c) {
            self.prefab_open = Some((id, key));
            self.prefab_version += 1;
        }

        // The selection left the opened copy: it closes.
        if let Some(o) = self.opened_copy(d) {
            let inside = sel.iter().any(|&el| std::iter::successors(Some(el), |&x| els.get(x).and_then(|y| y.parent)).any(|x| x == o));
            if !inside {
                self.prefab_open = None;
                self.prefab_version += 1;
            }
        }

        let mut changed = false;
        let mut items = vec![];
        for &el in &sel {
            // An element that is gone (an undone addition) leaves the selection.
            let Some(element) = els.get(el) else {
                changed = true;
                continue;
            };

            // The outermost closed copy above (or at) the element.
            let mut copy = None;
            let mut p = element.parent;
            while let Some(q) = p {
                if self.closed_prefab(d, q) {
                    copy = Some(q);
                }

                p = els[q].parent;
            }

            match copy {
                Some(c) => {
                    changed = true;
                    items.push(c);
                }

                None => items.push(el),
            }
        }

        items.sort_unstable();
        items.dedup();
        if changed {
            e.selection.clear();
            for &el in &items {
                e.selection.add(SourceRef { doc: id, item: item_of(el) });
            }
        }

        self.prefab_sel_before = items;
    }

    /// Places a prefab in the working layer, on the ground at `at` (world metres).
    ///
    /// # Errors
    ///
    /// No tile, no such prefab, or its file can't be read.
    pub fn place_prefab(&mut self, e: &mut Editor, key: &str, at: Float3) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let entry = lib.get(key).ok_or_else(|| format!("no prefab '{key}'"))?.clone();
        let p = lib.load(&entry.key)?;
        let d = document(e).ok_or("no tile open")?;
        let into = self.new_items_layer(d).ok_or("this tile has no scene graph")?;
        d.place_prefab(&entry.key, &p.group_text, into, at)?;
        Ok(format!("placed {}", entry.key))
    }

    /// Where a prefab placed from the panel goes: the terrain under the mouse, else the point the camera looks at.
    pub(crate) fn placement_point(&self, e: &Editor) -> Float3 {
        if self.terrain_hit { self.cursor } else { e.viewport.camera().target }
    }

    /// Opens a copy for editing (the one selected).
    ///
    /// # Errors
    ///
    /// Not a copy of a prefab, or another copy is being edited.
    pub fn edit_prefab(&mut self, e: &mut Editor, copy: usize) -> Result<String, String> {
        let d = document(e).ok_or("no tile open")?;
        let key = self.prefab_of(d, copy).ok_or("not a copy of a prefab")?;
        if self.edited_copy(d).is_some() {
            return Err("another copy is being edited: apply or cancel it first".to_owned());
        }

        let path = layer_key(d.tile().tscene(), copy).ok_or("the copy is not in the scene graph")?;
        self.prefab_edit = Some((d.id(), path, key.clone()));
        self.prefab_version += 1;
        Ok(format!("editing {key}: Apply writes the prefab and updates its copies in this tile"))
    }

    /// Ends the edit: writes the prefab file from the edited copy, and gives every other copy in the tile its contents (one undo entry).
    ///
    /// # Errors
    ///
    /// No edit, or the file can't be written.
    pub fn apply_prefab_edit(&mut self, e: &mut Editor) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let d = document(e).ok_or("no tile open")?;
        let copy = self.edited_copy(d).ok_or("no prefab is being edited")?;
        let key = self.prefab_edit.as_ref().map(|p| p.2.clone()).unwrap_or_default();
        let entry = lib.get(&key).ok_or_else(|| format!("no prefab '{key}'"))?.clone();
        let children = d.children_text(copy);

        // The other references keep their overrides (what they differ by from the prefab before the edit), on top of the new contents; one
        // that differs by more keeps its own contents. TEd's copies get the new contents.
        let old = lib.contents(&key).unwrap_or_default();
        let mut each = vec![];
        let mut kept = 0;
        for c in d.prefab_copies(&key).into_iter().filter(|&c| c != copy) {
            if !d.is_prefab_reference(c) {
                each.push((c, children.clone()));
                continue;
            }

            match wce_tile::prefab_refs::overrides(&d.children_text(c), &old).and_then(|ovs| wce_tile::prefab_refs::apply(&children, &ovs)) {
                Ok((text, _)) => each.push((c, text)),
                Err(_) => kept += 1,
            }
        }

        // The edit goes into the projects' prefab (its .wce), never into the kit.
        let projects = prefabs::prefab_root(&Self::workspace_dir(e).join("raw_data"));
        let saved = lib.save_edit(&entry.key, &projects, &children)?;
        let d = document(e).ok_or("no tile open")?;
        let n = d.set_each_group_children(&each, &format!("Update the copies of {key}"))?;
        if kept != 0 {
            log::warn(format!("{kept} reference(s) to {key} differ from it by more than values: they keep their own contents"));
        }

        self.end_prefab_edit(e);
        self.rescan_prefabs(e);
        Ok(format!("{key} saved to {}; {n} other cop{} updated", saved.display(), if n == 1 { "y" } else { "ies" }))
    }

    /// Ends the edit without saving: the copy gets the prefab file's contents back.
    ///
    /// # Errors
    ///
    /// No edit, or the file can't be read.
    pub fn cancel_prefab_edit(&mut self, e: &mut Editor) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let d = document(e).ok_or("no tile open")?;
        let copy = self.edited_copy(d).ok_or("no prefab is being edited")?;
        let key = self.prefab_edit.as_ref().map(|p| p.2.clone()).unwrap_or_default();
        let p = lib.load(&key)?;
        d.set_groups_children(&[copy], &p.children_text, &format!("Cancel the edit of {key}"))?;
        self.end_prefab_edit(e);
        Ok(format!("{key}: edit cancelled"))
    }

    fn end_prefab_edit(&mut self, e: &mut Editor) {
        // The copy stays selected (as a whole again).
        let copy = document(e).and_then(|d| self.edited_copy(d));
        self.prefab_edit = None;
        self.prefab_version += 1;
        if let (Some(c), Some(id)) = (copy, document_id(e)) {
            e.selection.set(SourceRef { doc: id, item: item_of(c) });
        }
    }

    /// Makes a copy a plain group (renamed `<key> (unpacked)`).
    ///
    /// # Errors
    ///
    /// Not a copy of a prefab.
    pub fn unpack_prefab(&mut self, e: &mut Editor, copy: usize) -> Result<String, String> {
        let d = document(e).ok_or("no tile open")?;
        let key = self.prefab_of(d, copy).ok_or("not a copy of a prefab")?;
        if self.edited_copy(d) == Some(copy) {
            return Err("the copy is being edited: apply or cancel it first".to_owned());
        }

        d.unpack_prefab_copy(copy, &format!("{key} (unpacked)"));
        self.prefab_version += 1;
        Ok(format!("{key} unpacked into a plain group"))
    }

    /// Writes an instance group as a new prefab `<prefab folder>/<folder>/<name>.xml` and names the group after it (so it is a copy of it).
    ///
    /// # Errors
    ///
    /// Not an instance group, no name, no prefab folder, or a prefab of that name already exists.
    pub fn save_as_prefab(&mut self, e: &mut Editor, group: usize, name: &str, folder: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
            return Err("give the prefab a file name".to_owned());
        }

        let lib = self.prefab_library(e);
        if lib.get(name).is_some() {
            return Err(format!("a prefab '{name}' already exists"));
        }

        let d = document(e).ok_or("no tile open")?;
        if d.tile().tscene().type_of(group) != "UserInstanceGroup" {
            return Err("only an instance group can be saved as a prefab".to_owned());
        }

        let root = lib.roots().first().cloned().or_else(|| d.project().raw_root.as_deref().map(prefabs::prefab_root)).ok_or("no kit to put the prefab in")?;
        let folder = folder.trim().trim_matches(['/', '\\']);
        let path = if folder.is_empty() { root.join(format!("{name}.wce")) } else { root.join(folder).join(format!("{name}.wce")) };
        d.rename(group, name);
        prefabs::write(&path, &d.element_text(group), Some(&|k: &str| lib.contents(k)))?;

        // The group is a reference to it now.
        d.make_prefab_reference(group, name);
        self.rescan_prefabs(e);
        Ok(format!("saved {}", path.display()))
    }

    /// "Save for Assembly Kit" of a prefab: its file as TEd reads it in the art/prefabs of `kit_raw` (a raw_data folder) or of the first kit
    /// (the prefabs placed in it baked).
    ///
    /// # Errors
    ///
    /// No such prefab, no kit, or the file can't be written.
    pub fn save_prefab_for_kit(&mut self, e: &mut Editor, key: &str, kit_raw: Option<PathBuf>) -> Result<String, String> {
        let lib = self.prefab_library(e);
        let projects = Self::workspace_dir(e).join("raw_data");
        let kit = match kit_raw {
            Some(k) => k,
            None => self.kit_roots(e).into_iter().find(|r| *r != projects).ok_or("no Assembly Kit (set it in the game's settings)")?,
        };

        let (path, report) = lib.save_for_kit(key, &prefabs::prefab_root(&kit))?;
        for line in report.polyfilled.iter().chain(&report.dropped) {
            log::info(format!("Save prefab for Assembly Kit: {line}"));
        }

        Ok(format!("{key} saved for the Assembly Kit: {} ({})", path.display(), report.summary()))
    }

    /// The "Save as prefab" dialog (opened from a group's menu in the Scene panel).
    pub(crate) fn draw_save_prefab_popup(&mut self, e: &mut Editor) {
        if self.prefab_save.as_ref().is_some_and(|s| s.0.starts_with('\u{1}')) {
            // Opened this frame.
            if let Some(s) = &mut self.prefab_save {
                s.0.remove(0);
            }

            gui::open_popup("Save as prefab");
        }

        if !gui::begin_popup_modal("Save as prefab", None, gui::WINDOW_ALWAYS_AUTO_RESIZE) {
            return;
        }

        let roots = self.prefab_library(e).roots().to_vec();
        let Some((group_key, name, folder)) = self.prefab_save.as_mut() else {
            gui::close_current_popup();
            gui::end_popup();
            return;
        };

        gui::text_wrapped("The group is written as a prefab file and renamed after it, so that it is a copy of it.");
        gui::set_next_item_width(320.0);
        gui::input_text("Name", Some("the prefab's key (file name)"), name, 128, 0);
        gui::set_next_item_width(320.0);
        gui::input_text("Folder", Some("below art/prefabs, e.g. global/rocks"), folder, 256, 0);
        if let Some(r) = roots.first() {
            gui::text_disabled(&format!("in {}", r.display()));
        }

        let (group_key, name, folder) = (group_key.clone(), name.clone(), folder.clone());
        if gui::button("Save") {
            let group = document(e).and_then(|d| layer_by_key(d.tile().tscene(), &group_key));
            self.status = match group {
                Some(g) => self.save_as_prefab(e, g, &name, &folder).unwrap_or_else(|err| err),
                None => "the group is gone".to_owned(),
            };

            self.prefab_save = None;
            gui::close_current_popup();
        }

        gui::same_line();
        if gui::button("Cancel") || gui::is_key_pressed(gui::KEY_ESCAPE, true) {
            self.prefab_save = None;
            gui::close_current_popup();
        }

        gui::end_popup();
    }

    /// Asks for "Save as prefab" on a group (the dialog opens on the next frame).
    pub(crate) fn request_save_prefab(&mut self, d: &TileDocument, group: usize) {
        if let Some(key) = layer_key(d.tile().tscene(), group) {
            let name = d.tile().tscene().name(group);
            self.prefab_save = Some((format!("\u{1}{key}"), name, String::new()));
        }
    }
}

/// The icon and type name a row of the Scene panel shows for a prefab copy.
pub(crate) const PREFAB_ICON: &str = icons::COMPONENT;

/// The colour of an edited copy's row marks.
pub(crate) fn edited_colour() -> [f32; 4] {
    theme::palette().highlight
}

/// What turning copies into references says.
pub(crate) fn recognised_message(n: usize, different: usize) -> String {
    let mut msg = format!("{n} group(s) that are copies of a prefab are references to it now");
    if different != 0 {
        msg += &format!("; {different} group(s) named after a prefab differ from it and stay plain groups");
    }

    msg
}
