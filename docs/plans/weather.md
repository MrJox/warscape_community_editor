# Weather tool: plan

Edit Attila's weather and lighting (`.environment`, `.lighting`, `weather_definitions.xml`, VFX XML) with a preview that runs the game's
own shaders; game data through the VFS, loose files written byte-identically. File layouts: docs/formats/weather_files.md.

## Status (2026-10-26)

Done: the battle weather pipeline (Phases 0 and 1), wind, precipitation and the VFX runtime (Phase 2 items 1 to 3), the weather
selection database (Phase 4: `warscape_asset_variation_db` picker and XML / FASTBIN conversion), more materials (Phase 5), real battle tiles
(Phase 6: the Tile tool). The weather tool in `wce.exe` (docs/editor.md); `.lighting` files open and light the scene. Engine leftovers of the
weather and particle code: docs/plans/weather_current_state.md.

## Decisions

- Output is loose files only, byte-identical round trip (attribute order, quoting, whitespace, line endings, encoding, untouched number
  text). No own pack writing.
- The preview runs the game's compiled shaders; the inputs are checked against memory dumps, not whole frames (docs/formats/weather_files.md).
- Modes: battle and campaign. Particles, VFX and precipitation are in scope.
- No general DB table editor (2026-10-08): only `warscape_asset_variation_db` is edited (docs/re/variation_db.md).
- The weather files have no keyframes; change over time is the engine's cross-fade, so the tool offers a cross-fade preview between two
  files (done for battle).

## Open

1. **VFX editor** (Phase 2 item 4). The VFX tool is still a placeholder (`crates/wce_app/src/tools.rs`, feature `placeholders`); vfx
   files open as XML documents of the weather tool. Wanted: a tree view of emitters and behaviour chains, a curve editor for
   `ranged_property`, byte-identical UTF-16 save, a standalone preview scene.
2. **Campaign mode** (Phase 3).
   - A campaign camera rig and scale.
   - A campaign test map: a flat plane with random `rigidmodels/campaign` models.
   - Campaign shaders: shroud, campaign fog, campaign cloud and mountain VFX. `g_is_campaign` is hardcoded 0
     (`wce_render` `renderer/decals.rs` ~391, `renderer/terrain.rs` ~1107).
   - The campaign skybox model and cube map.
   - A season cross-fade preview (spring, summer, autumn, winter) as the game does it.
