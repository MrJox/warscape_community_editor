//! Prefab references beyond filling them in (`project_files`): finding the groups that are exact copies of a prefab (so that they can be
//! made references), and a reference's **overrides**: the values its contents have that its prefab's do not.
//!
//! An override is an attribute of one of the contents' elements, found by its path (the element indices from the top of the contents, each
//! among its parent's elements) and checked by its element name. They are what the contents differ by from the prefab's, found when the
//! tile is saved (`overrides`), so any edit of a reference's contents is one; tile.wce keeps them in the reference, after its empty
//! `children`, and filling the reference in applies them (`apply`):
//!
//! ```text
//! <UserInstanceGroup wce_prefab="rocks" ...>
//!     <children/>
//!     <WceOverrides>
//!         <WceOverride path="1" element="BUILDING_INSTANCE" attr="scale" value="1.2"/>
//!     </WceOverrides>
//! </UserInstanceGroup>
//! ```
//!
//! Contents that differ by more than values (an element added, taken away or of another kind) are not a reference's: such a reference keeps
//! its own contents in the file.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use wce_formats::xml_doc::{XmlDoc, decode_entities, encode_entities};

use crate::ted_scene::TedScene;

/// The element that holds a reference's overrides in tile.wce, and the one of each.
pub const OVERRIDES_ELEMENT: &str = "WceOverrides";
pub const OVERRIDE_ELEMENT: &str = "WceOverride";

/// One overridden value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Override {
    /// The element: its index among the contents' top elements, then among each parent's elements.
    pub path: Vec<usize>,
    pub element: String,
    pub attr: String,

    /// The value (decoded).
    pub value: String,
}

/// Reads a contents text (elements at the top, at any column) as one document under a root element.
fn wrapped(children: &[u8]) -> Result<XmlDoc, String> {
    let mut text = b"<wce_contents>\n".to_vec();
    text.extend_from_slice(children);
    text.extend_from_slice(b"\n</wce_contents>\n");
    XmlDoc::parse(text).map_err(|e| e.to_string())
}

/// An element's path below the wrapping root.
fn path_of(doc: &XmlDoc, mut e: usize) -> Vec<usize> {
    let els = doc.elements();
    let mut path = vec![];
    while let Some(p) = els[e].parent {
        path.push(els[p].children.iter().position(|&c| c == e).unwrap_or(0));
        e = p;
    }

    path.reverse();
    path
}

/// The element at a path below the wrapping root.
fn at_path(doc: &XmlDoc, path: &[usize]) -> Option<usize> {
    let mut e = *doc.roots().first()?;
    for &i in path {
        e = *doc.elements()[e].children.get(i)?;
    }

    Some(e)
}

/// What a reference's contents (`current`) differ by from its prefab's (`base`): the attributes with other values or not in the prefab's,
/// in document order.
///
/// # Errors
///
/// A text does not read, or they differ by more than values (says where).
pub fn overrides(current: &[u8], base: &[u8]) -> Result<Vec<Override>, String> {
    let (cur, base) = (wrapped(current)?, wrapped(base)?);
    let (ce, be) = (cur.elements(), base.elements());
    let mut out = vec![];
    let mut stack = vec![(*cur.roots().first().ok_or("no contents")?, *base.roots().first().ok_or("no contents")?)];
    while let Some((c, b)) = stack.pop() {
        if ce[c].name != be[b].name || ce[c].children.len() != be[b].children.len() {
            let path = path_of(&cur, c).iter().map(usize::to_string).collect::<Vec<_>>().join(".");
            return Err(format!("the contents differ at {} ({} instead of {})", if path.is_empty() { "the top".to_owned() } else { path }, ce[c].name, be[b].name));
        }

        for (i, a) in ce[c].attrs.iter().enumerate() {
            let raw = cur.attr_raw(c, i);
            if base.find_attr(b, &a.name).is_none_or(|j| base.attr_raw(b, j) != raw) {
                out.push(Override { path: path_of(&cur, c), element: ce[c].name.clone(), attr: a.name.clone(), value: String::from_utf8_lossy(&decode_entities(raw)).into_owned() });
            }
        }

        // In document order: the last child first on the stack.
        stack.extend(ce[c].children.iter().copied().zip(be[b].children.iter().copied()).rev());
    }

    Ok(out)
}

