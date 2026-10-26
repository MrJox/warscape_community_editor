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
//! An exported tile extracted from the packs is a project like any other, at its own folder (the kit's tile of the same name and it share
//! the project); its `tile.wce` is marked `wce_extracted_tile="true"` (`EXTRACTED_ATTR`) so that the tile list can tell it. Projects of
//! the editor's first versions had `_extracted` at the end of their tile's folder and entry location instead: `migrate_extracted` moves
//! them.

use crate::ted_scene::TedScene;

/// The editor's tile file.
pub const WCE_FILE: &str = "tile.wce";

/// TEd's.
pub const TED_FILE: &str = "tile.ted";

/// The version of `tile.wce` this editor writes.
pub const WCE_VERSION: u32 = 1;

/// The root attribute that marks `tile.wce`.
pub const WCE_VERSION_ATTR: &str = "wce_version";

/// The root attribute of an extracted tile's `tile.wce`.
pub const EXTRACTED_ATTR: &str = "wce_extracted_tile";

/// The old mark of an extracted tile's folder and location (`migrate_extracted`).
const OLD_EXTRACTED: &str = "_extracted";

/// A tile file's text (`tile.wce`) with its root marked as an extracted tile, unless it is.
pub fn mark_extracted_text(text: Vec<u8>) -> Vec<u8> {
    if is_extracted_text(&text) {
        return text;
    }

    let Ok(mut scene) = TedScene::parse_any(text.clone()) else { return text };
    let doc = scene.doc();
    let Some(root) = (0..doc.elements().len()).find(|&e| doc.elements()[e].parent.is_none()) else { return text };
    scene.set_attr(root, EXTRACTED_ATTR, "true");
    scene.save()
}

/// Whether a tile file's text has its root marked as an extracted tile (its start tag is read, not the whole text).
pub fn is_extracted_text(text: &[u8]) -> bool {
    let Some(start) = text.windows(2).position(|w| w[0] == b'<' && w[1].is_ascii_alphabetic()) else { return false };
    let end = text[start..].iter().position(|&c| c == b'>').map_or(text.len(), |i| start + i);
    let tag = String::from_utf8_lossy(&text[start..end]);
    tag.contains(&format!("{EXTRACTED_ATTR}=\"true\"")) || tag.contains(&format!("{EXTRACTED_ATTR}='true'"))
}

/// Whether the tile in a variation folder is an extracted one (its `tile.wce`'s start tag).
pub fn is_extracted_dir(dir: &std::path::Path) -> bool {
    use std::io::Read as _;
    let Ok(file) = std::fs::File::open(dir.join(WCE_FILE)) else { return false };
    let mut head = vec![];
    file.take(4096).read_to_end(&mut head).is_ok() && is_extracted_text(&head)
}

/// What `migrate_extracted` did: the tiles moved, and those left where they are, with why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Migration {
    pub moved: Vec<String>,
    pub skipped: Vec<String>,
}

/// Moves the projects of the old extracted tiles (`<tile>_extracted` folders, `<name>_extracted.xml` entries whose location has the mark)
/// of a projects' raw_data to their own names: the tile's folder renamed, the entry renamed with its location unmarked, and each variation's
/// `tile.wce` marked `EXTRACTED_ATTR`. One whose place has a project already, or whose folder is in `busy` (open in the editor), is left as
/// it is.
///
/// # Errors
///
/// None so far: a failure on one tile is a skip.
pub fn migrate_extracted(projects_raw: &std::path::Path, busy: &[std::path::PathBuf]) -> Result<Migration, String> {
    let mut m = Migration::default();
    let Ok(kinds) = std::fs::read_dir(projects_raw.join("terrain/tiles")) else { return Ok(m) };
    for kind in kinds.flatten().map(|k| k.path()).filter(|k| k.is_dir()) {
        let Ok(read) = std::fs::read_dir(kind.join("_tile_database/TILES")) else { continue };
        let mut entries = read.flatten().map(|f| f.path()).collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            let name = entry.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(stem) = name.strip_suffix(&format!("{OLD_EXTRACTED}.xml")) else { continue };
            match migrate_one(projects_raw, &entry, &format!("{stem}.xml"), busy) {
                Ok(what) => m.moved.push(what),
                Err(why) => m.skipped.push(format!("{name}: {why}")),
            }
        }
    }

    if !m.moved.is_empty() {
        wce_formats::tile_index::TileIndex::forget_shared();
    }

    Ok(m)
}

