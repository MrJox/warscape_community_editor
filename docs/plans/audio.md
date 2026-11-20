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
| Encode | Wwise Vorbis, as vanilla, with aoTuV (libvorbis + aoTuV's encoder, C, BSD-3, built from source like SDL3) and our own Ogg -> Wwise Vorbis conversion: chosen after the spike (user, 2026-11-19; step 1.4). |
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
cbSize 0x30, a u16 (0 from Wwise 2014 for 16-bit WAVs; vanilla 5, 4 and 6 channel files have 16 or 24, apparently the source's
bit depth: metadata), the channel mask: mono 0x4, stereo 0x3, quad 0x603, 5 channels 0x607, 5.1 0x60F. Data: [seek table], the setup packet (u16 size + payload), then audio packets (u16
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

1. **Spikes.** **done** (1.4.10 carried into step 4)
   1. Rome 2's banks' version: v112 wherever the game loads them (its v88 release banks are all overridden). **done**
   2. The vanilla WEM survey (above). **done**
   3. The Wwise 2014.1.3 oracle (above). **done**
   4. The encoder (aoTuV vs native). **done: aoTuV**
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
      6. Realistic comparison (`tools\compare.sh 0.4`): 7 vanilla WEMs decoded (mono 28/32/44.1 kHz, stereo 28/44.1 kHz, 5 and 6
         channels), re-encoded by Wwise (Voice preset) and by aoTuV q0.4. Every file: **the same setup as Wwise's** (hash and alloc
         sizes); size -0.1 % to -3.1 % against Wwise's; channel order kept; SNR against the source equal or higher (Wwise's output
         is noisier, gain 0.89 to 0.98: stronger noise normalisation in its encoder). Bit-exact audio: no, at any input scaling
         (/32767, /32768, /65536) or nearby quality; Wwise's encoder is a modified aoTuV, and matching it bit for bit would need its
         encoder reverse engineered (a native encoder would not be closer). **done**
      7. Exact packet sizes: aoTuV's exact bit count per packet is `oggpack_bits(&vorbis_block.opb)` right after
         `vorbis_analysis` (public, no patch); a packet is ceil((bits - 1 - 2 for long blocks) / 8) bytes. With it 38 % of our
         packets end in a 0x00 byte, as Wwise's (37 %); decoding stays identical to aoTuV's own Ogg. **done**
      8. In-game test. `269336611.wem` is the 177 s quad stream `frontend.bnk` plays (the main menu's ambience; streamed, not in any
         bank's DIDX; TDD does not override `frontend.bnk`). Test signal `spike\ingame\src\beeps_quad.wav`: 30 s of beeps stepping
         FL 440 Hz, FR 660, RL 880, RR 1100, every 0.5 s. Packs in `spike\ingame\packs\`: `wce_audio_test_ours.pack` (aoTuV q0.4,
         converted) and `wce_audio_test_wwise.pack` (the control: Wwise's own encode of the same WAV). Test: one pack at a time in
         `data\`, enabled in the launcher, start the game, listen on the main menu; remove the pack after. **done** (user,
         2026-11-19: our pack plays; the beeps replace the horse-sound ambience layered under the main menu music; the other layer,
         vanilla's throat-singing music, is a different stream). The control pack was not needed.
      9. Verdict: **aoTuV** (user, 2026-11-19). Built from source like SDL3 (libogg + aoTuV, BSD; `THIRD_PARTY.md` when it goes
         in, step 4): plays in game, same setups and headers as Wwise, equal or better quality, slightly smaller; not bit-identical,
         which no encoder short of Wwise's own would be. **done**
      10. Decoder alloc sizes for setups not seen yet: they depend only on the setup (the same for mono and quad); the table covers
          the 19 setups seen; a formula (from the codebooks) or more oracle conversions per quality and channel layout. **todo,
          moved to step 4** (the encoder needs it, nothing before it does)
2. **Formats** with the round-trip gates (crate `wce_audio_format`). **done** (2026-11-19)
   1. Crate, FNV-1 IDs (`fnv::id`, `fnv::bytes`), bank chunks (`BKHD`, `DIDX`, `DATA`, `HIRC` objects as kind + ID + raw body,
      `STID`, others raw), WEM RIFF chunks + the `fmt`/Wwise Vorbis header view, `event_data.dat` typed; unit tests. **done**
   2. Gate `audio_formats_check <game> [mod packs]` (in `scripts/gates.sh`: `audio_formats`, `rome2_audio_formats`): every copy of
      every bank, WEM and `event_data.dat`, written back and compared; Vorbis codebook hash vs setup. **done**: Attila + TDD v1.2.0
      134 banks (all v112, the encrypted music packs too), 44,802 WEMs, 1 `event_data.dat`, 0 failures, all hashes match (53 s);
      Rome 2 159 banks (42 v88, 117 v112), 42,371 WEMs, 2 `event_data.dat`, 0 failures. HIRC object counts (Attila + TDD): Sound
      95,259, Random/Sequence 12,514, Action 3,318, Event 2,308, Switch 2,123, Music Segment 624, Music Track 600, Attenuation 546,
      Blend 289, Dialogue Event 220, Music Playlist 206, Effect 200, Actor-Mixer 199, Bus 198, Effect ShareSet 136, State 103,
      Music Switch 58, Aux Bus 14.
   3. Typed HIRC objects, v112 (`hirc::parse`, `Object::write`), one kind at a time, each gated (parse consumes the body
      exactly, writes it back identically); the bank keeps bodies raw, so untyped kinds still round-trip. Layouts learnt from
      pywarscape-sound's models (MIT), written anew. **done**
      - **done** (Attila + TDD, 0 failures): State 103, Sound 95,259, Action 3,318 (type + target typed, parameters raw), Event
        2,308, Random/Sequence 12,514, Switch 2,123, Actor-Mixer 199, Blend 289. Rome 2's v112 banks: 0 failures too (Sound
        83,677, Random/Sequence 13,103...). `hirc::parse` takes the bank version: v88 banks (Rome 2's release packs, all
        overridden) are not typed, they round-trip as chunks.
      - **done** (both games, 0 failures): Bus 198 + Aux Bus 14, Attenuation 546, Dialogue Event 220 (decision tree as its flat
        12-byte nodes, `DialogueEvent::leaves`), Effect 200 + ShareSet 136, Music Segment 624, Track 600, Switch 58, Playlist
        206 (`hirc/music.rs`). Every v112 kind the games use is typed.
      - **todo** (later, when editing needs it): Action parameters per action type (kept as bytes), the music transition rules'
        source/destination/transition blocks (kept as bytes), Effect plug-in parameters per plug-in.
   4. `.snd.meta`: the `.meta` layout, already read and written byte for byte by `wce_formats::anim_meta` (gate
      `unit_formats_check`); its `SOUND_TRIGGER` entries hold a 0-based row index into `sound_animation_triggers.csv` (checked: a
      bird's 84 is `50_Bird_Wing_Flaps`, a boar's 92 to 94 footsteps). The audio layer joins them (step 7). **done**
   5. Campaign `.amb` (`amb::Ambience`: emitters = event ID + polyline; IDs are `campaign_spot_ambience_*` events): all 3
      Attila and 14 Rome 2 copies byte for byte. The `.xml` beside it is plain settings XML (listener scaling, climate blends),
      read with `wce_formats`' XML when the tool needs it. **done**
   6. `sound_animation_triggers.csv` (`triggers::Triggers`: kept as bytes, columns read): Attila 168 rows x 24 columns, Rome 2
      163 x 18 and 20; every row has the header's field count. **done**
   7. Format docs: [wwise_bank.md](../formats/wwise_bank.md) (chunks, every HIRC layout), [wem.md](../formats/wem.md) (RIFF,
      the Wwise Vorbis header, the packing and encoding), [sound_data.md](../formats/sound_data.md) (`event_data.dat`,
      `.snd.meta` + triggers, `.amb` + `.xml`). **done**
3. **Browse and listen**: the bank documents, the name index, the global search, decode and playback. **done** (2026-11-19; playback
   heard by the user)
   1. Crate `wce_audio` (headless): the Wwise codebook library (ww2ogg's `packed_codebooks_aoTuV_603.bin`, BSD-3, vendored in
      `third_party/ww2ogg`; user 2026-11-19), the Wwise Vorbis -> Vorbis packet rebuild (`tools\wwvorbis.py`'s `wem2ogg`, in Rust,
      no Ogg container), decoding with `lewton` 0.10 (no default features; user-approved). `wce_audio::decode::decode_wem` ->
      `Pcm` (interleaved f32, WAVE channel order). About 400x real time (a 9-minute track in 1.3 s): decode on a worker. **done**
   2. Decode gate `audio_decode_check` (`scripts/gates.sh`: `audio_decode`, `rome2_audio_decode`): every WEM the game loads
      decodes to its header's frame count. Attila + TDD: 44,703 loose WEMs and 25,140 media inside banks (51 hours), 0 failures;
      12 bank media are prefetched starts of loose files (stream type 1: only the start is in the bank) and 1 is plug-in data
      (not RIFF). Rome 2: 41,088 loose WEMs, 0 failures. **done**
   3. Name index (`wce_audio::names`): `event_data.dat`, `STID`, bank file names, the triggers CSV, TheTollski's `TWA_Names.txt`
      (vendored in `third_party/soundbank_editor`, user's permission), user labels (`NameSource::Label`, first in priority; the
      UI to set them is step 3.6 or later); FNV-1 match. Attila + TDD: 3,559 names; every event (1,629) and dialogue event (171)
      named, 22 of 198 buses; containers and sounds have no names in Wwise (the tool labels them by content). DB tables as a
      source: later. **done**
   4. Resolver (`wce_audio::library::SoundLibrary`, example `audio_library_check`): loads every bank through `Files` (packs +
      loose project files; 126 banks, 98,009 objects in 0.2 s), indexes objects and media by ID across banks, resolves an object
      to a `PlayNode` tree (event -> actions -> play targets -> containers / switch values / dialogue tree leaves / music ->
      sounds -> media in a bank's `DATA` or a loose `audio/<id>.wem`). All 1,629 events in 0.1 s: 215,360 sound leaves, 204,360
      in banks, 9,635 loose, 1,365 missing (195 sound objects whose media is in no bank or pack: the game's own leftovers,
      silent in game too); 280 events play nothing (stop, set state...). **done**
   5. Playback (`wce_audio_tool::player`): one SDL3 audio stream per sound on the default device (SDL converts rate and
      channels), stop, progress; decoding on a worker thread. **done, not yet heard by the user** (unattended runs must not make
      sound).
   6. The Audio tool (`wce_audio_tool`, feature `audio`, on by default; icon `AUDIO_LINES`): panels Audio Banks (filter, every
      bank, the player strip), Audio Bank (tabs Events, Dialogue events, Music, Sounds in the bank, All objects; each event a
      Play button and a tree of what it plays, unnamed containers described: "Random of 3", "Switch on <group>"), Audio Search
      (names or #IDs across every bank), Audio Object (name and its source, banks, fields by kind, parent, bus, properties,
      children; Play). Event Play picks one sound: random and switch containers at random. Script commands `audio_bank`,
      `audio_search`, `audio_select`, `audio_expand`. The library loads on a worker when the game's files are mounted. **done**
   7. Seen working in the app (2026-11-19, `wce.exe --tool audio --cmd "audio_expand battle_bird_wing_flaps" --shot`): banks,
      events with names, the expanded tree, Audio Object. **done**; playback by ear: the user.
   - Follow-ups (later steps): property IDs beyond volume (0), pitch (2) and low-pass (3) to name (check with the oracle);
     labels set by the user (UI); DB tables as a names source; Wwise-faithful playback (blend layers mixed, sequences in order)
     is the Cinematic layer's (not chosen for v1).
4. **Replace and add variations**. **done** (2026-11-20; known anomaly: UI button sounds, 4.7)
   1. Decoder alloc sizes (1.4.10): oracle grid (scratch `tools\setup_grid.py`: Wwise at qualities 0 to 10 x 1, 2, 4, 5, 6
      channels x 8 rates, 440 conversions, against aoTuV at Q/10): Wwise uses one setup per quality and channel layout at any
      rate (chosen as for 44.1 kHz); ours matches for qualities 0 and 2 to 10 with 1 to 5 channels; quality 1 and 5.1 differ
      (docs/formats/wem.md). Table `wce_audio::setup_allocs` (35 setups). Import offers qualities 0 and 2 to 10, 1 to 5 channels;
      6 channels: todo (force the uncoupled template, or downmix). **done**
   2. Encoder in `wce_audio::encode`: `aotuv_lancer_vorbis_sys` 0.1.6 (crates.io, BSD-3; user 2026-11-19), set up for 44.1 kHz
      whatever the rate, exact packet bits from `vorbis_block.opb`, the Ogg -> Wwise conversion of `tools\wwvorbis.py` (setup,
      duplicate-ID rule, packets, header fields): `wce_audio::encode::encode_wem(pcm, quality)`, `QUALITIES` (0, 2 to 10),
      `DEFAULT_QUALITY` 4. The crate's (Lancer) aoTuV gives Wwise's Voice setup byte for byte (test). Gate `audio_encode_check`
      (`scripts/gates.sh`: `audio_encode`): 10 qualities x 1, 2, 4, 5 channels x 8 rates = 320 encodes, every setup known, one
      setup per quality and channels over the rates, all decode back to their frame count. Mono 44.1 kHz: 53 kbps (quality 0)
      to 252 (10); quality 4: 77. **done**
   3. Audio import (`wce_audio::import::read_sound`): WAV (PCM 8/16/24/32-bit, float 32/64, extensible), Ogg Vorbis (pages read
      here, lewton decodes), WEM; any rate (no resampling: the WEM keeps the rate). **done**
   4. Bank writer and edits. The game's rules (checked on every Attila bank): media in `DATA` at 16-byte boundaries, zeros
      between, nothing after the last; an in-bank source has `file_id` = its media bank's ID, `file_offset` = the media's offset
      from the start of that bank's file, `in_memory_size` = its size (all 38,117); a streamed one `file_id` = its media ID,
      `in_memory_size` 0. `Bank::set_media` / `remove_media` / `data_position` / `objects_mut`; `wce_audio::edit::EditSet`:
      `replace_media` (banks and loose file; a prefetched start keeps its length), `add_variation` (a sound copied from the
      container's first sound, new IDs, media in the same bank or loose), `remove_variation`, `set_property`, `fix_offsets` (every
      source into a changed media bank). Gate `audio_edit_check` (`audio_edit`) on Attila + TDD: 0 inconsistent sources as
      shipped; replace by the same bytes changes nothing; replace by a new sound edits 8 banks, stays consistent, decodes; add then
      remove a variation gives the banks back byte for byte; a property reads back. **done**
   5. Saving: the Audio tool keeps an edit session (`EditSet`) with Undo / Redo (20 steps), Revert, and Save, which writes the
      edited banks and sound files to `<projects>/loose/<path>` (read before the packs) and tells the editor
      (`loose_changed`, so the library reads them again); unsaved edits are listed on exit (`unsaved_work`). **done**; follow-up:
      the editor's Document / History (File > Save, autosave, the History panel) instead of the tool's own session.
   6. Tool: Audio Object offers Replace audio... (a sound: WAV, Ogg Vorbis or WEM, encoded on a worker at the chosen import
      quality), Add variation... (a random/sequence container), Remove (each child of such a container), Volume and Pitch
      (any node); edited objects show at once ("Edited, not saved"), the trees after Save; Play plays the edited audio. Seen in the
      app (2026-11-19, `audio_select 715511747`). **done**; follow-ups: weights and mode, edits shown in the trees before Save.
   7. In game: an in-bank replace. `grp_gw_generic_button` (global_ui, the generic button) plays media #25654632 (click) and
      #13563058 (mouse-over), held in `battle_ui_data` and copied in `campaign_ui_data` and `global_ui_data`; both replaced through
      `EditSet` with test tones (click: 660 then 990 Hz; mouse-over: a short 1,760 Hz blip). 6 banks edited (the 3 data banks
      and `battle_ui`, `campaign_ui`, `global_ui`, whose offsets moved). Pack: scratch `spike\ingame2\wce_audio_test_ui.pack`
      (vanilla banks; TDD overrides `campaign_ui`, so test without TDD). Result (user, 2026-11-19): **the original button sounds**,
      with the pack enabled alone (`used_mods.txt`). The main menu layout (`frontend ui/main`) does use
      `grp_gw_generic_button_small`, whose `lclick` / `mouse_on` play exactly the replaced media; every copy of them was
      replaced. Next: `spike\ingame2\wce_audio_test_ui2.pack` = the same banks + the main menu ambience WEM that worked in 1.4.8,
      to tell "the pack does not load" from "the game does not take the edited banks". Result: the ambience beeps, the buttons
      stay original: **the game does not take edited `*_data.bnk` banks from a mod pack** (TDD, also a Mod pack, only ever ships
      event banks, never data banks), while loose WEMs and event banks from a mod pack do load. So replacing in-bank media now
      redirects: `EditSet::replace_audio` (the tool's Replace) writes a streamed media file again under its ID, and for media in a
      data bank points every source using it at a new loose `audio/<new id>.wem` (`redirect_media`; stream type 2, no data bank
      touched); `add_variation` streams its new sound from a loose file too. Gate 3b checks it. Test pack 3:
      `spike\ingame3\wce_audio_test_ui3.pack` (`battle_ui`, `campaign_ui`, `global_ui` + the two new loose WEMs + the ambience
      control). Result of pack 3 (user): ambience beeps, buttons still original. **Open; next session starts here.**
      Findings so far: a UI component names its sound event in the string the UI XML labels `end of uientry 1?` (wce-ui to-xml);
      over the front-end and common layouts the most used are `grp_gw_generic_button_small` (248 components),
      `grp_gw_generic_button` (118), drop-downs, tick boxes, sliders; `frontend ui/main` sets only two (the small button and
      `ind_fe_start_campaign`), so the big main-menu buttons take their sound elsewhere (template, skin or the game's code).
      (1) The big buttons' event: likely `grp_fe_tutorial_main_buttons` (global_ui; no layout sets it, so the game's code fires
      it), whose click / hover play the same media (#25654632, #13563058) as the generic buttons. (2) The decisive pack:
      `spike\ingame4\wce_audio_test_ui4.pack`: all 60 media of every `global_ui` and `frontend` event redirected to one 880 Hz
      beep (`battle_ui`, `campaign_ui`, `frontend`, `global_ui` + 60 loose WEMs) + the ambience control. Beeping UI: the redirect
      works and pack 3 tested the wrong event; original UI: event banks of the UI are not taken from a mod pack either (loaded
      before mods?) and UI sounds need another route. Result (user, 2026-11-20): the main menu ambience beeps, the buttons (most
      of them) sound original. **Known anomaly, set aside by the user**: UI button sounds do not change through a mod pack's
      banks (event or data banks), while loose WEMs do; cause unknown (UI banks loaded before mods, another event, or cached
      media). Revisit with a runtime trace or a non-UI event (a battle sound) later. An added variation in game: todo.
5. **Remove and delete** with the reference warnings; **new events** with their triggers. **done** (2026-11-20)
   1. References finder (`wce_audio::references`): `ReferenceIndex::build` (once, about 5 s: on a worker) indexes the game's
      references by ID: `event_data.dat` (the game's code triggers these events by name), `sound_animation_triggers.csv` rows,
      campaign `.amb` emitters, UI layouts (their words hashed, kept when a bank has that ID); `references::find` adds the
      banks' own (child of, action of, target of, switch value of, in the tree of, in the playlist of), as edited. Gate step 6:
      `grp_gw_generic_button_small` 79 references (UI layouts), `battle_bird_wing_flaps` (the triggers CSV),
      `campaign_spot_ambience_coastal_waves_cliff` (`.amb`), `Battle_Order_VO_Mute` (`event_data.dat`), a container's parent and
      Play action. **done**
   2. Delete (`EditSet::delete_object`): the object, its subtree (children whose parent it is), an event's actions, the Play
      actions on any of them; every list naming them drops them (events, random/sequence children and playlists, switch
      children / assignments / child parameters, actor-mixers, blend children and layers); media stay (the game's packs are not
      changed); loose files of these edits no longer used are dropped. Music objects: step 6. Gate step 7: a container with
      its 4 sounds and its Play action (6 objects), nothing left naming them, the banks re-read. **done**
   3. New events (`EditSet::new_event`): a named event (ID = FNV-1 of the name, letters / digits / `_`) with a Play action on an
      object of the bank, the action copied from a Play action of that bank; the name kept in `EditSet::names`. Gate step 8.
      **done**
   4. Wiring: `EditSet::add_trigger` (a `sound_animation_triggers.csv` row firing battle / campaign events, its conditions
      copied from another row; returns the row index a `.snd.meta` holds; the frames themselves: step 7) and
      `EditSet::add_emitter` (a campaign `.amb` emitter along points). Gate step 8: row 168 added, an emitter added. **done**;
      the "nothing triggers it" warning: with the tool (5.5).
   5. Tool: Audio Object lists "Used by" (the game's references and the banks', objects selectable; the index builds on a
      worker after the banks), Delete... (a confirmation listing the game's uses as warnings), "New event playing this" (a name,
      for sounds and containers; the event lands in the object's first bank), "Add to the animation triggers" (an event: a
      new CSV row firing it in battle; the row index is what a `.snd.meta` frame holds), and the warning "Nothing triggers this
      event" for an event nobody names. New objects show from the edits ("New, not saved"); names given here are kept in
      `<projects>/audio/names.txt` and read with the banks (`NameSource::Label`). **done**; seen in the app (2026-11-20, the
      "Used by" section under a playlist).
6. **Music**: playlists, segments, tracks, transitions, switches, stingers. **done** (2026-11-20; 6.4 follow-ups open)
   1. How the game's music is built (Attila, TDD's music banks): a playlist's items are a depth-first tree, groups (no segment,
      a child count, a play kind: 0 sequence continuous, 1 sequence step, 2 random continuous, 3 random step) and leaves (a
      segment, kind 0xFFFFFFFF, weight 50,000); a segment holds one track with one streamed source and one clip; its duration
      is the entry cue's position plus the clip's end; markers: the entry cue (43573010) and the exit cue (1539036744, at the
      duration). **done**
   2. Music edits (`wce_audio::music`, on `EditSet`): `replace_segment_audio` (the track's source redirected to a new loose WEM,
      the clip the whole sound, the segment's duration and exit cue fitted), `add_playlist_entry` (a segment and track copied
      from a leaf's, the new leaf after it in its group, the group's count +1, new audio loose), `remove_playlist_entry` (the
      leaf, and its segment and tracks when nothing else plays them), `set_playlist_item` (weight, loop); `item_tree`,
      `group_kind` for display. Gate `audio_music_check` (`audio_music`) on Attila + TDD, a 22-item playlist with nested
      random groups: the entry added keeps the tree whole and fits its 3 s sound, removing it gives the banks back byte for byte,
      a segment's replace fits, weight and loop read back. **done**
   3. Tool: Audio Object shows a playlist's entries as a tree (groups with how they play and loop, segments with their length),
      each segment with Play, "Add after..." (a sound file, a new segment in the same group), Remove and its weight; a segment
      offers "Replace music..."; the playlist's transition rules and stingers are counted. Seen in the app (2026-11-20,
      `audio_select 487233623`: global_music's 22-entry playlist, 5 random groups). **done**; loop per entry: in the library,
      not yet in the tool.
   4. Later (follow-ups): editing transition rules (their source / destination / transition-segment blocks are kept as bytes),
      stingers, music switch trees (which playlist plays for which state), segments with several tracks, new playlists.
7. **`.snd.meta` timeline, `.amb` list, the sound DB tables, the localised VO.** **done** (2026-11-20)
   1. Animation sounds (`wce_audio::anim_sounds`): a `.snd.meta`'s `SOUND_TRIGGER` frames (time, trigger row), the row's
      events, `add_frame` / `move_frame` / `set_trigger` (`Meta::encode` writes the file again; unit test). Tool: the panel
      Audio Game Data (docked beside Audio Bank), tab Animations: every `.snd.meta` (2,943 with the user's mods), a filter, the
      chosen one's frames as marks on a timeline (seconds), and per frame Play (its battle event), time, trigger row, the
      trigger's name and event (selects it), Remove; "Add a sound frame" with a row. Edits are loose `.snd.meta` files of the
      edit session. Script command `audio_animation <part of a path>`. Seen in the app (2026-11-20, `boar_charge.snd.meta`:
      4 footstep frames, rows 91 to 94). **done**; follow-ups: the clip's real length (from its `.anim`), dragging frames on the
      timeline, the unit's model (the Unit / Cinematic tools).
   2. Campaign ambience: tab Campaign ambience: the maps' `.amb`, a map's emitters grouped by sound (emitters, points), Play and
      select. Placing emitters on the map: the campaign / terrain editor. **done**
   3. Sound DB tables (`wce_audio::sound_db`, rpfm's DB decoder and schema): Attila's sound tables are small:
      `audio_projectiles` (28 rows: fire, in-flight, impact, idle, hit-tree events per projectile) and `audio_explosions`
      (10) name events; `subculture_religion_voiceover_junctions` (120) gives the voice-over culture; `main_units.voiceover`
      each unit's; `sound_events`, `voice_types`, `unit_voice_categories`, `audio_campaign_building_enums` are key lists;
      `voice_events`, `unit_voice_events_junctions`, `agent_voice_events_junctions` are empty. Tab DB tables: the events the
      tables name (Play, select), unit voice-overs, voice-over by subculture and religion. The references gain DB rows and
      animations (`Reference::Db`, `Reference::Animations`: how many `.snd.meta` fire an event); gate step 6:
      `projectile_fire_arrow` (DB), `battle_footstep_walk_01` (animations). **done**; editing DB rows: with RPFM for now.
   4. Localised VO: the voice banks and files of the language the game is set to come from its `local_*` packs and are read
      with the others (banks, search, playback); tab Languages lists the voice banks. Attila installs one language (English).
      **done**
8. **Recipes**: music tracks, unit voice set, advisor/VO line, animation sound. **done** (2026-11-20; a new culture: follow-up)
   1. Library (`wce_audio::recipes`, `EditSet::new_sound_like`, `hirc::tree_paths`): `music_playlists` names the 157 playlists by
      the state values on the music switch paths choosing them ("Barbarian, prebattle / Barbarian", "BattleMusic_BigCombat,
      Bb / ..."); `animation_sound` adds a `.snd.meta` frame firing an existing event (its trigger row reused) or a new sound
      (made like another event's first sound, under the nearest actor-mixer or alone on the chain's bus; a new event; a trigger
      row) ; `culture_lines` / `group_values`: the battle voice trees decide on `vo_battle_subculture`, whose values
      (`Roman`, `barbarian_christian`, `nomad_hunnic_generic`...) are the DB's voice-over cultures. Gate `audio_recipes_check`
      (`audio_recipes`) on Attila: playlists named, a frame on existing row 91, the whole new-sound chain (frame -> row ->
      event -> sound -> loose media that decodes), 87 `Roman` lines of `battle_vo_land_idle`. **done**
   2. Tool: the panel Audio Recipes (docked beside Audio Bank): Music tracks (playlists by name, "Add a track...", select for
      the entries), A voice line (voice and advice events, Play, select, then Replace audio), A culture's voice lines (a voice
      event, a culture, its lines with Play and Replace audio), An animation sound (the animation picked in Audio Game Data, a
      time, an existing event or a new sound file). Script command `audio_show <panel>`. Seen in the app (2026-11-20). **done**
   3. Follow-up: a new voice-over culture (a branch in every voice tree for a new `vo_battle_subculture` value, and the units'
      `voiceover` / the DB junctions), which needs decision-tree insertion.
9. **Player and visuals** (user, 2026-11-20): a nicer, more useful player: the sound's waveform (amplitude envelope) as the
   progress bar, as is common now; seek by clicking it; the bar reset when the sound ends; play / pause / stop, loop, volume;
   waveforms next to sounds in the trees; overall polish of the Audio tool's panels (icons, colours, spacing). **done**
   (2026-11-20; the controls by ear: user)
   1. Player (`wce_audio_tool::player`): keeps the decoded sound, so it seeks, loops and replays; `peaks` (the loudest sample
      of 600 slices, all channels) for the waveform; pause / resume, stop (back to the start, the sound stays loaded), loop,
      volume 0 to 150 % (SDL stream gain); at the end it loops or goes back to the start (the bar resets). Unit test
      `envelope`. **done**
   2. Player strip (top of Audio Banks, `tool/player_view.rs`): play / pause, stop, loop, volume, time / length, the sound's
      name, and its waveform as the progress bar (played part in the accent colour, a playhead); a click seeks and plays.
      **done**
   3. Waveforms: Audio Object shows a selected sound's waveform (a click plays from there); every sound row of the trees has a
      small one. Read on a worker one at a time, only for what is shown, cached by media ID and read again after an edit,
      undo or redo. Each column draws the loudest of the peaks it covers. **done**
   4. Polish: Lucide icons on the Play buttons and the strip; `audio_expand` opens the event's first levels (down to the
      sounds of most events), and a select queued with it keeps the selection. Seen in the app (screenshots, no sound played
      by the unattended runs, 2026-11-20). **done**
   5. To check by ear (user): pause, seek by clicking, loop, volume, and the bar reset at the end. **todo (user)**

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
- Open leads: how the engine sets `music_culture` and `vo_campaign_character` (a spike in `empire.retail.dll`: the strings and
  their callers); whether the empty voice junction tables are still read by the engine (a test pack with rows).

## Open questions and risks

- Attila's Wwise runtime plays our aoTuV-encoded Vorbis WEMs (step 1.4.8, a streamed WEM replaced by a mod pack). Not yet tested:
  media held in a bank's DIDX/DATA (needs the bank writer, step 4).
- `event_data.dat`: the meaning of the events' f32 value and the difference between state groups A and B are still open.
- Music editing is the hardest part to keep byte-identical; the oracle can generate reference objects for it.
- pywarscape-sound (MIT) reports v112
  round trips and PCM WEMs (32 kHz mono) that load in Attila: a fallback if the encoder fails, and a cross-check for the bank writer.
