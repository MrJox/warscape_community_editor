//! A field of 16-bit samples (a map's LF heights) stored in pages of PAGE x PAGE samples, page after page: a brush's rows stay in one
//! page and a dab touches a few, the unit of undo and of GPU updates (docs/plans/terrain_editor.md, "Data structures"). The samples are
//! the file's own values: a field read and written back is the same bytes.

use rayon::prelude::*;

/// Samples a side of a page.
pub const PAGE: u32 = 64;

pub const PAGE_LEN: usize = (PAGE * PAGE) as usize;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PagedU16 {
    w: u32,
    h: u32,

    /// Pages across and down.
    pw: u32,
    ph: u32,

    /// PAGE_LEN samples per page, rows of PAGE; the samples past the field's right and bottom edges are 0.
    v: Vec<u16>,
}

impl PagedU16 {

    /// A field of `w` x `h` samples, all `value`.
    pub fn filled(w: u32, h: u32, value: u16) -> Self {
        let (pw, ph) = (w.div_ceil(PAGE), h.div_ceil(PAGE));
        let mut f = Self { w, h, pw, ph, v: vec![0; pw as usize * ph as usize * PAGE_LEN] };
        f.v.par_chunks_mut(PAGE_LEN).enumerate().for_each(|(p, page)| {
            let (px, py) = (p as u32 % pw * PAGE, p as u32 / pw * PAGE);
            for y in 0..PAGE.min(h.saturating_sub(py)) {
                page[(y * PAGE) as usize..(y * PAGE + PAGE.min(w - px)) as usize].fill(value);
            }
        });

        f
    }

    /// The field of `w` x `h` little-endian samples, row after row (a TIFF strip; `rows` may be longer).
    ///
    /// # Panics
    ///
    /// `rows` holds fewer than `w * h` samples.
    pub fn from_le_rows(w: u32, h: u32, rows: &[u8]) -> Self {
        assert!(rows.len() >= w as usize * h as usize * 2, "a sample per pixel");
        let (pw, ph) = (w.div_ceil(PAGE), h.div_ceil(PAGE));
        let mut f = Self { w, h, pw, ph, v: vec![0; pw as usize * ph as usize * PAGE_LEN] };
        f.v.par_chunks_mut(PAGE_LEN).enumerate().for_each(|(p, page)| {
            let (px, py) = (p as u32 % pw * PAGE, p as u32 / pw * PAGE);
            let n = PAGE.min(w - px) as usize;
            for y in 0..PAGE.min(h - py) {
                let src = ((py + y) as usize * w as usize + px as usize) * 2;
                let dst = &mut page[(y * PAGE) as usize..(y * PAGE) as usize + n];
                for (d, s) in dst.iter_mut().zip(rows[src..src + n * 2].as_chunks::<2>().0) {
                    *d = u16::from_le_bytes(*s);
                }
            }
        });

        f
    }

    /// The samples as little-endian rows (the inverse of [`Self::from_le_rows`]).
    pub fn to_le_rows(&self) -> Vec<u8> {
        let (w, h) = (self.w as usize, self.h as usize);
        let mut out = vec![0u8; w * h * 2];
        out.par_chunks_mut(w * 2).enumerate().for_each(|(y, row)| {
            for (x, d) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *d = self.at(x as u32, y as u32).to_le_bytes();
            }
        });

        out
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// Pages across and down.
    pub fn pages(&self) -> (u32, u32) {
        (self.pw, self.ph)
    }

    fn index(&self, x: u32, y: u32) -> usize {
        ((y / PAGE * self.pw + x / PAGE) as usize) * PAGE_LEN + ((y % PAGE) * PAGE + x % PAGE) as usize
    }

    /// The sample at (x, y), inside the field.
    pub fn at(&self, x: u32, y: u32) -> u16 {
        debug_assert!(x < self.w && y < self.h);
        self.v[self.index(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, value: u16) {
        debug_assert!(x < self.w && y < self.h);
        let i = self.index(x, y);
        self.v[i] = value;
    }

    /// The samples of page (px, py): PAGE rows of PAGE.
    pub fn page(&self, px: u32, py: u32) -> &[u16] {
        let p = (py * self.pw + px) as usize * PAGE_LEN;
        &self.v[p..p + PAGE_LEN]
    }

    /// The page holding sample (x, y) and the sample's place in it.
    pub fn locate(&self, x: u32, y: u32) -> (u32, usize) {
        (y / PAGE * self.pw + x / PAGE, ((y % PAGE) * PAGE + x % PAGE) as usize)
    }

    /// Page `p`'s samples (PAGE rows of PAGE; `p` is `py * pages across + px`).
    pub fn page_at(&self, p: u32) -> &[u16] {
        let o = p as usize * PAGE_LEN;
        &self.v[o..o + PAGE_LEN]
    }

    pub fn page_at_mut(&mut self, p: u32) -> &mut [u16] {
        let o = p as usize * PAGE_LEN;
        &mut self.v[o..o + PAGE_LEN]
    }

    /// The memory the samples take (bytes).
    pub fn bytes(&self) -> usize {
        self.v.len() * 2
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
        assert_eq!(f.page(1, 0)[(PAGE + 1) as usize], 9);
    }
}
