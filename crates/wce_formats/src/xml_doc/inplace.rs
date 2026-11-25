//! In-place structural edits for big documents.
//!
//! A 45 MB tile.ted has 130 000 elements: the functional edits read the whole file twice and copy it per edit. These change
//! the same text, but patch the element table instead of reading it again, and every change is also recorded as an
//! [`XmlSplice`] so that a caller can undo and redo it without keeping whole documents. Pending attribute edits must be baked
//! first ([`XmlDoc::bake_pending`]); the record of an edit then starts at [`XmlDoc::journal_mark`].

use std::collections::BTreeMap;

use super::parse::{parse_range, parse_start_tag};
use super::structural::{block_of, eof_cut, eol_of};
use super::{Result, XmlDoc, XmlElement, XmlError, XmlSplice, added_attr_text};

/// Moves every offset of an element by `delta`.
fn shift(el: &mut XmlElement, delta: isize) {
    el.start_off = el.start_off.wrapping_add_signed(delta);
    el.attrs_end = el.attrs_end.wrapping_add_signed(delta);
    el.start_end = el.start_end.wrapping_add_signed(delta);
    el.end_off = el.end_off.wrapping_add_signed(delta);
    for at in &mut el.attrs {
        at.value_off = at.value_off.wrapping_add_signed(delta);
    }
}

impl XmlDoc {
    /// Where the record of the next edits starts.
    pub fn journal_mark(&self) -> usize {
        self.journal.len()
    }

    /// The records since a mark.
    pub fn journal_since(&self, mark: usize) -> Vec<XmlSplice> {
        self.journal[mark..].to_vec()
    }

    /// Drops the records since a mark (the caller took them).
    pub fn journal_forget(&mut self, mark: usize) {
        self.journal.truncate(mark);
    }

    /// Reads the element table of the whole text again.
    fn reparse_in_place(&mut self) -> Result<()> {
        let mut elems = Vec::with_capacity(self.text.len() / 256 + 16);
        let mut roots = vec![];
        parse_range(&self.text, 0, self.text.len(), &mut elems, &mut roots, None)?;
        self.elems = elems;
        self.roots = roots;
        Ok(())
    }

    /// Patches the table for a replacement of whole elements below one parent element: the removed ones lie completely inside
    /// `pos..pos + remove_len`, the inserted text holds whole elements. False if the replacement is not one.
    fn patch_elements(&mut self, pos: usize, remove_len: usize, insert: &[u8]) -> Result<bool> {
        let end_pos = pos + remove_len;
        let n = self.elems.len();
        let a = self.elems.partition_point(|el| el.start_off < pos);
        let mut b = a;
        while b < n && self.elems[b].start_off < end_pos {
            if self.elems[b].end_off > end_pos {
                return Ok(false);
            }

            b += 1;
        }

        let mut parent = None;
        let mut candidate = a.checked_sub(1);
        while let Some(c) = candidate {
            let el = &self.elems[c];
            if !el.self_closing && el.start_end <= pos && pos < el.end_off && end_pos <= el.end_off {
                parent = Some(c);
                break;
            }

            candidate = el.parent;
        }

        let Some(parent) = parent else { return Ok(false) };
        if self.elems[a..b].iter().any(|el| el.parent != Some(parent) && !el.parent.is_some_and(|p| p >= a && p < b)) {
            return Ok(false);
        }

        let mut frag = vec![];
        let mut froots = vec![];
        if !insert.is_empty() {
            parse_range(insert, 0, insert.len(), &mut frag, &mut froots, None)?;
        }

        let delta = insert.len() as isize - remove_len as isize;
        let (m, k) = (b - a, frag.len());
        let map = |i: usize| if i < a { i } else { i - m + k };

        for i in (0..a).chain(b..n) {
            let el = &mut self.elems[i];
            el.parent = el.parent.map(map);
            if i == parent {
                let mut kids = Vec::with_capacity(el.children.len() + k);
                let mut placed = false;
                for &c in &el.children {
                    if c >= a && c < b {
                        continue;
                    }

                    if !placed && c >= b {
                        kids.extend(froots.iter().map(|&r| a + r));
                        placed = true;
                    }

                    kids.push(map(c));
                }

                if !placed {
                    kids.extend(froots.iter().map(|&r| a + r));
                }

                el.children = kids;
            } else {
                el.children.iter_mut().for_each(|c| *c = map(*c));
            }

            if i >= b {
                shift(el, delta);
            }
        }

        // The containers got longer or shorter.
        let mut container = Some(parent);
        while let Some(c) = container {
            let el = &mut self.elems[c];
            el.end_off = el.end_off.wrapping_add_signed(delta);
            container = el.parent;
        }

        for el in &mut frag {
            shift(el, pos as isize);
            el.parent = Some(el.parent.map_or(parent, |p| a + p));
            el.children.iter_mut().for_each(|c| *c += a);
        }

        self.elems.splice(a..b, frag);

        // A root is never inside the replaced range here: it would have no parent.
        self.roots.iter_mut().for_each(|r| *r = map(*r));
        Ok(true)
    }

