//! A unit's model from its variant mesh definition: which parts one soldier wears, picked by a seed.
//!
//! The engine's variation (`VARIANT_MESH_VARIATION`, `VARIANT_MESH_PART`, `VARIANT_MESH_PART_SLOT`): per slot a roll against its `probability`,
//! then one alternative; a `VARIANT_MESH_REFERENCE` stands for the file it names. The picks here use the editor's own generator: the same seed
//! gives the same soldier in the editor, not the soldier the game would pick. Each node draws from a generator of its own (the seed and its
//! place in the tree), so [`Pins`] that force one slot leave the others as they were. **ASSUMPTION** (by their names and values, not traced): the
//! probability is how often a slot is filled, every alternative is as likely, `maskN` is the faction colour the mask's channel N takes, and a
//! tint applies to the mask's first channel.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::variant_mesh::{Alternative, NodePath, Slot, VariantMesh, VariantMeshFile, texture_array};
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

    /// Where it comes from in the focus file ([`Pins::focus`], the root definition by default): its mesh, or the reference it was reached
    /// through (another file's meshes are that reference's). `None`: not from the focus file.
    pub node: Option<NodePath>,
}

/// One soldier.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssembledUnit {
    pub parts: Vec<AssembledPart>,

    /// The definitions named that could not be read: a mod's files, some of them gone.
    pub missing: Vec<String>,
}

/// What a slot shows whatever the seed says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotPin {
    /// Empty.
    Off,

    /// This alternative, even when its probability would leave the slot empty.
    Alternative(usize),
}

/// Choices that take the seed's place, by the [`NodePath`]s of the focus file: an editor's way to show the part being edited. A choice
/// leaves the others as the seed has them: each node draws from a generator of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pins {
    /// The file the paths name (normalized): the root definition when `None`, else a file it references, wherever it does.
    pub focus: Option<String>,

    /// A slot with an alternative that leads to the focus file (it, or a file that references it) takes that one, filled whatever its
    /// probability: the focus file is worn.
    pub show_focus: bool,

    pub slots: HashMap<NodePath, SlotPin>,

    /// A mesh's tint, by its index.
    pub tints: HashMap<NodePath, usize>,

    /// A mesh's decal among its `.texture_array`'s.
    pub decals: HashMap<NodePath, usize>,
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

/// A node's key from its parent's and its place: what its generator is seeded with.
fn key(parent: u64, place: u64) -> u64 {
    Rng(parent ^ place.wrapping_mul(0xD6E8_FEB8_6659_FD93)).next()
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

/// The attachment points of a model (LOD 0), by name: what a slot's `attach_point` can name besides the weapon bones.
pub fn attachment_points(read: &Reader, model: &str) -> Vec<String> {
    let Some(bytes) = read(&normalize(model)) else { return vec![] };
    let Ok(model) = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())) else { return vec![] };
    let mut names: Vec<String> = model
        .lods()
        .first()
        .map(|l| l.mesh_blocks().iter().flat_map(|b| b.material().attachment_points()).map(|p| p.name().to_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    names
}

/// A decal's alternatives: `name` is a `.texture_array` listing them, else a texture alone.
pub fn decals(read: &Reader, name: &str) -> Vec<String> {
    let name = normalize(name);
    if let Some(list) = read(&format!("{name}.texture_array")) {
        return texture_array(&list).iter().map(|l| format!("{}.dds", normalize(l))).collect();
    }

    vec![if name.ends_with(".dds") { name } else { format!("{name}.dds") }]
}

/// A decal's file: `name`'s alternatives, `pick` chooses.
fn decal(read: &Reader, name: Option<&str>, pick: usize) -> Option<String> {
    let all = decals(read, name?);
    all.get(pick % all.len().max(1)).cloned()
}

struct Assembler<'a> {
    read: &'a Reader<'a>,
    seed: u64,
    pins: &'a Pins,
    files: HashMap<String, Option<std::rc::Rc<VariantMeshFile>>>,

    /// Whether a file is the focus file or references it, at any depth.
    leads: HashMap<String, bool>,
    out: AssembledUnit,

    /// A ship's definition: only its intact pieces, and a part on every point of a slot's list.
    ship: bool,
}

