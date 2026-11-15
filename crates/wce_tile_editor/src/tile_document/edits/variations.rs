//! A combined project's variations in the scene (docs/plans/tile_variations.md, "Working in the editor"): which variations a layer,
//! group or variation folder belongs to, *Move to a variation folder*, *Split for this variation* and Tidy. Each is one undo entry.

use super::*;
use wce_tile::variations::edit;

impl TileDocument {
    /// The project's variations and the viewed one, of a combined project.
    fn combined(&self) -> Result<(Vec<String>, String), String> {
        let t = self.tile();
        match t.viewed_variation() {
            Some(v) if !t.variations().is_empty() => Ok((t.variations().to_vec(), v.to_owned())),
            _ => Err("the tile has one variation: this is for a project of several (Tile > Variations > Combine)".to_owned()),
        }
    }

    fn check_list(all: &[String], list: &[String]) -> Result<(), String> {
        if list.is_empty() {
            return Err("no variation: something no variation has is deleted instead".to_owned());
        }

        match list.iter().find(|v| !all.contains(v)) {
            Some(bad) => Err(format!("the project has no variation '{bad}' (it has {})", all.join(", "))),
            None => Ok(()),
        }
    }

    /// The selection's elements that are siblings' tops (what is below a selected element goes with it), and their items.
    fn variation_selection(&self, elems: &[usize]) -> Result<(Vec<usize>, Vec<u64>), String> {
        self.check_structure(elems, None)?;
        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        let top = top_level(sc, elems.iter().copied().filter(|&e| e < n).collect());
        if top.is_empty() {
            return Err("nothing selected".to_owned());
        }

        let items = top.iter().map(|&e| item_of(e)).collect();
        Ok((top, items))
    }

    /// Which variations layers, groups and variation folders belong to (`None`: their parent's). An instance has its folder's.
    ///
    /// # Errors
    ///
    /// Not a combined project, a variation it does not have, or nothing in the selection that can carry variations.
    pub fn set_variations(&mut self, elems: &[usize], list: Option<&[String]>) -> Result<(), String> {
        let (all, _) = self.combined()?;
        if let Some(l) = list {
            Self::check_list(&all, l)?;
        }

        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        let todo = elems.iter().copied().filter(|&e| e < n && edit::can_carry(sc, e)).collect::<Vec<_>>();
        if todo.is_empty() {
            return Err("only layers, groups and variation folders belong to variations (an instance has its folder's)".to_owned());
        }

        let items = todo.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        // Taking an attribute away changes no element's index.
        for &e in &todo {
            if let Err(err) = edit::set_variations(self.tscene_mut(), e, list) {
                return self.rolled_back(mark, err);
            }
        }

        let name = list.map_or_else(|| "Variations as the parent's".to_owned(), |l| format!("Variations: {}", l.join(", ")));
        self.push_splice(name, mark, items.clone(), items);
        Ok(())
    }

    /// *Move to a variation folder*: the selection into a new variation folder for `list`, at its place. Returns the folder.
    ///
    /// # Errors
    ///
    /// Not a combined project, a variation it does not have, or a selection that is not side by side in one layer, group or folder.
    pub fn to_variation_folder(&mut self, elems: &[usize], list: &[String]) -> Result<usize, String> {
        let (all, _) = self.combined()?;
        Self::check_list(&all, list)?;
        let (top, sel_before) = self.variation_selection(elems)?;
        let mark = self.begin()?;
        let folder = match edit::wrap_in_folder(self.tscene_mut(), &top, list) {
            Ok(f) => f,
            Err(e) => return self.rolled_back(mark, e),
        };

        let els = self.tile().tscene().doc().elements();
        let moved = els[folder].children.iter().copied().filter(|&c| els[c].name != wce_tile::variations::VALUES).map(item_of).collect();
        self.push_splice(format!("Move to a folder for {}", list.join(", ")), mark, sel_before, moved);
        Ok(folder)
    }

    /// *Split for this variation*: the selection copied, as the viewed variation has it, into a folder for it; the originals into a
    /// folder for their parent's other variations. The selection afterwards is the copies. Returns them.
    ///
    /// # Errors
    ///
    /// Not a combined project, a selection that is not side by side, not in the viewed variation, or only in it already.
    pub fn split_for_variation(&mut self, elems: &[usize], variation: &str) -> Result<Vec<usize>, String> {
        let (all, _) = self.combined()?;
        let v = all
            .iter()
            .find(|x| x.eq_ignore_ascii_case(variation))
            .cloned()
            .ok_or_else(|| format!("the project has no variation '{variation}' (it has {})", all.join(", ")))?;
        let (top, sel_before) = self.variation_selection(elems)?;
        let mark = self.begin()?;
        let copies = match edit::split_for(self.tscene_mut(), &top, &v, &all) {
            Ok(c) => c,
            Err(e) => return self.rolled_back(mark, e),
        };

        self.push_splice(format!("Split for {v}"), mark, sel_before, copies.iter().map(|&c| item_of(c)).collect());
        Ok(copies)
    }

