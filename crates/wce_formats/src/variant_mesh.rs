//! Variant mesh definitions (`.variantmeshdefinition`): what a unit's model is made of. XML, kept in an [`XmlDoc`] so that an editor can
//! change a value and leave the rest of the file as it was.
//!
//! `VARIANT_MESH` is a mesh (`model`, its decals and imposter, `mask0..2`) with `TINT`s and `SLOT`s, or just a group of slots; `SLOT` (`name`,
//! `attach_point`, `probability`) holds the alternatives for one part, `VARIANT_MESH`es and `VARIANT_MESH_REFERENCE`s (`definition`, another file). The game
//! picks among a slot's alternatives; picking is not this module's business.
//!
//! Mods ship hand-edited files that aren't XML (11 of TDD's 8,500: `/ >`, doubled quotes, end tags left over or left out, the game takes them all). What doesn't
//! parse is read through [`repair`], which keeps what the game most likely reads.

use crate::xml_doc::{XmlDoc, XmlError};

/// A `TINT`: one of the colours a mesh can take.
#[derive(Clone, Debug, PartialEq)]
pub struct Tint {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub random_range: f32,
}

/// A `VARIANT_MESH`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariantMesh {
    /// Its element in the document.
    pub element: usize,
    pub model: Option<String>,
    pub decal_diffuse: Option<String>,
    pub decal_normal: Option<String>,
    pub imposter_model: Option<String>,

    /// `mask0`, `mask1`, `mask2`.
    pub masks: [Option<u8>; 3],
    pub tints: Vec<Tint>,

    /// The text of its `META_DATA` elements (`female`).
    pub meta_data: Vec<String>,
    pub slots: Vec<Slot>,
}

/// One alternative of a slot.
#[derive(Clone, Debug, PartialEq)]
pub enum Alternative {
    Mesh(VariantMesh),

    /// A `VARIANT_MESH_REFERENCE`: its element and the file it names.
    Reference { element: usize, definition: String },
}

/// A `SLOT`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slot {
    pub element: usize,
    pub name: String,

    /// A bone without its `bn_` (`weapon_03`), an attachment point of the parent mesh (`crest_centre`), or a list of them (ships).
    pub attach_point: Option<String>,

    /// How likely the slot is filled at all (absent: always; ASSUMPTION, by its name and values, not traced).
    pub probability: Option<f32>,
    pub alternatives: Vec<Alternative>,
}

/// A parsed definition.
pub struct VariantMeshFile {
    pub doc: XmlDoc,
    pub root: VariantMesh,

    /// The file wasn't XML: `doc` is [`repair`]'s text, not the file's.
    pub repaired: bool,
}

fn number(doc: &XmlDoc, element: usize, name: &str) -> f32 {
    doc.get(element, name).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
}

fn text(doc: &XmlDoc, element: usize) -> String {
    let e = &doc.elements()[element];
    let bytes = doc.text_of_range(e.start_end, e.end_off);
    let inner = bytes.iter().rposition(|b| *b == b'<').map_or(&bytes[..], |p| &bytes[..p]);
    String::from_utf8_lossy(inner).trim().to_owned()
}

fn mesh(doc: &XmlDoc, element: usize) -> VariantMesh {
    let attr = |name: &str| doc.get(element, name);
    let mask = |name: &str| attr(name).and_then(|v| v.trim().parse().ok());
    let mut out = VariantMesh {
        element, model: attr("model"), decal_diffuse: attr("decal_diffuse"), decal_normal: attr("decal_normal"), imposter_model: attr("imposter_model"),
        masks: [mask("mask0"), mask("mask1"), mask("mask2")], ..VariantMesh::default()
    };

    for &child in &doc.elements()[element].children {
        match doc.elements()[child].name.as_str() {
            "TINT" => out.tints.push(Tint {
                red: number(doc, child, "red"), green: number(doc, child, "green"), blue: number(doc, child, "blue"), random_range: number(doc, child, "random_range"),
            }),
            "META_DATA" => out.meta_data.push(text(doc, child)),
            "SLOT" => out.slots.push(slot(doc, child)),
            _ => {}
        }
    }

    out
}

fn slot(doc: &XmlDoc, element: usize) -> Slot {
    let alternatives = doc.elements()[element].children.iter().filter_map(|&child| match doc.elements()[child].name.as_str() {
        "VARIANT_MESH" => Some(Alternative::Mesh(mesh(doc, child))),
        "VARIANT_MESH_REFERENCE" => Some(Alternative::Reference { element: child, definition: doc.get(child, "definition").unwrap_or_default() }),
        _ => None,
    }).collect();
    Slot {
        element, name: doc.get(element, "name").unwrap_or_default(), attach_point: doc.get(element, "attach_point"),
        probability: doc.get(element, "probability").and_then(|v| v.trim().parse().ok()), alternatives,
    }
}

/// The elements that hold no others: left open, what comes next is not theirs.
const LEAVES: [&[u8]; 2] = [b"TINT", b"VARIANT_MESH_REFERENCE"];

