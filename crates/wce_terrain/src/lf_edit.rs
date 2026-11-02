//! LF brushes on a paged field (docs/plans/terrain_editor.md, Phase 3): a stroke is dabs of an operation under a brush; it keeps a float
//! copy of the pages it touches (the dabs add up there, the field gets them rounded: many small dabs do not lose to rounding) and their
//! samples as they were, so that the finished stroke is an undo record of those pages alone (a 256-sample brush keeps about 80 pages,
//! 650 KB, where a copy of the campaign map's field is 93 MB).
//!
//! The brush is the Tile tool's (`wce_tile::height_field::Brush`: falloff, falloff curve, image) with Terry's aspect ratio and rotation.
//! Values are the field's raw units (0 .. 65535; metres = raw / 65535 * lf_height - vertical_offset).

use std::collections::BTreeMap;

use wce_tile::height_field::{Brush, fractal_noise};

use crate::paged::{PAGE_LEN, PagedU16};

/// A brush on the field: the Tile tool's brush (centre and radius in samples) stretched along its own x by `aspect` and turned by
/// `rotation` (radians).
#[derive(Clone, Debug, Default)]
pub struct LfBrush {
    pub brush: Brush,
    pub aspect: f64,
    pub rotation: f64,
}

impl LfBrush {
    /// Its weight at an offset (samples) from the centre.
    pub fn weight(&self, dx: f64, dy: f64) -> f64 {
        let (s, c) = self.rotation.sin_cos();
        let (u, v) = (c * dx + s * dy, -s * dx + c * dy);
        let aspect = if self.aspect > 0.0 { self.aspect } else { 1.0 };
        self.brush.weight(u / aspect, v)
    }

    /// The samples it can reach: x0, y0, x1, y1 (inclusive), clipped to a `w` x `h` field.
    fn reach(&self, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        let r = self.brush.radius * self.aspect.max(1.0) + 1.0;
        let (cx, cy) = (self.brush.cx, self.brush.cy);
        let (x0, y0) = ((cx - r).floor().max(0.0), (cy - r).floor().max(0.0));
        let (x1, y1) = ((cx + r).ceil().min(f64::from(w) - 1.0), (cy + r).ceil().min(f64::from(h) - 1.0));
        (x0 <= x1 && y0 <= y1).then_some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
    }
}

/// What a dab does at full weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfOp {
    /// Adds raw units (negative lowers).
    Raise(f64),

    /// Towards the mean of the 3 x 3 samples around.
    Smooth,

    /// Towards a height (raw).
    Flatten(f64),

    /// Adds fractal noise of `amount` raw units, features `scale` samples across (the same pattern wherever the brush goes).
    Noise { amount: f64, scale: f64 },
}

/// A stroke being painted.
#[derive(Debug, Default)]
pub struct LfStroke {
    /// The touched pages: their samples before the stroke and their float copy.
    before: BTreeMap<u32, Box<[u16]>>,
    shadow: BTreeMap<u32, Box<[f32]>>,

    /// What the stroke changed so far (x0, y0, x1, y1; end exclusive).
    rect: Option<[u32; 4]>,
}

/// A finished stroke: the pages it changed, before and after, and the samples it changed.
#[derive(Clone, Debug, Default)]
pub struct LfRecord {
    pub rect: [u32; 4],
    pages: Vec<(u32, Box<[u16]>, Box<[u16]>)>,
}

impl LfRecord {
    /// The field as the stroke left it.
    pub fn apply(&self, field: &mut PagedU16) {
        for (p, _, after) in &self.pages {
            field.page_at_mut(*p).copy_from_slice(after);
        }
    }

    /// The field as it was before the stroke.
    pub fn revert(&self, field: &mut PagedU16) {
        for (p, before, _) in &self.pages {
            field.page_at_mut(*p).copy_from_slice(before);
        }
    }

    /// The memory it keeps (bytes).
    pub fn bytes(&self) -> usize {
        self.pages.len() * PAGE_LEN * 4
    }
}

