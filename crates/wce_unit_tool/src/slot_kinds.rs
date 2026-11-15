//! What a slot takes, learnt from the game's definitions: per slot name (`helmets`, `trousers`, `boss`...), the folders its alternatives'
//! models, definitions and decals come from. The pickers list only what a node's slot takes: a helmet slot's model field lists helmets.
//!
//! A folder counts when it holds at least [`SHARE`] of what slots of that name use (the rest is the odd prop a definition put there); the
//! slot's own alternatives' folders always count. A folder covers its subfolders. A name the game doesn't use takes the slot's own folders;
//! a slot without any takes everything. The example `slot_kinds` reports every slot name's folders and their counts.

use std::collections::HashMap;

use wce_formats::variant_mesh::{Alternative, Slot, VariantMesh, VariantMeshFile};
use wce_vfs::normalize;

/// The least share of a slot name's alternatives a folder must hold to count.
pub const SHARE: f32 = 0.03;

/// What a field picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// A model (`model`, `imposter_model`) or a definition (`definition`): the alternatives themselves.
    Part,

    /// A decal texture (`decal_diffuse`, `decal_normal`).
    Decal,
}

/// The folder of a file, normalized.
pub fn folder(path: &str) -> String {
    let p = normalize(path);
    p.rsplit_once('/').map_or_else(String::new, |(f, _)| f.to_owned())
}

/// Per slot name and field, how many alternatives each folder gave.
#[derive(Clone, Debug, Default)]
pub struct SlotKinds {
    counts: HashMap<(String, Field), HashMap<String, usize>>,
}

impl SlotKinds {
    /// Adds a definition's slots.
    pub fn add(&mut self, file: &VariantMeshFile) {
        self.mesh(&file.root);
    }

    fn mesh(&mut self, mesh: &VariantMesh) {
        for slot in &mesh.slots {
            let name = slot.name.to_lowercase();
            for a in &slot.alternatives {
                for (field, path) in alternative_files(a) {
                    *self.counts.entry((name.clone(), field)).or_default().entry(folder(&path)).or_default() += 1;
                }

                if let Alternative::Mesh(m) = a {
                    self.mesh(m);
                }
            }
        }
    }

    /// The folders a slot's field takes, sorted; `None`: anything (nothing is known of it).
    pub fn folders(&self, slot: &Slot, field: Field) -> Option<Vec<String>> {
        let mut out: Vec<String> =
            slot.alternatives.iter().flat_map(alternative_files).filter(|(f, _)| *f == field).map(|(_, p)| folder(&p)).collect();
        if let Some(counts) = self.counts.get(&(slot.name.to_lowercase(), field)) {
            let total = counts.values().sum::<usize>().max(1) as f32;
            out.extend(counts.iter().filter(|(_, n)| **n as f32 / total >= SHARE).map(|(f, _)| f.clone()));
        }

        out.retain(|f| !f.is_empty());
        out.sort();
        out.dedup();

        // A folder inside another that counts adds nothing.
        let all = out.clone();
        out.retain(|f| !all.iter().any(|g| g != f && f.starts_with(&format!("{g}/"))));
        (!out.is_empty()).then_some(out)
    }
}

/// The files an alternative names, by field: its model, its imposter and decals, or its definition.
fn alternative_files(a: &Alternative) -> Vec<(Field, String)> {
    let named = |v: &Option<String>| v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned);
    match a {
        Alternative::Mesh(m) => {
            [(Field::Part, named(&m.model)), (Field::Decal, named(&m.decal_diffuse)), (Field::Decal, named(&m.decal_normal))]
                .into_iter()
                .filter_map(|(f, p)| Some((f, p?)))
                .collect()
        }
        Alternative::Reference { definition, .. } => vec![(Field::Part, definition.clone())],
    }
}

/// Whether a path is in one of the folders (or below one).
pub fn within(path: &str, folders: &[String]) -> bool {
    let p = normalize(path);
    folders.iter().any(|f| p.starts_with(&format!("{f}/")))
}

#[cfg(test)]
mod tests {
    use wce_formats::variant_mesh::{Node, VariantMeshFile};

    use super::{Field, SlotKinds, within};

    fn parse(text: &str) -> VariantMeshFile {
        VariantMeshFile::parse(text.as_bytes().to_vec()).expect("parses")
    }

    #[test]
    fn a_slot_takes_the_folders_its_name_mostly_uses() {
        let mut kinds = SlotKinds::default();
        let mut game = String::from("<VARIANT_MESH><SLOT name=\"Helmets\">");
        for k in 0..40 {
            game.push_str(&format!("<VARIANT_MESH model=\"M/Helmets/h{k}.rigid_model_v2\" decal_diffuse=\"m/tex/d{k}\"/>"));
        }

        // The odd one: under the share.
        game.push_str("<VARIANT_MESH model=\"m/props/x.rigid_model_v2\"/><VARIANT_MESH_REFERENCE definition=\"m/helmets/sub/r.variantmeshdefinition\"/>");
        game.push_str("</SLOT></VARIANT_MESH>");
        kinds.add(&parse(&game));

        let edited = parse(
            "<VARIANT_MESH><SLOT name=\"helmets\"><VARIANT_MESH model=\"m/odd/mine.rigid_model_v2\"/></SLOT><SLOT name=\"new_slot\"/></VARIANT_MESH>",
        );
        let Some(Node::Slot(helmets)) = edited.node(&[0]) else { panic!("a slot") };
        assert_eq!(kinds.folders(helmets, Field::Part), Some(vec!["m/helmets".to_owned(), "m/odd".to_owned()]));
        assert_eq!(kinds.folders(helmets, Field::Decal), Some(vec!["m/tex".to_owned()]));
        let Some(Node::Slot(new)) = edited.node(&[1]) else { panic!("a slot") };
        assert_eq!(kinds.folders(new, Field::Part), None, "nothing known: anything");

        let folders = kinds.folders(helmets, Field::Part).unwrap_or_default();
        assert!(within("M/Helmets/sub/a.rigid_model_v2", &folders) && !within("m/props/x.rigid_model_v2", &folders));
        assert!(!within("m/helmetsx/a.rigid_model_v2", &folders), "a folder, not a prefix of a name");
    }
}
