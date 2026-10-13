//! Structural edits that return a new document.
//!
//! Each one bakes the pending attribute edits into the text (they stop showing as modified), changes the text, and reads it
//! again: the elements after the edit point get new indices. The text outside the edited span stays byte-identical, so the
//! rest of the file still saves unchanged.

use memchr::memmem;

use super::{Result, XmlDoc, XmlElement, XmlError};
use crate::text_encoding::encode_text;

/// The lines an element occupies.
pub(super) struct Block {
    pub ls: usize,
    pub le: usize,

    /// The element starts a line (after indentation) and ends one: the whole lines `ls..le` can be cut or copied.
    pub aligned: bool,

    /// The element starts a line and ends the file.
    pub at_eof: bool,
}

pub(super) fn block_of(t: &[u8], el: &XmlElement) -> Block {
    let mut block = Block { ls: el.start_off, le: el.end_off, aligned: false, at_eof: false };
    let mut ls = el.start_off;
    while ls > 0 && (t[ls - 1] == b' ' || t[ls - 1] == b'\t') {
        ls -= 1;
    }

    let lead = ls == 0 || t[ls - 1] == b'\n';
    let mut p = el.end_off;
    while p < t.len() && (t[p] == b' ' || t[p] == b'\t') {
        p += 1;
    }

    if p < t.len() && t[p] == b'\r' {
        p += 1;
    }

    if lead && p < t.len() && t[p] == b'\n' {
        block.ls = ls;
        block.le = p + 1;
        block.aligned = true;
    } else if lead && p >= t.len() {
        block.ls = ls;
        block.at_eof = true;
    }

    block
}

/// The line break of a text: CRLF if it has any.
pub(super) fn eol_of(t: &[u8]) -> &'static [u8] {
    if memmem::find(t, b"\r\n").is_some() { b"\r\n" } else { b"\n" }
}

/// The leading whitespace of the line holding `off`.
fn line_indent(t: &[u8], off: usize) -> Vec<u8> {
    let mut ls = off;
    while ls > 0 && t[ls - 1] != b'\n' {
        ls -= 1;
    }

    let mut e = ls;
    while e < off && (t[e] == b' ' || t[e] == b'\t') {
        e += 1;
    }

    t[ls..e].to_vec()
}

/// Where a removed element's text starts, if it's the last line of the file: its line break goes with it.
pub(super) fn eof_cut(t: &[u8], ls: usize) -> usize {
    let mut q = ls;
    if t[q - 1] == b'\n' {
        q -= 1;
    }

    if q > 0 && t[q - 1] == b'\r' {
        q -= 1;
    }

    q
}

/// An element or attribute name the edits accept.
fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    match bytes.first() {
        None => false,
        Some(first) if first.is_ascii_digit() || *first == b'-' || *first == b'.' => false,
        _ => bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')),
    }
}

/// Whitespace as C's `isspace` sees it.
fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

/// The last occurrence of `needle` in `hay` starting at or before `pos`.
fn rfind(hay: &[u8], needle: &[u8], pos: usize) -> Option<usize> {
    let end = pos.saturating_add(needle.len()).min(hay.len());
    memmem::rfind(&hay[..end], needle)
}

fn edit_error<T>(what: &str) -> Result<T> {
    Err(XmlError::Edit(what.to_owned()))
}

impl XmlDoc {

    /// The document read again from its saved text, keeping the origin and the edit counter.
    pub(super) fn baked(&self) -> Result<Self> {
        let mut doc = Self::parse(self.save())?;
        doc.origin = self.origin.clone();
        doc.structural = self.structural;
        Ok(doc)
    }

    /// A document read from an edited text, keeping the origin and counting one more structural edit.
    fn rebuilt(&self, text: &[u8]) -> Result<Self> {
        let mut doc = Self::parse(encode_text(text, self.encoding))?;
        doc.origin = self.origin.clone();
        doc.structural = self.structural + 1;
        Ok(doc)
    }

    /// The document without an element (and its lines, if it has them to itself).
    ///
    /// # Errors
    ///
    /// Fails for the only root element, or if the result can't be read.
    pub fn removed_element(&self, e: usize) -> Result<Self> {
        let doc = self.baked()?;
        let el = &doc.elems[e];
        if el.parent.is_none() && doc.roots.len() == 1 {
            return edit_error("cannot remove the only root element");
        }

        let k = block_of(&doc.text, el);
        let mut t = doc.text.clone();
        if k.aligned {
            t.drain(k.ls..k.le);
        } else if k.at_eof && k.ls > 0 {
            t.truncate(eof_cut(&t, k.ls));
        } else {
            t.drain(el.start_off..el.end_off);
        }

        doc.rebuilt(&t)
    }

    /// The document with a copy of an element right after it. Returns it with the copy's index.
    ///
    /// # Errors
    ///
    /// Fails if the result can't be read.
    pub fn duplicated_element(&self, e: usize) -> Result<(Self, usize)> {
        let doc = self.baked()?;
        let el = &doc.elems[e];
        let k = block_of(&doc.text, el);
        let t0 = &doc.text;
        let mut t = t0.clone();
        if k.aligned {
            t.splice(k.le..k.le, t0[k.ls..k.le].iter().copied());
        } else if k.at_eof {
            let copy = [eol_of(t0), &t0[k.ls..el.end_off]].concat();
            t.splice(el.end_off..el.end_off, copy);
        } else {
            t.splice(el.end_off..el.end_off, t0[el.start_off..el.end_off].iter().copied());
        }

        let new_index = e + doc.subtree_size(e);
        Ok((doc.rebuilt(&t)?, new_index))
    }

