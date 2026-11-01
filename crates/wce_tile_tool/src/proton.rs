//! Launch in game on Linux: the Windows build of a game run through Proton as Steam runs it. A game folder with the game's executable is
//! the Windows build (run through Proton); one without it is a native Linux build, which Launch in game does not start.
//!
//! What Steam does, done here: the game's compatibility tool is Steam's choice for it (`config/config.vdf`'s `CompatToolMapping`, the
//! game's app id, else the default `0`), a folder of `steamapps/common` (Valve's Protons, named by their folder: `proton_experimental` is
//! `Proton - Experimental`) or of `compatibilitytools.d` (its `compatibilitytool.vdf` names it); the settings can name a Proton folder
//! instead. A tool that requires another (`toolmanifest.vdf`'s `require_tool_appid`: the Steam Linux Runtime) runs inside it; each tool's
//! `commandline` (with `%verb%` = `waitforexitandrun`) goes before the next, then the program. The game's prefix is
//! `steamapps/compatdata/<app id>` of the game's library: Steam made it at the game's first start.
//!
//! The game parses its command line itself, and Wine quotes and escapes every `"` of the arguments it is given, so the command line is
//! written into a batch file that the prefix's `cmd` runs: the game gets it as it is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The verb Steam gives the tools for a game: wait for the prefix's last process, then run.
const VERB: &str = "waitforexitandrun";

/// The environment the editor sets for its own DXVK-native (`wce_d3d11::init_platform`), which the game must not inherit: Proton's DXVK
/// reads them too, and with `DXVK_WSI_DRIVER=SDL3` the game crashes while a battle loads (Attila, `Empire.Retail.dll+0x178c1b6`).
pub const EDITOR_ONLY_ENV: [&str; 2] = ["DXVK_WSI_DRIVER", "DXVK_LOG_PATH"];

/// A KeyValues (VDF) value: a string or a block.
#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Block(Vec<(String, Vdf)>),
}

impl Vdf {
    /// Parses a text VDF (`"key" "value"`, `"key" { ... }`, `//` comments). What can't be read ends the block.
    pub fn parse(text: &str) -> Self {
        let tokens = tokens(text);
        let mut at = 0;
        Self::Block(block(&tokens, &mut at))
    }

    /// The value of a key (case aside), in this block.
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Self::Block(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Self::Str(_) => None,
        }
    }

    /// The value at a path of keys.
    pub fn at(&self, path: &[&str]) -> Option<&Vdf> {
        path.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            Self::Block(_) => None,
        }
    }

    pub fn items(&self) -> &[(String, Vdf)] {
        match self {
            Self::Block(items) => items,
            Self::Str(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = vec![];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }

            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(c) => s.push(c),
                            None => {}
                        },
                        c => s.push(c),
                    }
                }

                out.push(Token::Str(s));
            }

            c if c.is_whitespace() => {}
            c => {
                // An unquoted token.
                let mut s = c.to_string();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }

                    s.push(c);
                    chars.next();
                }

                out.push(Token::Str(s));
            }
        }
    }

    out
}

fn block(tokens: &[Token], at: &mut usize) -> Vec<(String, Vdf)> {
    let mut items = vec![];
    while let Some(t) = tokens.get(*at) {
        *at += 1;
        let Token::Str(key) = t else { break };
        match tokens.get(*at) {
            Some(Token::Str(v)) => {
                *at += 1;
                items.push((key.clone(), Vdf::Str(v.clone())));
            }

            Some(Token::Open) => {
                *at += 1;
                items.push((key.clone(), Vdf::Block(block(tokens, at))));
            }

            _ => break,
        }
    }

    items
}

/// How a game folder's build runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Build {
    /// The Windows build (the executable is there): on Linux, through Proton.
    Windows,

    /// A native Linux build (no Windows executable).
    Linux,
}

/// The build of a game folder: Windows when the game's executable (`exe`, case aside) is in it.
pub fn build_of(game_dir: &Path, exe: &str) -> Build {
    let has_exe = std::fs::read_dir(game_dir).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(exe)));
    if has_exe { Build::Windows } else { Build::Linux }
}

/// A compatibility tool: its folder and its manifest.
#[derive(Clone, Debug)]
struct Tool {
    dir: PathBuf,
    manifest: Vdf,
}

