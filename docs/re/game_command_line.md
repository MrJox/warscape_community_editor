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
| `game_startup_mode <battle\|campaign\|campaign_edit\|frontend\|naval> opt:<xml name> opt:<xsd name> opt:<building list>` | the mode the application starts in; `battle <xml>` loads that battle at once, no menu (default xml: `data/TestData/world/DemoLandBattle.xml`) | yes: `game_startup_mode battle script/benchmarks/siegeoflondinium/siegeoflondinium.xml;` went straight into the Londinium siege (vanilla, no mods) |
| `mod <file name>` | adds a mod pack to the start-up list (the launcher writes `mod "<pack>";`) | not yet |
| `add_working_directory <directory name>` | "redirect the non-pack (and mod pack) file search": loose files (the launcher adds `assembly_kit/working_data/`) | not yet |
| `docudemon` | writes `docudemon.txt` (UTF-16) in the game folder: every tweak with its default, the command library, Lua commands; the game then starts as usual | yes |
| `import_all_mods`, `exclude_pack_file <pack>`, `set_pack_file_precedence <a> <b>`, `load_pack_files <true\|false>`, `show_paths`, `audit_vfs` | the VFS's pack list and search paths | no |
| `quit` | quits | no |

Tweaks are set the same way (`<name> <value>;`, untested): `show_frontend_movies` (intro movies, default 1), `gfx_fullscreen`
(an "environment variable" kept in `scripts/preferences.script.txt`; 0 would test windowed), `win_on_quit_battle`.

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
