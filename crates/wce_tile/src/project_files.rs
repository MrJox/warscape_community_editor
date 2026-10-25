//! The editor's tiles (`tile.wce`) and TEd's (`tile.ted`). A tile is saved as `tile.wce` in the projects folder (`<projects>/assembly_kit`,
//! laid out like a kit's `raw_data`), never in the Assembly Kit; "Save for Assembly Kit" writes a `tile.ted` TEd reads.
//!
//! `tile.wce` is a superset of `tile.ted`: the same XML, its root `<project>` marked with `wce_version`, where the editor may keep what TEd
//! does not know. The convention, so that the conversion finds it all:
//! - an attribute TEd does not have is named `wce_...`;
//! - an element TEd does not have is named `Wce...` (with everything inside it).
//!
//! The conversion to TEd (`to_ted`) applies a feature's polyfill where it has one (what the editor shows becomes what TEd can hold: a
//! surface mode TEd lacks is written as freeform at the height it computed, ...), and drops what has none, telling what it did.
//!
//! An exported tile extracted from the packs gets `_extracted` at the end of its tile's folder (`.../farm_nordic/1x1_extracted/aa`) and of
//! its entry's location, so that it does not take the place of the kit's tile of the same name; the mark goes away when it is saved for the
//! kit or the game.

use crate::ted_scene::TedScene;

/// The editor's tile file.
pub const WCE_FILE: &str = "tile.wce";

/// TEd's.
pub const TED_FILE: &str = "tile.ted";

/// The version of `tile.wce` this editor writes.
pub const WCE_VERSION: u32 = 1;

/// The root attribute that marks `tile.wce`.
pub const WCE_VERSION_ATTR: &str = "wce_version";

/// The mark of an extracted tile's folder and location.
pub const EXTRACTED: &str = "_extracted";

/// A tile folder (`terrain/tiles/<kind>/.../<tile>/<variation>`, `/` or `\` separated) with its tile's folder marked as extracted.
pub fn mark_extracted(folder: &str) -> String {
    let f = folder.replace('\\', "/");
    let f = f.trim_end_matches('/');
    match f.rsplit_once('/') {
        Some((tile, variation)) if !tile.ends_with(EXTRACTED) => format!("{tile}{EXTRACTED}/{variation}"),
        _ => f.to_owned(),
    }
}

/// A path or a location without the extracted mark (on any of its folders), with its separators kept.
pub fn unmark_extracted(path: &str) -> String {
    path.split_inclusive(['/', '\\'])
        .map(|part| {
            let (name, sep) = part.strip_suffix(['/', '\\']).map_or((part, ""), |n| (n, &part[n.len()..]));
            format!("{}{sep}", name.strip_suffix(EXTRACTED).unwrap_or(name))
        })
        .collect()
}

/// Whether a path has the extracted mark.
pub fn is_extracted(path: &str) -> bool {
    path.split(['/', '\\']).any(|p| p.ends_with(EXTRACTED))
}

/// A tile.ted's text as `tile.wce` (its root marked).
///
/// # Errors
///
/// The text does not read.
pub fn to_wce(ted: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut scene = TedScene::parse(ted)?;
    mark_wce(&mut scene);
    Ok(scene.save())
}

/// Marks a scene's root as `tile.wce` (unless it is).
pub fn mark_wce(scene: &mut TedScene) {
    let doc = scene.doc();
    let Some(root) = (0..doc.elements().len()).find(|&e| doc.elements()[e].parent.is_none()) else { return };
    if doc.get(root, WCE_VERSION_ATTR).is_none() {
        scene.set_attr(root, WCE_VERSION_ATTR, &WCE_VERSION.to_string());
    }
}

/// A number as TEd would write it in an attribute: `vector(..)` / `euler(..)` components and plain numbers as `%.6f` of the float32
/// trimmed (`format_ted_number`), the `x,y,z` of points and spline handles and `lf_offset` as `%g` of the float32 (`format_general`),
/// integers as they are (measured over the kit's tile.ted files: `examples/ted_numbers_check.rs`). `None`: not a value of
/// numbers (or one whose shape this does not know).
pub fn ted_number_text(_element: &str, attr: &str, value: &str) -> Option<String> {
    use crate::ted_scene::{format_general, format_ted_number};

    let num = |t: &str| -> Option<f64> {
        let t = t.trim();
        (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) && t.bytes().any(|b| b.is_ascii_digit()))
            .then(|| t.parse::<f64>().ok())
            .flatten()
    };

    for prefix in ["vector", "euler"] {
        if let Some(inner) = value.strip_prefix(prefix).and_then(|r| r.strip_prefix('(')).and_then(|r| r.strip_suffix(')')) {
            let parts = inner.split(',').map(num).collect::<Option<Vec<_>>>()?;
            return Some(format!("{prefix}({})", parts.into_iter().map(format_ted_number).collect::<Vec<_>>().join(",")));
        }
    }

    if value.contains(',') {
        let parts = value.split(',').map(num).collect::<Option<Vec<_>>>()?;
        return (parts.len() == 3).then(|| parts.into_iter().map(format_general).collect::<Vec<_>>().join(","));
    }

    // Integers (the generation seeds) as they are; `lf_offset` as `%g`; the other numbers as the vectors' components.
    let v = num(value)?;
    let t = value.trim();
    if t.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return Some(format!("{}", v as i64));
    }

    Some(if attr == "lf_offset" { format_general(v) } else { format_ted_number(v) })
}

