# Audio: the Audio tool and the shared audio layer

Listen to, replace, add and remove the game's sounds: Wwise sound banks (`.bnk`), sound files (`.wem`), `event_data.dat`, the
animation sound files (`.snd.meta`, `sound_animation_triggers.csv`), the campaign map ambience (`.amb` + `.xml`), the sound DB tables and
the localised voice packs. High-level recipes do the common jobs in one dialog; IDs are shown as names wherever a name can be found. No
Wwise and no other dependency at runtime. The audio layer is headless so that the Cinematic tool (and later the Terrain and Unit tools)
reuse it.

State: decided with the user 2026-11-19 (the decisions below); spikes 1 to 3 done, the encoder spike (4) waits on the dependency question.

## Decisions (user, 2026-11-19)

| Area | Decision |
|---|---|
| Games | Attila and Rome 2 (both Wwise bank v112). Every parser is keyed on game and bank version, so later games (ToB, Warhammer...) slot in. |
| Decode | `lewton` (pure Rust Vorbis decoder, MIT/Apache) after our own Wwise-Vorbis -> Vorbis packet rebuild. Output through SDL3's audio (already linked). |
| Encode | Wwise Vorbis, as vanilla. aoTuV/libvorbis (C, BSD-3) is used only if the spike shows its output matches Wwise's (structure, codebook IDs, size, quality); otherwise a native Rust encoder. |
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
| Names | From the install at runtime (`event_data.dat`, DB tables, `.snd.meta`, the triggers CSV, file and bank names, unit and animation keys, hashed with FNV-1 and matched), user labels saved in the project, and an importable community name list. Nothing game-derived is committed. |
| Recipes | Music tracks (replace/add into the playlists), unit voice set (bank objects + `unit_voice_*` / `voice_events` rows), advisor/VO line, animation sound. |
| Cinematics | The shared layer plays an event at a time on a timeline (seekable, seeded randomness), positions it in 3D (the bank's attenuation and panning) and mixes a timeline down to WAV. |

## What the game has (Attila, checked 2026-11-19)

- `sound.pack`: 51 banks, 1,478 WEMs, `audio/event_data.dat` (165 KB, starts with 1,864 length-prefixed event names, the rest not yet
  decoded), `audio/sound_animation_triggers.csv`. `music.pack`: 4 banks (`battle_music`, `global_movies`, `global_music`, `music_data`),
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

### The Wwise 2014.1.3 oracle

Headless conversion works: `WwiseCLI.exe <project>.wproj -ConvertExternalSources Windows <list>.wsources -ExternalSourcesOutput Windows
<dir>` on a scratch copy of the SDK's `samples/IntegrationDemo/WwiseProject` (2014 has no `-CreateNewProject`); its `Voice` and `Music`
conversion ShareSets are Vorbis on Windows. The output has vanilla's exact layout (0x42 `fmt`, external codebook IDs, blocksizes 8/11);
only the `uid` differs per file. So tests can convert the same WAV with Wwise and with WCE and compare.

## Crates

- `wce_audio_format`: the files, no audio. Banks (`BKHD`, `DIDX`, `DATA`, `HIRC`, `STID` and any other chunk raw; HIRC objects of an
  unknown type kept as bytes), WEM RIFF, the Wwise-Vorbis <-> Vorbis packet rebuild and the codebook library, `event_data.dat`,
  `.snd.meta`, `.amb`/`.xml`, `sound_animation_triggers.csv`, FNV-1 IDs. Versioned per game and bank version.
- `wce_audio`: the headless runtime: the name index across banks/DB/files, decode, encode, the event resolver (what an event plays,
  across banks), the timeline player (seekable, seeded), 3D attenuation and panning, mixdown to WAV, SDL3 output. The Cinematic tool's
  dependency.
- `wce_audio_tool`: the Audio tool's UI, like the other tools.

Gates (Cargo examples, in `scripts/gates.sh`): every vanilla and TDD bank, `event_data.dat`, `.snd.meta` and `.amb` re-saves
byte-identical; every WEM decodes; the encoder and the bank writer match the Wwise oracle (skipped without it).

## Steps

1. **Spikes.**
   1. Rome 2's banks' version: v112 wherever the game loads them (its v88 release banks are all overridden). **done**
   2. The vanilla WEM survey (above). **done**
   3. The Wwise 2014.1.3 oracle (above). **done**
   4. The encoder: encode the oracle's WAV with aoTuV/libvorbis, rebuild it as Wwise Vorbis against the codebook library, compare with
      Wwise's WEM (structure, codebook IDs, size within about 5 %, SNR against the source at least Wwise's), then replace a vanilla
      sound and hear it in game. Passes: aoTuV (built from source like SDL3, `THIRD_PARTY.md`). Fails: a native encoder. **todo, waits
      on the dependency question** (aoTuV and libogg sources, and the codebook library: ww2ogg's packed file, BSD, or our own extraction).
2. **Formats** with the round-trip gates; `docs/formats/wwise_bank.md`, `wem.md`, `event_data.md`, `snd_meta.md`, `amb.md`.
3. **Browse and listen**: the bank documents, the name index, the global search, decode and playback.
4. **Replace and add variations**: the encoder, the bank writer (DIDX/DATA rebuilt, HIRC edited in place), containers' properties.
5. **Remove and delete** with the reference warnings; **new events** with their triggers.
6. **Music**: playlists, segments, tracks, transitions, switches, stingers.
7. **`.snd.meta` timeline, `.amb` list, the sound DB tables, the localised VO.**
8. **Recipes**: music tracks, unit voice set, advisor/VO line, animation sound.

## Open questions and risks

- Whether Attila's Wwise runtime accepts our Vorbis streams is only known once one plays in game (step 1.4).
- `event_data.dat` past the event names is not decoded yet.
- Music editing is the hardest part to keep byte-identical; the oracle can generate reference objects for it.
- TheTollski's `TWA_Names.txt` has no visible licence: not shipped, importable by the user. pywarscape-sound (MIT) reports v112
  round trips and PCM WEMs (32 kHz mono) that load in Attila: a fallback if the encoder fails, and a cross-check for the bank writer.