impl Tool {
    fn at(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("toolmanifest.vdf")).ok()?;
        let manifest = Vdf::parse(&text);
        Some(Self { dir: dir.to_path_buf(), manifest: manifest.get("manifest").cloned().unwrap_or(manifest) })
    }

    /// Its command line with the verb: the program (in its folder) and its arguments.
    fn command(&self) -> Vec<String> {
        let line = self.manifest.get("commandline").and_then(Vdf::str).unwrap_or("/proton %verb%");
        line.split_whitespace().enumerate().map(|(i, part)| {
            let part = part.replace("%verb%", VERB);
            if i == 0 { self.dir.join(part.trim_start_matches('/')).to_string_lossy().into_owned() } else { part }
        }).collect()
    }

    fn required_app(&self) -> Option<u32> {
        self.manifest.get("require_tool_appid").and_then(Vdf::str).and_then(|s| s.trim().parse().ok())
    }
}

/// The folder name of a Valve tool's internal name: lower case, every run of other characters than letters and digits one `_`.
fn tool_key(folder: &str) -> String {
    let mut key = String::new();
    for c in folder.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c);
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }

    key.trim_matches('_').to_owned()
}

/// What starts a game through Proton: the programs before the game (the runtime's and Proton's command lines) and the environment.
#[derive(Clone, Debug, PartialEq)]
pub struct Launcher {
    /// The command line before the game's program.
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,

    /// The Proton folder, for the log.
    pub proton: PathBuf,
}

/// Steam's folders: the client's, and the libraries (each with a `steamapps`), the game's first.
struct Steam {
    root: PathBuf,
    libraries: Vec<PathBuf>,
}

impl Steam {
    fn find(game_dir: &Path) -> Result<Self, String> {
        // The game's library: <library>/steamapps/common/<game>.
        let library = game_dir.parent().and_then(Path::parent).filter(|s| s.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))).and_then(Path::parent)
            .ok_or_else(|| format!("{} is not in a Steam library (<library>/steamapps/common/<game>): Proton needs Steam's", game_dir.display()))?;
        let roots = wce_editor::steam_detect::steam_roots();
        let root = roots.first().cloned().ok_or("Steam's folder was not found (~/.local/share/Steam, ~/.steam/steam)")?;
        let mut libraries = vec![library.to_path_buf()];
        for lib in wce_editor::steam_detect::steam_library_folders() {
            if !libraries.iter().any(|l| same(l, &lib)) {
                libraries.push(lib);
            }
        }

        Ok(Self { root, libraries })
    }

    /// The compatibility tool Steam runs the app with: its own entry, else the default (`0`).
    fn tool_name(&self, app: u32) -> Option<String> {
        let text = std::fs::read_to_string(self.root.join("config").join("config.vdf")).ok()?;
        let vdf = Vdf::parse(&text);
        let mapping = vdf.at(&["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping"])?;
        [app.to_string(), "0".to_owned()].iter().find_map(|id| mapping.at(&[id, "name"]).and_then(Vdf::str).filter(|n| !n.is_empty()).map(str::to_owned))
    }

    /// A tool by its internal name: a custom one (`compatibilitytools.d`, named by its `compatibilitytool.vdf`), else Valve's (the folder in
    /// `steamapps/common` whose name reads as it).
    fn tool(&self, name: &str) -> Option<Tool> {
        let custom = [self.root.join("compatibilitytools.d"), PathBuf::from("/usr/share/steam/compatibilitytools.d")];
        for parent in custom {
            for dir in subfolders(&parent) {
                let Ok(text) = std::fs::read_to_string(dir.join("compatibilitytool.vdf")) else { continue };
                let vdf = Vdf::parse(&text);
                let Some(tools) = vdf.at(&["compatibilitytools", "compat_tools"]) else { continue };
                for (key, t) in tools.items() {
                    if key == name {
                        let path = t.get("install_path").and_then(Vdf::str).unwrap_or(".");
                        return Tool::at(&dir.join(path));
                    }
                }
            }
        }

        let common = self.libraries.iter().flat_map(|l| subfolders(&l.join("steamapps").join("common"))).collect::<Vec<_>>();
        let key = |d: &PathBuf| tool_key(&d.file_name().unwrap_or_default().to_string_lossy());
        common.iter().find(|d| key(d) == name).or_else(|| common.iter().find(|d| key(d).starts_with(&format!("{name}_")))).and_then(|d| Tool::at(d))
    }

    /// The newest Proton of the libraries (by folder name; Experimental first), when Steam names none.
    fn any_proton(&self) -> Option<Tool> {
        let mut found = self.libraries.iter().flat_map(|l| subfolders(&l.join("steamapps").join("common")))
            .filter(|d| d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Proton")) && d.join("proton").is_file()).collect::<Vec<_>>();
        found.sort_by_key(|d| {
            let name = d.file_name().unwrap_or_default().to_string_lossy().into_owned();
            (name.contains("Experimental"), name)
        });
        found.last().and_then(|d| Tool::at(d))
    }

    /// The tool of an app id (a runtime): `steamapps/appmanifest_<id>.acf`'s `installdir`, in that library.
    fn app_tool(&self, app: u32) -> Option<Tool> {
        self.libraries.iter().find_map(|lib| {
            let steamapps = lib.join("steamapps");
            let text = std::fs::read_to_string(steamapps.join(format!("appmanifest_{app}.acf"))).ok()?;
            let dir = Vdf::parse(&text).at(&["AppState", "installdir"])?.str()?.to_owned();
            Tool::at(&steamapps.join("common").join(dir))
        })
    }
}

