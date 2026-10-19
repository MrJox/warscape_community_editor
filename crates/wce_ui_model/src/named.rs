//! The XML in the game's words.
//!
//! The converter writes every field of a component as a bare primitive (`<i>461</i><!-- x offset -->`) and the old guesses of the
//! format's reverse engineers as the only name. This module shows the same file with the names the editor's Fields panel uses
//! (`<offset_x>461</offset_x>`, `<visible>yes</visible>`, `<component>` for `<uientry>`), and turns such text back into the converter's
//! XML exactly. Nothing is stored in the named form: the model keeps the converter's XML, so the binary files stay byte for byte what
//! they were, and `to_raw` of anything `to_named` made gives back the same text (`named_check` tests that on every layout of the game).
//!
//! A field is renamed only when `named_table.rs` knows the name, the primitive and the converter's label together; any other field
//! (other versions, other file kinds) keeps its primitive tag and its comment, and passes through both ways untouched.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use crate::dom::{Dom, Element, Form, Node};
use crate::props::{self, Item};

include!("named_table.rs");

const LEAF: &[&str] = &["u", "i", "u2", "i2", "byte", "flt", "s", "unicode", "uuid", "yes", "no"];

/// Converter's structure tags and the names they are shown under.
const STRUCT: &[(&str, &str)] = &[
    ("uientry", "component"),
    ("images", "picture_files"),
    ("image", "picture_file"),
    ("image_uses", "picture_uses"),
    ("image_use", "picture_use"),
    ("mouse_states", "mouse_actions"),
    ("mouse_state", "mouse_action"),
];

/// Tags a field's name must not equal, or the text could not be told from the converter's own.
const RESERVED: &[&str] = &["ui", "states", "state", "children", "properties", "funcs", "color", "template", "images", "image"];

/// (primitive, label, first version, last version) of the fields a (parent tag, name) is used for.
type Table = HashMap<(&'static str, &'static str), Vec<(&'static str, &'static str, u32, u32)>>;

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let mut m: Table = HashMap::new();
        for &(parent, name, tag, label, lo, hi) in TABLE {
            m.entry((parent, name)).or_default().push((tag, label, lo, hi));
        }

        m
    })
}

fn shown_struct(raw: &str) -> &str {
    STRUCT.iter().find(|(r, _)| *r == raw).map(|(_, n)| *n).unwrap_or(raw)
}

fn raw_struct(shown: &str) -> &str {
    STRUCT.iter().find(|(_, n)| *n == shown).map(|(r, _)| *r).unwrap_or(shown)
}

fn snake(s: &str) -> String {
    let mut o = String::new();
    let mut sep = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if sep && !o.is_empty() {
                o.push('_');
            }

            sep = false;
            o.push(c.to_ascii_lowercase());
        } else {
            sep = true;
        }
    }

    if o.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        o.insert_str(0, "f_");
    }

    o
}

struct Field {
    idx: usize,
    stem: String,
    tag: String,
    /// The converter's label comment as written (with its spaces), "" if the field has none.
    label: String,
    /// Index of that comment among the element's children.
    label_node: Option<usize>,
}

/// The comment that names the field at `idx` (see `Element::label_of`): the last one after it that is not a note about its value.
fn label_node(el: &Element, idx: usize) -> Option<usize> {
    let mut found = None;
    for (k, n) in el.children.iter().enumerate().skip(idx + 1) {
        match n {
            Node::Comment { text, inline: true } => {
                let t = text.trim();
                let note = t.starts_with("ID (") || t.ends_with("degrees") || (t.len() == 11 && t.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
                if !note {
                    found = Some(k);
                }
            }
            _ => break,
        }
    }

    found
}

fn kind_of(el: &Element) -> &'static str {
    match el.name.as_str() {
        "uientry" => "component",
        "state" => "state",
        "image_use" => "picture",
        "mouse_state" => "mouse",
        _ => "other",
    }
}

