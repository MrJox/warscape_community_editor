# Built campaign forward height preparation

`built::campaign_heights` prepares explicit forward height deltas from stored campaign
CHMF/full/LQ baselines without modifying its inputs. The API returns three height resources
for inspection and validation. Campaign-normal generation, authoring-state persistence,
native publication and interactive editing are separate responsibilities. Existing built
reader/writer routing and behavior remain unchanged.

## Why the battle writer cannot be reused

The stock Rome II campaign witness has CHMF v5 2032×1441, header parameters (1,8),
spacing 1, 11,557 literal 16×16 regions, each with one 256-sample compressed block.
Full DDS is 8128×5764 and LQ DDS 4064×2882, both single-level L16. Those dimensions
do not make CHMF a point-sample of the full DDS. In a complete native comparison,
only 574,785 of 2,928,112 coarse samples equal full[4x,4y].

`attila::heights::campaign_lf_data` and `campaign_level`, documented in `docs/re/terry.md`,
start from the authoring TIFF: CHMF samples a source corner, full averages a source 2×2,
LQ averages a source 4×4. Normalized f32 sums are truncated. Full and LQ are separately
quantized; they are not a chain of quantized reductions. On the Rome witness,
`4*LQ - sum(full 2×2)` ranges −4…4 across 11,712,448 LQ samples. Repeating the raw
builder on the already-built full DDS would introduce another reduction stage.

The preserved legacy author supplies a separate continuous nodal delta field, samples its
corner for CHMF and its area means for full/LQ, rounds deltas with Python ties-to-even,
then adds them to the existing resources. It preserves untouched compressed regions,
headers, opaque bytes and padding and recomputes only changed region minima. These
preservation principles are compatible with Rust's existing CHMF/Blob architecture.
The legacy forward field is not present in WCE's built DDS edits; it is not a verified
inverse algorithm. Different source patches can have the same full mean and different
CHMF corners. An arbitrary full-pixel edit does not establish the corresponding corner.

The stock Rome II campaign has 736 `normals/normal_N.dds` resources, each 576×576 DXT5
with 10 mips. That layout fits the existing shipped-chunk mechanism, but its hardcoded
battle height scale (3000) does not establish correct campaign-scale normal deltas.
Attila's documented campaign `lf_normal.dds` is a different resource arrangement. Leaving
normals stale, or inventing a scale/filter conversion, does not close height output.

## Independently validated forward height preparation

`campaign_heights::NodalDelta` accepts a complete inclusive row-major rectangle of finite
raw f64 deltas with a zero border. Bounds must lie strictly inside the map, as in the
witnessed legacy operation. `prepare_delta` works from the stored CHMF/full/LQ baselines:

* CHMF uses the rounded delta at its literal integer node. Only changed 16×16 blocks
  are encoded, using the existing Rust `encode_region`; changed region minima are
  recomputed, with tag1, rectangles, header, padding, prefix/suffix bytes and every
  untouched compressed region retained.
* Full DDS deltas average the bilinear field at `((2x+a)/8,(2y+b)/8)`, a,b in {0,1}.
  LQ deltas average four **unrounded** full deltas. Each representation rounds once,
  nearest/ties-to-even, then adds to its own original integer sample. No baseline
  reduction or normalized-f32 reconversion occurs. Overflow/underflow is an error.
* The result contains exactly three height resources, with their original path casing.
  Normal files are not included as if corrected. DDS headers stay identical; this
  profile requires single-image L16 data, native 4x/2x grid relationships and the
  CHMF v5 (1,8), spacing1, min0/max65535, literal one-block/minimum/65535 layout.
  Other edited profiles and unsupported edge fields are refused. Quantized no-op fields
  keep original blobs without decoding unknown data.

Prior validation compared all three complete resources against independently audited
native outputs. Two archived Rome witnesses reproduced byte for byte:
Phase5B-2:535 CHMF samples/seven regions,9,685 full/2,438 LQ pixels; Phase5C-3C:
98 samples/two regions,1,967 full/492 LQ pixels. Their full/LQ delta discrepancy is0.75
raw units. Both preserve original dimensions, all unrelated regions and padding. No
original CA authoring TIFF or compiler equivalence is required for this forward proof.

## Output boundary

The adapter requires an explicit nodal field; it does not infer one from arbitrary
quantized full-DDS brush edits. `PreparedHeights` contains height-only validation data.
Complete edited native campaign output also requires verified normal scale, composition
and mip updates. The historical Rome witnesses validate the forward algorithm for those
inputs; Attila/Thrones compatibility and game-runtime acceptance remain unverified here.
