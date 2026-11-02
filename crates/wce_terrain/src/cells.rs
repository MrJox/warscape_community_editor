//! A map's painted cell layers (`tile_map.png`, `climate_map.png`, `culture_map.png`, the snow maps, season masks, the layout map): one
//! colour per cell, kept as an index into the layer's palette (every colour the image has, so a layer written back is the same image).
//! Tile sets, tiles, variations and climates are colours (docs/re/battle_maps.md), so the palette is what the user paints with.

use std::collections::HashMap;

/// A colour: RGBA (alpha 255 for an RGB image).
pub type Rgba = [u8; 4];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellLayer {
    w: u32,
    h: u32,

    /// The image has an alpha channel (RGBA rather than RGB).
    pub alpha: bool,

    palette: Vec<Rgba>,
    lookup: HashMap<Rgba, u16>,

    /// A palette index per cell, row after row (row 0 is the image's top).
    cells: Vec<u16>,
}

impl CellLayer {

    /// The layer of an 8-bit RGB or RGBA image's pixels (`channels` 3 or 4, rows top first).
    ///
    /// # Errors
    ///
    /// Another channel count, too few pixels, or more than 65 536 colours.
    pub fn from_pixels(w: u32, h: u32, channels: usize, px: &[u8]) -> Result<Self, String> {
        if channels != 3 && channels != 4 {
            return Err(format!("cell layer: {channels} channels"));
        }

        let n = w as usize * h as usize;
        if px.len() < n * channels {
            return Err("cell layer: too few pixels".to_owned());
        }

        let mut l = Self { w, h, alpha: channels == 4, cells: Vec::with_capacity(n), ..Default::default() };
        for p in px.chunks_exact(channels).take(n) {
            let c = [p[0], p[1], p[2], if channels == 4 { p[3] } else { 255 }];
            let i = l.index_of(c).ok_or("cell layer: more than 65 536 colours")?;
            l.cells.push(i);
        }

        Ok(l)
    }

    /// A layer of `w` x `h` cells of one colour.
    pub fn filled(w: u32, h: u32, alpha: bool, colour: Rgba) -> Self {
        let mut l = Self { w, h, alpha, ..Default::default() };
        let i = l.index_of(colour).expect("an empty palette has room");
        l.cells = vec![i; w as usize * h as usize];
        l
    }

    /// The colour's palette index, added when new; none when the palette is full.
    pub fn index_of(&mut self, c: Rgba) -> Option<u16> {
        if let Some(&i) = self.lookup.get(&c) {
            return Some(i);
        }

        let i = u16::try_from(self.palette.len()).ok()?;
        self.palette.push(c);
        self.lookup.insert(c, i);
        Some(i)
    }

    /// The pixels (rows top first, 3 or 4 channels as the layer was read).
    pub fn to_pixels(&self) -> Vec<u8> {
        let ch = if self.alpha { 4 } else { 3 };
        let mut out = Vec::with_capacity(self.cells.len() * ch);
        for &i in &self.cells {
            out.extend_from_slice(&self.palette[usize::from(i)][..ch]);
        }

        out
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn palette(&self) -> &[Rgba] {
        &self.palette
    }

    pub fn cells(&self) -> &[u16] {
        &self.cells
    }

    /// The colour of cell (x, y).
    pub fn colour(&self, x: u32, y: u32) -> Rgba {
        self.palette[usize::from(self.cells[(y * self.w + x) as usize])]
    }

    /// Paints cell (x, y); false when the palette is full.
    pub fn paint(&mut self, x: u32, y: u32, c: Rgba) -> bool {
        let Some(i) = self.index_of(c) else { return false };
        self.cells[(y * self.w + x) as usize] = i;
        true
    }

    /// The palette indices in use, with their cell counts.
    pub fn histogram(&self) -> Vec<(u16, usize)> {
        let mut n = vec![0usize; self.palette.len()];
        for &i in &self.cells {
            n[usize::from(i)] += 1;
        }

        n.into_iter().enumerate().filter(|(_, c)| *c > 0).map(|(i, c)| (i as u16, c)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_round_trip() {
        let px = [90, 118, 71, 150, 75, 50, 90, 118, 71, 0, 0, 0];
        let l = CellLayer::from_pixels(2, 2, 3, &px).expect("rgb");
        assert_eq!(l.palette().len(), 3);
        assert_eq!(l.to_pixels(), px);
        assert_eq!(l.colour(0, 1), [90, 118, 71, 255]);
    }

    #[test]
    fn paint_adds_colours() {
        let mut l = CellLayer::filled(3, 1, true, [1, 2, 3, 4]);
        assert!(l.paint(2, 0, [9, 9, 9, 9]));
        assert_eq!(l.histogram(), vec![(0, 2), (1, 1)]);
        assert_eq!(l.to_pixels(), [1, 2, 3, 4, 1, 2, 3, 4, 9, 9, 9, 9]);
    }
}
