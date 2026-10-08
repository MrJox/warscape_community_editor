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

## Selection (empire.retail.dll, all decompiled)
- fn 0x17d9f10 builds the query: map (argument), season (fn 0x1882bc0, "default" without a campaign season), climate (name
  table at battle +0x150, "default" without one), precipitation, time_of_day, climate_change_index, snow_attrition
  (`snow_attrition_on` / `_off`), battle_type, then asks for key `environment` with the "all matches" mode.
- fns 0x1183c740 / 0x1183c690: the query is the OR of one bit per category, bit = `1 << (index of the value across all metas
  in file order & 31)`; a value that is not in the lists adds no bit.
- fn 0x1862300: a variation matches when `flags == 0 || (flags & query) == query`. (Its other mode keeps only the matches with
  the fewest set bits, fn 0x1818010 = popcount; the battle query does not use it.)
- fn 0x1180c090: if the first query finds nothing it repeats it with map `default_map`; if there is still nothing no
  environment comes from the database. Otherwise it picks `candidates[random(n)]` with fn 0x10693bf0: the 0x343fd LCG with
  rejection sampling, i.e. uniform.

## What the editor shows
Per condition set: the candidate list, whether the `default_map` retry happened, probability 1/n each, which pack provides
each file (red when no pack has it) and an Open button. For the open `.environment`: the conditions under which the game
selects it, or that no variation refers to it. The choice of battle conditions (including the climate) is an input of the
view; how a campaign picks them is not part of it.

## Findings on the TDD data (variation_check, 2026-10-08)
239 variations, 0 files missing, 0 variations that can never match, every variation found by its own conditions.
Of the 19,200 combinations of all values, 7,830 find no file even after the default_map retry (only climates `default` and
`ROME_Atlantic` are fully covered; for example `ROME_Continental` has no variation for summer dry day with snow attrition
on), and 354 `.environment` files under `weather/` are not referenced by any variation (test, `at_*` and other files).
