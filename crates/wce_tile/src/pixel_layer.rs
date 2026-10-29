//! A painted map of a tile (the blend map, the ground type map) as the editor edits it: its samples, `channels` per pixel, with undo records
//! of the 64 x 64 pixel blocks an operation touched (a 64 x 64 tile's blend map is 2.3 GB: no whole copies), and the rectangle changed since
//! the display last took it.

/// Pixels a side of an undo block.
const BLOCK: u32 = 64;

/// The blocks an operation changed: their samples before and after.
#[derive(Clone, Debug, Default, PartialEq)]
struct Record<T> {
    blocks: Vec<(u32, Vec<T>, Vec<T>)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PixelLayer<T> {
    w: u32,
    h: u32,
    channels: usize,
    v: Vec<T>,
    editing: bool,

    /// The open edit's blocks as they were at its start, by block (`block_of`; a flat table: a pixel's lookup is an index, not a hash), and
    /// the blocks it holds.
    saved: Vec<Option<Box<[T]>>>,
    touched: Vec<u32>,
    undo: Vec<Record<T>>,
    redo: Vec<Record<T>>,

    /// The rectangle changed since the display last took it (exclusive end).
    changed: Option<(u32, u32, u32, u32)>,
}

impl<T: Copy + PartialEq> PixelLayer<T> {

