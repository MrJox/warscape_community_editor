# Cinematic: scenes, movies and historical battles

One editor, one scene model, two outputs: a **game-ready movie** (`.ca_vp8` with its audio event and subtitles) rendered offline from
the editor, and a **historical battle** (battle XML, Lua, DB rows, loc, sounds) that the game plays. Plus a movie viewer and editor for
the game's own movies. Everything on a timeline is keyed: moving the timeline gives the same picture every time.

State: decided with the user 2026-11-20 (the grilling, decisions below); static spikes done the same day (findings below). Nothing
built. Waits on the Audio plan's bank writer (user: audio first).

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
- Camera: `cam:move_to(pos, target, seconds, blend, fov)` in the vanilla scripts; the library calls it with 7 arguments
  (`..., false, 0, 0, 0)`): the last two are not known. `cam:fade(out, seconds)`.
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

- `movies.pack`: 153 `.ca_vp8`. The container: `CAMV`, a short header with `VP80`, width, height (1920 x 1080), milliseconds
  per frame as f32 (33.33: 30 fps), the frame count and the offset of a frame table at the end; then raw VP8 frames
  (**ASSUMPTION**: field layout read from one file's bytes; RPFM converts CAMV <-> IVF, `set_video_format`, which is the reference).
  No audio inside.
- `videos` (`video_name`, `video_type` FULLSCREEN_INTRO / CAMPAIGN_ESTABLISH / FULLSCREEN_FRONTEND, `audio_tracks`, `script_ref`);
  `movie_event_strings` (event, culture, movie, id).
- Audio: the Wwise event `Play_Movie_<name>` (the engine's prefix), vanilla's in `global_movies.bnk` (`music.pack`).
- Subtitles: `subtitle_timings` (`language`, `subtitle_field`, `text_section`, `foreign_key` = the movie, `start`, `end`,
  `script_id` = the video's `script_ref`, `text_id`), text in `vo_fmv_texts.loc`: one entry per movie
  (`vo_fmv_texts_text_<movie>`), its sections separated by `|`, section *n* shown from `start` to `end`. **ASSUMPTION**: those are
  frames at the movie's rate (att_seal_3: 2,163 frames, last subtitle ends at 1,841).
- TDD's own: `intro.pack` (`movies/tdd_intro.ca_vp8` + a `videos` row), and the campaign flybys (`*_flyby.pack`: faction intro Lua
  + advisor `.wem` lines + advice rows): campaign camera scripts, a later output if wanted.

## Crates

- `wce_cinematic`: the headless model: project, clips, tracks, keys, capability flags, `evaluate(clip, t)` as a pure function, the
  node graph, the compilers (battle XML, Lua, DB rows, loc), save/load.
- `wce_formats::ca_vp8`: the CAMV container (read, write, round trip byte for byte).
- `wce_video`: libvpx (decode, encode), built from source like aoTuV.
- `wce_cinematic_tool`: the UI (timeline, viewport, node canvas, movie viewer), publishing into the shared scene like every tool.
- Reused: `wce_audio` (the timeline player, mixdown, bank writer), `wce_render` (units, weather, the frame), Map context, Launch in
  game, `wce_weather` (environments).

## Steps

Status legend: **done**, **in progress**, **partial**, **todo**, **parked**, **blocked** (with why). Every step keeps its status here,
updated as work lands.

Scratch work (not in the repo, may be lost on a reboot; the findings that matter are recorded above):
`%TEMP%\claude\c3aa9bd9b1039f6c\235db9f9-a566-4b6b-8909-603929d37216\scratchpad\` — `hb\` two vanilla historical battles, `lib\` the
battle libraries, `tsv\` and `loc\` / `locall\` the menu, movie and subtitle tables and loc, `mv\` one movie.

0. **Prerequisites.** **blocked** (the Audio plan's bank writer: user chose audio first)
   1. The Audio plan's bank writer (new events in a bank, `global_movies` override). **blocked** (audio.md)
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
   7. `cam:move_to`'s last two arguments and its fov unit. **todo**
   8. The CAMV layout against RPFM's converter, a byte-for-byte round trip over `movies.pack`. **todo**
   9. An imnodes binding for dear-imgui-rs 0.20 (exists, or a thin one of our own). **todo**
   10. libvpx built from source on the three platforms; licence in `THIRD_PARTY.md`. **todo**
2. **Movie viewer.** **todo** (first milestone): a movie from any pack, decoded (CAMV -> VP8 -> frames), its `Play_Movie_<name>` audio
   through the audio layer in sync, its subtitles from `subtitle_timings` + `vo_fmv_texts` in the chosen language; scrub, frame step.
3. **Scene model and timeline.** **todo**: the project folder, clips, tracks (camera, units, sound, music, subtitles, VFX, weather,
   fades/letterbox, DOF), keys, capability flags, `evaluate(clip, t)` pure, undo, autosave.
4. **Scene preview.** **todo**: a campaign-map position through Map context; weather keyed (blending two environments is the editor's
   own rule: no such thing in game); units by slot, gait from speed along paths, men count, per-man phase; the camera spline; sounds
   and music on the audio timeline player (seeded); subtitles; VFX instances; letterbox and fades; the battle-preview mode (units
   walk to their ordered spot at their real speed in straight lines, forced animations off, movie-only tracks hidden).
5. **Movie export.** **todo**: offline frames at 30 fps, DOF, motion blur, libvpx -> CAMV; the audio mixdown as a WEM in a
   `Play_Movie_<name>` event (bank writer); `videos`, `subtitle_timings` rows and `vo_fmv_texts` loc in every language; a pack.
6. **Battle export: setup and cutscenes.** **todo**: the battle XML (armies from the scene's units, deployment areas,
   `tile_map_position`, the weather or a written `.environment`); each clip -> a `cutscene` (actions on 100 ms ticks, the camera
   baked into `move_to` segments, never `wait_for_vo`, subtitles as `scripted_subtitles` rows + loc, sounds as `new_sfx` events,
   music); the test launch through Launch in game.
7. **Battle logic node graph.** **todo**: the imnodes canvas; events (battle phase, timer, zone entered, unit routed / killed / below
   N men, cutscene ended, objective done), conditions, actions (orders, reinforcements, AI planner, patrols, objectives, advisor,
   music, play a clip, end the battle), a raw Lua node; compiled to Lua on the vanilla libraries; checked by the Lua checker
   (attila-lua-scripting).
8. **Historical Battles menu export.** **todo**: the `battles` and `historical_battles_ui_locations` rows, the loc, the loading
   screen title, a screenshot rendered by the editor.
9. **Movie editing.** **todo**: subtitle editing and export, audio replacement, video import through an external ffmpeg.

## Open questions and risks

- Units in battle follow the engine's pathing: arrival times drift from the preview. Teleports under a camera cut are exact.
- Whether the frontend accepts a historical battle on a campaign-map position (1.6).
- The cost of a full playable battle in a node graph: about as much as the cutscene and movie paths together.
- Movie fidelity is the editor's: the sea, vegetation leftovers and VFX (rendering_remaining.md, weather_current_state.md) show
  in every rendered movie.
- Cindy stays unknown (1.5): if a sample appears, units could follow keys exactly in battle.
