//! The tile.ted scene graph on top of the lossless `XmlDoc`.
//!
//! The file is the source of truth of a TDD tile: layers (LayerInstance, UserInstanceGroup, INSTANCE_GROUP) holding instances
//! (BUILDING_INSTANCE, VEGETATION_INSTANCE, ProjectedDecalInstance, VFXInstance, POINT_LIGHT_INSTANCE, polygons, AI hints, siege AI,
//! deployment, capture locations, rivers ...). Every instance carries the base attributes flags, lf_offset, name, position, orientation,
//! surface_mode; the rest is type specific (docs/formats/tile_ted.md).
//!
//! Edits are attribute edits on the XmlDoc, so everything outside the edited values stays byte-identical, and an edit that does not change a
//! value (as float32) leaves the text alone. Numbers are written the way TEd writes them ([`format_ted_number`]): "%.6f" rounded half away from
//! zero, trailing zeros removed.
//!
//! Errors are messages, like the C++ code's exceptions.

use std::collections::BTreeMap;
use std::sync::Arc;

use wce_formats::c_num::{printf_f_msvc, printf_g, strtod_prefix, trim_fraction};
use wce_formats::xml_doc::{XmlDoc, XmlSplice, encode_entities};
use wce_foundation::math::Double3;

pub type Result<T> = std::result::Result<T, String>;

/// How TEd writes a number: float32, `%.6f` rounded half away from zero, trailing zeros removed (1.000000 -> "1", -0.000000 -> "-0",
/// 12.5 -> "12.5").
pub fn format_ted_number(v: f64) -> String {
    let d = f64::from(v as f32);

    // Not a number TEd writes; the C++ code read past the end of its text for these.
    if !d.is_finite() {
        return printf_g(d);
    }

    trim_fraction(&printf_f_msvc(d, 6)).to_owned()
}

/// A number at full precision, as tile.wce keeps it (the shortest text that reads back as the same f64); TEd's precision is applied when a
/// tile is converted for it (`project_files::to_ted`).
pub fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return printf_g(v);
    }

    if v == 0.0 {
        return "0".to_owned();
    }

    format!("{v}")
}

/// `%g` (6 significant digits) of the value as float32, used for `<point position="x,y,z">`.
pub fn format_general(v: f64) -> String {
    printf_g(f64::from(v as f32))
}

/// "vector(1,2,3)" / "euler(..)", or "x,y,z" with an empty prefix.
pub fn parse_vec3(text: &str, prefix: &str) -> Option<Double3> {
    let mut t = text;
    if !prefix.is_empty() {
        if !t.starts_with(prefix) || t.len() < prefix.len() + 2 || t.as_bytes()[prefix.len()] != b'(' || !t.ends_with(')') {
            return None;
        }

        t = &t[prefix.len() + 1..t.len() - 1];
    }

    let mut v = [0.0; 3];
    for (i, value) in v.iter_mut().enumerate() {
        let (number, used) = strtod_prefix(t)?;
        *value = number;
        t = &t[used..];
        if i < 2 {
            t = t.strip_prefix(',')?;
        }
    }

    Some(v.into())
}

/// The first byte of the line that holds `pos` (start of its indentation).
fn line_start(t: &[u8], mut pos: usize) -> usize {
    while pos > 0 && t[pos - 1] != b'\n' {
        pos -= 1;
    }

    pos
}

/// Just after the newline that ends the line containing `pos`.
fn line_end(t: &[u8], pos: usize) -> usize {
    t[pos..].iter().position(|&c| c == b'\n').map_or(t.len(), |e| pos + e + 1)
}

fn indent_of(t: &[u8], pos: usize) -> Vec<u8> {
    let a = line_start(t, pos);
    let b = a + t[a..].iter().take_while(|&&c| c == b' ' || c == b'\t').count();
    t[a..b].to_vec()
}

/// The last occurrence of `pat` that starts at or before `pos`.
fn rfind_at(t: &[u8], pat: &[u8], pos: usize) -> Option<usize> {
    let last = pos.min(t.len().checked_sub(pat.len())?);
    (0..=last).rev().find(|&i| &t[i..i + pat.len()] == pat)
}

fn find(t: &[u8], pat: &[u8]) -> bool {
    t.windows(pat.len()).any(|w| w == pat)
}

/// Re-indents the lines of a cut piece from `old` to `new` indentation.
fn reindent(piece: &[u8], old: &[u8], new: &[u8], out: &mut Vec<u8>) {
    for line in piece.split_inclusive(|&c| c == b'\n') {
        if line.starts_with(old) {
            out.extend_from_slice(new);
            out.extend_from_slice(&line[old.len()..]);
        } else {
            out.extend_from_slice(line);
        }
    }
}

/// The text with `\r\n` line ends made `\n`.
fn lf(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for (i, &c) in text.iter().enumerate() {
        if c != b'\r' || text.get(i + 1) != Some(&b'\n') {
            out.push(c);
        }
    }

    out
}

/// `\n` line ends made `newline`.
fn with_newline(text: &[u8], newline: &[u8]) -> Vec<u8> {
    if newline == b"\n" {
        return text.to_vec();
    }

    let mut out = Vec::with_capacity(text.len() + text.len() / 40);
    for &c in text {
        if c == b'\n' {
            out.extend_from_slice(newline);
        } else {
            out.push(c);
        }
    }

    out
}