impl LfStroke {
    /// One dab: the samples under the brush moved by the operation, by the brush's weight. The samples it changed (x0, y0, x1, y1; end
    /// exclusive), none when it changed none.
    pub fn dab(&mut self, field: &mut PagedU16, b: &LfBrush, op: LfOp) -> Option<[u32; 4]> {
        let (x0, y0, x1, y1) = b.reach(field.width(), field.height())?;
        let mut changed: Option<[u32; 4]> = None;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let w = b.weight(f64::from(x) - b.brush.cx, f64::from(y) - b.brush.cy);
                if w <= 0.0 {
                    continue;
                }

                let (p, k) = field.locate(x, y);
                if !self.shadow.contains_key(&p) {
                    let page = field.page_at(p);
                    self.before.insert(p, page.into());
                    self.shadow.insert(p, page.iter().map(|&v| f32::from(v)).collect());
                }

                let cur = f64::from(self.shadow[&p][k]);
                let new = match op {
                    LfOp::Raise(d) => cur + d * w,
                    LfOp::Flatten(t) => cur + (t - cur) * w,
                    LfOp::Noise { amount, scale } => cur + amount * w * fractal_noise(f64::from(x) / scale.max(1.0), f64::from(y) / scale.max(1.0)),
                    LfOp::Smooth => {
                        let mut sum = 0.0;
                        let mut n = 0.0;
                        for (sx, sy) in [(-1i32, -1i32), (0, -1), (1, -1), (-1, 0), (0, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                            let (nx, ny) = (x as i32 + sx, y as i32 + sy);
                            if nx >= 0 && ny >= 0 && (nx as u32) < field.width() && (ny as u32) < field.height() {
                                sum += f64::from(field.at(nx as u32, ny as u32));
                                n += 1.0;
                            }
                        }

                        cur + (sum / n - cur) * w
                    }
                };

                let new = new.clamp(0.0, 65535.0);
                self.shadow.get_mut(&p).expect("made above")[k] = new as f32;
                let q = new.round() as u16;
                if q != field.at(x, y) {
                    field.set(x, y, q);
                    changed = Some(changed.map_or([x, y, x + 1, y + 1], |c| [c[0].min(x), c[1].min(y), c[2].max(x + 1), c[3].max(y + 1)]));
                }
            }
        }

        if let Some(c) = changed {
            self.rect = Some(self.rect.map_or(c, |r| [r[0].min(c[0]), r[1].min(c[1]), r[2].max(c[2]), r[3].max(c[3])]));
        }

        changed
    }

    /// The stroke's undo record: the pages it touched that changed (none when nothing did).
    pub fn finish(self, field: &PagedU16) -> Option<LfRecord> {
        let rect = self.rect?;
        let pages: Vec<(u32, Box<[u16]>, Box<[u16]>)> = self.before.into_iter()
            .filter_map(|(p, before)| {
                let after = field.page_at(p);
                (*before != *after).then(|| (p, before, after.into()))
            })
            .collect();

        (!pages.is_empty()).then_some(LfRecord { rect, pages })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(cx: f64, cy: f64, r: f64) -> LfBrush {
        LfBrush { brush: Brush { cx, cy, radius: r, strength: 1.0, falloff: 1.0, ..Default::default() }, aspect: 1.0, rotation: 0.0 }
    }

    #[test]
    fn small_dabs_add_up_and_undo_restores() {
        let mut f = PagedU16::filled(200, 150, 1000);
        let original = f.to_le_rows();
        let mut s = LfStroke::default();
        // A tenth of a raw unit at a time: rounded each dab it would never move.
        for _ in 0..50 {
            s.dab(&mut f, &brush(100.0, 75.0, 20.0), LfOp::Raise(0.1));
        }

        assert_eq!(f.at(100, 75), 1005);
        let r = s.finish(&f).expect("a record");
        // Inside the brush (the bell's edge moved less than a unit), around its centre.
        assert!(r.rect[0] >= 80 && r.rect[2] <= 121 && r.rect[0] <= 100 && r.rect[2] > 100, "{:?}", r.rect);
        assert!(r.bytes() <= 4 * PAGE_LEN * 4);
        let after = f.to_le_rows();
        r.revert(&mut f);
        assert_eq!(f.to_le_rows(), original);
        r.apply(&mut f);
        assert_eq!(f.to_le_rows(), after);
    }

    #[test]
    fn flatten_smooth_and_the_aspect() {
        let mut f = PagedU16::from_le_rows(64, 64, &(0..64 * 64).flat_map(|i: u32| ((i % 64) as u16 * 100).to_le_bytes()).collect::<Vec<_>>());
        let mut s = LfStroke::default();
        for _ in 0..30 {
            s.dab(&mut f, &brush(32.0, 32.0, 6.0), LfOp::Flatten(2000.0));
        }

        assert_eq!(f.at(32, 32), 2000);
        s.dab(&mut f, &brush(10.0, 10.0, 4.0), LfOp::Smooth);
        assert!(s.finish(&f).is_some());

        // Stretched along x: reaches 2 radii across, 1 down.
        let b = LfBrush { aspect: 2.0, ..brush(0.0, 0.0, 10.0) };
        assert!(b.weight(15.0, 0.0) > 0.0);
        assert_eq!(b.weight(0.0, 15.0), 0.0);
        let t = LfBrush { rotation: std::f64::consts::FRAC_PI_2, ..b };
        assert!(t.weight(0.0, 15.0) > 0.0);
    }
}
