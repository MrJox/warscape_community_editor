//! The XML of TEd's family of files (tile.ted, river splines, tile database entries) as a typed node tree, and back.
//!
//! These files are written by one serialiser each: one element per line, a fixed indent per level, attributes on the element's line in
//! the serialiser's order, leaves self-closed, no text, no comments. A document keeps its style ([`DocStyle`]: the bytes before and after
//! the root, the line end, the indent, the quote) and the writer predicts every byte between tokens from it; where a file differs from the
//! prediction the node keeps the actual bytes as layout trivia, so any file of this shape is written back byte for byte. Values are typed by
//! a [`Schema`]; a value whose text the schema's kind would not give back keeps its text as verbatim trivia.
//!
//! Not supported (a read error): text other than whitespace, comments, processing instructions or CDATA after the prolog, `=` with spaces
//! around it, entities other than the five XML ones and character references.

pub mod values;

use std::collections::HashMap;

use thiserror::Error;
use wce_tile_model::{Node, Sym, SymCache, Value};

use self::values::{Kind, Schema};

#[derive(Debug, Error)]
#[error("xml: {what} at byte {offset}")]
pub struct XmlError {
    pub offset: usize,
    pub what: String,
}

fn err<T>(offset: usize, what: impl Into<String>) -> Result<T, XmlError> {
    Err(XmlError { offset, what: what.into() })
}

/// The style of a document: what the writer needs besides the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocStyle {
    /// Everything before the root's `<` (a BOM, an XML declaration and its line end), and everything after the root's end.
    pub prolog: Vec<u8>,
    pub epilog: Vec<u8>,
    pub newline: Vec<u8>,

    /// One level of indentation.
    pub indent: Vec<u8>,

    /// `"` or `'`.
    pub quote: u8,
}

impl DocStyle {

    /// TEd's tile.ted style: CRLF, four spaces, double quotes, a CRLF after the root.
    pub fn ted() -> Self {
        Self { prolog: vec![], epilog: b"\r\n".to_vec(), newline: b"\r\n".to_vec(), indent: b"    ".to_vec(), quote: b'"' }
    }
}

/// Layout trivia position codes (see [`wce_tile_model::Trivia::layout`]).
pub mod layout {
    /// The bytes before the element's `<` (predicted: line end + indent).
    pub const BEFORE_START: u32 = 0;

    /// The bytes before `</` (predicted: line end + indent).
    pub const BEFORE_END: u32 = 1;

    /// The bytes before `>` or `/>` (predicted: none).
    pub const BEFORE_CLOSE: u32 = 2;

    /// An element without children written with an end tag: the bytes between `>` and `</` (predicted: self-closed).
    pub const EMPTY_PAIR: u32 = 3;

    /// + i: the bytes before attribute i (predicted: one space).
    pub const ATTR_GAP: u32 = 0x100;

    /// + i: the quote of attribute i, when it is not the document's.
    pub const ATTR_QUOTE: u32 = 0x1_0000;
}

/// What reading a document found that a gate wants to count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReadStats {
    pub elements: usize,
    pub props: usize,

    /// Values whose text the kind would not give back (kept as verbatim text), by (class, attribute).
    pub verbatim: HashMap<(Sym, Sym), usize>,

    /// Attributes the schema does not know (read as text), by (class, attribute).
    pub unknown: HashMap<(Sym, Sym), usize>,

    /// Nodes that carry layout trivia.
    pub layout_nodes: usize,
}

fn decode_entities(raw: &str, at: usize) -> Result<String, XmlError> {
    if !raw.contains('&') {
        return Ok(raw.to_owned());
    }

    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let Some(end) = rest[i..].find(';') else { return err(at, "unterminated entity") };
        let name = &rest[i + 1..i + end];
        let c = match name {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(h) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(h, 16).ok()
                } else {
                    name.strip_prefix('#').and_then(|d| d.parse().ok())
                };

                match code.and_then(char::from_u32) {
                    Some(c) => c,
                    None => return err(at, format!("entity &{name};")),
                }
            }
        };

        out.push(c);
        rest = &rest[i + end + 1..];
    }

    out.push_str(rest);
    Ok(out)
}

