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