/// A ship's piece in a damaged state: `bow_01_dm2` (`_dm1` is the piece intact).
fn damaged(slot: &str) -> bool {
    slot.rsplit_once("_dm").is_some_and(|(_, n)| n.parse::<u32>().is_ok_and(|n| n > 1))
}

/// Where the walk is: the node's key, and its path in the focus file (`None` above it). Pins apply in the focus file itself, not in the files
/// it references, whose parts take their reference's path.
#[derive(Clone)]
struct At {
    key: u64,
    path: Option<NodePath>,
    pinned: bool,
}

impl At {
    fn child(&self, place: usize) -> Self {
        let mut path = self.path.clone();
        if self.pinned
            && let Some(p) = &mut path
        {
            p.push(place);
        }

        Self { key: key(self.key, place as u64 + 1), path, pinned: self.pinned }
    }

    /// Into a reference's file: the focus file's root, or a file below or above it.
    fn into(self, file: &str, focus: Option<&str>) -> Self {
        if self.path.is_none() && focus == Some(file) {
            Self { path: Some(vec![]), pinned: true, ..self }
        } else {
            Self { pinned: false, ..self }
        }
    }

    fn pin<'p, T>(&self, of: &'p HashMap<NodePath, T>) -> Option<&'p T> {
        self.pinned.then(|| of.get(self.path.as_ref()?)).flatten()
    }
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

    /// Whether `path` is the focus file or a file that references it, at any depth.
    fn leads(&mut self, path: &str, depth: u32) -> bool {
        let path = normalize(path);
        let Some(focus) = self.pins.focus.clone() else { return false };
        if path == focus {
            return true;
        }

        if let Some(&known) = self.leads.get(&path) {
            return known;
        }

        // Not known while it is walked: a loop leads nowhere.
        self.leads.insert(path.clone(), false);
        let found = depth < 32
            && self.file(&path).is_some_and(|f| {
                let refs: Vec<String> = f
                    .doc
                    .elements()
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.name == "VARIANT_MESH_REFERENCE")
                    .filter_map(|(e, _)| f.doc.get(e, "definition"))
                    .collect();
                refs.iter().any(|r| self.leads(r, depth + 1))
            });
        self.leads.insert(path, found);
        found
    }

    fn slot(&mut self, slot: &Slot, at: &At, parent: Option<&str>, attach: &Attach, depth: u32) {
        if slot.alternatives.is_empty() || (self.ship && damaged(&slot.name)) {
            return;
        }

        // A ship's list of points: a part on each, picked each time. **ASSUMPTION**: `use_different_attach_point_parts` (on every such slot of
        // Attila's ships) asks for that.
        if self.ship
            && let Some(points) = slot.attach_point.as_deref().filter(|a| a.contains(','))
        {
            let points: Vec<&str> = points.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
            for (k, point) in points.into_iter().enumerate() {
                let one = Slot { attach_point: Some(point.to_owned()), ..slot.clone() };
                let at = At { key: key(at.key, k as u64 + 1), ..at.clone() };
                self.slot(&one, &at, parent, attach, depth);
            }

            return;
        }

        // The roll and the pick are drawn whatever a pin says, so a pin changes nothing else.
        let mut rng = Rng(key(self.seed, at.key));
        let roll = rng.unit();
        let mut pick = rng.below(slot.alternatives.len());
        // Above the focus file, the way to it.
        let towards = if self.pins.show_focus && at.path.is_none() {
            (0..slot.alternatives.len()).find(|&k| match &slot.alternatives[k] {
                Alternative::Reference { definition, .. } => self.leads(definition, depth),
                Alternative::Mesh(_) => false,
            })
        } else {
            None
        };

        match (at.pin(&self.pins.slots), towards) {
            (Some(SlotPin::Off), _) => return,
            (Some(SlotPin::Alternative(n)), _) => pick = (*n).min(slot.alternatives.len() - 1),
            (None, Some(k)) => pick = k,
            (None, None) if slot.probability.is_some_and(|p| roll >= p) => return,
            (None, None) => {}
        }

        // A person's slot names one point.
        let attach = match slot.attach_point.as_deref().and_then(|a| a.split(',').next()).map(str::trim) {
            None | Some("") => attach.clone(),
            Some(point) if point.starts_with("weapon_") => Attach::Bone(format!("bn_{point}")),
            Some(point) => match parent.and_then(|model| attachment_point(self.read, model, point)) {
                Some((bone, matrix)) => Attach::Point { bone, matrix },
                None => attach.clone(),
            },
        };

        let at = at.child(pick);
        match &slot.alternatives[pick] {
            Alternative::Mesh(mesh) => self.mesh(mesh, &at, parent, &attach, depth),
            Alternative::Reference { definition, .. } => {
                if depth < 32
                    && let Some(file) = self.file(definition)
                {
                    let focus = self.pins.focus.clone();
                    self.mesh(&file.root, &at.into(&normalize(definition), focus.as_deref()), parent, &attach, depth + 1);
                }
            }
        }
    }

    fn mesh(&mut self, mesh: &VariantMesh, at: &At, parent: Option<&str>, attach: &Attach, depth: u32) {
        let model = mesh.model.as_deref().map(normalize);
        if let Some(model) = &model {
            let mut rng = Rng(key(self.seed, at.key ^ 0xDEC4_1000));
            let decal_pick = rng.next() as usize;
            let decal_pick = at.pin(&self.pins.decals).copied().unwrap_or(decal_pick);
            let tint_pick = rng.below(mesh.tints.len());
            let tint = mesh.tints.get(at.pin(&self.pins.tints).copied().unwrap_or(tint_pick));
            self.out.parts.push(AssembledPart {
                model: model.clone(),
                decal_diffuse: decal(self.read, mesh.decal_diffuse.as_deref(), decal_pick),
                decal_normal: decal(self.read, mesh.decal_normal.as_deref(), decal_pick),
                masks: std::array::from_fn(|i| mesh.masks[i].filter(|m| *m < 3).unwrap_or(i as u8)),
                tint: tint.map_or([0.0; 4], |t| [t.red, t.green, t.blue, 1.0]),
                attach: attach.clone(),
                node: at.path.clone(),
            });
        }

        let parent = model.as_deref().or(parent);
        for (i, slot) in mesh.slots.iter().enumerate() {
            self.slot(slot, &at.child(i), parent, attach, depth);
        }
    }
}

