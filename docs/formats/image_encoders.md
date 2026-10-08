# Image encoders: what reproduces the kit's bytes

Goal: when the editor changes pixels, the saved file must be what TEd/BOB would have written. Findings (2026-10-08), scripts in `tools/proto/`.

## PNG (`height_map_0.png`, `ground_type_map.png`, `nogo.png`, `lf_influence_map.png`)

- Chunks: `IHDR`, `IDAT`, `IEND` only. No ancillary chunks (Photoshop-saved files add `pHYs/iCCP/cHRM`: those are pass-through, not reproducible).
- Colour: `height_map_0.png` 16-bit grey (type 0); `nogo.png`, `lf_influence_map.png` 16-bit grey; `ground_type_map.png` 8-bit RGBA (type 6).
- **zlib**: inflating the IDAT stream and recompressing it with zlib level 6, memLevel 8, default strategy, then splitting into IDAT chunks of the original size, reproduces the file byte-for-byte for 240 of 240 sampled files (60 per type; the one miss is the Photoshop-saved variant).
- **Row filters**: libpng's default heuristic (try None, Sub, Up, Average, Paeth; keep the filter with the smallest sum of absolute signed byte values; ties keep the earlier one) selects the original filter on 100% of rows in 9 of 9 sampled files (`png_filters.py`).
- So a PNG writer = unfiltered pixels, heuristic filter per row, zlib 6/8/default, IDAT chunk size as in the original (one chunk for small files, 8192-byte chunks for the large ones: take it from the file being replaced, default 8192 for new files; to confirm on a freshly written large file).
- Reading can use Windows WIC (as weather_editor does); writing needs the own encoder above because WIC picks other settings.

## TIFF (`blend_map.tif`, TEd/BOB-written `lf_heights.tif`)

Little-endian, header offset 8, 15 IFD entries, uncompressed, one strip at offset 242, no software/XMP tags; `blend_map.tif` is 16 samples per pixel x 16 bit. Files from Photoshop (`lf_sea_colour.tif`, map-level `lf_heights.tif` with XMP/EXIF) have other layouts: pass-through. A writer is a fixed header template + raw samples; to verify against the 1,690 `blend_map.tif` files (parse header, regenerate, compare).

## DDS

TEd/BOB write R32F / L8 / L16 textures with `dwFlags = 0` and no caps (`protection_map.dds`, `final_*.dds`, `ground_types.dds`, `lf_height_map.dds`): a fixed 128-byte header template per format plus raw data. `blend/index/normal.dds` are DXT5 with 11 mips (`flags 0xA1007`, caps `0x401008`); reproducing them means reproducing the compressor (kit ships `nvtt.dll` and `cudart32_30_14.dll`): the BOB oracle showed `blend.dds`, `index.dds`, `normal.dds` bit-identical across runs and against the kit's copies, so the compressor is deterministic on the CPU path. Plan: call the kit's `nvtt.dll` with BOB's settings, and only replace it when an own encoder matches on all 82 shipped files.

## TGA

`tile_normal.tga`: 18-byte header, type 2, 32 bpp, descriptor 0x08, raw BGRA rows: trivial.
