# Warscape Community Editor

One editor for Total War content on the Warscape engine (Attila first, Rome II and Thrones of Britannia too): weather and lighting,
prefabs, battle and campaign tiles, battle and campaign maps, UI layouts, sound banks, units, VFX and cinematics. It shares one viewport
and one renderer built on the game's own shaders. Plan and status: [PLAN.md](PLAN.md); what each tool does: [docs/editor.md](docs/editor.md).

**State:** `wce` is one Rust application (a Cargo workspace, `crates/`) with every tool in a shared shell; the stable ones ship in releases
("Editors" below). The gates are Cargo examples and tests, run by `scripts/`. The plans are in [docs/plans/](docs/plans/), the
reverse-engineering notes and file formats in [docs/re/](docs/re/) and [docs/formats/](docs/formats/).

## Build

rpfm_lib comes from [RPFM](https://github.com/Frodo45127/rpfm)'s develop branch (a git dependency); to work against a local checkout,
put it next to this repo as `../rpfm` and use the path line in `Cargo.toml`.

### Windows

The `wce` binary builds SDL3 from source. Cargo's `build-dir` (set in `.cargo/config.toml`) keeps the intermediates under
`%USERPROFILE%\.cargo\build`, so a long repo path does not break SDL's MSBuild compiler check (it needs Cargo 1.91 or newer):

```powershell
cargo build --release -p wce_app
```

The binary lands at `target\release\wce.exe`.

### Linux

The renderer stays D3D11: it runs on DXVK-native (D3D11 over Vulkan) with vkd3d-utils for the shader compiler and reflection, in an SDL3
window. A CMake configure step fetches and builds both into `build/_deps` (a few minutes, once; see [THIRD_PARTY.md](THIRD_PARTY.md));
`crates/wce_d3d11` links them from there (or from `$WCE_DEPS_PREFIX`).

Needs a Vulkan driver and: Rust, CMake 3.24+, Ninja, a C/C++ compiler, git, meson, glslang, autoconf/automake/libtool, bison, flex, perl,
`widl` (part of Wine), and the SDL3 and Vulkan headers. On Arch: `pacman -S cmake ninja meson glslang autoconf automake libtool bison flex
perl wine sdl3 vulkan-headers vulkan-icd-loader`.

```
cmake -S . -B build -G Ninja
cargo build --release -p wce_app
```

The binary lands in `target/release/wce`. Settings live in `~/.config/wce/settings.toml`, caches in `~/.cache/wce` (see "Settings and first run" in `docs/editor.md`). The game is the Windows (Proton)
install, found in the usual Steam folders. File dialogs use zenity or kdialog.

### Editors

Each editor is a Cargo feature of `wce_app`. A development build (the default features) has every editor; releases and nightlies build
only the stable ones, the `release` feature, with `cargo build --release -p wce_app --no-default-features --features release`:

| Feature | Editor | In releases |
|---|---|---|
| `weather` | Weather and lighting | yes |
| `prefab` | Prefabs (the Tile editor on a stage of its own; turns on `tile`) | yes |
| `tile` | Battle and campaign tiles | yes |
| `ui` | UI layouts | yes |
| `audio` | Sound banks: names, search, playback | yes |
| `terrain` | Battle maps shown whole | not yet |
| `cinematic` | Movies and keyed scenes | not yet |
| `unit` | Variant mesh definitions | not yet |
| `vfx` | VFX files | not yet |
| `placeholders` | The tabs of the editors still to come (none at present), off by default | no |

`cargo build -p wce_app --no-default-features --features tile` builds the Tile editor alone; the scripts build with `--all-features`.

## Releases and updates

Pushing a tag `vX.Y.Z` that matches the workspace version (`[workspace.package] version` in `Cargo.toml`) runs
`.github/workflows/release.yml`: it builds `wce.exe` for Windows and the Linux Flatpak, and publishes a GitHub release with
`wce-X.Y.Z-windows-x64.zip` and `wce-X.Y.Z-linux-x86_64.flatpak`. A tag with a pre-release part (`v0.2.0-beta.1`) is published as a
pre-release. Nightly builds run daily at 02:00 UTC (whenever new commits land on master) and on demand via `.github/workflows/nightly.yml`,
publishing pre-releases tagged `v<version>-nightly.YYYYMMDD` with the same two files.

On Linux, `flatpak install --user wce-X.Y.Z-linux-x86_64.flatpak` installs the release (it adds Flathub for the runtime when it is missing),
and `flatpak run io.github.mrjox.WarscapeCommunityEditor` (or the menu entry) starts it. `scripts/flatpak.sh` builds the bundle locally; see
[packaging/flatpak/README.md](packaging/flatpak/README.md).

Help > Update Manager manages the editor's updates and rpfm's schemas. It looks for a newer release when an interactive session starts
(release builds; Settings > General > Updates turns it off or includes pre-releases). On Windows it downloads the zip with a progress bar,
puts its files over the running build (the replaced files are renamed `*.wce-old` and deleted on the next start) and offers to restart.
In the Flatpak it downloads the release's `.flatpak` and installs it with the host's `flatpak` over the running one, then restarts into it.
Elsewhere (a source build) it links to the release page.

rpfm's schemas (`schema_att.ron`...) are needed to read the games' db tables (the buildings' data, the climate phases, the procedural
walls). The Update Manager compares the schemas in use with the newest commit of `Frodo45127/rpfm-schemas` and downloads every supported
game's schema into the editor's own schemas folder (`<data folder>/schemas`, with the commit in `.wce-schemas-version`). A schema is read
from `$WCE_RPFM_SCHEMA`, then Settings > General > DB schemas, then that folder, then rpfm's own. Without the current game's schema the
window opens at start, even with the start check turned off.

The art resources (Settings > Game > Art resources) are a git repository per game whose working tree is the projects'
`assembly_kit/raw_data/art`: a team shares its models, CS2 files, prefabs and vegetation sets there. The installed `git` does the work (its
credential manager or SSH keys sign in; Git LFS works when installed). The Update Manager sets the folder up (the files already there stay:
those the repository has differently are backed up to `<projects>/art_backup/<date>` first, the others become local changes), says when the
team pushed commits (looked for at start with the other updates, never pulled on its own) and brings them in. File > Publish art
resources..., the status bar's `art:` badge or the Update Manager's Publish button commit the files picked and push them, after bringing the
team's commits in. A file changed both locally and by the team is never merged: it asks Keep mine (the local file stays over theirs, to
publish) or Take theirs, per file, before anything is done.

## Gates

`scripts/gates.sh [out dir] [--corpus] [--render]` runs every gate with fixed inputs and writes one
output file per gate. `scripts/imgcmp.py <run a> <run b>` compares the reference screenshots of two runs. The recorded
baseline and the comparison rules are in [docs/baseline.md](docs/baseline.md). Every migration step must reproduce it.

The scripts run in bash: Git Bash (MSYS2) on Windows, the system's bash on Linux and macOS. `scripts/lib.sh` holds what differs between
them (path forms, `.exe`, the settings folder); the game folder is `$WCE_GAME`, else the current game's folder in the editor's
settings.toml.

Requirements: the game, the battle/campaign tilemap assembly kits for the tile gates, and
`manifest/corpus.tsv` (built by `scripts/corpus_inventory.py`; not in git).
