# Workflow review: TEd's workflows in wce (started 2026-10-26)

A review, workflow by workflow, of what the Assembly Kit's tools (TEd, BOB) do and whether wce replicates it, supersedes it or needs changes.
Held as a conversation with the user, one workflow at a time; this file keeps what was found and decided.

State (2026-11-22): workflows 1 to 3 reviewed and built (2026-10-26 .. 27); no other workflow has been reviewed since. TEd's remaining
tools went through docs/plans/tile_ted_parity.md instead.

## 1. Tile creation

**Before:** wce had no way to make a tile. A tile came in only by opening one: a kit tile (its first save makes a project), an exported tile
extracted from the packs (`open_exported`, `_extracted`), or a loose `tile.ted`. `save_to <folder>` writes a copy elsewhere, without a new
entry or location. The entry was read-only (Phase 6b of tile_ted_parity.md: New tile, Save as, resize, tile properties, database management).

### What TEd does

TEd's New Tile and Save As (static RE, 2026-10-26): docs/formats/tile_ted.md, "TEd's New Tile and Save As".

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

### Done (2026-10-26)

- `wce_tile::new_tile` (the values and checks, TEd's colour, the entry's text in TEd's style, `tile.wce` with seeds from the clock, the
  maps, the blend map streamed row by row; the kit's `_settings.xml` copied next to the projects' entries when they have none);
  **File > New tile...** (`new_tile_dialog.rs`) and `new_tile <name> [x] [y] [height] [density]`; `scripts/tile_new.txt`.
- The height field keeps metres, so brushes are not stopped by the entry's range; a save fits the range (`HeightField::fit_range`, TEd's
  rule; a range within half a step of the old one is kept) and writes the entry's `min_height` / `scale` (`TileProject::save_height_range`).
- `add_building` on a tile without buildings uses TEd's defaults (`instance_templates`).
- Fixed on the way: opening an open tile shows it (`Document::opened_from`); a project's other variations open from the kit or the packs
  until their first save brings them in (`scripts/tile_variations.txt`, `tile_variations_extracted.txt`).

## 2. Extracted tiles without the `_extracted` workaround (user, 2026-10-26)

**Before:** a tile extracted from the packs was decompiled into the projects at `<tile>_extracted` (its folder and its entry's location),
so that it never took a kit tile's place in the old workspace; Save for Assembly Kit, the game export and the walls' keys took the mark
off. The renaming caused bugs of its own (names and locations that differ from the game's, entries renamed back and forth).

**Decisions:** no renaming; the extracted tile is a project at its own folder (the kit's tile of that name and it share the project:
they are the same tile, the kit's source and the game's compiled copy; To the Assembly Kit's version still takes the kit's copy); its
`tile.wce` root is marked `wce_extracted_tile="true"` (a `wce_` attribute: the conversion for TEd and the game drop it), which the
Tiles panel shows; extracting over a project asks **Open the project / Overwrite / Cancel**; the old `_extracted` projects are moved once,
by a command (`migrate_extracted`; its Tile menu entry went in the menus' review of 2026-11-14), not on open.

**Done (2026-10-26):** `project_files::{EXTRACTED_ATTR, mark_extracted_text, is_extracted_text, is_extracted_dir,
migrate_extracted}` (the old marks
kept private for the migration only); `workflow::open_exported` writes at the tile's own folder and entry (no `extracted` argument);
`save_exported`, Save for Assembly Kit and the walls' keys use the folder as it is; the tool's `open_exported` replaces a project only
when `fresh` (closing its document, refused with unsaved changes), keeps the other variations' entry numbers, marks the new `tile.wce`;
`extract_or_ask` and the prompt for the Tiles panel's packs and File > Open exported tile...; an extracted project's missing variation is
extracted (a variation of it is marked) instead of taken from the kit; File > Revert > To the Assembly Kit's version (formerly Import
from Assembly Kit) reads the mark.

## 3. Ground texture painting (done 2026-10-26)

**Before:** wce displayed the ground with the game's terrain shaders (`blend_map.tif`: 16 weights per pixel, channel 4 g + v =
texture group g, variation v, summing to 65535; the entry's four `TEXTURE_GROUP`s; the climate and season the user picks) and TEd's
debug views (colour, blend groups, ground types, protection). It edited none of it: `blend_map.tif` and `ground_type_map.png` pass
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

**Done (2026-10-26), in the order above** (each with its tooltips and a script):

1. The blend brush: `wce_tile::blend_field::BlendField` (an unpainted map written byte for byte, a painted one differing only in the
   painted pixels; `paint_pixel` keeps the sum at 65535; `reduce_pixel` = BOB's blend / index reduction), the live preview
   (`patch_blend_maps`, a new `blend_id`), one undo entry per stroke (`BrushStepCommand`). The texture group picker
   (`TileEntry::set_texture_group`, `TextureGroupCommand`, `TileProject::save_texture_groups`). `scripts/tile_paint.txt`.
2. The ground type brush: `wce_tile::pixel_layer::PixelLayer` (shared by both painted maps), `wce_tile::ground_types` (the 13 types),
   `GroundTypeField` (alpha 255 as on every painted kit pixel; the kit's encoder).
3. The falloff curve editor: `wce_tile::falloff` (monotone cubic, presets, `FalloffLut`), `falloff_editor.rs`, `[tools.tile] brush_curve`;
   `scripts/tile_falloff.txt`.
4. The image brush: `wce_tile::brush_image::BrushImage`, `[tools.tile] brush_image`; `scripts/tile_image_brush.txt`.
5. The spline paint stroke: `TileDocument::stroke_along` (any brush along the selected spline, polygon or line; a spline's point
   `width`s give its width, TEd's Spline splat); `scripts/tile_stroke_along.txt`.
6. The Blendmap Filters: a hard 0 / 1 mask on every paint operation (height range, slope range, "Use No-Go Range" = 30 to 90 degrees;
   docs/re/terrain.md, "TEd's blend painting and its Blendmap Filters"); `wce_tile::blend_field::PaintFilter`;
   `scripts/tile_paint_filter.txt`.

Then TEd's other Blend Map operations (traced 2026-10-26, docs/re/terrain.md; the user asked for them with what each does in its
tooltip, since nobody knew): **Auto-Flange** (TEd's, `flange_pixel`) and **Rock-painting** (TEd's slope curve, `rock_weight`, but
following the brush's strength and falloff and never lowering the texture, as agreed) as `PaintMode`s; `scripts/tile_paint_modes.txt`.
**Normal (Grouped)** and the blur followed on 2026-10-27 (tile_ted_parity.md, Phase 5a). The Influence Map module and the nogo map were
closed as not needed (user, 2026-10-27; tile_ted_parity.md, Phase 5a).