/// The text with `&`, `<`, `>` and the quote escaped.
fn encode_entities(text: &str, quote: u8, out: &mut Vec<u8>) {
    for b in text.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' if quote == b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' if quote == b'\'' => out.extend_from_slice(b"&apos;"),
            _ => out.push(b),
        }
    }
}

/// Kinds of (class, attribute) looked up once per document.
struct Kinds<'a> {
    schema: &'a dyn Schema,
    cache: HashMap<(Sym, Sym), Option<Kind>>,
}

impl Kinds<'_> {
    fn get(&mut self, class: Sym, attr: Sym) -> Option<Kind> {
        *self.cache.entry((class, attr)).or_insert_with(|| self.schema.kind(class, attr))
    }
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn is_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':') || b >= 0x80
}

struct Reader<'a> {
    t: &'a [u8],
    pos: usize,
    style: DocStyle,
    kinds: Kinds<'a>,
    syms: SymCache,
    stats: ReadStats,
    text: String,
}

impl Reader<'_> {
    fn name(&mut self) -> Result<Sym, XmlError> {
        let start = self.pos;
        while self.pos < self.t.len() && is_name(self.t[self.pos]) {
            self.pos += 1;
        }

        if start == self.pos {
            return err(start, "expected a name");
        }

        let Ok(name) = std::str::from_utf8(&self.t[start..self.pos]) else { return err(start, "name is not UTF-8") };
        Ok(self.syms.get(name))
    }

    fn spaces(&mut self) -> &[u8] {
        let start = self.pos;
        while self.pos < self.t.len() && is_space(self.t[self.pos]) {
            self.pos += 1;
        }

        &self.t[start..self.pos]
    }

    /// The predicted bytes between tokens at `depth`: line end + indent.
    fn predicted(&self, depth: usize, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(&self.style.newline);
        for _ in 0..depth {
            out.extend_from_slice(&self.style.indent);
        }
    }

    /// Reads the element whose `<` is at `self.pos`. `gap` is what came before it (checked against the prediction by the caller).
    fn element(&mut self, depth: usize) -> Result<Node, XmlError> {
        let start = self.pos;
        self.pos += 1;
        let class = self.name()?;
        let mut node = Node::new(class);
        self.stats.elements += 1;
        let mut i = 0u32;
        let mut layout: Vec<(u32, Box<[u8]>)> = vec![];
        let self_closing = loop {
            let gap_start = self.pos;
            self.spaces();
            let gap = &self.t[gap_start..self.pos];
            match self.t.get(self.pos) {
                Some(b'/') if self.t.get(self.pos + 1) == Some(&b'>') => {
                    if !gap.is_empty() {
                        layout.push((layout::BEFORE_CLOSE, gap.into()));
                    }

                    self.pos += 2;
                    break true;
                }

                Some(b'>') => {
                    if !gap.is_empty() {
                        layout.push((layout::BEFORE_CLOSE, gap.into()));
                    }

                    self.pos += 1;
                    break false;
                }

                Some(_) => {
                    if gap.is_empty() {
                        return err(self.pos, "attributes without a space between them");
                    }

                    if gap != b" " {
                        layout.push((layout::ATTR_GAP + i, gap.into()));
                    }

                    let attr = self.name()?;
                    if self.t.get(self.pos) != Some(&b'=') {
                        return err(self.pos, "expected = right after the attribute name");
                    }

                    let quote = self.t.get(self.pos + 1).copied().unwrap_or(0);
                    if quote != b'"' && quote != b'\'' {
                        return err(self.pos + 1, "expected a quote");
                    }

                    if quote != self.style.quote {
                        layout.push((layout::ATTR_QUOTE + i, Box::new([quote])));
                    }

                    let vstart = self.pos + 2;
                    let Some(len) = memchr(quote, &self.t[vstart..]) else { return err(vstart, "unterminated value") };
                    let Ok(raw) = std::str::from_utf8(&self.t[vstart..vstart + len]) else { return err(vstart, "value is not UTF-8") };
                    self.pos = vstart + len + 1;
                    let decoded = decode_entities(raw, vstart)?;
                    let kind = self.kinds.get(class, attr);
                    if kind.is_none() {
                        *self.stats.unknown.entry((class, attr)).or_default() += 1;
                    }

                    let kind = kind.unwrap_or(Kind::Str);
                    let value = kind.parse(&decoded, &mut self.syms);
                    self.text.clear();
                    let mut written = std::mem::take(&mut self.text).into_bytes();
                    encode_entities(&kind.format(&value), quote, &mut written);
                    let verbatim = if written == raw.as_bytes() {
                        None
                    } else {
                        *self.stats.verbatim.entry((class, attr)).or_default() += 1;
                        Some(raw.into())
                    };

                    self.text = String::from_utf8(written).unwrap_or_default();
                    node.push_read(attr, value, verbatim);
                    self.stats.props += 1;
                    i += 1;
                }

                None => return err(start, "unterminated start tag"),
            }
        };

        if !self_closing {
            let mut predicted = vec![];
            loop {
                let gap_start = self.pos;
                self.spaces();
                let gap_end = self.pos;
                if self.pos >= self.t.len() {
                    return err(start, "element not closed");
                }

                if self.t[self.pos] != b'<' {
                    return err(self.pos, "text in an element");
                }

                if self.t.get(self.pos + 1) == Some(&b'/') {
                    self.pos += 2;
                    let end = self.name()?;
                    if end != class {
                        return err(self.pos, format!("</{end}> closes <{class}>"));
                    }

                    if self.t.get(self.pos) != Some(&b'>') {
                        return err(self.pos, "expected >");
                    }

                    self.pos += 1;
                    let gap = &self.t[gap_start..gap_end];
                    if node.children.is_empty() {
                        layout.push((layout::EMPTY_PAIR, gap.into()));
                    } else {
                        self.predicted(depth, &mut predicted);
                        if gap != predicted.as_slice() {
                            layout.push((layout::BEFORE_END, gap.into()));
                        }
                    }

                    break;
                }

                if matches!(self.t.get(self.pos + 1), Some(b'!' | b'?')) {
                    return err(self.pos, "comment, CDATA or processing instruction");
                }

                let gap = self.t[gap_start..gap_end].to_vec();
                let mut child = self.element(depth + 1)?;
                self.predicted(depth + 1, &mut predicted);
                if gap != predicted {
                    child.trivia_mut().layout.insert(0, (layout::BEFORE_START, gap.into()));
                }

                if child.trivia().is_some_and(|t| !t.layout.is_empty()) {
                    self.stats.layout_nodes += 1;
                }

                node.children.push(child);
            }
        }

        if !layout.is_empty() {
            node.trivia_mut().layout.extend(layout);
        }

        Ok(node)
    }
}

fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// The style the document's bytes suggest; the parts the root does not show are TEd's.
fn detect_style(t: &[u8], root_start: usize) -> DocStyle {
    let mut style = DocStyle::ted();
    let body = &t[root_start..];
    if let Some(nl) = memchr(b'\n', body) {
        style.newline = if nl > 0 && body[nl - 1] == b'\r' { b"\r\n".to_vec() } else { b"\n".to_vec() };

        // The first line that starts with whitespace and then `<` gives one level of indentation.
        let after = &body[nl + 1..];
        let n = after.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        if n > 0 && after.get(n) == Some(&b'<') && after.get(n + 1) != Some(&b'/') {
            style.indent = after[..n].to_vec();
        }
    }

    // The root's first attribute's quote.
    let tag_end = memchr(b'>', body).unwrap_or(body.len());
    if let Some(q) = body[..tag_end].iter().find(|&&b| b == b'"' || b == b'\'') {
        style.quote = *q;
    }

    style
}

/// Reads a document: its root node, its style and what the gate counts.
///
/// # Errors
///
/// The text is not of the supported shape (see the module docs).
pub fn read(bytes: &[u8], schema: &dyn Schema) -> Result<(Node, DocStyle, ReadStats), XmlError> {
    // The prolog: a BOM, an XML declaration, comments, whitespace: everything before the first element.
    let mut p = 0;
    loop {
        while p < bytes.len() && (is_space(bytes[p]) || bytes[p..].starts_with(&[0xEF, 0xBB, 0xBF])) {
            p += if bytes[p] == 0xEF { 3 } else { 1 };
        }

        if bytes[p..].starts_with(b"<?") {
            let Some(e) = bytes[p..].windows(2).position(|w| w == b"?>") else { return err(p, "unterminated declaration") };
            p += e + 2;
        } else if bytes[p..].starts_with(b"<!--") {
            let Some(e) = bytes[p..].windows(3).position(|w| w == b"-->") else { return err(p, "unterminated comment") };
            p += e + 3;
        } else {
            break;
        }
    }

    if bytes.get(p) != Some(&b'<') {
        return err(p, "no root element");
    }

    let mut style = detect_style(bytes, p);
    style.prolog = bytes[..p].to_vec();
    let mut r = Reader { t: bytes, pos: p, style, kinds: Kinds { schema, cache: HashMap::new() }, syms: SymCache::default(), stats: ReadStats::default(), text: String::new() };
    let root = r.element(0)?;
    if root.trivia().is_some_and(|t| !t.layout.is_empty()) {
        r.stats.layout_nodes += 1;
    }

    let mut style = r.style;
    style.epilog = bytes[r.pos..].to_vec();
    if !style.epilog.iter().all(|&b| is_space(b)) {
        return err(r.pos, "something after the root element");
    }

    Ok((root, style, r.stats))
}