    /// Patches the table for a replacement inside the start tag of one element (an attribute value, an added attribute): that tag
    /// is read again, everything after it shifts. False if the replacement is not one.
    fn patch_start_tag(&mut self, pos: usize, remove_len: usize, insert: &[u8]) -> Result<bool> {
        let a = self.elems.partition_point(|el| el.start_off < pos);
        let Some(c) = a.checked_sub(1) else { return Ok(false) };
        let el = &self.elems[c];

        // An insertion right after the tag is content, not part of the tag.
        if pos >= el.start_end || pos + remove_len > el.start_end {
            return Ok(false);
        }

        // The tag as it will read: the text is still the old one here.
        let delta = insert.len() as isize - remove_len as isize;
        let mut tag = self.text[el.start_off..el.start_end].to_vec();
        let at = pos - el.start_off;
        tag.splice(at..at + remove_len, insert.iter().copied());
        let (fresh, after) = parse_start_tag(&tag, tag.len(), 0, &mut vec![])?;
        if after != tag.len() || fresh.self_closing != el.self_closing || fresh.name != el.name {
            return Ok(false);
        }

        let el = &mut self.elems[c];
        let base = el.start_off;
        el.attrs = fresh.attrs;
        el.attrs.iter_mut().for_each(|at| at.value_off += base);
        el.attrs_end = fresh.attrs_end + base;
        el.start_end = fresh.start_end + base;
        el.end_off = el.end_off.wrapping_add_signed(delta);
        let mut container = el.parent;

        // Everything after the tag, children included.
        self.elems[a..].iter_mut().for_each(|el| shift(el, delta));
        while let Some(p) = container {
            let el = &mut self.elems[p];
            el.end_off = el.end_off.wrapping_add_signed(delta);
            container = el.parent;
        }

        Ok(true)
    }

    /// Patches the table for a replacement that cuts through elements (one splice over many removals below different layers, or its
    /// undo): the span widened to the whole children of the innermost element holding it, and that read again, as `patch_elements`.
    /// False when no element holds it.
    fn patch_widened(&mut self, pos: usize, remove_len: usize, insert: &[u8]) -> bool {
        let end_pos = pos + remove_len;
        let content_end = |el: &XmlElement| self.text[el.start_end..el.end_off].iter().rposition(|&c| c == b'<').map(|i| el.start_end + i);
        let mut candidate = self.elems.partition_point(|el| el.start_off < pos).checked_sub(1);
        let holder = loop {
            let Some(c) = candidate else { return false };
            let el = &self.elems[c];
            if !el.self_closing && el.start_end <= pos && content_end(el).is_some_and(|ce| end_pos <= ce) {
                break c;
            }

            candidate = el.parent;
        };

        // The children the span starts or ends inside.
        let kids = &self.elems[holder].children;
        let straddling = |at: usize| {
            let i = kids.partition_point(|&k| self.elems[k].start_off < at);
            i.checked_sub(1).map(|i| &self.elems[kids[i]]).filter(|k| at < k.end_off)
        };
        let lo = straddling(pos).map_or(pos, |k| k.start_off);
        let hi = if end_pos > pos { straddling(end_pos).map_or(end_pos, |k| k.end_off) } else { end_pos };
        if (lo, hi) == (pos, end_pos) {
            return false;
        }

        let wide = [&self.text[lo..pos], insert, &self.text[end_pos..hi]].concat();
        self.patch_elements(lo, hi - lo, &wide).unwrap_or(false)
    }

