//! Layer flags and the scene's structure: new layers and groups, moving, renaming, deleting.

use super::*;

impl TileDocument {
    // ---- layer flags and structure

    /// Layer flags (docs/formats/tile_ted.md, from toggling them in TEd): the eye is OIF_VISIBLE, the cursor arrow OIF_ACTIVE (selectable). A
    /// hidden layer hides everything below it (not drawn, not picked, still exported); an inactive one cannot be picked or moved with the
    /// gizmo. One undo entry; elements without a flags attribute are left alone.
    pub fn set_flags(&mut self, elems: &[usize], flag: &str, on: bool) -> bool {
        let t = self.tile();
        // Only elements with a flags attribute (layers and instances; TEd writes none on the scene graph): own_flag reads a missing attribute
        // as every flag set, which set_flag cannot write, and an added attribute is text the attribute edits of the command cannot take back.
        let todo = elems
            .iter()
            .copied()
            .filter(|&e| e < t.tscene().doc().elements().len() && t.tscene().doc().get(e, "flags").is_some() && t.own_flag(e, flag) != on)
            .collect::<Vec<_>>();
        if todo.is_empty() {
            return false;
        }

        let eye = flag == "OIF_VISIBLE";
        let sc = t.tscene();
        let mut n = if todo.len() == 1 { sc.name(todo[0]) } else { format!("{} items", todo.len()) };
        if n.is_empty() && todo.len() == 1 {
            n = sc.type_of(todo[0]).to_owned();
        }

        let verb = match (eye, on) {
            (true, true) => "Show ",
            (true, false) => "Hide ",
            (false, true) => "Unlock ",
            (false, false) => "Lock ",
        };

        let name = format!("{verb}{n}");

        // The new flags text of every element; the stack applies it.
        let sc = self.tscene_mut();
        let (mut old, mut new) = (vec![], vec![]);
        for &e in &todo {
            let before = sc.doc().get(e, "flags");
            sc.set_flag(e, flag, on);
            new.push(sc.doc().get(e, "flags"));
            sc.set_attr(e, "flags", before.as_deref().unwrap_or(""));
            old.push(before);
        }

        self.history.push(Box::new(TedFlagCommand { name, elems: todo, old, new }));
        true
    }

    pub fn set_flag(&mut self, elem: usize, flag: &str, on: bool) -> bool {
        self.set_flags(&[elem], flag, on)
    }

    /// The moved elements keep their place in the world (their parents changed).
    pub(in crate::tile_document) fn keep_worlds(&mut self, moved: &[usize], worlds: &[WorldTransform]) {
        let sc = self.tscene_mut();
        for (&m, w) in moved.iter().zip(worlds) {
            if sc.position(m).is_some() {
                set_world_position(sc, m, w.position);
            }

            if sc.euler(m).is_some() {
                set_world_rotation(sc, m, &w.rotation);
            }
        }
    }

