//! The battle tile database's tile sets (`terrain/tiles/battle/_tile_database/_settings.xml`, `TILE_SETS`): read, and a new one added as
//! TEd's New Tile Set does (Phase 6b of docs/plans/tile_ted_parity.md; its dialog asks a name and a colour). The editor adds sets to the
//! projects' copy of the file only; "Save for Assembly Kit" carries the ones a tile uses into the kit's (`merge_into`).

use std::path::Path;

use wce_formats::xml_doc::XmlDoc;

/// A tile set as `_settings.xml` has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileSetInfo {
    pub name: String,

    /// 0..255 each, as written (floats).
    pub colour: [f64; 3],
    pub link_as_set: String,
    pub linking_tile: String,
    pub shared_geometry: String,
    pub also_place_tile_set: String,
}

/// The settings file of a raw_data folder.
pub fn settings_file(raw: &Path) -> std::path::PathBuf {
    raw.join("terrain/tiles/battle/_tile_database/_settings.xml")
}

/// The tile sets of a `_settings.xml`; empty when it can't be read.
pub fn read(path: &Path) -> Vec<TileSetInfo> {
    let Ok(doc) = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| XmlDoc::parse(b).map_err(|e| e.to_string())) else { return vec![] };
    doc.elements().iter().enumerate().filter(|(_, e)| e.name == "TILE_SET").map(|(i, _)| {
        let s = |k: &str| doc.get(i, k).unwrap_or_default();
        let f = |k: &str| s(k).trim().parse::<f64>().unwrap_or(0.0);
        TileSetInfo {
            name: s("name"), colour: [f("red"), f("green"), f("blue")], link_as_set: s("link_as_set"), linking_tile: s("linking_tile"),
            shared_geometry: s("shared_geometry"), also_place_tile_set: s("also_place_tile_set"),
        }
    }).collect()
}

/// A colour no set in `sets` has: the first three bytes of the MD5 of the name (as TEd picks a battlefield's), one more while taken.
pub fn free_colour(name: &str, sets: &[TileSetInfo]) -> [u8; 3] {
    let taken = sets.iter().map(|s| s.colour.map(|c| c.round().clamp(0.0, 255.0) as u8)).collect::<std::collections::HashSet<_>>();
    crate::new_tile::colour_of(&format!("tile_set/{name}"), None, &taken)
}

/// Adds a tile set to a `_settings.xml` (the line TEd writes, before `</TILE_SETS>`, with the file's indentation and line ends).
///
/// # Errors
///
/// The file can't be read or has no `TILE_SETS`, the set exists, or the file can't be written.
pub fn add(path: &Path, set: &TileSetInfo) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if read(path).iter().any(|s| s.name == set.name) {
        return Err(format!("there is a tile set '{}' already", set.name));
    }

    let mut doc = XmlDoc::parse(bytes).map_err(|e| e.to_string())?;
    let list = doc.elements().iter().position(|e| e.name == "TILE_SETS").ok_or("no TILE_SETS in the tile database settings")?;
    let el = &doc.elements()[list];
    if el.self_closing {
        return Err("the tile database settings have an empty TILE_SETS".to_owned());
    }

    // The line of the last set gives the indentation and the line end.
    let text = doc.original_text().to_vec();
    let close = el.end_off - "</TILE_SETS>".len();
    let line_start = text[..close].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    let crlf = line_start >= 2 && text[line_start - 2] == b'\r';
    let nl = if crlf { "\r\n" } else { "\n" };
    let child = el.children.last().map(|&c| doc.elements()[c].start_off).unwrap_or(close);
    let child_line = text[..child].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    let indent = String::from_utf8_lossy(&text[child_line..child]).into_owned();
    let line = format!(
        "{indent}<TILE_SET link_as_set=\"{}\" serialise_version=\"1\" linking_tile=\"{}\" shared_geometry=\"{}\" also_place_tile_set=\"{}\" red=\"{:.6}\" blue=\"{:.6}\" name=\"{}\" green=\"{:.6}\"/>{nl}",
        set.link_as_set, set.linking_tile, set.shared_geometry, set.also_place_tile_set, set.colour[0], set.colour[2], set.name, set.colour[1]
    );
    doc.splice(line_start, 0, line.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::write(path, doc.save()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Carries tile sets of `from` (a projects' `_settings.xml`) named in `names` that `into` (a kit's) lacks into it; `into` gets a `.bak` when
/// it changes. Returns the sets added.
///
/// # Errors
///
/// A file can't be read or written.
pub fn merge_into(from: &Path, into: &Path, names: &[String]) -> Result<Vec<String>, String> {
    let (ours, theirs) = (read(from), read(into));
    let missing = names.iter().filter(|n| !n.is_empty() && !theirs.iter().any(|s| &s.name == *n)).filter_map(|n| ours.iter().find(|s| &s.name == n)).cloned().collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(vec![]);
    }

    let bak = std::path::PathBuf::from(format!("{}.bak", into.display()));
    if !bak.exists() {
        std::fs::copy(into, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
    }

    for s in &missing {
        add(into, s)?;
    }

    Ok(missing.into_iter().map(|s| s.name).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_is_added_as_teds_line() {
        let dir = std::env::temp_dir().join(format!("wce_tile_sets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("_settings.xml");
        let text = "<TILE_DATABASE serialise_version=\"1\">\r\n    <TILE_SETS>\r\n        <TILE_SET link_as_set=\"\" serialise_version=\"1\" linking_tile=\"\" shared_geometry=\"\" also_place_tile_set=\"\" red=\"1.000000\" blue=\"3.000000\" name=\"a\" green=\"2.000000\"/>\r\n    </TILE_SETS>\r\n</TILE_DATABASE>\r\n";
        std::fs::write(&path, text).unwrap();
        add(&path, &TileSetInfo { name: "b".to_owned(), colour: [4.0, 5.0, 6.0], ..Default::default() }).unwrap();
        let sets = read(&path);
        assert_eq!(sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(sets[1].colour, [4.0, 5.0, 6.0]);
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("\r\n        <TILE_SET link_as_set=\"\" serialise_version=\"1\" linking_tile=\"\" shared_geometry=\"\" also_place_tile_set=\"\" red=\"4.000000\" blue=\"6.000000\" name=\"b\" green=\"5.000000\"/>\r\n    </TILE_SETS>"));
        assert!(add(&path, &TileSetInfo { name: "b".to_owned(), ..Default::default() }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
