# Phase 0 baseline (2026-10-09)

What every later phase is compared against. Produced by `scripts/gates.sh build/gates_base --corpus --render` on the Phase 0 tree
(both legacy projects built unchanged, Release). The raw outputs are in `docs/baseline/`; the screenshots stay in
`build/gates_base/` (local, not committed). Compare a new run with:

```
scripts/gates.sh build/gates_new [--corpus] [--render]
diff -r docs/baseline build/gates_new -x '*.png' -x 'shot_*'      # the summary lines must be identical
python scripts/imgcmp.py build/gates_base build/gates_new          # screenshots, rules below
```

Run time on the dev machine: fast gates about 60 s (`xml_roundtrip` 47-86 s of it), `roundtrip` 753 s, `scene_check` 163 s,
screenshots 10 s. Inputs: the game folder, the VFS cache `%LOCALAPPDATA%\awe\x`, the engine constant-buffer dump for `env_vs_dump`
(`WCE_CBS`), and the corpus manifest `manifest/corpus.tsv` (37,041 files of the two assembly kits, git-ignored).

## Gate results

| Gate | Exit | Result (the lines a later run must reproduce) |
|---|---|---|
| `xml_roundtrip` | 0 | 621 files (549 `.environment`, 32 `.lighting`, 40 `.xml`), 0 failed |
| `format_check` | 0 | env_value formats: every value class reproduced (float 67,521, int 229,672, bool 44,038, rgb 11,823, argb 581, variance 576, euler 65), 0 differ |
| `variation_check` | 0 | `warscape_asset_variation_db` of `tdd_pack6_weather` and `data.pack`: writer identical, XML and bin equal; editing gates 0 failures; 239 variations, all found by their own conditions |
| `rmv2_check` | 0 | 0 files, 0 failed (no RMV2 sample in the default run) |
| `vfx_check_rain` | 0 | `rain heavy`: 4 emitters |
| `env_vs_dump_all` | 0 | ranking of the `.environment` files by matching constants: best `med_sp_land_day_dry` (40 variables), then `med_sp_land_day_dry_2` 34, `cont_sp_land_day_dry` 32 (16 lines in all) |
| `unit_tests` | 0 | all passed (euler round trip worst error 1.51e-13) |
| `height_check` | 0 | monte_rosa `height_map_0.png`: 1216x800 |
| `weather_check` | 0 | 263 environment files; the three queries of the gate |
| `tdb_build` | 0 | built 355,209 bytes from 699 tiles; the shipped file has 356,758 bytes, 190,816 bytes differ, first at 10,355 (the gate exits 0 in either case; the line must not change) |
| `roundtrip` (corpus) | 1 | the expected failures only, below |
| `scene_check` (corpus) | 0 | 1,583 `tile.ted`, 0 parse failures, 0 parse/save differences; 34,847,248 of 34,847,248 numbers reproduced |

### `roundtrip`: expected failures

`roundtrip` exits 1 because of failures that are known and not parser bugs. A later run must give the same counts per format
(`docs/baseline/roundtrip.txt`):

- **xml**: 10,061 files, 15 failed. These are malformed CA XML (`end tag </tree> without start`, an unclosed `<GroupFormationTable>`, an
  odd-length UTF-16 file); no parser repairs them and none drops data.
- **png**: 8,554 files, 2,432 failed. Photoshop output with ancillary chunks (`wm/*.png`, the alpha textures): the kit encoder
  cannot reproduce them, and `PngFile::write()` passes their bytes through unless the pixels were edited.
- **tiff**: 2,248 files, 1 failed: `plains/3x1/3x1_aa/blend_map.tif` (TEd-layout header differs from the generated one).
- Everything else passes: `bmd_bin` 860, `chmf` 171, `dds` 5,875, `markers` 449, `tga` 1,756, `tree_list_bin` 562,
  `tile_database_bin` 4, `rmv2_magic` 198, the civilian files, the opaque formats.
- **Skipped for size**: the gate skips files over 64 MB by default (`--max-size`): 5 dds, 3 opaque_authoring, 31 tiff, 2 xml. The older
  `tile_editor/scratch/full_gate.txt` ran without that cap (so its `ok` counts are higher by exactly those files) and predates the
  `tile_database_bin` format (it listed four `.bin` files as unclaimed). The failure counts are the same.

### Known bug found by the baseline