fn same(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b)
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut out = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect::<Vec<_>>()).unwrap_or_default();
    out.sort();
    out
}

/// How to start a game's Windows build through Proton: Steam's tool for `app` (or the `proton` folder of the settings), inside the runtime it
/// requires, in the game's prefix.
///
/// # Errors
///
/// The game is not in a Steam library, no Proton is found, or the game's prefix is not there (the game never ran through Steam).
pub fn launcher(game_dir: &Path, app: u32, proton: Option<&Path>) -> Result<Launcher, String> {
    let steam = Steam::find(game_dir)?;
    let tool = match proton {
        Some(dir) => Tool::at(dir).ok_or_else(|| format!("{} is not a Proton folder (no toolmanifest.vdf): Settings > Tile > Launch in game", dir.display()))?,
        None => match steam.tool_name(app) {
            Some(name) => steam.tool(&name).ok_or_else(|| format!("Steam runs the game with {name}, which is not installed"))?,
            None => steam.any_proton().ok_or("no Proton found: install one in Steam, or name its folder in Settings > Tile > Launch in game")?,
        },
    };

    let mut command = vec![];
    if let Some(runtime) = tool.required_app() {
        let rt = steam.app_tool(runtime).ok_or_else(|| format!("{} needs the Steam Linux Runtime (app {runtime}), which is not installed", tool.dir.display()))?;
        command.extend(rt.command());
    }

    command.extend(tool.command());

    let library = &steam.libraries[0];
    let compat = library.join("steamapps").join("compatdata").join(app.to_string());
    if !compat.join("pfx").is_dir() {
        return Err(format!("no Proton prefix at {}: start the game once from Steam", compat.display()));
    }

    let s = |p: &Path| p.to_string_lossy().into_owned();
    let env = BTreeMap::from([
        ("STEAM_COMPAT_DATA_PATH".to_owned(), s(&compat)),
        ("STEAM_COMPAT_CLIENT_INSTALL_PATH".to_owned(), s(&steam.root)),
        ("STEAM_COMPAT_INSTALL_PATH".to_owned(), s(game_dir)),
        ("STEAM_COMPAT_LIBRARY_PATHS".to_owned(), steam.libraries.iter().map(|l| s(&l.join("steamapps"))).collect::<Vec<_>>().join(":")),
        ("STEAM_COMPAT_APP_ID".to_owned(), app.to_string()),
        ("SteamAppId".to_owned(), app.to_string()),
        ("SteamGameId".to_owned(), app.to_string()),
    ]);

    Ok(Launcher { command, env, proton: tool.dir })
}

/// A Linux path as the game sees it under Wine (`Z:` is `/`), with backslashes.
pub fn windows_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

/// The batch file that starts the game with its command line as written: from the game's folder, `%` doubled (cmd expands it).
pub fn batch_file(game_dir: &Path, exe: &str, command_line: &str) -> String {
    format!("@echo off\r\ncd /d \"{}\"\r\n\"{exe}\" {}\r\n", windows_path(game_dir), command_line.replace('%', "%%"))
}

