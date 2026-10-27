# Battle weather selection (warscape_asset_variation_db)

Which `.environment` file a battle uses is data, in `warscape_asset_variation_db/warscape_asset_variation_db.bin`
(TDD overrides it in `tdd_pack6_weather`). Implemented in `src/formats/variation_db.*`, shown in the editor's
Selection > Weather picker, checked by `tools/variation_check.cpp`.

## File format (FASTBIN0, little endian)
`"FASTBIN0"`, u16 version 1, u32 meta count; per meta: u16 serialise 1, str name, u32 value count, str values;
u32 entry count; per entry: str key (`environment`), u16 serialise 1, u32 variation count; per variation: u16
serialise 1, str filename (backslashes, mixed case), u32 flags. `str` = u16 length + bytes. Verified byte for byte
against the XML export of the same file (239 variations, 0 differences, nothing left unread).

Metas in file order: map, season, precipitation, time_of_day, climate_change_index, snow_attrition, battle_type,
climate (31 values, so bits 0..30; bit 31 is free).

## XML form and conversion
The packs ship the database twice, `warscape_asset_variation_db.bin` (what the game loads) and `.xml` (what people edit), in vanilla
`data.pack` and in `tdd_pack6_weather`. The XML is UTF-8, CRLF, tab-indented, single-quoted, `serialise_version='1'` on the root,
every `meta_data_entry`, `value` and `variation`; categories are `<meta_data_entry name=..><values value='a' value='b'/>`, variations
are `<variation filename='..' flags='n'/>` inside `<entries><entry key='environment'><value><variations>`.
`VariationDb::to_bytes()` / `parse()` (FASTBIN0) and `to_xml()` / `parse_xml()` convert in both directions (`src/formats/variation_io.cpp`,
CLI `tools/variation_convert.cpp`). Verified on both pairs in the game files:
- bin -> parse -> bin is byte-identical; xml -> parse -> bin gives exactly the shipped .bin;
- vanilla bin -> xml is byte-identical to the shipped .xml; the TDD .xml differs from the canonical output only by one blank line
  before `</variations>` (a hand edit; the parser ignores blank lines, the writer does not produce it);
- the .xml and .bin of each pack hold the same data.

## Editing rules
`flags` are one bit per value over all categories in file order (bit 31 is free), so the value lists and the flags depend on each other.
`add_value` appends to a category and shifts the later categories' bits; a variation that allowed every existing value of that category
(and is not flags 0) also allows the new one. `remove_value` drops the bit and shifts later bits down; it refuses when a variation would
end up with flags 0 (which means "any") and it refuses to remove `default_map`, which the game asks for when nothing matched. More than
32 values cannot be added, nor a value another category already has. `clone_value`, `add_category`, `remove_category` (same flags-0
refusal) and `rename_category` complete the set. `validate()` reports: more than 32 values, duplicate categories or values (also across
categories), flag bits that belong to no value, a category with no allowed value (the variation can never match), a `map` category
without `default_map`. The converter and the editor refuse to save a database with problems unless forced. Gates (`variation_check`):
adding and removing a value restores the database, adding a value changes no existing condition's candidates (all 19,200 combinations),
removing one keeps the others, a clone of every map and climate finds what its source finds under every condition, and
`variation_rules::check` reports no error on the game's database.

## Selection (empire.retail.dll, all decompiled)
- fn 0x17d9f10 builds the query: map (argument), season (fn 0x1882bc0, "default" without a campaign season), climate (name
  table at battle +0x150, "default" without one), precipitation, time_of_day, climate_change_index, snow_attrition
  (`snow_attrition_on` / `_off`), battle_type, then asks for key `environment` with the "all matches" mode.
- fns 0x1183c740 / 0x1183c690: the query is the OR of one bit per category, bit = `1 << (index of the value across all metas
  in file order & 31)`. The value is looked up **by its name alone**, through the values of all metas in file order (the
  category of the query is not used), so the first meta that has the name gives the bit. A value that is in no list sets the
  query's +0x1d flag (re-checked 2026-10-26; the old note "adds no bit" was wrong).
- fn 0x1862300: if the +0x1d flag is set (fn 0x1017e2a0) the query **matches nothing**; otherwise a variation matches when
  `flags == 0 || (flags & query) == query`. (Its other mode keeps only the matches with the fewest set bits, fn 0x1818010 =
  popcount; the battle query does not use it.)
- fn 0x1180c090: if the first query finds nothing it repeats it with map `default_map`; if there is still nothing no
  environment comes from the database. Otherwise it picks `candidates[random(n)]` with fn 0x10693bf0: the 0x343fd LCG with
  rejection sampling, i.e. uniform. So an unknown map is rescued by the retry, an unknown value of any other category is not:
  those battles get no environment from the database.

## What the engine asks for (2026-10-26)
The loader (fn 0x117ba0b0, called from fn 0x117bf600 with `warscape_asset_variation_db`) reads `meta_data_entries` and `entries`
generically: no category name, count or value is checked. What is fixed is the query. Its values (strings in the DLL):

