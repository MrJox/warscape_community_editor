# Performance pass (between Phase 6 and Phase 7)

Target set by the user: the Tile tool on `tdd_settlement_dunedain_cities/dunedain_city_osgiliath/minor` (a 45 MB `tile.ted`, 130 000 XML elements, 77 633 instances of which 26 984 buildings,
a 1280 x 1280 height map) at 60 stable fps or better, and the tile loading in under a second. Measured on this machine (RTX 4070 Ti SUPER, 16 logical CPUs, default adapter = the high
performance one).

| | before | after |
|---|---|---|
| frame time, whole tile in view | 579 ms (1.7 fps), p99 1.2 s | 5 to 7 ms (140 to 270 fps), p99 under 9 ms |
| close-up orbit / zoom in and out | not usable | 150 to 270 fps |
| dragging a building | a full rebuild per frame | 205 fps |
| brush stroke | 53 fps | 137 fps |
| duplicate / delete a building | 0.5 to 0.9 s | about 55 ms (one hitch) |
| `tile` command to first image (files cached) | 5.7 s of frame time plus 3 s of model loads | 0.37 to 0.45 s |
| every model and texture in place | 6 to 7 s | about 0.5 s later (full-resolution textures follow in the background) |
| first open with an empty extraction cache | 24 s | first image 0.9 s, buildings appear while the pack tool extracts them |
| memory held by one structural edit (undo record) | two copies of a 150 MB XmlDoc | a few hundred bytes of text splices |

The integrated AMD GPU (`WCE_GPU="AMD Radeon"`) is GPU bound at 20 to 30 fps in the same views (the four shadow cascades dominate: `gfx shadows off` gives 58 fps); the editor does not pick
it by default (`libs/render/gpu_adapter`: the high performance adapter unless `WCE_GPU` names another).

## What was slow (profile of the first measurement, `docs/performance.md` numbers come from `perf` and `profile`)

* Every mesh of every entity was one `Map` + one draw call, in the camera pass, in four shadow cascades and in the six faces of the environment cube: well over 100 000 draw calls per frame,
  no culling, always LOD 0. CPU 579 ms, GPU 593 ms.
* `std::istreambuf_iterator` read every file byte by byte: a 4 MB texture took 3 ms, a 45 MB tile 240 ms.
* Models and textures loaded one after the other on the render thread (3 s), `piece_bounds` parsed every building model once more (0.3 s).
* `rebuild_items` walked and re-parsed the ancestors of every instance (0.5 s, twice), `rebuild_batch` looped all markers once per type.
* The Outliner copied and drew the whole element tree every frame (14 ms); a moved building rebuilt the draw list of the renderer (90 ms); a brush step rebuilt it too.
* A structural edit re-parsed the 45 MB file twice and copied the whole document for the undo stack.

## What changed

**Foundation.** `foundation/thread_pool` (`compute_pool().parallel_for`, a prioritised `io_pool().post` with `set_priority` / `cancel`), `foundation/profile` (`WCE_PROF("name")` scope timers),
`read_file` / `read_file_range` with `ReadFile`.

**Renderer** (`libs/render`):
* `DrawSet` (`draw_set.h`): every mesh entity is one item in parallel arrays (bounding sphere, 3x4 matrix, model, flags: structure of arrays); every mesh at every LOD is a batch. A view (camera,
  shadow cascade, environment face) culls the items in parallel (frustum, screen size, LOD by distance), sorts the survivors by batch with a deterministic counting sort, and the renderer writes
  one instance buffer per view and issues one `DrawIndexedInstanced` per visible batch (about 4 700 draw calls for all views of the Osgiliath frame). The instance buffer is a ring with one
  `Map` per view.
* LODs: the model cache loads every LOD of an RMV2 (the files always have them: 1 to 5, switch distances 100 to 1000 m) and the draw set picks one per instance. Only entities with
  `MeshInstance::lod` / `Terrain::lod` use LODs and the size culling (the weather sample map keeps LOD 0 so that its pixels stay what they were). Shadow casters use coarser LODs in the
  farther cascades and skip casters smaller than `shadow_min_texels` shadow texels.
* Terrain: patches of 64 x 64 cells (full resolution: 1280 x 1280 vertices for Osgiliath, the old tool downsampled it to 427 x 427), each with up to five index buffers on one vertex buffer
  (stride 1, 2, 4, 8, 16), skirts that close the cracks between patches of different detail, frustum culled per patch. A new height payload rebuilds only the patches whose heights (plus the margin
  the normals read) changed; terrain has its own draw set, so a brush stroke does not touch the 36 000 building items.
* Picking draws the visible items of both sets with an id stream (`TEXCOORD0` per instance) instead of one draw per entity.
* The scene overlays (markers, polygons) are transformed to world space once and reused until the scene changes; the grid and selection outline are added per frame.
* `RenderScene`: dense id index (ids are sequential), `reserve`, a log of entities whose matrix changed (`moved_since`) so a dragged building patches one item, and revisions that tell structure
  changes from terrain and overlay payload updates.
