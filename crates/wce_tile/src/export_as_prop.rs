//! Export as prop: a building marked `wce_export_as_prop="true"` is written to the game, and for TEd, as plain models instead of a
//! building: one `SIMPLE_MESH_INSTANCE` per piece, at the building's place and destruction level. The tile keeps the building; only the
//! text an export reads is converted ([`apply`]), so everything after it (the building lists, the terrain stamps, the outlines, the prop
//! markers) sees models and no building: no collision, destruction, effects or logic. TEd has no such option (newer games' tools do).
//!
//! A building stays a building, and the log says why, when it cannot be a prop: a capture location links it, or a piece of it has no
//! plain mesh (the siege engines' skinned bodies and wheels).
//!
//! BOB takes every `SIMPLE_MESH_INSTANCE` for a water plane and stops at the first model that is not one (`rasterise_water_planes`), so
//! the pieces carry [`PROP_PIECE_SUFFIX`] in their names and our stage B leaves them out (`compile::water_planes`). TEd's own export has
//! no such exception: [`PropExport::water_after`] counts the water planes it would lose.

use std::collections::HashMap;

use wce_formats::xml_doc::XmlDoc;

use crate::building_pieces::{destruct_number, piece_copy, piece_tag, static_piece, working_anim};
use crate::instance_templates::template;

/// The attribute of a `BUILDING_INSTANCE` that exports it as a prop.
pub const EXPORT_AS_PROP_ATTR: &str = "wce_export_as_prop";

/// Why the Inspector greys out the checkbox of a building with a capture location.
pub const CAPTURE_LOCATION_TIP: &str = "A capture location links this building, so it has to stay a building: clear its capture_location to export it as a prop.";

/// Why the Inspector greys out the scale of a building that does not export as a prop.
pub const SCALE_TIP: &str = "The game ignores the scale of a building: it only applies to a building exported as a prop. Kept here for when Export as prop is on again.";

/// Whether a building exports as a prop, from its attributes' values: marked, and no capture location keeps it a building. The one thing
/// the scale of a building depends on: the game ignores it otherwise (docs/re/buildings.md).
pub fn exports_as_prop(mark: Option<&str>, capture_location: Option<&str>) -> bool {
    mark == Some("true") && !capture_location_set(capture_location)
}

/// Whether a building's `capture_location` value links it to a capture location, which keeps it a building.
pub fn capture_location_set(value: Option<&str>) -> bool {
    value.is_some_and(|c| !c.trim().is_empty())
}

/// How the name of a model made from a building ends. Never in a tile of the kit, so that its water stays as BOB makes it.
pub const PROP_PIECE_SUFFIX: &str = "__wce_prop";

/// What [`apply`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PropExport {
    /// The buildings written as models, and how many models that made.
    pub buildings: usize,
    pub pieces: usize,

    /// The marked buildings that stayed buildings: name and why.
    pub kept: Vec<(String, String)>,

    /// The tile's water planes after the first such model: lost in TEd's own export, where BOB stops at that model.
    pub water_after: usize,
}

impl PropExport {
    /// A line for people per thing to say.
    pub fn notes(&self) -> Vec<String> {
        let mut out = self.written_note().into_iter().collect::<Vec<_>>();
        out.extend(self.kept_notes());
        out
    }

    /// What was written as models.
    pub fn written_note(&self) -> Option<String> {
        (self.buildings != 0).then(|| format!("export as prop: {} building(s) written as {} model(s)", self.buildings, self.pieces))
    }

    /// What stayed a building. One line per building and reason, however many times a tile places it.
    pub fn kept_notes(&self) -> Vec<String> {
        let mut out = vec![];
        let mut kept = std::collections::BTreeMap::<&(String, String), usize>::new();
        for k in &self.kept {
            *kept.entry(k).or_default() += 1;
        }

        out.extend(kept.iter().map(|((name, why), n)| format!("export as prop: '{name}' (x{n}) stays a building: {why}")));
        out
    }

    /// Into the report of a prefab's conversion for TEd: the models are a polyfill, what stayed a building is the feature dropped. A prefab
    /// does not know the tiles that will place it, so what TEd's export would lose is said whenever it has such models.
    pub fn add_to(&self, report: &mut crate::project_files::TedReport) {
        if let Some(line) = self.written_note() {
            report.polyfilled.push(line);
            report.polyfilled.push("export as prop: in a tile that places this prefab, BOB stops at these models: water planes after them are lost in TEd's own export".to_owned());
        }

        report.dropped.extend(self.kept_notes());
    }

