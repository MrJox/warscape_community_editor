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
3. **Visual editing, the rest** (UX pass, 2026-11-13; the first part is done: the sun handle, the focus pick, compare, the tone curve and
   histogram, `wce_weather_tool` `visual.rs`, docs/editor.md). Raised with the user, not started:
   - A wind arrow on the ground (wind_dir_degrees and strength), fog rings at the fog's start and end while a Fog row is edited, a colour
     eyedropper from the viewport (or a game screenshot) for the fog, sky and ambient colours.
   - A sky gradient strip (horizon to zenith) and a fog density curve by distance in the Inspector.
   - A preview-only switch per section (fog, grading, clouds off) to see what each does; nothing written.
   - A split wipe between the saved file and the edits (or two files). Compare flips the whole frame instead: the renderer's temporal state
     (exposure adaptation `tonemap.luma_*`, the environment cube) is the renderer's, not the view's, so two environments in one frame would
     fight; a wipe needs that state per view first.
   - Mix two files into a new one: the cross-fade preview's blend (`environment::blend`) written out as a file.
   - A gallery of the game's weather files rendered on the current scene, filtered by climate, season and time of day.
   - A game screenshot beside or over the viewport, from the same camera, to match a look by eye.
4. **One panel: everything from the Inspector** (user, 2026-11-13; how to do it is for later). Every edit the Properties panel makes
   reachable from the Inspector, each Inspector section collapsible, so the Properties panel can go. Today only Properties has: the tree
   of `weather_definitions.xml` and VFX files (picking the element the Inspector edits), structural edits (add, duplicate, move, delete
   elements, e.g. a second cloud layer), the elements no section lists (cloud sprites, scene nodes), the attribute filter, the Changes
   tab (per-attribute revert) and Revert all; its Save button repeats File > Save.
