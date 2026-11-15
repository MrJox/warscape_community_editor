//! One project for all of a tile's variations (docs/plans/tile_variations.md).
//!
//! A combined `tile.wce` lists its variations on the root (`wce_variations="minor,small,medium"`). A folder (a layer, a group, or a
//! variation folder: [`FOLDER`], an element of the editor's own) may carry `wce_variations` too: it then belongs to those variations only,
//! and so does everything in it. Without the attribute a folder belongs to every variation its parent belongs to. Instances never carry it:
//! what one variation has of its own sits in a folder for it.
//!
//! [`to_variation`] makes one variation's tile.wce out of a combined one: what does not belong to it goes, a variation folder gives its
//! place to its children (one indent less), and the attributes go. A file of one variation has no trace of the others, so a combined
//! project that was made from variations gives each of them back byte for byte ([`merge`]).

pub mod merge;

use wce_formats::xml_doc::XmlDoc;

/// The attribute of the variations a folder (or the root, the project) belongs to.
pub const VARIATIONS_ATTR: &str = "wce_variations";

/// A folder that only holds what some variations have, at its place in a shared layer.
pub const FOLDER: &str = "WceVariationFolder";

/// A variation's own view state of a shared container (TEd's `flags`: whether a layer showed, was active), the first child of the
/// container: its attributes are the values that variation's start tag has.
pub const START: &str = "WceVariationStart";

/// A variation's own values of the attributes of the element after it (an instance that variations share but for its height on
/// their terrains): to the element's start tag in that variation.
pub const VALUES: &str = "WceVariationValues";

/// The attributes of a container's start tag that variations may differ in and still share it ([`START`]).
pub const VIEW_STATE: [&str; 1] = ["flags"];

/// The folders that may carry the attribute themselves (TEd's layers and groups). Anything else that is only some variations' goes in a
/// variation folder.
pub const TAGGABLE: [&str; 4] = ["LayerInstance", "UserInstanceGroup", "INSTANCE_GROUP", "LAYER_INSTANCE_GROUP"];

/// The variations a group is a group in, when not all it is in: in the others it is not there, and what its `children` holds takes its
/// place (one level up). The same buildings that each level of a settlement keeps in other groups are kept once that way.
pub const GROUP_ATTR: &str = "wce_group_variations";

/// The groups that may be a group in some variations only ([`GROUP_ATTR`]). Never a layer: what is in a layer exports with it.
pub const WRAPPABLE: [&str; 2] = ["UserInstanceGroup", "INSTANCE_GROUP"];

/// The variations of an attribute's value, in its order (empty names dropped).
#[must_use]
pub fn parse_list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// An attribute's value for a list of variations.
#[must_use]
pub fn list_text(list: &[String]) -> String {
    list.join(",")
}

/// The first root element of a document.
fn root(doc: &XmlDoc) -> Option<usize> {
    doc.roots().first().copied()
}

/// The variations a project lists on its root; empty: a project of one variation (a version 1 file, or one made by [`to_variation`]).
#[must_use]
pub fn project_variations(doc: &XmlDoc) -> Vec<String> {
    root(doc).and_then(|r| doc.get(r, VARIATIONS_ATTR)).map(|v| parse_list(&v)).unwrap_or_default()
}

/// The variations an element's own attribute names; `None`: it has none (it belongs where its parent does). The root's is the project's
/// list, not a restriction, and is not read here.
#[must_use]
pub fn own_variations(doc: &XmlDoc, elem: usize) -> Option<Vec<String>> {
    doc.elements()[elem].parent?;
    doc.get(elem, VARIATIONS_ATTR).map(|v| parse_list(&v))
}

/// Whether an element belongs to a variation: it and every ancestor either have no attribute or name it.
#[must_use]
pub fn belongs(doc: &XmlDoc, elem: usize, variation: &str) -> bool {
    std::iter::successors(Some(elem), |&e| doc.elements()[e].parent)
        .all(|e| own_variations(doc, e).is_none_or(|l| l.iter().any(|v| v == variation)))
}