`unit_tests` crashed on a `tile.ted` without `SCENE_GRAPH` (`custom_monte_rosa/aa`): `TedScene::add_layer(-1)` indexed element -1.
The gate used `khazad_wall_test` meanwhile. Fixed in Phase 1 (layer operations throw `std::runtime_error`, Studio shows a status
line) and covered by a second `unit_tests` argument.

## Screenshots

`shot_weather_{dry,rain,snow}` (`weather_editor --open ... --cam 0.6 0.35 120 --frames 20 --shot`) and `shot_tile`
(`tile_studio --tile custom_monte_rosa/aa --cam 0.6 0.6 400 --frames 10 --shot`), all 1584x921.

Run to run on the same binaries (two runs, the baseline and an earlier fast run):

| Shot | Between two runs |
|---|---|
| `shot_tile` | 56 dB PSNR, mean 0.15, max difference 1 of 255: almost identical but not bit-identical (the earlier note said bit-identical; one pixel level of noise exists) |
| `shot_weather_dry` | 39 dB: wind and tone-map adaptation run on wall-clock time |
| `shot_weather_rain` | 43 dB; its `.distortion.png` 24 dB |
| `shot_weather_snow` | 27 dB |

Rules until Phase 3:

- `imgcmp.py` tolerance is 35 dB. Rain, snow and the distortion map are informational (particles are seeded per run).
- **Phase 3 adds the fixed-step `Clock` first.** After that the weather shots must be bit-identical between runs. Re-baseline them once at that
  point, explicitly, and say so in the commit.
- Screenshot files are written by `formats/png_file` since Phase 1 (the WIC encoder was dropped). The pixels are what `imgcmp.py` compares, so
  the file bytes are expected to differ from this baseline; `imgcmp.py` reports "identical pixels" in that case.

## Added in Phase 2

New gates, with their reference output in `docs/baseline/`:

| Gate | Exit | Result |
|---|---|---|
| `foundation_check` | 0 | all passed (JSON reader and writer, settings precedence) |
| `vfs_compare` | 0 | 27 packs (6 mod packs, 0 missing mods), 212,243 files, same pack order and winners through rpfm_server and rpfm_cli; 135 sample files (302.8 MB) in 21 packs and 20 shadowed paths byte-identical |

Timing, weather tool startup set (`scripts/vfs_timing.sh`, the dry screenshot command; empty cache = cold):

| Backend | Cold | Warm |
|---|---|---|
| rpfm_server | 6.5 s | 2.8 s |
| rpfm_cli | 10.4 s | 2.7 s |

Cold mount of all 27 pack indexes alone (`vfs_probe --compare --time`): rpfm_server 1.3 s, rpfm_cli 2.3 s.

## Phase 3: fixed-step clock and the weather screenshots re-baselined

`Clock` (`libs/foundation/clock.h`) is the renderer's only time source. `--shot` implies a fixed step of 1/60 s per rendered frame (`--fixed-step <s>` sets it
otherwise), the shot is taken on the 20th rendered frame, and GPU readbacks (ambient cube, depth range, sun occlusion) wait for their data instead of using it
whenever it happens to be ready. Two runs of the dry, rain and snow shots are now bit-identical (PNG, diagnostics text and the distortion map).

**The weather screenshots were re-baselined at this point** (the Phase 0 ones followed wall-clock time and are not reproducible): `build/gates_base_p3/` holds
the new reference (`build/gates_base/` keeps the Phase 0 originals). The new dry shot is 43 dB from the old one, rain 45 dB, snow 27 dB (rain and snow particles
used to depend on the frame times). From here on a weather shot must be **bit-identical** to `build/gates_base_p3/`; any difference must be explained and
re-baselined explicitly in its commit. The tile shot still comes from Tile Studio (own renderer until Phase 6) and keeps the Phase 0 tolerance rule.

### Phase 3 additions

| Gate | Result |
|---|---|
| `render_check` (label `render`, runs in `gates.sh --render`) | headless D3D11: two renderers give the same pixels; sample map picking (sky 0, ground = terrain entity, 3 of 8 house centres hit their own house; the rest hit other pieces, empty air or ground); locked and hidden layers; hill terrain in 64-cell patches; overlays in their own colour, depth tested, selection outline, grid; PNG capture equals the target. Output: `houses in view 8, picked right 3` and `all passed` |
| `scripts/shots.sh` | the three weather shots and two offscreen captures (`--capture 1280x720`, `800x600`) byte-identical to `build/gates_base_p3/` |

