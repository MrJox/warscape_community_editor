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
| Names | From the install at runtime (`event_data.dat`, DB tables, `.snd.meta`, the triggers CSV, file and bank names, unit and animation keys, hashed with FNV-1 and matched), user labels saved in the project, and TheTollski's `TWA_Names.txt` (SoundbankEditor), shipped with WCE: TheTollski is on the user's team and the user has permission (2026-11-19); credit it in `THIRD_PARTY.md` when it goes in. Other community lists importable. |
| Recipes | Music tracks (replace/add into the playlists), unit voice set (bank objects + `unit_voice_*` / `voice_events` rows), advisor/VO line, animation sound. |
| Cinematics | The shared layer plays an event at a time on a timeline (seekable, seeded randomness), positions it in 3D (the bank's attenuation and panning) and mixes a timeline down to WAV. |

## What the game has (Attila, checked 2026-11-19)

- `sound.pack`: 51 banks, 1,478 WEMs, `audio/event_data.dat` (165 KB, layout below), `audio/sound_animation_triggers.csv`. `music.pack`: 4 banks (`battle_music`, `global_movies`, `global_music`, `music_data`),
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

### `event_data.dat`

The same layout in Attila and both Rome 2 copies, consumed exactly to the end by all three (checked 2026-11-19 with a scratch reader;
SoundbankEditor reads it this way, TheAssetEditor's Rome 2 variant with min/max floats per setting does not fit either game). All
integers u32 LE, strings u32 length + UTF-8:

1. Events: count, then name + a 4-byte value read as f32 (0 for most; 57 distinct values in Attila such as 32, 35, 133, 140; meaning
   not yet known). Attila 1,864, Rome 2 1,361 / 1,523.
2. State groups A: count, then group name + count + state names (`VO_battle_actor`, `VO_battle_culture`, `VO_battle_order` with 517
   states...). Attila 32.
3. State groups B: same shape (`Mixer`, `Music_Dynamic`, `animation_data_pool`...). Attila 37.
4. Dialogue events: count, then event name + count + u32 state-group IDs (FNV-1 of the group's name: `vo_battle_subculture`,
   `vo_battle_remark`...). Attila 171.
5. Settings / game parameters: count + names (`meter_advice`, `setting_music_battle_intensity_...`; Rome 2: `advisor_volume`,
   `amount_forest_under_listener`...). Attila 393.
6. Dialogue trees: count, then a dialogue event ID + count + (state value ID + count + state value IDs). Attila 98. The IDs resolve
   with FNV-1 of the names above (`battle_vo_about_to_disembark`, `Barbarian`, `conversation_03`...).

It is a names source in itself: every string in it hashes to an ID the banks use. To document in `docs/formats/event_data.md` (step 2).

### The Wwise 2014.1.3 oracle

Headless conversion works: `WwiseCLI.exe <project>.wproj -ConvertExternalSources Windows <list>.wsources -ExternalSourcesOutput Windows
<dir>` on a scratch copy of the SDK's `samples/IntegrationDemo/WwiseProject` (2014 has no `-CreateNewProject`); its `Voice` and `Music`
conversion ShareSets are Vorbis on Windows. The output has vanilla's exact layout (0x42 `fmt`, external codebook IDs, blocksizes 8/11);
only the `uid` differs per file. So tests can convert the same WAV with Wwise and with WCE and compare.

### Wwise Vorbis encoding (spike findings)

The vorb block (fmt + 0x18, 0x2A bytes), named after Wwise's runtime header and checked on the oracle's output:

| Offset | Field | Value written |
|---|---|---|
| 0x00 | total PCM frames | the source's frame count |
| 0x04 | loop start packet offset | size of the setup packet with its 2-byte header (relative to the end of the seek table) |
| 0x08 | loop end packet offset | data size minus the seek table |
| 0x0C | loop begin extra (u16), loop end extra (u16) | 0, and the same value as last-granule extra |
| 0x10 | seek table size | 0 (Voice preset), 0x80 (Music preset with seek table granularity) |
| 0x14 | Vorbis data offset | seek table + setup packet: the first audio packet |
| 0x18 | max packet size (u16), last granule extra (u16) | largest audio packet; samples decoded beyond the frame count (52 for the test tone) |
| 0x1C, 0x20 | decode alloc size, x64 alloc size | fixed per setup: taken from a table of known setups (all 18 seen so far); unknown setups: formula still to find |
| 0x24 | codebook hash | FNV-1 32 of the setup packet's payload |
| 0x28, 0x29 | blocksize exponents | from the Vorbis identification header |

The `fmt` before it: codec 0xFFFF, channels, rate, average bytes per second = data size x rate / frames, block align 0, bits 0,
cbSize 0x30, a u16 0, the channel mask (mono 4). Data: [seek table], the setup packet (u16 size + payload), then audio packets (u16
size + payload).

Conversion from a standard Vorbis stream (ww2ogg's rebuild, reversed): the setup drops the 'vorbis' header, the time section, floor
types (always 1), residue types to 2 bits, mapping types, the modes' window/transform types and the framing bit; codebooks become
10-bit IDs into Wwise's library (`packed_codebooks_aoTuV_603.bin`, 598 entries, 26 contents present twice). When a codebook has two
IDs, Wwise uses the copies that keep each quality level's block contiguous: choose the assignment minimising the sum of every ID's
distance to its nearest other ID (ties: lowest). Audio packets drop the packet-type bit and, for long blocks, the previous/next window
flags (the decoder recomputes them from the neighbouring packets' modes).

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

Status legend: **done**, **in progress**, **partial**, **todo**, **blocked** (with why). Every step and sub-step keeps its status here,
updated as work lands (user, 2026-11-19), so the work can be resumed from this file alone.

Scratch work (not in the repo, may be lost on a reboot; the findings that matter are recorded here):
`%TEMP%\claude\439e225181e38936\b8cb548f-d7a7-4438-97ec-8a54f498b744\scratchpad\` — `dl\` the downloaded sources (aoTuV, libogg,
ww2ogg), `build\prefix` the built static libs, `tools\` (`build_vorbis.bat`, `vtool.c` an aoTuV encode/decode tool, `wwvorbis.py`
the Ogg <-> Wwise Vorbis converter, `snr.py`), `spike\` the encodes, `wwise\` the oracle project and its outputs, `audio\` vanilla
`sound.pack` audio, `rome2own\` Rome 2's banks.

1. **Spikes.** **in progress**
   1. Rome 2's banks' version: v112 wherever the game loads them (its v88 release banks are all overridden). **done**
   2. The vanilla WEM survey (above). **done**
   3. The Wwise 2014.1.3 oracle (above). **done**
   4. The encoder (aoTuV vs native). **in progress**
      1. Downloads (aoTuV `AO-Yumi/vorbis_aotuv` ef858ad, libogg `xiph/ogg` 06a5e02, ww2ogg `hcs64/ww2ogg` 14ed9b0 for
         `packed_codebooks_aoTuV_603.bin`; all BSD; user approved 2026-11-19) and an MSVC build of both libraries. **done**
      2. Wwise header fields understood (see "Wwise Vorbis encoding" below): codebook hash = FNV-1 of the setup packet (1,480 of
         1,480 WEMs); decoder alloc sizes fixed per setup. **done**
      3. Ogg Vorbis -> Wwise Vorbis conversion: every aoTuV codebook (q0.0 to q0.6) is in Wwise's library; the duplicate-ID rule
         reproduces all 18 distinct vanilla + oracle setups; aoTuV q0.4 gives Wwise's Voice preset's setup byte for byte (hash
         3017712712); aoTuV q0.0 gives vanilla's most common mono setup (997642109). **done**
      4. Back conversion (WEM -> Ogg) for decoding: vanilla and oracle WEMs decode; our WEM decodes to exactly the samples of the Ogg
         it came from. **done**
      5. Synthetic comparison with Wwise (3 s mono tone + noise, q0.4 vs Voice): same setup, same packet count, audio not
         bit-identical (encoder decisions differ from bit 2 of the first packet), size +0.08 %, SNR 20.48 dB vs Wwise's 20.22 dB.
         **done**
      6. Realistic comparison: decoded vanilla WEMs (mono VO/SFX, stereo music, 5.1) re-encoded by Wwise and by aoTuV at matching
         quality: size, SNR, setups. Also try input scaling (/32767 vs /32768) for bit-exact output. **todo**
      7. Packet sizes exact: Wwise keeps the exact bit count; ours rounds up from the Ogg packet (up to 1 byte per packet). Needs
         aoTuV's bit count (we build it from source) or the native encoder. **todo**
      8. In-game test: replace a vanilla sound with our WEM (bank DIDX/DATA or loose WEM), the user listens. **todo**
      9. Verdict: aoTuV (`THIRD_PARTY.md`, built from source like SDL3) or a native encoder; record it here. **todo**
2. **Formats** with the round-trip gates; `docs/formats/wwise_bank.md`, `wem.md`, `event_data.md`, `snd_meta.md`, `amb.md`. **todo**
3. **Browse and listen**: the bank documents, the name index, the global search, decode and playback. **todo**
4. **Replace and add variations**: the encoder, the bank writer (DIDX/DATA rebuilt, HIRC edited in place), containers' properties.
   **todo**
5. **Remove and delete** with the reference warnings; **new events** with their triggers. **todo**
6. **Music**: playlists, segments, tracks, transitions, switches, stingers. **todo**
7. **`.snd.meta` timeline, `.amb` list, the sound DB tables, the localised VO.** **todo**
8. **Recipes**: music tracks, unit voice set, advisor/VO line, animation sound. **todo**

## Open questions and risks

- Whether Attila's Wwise runtime accepts our Vorbis streams is only known once one plays in game (step 1.4).
- `event_data.dat`: the meaning of the events' f32 value and the difference between state groups A and B are still open.
- Music editing is the hardest part to keep byte-identical; the oracle can generate reference objects for it.
- pywarscape-sound (MIT) reports v112
  round trips and PCM WEMs (32 kHz mono) that load in Attila: a fallback if the encoder fails, and a cross-check for the bank writer.
