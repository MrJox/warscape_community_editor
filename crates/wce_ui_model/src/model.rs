//! The editable model: entries with rectangles, fields, and the operations that change the tree.

use std::rc::Rc;

use crate::dom::{Dom, Element, Form, Node};
use wce_ui_format::is_array_tag;

/// Where a moved component goes relative to the one it is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Before,
    After,
    Inside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Entry,
    Template,
}

/// A component of the layout (`uientry`) or a template holder (`template`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Indices into `children`, from the document root.
    pub path: Vec<usize>,
    pub depth: usize,
    pub parent: Option<usize>,
    pub kind: EntryKind,
    pub id: u32,
    pub name: String,
    /// `title2`: the component class.
    pub class: String,
    /// Offset in the parent.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Offset from the layout origin.
    pub ax: i32,
    pub ay: i32,
    /// The component's own visibility flag (the seventh): the layout shows it when it loads.
    pub visible: bool,
}

/// A primitive of an element, with the name the converter gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub idx: usize,
    pub tag: String,
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct UiModel {
    /// Shared, so an undo snapshot costs a pointer; edits copy the tree once (`dom_mut`).
    pub dom: Rc<Dom>,
    pub version: u32,
}

fn int_of(e: &Element) -> Option<i64> {
    e.text().trim().parse().ok()
}

/// The direct leaves called `name`, as (index in children, element).
pub(crate) fn leaves<'a>(e: &'a Element, name: &str) -> Vec<(usize, &'a Element)> {
    e.children.iter().enumerate().filter_map(|(i, n)| if let Node::Elem(c) = n { (c.name == name).then_some((i, c)) } else { None }).collect()
}

pub(crate) fn nth_int(e: &Element, name: &str, n: usize) -> Option<i64> {
    leaves(e, name).get(n).and_then(|(_, c)| int_of(c))
}

fn nth_idx(e: &Element, name: &str, n: usize) -> Option<usize> {
    leaves(e, name).get(n).map(|(i, _)| *i)
}

/// The index of the first direct leaf whose label starts with `prefix`.
pub fn by_label(e: &Element, prefix: &str) -> Option<usize> {
    (0..e.children.len()).find(|&i| matches!(e.children[i], Node::Elem(_)) && e.label_of(i).is_some_and(|l| l.starts_with(prefix)))
}

/// Colour of a `<color>` element (bytes B, G, R, A) as RGBA.
pub fn color_of(e: &Element) -> [u8; 4] {
    let b: Vec<u8> = e.elems().filter_map(|c| c.text().trim().parse().ok()).collect();
    if b.len() == 4 { [b[2], b[1], b[0], b[3]] } else { [255, 255, 255, 255] }
}

/// Sets an integer leaf and rewrites the hex note after it (`<!-- 02:00:00:00 -->`) the way the converter prints it, so the text
/// stays what converting the binary would give.
fn set_int(parent: &mut Element, idx: usize, n: i64) {
    if let Some(c) = parent.elem_at_mut(idx) {
        c.set_text(&n.to_string());
    }

    let b = (n as u32).to_le_bytes();
    let hex = format!("{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3]);
    for node in parent.children[idx + 1..].iter_mut() {
        match node {
            Node::Comment { text, inline: true } => {
                let t = text.trim().to_string();
                if t.len() == 11 && t.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
                    *text = text.replace(&t, &hex);
                    break;
                }
            }
            _ => break,
        }
    }
}

pub(crate) fn set_color_pub(e: &mut Element, rgba: [u8; 4]) {
    set_color(e, rgba)
}

fn set_color(e: &mut Element, rgba: [u8; 4]) {
    let bgra = [rgba[2], rgba[1], rgba[0], rgba[3]];
    let mut k = 0;
    for n in e.children.iter_mut() {
        if let Node::Elem(c) = n {
            if k < 4 {
                c.set_text(&bgra[k].to_string());
                k += 1;
            }
        }
    }
}

impl UiModel {
    pub fn dom_mut(&mut self) -> &mut Dom {
        Rc::make_mut(&mut self.dom)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let x = wce_ui_format::ui_to_xml(data).map_err(|e| e.to_string())?;
        let dom = Dom::parse(&x.xml)?;
        Ok(Self { dom: Rc::new(dom), version: x.version })
    }

