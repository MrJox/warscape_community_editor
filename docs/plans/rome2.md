# Rome 2 support (and other games)

The editor is built for Attila; this plan adds Total War: Rome II, generically, so that later games follow the same path (Thrones of
Britannia has its profile too). Rome 2's Assembly Kit has no tiles, no TEd and no BOB tile, vegetation or terrain processors: its tiles
come from the packs and go into mod packs. Formats: docs/formats/rome2_tiles.md; renderer: docs/re/rome2_renderer.md; effects:
docs/re/rome2_vfx.md.

Decisions (user, 2026-10-25): mod packs are the only output for Rome 2; a mod overriding a whole tile set file is acceptable; the
Attila ports of BOB's stages are good enough for generated data (checked against the shipped files and in game, no Rome 2 oracle);
siege AI and procedural walls are in scope (merged into master 2026-10-26); the game switches live in the editor.

Status legend: **done**, **partial**, **todo**.

## Phase 0: games as data — done

- `wce_foundation::game::GameProfile` (re-exported by `wce_vfs`): key, Steam folder and app id, preferences folder, rpfm's schema, the
  campaign battle map, the default environment, the vanilla packs (Attila's manifest; a list for Rome 2), the executable
  (`GameProfile::exe`), `GameFeatures` (`kit_tiles`, `civilians`, `escalation`, `lite_buildings`, `variation_db`, `kit_terry`, ...) and
  `unsupported_tools` (the ids of built tools a game hides; empty for every game). `GameProfile::ALL` (Attila, Rome 2, Thrones),
  `by_key`, `from_env` (`$WCE_GAME_KEY` for the examples and gates).
- Pack order: patch packs (header type 2) before release ones; an empty file in a pack removes the file below it. Rome 2's list is
  taken in name order (ASCII, case aside) inside each type, the first winning, as the engine loads them (`models3_rome2.pack` holds the
  soldiers' parts `models_rome2.pack` empties; 2026-11-12).
- The game switches live: File > Game, the Settings window's Game page and the toolbar's game combo (`Editor::switch_game`: documents
  close, asking about unsaved ones; `Tool::game_changed`; packs and renderer made again; a game not set up opens the first-run dialog).
  No `"attila"` literal outside the profiles; `Files::game` picks the renderer's passes (`RenderGame`).
- References (2026-11-12): a naval unit's ship is the definition named after its `ship_dbs.model` (Rome 2 has no
  `unit_variants_ships`); the `weightedcommon` vertex shaders sample the bone palette (`s_animation_map`, `renderer/skinned.rs`).

## Phase 1: Rome 2's tiles read and written — done

Done: the XML tile database (`rome2::tile_database`, all 1,437 tiles byte for byte); `GameData` / `workflow::Game` on the mounted packs
without a kit; `TileConventions` (decal sign, building list versions 10 to 12, `burn_amount`, PROP_MARKER v4, GENERIC_MARKER v1);
Rome 2's extra files carried (`tile.ted_binary`, `.outlines`); multi-list capture location files; Save to mod pack (all games); stage B
(the linking tiles decompiled with their final heights; no building stamps) and stage C (the project's static trees into the older tree
lists); the campaign battle map context (`tile_map` v6, `locations.blm` v7, neighbours decompiled from the packs on demand,
`workflow::neighbour_root` / `neighbour_tile`). Dropped: byte-exact building lists (the tool's string table order is not a model found;
the values are equal).

Open:
- The stage B port decimates meshes more than Rome 2's BOB did (3x fewer vertices).
- The grass list follows no edit (no grass parameters in Rome 2's kit).
- PARTICLE_EMITTER_MARKER v4's float in `unknown_64` comes back an ulp off from the text.
- Map context: the neighbours draw coarser than the open tile; the sea between them is not drawn (holes).

## Phase 2: Rome 2's renderer — partial

Done (details in docs/re/rome2_renderer.md and docs/re/graphics_options.md, "Rome 2"; checked against the user's screenshots and the
RenderDoc captures `rome2_carthage*.rdc` of 2026-11-01): the graphics options as its engine applies them (`gfx <option> <level>`); the
shaders of its containers, tone mapping in log Y, grain and vignette; the terrain (surface, blend, the DX11 composite, the `_ts` resolve
from the composite's maps, the ground noise and mid-distance detail); the cliff tiles' custom meshes and their `ps30_custom_geom_mat`
material (2026-11-16); the settlements' terrain-blend meshes (`terrain_rome2.rs`); the shadow cascades (its orthographic fit), the 4096
atlas, the baked distant shadows (`renderer/shadow_baked.rs`, the map context's streamed models left out of it), the cloud shade; the
forward sea (high technique, planar and screen-space reflection, the shore's slope-scaled depth bias, the foam band); rivers and water
planes; trees, grass (`ps30_gbuffer_no_clip_grass`), billboards and decals; depth of field (`renderer/dof.rs`); the buildings' climate
variants (`climate_variant`, `renderer/grass.rs`); the billboard effects as a port of the game's CPU simulation (`vfx_rome2::sim`); the open
tile's ground on the median LF and its edges in a map context.

Open:
- Shadow levels 1 to 3 leave a gap between the last cascade (151 / 176 units) and the baked shadows (from 239); the medium sea shader
  draws no shadows: check in game. The baked map's constants (span, height range, fades) are an ASSUMPTION from one capture.
- The terrain: the channel swizzle's source, the far composite, the virtual texture path.
- The effects: Distortion, SeaColourOverlay and shadow views, ribbons, spawn spacing, SpawnByMesh, particle events, rigid emitters
  (point lights), the weather particles; a capture to compare. The engine's `g_hide_foliage`.
- The terrain-blend meshes' climate texture variants (`cliff_rome_med_diffuse.dds`, ...) are not applied (not traced for buildings).
- Against the game (user report 2026-10-26, `settlement_egyptian_cities/egyptian_minor_city_c` near Rekem): the ground is still browner
  than the game's pale grey (most likely the game lit it as ROME_Med; to render with it); the haze. Not cliffs: battle cliff textures
  are only on `cliff_custom` link tiles (~/wce_re/scratch/rome2/cliff_re/notes.md). A tile alone takes the editor's climate
  (`ROME_Continental`); in a map context its placement's (`ContextPanel::adopt_climate`).
- Lakes come out dark (their planar reflection is not drawn; the water planes' `t_normal` is an ASSUMPTION).
- **blocked** A RenderDoc capture of a battle under Linux: Rome 2 is 32-bit and RenderDoc on Linux 64-bit only; under Proton's WoW64
  (`PROTON_USE_WOW64=1`) DXVK can't map memory on the NVIDIA driver (`VK_ERROR_OUT_OF_HOST_MEMORY`). To try: the Intel GPU
  (`DXVK_FILTER_DEVICE_NAME`) or Windows. Steam launch options `... bash -c 'exec "${@/launcher\/launcher.exe/Rome2.exe}"' -- %command%`,
  then RenderDoc's Attach to Running Instance; captures to `~/wce_re/scratch/rome2/captures/`.

## Phase 3: editor — partial

Done: rivers from the terrain mesh's baked river block (`tile_document::baked_river`), and editable: the decompile fits splines to it
(docs/formats/tile_decompile.md, "Rivers"; a river edit replaces only the block, type 48, and remakes `hf_water_map.data`); the tile's
weather follows its climate (`weather_panel::follow_look`); the map context in Rome 2 (variation-less names, the sea tiles' common maps,
the LF window's orientation); the Tile menu without the kit's Save for / Import from for a game whose kit has no tiles; the weather tool
on Rome 2's environments; the UI tool on its layouts (version 86); `GameFeatures` hiding what Rome 2's tiles lack (civilians,
escalation, lite buildings; the weather variation database); procedural walls and the siege AI generator on its settlements; Launch in
game per game (`launch_army` per game, the battle XML without what Rome 2 lacks); Assembly Kits per game; the packs' tile database
standing for the kit's.

Since (in other plans): Rome 2's battle and campaign maps in the Terrain tool, read, edited in their built form and exported
(docs/plans/terrain_editor.md, Phase 9); its startposes' settlements and roads (terrain_editor.md, Phase 8); the river tools' Rome 2
step and its shores table (docs/plans/rivers.md); its audio banks (docs/plans/audio.md).

Open:
- Launch in game on Rome 2 is **partial**: its battles always name a battle map folder (fn 0x114e8a40 reads `tile_map.index` /
  `.tiles`; no lone-tile path found), so Launch in game points at the map context's placement. To test in game: a launch at a
  placement.
- A terrain edit of a decompiled tile remakes the whole mesh in the port's (Attila's) format, not Rome 2's 0x42 blocks; not checked in
  game. Projects decompiled before the river splines keep the baked block (`open_exported ... --fresh` decompiles them again).
- In game, to confirm: procedural walls, the siege AI (roman_city_a/small: 76 fort pieces, 11 areas, 17 streets, 4 wall edges), Launch
  in game, the burning buildings' instance values.

## Validation without TEd

- Oracles: the game's files (decompile + compile within the text's precision, the database byte for byte), static RE of Rome 2's
  kit DLLs (EmpireUtility, ToolDataBuilderDll) and its `empire.retail.dll`, the game itself (a tile saved into a mod pack and loaded:
  todo, by a human).
- Gates: `scripts/gates.sh` with `WCE_ROME2` (docs/formats/rome2_tiles.md, "Checks").
