//! The building to destroy, read from the packs: its folder, its pieces' intact models (`<key>_pieceNN_destruct01.rigid_model_v2`, every
//! LOD) and their effects, its logic file, and the game's templates (the damaged fort's broken-masonry and rubble blocks, and the sounds of
//! a stone and a wooden collapse).

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::{MeshBlock, RigidModel};
use rpfm_lib::files::rigidmodel::vertices::Vertex;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim_meta::{MetaBody, MetaFile};
use wce_vfs::files::Files;
use wce_walls::building::BuildingFiles;

use crate::fracture::{lod_triangles, pos};
use crate::geom::Aabb;

/// A piece of the building: its number (`piece01` is 1), its intact model and the file names of its states (a gate's leaves).
#[derive(Clone, Debug)]
pub struct SourcePiece {
    pub number: u32,
    pub model: RigidModel,

    /// The intact level's other files of the piece (two-state pieces: `_gate_closed`, `_gate_open`, ...), by file name.
    pub states: Vec<String>,
}

impl SourcePiece {
    pub fn name(&self) -> String {
        format!("piece{:02}", self.number)
    }
}

/// A piece with no plain intact file but states (a gate's leaves: `_destruct01_gate_closed`, `_gate_open`): per state its suffix
/// (`_gate_closed`) and model.
#[derive(Clone, Debug)]
pub struct StatePiece {
    pub number: u32,
    pub states: Vec<(String, RigidModel)>,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub key: String,
    pub category: String,
    pub files: BuildingFiles,
    pub pieces: Vec<SourcePiece>,

    /// The two-state pieces, broken like the others (each state on its own), and the pieces this does not break (a siege engine's
    /// skinned or looped parts), copied as they are.
    pub state_pieces: Vec<StatePiece>,
    pub other_pieces: Vec<u32>,
    pub bounds: Aabb,
    pub notes: Vec<String>,
}

pub fn decode_model(bytes: &[u8]) -> Result<RigidModel, String> {
    RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).map_err(|e| format!("rmv2: {e}"))
}

impl Source {
    /// A building by its key through the editor's files, with its `battlefield_buildings` category (for the defaults).
    ///
    /// # Errors
    ///
    /// The folder is missing, or has no intact piece that reads.
    pub fn read(files: &dyn Files, key: &str, category: &str) -> Result<Self, String> {
        Self::from_files(BuildingFiles::read_files(files, key)?, category)
    }

    /// A building from its folder's files.
    ///
    /// # Errors
    ///
    /// It has no intact piece that reads.
    pub fn from_files(files: BuildingFiles, category: &str) -> Result<Self, String> {
        let prefix = format!("{}_piece", files.key);
        let mut pieces = vec![];
        let mut notes = vec![];
        for (name, bytes) in &files.files {
            let Some(rest) = name.strip_prefix(&prefix) else { continue };
            let Some(num) = rest.get(..2).and_then(|n| n.parse::<u32>().ok()) else { continue };
            if rest[2..] != *"_destruct01.rigid_model_v2" {
                continue;
            }

            match decode_model(bytes) {
                Ok(model) if !matches!(*model.version(), 5 | 6) => notes.push(format!("{name}: version {} not handled", model.version())),
                Ok(model) => {
                    let state_prefix = format!("{prefix}{num:02}_destruct01_");
                    let states = files.files.keys().filter(|n| n.starts_with(&state_prefix) && n.ends_with(".rigid_model_v2") && !wce_walls::building::is_collapse(n) && !n.contains("_anim")).cloned().collect();
                    pieces.push(SourcePiece { number: num, model, states });
                }
                Err(e) => notes.push(format!("{name}: {e}")),
            }
        }

        // The pieces without a plain intact file: their states, else copied.
        let mut numbers = files.files.keys().filter_map(|n| n.strip_prefix(&prefix).and_then(|r| r.get(..2)).and_then(|d| d.parse::<u32>().ok())).collect::<Vec<_>>();
        numbers.sort_unstable();
        numbers.dedup();
        let mut state_pieces = vec![];
        let mut other_pieces = vec![];
        for num in numbers.into_iter().filter(|n| !pieces.iter().any(|p: &SourcePiece| p.number == *n)) {
            let lead = format!("{prefix}{num:02}_destruct01_");
            let mut states = vec![];
            for (name, bytes) in &files.files {
                let Some(state) = name.strip_prefix(&lead).and_then(|r| r.strip_suffix(".rigid_model_v2")) else { continue };
                if state.ends_with("anim") {
                    continue;
                }

                match decode_model(bytes) {
                    Ok(model) if matches!(*model.version(), 5 | 6) => states.push((format!("_{state}"), model)),
                    Ok(_) | Err(_) => notes.push(format!("{name}: not read, the piece is copied as it is")),
                }
            }

            if states.is_empty() {
                other_pieces.push(num);
            } else {
                state_pieces.push(StatePiece { number: num, states });
            }
        }

        if pieces.is_empty() && state_pieces.is_empty() {
            return Err(format!("{}: no intact piece (`{prefix}NN_destruct01.rigid_model_v2`) to break", files.key));
        }

        pieces.sort_by_key(|p| p.number);
        let mut bounds = Aabb::EMPTY;
        for m in pieces.iter().map(|p| &p.model).chain(state_pieces.iter().flat_map(|s| s.states.iter().map(|x| &x.1))) {
            bounds.merge(&model_bounds(m));
        }

        Ok(Self { key: files.key.clone(), category: category.to_owned(), files, pieces, state_pieces, other_pieces, bounds, notes })
    }