    /// Tidy (`variations::edit::tidy`): no variation's file changes. Returns how many edits it made (none: no undo entry).
    ///
    /// # Errors
    ///
    /// Not a combined project, or an edit failed.
    pub fn tidy_variations(&mut self) -> Result<usize, String> {
        let (all, _) = self.combined()?;
        let mark = self.begin()?;
        let n = match edit::tidy(self.tscene_mut(), &all) {
            Ok(n) => n,
            Err(e) => return self.rolled_back(mark, e),
        };

        if n == 0 {
            let _ = self.tscene_mut().end_edit(mark);
            return Ok(0);
        }

        self.push_splice("Tidy variations".to_owned(), mark, vec![], vec![]);
        Ok(n)
    }
}

impl TileDocument {
    /// The Inspector's variations of a combined project: a checkbox per variation for a layer, a group or a variation folder; what an
    /// instance belongs to (its folder's), read-only.
    pub(super) fn variation_rows(&mut self, e: usize, ins: &mut dyn Inspector) {
        let t = self.tile();
        let all = t.variations().to_vec();
        if all.is_empty() {
            return;
        }

        let sc = t.tscene();
        let set = wce_tile::variations::effective(sc.doc(), e, &all);
        if !edit::can_carry(sc, e) {
            ins.hint("The variations it is in: an instance belongs where its layer, group or variation folder does.");
            ins.text("variations", "variations", &set.join(", "));
            return;
        }

        let own = wce_tile::variations::own_variations(sc.doc(), e).is_some();
        let names = all.iter().map(String::as_str).collect::<Vec<_>>();
        let mut on = all.iter().map(|v| set.contains(v)).collect::<Vec<_>>();
        ins.hint(if own {
            "The variations it belongs to, and everything in it."
        } else {
            "The variations it belongs to, and everything in it: its parent's, until one is changed here."
        });
        if ins.edit_checks("variations", "variations", &names, &mut on) {
            let list = all.iter().zip(&on).filter(|(_, on)| **on).map(|(v, _)| v.clone()).collect::<Vec<_>>();
            if let Err(err) = self.set_variations(&[e], Some(&list)) {
                wce_foundation::log::warn(err);
            }
        }
    }

    /// [`Self::variation_rows`] of several elements: a tick changes that variation only, on each of them that carries variations (one undo
    /// entry); instances show what they are in, read-only. Marked when they differ.
    pub(super) fn common_variation_rows(&mut self, elems: &[usize], ins: &mut dyn Inspector) {
        let t = self.tile();
        let all = t.variations().to_vec();
        if all.is_empty() {
            return;
        }

        let sc = t.tscene();
        let sets = elems.iter().map(|&e| wce_tile::variations::effective(sc.doc(), e, &all)).collect::<Vec<_>>();
        let carriers = elems.iter().copied().zip(&sets).filter(|(e, _)| edit::can_carry(sc, *e)).collect::<Vec<_>>();
        ins.section("Variations");
        if sets[1..].iter().any(|s| *s != sets[0]) {
            ins.mark_fields(vec![("variations".to_owned(), "differs between the selected items: the first one's is shown".to_owned())]);
        }

        if carriers.is_empty() {
            ins.hint("The variations they are in: an instance belongs where its layer, group or variation folder does.");
            ins.text("variations", "variations", &sets[0].join(", "));
            return;
        }

        let names = all.iter().map(String::as_str).collect::<Vec<_>>();
        let before = all.iter().map(|v| sets[0].contains(v)).collect::<Vec<_>>();
        let mut on = before.clone();
        ins.hint("A tick changes only that variation, on each selected layer, group or variation folder; instances keep their folder's.");
        if !ins.edit_checks("variations", "variations", &names, &mut on) {
            return;
        }

        // Each one keeps its own list but for the ticked variation: one undo entry for them all.
        let lists = carriers
            .into_iter()
            .map(|(e, set)| {
                let list = all
                    .iter()
                    .enumerate()
                    .filter(|(i, v)| if on[*i] == before[*i] { set.contains(v) } else { on[*i] })
                    .map(|(_, v)| v.clone())
                    .collect::<Vec<_>>();
                (e, list)
            })
            .collect::<Vec<_>>();
        let mark = match self.begin() {
            Ok(m) => m,
            Err(err) => return wce_foundation::log::warn(err),
        };

        for (e, list) in &lists {
            if let Err(err) = edit::set_variations(self.tscene_mut(), *e, Some(list)) {
                let _ = self.rolled_back::<()>(mark, &err);
                return wce_foundation::log::warn(err);
            }
        }

        let items = lists.iter().map(|(e, _)| item_of(*e)).collect::<Vec<_>>();
        self.push_splice(format!("Variations of {} items", items.len()), mark, items.clone(), items);
    }
}
