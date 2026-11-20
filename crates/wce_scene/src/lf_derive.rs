//! A campaign map's LF fields as the game reads them (RenderDoc, capture A of Attila's main_attila; docs/re/renderer.md, "The sea tiles
//! and the coast"). The kit's fields are denser than the game's textures, and the game draws from those: a view of a field derived so
//! (`LfDerive`) is what the editor draws, the field itself what it edits. The view's rows run as the field's (row 0 the map's top).

/// How a drawn view derives from a field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfDerive {
    /// The land: the field's k x k box averages (main_attila: k 4, the field's eight samples a cell to the texture's two, 4064 x 2882;
    /// BOB's `lf_height_map_lq.dds`), texel i the mean of samples k i .. k i + k - 1. The terrain shader puts texel i at i / 2 cells.
    Box(u32),

    /// The sea: the sea pass's read of its texture, on the field's own lattice (`game_sea`).
    GameSea,
}

impl LfDerive {
    /// The view's size for a field of `w` x `h`.
    pub fn size(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            Self::Box(k) => ((w / k).max(1), (h / k).max(1)),
            Self::GameSea => (w, h),
        }
    }

    /// The view's sample (i, j) from the field (`at`: a field sample, clamped by the caller's accessor to the field).
    pub fn sample(self, at: &dyn Fn(i64, i64) -> f64, w: u32, h: u32, i: i64, j: i64) -> f64 {
        match self {
            Self::Box(k) => {
                let k = i64::from(k);
                let mut s = 0.0;
                for b in 0..k {
                    for a in 0..k {
                        s += at(k * i + a, k * j + b);
                    }
                }
                s / (k * k) as f64
            }
            Self::GameSea => game_sea(at, w, h, i as f64 / f64::from(w), j as f64 / f64::from(h)),
        }
    }

    /// The view's samples a change of the field's rectangle (x0, y0, x1, y1; end exclusive) reaches, as a view rectangle.
    pub fn reach(self, r: [u32; 4], w: u32, h: u32) -> [u32; 4] {
        let (vw, vh) = self.size(w, h);
        match self {
            Self::Box(k) => [r[0] / k, r[1] / k, r[2].div_ceil(k).min(vw), r[3].div_ceil(k).min(vh)],
            // A texel of the sea's texture is 2 x 2 samples and the read spans two of them and a texel's jump: 8 samples either side.
            Self::GameSea => [r[0].saturating_sub(8), r[1].saturating_sub(8), (r[2] + 8).min(vw), (r[3] + 8).min(vh)],
        }
    }
}

/// The sea pass's read at (u, v) of the map (0 .. 1 over its width and height from its top left), in the field's units. Its texture is
/// the field's 2 x 2 box average (main_attila: 2032 x 1441 from 4064 x 2882, within rounding), but `vs41_geom_new` gets the dimensions of
/// its second mip (1016 x 720) and gathers from the first: a point at p = u * 1016 takes texels floor((floor(p) + 1) / 1016 * 2032 - 0.5)
/// and the next, weighted by frac(p); so a texel's height stands about 0.75 cell off its ground and jumps every two cells (six sea-pass
/// depths of capture A within 0.4 m of this, the field read where it lies 6 m off at one).
pub fn game_sea(at: &dyn Fn(i64, i64) -> f64, w: u32, h: u32, u: f64, v: f64) -> f64 {
    let (n0x, n0y) = ((w / 2).max(1), (h / 2).max(1));
    let (n1x, n1y) = (f64::from((n0x / 2).max(1)), f64::from((n0y / 2).max(1)));
    let t = |x: i64, y: i64| {
        let (x, y) = (x.clamp(0, i64::from(n0x) - 1), y.clamp(0, i64::from(n0y) - 1));
        (at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1)) * 0.25
    };
    let (px, py) = (u.clamp(0.0, 1.0 - 1.0 / n1x) * n1x, v.clamp(0.0, 1.0 - 1.0 / n1y) * n1y);
    let (i, j) = (px.floor(), py.floor());
    let (fx, fy) = (px - i, py - j);
    let ax = ((i + 1.0) / n1x * f64::from(n0x) - 0.5).floor() as i64;
    let ay = ((j + 1.0) / n1y * f64::from(n0y) - 0.5).floor() as i64;
    let a = t(ax, ay) + (t(ax + 1, ay) - t(ax, ay)) * fx;
    let c = t(ax, ay + 1) + (t(ax + 1, ay + 1) - t(ax, ay + 1)) * fx;
    a + (c - a) * fy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_averages_and_reach() {
        let (w, h) = (8u32, 8u32);
        let at = |x: i64, y: i64| (x.clamp(0, 7) + 10 * y.clamp(0, 7)) as f64;
        assert_eq!(LfDerive::Box(4).size(w, h), (2, 2));
        // Samples 4 .. 7 by 0 .. 3: x mean 5.5, y mean 1.5.
        assert!((LfDerive::Box(4).sample(&at, w, h, 1, 0) - (5.5 + 15.0)).abs() < 1e-9);
        assert_eq!(LfDerive::Box(4).reach([3, 0, 5, 1], w, h), [0, 0, 2, 1]);
    }

    #[test]
    fn game_sea_on_a_ramp_lags_three_quarters_of_a_texel_pair() {
        // A ramp along x of one unit a sample: the read at sample 0 is texel 1 of the 2 x 2 average (samples 2 and 3: 2.5).
        let (w, h) = (64u32, 64u32);
        let at = |x: i64, _: i64| x.clamp(0, 63) as f64;
        assert!((game_sea(&at, w, h, 0.0, 0.0) - 2.5).abs() < 1e-9);
    }
}
