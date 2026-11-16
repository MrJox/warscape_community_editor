//! What the generator knows of a base wall (docs/re/buildings.md, "The families" and "The kit table"): its family's frame,
//! walkway, outer face and how it changes height. In the piece's frame (scene units, which are the models' units): x along the wall, -z
//! the outer side, y = 0 the ground the piece stands on.

/// A base wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub key: &'static str,

    /// Its family in `walls.toml` (the 8 m and 15 m walls: `eight`, `fifteen`), and its name in the editor's list.
    pub family: &'static str,
    pub label: &'static str,

    /// Where the frame starts and ends along x (the 8 m pieces run 0..20 from their origin, the 15 m ones -10..10).
    pub x: (f64, f64),

    /// The walkway's height over the piece's ground.
    pub walkway: f64,

    /// The outer face's line (z): the docking line of the `models_building` row, where ladders and towers meet the wall; the ground the
    /// walkway keeps its height over is measured along it.
    pub outer_face: f64,

    /// The footprint across the wall (z), the outer side first.
    pub depth: (f64, f64),

    /// The most one stair rises, and its length along the wall at its own slope (8 m: one stair of the step piece, 2.5 over 3; 15 m: the
    /// stepped bastion's wooden stair, 10 over 12). A step of a smaller rise uses the start of the stair, at its own slope and size (cut,
    /// never squashed); larger rises chain stairs.
    pub step_rise: f64,
    pub step_run: f64,

    /// Between two chained step sections, flat.
    pub step_gap: f64,

    /// The steepest the wall itself may slope over a step (rise per unit along the wall): 8 m walls follow their stair's own slope (only
    /// the grass bank slopes, the stair replaces the walkway and palisade); 15 m walls keep the stone gentle and the stair is lengthened
    /// to it.
    pub max_slope: f64,

    /// The shortest flat run between two steps (no pile-ups).
    pub min_run: f64,

    /// A climb of about this much over a short straight stretch is one whole step piece (15 m: the stepped bastion, 10 over its 20, as
    /// vanilla); 0: never.
    pub whole_rise: f64,

    /// The pieces that close a free end, at the start and at the end: the game's own, placed past the ends as they are (not bent nor
    /// fitted to the ground), their joining x (the end of their walkway that meets the wall) on the wall's end. 8 m: the endcaps (2.6,
    /// joining at 0); 15 m: the tower ends (a tower and the start of a walkway, joining at 10 and -10).
    pub cap_start: &'static str,
    pub cap_end: &'static str,
    pub cap_join: (f64, f64),

    /// The foot: below `foot.1` the mesh stretches so its bottom (`foot.0`) reaches `bury` under the terrain where the terrain is lower
    /// than the piece's level (8 m: the buried 2 m under y = 0; 15 m: the bottom 2 m of the plinth, which stands on y = 0).
    pub foot: (f64, f64),
    pub bury: f64,

    /// The culture's step pieces, going up and going down along +x.
    pub step_up: &'static str,
    pub step_down: &'static str,

    /// The step section of a step piece (its frame): the x range going up and going down, the walkway's height where it starts (up,
    /// down), what of it is brought (z then y; none: its whole cross-section), and whether it replaces the base over its length (both
    /// families: the step piece's own walls, bank and stair, authored to join the straight where they are flat) or is added to it.
    pub slab_up: (f64, f64),
    pub slab_down: (f64, f64),
    pub slab_walk: (f64, f64),
    pub slab_zy: Option<((f64, f64), (f64, f64))>,
    pub step_replaces: bool,

    /// Only what lies within this (y) of the stair's slope is brought (15 m: the treads and stringers, not the stair well's walls).
    pub slab_band: Option<f64>,

    /// The stair spread across the whole walkway (15 m: one stair from side to side, a single platform, instead of a stair on the inner
    /// half beside a ramp on the outer one, which units walked around; user 2026-11-04).
    pub step_across: bool,

    /// The culture's door section (15 m: units go up through its doors; 8 m: none, the bank is the way up), and where its doors come
    /// out on the ground (the pipes' ground ends, the piece's frame: x, z).
    pub door: &'static str,
    pub door_at: (f64, f64),

    /// The culture's in-line towers and bastions (the same slot as a straight), used in turn at the wall's defence spacing: 15 m, the
    /// tower (`*_fort_tower_c_straight_with_extension`) then the bastion (`*_fort_straight_with_solid_squared_bastion`); 8 m, the bastion
    /// (`*_curved_bastion`; their watchtowers stand alone). Placed as the game's pieces, not bent nor fitted to the ground.
    pub defences: &'static [&'static str],
}