## Phase 4 additions

| Gate | Result |
|---|---|
| `editor_check` (fast) | all passed (command stack, documents, selection, registry, scripts, cameras, inspector gestures) |
| `app_check` (render) | `wce.exe` runs `scripts/app_smoke.txt` twice (exit 0 both) and `scripts/app_firstrun.txt` once; `capture identical between runs`, `shot identical between runs`, `first run: ok (wce.ini written)` |
| `scripts/shots.sh` | the three weather shots byte-identical to `build/gates_base_p3/`; two `wce.exe --capture` references (`capture_dry_1280x720.png`, `capture_dry_800x600.png`) |

**Capture references re-baselined.** The Phase 3 captures (`weather_editor --capture`) were taken with the default environment, because the legacy app only picks the active environment while it draws its UI. They are replaced by captures of
`wce.exe`, where the weather tool lights the sample map with `med_sp_land_day_dry.environment` (sky, clouds and shadows render as in the window shots). Weather window shots are unchanged.

## Phase 5 additions

| Gate | Result |
|---|---|
| fast gates (`xml_roundtrip`, `format_check`, `variation_check`, `rmv2_check`, `vfx_check_rain`, `env_vs_dump_all`, `unit_tests`, `height_check`, `weather_check`, `tdb_build`, `vfs_compare`, `editor_check`, `foundation_check`) | output identical to Phase 4 (`build/gates_p4`) |
| `roundtrip`, `scene_check` | same as Phase 4 (`roundtrip` keeps its documented expected failures; `scene_check` passes) |
| `weather_tool_check` (new, render; `scripts/weather_check.sh`) | the weather tool of `wce.exe`, scripted twice: attribute edit, undo/redo, structural edits, save (exactly one line differs from the pack file; both runs save the same bytes), cross-fade, view modes, the picker, a database edit saved as XML and FASTBIN (convert into each other byte for byte). Replaces `variation_e2e.ps1`, which only knew the legacy build folder |
| `app_check`, `render_check` | pass; `capture` identical between runs, `shot` identical between runs |
| `scripts/shots.sh` | the three legacy window shots **byte-identical** to `build/gates_base_p3/`; four `wce.exe --open ... --capture` references in `build/gates_base_p5/` (dry, rain, snow at 1280x720, dry at 800x600), identical on every run |

**The reference GPU.** This machine has two GPUs (NVIDIA RTX 4070 Ti SUPER, AMD Radeon(TM) Graphics) and the baseline pixels (Phase 3 on) were rendered on the **AMD** one, the adapter Windows used by default at that time. Windows later
switched the default adapter between runs, which made every shot differ (a rebuilt Phase 4 commit differed the same way: not a code change) and made runs differ from each other by GPU float noise. Since then `libs/render/gpu_adapter` picks the adapter
(`WCE_GPU=<part of the name>`, else the high-performance GPU; the renderer logs `GPU: <name>`), and `gates.sh`, `shots.sh`, `app_check.sh` and `weather_check.sh` default `WCE_GPU` to `AMD Radeon`. Pixels are only comparable on one adapter.

**Capture references re-baselined (second time).** An offscreen capture now restarts the renderer's temporal state (auto exposure snaps on its first frame, readback ring, wind clock) so it is a function of the scene only; before, it inherited
the live frames' adaptation (about 1e-6 of GPU noise made two scripted runs differ by a few one-level pixels). The `--open <env> --capture` references of `build/gates_base_p5/` replace the Phase 4 `capture_dry_*` ones (which had no
environment document open). The whole-window `shot` of `app_check` is compared with a tolerance of two levels: it shows the live viewport. Scripted runs also ignore the real mouse now (a cursor over a splitter changed the shot).

## Performance pass (between Phase 6 and Phase 7)

Numbers and what changed: `docs/performance.md`. Gate results against Phase 6 (`build/gates_p6`):