/// One old extracted tile (its entry file, the entry's new name); returns what was done.
fn migrate_one(projects_raw: &std::path::Path, entry: &std::path::Path, new_name: &str, busy: &[std::path::PathBuf]) -> Result<String, String> {
    let text = std::fs::read_to_string(entry).map_err(|e| e.to_string())?;
    let location = wce_formats::tile_entry::TileEntry::parse(text.clone().into_bytes()).map_err(|e| e.to_string())?.location();
    let old = projects_raw.join(location.replace('\\', "/"));
    let new = projects_raw.join(unmark_extracted(&location).replace('\\', "/"));
    let new_entry = entry.with_file_name(new_name);
    if !location.ends_with(OLD_EXTRACTED) || !old.is_dir() {
        return Err(format!("no folder {}", old.display()));
    }

    if new.exists() || new_entry.exists() {
        return Err(format!("a project is there already ({})", new.display()));
    }

    if busy.iter().any(|b| b.starts_with(&old)) {
        return Err("it is open in the editor".to_owned());
    }

    std::fs::rename(&old, &new).map_err(|e| format!("{} -> {}: {e}", old.display(), new.display()))?;
    std::fs::write(&new_entry, entry_with_location_mark(&text, false)).map_err(|e| e.to_string())?;
    std::fs::remove_file(entry).map_err(|e| e.to_string())?;
    let mut marked = 0;
    for variation in std::fs::read_dir(&new).map_err(|e| e.to_string())?.flatten().map(|v| v.path()) {
        let wce = variation.join(WCE_FILE);
        if let Ok(t) = std::fs::read(&wce) {
            std::fs::write(&wce, mark_extracted_text(t)).map_err(|e| e.to_string())?;
            marked += 1;
        }
    }

    Ok(format!("{} -> {} ({marked} variation(s))", old.display(), new.display()))
}

/// A path or a location without the old extracted mark (on any of its folders), with its separators kept.
fn unmark_extracted(path: &str) -> String {
    path.split_inclusive(['/', '\\'])
        .map(|part| {
            let (name, sep) = part.strip_suffix(['/', '\\']).map_or((part, ""), |n| (n, &part[n.len()..]));
            format!("{}{sep}", name.strip_suffix(OLD_EXTRACTED).unwrap_or(name))
        })
        .collect()
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

/// The attribute of a placed prefab: a group that is a reference to the prefab of this key. tile.wce (and a prefab file of the editor's)
/// keeps the group without its contents; the editor fills them in from the prefab when it reads the file (`expand_prefabs`) and leaves them
/// out when it writes it (`strip_prefab_contents`). For TEd the group is written with them (baked, as TEd copies a prefab).
pub const PREFAB_ATTR: &str = "wce_prefab";

/// A key's prefab contents, as a copy holds them (`PrefabLibrary::contents`).
pub type PrefabContents<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// A prefab reference in a text (outermost ones only): the range of its `children`'s contents (from just after `<children>` to the start of
/// `</children>`; a self-closing `<children/>` is the whole element, with `open` false), the indentation of that `children` line, its key,
/// and its overrides (`prefab_refs`) with the range of their element (from the line end before it to its end).
struct RefSpan {
    start: usize,
    end: usize,
    open: bool,
    indent: String,
    key: String,
    overrides: Vec<crate::prefab_refs::Override>,
    overrides_range: Option<(usize, usize)>,
}

/// The line end a text uses (its first line's): `\r\n` (TEd's tiles) or `\n`.
fn eol_of(text: &[u8]) -> &'static [u8] {
    match memchr_newline(text) {
        Some(i) if i > 0 && text[i - 1] == b'\r' => b"\r\n",
        _ => b"\n",
    }
}

