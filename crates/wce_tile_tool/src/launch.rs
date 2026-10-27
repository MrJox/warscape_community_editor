//! Launch in game (docs/plans/launch_in_game.md; the command line: docs/re/game_command_line.md): the open tile, exported, in a battle the
//! game starts straight into. What the game is given: a mod pack `data\wce_launch.pack` (a mod pack's files win over loose ones, and the
//! game takes mod packs from `data\` only) with the tile's exported files, the tile database and the battle's XML, and a command line
//! naming it first (first listed wins), then the mod packs chosen in the editor, then `game_startup_mode battle <xml>;`. The launcher's
//! `used_mods.txt` is never touched. The pack is removed when the game exits, or at the next launch or start of the editor; it carries a
//! marker, so a pack of that name the editor did not write is never removed.

use std::path::{Path, PathBuf};
use std::process::Child;

use crate::settings::{LaunchSettings, TileWeather};

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

/// What the battle XML is made from.
pub struct Battle<'a> {
    pub map: BattleMap,

    /// The tile's size in cells.
    pub cells: (u32, u32),
    pub weather: BattleWeather,
    pub settings: &'a LaunchSettings,
}

/// Text for an XML attribute or element.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The battle's XML (the benchmarks' format, as `benchmarks/londinium.benchmark` points at; docs/re/game_command_line.md): one unit a side
/// across the tile's centre, both deployment areas over the tile, no deployment phase, an hour long, no script. The sizes are those of the
/// tested 8 x 8 tile (2048 units: playable area 1800, deployment 950 from the centre, units 400 from it) scaled to the tile.
pub fn battle_xml(b: &Battle) -> String {
    let s = b.settings;
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
        a += &format!("\t\t\t<deployment_area_alliance_index>{}</deployment_area_alliance_index>\n\t\t\t<deployment_area>\n", 1 - id);
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
         \t\t<type>{}</type>\n\t\t<duration>3600</duration>\n\t\t<timeout_winning_alliance_index>0</timeout_winning_alliance_index>\n\
         \t\t<battlefield_owner_alliance_index>0</battlefield_owner_alliance_index>\n",
        w.time_of_day, w.season.0, w.precipitation, if settlement { "settlement_standard" } else { "land_normal" }
    );
    if !s.subculture.is_empty() {
        x += &format!("\t\t<subculture>{}</subculture>\n", esc(&s.subculture));
    }

    x += "\t</battle_description>\n";
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
            if settlement {
                for u in s.tile_upgrades.split_whitespace() {
                    x += &format!("\t\t<tile_upgrade>{}</tile_upgrade>\n", esc(u));
                }
            }
        }
    }

    x + &format!("\t</battle_map_definition>\n\t<playable_area dimension=\"{playable}\"></playable_area>\n</battle>\n")
}

