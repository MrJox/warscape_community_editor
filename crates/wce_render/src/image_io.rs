//! Reading textures back to the CPU, and saving them as PNG.

use std::path::Path;

use wce_d3d11::*;
use wce_formats::png_file::PngFile;

/// The first mip of an RGBA8 texture (UNORM or sRGB) as tightly packed rows, top row first. Waits for the GPU.
pub fn read_rgba8(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D) -> Option<Vec<u8>> {
    let mut d = tex.desc();
    if d.Format != DXGI_FORMAT_R8G8B8A8_UNORM && d.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB {
        return None;
    }

    d.Usage = D3D11_USAGE_STAGING;
    d.BindFlags = 0;
    d.CPUAccessFlags = D3D11_CPU_ACCESS_READ as u32;
    d.MiscFlags = 0;
    d.MipLevels = 1;
    let staging = dev.create_texture_2d(&d, None).ok()?;
    ctx.copy_subresource_region(&staging, 0, [0; 3], tex, 0, None);
    let ms = ctx.map(&staging, 0, D3D11_MAP_READ, 0).ok()?;
    let row = d.Width as usize * 4;
    let mut rgba = Vec::with_capacity(row * d.Height as usize);
    for y in 0..d.Height as usize {
        rgba.extend_from_slice(ms.row(y, row)?);
    }

    Some(rgba)
}

/// Saves an RGBA8 texture (e.g. the swap chain's back buffer) as an RGB PNG written with the kit's encoder (alpha dropped).
pub fn save_texture_png(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, path: &Path) -> bool {
    let Some(rgba) = read_rgba8(dev, ctx, tex) else { return false };
    let d = tex.desc();
    let png = PngFile {
        width: d.Width,
        height: d.Height,
        bit_depth: 8,
        color_type: 2,
        pixels: rgba.as_chunks::<4>().0.iter().flat_map(|px| [px[0], px[1], px[2]]).collect(),
        ..Default::default()
    };

    png.encode().is_ok_and(|bytes| std::fs::write(path, bytes).is_ok())
}
