# Radar generator (battle minimap)

Goal: replace the game's automatic battle minimap (made from the buildings, plain) with a picture drawn from the tile in the style of
CA's hand-made `loading_screen_radar.jpg`, which the game shows when the tile folder has one. Artistic: a prototype first, then rounds
of tuning with the user.

## What the game reads

The file, its extent and CA's look (black box, 2026-11-11): docs/re/battle_maps.md, "Radar (`loading_screen_radar.jpg`)". The export
carries the file from the project's folder (`wce_tile_io::attila::compile::carried_into_export`).

## Design

- `wce_tile_editor::radar` (pure): `RadarInput` (rasters and outlines in the tile's metres) -> `render(input, RadarStyle)` -> 512 x 512
  RGBA. Drawn at 1024 and averaged down. No assets: the paper is procedural noise (user's choice, 2026-11-11).
- `Tile::radar_input(water)` (`tile_document/radar.rs`) gathers:
  - heights (the height map, every 2 m), the sea (ground under -0.5 m, on a coastal set's tile only: `tile_sets::coastal_set`, the
    sets the game draws a sea on; another's low ground is dry ground), rivers and lakes: stage B's water depth and the tile's own
    water planes (their footprints from the model file's mesh headers when the viewport has not loaded the model: hidden
    layers, far from the camera) and river ribbons (`Tile::water_map`; its heights are the scene's, metres / unit), still water: no ripples or
    streaks. Any water from 1 cm deep shows (a plane barely over mud, a marsh's puddles: middle_men_port_a's town and stream),
    sampled every metre; shallower than `shallow_m` it is a little lighter with a light ink edge;
  - roads: the procedural roads' centre lines, the `path_*` / `road_*` decals as lines along their x, and the blend map's thin layers
    (`radar::blend_roads`: per channel, what is left after an opening of about 10 m, its long pieces) only when the tile has fewer than
    50 road lines; road pixels where roads cover most of the ground around (a vineyard's paths) are dropped;
  - trees: the shown tree list and the static vegetation, trees only (the lists hold shrubs, flowers and stones too: over half of
    middle_men_port_a's `rome_continental` list); a forest where they are denser than `forest_density` about 4 m around, so thin strips
    of wood stay strips;
  - buildings: exported, not fort pieces, not rocks / barricades; their hard outlines, else their logic's `collision_3d` triangles above
    1.5 m seen from above (vanilla houses have no 2D outlines); house-sized only (2.5 m across, 10 m2);
  - walls: the siege input's fort pieces (footprints and the line between their ends);
  - streets and squares: the exported siege AI's street edges and area nodes;
  - cliffs: the automatic no-go areas plus ground steeper than `cliff_slope`, none at inland water's banks or inside a water plane's
    footprint (middle_men_port_a's stream bed steps down past planes its floor stands 0.3 m above: a bank, not a cliff); sea cliffs: land at the shore 3.5 .. 6 m or more above
    the lowest ground within 8 m (the steep band at the waterline is too thin for the slope test). The rims are drawn after the water,
    whose shore ink would cover them.
- Cliffs (`RadarStyle::cliff_style`): `Rock` (default since 2026-11-11, on trial: the user was not convinced by the cyan rims) draws the
  rims (where `Rim` draws its line, a little wider: the user wants only those changed) as grey stone in lit and shaded facets (`rock_facet_m`), cracked,
  outlined in ink (taking the fairly steep ground at a cliff's foot too turned half of middle_men_port_a's hills into rock: not
  done); `Rim` is CA's cyan top edge with ink ticks. The example takes `RADAR_CLIFFS=rim|rock`.
- Ink extras (user: "vanilla base, ink extras"): a soft hill shade, hachures on slopes over `hachure_slope`, ripples off the shore.
- Editor: the Minimap panel's base chip has **radar** (redrawn at most every second after an edit, once the buttons are up; the tool
  works out the water depth and the no-go areas while it shows). Since 2026-11-16 the drawing (`Tile::radar_request`, then a Job in
  `minimap_panel.rs`) and stage B run off the editor's thread; `radar_save` works out the water depth and the no-go areas itself when the
  tile has none yet. `minimap radar`, `radar_save [force] [path.jpg|path.png]` (default: the tile
  folder's `loading_screen_radar.jpg`, which the export carries). The JPEG is baseline RGB at quality 92 (`image`'s encoder, already
  built for rpfm_lib) with a `wce radar generator` comment after the JFIF header: a radar without it (CA's, in a decompiled tile) is
  hand-made and only `force` replaces it. A `.png` path writes a PNG.
- Iterating: `cargo run --release -p wce_tile_tool --example radar -- <game> <out> <tile folder>... [--mods ...] [--kit ...]` writes
  `<tile>_radar.png` (and `<tile>_radar.jpg`, what the game gets) and copies CA's radar beside it (`<tile>_ca.jpg`). `RADAR_TREES=<list>` picks the tree list (the editor shows the
  climate's; the example's default is `default`), `RADAR_TREE_DUMP=1` / `RADAR_SLOPE_DUMP=1` write the trees / the slopes at 1 m a pixel.

- Export (2026-11-15): every export, pack save and launch draws the radar into the folder it compiles (`TileTool::export_parts`,
  `minimap_panel::ExportRadar`), which the compile carries: the project's folder, or each variation of a combined project in its own
  written folder, while that variation is viewed alone. Its rivers, lakes and cliffs come from stage B of that folder (the kits and
  the packs as the editor has them; about 2.5 s a variation on middle_men_port_a). A radar without the `wce radar generator` mark (a
  hand-made one, CA's in a decompiled tile) is exported as it is.

## Status (2026-11-22)

Built on master: the renderer, the Minimap's radar base, `radar_save` (JPEG with its mark), the export (2026-11-15). Left: the
user's tuning rounds.

## Open

- Asked (2026-11-15): the user sees the saved radar with the wrong rotation / mirroring and wants it over the playable area only. Our
  middle_men_port_b medium matches TDD's hand-made one there in both (north up, the whole tile without its border): what they compared
  it with is to be found out.
- Tuning with the user: colours, the paper, forest density, road widths, which decals are roads, hachure density, red roofs (now 30 %
  at random; CA's red may mean something: important buildings?).
- Style settings in the UI (now `RadarStyle::default()`).
