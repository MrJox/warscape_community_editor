//! Launch in game (docs/plans/launch_in_game.md; the command line: docs/re/game_command_line.md): the open tile, exported, in a battle the
//! game starts straight into, with the tile's exported files, the tile database and the battle's XML. Two ways to hand them over:
//!
//! - **loose** (the default): a working directory (`add_working_directory`), with only the mod packs the tile's assets need ([`needs`]).
//!   A mod pack's file wins over a loose one, so when a needed pack has a path the tile ships (TDD's tile database), that launch uses the
//!   pack instead;
//! - **pack**: a mod pack `data\wce_launch.pack` (the game takes mod packs from `data\` only) named first (first listed wins), then the
//!   mod packs chosen in the editor. It is removed when the game exits, or at the next launch or start of the editor; it carries a marker,
//!   so a pack of that name the editor did not write is never removed.
//!
//! The launcher's `used_mods.txt` is never touched.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Child;

use wce_vfs::{LaunchFacts, Vfs, normalize};

use crate::settings::{LaunchArmy, TileWeather};

/// The launch pack, in the game's data folder.
pub const LAUNCH_PACK: &str = "wce_launch.pack";

/// The battle's XML in the launch pack (a path of the editor's own, so no pack shadows it).
pub const BATTLE_XML: &str = "script/wce/wce_launch.xml";

/// The file that marks a launch pack as the editor's.
const MARKER: &str = "script/wce/wce_launch_pack.txt";
const MARKER_TEXT: &[u8] = b"Written by Warscape Community Editor's Launch in game; removed when the game exits.\n";

/// Game units of a tile cell (128 m).
pub const CELL_UNITS: f64 = 256.0;

/// The battle's map.
#[derive(Clone, Debug, PartialEq)]
pub enum BattleMap {
    /// The tile alone: its variation folder (`terrain/tiles/battle/<set>/<tile>/<variation>`).
    Tile(String),

    /// A position in a battle map: the map's folder (`terrain/battles/<map>/`) and the tile's centre in fractions of its tile map (y from
    /// the top: the Londinium benchmark's 0.152559, 0.22547 is the centre of its settlement location in main_attila_map, cell 310, 324).
    Position { map: String, x: f64, y: f64, settlement: bool },
}

/// The battle's weather, as the battle XML words it.
#[derive(Clone, Debug, PartialEq)]
pub struct BattleWeather {
    /// An `.environment` of the packs; empty: the game's choice.
    pub environment: String,

    /// `Spring` .. `Winter`, and the Season row's key (`season_spring` ..).
    pub season: (&'static str, &'static str),

    /// `dry`, `rain`, `snow`.
    pub precipitation: &'static str,

    /// `day`, `night`.
    pub time_of_day: &'static str,

    /// A climate key (`ROME_Atlantic`); empty: none written.
    pub climate: String,
}

impl BattleWeather {
    /// The weather a tile is lit with: its file, and its list's filters (environment_map.xml values; a filter that is "any" takes autumn,
    /// dry, day).
    pub fn from_tile(w: &TileWeather) -> Self {
        let q = |k: &str| w.query.get(k).map(String::as_str).filter(|v| !v.is_empty() && !v.contains(',')).unwrap_or("");
        let season = match q("season") {
            "sp" => ("Spring", "season_spring"),
            "su" => ("Summer", "season_summer"),
            "wi" => ("Winter", "season_winter"),
            _ => ("Autumn", "season_autumn"),
        };

        let precipitation = match q("precipitation") {
            "rain" | "storm" => "rain",
            "snow" => "snow",
            _ => "dry",
        };

        let time_of_day = if q("time_of_day") == "night" { "night" } else { "day" };
        Self { environment: w.file.clone(), season, precipitation, time_of_day, climate: q("climate").to_owned() }
    }
}

/// The note of a coast tile launched alone (checked in game, 2026-10-30, with the game's own tile too): the game places it without its
/// sea level, so the tile is dry ground down to its edges, the sea only around it, and the plants of its sea bed stand in the air.
pub const COAST_ALONE_NOTE: &str = " (a coast tile alone: the game draws it without its sea level, dry ground to the tile's edges and its sea-bed \
     plants in the air; launch it at a placement: Settings > Tile, The Map context's placement)";

