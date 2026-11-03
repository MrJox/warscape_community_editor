//! Edits of a cell layer (docs/plans/terrain_editor.md, Phase 4): a stroke paints cells (a round or square brush, a rectangle, a flood
//! fill) and keeps the 8 x 8-cell chunks it touches as they were, so that the finished stroke is an undo record of those chunks alone and
//! says which chunks of the map changed (the map context's chunks: what the fill run again rebuilds).

use std::collections::BTreeMap;

use crate::cells::{CellLayer, Rgba};

/// Cells a side of a chunk (the map context's `CHUNK`).
pub const CHUNK: u32 = 8;

/// The cells of a chunk, row by row (those past the layer's edge stay 0).
type ChunkCells = [u16; (CHUNK * CHUNK) as usize];

/// A stroke being painted on one layer.
#[derive(Debug, Default)]
pub struct CellStroke {
    /// The touched chunks (index: chunk row * chunks a row + chunk column), as they were before the stroke.
    before: BTreeMap<u32, ChunkCells>,

    /// What the stroke changed so far (x0, y0, x1, y1; end exclusive).
    rect: Option<[u32; 4]>,
}

/// A finished stroke: the chunks it changed, before and after, and the cells it changed.
#[derive(Clone, Debug, Default)]
pub struct CellRecord {
    pub rect: [u32; 4],
    chunks: Vec<(u32, ChunkCells, ChunkCells)>,
}

fn chunks_a_row(l: &CellLayer) -> u32 {
    l.width().div_ceil(CHUNK)
}

fn read_chunk(l: &CellLayer, c: u32) -> ChunkCells {
    let n = chunks_a_row(l);
    let (cx, cy) = (c % n * CHUNK, c / n * CHUNK);
    let mut out = [0; (CHUNK * CHUNK) as usize];
    for y in cy..(cy + CHUNK).min(l.height()) {
        for x in cx..(cx + CHUNK).min(l.width()) {
            out[((y - cy) * CHUNK + x - cx) as usize] = l.index(x, y);
        }
    }

    out
}

fn write_chunk(l: &mut CellLayer, c: u32, cells: &ChunkCells) {
    let n = chunks_a_row(l);
    let (cx, cy) = (c % n * CHUNK, c / n * CHUNK);
    for y in cy..(cy + CHUNK).min(l.height()) {
        for x in cx..(cx + CHUNK).min(l.width()) {
            l.set_index(x, y, cells[((y - cy) * CHUNK + x - cx) as usize]);
        }
    }
}

impl CellRecord {
    /// The layer as the stroke left it.
    pub fn apply(&self, l: &mut CellLayer) {
        for (c, _, after) in &self.chunks {
            write_chunk(l, *c, after);
        }
    }

    /// The layer as it was before the stroke.
    pub fn revert(&self, l: &mut CellLayer) {
        for (c, before, _) in &self.chunks {
            write_chunk(l, *c, before);
        }
    }

    /// The memory it keeps (bytes).
    pub fn bytes(&self) -> usize {
        self.chunks.len() * (CHUNK * CHUNK) as usize * 4
    }

    /// The chunks it changed (column, row).
    pub fn chunks(&self, l: &CellLayer) -> Vec<(u32, u32)> {
        let n = chunks_a_row(l);
        self.chunks.iter().map(|(c, ..)| (c % n, c / n)).collect()
    }
}

impl CellStroke {
    /// Sets a cell to a palette index, keeping its chunk as it was first. Whether it changed.
    pub fn set(&mut self, l: &mut CellLayer, x: u32, y: u32, i: u16) -> bool {
        if x >= l.width() || y >= l.height() || l.index(x, y) == i {
            return false;
        }

        let c = y / CHUNK * chunks_a_row(l) + x / CHUNK;
        self.before.entry(c).or_insert_with(|| read_chunk(l, c));
        l.set_index(x, y, i);
        let r = [x, y, x + 1, y + 1];
        self.rect = Some(self.rect.map_or(r, |o| [o[0].min(x), o[1].min(y), o[2].max(x + 1), o[3].max(y + 1)]));
        true
    }