fn stem_of(el: &Element, kind: &str, it: &Item, v: u32) -> Option<String> {
    if el.name == "color" {
        let n = match it.label.trim() {
            "B" => "blue",
            "G" => "green",
            "R" => "red",
            "A" => "alpha",
            _ => "",
        };

        if !n.is_empty() {
            return Some(n.into());
        }
    }

    let name = if props::bits_for(kind, it, v).is_some() {
        Some(if kind == "mouse" { "transition_fade_flags".to_string() } else { "state_flags".to_string() })
    } else {
        let def = match kind {
            "component" => props::component_def(it, v),
            "state" => props::state_def(it, v),
            "picture" => props::picture_def(it, v),
            "mouse" => props::mouse_def(it),
            _ => None,
        };

        match def {
            // several different unknowns each keep the converter's label as their name
            Some(df) if (df.name.starts_with("Unknown") || df.name.starts_with("Has ")) && !it.label.trim().is_empty() => Some(snake(it.label.trim().trim_end_matches('?'))),
            Some(df) => Some(snake(df.name)),
            None if !it.label.trim().is_empty() => Some(snake(it.label.trim().trim_end_matches('?'))),
            // the unlabelled id every component, state, picture and mouse action starts with (its bytes are noted beside it)
            None if it.tag == "u" && it.after.is_empty() && it.since == 0 => Some("id".to_string()),
            None => None,
        }
    };

    name.map(|n| if LEAF.contains(&n.as_str()) || RESERVED.contains(&n.as_str()) || STRUCT.iter().any(|(r, s)| *r == n || *s == n) { format!("{n}_value") } else { n })
}

fn fields(el: &Element, v: u32) -> Vec<Field> {
    let kind = kind_of(el);
    props::items(el)
        .into_iter()
        .filter_map(|it| {
            let stem = stem_of(el, kind, &it, v)?;
            let ln = label_node(el, it.idx);
            let label = match ln {
                Some(k) => match &el.children[k] {
                    Node::Comment { text, .. } => text.clone(),
                    _ => String::new(),
                },
                None => String::new(),
            };

            Some(Field { idx: it.idx, stem, tag: it.tag, label, label_node: ln })
        })
        .collect()
}

/// Every (parent tag, name, primitive, label) the file's fields are named with: the contents of `named_table.rs`.
pub fn collect(dom: &Dom, version: u32, out: &mut BTreeMap<(String, String, String, String), (u32, u32)>) {
    fn walk(el: &Element, v: u32, out: &mut BTreeMap<(String, String, String, String), (u32, u32)>) {
        for f in fields(el, v) {
            let r = out.entry((el.name.clone(), f.stem, f.tag, f.label)).or_insert((v, v));
            r.0 = r.0.min(v);
            r.1 = r.1.max(v);
        }

        for c in el.elems() {
            walk(c, v, out);
        }
    }

    walk(&dom.root, version, out);
}

/// How many primitives the name stands for in this version, if it stands for this field's.
fn known(parent: &str, f: &Field, v: u32) -> Option<usize> {
    let e = table().get(&(parent, f.stem.as_str()))?;
    let here: Vec<_> = e.iter().filter(|(_, _, lo, hi)| (*lo..=*hi).contains(&v)).collect();
    here.iter().any(|(t, l, _, _)| *t == f.tag && *l == f.label).then(|| here.iter().map(|(t, _, _, _)| if *t == "yes" || *t == "no" { "bool" } else { *t }).collect::<BTreeSet<_>>().len())
}