impl Profile {
    /// The length of one piece along the wall.
    pub fn length(&self) -> f64 {
        self.x.1 - self.x.0
    }

    /// How tightly the wall's line may bend before the wall folds over itself: as far as its footprint reaches from the line.
    pub fn fold_radius(&self) -> f64 {
        self.depth.0.abs().max(self.depth.1.abs())
    }
}

/// The wall bases' file in the app's tables folder, with the user's `walls_custom.toml` beside it (`wce_tile::user_files`; user
/// 2026-11-16: buildings added without a rebuild): their families' numbers and each base's pieces. Built in as [`DEFAULT_TEXT`].
pub const FILE_NAME: &str = "walls.toml";
pub const BUILT_IN: wce_tile::user_files::BuiltIn = &[
    ("attila", include_str!("../../../tables/attila/walls.toml")),
    ("rome_2", include_str!("../../../tables/rome_2/walls.toml")),
    ("thrones_of_britannia", include_str!("../../../tables/thrones_of_britannia/walls.toml")),
];
pub const DEFAULT_TEXT: &str = BUILT_IN[0].1;

/// The bases, from the file ([`load`]) or as shipped.
pub static WALLS: wce_tile::user_files::Registry<Walls> = wce_tile::user_files::Registry::new(FILE_NAME, BUILT_IN, parse_with);

/// The wall bases and the editor's list of them (key, label), in the file's order.
#[derive(Debug)]
pub struct Walls {
    pub bases: Vec<Profile>,
    pub choices: Vec<(&'static str, &'static str)>,
}

/// What a family gives its bases, and a base may set again: every number of a [`Profile`].
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
struct Params {
    x: Option<(f64, f64)>,
    walkway: Option<f64>,
    outer_face: Option<f64>,
    depth: Option<(f64, f64)>,
    step_rise: Option<f64>,
    step_run: Option<f64>,
    step_gap: Option<f64>,
    max_slope: Option<f64>,
    min_run: Option<f64>,
    whole_rise: Option<f64>,
    cap_join: Option<(f64, f64)>,
    foot: Option<(f64, f64)>,
    bury: Option<f64>,
    slab_up: Option<(f64, f64)>,
    slab_down: Option<(f64, f64)>,
    slab_walk: Option<(f64, f64)>,
    slab_zy: Option<((f64, f64), (f64, f64))>,
    step_replaces: Option<bool>,
    slab_band: Option<f64>,
    step_across: Option<bool>,
    door_at: Option<(f64, f64)>,
}

/// A base as the file has it.
#[derive(Debug, serde::Deserialize)]
struct BaseDef {
    key: String,
    #[serde(default)]
    label: String,
    family: String,
    #[serde(default)]
    defences: Vec<String>,
    #[serde(default)]
    cap_start: String,
    #[serde(default)]
    cap_end: String,
    #[serde(default)]
    step_up: String,
    #[serde(default)]
    step_down: String,
    #[serde(default)]
    door: String,
    #[serde(flatten)]
    params: Params,
}

#[derive(Debug, Default, serde::Deserialize)]
struct WallsFile {
    #[serde(default)]
    families: std::collections::HashMap<String, Params>,
    #[serde(default)]
    bases: Vec<BaseDef>,
}

