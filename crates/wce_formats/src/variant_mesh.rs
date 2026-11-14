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

/// A node of a definition's tree, from its root mesh: a slot's index among its mesh's slots, then an alternative's among its slot's, and so on.
/// Even lengths name a mesh or a reference (the root is empty), odd ones a slot. It names a node by its place, so it survives the edits that
/// renumber the document's elements, as long as nothing before it in its parents moves.
pub type NodePath = Vec<usize>;

/// A node of the tree, found by its [`NodePath`].
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Mesh(&'a VariantMesh),
    Slot(&'a Slot),
    Reference { element: usize, definition: &'a str },
}

impl Node<'_> {
    /// Its element in the document.
    pub fn element(&self) -> usize {
        match self {
            Node::Mesh(m) => m.element,
            Node::Slot(s) => s.element,
            Node::Reference { element, .. } => *element,
        }
    }
}

/// A `TINT`: one of the colours a mesh can take.
#[derive(Clone, Debug, PartialEq)]
pub struct Tint {
    /// Its element in the document.
    pub element: usize,
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

    /// Their elements, in the same order.
    pub meta_elements: Vec<usize>,
    pub slots: Vec<Slot>,
}

/// One alternative of a slot.
#[derive(Clone, Debug, PartialEq)]
pub enum Alternative {
    Mesh(VariantMesh),

    /// A `VARIANT_MESH_REFERENCE`: its element and the file it names.
    Reference {
        element: usize,
        definition: String,
    },
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
        element,
        model: attr("model"),
        decal_diffuse: attr("decal_diffuse"),
        decal_normal: attr("decal_normal"),
        imposter_model: attr("imposter_model"),
        masks: [mask("mask0"), mask("mask1"), mask("mask2")],
        ..VariantMesh::default()
    };

    for &child in &doc.elements()[element].children {
        match doc.elements()[child].name.as_str() {
            "TINT" => out.tints.push(Tint {
                element: child,
                red: number(doc, child, "red"),
                green: number(doc, child, "green"),
                blue: number(doc, child, "blue"),
                random_range: number(doc, child, "random_range"),
            }),
            "META_DATA" => {
                out.meta_data.push(text(doc, child));
                out.meta_elements.push(child);
            }

            "SLOT" => out.slots.push(slot(doc, child)),
            _ => {}
        }
    }

    out
}

fn slot(doc: &XmlDoc, element: usize) -> Slot {
    let alternatives = doc.elements()[element]
        .children
        .iter()
        .filter_map(|&child| match doc.elements()[child].name.as_str() {
            "VARIANT_MESH" => Some(Alternative::Mesh(mesh(doc, child))),
            "VARIANT_MESH_REFERENCE" => {
                Some(Alternative::Reference { element: child, definition: doc.get(child, "definition").unwrap_or_default() })
            }
            _ => None,
        })
        .collect();
    Slot {
        element,
        name: doc.get(element, "name").unwrap_or_default(),
        attach_point: doc.get(element, "attach_point"),
        probability: doc.get(element, "probability").and_then(|v| v.trim().parse().ok()),
        alternatives,
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
                            if file.get(p) == Some(&b'"')
                                && file.get(p + 1).is_some_and(|&c| !c.is_ascii_whitespace() && !matches!(c, b'/' | b'>'))
                            {
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

        Self::from_doc(doc, repaired)
    }

    /// The tree of a document as it is now, edits included: what an editor reads after each change, without saving and parsing it again.
    ///
    /// # Errors
    ///
    /// Returns an error if its root isn't a `VARIANT_MESH`.
    pub fn from_doc(doc: XmlDoc, repaired: bool) -> Result<Self, XmlError> {
        let root = doc
            .roots()
            .iter()
            .copied()
            .find(|&r| doc.elements()[r].name == "VARIANT_MESH")
            .ok_or_else(|| XmlError::Edit("not a variant mesh definition".to_owned()))?;
        let root = mesh(&doc, root);
        Ok(Self { doc, root, repaired })
    }

    /// The node at `path`; `None` past the tree, or below a reference (another file's).
    pub fn node(&self, path: &[usize]) -> Option<Node<'_>> {
        let mut mesh = &self.root;
        let mut k = 0;
        loop {
            if k == path.len() {
                return Some(Node::Mesh(mesh));
            }

            let slot = mesh.slots.get(path[k])?;
            if k + 1 == path.len() {
                return Some(Node::Slot(slot));
            }

            match slot.alternatives.get(path[k + 1])? {
                Alternative::Mesh(m) => mesh = m,
                Alternative::Reference { element, definition } => {
                    return (k + 2 == path.len()).then_some(Node::Reference { element: *element, definition });
                }
            }

            k += 2;
        }
    }

    /// The path of the node whose element is `element` (a mesh, a slot or a reference), if it is one.
    pub fn path_of(&self, element: usize) -> Option<NodePath> {
        fn in_mesh(mesh: &VariantMesh, element: usize, path: &mut NodePath) -> bool {
            if mesh.element == element {
                return true;
            }

            for (i, slot) in mesh.slots.iter().enumerate() {
                path.push(i);
                if slot.element == element {
                    return true;
                }

                for (j, alternative) in slot.alternatives.iter().enumerate() {
                    path.push(j);
                    let found = match alternative {
                        Alternative::Mesh(m) => in_mesh(m, element, path),
                        Alternative::Reference { element: e, .. } => *e == element,
                    };
                    if found {
                        return true;
                    }

                    path.pop();
                }

                path.pop();
            }

            false
        }

        let mut path = vec![];
        in_mesh(&self.root, element, &mut path).then_some(path)
    }
}