/// One soldier of a definition (`variantmeshes/variantmeshdefinitions/<name>.variantmeshdefinition`); `None` when it can't be read.
pub fn assemble(read: &Reader, definition: &str, seed: u64) -> Option<AssembledUnit> {
    assemble_as(read, definition, seed, false)
}

/// One soldier of a definition already read (an editor's, with its edits), with `pins` taking the seed's place where they say. To show a
/// file the definition references with its edits, `read` gives that file's edited text and `pins.focus` names it.
pub fn assemble_with(read: &Reader, file: &VariantMeshFile, seed: u64, pins: &Pins) -> AssembledUnit {
    let mut a = Assembler { read, seed, pins, files: HashMap::new(), leads: HashMap::new(), out: AssembledUnit::default(), ship: false };
    let root =
        if pins.focus.is_none() { At { key: 0, path: Some(vec![]), pinned: true } } else { At { key: 0, path: None, pinned: false } };
    a.mesh(&file.root, &root, None, &Attach::None, 0);
    a.out
}

/// A ship of a definition (`unit_variants_ships.variant`): its pieces intact (the `_dm2` slots are their damaged states), a part on every point
/// of a list (the shields along its sides).
pub fn assemble_ship(read: &Reader, definition: &str, seed: u64) -> Option<AssembledUnit> {
    assemble_as(read, definition, seed, true)
}