| category | values the game sends | source |
| --- | --- | --- |
| map | the campaign's map name; `default_map` on the retry | data |
| season | `sp` `su` `au` `wi` (fn 0x118f6360, battle +0x13c), `default` without a campaign season (+0x140) | code |
| precipitation | `dry` `rain` `snow` `fog` (table 0x11d209e0; index 4 = `dry`), from the battle setup fn 0x103b7410 | code |
| time_of_day | `day` / `night` (fn 0x103b7410) | code |
| climate_change_index | `cc<phase>`: the campaign's current climate change phase (see below); `cc0` outside a campaign | data |
| snow_attrition | `snow_attrition_on` / `snow_attrition_off` | code |
| battle_type | `land` / `sea` (fn 0x103b7410) | code |
| climate | a climate name of the battle tile database (fn 0x118e6860 on the list at +0x24/+0x28 filled from `<CLIMATES>` by fn 0x118b0fd0 / 0x118b10a0, i.e. `terrain/tiles/battle/tile_database.bin`), `default` without one | data |

Consequences: category names change nothing; a value the game sends must be in some list (removing `fog` costs every fog battle
its weather); values the game never sends (`storm` in both shipped files, new values in the code-fed categories, new categories)
only take bits; a new climate is asked for once the battle tile database has a climate of that name (and battle maps paint it).
Value names must be unique over all categories. The 32-bit limit is real (`& 31`: a 33rd value would share bit 0). Both shipped
databases use 31 bits; the shipped battle tile databases (vanilla kit, TDD packs) have exactly the 8 climates the lists have.
Encoded in `wce_weather::variation_rules` (the engine's categories, a value's role, `check` against `GameFacts` read from the packs).

**Climate change index (traced 2026-10-27).** `cc%d` formats `WARSCAPE::TERRAIN_BATTLE_SETTINGS::m_climate_change_index` (battle
+0x15c; the header's field order puts it 0x20 after `m_season` at +0x13c, after `m_climate_index` at +0x150). Its chain:
- `campaign_climate_change_phases_tables` (columns campaign, phase, climate_art_set, human_imperium_threshold, round_threshold,
  fertility_factor) gives each campaign a list of `EMPIRECAMPAIGN::CLIMATE_PHASE` {m_phase, m_round_number_threshold,
  m_human_fame_threhold, m_origin_phase_record}, 0x10 bytes each. The manager keeps the current phase at +4, the list at +0xc/+0x10.
- fn 0x108002b0 (each turn): from the last phase down to the first, the first one whose `round_threshold` the turn number reaches or
  whose `human_imperium_threshold` the leading human's imperium (+0xcec) reaches becomes current, only if its phase is higher than the
  current one (phases only advance); fn 0x107afe50 stores it. fn 0x107b6400 returns `list[current]` (so the phase is also the list
  index: phases are 0..n-1 in order), 0 for a campaign without phases; the script's `climate_phase_index` (fn 0x10839750) reads the same.
- fn 0x10b93720 (a campaign battle) passes `list[current].m_phase` (0 without phases) as the 18th argument of fn 0x10da7fc0, which stores
  it at +0xd8; fn 0x10da8ed0 hands +0xd8 to fn 0x117cd110 as the settings' `m_climate_change_index`. Battles not built from a campaign
  (fn 0x10da9430) pass 0.
- A debug variable `terrain_override_climate_change_index` (registered from static data at 0x100bba73) can override it; not followed.

So the values are `cc0` plus `cc<phase>` for every phase of the loaded tables: vanilla bel_attila / main_attila / pro_attila have 0..4,
cha_attila only 0, TDD's `rom_climate_change_phases` (main_attila_map) 0..4 — exactly the shipped lists' cc0..cc4. A mod that adds
phase 5 makes the game send `cc5`, which then has to be in the database (`check` reports it; it reads the tables with rpfm's schema).

## What the editor shows
Pick tab, per condition set: the candidate list, whether the `default_map` retry happened, probability 1/n each, which pack provides
each file (red when no pack has it) and an Open button. For the open `.environment`: the conditions under which the game
selects it, or that no variation refers to it. The choice of battle conditions (including the climate) is an input of the
view; how a campaign picks them is not part of it.

Edit tab, the whole file: the 32 flag bits coloured by role (game / data / unused / free); per category its values with bit, how many
variations allow it and its role, rename / remove (the game's values locked unless Expert is ticked), Clone for maps and climates
(`clone_value`: a new value every variation allows exactly when it allows the source), add values (data and unused categories; code-fed
ones in Expert), add / rename / remove categories (Expert, or categories the game never asks for); a coverage grid (season x
precipitation x time of day for one value of every other category: files the game picks from, `*` = after the default_map retry,
red = none; values the game never sends are left out; open by default; a cell lists its files and adds a file for exactly its conditions); the variations with a per-category conditions grid
(All / None); the file's problems and `variation_rules::check` (climates checked against the packs' battle tile database). Undo /
Redo. Files: Open (.bin or .xml), Save (both forms into `<output>/warscape_asset_variation_db/`), Save As (format by extension),
Convert (a file to the other format without opening it); saving is refused when the game would lose weather, unless "save anyway".

## Findings on the TDD data (variation_check, 2026-10-08)
239 variations, 0 files missing, 0 variations that can never match, every variation found by its own conditions.
Of the 19,200 combinations of all values, 7,830 find no file even after the default_map retry (only climates `default` and
`ROME_Atlantic` are fully covered; for example `ROME_Continental` has no variation for summer dry day with snow attrition
on), and 354 `.environment` files under `weather/` are not referenced by any variation (test, `at_*` and other files).
