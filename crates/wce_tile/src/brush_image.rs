//! An image brush (TEd's ImageBrush, Load Brush): a picture whose brightness is the brush's weight, stretched over the square the brush's
//! radius spans and turned by an angle. PNG (grey, grey + alpha, RGB, RGBA; 8 or 16 bit) or DDS (what the DDS decoder reads); the weight
//! is the luminance times the alpha.

use std::path::Path;

use wce_formats::dds_decode::dds_decode_mip;
use wce_formats::png_file::PngFile;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq)]
pub struct BrushImage {
    /// The file it was read from.
    pub path: String,
    pub w: u32,
    pub h: u32,

    /// The weights, 0..1, row by row.
    pub v: Vec<f32>,
}

impl BrushImage {

    /// # Errors
    ///
    /// The file does not read, or is not a PNG or DDS the decoders read.
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut image = if bytes.starts_with(b"DDS ") { Self::from_dds(&bytes)? } else { Self::from_png(&bytes)? };
        image.path = path.display().to_string();
        Ok(image)
    }

    fn from_dds(bytes: &[u8]) -> Result<Self> {
        let im = dds_decode_mip(bytes, 0).map_err(str::to_owned)?;
        let v = im.px.as_chunks::<4>().0.iter().map(|p| luminance(f32::from(p[0]), f32::from(p[1]), f32::from(p[2])) / 255.0 * f32::from(p[3]) / 255.0).collect();
        Ok(Self { path: String::new(), w: im.w, h: im.h, v })
    }

    fn from_png(bytes: &[u8]) -> Result<Self> {
        let png = PngFile::parse(bytes).map_err(|e| e.to_string())?;
        let channels = match png.color_type {
            0 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => return Err("a brush image must be a grey, grey + alpha, RGB or RGBA PNG (not palette)".to_owned()),
        };

        let (n, deep) = (png.width as usize * png.height as usize, png.bit_depth == 16);
        if png.bit_depth != 8 && !deep {
            return Err(format!("a brush image must be 8 or 16 bit, not {}", png.bit_depth));
        }

        let size = if deep { 2 } else { 1 };
        if png.pixels.len() < n * channels * size {
            return Err("the PNG is truncated".to_owned());
        }

        let sample = |i: usize| -> f32 {
            if deep { f32::from(u16::from_be_bytes([png.pixels[i * 2], png.pixels[i * 2 + 1]])) / 65535.0 } else { f32::from(png.pixels[i]) / 255.0 }
        };

        let v = (0..n).map(|p| {
            let at = p * channels;
            match channels {
                1 => sample(at),
                2 => sample(at) * sample(at + 1),
                3 => luminance(sample(at), sample(at + 1), sample(at + 2)),
                _ => luminance(sample(at), sample(at + 1), sample(at + 2)) * sample(at + 3),
            }
        }).collect();

        Ok(Self { path: String::new(), w: png.width, h: png.height, v })
    }

    /// The weight at (u, v), 0..1 across the image (v down its rows), bilinear; 0 outside.
    pub fn sample(&self, u: f64, v: f64) -> f64 {
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) || self.w == 0 || self.h == 0 {
            return 0.0;
        }

        let x = (u * f64::from(self.w) - 0.5).clamp(0.0, f64::from(self.w - 1));
        let y = (v * f64::from(self.h) - 0.5).clamp(0.0, f64::from(self.h - 1));
        let (x0, y0) = (x as u32, y as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - f64::from(x0), y - f64::from(y0));
        let at = |x: u32, y: u32| f64::from(self.v[y as usize * self.w as usize + x as usize]);
        let a = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
        let b = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
        a * (1.0 - fy) + b * fy
    }

    /// The weight at an offset (any unit) from the brush's centre, the image spanning `radius` each side and turned by `angle` (radians).
    pub fn at(&self, dx: f64, dy: f64, radius: f64, angle: f64) -> f64 {
        if radius <= 0.0 {
            return 0.0;
        }

        let (s, c) = angle.sin_cos();
        let (rx, ry) = (dx * c + dy * s, -dx * s + dy * c);
        self.sample(rx / radius * 0.5 + 0.5, ry / radius * 0.5 + 0.5)
    }
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_reads_as_weights_and_samples_turned() {
        // 2 x 1 grey: black, white.
        let png = PngFile { width: 2, height: 1, bit_depth: 8, color_type: 0, pixels: vec![0, 255], pixels_edited: true, ..Default::default() };
        let im = BrushImage::from_png(&png.encode().unwrap()).unwrap();
        assert_eq!(im.v, vec![0.0, 1.0]);
        assert!(im.at(0.9, 0.0, 1.0, 0.0) > 0.99, "the right half is white");
        assert!(im.at(-0.9, 0.0, 1.0, 0.0) < 0.01);
        assert!(im.at(-0.9, 0.0, 1.0, std::f64::consts::PI) > 0.99, "turned half a turn, the left is");
        assert_eq!(im.at(1.5, 0.0, 1.0, 0.0), 0.0, "outside the square");
    }
}