    /// Replaces text and patches the table, reading the whole text again if the change is neither whole elements nor inside one tag.
    /// Returns if the table was patched.
    pub(super) fn replace_text(&mut self, pos: usize, remove_len: usize, insert: &[u8], structural: bool) -> Result<bool> {
        if pos > self.text.len() || remove_len > self.text.len() - pos {
            return Err(XmlError::Edit("xml: splice outside the text".to_owned()));
        }

        if self.has_pending_edits() {
            return Err(XmlError::Edit("xml: splice with pending attribute edits".to_owned()));
        }

        // A fragment that can't be read is no patch: the whole text is read again, which gives the error if there is one.
        let patched = match self.patch_start_tag(pos, remove_len, insert) {
            Ok(true) => true,
            Ok(false) => self.patch_elements(pos, remove_len, insert).unwrap_or(false) || self.patch_widened(pos, remove_len, insert),
            Err(_) => false,
        };

        self.text.splice(pos..pos + remove_len, insert.iter().copied());
        if !patched {
            self.reparse_in_place()?;
        }

        if structural {
            self.structural += 1;
        }

        Ok(patched)
    }

    /// Replaces text; journaled and counted as a structural edit.
    ///
    /// # Errors
    ///
    /// Fails for a span outside the text, with pending attribute edits, or if the new text can't be read.
    pub fn splice(&mut self, pos: usize, remove_len: usize, insert: &[u8]) -> Result<XmlSplice> {
        let removed = self.text.get(pos..pos.saturating_add(remove_len)).map(<[u8]>::to_vec).unwrap_or_default();
        let splice = XmlSplice { pos, removed, inserted: insert.to_vec() };
        self.replace_text(pos, remove_len, insert, true)?;
        self.journal.push(splice.clone());
        Ok(splice)
    }

    /// Redoes a recorded replacement. Not journaled.
    ///
    /// # Errors
    ///
    /// Fails if the replacement doesn't fit the text, or the new text can't be read.
    pub fn apply_splice(&mut self, splice: &XmlSplice) -> Result<()> {
        let mark = self.journal.len();
        self.bake_pending()?;
        self.replace_text(splice.pos, splice.removed.len(), &splice.inserted, true)?;
        self.journal.truncate(mark);
        Ok(())
    }

    /// Undoes a recorded replacement. Not journaled.
    ///
    /// # Errors
    ///
    /// Fails if the replacement doesn't fit the text, or the new text can't be read.
    pub fn unapply_splice(&mut self, splice: &XmlSplice) -> Result<()> {
        let mark = self.journal.len();
        self.bake_pending()?;
        self.replace_text(splice.pos, splice.inserted.len(), &splice.removed, false)?;
        self.structural = self.structural.saturating_sub(1);
        self.journal.truncate(mark);
        Ok(())
    }

    /// Makes the pending attribute edits part of the text: journaled, not counted as structural.
    ///
    /// Value edits go in last first (so the offsets of the earlier ones stay valid); added attributes are inserted before the end of their tag.
    ///
    /// # Errors
    ///
    /// Fails if the new text can't be read.
    pub fn bake_pending(&mut self) -> Result<()> {
        if !self.has_pending_edits() {
            return Ok(());
        }

        let mut ops = std::mem::take(&mut self.edits).into_iter().map(|(off, edit)| (off, edit.len, edit.text)).collect::<Vec<_>>();

        // Several added attributes of one element go in as one insertion, in the order they were added.
        let mut insert_text = BTreeMap::<usize, Vec<u8>>::new();
        for ((elem, attr), raw) in std::mem::take(&mut self.appended_values) {
            insert_text.entry(elem).or_default().extend_from_slice(&added_attr_text(&self.elems[elem].attrs[attr], &raw));
        }

        ops.extend(insert_text.into_iter().map(|(elem, text)| (self.elems[elem].attrs_end, 0, text)));
        ops.sort_by_key(|op| std::cmp::Reverse(op.0));

        // The added attributes leave the table: they come back read from the new tag text.
        for el in &mut self.elems {
            while el.attrs.last().is_some_and(|at| at.appended) {
                el.attrs.pop();
            }
        }

        // Many edits (a whole tile flipped: 36,000 on a city) go in as one splice over the span they cover: each splice moves the whole
        // text and every offset after it, and undo replays them one by one (45 s for the flip of a 6.8 MB tile.wce; 2026-11-12). The text
        // is the same; the span is read again once.
        if ops.len() > COALESCE {
            let lo = ops.iter().map(|op| op.0).min().unwrap_or(0);
            let hi = ops.iter().map(|op| op.0 + op.1).max().unwrap_or(lo);
            let mut inserted = Vec::with_capacity(hi - lo + ops.iter().map(|op| op.2.len()).sum::<usize>());
            let mut at = lo;
            for (pos, remove_len, text) in ops.iter().rev() {
                inserted.extend_from_slice(&self.text[at..*pos]);
                inserted.extend_from_slice(text);
                at = pos + remove_len;
            }

            inserted.extend_from_slice(&self.text[at..hi]);
            let removed = self.text[lo..hi].to_vec();
            self.replace_text(lo, hi - lo, &inserted, false)?;
            self.journal.push(XmlSplice { pos: lo, removed, inserted });
            return Ok(());
        }

        for (pos, remove_len, text) in ops {
            let removed = self.text[pos..pos + remove_len].to_vec();
            self.replace_text(pos, remove_len, &text, false)?;
            self.journal.push(XmlSplice { pos, removed, inserted: text });
        }

        Ok(())
    }

