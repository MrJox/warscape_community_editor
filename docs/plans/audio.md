# Audio: the Audio tool and the shared audio layer

Listen to, replace, add and remove the game's sounds: Wwise sound banks (`.bnk`), sound files (`.wem`), `event_data.dat`, the
animation sound files (`.snd.meta`, `sound_animation_triggers.csv`), the campaign map ambience (`.amb` + `.xml`), the sound DB tables and
the localised voice packs. High-level recipes do the common jobs in one dialog; IDs are shown as names wherever a name can be found. No
Wwise and no other dependency at runtime. The audio layer is headless so that the Cinematic tool (and later the Terrain and Unit tools)
reuse it.

State (2026-11-22): decided with the user 2026-11-19 (the decisions below); built 2026-11-19 .. 11-20, steps 1 to 10 below, in master
(feature `audio`, in the `release` set). Left: the in-game checks and the items under "Open". How the tool works for a user:
docs/editor.md.

## Decisions (user, 2026-11-19)

| Area | Decision |
|---|---|
| Games | Attila and Rome 2 (both Wwise bank v112). Every parser is keyed on game and bank version, so later games (ToB, Warhammer...) slot in. |
| Decode | `lewton` (pure Rust Vorbis decoder, MIT/Apache) after our own Wwise-Vorbis -> Vorbis packet rebuild. Output through SDL3's audio (already linked). |
| Encode | Wwise Vorbis, as vanilla, with aoTuV (libvorbis + aoTuV's encoder, C, BSD-3, built from source like SDL3) and our own Ogg -> Wwise Vorbis conversion: chosen after the spike (user, 2026-11-19; step 1). |
| Reference | Wwise 2014.1.3 (on the user's machine) is a dev-only oracle: tests compare our WEMs and banks with its output and are skipped without it. WCE never needs Wwise. |
| Output | Whole-bank override, as the game loads banks by name (TDD already ships overriding banks). Save writes the bank, WEMs and other files to `<projects>/loose/<path>` like every tool; a round trip is byte-identical and an edit changes only its spans. |
| UI | Bank-centric: banks open as documents with a readable object tree (names, not IDs); a global search over banks, WEMs, DB rows and VO jumps into the right bank; cross-bank links are clickable. A raw object inspector for the rest. |
| v1 tasks | Browse and listen; replace audio; add and remove variations in containers (volume, pitch, weights); animation sounds. |
| Remove | Remove a variation from a container; delete objects and their now-unused WEMs, warning while the game, DB, `.snd.meta` or `.amb` still reference them. |
| New events | Yes, always with a trigger in the same step: a `.snd.meta` frame, a DB row or an `.amb` emitter; a warning when nothing triggers one. |
| Music | Full interactive-music editing: tracks, playlists, transitions, switches, stingers. |
| `.snd.meta` | A per-animation timeline with readable event names and playback, no model yet; the Unit/Cinematic tools later show the model on the same timeline. |
| `.amb` | Parsed, round-tripped and listed with readable names and preview in v1; the campaign/terrain tool places them on the map later with the same crate. |
| Also v1 | The sound DB tables with readable joins (unit -> voice category -> events), and the localised VO packs. |
| Names | From the install at runtime (`event_data.dat`, DB tables, `.snd.meta`, the triggers CSV, file and bank names, unit and animation keys, hashed with FNV-1 and matched), user labels saved in the project, and TheTollski's `TWA_Names.txt` (SoundbankEditor), shipped with WCE: TheTollski is on the user's team and the user has permission (2026-11-19); credit it in `THIRD_PARTY.md` when it goes in. Other community lists importable. |
| Recipes | Music tracks (replace/add into the playlists), unit voice set (bank objects + `unit_voice_*` / `voice_events` rows), advisor/VO line, animation sound. |
| Cinematics | The shared layer plays an event at a time on a timeline (seekable, seeded randomness), positions it in 3D (the bank's attenuation and panning) and mixes a timeline down to WAV. |

## What the game has (Attila, checked 2026-11-19)

- `sound.pack`: 51 banks, 1,478 WEMs, `audio/event_data.dat` (165 KB, layout in docs/formats/sound_data.md), `audio/sound_animation_triggers.csv`. `music.pack`: 4 banks (`battle_music`, `global_movies`, `global_music`, `music_data`),
  546 WEMs. `local_en_shared_rome2.pack`: about 39,000 VO files (`audio/`, `audio/advisor`, `audio/diplomacy`) and the VO banks
  (`battle_advice`, `battle_vo_orders_*`...). `music_en_shared_rome2.pack`: 99 files.
- `data.pack`: `.snd.meta` next to the animations, `audio/{main,bel,cha}_attila_map.amb` + `.xml` (campaign map ambience), and the DB
  tables `sound_events`, `voice_events`, `voice_types`, `unit_voice_categories`, `unit_voice_events_junctions`,
  `agent_voice_events_junctions`, `subculture_religion_voiceover_junctions`, `audio_projectiles`, `audio_explosions`,
  `audio_campaign_building_enums`. Battle tiles have `sound_emitter_list.xml` (the Tile tool's, later).
- TDD (`tdd_pack1_main_v1.2.0`): 8 overriding banks (`battle_music`, `battle_vo_orders`, `battle_vocalisations`, `campaign_ui`,
  `campaign_vo`, `global_movies`, `global_music`, `music_data`), 1,061 WEMs, advisor VO.
- Every bank is `BKHD` version 112, also those of the `*_shared_rome2` packs.
- Rome 2 (checked 2026-11-19): its release packs `sound.pack` (39 banks) and `music.pack` (4) are **v88**, but the patch packs
  `sound_rome2.pack` (51 banks + an empty `animation_data.bnk`, which removes it) and `music_rome2.pack` (4) override every one of them
  with v112, and the VO banks of `local_en_shared_rome2.pack` (61) are v112. So the game loads only v112 banks: one bank parser serves
  both games; v88 is read-only history (not supported unless a mod needs it). Rome 2 has its own `event_data.dat` (104 KB in
  `sound.pack`, 112 KB in `sound_rome2.pack`, same layout as Attila's) and eight campaign `.amb` + `.xml` pairs in `data.pack`
  (`main_emperor`, `main_gaul`, `main_greek`, `main_invasion`, `main_punic`, `main_rome_map`, `prologue_map_01`...).
- `music.pack` and `music_en_shared_rome2.pack` have the data-encrypted flag (PFH4 header flags `0x11`). rpfm_lib's develop branch (what
  WCE uses) decrypts them right; **rpfm_cli 4.3.14 does not**: it negates the position as a u64 where PFH4 needs a u32, and its output has
  every other u32 garbled. Scripts must not trust rpfm_cli extracts from those packs.

### WEMs (the 1,478 of `sound.pack`)

- All Wwise Vorbis (codec `0xFFFF`), all with a 0x42-byte `fmt ` (the `vorb` data embedded: sample count, setup and first audio packet
  offsets, uid, blocksize exponents). Chunks: `fmt`+`data` (1,314), plus `JUNK`+`akd ` (155), `LIST`+`cue `(+`smpl`) (9).
- Channels 1 (1,227), 2 (230), 6 (7), 5 (12), 4 (2). Sample rates mostly 28,000 Hz (1,132) and 32,000 (189); 15 others from 11,025 to
  48,000 (Wwise's sample-rate conversion settings).
- Blocksizes 2^8/2^11 (1,389), 2^9/2^9, 2^9/2^10, 2^10/2^10.
- The setup packet references codebooks by 10-bit ID (18 to 44 per file, highest ID 462): Wwise's built-in codebook library, not inline
  codebooks. Our encoder must emit streams that use only codebooks of that library.
- Bitrate: median 57 kbps per channel (10th percentile 47, 90th 67).

### `event_data.dat`, the WEM and bank layouts

In [sound_data.md](../formats/sound_data.md), [wem.md](../formats/wem.md) (the Wwise Vorbis header, packing, the setups by quality and
the encoding findings of step 1) and [wwise_bank.md](../formats/wwise_bank.md).

### The Wwise 2014.1.3 oracle

Headless conversion works: `WwiseCLI.exe <project>.wproj -ConvertExternalSources Windows <list>.wsources -ExternalSourcesOutput Windows
<dir>` on a scratch copy of the SDK's `samples/IntegrationDemo/WwiseProject` (2014 has no `-CreateNewProject`); its `Voice` and `Music`
conversion ShareSets are Vorbis on Windows. The output has vanilla's exact layout (0x42 `fmt`, external codebook IDs, blocksizes 8/11);
only the `uid` differs per file. So tests can convert the same WAV with Wwise and with WCE and compare.

## Crates

- `wce_audio_format`: the files, no audio. Banks (`BKHD`, `DIDX`, `DATA`, `HIRC`, `STID` and any other chunk raw; HIRC objects of an
  unknown type kept as bytes), WEM RIFF, `event_data.dat`, `.amb`, `sound_animation_triggers.csv`, FNV-1 IDs. Versioned per game and
  bank version. `.snd.meta` is `wce_formats::anim_meta`.
- `wce_audio`: the headless layer: the codebook library and the Wwise Vorbis -> Vorbis rebuild, decode, encode, imports, the name
  index, the resolver (`SoundLibrary`), edits (`EditSet`), references, music, decision trees, animation sounds, the sound DB tables,
  unit voices, recipes, analysis. The Cinematic tool's dependency. Not built in it: a timeline player and 3D attenuation and panning;
  the Cinematic tool's movie export mixes its own sound track (`wce_cinematic::movie_export::mixdown`).
- `wce_audio_tool`: the Audio tool's UI (SDL3 playback in `player.rs`), like the other tools.

Gates (Cargo examples, in `scripts/gates.sh`): `audio_formats` and `rome2_audio_formats` (every bank, WEM and `event_data.dat` written
back byte for byte), `audio_decode` and `rome2_audio_decode`, `audio_encode`, `audio_edit`, `audio_music`, `audio_recipes`. No gate
compares with the Wwise oracle: it was used by hand (scratch scripts) for the setups and alloc sizes.

## Steps (as built)

Every step was done 2026-11-19 .. 11-20; the detail is in the git history. Scratch work (Windows, not in the repo, may be lost): the
test packs under `%TEMP%\claude\439e225181e38936\b8cb548f-d7a7-4438-97ec-8a54f498b744\scratchpad\spike\` (`ingame*`), the
downloaded sources and the oracle project beside them.

1. **Spikes.** Rome 2 loads only v112 banks; the vanilla WEM survey; the Wwise 2014.1.3 oracle; the encoder: **aoTuV** (user,
   2026-11-19): it gives Wwise's own setups and headers, equal or better quality, slightly smaller files, not bit-identical (no encoder
   short of Wwise's own would be); an aoTuV WEM plays in game (the main menu's ambience stream replaced by a mod pack). Findings:
   docs/formats/wem.md.
2. **Formats** (`wce_audio_format`): FNV-1 IDs, bank chunks, every v112 HIRC kind the games use typed (`hirc::parse`,
   `Object::write`; v88 banks round-trip as chunks), WEM RIFF and the Wwise Vorbis header, `event_data.dat`, `.amb`, the triggers CSV;
   `.snd.meta` through `wce_formats::anim_meta`. Gate `audio_formats_check` (`audio_formats`, `rome2_audio_formats`): every bank, WEM
   and `event_data.dat` of Attila + TDD v1.2.0 (134 banks, 44,802 WEMs) and Rome 2 (159 banks, 42,371 WEMs) byte for byte. Layouts
   learnt from pywarscape-sound's models (MIT), written anew. Effect plug-in parameters stay bytes (no edit needs them).
3. **Browse and listen** (`wce_audio`, `wce_audio_tool`): the codebook library (vendored `third_party/ww2ogg`), the Wwise Vorbis ->
   Vorbis rebuild and `lewton` decoding (`decode::decode_wem`, gate `audio_decode_check`: every WEM the game loads decodes); the name
   index (`names`: `event_data.dat`, `STID`, file names, the triggers CSV, TheTollski's `TWA_Names.txt` in
   `third_party/soundbank_editor`, the DB tables, user labels); the resolver (`library::SoundLibrary`, example `audio_library_check`:
   an event to the sounds and media it plays, across banks); SDL3 playback; the tool's panels Audio Banks, Audio Bank, Audio Search,
   Audio Object. Script commands `audio_bank`, `audio_search`, `audio_select`, `audio_expand`.
4. **Replace and add variations.** Encoder `encode::encode_wem` (`aotuv_lancer_vorbis_sys`; qualities 0 and 2 to 10, 1 to 5
   channels; 5.1 and 7.1 imports folded to 5.0; decoder alloc sizes from `setup_allocs`; gate `audio_encode_check`); imports of WAV,
   Ogg Vorbis and WEM (`import::read_sound`); the bank writer and `edit::EditSet` (replace, add / remove a variation, properties,
   containers' mode and weights; gate `audio_edit_check`). In game: **the game does not take edited `*_data.bnk` banks from a mod
   pack**, while loose WEMs and event banks do, so a replace of in-bank media redirects its sources to a new loose
   `audio/<id>.wem` (`redirect_media`) and new variations stream from loose files. Property IDs: only volume (0), pitch (2) and low-pass
   (3) are checked; the others carry Wwise 2014's names as wwiser lists them.
5. **Remove and delete, new events** (`references`: the game's uses (`event_data.dat`, triggers CSV, `.amb`, UI layouts, DB rows,
   animations) and the banks'; `EditSet::delete_object`, `new_event`, `add_trigger`, `add_emitter`). Tool: "Used by", Delete with the
   game's uses as warnings, "New event playing this", "Add to the animation triggers", the "Nothing triggers this event" warning; names
   given by the user in `<projects>/audio/names.txt`.
6. **Music** (`music`, `trees`): playlists as trees (add after, remove, weight, loop), a segment's audio replaced, transition rules and
   stingers typed and edited, new playlists (`new_playlist_like`), what a music switch plays for a path of states
   (`EditSet::set_music_path`); gate `audio_music_check`. No game segment has several tracks.
7. **Game data** (panel Audio Game Data): animation sounds (`anim_sounds`: a `.snd.meta`'s frames on a timeline as long as its clip,
   added, dragged, removed; `audio_animation`), campaign ambience by sound, the sound DB tables (`sound_db`), the localised voice banks.
8. **Recipes** (`recipes`, gate `audio_recipes_check`): music playlists named by their switch paths, an animation sound (existing
   event or a new sound), a culture's voice lines. The Audio Recipes panel was **removed** (user, 2026-11-20: "it doesn't make sense to
   me"); the library stays, gated. What only the panel offered (a unit's own selection line, a culture's lines copied to another, a new
   sound for an animation from a file) has no UI now: if wanted back, as actions where the object is (Audio Object, Audio Game Data).
9. **Player and visuals**: the waveform as the progress bar (seek by clicking), play / pause / stop, loop, volume; a selected sound's
   waveform in Audio Object.
10. **User review 2 and the open items**: a new SDL stream per sound (the loop and rate bugs); the player and the sound's details in the
    Inspector (codec, setup in words, peak, RMS, loudness by ITU-R BS.1770-4: `analysis`); the unit voice banks browsable (tabs
    Containers and Sounds); unit voices (`unit_voices`, `EditSet::add_order_line`, `sound_db::set_cell`); every bank's copy of a switch
    or event merged (`SoundLibrary::merged`) and edits written to every copy; decision trees laid out again byte for byte
    (`hirc::tree_from_paths`), `add_dialogue_value` and `recipes::new_voice_culture` (a culture of the engine's list given another's
    lines); actions read field by field (`Action::parts`, delay and fade time edited); the edit session as an editor document
    (`AudioDocument`, "Audio edits": Edit > Undo / Redo, History, File > Save, Revert; script command `audio_volume`).

## Open

- **UI button sounds** (step 4, set aside by the user 2026-11-20): they do not change through a mod pack's banks (event or data banks),
  while loose WEMs do; cause unknown (UI banks loaded before mods, another event, or cached media). Revisit with a runtime trace or a
  non-UI event (a battle sound).
- **Unit selection lines, pack 2** (user): with only `selection_vo` set, part of the selections beeped (pack `ingame5`): edited voice
  banks, new streamed takes and a DB row do work in game. Pack `ingame6` sets all three selection columns: every selection beeping
  would mean the three columns are all there is. The tool now sets all three by default.
- Whether the empty voice junction tables (`voice_events`, `unit_voice_events_junctions`, `agent_voice_events_junctions`) are read (a
  test pack with rows).
- By ear (user): pause, seek by clicking, loop, volume, the bar reset at the end.
- The editor document (10, "Audio edits") not yet seen in the app.
- Wwise-faithful playback (blend layers mixed, sequences in order) is the Cinematic layer's, not chosen for v1.

## What the game lets mods choose (checked 2026-11-20, Attila's banks, DB and scripts)

- Music: the music switches decide on `music_culture` (Barbarian, Roman, arabic, hunnic), `game_mode`, `campaign_music_state`
  (standard, prebattle), `battle_intensity`, `battle_state`, `battle_result_state`, `music_key`, `Music_Dynamic` (battle events:
  charges, routs, generals killed...). No DB column sets a music culture (no table has a music column but unit musicians): the
  game's code sets `music_culture`, so a new culture value in the banks would never be chosen. Battle scripts can take over:
  `bm:set_music_auto_playback(false)`, `bm:play_music(event)`, `stop_music`, `cutscene:set_music`. No campaign script uses music
  or sound, and the toolkit's API list has no such call.
- Campaign character voices: 49 dialogue events decide on `vo_campaign_character` (Cha1, Cha2, Dig1 to Dig4, Gen1Land,
  Gen1Naval, Gen2Land, Gen2Naval, Spy1, Spy2), `vo_campaign_culture` and `vo_campaign_subculture`; all 1,819 leaves are random
  containers of 1 to 10 lines (median 5): more lines per actor is `add_variation`. Which actor a character gets is the game's
  code: no DB column links characters or agents to a voice (`agent_voice_events_junctions`, `unit_voice_events_junctions`,
  `voice_events` are empty in vanilla; `voice_types` holds generic, is_commanding_officer, is_hero).
- Battle unit voices: `main_units.voiceover` (7 values) and `subculture_religion_voiceover_junctions` (by DB) choose the
  `vo_battle_subculture` value; `land_units.selected_vo_secondary` / `_tertiary` choose selection lines.
- The engine (`empire.retail.dll`, 2026-11-20): it holds no state group name (`music_culture`, `vo_campaign_character`,
  `VO_battle_culture`... are not strings in it: it works with their hashed IDs or names from `event_data.dat`); it names the
  voice-over cultures it knows (barbarian, barbarian_christian, barbarian_pagan, barbarian_generic, nomad, nomad_hunnic,
  nomad_hunnic_generic, eastern, eastern_desert, eastern_persian, eastern_generic, roman, roman_christian, roman_generic) and
  reports any other as `invalid_voiceover_culture`: no new culture, but cultures without lines of their own can get them
  (step 10, `recipes::new_voice_culture`); it loads the order banks by fixed names (`Battle_VO_Orders_<Barbarian|Eastern|Nomad|Roman>[_Inf1-12|_Mtd1-7].bnk`):
  new order lines go into those banks.
- The engine's hashed IDs (2026-11-20, the FNV-1 IDs as 4-byte constants in `empire.retail.dll`): `music_culture` (5 places)
  with its values Barbarian, arabic and hunnic side by side (0x946792, 0x946799, 0x9467a0: a fixed table), `campaign_music_state`,
  `vo_campaign_character` with Cha1, Cha2, Dig1, Spy1, Gen1Land, Gen1Naval together (0x9459ef to 0x945bda: a fixed mapping of
  agents to actors), `vo_campaign_culture` / `_subculture`, `VO_battle_culture` / `_actor` / `_order`, `Battle_Order_VO_Play`;
  `vo_battle_subculture` is not there (set by name: the voice-over culture). So the music cultures and the campaign voice actors
  are fixed in the engine's code: a mod changes what their values play (every edit of this tool), not which values exist or who
  gets them.
- Open lead: whether the empty voice junction tables are still read (a test pack with rows).

## Open questions

- `event_data.dat`: the meaning of the events' f32 value and the difference between the two kinds of state groups.
- pywarscape-sound (MIT) reports v112 round trips and PCM WEMs (32 kHz mono) that load in Attila: a cross-check for the bank writer.