struct Writer<'a> {
    out: Vec<u8>,
    style: &'a DocStyle,
    kinds: Kinds<'a>,
}

impl Writer<'_> {
    fn newline(&mut self, depth: usize) {
        self.out.extend_from_slice(&self.style.newline);
        for _ in 0..depth {
            self.out.extend_from_slice(&self.style.indent);
        }
    }

    fn element(&mut self, node: &Node, depth: usize) {
        let layout = node.trivia().map_or(&[][..], |t| &t.layout[..]);
        let find = |code: u32| layout.iter().find(|(c, _)| *c == code).map(|(_, b)| &**b);
        self.out.push(b'<');
        self.out.extend_from_slice(node.class.as_str().as_bytes());
        for (i, p) in node.props().iter().enumerate() {
            let i = i as u32;
            match find(layout::ATTR_GAP + i) {
                Some(g) => self.out.extend_from_slice(g),
                None => self.out.push(b' '),
            }

            let quote = find(layout::ATTR_QUOTE + i).map_or(self.style.quote, |q| q[0]);
            self.out.extend_from_slice(p.name.as_str().as_bytes());
            self.out.push(b'=');
            self.out.push(quote);
            match node.verbatim(p.name) {
                Some(text) => self.out.extend_from_slice(text.as_bytes()),
                None => {
                    let kind = self.kinds.get(node.class, p.name).unwrap_or(Kind::Str);
                    encode_entities(&kind.format(&p.value), quote, &mut self.out);
                }
            }

            self.out.push(quote);
        }

        if let Some(g) = find(layout::BEFORE_CLOSE) {
            self.out.extend_from_slice(g);
        }

        if node.children.is_empty() {
            match find(layout::EMPTY_PAIR) {
                Some(inner) => {
                    self.out.push(b'>');
                    self.out.extend_from_slice(inner);
                    self.end_tag(node);
                }

                None => self.out.extend_from_slice(b"/>"),
            }

            return;
        }

        self.out.push(b'>');
        for c in &node.children {
            match c.trivia().and_then(|t| t.layout.iter().find(|(code, _)| *code == layout::BEFORE_START)) {
                Some((_, g)) => self.out.extend_from_slice(g),
                None => self.newline(depth + 1),
            }

            self.element(c, depth + 1);
        }

        match find(layout::BEFORE_END) {
            Some(g) => self.out.extend_from_slice(g),
            None => self.newline(depth),
        }

        self.end_tag(node);
    }

    fn end_tag(&mut self, node: &Node) {
        self.out.extend_from_slice(b"</");
        self.out.extend_from_slice(node.class.as_str().as_bytes());
        self.out.push(b'>');
    }
}