/// A definition that isn't XML as XML. **ASSUMPTION** (the game's reader isn't traced; what it makes of these files is what they were tested with):
/// - `/ >` closes a tag, and a quote where none belongs is dropped: `a=""b"`, `a="b"" c`;
/// - a leaf is closed where it is opened;
/// - an end tag closes the innermost open element if it names it and is left out if not: closing back to a match would end the slot, or the file, early;
/// - what is open at the end is closed there.
///
/// Comments and the text between tags stay. The file's bytes are taken one by one: no value here needs decoding.
fn repair(file: &[u8]) -> Vec<u8> {
    fn is_name_end(c: u8) -> bool {
        c.is_ascii_whitespace() || matches!(c, b'=' | b'>' | b'/' | b'"')
    }

    let (mut out, mut open) = (Vec::with_capacity(file.len()), Vec::<&[u8]>::new());
    let mut p = 0;
    while let Some(from) = file[p..].iter().position(|&c| c == b'<').map(|k| p + k) {
        out.extend_from_slice(&file[p..from]);
        p = from;

        // Comments and declarations, as they are.
        if file[p..].starts_with(b"<!") || file[p..].starts_with(b"<?") {
            let end: &[u8] = if file[p..].starts_with(b"<!--") { b"-->" } else { b">" };
            let close = file[p..].windows(end.len()).position(|w| w == end).map_or(file.len(), |k| p + k + end.len());
            out.extend_from_slice(&file[p..close]);
            p = close;
            continue;
        }

        let closing = file.get(p + 1) == Some(&b'/');
        p += if closing { 2 } else { 1 };
        let start = p;
        while p < file.len() && !is_name_end(file[p]) {
            p += 1;
        }

        let name = &file[start..p];
        if closing {
            p = file[p..].iter().position(|&c| c == b'>').map_or(file.len(), |k| p + k + 1);
            if open.last() == Some(&name) {
                open.pop();
                out.extend_from_slice(&[b"</", name, b">"].concat());
            }

            continue;
        }

        out.push(b'<');
        out.extend_from_slice(name);
        let mut empty = false;
        while p < file.len() {
            match file[p] {
                b'>' => {
                    p += 1;
                    break;
                }

                b'/' => {
                    empty = true;
                    p += 1;
                }

                c if c.is_ascii_whitespace() || c == b'"' => p += 1,
                _ => {
                    let attr = p;
                    while p < file.len() && !is_name_end(file[p]) {
                        p += 1;
                    }

                    let attr = &file[attr..p];
                    while p < file.len() && file[p].is_ascii_whitespace() {
                        p += 1;
                    }

                    let mut value: &[u8] = b"";
                    if file.get(p) == Some(&b'=') {
                        p += 1;
                        while p < file.len() && file[p].is_ascii_whitespace() {
                            p += 1;
                        }

                        // `a=""` is empty, unless the value follows: `a=""b"`.
                        if file.get(p) == Some(&b'"') {
                            p += 1;
                            if file.get(p) == Some(&b'"') && file.get(p + 1).is_some_and(|&c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>')) {
                                p += 1;
                            }

                            let begin = p;
                            while p < file.len() && file[p] != b'"' {
                                p += 1;
                            }

                            value = &file[begin..p];
                            p = (p + 1).min(file.len());
                        }
                    }

                    if !attr.is_empty() {
                        out.extend_from_slice(&[b" ", attr, b"=\"", value, b"\""].concat());
                    } else {
                        p += 1;
                    }
                }
            }
        }

        if empty || LEAVES.contains(&name) {
            out.extend_from_slice(b"/>");
        } else {
            out.push(b'>');
            open.push(name);
        }
    }

    out.extend_from_slice(&file[p..]);
    while let Some(name) = open.pop() {
        out.extend_from_slice(&[b"</", name, b">"].concat());
    }

    out
}

impl VariantMeshFile {
    /// Parses a definition, repairing one that isn't XML.
    ///
    /// # Errors
    ///
    /// Returns an error if not even that makes it XML, or its root isn't a `VARIANT_MESH`.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, XmlError> {
        let (doc, repaired) = match XmlDoc::parse(bytes.clone()) {
            Ok(doc) => (doc, false),
            Err(_) => (XmlDoc::parse(repair(&bytes))?, true),
        };

        let root = doc.roots().iter().copied().find(|&r| doc.elements()[r].name == "VARIANT_MESH").ok_or_else(|| XmlError::Edit("not a variant mesh definition".to_owned()))?;
        let root = mesh(&doc, root);
        Ok(Self { doc, root, repaired })
    }
}

#[cfg(test)]
mod tests {
    use super::{Alternative, VariantMesh, VariantMeshFile};