/// Whether an element is there in a variation as itself: it belongs to it, and it is not a group of other variations only
/// ([`GROUP_ATTR`]; its contents are there, in its parent's place).
#[must_use]
pub fn is_there(doc: &XmlDoc, elem: usize, variation: &str) -> bool {
    belongs(doc, elem, variation) && doc.get(elem, GROUP_ATTR).is_none_or(|l| parse_list(&l).iter().any(|v| v == variation))
}

/// The variations an element belongs to, of the project's `all`, in their order.
#[must_use]
pub fn effective(doc: &XmlDoc, elem: usize, all: &[String]) -> Vec<String> {
    all.iter().filter(|v| belongs(doc, elem, v)).cloned().collect()
}

/// Where the line holding `off` starts, when only spaces and tabs come before `off` on it.
pub(crate) fn line_start(text: &[u8], off: usize) -> Option<usize> {
    let mut i = off;
    while i > 0 && (text[i - 1] == b' ' || text[i - 1] == b'\t') {
        i -= 1;
    }

    (i == 0 || text[i - 1] == b'\n').then_some(i)
}

/// Just past the line end after `off`, when only a line end (and spaces) follows `off` on its line (or the text ends).
pub(crate) fn line_end(text: &[u8], off: usize) -> Option<usize> {
    let mut i = off;
    while i < text.len() && (text[i] == b' ' || text[i] == b'\t' || text[i] == b'\r') {
        i += 1;
    }

    if i < text.len() && text[i] == b'\n' { Some(i + 1) } else { (i == text.len()).then_some(i) }
}

/// The lines from the one `start` is on to the one `end` is on, both included; `None` when something else shares those lines.
pub(crate) fn whole_lines(text: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    Some((line_start(text, start)?, line_end(text, end)?))
}

/// The offset of an element's end tag (`</...>`); `None` for a self-closing one.
pub(crate) fn close_tag(doc: &XmlDoc, text: &[u8], elem: usize) -> Option<usize> {
    let el = &doc.elements()[elem];
    if el.self_closing {
        return None;
    }

    text[el.start_end..el.end_off].windows(2).rposition(|w| w == b"</").map(|i| el.start_end + i)
}

/// The leading spaces and tabs of the line starting at `at`.
pub(crate) fn indent_len(text: &[u8], at: usize) -> usize {
    text[at..].iter().take_while(|&&c| c == b' ' || c == b'\t').count()
}

/// The span of an attribute with the white space before it (`  name="value"`).
pub(crate) fn attr_span(text: &[u8], doc: &XmlDoc, elem: usize, name: &str) -> Option<(usize, usize)> {
    let a = &doc.elements()[elem].attrs[doc.find_attr(elem, name)?];
    if a.appended {
        return None;
    }

    let mut start = a.value_off - 2 - a.name.len();
    while start > 0 && text[start - 1].is_ascii_whitespace() {
        start -= 1;
    }

    Some((start, a.value_off + a.value_len + 1))
}

/// The variations each scene layer with a `wce_layer_id` belongs to (of the project's `all`), by that id: what a terrain layer that
/// exports with it belongs to as well (`TerrainLayers::set_view`, the exports).
#[must_use]
pub fn layer_key_variations(doc: &XmlDoc, all: &[String]) -> std::collections::HashMap<String, Vec<String>> {
    (0..doc.elements().len())
        .filter_map(|e| doc.get(e, LAYER_ID_ATTR).filter(|k| !k.is_empty()).map(|k| (k, effective(doc, e, all))))
        .collect()
}

/// How one variation sees a combined scene, element by element: what is not there (another variation's, or the bookkeeping: [`START`],
/// [`VALUES`]), what it goes through (a [`FOLDER`], a group of other variations only: [`GROUP_ATTR`]; their contents are there, in
/// their place), and the values its start tags have of their own. What the editor shows and picks of a combined project, and how it
/// places it: the same as [`to_variation`] gives, without making that text.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub variation: String,
    state: Vec<u8>,
    values: std::collections::HashMap<usize, Vec<(String, Vec<u8>)>>,
}

const GONE: u8 = 1;
const THROUGH: u8 = 2;

