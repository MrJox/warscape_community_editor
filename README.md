# Warscape Community Editor

**An all-in-one modding editor for Total War games on the Warscape engine.**

With Warscape Community Editor (WCE) you create new content for Total War games: build the battlefields your battles are fought on,
tune the weather and lighting, rework the interface and edit the game's sounds, all in one application. Its 3D view draws everything
with **the game's own shaders**, so what you see in the editor is what you get in the game.

<p align="center">
  <img src="https://github.com/user-attachments/assets/572c01d3-4599-4cb5-8481-e7911767d3ed" alt="The Tile editor with a battle tile open" width="100%">
</p>

## Supported games

- Total War: ATTILA
- Total War Saga: Thrones of Britannia
- Total War: Rome II

The editor runs on **Windows** and **Linux**.

## Editors

### Tile editor

Build the battlefields your battles are fought on, from a single field to a whole fortified city:

- Place and arrange buildings, props, trees, effects and lights, with snapping and multi-selection.
- Sculpt and paint the ground with brushes, stamps and images.
- Draw roads, rivers, walls and fences, and generate a city's siege AI in one click.
- See your tile among its neighbours on the campaign map, in any of the game's weathers.
- Keep every version of a tile (a city's sizes, its alternatives) in one project.
- Open tiles from the Assembly Kit, or straight from the game.
- Save them for the Assembly Kit, or export them ready for the game or as a mod pack, minimap included.
- **Launch in game**: one click and the game starts straight into a battle on your tile.

### Prefab editor

Build groups of buildings and props once and reuse them in any tile. Change a prefab, and every tile that uses it updates.

### Weather and lighting editor

Edit the game's weather and lighting (sun, sky, clouds, fog, rain and snow, colours) with a live preview on a sample map or on the tile
you are working on, and pick which weather a battle uses.

### UI editor

Open the game's interface layouts, edit them on a canvas or as XML, make skins of your own, and export them back to the game's format
or into a mod pack.

### Audio editor

Browse the game's sounds with their names: search them, see what plays when, and listen to them. Replace sounds with your own WAV or Ogg
files, add variations, and edit the music playlists and the units' voice lines.

### For every editor

- Undo and redo for every edit, and autosave with recovery after a crash.
- Works with the mods you have installed.
- Updates itself (Help > Update Manager).
- Teams can share their models, prefabs and vegetation through a git repository (needs git installed).

## Installing

Download the latest version from the [releases page](https://github.com/MrJox/warscape_community_editor/releases):

- **Windows**: unzip `wce-<version>-windows-x64.zip` anywhere and run `wce.exe`.
- **Linux**: `flatpak install --user wce-<version>-linux-x86_64.flatpak`, then start it from the menu or with
  `flatpak run io.github.mrjox.WarscapeCommunityEditor`.

On first run the editor asks for the game's folder (and finds it on its own when it can). The Assembly Kit is optional: with it, you can
open and save the kit's tiles. On Linux, the editor works with the Windows version of the games, installed through Steam's Proton.

## Building from source

**Windows**: you need [Rust](https://rustup.rs) 1.91 or newer, and Visual Studio 2022's C++ build tools with CMake.

```powershell
cargo build --release -p wce_app --no-default-features --features release
```

**Linux**: you need [Rust](https://rustup.rs) 1.91 or newer, a Vulkan driver, CMake 3.24+, Ninja, a C/C++ compiler, git, meson, glslang, autoconf,
automake, libtool, bison, flex, perl, Wine, and the SDL3 and Vulkan headers. On Arch Linux:

```sh
sudo pacman -S cmake ninja meson glslang autoconf automake libtool bison flex perl wine sdl3 vulkan-headers vulkan-icd-loader
cmake -S . -B build -G Ninja
cargo build --release -p wce_app --no-default-features --features release
```

The editor is `target/release/wce` (`wce.exe` on Windows). On Linux, file dialogs need zenity or kdialog.

Contributors: how the editor works is in [docs/editor.md](docs/editor.md), the roadmap in [PLAN.md](PLAN.md), the checks in
[docs/baseline.md](docs/baseline.md), and the project's rules in [AGENTS.md](AGENTS.md).

## Credits

Built on [RPFM](https://github.com/Frodo45127/rpfm) for the game's files, [Dear ImGui](https://github.com/ocornut/imgui) for the
interface and [DXVK](https://github.com/doitsujin/dxvk) on Linux; the full list is in [THIRD_PARTY.md](THIRD_PARTY.md).

Warscape Community Editor is a fan project, not affiliated with or endorsed by Creative Assembly or SEGA. Total War and the games' names
are their trademarks. The editor ships no game content: it reads it from your own installation.

## License

The editor's original code and documentation are licensed under the [WCE Noncommercial Share-Alike License](LICENSE). It permits
noncommercial use, modification and redistribution. Distributed forks must provide their corresponding source under the same terms.
Commercial use or closed-source distribution requires prior written permission from the relevant copyright holders. This is a
source-available license and is not an OSI-approved open source license. Third-party components remain under their own licenses.
