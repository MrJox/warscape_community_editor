# The Linux Flatpak

`io.github.mrjox.WarscapeCommunityEditor.yaml` builds the editor as a Flatpak on the freedesktop.org runtime (26.08: SDL3 3.4, the Vulkan
loader, zenity for the file dialogs). The release and nightly workflows publish it as `wce-<version>-linux-x86_64.flatpak` beside the Windows
zip.

## Building it locally

```
scripts/flatpak.sh [--skip-cargo-sources] [--install]
```

Needs `flatpak`, `flatpak-builder` and `flatpak-cargo-generator` ([flatpak-builder-tools](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo));
the SDK, its Rust extension and the runtime are installed from Flathub for the user when missing. The bundle lands in `target/flatpak`
(`$WCE_FLATPAK_OUT` moves it). `flatpak run io.github.mrjox.WarscapeCommunityEditor` starts an installed one.

The build is offline, as Flathub's are:

- the crates come from `cargo-sources.json`, generated from `Cargo.lock` (not in git: the script and the workflows make it each time, so it
  never lags behind the lock file);
- DXVK-native, vkd3d and SPIRV-Headers are sources of the manifest, put where `cmake/linux_deps.cmake` would clone them, then built by it as
  on any Linux machine (its patches included). vkd3d comes from its release tarball: a git checkout needs Wine's `widl` to make headers the
  tarball already has. **A new DXVK/vkd3d/SPIRV-Headers release in `cmake/linux_deps.cmake` goes into the manifest too.**

## The sandbox

The editor needs the host: `--filesystem=host` (games, kits and projects anywhere, plus the folders of Steam's and other launchers'
Flatpaks) and `--talk-name=org.freedesktop.Flatpak`, so that `wce_foundation::sandbox::on_host` can run programs on the host through
`flatpak-spawn --host`:

- **Launch in game**: Steam's Linux runtime and Proton start as Steam starts them, and the check for a running game or Steam reads the host's
  process list (the sandbox's `/proc` has only its own);
- **art resources**: the host's `git`, with the user's credential helper and SSH keys (the runtime has no git);
- **updates**: the host's `flatpak` installs the next bundle;
- opening a folder or a web page uses the host's `xdg-open`.

The file dialogs are the runtime's zenity, inside the sandbox; with `--filesystem=host` they return the real paths.

The Flatpak keeps its settings, data, caches and log under `~/.var/app/io.github.mrjox.WarscapeCommunityEditor` (`config/wce`, `data/wce`,
`cache/wce`, `.local/state/wce`), not in `~/.config/wce` and the rest: a source build's settings are not shared. The editor's tables are in
`/app/share/wce/tables`, which can't be written: a user's own table files (`walls_mine.toml`...) go in `data/wce/tables/<game>`, read over the
app's on every platform (`wce_tile::user_files`).

## Updates

A `.flatpak` bundle has no repository to update from (`flatpak update` skips it) and GitHub releases can't host one, so the editor updates
itself, as on Windows: Help > Update Manager (and the check at start) finds the newer release's `wce-<version>-linux-x86_64.flatpak`,
downloads it into the Flatpak's cache folder (the same path on the host) and runs the host's `flatpak install --user --bundle` (`--system`
for a system install, which asks for a password). A newer bundle over an installed one updates it in place, even while the editor runs
(checked with flatpak 1.18: the running instance keeps its files until it quits); "Restart now" starts the new one with `flatpak run`.

The other way, not taken: an OSTree repository published to GitHub Pages and named in the bundle (`flatpak build-bundle --repo-url`), which
`flatpak update`, GNOME Software and Discover would update from. It needs Pages turned on, a GPG key in the repository's secrets, and the
repository rebuilt on each release; the in-app update needs neither.
