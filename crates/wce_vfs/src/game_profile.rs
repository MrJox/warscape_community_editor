//! Everything that is specific to one Total War game, in one place, so that Rome 2 or Thrones of Britannia could be added later.

use std::path::{Path, PathBuf};

/// The facts about a game the editor needs.
#[derive(Clone, Debug)]
pub struct GameProfile {
    /// Key of the game in rpfm, like `attila`.
    pub key: &'static str,

    /// Name of the game for people.
    pub display_name: &'static str,

    /// Folder of the packs, under the game's folder.
    pub data_dir: &'static str,

    /// File of the launcher listing the enabled mods, under the game's folder: `mod "<name>.pack";` lines, first listed wins.
    pub used_mods_file: &'static str,

    /// File listing the vanilla packs, under `data_dir`: `<pack>\t<...>` lines.
    pub manifest_file: &'static str,

    /// DLC packs the manifest doesn't list. They're searched before the manifest's packs.
    pub extra_vanilla: &'static [&'static str],
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

impl GameProfile {

    /// Total War: Attila.
    pub const ATTILA: GameProfile = GameProfile {
        key: "attila",
        display_name: "Total War: Attila",
        data_dir: "data",
        used_mods_file: "used_mods.txt",
        manifest_file: "manifest.txt",
        extra_vanilla: &["charlemagne.pack", "belisarius.pack", "blood.pack", "slavs.pack"],
    };

    /// Returns the packs of the game in priority order: the enabled mods in the order the launcher lists them (first listed wins),
    /// then the extra vanilla packs, then the manifest's packs in file order. Repeated packs (ignoring case) keep their first place.
    ///
    /// The engine's exact order between mods and between vanilla packs is not verified.
    ///
    /// # Arguments
    ///
    /// * `game_dir` - Folder of the game.
    /// * `use_mods` - If the enabled mods are included.
    pub fn pack_order(&self, game_dir: &Path, use_mods: bool) -> PackOrder {
        let mods: Vec<String> = if use_mods { read_lines(&game_dir.join(self.used_mods_file)).iter().filter_map(|l| used_mod(l).map(str::to_owned)).collect() } else { vec![] };
        self.pack_order_with(game_dir, &mods)
    }

    /// The packs of the game's release (the extra vanilla packs and the manifest's), without any mod.
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

    /// Like `pack_order`, with the mods named by the caller (first listed wins) instead of the launcher's list.
    pub fn pack_order_with(&self, game_dir: &Path, mods: &[String]) -> PackOrder {
        let data = game_dir.join(self.data_dir);
        let mut order = PackOrder::default();
        let mut add = |name: &str, is_mod: bool| {
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
            add(name, true);
        }

        for name in self.extra_vanilla {
            add(name, false);
        }

        for line in read_lines(&data.join(self.manifest_file)) {
            let name = line.split('\t').next().unwrap_or_default();
            if name.len() > 5 && name.to_ascii_lowercase().ends_with(".pack") && !name.contains('/') {
                add(name, false);
            }
        }

        order
    }
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
