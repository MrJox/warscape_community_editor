//! The Smooth brushes' blur (heights and the ground's textures): a box average over a neighbourhood that grows with the brush, so that a
//! wide brush evens out wide bumps and not only the pixel-sized ones.

/// The blur's reach in pixels each way for a brush `radius_px` across: an eighth of the brush, 1 to 8 pixels.
pub fn kernel_radius(radius_px: f64) -> u32 {
    (radius_px / 8.0).round().clamp(1.0, 8.0) as u32
}

/// The box average (reach `k` each way, clipped to the `w` x `h` map: only the pixels on it count) of `channels` values a pixel over the
/// rectangle `x0..=x1`, `y0..=y1`, row by row; `get` fills a pixel's values. Two passes (rows, then columns) over the rectangle and its margin.
pub fn box_average(
    (w, h): (u32, u32),
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    k: u32,
    channels: usize,
    get: impl Fn(u32, u32, &mut [f64]),
) -> Vec<f64> {
    // The rectangle grown by the reach (clipped): what the averages read.
    let (gx0, gy0) = (x0.saturating_sub(k), y0.saturating_sub(k));
    let (gx1, gy1) = ((x1 + k).min(w - 1), (y1 + k).min(h - 1));
    let (gw, gh) = ((gx1 - gx0 + 1) as usize, (gy1 - gy0 + 1) as usize);
    let mut src = vec![0.0; gw * gh * channels];
    for y in gy0..=gy1 {
        for x in gx0..=gx1 {
            let i = ((y - gy0) as usize * gw + (x - gx0) as usize) * channels;
            get(x, y, &mut src[i..i + channels]);
        }
    }

    // Rows: the horizontal sums of the rectangle's columns, every grown row.
    let ow = (x1 - x0 + 1) as usize;
    let mut rows = vec![0.0; ow * gh * channels];
    for ry in 0..gh {
        for ox in 0..ow {
            let x = x0 + ox as u32;
            let (a, b) = (x.saturating_sub(k).max(gx0), (x + k).min(gx1));
            let o = (ry * ow + ox) * channels;
            for sx in a..=b {
                let i = (ry * gw + (sx - gx0) as usize) * channels;
                for c in 0..channels {
                    rows[o + c] += src[i + c];
                }
            }

            let n = f64::from(b - a + 1);
            for v in &mut rows[o..o + channels] {
                *v /= n;
            }
        }
    }

    // Columns.
    let oh = (y1 - y0 + 1) as usize;
    let mut out = vec![0.0; ow * oh * channels];
    for oy in 0..oh {
        let y = y0 + oy as u32;
        let (a, b) = (y.saturating_sub(k).max(gy0), (y + k).min(gy1));
        let n = f64::from(b - a + 1);
        for ox in 0..ow {
            let o = (oy * ow + ox) * channels;
            for sy in a..=b {
                let i = ((sy - gy0) as usize * ow + ox) * channels;
                for c in 0..channels {
                    out[o + c] += rows[i + c];
                }
            }

            for v in &mut out[o..o + channels] {
                *v /= n;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_of_a_box_clipped_to_the_map() {
        // A 5 x 1 map 0, 0, 10, 0, 0: reach 1 at x = 2 is 10 / 3; at the edge x = 0, (0 + 0) / 2.
        let v = [0.0, 0.0, 10.0, 0.0, 0.0];
        let out = box_average((5, 1), (0, 0, 4, 0), 1, 1, |x, _, o| o[0] = v[x as usize]);
        assert!((out[2] - 10.0 / 3.0).abs() < 1e-12 && out[0] == 0.0 && (out[1] - 10.0 / 3.0).abs() < 1e-12);

        // A part of a map, two channels, reach 2 over a 6 x 6 map of x + 10 y.
        let out = box_average((6, 6), (2, 2, 3, 3), 2, 2, |x, y, o| {
            o[0] = f64::from(x + 10 * y);
            o[1] = 1.0;
        });
        assert_eq!(out.len(), 2 * 2 * 2);
        // (2, 2): x 0..=4 and y 0..=4: mean x 2, mean y 2.
        assert!((out[0] - 22.0).abs() < 1e-12 && (out[1] - 1.0).abs() < 1e-12);
        // (3, 3): x 1..=5, y 1..=5: 33.
        assert!((out[6] - 33.0).abs() < 1e-12);
    }

    #[test]
    fn the_reach_grows_with_the_brush() {
        assert_eq!((kernel_radius(2.0), kernel_radius(12.0), kernel_radius(40.0), kernel_radius(500.0)), (1, 2, 5, 8));
    }
}
