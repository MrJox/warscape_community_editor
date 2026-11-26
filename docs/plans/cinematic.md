# Cinematic: scenes, movies and historical battles

One editor, one scene model, two outputs: a **game-ready movie** (`.ca_vp8` with its audio event and subtitles) rendered offline from
the editor, and a **historical battle** (battle XML, Lua, DB rows, loc, sounds) that the game plays. Plus a movie viewer and editor for
the game's own movies. Everything on a timeline is keyed: moving the timeline gives the same picture every time.

State (2026-11-22): decided with the user 2026-11-20 (the grilling, decisions below); static spikes done the same day (findings
below). Built 2026-11-20 .. 11-21 in master (feature `cinematic`, in the default features, not in the `release` set): the video
layer, the movie viewer, the scene editor and its preview, both exports, the logic graph and movie editing (steps 1.10 to 9), then a
UX pass on the Tile tool's model and Launch in game. Left: every in-game check, the items each step lists, and the Tile tool's other
tools below (3 to 8).

## Decisions (user, 2026-11-20)

| Area | Decision |
|---|---|
| Feasibility | Both outputs from one editor: a movie reproduces the timeline frame for frame; a battle reproduces the camera and the timed events exactly and the units as orders (the engine simulates them). |
| Battle gap | Every key is allowed. Keys a battle can't express are flagged per track ("movie only", "approximate"); a battle-preview mode shows what the game would do. |
| Cindy | A time-boxed reverse-engineering spike of the engine's `cindy_playback`; fall back to orders and teleports. (Spike: parked, see 1.5.) |
| Historical battle | The **full playable battle** in v1: setup, cutscenes, objectives, AI, reinforcements, advisor, victory conditions. |
| Battle logic | A **node graph** (events -> conditions -> actions) compiled to Lua on Attila's own battle libraries. Canvas: **imnodes** (MIT) through a Rust binding for dear-imgui-rs, in `THIRD_PARTY.md`. |
| Timelines | Each cutscene is its own keyed clip; the battle logic plays clips on events. A movie is one clip rendered out. |
| Camera | The editor's own spline, baked on export into short `cam:move_to` segments (no need to match the game's easing). |
| Battle map | A position on the campaign map (`tile_map_position`), as Launch in game and the Map context mode do. |
| Units | A key picks a fragment slot (STAND, WALK, RUN, CHARGE, ATTACK, DIE, CELEBRATE...) for the whole unit, each man at his own phase. Moving along a path picks the gait from the speed needed, using each clip's ground travel (no foot sliding); an explicit slot key overrides. No matched combat in v1. |
| Men per unit | Free in movies. In battles: `kill_number_of_men` ("start with N men", only reducing), checked in game; flagged "movie only" if it fails. |
| Music | Music keys in cutscenes, and they may also take over during play (stop the dynamic music, play a chosen event, give it back). |
| VFX | Instance placement is movie only (no battle Lua places VFX); fidelity follows the VFX engine (weather_current_state.md). |
| Weather | Keyable (weather and time of day over a scene): movie only, flagged for battles; a battle takes the value at the start of its intro clip. |
| Movie | Rendered offline by the editor, **1080p30** by default (as vanilla); other sizes and rates once checked in game. Encoded with **libvpx** (BSD), built from source like aoTuV. |
| Movie look | Depth of field (the game's `depth_of_field` shaders, keyed focus), **motion blur** (accumulated sub-frames: no game shader, a deviation the user approved 2026-11-20), letterbox and fades (keyed). No extra AA asked. |
| Movie audio | A `Play_Movie_<name>` Wwise event (vanilla's are in `global_movies.bnk`): needs the Audio plan's bank writer, which comes first. |
| Movie tool | View a movie with its audio and subtitles (scrub, frame step); edit subtitles (retime, rewrite, export); replace audio; import external video through a user-installed **ffmpeg run as a separate process** (nothing bundled or linked); without ffmpeg, PNG sequences + WAV. |
| HB menu | Exports appear in the game's Historical Battles menu (DB rows, loc, screenshot, map location), plus a one-click test launch. |
| Languages | Text per language in the scene, English first; missing languages fall back to English on export. |
| Save | Diffable text in `<projects>/cinematics/<name>/` (scene, clips, node graph), references by DB key and pack path, no game assets copied; autosave as the other tools. |
| First milestone | The movie viewer (after the bank writer). |

## What the game has (Attila, checked 2026-11-20)

### Historical battles

- 11 vanilla battles, each a folder `script/<xx_name>/`: `<xx>_battle.xml`, `<xx>_start.lua`, `<xx>_cutscenes.lua`, often
  `<xx>_main.lua` and `<xx>_declarations.lua`, an optional `<xx>.environment`, `screenshot_small.png`.
- **Battle XML** (UTF-8 or UTF-16 LE, both shipped): `alliance` > `army` (`faction`, `deployment_area` centre/width/height/orientation,
  `unit`s with `unit_type`, `position`, `orientation`, `width` in metres, `unit_experience`, `general` with a name id and
  `star_rating`, `unit_capabilities`), `victory_condition`, `rout_position`; `battle_description` (`battle_script` with
  `prepare_for_fade_in`, `time_of_day`, `season`, `precipitation_type`, `type`, `duration`, `timeout_winning_alliance_index`);
  `sea_surface_name`; `weather` (`environment_key`, a weather file or the battle's own `.environment`, and `prevailing_wind`);
  `battle_map_definition` (vanilla: all `terrain/battles/<name>/`); `playable_area`; `loading_screen_title_override`.
  No men count anywhere.
- **Menu**: `battles` (`key`, `type napoleon_historic`, `specification` = the XML's path, `screenshot_path`, `intro_movie`, `year`...),
  `historical_battles_ui_locations` (`key`, `x`, `y` in pixels on the frontend map, `height_percent`), loc
  `battles_localised_name_<key>`, `battles_description_<key>`, and the loading screen title's
  (`scripted_objectives_localised_text_...`).
- **Libraries** (`script/_lib/`): `lib_battlemanager`, `lib_cutscenes`, `lib_generated_battle`, `lib_objectives`,
  `lib_script_ai_planner`, `lib_patrol_manager`, `lib_zone_manager`, `lib_teleport_manager`, `lib_script_unit`, `lib_timer_manager`,
  `lib_script_messager`, `lib_misc_battle`... The node graph compiles to calls on these.

### Cutscenes (`lib_cutscenes.lua`)

- `cutscene:new(name, player_army_uc, length_ms[, end_callback])`; `:action(fn, ms)` queues a call at a time from the start; `:start()`.
  Options: skippable with a skip callback and skip camera, music (`set_music(event, fade_in, fade_out)`), cinematic bars, restore
  camera, release the player's army, suppress unit voices.
- **Actions run on the battle tick**: `time_increment_ms = 100` (`lib_header.lua`). An action is due when its time is within the next
  tick, so times are quantised to 100 ms: a baked camera segment is at least one tick.
- **`wait_for_vo()` / `wait_for_advisor()` shift every later action** by 500 ms steps while a sound plays: an exported clip must never
  use them, or its timeline stops being keyed.
- Camera (the battle camera's Lua methods, table at RVA 0x1d22ea0): `look_at`, `move_to`, `position`, `target`,
  `enable_functionality`, `disable_functionality`, `change_depth_of_field`, `fade`, `change_height_range`, `enable_shake`,
  `disable_shake`, `enable_anchor_to_army`, `disable_anchor_to_army`.
  - `move_to` (RVA 0x1abaa0): 3 to 7 arguments: position, target (vectors), duration (s), a flag (the scripts' "blend"), the field
    of view in **degrees** (multiplied by pi / 180; default -1, read as "keep"), then two more numbers, default -1, not known. The
    vanilla scripts pass 5 (fov 33 to 45); the library's skip and restore pass `false, 0, 0, 0`. The flag is `true` in every move of
    vanilla's cutscenes (cuts of duration 0 and long moves alike), `false` only in the hand-over to the battle camera at their end.
    The export used `false` on its short steps and the user saw the camera judder and leave the editor's path in game
    (2026-11-21): the steps are `true` now, 0.2 s long (CAMERA_STEP_MS). In game: **todo** (the user).
  - `change_depth_of_field` (RVA 0x1abe60): a duration alone, or duration, focus distance, focal length, blur size. So depth of
    field can be keyed in battle cutscenes too, not only in movies.
  - `fade(out, seconds)`.
- Subtitles: `cutscene:subtitles():set_text(key)` / `clear()`; the key is a `scripted_subtitles` row (`key`, `character_for_vo`), its
  text the loc `scripted_subtitles_localised_text_<key>`.
- Sounds: `new_sfx("<Wwise event>")`, `cutscene:play_sound(sfx)`, `stop_sound(sfx)`.
- Units: script units (`script_unit`, unit controllers) take orders: `goto_location`, `goto_location_angle_width`,
  `teleport_to_location`, `teleport_to_start_location_offset`, `halt`, `start_taunting`, `set_visible_to_all`,
  `set_invisible_to_all`, `kill`, `take_control` / `release_control`.

### Engine calls (checked in the vanilla libraries or `empire.retail.dll`)

- `unit:kill_number_of_men(n[, bool])` and `unit:initial_number_of_men()`: called by `lib_misc_battle.lua` and
  `lib_firestorm_manager.lua`, so Lua can use them. How the dead men look at a battle's start is not seen yet.
- Music during play: `bm:set_music_auto_playback(false)`, `bm:play_music(event)`, `bm:play_music_custom_fade(event, s)`,
  `bm:stop_music()`, `bm:stop_music_custom_fade(s)`, then auto playback back on (`lib_cutscenes.lua` does exactly this).
- **Cindy**: the engine has CA's in-game cinematic editor (`EmpireBattle\Source\CinematicEditor\CinematicEditor.cpp`: scene keyframes,
  `keyframe_sequence`, point-light shadows, recording LODs) and the battle Lua methods `cindy_playback`,
  `cindy_playback_no_camera`, `stop_cindy_playback`, `stop_cindy_playback_no_camera` (registration table at RVA 0x1d231b0;
  `cindy_playback` at RVA 0x1cd600: 2 to 4 arguments, the first a string, then flags read as `> 0`, queued as a
  `BCQ_CINDY_PLAYBACK` command logged as `cindy_playback %s %s %s`). Attila, its Assembly Kit and TDD hold **no Cindy scene file**:
  nothing to learn the format from.
- No battle Lua call that places VFX was found (none in the libraries; no `vfx` / `composite_scene` method name in the DLL).

### Movies

- `movies.pack`: 153 `.ca_vp8`: 19 at 1920 x 1080 (the intro and the seals), 134 at 380 x 214 (one at 213; the campaign event
  pictures, `CAMPAIGN_ESTABLISH`); all 30 fps. No audio inside. TDD's `tdd_intro` is 1920 x 1080, 2,880 frames.
- The container, as rpfm_lib's `files::video` reads it (`ca_vp8.rs`): `CAMV`, version u16, header length - 8 u16, `VP80`, width
  and height u16, ms per frame f32, a u32 always 1, the frame count (or count - 1), the frame table's offset, the frame count, the
  largest frame's size; when the two counts are equal, 9 more bytes (u8, u32, u32); then the VP8 frames; then the table, per frame
  offset u32, size u32 (and a third u32 in 13-byte entries), key frame u8.
- Survey of all 154 (2026-11-20, `tools\camv_survey.py` in the scratch folder): one layout only: version 0, header length field 32
  (data at 40), count - 1, no extra bytes, 9-byte entries, frames contiguous in table order, every table key flag equal to the VP8
  frame tag's, the largest-size field right. rpfm_lib's writer, simulated on each, gives **every file back byte for byte**. So the
  editor uses rpfm_lib's `Video` (already a dependency) and needs no parser of its own; a gate confirms it with the real code. The
  writer is not lossless for the variants Attila doesn't use (13-byte entries lose their third u32, out-of-order frames are
  rewritten in order): a fix to port to rpfm_lib if another game has them.
- `videos` (`video_name`, `video_type` FULLSCREEN_INTRO / CAMPAIGN_ESTABLISH / FULLSCREEN_FRONTEND, `audio_tracks`, `script_ref`);
  `movie_event_strings` (event, culture, movie, id).
- Audio: the Wwise event `Play_Movie_<name>` (the engine's prefix), vanilla's in `global_movies.bnk` (`music.pack`). Each plays a
  switch container on the speaker setup: `stereo` (a 2-channel WEM), `surround` (a 5.1 one) and `None` (the surround one again),
  both 48 kHz and as long as the movie (intro: 173.3 s for 171.7 s of pictures); the SEGA sting plays one stereo sound. 34 of the
  388 movies the game loads have sound; 27 have subtitles (`movie_catalog_check`).
- Subtitles: `subtitle_timings` (`language`, `subtitle_field`, `text_section`, `foreign_key` = the movie, `start`, `end`,
  `script_id` = the video's `script_ref`, `text_id`), text in `vo_fmv_texts.loc`: one entry per movie
  (`vo_fmv_texts_text_<movie>`), its sections separated by `|`, section *n* shown from `start` to `end`, in **frames** (30 fps):
  in all 17 movies of `movies.pack` with subtitles the last `end` is 90 to 95 % of the frame count (intro 4,709 of 5,152); read as
  tenths of a second they would run past the movie. The other 10 subtitle sets are the Belisarius and Charlemagne campaigns' movies,
  not in `movies.pack`.
- TDD's own: `intro.pack` (`movies/tdd_intro.ca_vp8` + a `videos` row), and the campaign flybys (`*_flyby.pack`: faction intro Lua
  + advisor `.wem` lines + advice rows): campaign camera scripts, a later output if wanted.

## Crates

- `wce_cinematic`: `movies` (the catalog: movies, `videos` types, the sound event's versions, subtitles by language); the headless model: project, clips, tracks, keys, capability flags, `evaluate(clip, t)` as a pure function, the
  node graph, the compilers (battle XML, Lua, DB rows, loc), save/load.
- The CAMV container: rpfm_lib's `files::video::Video` (see Movies); no parser of our own.
- `wce_video`: libvpx 1.16.0's VP8 encoder and decoder, vendored as plain C in `third_party/libvpx` (`scripts/vendor_libvpx.py`
  writes what libvpx's configure would for a generic C target: no assembler, gcc or make needed) and built by `build.rs` with cc;
  a C shim (`src/shim.c`) keeps libvpx's structs out of Rust. `Decoder`, `Encoder` (one pass, constrained quality, alt-ref
  look-ahead), `I420` <-> RGBA (BT.601 video range, **ASSUMPTION** for the game's movies), `Movie` (rpfm_lib's `Video`; new movies
  go through IVF, and 30 fps is written as vanilla's 33.333 ms).
- `wce_cinematic_tool`: the UI (timeline, viewport, node canvas, movie viewer), publishing into the shared scene like every tool.
  The node canvas: `dear-imnodes` 0.20.0 (MIT OR Apache-2.0; imnodes itself MIT), the same project and version as the
  `dear-imgui-rs` 0.20 the editor uses; added when step 7 starts.
- Reused: `wce_audio` (the timeline player, mixdown, bank writer), `wce_render` (units, weather, the frame), Map context, Launch in
  game, `wce_weather` (environments).

## Steps

Status legend: **done**, **in progress**, **partial**, **todo**, **parked**, **blocked** (with why). Every step keeps its status here,
updated as work lands.

Scratch work (not in the repo, may be lost on a reboot; the findings that matter are recorded above):
`%TEMP%\claude\c3aa9bd9b1039f6c\235db9f9-a566-4b6b-8909-603929d37216\scratchpad\` — `hb\` two vanilla historical battles, `lib\` the
battle libraries, `tsv\` and `loc\` / `locall\` the menu, movie and subtitle tables and loc, `mv\` one movie.

0. **Prerequisites.** **partial**
   1. The Audio plan's bank writer (new events in a bank, `global_movies` override). **done** (audio.md steps 4 to 8)
   2. Units step 8 (close-ups) only as the scenes need it. **todo**
1. **Spikes.** **partial**
   1. The Historical Battles menu: tables, loc, screenshot (above). **done** (static); an exported battle seen in the menu: **todo**
   2. The cutscene library: actions, ticks, camera, subtitles, sounds, unit orders (above). **done**
   3. `kill_number_of_men` callable from Lua. **done** (vanilla libraries call it); what it looks like at a battle's start: **todo**
      (in game, with the user)
   4. Music during play (above). **done** (static); heard in game: **todo**
   5. Cindy: the Lua methods and their arguments found; no scene file in Attila, its kit or TDD. **parked**: resume only if a sample
      turns up (another Warscape game's Cindy scenes, a CA leak of the format); until then battles use orders and teleports.
   6. A `battles` row whose XML uses `tile_map_position` (vanilla's all use `terrain/battles/<name>/`). **todo** (in game)
   7. `cam:move_to`'s arguments. **partial**: fov in degrees, -1 keeps; the last two numbers not known (they stay -1 on export).
      Found on the way: `change_depth_of_field` (above).
   8. The CAMV layout against rpfm_lib, a byte-for-byte round trip. **done** (simulated in Python on all 154 movies, see Movies);
      the same check on rpfm_lib's own code is step 2's gate.
   9. An imnodes binding for dear-imgui-rs 0.20. **done**: `dear-imnodes` 0.20.0 (see Crates).
   10. libvpx built from source; licence in `THIRD_PARTY.md`. **done** on Windows (MSVC, 2026-11-20; the source fetched from
       chromium.googlesource.com at tag v1.16.0 under the user's "run unattended"); Linux and macOS not built yet (plain C, no
       platform code but `vpx_pthread.h`'s, so expected to build).
2. **Movie viewer.** **done** (2026-11-20) but for listening: the Cinematic tool (`wce_cinematic_tool`, feature `cinematic` of
   `wce_app`, in the default set): Movies (every `.ca_vp8` by `videos` type, a filter) and Movie (the picture fitted, subtitles at
   the bottom centre, play / pause, frame steps, the scrub bar, the subtitle language, the sound version, volume). Live: a worker
   decodes, the sound drives the clock while it plays (the Audio tool's SDL player). Scripted: the fixed step drives the viewer's own
   clock (the editor's ticks only with a viewport) and frames decode on the spot. Commands `movie_open`, `movie_seek`,
   `movie_play`, `movie_pause`, `movie_language`, `movie_info`, `assert_movie <frame> [text]`.
   Gates: `video_check` (all 388 movies of Attila with its DLC give their bytes back through rpfm_lib's own code and decode, 67,694
   frames: every frame of the small ones, the first 120 of the large; a synthetic clip encodes, is written as CAMV and decodes back
   at 51 dB luma PSNR), `movie_catalog_check`, `scripts/cinematic_check.sh` (render: two scripted runs, identical shots).
   Not checked: the sound by ear and its sync with the pictures in live playback (user).
3. **Scene model and timeline.** **done** (2026-11-20). Model: `wce_cinematic::scene` (project, map placement, battle
   setup, armies, units, clips of tracks: camera, unit (path, actions, men, visibility), sound, music, subtitles, VFX, fade,
   letterbox, depth of field, weather; texts per language; the logic graph), `project.json` as pretty JSON. `eval::evaluate` is a
   pure function of the keys (Catmull-Rom with the keys' times as knots; the gait from the speed along the path by each unit's own
   walk / run / charge speeds, `speeds::speed_table` from `main_units` -> `land_units.man_entity` -> `battle_entities`; the slot's
   start and the ground covered). `support::track_support` flags each track for battles (exact, approximate, movie only).
   Tool: the scene is a document (`scene_doc.rs`: undo / redo / History / Save, `<projects>/cinematics/<key>/project.json`); panels
   Scene (setup, armies, units, texts), Timeline (clips, play / scrub, tracks with a support dot, keys selected, dragged, added at the
   playhead holding the value there), Key (the selected key's values; replaced by the Inspector in the UX pass), Layout (the field from above at the playhead; Ctrl+click keys
   the selected unit's path). File > New / Open cinematic scene. Autosave (2026-11-21): a scene's
   `project.json` is copied and taken back (`Tool::restore`), unsaved until saved. Texts in the game's nine languages (Scene >
   Texts' language; the Key panel and the exports follow it, English standing in). Clips are deleted (a scene keeps one); not
   reordered.
4. **Scene preview.** **done** (2026-11-20, 2026-11-21): `preview.rs` draws the scene in the viewport at the playhead: the battle's ground
   (`ground.rs`, see "The Tile tool's other tools" 2; flat grass until it loads), each unit's men in formation (`formation` of `land_units` rank depth and spacing)
   posed by the game's animations in the evaluated slot (`UnitModel`, 6 soldiers a unit and slot taken in turn, the slot's time
   from its start), the camera following its keys (Free camera to fly; a camera key holds the viewport's view), the weather blended
   between keyed environments (`set_weather_params`, `blend_environments`). Keyed depth of field through the environment's DOF block (the
   game's DOF constants are `change_depth_of_field`'s values), written to the scene at once (a movie renders many frames in one
   editor frame). A moving slot plays by the ground covered over its clip's own travel speed (no foot sliding). Battle preview
   (the Timeline's Battle, `eval::evaluate_battle`): units teleported to their first key, then under orders at their own walk /
   run speed in straight lines (early or late against their keys), movie-only tracks gone. VFX instances (2026-11-21): the keyed
   instances alive at the playhead as `VfxEmitter` entities, in the preview and in movies; scrubbing back does not rewind particles
   already emitted. The sound while it plays (2026-11-21, `scene_audio.rs`): the clip's sound and music mixed as the movie export
   does and played from the playhead through the Audio tool's player; scripted runs stay silent; not checked by ear.
5. **Movie export.** **done** (2026-11-20; motion blur 2026-11-21: per clip, four sub-frames over a 180 degree shutter) but the in-game test. `movie_export.rs`: `MovieEncoder`,
   `render_clip`, `mixdown` (48 kHz stereo: sound events at their times and volumes, music cues with fades; a switched event's
   stereo version, a random container's first child), `movie_files` (an overriding `global_movies.bnk` with `Play_Movie_<name>` on
   a sound made like the SEGA sting's, streamed from `audio/<id>.wem`; IDs seeded by the name, `EditSet::id_seed`; the `videos` row
   with a `script_ref` after the game's; `subtitle_timings` in frames; `text/db/vo_fmv_texts_wce_<name>.loc`). Tool: File > Export >
   Movie pack (1080p) renders the clip offscreen frame by frame (`wce_editor::viewport::render_offscreen`), lays the fades and bars
   over each picture, encodes, writes the pack (`export.rs`: a few frames per editor frame live, all at once in scripts). Renderer
   fixes for it (`wce_render` skinned draws): parts waited on in fixed-step runs, runs sorted by model path, not address, so a
   movie with units is the same in every run. Gates: `movie_export_check` (a stand-in renderer; the new event resolves over the
   game to the new 3.00 s sound; rows, loc and catalog read back; two exports identical) and `cinematic_check.sh`'s preview run
   (the movie pack byte-identical between runs; `render_offscreen` waits until nothing loads before a frame, else a texture's
   full mips landing mid-movie differed between runs). Encoder defaults (2026-11-21): no alt-ref frames (vanilla has one picture
   per stored frame, and the community's ffmpeg pipeline sets `-auto-alt-ref 0`), 20 Mbit/s at constrained quality 6. Not
   checked in game: the movie playing, its sound, its subtitles.
6. **Battle export: setup and cutscenes.** **done** (2026-11-20) but the in-game test. `battle_export.rs`: the battle XML (armies,
   units with `script_name="wce_u<id>"`, the campaign map position `terrain/battles/<map>/` + `tile_map_position`, the weather);
   `<key>_start.lua` (Battle_Script_Header, battle_manager, `generated_battle` with the first clip as the intro), `_declarations.lua`
   (generated armies `ga_<n>`, script units `SU_<id>`), `_cutscenes.lua` (each clip a `cutscene`: actions on the 100 ms tick grid,
   the camera baked into `move_to` steps of 0.2 s (flag `true`, as vanilla's moves) with the field of view, teleports and `goto_location_angle_width` orders,
   `start_taunting`, `kill_number_of_men`, `new_sfx` sounds, music with the dynamic music paused, `scripted_subtitles` keys, `cam:fade`,
   `change_depth_of_field`, the cinematic bars, a skip that teleports units to their ends; never `wait_for_vo`); movie-only tracks
   reported. File > Export > Historical battle pack. Gate `battle_export_check` (XML parses, tables and loc decode back, the expected
   calls on the tick grid, two exports identical; the toolkit's Lua checker finds every called name in the engine or vanilla). Not
   done: the test launch through Launch in game. Walk / run orders follow the unit's own walk speed (2026-11-21).
7. **Battle logic node graph.** **done** (2026-11-20) but conditions and some events (zones, objectives done, unit below N men).
   `logic.rs`: node types (events: battle starts, a cutscene ends, a clip ends, time, army near the enemy, casualties, rout,
   general dies, seen, wins, loses, a message; actions: attack, defend a spot, release to the AI, advice, play a clip, fade in,
   music, send a message, raw Lua), compiled to `generated_battle` messages (`message_on_*`, `*_on_message`, `gb:add_listener`); an
   empty graph: the enemy attacks and allies are released when the intro ends. Tool: the Logic panel (`logic_ui.rs`) on
   imnodes' canvas through `dear-imnodes-sys` (the C API on the editor's own ImGui context; the safe crate needs a `Ui` the tools
   don't have): nodes with their settings, links by dragging, right-click adds, Delete removes, moves kept on release, the
   compiled Lua shown. Commands `logic_node`, `logic_link`, `assert_logic`; `cinematic_check.sh`'s logic run.
8. **Historical Battles menu export.** **done** (2026-11-20) but the in-game test: `battle_tables.rs` writes the
   `battles` row (copied from a vanilla historical battle's, key, specification, screenshot path, year), the
   `historical_battles_ui_locations` row, the `scripted_subtitles` rows and the loc (`battles_localised_name_`,
   `battles_description_`, the subtitles) in one pack with the scripts. `screenshot_small.png` (512 x 256 RGBA, as vanilla's): the
   viewport's view at export (2026-11-21). Not done: the loading screen title.
9. **Movie editing.** **done** (2026-11-21) but the in-game test. `movie_edit.rs`: a movie's subtitles as `subtitle_timings` rows
   (tied to its `videos` row's `script_ref`, which the catalog now keeps) and its `vo_fmv_texts` loc; its sound replaced in every
   version of its event; new movies from a PNG sequence or any video through an ffmpeg the user has (`WCE_FFMPEG` or the PATH,
   run as a process), with the video's sound. The catalog lets a later table file's row replace one with the same key
   (language, field, section, movie), as the game does. Tool: Movie > Edit subtitles, Subtitles pack..., Replace sound...;
   File > Export > movie or event picture from a video, movie from PNGs. Gates: `movie_edit_check` (the user's ffmpeg 7.1.1:
   a test video imported as 60 event-picture frames with 2.02 s of sound) and `cinematic_check.sh`'s movie edit run.

## UX (user, 2026-11-21)

The user found the first version built for scripts rather than people (keys typed by hand, no visible first step, no obvious way
to fill a scene), then asked for the Tile tool's ways of making and editing things to be reused ("a very nice ref"). All **done**
2026-11-21:

1. Pickers fed from the packs for every field naming game data (units by name and by the factions that field them, from
   `unit_variants` and `units_custom_battle_permissions`; factions, weather files, VFX, battle maps, sound and music events, animation
   slots; armies and clips in logic nodes).
2. The Tile tool's model: the scene's items (`items.rs`: battle, armies, units, VFX instances, camera keys) are the editor's selection
   and move with its gizmo (a unit with a path keyed at the playhead; a drag is one undo entry); a **Create** panel (`create.rs`: unit
   cards, placing by clicks or the Space quick-add, the Inspector showing the draft); a **Scene** panel (`outliner.rs`); the shared
   **Inspector** (`inspect.rs`; the Key panel is gone); the **viewport** (`view.rs`: click and box select, formations in their side's
   colour, the placing ghost, a right-click menu, the shortcuts). Gate: `scripts/cinematic_ux.txt` in `cinematic_check.sh`.
3. The battle's map position picked on a picture (`map_pick.rs`, the Battle's Inspector, "Pick on the map...").
4. The Timeline: a double-click on a track's row adds a key there; Add track lists the units without a track by name.
5. Camera keys as scene items, drawn in the viewport while the camera is free; the viewport's chips (play, free camera, key this
   view, battle preview).
6. No first-run walkthrough text: every empty state offers its next step as a button (New scene / Open, Place units..., Pick on the
   map..., the Timeline's double-click); Logic has an Add a node button.
7. Placing on the press, not the click (the tool owns the left button while placing); the gate drives the real mouse (`click_ground`).
8. Battle > Fought on: a place on a campaign map, one battle tile (`map.tile`, named alone in the battle XML as Launch in game does), or
   a whole battle map (`map.whole_map`, as vanilla's historical battles); in game: **todo**.
9. A sample scene: ten vanilla units in two armies, a camera move, fires, a subtitle, the bars and a fade in; since 2026-11-23 a
   data file, not code (see 19).

## The user's second round (2026-11-21)

1. The free camera stuck: not reproduced (scripted W and the wheel move it over Catalaunian Plains). Likely causes found: the
   camera put back on its keys every frame while "Free camera" was off (gone, 7), placing holding the left button (Esc stops it),
   the editor camera's steps shrinking near the ground (its speed and wheel follow the height above it). **partial**: the user's
   case to confirm.
2. Key the camera at the playhead (Timeline button, the viewport's camera chip, the Create panel's Camera tab) and **recording**:
   the clip plays from the playhead, the viewport's camera keyed every half second (the keys it passes replaced, one undo entry),
   stopping at the clip's end. **done**
3. Camera paths: the path drawn as the camera takes it (the keys' curve sampled); each key's target a point of its own (picked,
   moved by the gizmo); the key's Inspector: duration and speed to the next key (the later keys move with it), Insert a key after,
   Look along the path; a new path by clicks (Create > Camera > Camera path: a key per click, `cam_height` above the ground,
   `cam_step` seconds apart, each looking on to the next). **done**
4. The Layout over the ground: a top-down orthographic render of the scene at the Layout's centre and scale under its marks,
   again when the view moves and every 1.5 s. **done**
5. The ground's textures: the ground between and beyond the tiles took the default climate (Continental) everywhere; it takes
   the climate of the tile at the battle's spot now (the tiles had theirs). **done**
6. A black screen in game: the battle starts black (`prepare_for_fade_in`) and only faded in when the cutscene ended; a fade track
   starting black also put "black at once" after its own fade-in at tick 0. Every clip now fades in first thing, as vanilla's
   intros (`cam:fade(false, 0.5)` at 0), unless its fade track starts black (then black first, its fades after).
   `battle_export_check`. **done**; in game: **todo**
7. The camera free by default; it follows the keys only while the clip plays (not while recording) or with Look through. **done**
8. No music: Attila's battle music is interactive (`game_music_play` plays a switch tree; the `music_set_*` events only set its
   states), so a cue on a `music_set_*` event played nothing. Music cues are now **moods** (`music.rs`: the state-setting events,
   by group: Faction, Intensity, Battle, Key), the editor renders the branch the moods lead to (the decision trees walked with the
   moods' states in the game mode Battle, the playlist's segments in turn, each segment's tracks mixed); the battle plays the
   mood events (`new_sfx` + `play_sound`), its music system following. Gate `music_check`. **done**; in game: **todo**
9. Each track an on / off eye before its name (off: left out of the preview, the movie and the battle; kept). **done**
10. Listening: the sound picker's rows each a play button, the key's play button, the music key's Listen (20 s of its moods). **done**
12. The viewport shows what the movie lays over its pictures: the fade, the cinematic bars (an eighth of the height each), the
    subtitle at the bottom (the language edited). Play at the clip's end starts it over (the Timeline's button and the viewport's
    chip alike). **done**
13. Accuracy against the game (user, 2026-11-21). **done**, in game: **todo**:
    - The grass: the map context's tiles had their trees but not their grass (the Tile tool draws only the open tile's). The context
      request has `grass` (the Cinematic tool's ground only): each tile's grass generated as the Tile tool's (waited for on the
      worker reading it), its patches moved into the map's frame and lifted (`Frame::place_entities`), cached with the template
      (`cache` kind 6; the key unchanged without grass). The renderer draws the grass sets in an order of their own (model, place),
      not the scene's: the chunks arrive as the workers finish them and the grass is blended (movies differed between runs).
    - The camera's remaining judder: each 0.2 s step ended exactly at its point, so the speed broke at every step; a moving step now
      aims at the point two steps on and takes as long, the camera chasing a point just ahead on the path (not across a cut).
    - The battle camera's limits: `battle_cameras` (land: 2 to 90 m over the ground, the game's battle camera). The preview holds the
      keyed camera within them (the Timeline's Game limits, on by default; off: as a movie shows it), the camera's path is red where
      it leaves them.
14. Accuracy against the game, second round (user, 2026-11-21). **done**, in game: **todo**:
    - The trees: the map context's tiles were taken before their trees were generated (generated on a thread of their own since
      92227867, the template did not wait), and cached without them: only the tiles' static trees were drawn (3.5k on the sample's
      map, none near the armies). The template waits for them (`trees_now`, as the grass); cache version 15. The sample's map: 419k
      trees and bushes, 7.8k within 500 m of the armies (`ground_status`). The Tile tool's map context had lost them as well.
    - The camera's jolt at every key: the steps started again at each key, leaving a short one before it (and the chase ahead
      changing pace there). Through a run of smooth keys the steps keep one beat (every 0.2 s of the clip), the samples passing
      through the keys; a key is a stop of its own only where a cut or a straight move begins or ends. The field of view follows the
      same curve as the position (it stopped at every key). Gate `battle_export_check` (the sample's beat).
    - How the battle camera moves between two `move_to`s (its easing, what the flag does) is still not known: `move_to` (and
      `look_at`) only queue a command object; its handler is behind the command dispatcher. In game: **todo** (the user).
    - The Layout picture: the last one stays (reprojected) while the camera moves, a new one rendered after 0.25 s still and swapped
      in when it is uploaded (it was dropped and rendered again every move: flicker).
    - The subtitle in the viewport: the game's `subtitle` font category (`fontcategories.fc`: la_gioconda 22, white, no shadow)
      drawn from its `.cuf`, scaled with the picture's height (1080 lines).
    - The camera path is red outside the battle camera's limits only where the ground's tiles are read (none while they stream in).
    - Movies the same every run with the trees: the clouds' shade drifts with every frame drawn and the wind simulation runs on, and a
      movie's first frame came after however many frames its loading took (`wait_loaded` waits a frame at a time); a restart of the
      temporal state (`Renderer::restart_temporal`: movies, captures, thumbnails) starts both again. A tile's trees generated afresh
      come in another order each run: fixed-step renders draw each batch's instances in an order of their own (`DrawSet::stable`, by
      where they stand), and wait for the models of a streamed chunk's entities as a full build does (`update_draws`). The gate's
      preview scene is on the plains tile (`scripts/cinematic_preview.txt`): on the default map its camera was under the ground, over
      the sea; there run 2's movie still came out a shade darker than run 1's from frame 48 (not found why; a camera under the map).
15. The enemy in game (user, 2026-11-21: line of sight hid it). Each clip shows the other side's armies through the fog of war while it
    plays, as vanilla's intros do (`ga:set_visible_to_all(true)` at 0, `false` in the clip's end callback and its skip). **done**, in
    game: **todo**.
16. Edited movies (user, 2026-11-21: vanilla's movies are shots cut together, as a trailer). **dropped** (user, 2026-11-23): WCE
    exports clips (item 17's files, a file a shot); the cutting is done in a video editor, as the community's trailers are made.
17. A tester's first impressions (user, 2026-11-23), after Unreal's Sequencer:
    - Formation width: the preview laid the men out with the unit's rank depth and never its Width (1000 men in ranks 250 wide).
      The men stand as wide as Width (`unit_catalog::ranks_for_width`), the Layout's box as deep as they stand. **done**
    - Animations: the action keys offered a fixed list of 24 slots, most of which no unit has (`TAUNT` is `TAUNT_1` to `_4`,
      `CELEBRATE` `_1` to `_10`; `CHEER`, `SALUTE`, `DIE`, `PANIC`, `PRAY` none): the men stood, silently (the sample's cohort
      too). The keys offer the unit's own slots (its animation tables', read as its men load: a cohort's 1297), by family and
      group (Idle, Movement, Combat, Death, Morale and gestures, Other), searchable; a family's variants spread over the men
      (not in step) or one picked (the key's Variant). A key whose animation the unit hasn't is red on the Timeline and says
      so. A rider plays his own (`RIDER_`) slots (the movement's gaits the horse's: walk, canter, gallop); his list is those.
      In a battle only a taunt plays (no Lua call plays a slot; `start_celebrating` exists in the engine, not used: untried);
      the key's tooltip says so. **done**
    - Cameras: a camera is an object (a Camera track): drawn at the playhead with the frame it sees (16:9; the live one
      green), selected in the viewport or the Scene panel's Cameras, moved and turned by the gizmo (keyed at the playhead), its
      Inspector the name, focal length (mm, full frame) and field of view, place and aim at the playhead, Look through it,
      Key this view into it, Cut to it here, Delete. New cameras from the view (Scene panel, Create > Camera, Add track). Key,
      Record and the click-a-path tool write into the camera in focus (selected, else the Timeline's, else the live one).
      **done**
    - Shots: the Camera Cuts track (Unreal's): from each cut the camera it names is live; the Timeline shows the shots in
      their cameras' colours; the preview, the movie and the battle follow it (a cut an instant `move_to`, each shot's path
      baked as before). The sample has two cameras and three shots. **done**, in game: **todo**
    - Export for editing: Export > Movie's format: the game's movie, a PNG sequence with a WAV, MP4 (H.264) or ProRes 422 HQ
      (MP4 and ProRes through the user's own ffmpeg: on the PATH or `WCE_FFMPEG`; greyed without it); the clip whole or a file
      a shot (`<name>_010_<camera>`...) with handle frames seen through the shot's own camera; the fade and the bars on or off.
      Gate: `scripts/cinematic_cameras.txt`. **done** (MP4 and ProRes: the ffmpeg command tested, not run here: no ffmpeg on this
      machine)
18. A second sample (user, 2026-11-23): two men fighting, the camera fixed on them: a Roman soldier and a Hun warrior, sword and shield both, face to face on the flat plains
    tile, trading attacks, blocks and a shield bash (each key a slot both men have), one camera from the side. **done**
19. The editor and its data apart (user, 2026-11-23: no menu entries or buttons for particular example scenes). The samples are
    scene files of the repository, `samples/cinematic/wce_sample_battle` and `wce_sample_duel` (`project.json` each), opened as any
    scene: File > Open scene file... (a scene's `project.json` anywhere), or copied into `<projects>/cinematics` to be listed. The
    code that built them (`wce_cinematic::sample`, `examples/sample_scene.rs`), the File menu's and the empty Scene panel's sample
    entries and `scene_sample` are gone; the gate copies the samples into its throw-away projects folders and opens them by name
    (`scene_open wce_sample_battle`), and `battle_export_check` reads the battle sample's file. **done** (not run since: the user
    asked for no verification run after this change)
    Then (user, 2026-11-23): opening the duel's file from File > Open scene file... crashed (ImGui's draw list splitter assert,
    `imgui_draw.cpp:2260`: the scene opened inside the menu); every open from a menu or a panel now waits for the next frame. The
    scene lists show the samples shipped beside the program (`samples/cinematic` there or above it), marked; one opened is copied
    into the projects folder first. **done**
20. Matched combat and no hard switches between animations (user, 2026-11-23). **done** (editor and movies; a battle plays none of it)
    - A unit's run of action keys (back to the last `AUTO`, on to the next) plays as one sequence (`UnitModel::play_sequence`): each
      key's animation for its time exactly, blending in from the one before over its fragment's `blend_in_time` or at least 0.15 s,
      its last frame held after its end. Before, each key loaded a looping model of its own and the men swapped to it at once.
    - A key may name an animation file of the packs (`file:<path>.anim`: a matched combat's clip, its `.meta` beside it), and place it
      (`ActionKey::place`: x, z metres and a turn, in the unit's frame; the key's Inspector row *Place*). A file's clip keeps its travel
      (the root moves as authored); a slot's stays on the spot as before.
    - The game's matched combat: `animations/matched_combat/attila_generated.txt` pairs an attacker's clip (alliance 0) with a
      defender's (alliance 1), each with a start and an end state (`combat_ready`, `attack_a`..`f`, `defend_1`..`6`, `dead`). Both clips
      of a pair are authored in one space (the defender's hips 1.3 to 1.5 m before the attacker's, turned to him), each pair about its
      own origin: chained, a pair is turned and moved so both men carry on where they ended.
    - The duel sample (`samples/cinematic/wce_sample_duel`) is such a chain: both men ready, eight matched pairs of sword and shield
      (the Roman attacks, the Hun counters, they trade, the Hun kills him), each man's end state the next pair's start, the pairs placed
      so neither man moves more than 6 mm or 25 degrees at a change of pair; the camera frames the whole fight from the side. Made
      offline from the game's data (a script outside the editor), as data.
    - Not done: a picker of the matched pairs in the editor (a pair's two keys are set by hand, the second on the other unit).
21. Men less alike, and characters (tester, 2026-11-26: the variants are pulled too uniformly; no officers or standards, nothing for
    characters). **done** (editor and movies; a battle has the game's own)
    - Each man is one of 12 soldiers of his unit (6 before) by a hash of his place, not `k % 6`, which striped the ranks, and a little
      out of step with the others of his soldier (four time offsets). A mesh's tint varies from soldier to soldier by its
      `random_range` (**ASSUMPTION**: brighter or darker by up to 30 %, each channel 10 %, times the range; the engine's use isn't
      traced). The definitions weight an alternative by listing it again, so the pick among them stays even.
    - A unit's command group (`land_units.officers` -> `land_units_officers`: officers, standard bearers, musicians, each a
      `battle_personalities` row with its own variant and animations) stands at its place: before the front rank or behind the last
      (`personality_location`), side by side. *Command group* in the unit's Inspector (on by default; the duel's men have it off).
    - A unit can be one *Character* alone in its place (`SceneUnit::character`): a `battle_personalities` man (`personality:<key>`,
      on his own animations) or a general's battle variant (`uniform:<agent_uniforms.battle_filename>`, on the unit's animations);
      both ride the unit's mount. The picker lists the unit's own command group first. A battle has the unit.
11. Export > Movie pack...: a window with the clip, the size (4K to 960 x 540, or custom), the frame rate, the quality (5.5 to
    40 Mbit/s), motion blur and the subtitles' language. **done**

## The Tile tool's other tools for historical battles (assessment, 2026-11-21)

The official pipeline makes a historical battle's map in TEd (vanilla's `terrain/battles/<name>/`: a tile map of tiles) and its
setup in the battle XML; the Lua is written apart. What the Tile tool has that would serve the scene:

1. **Launch in game** (`wce_launch`). **done** (2026-11-21, `launch.rs`): the battle's pack as `data\wce_launch.pack`, the editor's
   mods after it, `game_startup_mode battle script/<key>/<key>_battle.xml` (its own XML: the start script is named relative to it), the
   Tile and Terrain tools' launch checks, the pack removed when the game exits. Cinematic > Launch in game, the viewport's launch chip,
   `scene_launch [--dry-run] [--ignore-movie-packs]`. In game: **todo** (the user).
2. **The ground** (`wce_map_view::MapView`, the Map context). **done** (2026-11-21, `ground.rs`): the Tile tool's map streamer on
   the battle's map from the packs (`load_map`, the packs' tiles through `PacksTree`, the kits' first), 2 m a scene unit as the
   Terrain tool draws battle maps. A place on a campaign map: the map with its origin at the position; a whole battle map: at its
   centre; a battle tile: a map of its one placement. Read on a worker when the choice has held still a moment (scripts at once,
   streamed to idle); the flat grass until then. The men stand on it, the viewport's clicks find it (`ground_under`), the
   formations, paths and the placing ghost are drawn on it, effects are placed at their height over it, the gizmo is at its height;
   the camera is framed on it when it arrives; the movie streams it around each frame's camera. Camera keys stay absolute heights,
   as `cam:move_to` takes them (Catalaunian Plains' ground is about 460 to 515 m). New Attila scenes and the sample are fought on
   vanilla's Catalaunian Plains map, whole (the campaign map's centre is the Aegean). Gate: `assert_ground_height` on the sample's map
   and a tile (`scene_ground`).

Not built yet, by worth:

3. **Deployment zones and objectives drawn** (the Create panel's gameplay markers and areas, holders' shapes): zones as the battle
   XML has them (centre, width, height, orientation) drawn and dragged in the viewport; capture points and victory locations for the
   logic's zone and objective events (step 7's missing events). High.
4. **Reference image** (`reference_image.rs`): a historical map of the battle laid under the Layout or the viewport, to place the
   armies as they stood. Medium, cheap.
5. **Grab, snapping, measure** (G over the ground, the snap chip, the tape measure and the angle): lines of units at even spacing,
   distances between the armies, camera paths of a known length. Medium, cheap (the editor's chips and gestures).
6. **Selection tools and saved camera views** (select all of an army or a kind, saved selections; `camera_views.rs`). Low-medium.
7. **Prefabs** as formation presets (a triplex acies of N units placed at once). Low-medium.
8. **AI hints and regions** for the logic's "defend a spot" and the AI's behaviour. Medium, needs in-game checks.

Not for the scene: terrain and vegetation painting, building placement and destruction, links, kits, flips: map authoring, which
stays in the Tile and Terrain tools (a map made there is then picked as "a whole battle map").

## Open questions and risks

- Units in battle follow the engine's pathing: arrival times drift from the preview. Teleports under a camera cut are exact.
- Whether the frontend accepts a historical battle on a campaign-map position (1.6).
- The cost of a full playable battle in a node graph: about as much as the cutscene and movie paths together.
- Movie fidelity is the editor's: the sea, vegetation leftovers and VFX (rendering_remaining.md, weather_current_state.md) show
  in every rendered movie.
- Cindy stays unknown (1.5): if a sample appears, units could follow keys exactly in battle.