/// The climate a tile launched alone has in game: the battle map's cell climate, and a lone tile has no cells (index 0 of the tile
/// database's climates; checked in game, 2026-10-30). The battle XML's `<Climate>` is not read (CA's own battle files have it too).
pub const ALONE_CLIMATE: &str = "default";

/// The note of a tile launched alone whose look in the editor is another climate's.
pub fn climate_alone_note(climate: &str) -> String {
    format!(
        " (the tile alone has the `{ALONE_CLIMATE}` climate's ground in game (summer textures), not {climate}'s: a battle's ground climate is \
         its map's; launch it at a placement for the map's climate: Settings > Tile, The Map context's placement)"
    )
}

/// Whether a battle tile set lies on the sea (`sea`, `sea_coast`, `sea_coast_landing`, the `settlement_*_ports`, `river_mouth*`).
pub fn coastal_set(set: &str) -> bool {
    let set = set.to_ascii_lowercase();
    set.starts_with("sea") || set.starts_with("river_mouth") || set.ends_with("_ports")
}

/// What the battle XML is made from.
pub struct Battle<'a> {
    pub map: BattleMap,

    /// The tile's size in cells.
    pub cells: (u32, u32),
    pub weather: BattleWeather,

    /// The sea's surface (a key of `sea_surfaces_tables`, e.g. `wind_level_2`); empty: none written. A battle whose map has sea needs it:
    /// without it the game crashes while loading a coast tile (alone or at a placement on the campaign map; access violation).
    pub sea_surface: String,

    /// The armies, every key given ([`LaunchArmy::resolved`]).
    pub army: &'a LaunchArmy,

    /// What the game's battle XML has.
    pub facts: &'a LaunchFacts,
}

