//! A DDS file (DXT1 / DXT3 / DXT5, 32-bit RGBA / BGRA) as a downscaled PNG to look at: its top mip box-filtered to at most `size` pixels
//! on its longer side (RGB; with `alpha`, the alpha channel as grey instead).
//!
//!   cargo run --release -p wce_terrain_io --example dds_preview -- <in.dds> <out.png> [size] [alpha]

use wce_formats::dds_decode::dds_decode_mip;
use wce_formats::png_file::PngFile;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(src), Some(dst)) = (a.first(), a.get(1)) else {
        eprintln!("usage: dds_preview <in.dds> <out.png> [size] [alpha]");
        return;
    };
    let size: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let alpha = a.get(3).is_some_and(|s| s == "alpha");
    let bytes = std::fs::read(src).expect("the dds");
    let img = dds_decode_mip(&bytes, 0).expect("a decodable dds");
    let k = img.w.max(img.h).div_ceil(size).max(1);
    let (w, h) = (img.w / k, img.h / k);
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let p = img.at(x * k + dx, y * k + dy);
                    for c in 0..4 {
                        sum[c] += u32::from(p[c]);
                    }
                }
            }
            let n = k * k;
            if alpha {
                let v = (sum[3] / n) as u8;
                rgb.extend_from_slice(&[v, v, v]);
            } else {
                rgb.extend_from_slice(&[(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]);
            }
        }
    }
    let png = PngFile { width: w, height: h, bit_depth: 8, color_type: 2, pixels: rgb, ..Default::default() };
    std::fs::write(dst, png.encode().expect("png")).expect("the png");
    println!("{} x {} -> {w} x {h}", img.w, img.h);
}
