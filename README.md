# Warscape Community Editor

One editor for Total War: Attila (Warscape engine) content: weather and lighting, battle and campaign tiles, and later
units, VFX, battle maps and cinematics. It shares one viewport and one renderer built on the game's own shaders. Plan
and status: [PLAN.md](PLAN.md).

**State: the two former editors are retired (Phase 8).** `wce.exe` is the one application: the weather tool, the tile tool
(the old Tile Studio) and the shared shell, with the gates under `cli/` and `scripts/`. The earlier plans are in
[docs/plans/](docs/plans/).

## Build

### Windows

```
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
```

Executables land in `build/bin/<config>` (`wce.exe` and the gates). Use `--config RelWithDebInfo` for test builds while an editor
is open (a Release build cannot overwrite a running exe).

### Linux

The renderer stays D3D11: it runs on DXVK-native (D3D11 over Vulkan) with vkd3d-utils for the shader compiler and reflection, in an SDL3
window. The first configure fetches and builds both into `build/_deps` (a few minutes, once; see [THIRD_PARTY.md](THIRD_PARTY.md)).

Needs a Vulkan driver and: CMake 3.24+, Ninja, a C++20 compiler, git, meson, glslang, autoconf/automake/libtool, bison, flex, perl, `widl`
(part of Wine), and the SDL3, libcurl and Vulkan headers. On Arch: `pacman -S cmake ninja meson glslang autoconf automake libtool bison flex
perl wine sdl3 curl vulkan-headers vulkan-icd-loader`.

```
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo
cmake --build build
```

Executables land in `build/bin` (`wce` and the gates, no `.exe`). Settings live in `~/.local/share/awe` (`$XDG_DATA_HOME/awe`). The game is
the Windows (Proton) install, found in the usual Steam folders. RPFM is found on `$PATH` (`rpfm_cli`, `rpfm_server`), or set
`WCE_RPFM_SERVER` / `RPFM_CLI`. File dialogs use zenity or kdialog.

## Gates

`scripts/gates.sh [out dir] [--corpus] [--render]` runs every gate with fixed inputs and writes one
output file per gate. `scripts/imgcmp.py <run a> <run b>` compares the reference screenshots of two runs. The recorded
baseline and the comparison rules are in [docs/baseline.md](docs/baseline.md). Every migration step must reproduce it.

Requirements: the game, RPFM (`rpfm_cli`), the battle/campaign tilemap assembly kits for the tile gates, and
`manifest/corpus.tsv` (built by `scripts/corpus_inventory.py`; not in git).