impl Params {
    /// These numbers, the rest `under`'s.
    fn over(self, under: Self) -> Self {
        macro_rules! or {
            ($($f:ident),*) => { Self { $($f: self.$f.or(under.$f)),* } };
        }
        or!(
            x,
            walkway,
            outer_face,
            depth,
            step_rise,
            step_run,
            step_gap,
            max_slope,
            min_run,
            whole_rise,
            cap_join,
            foot,
            bury,
            slab_up,
            slab_down,
            slab_walk,
            slab_zy,
            step_replaces,
            slab_band,
            step_across,
            door_at
        )
    }
}

/// Reads a walls file (no custom one).
///
/// # Errors
///
/// As [`parse_with`].
pub fn parse(text: &str) -> Result<Walls, String> {
    parse_with(text, None)
}

/// Reads the shipped walls file and the custom one over it: a custom family's numbers over the shipped one's of its name (or a new
/// family), a custom base in place of the shipped one of its key (whole) or after them.
///
/// # Errors
///
/// Either is not TOML of its shape, there is no base, a base's family is not one of the families, a number is missing from both a base
/// and its family, or a key is listed twice.
pub fn parse_with(shipped: &str, custom: Option<&str>) -> Result<Walls, String> {
    use wce_tile::user_files::keep;
    let mut file: WallsFile = toml::from_str(shipped).map_err(|e| format!("{FILE_NAME}: {e}"))?;
    if let Some(text) = custom {
        let name = wce_tile::user_files::custom_name(FILE_NAME);
        let c: WallsFile = toml::from_str(text).map_err(|e| format!("{name}: {e}"))?;
        for (fam, p) in c.families {
            let under = file.families.get(&fam).copied().unwrap_or_default();
            file.families.insert(fam, p.over(under));
        }

        wce_tile::user_files::replace_or_add(&mut file.bases, c.bases, |a, b| a.key.eq_ignore_ascii_case(&b.key));
    }

    if file.bases.is_empty() {
        return Err("no [[bases]]".to_owned());
    }

    let mut bases = Vec::with_capacity(file.bases.len());
    for b in &file.bases {
        let Some(fam) = file.families.get(&b.family) else { return Err(format!("{}: no family '{}'", b.key, b.family)) };
        if bases.iter().any(|p: &Profile| p.key.eq_ignore_ascii_case(&b.key)) {
            return Err(format!("{}: twice", b.key));
        }

        let (p, f) = (&b.params, fam);
        macro_rules! need {
            ($field:ident) => {
                p.$field.or(f.$field).ok_or_else(|| format!("{}: no {} (nor its family's)", b.key, stringify!($field)))?
            };
        }
        bases.push(Profile {
            key: keep(&b.key),
            family: keep(&b.family),
            label: keep(if b.label.is_empty() { &b.key } else { &b.label }),
            x: need!(x),
            walkway: need!(walkway),
            outer_face: need!(outer_face),
            depth: need!(depth),
            step_rise: need!(step_rise),
            step_run: need!(step_run),
            step_gap: need!(step_gap),
            max_slope: need!(max_slope),
            min_run: need!(min_run),
            whole_rise: need!(whole_rise),
            cap_start: keep(&b.cap_start),
            cap_end: keep(&b.cap_end),
            cap_join: need!(cap_join),
            foot: need!(foot),
            bury: need!(bury),
            step_up: keep(&b.step_up),
            step_down: keep(&b.step_down),
            slab_up: need!(slab_up),
            slab_down: need!(slab_down),
            slab_walk: need!(slab_walk),
            slab_zy: p.slab_zy.or(f.slab_zy),
            step_replaces: need!(step_replaces),
            slab_band: p.slab_band.or(f.slab_band),
            step_across: need!(step_across),
            door: keep(&b.door),
            door_at: need!(door_at),
            defences: Box::leak(b.defences.iter().map(|d| keep(d)).collect::<Vec<_>>().into_boxed_slice()),
        });
    }

    let choices = bases.iter().map(|p| (p.key, p.label)).collect();
    Ok(Walls { bases, choices })
}

/// Reads the walls files in the tables folder (`wce_tile::user_files::tables_dir`): the bases from then on.
pub fn load(tables: &std::path::Path, game: &str) {
    WALLS.load(tables, game);
}