/// The editor's attributes a polyfill turns into what TEd holds (reported as polyfilled, not as dropped).
const POLYFILLED_ATTRS: [&str; 1] = ["wce_prefab"];

/// What the conversion to TEd did: the polyfills it applied and what it dropped (each a line for people).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TedReport {
    pub polyfilled: Vec<String>,
    pub dropped: Vec<String>,
}

impl TedReport {
    pub fn summary(&self) -> String {
        match (self.polyfilled.len(), self.dropped.len()) {
            (0, 0) => "nothing TEd lacks".to_owned(),
            (p, d) => format!("{p} written as TEd can hold them, {d} dropped (TEd has no such thing)"),
        }
    }
}

/// `tile.wce`'s text (or a tile.ted's) as a tile.ted TEd reads: the root's mark, the `wce_` attributes and the `Wce` elements go, after
/// the polyfills of the features that have one.
///
/// # Errors
///
/// The text does not read.
pub fn to_ted(wce: Vec<u8>) -> Result<(Vec<u8>, TedReport), String> {
    let mut scene = TedScene::parse(wce)?;
    let mut report = TedReport::default();

    // The polyfills of the editor's features go here, before the attributes and elements TEd lacks are removed.

    // Elements TEd lacks, the outermost of each (with what is inside), last first so that the indices stay valid.
    let doc = scene.doc();
    let els = doc.elements();
    let wce_element = |e: usize| els[e].name.starts_with("Wce");
    let mut outermost = (0..els.len())
        .filter(|&e| wce_element(e) && !std::iter::successors(els[e].parent, |&p| els[p].parent).any(wce_element))
        .collect::<Vec<_>>();
    outermost.sort_unstable();
    for &e in outermost.iter().rev() {
        let name = scene.doc().elements()[e].name.clone();
        report.dropped.push(format!("{name} '{}'", scene.name(e)));
        scene.remove(e)?;
    }

    // Attributes TEd lacks: cut out of the text (with the space before each); numbers at more precision than TEd's: written as TEd writes
    // them (`ted_number_text`, only where that is shorter, so what TEd wrote stays as it is). Read again so that the offsets are the text's.
    let text = scene.save();
    let fresh = wce_formats::xml_doc::XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?;
    let mut cuts: Vec<(usize, usize, Vec<u8>)> = vec![];
    let mut rounded = 0;
    for (e, el) in fresh.elements().iter().enumerate() {
        for a in &el.attrs {
            if a.name.starts_with("wce_") {
                if a.name != WCE_VERSION_ATTR && !POLYFILLED_ATTRS.contains(&a.name.as_str()) {
                    report.dropped.push(format!("{} of {} '{}'", a.name, el.name, fresh.get(e, "name").unwrap_or_default()));
                }

                // `name=` and the quote before the value, the quote after it.
                let mut start = a.value_off - 2 - a.name.len();
                while start > 0 && text[start - 1].is_ascii_whitespace() {
                    start -= 1;
                }

                cuts.push((start, a.value_off + a.value_len + 1, vec![]));
                continue;
            }

            let raw = &text[a.value_off..a.value_off + a.value_len];
            let Ok(value) = std::str::from_utf8(raw) else { continue };
            if let Some(ted) = ted_number_text(&el.name, &a.name, value).filter(|t| t.len() < value.len()) {
                cuts.push((a.value_off, a.value_off + a.value_len, ted.into_bytes()));
                rounded += 1;
            }
        }
    }

    if rounded != 0 {
        report.polyfilled.push(format!("{rounded} number(s) written at TEd's precision"));
    }

    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for (start, end, with) in cuts {
        out.extend_from_slice(&text[at..start]);
        out.extend_from_slice(&with);
        at = end;
    }

    out.extend_from_slice(&text[at..]);
    Ok((out, report))
}

