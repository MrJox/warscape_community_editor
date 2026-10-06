# Attila Weather Editor

Standalone weather and lighting editor for Total War: Attila. See [PLAN.md](PLAN.md) for scope and phases.

## Build

Needs Visual Studio 2022 (C++ workload) and CMake 3.24+.

```
cmake -S . -B build -G "Visual Studio 17 2022" -A x64
cmake --build build --config Release
```

## Run

`build/Release/weather_editor.exe`. It reads the game packs through `rpfm_cli`. Override paths with environment variables:

| Variable | Default |
| --- | --- |
| `ATTILA_DIR` | `D:\SteamLibrary\steamapps\common\Total War Attila` |
| `RPFM_CLI` | `C:\Users\Khaiali\Downloads\rpfm-v4.3.14-x86_64-pc-windows-msvc\rpfm_cli.exe` |
| `AWE_CACHE` | `%LOCALAPPDATA%\awe` (pack indexes, extracted files, settings) |

Edited files are saved as loose files under the output folder (Settings menu), keeping their in-pack path. Unchanged bytes stay identical to the source.

## Tools

| Tool | Purpose |
| --- | --- |
| `xml_roundtrip <dir> [--schema out.tsv]` | Gate: byte-identical load/edit/revert/save of every weather, lighting and vfx file |
| `format_check <dir>` | Gate: every attribute value re-formats to its original text |
| `fxc_dump <fxc_dir> <out_dir>` | Gate: every dx11 game shader creates on D3D11; dumps reflection to `docs/re/shaders` |
| `vfs_probe [path...]` | Shows mounted packs and where files resolve |
