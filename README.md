# Warscape Community Editor

**An all-in-one modding editor for Total War games on the Warscape engine.**

[![Tests](https://github.com/MrJox/warscape_community_editor/actions/workflows/test.yml/badge.svg)](https://github.com/MrJox/warscape_community_editor/actions/workflows/test.yml)

Warscape Community Editor (WCE) brings the work of the Assembly Kit's separate tools into one modern application: you build the tiles
battles are fought on, tune the weather and lighting, rework the interface and edit the game's sounds in the same window, with the same
3D viewport.

The viewport renders with **the game's own shaders**, so what you see in the editor is what you get in the game. Edits are
**lossless**: a file you open and save changes only where you edited it, and the game's own files are never touched; your work goes
to a projects folder, to loose files or to a mod pack.

## Supported games

| Game | Support |
|---|---|
| Total War: ATTILA | Main target: every editor, with or without the Assembly Kit |
| Total War Saga: Thrones of Britannia | Same engine and Assembly Kit as Attila |
| Total War: Rome II | From the game's packs (its Assembly Kit has no battle tiles to open) |

The editor runs on **Windows** and **Linux** (where it finds the Steam / Proton install of the game).

## Editors

### Tile editor

Build and edit battle and campaign tiles, the pieces the game's battle maps are made of:

- Place and arrange buildings, props, vegetation, effects, lights and logic with a gizmo, snapping and multi-selection.
- Paint the terrain: heights, ground textures and blends, with brushes, stamps and imported images.
- Draw roads, rivers and walls along splines, and fences along a line; generate the siege AI layout of a settlement in one click.
- See the tile in context, surrounded by its neighbours on the campaign's battle map, under any of the game's weathers.
- Keep a tile's variations (a settlement's sizes, random alternatives) in one project.
- Open tiles from the Assembly Kit, or extract them straight from the game's packs.
- Export to the Assembly Kit, to the game's tile format or to a mod pack, with the battle minimap drawn for you.
- **Launch in game**: one click exports the tile and starts the game straight into a battle on it.

### Prefab editor

Groups of buildings and props you reuse across tiles. A prefab is edited on a stage of its own and placed in tiles as a reference: change
the prefab, and every tile that uses it follows.

### Weather and lighting editor

Edit the game's `.environment` and `.lighting` files (sun, sky, clouds, fog, rain and snow, colour grading, exposure) with a live
preview on a sample map or on the tile you are working on, and pick which weather a battle uses.

### UI editor

Open the game's interface layouts, edit them on a canvas or as XML, make skins of your own, and export them back to the game's format
or into a mod pack.

### Audio editor

Browse the game's Wwise sound banks with names instead of numbers: search across every bank, see what an event plays, and listen to it.
Replace sounds from WAV or Ogg files, add variations, edit music playlists and unit voice lines, and save the changed banks into your
project.

### For every editor

- Undo and redo for every edit, and autosave with recovery after a crash.
- Your game's mod packs mounted alongside the game's own.
- Built-in updates (Help > Update Manager), including the database schemas the editor needs to read the game's tables.
- Shared art resources: a team can keep its models, prefabs and vegetation sets in a git repository the editor publishes to and updates
  from.

## Installing

Download the latest version from the [releases page](https://github.com/MrJox/warscape_community_editor/releases):

- **Windows**: unzip `wce-<version>-windows-x64.zip` anywhere and run `wce.exe`.
- **Linux**: `flatpak install --user wce-<version>-linux-x86_64.flatpak`, then start it from the menu or with
  `flatpak run io.github.mrjox.WarscapeCommunityEditor`.

On first run the editor asks for the game's folder (and finds it on its own when it can). The Assembly Kit is optional: with it, you can
open and save the kit's tiles.

## Building from source

You need [Rust](https://rustup.rs) (stable). The build fetches [rpfm_lib](https://github.com/Frodo45127/rpfm) from git.

**Windows**: Cargo 1.91 or newer, and Visual Studio 2022's C++ build tools with CMake (SDL3 is built from source):

```powershell
cargo build --release -p wce_app --no-default-features --features release
```

**Linux**: the editor's Direct3D 11 renderer runs on DXVK-native (over Vulkan). A one-time CMake step builds it and vkd3d-utils into
`build/` (see [THIRD_PARTY.md](THIRD_PARTY.md)). You need a Vulkan driver, CMake 3.24+, Ninja, a C/C++ compiler, git, meson, glslang,
autoconf, automake, libtool, bison, flex, perl, `widl` (from Wine), and the SDL3 and Vulkan headers. On Arch Linux:

```sh
sudo pacman -S cmake ninja meson glslang autoconf automake libtool bison flex perl wine sdl3 vulkan-headers vulkan-icd-loader
cmake -S . -B build -G Ninja
cargo build --release -p wce_app --no-default-features --features release
```

The editor is `target/release/wce` (`wce.exe` on Windows). On Linux, file dialogs use zenity or kdialog. `scripts/flatpak.sh` builds
the Flatpak.

Contributors: how the editor works is in [docs/editor.md](docs/editor.md), the roadmap in [PLAN.md](PLAN.md), the checks in
[docs/baseline.md](docs/baseline.md), and the project's rules in [AGENTS.md](AGENTS.md).

## Credits

Built on [RPFM](https://github.com/Frodo45127/rpfm) for the game's files, [Dear ImGui](https://github.com/ocornut/imgui) for the
interface and [DXVK](https://github.com/doitsujin/dxvk) on Linux; the full list is in [THIRD_PARTY.md](THIRD_PARTY.md).

Warscape Community Editor is a fan project, not affiliated with or endorsed by Creative Assembly or SEGA. Total War and the games' names
are their trademarks. The editor ships no game content: it reads it from your own installation.
