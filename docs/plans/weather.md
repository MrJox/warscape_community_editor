# Weather tool: plan

Edit Attila's weather and lighting (`.environment`, `.lighting`, `weather_definitions.xml`; VFX XML since 2026-11-18 in the VFX tool) with
a preview that runs the game's own shaders; game data through the VFS, loose files written byte-identically. File layouts: docs/formats/weather_files.md.

## Status (2026-11-22)

Done: the battle weather pipeline (Phases 0 and 1), wind, precipitation and the VFX runtime (Phase 2 items 1 to 3), the weather
selection database (Phase 4: `warscape_asset_variation_db` picker and XML / FASTBIN conversion), more materials (Phase 5), real battle tiles
(Phase 6: the Tile tool), the visual editing pass (2026-11-13, item 3 below). The weather tool in `wce.exe` (docs/editor.md); `.lighting`
files open and light the scene. VFX files moved to the VFX tool (2026-11-18). Engine leftovers of the weather and particle code:
docs/plans/weather_current_state.md.

## Decisions

- Output is loose files only, byte-identical round trip (attribute order, quoting, whitespace, line endings, encoding, untouched number
  text). No own pack writing.
- The preview runs the game's compiled shaders; the inputs are checked against memory dumps, not whole frames (docs/formats/weather_files.md).
- Modes: battle and campaign. Particles, VFX and precipitation are in scope.
- No general DB table editor (2026-10-08): only `warscape_asset_variation_db` is edited (docs/re/variation_db.md).
- The weather files have no keyframes; change over time is the engine's cross-fade, so the tool offers a cross-fade preview between two
  files (done for battle).

## Open

1. **VFX editor** (Phase 2 item 4): moved to a tool of its own, [vfx_tool.md](vfx_tool.md) (2026-11-18, built). The Weather tool no
   longer lists or opens vfx files.
2. **Campaign mode** (Phase 3): moved to [campaign_environment.md](campaign_environment.md) (2026-11-16), which holds its plan.
3. **Visual editing** (UX pass, 2026-11-13): done. The sun handle, the focus pick, the split compare, the tone curve and histogram,
   Mix with another file, the wind arrow, the fog rings and the Inspector's fog curve, the preview switches, the sky strips, the gallery,
   the reference screenshot with its colour comparison (`wce_weather_tool` `visual.rs`, `mix.rs`, `preview.rs`, `sky_strip.rs`,
   `gallery.rs`, `reference.rs`; docs/editor.md). Left as they are: the colour eyedropper writes nothing into the file (the files'
   colours are lit and tone-mapped inputs; the comparison guides the edit instead); fog_distance_start is an offset, not a distance
   (docs/re/renderer.md, "Battle fog").
4. **One panel: everything from the Inspector** (user, 2026-11-13; how to do it is for later). Every edit the Properties panel makes
   reachable from the Inspector, each Inspector section collapsible, so the Properties panel can go. Today only Properties has: the tree
   of `weather_definitions.xml` (picking the element the Inspector edits), structural edits (add, duplicate, move, delete
   elements, e.g. a second cloud layer), the elements no section lists (cloud sprites, scene nodes), the attribute filter, the Changes
   tab (per-attribute revert) and Revert all; its Save button repeats File > Save.