/// A tile database entry's XML (`<TILE ... location='terrain\tiles\...'>`) with its location's extracted mark set (`mark`) or removed.
pub fn entry_with_location_mark(entry: &str, mark: bool) -> String {
    let Some(at) = entry.find(" location=") else { return entry.to_owned() };
    let quote_at = at + " location=".len();
    let Some(q) = entry[quote_at..].chars().next().filter(|c| *c == '\'' || *c == '"') else { return entry.to_owned() };
    let start = quote_at + 1;
    let Some(len) = entry[start..].find(q) else { return entry.to_owned() };
    let location = &entry[start..start + len];
    let new = if mark {
        if location.ends_with(EXTRACTED) { location.to_owned() } else { format!("{location}{EXTRACTED}") }
    } else {
        unmark_extracted(location)
    };

    format!("{}{new}{}", &entry[..start], &entry[start + len..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extracted_mark_goes_on_the_tile_folder_and_comes_off_anywhere() {
        assert_eq!(mark_extracted("terrain/tiles/battle/farm_nordic/1x1/aa"), "terrain/tiles/battle/farm_nordic/1x1_extracted/aa");
        assert_eq!(mark_extracted("terrain/tiles/battle/farm_nordic/1x1_extracted/aa"), "terrain/tiles/battle/farm_nordic/1x1_extracted/aa");
        assert_eq!(unmark_extracted("x/raw_data/terrain/tiles/battle/farm_nordic/1x1_extracted/aa"), "x/raw_data/terrain/tiles/battle/farm_nordic/1x1/aa");
        assert_eq!(unmark_extracted("terrain\\tiles\\battle\\farm_nordic\\1x1_extracted"), "terrain\\tiles\\battle\\farm_nordic\\1x1");
        assert!(is_extracted("a/1x1_extracted/aa") && !is_extracted("a/1x1/aa"));

        let entry = "<TILE serialise_version='3' location='terrain\\tiles\\battle\\farm_nordic\\1x1' tile_set='farm_nordic'>";
        let marked = entry_with_location_mark(entry, true);
        assert!(marked.contains("location='terrain\\tiles\\battle\\farm_nordic\\1x1_extracted'"), "{marked}");
        assert_eq!(entry_with_location_mark(&marked, false), entry);
    }

    #[test]
    fn wce_round_trips_to_ted_dropping_what_ted_lacks() {
        let ted = b"<project version=\"16\">\n    <scene>\n        <SCENE_GRAPH name=\"root\">\n            <children>\n                <BUILDING_INSTANCE key=\"a\" name=\"a\" position=\"vector(0,0,0)\"/>\n            </children>\n        </SCENE_GRAPH>\n    </scene>\n</project>\n".to_vec();
        let wce = to_wce(ted.clone()).unwrap();
        assert!(String::from_utf8_lossy(&wce).starts_with("<project version=\"16\" wce_version=\"1\">"));
        let (back, report) = to_ted(wce).unwrap();
        assert_eq!(back, ted, "a tile with nothing of the editor's own comes back byte for byte");
        assert_eq!(report, TedReport::default());

        let extra = String::from_utf8(ted).unwrap().replace(
            "key=\"a\" name=\"a\"",
            "key=\"a\" wce_surface=\"entity\" name=\"a\"",
        ).replace("            </children>", "                <WceSpline name=\"s\">\n                    <points/>\n                </WceSpline>\n            </children>");
        let (back, report) = to_ted(extra.into_bytes()).unwrap();
        let back = String::from_utf8(back).unwrap();
        assert!(!back.contains("wce_") && !back.contains("Wce"), "{back}");
        assert_eq!(report.dropped.len(), 2, "{report:?}");
    }

    #[test]
    fn numbers_get_teds_precision_and_teds_own_stay() {
        let wce = b"<project version=\"16\" wce_version=\"1\">\n    <BUILDING_INSTANCE lf_offset=\"-1.7191e-05\" position=\"vector(133.46086123456,-7.737109999999,0.5)\" seed=\"1869639023\"/>\n    <point position=\"0.123456789,2,3\"/>\n</project>\n".to_vec();
        let (ted, report) = to_ted(wce).unwrap();
        let ted = String::from_utf8(ted).unwrap();
        assert!(ted.contains("position=\"vector(133.460861,-7.73711,0.5)\""), "{ted}");
        assert!(ted.contains("lf_offset=\"-1.7191e-05\"") && ted.contains("seed=\"1869639023\""), "TEd's own numbers stay: {ted}");
        assert!(ted.contains("position=\"0.123457,2,3\""), "{ted}");
        assert_eq!(report.polyfilled.len(), 1, "{report:?}");
    }
}
