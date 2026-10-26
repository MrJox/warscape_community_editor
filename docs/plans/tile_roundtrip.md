# Tile round trip: a game-neutral tile model with per-game readers and writers

A tile is read into one in-memory model (`wce_tile_model`) and written out of it by format modules (`wce_tile_io`), one reader and one
writer per game format. Phase 1: Attila's TEd project (raw tile) both ways, byte for byte. Phase 2: Attila's exported binary tile (what
BOB writes and the packs ship) into the same model and back, then binary -> raw (decompile) and raw -> binary (compile).

## Status (2026-10-26)

Phase 1 done (2026-10-22). Phase 2 done: step 1 inventory and codecs, step 2 `attila.tile_binary`, step 3 the decompiler, step 4 the
compiler with stages A, B and C, carried outputs and the editor workflow (2026-10-24). The open items are below.

## Decisions

- (2026-10-21) Phase 1 models TEd's files; BOB outputs and artist leftovers are carried along untouched. Scene nodes are a typed property
  bag per TEd class (a schema per class), not one struct per class. Images that cannot be re-encoded exactly write their original bytes
  while their pixels are unchanged (reported as "kept"); edited ones are re-encoded in TEd's style.
- (2026-10-22) Binary -> raw first, then raw -> binary. Raw -> binary is reimplemented exactly (terrain mesh decimation and the DXT5
  compressor included), each output proven byte-identical against fresh BOB on oracle tiles. Decompiled trees are explicit
  `VEGETATION_INSTANCE`s in a "Decompiled vegetation" layer (the seeds are not in the binary); grass lists are carried as files.
- (2026-10-22) Exactness: the shipped binaries were exported from TEd's floats in memory, finer than tile.ted's `%.6f`, so binary -> raw ->
  binary tolerates what the text cannot carry (a few f32 ulps on positions, 1e-5 on quaternions) and is byte-exact for everything else
  (docs/formats/tile_compile.md, "Exactness").
- (2026-10-22) Corpus: every tile of Attila's packs through the VFS, BOB's working_data in the oracle kit, mod packs (TDD) when present.
- (2026-10-23) No corpus runs until the plan is finished; check each piece on a few tiles (`--filter`).
- (2026-10-24) BOB's outputs are carried with fingerprints and regenerated only for an edited group (`attila::carry`). The editor works
  in a workspace folder; output to a folder now, packs later.

## As built

- Crates `wce_tile_model`, `wce_tile_io` (the format traits, the registry, `attila.ted_project`, `attila.tile_binary`,
  `attila::{decompile, compile, stage_b, stage_c, carry, workflow}`); TEd's number formatter in `wce_formats::c_num`.
- Docs: docs/formats/tile_model.md, tile_ted.md, tile_binary.md, tile_decompile.md, tile_compile.md, tile_workflow.md; baselines in
  docs/baseline.md.
- Tests and gates: `tests/ted_project.rs`, `tests/tile_binary.rs` (synthetic), `tests/kit_samples.rs` (`WCE_ATTILA_KIT`),
  `tests/pack_samples.rs` (`WCE_ATTILA_GAME`); `tile_roundtrip(_all)`, `tile_binary_roundtrip(_all)`, `tile_decompile(_all)`,
  `tile_workflow_check`, `bob_tile_check`, `terrain_mesh_check`, `stage_c_check`.
- Phase 1: the whole kit round-trips (every modelled file byte-identical, 222 Photoshop-style PNGs kept, 0 unknown attributes). Legacy
  `.markers` exports (19 folders) and `custom_normal.*` are carried as files, not modelled.
- Phase 2 corpus (2026-10-24): `tile_binary_roundtrip` reads and writes back every tile folder of the 31 packs (1,756: battle, settlement
  and campaign folders); `tile_decompile_check` with `WCE_COMPILE_STAGE_B=1` decompiles and reads back 1,654 tile folders, none
  panics; stage B compiles 1,028 of them. On 34 tiles every file of the binary compiles back, none is added, every stage-B and stage-C
  file byte for byte. Stage B and C match fresh BOB on 13 oracle tiles.
- The oracle runs were made with BOB under Wine on a Linux machine (`~/wce_re/scripts/bob_tile.sh`, `bob_veg.sh`). The kit lacks most
  buildings' raw height-map meshes (`raw_data/art/.../*.cs2`), so oracle tiles are those without such buildings.

## Open

1. **Pack output for "Save as exported tile"**: PARTIAL. Only tiles with procedural walls get a pack
   (`wce_walls::export::save_exported_walls`); the others go through `attila::workflow::save_exported_scene`, a folder only.
2. **Stage A: 16 values beyond the text's precision** on the corpus run, all of the known kinds (escalation BMD and prop transforms,
   definition, deployment). Decide per value whether it is the text's limit or a port difference.
3. **`vegetation_exclusion.xml`** (4 encampments) is carried: TEd's box comes from the editor's loaded models and is not reproduced.
4. **`tile_normal.tga` as output**: the algorithm is documented (docs/formats/tile_compile.md, "Normal map") but no compile path writes
   the file. BOB rebuilds it from the raw files, so it may be unnecessary.
5. **The DXT5 residual and the stage-B paths no oracle has exercised**: docs/formats/tile_compile.md, "Unverified in stage B". Check each
   when a tile that uses it turns up.
6. **Decals possibly written relative to the ground** in the families whose kit is not the shipped version (docs/formats/tile_decompile.md,
   "The kit is evidence, not a gate"); needs TEd's exporter.
7. **Where the `lite` flag is consumed** (the game's or BOB's code, static RE), before giving the decompiler's lite building zones a
   nicer, outline-like shape (today a quadtree of rectangles: 4,722 zones on 102 tiles where the kit has 140, no flag changed).
8. **Campaign stage B**: refuses 485 projects whose density does not divide 128 (campaign tiles and some coarse lakes) and 141 without a
   blend map (campaign).
9. **Move the editor onto `wce_tile_model`**: not started; `wce_tile_editor` still edits through `TedScene`. The cross check between
   the model and `TedScene` passes on the kit.
10. **TDD raw and working tiles**: the 1,583-file `tile.ted` corpus with TDD's v17 tiles still owes `scene_check` and the differential
    dumps (PLAN.md points here). Needed from the user: TDD's raw tiles (tile.ted, maps, `_tile_database`), at least dwarven_city_b/medium
    and middle_men_city_hornburg/medium and small (the 6 non-unit building quaternions); TDD's working_data of a few tiles for stage B if
    its export setup differs from vanilla.
11. **Codecs still raw inside**: the bytes of the older particle emitter versions (4 to 8), a few prop bytes, the campaign
    `bmd.grass_list` / `bmd.tree_list` ESF nodes, `tile.ted_binary` (docs/formats/tile_binary.md, "Step 1 status").