impl View {
    /// The view of a variation; of a document of one variation, everything is there.
    #[must_use]
    pub fn new(doc: &XmlDoc, variation: &str) -> Self {
        let els = doc.elements();
        let mut state = vec![0u8; els.len()];
        let mut values = std::collections::HashMap::<usize, Vec<(String, Vec<u8>)>>::new();
        let named = |l: Option<String>| l.is_none_or(|l| parse_list(&l).iter().any(|v| v == variation));

        // Document order: a parent comes before its children.
        for e in 0..els.len() {
            let el = &els[e];
            if el.parent.is_some_and(|p| state[p] == GONE) || !named(own_variations(doc, e).map(|l| list_text(&l))) {
                state[e] = GONE;
                continue;
            }

            if el.name == START || el.name == VALUES {
                state[e] = GONE;
                let target = if el.name == START {
                    el.parent
                } else {
                    el.parent.and_then(|p| {
                        let list = &els[p].children;
                        let at = list.iter().position(|&c| c == e)?;
                        list[at + 1..].iter().copied().find(|&c| els[c].name != VALUES)
                    })
                };

                if let Some(target) = target {
                    let own = el.attrs.iter().enumerate().filter(|(_, a)| a.name != VARIATIONS_ATTR);
                    values.entry(target).or_default().extend(own.map(|(i, a)| (a.name.clone(), doc.attr_raw(e, i).to_vec())));
                }

                continue;
            }

            let through_group = el.parent.is_some_and(|p| state[p] == THROUGH && els[p].name != FOLDER) && el.name == "children";
            if el.name == FOLDER || !named(doc.get(e, GROUP_ATTR)) || through_group {
                state[e] = THROUGH;
            }
        }

        Self { variation: variation.to_owned(), state, values }
    }

    /// Not in the variation (another's, or the bookkeeping): not shown, not picked.
    #[must_use]
    pub fn is_gone(&self, elem: usize) -> bool {
        self.state.get(elem) == Some(&GONE)
    }

    /// A variation folder, or a group of other variations only (with its `children`): not there as itself, its contents are, in its
    /// place: it adds no transform and no view state.
    #[must_use]
    pub fn is_through(&self, elem: usize) -> bool {
        self.state.get(elem) == Some(&THROUGH)
    }

    /// An attribute's raw value as the variation has it: its own ([`START`], [`VALUES`]), else the element's.
    #[must_use]
    pub fn raw<'a>(&'a self, doc: &'a XmlDoc, elem: usize, name: &str) -> Option<&'a [u8]> {
        if let Some((_, v)) = self.values.get(&elem).and_then(|l| l.iter().find(|(n, _)| n == name)) {
            return Some(v);
        }

        doc.find_attr(elem, name).map(|i| doc.attr_raw(elem, i))
    }

    /// The values the variation has of its own for an element (none for most).
    #[must_use]
    pub fn own_values(&self, elem: usize) -> &[(String, Vec<u8>)] {
        self.values.get(&elem).map_or(&[], Vec::as_slice)
    }

    /// The element as the variation has it above `elem`: the nearest ancestor it does not go through.
    #[must_use]
    pub fn parent(&self, doc: &XmlDoc, elem: usize) -> Option<usize> {
        std::iter::successors(doc.elements()[elem].parent, |&p| doc.elements()[p].parent).find(|&p| !self.is_through(p))
    }
}

/// The attribute that names a scene layer for the terrain layers that export with it.
pub const LAYER_ID_ATTR: &str = "wce_layer_id";

