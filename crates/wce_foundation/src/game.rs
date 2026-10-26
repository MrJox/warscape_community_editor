//! Everything that is specific to one Total War game, in one place: [`GameProfile`] has a constant per supported game, and the rest
//! of the editor reads the facts it needs from the current game's profile instead of naming a game.

use std::path::{Path, PathBuf};

/// The facts about a game the editor needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameProfile {
    /// Key of the game, in rpfm and in the settings, like `attila`.
    pub key: &'static str,

    /// Name of the game for people.
    pub display_name: &'static str,

    /// Folder of the game under `steamapps/common`.
    pub steam_folder: &'static str,

    /// Steam's app id of the game (the Proton prefix on Linux is `compatdata/<id>`).
    pub steam_app_id: u32,

    /// Folder of the game's user files under `%APPDATA%/The Creative Assembly` (its `scripts/preferences.script.txt`).
    pub appdata_folder: &'static str,

    /// Folder of the packs, under the game's folder.
    pub data_dir: &'static str,

    /// File of the launcher listing the enabled mods, under the game's folder: `mod "<name>.pack";` lines, first listed wins.
    pub used_mods_file: &'static str,

    /// The game's executable, in the game's folder (started with the folder as its working directory; docs/re/game_command_line.md).
    pub exe: &'static str,

    /// Which packs are the game's own.
    pub vanilla: VanillaPacks,

    /// File name of rpfm's schema of the game's db tables.
    pub rpfm_schema: &'static str,

    /// Folder of the campaign's battle map in the packs (`tile_map.index`, `locations.blm`, ...).
    pub campaign_battle_map: &'static str,

    /// The environment the weather tool opens first.
    pub default_environment: &'static str,

    /// What the game and its Assembly Kit have.
    pub features: GameFeatures,
}

/// How the game's own packs are found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VanillaPacks {
    /// The packs of a manifest under the data folder (`<pack>\t<...>` lines), after DLC packs it doesn't list.
    Manifest { file: &'static str, extra: &'static [&'static str] },

    /// A fixed list (the game's manifest is wrong).
    List(&'static [&'static str]),
}

/// What a game and its Assembly Kit have, where the editor has to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameFeatures {
    /// The Assembly Kit has the battle tiles (`raw_data/terrain/tiles`), TEd and the BOB that builds them: tiles are edited as kit
    /// projects and saved for the kit. Without it tiles are read from the packs and saved into mod packs only.
    pub kit_tiles: bool,

    /// The tiles' civilians (`CivilianShelter`, `CivilianDeploymentZone`: Attila's FASTBIN civilian files).
    pub civilians: bool,

    /// The escalation layers (TEd's `Escalation1..3` export types: Attila's escalation BMDs).
    pub escalation: bool,

    /// Lite building zones (`LiteBuildingZone`: the building list's `lite` flag, Attila's version 12).
    pub lite_buildings: bool,
}

/// A pack in the priority order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackRef {
    /// File name of the pack.
    pub name: String,

    /// Path of the pack on disk.
    pub path: PathBuf,

    /// If the pack is a mod.
    pub is_mod: bool,
}

/// The packs of a game, highest priority first, and the enabled mods that aren't on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackOrder {
    pub packs: Vec<PackRef>,
    pub missing_mods: Vec<String>,
}

/// The kind of a pack, from its header (`PFHFileType` in rpfm).
const PACK_TYPE_PATCH: u32 = 2;

impl GameProfile {

    /// Total War: Attila.
    pub const ATTILA: GameProfile = GameProfile {
        key: "attila",
        display_name: "Total War: Attila",
        steam_folder: "Total War Attila",
        steam_app_id: 325_610,
        appdata_folder: "Attila",
        data_dir: "data",
        used_mods_file: "used_mods.txt",
        exe: "Attila.exe",
        vanilla: VanillaPacks::Manifest { file: "manifest.txt", extra: &["charlemagne.pack", "belisarius.pack", "blood.pack", "slavs.pack"] },
        rpfm_schema: "schema_att.ron",
        campaign_battle_map: "terrain/battles/main_attila_map",
        default_environment: "weather/charlemagne/med_sp_land_day_dry.environment",
        features: GameFeatures { kit_tiles: true, civilians: true, escalation: true, lite_buildings: true },
    };

    /// Total War: Rome II. Its manifest lists neither the patch packs (`*_rome2.pack`) nor the right sizes, so the packs are rpfm's list.
    pub const ROME_2: GameProfile = GameProfile {
        key: "rome_2",
        display_name: "Total War: Rome II",
        steam_folder: "Total War Rome II",
        steam_app_id: 214_950,
        appdata_folder: "Rome2",
        data_dir: "data",
        used_mods_file: "used_mods.txt",
        exe: "Rome2.exe",
        vanilla: VanillaPacks::List(&[
            "blood_rome2.pack", "boot.pack", "data.pack", "data_rome2.pack", "divided.pack", "gaul.pack", "greeks.pack", "invasion.pack",
            "local_en_shared_rome2.pack", "local_cz.pack", "local_en.pack", "local_fr.pack", "local_ge.pack", "local_it.pack", "local_pl.pack",
            "local_ru.pack", "local_tr.pack", "local_sp.pack", "models.pack", "models_rome2.pack", "models2.pack", "models2_rome2.pack",
            "models3_rome2.pack", "movies.pack", "movies_rome2.pack", "music.pack", "music_rome2.pack", "music_en_shared_rome2.pack",
            "music_fr.pack", "music_ge.pack", "music_it.pack", "music_ru.pack", "music_sp.pack", "punic.pack", "sound.pack", "sound_rome2.pack",
            "terrain.pack", "terrain_rome2.pack", "terrain2.pack", "terrain2_rome2.pack", "terrain3_rome2.pack", "tiles.pack", "tiles_rome2.pack",
            "tiles2.pack", "tiles2_rome2.pack", "tiles3.pack", "tiles3_rome2.pack", "tiles4.pack", "tiles4_rome2.pack",
        ]),
        rpfm_schema: "schema_rom2.ron",
        campaign_battle_map: "terrain/battles/main_rome_map",
        default_environment: "weather/rome_med/land/day/dry/day_dry_01.environment",
        features: GameFeatures { kit_tiles: false, civilians: false, escalation: false, lite_buildings: false },
    };

