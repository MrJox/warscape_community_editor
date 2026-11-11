# Radar generator (battle minimap)

Goal: replace the game's automatic battle minimap (made from the buildings, plain) with a picture drawn from the tile in the style of
CA's hand-made `loading_screen_radar.jpg`, which the game shows when the tile folder has one. Artistic: a prototype first, then rounds
of tuning with the user.

## What the game reads (black box, 2026-11-11)

- `loading_screen_radar.jpg` in the battle tile folder: 512 x 512 JPEG (Photoshop CS2, 2014), 87 vanilla folders have one. The export
  carries it from the project's folder (`wce_tile_io::attila::compile::carried_into_export`); nothing in TEd or BOB makes it.
- It covers **the tile without its border** (0 .. extent), north (+z) up: CA's port radars line up with the tile's coast and quays
  only that way (the bordered area is 1.25 x too big on a 1024 m tile with a 128 m border).
- CA's look, measured on nordic_city_b, sassanid_port_b, western_city_b, dunedain_city_c and the TDD ports:
  - parchment, median land about (147, 115, 78), with a faint pattern of rectangular field strips and grain;
  - roads cream-white, about 3 px (6 m) wide, an even network;
  - forests olive (about 90, 95, 50) blobs with a dark edge, only the dense woods;
  - the sea grey-blue to teal (about 100, 120, 125), textured, an ink shore;
  - buildings grey-brown with black outlines, merged into blocks, some red;
  - walls thick black lines;
  - **cliffs**: their top edge a cyan line with ink ticks hanging down the slope (not walls: elven_port_a's cyan lines follow its
    cliffs; western_city_b's arc is the cliff north of the town, its rocks grey beyond).

## Design

- `wce_tile_editor::radar` (pure): `RadarInput` (rasters and outlines in the tile's metres) -> `render(input, RadarStyle)` -> 512 x 512
  RGBA. Drawn at 1024 and averaged down. No assets: the paper is procedural noise (user's choice, 2026-11-11).
- `Tile::radar_input(water)` (`tile_document/radar.rs`) gathers:
  - heights (the height map, every 2 m), the sea (ground under -0.5 m), rivers and lakes (stage B's water depth, the tool's);
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
  - cliffs: the automatic no-go areas plus ground steeper than `cliff_slope`; sea cliffs: land at the shore 3.5 .. 6 m or more above
    the lowest ground within 8 m (the steep band at the waterline is too thin for the slope test). The rims are drawn after the water,
    whose shore ink would cover them.
- Ink extras (user: "vanilla base, ink extras"): a soft hill shade, hachures on slopes over `hachure_slope`, ripples off the shore.
- Editor: the Minimap panel's base chip has **radar** (redrawn at most every second after an edit, once the buttons are up; the tool
  works out the water depth and the no-go areas while it shows). `minimap radar`, `radar_save [path.png]` (default: the tile folder's
  `loading_screen_radar.png`).
- Iterating: `cargo run --release -p wce_tile_tool --example radar -- <game> <out> <tile folder>... [--mods ...] [--kit ...]` writes
  `<tile>_radar.png` and copies CA's radar beside it (`<tile>_ca.jpg`). `RADAR_TREES=<list>` picks the tree list (the editor shows the
  climate's; the example's default is `default`), `RADAR_TREE_DUMP=1` / `RADAR_SLOPE_DUMP=1` write the trees / the slopes at 1 m a pixel.

## Open

- JPEG: the game wants `loading_screen_radar.jpg`; the prototype writes PNG (user, 2026-11-11: no new dependency for now). Options: the
  `image` crate's `jpeg` feature (zune-jpeg, zune-core) or a small baseline encoder of our own.
- Save next to the export: a button and the export carrying the generated file, never overwriting a hand-made one without asking.
- Tuning with the user: colours, the paper, forest density, road widths, which decals are roads, hachure density, red roofs (now 30 %
  at random; CA's red may mean something: important buildings?).
- The radar is drawn on the UI thread (0.25 s on dunedain_city_c); move to a thread if it gets in the way.
- Style settings in the UI (now `RadarStyle::default()`).