/// The converter's element (a component, the whole file) with the names of the game.
pub fn element_to_named(el: &Element, version: u32) -> Element {
    let fs = fields(el, version);
    let hits: HashMap<usize, (&Field, usize)> = fs.iter().filter_map(|f| known(&el.name, f, version).map(|n| (f.idx, (f, n)))).collect();
    let dropped: HashSet<usize> = hits.values().filter_map(|(f, _)| f.label_node).collect();
    let mut out = Element { name: shown_struct(&el.name).to_string(), attrs: el.attrs.clone(), children: vec![], form: el.form };
    let mut used: HashMap<String, usize> = HashMap::new();
    for (i, n) in el.children.iter().enumerate() {
        match n {
            Node::Comment { .. } if dropped.contains(&i) => {}
            Node::Elem(c) => match hits.get(&i) {
                Some((f, count)) => {
                    let k = used.entry(f.stem.clone()).or_insert(0);
                    *k += 1;
                    let name = if *k == 1 { f.stem.clone() } else { format!("{}_{}", f.stem, k) };
                    let attrs = if *count > 1 { vec![("type".to_string(), if f.tag == "yes" || f.tag == "no" { "bool".to_string() } else { f.tag.clone() })] } else { vec![] };
                    let (children, form) = if f.tag == "yes" || f.tag == "no" { (vec![Node::Text(f.tag.clone())], Form::Normal) } else { (c.children.clone(), c.form) };
                    out.children.push(Node::Elem(Element { name, attrs, children, form }));
                }
                None => out.children.push(Node::Elem(element_to_named(c, version))),
            },
            other => out.children.push(other.clone()),
        }
    }

    out
}

/// The file with the names of the game.
pub fn to_named(dom: &Dom, version: u32) -> Dom {
    Dom { root: element_to_named(&dom.root, version) }
}

fn lookup(parent: &str, name: &str, ty: Option<&str>, v: u32) -> Option<(&'static str, &'static str)> {
    let pick = |n: &str| table().get(&(parent, n)).and_then(|e| e.iter().find(|(t, _, lo, hi)| (*lo..=*hi).contains(&v) && ty.is_none_or(|y| y == *t || (y == "bool" && (*t == "yes" || *t == "no")))).map(|(t, l, _, _)| (*t, *l)));
    if let Some(r) = pick(name) {
        return Some(r);
    }

    // "name_2": the second field of that name in the element
    let (stem, num) = name.rsplit_once('_')?;
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    pick(stem)
}

/// Text of the named form (or of the converter's, which passes through) back as the converter's element.
pub fn element_to_raw(el: &Element, version: u32) -> Result<Element, String> {
    let raw_name = raw_struct(&el.name).to_string();
    let mut out = Element { name: raw_name.clone(), attrs: el.attrs.clone(), children: vec![], form: el.form };
    let mut k = 0;
    while k < el.children.len() {
        match &el.children[k] {
            Node::Elem(c) => {
                k += 1;
                match lookup(&raw_name, &c.name, c.attr("type"), version) {
                    Some((tag, label)) => {
                        let (children, form) = if tag == "yes" || tag == "no" {
                            let t = c.text();
                            let tag = match t.trim() {
                                "yes" => "yes",
                                "no" => "no",
                                other => return Err(format!("<{}> is yes or no, not {:?}", c.name, other)),
                            };

                            out.children.push(Node::Elem(Element { name: tag.into(), attrs: vec![], children: vec![], form: Form::SelfClosing { space: true } }));
                            (None, Form::Normal)
                        } else {
                            (Some(c.children.clone()), c.form)
                        };

                        if let Some(children) = children {
                            out.children.push(Node::Elem(Element { name: tag.into(), attrs: vec![], children, form }));
                        }

                        // its notes stay in front, the label goes last
                        while let Some(Node::Comment { text, inline: true }) = el.children.get(k) {
                            out.children.push(Node::Comment { text: text.clone(), inline: true });
                            k += 1;
                        }

                        if !label.is_empty() {
                            out.children.push(Node::Comment { text: label.to_string(), inline: true });
                        }
                    }
                    None => out.children.push(Node::Elem(element_to_raw(c, version)?)),
                }
            }
            other => {
                out.children.push(other.clone());
                k += 1;
            }
        }
    }

    Ok(out)
}

/// The converter's XML from text in either form.
pub fn to_raw(dom: &Dom) -> Result<Dom, String> {
    let version = dom.root.attr("version").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    Ok(Dom { root: element_to_raw(&dom.root, version)? })
}