/// The structural edits an editor makes to a definition. Each returns the new document, as [`XmlDoc`]'s structural edits do (the elements are
/// numbered again), with the new element's index; what the file already holds is left as it was.
pub mod edit {
    use crate::xml_doc::{XmlDoc, XmlError};

    fn child(doc: &XmlDoc, parent: usize, name: &str, attrs: &[(&str, &str)]) -> Result<(XmlDoc, usize), XmlError> {
        let (mut doc, element) = doc.with_child(parent, name)?;
        for (name, value) in attrs {
            doc.set(element, name, value);
        }

        Ok((doc, element))
    }

    /// A `SLOT` named `name`, the last of a `VARIANT_MESH`.
    ///
    /// # Errors
    ///
    /// Returns why the document can't take it.
    pub fn add_slot(doc: &XmlDoc, mesh: usize, name: &str) -> Result<(XmlDoc, usize), XmlError> {
        child(doc, mesh, "SLOT", &[("name", name)])
    }

    /// A `VARIANT_MESH` of `model`, the last alternative of a `SLOT`.
    ///
    /// # Errors
    ///
    /// Returns why the document can't take it.
    pub fn add_mesh(doc: &XmlDoc, slot: usize, model: &str) -> Result<(XmlDoc, usize), XmlError> {
        child(doc, slot, "VARIANT_MESH", &[("model", model)])
    }

    /// A `VARIANT_MESH_REFERENCE` to `definition`, the last alternative of a `SLOT`.
    ///
    /// # Errors
    ///
    /// Returns why the document can't take it.
    pub fn add_reference(doc: &XmlDoc, slot: usize, definition: &str) -> Result<(XmlDoc, usize), XmlError> {
        child(doc, slot, "VARIANT_MESH_REFERENCE", &[("definition", definition)])
    }

    /// A white `TINT` without random range, the last child of a `VARIANT_MESH`.
    ///
    /// # Errors
    ///
    /// Returns why the document can't take it.
    pub fn add_tint(doc: &XmlDoc, mesh: usize) -> Result<(XmlDoc, usize), XmlError> {
        child(doc, mesh, "TINT", &[("red", "1.0"), ("green", "1.0"), ("blue", "1.0"), ("random_range", "0.0")])
    }
}

