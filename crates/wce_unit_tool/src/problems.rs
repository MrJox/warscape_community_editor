//! What is wrong with a definition, node by node: files it names that the packs don't have, values out of range, attach points nothing
//! has, references that come back to it.

use std::collections::HashMap;

use wce_formats::variant_mesh::{Alternative, NodePath, VariantMesh, VariantMeshFile};
use wce_render::unit_assembly::{Reader, attachment_points};
use wce_vfs::normalize;

/// The weapon bones a slot can name (`weapon_01` is `bn_weapon_01`).
pub const WEAPON_POINTS: [&str; 5] = ["weapon_01", "weapon_02", "weapon_03", "weapon_04", "weapon_05"];

/// One thing wrong, at a node.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub path: NodePath,
    pub text: String,
}

struct Checker<'a> {
    read: &'a Reader<'a>,
    exists: &'a dyn Fn(&str) -> bool,
    own: String,
    points: HashMap<String, Vec<String>>,
    out: Vec<Problem>,
}

impl Checker<'_> {
    fn add(&mut self, path: &NodePath, text: String) {
        self.out.push(Problem { path: path.clone(), text });
    }

    fn points(&mut self, model: &str) -> &[String] {
        let read = self.read;
        self.points.entry(model.to_owned()).or_insert_with(|| attachment_points(read, model))
    }

    /// A decal names a texture (`.dds` added when missing) or a `.texture_array` of them.
    fn decal(&mut self, path: &NodePath, what: &str, name: &str) {
        let name = normalize(name);
        let array = format!("{name}.texture_array");
        if (self.exists)(&array) {
            let list = (self.read)(&array).map(|b| wce_formats::variant_mesh::texture_array(&b)).unwrap_or_default();
            for t in list {
                let t = format!("{}.dds", normalize(&t));
                if !(self.exists)(&t) {
                    self.add(path, format!("{what}: {array} lists {t}, which isn't in the packs"));
                }
            }

            return;
        }

        let file = if name.ends_with(".dds") { name } else { format!("{name}.dds") };
        if !(self.exists)(&file) {
            self.add(path, format!("{what}: {file} isn't in the packs"));
        }
    }

    fn mesh(&mut self, doc: &VariantMeshFile, mesh: &VariantMesh, path: &NodePath, parent: Option<&str>) {
        let model = mesh.model.as_deref().map(str::trim).filter(|m| !m.is_empty());
        if let Some(m) = model
            && !(self.exists)(&normalize(m))
        {
            self.add(path, format!("model: {m} isn't in the packs"));
        }

        for (what, value) in [("decal_diffuse", &mesh.decal_diffuse), ("decal_normal", &mesh.decal_normal)] {
            if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                self.decal(path, what, v);
            }
        }

        for k in 0..3 {
            let name = format!("mask{k}");
            if let Some(v) = doc.doc.get(mesh.element, &name)
                && !matches!(v.trim(), "0" | "1" | "2")
            {
                self.add(path, format!("{name} is '{v}': a faction colour is 0, 1 or 2"));
            }
        }

        for (t, tint) in mesh.tints.iter().enumerate() {
            if [tint.red, tint.green, tint.blue].iter().any(|c| !(0.0..=1.0).contains(c)) {
                self.add(path, format!("tint {} is outside 0 to 1", t + 1));
            }
        }

        let parent = model.or(parent);
        for (i, slot) in mesh.slots.iter().enumerate() {
            let mut at = path.clone();
            at.push(i);
            if let Some(p) = doc.doc.get(slot.element, "probability")
                && p.trim().parse::<f32>().map_or(true, |p| !(0.0..=1.0).contains(&p))
            {
                self.add(&at, format!("probability is '{p}': it is 0 to 1"));
            }

            if let Some(point) = slot.attach_point.as_deref().map(str::trim).filter(|p| !p.is_empty() && !p.contains(',')) {
                if point.starts_with("weapon_") {
                    if !WEAPON_POINTS.contains(&point) {
                        self.add(&at, format!("attach_point {point}: the weapon bones are weapon_01 to weapon_05"));
                    }
                } else {
                    match parent {
                        Some(model) if !self.points(&normalize(model)).iter().any(|p| p == point) => {
                            self.add(&at, format!("attach_point {point}: {model} has no such point"))
                        }

                        None => self.add(&at, format!("attach_point {point}: no model above the slot to have it")),
                        _ => {}
                    }
                }
            }

            for (j, alternative) in slot.alternatives.iter().enumerate() {
                let mut at = at.clone();
                at.push(j);
                match alternative {
                    Alternative::Mesh(m) => self.mesh(doc, m, &at, parent),
                    Alternative::Reference { definition, .. } => self.reference(&at, definition),
                }
            }
        }
    }

    /// A reference's file, and whether following references from it comes back to this file.
    fn reference(&mut self, path: &NodePath, definition: &str) {
        let file = normalize(definition);
        if file.is_empty() {
            self.add(path, "a reference to no file".to_owned());
            return;
        }

        if !(self.exists)(&file) {
            self.add(path, format!("{definition} isn't in the packs"));
            return;
        }

        let mut seen = vec![];
        let mut pending = vec![file];
        while let Some(f) = pending.pop() {
            if f == self.own {
                self.add(path, format!("{definition} comes back to this file: the game would never end it"));
                return;
            }

            if seen.contains(&f) || seen.len() > 64 {
                continue;
            }

            if let Some(parsed) = (self.read)(&f).and_then(|b| VariantMeshFile::parse(b).ok()) {
                parsed.doc.elements().iter().enumerate().filter(|(_, e)| e.name == "VARIANT_MESH_REFERENCE").for_each(|(e, _)| {
                    if let Some(d) = parsed.doc.get(e, "definition") {
                        pending.push(normalize(&d));
                    }
                });
            }

            seen.push(f);
        }
    }
}