    /// The cells (x0, y0, x1, y1; end exclusive, clipped) changed by painting `colour` where `inside` says, over `area`.
    fn paint_where(&mut self, l: &mut CellLayer, area: [i64; 4], colour: Rgba, inside: impl Fn(i64, i64) -> bool) -> Option<[u32; 4]> {
        let i = l.index_of(colour)?;
        let (x0, y0) = (area[0].max(0), area[1].max(0));
        let (x1, y1) = (area[2].min(i64::from(l.width())), area[3].min(i64::from(l.height())));
        let mut changed: Option<[u32; 4]> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                if inside(x, y) && self.set(l, x as u32, y as u32, i) {
                    let (x, y) = (x as u32, y as u32);
                    changed = Some(changed.map_or([x, y, x + 1, y + 1], |c| [c[0].min(x), c[1].min(y), c[2].max(x + 1), c[3].max(y + 1)]));
                }
            }
        }

        changed
    }

    /// The brush at cell (cx, cy): `radius` cells (0: the cell alone), round or square. The cells it changed.
    pub fn brush(&mut self, l: &mut CellLayer, cx: i64, cy: i64, radius: u32, round: bool, colour: Rgba) -> Option<[u32; 4]> {
        let r = i64::from(radius);
        let r2 = (r * r) as f64 + r as f64;
        self.paint_where(l, [cx - r, cy - r, cx + r + 1, cy + r + 1], colour, |x, y| !round || ((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f64 <= r2)
    }

    /// A rectangle of cells (x0, y0, x1, y1; end exclusive). The cells it changed.
    pub fn rect(&mut self, l: &mut CellLayer, rect: [i64; 4], colour: Rgba) -> Option<[u32; 4]> {
        self.paint_where(l, rect, colour, |_, _| true)
    }

    /// The flood fill from cell (x, y): every cell of its colour reached across edges (4-connected) painted `colour`. The cells it changed.
    pub fn flood(&mut self, l: &mut CellLayer, x: u32, y: u32, colour: Rgba) -> Option<[u32; 4]> {
        if x >= l.width() || y >= l.height() {
            return None;
        }

        let from = l.index(x, y);
        let to = l.index_of(colour)?;
        if from == to {
            return None;
        }

        let (w, h) = (l.width(), l.height());
        let mut changed: Option<[u32; 4]> = None;
        let mut todo = vec![(x, y)];
        while let Some((x, y)) = todo.pop() {
            // A run along the row, then the rows above and below it.
            if l.index(x, y) != from {
                continue;
            }

            let (mut a, mut b) = (x, x);
            while a > 0 && l.index(a - 1, y) == from {
                a -= 1;
            }

            while b + 1 < w && l.index(b + 1, y) == from {
                b += 1;
            }

            for k in a..=b {
                self.set(l, k, y, to);
            }

            changed = Some(changed.map_or([a, y, b + 1, y + 1], |c| [c[0].min(a), c[1].min(y), c[2].max(b + 1), c[3].max(y + 1)]));
            for ny in [y.checked_sub(1), (y + 1 < h).then_some(y + 1)].into_iter().flatten() {
                let mut k = a;
                while k <= b {
                    if l.index(k, ny) == from {
                        todo.push((k, ny));
                        while k <= b && l.index(k, ny) == from {
                            k += 1;
                        }
                    } else {
                        k += 1;
                    }
                }
            }
        }

        changed
    }

    /// The stroke's undo record: the chunks it touched that changed (none when nothing did).
    pub fn finish(self, l: &CellLayer) -> Option<CellRecord> {
        let rect = self.rect?;
        let chunks: Vec<(u32, ChunkCells, ChunkCells)> = self.before.into_iter()
            .filter_map(|(c, before)| {
                let after = read_chunk(l, c);
                (before != after).then_some((c, before, after))
            })
            .collect();

        (!chunks.is_empty()).then_some(CellRecord { rect, chunks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rgba = [10, 20, 30, 255];
    const B: Rgba = [200, 0, 0, 255];

    #[test]
    fn brush_undo_redo() {
        let mut l = CellLayer::filled(30, 20, false, A);
        let original = l.to_pixels();
        let mut s = CellStroke::default();
        assert_eq!(s.brush(&mut l, 10, 10, 2, true, B), Some([8, 8, 13, 13]));
        assert_eq!(l.colour(10, 8), B);
        assert_eq!(l.colour(8, 8), A, "a round brush leaves the corners");
        let r = s.finish(&l).expect("a record");
        assert_eq!(r.chunks(&l), vec![(1, 1)]);
        let painted = l.to_pixels();
        r.revert(&mut l);
        assert_eq!(l.to_pixels(), original);
        r.apply(&mut l);
        assert_eq!(l.to_pixels(), painted);
    }

    #[test]
    fn flood_stops_at_other_colours() {
        let mut l = CellLayer::filled(12, 9, false, A);
        let mut s = CellStroke::default();
        // A wall of B down column 5 with a gap at row 8.
        s.rect(&mut l, [5, 0, 6, 8], B);
        let mut f = CellStroke::default();
        f.flood(&mut l, 0, 0, [1, 2, 3, 255]);
        assert_eq!(l.colour(11, 0), [1, 2, 3, 255], "through the gap");
        let mut l2 = CellLayer::filled(12, 9, false, A);
        let mut s2 = CellStroke::default();
        s2.rect(&mut l2, [5, 0, 6, 9], B);
        let mut f2 = CellStroke::default();
        f2.flood(&mut l2, 0, 0, [1, 2, 3, 255]);
        assert_eq!(l2.colour(11, 0), A, "the wall holds");
        assert_eq!(l2.colour(4, 8), [1, 2, 3, 255]);
        assert_eq!(l2.colour(5, 3), B);
    }
}