/// The bases, in the editor's order.
#[must_use]
pub fn bases() -> &'static [Profile] {
    &WALLS.get().bases
}

/// The editor's list of bases (key, label): every base, whatever the game has.
#[must_use]
pub fn choices() -> &'static [(&'static str, &'static str)] {
    &WALLS.get().choices
}

/// The editor's list of bases ([`choices`], `walls.toml`) that the game's files have whole (every piece's logic file: Rome 2
/// has the barbarian, roman, greek, egyptian and persian forts; TDD's bases come with its packs), made once per game and mount (made again
/// when mods are added or taken away: `Files::mount_id`); every base without the files' game.
pub fn available_bases(files: &dyn wce_vfs::files::Files) -> &'static [(&'static str, &'static str)] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type List = &'static [(&'static str, &'static str)];
    static MADE: OnceLock<Mutex<HashMap<String, (u64, usize, List)>>> = OnceLock::new();
    let all = choices();
    let Some(game) = files.game() else { return all };
    let made = MADE.get_or_init(Mutex::default);
    let mut made = made.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mount = files.mount_id();
    // Made again for another mount, or another walls file.
    let table = all.as_ptr() as usize;
    if let Some(&(m, t, list)) = made.get(game.key)
        && m == mount
        && t == table
    {
        return list;
    }

    let has = |key: &str| key.is_empty() || files.exists(&format!("{}{key}_tech.cs2.parsed", crate::building::folder(key)));
    let whole = |p: &Profile| {
        [p.key, p.cap_start, p.cap_end, p.step_up, p.step_down, p.door].into_iter().chain(p.defences.iter().copied()).all(has)
    };
    let list: Vec<(&'static str, &'static str)> = all.iter().copied().filter(|(k, _)| profile(k).is_none_or(whole)).collect();
    let list: &'static [(&'static str, &'static str)] = Box::leak(list.into_boxed_slice());
    // The list a mount before had stays (leaked, a few bytes): the editor's property rows may still hold it.
    made.insert(game.key.to_owned(), (mount, table, list));
    list
}

