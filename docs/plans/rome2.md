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

- **done** The graphics options (2026-11-01, static analysis of its engine; docs/re/graphics_options.md, "Rome 2"): shadows (none at 0,
  two to four cascades by level, their lengths, the baked map 2048 / 4096), water (low to 2, medium, high, ultra for the water planes
  at 5, high for the sea; the planar reflection from 3), sky (512 / 1024), effects, texture filtering, MLAA, god rays, tessellation, alpha
  blending (alpha test at every level, as the game). Not: MSAA, building / unit detail (LOD distance multipliers), trees, grass, terrain,
  texture quality. Script command `gfx <shadow_quality|water_quality|sky_quality|effects_quality|aa|texture_filtering> <level>`.
  To check in game: shadow levels 1 .. 3 leave a gap between the last cascade (151 / 176 units) and the baked shadows (from 239);
  the medium sea shader draws no shadows.
- **done** Shaders of Rome 2's containers (rpfm); tone mapping in log Y; the S-curve's single biases; the weather from folders.
- **done** The terrain (surface, blend, composite, resolve; shadows). The composite is the game's DX11 one (2026-10-28): the index map's
  variations, a repeat per group (RENDER_PARAMS' 9 m where the climate lists none, not 40.1), a gloss per layer. Open: the channel
  swizzle's source, the far composite, the virtual texture path.
- **done** The sun's shadow cascades (2026-10-28): Rome 2's orthographic fit, splits and constants (they were Attila's warped ones,
  which its light pass cannot read: no shadows at all). **todo** The cloud shade and baked shadows: check against a frame of the game.
- **done** The sea (the forward ocean). 2026-11-01, against the user's screenshots of Carthage (`greek_carthage/large`, its campaign
  placement, ROME_Med, summer, day, dry): the planar reflection (the mirrored sky; ASSUMPTION: the game's reflects the scene too), the
  shadowed variant, the sea around the battle's centre (docs/re/rome2_renderer.md, "Sea"). 2026-11-01, against a RenderDoc capture of
  the game (`D:\renderdoc\rome2_carthage.rdc`): the high water technique (no whitecaps), the game's screen-space reflection over the
  mirrored sky; at the same camera and weather the image is within a few per cent of the game's.
- **done** Distant shadows (2026-11-01, the capture; user report: shadows ended at a distance): the baked shadow map past the
  cascades (`renderer/shadow_baked.rs`): the casters seen along the sun, depth = height, 4272 units over 4096 texels, faded in from
  238.7 units (the cascades end at 341) and out towards 1922. The game scrolls one map toroidally around the camera; the editor renders
  it around the camera on a 128-unit grid. ASSUMPTION: the capture's constants (span, height range, fades) hold for every battle.
- **done** Grass and ground brightness (2026-11-01, the capture; user report: grass too bright): ps_height_map_cb's noise on (albedo times
  0.3 + 0.7 x `terrain_noise` at world xz / 1000) and the mid-distance detail at (20, 400) game units (it was off, and at doubled
  distances); Rome 2's grass_PS constants (g_alpha (1, 0.75), normal decode (2, -1), lod distance 200). Then (user report: dark grass
  patches at mid distance): the game resolves the ground with `ps30_terrain_to_gbuffer_ts`, from finished maps (albedo, normal * 0.5 +
  0.5, gloss) its virtual texture path writes; `_ss` (the editor's) reads the composite's sums and divides the noise back out, so the
  ground stayed bright while the grass (which reads the maps as `_ts` does, and takes the ground's colour from 100 units) came out dark,
  and read signed normals as encoded ones. The editor converts its composite into those maps (shared.rs `ts_maps_ps`, an editor helper)
  and resolves with `_ts`; the grass draws with the game's `ps30_gbuffer_no_clip_grass` (one pass, the blade's colour divided by its
  alpha). The beach view: sand and grass within 5 % of the game's.
- **done** Roof stripes in the baked shadows (user report): casters stored 1 unit lower, drawn at the view's levels of detail.
- **done** Depth of field (2026-11-01, the capture; `renderer/dof.rs`): `pixel_dof_coc` then `cs_dof_horizontal_blur` /
  `cs_dof_vertical_blur`, before hdr_to_screen; the distant city blurs as the game's.
- **done** The shadow atlas at 4096 (the game's at the highest quality); the casters' culling: the light-facing faces (the editor's light
  space is mirrored against the game's: FRONT for the cascades, BACK for the baked map).
- **done** The buildings' climate variants (`cliff_rome_med_diffuse.dds` ..., `Renderer::climate_variant`; ASSUMPTION: resolved as the
  custom meshes' are).
- **checked, nothing to do** The sea tiles' LF scale (400 in the game): the sea LF is 0 around Carthage (5246 non-zero pixels on the whole
  map, as Attila's), so `LF * 400 - 20` is the editor's -20.
- **done** A bright weather against the game (2026-11-01, `D:\renderdoc\rome2_carthage_bright.rdc` / `_bright_close.rdc`; the game
  picked `rome_med/su/land/day/dry/day_dry_04`, matched by its sun direction and colour): the editor's sunlit ground was 10 .. 20 %
  brighter; Rome 2's light pass applies the cloud shade (`cloud_shade.dds`, as Thrones of Britannia), which the editor had off. With it:
  within 2 .. 12 % (the shade's drifting offset differs). `day_dry_01`'s far haze looking into the sun is the weather's own (the same
  pipeline matches the game's `day_dry_04` and `day_dry_05`); not compared with the game in that file.
- **done** Objects off the ground in a map context (2026-11-01, user report: the grass pebbles floated everywhere): the open tile's ground
  was drawn relative to the mean LF under it, 0.2 m over the flat LF the campaign has under the battle (its edges pulled the mean up);
  the median now (docs/re/battle_maps.md, "Heights as the game draws them").
- **done** The open tile's edges in a map context (2026-11-01, user reports: a strip of land above the water along the tile's sea edges,
  dark bands where it meets the land tiles): the editor faded the decompiled tile's heights to 0 m at its edges as the export does, but
  they are the game's final ones already (its sea edges at -20 m rose to the surface); it no longer does for a decompiled tile. A floor
  under the open tile closes the gap between its sea edges (-20 m) and the sea tiles' bed (-40 m), through which the sea refracted the
  sky. Its material takes the game's alpha over its border, as the neighbours', so they crossfade instead of meeting in a seam
  (docs/re/battle_maps.md, "The map context's heights at seams").
- **done** The shore foam band (2026-11-01, the capture; it was a thin line where the game's is a wide band): the game draws its terrain
  surface with a slope-scaled depth bias (DepthBias 1, SlopeScaledDepthBias 2, the shadows' state), so where the camera grazes the beach
  the ground's depth lies 1.7 units behind it and the sea covers the shore's edge further up (`States::rs_terrain_rome2`). The sea bed
  holds the ground as drawn, the LF in (the texture within its 8-bit step of the terrain's vertices at the shore); the "0.6 m under its
  sea bed" measured before was that bias read back from the depth, and leaving the LF out of the sea bed (done for it) is undone. The
  game's terrain is a decimated mesh (vertices 2 to 16 units apart), the editor's the full height grid: equal at the vertices.
- **done** The output (2026-11-01, the capture): `hdr_to_screen`'s grain variants with gfx_grain (the image 1.7 x brighter before gamma
  with it); the vignette is the tone mapper's `vignette_buffer` (gfx_vignette; no lens-flare vignette). Gamma and brightness were the
  game's already.
- **done** The terrain-blend meshes (2026-11-01; the settlements' cliff faces, `cliff`, `cliff02`): Rome 2's `rigidterrainblend` has no
  terrain-material entry (ps30_gbuffer, ps30_null_gbuffer, shadow). ps30_gbuffer writes the vertex alpha (0 on the plateau, 1 on the
  faces) into the normal's and the albedo's alpha: drawn after the terrain resolve and blended by it (`terrain_rome2.rs`, step 4) over the
  ground the resolve gave them. Confirmed by the capture: ps30_null_gbuffer first (the terrain's virtual texture), then ps30_gbuffer with
  SRC_ALPHA / INV_SRC_ALPHA on colour (write mask RGB) into the three G-buffer targets. Not applied: the
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

- **done** Rivers (user report 2026-11-01: no river water, in a tile or a map context): the packs ship no river splines, the water
  surface is baked into the terrain mesh's last block (Rome 2's type 48, Attila's 90), which a decompiled tile carries; drawn from it
  (`tile_document::baked_river`, the eight floats passed to the river shader as they are; x, z the tile's metres, z the other way).
  Rome 2 ships the river pixel shader with shadows only: it is the fallback. Lakes (the same report: a white plane): Rome 2's
  `waterplane.hlsl` reads an RGB normal map, its `sea/water_detail_normal.dds` is DXT5nm, so every normal tilted and the sun's
  unscaled specular term glared; the river's normal map bound (ASSUMPTION: the engine's `t_normal` for water planes not traced). The
  lakes come out dark: their planar reflection is not drawn.
- **todo** Launch in game on Rome 2: the battle shows only water across the map (user report 2026-11-01; not looked at yet).
- **done** The tile's weather follows its climate (2026-11-01): a map context gives the tile its placement's climate after the tile picked
  its weather; a weather of another climate's folder gives way to the new climate's first dry one (`WeatherUi::follow_climate`).
- **done** The dark slabs after camera jumps in a map context (2026-11-01): the baked shadow map (4 km wide) took the map context's
  streamed neighbours as casters, whose tall and far models under the low sun shaded the battle and the hazy distance in slabs, and
  which chunks were on the scene when the map was made depended on the streaming. The map takes the battle's own models alone (as the
  game's, made at the battle's load of its tile): draw items on the `CONTEXT_LAYER` are marked (`DrawSet::mark_context`) and the baked
  view skips them (`View::no_context`); never the terrain's patches; depth-clipped, as the game's shadow passes. Fifteen jump views clean
  where every hill view had them.

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
