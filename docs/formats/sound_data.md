# The game's other sound data

`event_data.dat`, `.snd.meta` with `sound_animation_triggers.csv`, and campaign ambience (`.amb` + `.xml`). Read and written by
`wce_audio_format` (`event_data`, `triggers`, `amb`) and `wce_formats::anim_meta` (`.snd.meta`); gates `audio_formats_check` and
`unit_formats_check`. Checked 2026-11-19 on Attila + TDD v1.2.0 and Rome 2: every copy written back byte for byte.

## `audio/event_data.dat`

The game's list of sound names. u32 integers, strings as u32 length + UTF-8. Six sections, in order:

1. Events: [n] (name, 4-byte value read as f32: 0 for most, 57 distinct values in Attila such as 32, 35, 133, 140; meaning not known).
2. State groups: [n] (name, [n] state names): `VO_battle_actor`, `VO_battle_culture`, `VO_battle_order` (517 states)...
3. More state groups, same shape: `Mixer`, `Music_Dynamic`, `animation_data_pool`...
4. Dialogue events: [n] (name, [n] u32 state group IDs).
5. Game parameters: [n] names (`meter_advice`, `setting_music_battle_intensity_...`).
6. Dialogue trees: [n] (u32 dialogue event ID, [n] (u32 state ID, [n] u32 child state IDs)).

Attila: 1,864 events, 32 + 37 groups, 171 dialogue events, 393 parameters, 98 trees. Rome 2: 1,361 (`sound.pack`) and 1,523
(`sound_rome2.pack`) events. Every string hashes (FNV-1, lower case) to an ID the banks use, so the file is a names source. The same
layout reads exactly in all three files; TheAssetEditor's variant with min/max floats per parameter fits neither game.

## `.snd.meta` and `audio/sound_animation_triggers.csv`

A `.snd.meta` sits next to an animation (`animations/.../<clip>.snd.meta`, 2,924 in Attila's `data.pack`) and has the `.meta`
layout (`wce_formats::anim_meta`): u32 version 2, u32 count, entries (u16-length kind, u32 version, f32 start, f32 end, u16-length
tag, fields). Its entries are `SOUND_TRIGGER` (two u32: the trigger, then 0 in the files seen) at the frame the sound fires.

The trigger is a 0-based data-row index into `sound_animation_triggers.csv` (`bird_cycle.snd.meta`'s 84 is `50_Bird_Wing_Flaps`; a
boar's 92 to 94 are footsteps). The row names the events it fires (`sound_event_name_battle`, `..._battle_end`,
`..._campaign`, `..._campaign_end`) and its conditions (entities inside and outside, ground, entity, armour, gender, melee weapon,
shield...). Attila: 168 rows, 24 columns; Rome 2: 163 rows, 18 (`sound.pack`) and 20 (`sound_rome2.pack`) columns.

## Campaign ambience: `audio/<map>.amb` + `.xml`

`.amb` (Attila: `main_attila_map`, `bel_attila_map`, `cha_attila_map` in `data.pack`; Rome 2: seven maps): u32 count, then per
emitter a u32 event ID, a u32 point count and the points (f32 x, y, z; y is 0 in every file). The events are
`campaign_spot_ambience_*`: coastal waves (cliff, beach), forests (alpine, European, Mediterranean, desert), rivers, lakes,
mountains, open fields, a volcano. Attila's main map: 2,799 emitters, 5,798 points. The points are campaign map coordinates (main map
x 0 to about 1,000).

The `.xml` of the same name: `<settings>` with the listener scaling factor and the climate blend distances and offsets (start, end),
optionally per season (`season="Winter"`).
