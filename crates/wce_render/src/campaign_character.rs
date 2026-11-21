//! A campaign map's characters as the game draws them (docs/plans/startpos_editing.md, "The models"): a character's art set
//! (`campaign_character_art_sets`) gives rows of `campaign_character_arts` by rank, age, season and faction leader, each a uniform for
//! land, for an army at sea and for a navy (`agent_uniforms`: the uniform's variant mesh) and their animation sets. The animation sets
//! are the campaign's own text tables (`animations/campaign_animation_tables/`): `animation_set_table.txt` (a set's states and their
//! fragments), `fragment_table.txt` (a fragment's lines: `single:idle:<clip>:<weight>:...`) and `animation_table.txt` (a clip's
//! skeleton). The figure plays its `NORMAL` state's idles, as a battle soldier plays his `STAND` (`unit_animation::Figure`).
//!
//! The row: the faction leader's for a faction's leader, the season's (`winter` in winter, else `none`), rank 0 (every character's
//! rank and experience are 0 at a campaign's start: `CAMPAIGN_SKILLS`), the adult's (age 16: a figure on the map leads an army, or
//! is an agent). ASSUMPTION: the religion column is not matched.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_formats::anim_tables::FragmentEntry;
use wce_vfs::files::Files;

use crate::unit_animation::{AnimationSet, Figure, Rig, UnitInstance};
use crate::unit_assembly::{Reader, Rng, assemble};
use crate::unit_catalog::{hex_colour, variant_path};

const TABLES: &str = "animations/campaign_animation_tables";

/// Where a character stands: on land, an army at sea, a navy.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Afloat {
    Land,
    Sea,
    Navy,
}

/// What a character's figure is made from.
#[derive(Clone, Debug)]
pub struct Wanted {
    pub art_set: String,
    pub afloat: Afloat,

    /// It leads its faction (the faction leader's row).
    pub leader: bool,

    /// The view is in winter (the `winter` row).
    pub winter: bool,

    /// Its faction (its colours).
    pub faction: String,

    /// Its variation's seed (the variant mesh's picks, the idles' order).
    pub seed: u64,
}

/// A row of `campaign_character_arts`.
#[derive(Clone, Debug, Default)]
struct ArtRow {
    level: u32,
    age: u32,
    season: String,
    leader: bool,
    uniforms: [String; 3],
    animations: [String; 3],
}

/// A row of `campaign_character_art_sets`.
#[derive(Clone, Debug, Default)]
struct ArtSet {
    agent: String,
    id: String,
    culture: String,
    subculture: String,
    faction: String,
    male: bool,
}

/// The tables a campaign character's model and animation come from.
#[derive(Clone, Debug, Default)]
pub struct CampaignCatalog {
    arts: HashMap<String, Vec<ArtRow>>,
    sets: Vec<ArtSet>,

    /// `agent_uniforms`: a uniform's variant mesh.
    uniforms: HashMap<String, String>,

    /// An animation set's fragments by state.
    anim_sets: HashMap<String, Vec<(String, Vec<String>)>>,

    /// A fragment's idle clips and their weights.
    idles: HashMap<String, Vec<(String, f32)>>,

    /// A clip's skeleton (its path normalised).
    skeletons: HashMap<String, String>,

    /// By faction: primary, secondary and uniform colours, linear.
    pub colours: HashMap<String, [[f32; 3]; 4]>,
}

/// The rows of a db table (every file of it, in `files`' order), the columns as text in `columns`' order; an empty column where the
/// table's version lacks it.
fn rows(files: &dyn Files, schema: &Schema, table: &str, columns: &[&str]) -> Vec<Vec<String>> {
    let name = format!("{table}_tables");
    let mut out = vec![];
    for path in files.list(&format!("db/{name}/"), "") {
        let Ok(bytes) = files.read(&path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(&name));
        let Ok(db) = DB::decode(&mut Cursor::new(bytes.to_vec()), &Some(extra)) else { continue };
        let at: Vec<Option<usize>> = columns.iter().map(|c| db.column_position_by_name(c)).collect();
        out.extend(
            db.data().iter().map(|row| at.iter().map(|k| k.map(|k| row[k].data_to_string().to_string()).unwrap_or_default()).collect()),
        );
    }
    out
}

