# Exact land LF region PNG interchange

The Terrain panel's **Land LF region PNG** section and the following commands transfer a bounded rectangular region of the document's land LF:

```text
terrain_lf_region_export <x> <y> <width> <height> <new.png>
terrain_lf_region_import <x> <y> <width> <height> <source.png>
```

Coordinates and dimensions are unsigned file sample counts. `(0, 0)` is the top-left sample, x increases right and y increases down. The end is exclusive. Both dimensions must be positive, the entire rectangle must be inside the field, and its area must not exceed 1,048,576 samples. This is the land LF only, independent of the LF brush's sea selection.

Each PNG pixel is the exact unsigned 16-bit LF value. Files must be non-interlaced 16-bit grayscale without alpha or transparency; dimensions must exactly match the requested region. There is no brightness conversion, normalization, gamma/profile transformation, row inversion or resampling. PNG sample bytes are big-endian as required by PNG. The bounded decoder checks dimensions, framing, chunk CRCs and critical chunk support before the general PNG decoder allocates pixel storage. Input files including metadata are limited to 16 MiB.

PNG export reads the document's current full LF samples without editing its model, history, source files or native height resources. It creates a new output file and refuses existing paths, including aliases to existing map resources. A cancelled chooser does nothing. Native/built battle and campaign documents support this read-only export. **PNG export does not establish that native campaign-height Save/Export is safe.**

Replacement is available only for raw/kit documents. Native/built import refuses before input-file access, pixel mutation or history access. All region, image and mirror checks precede mutation. A successful change uses one existing `LfStrokeCommand` with before/after copies of only changed 64 x 64 pages. Identical replacement records nothing and retains any redo tail. Undo and Redo each mirror the region once; the existing `follow_lf_edits` path updates picking and affected view chunks. There is no per-pixel scene reconstruction or automatic fill.

Persistence uses the existing raw writer. TIFF headers, trailing bytes, samples outside the rectangle, sea LF and other resources retain their existing preservation rules. Database metadata and project override ordering are unchanged. No native writer, normal-authority API, campaign-height preparation API, dependency manifest or lockfile changes are needed.

Focused coverage is in `wce_formats/tests/lf_region_png.rs`, the `wce_terrain::lf_edit` controls, `wce_terrain_editor/tests/lf_region.rs`, and the Terrain tool's `lf_region_tests`. The real-witness test is opt-in and maps only the land LF, reads a 5 x 5 region plus file headers, never saves, and takes witness paths from `D3E_ATTILA_MAP` and `D3E_ROME2_MAP`. Destructive replacement and Save/reopen checks use tiny synthetic maps.
