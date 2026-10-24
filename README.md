# Warscape Community Editor

One editor for Total War: Attila (Warscape engine) content: weather and lighting, battle and campaign tiles, and later
units, VFX, battle maps and cinematics. It shares one viewport and one renderer built on the game's own shaders. Plan
and status: [PLAN.md](PLAN.md).

**State:** `wce` is one Rust application (a Cargo workspace, `crates/`): the weather tool, the tile tool and the UI editor in a shared
shell. The gates are Cargo examples and tests, run by `scripts/`. The earlier plans are in [docs/plans/](docs/plans/).

## Build

rpfm_lib is a path dependency: check out [RPFM](https://github.com/Frodo45127/rpfm) (develop branch) next to this repo, as `../rpfm`.

### Windows

The `wce` binary builds SDL3 from source. In PowerShell, use a short Cargo target path so SDL's MSBuild compiler check can create
its temporary files:

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP "wce-target"
cargo build --release -p wce_app
```

The binary lands at `$env:TEMP\wce-target\release\wce.exe`.
elease\wce.exe`.

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

## Gates

`scripts/gates.sh [out dir] [--corpus] [--render]` runs every gate with fixed inputs and writes one
output file per gate. `scripts/imgcmp.py <run a> <run b>` compares the reference screenshots of two runs. The recorded
baseline and the comparison rules are in [docs/baseline.md](docs/baseline.md). Every migration step must reproduce it.

Requirements: the game, the battle/campaign tilemap assembly kits for the tile gates, and
`manifest/corpus.tsv` (built by `scripts/corpus_inventory.py`; not in git).