/// Writes a document.
pub fn write(root: &Node, style: &DocStyle, schema: &dyn Schema) -> Vec<u8> {
    let mut w = Writer { out: Vec::with_capacity(1 << 16), style, kinds: Kinds { schema, cache: HashMap::new() } };
    w.out.extend_from_slice(&style.prolog);
    w.element(root, 0);
    w.out.extend_from_slice(&style.epilog);
    w.out
}

/// A value's text as a schema writes it (for tools and tests).
pub fn value_text(schema: &dyn Schema, class: Sym, attr: Sym, value: &Value) -> String {
    schema.kind(class, attr).unwrap_or(Kind::Str).format(value)
}

#[cfg(test)]
mod tests {
    use super::values::Num;
    use super::*;

    struct Plain;

    impl Schema for Plain {
        fn kind(&self, _: Sym, attr: Sym) -> Option<Kind> {
            Some(match attr.as_str() {
                "position" => Kind::Vec3(Num::Ted, "vector"),
                "flags" => Kind::Flags { leading_space: true },
                "n" => Kind::Int,
                _ => return None,
            })
        }
    }

    fn round(text: &str) -> (Node, ReadStats) {
        let (root, style, stats) = read(text.as_bytes(), &Plain).unwrap();
        assert_eq!(String::from_utf8(write(&root, &style, &Plain)).unwrap(), text);
        (root, stats)
    }

    #[test]
    fn ted_shape_needs_no_trivia() {
        let text = "<project version=\"16\">\r\n    <scene>\r\n        <A flags=\" OIF_VISIBLE\" position=\"vector(1,2.5,-3)\" n=\"7\"/>\r\n    </scene>\r\n</project>\r\n";
        let (root, stats) = round(text);
        assert_eq!(stats.layout_nodes, 0);
        assert!(stats.verbatim.is_empty());
        assert_eq!(stats.unknown.len(), 1);
        let a = &root.children[0].children[0];
        assert_eq!(a.get("position"), Some(&Value::Vec3([1.0, 2.5, -3.0])));
        assert!(a.get("flags").unwrap().has_flag("OIF_VISIBLE"));
    }

    #[test]
    fn odd_layout_and_values_survive() {
        let text = "\u{feff}<?xml version='1.0'?>\n<r a='x &amp; y'  n=\"07\">\n\t<b position='vector(1.0000001,0,0)' ></b>\n\t<c>\n\t  <d/>\n</c>\n</r>";
        let (root, stats) = round(text);
        assert_eq!(root.get("a"), Some(&Value::Str("x & y".into())));
        assert_eq!(stats.verbatim.len(), 2);
        assert!(stats.layout_nodes >= 2);
    }

    #[test]
    fn an_edit_writes_the_value_not_the_old_text() {
        let text = "<r n=\"07\"/>\n";
        let (mut root, style, _) = read(text.as_bytes(), &Plain).unwrap();
        root.set("n", Value::Int(8));
        assert_eq!(write(&root, &style, &Plain), b"<r n=\"8\"/>\n");
    }

    #[test]
    fn unsupported_shapes_are_errors() {
        for bad in ["<r>text</r>", "<r><!-- c --></r>", "<r a = \"1\"/>", "<r></s>", "<r/>junk", "<r a=\"&bogus;\"/>"] {
            assert!(read(bad.as_bytes(), &Plain).is_err(), "{bad}");
        }
    }
}