    /// Removes an element: the same text as [`XmlDoc::removed_element`].
    ///
    /// # Errors
    ///
    /// Fails for the only root element, or if the new text can't be read.
    pub fn remove_element_inplace(&mut self, e: usize) -> Result<()> {
        self.bake_pending()?;
        let el = &self.elems[e];
        if el.parent.is_none() && self.roots.len() == 1 {
            return Err(XmlError::Edit("cannot remove the only root element".to_owned()));
        }

        let k = block_of(&self.text, el);
        if k.aligned {
            self.splice(k.ls, k.le - k.ls, b"")?;
        } else if k.at_eof && k.ls > 0 {
            let q = eof_cut(&self.text, k.ls);
            self.splice(q, self.text.len() - q, b"")?;
        } else {
            self.splice(el.start_off, el.end_off - el.start_off, b"")?;
        }

        Ok(())
    }

    /// Removes elements (none inside another): the text of removing them one by one from the last, in one splice. One by one, every
    /// removal moves the text and the element table after it: deleting thousands of trees took seconds, and their undo as long.
    ///
    /// # Errors
    ///
    /// As [`XmlDoc::remove_element_inplace`].
    pub fn remove_elements_inplace(&mut self, elems: &[usize]) -> Result<()> {
        self.bake_pending()?;
        let mut elems = elems.to_vec();
        elems.sort_unstable();
        elems.dedup();

        // Whole lines that do not touch: else (an element sharing a line, at the end of the file) one by one, as before.
        let cuts = elems.iter().map(|&e| block_of(&self.text, &self.elems[e])).collect::<Vec<_>>();
        let apart = cuts.windows(2).all(|w| w[0].le <= w[1].ls);
        if elems.len() < 2 || !apart || cuts.iter().any(|k| !k.aligned) {
            for &e in elems.iter().rev() {
                self.remove_element_inplace(e)?;
            }

            return Ok(());
        }

        let (lo, hi) = (cuts[0].ls, cuts[cuts.len() - 1].le);
        let mut kept = Vec::with_capacity(hi - lo - cuts.iter().map(|k| k.le - k.ls).sum::<usize>());
        let mut at = lo;
        for k in &cuts {
            kept.extend_from_slice(&self.text[at..k.ls]);
            at = k.le;
        }

        self.splice(lo, hi - lo, &kept)?;
        Ok(())
    }

    /// Copies an element right after it: the same text as [`XmlDoc::duplicated_element`]. Returns the copy's index.
    ///
    /// # Errors
    ///
    /// Fails if the new text can't be read.
    pub fn duplicate_element_inplace(&mut self, e: usize) -> Result<usize> {
        self.bake_pending()?;
        let el = &self.elems[e];
        let k = block_of(&self.text, el);
        let new_index = e + self.subtree_size(e);
        let (pos, copy) = if k.aligned {
            (k.le, self.text[k.ls..k.le].to_vec())
        } else if k.at_eof {
            (el.end_off, [eol_of(&self.text), &self.text[k.ls..el.end_off]].concat())
        } else {
            (el.end_off, self.text[el.start_off..el.end_off].to_vec())
        };

        self.splice(pos, 0, &copy)?;
        Ok(new_index)
    }
}

/// Past this many pending edits, `bake_pending` makes them one splice.
const COALESCE: usize = 64;