/// The contents with the overrides applied. Returns them and the overrides that do not fit (no element at the path, or another kind):
/// those are left out.
///
/// # Errors
///
/// The contents do not read.
pub fn apply(children: &[u8], ovs: &[Override]) -> Result<(Vec<u8>, Vec<Override>), String> {
    if ovs.is_empty() {
        return Ok((children.to_vec(), vec![]));
    }

    let mut doc = wrapped(children)?;
    let mut lost = vec![];
    for o in ovs {
        match at_path(&doc, &o.path).filter(|&e| doc.elements()[e].name == o.element) {
            Some(e) if !o.path.is_empty() => doc.set(e, &o.attr, &o.value),
            _ => lost.push(o.clone()),
        }
    }

    // The root's lines off again.
    let text = doc.save();
    let start = b"<wce_contents>\n".len();
    let end = text.len() - b"\n</wce_contents>\n".len();
    Ok((text[start..end].to_vec(), lost))
}

/// The `WceOverrides` element of some overrides (at column 0, `\n` line ends, four spaces a level).
pub fn overrides_text(ovs: &[Override]) -> Vec<u8> {
    let mut out = format!("<{OVERRIDES_ELEMENT}>\n").into_bytes();
    for o in ovs {
        let path = o.path.iter().map(usize::to_string).collect::<Vec<_>>().join(".");
        let value = encode_entities(o.value.as_bytes(), b'"');
        out.extend_from_slice(format!("    <{OVERRIDE_ELEMENT} path=\"{path}\" element=\"{}\" attr=\"{}\" value=\"", o.element, o.attr).as_bytes());
        out.extend_from_slice(&value);
        out.extend_from_slice(b"\"/>\n");
    }

    out.extend_from_slice(format!("</{OVERRIDES_ELEMENT}>").as_bytes());
    out
}

/// The overrides an element holds (a reference's `WceOverrides`), those that read.
pub fn read_overrides(doc: &XmlDoc, reference: usize) -> Vec<Override> {
    let els = doc.elements();
    let Some(&holder) = els[reference].children.iter().find(|&&c| els[c].name == OVERRIDES_ELEMENT) else { return vec![] };
    els[holder].children.iter().filter(|&&c| els[c].name == OVERRIDE_ELEMENT).filter_map(|&c| {
        let path = doc.get(c, "path")?;
        let path = if path.is_empty() { vec![] } else { path.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<usize>>>()? };
        Some(Override { path, element: doc.get(c, "element")?, attr: doc.get(c, "attr")?, value: doc.get(c, "value")? })
    }).collect()
}

// ---- exact copies

/// The hashes of elements: their name, their attributes (names and values, in order) and their elements, whatever the spacing between.
fn element_hashes(doc: &XmlDoc) -> Vec<u64> {
    let els = doc.elements();
    let mut out = vec![0u64; els.len()];

    // Children come after their parent in the table: from the last one, every child is hashed before its parent.
    for e in (0..els.len()).rev() {
        let mut h = DefaultHasher::new();
        els[e].name.hash(&mut h);
        for (i, a) in els[e].attrs.iter().enumerate() {
            a.name.hash(&mut h);
            doc.attr_raw(e, i).hash(&mut h);
        }

        for &c in &els[e].children {
            out[c].hash(&mut h);
        }

        out[e] = h.finish();
    }

    out
}

/// The hash of a list of elements (a group's contents), from theirs.
fn list_hash(hashes: impl Iterator<Item = u64>) -> u64 {
    let mut h = DefaultHasher::new();
    for x in hashes {
        x.hash(&mut h);
    }

    h.finish()
}

/// The prefabs by the hash of their contents (the keys of the prefabs with those very contents), to find a group's prefab by its own.
#[derive(Clone, Debug, Default)]
pub struct ContentIndex {
    by_hash: HashMap<u64, Vec<String>>,
}

impl ContentIndex {
    /// From each prefab's key and contents (as a copy holds them: the references in them filled in). Empty contents are left out.
    pub fn new<'a>(prefabs: impl IntoIterator<Item = (&'a str, Vec<u8>)>) -> Self {
        let mut by_hash: HashMap<u64, Vec<String>> = HashMap::new();
        for (key, contents) in prefabs {
            let Ok(doc) = wrapped(&contents) else { continue };
            let Some(&root) = doc.roots().first() else { continue };
            if doc.elements()[root].children.is_empty() {
                continue;
            }

            let hashes = element_hashes(&doc);
            by_hash.entry(list_hash(doc.elements()[root].children.iter().map(|&c| hashes[c]))).or_default().push(key.to_owned());
        }

        Self { by_hash }
    }

    pub fn is_empty(&self) -> bool {
        self.by_hash.is_empty()
    }
}

/// What turning a tile's groups into references found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recognised {
    /// The instance groups that are exact copies of a prefab, and its key (outermost ones only; not the references there are already).
    pub copies: Vec<(usize, String)>,

    /// Groups named after a prefab whose contents differ from it (left as they are).
    pub named_but_different: usize,
}

