//! TODO(pre-release): remove this module (and its call in `Settings::load`). It only moves the development machines' settings from the old
//! `key=value` wce.ini (in `%LOCALAPPDATA%\awe`, `~/.local/share/awe` or `$WCE_SETTINGS_DIR`) to settings.toml; no release ever wrote wce.ini.

use std::path::{Path, PathBuf};

use super::{Paths, Settings, env_value};

/// The old settings folder of `paths`: a wce.ini next to where settings.toml goes (`$WCE_SETTINGS_DIR`, the scripts' copies), else, for the
/// system's folders only, `%LOCALAPPDATA%\awe` (elsewhere `$XDG_DATA_HOME/awe`, else `~/.local/share/awe`).
fn legacy_dir(paths: &Paths) -> Option<PathBuf> {
    if paths.config_dir().join("wce.ini").is_file() {
        return Some(paths.config_dir().to_path_buf());
    }

    if *paths != Paths::standard() || env_value("WCE_SETTINGS_DIR").is_some() {
        return None;
    }

    let local = env_value("LOCALAPPDATA").map(PathBuf::from)
        .or_else(|| if cfg!(windows) { None } else { env_value("XDG_DATA_HOME").map(PathBuf::from) })
        .or_else(|| if cfg!(windows) { None } else { env_value("HOME").map(|home| Path::new(&home).join(".local").join("share")) })?;
    Some(local.join("awe"))
}

/// The settings of the old wce.ini, if there is one, and its tools' layouts copied to the new layouts folder.
pub(super) fn migrate(paths: &Paths) -> Option<Settings> {
    let dir = legacy_dir(paths)?;
    let text = std::fs::read(dir.join("wce.ini")).ok()?;
    let text = String::from_utf8_lossy(&text);
    let mut s = Settings::default();
    let list = |v: &str| v.split(';').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let mut tile = toml::Table::new();
    let mut ui = toml::Table::new();
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !l.starts_with('#')) {
        let Some((key, value)) = line.split_once('=') else { continue };
        if value.is_empty() {
            continue;
        }

        let flag = value != "0";
        match key {
            "game_dir" => s.game_mut().dir = Some(PathBuf::from(value)),
            "mod_packs" => s.game_mut().mod_packs = list(value),
            "overlay_folders" => s.game_mut().overlay_folders = list(value).into_iter().map(PathBuf::from).collect(),
            "ui_project" => s.game_mut().project_dir = Some(PathBuf::from(value.trim())),
            "output_dir" => s.output_dir = Some(PathBuf::from(value)),
            "frame_cap" => s.frame_cap = value.trim().parse().unwrap_or(s.frame_cap),
            "tile_kits" => {
                tile.insert("kits".to_owned(), list(value).into());
            }
            "tile_show_sea" => {
                tile.insert("show_sea".to_owned(), flag.into());
            }
            "tile_show_vfx_boxes" => {
                tile.insert("show_vfx_boxes".to_owned(), flag.into());
            }
            "tile_sea_state" => {
                tile.insert("sea_state".to_owned(), value.into());
            }
            "ui_export_pack" => {
                ui.insert("export_pack".to_owned(), value.into());
            }
            "ui_mod_pack" => {
                ui.insert("mod_pack".to_owned(), value.into());
            }
            _ => {}
        }
    }

    for (id, table) in [("tile", tile), ("ui", ui)] {
        if !table.is_empty() {
            s.tools.insert(id.to_owned(), table);
        }
    }

    // The layouts are copied, never moved: the old folder stays as it was.
    if let Ok(entries) = std::fs::read_dir(dir.join("layouts")) {
        let to = paths.layouts_dir();
        let _ = std::fs::create_dir_all(&to);
        for entry in entries.flatten() {
            let target = to.join(entry.file_name());
            if !target.exists() {
                let _ = std::fs::copy(entry.path(), target);
            }
        }
    }

    Some(s)
}