/// Starts the game through Proton: writes the batch file into `scratch` and runs it with the prefix's `cmd`.
///
/// # Errors
///
/// No launcher (see [`launcher`]), the batch file can't be written, or the runtime can't be started.
pub fn start(game_dir: &Path, exe: &str, command_line: &str, app: u32, proton: Option<&Path>, scratch: &Path) -> Result<(std::process::Child, Launcher), String> {
    let launcher = launcher(game_dir, app, proton)?;
    let bat = scratch.join("wce_launch.bat");
    std::fs::write(&bat, batch_file(game_dir, exe, command_line)).map_err(|e| format!("{}: {e}", bat.display()))?;

    let (program, args) = launcher.command.split_first().ok_or("no Proton command")?;
    let mut command = Command::new(program);
    for name in EDITOR_ONLY_ENV {
        command.env_remove(name);
    }

    let child = command
        .args(args)
        .args(["c:\\windows\\system32\\cmd.exe".to_owned(), "/c".to_owned(), windows_path(&bat)])
        .envs(&launcher.env)
        .current_dir(game_dir)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok((child, launcher))
}

/// Which of these executables (Windows names, case aside) run, from `/proc`: a Wine process has the Windows path of its program in its
/// command line; `steam` is Steam's Linux client. None when `/proc` can't be read.
pub fn processes_running(exes: &[&str]) -> Option<Vec<bool>> {
    let mut found = vec![false; exes.len()];
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        if !entry.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }

        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let Some(program) = cmdline.split(|&b| b == 0).next() else { continue };
        let program = String::from_utf8_lossy(program).to_lowercase();
        let name = program.rsplit(['/', '\\']).next().unwrap_or_default();
        for (i, exe) in exes.iter().enumerate() {
            let exe = exe.to_lowercase();
            // Steam's client is `steam` on Linux (the Windows name, `steam.exe`, is Proton's own stand-in in every prefix).
            let linux = exe.strip_suffix(".exe").filter(|e| *e == "steam");
            found[i] |= match linux {
                Some(l) => name == l || name == "steam.sh",
                None => name == exe,
            };
        }
    }

    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_reads_blocks_strings_and_comments() {
        let v = Vdf::parse("// x\n\"a\"\n{\n\t\"b\"\t\t\"1\" // c\n\t\"C\" { \"d\" \"e\\\\f\" }\n}\n");
        assert_eq!(v.at(&["a", "b"]).and_then(Vdf::str), Some("1"));
        assert_eq!(v.at(&["A", "c", "D"]).and_then(Vdf::str), Some("e\\f"));
    }

    #[test]
    fn valve_tool_folders_read_as_their_names() {
        assert_eq!(tool_key("Proton - Experimental"), "proton_experimental");
        assert_eq!(tool_key("Proton 9.0 (Beta)"), "proton_9_0_beta");
        assert_eq!(tool_key("Proton Hotfix"), "proton_hotfix");
    }

    #[test]
    fn tool_command_lines_chain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("toolmanifest.vdf"), "\"manifest\"\n{\n  \"commandline\" \"/_v2-entry-point --verb=%verb% --\"\n}\n").unwrap();
        let t = Tool::at(dir.path()).unwrap();
        assert_eq!(t.command(), [dir.path().join("_v2-entry-point").to_string_lossy().into_owned(), "--verb=waitforexitandrun".to_owned(), "--".to_owned()]);
        assert_eq!(t.required_app(), None);
    }

    #[test]
    fn the_batch_file_keeps_the_command_line() {
        let b = batch_file(Path::new("/games/Total War Attila"), "Attila.exe", "mod \"a%b.pack\"; game_startup_mode battle x.xml;");
        assert_eq!(b, "@echo off\r\ncd /d \"Z:\\games\\Total War Attila\"\r\n\"Attila.exe\" mod \"a%%b.pack\"; game_startup_mode battle x.xml;\r\n");
    }

    #[test]
    fn a_folder_with_the_exe_is_the_windows_build() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(build_of(dir.path(), "Attila.exe"), Build::Linux);
        std::fs::write(dir.path().join("attila.EXE"), b"").unwrap();
        assert_eq!(build_of(dir.path(), "Attila.exe"), Build::Windows);
    }
}
