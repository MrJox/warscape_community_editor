//! The game's refit of a battle prop decal's box to the terrain under it (docs/re/renderer.md, "Traced ... the game refits battle prop
//! decals to the terrain"; empire.retail.dll fn 0x104c4250 and its terrain measure fn 0x1043b0c0). A decal's box (the unit box, -0.5 .. 0.5,
//! by its world matrix) is measured against the lowest and highest terrain under its footprint along its own vertical axis; the box from
//! `LOWER_MARGIN` under the lowest to `UPPER_MARGIN` over the highest takes its place when that is no taller than the box. So a road's tall
//! box paints the ground and what stands on it up to half a unit over the ground, not the tents and walls above; a box already shorter is
//! kept, wherever it is (a decal on a bridge's deck).
//!
//! NOT DONE: the game's measure also takes the points of the ground surfaces overlapping the footprint (building platforms with `flag_2`,
//! or of a `ground_platform` building, and one unidentified class; they lie at about the ground's height); and it refits only decals whose
//! footprint is inside the map's bounds (all here).

/// `rescale_battle_prop_decals_lower_safety_margin` and `..._upper_safety_margin` (scene units, the game's).
pub const LOWER_MARGIN: f64 = 0.25;
pub const UPPER_MARGIN: f64 = 0.5;

/// A decal's world matrix (rows of a column-vector 3 x 4 transform: `rows[r][c]`, the columns its axes, `rows[r][3]` its origin) refitted to
/// the ground, or `None` when the game keeps it (the refitted box would be taller, or its vertical axis is degenerate). `ground(x, z)` is
/// the terrain's height in the matrix's units; `step` the height map's spacing (the footprint is sampled at it, a sample beyond each side,
/// as the game reads every cell under it); `margins` the lower and upper margins in the matrix's units.
#[must_use]
pub fn refit(rows: [[f64; 4]; 3], ground: &dyn Fn(f64, f64) -> f64, step: f64, margins: [f64; 2]) -> Option<[[f64; 4]; 3]> {
    let col = |c: usize| [rows[0][c], rows[1][c], rows[2][c]];
    let origin = col(3);
    let y = col(1);
    let height = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
    if height < 1e-9 || step <= 0.0 {
        return None;
    }

    let axis = y.map(|v| v / height);

    // The footprint: the box's corners on the map.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for corner in 0..8 {
        let s = [corner & 1, corner >> 1 & 1, corner >> 2 & 1].map(|b| if b == 0 { -0.5 } else { 0.5 });
        let p = (0..3).map(|r| origin[r] + rows[r][0] * s[0] + rows[r][1] * s[1] + rows[r][2] * s[2]).collect::<Vec<_>>();
        (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(p[0]), lo[1].min(p[2]), hi[0].max(p[0]), hi[1].max(p[2]));
    }

    // The lowest and highest ground under it, measured along the box's vertical axis from its origin.
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    let along = |x: f64, z: f64| (x - origin[0]) * axis[0] + (ground(x, z) - origin[1]) * axis[1] + (z - origin[2]) * axis[2];
    let (nx, nz) = (((hi[0] - lo[0]) / step).ceil() as i64 + 2, ((hi[1] - lo[1]) / step).ceil() as i64 + 2);
    if nx * nz > 4_000_000 {
        return None;
    }

    for i in -1..nx {
        for k in -1..nz {
            let (x, z) = ((lo[0] + i as f64 * step).min(hi[0] + step), (lo[1] + k as f64 * step).min(hi[1] + step));
            let d = along(x, z);
            (low, high) = (low.min(d), high.max(d));
        }
    }

    // The game swaps the margins when the axis points down.
    let (below, above) = if axis[1] < 0.0 { (margins[1], margins[0]) } else { (margins[0], margins[1]) };
    let new_height = (high + above) - (low - below);
    if new_height > height {
        return None;
    }

    let shift = new_height * 0.5 + (low - below);
    let mut out = rows;
    for r in 0..3 {
        out[r][1] = axis[r] * new_height;
        out[r][3] = origin[r] + axis[r] * shift;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_rows(size: [f64; 3], at: [f64; 3]) -> [[f64; 4]; 3] {
        [[size[0], 0.0, 0.0, at[0]], [0.0, size[1], 0.0, at[1]], [0.0, 0.0, size[2], at[2]]]
    }

    #[test]
    fn on_flat_ground_a_tall_box_goes_to_three_quarters_from_a_quarter_under_it() {
        let r = refit(box_rows([60.0, 40.0, 5.0], [0.0, 10.0, 0.0]), &|_, _| 30.0, 1.0, [LOWER_MARGIN, UPPER_MARGIN]).unwrap();
        assert!((r[1][1] - 0.75).abs() < 1e-9, "{r:?}");
        assert!((r[1][3] - 30.125).abs() < 1e-9, "from 29.75 to 30.5: {r:?}");
        assert_eq!((r[0][0], r[2][2], r[0][3], r[2][3]), (60.0, 5.0, 0.0, 0.0), "the rest as it was");
    }

    #[test]
    fn on_a_slope_it_spans_the_ground_under_it() {
        // Ground rising 1 in 10 along x over a box 20 long: 2 from end to end (plus a cell beyond each side).
        let r = refit(box_rows([20.0, 40.0, 4.0], [0.0, 0.0, 0.0]), &|x, _| x * 0.1, 1.0, [LOWER_MARGIN, UPPER_MARGIN]).unwrap();
        let (bottom, top) = (r[1][3] - r[1][1] * 0.5, r[1][3] + r[1][1] * 0.5);
        assert!((bottom - (-1.1 - 0.25)).abs() < 1e-9 && (top - (1.1 + 0.5)).abs() < 1e-9, "{bottom} {top}");
    }

    #[test]
    fn a_box_shorter_than_the_refit_is_kept_wherever_it_is() {
        // A deck decal 0.7 tall, 3 over flat ground: the refit (0.75) is taller, so the game leaves it on the deck.
        assert!(refit(box_rows([10.0, 0.7, 5.0], [0.0, 33.0, 0.0]), &|_, _| 30.0, 1.0, [LOWER_MARGIN, UPPER_MARGIN]).is_none());
        // A turned box (its z scale negative, as TEd's decals): its height is its axis' length either way.
        let r = refit(box_rows([10.0, 2.0, -5.0], [0.0, 30.0, 0.0]), &|_, _| 30.0, 1.0, [LOWER_MARGIN, UPPER_MARGIN]).unwrap();
        assert!((r[1][1] - 0.75).abs() < 1e-9);
    }
}
