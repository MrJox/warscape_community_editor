# Wwise sound banks (`.bnk`)

Read and written by `wce_audio_format::bank` (chunks) and `wce_audio_format::hirc` (typed objects); gate `audio_formats_check`
(`scripts/gates.sh`: `audio_formats`, `rome2_audio_formats`). Checked 2026-11-19: every copy of every bank of Attila + TDD v1.2.0
(134, all version 112) and Rome 2 (159: 117 v112, 42 v88) is written back byte for byte, and every v112 object of every kind parses
and writes back byte for byte. Layouts learnt from pywarscape-sound's models (MIT) and checked against the corpus; the code is new.

## Where banks are

`audio/*.bnk` in `sound.pack`, `music.pack` (data-encrypted: PFH4 flag 0x10; rpfm_lib's develop branch decrypts it, rpfm_cli 4.3.14
does not), `local_en_shared_rome2.pack` (VO) and mods' packs. The game loads banks by name, so a mod changes a bank by shipping a
whole one with the same path. Rome 2's release banks (`sound.pack`, `music.pack`) are v88, but its patch packs (`sound_rome2.pack`,
`music_rome2.pack`) override every one with v112, and an empty `animation_data.bnk` removes that bank.

Two kinds of bank: `*_data.bnk` hold only media (`BKHD`, `DIDX`, `DATA`); the others only the hierarchy (`BKHD`, `HIRC`, often
`STID`). `init.bnk` also has `STMG` and `ENVS` (kept as bytes).

## Chunks

A 4-byte tag, a u32 size, the data; nothing between chunks. All integers little-endian.

| Chunk | Layout |
|---|---|
| `BKHD` | u32 bank version (112), u32 bank ID (FNV-1 of the name), u32 language ID, then 12 bytes (20 in `init.bnk`): feedback flag, project ID, padding |
| `DIDX` | (u32 media ID, u32 offset in `DATA`, u32 size) per media file |
| `DATA` | the media (`.wem` files), at the index's offsets |
| `HIRC` | u32 count, then per object: u8 kind, u32 size, `size` bytes starting with the object's u32 ID |
| `STID` | u32 kind (1), u32 count, then (u32 bank ID, u8 length, name) |

IDs are FNV-1 32 of the lower-case name (`fnv::id`): offset basis 2166136261, multiply by 16777619 then xor each byte.

## Hierarchy objects (version 112)

Counts are of Attila + TDD. `[n]` a u32 count then n items unless noted; `f32`/`f64` floats.

| Kind | Object | Count | Body after the ID |
|---|---|---|---|
| 1 | State | 103 | props (u8 n, n u8 IDs, n f32 values) |
| 2 | Sound | 95,259 | Source, NodeBase |
| 3 | Action | 3,318 | u16 type (high byte: 0x04 play, 0x01 stop, 0x12 set state...), u32 target, u8 target is bus, parameters by type (kept as bytes) |
| 4 | Event | 2,308 | [n] u32 action IDs |
| 5 | Random/Sequence | 12,514 | NodeBase, u16 loop count, u16 loop min, u16 loop max, f32 transition time, f32 min, f32 max, u16 avoid repeat, u8 transition mode, u8 random mode, u8 mode (0 sequence, 1 random), u8 bits, [n] u32 children, u16 n then (u32 child, i32 weight) |
| 6 | Switch | 2,123 | NodeBase, u8 group type (0 switch, 1 state), u32 group, u32 default, u8 continuous validation, [n] u32 children, [n] (u32 switch value, [n] u32 nodes), [n] (u32 node, u8, u8, f32 fade out, f32 fade in) |
| 7 | Actor-Mixer | 199 | NodeBase, [n] u32 children |
| 8, 20 | Bus, Aux Bus | 198, 14 | u32 parent bus; then, unless the object ends there: props, Positioning, u8 advanced bits, u16 max instances, u32 channel config, u8 HDR bits, i32 recovery time, f32 max duck volume, [n] ducks (u32 bus, f32 volume, i32 fade out, i32 fade in, u8 curve, u8 target), effect slots, u8 override attachment, u32 effect 0, u8 is share set, RTPCs, state groups |
| 9 | Blend | 289 | NodeBase, [n] u32 children, [n] layers (u32 ID, u16 n RTPCs, u32 crossfade RTPC, u8 its type, [n] (u32 child, [n] curve points)) |
| 10 | Music Segment | 624 | MusicNode, f64 duration (ms), [n] markers (u32 ID, f64 position, [n] name bytes) |
| 11 | Music Track | 600 | u8 flags, [n] Source, [n] clips (u32 track, u32 source, f64 play at, f64 begin trim, f64 end trim, f64 source duration), u32 sub-track count if there are clips, [n] clip automation (u32 clip, u32 kind, [n] curve points), NodeBase, u8 track type (3 switch: u8 group type, u32 group, u32 default, [n] u32 switch values, 8 u32 transition fields), i32 look-ahead |
| 12 | Music Switch | 58 | MusicNode, transition rules, u8 continue playback, decision tree |
| 13 | Music Playlist | 206 | MusicNode, transition rules, [n] items of 30 bytes (u32 segment, u32 item ID, u32 child count, u32 kind, i16 loop, i16 min, i16 max, u32 weight, u16 avoid repeat, u8 use weight, u8 shuffle), depth first |
| 14 | Attenuation | 546 | u8 cone enabled, 5 f32 cone if enabled, 7 u8 curve per property (255 none), u8 n curves (u8 scaling, u16 n curve points), RTPCs if bytes remain |
| 15 | Dialogue Event | 220 | u8 probability, decision tree |
| 18, 19 | Effect ShareSet, Effect | 136, 200 | u32 plug-in, [n] parameter bytes, u8 n media (u8 index, u32 source), RTPCs if bytes remain, u16 RTPC init flags if 2 bytes remain |

