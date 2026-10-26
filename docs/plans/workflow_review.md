# Workflow review: TEd's workflows in wce (started 2026-10-26)

A review, workflow by workflow, of what the Assembly Kit's tools (TEd, BOB) do and whether wce replicates it, supersedes it or needs changes.
Held as a conversation with the user, one workflow at a time; this file keeps what was found and decided.

## 1. Tile creation

**Before:** wce had no way to make a tile. A tile came in only by opening one: a kit tile (its first save makes a project), an exported tile
extracted from the packs (`open_exported`, `_extracted`), or a loose `tile.ted`. `save_to <folder>` writes a copy elsewhere, without a new
entry or location. The entry was read-only (Phase 6b of tile_ted_parity.md: New tile, Save as, resize, tile properties, database management).

### What TEd does (static RE of `TEd.AssemblyKit.exe` and `Warscape.AssemblyKit.dll`, both 32-bit; Ghidra projects in `C:\Users\frodo\ghidra_work\ted`, full notes in its `new_tile_findings.md`; TEd was not run)

- **New Tile writes nothing.** Menu `FUN_004b85d0` -> `FUN_004b9d80` -> dialog `FUN_004db6f0` (rows: Database, Size X x Y, Triangle
  Density, Terrain Height), then `FUN_004b9f30(db, X, Y, H, density)`: the terrain editor makes the tile in memory (Warscape `0x104b4890`),
  every height sample is set to H (`FUN_004b74a0`), the document gets `normal_strength` 2, `requires_infield_lodding`, colour (0, 0, 0), no
  name; texture groups `grass, mud, forest, rock` for battle (`grass_a, grass_b, sand_a, rock` otherwise). No name is asked: the window is
  "new tile (XxY)", the name and variation come with Save As.
- **Limits:** X and Y 1..64 (`FUN_004dac00(1, 0x40)`); density Normal 128, Low 32 or Custom (a spin box, 16..128); above 16 cells on a side
  the density switches to Low unless the user changed it (`FUN_004dc370`); Terrain Height 10 for battle, 0 for campaign (`FUN_004b7a30`). No
  size or memory check: 64 x 64 at 128 means about 4.5 GB of blend floats, which the 32-bit TEd cannot allocate.
- **On save** (`FUN_004bbd30`, Save As `FUN_004bc4a0`): `height_map_0.png` (16-bit grey), `blend_map.tif` (16 channels; at creation
  channel 0 = 1, the rest 0), `ground_type_map.png` (RGBA; at creation (0, 0, 0, 255)), all `(cells + 2) * density` a side; `tile.ted`
  (version 17: Vegetation with generation, decal and vfx seeds, Grass with one, the scene, the camera; a seed of 0 becomes
  `round(timeGetTime() * 0.5)`); the entry; a BMD export into working_data and `protection_map.dds`.
- **The height range is fitted on every save** (`FUN_0051a310` / `FUN_00518d60`, TerrainIO.cpp): min_height = the lowest sample, scale =
  the highest minus the lowest, no padding; a flat map gets scale 50. A new flat tile at H is saved with min_height H, scale 50, every
  sample 0.
- **The colour** (Save As Battlefield, `FUN_004b7300`): the first three bytes of the MD5 of
  `<kit>/working_data/terrain/tiles/battle/assembly_kit/<name>/battlefield/` (forward slashes, trailing slash), then the colour + 1 as a
  24-bit number while another tile has it. Checked on tesd, testd, testdoras, hornburg, orc_test and testise.
- **Names:** Save As needs a name and a variation (`FUN_004dd210`), lower-cased, nothing else checked. Save As Battlefield forces the
  `ak_battle` database, variation `battlefield`, outfield tile set `plains`; an existing name of another size is refused, of the same size
  it offers to save as a variation (a battlefield has one); its entry goes to working_data's `_ak_bf_tile_database/TILES`. Save As
  Battlefield then also makes the battle (`terrain/battles/<name>`) and runs BOB: the next workflow (making a battle from a tile).
- **The tile database:** New Tile never touches `tile_database.bin`; TEd lists tiles from the `TILES/*.xml` entries (Warscape's
  `TILE_DATABASE` constructor `0x1029b570` reads the binary only for a path ending in `tile_database.bin`, else the XML folder, merging
  `_ak_bf_tile_database` for battle).