/// A campaign animation table's lines, past its version line.
fn table_lines(files: &dyn Files, name: &str) -> Vec<String> {
    files
        .read(&format!("{TABLES}/{name}"))
        .map(|b| String::from_utf8_lossy(&b).lines().skip(1).map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

impl CampaignCatalog {
    /// Reads the tables (the db's through `schema`, the animation tables as text).
    pub fn load(files: &dyn Files, schema: &Schema) -> Self {
        let mut c = Self::default();
        let truth = |s: &str| s == "true" || s == "1";
        let cols = [
            "art_set_id",
            "level",
            "age",
            "season",
            "uniform",
            "sea_uniform",
            "navy_uniform",
            "land_animation",
            "sea_animation",
            "navy_animation",
            "is_faction_leader",
        ];
        for r in rows(files, schema, "campaign_character_arts", &cols) {
            c.arts.entry(r[0].clone()).or_default().push(ArtRow {
                level: r[1].parse().unwrap_or(0),
                age: r[2].parse().unwrap_or(0),
                season: r[3].clone(),
                leader: truth(&r[10]),
                uniforms: [r[4].clone(), r[5].clone(), r[6].clone()],
                animations: [r[7].clone(), r[8].clone(), r[9].clone()],
            });
        }
        let cols = ["agent_type", "art_set_id", "culture", "subculture", "faction", "is_male"];
        c.sets = rows(files, schema, "campaign_character_art_sets", &cols)
            .into_iter()
            .map(|r| ArtSet {
                agent: r[0].clone(),
                id: r[1].clone(),
                culture: r[2].clone(),
                subculture: r[3].clone(),
                faction: r[4].clone(),
                male: truth(&r[5]),
            })
            .collect();
        for r in rows(files, schema, "agent_uniforms", &["uniform_name", "filename"]) {
            c.uniforms.entry(r[0].to_lowercase()).or_insert_with(|| r[1].to_lowercase());
        }
        for r in rows(files, schema, "factions", &["key", "primary_colour_hex", "secondary_colour_hex", "uniform_colour_hex"]) {
            c.colours.entry(r[0].clone()).or_insert([hex_colour(&r[1]), hex_colour(&r[2]), hex_colour(&r[3]), [0.0; 3]]);
        }

        // `key:<set>`, then `state:<state>` and its `fragment:<fragment>` lines.
        let mut set = String::new();
        for l in table_lines(files, "animation_set_table.txt") {
            if let Some(k) = l.strip_prefix("key:") {
                set = k.to_owned();
                c.anim_sets.entry(set.clone()).or_default();
            } else if let Some(s) = l.strip_prefix("state:") {
                c.anim_sets.entry(set.clone()).or_default().push((s.to_owned(), vec![]));
            } else if let Some(f) = l.strip_prefix("fragment:")
                && let Some(last) = c.anim_sets.get_mut(&set).and_then(|s| s.last_mut())
            {
                last.1.push(f.to_owned());
            }
        }
        // `key:<fragment>`, then `single:idle:<clip>:<weight>:<state>:<blend>:`.
        let mut fragment = String::new();
        for l in table_lines(files, "fragment_table.txt") {
            if let Some(k) = l.strip_prefix("key:") {
                fragment = k.to_owned();
            } else if let Some(rest) = l.strip_prefix("single:idle:") {
                let mut p = rest.split(':');
                if let (Some(clip), Some(weight)) = (p.next(), p.next()) {
                    c.idles.entry(fragment.clone()).or_default().push((clip.to_owned(), weight.parse().unwrap_or(1.0)));
                }
            }
        }
        // `<clip>:_:_:<skeleton>:`.
        for l in table_lines(files, "animation_table.txt") {
            let p: Vec<&str> = l.split(':').collect();
            if let (Some(clip), Some(skeleton)) = (p.first(), p.get(3)) {
                c.skeletons.insert(wce_vfs::normalize(clip), (*skeleton).to_owned());
            }
        }
        c
    }

    /// Whether it read anything (a game without campaign characters' tables reads nothing).
    pub fn is_empty(&self) -> bool {
        self.arts.is_empty()
    }

    /// An agent's art set when its startpos names none: the art sets of its agent type, by its faction, else its subculture, else
    /// its culture, else any (the first of the table's order).
    pub fn art_set_for(&self, agent: &str, faction: &str, subculture: &str, culture: &str) -> Option<&str> {
        let of: Vec<&ArtSet> = self.sets.iter().filter(|s| s.agent == agent && s.male).collect();
        [
            of.iter().find(|s| s.faction == faction),
            of.iter().find(|s| !subculture.is_empty() && s.subculture == subculture),
            of.iter().find(|s| !culture.is_empty() && s.culture == culture),
            of.first(),
        ]
        .into_iter()
        .flatten()
        .next()
        .map(|s| s.id.as_str())
    }

    /// The row of an art set drawn: by its faction leader flag, then the season (`winter` or `none`), rank 0, the oldest.
    fn row(&self, art_set: &str, leader: bool, winter: bool) -> Option<&ArtRow> {
        let rows = self.arts.get(art_set)?;
        let season = if winter { "winter" } else { "none" };
        let score = |r: &ArtRow| (r.leader == leader, r.season == season, r.level == 0, r.age);
        rows.iter().filter(|r| !r.uniforms[0].is_empty() || !r.uniforms[1].is_empty()).max_by_key(|r| score(r))
    }

    /// The figure of a character: its art set's uniform and animation set where it stands.
    ///
    /// # Errors
    ///
    /// Returns which link is missing.
    pub fn figure(&self, read: &Reader, w: &Wanted) -> Result<CampaignFigure, String> {
        let (art_set, faction, seed) = (w.art_set.as_str(), w.faction.as_str(), w.seed);
        let row = self.row(art_set, w.leader, w.winter).ok_or_else(|| format!("no campaign_character_arts row for '{art_set}'"))?;
        let k = w.afloat as usize;
        let uniform = &row.uniforms[k];
        let variant = self.uniforms.get(&uniform.to_lowercase()).ok_or_else(|| format!("no agent_uniforms row for '{uniform}'"))?;
        let parts = assemble(read, &variant_path(variant), seed).ok_or_else(|| format!("variant '{variant}' does not load"))?;
        let anim = &row.animations[k];
        let set = self.animation_set(anim).ok_or_else(|| format!("no idles for the animation set '{anim}'"))?;
        let rig = Rig::load(read, &set).ok_or_else(|| format!("skeleton '{}' does not load", set.skeleton))?;
        let mut figure = Figure { parts, rig, set, segments: vec![] };
        let mut rng = Rng(seed ^ 0x5851_F42D_4C95_7F2D);
        let schedule = vec!["STAND".to_owned(); 8];
        if !figure.play(read, &schedule, &[], &mut rng) {
            return Err(format!("the idles of '{anim}' do not load"));
        }
        let white = [[1.0; 3], [1.0; 3], [1.0; 3], [0.0; 3]];
        let colours = self.colours.get(faction).copied().unwrap_or(white);
        let phase = rng.unit() * figure.duration();
        Ok(CampaignFigure { figure, colours, phase })
    }

    /// An animation set's `NORMAL` idles as a battle set's `STAND` slot (each line weighted by its `selection_weight`), on the first
    /// clip's skeleton.
    fn animation_set(&self, key: &str) -> Option<AnimationSet> {
        let states = self.anim_sets.get(key)?;
        let normal = states.iter().find(|(s, _)| s == "NORMAL").or_else(|| states.first())?;
        let lines: Vec<FragmentEntry> = normal
            .1
            .iter()
            .flat_map(|f| self.idles.get(f).into_iter().flatten())
            .map(|(clip, weight)| FragmentEntry {
                slot: "STAND".to_owned(),
                filename: clip.clone(),
                flags: vec![("selection_weight".to_owned(), format!("{weight}"))],
                ..FragmentEntry::default()
            })
            .collect();
        let skeleton = self.skeletons.get(&wce_vfs::normalize(&lines.first()?.filename))?.clone();
        Some(AnimationSet { skeleton, slots: HashMap::from([("STAND".to_owned(), lines)]) })
    }
}

/// A campaign character's figure, idling.
#[derive(Clone, Debug, Default)]
pub struct CampaignFigure {
    pub figure: Figure,
    pub colours: [[f32; 3]; 4],
    phase: f32,
}

impl CampaignFigure {
    /// What the scene draws at `t` (seconds), in the model's units (metres).
    pub fn instances(&self, t: f32, casts_shadow: bool) -> Vec<UnitInstance> {
        self.figure.instances(t + self.phase, 1.0, self.colours, casts_shadow)
    }

    /// Its models, for waiting on them.
    pub fn models(&self) -> Vec<String> {
        self.figure.parts.parts.iter().map(|p| p.model.clone()).collect()
    }
}