    /// Into a layer, in front of `before` (`None`: the end). One undo entry; the selection afterwards is the moved items.
    ///
    /// # Errors
    ///
    /// Nothing to move, a target that is not a layer, or a move the text does not allow.
    pub fn move_items(&mut self, elems: &[usize], layer: usize, before: Option<usize>) -> Result<(), String> {
        self.check_structure(elems, Some(layer))?;
        let sc = self.tile().tscene();
        let holder = layer < sc.doc().elements().len() && wce_tile::parents::child_kind(sc.type_of(layer)).is_some();
        if elems.is_empty() || layer >= sc.doc().elements().len() || !(sc.is_layer(layer) || holder) {
            return Err("nothing to move, or the target is not a layer".to_owned());
        }

        let n = sc.doc().elements().len();
        self.check_fits(top_level(sc, elems.iter().copied().filter(|&e| e < n).collect()).into_iter().map(|e| sc.type_of(e)), layer)?;

        if elems.iter().any(|&e| e >= sc.doc().elements().len() || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot move this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let moved = match self.tscene_mut().move_elements(top, layer, before) {
            Ok(moved) => moved,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        if moved.is_empty() {
            let _ = self.tscene_mut().end_edit(mark);
            return Err(String::new());
        }

        self.keep_worlds(&moved, &worlds);
        let sel_after = moved.iter().map(|&m| item_of(m)).collect();
        let name = if moved.len() == 1 { "Move to layer".to_owned() } else { format!("Move {} items to layer", moved.len()) };
        self.push_splice(name, mark, sel_before, sel_after);
        Ok(())
    }

    /// A new layer next to the first item holding all of them; returns it.
    ///
    /// # Errors
    ///
    /// Nothing selected, an element that can't be grouped, or a selection outside a layer.
    pub fn group_items(&mut self, elems: &[usize], name: &str) -> Result<usize, String> {
        self.check_structure(elems, None)?;
        let sc = self.tile().tscene();
        if elems.is_empty() {
            return Err("nothing selected".to_owned());
        }

        if elems.iter().any(|&e| e >= sc.doc().elements().len() || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot group this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        top.iter().try_for_each(|&e| wce_tile::parents::fits(sc.type_of(e), "UserInstanceGroup"))?;
        let els = sc.doc().elements();
        let mut parent = None;
        let mut p = els[top[0]].parent;
        while let Some(q) = p {
            if sc.is_layer(q) {
                parent = Some(q);
                break;
            }

            p = els[q].parent;
        }

        let parent = parent.ok_or("the selection is not inside a layer")?;
        if top.iter().any(|&e| e == parent || sc.is_descendant(parent, e)) {
            return Err("cannot group a layer into a layer inside itself".to_owned());
        }

        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<Vec<usize>, String> {
            let sc = self.tscene_mut();

            // Appended to the parent: the elements after it shift by its size.
            let layer = sc.add_layer(Some(parent), if name.is_empty() { "New layer" } else { name })?;
            let shift = sc.doc().subtree_size(layer);
            let shifted = top.iter().map(|&e| if e >= layer { e + shift } else { e }).collect();
            sc.move_elements(shifted, layer, None)
        })();

        let moved = match result {
            Ok(moved) => moved,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        self.keep_worlds(&moved, &worlds);

        // moved[0] sits in <children> of the new layer.
        let els = self.tile().tscene().doc().elements();
        let new_layer = els[els[moved[0]].parent.expect("a children wrapper")].parent.expect("the new layer");
        self.push_splice("Group into layer".to_owned(), mark, sel_before, vec![item_of(new_layer)]);
        Ok(new_layer)
    }

    /// Groups items into a new instance group (TEd's `UserInstanceGroup`) in the layer `into` (the working layer, or the scene graph), at the
    /// centre of the items, which keep their place in the world. One undo entry; the selection afterwards is the group.
    ///
    /// # Errors
    ///
    /// Nothing to group, the scene graph, or `into` inside one of the items.
    pub fn group_instances(&mut self, elems: &[usize], into: usize, name: &str) -> Result<usize, String> {
        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        if elems.is_empty() || into >= n || !sc.is_layer(into) {
            return Err("nothing to group, or no layer to put the group in".to_owned());
        }

        if elems.iter().any(|&e| e >= n || sc.type_of(e) == "SCENE_GRAPH") {
            return Err("cannot group this element".to_owned());
        }

        let top = top_level(sc, elems.to_vec());
        if top.iter().any(|&e| e == into || sc.is_descendant(into, e)) {
            return Err("the working layer is inside the selection".to_owned());
        }

        let worlds = top.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let k = worlds.len() as f64;
        let centre = worlds.iter().fold(Double3::default(), |a, w| a + w.position) * (1.0 / k);
        let pw = world_transform(sc, into);
        let local = pw.rotation.transpose() * (centre - pw.position);
        let sel_before = top.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<(usize, Vec<usize>), String> {
            let sc = self.tscene_mut();

            // Appended to `into`: the elements after it shift by its size.
            let group = sc.add_group(Some(into), if name.is_empty() { "Instance Group" } else { name }, local)?;
            let shift = sc.doc().subtree_size(group);
            let shifted = top.iter().map(|&e| if e >= group { e + shift } else { e }).collect();
            let moved = sc.move_elements(shifted, group, None)?;
            Ok((group, moved))
        })();

        let (group, moved) = match result {
            Ok(r) => r,
            Err(e) => {
                let record = self.tscene_mut().end_edit(mark).unwrap_or_default();
                let _ = self.tscene_mut().unapply(&record);
                return Err(e);
            }
        };

        self.keep_worlds(&moved, &worlds);

        // The group sits before its moved contents when it came after them.
        let els = self.tile().tscene().doc().elements();
        let group = els[els[moved[0]].parent.expect("a children wrapper")].parent.unwrap_or(group);
        self.push_splice("Group".to_owned(), mark, sel_before, vec![item_of(group)]);
        Ok(group)
    }

    /// Takes instance groups apart: their contents go into the layer `into` (the working layer, or the scene graph), keeping their place in
    /// the world, and the groups are deleted. Only instance groups (`UserInstanceGroup`, `INSTANCE_GROUP`) among `elems` are taken apart. One
    /// undo entry; the selection afterwards is what they held.
    ///
    /// # Errors
    ///
    /// No group among the elements, or `into` inside one of them.
    pub fn ungroup(&mut self, elems: &[usize], into: usize) -> Result<Vec<usize>, String> {
        self.check_structure(elems, Some(into))?;
        let sc = self.tile().tscene();
        let n = sc.doc().elements().len();
        let groups = top_level(
            sc,
            elems.iter().copied().filter(|&e| e < n && matches!(sc.type_of(e), "UserInstanceGroup" | "INSTANCE_GROUP")).collect(),
        );
        if groups.is_empty() || into >= n || !sc.is_layer(into) {
            return Err("no instance group selected".to_owned());
        }

        if groups.iter().any(|&g| g == into || sc.is_descendant(into, g)) {
            return Err("the working layer is inside a selected group".to_owned());
        }

        let sel_before = groups.iter().map(|&e| item_of(e)).collect::<Vec<_>>();
        let group_worlds = groups.iter().map(|&e| world_transform(sc, e)).collect::<Vec<_>>();
        let mark = self.begin()?;
        let result = (|| -> Result<Vec<usize>, String> {
            // 1. The groups move into `into` together (they keep their place): they are its last children now.
            let moved_groups = self.tscene_mut().move_elements(groups.clone(), into, None)?;
            self.keep_worlds(&moved_groups, &group_worlds);

            // 2. From the last group to the first: its contents go to the end of `into` and the empty group goes. Every change is after the
            // groups still to do, so their indices hold.
            let mut freed: Vec<usize> = vec![];
            for &g in moved_groups.iter().rev() {
                let sc = self.tile().tscene();
                let kids = sc.children(g);
                let worlds = kids.iter().map(|&k| world_transform(sc, k)).collect::<Vec<_>>();
                if !kids.is_empty() {
                    // The contents freed before sit after this group: cutting its contents moves them down by their size.
                    let cut = kids.iter().map(|&k| sc.doc().subtree_size(k)).sum::<usize>();
                    let moved = self.tscene_mut().move_elements(kids, into, None)?;
                    self.keep_worlds(&moved, &worlds);
                    for f in &mut freed {
                        *f -= cut;
                    }

                    freed.extend(moved);
                }

                // Removing the (now empty) group moves what comes after it down by its size.
                let size = self.tile().tscene().doc().subtree_size(g);
                self.tscene_mut().remove(g)?;
                for f in &mut freed {
                    if *f > g {
                        *f -= size;
                    }
                }
            }

            Ok(freed)
        })();

        match result {
            Ok(freed) => {
                self.push_splice(
                    if groups.len() == 1 { "Ungroup".to_owned() } else { format!("Ungroup {} groups", groups.len()) },
                    mark,
                    sel_before,
                    freed.iter().map(|&m| item_of(m)).collect(),
                );
                Ok(freed)
            }

            Err(e) => self.rolled_back(mark, e),
        }
    }
}