### Decisions (user, 2026-10-26)

- A **Tile > New tile...** that makes a **project** (the projects folder in the kit's layout), never an Assembly Kit tile.
- **Battle only**; a **blank flat tile**; tile set **`assembly_kit`** (`terrain/tiles/battle/assembly_kit/<name>`, variation `battlefield`).
- The dialog: **name** (required, no spaces; asked at once, since a project needs its folder), **size** X and Y (1..64, default 8),
  **terrain height** (default 10: all the terrain is at it when the tile is made), **terrain density** (32, 64 or 128, default 128; TEd's
  custom value may come later). TEd allows 64 x 64 at 128 (a 2.3 GB `blend_map.tif`): so does wce, the dialog says how large the files
  are.
- **The minimum of a map is written at once**: the entry, `tile.wce`, `height_map_0.png`, `blend_map.tif`, `ground_type_map.png`.
- **The height range is fitted on save, as TEd does** (with New tile).
- **`tile_database.bin` is not touched**: it is compiled from the entries (BOB's "Process tile database"); TEd lists tiles from the
  entries; wce's export (`save_exported`) already merges the project's entry into the mod's `tile_database.bin` (docs/formats/tile_workflow.md);
  Save for Assembly Kit writes the entry into the kit's `TILES`, from which BOB compiles. A project's database would never be read.

- **The terrain height keeps TEd's meaning** (user, 2026-10-26): the value goes into the height map's units, which wce shows halved (the
  scene's 2 units a metre): a new tile at 10 shows 5 m in wce, as every kit tile shows half its entry's numbers.

### Done (branch `workflow-review`)

- `wce_tile::new_tile`: the values, the checks (name, limits, an existing tile in the projects, the kit's raw_data or working_data), TEd's
  colour, the entry's text (TEd's: single quotes, tabs, CRLF), `tile.wce` (version 17, the seeds from the clock, the root with an empty
  `<children>` so that things go in, a camera over the tile), the maps (the blend map streamed row by row). The kit's `_settings.xml` is
  copied next to the projects' entries when they have none (the vegetation generator reads it).
- The height field keeps the heights in metres (32-bit offsets from the range's minimum it was read with, far below a 16-bit step), so
  brushes are no longer stopped by the entry's range; a save that writes the height map fits the range (`HeightField::fit_range`, TEd's
  rule; a range within half a step of the old one is kept, so an unedited map is written with its own numbers) and writes the entry's
  `min_height` / `scale` (`%f`) into the entry next to the saved tile when its text changed (`TileProject::save_height_range`). The first
  save of a kit's tile points the document's entry at the project's copy (it pointed at the kit's). The vegetation generator reads the
  heights with the range the next save will give them.
- `add_building` on a tile without buildings makes one with TEd's defaults (`instance_templates`) instead of failing.
- Tile > New tile... (`wce_tile_tool::new_tile_dialog`) and the command `new_tile <name> [x] [y] [height] [density]`; the tile opens.

- Old bugs fixed on the way (user, 2026-10-26): opening a tile that is open shows it instead of a second document (`Document::opened_from`,
  `scripts/tile_reopen.txt`); a kit or extracted tile's other variations open once the tile is a project (they open from the kit or the
  packs until their first save brings them in; the project's entry is kept, each save writing only its own variation's numbers;
  `scripts/tile_variations.txt`, `tile_variations_extracted.txt`).

## 2. Extracted tiles without the `_extracted` workaround (user, 2026-10-26)

**Before:** a tile extracted from the packs was decompiled into the projects at `<tile>_extracted` (its folder and its entry's location),
so that it never took a kit tile's place in the old workspace; Save for Assembly Kit, the game export and the walls' keys took the mark
off. The renaming caused bugs of its own (names and locations that differ from the game's, entries renamed back and forth).

**Decisions:** no renaming; the extracted tile is a project at its own folder (the kit's tile of that name and it share the project:
they are the same tile, the kit's source and the game's compiled copy; Import from Assembly Kit still takes the kit's copy); its
`tile.wce` root is marked `wce_extracted_tile="true"` (a `wce_` attribute: the conversion for TEd and the game drop it), which the
Tiles panel shows; extracting over a project asks **Open the project / Overwrite / Cancel**; the old `_extracted` projects are moved once,
by a command (Tile > Migrate old extracted projects), not on open.

**Done:** `project_files::{EXTRACTED_ATTR, mark_extracted_text, is_extracted_text, is_extracted_dir, migrate_extracted}` (the old marks
kept private for the migration only); `workflow::open_exported` writes at the tile's own folder and entry (no `extracted` argument);
`save_exported`, Save for Assembly Kit and the walls' keys use the folder as it is; the tool's `open_exported` replaces a project only
when `fresh` (closing its document, refused with unsaved changes), keeps the other variations' entry numbers, marks the new `tile.wce`;
`extract_or_ask` and the prompt for the Tiles panel's packs and Tile > Open exported tile...; an extracted project's missing variation is
extracted (a variation of it is marked) instead of taken from the kit; Import from Assembly Kit reads the mark.

## 3. Ground texture painting (next; started 2026-10-26)

**What wce does today:** displays the ground with the game's terrain shaders (`blend_map.tif`: 16 weights per pixel, channel 4 g + v =
texture group g, variation v, summing to 65535; the entry's four `TEXTURE_GROUP`s; the climate and season the user picks) and TEd's
debug views (colour, blend groups, ground types, protection). It edits none of it: `blend_map.tif` and `ground_type_map.png` pass
through on save, Save for Assembly Kit and the export (compiled to BOB's `blend.dds`, `index.dds`, `ground_types.dds`); New tile writes
them (all the first group, no ground types) and the extraction rebuilds them from the game's maps (a group that mixed more than two
variations loses the rest). The texture groups are read-only (the entry is), the material is built once from the file on disk (no live
redraw), the procedural trees read the saved blend map, and only the height map has stroke undo. Planned as tile_ted_parity.md 5a.

**Decisions (user, 2026-10-26):**

- **Blend brush:** the user picks a group and a variation (one of the 16 channels); a dab moves weight towards that channel and scales
  the other 15 down so that the pixel keeps summing to 65535; erasing is painting the base channel.
- **Ground types:** a brush of their own with a palette of the ground types (`ground_type_map.png`'s colours,
  `EMPIREUTILITY::ground_type_to_colour`), hard values, no blending, drawn over the ground types debug view.
- **Texture groups:** editable from a picker in the Tile panel (the game's texture groups); a text splice of the entry, the terrain
  material rebuilt, undoable.
- **Live preview with the game's shaders:** the edited rectangle of the blend texture is uploaded during the stroke; one undo entry per
  stroke.
- **wce's own brush math, TEd's files:** the dab is not reverse-engineered; what is written goes through the kit-exact encoders.
- **All of TEd's extras:** the falloff curve editor, the image brush, the spline paint stroke and the Blendmap Filters (which may be broken
  in TEd: find out what they really do before porting).
- **Order:** 1) blend brush + live preview + stroke undo, 2) ground type brush, 3) falloff curve editor, 4) image brush, 5) spline paint
  stroke, 6) Blendmap Filters; the texture group picker alongside.