    /// `v` holds `channels` samples per pixel, row by row.
    pub fn new(w: u32, h: u32, channels: usize, v: Vec<T>) -> Self {
        debug_assert_eq!(v.len(), w as usize * h as usize * channels);
        Self { w, h, channels, v, editing: false, saved: vec![], touched: vec![], undo: vec![], redo: vec![], changed: None }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The samples of a pixel.
    pub fn pixel(&self, x: u32, y: u32) -> &[T] {
        let i = (y as usize * self.w as usize + x as usize) * self.channels;
        &self.v[i..i + self.channels]
    }

    /// The samples of a pixel as they were when the open edit began (as they are now outside an edit, or where it changed nothing).
    pub fn original(&self, x: u32, y: u32) -> &[T] {
        let block = self.block_of(x, y);
        let Some(saved) = self.saved.get(block as usize).and_then(Option::as_deref) else { return self.pixel(x, y) };
        let (x0, y0, x1, _) = self.block_rect(block);
        let i = ((y - y0) as usize * (x1 - x0) as usize + (x - x0) as usize) * self.channels;
        &saved[i..i + self.channels]
    }

    /// All the samples, row by row.
    pub fn values(&self) -> &[T] {
        &self.v
    }

    /// Replaces a pixel's samples during an edit; false when they were the same.
    ///
    /// # Panics
    ///
    /// No edit is open.
    pub fn set_pixel(&mut self, x: u32, y: u32, samples: &[T]) -> bool {
        assert!(self.editing, "begin_edit() before editing");
        let i = (y as usize * self.w as usize + x as usize) * self.channels;
        if self.v[i..i + self.channels] == *samples {
            return false;
        }

        self.save_block(self.block_of(x, y));
        self.v[i..i + self.channels].copy_from_slice(samples);
        self.mark(x, y, x + 1, y + 1);
        true
    }

    /// Sets the pixels `x0 .. x1` of row `y` to `samples` during an edit; false when they all had them.
    ///
    /// # Panics
    ///
    /// No edit is open.
    pub fn fill_span(&mut self, y: u32, x0: u32, x1: u32, samples: &[T]) -> bool {
        assert!(self.editing, "begin_edit() before editing");
        let (c, row) = (self.channels, y as usize * self.w as usize);
        let (mut lo, mut hi) = (u32::MAX, 0);
        // A block's part of the run at a time: a block is saved only when something in it changes.
        let mut x = x0;
        while x < x1 {
            let end = ((x / BLOCK + 1) * BLOCK).min(x1);
            let run = (row + x as usize) * c..(row + end as usize) * c;
            let differs = |p: &[T]| p != samples;
            if let Some(first) = self.v[run.clone()].chunks_exact(c).position(differs) {
                let last = self.v[run.clone()].chunks_exact(c).rposition(differs).unwrap_or(first);
                self.save_block(self.block_of(x, y));
                for p in self.v[run].chunks_exact_mut(c).take(last + 1).skip(first) {
                    p.copy_from_slice(samples);
                }

                lo = lo.min(x + first as u32);
                hi = x + last as u32 + 1;
            }

            x = end;
        }

        if lo == u32::MAX {
            return false;
        }

        self.mark(lo, y, hi, y + 1);
        true
    }

    /// Keeps a block as it is now, the first time the open edit changes it.
    fn save_block(&mut self, b: u32) {
        if self.saved[b as usize].is_none() {
            self.saved[b as usize] = Some(self.block_samples(b).into_boxed_slice());
            self.touched.push(b);
        }
    }

    fn block_of(&self, x: u32, y: u32) -> u32 {
        (y / BLOCK) * self.w.div_ceil(BLOCK) + x / BLOCK
    }

    fn block_rect(&self, b: u32) -> (u32, u32, u32, u32) {
        let per_row = self.w.div_ceil(BLOCK);
        let (x0, y0) = ((b % per_row) * BLOCK, (b / per_row) * BLOCK);
        (x0, y0, (x0 + BLOCK).min(self.w), (y0 + BLOCK).min(self.h))
    }

    fn block_samples(&self, b: u32) -> Vec<T> {
        let (x0, y0, x1, y1) = self.block_rect(b);
        let mut out = Vec::with_capacity((x1 - x0) as usize * (y1 - y0) as usize * self.channels);
        for y in y0..y1 {
            let i = (y as usize * self.w as usize + x0 as usize) * self.channels;
            out.extend_from_slice(&self.v[i..i + (x1 - x0) as usize * self.channels]);
        }

        out
    }

    fn put_block(&mut self, b: u32, samples: &[T]) {
        let (x0, y0, x1, y1) = self.block_rect(b);
        let n = (x1 - x0) as usize * self.channels;
        for (k, y) in (y0..y1).enumerate() {
            let i = (y as usize * self.w as usize + x0 as usize) * self.channels;
            self.v[i..i + n].copy_from_slice(&samples[k * n..(k + 1) * n]);
        }

        self.mark(x0, y0, x1, y1);
    }

    fn mark(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        self.changed = Some(match self.changed {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }

    /// Whether something changed since `take_changed` was last called.
    pub fn changed(&self) -> bool {
        self.changed.is_some()
    }

    /// The rectangle changed since the last call (exclusive end), `None` when nothing changed.
    pub fn take_changed(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.changed.take()
    }

    /// Marks the start of an operation for undo; false when one is already open.
    pub fn begin_edit(&mut self) -> bool {
        if self.editing {
            return false;
        }

        self.editing = true;
        for b in self.touched.drain(..) {
            self.saved[b as usize] = None;
        }

        let blocks = self.w.div_ceil(BLOCK) as usize * self.h.div_ceil(BLOCK) as usize;
        if self.saved.len() != blocks {
            self.saved = (0..blocks).map(|_| None).collect();
        }

        true
    }

    /// Stores the blocks the open operation changed.
    pub fn end_edit(&mut self) {
        if !self.editing {
            return;
        }

        self.editing = false;
        if self.touched.is_empty() {
            return;
        }

        let mut touched = std::mem::take(&mut self.touched);
        touched.sort_unstable();
        let blocks = touched.into_iter().map(|b| {
            let before = self.saved[b as usize].take().expect("a saved block").into_vec();
            let after = self.block_samples(b);
            (b, before, after)
        }).collect::<Vec<_>>();

        self.undo.push(Record { blocks });
        self.redo.clear();
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn undo(&mut self) -> bool {
        let Some(r) = self.undo.pop() else { return false };
        for (b, before, _) in &r.blocks {
            self.put_block(*b, before);
        }

        self.redo.push(r);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(r) = self.redo.pop() else { return false };
        for (b, _, after) in &r.blocks {
            self.put_block(*b, after);
        }

        self.undo.push(r);
        true
    }

    pub fn edit_depth(&self) -> usize {
        self.undo.len()
    }

    /// The pixels under a brush (centre and radius in pixels), clipped to the map: (x, y, offset from the centre).
    pub fn under(&self, cx: f64, cy: f64, radius: f64) -> impl Iterator<Item = (u32, u32, f64, f64)> + use<T> {
        let (x0, y0, x1, y1) = self.under_bounds(cx, cy, radius);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x as u32, y as u32, x as f64 - cx, y as f64 - cy)))
    }

    /// The square of pixels `under` walks (x0, y0, x1, y1; ends included), empty (an end before its start) off the map.
    pub fn under_bounds(&self, cx: f64, cy: f64, radius: f64) -> (i64, i64, i64, i64) {
        let x0 = ((cx - radius).floor() as i64).max(0);
        let x1 = ((cx + radius).ceil() as i64).min(i64::from(self.w) - 1);
        let y0 = ((cy - radius).floor() as i64).max(0);
        let y1 = ((cy + radius).ceil() as i64).min(i64::from(self.h) - 1);
        (x0, y0, x1, y1)
    }
}