fn xml_error(error: wce_formats::xml_doc::XmlError) -> String {
    error.to_string()
}

/// A whole-document snapshot for undo of structural edits.
pub type Snapshot = Arc<XmlDoc>;

#[derive(Clone, Debug)]
pub struct TedScene {
    doc: XmlDoc,
}

impl TedScene {

    /// Keeps the bytes as the document's origin.
    ///
    /// # Errors
    ///
    /// Not XML, or not a tile.ted project.
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        let doc = XmlDoc::parse(bytes).map_err(xml_error)?;
        if doc.roots().first().is_none_or(|&r| doc.elements()[r].name != "project") {
            return Err("not a tile.ted project".to_owned());
        }

        Ok(Self { doc })
    }

    /// Another file of TEd's elements (a prefab's `INSTANCE_SELECTION`), read the same way.
    ///
    /// # Errors
    ///
    /// Not XML.
    pub fn parse_any(bytes: Vec<u8>) -> Result<Self> {
        Ok(Self { doc: XmlDoc::parse(bytes).map_err(xml_error)? })
    }

    pub fn save(&self) -> Vec<u8> {
        self.doc.save()
    }

    pub fn doc(&self) -> &XmlDoc {
        &self.doc
    }

    /// `<project version="17">`.
    pub fn project_version(&self) -> i32 {
        self.doc.get(self.doc.roots()[0], "version").and_then(|v| wce_formats::c_num::strtol(&v)).map_or(0, |v| v as i32)
    }

    /// The SCENE_GRAPH element, if the tile has one.
    pub fn scene_graph(&self) -> Option<usize> {
        let els = self.doc.elements();
        els[self.doc.roots()[0]].children.iter().filter(|&&c| els[c].name == "scene")
            .flat_map(|&c| els[c].children.iter().copied())
            .find(|&g| els[g].name == "SCENE_GRAPH")
    }

    /// Instances below `elem` without the `children` wrappers: direct instance children.
    pub fn children(&self, elem: usize) -> Vec<usize> {
        let els = self.doc.elements();
        els[elem].children.iter().filter(|&&c| els[c].name == "children").flat_map(|&c| els[c].children.iter().copied()).collect()
    }

    pub fn type_of(&self, elem: usize) -> &str {
        &self.doc.elements()[elem].name
    }

    /// LayerInstance, UserInstanceGroup, INSTANCE_GROUP, SCENE_GRAPH.
    pub fn is_layer(&self, elem: usize) -> bool {
        matches!(self.type_of(elem), "LayerInstance" | "UserInstanceGroup" | "INSTANCE_GROUP" | "SCENE_GRAPH")
    }

    pub fn name(&self, elem: usize) -> String {
        self.doc.get(elem, "name").unwrap_or_default()
    }

    pub fn position(&self, elem: usize) -> Option<Double3> {
        parse_vec3(&self.doc.get(elem, "position")?, "vector")
    }

    /// orientation="euler(x,y,z)" (radians).
    pub fn euler(&self, elem: usize) -> Option<Double3> {
        parse_vec3(&self.doc.get(elem, "orientation")?, "euler")
    }

    /// scale="vector(..)" or a plain number (uniform).
    pub fn scale(&self, elem: usize) -> Option<Double3> {
        let v = self.doc.get(elem, "scale")?;
        parse_vec3(&v, "vector").or_else(|| strtod_prefix(&v).map(|(d, _)| Double3::new(d, d, d)))
    }

    fn flags(&self, elem: usize) -> String {
        self.doc.get(elem, "flags").unwrap_or_default()
    }

    /// OIF_VISIBLE in flags.
    pub fn visible(&self, elem: usize) -> bool {
        self.flags(elem).contains("OIF_VISIBLE")
    }

    /// OIF_ACTIVE in flags.
    pub fn active(&self, elem: usize) -> bool {
        self.flags(elem).contains("OIF_ACTIVE")
    }

    /// A value equal (as float32) to the current one is a no-op.
    pub fn set_position(&mut self, elem: usize, v: Double3) {
        self.set_vector(elem, "position", "vector", v);
    }

    pub fn set_euler(&mut self, elem: usize, v: Double3) {
        self.set_vector(elem, "orientation", "euler", v);
    }

    /// # Errors
    ///
    /// The element has no position.
    pub fn translate(&mut self, elem: usize, delta: Double3) -> Result<()> {
        let p = self.position(elem).ok_or("element has no position")?;
        self.set_position(elem, p + delta);
        Ok(())
    }

    pub fn set_attr(&mut self, elem: usize, name: &str, value: &str) {
        self.doc.set(elem, name, value);
    }

    pub fn modified(&self) -> bool {
        self.doc.modified()
    }

    pub fn revert_all(&mut self) {
        self.doc.revert_all();
    }

    fn set_vector(&mut self, elem: usize, attr: &str, prefix: &str, v: Double3) {
        if let Some(old) = self.doc.get(elem, attr).and_then(|cur| parse_vec3(&cur, prefix))
            && (0..3).all(|i| old[i] == v[i])
        {
            return;
        }

        let text = format!("{prefix}({},{},{})", format_number(v.x), format_number(v.y), format_number(v.z));
        self.doc.set(elem, attr, &text);
    }

    /// Sets or clears a flag. TEd writes the flags as " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE" (a leading space, always in that order; " " when
    /// none is set; checked by toggling the eye and the cursor of a layer in TEd and diffing the saved .ted, docs/formats/tile_ted.md), so a flag
    /// toggled here changes the same bytes TEd changes.
    pub fn set_flag(&mut self, elem: usize, flag: &str, on: bool) {
        let f = self.flags(elem);
        let mut tokens = f.split('|').map(|t| t.trim_matches(' ')).filter(|t| !t.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        let at = tokens.iter().position(|t| t == flag);
        if at.is_some() == on {
            return;
        }

        match at {
            Some(i) => {
                tokens.remove(i);
            }

            None => tokens.push(flag.to_owned()),
        }

        let rank = |x: &String| ["OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"].iter().position(|o| o == x).unwrap_or(3);
        tokens.sort_by_key(rank);
        self.doc.set(elem, "flags", &format!(" {}", tokens.join("|")));
    }

    /// Structural edits are done in place on the XmlDoc (no re-parse of the file) and recorded as text splices. A caller that wants an undo
    /// record brackets them:
    ///
    /// ```text
    /// let mark = scene.begin_edit();   // bakes pending attribute edits (they belong to earlier commands)
    /// ... structural edits and attribute edits ...
    /// let record = scene.end_edit(mark);   // bakes the attribute edits of this command too
    /// ```
    ///
    /// and undoes / redoes the record with `unapply` / `apply`. The text a record refers to is the text after the commands before it, which is
    /// what the undo stack guarantees (last in, first out).
    ///
    /// # Errors
    ///
    /// The pending edits can't be baked.
    pub fn begin_edit(&mut self) -> Result<usize> {
        self.doc.bake_pending().map_err(xml_error)?;
        Ok(self.doc.journal_mark())
    }

    /// # Errors
    ///
    /// The pending edits can't be baked.
    pub fn end_edit(&mut self, mark: usize) -> Result<Vec<XmlSplice>> {
        self.doc.bake_pending().map_err(xml_error)?;
        let record = self.doc.journal_since(mark);
        self.doc.journal_forget(mark);
        Ok(record)
    }

    /// # Errors
    ///
    /// The record does not fit the text.
    pub fn apply(&mut self, record: &[XmlSplice]) -> Result<()> {
        record.iter().try_for_each(|s| self.doc.apply_splice(s)).map_err(xml_error)
    }

    /// # Errors
    ///
    /// The record does not fit the text.
    pub fn unapply(&mut self, record: &[XmlSplice]) -> Result<()> {
        record.iter().rev().try_for_each(|s| self.doc.unapply_splice(s)).map_err(xml_error)
    }

    /// A copy placed right after `elem`; returns the copy's index. Element indices after the edit point shift: re-resolve any stored index.
    ///
    /// # Errors
    ///
    /// The element can't be copied.
    pub fn duplicate(&mut self, elem: usize) -> Result<usize> {
        self.doc.duplicate_element_inplace(elem).map_err(xml_error)
    }

    /// # Errors
    ///
    /// The element can't be removed.
    pub fn remove(&mut self, elem: usize) -> Result<()> {
        self.doc.remove_element_inplace(elem).map_err(xml_error)
    }

    pub fn snapshot(&self) -> Snapshot {
        Arc::new(self.doc.clone())
    }

    pub fn restore(&mut self, s: &Snapshot) {
        self.doc = (**s).clone();
    }

    pub fn is_descendant(&self, elem: usize, ancestor: usize) -> bool {
        let els = self.doc.elements();
        let mut p = els[elem].parent;
        while let Some(q) = p {
            if q == ancestor {
                return true;
            }

            p = els[q].parent;
        }

        false
    }

    /// A tile.ted without SCENE_GRAPH (e.g. custom_monte_rosa/aa) has no layer to work on; layer operations on it must fail cleanly.
    fn require_element(&self, elem: Option<usize>, what: &str) -> Result<usize> {
        elem.filter(|&e| e < self.doc.elements().len()).ok_or_else(|| format!("no such {what} (the tile has no SCENE_GRAPH?)"))
    }

    fn children_wrapper(&self, layer: usize) -> Option<usize> {
        let els = self.doc.elements();
        els[layer].children.iter().copied().rev().find(|&c| els[c].name == "children")
    }

    fn newline(&self) -> &'static [u8] {
        if find(self.doc.original_text(), b"\r\n") { b"\r\n" } else { b"\n" }
    }

    /// Where a new child of `layer` goes: the start of the line that holds its `</children>`, with the children's indentation.
    fn append_point(&self, layer: usize, missing: &str) -> Result<(usize, Vec<u8>)> {
        let wrapper = self.children_wrapper(layer).ok_or(missing)?;
        let t = self.doc.original_text();
        let close = rfind_at(t, b"</children>", self.doc.elements()[wrapper].end_off).ok_or(missing)?;
        let mut ind = indent_of(t, close);
        ind.extend_from_slice(b"    ");
        Ok((line_start(t, close), ind))
    }

    fn last_child(&self, layer: usize) -> usize {
        let w = self.children_wrapper(layer).expect("the layer has its children element");
        *self.doc.elements()[w].children.last().expect("the inserted element")
    }

    /// An empty LayerInstance appended to the parent's children; returns its index. Text splices on the document: element indices change.
    ///
    /// # Errors
    ///
    /// No such layer, or it has no children element.
    pub fn add_layer(&mut self, parent_layer: Option<usize>, name: &str) -> Result<usize> {
        let mut open = b"<LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"".to_vec();
        open.extend(encode_entities(name.as_bytes(), b'"'));
        open.extend_from_slice(b"\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">");
        self.add_container(parent_layer, &open, b"</LayerInstance>")
    }

    /// An empty UserInstanceGroup (TEd's instance group) at a local position, appended to the layer's children; returns its index. Text
    /// splices on the document: element indices change.
    ///
    /// # Errors
    ///
    /// No such layer, or it has no children element.
    pub fn add_group(&mut self, parent_layer: Option<usize>, name: &str, position: Double3) -> Result<usize> {
        let mut open = b"<UserInstanceGroup flags=\" OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE\" lf_offset=\"0\" low_frequency_change_effect=\"GRM_AS_SINGLE\" position=\"".to_vec();
        open.extend_from_slice(format!("vector({},{},{})", format_number(position.x), format_number(position.y), format_number(position.z)).as_bytes());
        open.extend_from_slice(b"\" name=\"");
        open.extend(encode_entities(name.as_bytes(), b'"'));
        open.extend_from_slice(b"\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">");
        self.add_container(parent_layer, &open, b"</UserInstanceGroup>")
    }

    /// An element's lines with every pending edit, at column 0 and with `\n` line ends: the text a prefab file holds, or that
    /// [`Self::insert_element_text`] puts somewhere else.
    pub fn element_text(&self, elem: usize) -> Vec<u8> {
        let t = self.doc.original_text();
        let e = &self.doc.elements()[elem];
        let (a, b) = (line_start(t, e.start_off), line_end(t, e.end_off.saturating_sub(1)));
        let ind = indent_of(t, e.start_off);
        let mut out = vec![];
        reindent(&lf(&self.doc.text_of_range(a, b)), &ind, b"", &mut out);
        out
    }

    /// The lines of the elements inside a container's `<children>` (with every pending edit), at column 0 and with `\n` line ends; empty when
    /// it has none.
    pub fn children_text(&self, elem: usize) -> Vec<u8> {
        let Some(w) = self.children_wrapper(elem) else { return vec![] };
        let els = self.doc.elements();
        let (Some(&first), Some(&last)) = (els[w].children.first(), els[w].children.last()) else { return vec![] };
        let t = self.doc.original_text();
        let (a, b) = (line_start(t, els[first].start_off), line_end(t, els[last].end_off.saturating_sub(1)));
        let ind = indent_of(t, els[first].start_off);
        let mut out = vec![];
        reindent(&lf(&self.doc.text_of_range(a, b)), &ind, b"", &mut out);
        out
    }

    /// Replaces what is inside a container's `<children>` with `text` (lines at column 0, as [`Self::children_text`] gives them), indented
    /// under it and with the document's line ends. Text splices on the document: element indices change.
    ///
    /// # Errors
    ///
    /// No such element, or it has no `<children>` element.
    pub fn set_children_text(&mut self, elem: usize, text: &[u8]) -> Result<()> {
        let elem = self.require_element(Some(elem), "element")?;
        self.doc.bake_pending().map_err(xml_error)?;
        let w = self.children_wrapper(elem).ok_or("the element has no children element")?;
        let t = self.doc.original_text();
        let open_end = line_end(t, self.doc.elements()[w].start_off);
        let close = rfind_at(t, b"</children>", self.doc.elements()[w].end_off).ok_or("the element has no children element")?;
        let close_start = line_start(t, close);
        if close_start < open_end {
            return Err("the children element is on one line".to_owned());
        }

        let mut ind = indent_of(t, close);
        ind.extend_from_slice(b"    ");
        let mut body = vec![];
        reindent(text, b"", &ind, &mut body);
        let body = with_newline(&body, self.newline());
        self.doc.splice(open_end, close_start - open_end, &body).map_err(xml_error)?;
        Ok(())
    }

    /// Appends an element given as text (lines at column 0, as [`Self::element_text`] gives them) to a layer's children; returns its index.
    /// Text splices on the document: element indices change.
    ///
    /// # Errors
    ///
    /// No such layer, it has no children element, or the text is not one element.
    pub fn insert_element_text(&mut self, layer: usize, text: &[u8]) -> Result<usize> {
        let layer = self.require_element(Some(layer), "layer")?;
        self.doc.bake_pending().map_err(xml_error)?;
        let (at, ind) = self.append_point(layer, "layer has no children element")?;
        let before = self.doc.elements()[self.children_wrapper(layer).ok_or("layer has no children element")?].children.len();
        let mut body = vec![];
        reindent(text, b"", &ind, &mut body);
        if !body.ends_with(b"\n") {
            body.push(b'\n');
        }

        let body = with_newline(&body, self.newline());
        self.doc.splice(at, 0, &body).map_err(xml_error)?;
        let w = self.children_wrapper(layer).ok_or("layer has no children element")?;
        if self.doc.elements()[w].children.len() != before + 1 {
            return Err("the text is not one element".to_owned());
        }

        Ok(self.last_child(layer))
    }

    /// An element with an empty `<children>` appended to the layer's children: `open` is its start tag, `close` its end tag.
    fn add_container(&mut self, parent_layer: Option<usize>, open: &[u8], close: &[u8]) -> Result<usize> {
        let parent = self.require_element(parent_layer, "layer")?;
        self.doc.bake_pending().map_err(xml_error)?;
        let (at, ind) = self.append_point(parent, "layer has no children element")?;
        let nl = self.newline();
        let mut snippet = ind.clone();
        snippet.extend_from_slice(open);
        for part in [b"    <children>".as_slice(), b"    </children>", close] {
            snippet.extend_from_slice(nl);
            snippet.extend_from_slice(&ind);
            snippet.extend_from_slice(part);
        }

        snippet.extend_from_slice(nl);
        self.doc.splice(at, 0, &snippet).map_err(xml_error)?;
        Ok(self.last_child(parent))
    }

    /// An instance without children (`<tag attr="value" .../>`, the attributes in the order given, then `position` and `orientation` at
    /// zero when not given) appended to the layer's children; returns its index. Text splices on the document: element indices after it
    /// change.
    ///
    /// # Errors
    ///
    /// No such layer, or it has no children element.
    pub fn add_instance(&mut self, layer: Option<usize>, tag: &str, attrs: &[(String, String)]) -> Result<usize> {
        let layer = self.require_element(layer, "layer")?;
        self.doc.bake_pending().map_err(xml_error)?;
        let (at, ind) = self.append_point(layer, "layer has no children element")?;
        let mut snippet = ind.clone();
        snippet.push(b'<');
        snippet.extend_from_slice(tag.as_bytes());
        let mut all = attrs.to_vec();
        for (k, v) in [("position", "vector(0,0,0)"), ("orientation", "euler(0,0,0)")] {
            if !all.iter().any(|(n, _)| n == k) {
                all.push((k.to_owned(), v.to_owned()));
            }
        }

        for (k, v) in &all {
            snippet.push(b' ');
            snippet.extend_from_slice(k.as_bytes());
            snippet.extend_from_slice(b"=\"");
            snippet.extend(encode_entities(v.as_bytes(), b'"'));
            snippet.push(b'"');
        }

        snippet.extend_from_slice(b"/>");
        snippet.extend_from_slice(self.newline());
        self.doc.splice(at, 0, &snippet).map_err(xml_error)?;
        Ok(self.last_child(layer))
    }

    /// An instance with an empty `<children>` (a siege AI node or edge, a deployment zone group), its attributes as
    /// [`Self::add_instance`]'s, appended to the layer's children; returns its index. Text splices on the document.
    ///
    /// # Errors
    ///
    /// No such layer, or it has no children element.
    pub fn add_container_instance(&mut self, layer: Option<usize>, tag: &str, attrs: &[(String, String)]) -> Result<usize> {
        let layer = self.require_element(layer, "layer")?;
        self.doc.bake_pending().map_err(xml_error)?;
        let (at, ind) = self.append_point(layer, "layer has no children element")?;
        let nl = self.newline().to_vec();
        let mut snippet = ind.clone();
        snippet.push(b'<');
        snippet.extend_from_slice(tag.as_bytes());
        let mut all = attrs.to_vec();
        for (k, v) in [("position", "vector(0,0,0)"), ("orientation", "euler(0,0,0)")] {
            if !all.iter().any(|(n, _)| n == k) {
                all.push((k.to_owned(), v.to_owned()));
            }
        }

        for (k, v) in &all {
            snippet.push(b' ');
            snippet.extend_from_slice(k.as_bytes());
            snippet.extend_from_slice(b"=\"");
            snippet.extend(encode_entities(v.as_bytes(), b'"'));
            snippet.push(b'"');
        }

        snippet.push(b'>');
        let close = format!("</{tag}>");
        for part in [b"    <children>".as_slice(), b"    </children>", close.as_bytes()] {
            snippet.extend_from_slice(&nl);
            snippet.extend_from_slice(&ind);
            snippet.extend_from_slice(part);
        }

        snippet.extend_from_slice(&nl);
        self.doc.splice(at, 0, &snippet).map_err(xml_error)?;
        Ok(self.last_child(layer))
    }

    /// Moves the element (with its subtree) to the end of the layer's children; returns the new index.
    ///
    /// # Errors
    ///
    /// As [`Self::move_element`].
    pub fn move_to_layer(&mut self, elem: usize, layer: usize) -> Result<usize> {
        self.move_element(elem, layer, None)
    }

    /// Moves `elem` (with its subtree) into the children of `layer`, in front of the child `before` (`None`: at the end); also reorders
    /// inside one layer. Returns the moved element's new index.
    ///
    /// # Errors
    ///
    /// No such element or layer, a layer moved into itself, or `before` is not a child of the layer.
    pub fn move_element(&mut self, elem: usize, layer: usize, before: Option<usize>) -> Result<usize> {
        let elem = self.require_element(Some(elem), "element")?;
        let layer = self.require_element(Some(layer), "layer")?;
        if elem == layer || self.is_descendant(layer, elem) {
            return Err("cannot move a layer into itself".to_owned());
        }

        if before.is_some_and(|b| b == elem || self.is_descendant(b, elem)) {
            return Err("cannot move an element in front of itself".to_owned());
        }

        self.doc.bake_pending().map_err(xml_error)?;
        let t = self.doc.original_text();
        let e = &self.doc.elements()[elem];
        let sub = self.doc.subtree_size(elem);
        let (cut_a, cut_b) = (line_start(t, e.start_off), line_end(t, e.end_off - 1));
        let piece = t[cut_a..cut_b].to_vec();
        let wrapper = self.children_wrapper(layer).ok_or("target layer has no children element")?;
        let w = &self.doc.elements()[wrapper];

        // Where it goes: in front of `before` (the start of its line), or in front of </children>; k = its position among the target's
        // children afterwards.
        let kids = &w.children;
        let pos_elem = kids.iter().position(|&c| c == elem).unwrap_or(kids.len());
        let (at, ref_off, k) = match before {
            Some(b) => {
                let pos_before = kids.iter().position(|&c| c == b).ok_or("the element to put it in front of is not in that layer")?;
                let off = self.doc.elements()[b].start_off;
                (line_start(t, off), off, pos_before - usize::from(pos_elem < pos_before))
            }

            None => {
                let close = rfind_at(t, b"</children>", w.end_off).ok_or("target layer has no children element")?;
                (line_start(t, close), close, kids.len() - usize::from(pos_elem < kids.len()))
            }
        };

        // Already there.
        if at == cut_a || at == cut_b {
            return Ok(elem);
        }

        let old_ind = indent_of(t, e.start_off);
        let mut new_ind = indent_of(t, ref_off);
        if before.is_none() {
            new_ind.extend_from_slice(b"    ");
        }

        let mut moved = vec![];
        reindent(&piece, &old_ind, &new_ind, &mut moved);

        // The later edit first, so the earlier offsets stay valid.
        if cut_a < at {
            self.doc.splice(at, 0, &moved).map_err(xml_error)?;
            self.doc.splice(cut_a, cut_b - cut_a, b"").map_err(xml_error)?;
        } else {
            self.doc.splice(cut_a, cut_b - cut_a, b"").map_err(xml_error)?;
            self.doc.splice(at, 0, &moved).map_err(xml_error)?;
        }

        // The layer's own index shifts down when the cut was before it.
        let new_layer = layer - if elem < layer { sub } else { 0 };
        let w2 = self.children_wrapper(new_layer).ok_or("target layer has no children element")?;
        Ok(self.doc.elements()[w2].children[k])
    }

    /// Moves several elements (with their subtrees) into `layer`, in front of the child `before` (`None`: at the end), keeping their document
    /// order. All the cuts and the one insertion are computed against the same text and applied from the highest offset down, so none of them
    /// disturbs the offsets of the others. Returns the new indices of the moved elements in that order (elements below one that is also moved
    /// go with it and are not listed).
    ///
    /// # Errors
    ///
    /// As [`Self::move_element`], or the target is inside a moved element.
    pub fn move_elements(&mut self, mut elems: Vec<usize>, layer: usize, before: Option<usize>) -> Result<Vec<usize>> {
        let layer = self.require_element(Some(layer), "layer")?;
        elems.sort_unstable();
        elems.dedup();
        for &e in &elems {
            self.require_element(Some(e), "element")?;
            if e == layer || self.is_descendant(layer, e) {
                return Err("cannot move a layer into itself".to_owned());
            }
        }

        // An element whose ancestor is moved too goes with it.
        let els = self.doc.elements();
        let top = elems.iter().copied().filter(|&e| {
            let mut p = els[e].parent;
            while let Some(q) = p {
                if elems.binary_search(&q).is_ok() {
                    return false;
                }

                p = els[q].parent;
            }

            true
        }).collect::<Vec<_>>();

        let elems = top;
        if elems.is_empty() {
            return Ok(vec![]);
        }

        if let Some(b) = before
            && elems.iter().any(|&e| b == e || self.is_descendant(b, e))
        {
            return Err("cannot move an element in front of itself".to_owned());
        }

        self.doc.bake_pending().map_err(xml_error)?;
        let t = self.doc.original_text();
        let wrapper = self.children_wrapper(layer).ok_or("target layer has no children element")?;
        let w = &self.doc.elements()[wrapper];
        let (at, ref_off) = match before {
            Some(b) => {
                if !w.children.contains(&b) {
                    return Err("the element to put it in front of is not in that layer".to_owned());
                }

                let off = self.doc.elements()[b].start_off;
                (line_start(t, off), off)
            }

            None => {
                let close = rfind_at(t, b"</children>", w.end_off).ok_or("target layer has no children element")?;
                (line_start(t, close), close)
            }
        };

        let mut new_ind = indent_of(t, ref_off);
        if before.is_none() {
            new_ind.extend_from_slice(b"    ");
        }

        let mut edits = vec![];
        let mut inserted = vec![];
        let mut cut_before_target = 0;
        for &e in &elems {
            let el = &self.doc.elements()[e];
            let (a, b) = (line_start(t, el.start_off), line_end(t, el.end_off - 1));
            reindent(&t[a..b], &indent_of(t, el.start_off), &new_ind, &mut inserted);
            if at > a && at < b {
                return Err("the target position is inside a moved element".to_owned());
            }

            edits.push((a, b - a, vec![]));
            if e < layer {
                cut_before_target += self.doc.subtree_size(e);
            }
        }

        // Position of the moved block among the target's children afterwards.
        let k = w.children.iter().take_while(|&&c| Some(c) != before).filter(|c| elems.binary_search(c).is_err()).count();
        edits.push((at, 0, inserted));

        // Highest offset first; a stable sort keeps the insertion after a cut that starts at the same offset.
        edits.sort_by_key(|e| std::cmp::Reverse(e.0));
        for (pos, len, text) in edits {
            self.doc.splice(pos, len, &text).map_err(xml_error)?;
        }

        let w2 = self.children_wrapper(layer - cut_before_target).ok_or("target layer has no children element")?;
        let kids = &self.doc.elements()[w2].children;
        Ok((0..elems.len()).map(|i| kids[k + i]).collect())
    }

    /// The `<point>` children of a polygon (instances with a `<points>` child: AI hints, no-go / go zones, vegetation exclusion, siege AI,
    /// deployment ...).
    pub fn polygon_points(&self, elem: usize) -> Vec<usize> {
        let els = self.doc.elements();
        els[elem].children.iter().filter(|&&c| els[c].name == "points").flat_map(|&c| els[c].children.iter().copied()).filter(|&p| els[p].name == "point").collect()
    }

    /// Local offset from the instance position (scene units).
    pub fn point_position(&self, point_elem: usize) -> Option<Double3> {
        parse_vec3(&self.doc.get(point_elem, "position")?, "")
    }

    /// Shoelace on x, z of the local points (what TEd stores in `area`).
    pub fn polygon_area(&self, elem: usize) -> f64 {
        let pts = self.polygon_points(elem);
        let mut a = 0.0;
        for i in 0..pts.len() {
            let (Some(p), Some(q)) = (self.point_position(pts[i]), self.point_position(pts[(i + 1) % pts.len()])) else { return 0.0 };
            a += p.x * q.z - q.x * p.z;
        }

        a.abs() / 2.0
    }

    fn refresh_area(&mut self, elem: usize) {
        if self.doc.get(elem, "area").is_some() {
            let area = format_number(self.polygon_area(elem));
            self.set_attr(elem, "area", &area);
        }
    }

    fn point_text(local: Double3) -> String {
        format!("{},{},{}", format_number(local.x), format_number(local.y), format_number(local.z))
    }

    /// Also refreshes `area`.
    ///
    /// # Errors
    ///
    /// No such point.
    pub fn set_polygon_point(&mut self, elem: usize, index: usize, local: Double3) -> Result<()> {
        let pts = self.polygon_points(elem);
        let p = *pts.get(index).ok_or("point index out of range")?;
        self.doc.set(p, "position", &Self::point_text(local));
        self.refresh_area(elem);
        Ok(())
    }

    /// # Errors
    ///
    /// No such point.
    pub fn insert_polygon_point(&mut self, elem: usize, after_index: usize, local: Double3) -> Result<()> {
        let pts = self.polygon_points(elem);
        let p = *pts.get(after_index).ok_or("point index out of range")?;
        let copy = self.doc.duplicate_element_inplace(p).map_err(xml_error)?;
        self.doc.set(copy, "position", &Self::point_text(local));
        self.refresh_area(elem);
        Ok(())
    }

    /// Refuses below 2 points.
    ///
    /// # Errors
    ///
    /// The point can't be removed from the text.
    pub fn remove_polygon_point(&mut self, elem: usize, index: usize) -> Result<bool> {
        let pts = self.polygon_points(elem);
        if pts.len() <= 2 || index >= pts.len() {
            return Ok(false);
        }

        self.doc.remove_element_inplace(pts[index]).map_err(xml_error)?;
        self.refresh_area(elem);
        Ok(true)
    }

    /// A new polygon in `layer`, points in scene space (world). type: AI_HINT_POLYGON_INSTANCE, NOGO_REGION_POLYGON_INSTANCE,
    /// GO_REGION_POLYGON_INSTANCE, VegetationExclusionPolygonInstance, SIEGE_AI_POLYGON_INSTANCE. `extra` overrides attributes (hint_type,
    /// polygon_closed ...).
    ///
    /// # Errors
    ///
    /// No such layer, or fewer than 2 points.
    pub fn add_polygon(&mut self, layer: Option<usize>, kind: &str, name: &str, world_points: &[Double3], extra: &BTreeMap<String, String>) -> Result<usize> {
        let layer = self.require_element(layer, "layer")?;
        if world_points.len() < 2 {
            return Err("a polygon needs at least 2 points".to_owned());
        }

        // Attribute defaults per type (most common values in the shipped tiles).
        let mut a = [
            ("flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"), ("lf_offset", "0"), ("line_colour", "argb(1.0,1.0,1.0,1.0)"), ("polygon_closed", "1"),
            ("recenter_polygon", "0"), ("polygon_shape", "PS_POLYGON"), ("selected_line_colour", "argb(1.0,0.0,0.0,1.0)"), ("surface_mode", "MSM_LANDSCAPE"),
            ("orientation", "euler(0,0,0)"),
        ].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect::<BTreeMap<_, _>>();
        let mut set = |k: &str, v: &str| {
            a.insert(k.to_owned(), v.to_owned());
        };

        match kind {
            "AI_HINT_POLYGON_INSTANCE" => {
                for (k, v) in [("hint_type", "AIH_COVER_LINE"), ("generated", "false"), ("is_generated", "false"), ("max_subdivisions", "100"), ("subdivision_length", "5"), ("polygon_closed", "0")] {
                    set(k, v);
                }
            }

            "NOGO_REGION_POLYGON_INSTANCE" => set("line_colour", "argb(0.65,0.0,0.0,1.0)"),
            "SIEGE_AI_POLYGON_INSTANCE" => set("polygon_closed", "0"),
            // Not TEd's: a procedural wall (docs/plans/procedural_walls.md).
            "WceWallSpline" => {
                for (k, v) in [("polygon_closed", "0"), ("line_colour", "argb(1.0,0.8,0.0,1.0)"), ("wall_base", "barbarian_fort_straight"), ("wall_tolerance", "0.5"), ("wall_flip", "false"), ("wall_access_every", "200"), ("wall_defence_every", "0")] {
                    set(k, v);
                }
            }
            _ => {}
        }

        for (k, v) in extra {
            a.insert(k.clone(), v.clone());
        }

        // Centroid position, local points.
        let n = world_points.len() as f64;
        let mut c = Double3::ZERO;
        for &p in world_points {
            c += p / n;
        }

        a.insert("position".to_owned(), format!("vector({},{},{})", format_number(c.x), format_number(c.y), format_number(c.z)));
        a.insert("name".to_owned(), name.to_owned());
        let local = world_points.iter().map(|&p| p - c).collect::<Vec<_>>();
        let mut area = 0.0;
        for i in 0..local.len() {
            let (p, q) = (local[i], local[(i + 1) % local.len()]);
            area += p.x * q.z - q.x * p.z;
        }

        a.insert("area".to_owned(), format_number(area.abs() / 2.0));

        self.doc.bake_pending().map_err(xml_error)?;
        let (at, ind) = self.append_point(layer, "layer has no children element")?;
        let nl = self.newline();
        let mut snippet = ind.clone();
        snippet.push(b'<');
        snippet.extend_from_slice(kind.as_bytes());

        // TEd's attribute order (as in shipped tiles), then anything else.
        let emit = |snippet: &mut Vec<u8>, k: &str, v: &str| {
            snippet.push(b' ');
            snippet.extend_from_slice(k.as_bytes());
            snippet.extend_from_slice(b"=\"");
            snippet.extend(encode_entities(v.as_bytes(), b'"'));
            snippet.push(b'"');
        };

        let order = [
            "flags", "recenter_polygon", "lf_offset", "line_colour", "subdivision_length", "max_subdivisions", "hint_type", "is_generated", "polygon_closed",
            "position", "selected_line_colour", "name", "polygon_shape", "area", "generated", "surface_mode", "orientation",
        ];

        for k in order {
            if let Some(v) = a.remove(k) {
                emit(&mut snippet, k, &v);
            }
        }

        for (k, v) in &a {
            emit(&mut snippet, k, v);
        }

        let line = |snippet: &mut Vec<u8>, extra_indent: &str, text: &[u8]| {
            snippet.extend_from_slice(nl);
            snippet.extend_from_slice(&ind);
            snippet.extend_from_slice(extra_indent.as_bytes());
            snippet.extend_from_slice(text);
        };

        snippet.push(b'>');
        line(&mut snippet, "    ", b"<points>");
        for p in &local {
            line(&mut snippet, "        ", format!("<point position=\"{}\"/>", Self::point_text(*p)).as_bytes());
        }

        line(&mut snippet, "    ", b"</points>");
        line(&mut snippet, "", format!("</{kind}>").as_bytes());
        snippet.extend_from_slice(nl);
        self.doc.splice(at, 0, &snippet).map_err(xml_error)?;
        Ok(self.last_child(layer))
    }

    /// All instances (depth first) of a given element name, e.g. "BUILDING_INSTANCE".
    pub fn find_all(&self, kind: &str) -> Vec<usize> {
        (0..self.doc.elements().len()).filter(|&i| self.doc.elements()[i].name == kind).collect()
    }

    /// Layer path "Logic/Go Zones" of an element.
    pub fn layer_path(&self, elem: usize) -> String {
        let els = self.doc.elements();
        let mut names = vec![];
        let mut p = els[elem].parent;
        while let Some(q) = p {
            if self.is_layer(q) && els[q].name != "SCENE_GRAPH" {
                names.push(self.name(q));
            }

            p = els[q].parent;
        }

        names.reverse();
        names.join("/")
    }
}
