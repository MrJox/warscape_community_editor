//! The Flatpak build (packaging/flatpak): whether the editor runs in its sandbox, and running a program on the host from there.
//!
//! The sandbox sees the host's files (`--filesystem=host`) but not its processes, and it has no git: what has to run as the user's own
//! programs (the game through Proton, git with the user's credentials, `flatpak` to install an update) goes through `flatpak-spawn --host`
//! (the manifest's `--talk-name=org.freedesktop.Flatpak`). Elsewhere [`on_host`] changes nothing.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// The file Flatpak puts at the root of every sandbox: its app id, its installation.
const INFO: &str = "/.flatpak-info";

/// The editor runs in its Flatpak sandbox.
pub fn flatpak() -> bool {
    static IN: OnceLock<bool> = OnceLock::new();
    *IN.get_or_init(|| Path::new(INFO).is_file())
}

/// The app id of the Flatpak the editor runs in (none outside one).
pub fn flatpak_app_id() -> Option<String> {
    flatpak().then(|| std::env::var("FLATPAK_ID").ok().filter(|id| !id.is_empty())).flatten()
}

/// The Flatpak the editor runs in was installed for the user (`~/.local/share/flatpak`), not for the system: its `[Instance] app-path`.
pub fn flatpak_user_install() -> bool {
    let info = std::fs::read_to_string(INFO).unwrap_or_default();
    let app_path = info.lines().find_map(|l| l.trim().strip_prefix("app-path=")).unwrap_or_default();
    !app_path.starts_with("/var/lib/flatpak/")
}

/// `command` as the host runs it: in the sandbox, `flatpak-spawn --host` with its folder and the variables it sets or removes (the host
/// program gets the host session's environment, not the editor's); elsewhere `command` itself. The program is looked for in the host's
/// `PATH`. A host program outlives the editor (no `--watch-bus`): a game started from it keeps running when it quits.
pub fn on_host(command: Command) -> Command {
    if !flatpak() {
        return command;
    }

    let mut host = Command::new("flatpak-spawn");
    host.arg("--host");
    if let Some(dir) = command.get_current_dir() {
        host.arg(prefixed("--directory=", dir.as_os_str()));
    }

    for (name, value) in command.get_envs() {
        match value {
            Some(value) => {
                let mut arg = prefixed("--env=", name);
                arg.push("=");
                arg.push(value);
                host.arg(arg);
            }

            None => {
                host.arg(prefixed("--unset-env=", name));
            }
        }
    }

    host.arg(command.get_program()).args(command.get_args());
    host
}

fn prefixed(prefix: &str, value: &std::ffi::OsStr) -> OsString {
    let mut arg = OsString::from(prefix);
    arg.push(value);
    arg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_a_sandbox_the_command_is_unchanged() {
        if flatpak() {
            return;
        }

        let mut c = Command::new("git");
        c.arg("status").env("A", "1").current_dir("/");
        let c = on_host(c);
        assert_eq!(c.get_program(), "git");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["status"]);
    }
}
