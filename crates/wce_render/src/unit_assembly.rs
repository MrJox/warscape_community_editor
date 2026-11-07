//! A unit's model from its variant mesh definition: which parts one soldier wears, picked by a seed.
//!
//! The engine's variation (`VARIANT_MESH_VARIATION`, `VARIANT_MESH_PART`, `VARIANT_MESH_PART_SLOT`): per slot a roll against its `probability`,
//! then one alternative; a `VARIANT_MESH_REFERENCE` stands for the file it names. The picks here use the editor's own generator: the same seed
//! gives the same soldier in the editor, not the soldier the game would pick. **ASSUMPTION** (by their names and values, not traced): the
//! probability is how often a slot is filled, every alternative is as likely, `maskN` is the faction colour the mask's channel N takes, and a
//! tint applies to the mask's first channel.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::variant_mesh::{Alternative, Slot, VariantMesh, VariantMeshFile};
use wce_vfs::normalize;

/// The bytes of an in-pack file; `None` when it isn't there.
pub type Reader<'a> = dyn Fn(&str) -> Option<Vec<u8>> + 'a;

/// What moves a part that isn't skinned.
#[derive(Clone, Debug, PartialEq)]
pub enum Attach {
    /// Nothing: it is skinned, by its vertices' bones.
    None,

    /// A bone of the skeleton, by name.
    Bone(String),

    /// An attachment point of the parent part's model: a bone by number, and its matrix there (3 x 4 rows).
    Point { bone: u32, matrix: [f32; 12] },
}

/// One part of a soldier.
#[derive(Clone, Debug, PartialEq)]
pub struct AssembledPart {
    pub model: String,
    pub decal_diffuse: Option<String>,
    pub decal_normal: Option<String>,

    /// The faction colour each channel of the colour mask takes.
    pub masks: [u8; 3],

    /// rgb and the mask channel it applies to (0: none).
    pub tint: [f32; 4],
    pub attach: Attach,
}

/// One soldier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssembledUnit {
    pub parts: Vec<AssembledPart>,

    /// The definitions named that could not be read: a mod's files, some not well-formed.
    pub missing: Vec<String>,
}

/// splitmix64.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub(crate) fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// An attachment point of a model.
fn attachment_point(read: &Reader, model: &str, name: &str) -> Option<(u32, [f32; 12])> {
    let bytes = read(model)?;
    let model = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).ok()?;
    model.lods().first()?.mesh_blocks().iter().flat_map(|b| b.material().attachment_points()).find(|p| p.name() == name).map(|p| {
        let m = p.matrix();
        (*p.bone_id(), std::array::from_fn(|i| m[(i / 4, i % 4)]))
    })
}

/// A decal's file: `name` is a `.texture_array` listing alternatives (`pick` chooses), else a texture.
fn decal(read: &Reader, name: Option<&str>, pick: usize) -> Option<String> {
    let name = normalize(name?);
    if let Some(list) = read(&format!("{name}.texture_array")) {
        let list = String::from_utf8_lossy(&list).into_owned();
        let lines: Vec<&str> = list.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        return lines.get(pick % lines.len().max(1)).map(|l| format!("{}.dds", normalize(l)));
    }

    Some(if name.ends_with(".dds") { name } else { format!("{name}.dds") })
}

struct Assembler<'a> {
    read: &'a Reader<'a>,
    rng: Rng,
    files: HashMap<String, Option<std::rc::Rc<VariantMeshFile>>>,
    out: AssembledUnit,
}

