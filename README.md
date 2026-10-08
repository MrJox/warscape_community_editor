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

Command-line options (also used for automated screenshots): `--open <path>` (repeatable), `--cam <yaw> <pitch> <dist> [target_y]`,
`--view <n>`, `--blend <t>`, `--pick <category>=<value>` (repeatable, opens the Weather picker with those conditions), `--picker x` / `--picker edit` (opens the Weather picker on the Pick / Edit tab), `--vdb-script "add-value:climate=X;save:both"` (edits and saves the database, for tests), `--frames <n>` and `--shot <file.png>` (saves the window at frame n, plus `.txt` diagnostics and,
when precipitation uses it, `.distortion.png`). `AWE_D3D_DEBUG=1` enables the D3D debug layer messages in the diagnostics.

Precipitation (Precipitation menu) comes from the WEATHER definition_name of the open environment; open `vfx/*.xml` or
`weather/weather_definitions.xml` documents are used instead of the pack versions, so their edits show immediately.

Selection > Weather picker works on the weather selection database (`warscape_asset_variation_db`). The Pick tab shows which `.environment` file the game chooses for a set of battle conditions (map, season, precipitation, time of day, climate change index, snow attrition, battle type, climate) and, for the open file, the conditions under which it is chosen. The Edit tab edits the value lists and the variations (file name and allowed conditions per variation). The Source box loads the `.bin` or the `.xml` of any pack, or a loose file; Save XML / Save FASTBIN / Save both write them as loose files under the output folder, `warscape_asset_variation_db\`. See [docs/re/variation_db.md](docs/re/variation_db.md).

Edited files are saved as loose files under the output folder (Settings menu), keeping their in-pack path. Unchanged bytes stay identical to the source.

## Tools

| Tool | Purpose |
| --- | --- |
| `xml_roundtrip <dir> [--schema out.tsv]` | Gate: byte-identical load/edit/revert/save of every weather, lighting and vfx file |
| `format_check <dir>` | Gate: every attribute value re-formats to its original text |
| `fxc_dump <fxc_dir> <out_dir>` | Gate: every dx11 game shader creates on D3D11; dumps reflection to `docs/re/shaders` |
| `vfs_probe [path...]` | Shows mounted packs and where files resolve |
| `rmv2_check` | Parses building rigid models |
| `env_vs_dump <env path or --all> <dump_cbs.txt> [gamma brightness]` | Compares the editor's lighting constants with a game memory dump; `--all` finds which weather file the dump used |
| `vfx_check "<weather definition>" [rows.txt]` | Builds the precipitation emitters of a weather definition and compares their constants with dumped rows |
| `variation_check` | Gate on `warscape_asset_variation_db` in every pack that has it: FASTBIN writer identical, XML and bin equal, XML to bin identical, canonical XML output, editing operations; also every referenced file exists, every variation found by its own conditions; reports condition combinations without a file and unreferenced `.environment` files |
| `variation_convert <in> <out> [--force]` | Converts the weather selection database between `.xml` and `.bin` (FASTBIN0) by file extension, validating it first |
| `tools/variation_e2e.ps1` | End to end: edit in the editor (scripted), save XML and FASTBIN, convert both ways, compare byte for byte |
| `tools/re/dump_cbs.py <dmp> [regex] [--array=512]` | Live shader constants from a Task Manager full dump of Attila.exe |
| `tools/re/schema.py <fn_rva>` | Property names, types and defaults of a schema function in empire.retail.dll |

## Decompiler (reverse engineering)

`TDD Claude\tools\` holds Ghidra 12.1.4 and a portable JDK 21 with an analysed project of `empire.retail.dll`
(`tools\ghidra_proj`). `powershell -File tools\ghidra.ps1 Decomp out.c <rva>...` decompiles functions,
`... Xrefs out.txt <rva>...` lists references, `... DecompRange out.c <start> <end>` decompiles a whole range
(`tools\decomp\r16_1a.c` covers the renderer, VFX and weather code, RVA 0x1600000-0x1a00000). Addresses are RVAs;
Ghidra shows them at image base 0x10000000.