**Done (step 1, the blend brush):** `wce_tile::blend_field::BlendField` (the map in memory: the file's bytes before and after the samples
kept, so an unpainted map is written byte for byte and a painted one differs only in the painted pixels; `paint_pixel` moves the brush's
weight of the other 15 channels to the painted one, at least one step, largest remainders get the rounding; undo records of the 64 x 64
blocks a stroke touched; `reduce_pixel` = BOB's blend / index reduction, shared with the material). `TileProject::blend` is read on first
use (the ground's material, a paint stroke) and written on save when painted (`blend_dirty`; Save for Assembly Kit and Save as write the
one in memory); the vegetation generator reads it too. `BrushKind::Paint(channel)` goes through the height brushes' stroke and undo
(`BrushStepCommand`, one entry per stroke, "Paint ground"). The live preview: the stroke patches the material's blend / index maps over
the changed rectangle (`patch_blend_maps`) and gives them a new `blend_id`, on which the renderer uploads them again. The UI: the Paint
button in the toolbar (Ctrl erases: paints the first group's first variation), its Inspector (radius, strength, falloff, texture group,
variation); commands `tile_mode paint`, `paint_ground`, `assert_blend`, `brush`'s fifth number; `scripts/tile_paint.txt` (paint, undo,
redo, erase, save, reopen).

