# tile.wce and terrain_layers.wcl (the editor's tile project)

A tile project in the projects folder (`<projects>/assembly_kit/raw_data/terrain/tiles/...`) is the editor's own: `tile.wce` for the scene
and `terrain_layers.wcl` for the terrain's layers, beside the maps TEd reads. Nothing here is read by TEd or the game: an export makes
their files ([editor.md](../editor.md), "Saving and exporting"). This page describes both files, and the combined project that holds all
of a tile's variations ([plans/tile_variations.md](../plans/tile_variations.md)).

## tile.wce

`tile.ted`'s XML ([tile_ted.md](tile_ted.md)) with what TEd does not know added, named so the conversion for TEd (`project_files::to_ted`)
finds it: an attribute `wce_...`, an element `Wce...`. The root `<project>` is marked `wce_version="1"`. Numbers keep full precision;
TEd's precision is applied by the conversion only. A placed prefab is an instance group with `wce_prefab="<key>"`, saved without its
contents (the editor fills them in from the prefab on open).
A project extracted from an exported tile (docs/formats/tile_workflow.md) has `wce_extracted_tile="true"` on its root.

### A combined project (several variations in one file)

A tile's variations (its entry's `<VARIATION>` rows, by their `location`: `minor`, `small`, `medium`) can be one project, `tile.wce` in
the tile's folder (`<location>/tile.wce`, not `<location>/<variation>/`). Its root lists them: `<project wce_variations="minor,small,medium">`.
A file without the attribute is a project of one variation; there is one format and one code path either way. `wce_version` stays `1`: an
older editor reads a combined file as one variation holding every variation's elements.

What each variation has (`wce_tile::variations`):

| Mark | On | Meaning |
|---|---|---|
| `wce_variations="small,medium"` | a `LayerInstance`, `UserInstanceGroup`, `INSTANCE_GROUP`, `LAYER_INSTANCE_GROUP` | it, and everything in it, belongs to those variations only. Without it: every variation its parent belongs to. It can only narrow the parent's set (Problems lists a name the parent excludes, or one the project does not have). Instances never carry it. |
| `<WceVariationFolder wce_variations="minor">` | inside any container | a folder of the editor's that holds what those variations have, at its place in a shared layer; its children sit directly in it (no `children` element). In a variation's file it is gone and its children are in its place, one indent less. |
| `wce_group_variations="minor"` | a `UserInstanceGroup`, `INSTANCE_GROUP` (never a layer) | the group is a group in those variations only; in the others its lines go and what its `children` holds takes its place, one level up (a settlement's level that grouped the same buildings otherwise). |
| `<WceVariationValues wce_variations="small" position="..."/>` | the line right before an element | that variation's own values of the element's attributes (most often `position`'s height: TEd puts a Landscape instance on the terrain, and the levels' terrains differ). |
| `<WceVariationStart wce_variations="small" flags="..."/>` | a container's first child | that variation's own values of the container's start tag (`flags`: whether a layer showed, was active; on the root any attribute). |

A variation's own file (`variations::to_variation`) keeps what belongs to it, unwraps the variation folders and the groups it is not a group
in, puts its values in place and drops every mark: byte for byte the file it was combined from. The elements are in an order where each
variation's, read in file order, are exactly its file. Every export of a combined project goes through `to_variation`, then `to_ted`.

What a combined project's folder holds:

```
<location>/
    tile.wce                 the scene, wce_variations on the root
    terrain_layers.wcl       the shared base and every layer, of every variation
    nogo.png ...             a file every variation has the same
    variations/<v>/...       what one variation has of its own (a painted map that differs, its river files, files the project does not model)
    <v>/tile.wce ...         a variation not in the combined project: a project of its own, as before
```

There are no `height_map_0.png`, `blend_map.tif` or `ground_type_map.png`: an export composes each variation's from the layers. BOB's
intermediates (`final_heights.dds`, `final_alpha.dds`, `tile_normal.tga`) are left out; every export makes them again.

The entry stays at `_tile_database/TILES/<name>.xml` with every row. A variation listed on the root of `<location>/tile.wce` is served by
the combined project; otherwise its own folder serves it, or the kit, or the packs (`wce_tile::sources`' order). A loader asking for a
combined project's variation (`sources::find_tile`) gets it written as a project of its own under `<projects>/build/variations/raw_data`
(`variations::materialize`), with a `wce_variation.stamp` naming what it was written from (the scene's, the layers', the entry's and the
files' names, sizes and times); a stale or missing stamp has it written again.

## terrain_layers.wcl

The terrain's layers (`wce_tile::terrain_layers`): per map type a base and an ordered stack of layers, composed into the map the editor
shows and an export writes. Little-endian.

```
"WCTL"                      magic
u32  version                4 (1 to 3 still read)
u64  size                   of the inflated body
...  body, deflated:
    u64  next_id            the last layer id given
    per type, height, blend, ground type:
        u8   present
        if present: a stack
```

A stack:

```
u32 width, u32 height, u32 layer count
u64 working layer id        0: the base
blocks                      the base
per layer:
    u64  id
    text name               (u32 length, UTF-8)
    text owner              empty: the user's; else made by a feature, made again when what it is made from changes:
                            "road:<id>[@<variation>]#<signature>", "wall:<key>[@<variation>]#<signature>",
                            "river:<id>[@<variation>]#<signature>" (a river's bed), "prefab:<n>#<signature>" (a prefab
                            reference's ground), "texture_rules" (the texture rules' blend layer)
    text depends            (version 3+) the scene layer it exports with (its wce_layer_id); empty: always
    text variations         (version 4+) the combined project's variations it belongs to, comma separated; empty: all
    u8   replace            (version 4+) 1: where it holds a block, it is what shows (a "Variation patch" layer)
    u8   visible
    f32  opacity
    blocks                  its samples
```

Blocks: `u32` count, then per block present its `u32` index (row-major over the map's 64 x 64 blocks) and its 4096 pixels. A pixel is the
type's samples: height `f32` metres (the base the ground, a layer what it adds times its opacity, a replacing layer what shows); blend 16 x
`u16` (TEd's channels); ground type 4 x `u8` (RGBA). A block a layer lacks is nothing there.

A stack keeps its layers in three bands by owner (`terrain_layers::order_of`): the texture rules' under the user's, the user's, then the
features' (roads, walls, rivers, prefabs) on top. Layers of a tool no longer in the editor (`erosion#`, shelved 2026-11-21) are dropped
when a tile is opened.

In a combined project a layer belongs to its own variations, narrowed by its `depends` layer's (a layer exporting with a scene layer some
variations lack is not in theirs). The merge keeps the reference variation's base; the others' become **patches that build on each other**,
replacing layers right above it ("Variation patch: small, medium"): each variation in turn, against what the patches so far give it, a
block it differs in goes into a layer of it and of every later variation whose base has that block the same (heights compared as each
stores them). A procedural road's, a kit wall's and a river bed's layers are made per variation, over that variation's ground (their owners carry
`@<variation>`); a prefab reference's ground layer has the reference's variations.
