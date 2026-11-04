//! A field of 16-bit samples (a map's LF heights): row-major in a shared [`SampleGrid`] (the map's model, its placed map and the scene read
//! the same samples; a campaign map's are a copy-on-write view of its file), worked on in pages of PAGE x PAGE samples: a brush's dab
//! touches a few, the unit of undo (docs/plans/terrain_editor.md, "Data structures"). The samples are the file's own values: a field read
//! and written back is the same bytes.
//!
//! A clone is a copy (its own samples); [`PagedU16::grid`] is the shared samples.

use std::sync::Arc;

use rayon::prelude::*;
use wce_foundation::samples::SampleGrid;

/// Samples a side of a page.
pub const PAGE: u32 = 64;

pub const PAGE_LEN: usize = (PAGE * PAGE) as usize;

pub struct PagedU16 {
    grid: Arc<SampleGrid>,

    /// Pages across and down.
    pw: u32,
    ph: u32,
}

impl Clone for PagedU16 {
    fn clone(&self) -> Self {
        Self::from_grid(Arc::new(self.grid.snapshot()))
    }
}

impl Default for PagedU16 {
    fn default() -> Self {
        Self::from_grid(Arc::new(SampleGrid::new(0, 0, vec![])))
    }
}

impl PartialEq for PagedU16 {
    fn eq(&self, o: &Self) -> bool {
        self.width() == o.width() && self.height() == o.height() && (Arc::ptr_eq(&self.grid, &o.grid) || *self.grid.read() == *o.grid.read())
    }
}

impl Eq for PagedU16 {}

impl std::fmt::Debug for PagedU16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PagedU16({:?})", self.grid)
    }
}

impl PagedU16 {

    /// A field over shared samples.
    pub fn from_grid(grid: Arc<SampleGrid>) -> Self {
        let (pw, ph) = (grid.width().div_ceil(PAGE), grid.height().div_ceil(PAGE));
        Self { grid, pw, ph }
    }

    /// A field of `w` x `h` samples, all `value`.
    pub fn filled(w: u32, h: u32, value: u16) -> Self {
        Self::from_grid(Arc::new(SampleGrid::new(w, h, vec![value; w as usize * h as usize])))
    }

    /// The field of `w` x `h` little-endian samples, row after row (a TIFF strip; `rows` may be longer).
    ///
    /// # Panics
    ///
    /// `rows` holds fewer than `w * h` samples.
    pub fn from_le_rows(w: u32, h: u32, rows: &[u8]) -> Self {
        let n = w as usize * h as usize;
        assert!(rows.len() >= n * 2, "a sample per pixel");
        let mut v = vec![0u16; n];
        v.par_chunks_mut(65536).zip(rows[..n * 2].par_chunks(131_072)).for_each(|(d, s)| {
            for (d, s) in d.iter_mut().zip(s.as_chunks::<2>().0) {
                *d = u16::from_le_bytes(*s);
            }
        });

        Self::from_grid(Arc::new(SampleGrid::new(w, h, v)))
    }

    /// The samples as little-endian rows (the inverse of [`Self::from_le_rows`]).
    pub fn to_le_rows(&self) -> Vec<u8> {
        let v = self.grid.read();
        let mut out = vec![0u8; v.len() * 2];
        out.par_chunks_mut(131_072).zip(v.par_chunks(65536)).for_each(|(d, s)| {
            for (d, s) in d.as_chunks_mut::<2>().0.iter_mut().zip(s) {
                *d = s.to_le_bytes();
            }
        });

        out
    }

    /// The samples, row by row (a copy).
    pub fn to_vec(&self) -> Vec<u16> {
        self.grid.read().to_vec()
    }

    /// The shared samples.
    pub fn grid(&self) -> &Arc<SampleGrid> {
        &self.grid
    }

    pub fn width(&self) -> u32 {
        self.grid.width()
    }

    pub fn height(&self) -> u32 {
        self.grid.height()
    }