    /// The document with an element swapped with its previous (`dir` < 0) or next sibling. Both must have their lines to themselves.
    ///
    /// # Errors
    ///
    /// Fails if there's no sibling that way, the two are not on separate adjacent lines, or the result can't be read.
    pub fn moved_element(&self, e: usize, dir: i32) -> Result<Self> {
        let doc = self.baked()?;
        let el = &doc.elems[e];
        let siblings = match el.parent {
            Some(parent) => &doc.elems[parent].children,
            None => &doc.roots,
        };

        let at = siblings.iter().position(|&sibling| sibling == e).unwrap_or_default();
        if if dir < 0 { at == 0 } else { at + 1 == siblings.len() } {
            return edit_error("no sibling to swap with");
        }

        // `a` comes first in the file.
        let (a, c) = if dir < 0 { (siblings[at - 1], e) } else { (e, siblings[at + 1]) };
        let t0 = &doc.text;
        let (ba, bc) = (block_of(t0, &doc.elems[a]), block_of(t0, &doc.elems[c]));
        if !ba.aligned || !bc.aligned || ba.le != bc.ls {
            return edit_error("elements are not on separate adjacent lines");
        }

        let t = [&t0[..ba.ls], &t0[bc.ls..bc.le], &t0[ba.ls..ba.le], &t0[bc.le..]].concat();
        doc.rebuilt(&t)
    }

    /// The document with a new empty element as the last child of `parent`. Returns it with the new element's index.
    ///
    /// # Errors
    ///
    /// Fails for an invalid name, a parent with text content, or if the result can't be read.
    pub fn with_child(&self, parent: usize, name: &str) -> Result<(Self, usize)> {
        if !valid_name(name) {
            return edit_error("invalid element name");
        }

        let doc = self.baked()?;
        let p = &doc.elems[parent];
        let t0 = &doc.text;
        let eol = eol_of(t0);
        let child = format!("<{name}/>").into_bytes();
        let pin = line_indent(t0, p.start_off);
        let unit: &[u8] = if pin.contains(&b'\t') { b"\t" } else { b"    " };
        let mut t = t0.clone();
        if let Some(&last) = p.children.last() {
            let last = &doc.elems[last];
            let k = block_of(t0, last);
            if k.aligned {
                let line = [&line_indent(t0, last.start_off), &child[..], eol].concat();
                t.splice(k.le..k.le, line);
            } else {
                t.splice(last.end_off..last.end_off, child);
            }
        } else if p.self_closing {
            let body = [b">", eol, &pin, unit, &child, eol, &pin, b"</", p.name.as_bytes(), b">"].concat();
            t.splice(p.attrs_end..p.end_off, body);
        } else {
            let end_tag = [b"</", p.name.as_bytes()].concat();
            let Some(et) = rfind(t0, &end_tag, p.end_off - 1) else { return edit_error("element has no end tag") };
            if t0[p.start_end..et].iter().any(|&byte| !is_c_space(byte)) {
                return edit_error("element has text content");
            }

            let body = [eol, &pin, unit, &child, eol, &pin].concat();
            t.splice(p.start_end..et, body);
        }

        let new_index = parent + doc.subtree_size(parent);
        Ok((doc.rebuilt(&t)?, new_index))
    }

    /// The document with an attribute set (added after the others if missing), as a structural edit.
    ///
    /// # Errors
    ///
    /// Fails for an invalid name, or if the result can't be read.
    pub fn with_attr_added(&self, elem: usize, name: &str, value: &str) -> Result<Self> {
        if !valid_name(name) {
            return edit_error("invalid attribute name");
        }

        let mut doc = self.clone();
        doc.set(elem, name, value);
        let baked = doc.baked()?;
        baked.rebuilt(&baked.text)
    }

    /// The document without an attribute, and the whitespace before it.
    ///
    /// # Errors
    ///
    /// Fails if the element has no such attribute, or if the result can't be read.
    pub fn without_attr(&self, elem: usize, attr: usize) -> Result<Self> {
        let doc = self.baked()?;
        let Some(a) = doc.elems[elem].attrs.get(attr) else { return edit_error("no such attribute") };
        let t0 = &doc.text;

        // From the separator before the name to the closing quote.
        let end = a.value_off + a.value_len + 1;
        let mut p = a.value_off - 1;
        while p > 0 && is_c_space(t0[p - 1]) {
            p -= 1;
        }

        if p > 0 && t0[p - 1] == b'=' {
            p -= 1;
        }

        while p > 0 && is_c_space(t0[p - 1]) {
            p -= 1;
        }

        p -= p.min(a.name.len());
        if p > 0 && (t0[p - 1] == b' ' || t0[p - 1] == b'\t') {
            p -= 1;
        }

        let mut t = t0.clone();
        t.drain(p..end);
        doc.rebuilt(&t)
    }
}
