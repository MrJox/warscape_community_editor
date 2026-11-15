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

/// The slot names the engine asks a soldier's model for, and what each is to it (docs/re/variant_mesh_slots.md).
pub const LOOKED_UP: [(&str, &str); 5] = [
    ("shield", "the soldier's shield"),
    ("weapon_1", "the soldier's first weapon"),
    ("weapon_2", "the soldier's second weapon"),
    ("weapon_3", "the soldier's third weapon"),
    ("weapon_4", "the soldier's fourth weapon"),
];

/// What the engine makes of a slot name, when it makes anything of it: a soldier's weapon or shield, a ship piece's damage state.
pub fn meaning(name: &str) -> Option<String> {
    if let Some((_, what)) = LOOKED_UP.iter().find(|(n, _)| *n == name) {
        return Some(format!("The game looks this slot up by its name: {what} (what the battle does with it isn't traced)."));
    }

    match name {
        "misc_99_dm1" | "night_lights_dm1" => return Some("The game looks this ship slot up by its name.".to_owned()),
        _ => {}
    }

    let (piece, state) = name.rsplit_once("_dm")?;
    let state: u32 = state.parse().ok()?;
    Some(format!(
        "A ship piece's damage state: '{piece}' {}.",
        if state == 1 { "intact".to_owned() } else { format!("damaged, state {state}") }
    ))
}

/// A name that is nearly one the engine looks up but isn't (`weapon_01`, `weapon_5`, `Shield`, `shield 1`): the one it is like.
pub fn near_miss(name: &str) -> Option<&'static str> {
    if LOOKED_UP.iter().any(|(n, _)| *n == name) {
        return None;
    }

    let plain: String = name.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect();
    // `shields` is a name of its own (the slot inside `shield` holding the shield's model): only `shield` spelt otherwise, or numbered.
    if plain.strip_prefix("shield").is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit())) {
        return Some("shield");
    }

    let digits = plain.strip_prefix("weapon")?;
    if digits.is_empty() || digits == "s" {
        return None;
    }

    match digits.trim_start_matches('0').parse::<u32>().ok()? {
        n @ 1..=4 => LOOKED_UP.get(n as usize).map(|(n, _)| *n),
        _ => Some("weapon_1"),
    }
}

/// Per slot name and field, how many alternatives each folder gave; how many slots have each name.
#[derive(Clone, Debug, Default)]
pub struct SlotKinds {
    counts: HashMap<(String, Field), HashMap<String, usize>>,
    names: HashMap<String, usize>,
}

impl SlotKinds {
    /// Adds a definition's slots.
    pub fn add(&mut self, file: &VariantMeshFile) {
        self.mesh(&file.root);
    }

    /// The names the game's slots have, as written, most used first.
    pub fn names(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = self.names.iter().filter(|(n, _)| !n.is_empty()).map(|(n, k)| (n.clone(), *k)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    fn mesh(&mut self, mesh: &VariantMesh) {
        for slot in &mesh.slots {
            *self.names.entry(slot.name.clone()).or_default() += 1;
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

    use super::{Field, SlotKinds, meaning, near_miss, within};

    #[test]
    fn names_the_engine_knows_and_the_ones_nearly_them() {
        assert!(meaning("weapon_2").is_some_and(|m| m.contains("second weapon")));
        assert!(meaning("bow_01_dm2").is_some_and(|m| m.contains("damaged, state 2")));
        assert!(meaning("night_lights_dm1").is_some() && meaning("helmets").is_none() && meaning("x_dmz").is_none());
        for (name, like) in [
            ("weapon_01", Some("weapon_1")),
            ("weapon_5", Some("weapon_1")),
            ("Shield", Some("shield")),
            ("shield 1", Some("shield")),
            ("weapon", None),
            ("weapons", None),
            ("weapon_3", None),
            ("shields", None),
            ("shield1", Some("shield")),
            ("helmets", None),
        ] {
            assert_eq!(near_miss(name), like, "{name}");
        }
    }

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
