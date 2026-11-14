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

/// The attributes of a container's start tag that variations may differ in and still share it ([`START`]).
pub const VIEW_STATE: [&str; 1] = ["flags"];

/// The folders that may carry the attribute themselves (TEd's layers and groups). Anything else that is only some variations' goes in a
/// variation folder.
pub const TAGGABLE: [&str; 4] = ["LayerInstance", "UserInstanceGroup", "INSTANCE_GROUP", "LAYER_INSTANCE_GROUP"];

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
        } else if let Some(span) = attr_span(text, &doc, e, VARIATIONS_ATTR) {
            cuts.push(span);
        }

        stack.extend(el.children.iter().rev().copied());
    }

    for (at, n) in dedent {
        if text[at..].iter().take(n).any(|&c| c != b' ' && c != b'\t') {
            return Err(format!("a line in a {FOLDER} at offset {at} has less indent than its folder's contents"));
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
    fn nested_folders_take_off_both_indents() {
        let text = "<project wce_variations=\"a,b,c\">\n    <WceVariationFolder wce_variations=\"a,b\">\n        <x/>\n        <WceVariationFolder wce_variations=\"a\">\n            <y/>\n        </WceVariationFolder>\n    </WceVariationFolder>\n    <z/>\n</project>\n";
        let a = String::from_utf8(to_variation(text.as_bytes(), "a").unwrap()).unwrap();
        assert_eq!(a, "<project>\n    <x/>\n    <y/>\n    <z/>\n</project>\n");
        let c = String::from_utf8(to_variation(text.as_bytes(), "c").unwrap()).unwrap();
        assert_eq!(c, "<project>\n    <z/>\n</project>\n");
    }
}