impl Assembler<'_> {
    fn file(&mut self, path: &str) -> Option<std::rc::Rc<VariantMeshFile>> {
        let path = normalize(path);
        if !self.files.contains_key(&path) {
            let file = (self.read)(&path).and_then(|b| VariantMeshFile::parse(b).ok()).map(std::rc::Rc::new);
            if file.is_none() {
                self.out.missing.push(path.clone());
            }

            self.files.insert(path.clone(), file);
        }

        self.files[&path].clone()
    }

    fn slot(&mut self, slot: &Slot, parent: Option<&str>, attach: &Attach, depth: u32) {
        if slot.alternatives.is_empty() || slot.probability.is_some_and(|p| self.rng.unit() >= p) {
            return;
        }

        // A list of points is a ship's; a person's slot names one.
        let attach = match slot.attach_point.as_deref().and_then(|a| a.split(',').next()).map(str::trim) {
            None | Some("") => attach.clone(),
            Some(point) if point.starts_with("weapon_") => Attach::Bone(format!("bn_{point}")),
            Some(point) => match parent.and_then(|model| attachment_point(self.read, model, point)) {
                Some((bone, matrix)) => Attach::Point { bone, matrix },
                None => attach.clone(),
            },
        };

        let pick = self.rng.below(slot.alternatives.len());
        match &slot.alternatives[pick] {
            Alternative::Mesh(mesh) => self.mesh(mesh, parent, &attach, depth),
            Alternative::Reference { definition, .. } => {
                if depth < 32 && let Some(file) = self.file(definition) {
                    self.mesh(&file.root, parent, &attach, depth + 1);
                }
            }
        }
    }

    fn mesh(&mut self, mesh: &VariantMesh, parent: Option<&str>, attach: &Attach, depth: u32) {
        let model = mesh.model.as_deref().map(normalize);
        if let Some(model) = &model {
            let pick = self.rng.next() as usize;
            let tint = (!mesh.tints.is_empty()).then(|| &mesh.tints[self.rng.below(mesh.tints.len())]);
            self.out.parts.push(AssembledPart {
                model: model.clone(),
                decal_diffuse: decal(self.read, mesh.decal_diffuse.as_deref(), pick),
                decal_normal: decal(self.read, mesh.decal_normal.as_deref(), pick),
                masks: std::array::from_fn(|i| mesh.masks[i].filter(|m| *m < 3).unwrap_or(i as u8)),
                tint: tint.map_or([0.0; 4], |t| [t.red, t.green, t.blue, 1.0]),
                attach: attach.clone(),
            });
        }

        let parent = model.as_deref().or(parent);
        for slot in &mesh.slots {
            self.slot(slot, parent, attach, depth);
        }
    }
}

/// One soldier of a definition (`variantmeshes/variantmeshdefinitions/<name>.variantmeshdefinition`); `None` when it can't be read.
pub fn assemble(read: &Reader, definition: &str, seed: u64) -> Option<AssembledUnit> {
    let mut a = Assembler { read, rng: Rng(seed), files: HashMap::new(), out: AssembledUnit::default() };
    let root = a.file(definition)?;
    a.mesh(&root.root, None, &Attach::None, 0);
    Some(a.out)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Attach, assemble};

    fn files() -> HashMap<String, Vec<u8>> {
        let mut f = HashMap::new();
        f.insert("a.variantmeshdefinition".to_owned(), br#"<VARIANT_MESH>
            <SLOT name="skin"><VARIANT_MESH model="Body.rigid_model_v2" mask0="2" mask1="0" mask2="1" /></SLOT>
            <SLOT name="never" probability="0"><VARIANT_MESH model="no.rigid_model_v2" /></SLOT>
            <SLOT name="weapon_1" attach_point="weapon_03">
                <VARIANT_MESH_REFERENCE definition="B.VariantMeshDefinition" />
            </SLOT>
            <SLOT name="other"><VARIANT_MESH_REFERENCE definition="gone.VariantMeshDefinition" /></SLOT>
        </VARIANT_MESH>"#.to_vec());
        f.insert("b.variantmeshdefinition".to_owned(), br#"<VARIANT_MESH>
            <SLOT name="s"><VARIANT_MESH model="shield.rigid_model_v2" decal_diffuse="P/d"><TINT red="1" green="0.5" blue="0.25" random_range="0" /></VARIANT_MESH></SLOT>
        </VARIANT_MESH>"#.to_vec());
        f.insert("p/d.texture_array".to_owned(), b"P/one\nP/two\n".to_vec());
        f
    }

    #[test]
    fn walks_slots_and_references() {
        let f = files();
        let read = |p: &str| f.get(p).cloned();
        let u = assemble(&read, "A.variantmeshdefinition", 1).expect("assembles");
        assert_eq!(u.parts.iter().map(|p| p.model.as_str()).collect::<Vec<_>>(), ["body.rigid_model_v2", "shield.rigid_model_v2"]);
        assert_eq!((u.parts[0].masks, u.parts[0].attach.clone()), ([2, 0, 1], Attach::None));
        assert_eq!(u.parts[1].attach, Attach::Bone("bn_weapon_03".to_owned()));
        assert_eq!(u.parts[1].tint, [1.0, 0.5, 0.25, 1.0]);
        assert!(matches!(u.parts[1].decal_diffuse.as_deref(), Some("p/one.dds" | "p/two.dds")));
        assert_eq!(u.missing, ["gone.variantmeshdefinition"]);
        assert_eq!(assemble(&read, "A.variantmeshdefinition", 1), Some(u), "a seed gives one soldier");
    }

    #[test]
    fn an_unreadable_definition_is_nothing() {
        let f = files();
        assert!(assemble(&|p: &str| f.get(p).cloned(), "none.variantmeshdefinition", 1).is_none());
    }
}
