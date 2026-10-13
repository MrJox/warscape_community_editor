//! Lossless XML document.
//!
//! The original text is never re-serialised. Elements and attributes only record byte spans into it; edits are span
//! replacements applied on save. An unedited document saves byte-identically, and an edited one differs only inside
//! the replaced spans. Several top-level elements are allowed (vfx files have `<shader_library/>`, `<vfx>` and
//! `<vfx_references>` side by side).
//!
//! The text is kept as bytes (see [`crate::text_encoding`]). Element and attribute names are read as UTF-8, lossily.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::text_encoding::{OddUtf16, TextEncoding, decode_text, encode_text};

mod inplace;
mod parse;
mod structural;

/// Errors of reading or editing a document.
#[derive(Debug, thiserror::Error)]
pub enum XmlError {
    #[error("xml: {what} at offset {offset}")]
    Syntax { offset: usize, what: String },

    #[error(transparent)]
    OddUtf16(#[from] OddUtf16),

    #[error("{0}")]
    Edit(String),
}

pub type Result<T> = std::result::Result<T, XmlError>;

/// An attribute of an element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlAttr {
    pub name: String,

    /// Span of the raw value text, without quotes.
    pub value_off: usize,
    pub value_len: usize,

    /// The quote around the value, `'` or `"`.
    pub quote: u8,

    /// Added by an edit: the raw value lives in the document and is inserted at the element's `attrs_end`.
    pub appended: bool,
}

/// An element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlElement {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub attrs: Vec<XmlAttr>,

    /// Offset of the `<`.
    pub start_off: usize,

    /// Offset of the `/>` or `>` that ends the start tag: where attributes are inserted.
    pub attrs_end: usize,

    /// Offset just after the start tag.
    pub start_end: usize,

    /// Offset just after the matching end tag (`start_end` if self-closing).
    pub end_off: usize,

    pub self_closing: bool,
}

/// One replacement of text: `removed` at `pos` became `inserted`. A list of them is an exact, small undo record of a structural edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XmlSplice {
    pub pos: usize,
    pub removed: Vec<u8>,
    pub inserted: Vec<u8>,
}

/// A pending replacement of an attribute value.
#[derive(Clone, Debug)]
struct Edit {
    len: usize,
    text: Vec<u8>,
}

/// A lossless XML document.
#[derive(Clone, Debug, Default)]
pub struct XmlDoc {
    text: Vec<u8>,
    origin: Arc<Vec<u8>>,
    structural: u32,
    encoding: TextEncoding,
    elems: Vec<XmlElement>,
    roots: Vec<usize>,
    journal: Vec<XmlSplice>,

    /// Value edits, keyed by the value's offset; spans never overlap.
    edits: BTreeMap<usize, Edit>,

    /// (element, attribute index) -> raw value of an added attribute.
    appended_values: BTreeMap<(usize, usize), Vec<u8>>,
}

/// Size from which [`XmlDoc::parse`] reads a text in pieces, side by side.
pub const PARALLEL_MIN: usize = 4 << 20;

impl XmlDoc {