    pub fn from_xml(xml: &str) -> Result<Self, String> {
        let dom = Dom::parse(xml)?;
        let version = dom.root.attr("version").and_then(|v| v.trim().parse().ok()).ok_or("the root element needs a version attribute")?;
        Ok(Self { dom: Rc::new(dom), version })
    }

    pub fn to_xml(&self) -> String {
        self.dom.to_xml()
    }

    /// The binary file. Array counts are brought in line with the items first.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>, String> {
        fix_counts(&mut self.dom_mut().root);
        wce_ui_format::xml_to_ui(&self.dom.to_xml()).map_err(|e| e.to_string())
    }

    /// True for the files that have components (not `.cml`, `.fc` and the image lists).
    pub fn is_layout(&self) -> bool {
        self.dom.root.name == "ui"
    }

    /// Every component in paint order (parents first).
    pub fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        if !self.is_layout() {
            return out;
        }
        if let Some(i) = self.dom.root.find_idx("uientry") {
            self.walk(self.dom.root.elem_at(i).unwrap(), vec![i], 0, None, 0, 0, &mut out);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(&self, el: &Element, path: Vec<usize>, depth: usize, parent: Option<usize>, ox: i32, oy: i32, out: &mut Vec<Entry>) {
        let v = self.version;
        let id = leaves(el, "u").first().and_then(|(_, c)| int_of(c)).unwrap_or(0) as u32;
        let ss = leaves(el, "s");
        let name = ss.first().map(|(_, c)| c.text()).unwrap_or_default();
        let class = if v >= 43 { ss.get(1).map(|(_, c)| c.text()).unwrap_or_default() } else { String::new() };
        let x = nth_int(el, "i", 0).unwrap_or(0) as i32;
        let y = nth_int(el, "i", 1).unwrap_or(0) as i32;
        let (w, h) = self.default_state(el).map(|s| (nth_int(s, "i", 0).unwrap_or(0) as i32, nth_int(s, "i", 1).unwrap_or(0) as i32)).unwrap_or((0, 0));
        let (ax, ay) = (ox + x, oy + y);
        let me = out.len();
        let visible = self.own_visible(el);
        out.push(Entry { path: path.clone(), depth, parent, kind: EntryKind::Entry, id, name, class, x, y, w, h, ax, ay, visible });
        self.walk_children(el, &path, depth, me, ax, ay, out);
    }

    fn walk_children(&self, el: &Element, path: &[usize], depth: usize, me: usize, ax: i32, ay: i32, out: &mut Vec<Entry>) {
        let Some(ci) = el.find_idx("children") else { return };
        let ch = el.elem_at(ci).unwrap();
        for (i, n) in ch.children.iter().enumerate() {
            let Node::Elem(c) = n else { continue };
            let mut p = path.to_vec();
            p.push(ci);
            p.push(i);
            match c.name.as_str() {
                "uientry" => self.walk(c, p, depth + 1, Some(me), ax, ay, out),
                "template" => {
                    let name = leaves(c, "s").first().map(|(_, e)| e.text()).unwrap_or_default();
                    let t = out.len();
                    out.push(Entry { path: p.clone(), depth: depth + 1, parent: Some(me), kind: EntryKind::Template, id: 0, name, class: "template".into(), x: 0, y: 0, w: 0, h: 0, ax, ay, visible: true });
                    if let Some(tc) = c.find_idx("children") {
                        let tch = c.elem_at(tc).unwrap();
                        for (j, n2) in tch.children.iter().enumerate() {
                            if let Node::Elem(u) = n2 {
                                if u.name == "uientry" {
                                    let mut p2 = p.clone();
                                    p2.push(tc);
                                    p2.push(j);
                                    self.walk(u, p2, depth + 2, Some(t), ax, ay, out);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The seventh of the twelve flags after the offsets says if the component is visible when the layout loads.
    fn own_visible(&self, el: &Element) -> bool {
        if self.version < 55 {
            return true;
        }

        let flags: Vec<&Element> = el.elems().filter(|c| matches!(c.name.as_str(), "yes" | "no" | "byte")).take(12).collect();
        match flags.get(6) {
            Some(f) => f.name != "no",
            None => true,
        }
    }

    /// The state that is shown by default: the one the entry names, else the first.
    pub fn default_state<'a>(&self, el: &'a Element) -> Option<&'a Element> {
        let states = el.find("states")?;
        let all: Vec<&Element> = states.elems().filter(|e| e.name == "state").collect();
        if self.version >= 55 {
            let ds_idx = if self.version >= 77 { 5 } else { 3 };
            if let Some(ds) = nth_int(el, "i", ds_idx) {
                if let Some(s) = all.iter().find(|s| leaves(s, "u").first().and_then(|(_, c)| int_of(c)).map(|u| u as u32) == Some(ds as u32)) {
                    return Some(s);
                }
            }
        }
        all.first().copied()
    }

    /// Path to the default state of the entry at `path`.
    pub fn default_state_path(&self, path: &[usize]) -> Option<Vec<usize>> {
        let el = self.dom.at(path)?;
        let st = self.default_state(el)?;
        let si = el.find_idx("states")?;
        let states = el.elem_at(si)?;
        let k = states.children.iter().position(|n| matches!(n, Node::Elem(e) if std::ptr::eq(e, st)))?;
        let mut p = path.to_vec();
        p.push(si);
        p.push(k);
        Some(p)
    }

    // ---- fields ----

    pub fn fields(&self, path: &[usize]) -> Vec<Field> {
        let Some(el) = self.dom.at(path) else { return vec![] };
        el.children
            .iter()
            .enumerate()
            .filter_map(|(i, n)| match n {
                Node::Elem(c) if c.is_leaf() && matches!(c.name.as_str(), "u" | "i" | "u2" | "i2" | "byte" | "flt" | "s" | "unicode" | "uuid" | "yes" | "no" | "key" | "value") => {
                    let value = match c.name.as_str() {
                        "yes" => "yes".to_string(),
                        "no" => "no".to_string(),
                        _ => c.text(),
                    };
                    Some(Field { idx: i, tag: c.name.clone(), value, label: el.label_of(i).unwrap_or("").to_string() })
                }
                _ => None,
            })
            .collect()
    }

    pub fn set_field(&mut self, path: &[usize], idx: usize, value: &str) -> Result<(), String> {
        let el = self.dom_mut().at_mut(path).ok_or("no such element")?;
        let mut note: Option<i64> = None;
        let c = el.elem_at_mut(idx).ok_or("no such field")?;
        match c.name.as_str() {
            "u" | "i" | "u2" | "i2" | "byte" => {
                let t = value.trim();
                let n: i64 = t.parse().map_err(|_| format!("{:?} is not an integer", t))?;
                let (lo, hi) = match c.name.as_str() {
                    "u" => (0, u32::MAX as i64),
                    "i" => (i32::MIN as i64, u32::MAX as i64),
                    "u2" | "i2" => (0, 65535),
                    _ => (0, 255),
                };
                if n < lo || n > hi {
                    return Err(format!("{} is outside {}..{}", n, lo, hi));
                }
                c.set_text(&n.to_string());
                if c.name == "i" {
                    note = Some(n);
                }
            }
            "flt" => {
                let t = value.trim();
                let f: f64 = t.parse().map_err(|_| format!("{:?} is not a number", t))?;
                if !f.is_finite() {
                    return Err("not finite".into());
                }
                c.set_text(t);
            }
            "yes" | "no" => {
                let on = match value.trim().to_ascii_lowercase().as_str() {
                    "yes" | "true" | "1" | "on" => true,
                    "no" | "false" | "0" | "off" => false,
                    other => return Err(format!("{:?} is not yes or no", other)),
                };
                c.name = if on { "yes".into() } else { "no".into() };
            }
            "uuid" => {
                let h: String = value.trim().chars().filter(|c| *c != '-').collect();
                if h.len() != 32 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("a uuid is 32 hex digits".into());
                }
                c.set_text(value.trim());
            }
            _ => c.set_text(value),
        }

        if let Some(n) = note {
            set_int(el, idx, n);
        }

        Ok(())
    }

    // ---- typed edits ----

    /// Moves the entry to `x, y` in its parent. A docked entry keeps its dock point: the dock offset takes the difference, as the game
    /// puts a docked component at its dock point plus the dock offset and ignores the stored offset.
    pub fn set_offset(&mut self, path: &[usize], x: i32, y: i32) -> Result<(), String> {
        let docked_at = self.dock_target(path, x, y);
        let v = self.version;
        let el = self.dom_mut().at_mut(path).ok_or("no such entry")?;
        let xi = nth_idx(el, "i", 0).ok_or("entry has no x offset")?;
        let yi = nth_idx(el, "i", 1).ok_or("entry has no y offset")?;
        el.elem_at_mut(xi).unwrap().set_text(&x.to_string());
        el.elem_at_mut(yi).unwrap().set_text(&y.to_string());
        if let (Some((ox, oy)), true) = (docked_at, v >= 77) {
            if let (Some(a), Some(b)) = (nth_idx(el, "i", 3), nth_idx(el, "i", 4)) {
                set_int(el, a, ox as i64);
                set_int(el, b, oy as i64);
            }
        }

        Ok(())
    }

    /// The dock offset that puts the entry (with its current size) at `x, y` in its parent, when it is docked.
    fn dock_target(&self, path: &[usize], x: i32, y: i32) -> Option<(i32, i32)> {
        let el = self.dom.at(path)?;
        let (dock, _) = crate::layout::dock_of(self, el);
        let (own, parent) = self.sizes_for_dock(path)?;
        let (fx, fy) = crate::layout::dock_position(dock, parent, own)?;
        Some((x - fx as i32, y - fy as i32))
    }

    /// (own size, parent size) of the entry at `path`, from the default states.
    fn sizes_for_dock(&self, path: &[usize]) -> Option<((f32, f32), (f32, f32))> {
        let ents = self.entries();
        let me = ents.iter().position(|e| e.path == path)?;
        let own = (ents[me].w as f32, ents[me].h as f32);
        let parent = ents[me].parent.map(|p| (ents[p].w as f32, ents[p].h as f32)).unwrap_or(own);
        Some((own, parent))
    }

    /// Sets the size of the default state. A docked entry keeps its dock offset, so its place follows from its new size.
    pub fn set_size(&mut self, path: &[usize], w: i32, h: i32) -> Result<(), String> {
        let sp = self.default_state_path(path).ok_or("entry has no state")?;
        let st = self.dom_mut().at_mut(&sp).unwrap();
        let wi = nth_idx(st, "i", 0).ok_or("state has no size")?;
        let hi = nth_idx(st, "i", 1).ok_or("state has no size")?;
        st.elem_at_mut(wi).unwrap().set_text(&w.to_string());
        st.elem_at_mut(hi).unwrap().set_text(&h.to_string());
        let el = self.dom.at(path).ok_or("no such entry")?;
        let (dock, doff) = crate::layout::dock_of(self, el);
        if dock > 0 {
            if let Some((own, parent)) = self.sizes_for_dock(path) {
                if let Some((fx, fy)) = crate::layout::dock_position(dock, parent, own) {
                    let (x, y) = (fx as i32 + doff.0, fy as i32 + doff.1);
                    let el = self.dom_mut().at_mut(path).ok_or("no such entry")?;
                    if let (Some(xi), Some(yi)) = (nth_idx(el, "i", 0), nth_idx(el, "i", 1)) {
                        el.elem_at_mut(xi).unwrap().set_text(&x.to_string());
                        el.elem_at_mut(yi).unwrap().set_text(&y.to_string());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn set_name(&mut self, path: &[usize], name: &str) -> Result<(), String> {
        let el = self.dom_mut().at_mut(path).ok_or("no such entry")?;
        let i = nth_idx(el, "s", 0).ok_or("entry has no name")?;
        el.elem_at_mut(i).unwrap().set_text(name);
        Ok(())
    }

    /// The text of the default state.
    pub fn set_text(&mut self, path: &[usize], text: &str) -> Result<(), String> {
        let sp = self.default_state_path(path).ok_or("entry has no state")?;
        let st = self.dom_mut().at_mut(&sp).unwrap();
        let i = nth_idx(st, "unicode", 0).ok_or("state has no text")?;
        st.elem_at_mut(i).unwrap().set_text(text);
        Ok(())
    }

    /// The text of the default state.
    pub fn state_text(&self, path: &[usize]) -> String {
        let Some(sp) = self.default_state_path(path) else { return String::new() };
        self.dom.at(&sp).and_then(|st| st.find("unicode")).map(|c| c.text()).unwrap_or_default()
    }

    /// The colour of the default state (its text colour), RGBA.
    pub fn state_color(&self, path: &[usize]) -> [u8; 4] {
        let Some(sp) = self.default_state_path(path) else { return [255; 4] };
        self.dom.at(&sp).and_then(|st| st.find("color")).map(color_of).unwrap_or([255; 4])
    }

    pub fn set_state_color(&mut self, path: &[usize], rgba: [u8; 4]) -> Result<(), String> {
        let sp = self.default_state_path(path).ok_or("entry has no state")?;
        let st = self.dom_mut().at_mut(&sp).unwrap();
        set_color(st.find_mut("color").ok_or("state has no colour")?, rgba);
        Ok(())
    }

    // ---- structure ----

    fn parent_array_mut(&mut self, path: &[usize]) -> Result<(&mut Element, usize), String> {
        let (last, up) = path.split_last().ok_or("the root cannot be moved")?;
        let arr = self.dom_mut().at_mut(up).ok_or("no such parent")?;
        if arr.name != "children" {
            return Err("only components in a children list can be moved".into());
        }
        Ok((arr, *last))
    }

    pub fn delete(&mut self, path: &[usize]) -> Result<(), String> {
        let (arr, i) = self.parent_array_mut(path)?;
        arr.children.remove(i);
        if arr.elems().next().is_none() {
            arr.children.clear();
            arr.form = Form::EmptyBlock;
        }
        fix_counts(&mut self.dom_mut().root);
        Ok(())
    }

    /// A copy right after the original, with new ids. Returns its path.
    pub fn duplicate(&mut self, path: &[usize]) -> Result<Vec<usize>, String> {
        let mut copy = self.dom.at(path).ok_or("no such entry")?.clone();
        let mut used = Vec::new();
        collect_ids(&self.dom.root, &mut used);
        renew_ids(&mut copy, &mut used);
        let (arr, i) = self.parent_array_mut(path)?;
        arr.children.insert(i + 1, Node::Elem(copy));
        fix_counts(&mut self.dom_mut().root);
        let mut p = path.to_vec();
        *p.last_mut().unwrap() = i + 1;
        Ok(p)
    }

    /// Moves the entry among its siblings by `delta` places in the paint order. Returns its new path.
    pub fn move_sibling(&mut self, path: &[usize], delta: i32) -> Result<Vec<usize>, String> {
        let (arr, i) = self.parent_array_mut(path)?;
        let elems: Vec<usize> = (0..arr.children.len()).filter(|&k| matches!(arr.children[k], Node::Elem(_))).collect();
        let pos = elems.iter().position(|&k| k == i).ok_or("not an element")?;
        let np = (pos as i32 + delta).clamp(0, elems.len() as i32 - 1) as usize;
        if np == pos {
            return Ok(path.to_vec());
        }
        let node = arr.children.remove(i);
        let target = elems[np];
        arr.children.insert(target, node);
        let mut p = path.to_vec();
        *p.last_mut().unwrap() = target;
        Ok(p)
    }

    /// Moves an entry into another entry's children (at the end, or at `index` among them), keeping its place on screen if asked.
    pub fn reparent(&mut self, path: &[usize], new_parent: &[usize], index: Option<usize>, keep_screen_position: bool) -> Result<Vec<usize>, String> {
        if new_parent.starts_with(path) {
            return Err("an entry cannot move into itself".into());
        }
        let ents = self.entries();
        let src = ents.iter().find(|e| e.path == path).cloned().ok_or("no such entry")?;
        let dst = ents.iter().find(|e| e.path == new_parent).cloned().ok_or("no such parent")?;
        if dst.kind == EntryKind::Template {
            return Err("pick a component as the parent, not a template".into());
        }
        let mut node = self.dom.at(path).ok_or("no such entry")?.clone();
        self.delete(path)?;
        // Paths after the removal: the new parent may have shifted if it shares the old array.
        let mut np = new_parent.to_vec();
        if np.len() >= path.len() && np[..path.len() - 1] == path[..path.len() - 1] && np[path.len() - 1] > path[path.len() - 1] {
            np[path.len() - 1] -= 1;
        }
        if keep_screen_position {
            let x = src.ax - dst.ax;
            let y = src.ay - dst.ay;
            if let (Some(xi), Some(yi)) = (nth_idx(&node, "i", 0), nth_idx(&node, "i", 1)) {
                node.elem_at_mut(xi).unwrap().set_text(&x.to_string());
                node.elem_at_mut(yi).unwrap().set_text(&y.to_string());
            }
        }
        let parent = self.dom_mut().at_mut(&np).ok_or("no such parent")?;
        let ci = match parent.find_idx("children") {
            Some(c) => c,
            None => return Err("the parent has no children list".into()),
        };
        let arr = parent.elem_at_mut(ci).unwrap();
        arr.children.retain(|n| !matches!(n, Node::Text(_)));
        arr.form = Form::Normal;
        let elems: Vec<usize> = (0..arr.children.len()).filter(|&k| matches!(arr.children[k], Node::Elem(_))).collect();
        let at = match index {
            Some(k) if k < elems.len() => elems[k],
            _ => arr.children.len(),
        };
        arr.children.insert(at, Node::Elem(node));
        fix_counts(&mut self.dom_mut().root);
        let mut p = np;
        p.push(ci);
        p.push(at);
        Ok(p)
    }

    /// Moves a component next to another one (before or after it, as its sibling) or into it (as its last child), with everything below
    /// it. The component keeps its place on screen. Returns its new path.
    pub fn move_to(&mut self, path: &[usize], target: &[usize], place: Place, keep_screen_position: bool) -> Result<Vec<usize>, String> {
        if target.starts_with(path) {
            return Err("a component cannot move into itself".into());
        }

        if place == Place::Inside {
            return self.reparent(path, target, None, keep_screen_position);
        }

        let ents = self.entries();
        let s = ents.iter().position(|e| e.path == path).ok_or("no such component")?;
        let t = ents.iter().position(|e| e.path == target).ok_or("no such target")?;
        let parent = ents[t].parent.ok_or("the root has no siblings: drop onto it instead")?;
        let parent_path = ents[parent].path.clone();
        // the place among the target's siblings once the moved component is out of the list
        let sibs: Vec<usize> = (0..ents.len()).filter(|&k| ents[k].parent == Some(parent) && k != s).collect();
        let at = sibs.iter().position(|&k| k == t).ok_or("the target is not in its parent")?;
        self.reparent(path, &parent_path, Some(if place == Place::After { at + 1 } else { at }), keep_screen_position)
    }

    /// Adds a component to `parent`: a copy of `like` (default: the parent's first child, else the parent itself) without its own children.
    pub fn add_child(&mut self, parent: &[usize], like: Option<&[usize]>, name: &str) -> Result<Vec<usize>, String> {
        let par = self.dom.at(parent).ok_or("no such parent")?;
        let proto_path: Vec<usize> = match like {
            Some(l) => l.to_vec(),
            None => {
                let mut p = parent.to_vec();
                match par.find_idx("children").and_then(|ci| par.elem_at(ci).and_then(|c| c.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == "uientry")).map(|k| (ci, k)))) {
                    Some((ci, k)) => {
                        p.push(ci);
                        p.push(k);
                    }
                    None => {}
                }
                p
            }
        };
        let mut node = match self.dom.at(&proto_path) {
            Some(n) if n.name == "uientry" => n.clone(),
            // a parent without children: copy the first plain component found anywhere in the layout
            _ => first_uientry(&self.dom.root).ok_or("the layout has no component to copy")?.clone(),
        };
        if node.name != "uientry" {
            return Err("the copied item is not a component".into());
        }
        if let Some(ci) = node.find_idx("children") {
            let c = node.elem_at_mut(ci).unwrap();
            c.children.clear();
            c.form = Form::EmptyBlock;
            c.set_attr("count", "0");
        }
        if let Some(i) = nth_idx(&node, "s", 0) {
            node.elem_at_mut(i).unwrap().set_text(name);
        }
        let mut used = Vec::new();
        collect_ids(&self.dom.root, &mut used);
        renew_ids(&mut node, &mut used);
        let parent_el = self.dom_mut().at_mut(parent).ok_or("no such parent")?;
        let ci = parent_el.find_idx("children").ok_or("the parent has no children list")?;
        let arr = parent_el.elem_at_mut(ci).unwrap();
        arr.push_elem(node);
        let at = arr.children.len() - 1;
        fix_counts(&mut self.dom_mut().root);
        let mut p = parent.to_vec();
        p.push(ci);
        p.push(at);
        Ok(p)
    }
}

/// Sets every `count` attribute to the number of items below it.
pub fn fix_counts(e: &mut Element) {
    if is_array_tag(&e.name) && e.attr("count").is_some() {
        let n = e.elems().count();
        e.set_attr("count", &n.to_string());
    }
    for c in e.children.iter_mut() {
        if let Node::Elem(c) = c {
            fix_counts(c);
        }
    }
}

fn id_comment(e: &Element, i: usize) -> bool {
    matches!(e.children.get(i + 1), Some(Node::Comment { text, inline: true }) if text.trim_start().starts_with("ID ("))
}

/// The first component of the layout that has no components below it: the plainest thing to copy.
fn first_uientry(e: &Element) -> Option<&Element> {
    for n in &e.children {
        if let Node::Elem(c) = n {
            if c.name == "uientry" {
                let has_kids = c.find("children").is_some_and(|ch| ch.elems().any(|k| k.name == "uientry"));
                if !has_kids {
                    return Some(c);
                }
            }

            if let Some(f) = first_uientry(c) {
                return Some(f);
            }
        }
    }

    None
}

fn collect_ids(e: &Element, out: &mut Vec<u32>) {
    for (i, n) in e.children.iter().enumerate() {
        if let Node::Elem(c) = n {
            if c.name == "u" && id_comment(e, i) {
                if let Ok(v) = c.text().trim().parse::<u32>() {
                    out.push(v);
                }
            }
            collect_ids(c, out);
        }
    }
}

fn renew_ids(root: &mut Element, used: &mut Vec<u32>) {
    let mut old = Vec::new();
    collect_ids(root, &mut old);
    let mut seed = used.iter().fold(0x9e37_79b9_u32, |a, b| a.rotate_left(5) ^ *b) | 1;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut map: Vec<(u32, u32)> = Vec::new();
    for o in old {
        if map.iter().any(|(a, _)| *a == o) {
            continue;
        }
        let mut n = next();
        while n < 65536 || used.contains(&n) {
            n = next();
        }
        used.push(n);
        map.push((o, n));
    }
    apply_ids(root, &map);
}

fn apply_ids(e: &mut Element, map: &[(u32, u32)]) {
    let is_i = e.name == "i";
    if is_i {
        if let Ok(v) = e.text().trim().parse::<i64>() {
            if let Some((_, n)) = map.iter().find(|(o, _)| *o == v as u32 && v.unsigned_abs() > 65536) {
                e.set_text(&(*n as i32).to_string());
            }
        }
    }
    let mut renew: Vec<(usize, u32)> = Vec::new();
    for i in 0..e.children.len() {
        if let Node::Elem(c) = &e.children[i] {
            if c.name == "u" && id_comment(e, i) {
                if let Ok(v) = c.text().trim().parse::<u32>() {
                    if let Some((_, n)) = map.iter().find(|(o, _)| *o == v) {
                        renew.push((i, *n));
                    }
                }
            }
        }
    }
    for (i, n) in renew {
        if let Node::Elem(c) = &mut e.children[i] {
            c.set_text(&n.to_string());
        }
        if let Some(Node::Comment { text, .. }) = e.children.get_mut(i + 1) {
            let b = n.to_le_bytes();
            *text = format!(" ID ({:02x} {:02x} {:02x} {:02x}) ", b[0], b[1], b[2], b[3]);
        }
    }
    for c in e.children.iter_mut() {
        if let Node::Elem(c) = c {
            apply_ids(c, map);
        }
    }
}
