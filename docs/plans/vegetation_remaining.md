# Vegetation: what's left (2026-10-18)

State: `wce_tile::vegetation` generates everything BOB's Vegetation processor writes for a battle tile:
- the tree lists, static trees included and converted per climate;
- the grass list;
- the props, decals and VFX of `<category>_procedural_bmd_data`;
- the list heights (the height alpha map).

All of it is bit-exact against fresh BOB output on 11 tiles. docs/re/vegetation.md has the RE, the checks and the BOB headless recipe.

## Left to do, in order

1. **Write the files.** Done (2026-10-24): `wce_tile_io::attila::stage_c` writes the tree lists, the procedural BMDs with their twins
   and the grass list byte for byte against fresh BOB (docs/re/vegetation.md, "The writers"). The notes below are kept for the record.
   The editor generates and draws the lists but doesn't write them. That means `<category>.tree_list.bin`,
   `default.grass_list.bin` and `<category>_procedural_bmd_data.bin` / `.xml`, through rpfm's encoders. To settle when writing:
   - **Tree item `id`:** a 64-bit value per item, possibly random; not reproduced yet.
   - **Grass f16 rounding:** grass positions are stored as f16, rounded to nearest with ties toward zero (`FUN_10057610`).
   - **Prop decal fields:** BOB leaves a prop's decal fields uninitialised; write 0.
   - **BOB's file rules:** `DeleteEmptyClimateTreeLists`, and the removal of `bmd.tree_list`.
2. **PROPS key order** (optional, no effect in the game): BOB's `MARKER_TABLE` (a tree of the key's path parts in a
   `CA::HashTableNoRemove`, the hasher `FUN_10011aa0` didn't decompile, walked by the flat marker iterator). The port groups by key in
   first-seen order; each key's markers are already in BOB's order.
3. **Editor follow-ups:**
   - Converted static trees whose converted model doesn't exist: BOB's list drops them, the editor keeps drawing the instance's own
     model.
   - Some small dark squares in farm_eastern/1x1/ab's orchard rows show with every list. They come from some static model, not yet
     looked at.
4. **Leaf density comparison** (last): mesh trees look sparser than their billboards. It waits on the Terry camera fix for the
   screenshots (docs/re/vegetation.md, Open).

## Notes for picking it up

- **Checks:**
  - `cargo run --release -p wce_tile --example veg_check -- <game> [filter] [--full] [--diff] [--oracle-dir <kit>/working_data]`
  - `grass_check` and `bmd_check` take the same options; `bmd_check` also has `--params-dir`.
  - Against the packs: 1,995 / 2,788 tree lists exact and 119 / 125 grass lists. The rest are stale packs, not port bugs.
- **Fresh BOB:** `~/wce_re/scripts/bob_veg.sh <tile paths>` runs it under Wine in the scratch kit `~/wce_re/bobkit`.
- **Prop and decal test file:** `~/wce_re/bobkit_tests/zztest_tree_parameters.xml`. Copy it into the scratch kit only while testing;
  it changes the rome_med outputs.
- **Wine caveat:** BOB under Wine rounds an f32 tie in the grass key count differently from CA's build (farm_eastern/1x1/ab). The packs
  are the reference there.
