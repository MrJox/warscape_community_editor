//! I420 <-> RGBA, BT.601 video range (**ASSUMPTION**: the matrix and range of the game's movies, as VP8's default).

use crate::vp8::I420;

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// RGBA, 8 bits per channel, alpha 255.
pub fn to_rgba(p: &I420) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let cw = p.width.div_ceil(2) as usize;
    let mut out = vec![0u8; w * h * 4];
    for row in 0..h {
        for col in 0..w {
            let y = p.y[row * w + col] as i32 - 16;
            let ci = (row / 2) * cw + col / 2;
            let u = p.u[ci] as i32 - 128;
            let v = p.v[ci] as i32 - 128;
            // Fixed point, 8 fractional bits: 1.164, 1.596, 0.391, 0.813, 2.018.
            let c = 298 * y + 128;
            let o = (row * w + col) * 4;
            out[o] = clamp((c + 409 * v) >> 8);
            out[o + 1] = clamp((c - 100 * u - 208 * v) >> 8);
            out[o + 2] = clamp((c + 516 * u) >> 8);
            out[o + 3] = 255;
        }
    }
    out
}

/// I420 from RGBA (alpha ignored); chroma is the average of each 2 x 2 block.
pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> I420 {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = I420::chroma_size(width, height);
    let (cw, ch) = (cw as usize, ch as usize);
    let mut y = vec![0u8; w * h];
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    for row in 0..h {
        for col in 0..w {
            let o = (row * w + col) * 4;
            let (r, g, b) = (rgba[o] as i32, rgba[o + 1] as i32, rgba[o + 2] as i32);
            y[row * w + col] = clamp(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16);
        }
    }
    for crow in 0..ch {
        for ccol in 0..cw {
            let (mut r, mut g, mut b, mut n) = (0i32, 0i32, 0i32, 0i32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let (row, col) = (crow * 2 + dy, ccol * 2 + dx);
                    if row < h && col < w {
                        let o = (row * w + col) * 4;
                        r += rgba[o] as i32;
                        g += rgba[o + 1] as i32;
                        b += rgba[o + 2] as i32;
                        n += 1;
                    }
                }
            }
            let (r, g, b) = (r / n, g / n, b / n);
            u[crow * cw + ccol] = clamp(((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128);
            v[crow * cw + ccol] = clamp(((112 * r - 94 * g - 18 * b + 128) >> 8) + 128);
        }
    }
    I420 { width, height, y, u, v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_close() {
        let (w, h) = (8u32, 6u32);
        let mut rgba = vec![];
        for i in 0..(w * h) {
            // Flat 2 x 2 blocks, so chroma subsampling loses nothing.
            let (x, y) = (i % w / 2, i / w / 2);
            rgba.extend_from_slice(&[(x * 60) as u8, (y * 80) as u8, 200, 255]);
        }
        let back = to_rgba(&from_rgba(w, h, &rgba));
        for (a, b) in rgba.iter().zip(&back) {
            assert!((*a as i32 - *b as i32).abs() <= 3, "{a} vs {b}");
        }
    }
}
