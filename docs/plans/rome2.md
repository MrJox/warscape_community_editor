# Rome 2 support (and other games)

The editor is built for Attila; this plan adds Total War: Rome II, generically, so that later games follow the same path. Rome 2's
Assembly Kit has no tiles, no TEd and no BOB tile, vegetation or terrain processors: its tiles come from the packs and go into mod
packs. Formats: docs/formats/rome2_tiles.md; renderer: docs/re/rome2_renderer.md.

Decisions (user, 2026-10-25): mod packs are the only output for Rome 2; a mod overriding a whole tile set file is acceptable; the
Attila ports of BOB's stages are good enough for generated data (checked against the shipped files and in game, no Rome 2 oracle);
siege AI and procedural walls are in scope (merged into master 2026-10-26); the game switches live in the editor.

Status legend: **done**, **partial**, **todo**.

## Phase 0: games as data — done

- `wce_foundation::game::GameProfile` (re-exported by `wce_vfs`): key, Steam folder and app id, the preferences folder, rpfm's schema,
  the campaign battle map, the default environment, where the vanilla packs come from (Attila's manifest; a list for Rome 2), and
  `GameFeatures` (`kit_tiles`). `GameProfile::ALL`, `by_key`, `from_env` (`$WCE_GAME_KEY` for the examples and gates).
- The pack order puts the patch packs (header type 2) before the release ones; an empty file in a pack removes the file below it.
- Settings: `game` picks the profile (`Settings::game_profile`); File > Game and the Settings window's Game page switch it live
  (`Editor::switch_game`: the documents close, asking about unsaved ones; `Tool::game_changed`; the packs and the renderer are made
  again; a game not set up opens the first-run dialog, which has the game's combo).
- No `"attila"` literal outside the profiles: mod packs (`wce_vfs::mod_pack`), the tile tool's game data and battle map, the weather
  tool's default environment, the preferences file, Steam detection.
- `Files::game`: the renderer picks its game's passes (`RenderGame`).

## Phase 1: Rome 2's tiles read and written — done (but stage B's terrain and stage C)

- **done** The XML tile database (`rome2::tile_database`), written back byte for byte for all 1,437 tiles.
- **done** `GameData` and `workflow::Game` read the editor's mounted packs (not every pack of the folder) and work without a kit.
- **done** `TileConventions`: the decal sign, the building list layouts (versions 10 to 12, `burn_amount`), the versions the game's
  TEd writes. PROP_MARKER version 4.
- **done** Rome 2's extra files carried (`tile.ted_binary`, `.outlines`); the vegetation carried without vegetation parameters.
- **done** Save to mod pack (all games).
- **done** Stage B for Rome 2: the linking tiles decompiled into the workspace with their final heights; no building stamps (Rome 2's
  buildings have no height-map meshes). Open: the port decimates the meshes more than Rome 2's BOB did (3x fewer vertices).
- **dropped** Byte-exact building lists: the string table order of Rome 2's tool is not a model found (formats doc, "Building lists"); the values are the same, the game reads strings by index.
- **done** GENERIC_MARKER v1 (one file: read, written back byte for byte, carried along).
- **done** Multi-list capture location files (two tiles): the other lists carried, written after the project's. PARTICLE_EMITTER_MARKER v4's float in `unknown_64` (the effect's scale?)
  comes back an ulp off from the text.
- **done** Stage C for Rome 2: the project's static trees into every older tree list (the generated ones kept); the decompiler reads
  them back from `default.tree_list`. Open: the grass list follows no edit (no grass parameters).
- **done** The campaign battle map context for Rome 2: `tile_map` version 6 (16-byte cells; names without the variation: the tile's
  first one), `locations.blm` version 7; the neighbours decompiled from the packs on demand into the cache
  (`workflow::neighbour_root` / `neighbour_tile`, `ContextRequest::provider`). Open: the neighbours draw coarser than the open tile, the
  sea is not drawn (holes).

## Phase 2: Rome 2's renderer — partial

- **done** Shaders of Rome 2's containers (rpfm); tone mapping in log Y; the S-curve's single biases; the weather from folders.
- **done** The terrain (surface, blend, composite, resolve; shadows). Open: the variation textures (the index map is not read).
- **todo** The light pass's new inputs (cloud shade, baked shadows, split depths): check against a frame of the game.
- **done** The sea (the forward ocean; no planar reflection).
- **partial** The effects (docs/re/rome2_vfx.md): the billboard emitters (`continuous_mesh`, `burst_mesh`) are a port of the game's
  CPU simulation (`vfx_rome2::sim`: its state machines, spawning, update and generator order) drawn with the shaders generated per
  effect, sorted as the game sorts (its `std::sort`). Open: the Distortion, SeaColourOverlay and shadow views, ribbons, spawn spacing,
  SpawnByMesh, particle events, the rigid emitters (point lights), the weather particles; a capture of the game to compare.
- **done** Trees, grass, billboards and decals (their shaders' other slots and names; the older tree and grass lists read). Open: the
  engine's `g_hide_foliage`.
- **done** Rivers and water planes (forward, as the sea; no planar reflection).
- **todo** Against the game (user report 2026-10-26, `settlement_egyptian_cities/egyptian_minor_city_c`, in game near Rekem): with
  the eastern desert's ground and weather the hill is still dark rock and grass where the game shows pale limestone (no climate or season
  gives it: the layers' textures or the variation index map), our colours are more saturated, contrasted and purple than the game's flat
  hazy frame (tone mapping, grading or fog), and the trees' shadows are faint. The climate is a guess (`ROME_Continental` by default):
  take it from the map context's region.
- **blocked** A RenderDoc capture of a battle (the light pass, tone map and fog inputs, the ground's textures): Rome 2 is 32-bit and
  RenderDoc on Linux 64-bit only; under Proton's WoW64 (`PROTON_USE_WOW64=1`) the layer loads but DXVK can't map memory on the NVIDIA
  driver (`VK_ERROR_OUT_OF_HOST_MEMORY`, the game then crashes on its null device). To try: the Intel GPU (`DXVK_FILTER_DEVICE_NAME`),
  or another machine (Windows: RenderDoc's D3D11 capture). Launch options and the attach steps: Steam `... bash -c 'exec
  "${@/launcher\/launcher.exe/Rome2.exe}"' -- %command%` (no launcher), RenderDoc's File > Attach to Running Instance, Trigger Capture;
  captures to `~/wce_re/scratch/rome2/captures/`.

## Phase 3: editor — partial

- **done** The game selector in the top bar: the toolbar's first item, a combo of the games (as File > Game).
- **done** The map context in Rome 2 (user report 2026-10-26, Carthage): a tile the map names without a variation is found as any of its
  variations; the sea tiles' common maps (`blend_common` ...) read from the packs (their ground had no material, and one terrain
  without a material blanked every neighbour: Rome 2 has no legacy ground to fall back on, such a terrain is left out); Rome 2's LF
  window was mirrored in v (unseen on Rome's flat LF); the LF sampled with points (the shader filters it itself).

- **done** Tile menu: Save to mod pack; no Save for / Import from Assembly Kit for a game whose kit has no tiles.
- **done** The weather tool on Rome 2's environments (the test map lit, the default environment `rome_med` day_dry_01).
- **done** The UI tool: Rome 2's layouts (version 86) read and write back byte for byte and draw (before version 90 the visible flag is
  the eighth of thirteen: every component read as hidden, Attila's older layouts too).
- **done** Hide what Rome 2's tiles have not (`GameFeatures`: `civilians`, `escalation`, `lite_buildings`): the Create panel's civilian
  and lite building zone entries, the layers' escalation export types. Kept: seasons and snow (Rome 2's weather has seasons and snow
  folders). Procedural BMDs and tents have no UI of their own (the decompiler finds none in Rome 2's tiles).
- **done** Instance templates: none needed of Rome 2 (its projects are TEd scenes as Attila's, compiled with Rome 2's conventions); an
  effect without a known scale gets the type's default.
- **done** Procedural walls: saved into mod packs (any game), the packs written for the files' game, the bases Rome 2 has
  (docs/plans/procedural_walls.md, "Mod packs and Rome 2"). In game: to confirm.
- **done** Siege AI generator on Rome 2's settlements: the same marker files (decompiled and compiled as Attila's); on roman_city_a/small
  (`WCE_GAME_KEY=rome_2 siege_check <Rome 2> <out> <extracted project>`): 76 fort pieces, 11 areas, 17 streets, 4 wall edges, no problem
  in ours (CA's own layout: two nodes without a boundary). In game: to confirm.

## Validation without TEd

- Oracles: the game's files (decompile + compile within the text's precision, the database byte for byte), static RE of Rome 2's
  kit DLLs (EmpireUtility, ToolDataBuilderDll) and its `empire.retail.dll`, the game itself (a tile saved into a mod pack and loaded:
  todo, by a human).
- Gates: `scripts/gates.sh` with `WCE_ROME2` (docs/formats/rome2_tiles.md, "Checks").