/// The game's command line: the launch pack first (first listed wins), then `mods` (the editor's mod packs, file names in `data\`), no
/// intro movies, and the battle.
pub fn command_line(mods: &[String]) -> String {
    let mut c = format!("mod \"{LAUNCH_PACK}\";");
    for m in mods.iter().filter(|m| !m.eq_ignore_ascii_case(LAUNCH_PACK)) {
        c += &format!(" mod \"{m}\";");
    }

    c + &format!(" show_frontend_movies false; game_startup_mode battle {BATTLE_XML};")
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

/// Writes the launch pack at `path`: `files` (path in the packs, bytes), the battle's XML and the marker.
///
/// # Errors
///
/// The pack can't be written.
pub fn write_launch_pack(path: &Path, mut files: Vec<(String, Vec<u8>)>, xml: &str) -> Result<(), String> {
    files.retain(|(p, _)| !p.eq_ignore_ascii_case(BATTLE_XML) && !p.eq_ignore_ascii_case(MARKER));
    files.push((BATTLE_XML.to_owned(), xml.as_bytes().to_vec()));
    files.push((MARKER.to_owned(), MARKER_TEXT.to_vec()));
    wce_walls::pack::write_pack(path, "attila", files, vec![])
}

/// Removes the launch pack of `data`, if there is one and it is the editor's (it has the marker). Returns what was done, `None` when there
/// is no pack.
///
/// # Errors
///
/// A pack of that name without the marker (it is left alone), or it can't be removed (the game has it open).
pub fn remove_launch_pack(data: &Path) -> Result<Option<String>, String> {
    let path = data.join(LAUNCH_PACK);
    if !path.is_file() {
        return Ok(None);
    }

    let files = wce_walls::pack::read_pack_files(&path, "attila").map_err(|e| format!("{} left in place: {e}", path.display()))?;
    if !files.iter().any(|(p, _)| p.eq_ignore_ascii_case(MARKER)) {
        return Err(format!("{} is not the editor's launch pack: left in place", path.display()));
    }

    std::fs::remove_file(&path).map_err(|e| format!("{} could not be removed: {e}", path.display()))?;
    Ok(Some(format!("removed {}", path.display())))
}

/// Whether a process of that executable runs (`None`: unknown, outside Windows or when `tasklist` fails).
pub fn process_running(exe: &str) -> Option<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {exe}"), "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).to_lowercase().contains(&format!("\"{}\"", exe.to_lowercase())))
    }

    #[cfg(not(windows))]
    {
        let _ = exe;
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

/// A launched game: waited for, then its launch pack and staging folder removed.
pub struct Running {
    pub child: Child,
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
        let pack = match remove_launch_pack(&self.data) {
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

    #[test]
    fn tile_battle_as_tested() {
        let s = LaunchSettings::default();
        let x = battle_xml(&Battle { map: BattleMap::Tile("terrain/tiles/battle/plains/8x8/8x8_aa".to_owned()), cells: (8, 8), weather: weather(), settings: &s });
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
        assert_eq!(x.matches("<alliance id=").count(), 2);
    }

    #[test]
    fn settlement_position() {
        let s = LaunchSettings { tile_upgrades: "level1 escalation3".to_owned(), ..LaunchSettings::default() };
        let map = BattleMap::Position { map: "terrain/battles/main_attila_map/".to_owned(), x: 310.0 / 2032.0, y: 324.0 / 1437.0, settlement: true };
        let x = battle_xml(&Battle { map, cells: (8, 8), weather: weather(), settings: &s });
        assert!(x.contains("<tile_map_position x=\"0.152559\" y=\"0.225470\">/</tile_map_position>"));
        assert!(x.contains("<type>settlement_standard</type>") && x.contains("<capture_settlement>"));
        assert!(x.contains("<tile_upgrade>level1</tile_upgrade>\n\t\t<tile_upgrade>escalation3</tile_upgrade>"));
    }

    #[test]
    fn command_line_puts_the_launch_pack_first() {
        let c = command_line(&["tdd_pack1_main_v1.1.0.pack".to_owned(), "wce_launch.pack".to_owned()]);
        assert_eq!(c, "mod \"wce_launch.pack\"; mod \"tdd_pack1_main_v1.1.0.pack\"; show_frontend_movies false; game_startup_mode battle script/wce/wce_launch.xml;");
    }

    #[test]
    fn launch_pack_round_trip_and_marker() {
        let dir = std::env::temp_dir().join(format!("wce_launch_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join(LAUNCH_PACK);
        write_launch_pack(&pack, vec![("terrain/tiles/battle/x/y/z/tile.data".to_owned(), vec![1, 2, 3])], "<battle/>").unwrap();
        assert_eq!(pack_type(&pack), Some(3));
        assert!(stray_movie_packs(&dir, &[]).is_empty());
        assert!(remove_launch_pack(&dir).unwrap().is_some());
        assert!(!pack.exists());

        // A pack of that name without the marker is left alone.
        wce_walls::pack::write_pack(&pack, "attila", vec![("a/b.txt".to_owned(), vec![0])], vec![]).unwrap();
        assert!(remove_launch_pack(&dir).is_err());
        assert!(pack.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