    /// Reads a document, keeping the bytes as [`Self::origin_bytes`]. A big text is read in pieces side by side.
    ///
    /// # Errors
    ///
    /// Returns [`XmlError::Syntax`] with the offset of the problem, or [`XmlError::OddUtf16`].
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        Self::parse_with(bytes, PARALLEL_MIN)
    }

    /// [`Self::parse`], reading a text of `parallel_min` bytes or more in pieces (the same element table as reading it in one go;
    /// `usize::MAX` reads it in one go).
    ///
    /// # Errors
    ///
    /// Returns [`XmlError::Syntax`] with the offset of the problem, or [`XmlError::OddUtf16`].
    pub fn parse_with(bytes: Vec<u8>, parallel_min: usize) -> Result<Self> {
        let (text, encoding) = decode_text(&bytes)?;
        let (elems, roots) = parse::parse_text(&text, parallel_min)?;
        Ok(Self { text, origin: Arc::new(bytes), encoding, elems, roots, ..Default::default() })
    }

    /// The document as a file, with every edit applied.
    pub fn save(&self) -> Vec<u8> {
        encode_text(&self.baked_text(), self.encoding)
    }

    pub fn elements(&self) -> &[XmlElement] {
        &self.elems
    }

    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// The decoded text, without pending attribute edits.
    pub fn original_text(&self) -> &[u8] {
        &self.text
    }

    /// The file as first read, before any structural edit.
    pub fn origin_bytes(&self) -> &[u8] {
        &self.origin
    }

    /// Number of structural edits since the document was read.
    pub fn structural_edits(&self) -> u32 {
        self.structural
    }

    /// If the document differs from the file it was read from.
    pub fn modified(&self) -> bool {
        self.has_pending_edits() || self.structural > 0
    }

    /// If there are attribute edits not yet part of the text.
    pub fn has_pending_edits(&self) -> bool {
        !self.edits.is_empty() || !self.appended_values.is_empty()
    }

    /// The text with every attribute edit applied.
    fn baked_text(&self) -> Vec<u8> {
        if !self.has_pending_edits() {
            return self.text.clone();
        }

        // Value edits and attribute insertions go into one ordered list of span replacements.
        let mut ops = self.edits.clone();
        for (&(elem, attr), raw) in &self.appended_values {
            let element = &self.elems[elem];
            let op = ops.entry(element.attrs_end).or_insert(Edit { len: 0, text: vec![] });
            op.len = 0;
            op.text.extend_from_slice(&added_attr_text(&element.attrs[attr], raw));
        }

        let mut out = Vec::with_capacity(self.text.len() + 256);
        let mut pos = 0;
        for (&off, op) in &ops {
            out.extend_from_slice(&self.text[pos..off]);
            out.extend_from_slice(&op.text);
            pos = off + op.len;
        }

        out.extend_from_slice(&self.text[pos..]);
        out
    }

    /// The current raw value of an attribute (edits applied), still entity-encoded.
    pub fn attr_raw(&self, elem: usize, attr: usize) -> &[u8] {
        let at = &self.elems[elem].attrs[attr];
        if at.appended {
            return self.appended_values.get(&(elem, attr)).map_or(&[], |raw| raw.as_slice());
        }

        match self.edits.get(&at.value_off) {
            Some(edit) => &edit.text,
            None => &self.text[at.value_off..at.value_off + at.value_len],
        }
    }

    /// The raw value of an attribute as read (empty for attributes added by an edit).
    pub fn original_raw(&self, elem: usize, attr: usize) -> &[u8] {
        let at = &self.elems[elem].attrs[attr];
        if at.appended {
            return &[];
        }

        &self.text[at.value_off..at.value_off + at.value_len]
    }

    /// If an attribute was edited or added.
    pub fn attr_modified(&self, elem: usize, attr: usize) -> bool {
        self.attr_raw(elem, attr) != self.original_raw(elem, attr) || self.elems[elem].attrs[attr].appended
    }

    /// The index of an element's attribute by name.
    pub fn find_attr(&self, elem: usize, name: &str) -> Option<usize> {
        self.elems[elem].attrs.iter().position(|at| at.name == name)
    }

    /// The entity-decoded value of an element's attribute, read as UTF-8 (lossily).
    pub fn get(&self, elem: usize, name: &str) -> Option<String> {
        let attr = self.find_attr(elem, name)?;
        Some(String::from_utf8_lossy(&decode_entities(self.attr_raw(elem, attr))).into_owned())
    }

    /// Replaces the raw value text of an attribute. Setting the original text back removes the edit.
    pub fn set_attr_raw(&mut self, elem: usize, attr: usize, raw: Vec<u8>) {
        let at = &self.elems[elem].attrs[attr];
        if at.appended {
            self.appended_values.insert((elem, attr), raw);
        } else if self.text[at.value_off..at.value_off + at.value_len] == raw[..] {
            self.edits.remove(&at.value_off);
        } else {
            self.edits.insert(at.value_off, Edit { len: at.value_len, text: raw });
        }
    }

    /// Sets an attribute's value, entity-encoded. A missing attribute is added after the others, with the quote of the last one.
    pub fn set(&mut self, elem: usize, name: &str, value: &str) {
        if let Some(attr) = self.find_attr(elem, name) {
            let quote = self.elems[elem].attrs[attr].quote;
            self.set_attr_raw(elem, attr, encode_entities(value.as_bytes(), quote));
            return;
        }

        let element = &mut self.elems[elem];
        let quote = element.attrs.last().map_or(b'\'', |at| at.quote);
        self.appended_values.insert((elem, element.attrs.len()), encode_entities(value.as_bytes(), quote));
        element.attrs.push(XmlAttr { name: name.to_owned(), value_off: element.attrs_end, value_len: 0, quote, appended: true });
    }

    /// Drops every attribute edit. Structural edits stay: undoing those needs [`Self::parse`] of [`Self::origin_bytes`].
    pub fn revert_all(&mut self) {
        self.edits.clear();
        self.appended_values.clear();
        for element in &mut self.elems {
            while element.attrs.last().is_some_and(|at| at.appended) {
                element.attrs.pop();
            }
        }
    }

    /// The path of an element, like `SCENE/SCENE_NODE_LIST/WEATHER_SCENE_NODE/SKY_DOME/SKYGEN`.
    pub fn path_of(&self, elem: usize) -> String {
        let mut names = vec![];
        let mut current = Some(elem);
        while let Some(index) = current {
            names.push(self.elems[index].name.as_str());
            current = self.elems[index].parent;
        }

        names.reverse();
        names.join("/")
    }

    /// The number of elements of an element's subtree, itself included.
    pub fn subtree_size(&self, elem: usize) -> usize {
        let mut count = 0;
        let mut pending = vec![elem];
        while let Some(index) = pending.pop() {
            count += 1;
            pending.extend_from_slice(&self.elems[index].children);
        }

        count
    }
}

/// The text of an added attribute: ` name='raw'`.
fn added_attr_text(at: &XmlAttr, raw: &[u8]) -> Vec<u8> {
    [b" ", at.name.as_bytes(), b"=", &[at.quote], raw, &[at.quote]].concat()
}

/// Decodes the predefined entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`); any other is kept as it is.
pub fn decode_entities(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let byte = raw[i];
        let semi = if byte == b'&' { memchr::memchr(b';', &raw[i..]).map(|semi| i + semi) } else { None };
        let Some(semi) = semi else {
            out.push(byte);
            i += 1;
            continue;
        };

        match &raw[i + 1..semi] {
            b"amp" => out.push(b'&'),
            b"lt" => out.push(b'<'),
            b"gt" => out.push(b'>'),
            b"quot" => out.push(b'"'),
            b"apos" => out.push(b'\''),
            _ => out.extend_from_slice(&raw[i..=semi]),
        }

        i = semi + 1;
    }

    out
}

/// Encodes a value for an attribute quoted with `quote`: `&`, `<` and the quote become entities.
pub fn encode_entities(text: &[u8], quote: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &byte in text {
        match byte {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            _ if byte == quote => out.extend_from_slice(if quote == b'"' { b"&quot;" } else { b"&apos;" }),
            _ => out.push(byte),
        }
    }

    out
}