fn memchr_newline(text: &[u8]) -> Option<usize> {
    text.iter().position(|&b| b == b'\n')
}

/// Lines with `\n` ends written with `eol` ones.
fn with_eol(text: &[u8], eol: &[u8]) -> Vec<u8> {
    if eol == b"\n" {
        return text.to_vec();
    }

    let mut out = Vec::with_capacity(text.len() + text.len() / 32);
    for line in text.split_inclusive(|&c| c == b'\n') {
        match line.strip_suffix(b"\n") {
            Some(l) => {
                out.extend_from_slice(l.strip_suffix(b"\r").unwrap_or(l));
                out.extend_from_slice(eol);
            }

            None => out.extend_from_slice(line),
        }
    }

    out
}

fn reference_spans(doc: &wce_formats::xml_doc::XmlDoc, text: &[u8]) -> Vec<RefSpan> {
    let els = doc.elements();
    let is_ref = |e: usize| doc.get(e, PREFAB_ATTR).is_some_and(|k| !k.is_empty());
    let mut out = vec![];
    for (e, el) in els.iter().enumerate() {
        if !is_ref(e) || std::iter::successors(el.parent, |&p| els[p].parent).any(is_ref) {
            continue;
        }

        let key = doc.get(e, PREFAB_ATTR).unwrap_or_default();
        let Some(&c) = el.children.iter().find(|&&c| els[c].name == "children") else { continue };
        let ch = &els[c];
        let line_start = |at: usize| text[..at].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let indent = String::from_utf8_lossy(&text[line_start(ch.start_off)..ch.start_off]).into_owned();
        let overrides_range = el.children.iter().find(|&&o| els[o].name == crate::prefab_refs::OVERRIDES_ELEMENT).map(|&o| (line_start(els[o].start_off).saturating_sub(1), els[o].end_off));
        let overrides = crate::prefab_refs::read_overrides(doc, e);
        let (start, end, open) = if ch.self_closing { (ch.start_off, ch.end_off, false) } else { (ch.start_end, ch.end_off - "</children>".len(), true) };
        out.push(RefSpan { start, end, open, indent, key, overrides, overrides_range });
    }

    out
}

/// The text without the contents of the prefab references (each `children` left empty), as tile.wce keeps them. `base`, a key's
/// contents (`PrefabLibrary::contents`), keeps what each reference's contents differ by from them as its overrides (`prefab_refs`); a
/// reference whose contents differ by more keeps them all. Without it, the contents just go. Returns the text and, for each reference that
/// kept its contents, why.
pub fn strip_prefab_contents_with(text: Vec<u8>, base: Option<PrefabContents<'_>>) -> (Vec<u8>, Vec<String>) {
    if !text.windows(PREFAB_ATTR.len()).any(|w| w == PREFAB_ATTR.as_bytes()) {
        return (text, vec![]);
    }

    let Ok(doc) = wce_formats::xml_doc::XmlDoc::parse(text.clone()) else { return (text, vec![]) };
    let eol = eol_of(&text);
    let mut out = Vec::with_capacity(text.len());
    let mut kept = vec![];
    let mut at = 0;
    for r in reference_spans(&doc, &text) {
        // What the contents differ by (the overrides read with them are in them already).
        let ovs = match base.map(|b| b(&r.key)) {
            Some(Some(b)) if r.start < r.end || r.open => match crate::prefab_refs::overrides(&text[r.start..r.end], &b) {
                Ok(ovs) => ovs,
                Err(why) => {
                    kept.push(format!("{}: {why}", r.key));
                    continue;
                }
            },

            _ => vec![],
        };

        out.extend_from_slice(&text[at..r.start]);
        if r.open {
            out.extend_from_slice(eol);
            out.extend_from_slice(r.indent.as_bytes());
        } else {
            out.extend_from_slice(b"<children/>");
        }

        at = r.end;

        // The old overrides element goes; the new one follows the `children` element.
        let close = if r.open { r.end + "</children>".len() } else { r.end };
        out.extend_from_slice(&text[at..close]);
        at = close;
        if let Some((o_start, o_end)) = r.overrides_range {
            out.extend_from_slice(&text[at..o_start.max(at)]);
            at = o_end;
        }

        if !ovs.is_empty() {
            out.extend_from_slice(eol);
            for line in with_eol(&crate::prefab_refs::overrides_text(&ovs), eol).split_inclusive(|&c| c == b'\n') {
                out.extend_from_slice(r.indent.as_bytes());
                out.extend_from_slice(line);
            }
        }
    }

    out.extend_from_slice(&text[at..]);
    (out, kept)
}

