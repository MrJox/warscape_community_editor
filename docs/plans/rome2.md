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
  `GameFeatures` (`kit_tiles`), and `unsupported_tools`: the ids of built tools the game hides (the switcher, Ctrl+1.., the files they
  open, their settings pages and the `tool` command skip them; a hidden active tool gives way to the first shown one). Empty for both games. `GameProfile::ALL`, `by_key`, `from_env` (`$WCE_GAME_KEY` for the examples and gates).
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
- **done** The terrain (surface, blend, composite, resolve; shadows). The composite is the game's DX11 one (2026-10-28): the index map's
  variations, a repeat per group (RENDER_PARAMS' 9 m where the climate lists none, not 40.1), a gloss per layer. Open: the channel
  swizzle's source, the far composite, the virtual texture path.
- **done** The sun's shadow cascades (2026-10-28): Rome 2's orthographic fit, splits and constants (they were Attila's warped ones,
  which its light pass cannot read: no shadows at all). **todo** The cloud shade and baked shadows: check against a frame of the game.
- **done** The sea (the forward ocean). 2026-11-01, against the user's screenshots of Carthage (`greek_carthage/large`, its campaign
  placement, ROME_Med, summer, day, dry): the planar reflection (the mirrored sky; ASSUMPTION: the game's reflects the scene too), the
  shadowed variant, the sea around the battle's centre (docs/re/rome2_renderer.md, "Sea"). Open: the whitecaps and the colour (to
  capture the game).
- **done** The terrain-blend meshes (2026-11-01; the settlements' cliff faces, `cliff`, `cliff02`): Rome 2's `rigidterrainblend` has no
  terrain-material entry (ps30_gbuffer, ps30_null_gbuffer, shadow). ps30_gbuffer writes the vertex alpha (0 on the plateau, 1 on the
  faces) into the normal's and the albedo's alpha: drawn after the terrain resolve and blended by it (`terrain_rome2.rs`, step 4) over the
  ground the resolve gave them. ASSUMPTION: the game blends it over ps30_null_gbuffer's (the terrain's virtual texture). Not applied: the
  climate's variants of their textures (`cliff_rome_med_diffuse.dds`, `rock01_rome_med_*`, `rockstraight_*`), which the engine picks
  as it does the custom meshes' (`<name>_<climate>_diffuse.dds`; not traced for buildings).
- **partial** The effects (docs/re/rome2_vfx.md): the billboard emitters (`continuous_mesh`, `burst_mesh`) are a port of the game's
  CPU simulation (`vfx_rome2::sim`: its state machines, spawning, update and generator order) drawn with the shaders generated per
  effect, sorted as the game sorts (its `std::sort`). Open: the Distortion, SeaColourOverlay and shadow views, ribbons, spawn spacing,
  SpawnByMesh, particle events, the rigid emitters (point lights), the weather particles; a capture of the game to compare.
- **done** Trees, grass, billboards and decals (their shaders' other slots and names; the older tree and grass lists read). Open: the
  engine's `g_hide_foliage`.
- **done** Rivers and water planes (forward, as the sea; no planar reflection).
- **todo** Against the game (user report 2026-10-26, `settlement_egyptian_cities/egyptian_minor_city_c`, in game near Rekem): the
  ground's variations and repeats now follow the game (the hill shows rock's pale variation, at 40 m), but it is still browner than the
  game's pale grey. Of every climate's rock only `rome_med`'s first variations are that pale grey with moss: the game's battle was
  most likely lit and textured as ROME_Med (2026-10-28; to render with it). Not cliffs: `terrain/textures/battle/cliff/*` are only on
  the `cliff_custom` link tiles' meshes (material 49, TerrainCustomTile, fn 0x114b80a0: `<mesh path>_<climate>_diffuse.dds`, else
  `<mesh path>_diffuse.dds`), coastal; tile ground never gets them (~/wce_re/scratch/rome2/cliff_re/notes.md). The orange cast was the weather: the editor lit with the `default/` season folder's heavily graded files, the
  game with the battle season's (2026-10-28; the colour matrix and the environment's fields checked against the game, the same); still
  open: the haze. The climate: a tile shown in a map context takes its
  placement's (`ContextPanel::adopt_climate`, the region's as in the game); alone it is the editor's choice (`ROME_Continental` by
  default).
- **blocked** A RenderDoc capture of a battle (the light pass, tone map and fog inputs, the ground's textures): Rome 2 is 32-bit and
  RenderDoc on Linux 64-bit only; under Proton's WoW64 (`PROTON_USE_WOW64=1`) the layer loads but DXVK can't map memory on the NVIDIA
  driver (`VK_ERROR_OUT_OF_HOST_MEMORY`, the game then crashes on its null device). To try: the Intel GPU (`DXVK_FILTER_DEVICE_NAME`),
  or another machine (Windows: RenderDoc's D3D11 capture). Launch options and the attach steps: Steam `... bash -c 'exec
  "${@/launcher\/launcher.exe/Rome2.exe}"' -- %command%` (no launcher), RenderDoc's File > Attach to Running Instance, Trigger Capture;
  captures to `~/wce_re/scratch/rome2/captures/`.

## Phase 3: editor — partial

- **done** The tile's weather follows its climate (2026-11-01): a map context gives the tile its placement's climate after the tile picked
  its weather; a weather of another climate's folder gives way to the new climate's first dry one (`WeatherUi::follow_climate`).
- **known** In a map context, the first frames after the camera jumps far can show dark rectangles in the sky and dark patches on the
  ground (seen on Rome 2's Carthage, also with the sea in its old frame); 20 frames later they are gone. Not traced (the LF window and
  the streamed chunks' level changes are the suspects). Scripted captures wait 30 frames after `map_context_settle`.

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

- **done** Master's features after the rebase of 2026-10-28 (docs/plans/launch_in_game.md, tile_ted_parity.md):
  - Launch in game for any game: the profile's executable (`GameProfile::exe`, also what tells the games' folders apart), data folder,
    pack format and campaign map; the armies per game (`[games.<id>.tools.tile] launch_army`, Rome 2's vanilla keys by default); the
    battle XML without what Rome 2's engine lacks. The game's tile data for exports and launches is read from the mounted packs on the
    preload thread too. In game: to confirm.
  - Assembly Kits: the list is each game's (Settings > Game shows the shown game's); the exports' vegetation parameters come from the
    first kit with tiles (none for Rome 2). rpfm's schema for the walls and the weather rules is the game's.
  - The weather selection database: Rome 2's engine has none (`GameFeatures::variation_db`): the picker and its commands say so.
  - Tile database window, New tile, Save tile as and the tiles that can follow: for Rome 2 the packs' database (the map context's tree)
    stands for the kit's; its tiles open from the packs.
  - Checked, nothing to do: the weather inspector (Rome 2's environment files have Attila's 110 element attributes), the ground texture
    groups (the tile's climates), the burning buildings (a material without a fire shader draws plain; Rome 2's instance values: to
    confirm in its renderer).

## Validation without TEd

- Oracles: the game's files (decompile + compile within the text's precision, the database byte for byte), static RE of Rome 2's
  kit DLLs (EmpireUtility, ToolDataBuilderDll) and its `empire.retail.dll`, the game itself (a tile saved into a mod pack and loaded:
  todo, by a human).
- Gates: `scripts/gates.sh` with `WCE_ROME2` (docs/formats/rome2_tiles.md, "Checks").