| Gate | Result |
|---|---|
| fast gates, `vfs_compare`, `roundtrip`, `scene_check`, `editor_check`, `app_check`, `weather_tool_check`, `tile_tool_check`, `tile_open_check` | pass; the text outputs are identical except `render_check` |
| `render_check` | `houses in view 8, picked right 3` is now `picked right 6`: the id pass draws meshes in batches, so equal-depth surfaces tie the other way; the check is a lower bound (at least a third), still satisfied. A locked layer is checked at pick time again |
| `perf_check` (new, fast), `xml_splice_check` (new, fast) | pass |
| legacy window shots, `wce` weather captures | within one level in 4, 1 and 1 pixels (dry, rain, snow); the dry captures as in Phase 6 (7 and 94 pixels). Cause: batches draw a mesh's instances together, so surfaces at exactly the same depth can resolve the other way. `scripts/shots.sh` now compares the legacy shots with the same tolerance as the captures |
| two tile captures | **re-baselined** (`build/gates_base_perf/`): terrain at full resolution (was every third sample), skirts close the pinhole cracks between patches (the black pixels of the old captures), LODs on building models, batch draw order. Checked by eye against `build/gates_base_p6/` |
| legacy `shot_tile` | identical |

New behaviours checked by the existing scripts: structural edits undo and redo through text splices (`tile_roundtrip`: every edit undone and saved gives the original bytes), models and textures load in the background in live runs and synchronously in fixed-step runs (the two capture runs stay identical).

## Phase 8: legacy apps retired

The window shots of `weather_editor.exe` and `tile_studio.exe` (the "Screenshots" section above) are no longer produced: the apps are gone. The render regression is `scripts/shots.sh` (`wce.exe --capture`, same environments, cameras and frame counts; tolerance as in that script), and `app_check` covers the window path. `vfx_check` and `env_vs_dump` build from `cli/`; their gate output is unchanged.

## Phase 6 additions

