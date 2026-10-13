//! Machine settings: a `key=value` text file (wce.ini, in `%LOCALAPPDATA%\awe` on Windows and `~/.local/share/awe` on Linux by
//! default) with environment overrides. Nothing about the machine lives in code; the first-run dialog writes this file. Lines
//! starting with `#` are comments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// An environment variable, if set and not empty.
fn env_value(name: Option<&str>) -> Option<String> {
    std::env::var(name?).ok().filter(|value| !value.is_empty())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    file: PathBuf,
    values: BTreeMap<String, String>,
}

impl Settings {

    /// The folder that holds wce.ini: `$WCE_SETTINGS_DIR`, else `%LOCALAPPDATA%\awe` (Linux: `$XDG_DATA_HOME/awe`, else `~/.local/share/awe`).
    pub fn default_dir() -> PathBuf {
        if let Some(dir) = env_value(Some("WCE_SETTINGS_DIR")) {
            return PathBuf::from(dir);
        }

        let local = env_value(Some("LOCALAPPDATA")).map(PathBuf::from)
            .or_else(|| if cfg!(windows) { None } else { env_value(Some("XDG_DATA_HOME")).map(PathBuf::from) })
            .or_else(|| if cfg!(windows) { None } else { env_value(Some("HOME")).map(|home| Path::new(&home).join(".local").join("share")) })
            .unwrap_or_else(std::env::temp_dir);
        local.join("awe")
    }

    /// Reads wce.ini from a folder; no file gives empty settings that save there.
    pub fn load(dir: &Path) -> Self {
        let file = dir.join("wce.ini");
        let text = std::fs::read(&file).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
        let values = text.split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();

        Self { file, values }
    }

    /// The VFS cache folder: `$WCE_CACHE_DIR`, else `$AWE_CACHE`, else `cache_dir` in the file, else the settings folder.
    pub fn cache_dir(&self) -> PathBuf {
        self.get("cache_dir", Some("WCE_CACHE_DIR"))
            .or_else(|| env_value(Some("AWE_CACHE")))
            .map(PathBuf::from)
            .unwrap_or_else(|| self.file.parent().map(Path::to_path_buf).unwrap_or_default())
    }

    /// A value: the environment variable `env` (when given and not empty) wins over the file's entry `key`. Empty entries are missing.
    pub fn get(&self, key: &str, env: Option<&str>) -> Option<String> {
        env_value(env).or_else(|| self.values.get(key).filter(|value| !value.is_empty()).cloned())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_owned(), value.to_owned());
    }

    /// Rewrites wce.ini with the current values.
    ///
    /// # Errors
    ///
    /// Fails if the folder or the file can't be written.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let text = self.values.iter().map(|(key, value)| format!("{key}={value}\n")).collect::<String>();
        std::fs::write(&self.file, text)
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}