/// Text for an XML attribute or element.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The battle's XML (the benchmarks' format, as `benchmarks/londinium.benchmark` points at; docs/re/game_command_line.md): one unit a side
/// across the tile's centre, both deployment areas over the tile, no deployment phase, an hour long, no script. The sizes are those of the
/// tested 8 x 8 tile (2048 units: playable area 1800, deployment 950 from the centre, units 400 from it) scaled to the tile. The elements
/// a game's engine lacks are left out (Rome 2's: the alliances' indices, the settlement upgrades).
pub fn battle_xml(b: &Battle) -> String {
    let s = b.army;
    let half = (f64::from(b.cells.0.max(1)) * CELL_UNITS / 2.0, f64::from(b.cells.1.max(1)) * CELL_UNITS / 2.0);
    let (dx, dy) = ((half.0 * 0.93).round(), (half.1 * 0.93).round());
    let reach = (half.0.min(half.1) * 0.4).round();
    let playable = (half.0.min(half.1) * 2.0 * 0.88).round();
    let settlement = matches!(b.map, BattleMap::Position { settlement: true, .. });

    let boundary = |kind: &str| {
        format!(
            "\t\t\t\t<boundary type='{kind}'>\n\t\t\t\t\t<position x='{x0}' y='{y0}'/>\n\t\t\t\t\t<position x='{x0}' y='{dy}'/>\n\t\t\t\t\t<position x='{dx}' y='{dy}'/>\n\
             \t\t\t\t\t<position x='{dx}' y='{y0}'/>\n\t\t\t\t\t<orientation radians=\"0.00\"/>\n\t\t\t\t</boundary>\n",
            x0 = -dx, y0 = -dy
        )
    };

    let alliance = |id: u32, faction: &str, unit: &str, name: &str, y: f64, attacker: bool| {
        let mut a = format!("\t<alliance id=\"{id}\">\n\t\t<army>\n\t\t\t<faction>{}</faction>\n", esc(faction));
        if b.facts.alliance_indices {
            a += &format!("\t\t\t<deployment_area_alliance_index>{}</deployment_area_alliance_index>\n", 1 - id);
        }

        a += "\t\t\t<deployment_area>\n";
        a += &boundary("standard additive");
        a += &boundary("guerrilla exclusion additive");
        if attacker {
            a += &boundary("guerrilla exclusion subtractive");
        }

        a += "\t\t\t</deployment_area>\n";
        a += &format!(
            "\t\t\t<unit script_name=\"{name}\">\n\t\t\t\t<unit_type type=\"{}\"/>\n\t\t\t\t<position x=\"0\" y=\"{y}\"/>\n\t\t\t\t<orientation radians=\"0\"/>\n\
             \t\t\t\t<width metres=\"20\"/>\n\t\t\t\t<unit_experience level=\"0\"/>\n\t\t\t</unit>\n\t\t</army>\n",
            esc(unit)
        );
        a += "\t\t<victory_condition>\n\t\t\t<kill_or_rout_enemy></kill_or_rout_enemy>\n\t\t</victory_condition>\n";
        if attacker && settlement {
            a += "\t\t<victory_condition>\n\t\t\t<capture_settlement></capture_settlement>\n\t\t</victory_condition>\n";
        }

        if !attacker {
            a += "\t\t<starting_tickets>100</starting_tickets>\n";
        }

        a += "\t\t<rout_position x=\"0.00\" y=\"0.00\"/>\n";
        if attacker {
            a += "\t\t<deploys_first></deploys_first>\n";
        }

        a + "\t</alliance>\n"
    };

    let w = &b.weather;
    let mut x = String::from("<?xml version=\"1.0\"?>\n<!-- Warscape Community Editor: Launch in game -->\n<battle>\n");
    x += &alliance(0, &s.defender_faction, &s.defender_unit, "wce_defender", -reach, false);
    x += &alliance(1, &s.attacker_faction, &s.attacker_unit, "wce_attacker", reach, true);
    x += &format!(
        "\t<battle_description>\n\t\t<time_of_day>{}</time_of_day>\n\t\t<season>{}</season>\n\t\t<precipitation_type>{}</precipitation_type>\n\
         \t\t<type>{}</type>\n\t\t<duration>3600</duration>\n\t\t<timeout_winning_alliance_index>0</timeout_winning_alliance_index>\n",
        w.time_of_day, w.season.0, w.precipitation, if settlement { "settlement_standard" } else { "land_normal" }
    );
    if b.facts.alliance_indices {
        x += "\t\t<battlefield_owner_alliance_index>0</battlefield_owner_alliance_index>\n";
    }

    if !s.subculture.is_empty() {
        x += &format!("\t\t<subculture>{}</subculture>\n", esc(&s.subculture));
    }

    x += "\t</battle_description>\n";
    if !b.sea_surface.is_empty() {
        x += &format!("\t<sea_surface_name>{}</sea_surface_name>\n", esc(&b.sea_surface));
    }

    if !w.environment.is_empty() {
        x += &format!("\t<weather>\n\t\t<prevailing_wind x=\"0.00\" y=\"9.00\"/>\n\t\t<environment_key>{}</environment_key>\n\t</weather>\n", esc(&w.environment));
    }

    if !w.climate.is_empty() {
        x += &format!("\t<Climate>\n\t\t<name>{}</name>\n\t</Climate>\n", esc(&w.climate));
    }

    x += &format!("\t<Season>\n\t\t<name>{}</name>\n\t</Season>\n\t<skip_deployment></skip_deployment>\n\t<battle_map_definition>\n", w.season.1);
    match &b.map {
        BattleMap::Tile(folder) => x += &format!("\t\t<name>{}</name>\n", esc(folder)),
        BattleMap::Position { map, x: px, y: py, .. } => {
            x += &format!("\t\t<name>{}</name>\n\t\t<tile_map_position x=\"{px:.6}\" y=\"{py:.6}\">/</tile_map_position>\n", esc(map));
            if settlement && b.facts.tile_upgrades {
                for u in s.tile_upgrades.split_whitespace() {
                    x += &format!("\t\t<tile_upgrade>{}</tile_upgrade>\n", esc(u));
                }
            }
        }
    }

    x + &format!("\t</battle_map_definition>\n\t<playable_area dimension=\"{playable}\"></playable_area>\n</battle>\n")
}

/// How the tile reaches the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Delivery {
    /// A working directory of loose files (its path).
    Loose(PathBuf),

    /// The launch pack in `data\`.
    Pack,
}