    /// Every supported game, the default first.
    pub const ALL: [&'static GameProfile; 2] = [&Self::ATTILA, &Self::ROME_2];

    /// The game with a key, if it's supported.
    pub fn by_key(key: &str) -> Option<&'static GameProfile> {
        Self::ALL.into_iter().find(|game| game.key == key)
    }

    /// The game with a key, or the default one (Attila) for an unknown key.
    pub fn by_key_or_default(key: &str) -> &'static GameProfile {
        Self::by_key(key).unwrap_or(&Self::ATTILA)
    }

    /// The game of `$WCE_GAME_KEY` (a key, like `rome_2`), else Attila: the examples and gates use it to choose the game.
    pub fn from_env() -> &'static GameProfile {
        std::env::var("WCE_GAME_KEY").ok().and_then(|key| Self::by_key(key.trim())).unwrap_or(&Self::ATTILA)
    }

    /// Returns the packs of the game in priority order: the enabled mods in the order the launcher lists them (first listed wins),
    /// then the vanilla packs (see [`Self::pack_order_with`]).
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Folder of the game.
    /// * `use_mods` - If the enabled mods are included.
    pub fn pack_order(&self, game_dir: &Path, use_mods: bool) -> PackOrder {
        let mods: Vec<String> = if use_mods { read_lines(&game_dir.join(self.used_mods_file)).iter().filter_map(|l| used_mod(l).map(str::to_owned)).collect() } else { vec![] };
        self.pack_order_with(game_dir, &mods)
    }

    /// The packs of the game's release (the vanilla packs), without any mod.
    pub fn release_pack_names(&self, game_dir: &Path) -> Vec<String> {
        self.pack_order_with(game_dir, &[]).packs.into_iter().map(|p| p.name).collect()
    }

    /// The `.pack` files of the data folder that are not part of the release: the mods one can choose from, by file name.
    pub fn available_mod_packs(&self, game_dir: &Path) -> Vec<String> {
        let release = self.release_pack_names(game_dir);
        let mut v: Vec<String> = std::fs::read_dir(game_dir.join(self.data_dir))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.retain(|n| n.to_ascii_lowercase().ends_with(".pack") && !release.iter().any(|r| r.eq_ignore_ascii_case(n)));
        v.sort_by_key(|n| n.to_ascii_lowercase());
        v
    }

    /// Like `pack_order`, with the mods named by the caller (first listed wins) instead of the launcher's list. The vanilla packs follow
    /// the mods: the patch packs first (their header says so: they replace files of the release), then the rest; each group in the
    /// order of the manifest (after the DLC packs it doesn't list) or of the profile's list. Repeated packs (ignoring case) keep their
    /// first place.
    ///
    /// The engine's exact order between mods and between vanilla packs of one kind is not verified.
    pub fn pack_order_with(&self, game_dir: &Path, mods: &[String]) -> PackOrder {
        let data = game_dir.join(self.data_dir);
        let mut order = PackOrder::default();
        let add = |name: &str, is_mod: bool, order: &mut PackOrder| {
            let path = data.join(name);
            if !path.exists() {
                if is_mod {
                    order.missing_mods.push(name.to_owned());
                }
                return;
            }

            if !order.packs.iter().any(|pack| pack.name.eq_ignore_ascii_case(name)) {
                order.packs.push(PackRef { name: name.to_owned(), path, is_mod });
            }
        };

        for name in mods {
            add(name, true, &mut order);
        }

        let mods_end = order.packs.len();
        match &self.vanilla {
            VanillaPacks::Manifest { file, extra } => {
                for name in *extra {
                    add(name, false, &mut order);
                }

                for line in read_lines(&data.join(file)) {
                    let name = line.split('\t').next().unwrap_or_default();
                    if name.len() > 5 && name.to_ascii_lowercase().ends_with(".pack") && !name.contains('/') {
                        add(name, false, &mut order);
                    }
                }
            }

            VanillaPacks::List(names) => {
                for name in *names {
                    add(name, false, &mut order);
                }
            }
        }

        // A stable sort: the order inside each group stays.
        order.packs[mods_end..].sort_by_key(|pack| pack_type(&pack.path) != Some(PACK_TYPE_PATCH));
        order
    }
}

/// The type of a pack from its header (the low bits of the u32 after the `PFHx` magic), if the file starts like a pack.
fn pack_type(path: &Path) -> Option<u32> {
    use std::io::Read;

    let mut header = [0u8; 8];
    std::fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    header.starts_with(b"PFH").then(|| u32::from_le_bytes([header[4], header[5], header[6], header[7]]) & 0xF)
}

/// Lines of a text file, or none if it can't be read.
fn read_lines(path: &Path) -> Vec<String> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).lines().map(|line| line.to_owned()).collect())
        .unwrap_or_default()
}

/// The pack of a `mod "<name>";` line of the launcher, if the line is one.
fn used_mod(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("mod")?;
    let rest = rest.trim_start();
    if rest.len() == line.trim_start().len() - 3 {
        return None;
    }

    let rest = rest.strip_prefix('"')?;
    let (name, rest) = rest.split_once('"')?;
    (!name.is_empty() && rest.trim_start().starts_with(';')).then_some(name)
}