    const FILE: &str = "<VARIANT_MESH>\r\n\t<SLOT name=\"skin\" >\r\n\t\t<VARIANT_MESH model=\"a/b.rigid_model_v2\"  mask0=\"2\"  mask2=\"1\" >\r\n\t\t\t<TINT red=\"1.0\" green=\"0.5\" blue=\"0.25\" random_range=\"0.1\" />\r\n\t\t\t<META_DATA>female</META_DATA>\t<SLOT name=\"crest\" attach_point=\"crest_centre\" probability=\"0.25\" />\r\n\t\t</VARIANT_MESH>\r\n\t\t<VARIANT_MESH_REFERENCE definition=\"c/d.VariantMeshDefinition\" />\r\n\t</SLOT>\r\n</VARIANT_MESH>\r\n";

    fn repaired(file: &str) -> VariantMesh {
        let f = VariantMeshFile::parse(file.as_bytes().to_vec()).expect("repairs");
        assert!(f.repaired, "{file}");
        f.root
    }

    fn models(mesh: &VariantMesh) -> Vec<&str> {
        mesh.slots[0].alternatives.iter().filter_map(|a| if let Alternative::Mesh(m) = a { m.model.as_deref() } else { None }).collect()
    }

    #[test]
    fn a_tag_closed_with_a_space_is_closed() {
        let m = repaired("<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"><SLOT name=\"t\"><VARIANT_MESH model=\"b\"/ ></SLOT></VARIANT_MESH><VARIANT_MESH model=\"c\"/></SLOT></VARIANT_MESH>");
        assert_eq!(models(&m), ["a", "c"]);
        let Alternative::Mesh(a) = &m.slots[0].alternatives[0] else { panic!("a mesh") };
        assert_eq!(models(a), ["b"]);
    }

    #[test]
    fn doubled_quotes_go_and_empty_values_stay() {
        let m = repaired("<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\" imposter_model=\"\"i\" /><VARIANT_MESH model=\"b\"\" mask0=\"2\"/><VARIANT_MESH model=\"\" mask0=\"1\"/ ></SLOT></VARIANT_MESH>");
        assert_eq!(models(&m), ["a", "b", ""]);
        let Alternative::Mesh(a) = &m.slots[0].alternatives[0] else { panic!("a mesh") };
        assert_eq!(a.imposter_model.as_deref(), Some("i"));
        let Alternative::Mesh(b) = &m.slots[0].alternatives[1] else { panic!("a mesh") };
        assert_eq!(b.masks[0], Some(2));
    }

    #[test]
    fn a_tint_left_open_ends_nothing() {
        // vik_tunics_poor: the mesh is closed, the tint open, and its end tag ends neither.
        let m = repaired("<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"/><TINT red=\"1\" green=\"1\" blue=\"1\" random_range=\"0\" ></VARIANT_MESH><VARIANT_MESH model=\"b\"/></SLOT></VARIANT_MESH>");
        assert_eq!(models(&m), ["a", "b"]);
    }

    #[test]
    fn an_end_tag_left_over_is_left_out_and_an_open_file_closes() {
        // orc_skin_cut4_brown, and blacklock_t3_hammers's last line.
        let m = repaired("<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"></VARIANT_MESH></VARIANT_MESH><VARIANT_MESH model=\"b\"/></SLOT><VARIANT_MESH>");
        assert_eq!(models(&m), ["a", "b"]);
    }

    #[test]
    fn a_good_file_is_kept_as_it_is() {
        let f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        assert!(!f.repaired);
        assert_eq!(f.doc.save(), FILE.as_bytes());
    }

    #[test]
    fn reads_the_tree() {
        let f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        let skin = &f.root.slots[0];
        assert_eq!((skin.name.as_str(), skin.alternatives.len()), ("skin", 2));
        let Alternative::Mesh(m) = &skin.alternatives[0] else { panic!("a mesh") };
        assert_eq!(m.model.as_deref(), Some("a/b.rigid_model_v2"));
        assert_eq!(m.masks, [Some(2), None, Some(1)]);
        assert_eq!((m.tints[0].green, m.tints[0].random_range), (0.5, 0.1));
        assert_eq!(m.meta_data, ["female"]);
        assert_eq!((m.slots[0].attach_point.as_deref(), m.slots[0].probability, skin.probability), (Some("crest_centre"), Some(0.25), None));
        assert!(matches!(&skin.alternatives[1], Alternative::Reference { definition, .. } if definition == "c/d.VariantMeshDefinition"));
    }

    #[test]
    fn an_edit_changes_only_its_value() {
        let mut f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        assert_eq!(f.doc.save(), FILE.as_bytes());
        let Alternative::Mesh(m) = &f.root.slots[0].alternatives[0] else { panic!("a mesh") };
        let element = m.element;
        f.doc.set(element, "model", "x.rigid_model_v2");
        assert_eq!(f.doc.save(), FILE.replace("a/b.rigid_model_v2", "x.rigid_model_v2").as_bytes());
    }

    #[test]
    fn another_root_is_refused() {
        assert!(VariantMeshFile::parse(b"<presentation/>".to_vec()).is_err());
    }
}