/// The game's command line: the launch pack first (first listed wins), `mods` (file names in `data\`), the working directory of the
/// loose files, no intro movies, and the battle.
pub fn command_line(delivery: &Delivery, mods: &[String]) -> String {
    let mut c = Vec::new();
    if *delivery == Delivery::Pack {
        c.push(format!("mod \"{LAUNCH_PACK}\";"));
    }

    for m in mods.iter().filter(|m| !m.eq_ignore_ascii_case(LAUNCH_PACK)) {
        c.push(format!("mod \"{m}\";"));
    }

    if let Delivery::Loose(dir) = delivery {
        // As the launcher writes its own: forward slashes, a trailing one.
        c.push(format!("add_working_directory \"{}/\";", dir.to_string_lossy().replace('\\', "/").trim_end_matches('/')));
    }

    c.push(format!("show_frontend_movies false; game_startup_mode battle {BATTLE_XML};"));
    c.join(" ")
}

/// What a launch's loose files need from the mod packs ([`needs`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Needs {
    /// The mod packs that serve a dependency, highest priority first, with one dependency each serves (why it loads).
    pub mods: Vec<(String, String)>,

    /// A pack that loads (the release's, a needed mod) and also has a path the launch ships, as (pack, path): its file wins over the loose
    /// one.
    pub shadowing: Vec<(String, String)>,

    /// Dependencies read from the editor's loose folders (its own edits, the output folder): the launch carries them.
    pub overlay: Vec<String>,

    /// The files followed (asset paths in the shipped files, in the models they name, and so on).
    pub followed: usize,
}

/// The extensions of files whose bytes are searched for more asset paths (models, technical files, definitions); textures are not.
const FOLLOW: [&str; 7] = [".rigid_model_v2", ".cs2.parsed", ".variantmeshdefinition", ".xml", ".wsmodel", ".bmd", ".environment"];

/// Asset paths in a file's bytes: runs of path characters (UTF-8, or UTF-16 LE) with a `/` or `\` and an extension, normalized.
pub fn asset_paths(bytes: &[u8]) -> BTreeSet<String> {
    let path_char = |c: u8| c.is_ascii_alphanumeric() || b"_-./\\ ".contains(&c);
    let mut found = BTreeSet::new();
    let mut take = |run: &[u8]| {
        let s = String::from_utf8_lossy(run);
        let s = s.trim().trim_start_matches(['/', '\\']);
        if s.len() >= 6 && (s.contains('/') || s.contains('\\')) && s.rsplit(['/', '\\']).next().is_some_and(|name| name.contains('.')) {
            found.insert(normalize(s));
        }
    };

    for run in bytes.split(|&c| !path_char(c)) {
        take(run);
    }

    // UTF-16 LE: every other byte zero.
    for start in 0..2 {
        let narrow: Vec<u8> = bytes[start..].chunks(2).map(|c| if c.len() == 2 && c[1] == 0 && path_char(c[0]) { c[0] } else { 0 }).collect();
        for run in narrow.split(|&c| c == 0) {
            take(run);
        }
    }

    found
}