Shared parts:

- **Source** (sounds, music tracks): u32 plug-in (0x40001 Vorbis), u8 stream type (0 in a bank's `DATA`, 1 prefetched + streamed, 2
  streamed: a loose `audio/<id>.wem`), u32 source ID, u32 file ID, u32 file offset unless streamed, u32 in-memory size, u8 bits,
  [n] parameter bytes when the plug-in's low nibble is 2 or 5.
- **NodeBase**: u8 override effects, u8 n effects, then if n > 0 a u8 bypass and n slots (u8 index, u32 effect, u8 is share set, u8
  rendered); u8 override attachment, u32 override bus, u32 parent, u8 priority bits; props; ranged props (u8 n, n u8 IDs, n (f32 min,
  f32 max)); Positioning; u8 aux bits (bit 3: 4 u32 aux buses follow); advanced settings (u8 bits, u8 virtual queue, u16 max
  instances, u8 below threshold, u8 HDR bits); state groups; RTPCs.
- **Positioning**: u8 bits; when bits 0 and 3 are set: u8 3D bits, u32 attenuation; when the 3D bits' low two bits are not 1 a path:
  u8 mode, i32 transition time, [n] vertices (f32 x, y, z, i32 duration), [n] items (u32 first vertex, u32 count), then per item 3
  f32 random ranges.
- **State groups**: [n] (u32 group, u8 sync type, u16 n (u32 state, u32 its property object)).
- **RTPCs**: u16 n, then (u32 RTPC, u8 type, u8 accumulation, u8 parameter, u32 curve, u8 scaling, u16 n curve points).
- **Curve point**: f32 from, f32 to, u32 interpolation.
- **MusicNode**: u8 flags, NodeBase, [n] u32 children, meter (f64 grid period, f64 grid offset, f32 tempo, u8 beats per bar, u8
  beat value, u8 flags: 23 bytes), [n] stingers (u32 trigger, u32 segment, u32 sync, u32 cue filter, i32 don't-repeat time, u32
  look-ahead).
- **Transition rules**: [n] (u32-counted source IDs, u32-counted destination IDs, 21-byte source rule, 24-byte destination rule, u8
  flag, 30-byte transition object when the flag is 1); the three blocks are kept as bytes.
- **Decision tree**: u32 depth, depth u32 group IDs, depth u8 group types, u32 tree size (bytes, a multiple of 12), u8 mode, then
  nodes breadth first: (u32 key, u32 value, u16 weight, u16 probability), the value being (u16 first child, u16 child count) above
  the last level and the audio node's ID at it.

Not typed yet (kept as bytes): an action's parameters per type, the transition rules' blocks, effect plug-ins' parameters.
