# The game's command line (Attila, 2026-10-27)

What `Attila.exe` accepts at start-up, for Launch in game ([plans/launch_in_game.md](../plans/launch_in_game.md)). Sources: the strings
of `empire.retail.dll` (Attila 1.6, build 13356 Curator), the launcher's native module
(`launcher/resources/app.asar.unpacked/node_modules/greenworks/lib/greenworks-win32.node`), the game's own `docudemon` dump and launches.

## Form

The command line is a script: commands with their arguments, each ended by `;`. The launcher builds it from `mod "%1";` and
`add_working_directory "%1";` entries, and can also pass `game_startup_mode campaign_load <save>;` and `-dx11` / `-dx12`. Run with the
game folder as the working directory; `steam_appid.txt` is there, so Steam (running) accepts a direct start without the launcher. A second
instance is refused ("Another instance of %s is running").

## Commands that matter here

| Command | What it does | Checked |
|---|---|---|
| `game_startup_mode <battle\|campaign\|campaign_edit\|frontend\|naval> opt:<xml name> opt:<xsd name> opt:<building list>` | the mode the application starts in; `battle <xml>` loads that battle at once, no menu (default xml: `data/TestData/world/DemoLandBattle.xml`); the xml is a VFS path, found in packs and in working directories | yes: the benchmark's XML from data.pack, and a loose XML (`script/wce/wce_launch.xml` under a working directory), both straight into the battle |
| `mod <file name>` | adds a mod pack to the start-up list (the launcher writes `mod "<pack>";`) | yes: `mod "tdd_pack1_main_v1.1.0.pack";` ... loaded TDD (its units on the loading screen); `used_mods.txt` untouched |
| `add_working_directory <directory name>` | "redirect the non-pack (and mod pack) file search": loose files (the launcher adds `assembly_kit/working_data/`); an absolute path with `/` and a trailing `/`, quoted, works | yes for a file no pack has; against a mod pack's file of the same path: open (below) |
| `docudemon` | writes `docudemon.txt` (UTF-16) in the game folder: every tweak with its default, the command library, Lua commands; the game then starts as usual | yes |
| `import_all_mods`, `exclude_pack_file <pack>`, `set_pack_file_precedence <a> <b>`, `load_pack_files <true\|false>`, `show_paths`, `audit_vfs` | the VFS's pack list and search paths | no |
| `quit` | quits | no |

Tweaks are set the same way (`<name> <value>;`): `show_frontend_movies false;` was accepted (the battle started; movies are not
played in battle mode anyway), `gfx_fullscreen` (an "environment variable" kept in `scripts/preferences.script.txt` as
`gfx_fullscreen false;`, bool words), `win_on_quit_battle`. The launches did not rewrite `preferences.script.txt`.

## Launch results (2026-10-27)

1. Vanilla, `docudemon; game_startup_mode battle <benchmark xml>;`: the battle loaded, then crashed in the NVIDIA driver
   (`nvwgf2um.dll+0xa4473b`, a copy loop, no `crash_report`).
2. TDD by `mod` arguments, loose XML with TDD units: the same crash late in loading.
3. As 2 with vanilla units, after moving `@tdd_dev_buildings_25_10_2026.pack` (a **movie-type** pack, which loads whatever the mod list
   says) out of `data\`: loaded and played, clean exit. The crash was that stray pack (the toolkit's known signature), not the command line.
   A loose `.environment` (red overlay, magenta sky) at the path of a TDD pack's file, named by `<environment_key>`, did **not** show:
   either the mod pack beat the loose file, or the battle does not take its weather from `<environment_key>`.
4. **Loose against a mod pack:** the Londinium XML loose at `script/foi_fords_of_isen/foi_battle.xml`, a path `tdd_pack1_main` also has
   (TDD's Fords of Isen battle, the same format): the game loaded Fords of Isen. **A mod pack's file wins over a working directory's.**
   Loose files serve only paths no pack has.
5. **`mod` with an absolute path** (a pack outside `data\`, listed first): the game stopped at start-up on a breakpoint exception
   (`0x80000003`, `Empire.Retail.dll+0x1637eac`, the engine's fatal-error stop; the crash report's stack is all "unknown"). Mod packs
   must be in `data\`, named by file name.

TDD's `foi_battle.xml` has a commented-out `<battle_map_definition><name>terrain/tiles/battle/assembly_kit/battleofthefivearmies/battlefield</name>`:
a single tile named as the map, without a `tile_map_position`. Untested; if the engine accepts it, a tile painted on no map can be launched.

`game_startup_mode` is not in docudemon's command library (it lists the commands of a running game); the start-up commands are in the
dll's strings next to their help texts.

## The battle XML

The benchmarks' format (`benchmarks/londinium.benchmark` is UTF-16 `<benchmark><historical>script/Benchmarks/.../X.xml</historical>`,
pointing at the XML). Root `<battle>`:

- `<alliance id="n">`: `<army>` (`<faction>`, `<deployment_area_alliance_index>`, `<deployment_area>` of `<boundary type='standard
  additive'>` points, `<unit script_name>` with `<unit_type type>`, `<position x y>`, `<orientation radians>`, `<width metres>`,
  `<unit_experience level>`, `<assault_equipment>`, `<building_mountable_artillery>`), `<victory_condition>`, `<starting_tickets>`,
  `<rout_position>`, `<deploys_first>`.
- `<battle_description>`: `<battle_script prepare_for_fade_in>` (a Lua file beside the XML; optional), `<time_of_day>` (day / night),
  `<season>`, `<precipitation_type>` (dry / rain / snow), `<type>` (`settlement_standard`, ...), `<duration>` (s),
  `<timeout_winning_alliance_index>`, `<battlefield_owner_alliance_index>`, `<subculture>`.
- `<fortification_wall_damage>`, `<fortification_building_damage>` (0-1), `<fortification_wall_breaches>`, `<boiling_oil>`.
- `<weather>`: `<prevailing_wind x y>`, `<environment_key>` (an `.environment` path).
- `<Climate><name>`, `<Season><name>` (`season_autumn`), `<skip_deployment>`.
- `<battle_map_definition>`: `<name>Terrain/battles/<map>/</name>`, `<tile_map_position x y>` (fractions of the map's tile map:
  Londinium 0.152559, 0.22547 on `main_attila_map`), `<tile_upgrade>` (`level1`, `escalation3`: tile_upgrades.xml keys).
- `<playable_area dimension="1924">`.

Other names in the dll near it: `next_battle`, `intro_movie_file`, `gate_opened`, `placed_deployables`, `is_tutorial`.