/// The mod packs a launch's loose files need, read from the editor's mounted files (`vfs`: the release, the mod packs chosen in the
/// editor and its loose folders): the files of the buildings (`rigidmodels/buildings/<key>/`), every asset path in the shipped files and,
/// in turn, in the models, technical files and definitions those name (`prefixes`: whole folders more, like the climate's ground
/// textures), the pack each is read from; and the mod packs whose database tables (`tables`, like `models_building_tables`) hold one of
/// `keys` (the buildings', the armies'). `shipped`: the paths the launch ships, for [`Needs::shadowing`].
pub fn needs(vfs: &Vfs, shipped: &[(String, Vec<u8>)], buildings: &[String], prefixes: &[String], tables: &[&str], keys: &[String]) -> Needs {
    let packs = vfs.packs();
    let mut why: BTreeMap<usize, String> = BTreeMap::new();
    let mut out = Needs::default();
    let shipped_paths: HashSet<String> = shipped.iter().map(|(p, _)| normalize(p)).collect();

    // The dependencies, followed breadth first.
    let mut queue: Vec<String> = vec![];
    for key in buildings {
        let _a = wce_foundation::activity::start(format!("Dependencies: the files of building {key}"));
        queue.extend(vfs.list(&format!("rigidmodels/buildings/{key}/"), "").into_iter().map(str::to_owned));
    }

    for prefix in prefixes {
        queue.extend(vfs.list(prefix, "").into_iter().map(str::to_owned));
    }

    {
        let _a = wce_foundation::activity::start(format!("Dependencies: asset paths in the {} shipped files", shipped.len()));
        for (_, bytes) in shipped {
            queue.extend(asset_paths(bytes));
        }
    }

    let mut seen = HashSet::new();
    let followed = wce_foundation::activity::start("Dependencies: following models and definitions to their textures");
    while let Some(path) = queue.pop() {
        if shipped_paths.contains(&path) || !seen.insert(path.clone()) {
            continue;
        }

        let Some(owner) = vfs.owner(&path) else { continue };
        let pack = &packs[owner];
        if pack.loose {
            out.overlay.push(path.clone());
        } else if pack.is_mod {
            why.entry(owner).or_insert_with(|| path.clone());
        }

        if FOLLOW.iter().any(|x| path.ends_with(x))
            && let Ok(bytes) = vfs.read(&path)
        {
            out.followed += 1;
            queue.extend(asset_paths(&bytes));
        }
    }

    drop(followed);

    // The database rows: a mod pack's table files that hold a key the release's tables lack (a mod's own building or unit; a key of the
    // release is found in a mod's copy of a whole table too, which it does not need), as its strings are stored: UTF-8 or UTF-16 LE.
    let in_tables = |i: usize, needle: &(String, Vec<u8>, Vec<u8>)| {
        tables.iter().flat_map(|t| vfs.list(&format!("db/{t}/"), "")).filter(|p| vfs.owners(p).contains(&i)).any(|p| {
            vfs.read_from(i, p).is_ok_and(|b| contains(&b, &needle.1) || contains(&b, &needle.2))
        })
    };

    let release: Vec<usize> = packs.iter().enumerate().filter(|(_, p)| !p.is_mod && !p.loose).map(|(i, _)| i).collect();
    let needles: Vec<(String, Vec<u8>, Vec<u8>)> = {
        let _a = wce_foundation::activity::start("Dependencies: the armies' and buildings' keys in the game's tables");
        keys.iter()
            .filter(|k| !k.is_empty())
            .map(|k| (k.clone(), k.as_bytes().to_vec(), k.encode_utf16().flat_map(u16::to_le_bytes).collect()))
            .filter(|n| !release.iter().any(|&i| in_tables(i, n)))
            .collect()
    };

    for (i, pack) in packs.iter().enumerate().filter(|(_, p)| p.is_mod && !p.loose && !needles.is_empty()) {
        let _a = wce_foundation::activity::start(format!("Dependencies: the tables of {}", pack.name));
        'tables: for table in tables {
            for path in vfs.list(&format!("db/{table}/"), "") {
                if !vfs.owners(path).contains(&i) {
                    continue;
                }

                let Ok(bytes) = vfs.read_from(i, path) else { continue };
                for (key, utf8, utf16) in &needles {
                    if contains(&bytes, utf8) || contains(&bytes, utf16) {
                        why.entry(i).or_insert_with(|| format!("{path}: {key}"));
                        break 'tables;
                    }
                }
            }
        }
    }

    out.mods = why.iter().map(|(&i, path)| (packs[i].name.clone(), path.clone())).collect();

    // The packs that load (the release's, the needed mods) win over loose files: one with a shipped path hides it (the release's
    // tiles.pack has the tile database, so a tile's own database never reaches the game loose: a crash on 2026-10-28).
    for (i, pack) in packs.iter().enumerate().filter(|(i, p)| !p.loose && (!p.is_mod || why.contains_key(i))) {
        if let Some(path) = shipped_paths.iter().filter(|p| vfs.owners(p).contains(&i)).min() {
            out.shadowing.push((pack.name.clone(), path.clone()));
        }
    }

    out.overlay.sort();
    out
}