    /// [`Self::notes`] and what only TEd's export has to fear.
    pub fn kit_notes(&self) -> Vec<String> {
        let mut out = self.notes();
        if self.water_after != 0 {
            out.push(format!("export as prop: {} water plane(s) come after a prop model: BOB stops there, so TEd's own export would lose them (this editor's export does not)", self.water_after));
        }

        out
    }
}

/// The models of a building at a destruction level (0 intact), from the files of its folder: one per piece that stands at level 0, each at
/// the level or the last it has.
///
/// # Errors
///
/// No model, or a piece that exists only as an animation (the message says which).
pub fn pieces_at(files: &[String], level: i64) -> Result<Vec<String>, String> {
    let name = |p: &str| p[p.rfind('/').map_or(0, |s| s + 1)..].to_owned();
    let files = files.iter().filter(|p| !piece_copy(p, |q| files.iter().any(|f| f == q))).collect::<Vec<_>>();
    let intact = files.iter().filter(|p| static_piece(&name(p)) && (!p.contains("destruct") || p.contains("destruct01"))).collect::<Vec<_>>();
    if intact.is_empty() && files.iter().all(|p| !working_anim(&name(p))) {
        return Err("no model in the game's files".to_owned());
    }

    let has_tag = |tag: &str| intact.iter().any(|p| piece_tag(&name(p)) == Some(tag));
    if files.iter().any(|p| working_anim(&name(p)) && destruct_number(&name(p)) == 1 && piece_tag(&name(p)).is_none_or(|t| !has_tag(t))) {
        return Err("a piece has no plain mesh (it stands as an animation)".to_owned());
    }

    let wanted = u32::try_from(level.max(0)).unwrap_or(0).saturating_add(1);
    let mut out = intact
        .iter()
        .map(|p| {
            // The highest of the piece's levels up to the wanted one.
            (2..=wanted)
                .rev()
                .map(|l| p.replacen("destruct01", &format!("destruct{l:02}"), 1))
                .find(|q| p.contains("destruct01") && files.iter().any(|f| *f == q))
                .unwrap_or_else(|| (**p).clone())
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

/// The `.rigid_model_v2` files under a folder of the game's files (`rigidmodels/buildings/<key>/`).
pub type ModelsIn<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// `text` (a tile's scene, the editor's or TEd's) with its marked buildings as models; as it is without any. Without the game's files
/// every marked building stays one.
///
/// # Errors
///
/// The text does not read.
pub fn apply(text: Vec<u8>, models_in: Option<ModelsIn>) -> Result<(Vec<u8>, PropExport), String> {
    let mut report = PropExport::default();
    if !text.windows(EXPORT_AS_PROP_ATTR.len()).any(|w| w == EXPORT_AS_PROP_ATTR.as_bytes()) {
        return Ok((text, report));
    }

    let template = template("SIMPLE_MESH_INSTANCE").ok_or("no template for a model")?;
    let doc = XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?;
    let mut folders = HashMap::<String, Vec<String>>::new();
    let mut cuts: Vec<(usize, usize, Vec<u8>)> = vec![];
    let mut first = None;
    for (e, el) in doc.elements().iter().enumerate() {
        if el.name != "BUILDING_INSTANCE" || doc.get(e, EXPORT_AS_PROP_ATTR).as_deref() != Some("true") {
            continue;
        }

        let name = doc.get(e, "name").unwrap_or_default();
        let key = doc.get(e, "key").unwrap_or_default().to_lowercase();
        let level = doc.get(e, "destruction_level").and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        let linked = capture_location_set(doc.get(e, "capture_location").as_deref());
        let pieces = if linked {
            Err("a capture location links it".to_owned())
        } else if let Some(models_in) = models_in {
            let folder = folders.entry(key.clone()).or_insert_with(|| models_in(&format!("rigidmodels/buildings/{key}/")));
            pieces_at(folder, level)
        } else {
            Err("the game's files are not mounted".to_owned())
        };

        match pieces {
            Ok(pieces) => {
                let ind = indent(&text, el.start_off);
                let mut with = vec![];
                for (k, piece) in pieces.iter().enumerate() {
                    if k != 0 {
                        with.extend_from_slice(newline(&text));
                        with.extend_from_slice(&ind);
                    }

                    with.extend(model_text(&doc, e, template.attrs, piece, k));
                }

                cuts.push((el.start_off, el.end_off, with));
                report.buildings += 1;
                report.pieces += pieces.len();
                first.get_or_insert(e);
            }

            // The mark goes quietly: this was said here.
            Err(why) => {
                report.kept.push((name, why));
                if let Some(a) = el.attrs.iter().find(|a| a.name == EXPORT_AS_PROP_ATTR) {
                    cuts.push((a.value_off, a.value_off + a.value_len, b"false".to_vec()));
                }
            }
        }
    }

    if let Some(first) = first {
        let water = |e: usize| doc.elements()[e].name == "SIMPLE_MESH_INSTANCE" && doc.get(e, "key").is_some_and(|k| k.to_lowercase().contains("water_plane"));
        report.water_after = (first..doc.elements().len()).filter(|&e| water(e)).count();
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

/// The element of piece `k` of a building: a new model's attributes, with the building's place and flags, in the file's encoding.
fn model_text(doc: &XmlDoc, building: usize, attrs: &[(&str, &str)], piece: &str, k: usize) -> Vec<u8> {
    let own = |name: &str| doc.elements()[building].attrs.iter().position(|a| a.name == name).map(|i| doc.attr_raw(building, i).to_vec());
    let mut s = b"<SIMPLE_MESH_INSTANCE".to_vec();
    for (name, default) in attrs {
        let value = match *name {
            "key" => piece.as_bytes().to_vec(),
            "scale" => own("scale").unwrap_or_else(|| b"vector(1,1,1)".to_vec()),
            "name" => {
                let mut n = own("name").unwrap_or_default();
                n.extend_from_slice(format!("_{k}{PROP_PIECE_SUFFIX}").as_bytes());
                n
            }

            "position" | "orientation" | "surface_mode" | "lf_offset" | "flags" => own(name).unwrap_or_else(|| default.as_bytes().to_vec()),
            _ => default.as_bytes().to_vec(),
        };

        s.push(b' ');
        s.extend_from_slice(name.as_bytes());
        s.extend_from_slice(b"=\"");
        s.extend_from_slice(&value);
        s.push(b'"');
    }

    s.extend_from_slice(b"/>");
    s
}

/// The white space before the element at `at`, on its line.
fn indent(text: &[u8], at: usize) -> Vec<u8> {
    let start = text[..at].iter().rposition(|&c| c == b'\n').map_or(0, |n| n + 1);
    text[start..at].iter().copied().take_while(u8::is_ascii_whitespace).collect()
}

fn newline(text: &[u8]) -> &'static [u8] {
    if text.windows(2).any(|w| w == b"\r\n") { b"\r\n" } else { b"\n" }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WATER: &str = "<SIMPLE_MESH_INSTANCE key=\"RigidModels/Buildings/water_plane/water_plane.rigid_model_v2\" name=\"water_plane\"/>";

    /// A folder per building.
    fn models_in(prefix: &str) -> Vec<String> {
        match prefix {
            "rigidmodels/buildings/house/" => vec![format!("{prefix}house_piece01_destruct01.rigid_model_v2"), format!("{prefix}house_piece01_destruct02.rigid_model_v2"), format!("{prefix}house_piece02_destruct01.rigid_model_v2")],
            "rigidmodels/buildings/ram/" => vec![format!("{prefix}ram_piece01_destruct01_weighted_anim.rigid_model_v2")],
            _ => vec![],
        }
    }

    fn scene(attrs: &str) -> String {
        format!("<project version=\"16\">\n    <BUILDING_INSTANCE name=\"a&amp;b\" key=\"house\" position=\"vector(1,2,3)\" orientation=\"euler(0,1,0)\" surface_mode=\"MSM_LANDSCAPE\" {attrs}/>\n    {WATER}\n</project>\n")
    }

    fn folder(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| format!("rigidmodels/buildings/k/k_{n}.rigid_model_v2")).collect()
    }

    #[test]
    fn a_piece_goes_as_far_as_it_has_levels() {
        let f = folder(&["piece01_destruct01", "piece01_destruct02", "piece01_destruct01_anim", "piece02_destruct01", "piece01_destruct01_0"]);
        assert_eq!(pieces_at(&f, 0).unwrap(), folder(&["piece01_destruct01", "piece02_destruct01"]));
        assert_eq!(pieces_at(&f, 1).unwrap(), folder(&["piece01_destruct02", "piece02_destruct01"]), "piece02 has one level");
        assert_eq!(pieces_at(&f, 9).unwrap(), folder(&["piece01_destruct02", "piece02_destruct01"]));
    }

    #[test]
    fn a_model_without_levels_and_a_gate_in_its_closed_state() {
        let plain = vec!["rigidmodels/buildings/rock/rock.rigid_model_v2".to_owned()];
        assert_eq!(pieces_at(&plain, 0).unwrap(), plain);
        let gate = folder(&["piece01_destruct01_gate_closed", "piece01_destruct01_gate_open", "piece01_destruct01_gate_opening_anim"]);
        assert_eq!(pieces_at(&gate, 0).unwrap(), folder(&["piece01_destruct01_gate_closed"]));
    }

    #[test]
    fn what_cannot_be_a_prop_says_so() {
        assert!(pieces_at(&[], 0).is_err());
        assert!(pieces_at(&folder(&["piece01_destruct01_weighted_anim"]), 0).is_err());
        let wheels = folder(&["piece01_destruct01", "piece02_destruct01_left_wheel_looped_anim"]);
        assert!(pieces_at(&wheels, 0).is_err(), "one animated piece keeps the whole building");
    }

    #[test]
    fn a_marked_building_becomes_its_pieces_in_place() {
        let (out, report) = apply(scene("wce_export_as_prop=\"true\"").into_bytes(), Some(&models_in)).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("BUILDING_INSTANCE"), "{out}");
        assert_eq!(out.matches("<SIMPLE_MESH_INSTANCE").count(), 3, "two pieces and the water: {out}");
        assert!(out.contains("key=\"rigidmodels/buildings/house/house_piece01_destruct01.rigid_model_v2\""), "{out}");
        assert!(out.contains("name=\"a&amp;b_1__wce_prop\""), "the name's encoding is kept: {out}");
        assert!(out.contains("position=\"vector(1,2,3)\"") && out.contains("orientation=\"euler(0,1,0)\"") && out.contains("surface_mode=\"MSM_LANDSCAPE\"") && out.contains("scale=\"vector(1,1,1)\""), "{out}");
        assert!(out.contains(">\n    <SIMPLE_MESH_INSTANCE"), "each on its line at the building's indent: {out}");
        assert!(XmlDoc::parse(out.into_bytes()).is_ok());
        assert_eq!(report, PropExport { buildings: 1, pieces: 2, kept: vec![], water_after: 1 });
    }

    #[test]
    fn a_scaled_building_gives_scaled_models() {
        let (out, _) = apply(scene("scale=\"vector(2,0.5,1)\" wce_export_as_prop=\"true\"").into_bytes(), Some(&models_in)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().matches("scale=\"vector(2,0.5,1)\"").count(), 2);
    }

    #[test]
    fn the_destruction_level_picks_the_piece_files() {
        let (out, _) = apply(scene("destruction_level=\"1\" wce_export_as_prop=\"true\"").into_bytes(), Some(&models_in)).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("house_piece01_destruct02") && out.contains("house_piece02_destruct01") && !out.contains("house_piece01_destruct01"), "{out}");
    }

    #[test]
    fn unmarked_tiles_are_not_touched() {
        for text in [scene(""), scene("wce_export_as_prop=\"false\"")] {
            let (out, report) = apply(text.clone().into_bytes(), Some(&models_in)).unwrap();
            assert_eq!((out, report), (text.into_bytes(), PropExport::default()));
        }
    }

    #[test]
    fn a_building_that_cannot_be_a_prop_stays_with_a_reason() {
        let linked = scene("capture_location=\"cl\" wce_export_as_prop=\"true\"");
        let (out, report) = apply(linked.into_bytes(), Some(&models_in)).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("BUILDING_INSTANCE") && out.contains("wce_export_as_prop=\"false\""), "{out}");
        assert_eq!(report.buildings, 0);
        assert!(report.notes()[0].contains("capture location"), "{:?}", report.notes());
        let twice = PropExport { kept: vec![("a".to_owned(), "why".to_owned()); 2], ..Default::default() };
        assert_eq!(twice.notes(), ["export as prop: 'a' (x2) stays a building: why"], "the log is not flooded");
        let ram = scene("wce_export_as_prop=\"true\"").replace("key=\"house\"", "key=\"ram\"");
        assert!(apply(ram.into_bytes(), Some(&models_in)).unwrap().1.kept[0].1.contains("plain mesh"));
        assert_eq!(apply(scene("wce_export_as_prop=\"true\"").into_bytes(), None).unwrap().1.kept.len(), 1, "no game files");
    }

    #[test]
    fn water_before_the_first_prop_is_safe_from_teds_export() {
        let text = scene("wce_export_as_prop=\"true\"").replacen(&format!("    {WATER}\n"), "", 1).replacen("    <BUILDING", &format!("    {WATER}\n    <BUILDING"), 1);
        let (_, report) = apply(text.into_bytes(), Some(&models_in)).unwrap();
        assert_eq!(report.water_after, 0);
        assert!(report.kit_notes().iter().all(|n| !n.contains("water")));
    }
}