    /// The logic file's bytes.
    pub fn logic(&self) -> Option<&Vec<u8>> {
        self.files.files.get(&self.files.logic_name())
    }

    /// Every texture the intact pieces name.
    pub fn textures(&self) -> Vec<String> {
        self.models().flat_map(|m| m.lods().iter().flat_map(|l| l.mesh_blocks().iter().flat_map(|b| b.material().textures().iter().map(|t| t.path().clone())))).collect()
    }

    /// The intact models: the pieces', and every state of the two-state pieces.
    pub fn models(&self) -> impl Iterator<Item = &RigidModel> {
        self.pieces.iter().map(|p| &p.model).chain(self.state_pieces.iter().flat_map(|s| s.states.iter().map(|x| &x.1)))
    }

    /// The files of a piece (every level), by file name.
    pub fn piece_files(&self, number: u32) -> Vec<&String> {
        let lead = format!("{}_piece{number:02}_", self.key);
        self.files.files.keys().filter(|n| n.starts_with(&lead)).collect()
    }
}

/// The box of a model's first LOD (its vertices plus their pivots).
pub fn model_bounds(m: &RigidModel) -> Aabb {
    m.lods().first().map_or(Aabb::EMPTY, |lod| Aabb::of(lod_triangles(lod).iter().flatten().flatten().map(pos)))
}

/// The game's templates for what the generator makes.
#[derive(Clone, Debug, Default)]
pub struct Templates {
    /// A broken-masonry block and a rubble block (`DefaultMaterial`), with a vertex of each.
    pub damage: Option<(MeshBlock, Vertex)>,
    pub rubble: Option<(MeshBlock, Vertex)>,
    pub stone: Sounds,
    pub wood: Sounds,
}

/// A collapse's sounds: those that start it (played where the damage is), and the impacts (played where fragments land).
#[derive(Clone, Debug, Default)]
pub struct Sounds {
    pub start: Vec<u32>,
    pub impacts: Vec<u32>,
}

/// Where the templates are read (Attila's packs; a game without them falls back on the building's own blocks).
const DAMAGED_FORT: &str = "rigidmodels/buildings/roman_fort_straight/roman_fort_straight_piece01_destruct02.rigid_model_v2";
const STONE_COLLAPSE: &str = "rigidmodels/buildings/roman_fort_straight/roman_fort_straight_piece01_destruct01_anim.meta";
const WOOD_COLLAPSE: &str = "rigidmodels/buildings/barbarian_fort_straight/barbarian_fort_straight_piece01_destruct01_anim.meta";

impl Templates {
    pub fn load(files: &dyn Files) -> Self {
        let mut t = Self::default();
        if let Some(model) = files.read(DAMAGED_FORT).ok().and_then(|b| decode_model(&b).ok()) {
            let find = |word: &str| model.lods().first().and_then(|l| l.mesh_blocks().iter().find(|b| crate::materials::diffuse(b.material()).contains(word)))
                .and_then(|b| b.mesh().vertices().first().map(|v| (b.clone(), v.clone())));
            t.damage = find("damage_bricks");
            t.rubble = find("wall_rubble");
        }

        t.stone = sounds(files, STONE_COLLAPSE);
        t.wood = sounds(files, WOOD_COLLAPSE);
        t
    }
}

fn sounds(files: &dyn Files, path: &str) -> Sounds {
    let mut s = Sounds::default();
    let Some(meta) = files.read(path).ok().and_then(|b| MetaFile::parse(&b)) else { return s };
    for e in &meta.entries {
        let MetaBody::Sound(raw) = &e.body else { continue };
        let id = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let list = if e.start < 0.1 { &mut s.start } else { &mut s.impacts };
        if !list.contains(&id) {
            list.push(id);
        }
    }
    s
}

/// A `SOUND_BUILDING` entry: the sound and where it plays (the piece's model space): u32 id, 3 f32, u32 0 (the game's collapses).
pub fn sound_entry(id: u32, at: [f32; 3], time: f32) -> wce_formats::anim_meta::MetaEntry {
    let mut raw = [0u8; 20];
    raw[..4].copy_from_slice(&id.to_le_bytes());
    for (k, v) in at.iter().enumerate() {
        raw[4 + 4 * k..8 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }

    wce_formats::anim_meta::MetaEntry { kind: "SOUND_BUILDING".into(), version: 2, start: time, end: time, tag: Some(String::new()), body: MetaBody::Sound(raw) }
}
