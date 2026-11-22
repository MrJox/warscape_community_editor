# Exact land LF region PNG interchange

Contributed by Jordanakoos, merged 2026-11-21 (#75). The Terrain tool's plan: [terrain_editor.md](terrain_editor.md).

The Terrain panel's **Land LF region PNG** section and the following commands transfer a bounded rectangular region of the document's land LF:

```text
terrain_lf_region_export <x> <y> <width> <height> <new.png>
terrain_lf_region_import <x> <y> <width> <height> <source.png>
```

Coordinates and dimensions are unsigned file sample counts. `(0, 0)` is the top-left sample, x increases right and y increases down. The end is exclusive. Both dimensions must be positive, the entire rectangle must be inside the field, and its area must not exceed 1,048,576 samples. This is the land LF only, independent of the LF brush's sea selection.

Each PNG pixel is the exact unsigned 16-bit LF value. Files must be non-interlaced 16-bit grayscale without alpha or transparency; dimensions must exactly match the requested region. There is no brightness conversion, normalization, gamma/profile transformation, row inversion or resampling. PNG sample bytes are big-endian as required by PNG. The bounded decoder checks dimensions, framing, chunk CRCs and critical chunk support before the general PNG decoder allocates pixel storage. Input files including metadata are limited to 16 MiB.

PNG export reads the document's current full LF samples without editing its model, history, source files or native height resources. Maps opened from the packs export their shared placed-map LF directly without creating an editable model or copying the entire field. Export creates a new output file and refuses existing paths, including aliases to existing map resources. Names without an extension receive `.png`. After a successful transfer, the chooser remembers the parent folder so it does not suggest the existing file again; failures and cancellation leave that location unchanged. Native/built battle and campaign documents support read-only export. **PNG export does not establish that native campaign-height Save/Export is safe.**

Replacement is available only for raw/kit documents. Native/built import refuses before input-file access, pixel mutation or history access. All region, image and mirror checks precede mutation. A successful change creates one **Import LF region PNG** history entry using the existing LF stroke undo implementation, with before/after copies of only changed 64 x 64 pages. Identical replacement records nothing and retains any redo tail. Undo and Redo each mirror the region once; the existing `follow_lf_edits` path updates picking and affected view chunks. Transfers wait for active brushes, cell strokes, placement/Terry operations, gizmo drags and background jobs to finish. There is no per-pixel scene reconstruction or automatic fill.

Persistence uses the existing raw writer. TIFF headers, trailing bytes, samples outside the rectangle, sea LF and other resources retain their existing preservation rules. Database metadata and project override ordering are unchanged. No native writer, normal-authority API, campaign-height preparation API, dependency manifest or lockfile changes are needed.

Focused coverage is in `wce_formats/tests/lf_region_png.rs`, the `wce_terrain::lf_edit` controls, `wce_terrain_editor/tests/lf_region.rs`, and the Terrain tool's `lf_region_tests`. The real-witness test is opt-in and maps only the land LF, reads a 5 x 5 region plus file headers, never saves, and takes witness paths from `WCE_ATTILA_MAP` and `WCE_ROME2_MAP`. Destructive replacement and Save/reopen checks use tiny synthetic maps whose temporary directories are cleaned automatically.

## Example and verification

On a raw/kit map at least 130 by 67 samples, export region `(62, 62, 68, 5)` to a new file. Modify one pixel in an image tool that preserves grayscale16, then replace the same rectangle. The changed region is one history entry; Undo restores the original samples and Redo restores the replacement. Save, close and reopen the raw project, export to another new filename, and compare decoded samples. Samples outside the rectangle and unrelated resources must remain unchanged. Image extraction is separate from **Export built files**.

Run synthetic checks without game assets:

```text
cargo test --locked -p wce_formats --test lf_region_png
cargo test --locked -p wce_terrain --lib lf_edit
cargo test --locked -p wce_terrain_editor --test lf_region
cargo test --locked -p wce_terrain_tool --lib lf_region
cargo check --locked -p wce_app -p wce_terrain_tool --all-targets
```

Only enable the real-witness test after setting its read-only source paths. Use fresh disposable output directories for file-writing checks. The feature adds no native import, TIFF interchange, sea-LF editing or map resizing.