/// The text without the contents of the prefab references (each `children` left empty), their overrides dropped.
pub fn strip_prefab_contents(text: Vec<u8>) -> Vec<u8> {
    strip_prefab_contents_with(text, None).0
}

/// The text with the empty prefab references filled in from their prefabs (`contents`: a key's group contents, at column 0) and their
/// overrides applied (their element goes), the references inside those too (up to 8 deep). Returns the text and the keys no prefab was found
/// for (an override that no longer fits its prefab is left out, logged).
///
/// # Errors
///
/// The text does not read.
pub fn expand_prefabs(mut text: Vec<u8>, contents: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut missing = vec![];
    for _ in 0..8 {
        if !text.windows(PREFAB_ATTR.len()).any(|w| w == PREFAB_ATTR.as_bytes()) {
            break;
        }

        let doc = wce_formats::xml_doc::XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?;
        let eol = eol_of(&text);
        let mut out = Vec::with_capacity(text.len());
        let mut at = 0;
        let mut filled = 0;
        for r in reference_spans(&doc, &text) {
            let empty = text[r.start..r.end].iter().all(u8::is_ascii_whitespace) || !r.open;
            if !empty {
                continue;
            }

            let Some(body) = contents(&r.key) else {
                if !missing.contains(&r.key) {
                    missing.push(r.key);
                }

                continue;
            };

            let (body, lost) = crate::prefab_refs::apply(&body, &r.overrides)?;
            if !lost.is_empty() {
                wce_foundation::log::warn(format!("prefab {}: {} override(s) no longer fit it, left out", r.key, lost.len()));
            }

            out.extend_from_slice(&text[at..r.start]);
            if !r.open {
                out.extend_from_slice(b"<children>");
            }

            out.extend_from_slice(eol);
            for line in with_eol(&body, eol).split_inclusive(|&c| c == b'\n') {
                out.extend_from_slice(r.indent.as_bytes());
                out.extend_from_slice(b"    ");
                out.extend_from_slice(line);
            }

            if !out.ends_with(b"\n") {
                out.extend_from_slice(eol);
            }

            out.extend_from_slice(r.indent.as_bytes());
            if !r.open {
                out.extend_from_slice(b"</children>");
            }

            at = r.end;

            // Applied: the overrides' element goes (with its line).
            if let Some((o_start, o_end)) = r.overrides_range {
                out.extend_from_slice(&text[at..o_start.max(at)]);
                at = o_end;
            }

            filled += 1;
        }

        out.extend_from_slice(&text[at..]);
        text = out;
        if filled == 0 {
            break;
        }
    }

    Ok((text, missing))
}

/// The editor's attributes a polyfill turns into what TEd holds (reported as polyfilled, not as dropped).
const POLYFILLED_ATTRS: [&str; 3] = [PREFAB_ATTR, SURFACE_ATTR, ROTATION_ATTR];

/// An item placed on another thing ("Entity: Above"): `wce_surface="entity"`, kept as freeform at the height it was put at.
pub const SURFACE_ATTR: &str = "wce_surface";