fn assemble_as(read: &Reader, definition: &str, seed: u64, ship: bool) -> Option<AssembledUnit> {
    let pins = Pins::default();
    let mut a = Assembler { read, seed, pins: &pins, files: HashMap::new(), leads: HashMap::new(), out: AssembledUnit::default(), ship };
    let root = a.file(definition)?;
    a.mesh(&root.root, &At { key: 0, path: Some(vec![]), pinned: true }, None, &Attach::None, 0);
    Some(a.out)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use wce_formats::variant_mesh::VariantMeshFile;

    use super::{Attach, Pins, SlotPin, assemble, assemble_ship, assemble_with, damaged};

    fn files() -> HashMap<String, Vec<u8>> {
        let mut f = HashMap::new();
        f.insert(
            "a.variantmeshdefinition".to_owned(),
            br#"<VARIANT_MESH>
            <SLOT name="skin"><VARIANT_MESH model="Body.rigid_model_v2" mask0="2" mask1="0" mask2="1" /></SLOT>
            <SLOT name="never" probability="0"><VARIANT_MESH model="no.rigid_model_v2" /></SLOT>
            <SLOT name="weapon_1" attach_point="weapon_03">
                <VARIANT_MESH_REFERENCE definition="B.VariantMeshDefinition" />
            </SLOT>
            <SLOT name="other"><VARIANT_MESH_REFERENCE definition="gone.VariantMeshDefinition" /></SLOT>
        </VARIANT_MESH>"#
                .to_vec(),
        );
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
    fn a_ship_is_intact_with_a_part_on_every_point() {
        let mut f = HashMap::new();
        f.insert("s.variantmeshdefinition".to_owned(), br#"<VARIANT_MESH>
            <SLOT name="bow_01_dm1"><VARIANT_MESH model="bow1.rigid_model_v2" /></SLOT>
            <SLOT name="bow_01_dm2"><VARIANT_MESH model="bow2.rigid_model_v2" /></SLOT>
            <SLOT name="port_01_dm1"><VARIANT_MESH model="port.rigid_model_v2">
                <SLOT name="shields" attach_point="a,b, c" use_different_attach_point_parts="true"><VARIANT_MESH model="shield.rigid_model_v2" /></SLOT>
            </VARIANT_MESH></SLOT>
        </VARIANT_MESH>"#.to_vec());
        let read = |p: &str| f.get(p).cloned();
        let ship = assemble_ship(&read, "s.variantmeshdefinition", 3).expect("assembles");
        let models: Vec<&str> = ship.parts.iter().map(|p| p.model.as_str()).collect();
        assert_eq!(
            models,
            ["bow1.rigid_model_v2", "port.rigid_model_v2", "shield.rigid_model_v2", "shield.rigid_model_v2", "shield.rigid_model_v2"]
        );
        assert_eq!(
            assemble(&read, "s.variantmeshdefinition", 3).expect("assembles").parts.len(),
            4,
            "a unit's slot takes the list's first point"
        );
        assert!(damaged("stern_02_dm2") && !damaged("stern_02_dm1") && !damaged("sails") && !damaged("x_dmz"));
    }

    #[test]
    fn pins_change_their_slot_only() {
        let mut f = files();
        f.insert(
            "c.variantmeshdefinition".to_owned(),
            br#"<VARIANT_MESH>
            <SLOT name="head"><VARIANT_MESH model="h1"/><VARIANT_MESH model="h2"/><VARIANT_MESH model="h3"/></SLOT>
            <SLOT name="body"><VARIANT_MESH model="b1"/><VARIANT_MESH model="b2"/><VARIANT_MESH model="b3"/><VARIANT_MESH model="b4"/></SLOT>
            <SLOT name="cape" probability="0.5"><VARIANT_MESH model="c1"/><VARIANT_MESH model="c2"/></SLOT>
            <SLOT name="shield"><VARIANT_MESH_REFERENCE definition="b.variantmeshdefinition"/></SLOT>
        </VARIANT_MESH>"#
                .to_vec(),
        );
        let read = |p: &str| f.get(p).cloned();
        let file = VariantMeshFile::parse(f["c.variantmeshdefinition"].clone()).expect("parses");
        let models = |u: &super::AssembledUnit| u.parts.iter().map(|p| p.model.clone()).collect::<Vec<_>>();
        for seed in 0..64 {
            let free = assemble_with(&read, &file, seed, &Pins::default());
            assert_eq!(Some(&free), assemble(&read, "c.variantmeshdefinition", seed).as_ref(), "no pins: the file's soldier");
            let shield = free.parts.iter().find(|p| p.model == "shield.rigid_model_v2").expect("a shield");
            assert_eq!(shield.node, Some(vec![3, 0]), "another file's part is its reference's");

            let mut pins = Pins::default();
            pins.slots.insert(vec![0], SlotPin::Alternative(2));
            pins.slots.insert(vec![2], SlotPin::Alternative(1));
            let pinned = assemble_with(&read, &file, seed, &pins);
            let mut expected = models(&free);
            expected[0] = "h3".to_owned();
            if free.parts.iter().any(|p| p.node == Some(vec![2, 0]) || p.node == Some(vec![2, 1])) {
                expected[2] = "c2".to_owned();
            } else {
                expected.insert(2, "c2".to_owned());
            }

            assert_eq!(models(&pinned), expected, "seed {seed}");
            pins.slots.insert(vec![1], SlotPin::Off);
            assert!(!assemble_with(&read, &file, seed, &pins).parts.iter().any(|p| p.node.as_ref().and_then(|n| n.first()) == Some(&1)));
        }
    }

    #[test]
    fn a_focus_file_is_worn_and_pinned_where_it_is_referenced() {
        let mut f = files();
        f.insert(
            "unit.variantmeshdefinition".to_owned(),
            br#"<VARIANT_MESH>
            <SLOT name="body"><VARIANT_MESH model="body"/></SLOT>
            <SLOT name="shield" probability="0.1"><VARIANT_MESH model="other_shield"/><VARIANT_MESH_REFERENCE definition="group.variantmeshdefinition"/></SLOT>
        </VARIANT_MESH>"#
                .to_vec(),
        );
        f.insert(
            "group.variantmeshdefinition".to_owned(),
            br#"<VARIANT_MESH><SLOT name="g"><VARIANT_MESH_REFERENCE definition="shields.variantmeshdefinition"/></SLOT></VARIANT_MESH>"#
                .to_vec(),
        );
        // The file being edited, as the editor has it.
        f.insert(
            "shields.variantmeshdefinition".to_owned(),
            br#"<VARIANT_MESH><SLOT name="s"><VARIANT_MESH model="round"/><VARIANT_MESH model="oval"/></SLOT></VARIANT_MESH>"#.to_vec(),
        );
        let read = |p: &str| f.get(p).cloned();
        let unit = VariantMeshFile::parse(f["unit.variantmeshdefinition"].clone()).expect("parses");
        let mut pins = Pins { focus: Some("shields.variantmeshdefinition".to_owned()), show_focus: true, ..Pins::default() };
        pins.slots.insert(vec![0], SlotPin::Alternative(1));
        for seed in 0..32 {
            let u = assemble_with(&read, &unit, seed, &pins);
            let models: Vec<(&str, Option<Vec<usize>>)> = u.parts.iter().map(|p| (p.model.as_str(), p.node.clone())).collect();
            assert_eq!(models, [("body", None), ("oval", Some(vec![0, 1]))], "seed {seed}");
        }
    }

    #[test]
    fn an_unreadable_definition_is_nothing() {
        let f = files();
        assert!(assemble(&|p: &str| f.get(p).cloned(), "none.variantmeshdefinition", 1).is_none());
    }
}