/// One variation's tile.wce out of a combined one: what does not belong to it removed (whole lines), each variation folder's tags taken
/// away and its contents one indent less, the `wce_variations` attributes gone (the root's too). A text without any of it comes back as
/// it is.
///
/// # Errors
///
/// The text does not read, the project does not have that variation, or a folder is not laid out one element per line.
pub fn to_variation(text: &[u8], variation: &str) -> Result<Vec<u8>, String> {
    let doc = XmlDoc::parse(text.to_vec()).map_err(|e| e.to_string())?;
    let all = project_variations(&doc);
    if !all.is_empty() && !all.iter().any(|v| v == variation) {
        return Err(format!("the project has no variation '{variation}' (it has {})", list_text(&all)));
    }

    let text = doc.original_text();
    let els = doc.elements();
    let mut cuts: Vec<(usize, usize)> = vec![];

    // Values replaced: (start, end, the new raw value).
    let mut values: Vec<(usize, usize, Vec<u8>)> = vec![];

    // Lines dedented: by line start, the bytes to take off.
    let mut dedent = std::collections::BTreeMap::<usize, usize>::new();
    let mut stack = doc.roots().iter().rev().copied().collect::<Vec<_>>();
    while let Some(e) = stack.pop() {
        let el = &els[e];
        let own = own_variations(&doc, e);
        if own.as_ref().is_some_and(|l| !l.iter().any(|v| v == variation)) {
            cuts.push(whole_lines(text, el.start_off, el.end_off).unwrap_or((el.start_off, el.end_off)));
            continue;
        }

        if el.name == START {
            // This variation's view state of the container: its values put in the container's start tag, the line gone.
            cuts.push(whole_lines(text, el.start_off, el.end_off).unwrap_or((el.start_off, el.end_off)));
            let Some(parent) = el.parent else { continue };
            for (i, a) in el.attrs.iter().enumerate().filter(|(_, a)| a.name != VARIATIONS_ATTR) {
                let Some(pi) = doc.find_attr(parent, &a.name) else {
                    return Err(format!("{START} at offset {} sets {}, which its container does not have", el.start_off, a.name));
                };
                let pa = &els[parent].attrs[pi];
                values.push((pa.value_off, pa.value_off + pa.value_len, doc.attr_raw(e, i).to_vec()));
            }

            continue;
        }

        if el.name == VALUES {
            // This variation's values of the next element: put in its start tag, the line gone.
            cuts.push(whole_lines(text, el.start_off, el.end_off).unwrap_or((el.start_off, el.end_off)));
            let next = el.parent.and_then(|p| {
                let list = &els[p].children;
                let at = list.iter().position(|&c| c == e)?;
                list[at + 1..].iter().copied().find(|&c| els[c].name != VALUES)
            });
            let Some(next) = next else { return Err(format!("{VALUES} at offset {} is the last of its parent's", el.start_off)) };
            for (i, a) in el.attrs.iter().enumerate().filter(|(_, a)| a.name != VARIATIONS_ATTR) {
                let Some(ni) = doc.find_attr(next, &a.name) else {
                    return Err(format!("{VALUES} at offset {} sets {}, which the element after it does not have", el.start_off, a.name));
                };
                let na = &els[next].attrs[ni];
                values.push((na.value_off, na.value_off + na.value_len, doc.attr_raw(e, i).to_vec()));
            }

            continue;
        }

        if let Some(groups) = doc.get(e, GROUP_ATTR)
            && !parse_list(&groups).iter().any(|v| v == variation)
        {
            // Not a group in this variation: its lines go (with its view states), what its `children` holds is in its place.
            let bad = || format!("a group at offset {} with {GROUP_ATTR} is not laid out one element per line", el.start_off);
            let &c = el.children.iter().find(|&&c| els[c].name == "children").ok_or_else(bad)?;
            let group_line = line_start(text, el.start_off).ok_or_else(bad)?;
            let after_open = line_end(text, els[c].start_end).ok_or_else(bad)?;
            let close = close_tag(&doc, text, c).ok_or_else(bad)?;
            let (close_line, after) = whole_lines(text, close, el.end_off).ok_or_else(bad)?;
            cuts.push((group_line, after_open));
            cuts.push((close_line, after));
            if let Some(&first) = els[c].children.first() {
                let unit =
                    indent_len(text, line_start(text, els[first].start_off).ok_or_else(bad)?).saturating_sub(indent_len(text, group_line));
                let mut at = after_open;
                while at < close_line {
                    *dedent.entry(at).or_default() += unit;
                    at = text[at..close_line].iter().position(|&c| c == b'\n').map_or(close_line, |i| at + i + 1);
                }
            }

            stack.extend(els[c].children.iter().rev().copied());
            continue;
        }

        if el.name == FOLDER {
            let not_alone = || format!("{FOLDER} at offset {} is not on lines of its own", el.start_off);
            let (start_line, after_start) = whole_lines(text, el.start_off, el.start_end).ok_or_else(not_alone)?;
            cuts.push((start_line, after_start));
            if let Some(close) = close_tag(&doc, text, e) {
                let (close_line, after) = whole_lines(text, close, el.end_off).ok_or_else(not_alone)?;
                cuts.push((close_line, after));
                if let Some(&first) = el.children.first() {
                    let first_line = line_start(text, els[first].start_off).ok_or_else(not_alone)?;
                    let unit = indent_len(text, first_line).saturating_sub(indent_len(text, start_line));
                    let mut at = after_start;
                    while at < close_line {
                        *dedent.entry(at).or_default() += unit;
                        at = text[at..close_line].iter().position(|&c| c == b'\n').map_or(close_line, |i| at + i + 1);
                    }
                }
            }
        } else {
            cuts.extend([VARIATIONS_ATTR, GROUP_ATTR].iter().filter_map(|a| attr_span(text, &doc, e, a)));
        }

        stack.extend(el.children.iter().rev().copied());
    }

    for (at, n) in dedent {
        if text[at..].iter().take(n).any(|&c| c != b' ' && c != b'\t') {
            return Err(format!("a line in a {FOLDER} or a group at offset {at} has less indent than the rest of its contents"));
        }

        cuts.push((at, at + n));
    }

    let mut cuts = cuts.into_iter().map(|(s, t)| (s, t, vec![])).chain(values).collect::<Vec<_>>();
    cuts.sort_unstable();
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for (s, t, with) in cuts {
        if t <= at && s < at {
            continue;
        }

        if s >= at {
            out.extend_from_slice(&text[at..s]);
            out.extend_from_slice(&with);
        }

        at = at.max(t);
    }

    out.extend_from_slice(&text[at..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMBINED: &str = "<project version=\"16\" wce_version=\"1\" wce_variations=\"minor,small\">\r\n    <scene>\r\n        <SCENE_GRAPH name=\"root\">\r\n            <children>\r\n                <LayerInstance name=\"walls\" wce_variations=\"small\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"wall\"/>\r\n                    </children>\r\n                </LayerInstance>\r\n                <BUILDING_INSTANCE key=\"house\"/>\r\n                <WceVariationFolder wce_variations=\"minor\">\r\n                    <BUILDING_INSTANCE key=\"hut\"/>\r\n                    <UserInstanceGroup name=\"g\">\r\n                        <children>\r\n                            <BUILDING_INSTANCE key=\"shed\"/>\r\n                        </children>\r\n                    </UserInstanceGroup>\r\n                </WceVariationFolder>\r\n            </children>\r\n        </SCENE_GRAPH>\r\n    </scene>\r\n</project>\r\n";

    #[test]
    fn a_variation_keeps_what_belongs_to_it_and_no_trace_of_the_others() {
        let small = String::from_utf8(to_variation(COMBINED.as_bytes(), "small").unwrap()).unwrap();
        assert_eq!(
            small,
            "<project version=\"16\" wce_version=\"1\">\r\n    <scene>\r\n        <SCENE_GRAPH name=\"root\">\r\n            <children>\r\n                <LayerInstance name=\"walls\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"wall\"/>\r\n                    </children>\r\n                </LayerInstance>\r\n                <BUILDING_INSTANCE key=\"house\"/>\r\n            </children>\r\n        </SCENE_GRAPH>\r\n    </scene>\r\n</project>\r\n"
        );

        let minor = String::from_utf8(to_variation(COMBINED.as_bytes(), "minor").unwrap()).unwrap();
        assert_eq!(
            minor,
            "<project version=\"16\" wce_version=\"1\">\r\n    <scene>\r\n        <SCENE_GRAPH name=\"root\">\r\n            <children>\r\n                <BUILDING_INSTANCE key=\"house\"/>\r\n                <BUILDING_INSTANCE key=\"hut\"/>\r\n                <UserInstanceGroup name=\"g\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"shed\"/>\r\n                    </children>\r\n                </UserInstanceGroup>\r\n            </children>\r\n        </SCENE_GRAPH>\r\n    </scene>\r\n</project>\r\n"
        );

        assert!(to_variation(COMBINED.as_bytes(), "medium").is_err(), "not a variation of the project");
        let single = to_variation(minor.as_bytes(), "anything").unwrap();
        assert_eq!(single, minor.as_bytes(), "a file of one variation is every variation's");
    }

    #[test]
    fn membership_is_inherited_and_narrowed() {
        let doc = XmlDoc::parse(COMBINED.as_bytes().to_vec()).unwrap();
        let all = project_variations(&doc);
        assert_eq!(all, ["minor", "small"]);
        let find = |key: &str| (0..doc.elements().len()).find(|&e| doc.get(e, "key").as_deref() == Some(key)).unwrap();
        assert_eq!(effective(&doc, find("wall"), &all), ["small"]);
        assert_eq!(effective(&doc, find("house"), &all), ["minor", "small"]);
        assert_eq!(effective(&doc, find("shed"), &all), ["minor"]);
    }

    #[test]
    fn a_view_sees_what_to_variation_gives() {
        let text = "<project wce_variations=\"a,b\">\n    <WceVariationStart wce_variations=\"b\" version=\"15\"/>\n    <UserInstanceGroup position=\"1\" wce_group_variations=\"a\">\n        <WceVariationStart wce_variations=\"b\" position=\"9\"/>\n        <children>\n            <WceVariationValues wce_variations=\"b\" position=\"vector(0,2,0)\"/>\n            <x position=\"vector(0,1,0)\"/>\n            <WceVariationFolder wce_variations=\"b\">\n                <y/>\n            </WceVariationFolder>\n        </children>\n    </UserInstanceGroup>\n</project>\n";
        let doc = XmlDoc::parse(text.as_bytes().to_vec()).unwrap();
        let find = |name: &str| (0..doc.elements().len()).find(|&e| doc.elements()[e].name == name).unwrap();
        let (root, group, x, y, folder) = (find("project"), find("UserInstanceGroup"), find("x"), find("y"), find(FOLDER));
        let a = View::new(&doc, "a");
        assert!(!a.is_through(group) && a.is_gone(y) && !a.is_gone(x));
        assert_eq!(a.raw(&doc, x, "position"), Some(&b"vector(0,1,0)"[..]));
        assert_eq!(a.parent(&doc, x), Some(doc.elements()[x].parent.unwrap()), "the group's children");

        let b = View::new(&doc, "b");
        assert!(b.is_through(group) && b.is_through(folder) && !b.is_gone(y));
        assert_eq!(b.raw(&doc, x, "position"), Some(&b"vector(0,2,0)"[..]), "its own height");
        assert_eq!(b.raw(&doc, group, "position"), Some(&b"9"[..]));
        assert_eq!(b.raw(&doc, root, "version"), Some(&b"15"[..]));
        assert_eq!(b.parent(&doc, x), Some(root), "the group is not there in b");
        assert_eq!(b.parent(&doc, y), Some(root));
        assert!(b.is_gone(find(START)) && b.is_gone(find(VALUES)));
    }

    #[test]
    fn nested_folders_take_off_both_indents() {
        let text = "<project wce_variations=\"a,b,c\">\n    <WceVariationFolder wce_variations=\"a,b\">\n        <x/>\n        <WceVariationFolder wce_variations=\"a\">\n            <y/>\n        </WceVariationFolder>\n    </WceVariationFolder>\n    <z/>\n</project>\n";
        let a = String::from_utf8(to_variation(text.as_bytes(), "a").unwrap()).unwrap();
        assert_eq!(a, "<project>\n    <x/>\n    <y/>\n    <z/>\n</project>\n");
        let c = String::from_utf8(to_variation(text.as_bytes(), "c").unwrap()).unwrap();
        assert_eq!(c, "<project>\n    <z/>\n</project>\n");
    }
}
