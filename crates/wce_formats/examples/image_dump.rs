//! Differential dump of the image round trips with the rules of the C++ format registry (PNG: decode and re-encode with the kit's
//! encoder; DDS and TIFF: the generated TEd headers where they apply; TGA: verbatim), for comparing with the C++ code.
//!
//!   image_dump <file with one path per line>

use std::io::{BufWriter, Write};

use wce_formats::image_containers::{DdsFile, TgaFile, TiffFile};
use wce_formats::png_file::PngFile;

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xCBF2_9CE4_8422_2325, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

fn describe_diff(a: &[u8], b: &[u8]) -> String {
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    format!("first difference at byte {i}, sizes {}/{}", a.len(), b.len())
}

/// The format's name, its output, and an error.
fn round_trip(path: &str, bytes: &[u8]) -> Option<(&'static str, Vec<u8>, String)> {
    let lower = path.to_ascii_lowercase();
    let verdict = |output: Vec<u8>| {
        let error = if output == bytes { String::new() } else { describe_diff(bytes, &output) };
        (output, error)
    };

    if lower.ends_with(".dds") && bytes.starts_with(b"DDS ") {
        let (output, error) = match DdsFile::parse(bytes) {
            Err(error) => (vec![], format!("parse: {error}")),
            Ok(d) if d.flags() == 0 && d.fourcc() == 114 && d.pixel_format_flags() == 4 && !d.is_r32f_template() => {
                (vec![], "R32F header differs from the generated one".to_owned())
            }

            Ok(d) if d.flags() == 0 && d.fourcc() == 114 && d.pixel_format_flags() == 4 => verdict(DdsFile::make_r32f(d.width(), d.height(), d.payload).write()),
            Ok(d) => verdict(d.write()),
        };

        return Some(("dds", output, error));
    }

    if lower.ends_with(".tga") {
        let (output, error) = TgaFile::parse(bytes).map_or_else(|error| (vec![], format!("parse: {error}")), |t| verdict(t.write()));
        return Some(("tga", output, error));
    }

    if lower.ends_with(".tif") && bytes.starts_with(b"II*\0") {
        let (output, error) = match TiffFile::parse(bytes) {
            Err(error) => (vec![], format!("parse: {error}")),
            Ok(t) if t.ted_layout && !t.header_is_regenerable() => (vec![], "TEd-layout header differs from the generated one".to_owned()),
            Ok(mut t) => {
                if t.ted_layout {
                    t.header = TiffFile::ted_header(t.width, t.height);
                }

                verdict(t.write())
            }
        };

        return Some(("tiff", output, error));
    }

    if (path.ends_with(".png") || path.ends_with(".PNG")) && bytes.len() >= 8 && bytes[1] == b'P' {
        let (output, error) = match PngFile::parse(bytes).and_then(|p| Ok((p.encode()?, p.has_ancillary))) {
            Err(error) => (vec![], format!("parse: {error}")),
            Ok((output, _)) if output == bytes => (output, String::new()),
            Ok((output, ancillary)) => {
                let why = if ancillary { "ancillary chunks (not kit encoder); " } else { "encoder differs; " };
                let error = format!("{why}{}", describe_diff(bytes, &output));
                (output, error)
            }
        };

        return Some(("png", output, error));
    }

    None
}

fn main() -> std::io::Result<()> {
    let list = std::fs::read_to_string(std::env::args().nth(1).unwrap_or_default())?;
    let mut out = BufWriter::new(std::io::stdout().lock());
    for path in list.lines() {
        let bytes = std::fs::read(path)?;
        match round_trip(path, &bytes) {
            None => writeln!(out, "none {path}")?,
            Some((name, output, error)) => writeln!(out, "{name} {} {:016x} {error} | {path}", i32::from(error.is_empty()), fnv(&output))?,
        }
    }

    Ok(())
}
