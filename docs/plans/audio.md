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

## What the game has

The sound files in the packs (Attila, TDD and Rome 2, checked 2026-11-19) and what the game lets mods choose (2026-11-20):
[docs/re/audio.md](../re/audio.md). The WEM statistics of `sound.pack`: [wem.md](../formats/wem.md), "Vanilla files".
Rome 2 loads only v112 banks, so one bank parser serves both games; v88 is read-only history (not supported unless a mod needs it).

### `event_data.dat`, the WEM and bank layouts

In [sound_data.md](../formats/sound_data.md), [wem.md](../formats/wem.md) (the Wwise Vorbis header, packing, the setups by quality and
the encoding findings of step 1) and [wwise_bank.md](../formats/wwise_bank.md).

### The Wwise 2014.1.3 oracle

How to run it headless and what its output is: [wem.md](../formats/wem.md), "The Wwise 2014.1.3 oracle". Tests can convert the same
WAV with Wwise and with WCE and compare.

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
   containers' mode and weights; gate `audio_edit_check`). The game does not take edited `*_data.bnk` banks from a mod pack
   (docs/re/audio.md, "What a mod pack can change"), so a replace of in-bank media redirects its sources to a new loose
   `audio/<id>.wem` (`redirect_media`) and new variations stream from loose files. Property IDs: only volume (0), pitch (2) and low-pass
   (3) are checked; the others carry Wwise 2014's names as wwiser lists them.
5. **Remove and delete, new events** (`references`: the game's uses (`event_data.dat`, triggers CSV, `.amb`, UI layouts, DB rows,
   animations) and the banks'; `EditSet::delete_object`, `new_event`, `add_trigger`, `add_emitter`). Tool: "Used by", Delete with the
   game's uses as warnings, "New event playing this", "Add to the animation triggers", the "Nothing triggers this event" warning; names
   given by the user in `<projects>/audio/names.txt`.
6. **Music** (`music`, `trees`): playlists as trees (add after, remove, weight, loop), a segment's audio replaced, transition rules and
   stingers typed and edited, new playlists (`new_playlist_like`), what a music switch plays for a path of states
   (`EditSet::set_music_path`); gate `audio_music_check`.
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

## What the game lets mods choose

Checked 2026-11-20: docs/re/audio.md, "What the game lets mods choose". More lines per actor is `add_variation`; cultures without lines
of their own get them with `recipes::new_voice_culture` (step 10).

## Open questions

- `event_data.dat`: the meaning of the events' f32 value and the difference between the two kinds of state groups.
- pywarscape-sound (MIT) reports v112 round trips and PCM WEMs (32 kHz mono) that load in Attila: a cross-check for the bank writer.