* `ModelCache` / `TextureCache` are thread safe and asynchronous: `try_get` queues a load on the io pool, nearest model first, and returns nothing until it is ready (the renderer keeps
  asking, `revision()` says when something finished); `model()` blocks (fixed-step runs request everything in parallel and wait once, so frame N is the same every run).
* Texture mip streaming: a large DDS first loads only its small mips (it reads the tail of the file: a 2048 x 2048 DXT5 texture becomes 256 x 256 and 1/64 of the bytes), the full texture
  loads in the background after the models and replaces the view at the start of a frame (`TextureCache::pump`). Off in fixed-step runs.
* Environment cube, shadows, G-buffer share the same culling code; GPU time per pass is measured with timestamp queries (`GpuTimer`).

**Tile tool** (`tools/tile`):
* `TileDocument::Index`: one parallel pass over the XML elements (type, position, orientation, scale, building key, polygon flag) and one sequential pass for the parent chains replace the per
  instance walk; `is_instance`, `world_position`, `world_matrix` read it. Kept in sync by attribute edits (`index_edited`) and rebuilt after structural ones.
* Markers (everything without a model) are grouped by type in structure-of-arrays form; draw-distance culling is a loop over three float arrays. Pick: a uniform grid over the ground plane and a
  ray walk through its cells instead of testing 77 000 spheres.
* Opening: the scene parses while the tile database and the height map load; models are not read at open any more (the draw set requests them, the document learns from the model cache which
  ones failed and turns those buildings into boxes, `poll_models`); a background job extracts the building pieces and their textures from the packs in batches on the first open.
* Structural edits (duplicate, delete, add, move to layer, polygon points): `XmlDoc::splice` patches the element table in place (no re-parse) and records the text change; the undo entry is that list
  of splices (`TedSpliceCommand`). `bake_pending` turns pending attribute edits into text first, so a record always refers to the text the undo stack leaves behind (last in, first out).
* Outliner: cached per document (`outline_stamp`), clipped lists, nodes below the second level start closed.

**Vfs**: extraction requests from many threads combine into one pack tool call (`Vfs::extract`), 130 single-file calls became about 10.

## Tools for the next person

* Menu bar (right): frames per second and milliseconds of the last 60 frames; not in fixed-step runs (their shots must not depend on the machine).
* `perf [reset|full]`: frame time (mean, p95, p99, worst), draw set statistics, GPU time per pass and, with `full`, the CPU scope timers. `profile [reset]`: the scope timers alone. `vsync off`,
  `gfx <feature> on|off` (shadows, ssao, fog, clouds, ...), `gfx lod_scale|min_pixels|shadow_min_texels <x>`, `wait_loaded [s]` (script: until models and textures are in, prints how long it took).
* `wce.exe --live --script file.txt` runs a script with the live clock, background loading and the real mouse (the default for scripts is the fixed-step clock). Scenario scripts used for the
  numbers above are in `scripts/perf/` (`nav.txt` orbit and zoom, `edit.txt` drag, brush, duplicate, delete, undo, `load.txt` opening).
* Gates: `perf_check` (thread pool, frustum and LOD culling, DDS stream plan, element index against the transform walk on three tiles, pick grid against brute force), `xml_splice_check`
  (in-place structural edits against the re-parsing ones, undo and redo exact). Both in `scripts/gates.sh`.

## Differences the gates show (explained)

* The two tile captures (`capture_tile_*`) were re-baselined: terrain at full resolution instead of every third sample, skirts close the pinhole cracks between patches (black pixels in the old
  captures), LODs on building models, buildings drawn in batches. They were compared by eye against the old ones (`build/gates_base_p6`).
* The weather window shots and wce captures accept one level in under 0.05 % of the pixels (4 pixels in the dry shot): meshes of one kind are now drawn together, so surfaces at exactly the same
  depth can resolve the other way. The sample map's pixels are otherwise identical.
* `render_check`: "houses in view 8, picked right 3" became "picked right 6" (same tie-breaking reason in the id pass); the check is a lower bound and stays satisfied.

## Not done / ideas

* Occlusion culling. Frustum, size and LOD culling were enough (4 to 7 ms); the hardware path would be a Hi-Z from the previous frame's depth.
* The XML parse (120 ms for 45 MB) is the largest piece of opening that is left; it could be split at layer boundaries and parsed in parallel.
* The shadow cascades cost 3.5 ms on the discrete GPU and 25 ms on the integrated one: casters are drawn into every cascade they touch.
* `XmlElement` is a heavy structure (strings and vectors per element): a flat attribute array and interned names would halve the memory of an open tile.