**Done (the texture group picker):** the Tile panel's entry section has a combo per group (R, G, B, A) listing the game's groups (every
climate's `TEXTURE` names, `TileDocument::texture_group_names`); `TileEntry::set_texture_group` (an attribute edit, the rest of the entry
as it was); `TextureGroupCommand` (undoable; the ground's material made again); a save writes the groups into the entry next to the tile
when they differ there (`TileProject::save_texture_groups`). Command `texture_group <0-3> <name>`; in `scripts/tile_paint.txt`.

**Done (step 2, the ground type brush):** `wce_tile::pixel_layer::PixelLayer` (the block undo and changed rectangle, shared by both
painted maps; `BlendField` is built on it); `wce_tile::ground_types` (the 13 types' names and colours, `wce_tile_io`'s decompile uses
it); `GroundTypeField` (`ground_type_map.png` in memory: hard values under the brush, alpha 255 as on every painted pixel of the kit's
711 battle tiles; an unpainted map is written as read, a painted one through the kit's encoder; the file's name in any case, BOB's tiles
have `.PNG`). `TileProject::ground` / `load_ground` / `ground_dirty`, written by save, Save as and Save for Assembly Kit.
`BrushKind::Ground(type)` ("Paint ground types", one undo entry per stroke). The tool's Ground types mode (the palette button; its
Inspector: radius and the type, "none" or Ctrl erase) switches the view to TEd's ground types view while it is active, the look before
coming back after. The debug views read the maps in memory: the ground types view is baked again at every step of a stroke, the blend
groups view when a paint stroke ends. Commands `tile_mode ground`, `paint_ground_type`, `assert_ground_type`; in `scripts/tile_paint.txt`.

**Done (step 3, the falloff curve editor):** `wce_tile::falloff` (a curve through points, distance from the centre -> weight, monotone
cubic so that it never overshoots between them; five presets; sampled into a 65-entry `FalloffLut` that `Brush::weight` reads instead of
the falloff slider, so brushes stay `Copy`). The height and paint brushes use it (the ground types brush stays hard). The Brush Falloff
window (`falloff_editor.rs`; TEd's Brush Falloff Editor, wce's own curve): drag a point, double-click adds one, right-click takes one
away, the presets; opened from the brush's Inspector (a "falloff curve" switch and "Edit the curve..."); kept in `[tools.tile]
brush_curve` (written when a drag ends). Commands `brush_curve off | on | <preset> | <distance,weight>...`, `falloff_window 0|1`;
`scripts/tile_falloff.txt` (the hard and linear curves' heights at 0.8 of the radius).

**Done (step 4, the image brush):** `wce_tile::brush_image::BrushImage` (a PNG, grey / grey + alpha / RGB / RGBA, 8 or 16 bit, or a DDS
the decoder reads; the weight is the luminance times the alpha; bilinear over the square the brush's radius spans, turned by an angle).
`Brush::image` replaces the falloff and the curve (so `Brush` and `BrushSettings` are `Clone` now, the image shared by an `Arc`); the
height and paint brushes use it. The Inspector: "image brush", the file, Load image..., the turn or a random turn at every step; kept in
`[tools.tile] brush_image`. Command `brush_image off | <file> [degrees | random]`; `scripts/tile_image_brush.txt` (%IMAGE%: a picture
white on its left half: the stroke rises on one side only, the other after half a turn).

**Done (step 5, the spline paint stroke):** `TileDocument::stroke_along` (TEd's Spline Paint Stroke, on the tile's own lines: the active
brush, any of them, stroked along the selected spline's curve, polygon or line, round to the first point when the polygon is closed, e.g.
not an AI hint line; a dab every quarter radius at the most a frame gives; one undo entry). The brush's Inspector: "Stroke along the
selected line" (select the line first, then pick the brush). Commands `stroke_along`, `brush_paint <group> <variation>`, `brush_ground
<type>`; `scripts/tile_stroke_along.txt` (a closed no-go square painted and given a road round its edge, undone; a wall spline).