    /// Pages across and down.
    pub fn pages(&self) -> (u32, u32) {
        (self.pw, self.ph)
    }

    /// The sample at (x, y), inside the field.
    pub fn at(&self, x: u32, y: u32) -> u16 {
        debug_assert!(x < self.width() && y < self.height());
        self.grid.read()[(y * self.width() + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, value: u16) {
        debug_assert!(x < self.width() && y < self.height());
        let w = self.width();
        self.grid.write()[(y * w + x) as usize] = value;
    }

    /// The page holding sample (x, y) and the sample's place in it.
    pub fn locate(&self, x: u32, y: u32) -> (u32, usize) {
        (y / PAGE * self.pw + x / PAGE, ((y % PAGE) * PAGE + x % PAGE) as usize)
    }

    /// The samples of page `p` (`py * pages across + px`): PAGE rows of PAGE, 0 past the field's edges (a copy).
    pub fn page_at(&self, p: u32) -> Box<[u16]> {
        page_of(&self.grid.read(), self.width(), self.height(), self.pw, p)
    }

    /// Page `p`'s samples set (PAGE rows of PAGE; those past the field's edges ignored).
    pub fn write_page(&mut self, p: u32, page: &[u16]) {
        let (w, h, pw) = (self.width(), self.height(), self.pw);
        write_page_of(&mut self.grid.write(), w, h, pw, p, page);
    }

    /// The memory the samples take when they are in memory (bytes).
    pub fn bytes(&self) -> usize {
        self.width() as usize * self.height() as usize * 2
    }
}

/// Page `p` of row-major samples `w` x `h` (`pw` pages across).
pub fn page_of(v: &[u16], w: u32, h: u32, pw: u32, p: u32) -> Box<[u16]> {
    let (px, py) = (p % pw * PAGE, p / pw * PAGE);
    let mut page = vec![0u16; PAGE_LEN];
    let n = PAGE.min(w.saturating_sub(px)) as usize;
    for y in 0..PAGE.min(h.saturating_sub(py)) {
        let src = ((py + y) * w + px) as usize;
        page[(y * PAGE) as usize..(y * PAGE) as usize + n].copy_from_slice(&v[src..src + n]);
    }

    page.into()
}

/// Page `p` into row-major samples.
pub fn write_page_of(v: &mut [u16], w: u32, h: u32, pw: u32, p: u32, page: &[u16]) {
    let (px, py) = (p % pw * PAGE, p / pw * PAGE);
    let n = PAGE.min(w.saturating_sub(px)) as usize;
    for y in 0..PAGE.min(h.saturating_sub(py)) {
        let dst = ((py + y) * w + px) as usize;
        v[dst..dst + n].copy_from_slice(&page[(y * PAGE) as usize..(y * PAGE) as usize + n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_across_partial_pages() {
        let (w, h) = (130, 67);
        let rows: Vec<u8> = (0..w * h).flat_map(|i| (i as u16).wrapping_mul(7).to_le_bytes()).collect();
        let f = PagedU16::from_le_rows(w, h, &rows);
        assert_eq!(f.pages(), (3, 2));
        assert_eq!(f.at(129, 66), ((129 + 66 * 130) as u16).wrapping_mul(7));
        assert_eq!(f.to_le_rows(), rows);
    }

    #[test]
    fn filled_and_set() {
        let mut f = PagedU16::filled(70, 5, 436);
        assert_eq!(f.at(69, 4), 436);
        f.set(65, 1, 9);
        assert_eq!(f.at(65, 1), 9);
        assert_eq!(f.page_at(1)[(PAGE + 1) as usize], 9);
        let mut page = f.page_at(1);
        page[(PAGE + 2) as usize] = 4;
        f.write_page(1, &page);
        assert_eq!(f.at(66, 1), 4);
    }

    #[test]
    fn a_clone_is_a_copy() {
        let mut f = PagedU16::filled(4, 4, 1);
        let g = f.clone();
        f.set(0, 0, 2);
        assert_eq!(g.at(0, 0), 1);
    }
}