/// The instance groups of a scene whose contents are exactly a prefab's (`index`), outermost first: a group inside one found, or inside a
/// reference, is part of it. When several prefabs have those contents, the one the group is named after, else the first.
pub fn recognise(scene: &TedScene, index: &ContentIndex, named: &dyn Fn(&str) -> bool) -> Recognised {
    let mut out = Recognised::default();
    if index.is_empty() {
        return out;
    }

    let doc = scene.doc();
    let els = doc.elements();
    let hashes = element_hashes(doc);
    let is_ref = |e: usize| doc.get(e, crate::project_files::PREFAB_ATTR).is_some_and(|k| !k.is_empty());
    let mut taken: Vec<bool> = vec![false; els.len()];
    for (g, el) in els.iter().enumerate() {
        // Inside a reference or a copy found: part of it (the parent comes first in the table, so it is marked already).
        if el.parent.is_some_and(|p| taken[p]) {
            taken[g] = true;
            continue;
        }

        if is_ref(g) {
            taken[g] = true;
            continue;
        }

        if el.name != "UserInstanceGroup" {
            continue;
        }

        let Some(&ch) = el.children.iter().find(|&&c| els[c].name == "children") else { continue };
        if els[ch].children.is_empty() {
            continue;
        }

        let name = doc.get(g, "name").unwrap_or_default();
        match index.by_hash.get(&list_hash(els[ch].children.iter().map(|&c| hashes[c]))) {
            Some(keys) => {
                let key = keys.iter().find(|k| k.eq_ignore_ascii_case(&name)).unwrap_or(&keys[0]);
                out.copies.push((g, key.clone()));
                taken[g] = true;
            }

            None if named(&name) => out.named_but_different += 1,
            None => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "<BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" scale=\"1\"/>\n<UserInstanceGroup name=\"in\">\n    <children>\n        <BUILDING_INSTANCE position=\"vector(2,0,0)\" key=\"rock\"/>\n    </children>\n</UserInstanceGroup>\n";

    #[test]
    fn overrides_are_the_values_that_differ_and_apply_back() {
        let current = BASE.replace("scale=\"1\"", "scale=\"1.5\"").replace("vector(2,0,0)", "vector(2,0,3)");
        let ovs = overrides(current.as_bytes(), BASE.as_bytes()).unwrap();
        assert_eq!(ovs, vec![
            Override { path: vec![0], element: "BUILDING_INSTANCE".into(), attr: "scale".into(), value: "1.5".into() },
            Override { path: vec![1, 0, 0], element: "BUILDING_INSTANCE".into(), attr: "position".into(), value: "vector(2,0,3)".into() },
        ]);

        let (back, lost) = apply(BASE.as_bytes(), &ovs).unwrap();
        assert!(lost.is_empty());
        assert_eq!(String::from_utf8(back).unwrap(), current);

        // Through the file's text.
        let text = overrides_text(&ovs);
        let doc = XmlDoc::parse([b"<g>".as_slice(), &text, b"</g>"].concat()).unwrap();
        assert_eq!(read_overrides(&doc, 0), ovs);

        // An element added is not an override; one that no longer fits is left out.
        assert!(overrides(format!("{BASE}<VFXInstance/>\n").as_bytes(), BASE.as_bytes()).is_err());
        let stale = Override { path: vec![1], element: "BUILDING_INSTANCE".into(), attr: "scale".into(), value: "2".into() };
        let (same, lost) = apply(BASE.as_bytes(), std::slice::from_ref(&stale)).unwrap();
        assert_eq!((same, lost), (BASE.as_bytes().to_vec(), vec![stale]));
    }

    #[test]
    fn exact_copies_are_found_outermost_first() {
        let index = ContentIndex::new([("rocks", BASE.as_bytes().to_vec()), ("inner", b"<BUILDING_INSTANCE position=\"vector(2,0,0)\" key=\"rock\"/>\n".to_vec())]);
        let indented = BASE.lines().map(|l| format!("                {l}\n")).collect::<String>();
        let tile = format!(
            "<project><SCENE_GRAPH><children>\n<UserInstanceGroup name=\"copy\" position=\"vector(9,9,9)\">\n<children>\n{indented}</children>\n</UserInstanceGroup>\n<UserInstanceGroup name=\"rocks\">\n<children>\n<BUILDING_INSTANCE position=\"vector(5,0,0)\" key=\"rock\"/>\n</children>\n</UserInstanceGroup>\n</children></SCENE_GRAPH></project>\n"
        );
        let scene = TedScene::parse_any(tile.into_bytes()).unwrap();
        let found = recognise(&scene, &index, &|n| n == "rocks");
        assert_eq!(found.copies.len(), 1, "the inner group is part of the copy: {found:?}");
        assert_eq!(found.copies[0].1, "rocks");
        assert_eq!(scene.name(found.copies[0].0), "copy");
        assert_eq!(found.named_but_different, 1);
    }
}