/// A pack of the game's release (always loaded) that has a path the launch ships, as (pack, path): its file wins over the loose one (the
/// release's tiles.pack has the tile database). Found without [`needs`]' scan.
pub fn release_shadowing(vfs: &Vfs, shipped: &[(String, Vec<u8>)]) -> Option<(String, String)> {
    let packs = vfs.packs();
    let mut paths: Vec<String> = shipped.iter().map(|(p, _)| normalize(p)).collect();
    paths.sort();
    paths.iter().find_map(|p| vfs.owners(p).iter().find(|&&i| !packs[i].is_mod && !packs[i].loose).map(|&i| (packs[i].name.clone(), p.clone())))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// A pack's type from its header (`PFH?` then the type in the low bits of the next u32: 3 mod, 4 movie), if it reads.
pub fn pack_type(path: &Path) -> Option<u32> {
    use std::io::Read;
    let mut h = [0u8; 8];
    std::fs::File::open(path).ok()?.read_exact(&mut h).ok()?;
    (&h[..3] == b"PFH").then(|| u32::from_le_bytes([h[4], h[5], h[6], h[7]]) & 0xf)
}

/// The movie-type packs of `data` that are not the game's (`release`) nor the editor's own walls pack: the game loads them whatever the
/// mod list says, and a stale one shadows the mods' files (the GPU driver crashes of docs/re/game_command_line.md).
pub fn stray_movie_packs(data: &Path, release: &[String]) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(data) else { return vec![] };
    let mut v: Vec<String> = dir
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".pack"))
        .filter(|n| !release.iter().any(|r| r.eq_ignore_ascii_case(n)) && !n.eq_ignore_ascii_case(wce_walls::pack::PROC_PACK))
        .filter(|n| pack_type(&data.join(n)) == Some(4))
        .collect();
    v.sort_by_key(|n| n.to_ascii_lowercase());
    v
}

/// Writes the launch pack at `path` for a game (its rpfm key): `files` (path in the packs, bytes), the battle's XML and the marker.
///
/// # Errors
///
/// The pack can't be written.
pub fn write_launch_pack(path: &Path, game: &str, mut files: Vec<(String, Vec<u8>)>, xml: &str) -> Result<(), String> {
    files.retain(|(p, _)| !p.eq_ignore_ascii_case(BATTLE_XML) && !p.eq_ignore_ascii_case(MARKER));
    files.push((BATTLE_XML.to_owned(), xml.as_bytes().to_vec()));
    files.push((MARKER.to_owned(), MARKER_TEXT.to_vec()));
    wce_walls::pack::write_pack(path, game, files, vec![])
}

/// Removes the launch pack of `data` (a game's data folder; `game` its rpfm key), if there is one and it is the editor's (it has the
/// marker). Returns what was done, `None` when there is no pack.
///
/// # Errors
///
/// A pack of that name without the marker (it is left alone), or it can't be removed (the game has it open).
pub fn remove_launch_pack(data: &Path, game: &str) -> Result<Option<String>, String> {
    let path = data.join(LAUNCH_PACK);
    if !path.is_file() {
        return Ok(None);
    }

    let files = wce_walls::pack::read_pack_files(&path, game).map_err(|e| format!("{} left in place: {e}", path.display()))?;
    if !files.iter().any(|(p, _)| p.eq_ignore_ascii_case(MARKER)) {
        return Err(format!("{} is not the editor's launch pack: left in place", path.display()));
    }

    std::fs::remove_file(&path).map_err(|e| format!("{} could not be removed: {e}", path.display()))?;
    Ok(Some(format!("removed {}", path.display())))
}

/// Whether a process of that executable runs (`None`: unknown, outside Windows or when `tasklist` fails).
pub fn process_running(exe: &str) -> Option<bool> {
    processes_running(&[exe]).map(|r| r[0])
}

/// [`process_running`] for several executables from one process list (`tasklist` takes about 0.2 s).
pub fn processes_running(exes: &[&str]) -> Option<Vec<bool>> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("tasklist").args(["/NH", "/FO", "CSV"]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
        let list = String::from_utf8_lossy(&out.stdout).to_lowercase();
        Some(exes.iter().map(|exe| list.contains(&format!("\"{}\"", exe.to_lowercase()))).collect())
    }

    #[cfg(not(windows))]
    {
        let _ = exes;
        None
    }
}

/// Starts the game with the command line, from its folder.
///
/// # Errors
///
/// Not on Windows, or the game can't be started.
pub fn start(game_dir: &Path, exe: &str, command_line: &str) -> Result<Child, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let path = game_dir.join(exe);
        // As one raw string: the game parses `name "arg";` itself, and Rust's quoting would escape the quotes.
        std::process::Command::new(&path).current_dir(game_dir).raw_arg(command_line).spawn().map_err(|e| format!("{}: {e}", path.display()))
    }

    #[cfg(not(windows))]
    {
        let _ = (game_dir, exe, command_line);
        Err("Launch in game runs the game on Windows only".to_owned())
    }
}

