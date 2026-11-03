//! Terry's "Apply Per-Tile LF Patches" (`TerrainMetadataEditor::apply_lf_patch`, TWeak_TerrainMetadataEditor.AssemblyKit.dll fn
//! 0x10001270; docs/re/terry.md): a custom-mesh tile's `lf_mask.tif` laid into the LF under each of its placements.
//!
//! The mask is the tile's rectangle at the LF's resolution (4 samples a cell: 8 x 8 for a 2 x 2 cliff), 65535 on the plateau side and 0
//! where the cliff drops to the sea. Each mask sample under 0.9 (of 65535) becomes the LF sample under it; the others leave it. The
//! placement's turn maps mask sample (u, v) to the field (fn 0x10001080): none (u, v); 0x20 (h - 1 - v, u); 0x40 (w - 1 - u, h - 1 - v);
//! 0x80 (v, w - 1 - u), from its origin cell times the samples a cell. A placement whose rectangle leaves the field is skipped whole.
//!
//! BOB has the same walk (`apply_per_tile_lf_patches`, ToolDataBuilderDll fn 0x1002c5c0, multiplying the LF by the mask) but nothing
//! calls it: the built LF is the TIFF's, patched or not.
//!
//! ASSUMPTION: Terry reads the mask and the LF as 0 .. 1 of 65535 (the mask's float goes into the LF as it is), so a sample written is
//! the mask's raw value; the masks hold only 0 and 65535.

use crate::lf_edit::LfStroke;
use crate::paged::PagedU16;

/// The mask samples below this (of 65535) are written into the LF (Terry's 0.9).
const WRITE_BELOW: f32 = 0.9;

/// A tile's `lf_mask.tif`: `w` x `h` samples, row by row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LfPatch {
    pub w: u32,
    pub h: u32,
    pub values: Vec<u16>,
}

/// The field sample of mask sample (u, v) under a placement turned by `rotation` (relative to the placement's first sample).
fn turned(rotation: u16, w: u32, h: u32, u: u32, v: u32) -> (u32, u32) {
    match rotation & 0xf0 {
        0x20 => (h - 1 - v, u),
        0x40 => (w - 1 - u, h - 1 - v),
        0x80 => (v, w - 1 - u),
        _ => (u, v),
    }
}

/// Lays `patch` into `field` under a placement at cell `origin` turned by `rotation`, `per_cell` field samples a cell; the samples go
/// through `stroke` (one undo record for every patch). The mask's samples written and how many of them changed the LF; none when the
/// placement leaves the field (skipped).
pub fn apply(stroke: &mut LfStroke, field: &mut PagedU16, patch: &LfPatch, origin: (i32, i32), rotation: u16, per_cell: u32) -> Option<(usize, usize)> {
    let (w, h) = (patch.w, patch.h);
    if w == 0 || h == 0 || patch.values.len() < (w * h) as usize {
        return None;
    }

    let (rw, rh) = if matches!(rotation & 0xf0, 0x20 | 0x80) { (h, w) } else { (w, h) };
    let (x0, y0) = (i64::from(origin.0) * i64::from(per_cell), i64::from(origin.1) * i64::from(per_cell));
    if x0 < 0 || y0 < 0 || x0 + i64::from(rw) > i64::from(field.width()) || y0 + i64::from(rh) > i64::from(field.height()) {
        return None;
    }

    let (mut written, mut changed) = (0, 0);
    for v in 0..h {
        for u in 0..w {
            let m = patch.values[(v * w + u) as usize];
            if f32::from(m) * (1.0 / 65535.0) < WRITE_BELOW {
                let (dx, dy) = turned(rotation, w, h, u, v);
                written += 1;
                changed += usize::from(stroke.set(field, x0 as u32 + dx, y0 as u32 + dy, m));
            }
        }
    }

    Some((written, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 x 1-cell mask (8 x 4 samples) open (0) along its first row of samples only at u = 0.
    fn patch() -> LfPatch {
        let (w, h) = (8, 4);
        let mut values = vec![65535; (w * h) as usize];
        values[0] = 0;
        values[1] = 30000;
        LfPatch { w, h, values }
    }

    #[test]
    fn writes_the_low_samples_where_the_turn_puts_them() {
        // (rotation, where u = 0, v = 0 lands from the origin)
        for (rotation, at) in [(0x10, (0, 0)), (0x20, (3, 0)), (0x40, (7, 3)), (0x80, (0, 7))] {
            let mut f = PagedU16::filled(40, 40, 5000);
            let mut s = LfStroke::default();
            assert_eq!(apply(&mut s, &mut f, &patch(), (2, 3), rotation, 4), Some((2, 2)));
            let (x, y) = (8 + at.0, 12 + at.1);
            assert_eq!(f.at(x, y), 0, "rotation {rotation:#x}");
            let changed = (0..40).flat_map(|y| (0..40).map(move |x| (x, y))).filter(|&(x, y)| f.at(x, y) != 5000).count();
            assert_eq!(changed, 2, "rotation {rotation:#x}: the two low samples only");
            let r = s.finish(&f).expect("a record");
            r.revert(&mut f);
            assert!((0..40).all(|y| (0..40).all(|x| f.at(x, y) == 5000)));
        }
    }

    #[test]
    fn a_placement_off_the_field_is_skipped() {
        let mut f = PagedU16::filled(16, 16, 5000);
        let mut s = LfStroke::default();
        assert_eq!(apply(&mut s, &mut f, &patch(), (3, 0), 0x10, 4), None);
        assert_eq!(apply(&mut s, &mut f, &patch(), (-1, 0), 0x10, 4), None);
        assert!(s.finish(&f).is_none());
    }
}
