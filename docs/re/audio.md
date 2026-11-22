# Sound in the game: the files and what mods can change (Attila, Rome 2; 2026-11-19 / 20)

Where the game's sound files are and what a mod can change, from the packs, the DB, the scripts and `empire.retail.dll`. The file
layouts: [wwise_bank.md](../formats/wwise_bank.md), [wem.md](../formats/wem.md), [sound_data.md](../formats/sound_data.md). The
Audio tool: docs/plans/audio.md.

## The sound files (checked 2026-11-19)

Attila:

- `sound.pack`: 51 banks, 1,478 WEMs (their statistics: [wem.md](../formats/wem.md), "Vanilla files"), `audio/event_data.dat` (165 KB),
  `audio/sound_animation_triggers.csv`. `music.pack`: 4 banks (`battle_music`, `global_movies`, `global_music`, `music_data`), 546
  WEMs. `local_en_shared_rome2.pack`: about 39,000 VO files (`audio/`, `audio/advisor`, `audio/diplomacy`) and the VO banks
  (`battle_advice`, `battle_vo_orders_*`...). `music_en_shared_rome2.pack`: 99 files.
- `data.pack`: `.snd.meta` next to the animations, `audio/{main,bel,cha}_attila_map.amb` + `.xml` (campaign map ambience), and the DB
  tables `sound_events`, `voice_events`, `voice_types`, `unit_voice_categories`, `unit_voice_events_junctions`,
  `agent_voice_events_junctions`, `subculture_religion_voiceover_junctions`, `audio_projectiles`, `audio_explosions`,
  `audio_campaign_building_enums`. Battle tiles have `sound_emitter_list.xml`.
- Every bank is `BKHD` version 112, also those of the `*_shared_rome2` packs.

TDD (`tdd_pack1_main_v1.2.0`): 8 overriding banks (`battle_music`, `battle_vo_orders`, `battle_vocalisations`, `campaign_ui`,
`campaign_vo`, `global_movies`, `global_music`, `music_data`), 1,061 WEMs, advisor VO.

Rome 2: its release packs `sound.pack` (39 banks) and `music.pack` (4) are v88, but the patch packs `sound_rome2.pack` (51 banks + an
empty `animation_data.bnk`, which removes it) and `music_rome2.pack` (4) override every one of them with v112, and the VO banks of
`local_en_shared_rome2.pack` (61) are v112. So the game loads only v112 banks. Rome 2 has its own `event_data.dat` (104 KB in
`sound.pack`, 112 KB in `sound_rome2.pack`, same layout as Attila's) and eight campaign `.amb` + `.xml` pairs in `data.pack`
(`main_emperor`, `main_gaul`, `main_greek`, `main_invasion`, `main_punic`, `main_rome_map`, `prologue_map_01`...).

`music.pack` and `music_en_shared_rome2.pack` have the data-encrypted flag (PFH4 header flags `0x11`). rpfm_lib's develop branch
decrypts them right; **rpfm_cli 4.3.14 does not**: it negates the position as a u64 where PFH4 needs a u32, and its output has every
other u32 garbled. Scripts must not trust rpfm_cli extracts from those packs.

## What a mod pack can change (in game, 2026-11-19 / 20)

- **The game does not take edited `*_data.bnk` banks from a mod pack**, while loose WEMs and event banks do. A replaced in-bank sound
  has to stream from a loose `audio/<id>.wem`.
- UI button sounds do not change through a mod pack's banks (event or data banks), while loose WEMs do; the cause is not known.
- No game music segment has several tracks.

## What the game lets mods choose (checked 2026-11-20, Attila's banks, DB and scripts)

- Music: the music switches decide on `music_culture` (Barbarian, Roman, arabic, hunnic), `game_mode`, `campaign_music_state`
  (standard, prebattle), `battle_intensity`, `battle_state`, `battle_result_state`, `music_key`, `Music_Dynamic` (battle events:
  charges, routs, generals killed...). No DB column sets a music culture (no table has a music column but unit musicians): the
  game's code sets `music_culture`, so a new culture value in the banks would never be chosen. Battle scripts can take over:
  `bm:set_music_auto_playback(false)`, `bm:play_music(event)`, `stop_music`, `cutscene:set_music`. No campaign script uses music
  or sound, and the toolkit's API list has no such call.
- Campaign character voices: 49 dialogue events decide on `vo_campaign_character` (Cha1, Cha2, Dig1 to Dig4, Gen1Land,
  Gen1Naval, Gen2Land, Gen2Naval, Spy1, Spy2), `vo_campaign_culture` and `vo_campaign_subculture`; all 1,819 leaves are random
  containers of 1 to 10 lines (median 5): more lines per actor is a matter of adding variations. Which actor a character gets is the
  game's code: no DB column links characters or agents to a voice (`agent_voice_events_junctions`, `unit_voice_events_junctions`,
  `voice_events` are empty in vanilla; `voice_types` holds generic, is_commanding_officer, is_hero).
- Battle unit voices: `main_units.voiceover` (7 values) and `subculture_religion_voiceover_junctions` (by DB) choose the
  `vo_battle_subculture` value; `land_units.selected_vo_secondary` / `_tertiary` choose selection lines.
- The engine (`empire.retail.dll`, 2026-11-20): it holds no state group name (`music_culture`, `vo_campaign_character`,
  `VO_battle_culture`... are not strings in it: it works with their hashed IDs or names from `event_data.dat`); it names the
  voice-over cultures it knows (barbarian, barbarian_christian, barbarian_pagan, barbarian_generic, nomad, nomad_hunnic,
  nomad_hunnic_generic, eastern, eastern_desert, eastern_persian, eastern_generic, roman, roman_christian, roman_generic) and
  reports any other as `invalid_voiceover_culture`: no new culture, but cultures without lines of their own can get them; it loads
  the order banks by fixed names (`Battle_VO_Orders_<Barbarian|Eastern|Nomad|Roman>[_Inf1-12|_Mtd1-7].bnk`): new order lines go
  into those banks.
- The engine's hashed IDs (2026-11-20, the FNV-1 IDs as 4-byte constants in `empire.retail.dll`): `music_culture` (5 places)
  with its values Barbarian, arabic and hunnic side by side (0x946792, 0x946799, 0x9467a0: a fixed table), `campaign_music_state`,
  `vo_campaign_character` with Cha1, Cha2, Dig1, Spy1, Gen1Land, Gen1Naval together (0x9459ef to 0x945bda: a fixed mapping of
  agents to actors), `vo_campaign_culture` / `_subculture`, `VO_battle_culture` / `_actor` / `_order`, `Battle_Order_VO_Play`;
  `vo_battle_subculture` is not there (set by name: the voice-over culture). So the music cultures and the campaign voice actors
  are fixed in the engine's code: a mod changes what their values play, not which values exist or who gets them.

## Not known

- Whether the empty voice junction tables (`voice_events`, `unit_voice_events_junctions`, `agent_voice_events_junctions`) are still
  read (a test pack with rows).
- Why UI button sounds do not change through a mod pack's banks (UI banks loaded before mods, another event, or cached media).
