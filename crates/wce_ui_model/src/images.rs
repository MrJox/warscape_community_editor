//! Decoding the pictures a layout names: PNG and TGA through the `image` crate, DDS through `wce_formats`.

pub use wce_formats::dds_decode::Rgba8Image;

/// Decodes by the extension of `path` (`png`, `tga`, `dds`), falling back to sniffing the bytes.
///
/// # Errors
///
/// Why the bytes are not an image this can read.
pub fn decode(path: &str, bytes: &[u8]) -> Result<Rgba8Image, String> {
    let lower = path.to_lowercase();
    if lower.ends_with(".dds") || bytes.starts_with(b"DDS ") {
        return wce_formats::dds_decode::dds_decode_mip(bytes, 0).map_err(|e| e.to_string());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Rgba8Image { w: img.width(), h: img.height(), px: img.into_raw() })
}

/// The paths a layout's picture can be stored under: the same name with the other picture extensions.
pub fn candidates(path: &str) -> Vec<String> {
    let base = path.rsplit_once('.').map(|(b, _)| b).unwrap_or(path);
    let mut v = vec![path.to_string()];
    for ext in ["png", "tga", "dds"] {
        let c = format!("{base}.{ext}");
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}