| Gate | Result |
|---|---|
| fast gates, `vfs_compare`, `roundtrip`, `scene_check`, `render_check`, `editor_check` | output identical to Phase 5 (`build/gates_p5`); `roundtrip` keeps its documented expected failures |
| `tile_tool_check` (new, render; `scripts/tile_check.sh`) | the tile tool of `wce.exe` on `khazad_wall_test`: `tile_smoke` twice (select, move, rotate, duplicate, delete, polygons and their points, layers, brush strokes; each undone and redone; then saved) with identical saved files and capture; `tile_mouse` (click select, G grab with confirm and cancel, brush drag = one undo entry, polygon drawing with the mouse and Enter, point handle drag = one entry); the saved tile.ted differs from the original in exactly the moved building's line; the saved height map differs only inside the brush (437 samples around pixel 568, 598 = (440, 470) m: the height map placement is confirmed); `tile_roundtrip` (every edit, undo all, save: both files equal the originals byte for byte, and the redone state opens again); `tile_save` (save in place keeps a `.bak` of the original; undo and save again puts the original bytes back) |
| `tile_open_check` (new, corpus; `scripts/tile_open_check.sh`) | one tile of every tile set of both kits (128 sets: 83 battle, 45 campaign, 1.33 million instances) opens in `wce.exe` and renders a frame without an error, about 8 minutes |
| `app_check`, `weather_tool_check` | pass (the scripted mouse replaced the "ignore the real mouse" flag: `shot identical between runs`) |
| `scripts/shots.sh` | the three legacy window shots byte-identical to `build/gates_base_p3/`; the weather captures of `wce.exe` as in Phase 5; two new tile captures (monte_rosa terrain with polygons, the Rome settlement with its real models) in `build/gates_base_p6/`, identical on every run |
| legacy `shot_tile` (Tile Studio's own renderer) | identical pixels to the Phase 0 / 3 baseline |

**Wce captures are compared with a tolerance.** The dry weather captures of Phase 5 now differ from a fresh run in 7 (1280x720) and 94 (800x600) pixels by one level, identically in three runs; rain and snow are byte-identical. Nothing in Phase 6 touches the renderer, so this is GPU float noise
of the kind Phase 5 documented for the legacy dry shot. `scripts/shots.sh` accepts a wce capture that is byte-identical or within one level in under 0.05 % of the pixels; the legacy window shots stay byte-exact.

**Fixes found by the new tests:** `CommandStack::last_dirty` only described the last command, so a multi-step undo (`undo 3`, a jump in the History panel) re-published only what the last step touched and left structural changes unrendered; documents now re-publish from `take_dirty()`, which accumulates (checked in `editor_check`). A cancelled grab leaves no redo entry (`CommandStack::drop_redo`).

## Phase 0 / 0b of the TEd parity plan (`docs/plans/tile_ted_parity.md`, 2026-10-09)

Built in the private tree `build/work2` (`WCE_BIN=build/work2/bin/RelWithDebInfo`). `scripts/gates.sh build/<out> --corpus --render` against the Phase 8 baseline: **every earlier gate gives the same output** (`diff -r docs/baseline build/<out>` shows only the new gates, and `render_check`'s `picked right 6` which the performance pass already documented). `tdb_build`: 0 bytes differ from BOB's output, unchanged.

| Gate | Group | Result |
|---|---|---|
| `tile_entry_check` (new) | fast | 1,852 entries (700 battle kit, 1,152 campaign kit) round-trip byte for byte, 0 unknown attributes or children, 2,716 variations, 834 entries with links, 827 with link targets; the index built plain, with a cold cache and with the warm cache gives the same rows (the warm one parses nothing), all 2,716 variation folders resolve (and `tile.ted`, and the tile folder), one location claimed twice (a stray campaign entry in the campaign kit's `battle` folder); three kit tiles (`khazad_wall_test`, `custom_monte_rosa`, and `plains/3x1/3x1_aa` whose `height_map_0.png` is damaged in the kit and fails the same way each time) open identically by folder, by `tile.ted` and by entry; a copy of a tile outside the kit opens with a warning and no entry |
| `tile_entry_check --all` (new) | corpus | all 1,516 kit tiles that have a `tile.ted` and an entry open as the same document by folder, `tile.ted` and entry (about 80 s); 1 cannot be opened (the damaged PNG above) |
| `tile_entry_script` (new) | render | `scripts/tile_entry.txt`: open by name, entry file, variation folder, `tile.ted`, tile folder; `assert_entry` on `osgiliath_test` and `dunedain_city_osgiliath` (minor / small); a missing variation lists the real ones |
| `placement_check` (new) | corpus | the tool's building transforms against TEd's `building_list.xml` over the kit (73 tiles, 357,666 buildings): x / z 0.0001 m, rotation 0.00 degrees, y exact except 15 crates; landscape buildings sit on `min_height + scale * raw / 65535` with a median of 0.000 m. Output in `docs/baseline/placement_check.txt` (it reports; the exit code is always 0) |

New helper commands are test infrastructure: `gizmo_drag`, `cam_info`, `show_panel <tool panel title>`, `tile` / `tiles` / `tile_entry` / `assert_entry` / `assert_tiles`. No screenshot baseline changed (the Tile panel and the left dock gained a tab; the offscreen captures of `scripts/shots.sh` do not show panels).

## Phases 1 to 3 of the TEd parity plan (2026-10-10)

`scripts/gates.sh build/<out> --render` on `build/work2`: **all 23 gates exit 0** after Phase 3 (the Phase 1 and 2 runs: 22 each). New or changed gates:

| Gate | Group | Result |
|---|---|---|
| `tile_tool_check` (`tile_check.sh`) | render | adds `tile_gizmo` (drags on each axis, one undo entry per drag, no camera movement; the y handle does nothing on a landscape building), `tile_layers` (flags, hide / lock, every structure command with undo / redo; undo all + save = the original 55 MB `tile.ted`), `tile_scene_ui` (the Scene panel with the scripted mouse), `tile_placement` (landscape / freeform rules, strokes that carry landscape instances; undo all + save = the original `tile.ted` and height map), `tile_terrain` (blended ground, climate, flat, debug views; every capture identical between two runs) |
| `terrain_check` (new) | fast | the DXT decoder on hand-made blocks, the kit's 8 climates, the texture path fallbacks, and a bake of `tdd_custom_lothlorien/1x1/aa` against the real packs (12 layers, 1536 x 1536 texels, byte-identical twice) |
| `perf_check` | fast | the brute-force pick test skips hidden and locked instances (the pick grid does not hold them) |

`scripts/shots.sh` (reference `build/gates_base_perf`): the **two tile captures changed on purpose** (the ground is the blended colour now instead of one tiled texture; only the terrain differs) and were re-baselined; the weather captures `rain` and `snow` match, `dry` (1280x720 and 800x600) differs by at most 2 levels in 4 % of the pixels on this machine today (not investigated: the weather path does not use the terrain skin or the placement code, and the same difference shows with no tile open; the reference may be stale or the driver may have changed).
