# Cinematic: scenes, movies and historical battles

One editor, one scene model, two outputs: a **game-ready movie** (`.ca_vp8` with its audio event and subtitles) rendered offline from
the editor, and a **historical battle** (battle XML, Lua, DB rows, loc, sounds) that the game plays. Plus a movie viewer and editor for
the game's own movies. Everything on a timeline is keyed: moving the timeline gives the same picture every time.

State: decided with the user 2026-11-20 (the grilling, decisions below); static spikes done the same day (findings below; the CAMV survey, the camera methods and the node canvas binding added later that day). The Audio plan's bank writer
landed (its steps 4 to 8), so work started the same day (user: run unattended): the video layer and the movie viewer are built
(steps 1.10 and 2), then the scene editor, the preview, both exports and the logic graph (steps 3 to 8, each with what is
left); movie editing (9) and every in-game check are open.

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
    vanilla scripts pass 5 (fov 33 to 45); the library's skip and restore pass `false, 0, 0, 0`.
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
   playhead holding the value there), Key (the selected key's values), Layout (the field from above at the playhead; Ctrl+click keys
   the selected unit's path). File > New / Open cinematic scene. Autosave (2026-11-21): a scene's
   `project.json` is copied and taken back (`Tool::restore`), unsaved until saved. Texts in the game's nine languages (Scene >
   Texts' language; the Key panel and the exports follow it, English standing in). Clips are deleted (a scene keeps one); not
   reordered.
4. **Scene preview.** **partial** (2026-11-20, 2026-11-21): `preview.rs` draws the scene in the viewport at the playhead: a flat battle ground
   (grass, as the weather tool's sample map), each unit's men in formation (`formation` of `land_units` rank depth and spacing)
   posed by the game's animations in the evaluated slot (`UnitModel`, 6 soldiers a unit and slot taken in turn, the slot's time
   from its start), the camera following its keys (Free camera to fly; a camera key holds the viewport's view), the weather blended
   between keyed environments (`set_weather_params`, `blend_environments`). Keyed depth of field through the environment's DOF block (the
   game's DOF constants are `change_depth_of_field`'s values), written to the scene at once (a movie renders many frames in one
   editor frame). A moving slot plays by the ground covered over its clip's own travel speed (no foot sliding). Battle preview
   (the Timeline's Battle, `eval::evaluate_battle`): units teleported to their first key, then under orders at their own walk /
   run speed in straight lines (early or late against their keys), movie-only tracks gone. Not done: the campaign-map ground at
   the battle's position (Map context), VFX instances, sounds heard while scrubbing.
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
   the camera baked into `move_to` steps of 0.5 s with the field of view, teleports and `goto_location_angle_width` orders,
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
to fill a scene), then asked for the Tile tool's ways of making and editing things to be reused ("a very nice ref").

1. Pickers fed from the packs for every field naming game data (units by name and by the factions that field them, factions,
   weather files, VFX, battle maps, sound and music events, animation slots; armies and clips in logic nodes); New scene / Open a
   scene in the empty panels; the Timeline's bar on two lines. **done** (2026-11-21)
2. The Tile tool's model (2026-11-21). **done**:
   - The scene's items (`items.rs`: the battle, armies, units, VFX instances) are the editor's selection; the document gives their
     place to the editor's gizmo (`item_transform` / `transform_items`): units move and turn in the 3D viewport, a unit with a path in
     the clip shown is keyed at the playhead (its start stays). A drag is one undo entry (merged snapshots, closed when it ends).
   - **Create** panel (`create.rs`, as the Tile tool's): one search, Units / Effects / Armies tabs, the units as the game's cards
     (`unit_variants.unit_card`, `ui/units/icons`), the chosen army's faction's first; a click starts placing, clicks on the viewport's
     ground or the Layout put copies (R / Shift+R turn, Esc or a right click stops), the Inspector shows the draft every copy takes
     (army, men from `main_units.num_men`, width, turn, general; an effect's length and scale); Space over the viewport: the quick-add
     popup, placing at the mouse. Armies are added by faction.
   - **Scene** panel (`outliner.rs`, as the Tile tool's): the battle, each army with its units (side colour, the player's star, an
     eye that hides a unit in the editor only, drag units onto another army, right-click menus: place units, duplicate, look at it,
     delete), the clip's effects; Ctrl / Shift click.
   - **Inspector** (`inspect.rs`): the selected item's rows in the shared Inspector (battle setup and texts, army faction / side /
     zone, unit kind / army / start / facing / men / width / general, its card; "Key its place here" and "Animate" at the playhead;
     effect kind / times / place / scale); a key clicked on the timeline shows there (the Key panel is gone).
   - **Viewport** (`view.rs`): clicks select units (by their formation on the ground) and effects (their markers), box select in
     Select mode, formations drawn in their side's colour, the selected ones named with their path's keys, the placing ghost and its
     hint, a right-click menu (duplicate, look at it, delete, add here). Delete, Ctrl+D, Space, R / Shift+R, Esc in the shortcut
     registry.
   - A new or opened scene shows the Scene, Timeline and Viewport tabs, the camera behind the player's zone.
   - Found on the way: the unit list by faction compared land-unit keys with main-unit keys (fixed); the gizmo read a unit's place
     from the previous frame (it is evaluated when asked now).
   - Gate: `scripts/cinematic_ux.txt` in `cinematic_check.sh` (placing by clicks, moves and turns as one undo entry each, duplicate,
     delete, a move keying a path, an army deleted with its units; the saved project identical between two runs).
3. The battle's map position picked on a picture (`map_pick.rs`: the battle map's `lf_height_map.dds` coloured as the Tile tool's
   minimap; click, wheel zoom, right-drag pan; the Battle's Inspector, "Pick on the map..."). **done** (2026-11-21)
4. The Timeline: a double-click on a track's row adds a key there (the hover says how). **done** (2026-11-21)
5. Camera keys as scene items (2026-11-21). **done**: drawn in the viewport while the camera is free (each key, its target, the
   path), picked, moved and turned by the gizmo (the target goes along; a turn pans it about itself), in the Scene panel (Camera in
   the clip) and the Inspector (time, place, target, field of view, interpolation; Look through it, Take the view). The viewport's
   chips at the top left: play, free camera, key this view (a camera track made when the clip has none), battle preview. The
   playhead runs whichever panels show.
6. A first-run walkthrough: not built as text (UI explanations are one-line tooltips); every empty state offers its next step as a
   button instead (New scene / Open, Place units... under an army without units, Pick on the map..., the chips, the Timeline's
   double-click). **done** that way.

## Open questions and risks

- Units in battle follow the engine's pathing: arrival times drift from the preview. Teleports under a camera cut are exact.
- Whether the frontend accepts a historical battle on a campaign-map position (1.6).
- The cost of a full playable battle in a node graph: about as much as the cutscene and movie paths together.
- Movie fidelity is the editor's: the sea, vegetation leftovers and VFX (rendering_remaining.md, weather_current_state.md) show
  in every rendered movie.
- Cindy stays unknown (1.5): if a sample appears, units could follow keys exactly in battle.