/// A launched game: waited for, then its launch pack and staging folder removed (the game's own, whatever game the editor is on then).
pub struct Running {
    pub child: Child,

    /// The game's rpfm key and data folder.
    pub game: &'static str,
    pub data: PathBuf,
    pub staging: PathBuf,
}

impl Running {
    /// What happened when the game has exited (the pack and the staging folder removed), `None` while it runs.
    pub fn poll(&mut self) -> Option<String> {
        let status = match self.child.try_wait() {
            Ok(None) => return None,
            Ok(Some(s)) => format!("the game exited ({s})"),
            Err(e) => format!("the game could not be waited for ({e})"),
        };

        Some(format!("{status}; {}", self.clean_up()))
    }

    /// Removes the launch pack and the staging folder: what was done.
    pub fn clean_up(&self) -> String {
        let pack = match remove_launch_pack(&self.data, self.game) {
            Ok(Some(m)) => m,
            Ok(None) => "no launch pack to remove".to_owned(),
            Err(e) => e,
        };

        let _ = std::fs::remove_dir_all(&self.staging);
        pack
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn weather() -> BattleWeather {
        let w = TileWeather {
            file: "weather/rome_atlantic/au/land/day/dry/demo_morning.environment".to_owned(),
            query: BTreeMap::from([("season".to_owned(), "au".to_owned()), ("climate".to_owned(), "ROME_Atlantic".to_owned()), ("precipitation".to_owned(), "dry,rain".to_owned())]),
        };
        BattleWeather::from_tile(&w)
    }

    fn attila_army() -> LaunchArmy {
        LaunchArmy::default().resolved(&wce_vfs::GameProfile::ATTILA.launch)
    }

    #[test]
    fn coastal_sets() {
        for set in ["sea", "sea_coast", "sea_coast_landing", "settlement_western_ports", "river_mouth_shallow"] {
            assert!(coastal_set(set), "{set}");
        }

        for set in ["river", "river_crossing", "settlement_western_cities", "plains", "lakes"] {
            assert!(!coastal_set(set), "{set}");
        }
    }

    #[test]
    fn tile_battle_as_tested() {
        let s = attila_army();
        let facts = wce_vfs::GameProfile::ATTILA.launch;
        let x = battle_xml(&Battle { map: BattleMap::Tile("terrain/tiles/battle/plains/8x8/8x8_aa".to_owned()), cells: (8, 8), weather: weather(), sea_surface: String::new(), army: &s, facts: &facts });
        assert!(x.contains("<name>terrain/tiles/battle/plains/8x8/8x8_aa</name>"));
        assert!(!x.contains("tile_map_position"));
        assert!(x.contains("<type>land_normal</type>"));
        assert!(x.contains("<playable_area dimension=\"1802\">"));
        assert!(x.contains("<position x='-952' y='-952'/>"));
        assert!(x.contains("<position x=\"0\" y=\"-410\"/>") && x.contains("<position x=\"0\" y=\"410\"/>"));
        assert!(x.contains("<season>Autumn</season>") && x.contains("<name>season_autumn</name>"));
        assert!(x.contains("<precipitation_type>dry</precipitation_type>"), "a filter of several values is 'any'");
        assert!(x.contains("<Climate>\n\t\t<name>ROME_Atlantic</name>"));
        assert!(x.contains("<faction>att_fact_western_roman_empire</faction>") && x.contains("<unit_type type=\"att_nor_saxon_spears\"/>"));
        assert!(x.contains("<deployment_area_alliance_index>1</deployment_area_alliance_index>") && x.contains("<battlefield_owner_alliance_index>"));
        assert_eq!(x.matches("<alliance id=").count(), 2);
        assert!(!x.contains("sea_surface_name"), "no sea surface given: none written");
    }

    #[test]
    fn rome2_battle_has_its_keys_and_not_attilas_elements() {
        let game = &wce_vfs::GameProfile::ROME_2;
        let s = LaunchArmy { tile_upgrades: "level1".to_owned(), ..LaunchArmy::default() }.resolved(&game.launch);
        let map = BattleMap::Position { map: format!("{}/", game.campaign_battle_map), x: 0.5, y: 0.5, settlement: true };
        let x = battle_xml(&Battle { map, cells: (8, 8), weather: weather(), sea_surface: String::new(), army: &s, facts: &game.launch });
        assert!(x.contains("<faction>rom_rome</faction>") && x.contains("<unit_type type=\"Rom_Hastati\"/>"));
        assert!(x.contains("<faction>rom_suebi</faction>") && x.contains("<unit_type type=\"Ger_Spear_Levy\"/>"));
        assert!(x.contains("<subculture>sc_rom_roman</subculture>") && x.contains("<name>terrain/battles/main_rome_map/</name>"));
        assert!(!x.contains("deployment_area_alliance_index") && !x.contains("battlefield_owner_alliance_index") && !x.contains("tile_upgrade"));
    }

    #[test]
    fn settlement_position() {
        let s = LaunchArmy { tile_upgrades: "level1 escalation3".to_owned(), ..attila_army() };
        let map = BattleMap::Position { map: "terrain/battles/main_attila_map/".to_owned(), x: 310.0 / 2032.0, y: 324.0 / 1437.0, settlement: true };
        let x = battle_xml(&Battle { map, cells: (8, 8), weather: weather(), sea_surface: "wind_level_2".to_owned(), army: &s, facts: &wce_vfs::GameProfile::ATTILA.launch });
        assert!(x.contains("</battle_description>
	<sea_surface_name>wind_level_2</sea_surface_name>
	<weather>"));
        assert!(x.contains("<tile_map_position x=\"0.152559\" y=\"0.225470\">/</tile_map_position>"));
        assert!(x.contains("<type>settlement_standard</type>") && x.contains("<capture_settlement>"));
        assert!(x.contains("<tile_upgrade>level1</tile_upgrade>\n\t\t<tile_upgrade>escalation3</tile_upgrade>"));
    }

    #[test]
    fn command_line_puts_the_launch_pack_first() {
        let c = command_line(&Delivery::Pack, &["tdd_pack1_main_v1.1.0.pack".to_owned(), "wce_launch.pack".to_owned()]);
        assert_eq!(c, "mod \"wce_launch.pack\"; mod \"tdd_pack1_main_v1.1.0.pack\"; show_frontend_movies false; game_startup_mode battle script/wce/wce_launch.xml;");
    }

    #[test]
    fn command_line_of_loose_files() {
        let c = command_line(&Delivery::Loose(PathBuf::from(r"C:\out\launch\a1\game\")), &["tdd_pack4_models_v1.1.0.pack".to_owned()]);
        assert_eq!(
            c,
            "mod \"tdd_pack4_models_v1.1.0.pack\"; add_working_directory \"C:/out/launch/a1/game/\"; show_frontend_movies false; game_startup_mode battle script/wce/wce_launch.xml;"
        );
    }

    #[test]
    fn asset_paths_in_bytes() {
        let mut b = b"\x05\x00rigidmodels/buildings/rock_flat/rock_flat_piece01.rigid_model_v2\x00junk/noext\x00".to_vec();
        // A UTF-16 string as CA's formats store it: its length first.
        b.extend([22, 0]);
        b.extend("Terrain\\Textures\\a.dds".encode_utf16().flat_map(u16::to_le_bytes));
        let found = asset_paths(&b);
        assert!(found.contains("rigidmodels/buildings/rock_flat/rock_flat_piece01.rigid_model_v2"));
        assert!(found.contains("terrain/textures/a.dds"));
        assert!(!found.iter().any(|p| p.contains("noext")));
    }

    #[test]
    fn launch_pack_round_trip_and_marker() {
        let dir = std::env::temp_dir().join(format!("wce_launch_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join(LAUNCH_PACK);
        write_launch_pack(&pack, "attila", vec![("terrain/tiles/battle/x/y/z/tile.data".to_owned(), vec![1, 2, 3])], "<battle/>").unwrap();
        assert_eq!(pack_type(&pack), Some(3));
        assert!(stray_movie_packs(&dir, &[]).is_empty());
        assert!(remove_launch_pack(&dir, "attila").unwrap().is_some());
        assert!(!pack.exists());

        // A pack of that name without the marker is left alone.
        wce_walls::pack::write_pack(&pack, "attila", vec![("a/b.txt".to_owned(), vec![0])], vec![]).unwrap();
        assert!(remove_launch_pack(&dir, "attila").is_err());
        assert!(pack.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
