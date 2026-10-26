# Vegetation: what's left

Status (2026-10-26). `wce_tile::vegetation` generates everything BOB's Vegetation processor writes for a battle tile (tree lists with
static trees converted per climate, the grass list, the props / decals / VFX of `<category>_procedural_bmd_data`, the list heights),
bit-exact against fresh BOB. Done: writing the files (`wce_tile_io::attila::stage_c`, byte for byte against fresh BOB on 13 tiles;
docs/re/vegetation.md, "The writers") and the PROPS key order (`compile::marker_table_order`, docs/re/ted_export.md "Marker tables").
RE, check commands, the BOB-under-Wine recipe and its rounding caveat: docs/re/vegetation.md.

## Open

1. **Converted static trees whose converted model doesn't exist.** BOB's list drops them; the editor keeps drawing the instance's own
   model (no existence check in `converted_vegetation_key` / `update_static_tree_models`,
   `crates/wce_tile_editor/src/tile_document/vegetation.rs`).
2. **Dark squares in farm_eastern/1x1/ab.** Small dark squares in the orchard rows with every list; from some static model, not looked at.
3. **Leaf density** (last): mesh trees look sparser than their billboards (docs/re/vegetation.md, "Open"). Needs matched game / editor
   screenshots.