/// An item tilted to the surface it stands on ("Follow surface"): `wce_rotation="surface"`, its orientation the tilted one.
pub const ROTATION_ATTR: &str = "wce_rotation";

/// A file's text (tile.wce, a prefab of the editor's) with its root element marked (`wce_version`), unless it is.
pub fn mark_wce_text(mut text: Vec<u8>) -> Vec<u8> {
    let name = WCE_VERSION_ATTR.as_bytes();

    // The root's start tag: the first `<` that starts an element (not `<?xml` or a comment).
    let mut start = None;
    for (i, w) in text.windows(2).enumerate() {
        if w[0] == b'<' && w[1].is_ascii_alphabetic() {
            start = Some(i);
            break;
        }
    }

    let Some(start) = start else { return text };
    let Some(end) = text[start..].iter().position(|&c| c == b'>').map(|i| start + i) else { return text };
    if text[start..end].windows(name.len()).any(|w| w == name) {
        return text;
    }

    let at = if text[end - 1] == b'/' { end - 1 } else { end };
    text.splice(at..at, format!(" {WCE_VERSION_ATTR}=\"{WCE_VERSION}\"").into_bytes());
    text
}

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

/// `tile.wce`'s text (or a tile.ted's, or a prefab's) as TEd reads it: the root's mark, the `wce_` attributes and the `Wce` elements go,
/// after the polyfills of the features that have one, and numbers get TEd's precision. A prefab reference must have its contents (the
/// editor's text, `expand_prefabs`).
///
/// # Errors
///
/// The text does not read.
pub fn to_ted(wce: Vec<u8>) -> Result<(Vec<u8>, TedReport), String> {
    let mut scene = TedScene::parse_any(wce)?;
    let mut report = TedReport::default();

    // The polyfills of the editor's features, before the attributes and elements TEd lacks are removed:
    // - a prefab reference is written as the group it holds (the text is the editor's, with the contents filled in): TEd's copy of a prefab.
    let refs = (0..scene.doc().elements().len()).filter(|&e| scene.doc().get(e, PREFAB_ATTR).is_some_and(|k| !k.is_empty())).count();
    if refs != 0 {
        report.polyfilled.push(format!("{refs} prefab reference(s) written as groups"));
    }

    // - an item on another thing (Entity: Above) is freeform at its height already: TEd gets it as such (the mark goes);
    // - a tilted item (Follow surface) has its tilted orientation: TEd gets it (the mark goes).
    let count = |attr: &str, value: &str| (0..scene.doc().elements().len()).filter(|&e| scene.doc().get(e, attr).is_some_and(|v| v == value)).count();
    let on_entities = count(SURFACE_ATTR, "entity");
    if on_entities != 0 {
        report.polyfilled.push(format!("{on_entities} item(s) placed on other things written as freeform at their height"));
    }

    let tilted = count(ROTATION_ATTR, "surface");
    if tilted != 0 {
        report.polyfilled.push(format!("{tilted} item(s) tilted to their surface written with that orientation"));
    }

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
                if a.name != WCE_VERSION_ATTR && a.name != EXTRACTED_ATTR && !POLYFILLED_ATTRS.contains(&a.name.as_str()) {
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

/// A tile database entry's XML (`<TILE ... location='terrain\tiles\...'>`) with its location's old extracted mark set (`mark`) or removed.
fn entry_with_location_mark(entry: &str, mark: bool) -> String {
    let Some(at) = entry.find(" location=") else { return entry.to_owned() };
    let quote_at = at + " location=".len();
    let Some(q) = entry[quote_at..].chars().next().filter(|c| *c == '\'' || *c == '"') else { return entry.to_owned() };
    let start = quote_at + 1;
    let Some(len) = entry[start..].find(q) else { return entry.to_owned() };
    let location = &entry[start..start + len];
    let new = if mark {
        if location.ends_with(OLD_EXTRACTED) { location.to_owned() } else { format!("{location}{OLD_EXTRACTED}") }
    } else {
        unmark_extracted(location)
    };

    format!("{}{new}{}", &entry[..start], &entry[start + len..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extracted_flag_is_on_the_root_and_old_projects_move() {
        let wce = b"<project version=\"16\" wce_version=\"1\">\n    <scene/>\n</project>\n".to_vec();
        assert!(!is_extracted_text(&wce));
        let marked = mark_extracted_text(wce);
        assert!(is_extracted_text(&marked) && String::from_utf8_lossy(&marked).starts_with("<project version=\"16\" wce_version=\"1\" wce_extracted_tile=\"true\">"));
        let (ted, report) = to_ted(marked).unwrap();
        assert!(!String::from_utf8_lossy(&ted).contains("wce_") && report == TedReport::default(), "the flag goes silently");

        // An old extracted project: moved to its own name, its entry too, its variations flagged; one whose place is taken stays.
        let tmp = tempfile::tempdir().unwrap();
        let raw = tmp.path().join("raw_data");
        let tiles = raw.join("terrain/tiles/battle/_tile_database/TILES");
        std::fs::create_dir_all(&tiles).unwrap();
        for (tile, taken) in [("farm_nordic", false), ("farm_western", true)] {
            let old = raw.join(format!("terrain/tiles/battle/{tile}/1x1_extracted/aa"));
            std::fs::create_dir_all(&old).unwrap();
            std::fs::write(old.join(WCE_FILE), b"<project version=\"16\" wce_version=\"1\">\n</project>\n").unwrap();
            let entry = format!("<TILE serialise_version='3' location='terrain\\tiles\\battle\\{tile}\\1x1_extracted' tile_set='{tile}'>\r\n</TILE>\r\n");
            std::fs::write(tiles.join(format!("{tile}_1x1_extracted.xml")), entry).unwrap();
            if taken {
                std::fs::create_dir_all(raw.join(format!("terrain/tiles/battle/{tile}/1x1/aa"))).unwrap();
            }
        }

        let m = migrate_extracted(&raw, &[]).unwrap();
        assert_eq!((m.moved.len(), m.skipped.len()), (1, 1), "{m:?}");
        let moved = raw.join("terrain/tiles/battle/farm_nordic/1x1/aa");
        assert!(is_extracted_dir(&moved) && !raw.join("terrain/tiles/battle/farm_nordic/1x1_extracted").exists());
        let entry = std::fs::read_to_string(tiles.join("farm_nordic_1x1.xml")).unwrap();
        assert!(entry.contains("location='terrain\\tiles\\battle\\farm_nordic\\1x1'") && !tiles.join("farm_nordic_1x1_extracted.xml").exists());
        assert!(tiles.join("farm_western_1x1_extracted.xml").exists(), "the taken one stays");

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
            "key=\"a\" wce_surface=\"entity\" wce_custom=\"x\" name=\"a\"",
        ).replace("            </children>", "                <WceSpline name=\"s\">\n                    <points/>\n                </WceSpline>\n            </children>");
        let (back, report) = to_ted(extra.into_bytes()).unwrap();
        let back = String::from_utf8(back).unwrap();
        assert!(!back.contains("wce_") && !back.contains("Wce"), "{back}");
        assert_eq!(report.dropped.len(), 2, "the spline and wce_custom: {report:?}");
        assert_eq!(report.polyfilled.len(), 1, "the item on another thing: {report:?}");
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

    #[test]
    fn a_prefab_reference_is_filled_stripped_and_baked() {
        let stored = b"<project version=\"16\" wce_version=\"1\">\n    <scene>\n        <UserInstanceGroup name=\"rocks\" wce_prefab=\"rocks\" position=\"vector(5,0,5)\">\n            <children>\n            </children>\n        </UserInstanceGroup>\n    </scene>\n</project>\n".to_vec();
        let rock = b"<BUILDING_INSTANCE key=\"rock\" name=\"rock\" position=\"vector(1,0,0)\"/>\n".to_vec();
        let (filled, missing) = expand_prefabs(stored.clone(), &|k| (k == "rocks").then(|| rock.clone())).unwrap();
        let text = String::from_utf8(filled.clone()).unwrap();
        assert!(missing.is_empty());
        assert!(text.contains("            <children>\n                <BUILDING_INSTANCE key=\"rock\""), "{text}");
        assert_eq!(strip_prefab_contents(filled.clone()), stored, "stripping gives back what was stored");

        let (ted, report) = to_ted(filled).unwrap();
        let ted = String::from_utf8(ted).unwrap();
        assert!(ted.contains("<UserInstanceGroup name=\"rocks\" position=\"vector(5,0,5)\">") && ted.contains("key=\"rock\""), "baked: {ted}");
        assert_eq!((report.polyfilled.len(), report.dropped.len()), (1, 0), "{report:?}");

        let (_, missing) = expand_prefabs(stored, &|_| None).unwrap();
        assert_eq!(missing, vec!["rocks".to_owned()]);
        assert!(String::from_utf8(mark_wce_text(b"<INSTANCE_SELECTION version=\"17\">\n</INSTANCE_SELECTION>\n".to_vec())).unwrap().starts_with("<INSTANCE_SELECTION version=\"17\" wce_version=\"1\">"));
    }

    #[test]
    fn a_references_overrides_are_kept_on_save_and_applied_on_open() {
        let stored = b"<project version=\"16\" wce_version=\"1\">\n    <scene>\n        <UserInstanceGroup name=\"rocks\" wce_prefab=\"rocks\" position=\"vector(5,0,5)\">\n            <children>\n            </children>\n        </UserInstanceGroup>\n    </scene>\n</project>\n".to_vec();
        let rock = b"<BUILDING_INSTANCE key=\"rock\" name=\"rock\" position=\"vector(1,0,0)\"/>\n<BUILDING_INSTANCE key=\"rock\" name=\"rock\" position=\"vector(2,0,0)\"/>\n".to_vec();
        let base = |k: &str| (k == "rocks").then(|| rock.clone());
        let (filled, _) = expand_prefabs(stored.clone(), &base).unwrap();

        // An edit of the contents is an override.
        let edited = String::from_utf8(filled).unwrap().replace("vector(2,0,0)", "vector(2,0,7)").into_bytes();
        let (saved, kept) = strip_prefab_contents_with(edited.clone(), Some(&base));
        let text = String::from_utf8(saved.clone()).unwrap();
        assert!(kept.is_empty());
        assert!(text.contains("            </children>\n            <WceOverrides>\n                <WceOverride path=\"1\" element=\"BUILDING_INSTANCE\" attr=\"position\" value=\"vector(2,0,7)\"/>\n            </WceOverrides>\n        </UserInstanceGroup>"), "{text}");
        assert!(!text.contains("vector(1,0,0)"), "the contents went: {text}");

        // Opened again: the very text edited.
        let (back, _) = expand_prefabs(saved.clone(), &base).unwrap();
        assert_eq!(String::from_utf8(back.clone()).unwrap(), String::from_utf8(edited.clone()).unwrap());

        // Saved again: the same file; for TEd: baked with the override, no trace of it.
        assert_eq!(strip_prefab_contents_with(back.clone(), Some(&base)).0, saved);
        let (ted, _) = to_ted(back).unwrap();
        let ted = String::from_utf8(ted).unwrap();
        assert!(ted.contains("vector(2,0,7)") && !ted.contains("Wce") && !ted.contains("wce_"), "{ted}");

        // Contents that differ by more keep them all.
        let more = String::from_utf8(edited).unwrap().replace("<BUILDING_INSTANCE key=\"rock\" name=\"rock\" position=\"vector(1,0,0)\"/>\n", "").into_bytes();
        let (kept_text, kept) = strip_prefab_contents_with(more.clone(), Some(&base));
        assert_eq!((kept_text, kept.len()), (more, 1));
    }
}