/// A base by its key.
pub fn profile(key: &str) -> Option<&'static Profile> {
    bases().iter().find(|p| p.key.eq_ignore_ascii_case(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_has_the_bases_and_their_families_numbers() {
        let w = WALLS.shipped();
        assert_eq!(w.bases.len(), 15);
        assert_eq!(w.choices.len(), 15);
        let b = w.bases.iter().find(|p| p.key == "barbarian_fort_straight").unwrap();
        assert_eq!((b.family, b.label, b.x, b.walkway), ("eight", "8 m: barbarian", (0.0, 20.0), 8.0));
        assert!((b.max_slope - 2.5 / 3.0).abs() < 1e-15 && b.slab_band.is_none() && b.step_replaces);
        assert_eq!(b.defences, ["barbarian_fort_curved_bastion"]);
        let r = w.bases.iter().find(|p| p.key == "roman_fort_straight").unwrap();
        assert_eq!((r.walkway, r.slab_band, r.door), (15.0, Some(0.9), "roman_fort_straight_with_doors_a"));
        // Gondor's own frame over its family's.
        let g = w.bases.iter().find(|p| p.key == "gondor_fort_straight").unwrap();
        assert_eq!((g.x, g.cap_join, g.walkway), ((-10.5, 10.5), (10.5, -10.5), 15.0));
        assert_eq!(g.slab_zy, Some(((3.74, 8.09), (14.0, 26.5))));
    }

    #[test]
    fn every_games_walls_read_and_are_attilas() {
        // Rome 2 and Thrones of Britannia: the bases their kits' battlefield_buildings have, as Attila's file has them.
        let attila = WALLS.shipped();
        for (game, text, n) in [("rome_2", BUILT_IN[1].1, 5), ("thrones_of_britannia", BUILT_IN[2].1, 3)] {
            let w = parse(text).unwrap_or_else(|e| panic!("{game}: {e}"));
            assert_eq!(w.bases.len(), n, "{game}");
            for b in &w.bases {
                assert_eq!(Some(b), attila.bases.iter().find(|a| a.key == b.key), "{game}: {}", b.key);
            }
        }
    }

    #[test]
    fn a_custom_walls_file_changes_a_family_replaces_a_base_and_adds_one() {
        let custom = "[families.fifteen]\nwalkway = 16.0\n\n[[bases]]\nkey = \"roman_fort_straight\"\nlabel = \"mine\"\nfamily = \"eight\"\n\n[[bases]]\nkey = \"my_fort_straight\"\nfamily = \"fifteen\"\n";
        let w = parse_with(DEFAULT_TEXT, Some(custom)).unwrap();
        assert_eq!(w.bases.len(), 16);
        let roman = w.bases.iter().position(|p| p.key == "roman_fort_straight").unwrap();
        assert_eq!(roman, WALLS.shipped().bases.iter().position(|p| p.key == "roman_fort_straight").unwrap(), "in its place");
        assert_eq!((w.bases[roman].label, w.bases[roman].walkway, w.bases[roman].door), ("mine", 8.0, ""), "replaced whole");
        let greek = w.bases.iter().find(|p| p.key == "greek_fort_straight").unwrap();
        assert_eq!((greek.walkway, greek.depth), (16.0, (-6.0, 16.0)), "the family's walkway changed, its other numbers kept");
        assert_eq!(w.bases.last().unwrap().key, "my_fort_straight");
        assert_eq!(w.choices.last().unwrap(), &("my_fort_straight", "my_fort_straight"));
        assert!(parse_with(DEFAULT_TEXT, Some("x = [")).unwrap_err().starts_with("walls_custom.toml"));
    }

    #[test]
    fn a_bad_walls_file_says_why() {
        assert!(parse("[families]\n[[bases]]\nkey = \"a\"\nfamily = \"none\"\n").unwrap_err().contains("no family"));
        let missing = "[families.f]\nwalkway = 1.0\n[[bases]]\nkey = \"a\"\nfamily = \"f\"\n";
        assert!(parse(missing).unwrap_err().contains("no x"));
        // A family's numbers, the base's key: a wall of its own.
        let whole = DEFAULT_TEXT.split("\n[[bases]]").next().unwrap().to_owned()
            + "\n[[bases]]\nkey = \"my_fort_straight\"\nfamily = \"eight\"\nwalkway = 9.0\n";
        let w = parse(&whole).unwrap();
        assert_eq!(
            (w.bases[0].key, w.bases[0].label, w.bases[0].walkway, w.bases[0].bury),
            ("my_fort_straight", "my_fort_straight", 9.0, 2.0)
        );
    }

    #[test]
    fn the_bases_are_looked_for_again_in_another_mount() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        static ATTILA: wce_vfs::GameProfile = wce_vfs::GameProfile::ATTILA;

        /// Packs that have every base's files or none, as a mount with or without a mod.
        struct Packs {
            whole: AtomicBool,
            mount: AtomicU64,
        }

        impl wce_vfs::files::Files for Packs {
            fn read(&self, _: &str) -> Result<Vec<u8>, String> {
                Err("no such file".to_owned())
            }

            fn exists(&self, _: &str) -> bool {
                self.whole.load(Ordering::Relaxed)
            }

            fn list(&self, _: &str, _: &str) -> Vec<String> {
                vec![]
            }

            fn game(&self) -> Option<&wce_vfs::GameProfile> {
                Some(&ATTILA)
            }

            fn mount_id(&self) -> u64 {
                self.mount.load(Ordering::Relaxed)
            }
        }

        let all = choices().len();
        let packs = Packs { whole: AtomicBool::new(false), mount: AtomicU64::new(u64::MAX - 1) };
        let few = available_bases(&packs).len();
        assert!(few < all);
        packs.whole.store(true, Ordering::Relaxed);
        assert_eq!(available_bases(&packs).len(), few, "the same mount: the list made for it");
        packs.mount.store(u64::MAX, Ordering::Relaxed);
        assert_eq!(available_bases(&packs).len(), all, "another mount: looked for again");
    }
}
