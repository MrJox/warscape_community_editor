//! The XML of a UI file as a tree that prints back to the same text.
//!
//! The converter's output has a fixed shape (two-space indent, one element per line, comments trailing a leaf on its
//! line), so printing a parsed tree reproduces the text byte for byte. Whitespace between elements is not stored;
//! it is regenerated on printing, which is also what lets elements move between depths.

use wce_ui_format::xml_escape;
use wce_ui_format::xml_lite::{Ev, line_col, parse};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Elem(Element),
    Text(String),
    /// `inline`: written on the line of the node before it.
    Comment { text: String, inline: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `<a>...</a>`
    Normal,
    /// `<a/>` or `<a />`
    SelfClosing { space: bool },
    /// `<a>` newline `</a>`, an array without items
    EmptyBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub form: Form,
}

impl Element {
    pub fn new(name: &str) -> Self {
        Self { name: name.into(), attrs: vec![], children: vec![], form: Form::Normal }
    }

    pub fn leaf(name: &str, text: &str) -> Self {
        Self { name: name.into(), attrs: vec![], children: vec![Node::Text(text.into())], form: Form::Normal }
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, key: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == key) {
            Some(a) => a.1 = value.to_string(),
            None => self.attrs.push((key.to_string(), value.to_string())),
        }
    }

    /// True for an element that holds text only (a primitive such as `<i>5</i>`, `<yes />`, `<s></s>`).
    pub fn is_leaf(&self) -> bool {
        self.children.iter().all(|n| matches!(n, Node::Text(_))) && self.form != Form::EmptyBlock
    }

    /// The text of a leaf.
    pub fn text(&self) -> String {
        self.children.iter().filter_map(|n| if let Node::Text(t) = n { Some(t.as_str()) } else { None }).collect()
    }

    pub fn set_text(&mut self, text: &str) {
        self.children = vec![Node::Text(text.to_string())];
        self.form = Form::Normal;
    }

    pub fn elems(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| if let Node::Elem(e) = n { Some(e) } else { None })
    }

    /// Index in `children` of the first element called `name`.
    pub fn find_idx(&self, name: &str) -> Option<usize> {
        self.children.iter().position(|n| matches!(n, Node::Elem(e) if e.name == name))
    }

    pub fn find(&self, name: &str) -> Option<&Element> {
        self.elems().find(|e| e.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|n| match n {
            Node::Elem(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// Indices in `children` of the direct elements called `name`.
    pub fn idxs(&self, name: &str) -> Vec<usize> {
        self.children.iter().enumerate().filter(|(_, n)| matches!(n, Node::Elem(e) if e.name == name)).map(|(i, _)| i).collect()
    }

    pub fn elem_at(&self, idx: usize) -> Option<&Element> {
        match self.children.get(idx) {
            Some(Node::Elem(e)) => Some(e),
            _ => None,
        }
    }

    pub fn elem_at_mut(&mut self, idx: usize) -> Option<&mut Element> {
        match self.children.get_mut(idx) {
            Some(Node::Elem(e)) => Some(e),
            _ => None,
        }
    }

    /// Adds a child element (dropping the placeholder text of an empty block).
    pub fn push_elem(&mut self, e: Element) {
        self.children.retain(|n| !matches!(n, Node::Text(_)));
        self.form = Form::Normal;
        self.children.push(Node::Elem(e));
    }

    /// The label a field carries: the last comment after it that names it (not an ID, hex or degree note).
    pub fn label_of(&self, idx: usize) -> Option<&str> {
        let mut label = None;
        for n in &self.children[idx + 1..] {
            match n {
                Node::Comment { text, inline: true } => {
                    let t = text.trim();
                    let note = t.starts_with("ID (") || t.ends_with("degrees") || (t.len() == 11 && t.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
                    if !note {
                        label = Some(t);
                    }
                }
                _ => break,
            }
        }
        label
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dom {
    pub root: Element,
}

fn dom_escape(s: &str) -> String {
    xml_escape(s).replace('\u{e01f}', "&#xE01F;")
}

fn tag_end(src: &str, start: usize) -> (usize, bool, bool) {
    // (offset of `>`, self closing, space before the slash)
    let b = src.as_bytes();
    let mut i = start;
    let mut q = 0u8;
    while i < b.len() {
        let c = b[i];
        if q != 0 {
            if c == q {
                q = 0;
            }
        } else if c == b'"' || c == b'\'' {
            q = c;
        } else if c == b'>' {
            let sc = i > start && b[i - 1] == b'/';
            let space = sc && i > start + 1 && b[i - 2].is_ascii_whitespace();
            return (i, sc, space);
        }
        i += 1;
    }
    (b.len(), false, false)
}

impl Dom {
    pub fn parse(src: &str) -> Result<Dom, String> {
        let evs = parse(src).map_err(|e| {
            let (l, c) = line_col(src, e.offset);
            format!("line {}, column {}: {}", l, c, e.msg)
        })?;
        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;
        for (off, ev) in evs {
            match ev {
                Ev::Start(name, attrs) => {
                    let (_, sc, space) = tag_end(src, off);
                    let form = if sc { Form::SelfClosing { space } } else { Form::Normal };
                    stack.push(Element { name, attrs, children: vec![], form });
                }
                Ev::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.children.push(Node::Text(t));
                    }
                }
                Ev::Comment(text) => {
                    if let Some(top) = stack.last_mut() {
                        let inline = match top.children.last() {
                            None => false,
                            Some(Node::Text(t)) => !t.contains('\n') && (!t.trim().is_empty() || top.children.len() > 1),
                            Some(_) => true,
                        };
                        top.children.push(Node::Comment { text, inline });
                    }
                }
                Ev::End(_) => {
                    let mut e = stack.pop().unwrap();
                    let has_struct = e.children.iter().any(|n| !matches!(n, Node::Text(_)));
                    if has_struct {
                        e.children.retain(|n| match n {
                            Node::Text(t) => !t.trim().is_empty(),
                            _ => true,
                        });
                    } else if !e.children.is_empty() {
                        let all: String = e.children.iter().map(|n| if let Node::Text(t) = n { t.as_str() } else { "" }).collect();
                        if all.trim().is_empty() && all.contains('\n') {
                            e.children.clear();
                            e.form = Form::EmptyBlock;
                        }
                    }
                    match stack.last_mut() {
                        Some(p) => p.children.push(Node::Elem(e)),
                        None => root = Some(e),
                    }
                }
            }
        }
        root.map(|root| Dom { root }).ok_or_else(|| "no root element".to_string())
    }

    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        write_elem(&self.root, 0, &mut out);
        out.push('\n');
        out
    }

    pub fn at(&self, path: &[usize]) -> Option<&Element> {
        let mut e = &self.root;
        for &i in path {
            e = e.elem_at(i)?;
        }
        Some(e)
    }

    pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Element> {
        let mut e = &mut self.root;
        for &i in path {
            e = e.elem_at_mut(i)?;
        }
        Some(e)
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn open_tag(e: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        out.push_str(&format!(" {}=\"{}\"", k, dom_escape(v)));
    }
}

fn write_elem(e: &Element, depth: usize, out: &mut String) {
    indent(depth, out);
    open_tag(e, out);
    if let Form::SelfClosing { space } = e.form {
        if e.children.is_empty() {
            out.push_str(if space { " />" } else { "/>" });
            return;
        }
    }
    out.push('>');
    if e.form == Form::EmptyBlock && e.children.is_empty() {
        out.push('\n');
        indent(depth, out);
    } else if e.is_leaf() {
        for n in &e.children {
            if let Node::Text(t) = n {
                out.push_str(&dom_escape(t));
            }
        }
    } else {
        for n in &e.children {
            match n {
                Node::Elem(c) => {
                    out.push('\n');
                    write_elem(c, depth + 1, out);
                }
                Node::Comment { text, inline } => {
                    if !*inline {
                        out.push('\n');
                        indent(depth + 1, out);
                    }
                    out.push_str("<!--");
                    out.push_str(text);
                    out.push_str("-->");
                }
                Node::Text(_) => {}
            }
        }
        out.push('\n');
        indent(depth, out);
    }
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}