/// The textures a `.texture_array` lists, one per line, in order: a decal that is one of them.
pub fn texture_array(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::{Alternative, Node, VariantMesh, VariantMeshFile, edit, texture_array};

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
        let m = repaired(
            "<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"><SLOT name=\"t\"><VARIANT_MESH model=\"b\"/ ></SLOT></VARIANT_MESH><VARIANT_MESH model=\"c\"/></SLOT></VARIANT_MESH>",
        );
        assert_eq!(models(&m), ["a", "c"]);
        let Alternative::Mesh(a) = &m.slots[0].alternatives[0] else { panic!("a mesh") };
        assert_eq!(models(a), ["b"]);
    }

    #[test]
    fn doubled_quotes_go_and_empty_values_stay() {
        let m = repaired(
            "<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\" imposter_model=\"\"i\" /><VARIANT_MESH model=\"b\"\" mask0=\"2\"/><VARIANT_MESH model=\"\" mask0=\"1\"/ ></SLOT></VARIANT_MESH>",
        );
        assert_eq!(models(&m), ["a", "b", ""]);
        let Alternative::Mesh(a) = &m.slots[0].alternatives[0] else { panic!("a mesh") };
        assert_eq!(a.imposter_model.as_deref(), Some("i"));
        let Alternative::Mesh(b) = &m.slots[0].alternatives[1] else { panic!("a mesh") };
        assert_eq!(b.masks[0], Some(2));
    }

    #[test]
    fn a_tint_left_open_ends_nothing() {
        // vik_tunics_poor: the mesh is closed, the tint open, and its end tag ends neither.
        let m = repaired(
            "<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"/><TINT red=\"1\" green=\"1\" blue=\"1\" random_range=\"0\" ></VARIANT_MESH><VARIANT_MESH model=\"b\"/></SLOT></VARIANT_MESH>",
        );
        assert_eq!(models(&m), ["a", "b"]);
    }

    #[test]
    fn an_end_tag_left_over_is_left_out_and_an_open_file_closes() {
        // orc_skin_cut4_brown, and blacklock_t3_hammers's last line.
        let m = repaired(
            "<VARIANT_MESH><SLOT name=\"s\"><VARIANT_MESH model=\"a\"></VARIANT_MESH></VARIANT_MESH><VARIANT_MESH model=\"b\"/></SLOT><VARIANT_MESH>",
        );
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
        assert_eq!(
            (m.slots[0].attach_point.as_deref(), m.slots[0].probability, skin.probability),
            (Some("crest_centre"), Some(0.25), None)
        );
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
    fn the_tree_of_an_edited_doc_is_the_tree_of_its_text() {
        let mut f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        let crest = f.root.slots[0].alternatives.first().map(|a| if let Alternative::Mesh(m) = a { m.slots[0].element } else { 0 });
        f.doc.set(crest.expect("a crest"), "probability", "0.5");
        let tree = VariantMeshFile::from_doc(f.doc.clone(), false).expect("a tree");
        let parsed = VariantMeshFile::parse(f.doc.save()).expect("parses");
        assert_eq!(tree.root, parsed.root);
        let Some(Node::Slot(s)) = tree.node(&[0, 0, 0]) else { panic!("the crest's slot") };
        assert_eq!((s.name.as_str(), s.probability), ("crest", Some(0.5)));
    }

    #[test]
    fn paths_find_nodes_and_back() {
        let f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        for path in [vec![], vec![0], vec![0, 0], vec![0, 0, 0], vec![0, 1]] {
            let node = f.node(&path).expect("a node");
            assert_eq!(f.path_of(node.element()), Some(path));
        }

        assert!(matches!(f.node(&[0, 1]), Some(Node::Reference { definition: "c/d.VariantMeshDefinition", .. })));
        assert!(f.node(&[0, 1, 0]).is_none() && f.node(&[3]).is_none());
    }

    #[test]
    fn additions_touch_only_their_place() {
        let f = VariantMeshFile::parse(FILE.as_bytes().to_vec()).expect("parses");
        let (doc, slot) = edit::add_slot(&f.doc, f.root.element, "helmet").expect("adds");
        let (doc, _) = edit::add_mesh(&doc, slot, "h.rigid_model_v2").expect("adds");
        let text = String::from_utf8(doc.save()).expect("text");
        let added = "\t<SLOT name=\"helmet\">\r\n\t\t<VARIANT_MESH model=\"h.rigid_model_v2\"/>\r\n\t</SLOT>\r\n";
        assert_eq!(text, FILE.replace("\t</SLOT>\r\n</VARIANT_MESH>", &format!("\t</SLOT>\r\n{added}</VARIANT_MESH>")));
        let tree = VariantMeshFile::from_doc(doc, false).expect("a tree");
        assert!(matches!(tree.node(&[1, 0]), Some(Node::Mesh(m)) if m.model.as_deref() == Some("h.rigid_model_v2")));

        let Some(Node::Mesh(m)) = f.node(&[0, 0]) else { panic!("a mesh") };
        let (doc, tint) = edit::add_tint(&f.doc, m.element).expect("adds");
        let tree = VariantMeshFile::from_doc(doc, false).expect("a tree");
        let Some(Node::Mesh(m)) = tree.node(&[0, 0]) else { panic!("a mesh") };
        assert_eq!((m.tints.len(), m.tints[1].element, m.tints[1].red), (2, tint, 1.0));
    }

    #[test]
    fn texture_arrays_list_their_lines() {
        assert_eq!(texture_array(b"a/one\r\n\r\n  a/two \n"), ["a/one", "a/two"]);
    }

    #[test]
    fn another_root_is_refused() {
        assert!(VariantMeshFile::parse(b"<presentation/>".to_vec()).is_err());
    }
}
