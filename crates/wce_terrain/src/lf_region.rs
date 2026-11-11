//! A bounded region in the LF file's sample coordinates (x right, y down, end exclusive).

use crate::paged::PagedU16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LfRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl LfRegion {
    pub fn validate(self, field: &PagedU16) -> Result<[u32; 4], String> {
        wce_formats::lf_region_png::sample_count(self.width, self.height).map_err(|e| e.0)?;
        let x1 = self
            .x
            .checked_add(self.width)
            .filter(|&x| x <= field.width());
        let y1 = self
            .y
            .checked_add(self.height)
            .filter(|&y| y <= field.height());
        match (x1, y1) {
            (Some(x1), Some(y1)) => Ok([self.x, self.y, x1, y1]),
            _ => Err(
                "LF region is outside the field (file sample coordinates, top row first)".into(),
            ),
        }
    }

    pub fn read(self, field: &PagedU16) -> Result<Vec<u16>, String> {
        let [x0, y0, x1, y1] = self.validate(field)?;
        let grid = field.grid().read();
        let mut samples = Vec::with_capacity(self.width as usize * self.height as usize);
        let w = field.width() as usize;
        for y in y0..y1 {
            samples.extend_from_slice(
                &grid[y as usize * w + x0 as usize..y as usize * w + x1 as usize],
            );
        }
        Ok(samples)
    }
}