/// The problems of a definition; `own` is its path in the packs (for references that come back to it).
pub fn check(read: &Reader, exists: &dyn Fn(&str) -> bool, file: &VariantMeshFile, own: &str) -> Vec<Problem> {
    let mut c = Checker { read, exists, own: normalize(own), points: HashMap::new(), out: vec![] };
    c.mesh(file, &file.root, &vec![], None);
    c.out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use wce_formats::variant_mesh::VariantMeshFile;

    use super::check;

    #[test]
    fn finds_what_is_missing_and_out_of_range() {
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        files.insert("a.rigid_model_v2".into(), vec![]);
        files.insert("d.dds".into(), vec![]);
        files.insert(
            "self.variantmeshdefinition".into(),
            br#"<VARIANT_MESH><SLOT name="s"><VARIANT_MESH_REFERENCE definition="self.variantmeshdefinition"/></SLOT></VARIANT_MESH>"#
                .to_vec(),
        );
        let file = VariantMeshFile::parse(
            br#"<VARIANT_MESH>
                <SLOT name="body" probability="1.5"><VARIANT_MESH model="a.rigid_model_v2" decal_diffuse="d" mask0="3"/><VARIANT_MESH model="gone.rigid_model_v2"/></SLOT>
                <SLOT name="w" attach_point="weapon_07"><VARIANT_MESH_REFERENCE definition="self.variantmeshdefinition"/></SLOT>
            </VARIANT_MESH>"#
                .to_vec(),
        )
        .expect("parses");
        let read = |p: &str| files.get(p).cloned();
        let exists = |p: &str| files.contains_key(p);
        let found: Vec<(Vec<usize>, String)> =
            check(&read, &exists, &file, "self.variantmeshdefinition").into_iter().map(|p| (p.path, p.text)).collect();
        let at = |path: &[usize]| found.iter().filter(|(p, _)| p == path).count();
        assert_eq!((at(&[0]), at(&[0, 0]), at(&[0, 1]), at(&[1]), at(&[1, 0])), (1, 1, 1, 1, 1), "{found:#?}");
    }
}
