//! First-run helpers: find an installed game through Steam; file and folder dialogs (zenity or kdialog on Linux, the Windows
//! dialogs through PowerShell on Windows). The C++ editor also looked for an RPFM folder: the packs are read in-process now.

use std::path::{Path, PathBuf};
use std::process::Command;

use wce_vfs::GameProfile;

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The Steam install folders that exist: the native client, its ~/.steam link and the Flatpak one (Windows: the usual install folder).
pub fn steam_roots() -> Vec<PathBuf> {
    let candidates = if cfg!(windows) {
        let pf = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Program Files (x86)"));
        vec![pf.join("Steam")]
    } else {
        let Some(home) = home_dir() else { return vec![] };
        vec![home.join(".local/share/Steam"), home.join(".steam/steam"), home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")]
    };

    let mut out = vec![];
    for p in candidates {
        if !p.join("steamapps").is_dir() {
            continue;
        }

        if let Ok(canon) = p.canonicalize()
            && !out.contains(&canon)
        {
            out.push(canon);
        }
    }

    out
}

/// Steam library folders: the Steam folders and the paths of their steamapps/libraryfolders.vdf.
pub fn steam_library_folders() -> Vec<PathBuf> {
    let mut out = vec![];
    for root in steam_roots() {
        if !out.contains(&root) {
            out.push(root.clone());
        }

        let text = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")).unwrap_or_default();

        // "path"   "<folder>" (vdf doubles its backslashes).
        let mut rest = text.as_str();
        while let Some(at) = rest.find("\"path\"") {
            rest = &rest[at + 6..];
            let trimmed = rest.trim_start();
            let Some(value) = trimmed.strip_prefix('"') else { continue };
            let Some(end) = value.find('"') else { break };
            let lib = PathBuf::from(value[..end].replace("\\\\", "\\"));
            if !out.contains(&lib) {
                out.push(lib);
            }

            rest = &value[end + 1..];
        }
    }

    out
}

/// The game's folder in a Steam library: <library>/steamapps/common/<the game's folder> with data/data.pack.
pub fn detect_game_folder(game: &GameProfile) -> Option<PathBuf> {
    steam_library_folders().into_iter().map(|lib| lib.join("steamapps").join("common").join(game.steam_folder)).find(|g| g.join(game.data_dir).join("data.pack").exists() && game.folder_problem(g).is_none())
}

/// The program `name` on $PATH.
fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|p| p.is_file())
}

/// Runs a dialog program and returns the path it prints (`None` when it was cancelled or is missing).
fn run_dialog(exe: Option<PathBuf>, args: &[String]) -> Option<PathBuf> {
    let out = Command::new(exe?).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }

    // The first line: GTK may log warnings after it.
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or_default().trim_end();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// An open or save file dialog; `filters` are (name, patterns separated by spaces) pairs. `None` when cancelled.
pub fn file_dialog(save: bool, initial: &Path, filters: &[(String, String)]) -> Option<PathBuf> {
    if cfg!(windows) {
        let kind = if save { "SaveFileDialog" } else { "OpenFileDialog" };
        let filter = filters.iter().map(|(n, p)| format!("{n}|{}", p.replace(' ', ";"))).collect::<Vec<_>>().join("|");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.{kind}; $d.Filter = '{filter}'; $d.FileName = '{}'; if ($d.ShowDialog() -eq 'OK') {{ $d.FileName }}",
            initial.display()
        );

        return run_dialog(Some(PathBuf::from("powershell")), &["-NoProfile".to_owned(), "-Command".to_owned(), script]);
    }

    if let Some(zenity) = find_in_path("zenity") {
        let mut args = vec!["--file-selection".to_owned()];
        if save {
            args.extend(["--save".to_owned(), "--confirm-overwrite".to_owned()]);
        }

        if !initial.as_os_str().is_empty() {
            args.push(format!("--filename={}", initial.display()));
        }

        args.extend(filters.iter().map(|(n, p)| format!("--file-filter={n} | {p}")));
        return run_dialog(Some(zenity), &args);
    }

    let kfilter = filters.iter().map(|(n, p)| format!("{p}|{n}")).collect::<Vec<_>>().join("\n");
    let start = if initial.as_os_str().is_empty() { ".".to_owned() } else { initial.display().to_string() };
    run_dialog(find_in_path("kdialog"), &[if save { "--getsavefilename" } else { "--getopenfilename" }.to_owned(), start, kfilter])
}

/// A folder picker. `None` when cancelled.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '{title}'; if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}"
        );

        return run_dialog(Some(PathBuf::from("powershell")), &["-NoProfile".to_owned(), "-Command".to_owned(), script]);
    }

    if let Some(zenity) = find_in_path("zenity") {
        return run_dialog(Some(zenity), &["--file-selection".to_owned(), "--directory".to_owned(), format!("--title={title}")]);
    }

    run_dialog(find_in_path("kdialog"), &["--getexistingdirectory".to_owned(), ".".to_owned(), "--title".to_owned(), title.to_owned()])
}
