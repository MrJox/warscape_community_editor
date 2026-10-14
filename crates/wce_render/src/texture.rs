//! DDS loading straight into D3D11 (BCn uploaded as it is, no conversion). Game textures: DXT1 / DXT5 (buildings), cube maps
//! (campaign skybox), volumes, arrays and uncompressed layouts.

use std::ffi::c_void;

use wce_d3d11::*;
use windows_core::Interface;

use crate::RenderError;

/// A texture's view and what it is.
#[derive(Clone, Debug, Default)]
pub struct Texture {
    pub srv: Option<ID3D11ShaderResourceView>,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub cube: bool,

    /// More than 1: a volume texture (Texture3D).
    pub depth: u32,
    pub format: DXGI_FORMAT,
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

fn to_srgb(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_BC1_UNORM => DXGI_FORMAT_BC1_UNORM_SRGB,
        DXGI_FORMAT_BC2_UNORM => DXGI_FORMAT_BC2_UNORM_SRGB,
        DXGI_FORMAT_BC3_UNORM => DXGI_FORMAT_BC3_UNORM_SRGB,
        DXGI_FORMAT_BC7_UNORM => DXGI_FORMAT_BC7_UNORM_SRGB,
        DXGI_FORMAT_R8G8B8A8_UNORM => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        DXGI_FORMAT_B8G8R8A8_UNORM => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        DXGI_FORMAT_B8G8R8X8_UNORM => DXGI_FORMAT_B8G8R8X8_UNORM_SRGB,
        _ => f,
    }
}

/// The resource format that allows both the UNORM and the _SRGB view.
fn to_typeless(f: DXGI_FORMAT) -> DXGI_FORMAT {
    match f {
        DXGI_FORMAT_BC1_UNORM => DXGI_FORMAT_BC1_TYPELESS,
        DXGI_FORMAT_BC2_UNORM => DXGI_FORMAT_BC2_TYPELESS,
        DXGI_FORMAT_BC3_UNORM => DXGI_FORMAT_BC3_TYPELESS,
        DXGI_FORMAT_BC7_UNORM => DXGI_FORMAT_BC7_TYPELESS,
        DXGI_FORMAT_R8G8B8A8_UNORM => DXGI_FORMAT_R8G8B8A8_TYPELESS,
        DXGI_FORMAT_B8G8R8A8_UNORM => DXGI_FORMAT_B8G8R8A8_TYPELESS,
        DXGI_FORMAT_B8G8R8X8_UNORM => DXGI_FORMAT_B8G8R8X8_TYPELESS,
        _ => f,
    }
}

fn is_block(f: DXGI_FORMAT) -> bool {
    (DXGI_FORMAT_BC1_TYPELESS..=DXGI_FORMAT_BC5_SNORM).contains(&f) || (DXGI_FORMAT_BC6H_TYPELESS..=DXGI_FORMAT_BC7_UNORM_SRGB).contains(&f)
}

fn block_bytes(f: DXGI_FORMAT) -> usize {
    match f {
        DXGI_FORMAT_BC1_UNORM | DXGI_FORMAT_BC1_UNORM_SRGB | DXGI_FORMAT_BC4_UNORM | DXGI_FORMAT_BC4_SNORM => 8,
        _ => 16,
    }
}

fn pixel_bytes(f: DXGI_FORMAT) -> usize {
    match f {
        DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8X8_UNORM | DXGI_FORMAT_R16G16_UNORM | DXGI_FORMAT_R32_FLOAT
        | DXGI_FORMAT_R10G10B10A2_UNORM => 4,
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_UNORM => 8,
        DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
        DXGI_FORMAT_R8_UNORM | DXGI_FORMAT_A8_UNORM => 1,
        DXGI_FORMAT_R8G8_UNORM | DXGI_FORMAT_R16_FLOAT | DXGI_FORMAT_B5G6R5_UNORM | DXGI_FORMAT_B5G5R5A1_UNORM => 2,
        _ => 0,
    }
}

fn from_masks(bits: u32, r: u32, g: u32, b: u32, a: u32) -> DXGI_FORMAT {
    match (bits, r, g, b) {
        (32, 0xFF, 0xFF00, 0xFF_0000) => DXGI_FORMAT_R8G8B8A8_UNORM,
        (32, 0xFF_0000, 0xFF00, 0xFF) => if a != 0 { DXGI_FORMAT_B8G8R8A8_UNORM } else { DXGI_FORMAT_B8G8R8X8_UNORM },
        (32, 0xFFFF, 0xFFFF_0000, _) => DXGI_FORMAT_R16G16_UNORM,
        (16, 0xF800, 0x7E0, 0x1F) => DXGI_FORMAT_B5G6R5_UNORM,
        (8, _, _, _) if r == 0xFF || a == 0xFF => if r != 0 { DXGI_FORMAT_R8_UNORM } else { DXGI_FORMAT_A8_UNORM },
        _ => DXGI_FORMAT_UNKNOWN,
    }
}

/// The layout of a DDS file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DdsLayout {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mips: u32,
    pub array: u32,
    pub faces: u32,
    pub fmt: DXGI_FORMAT,

    /// Offset of the first mip.
    pub data: usize,
    pub cube: bool,
    pub volume: bool,
}

fn fail<T>(what: &str) -> Result<T, RenderError> {
    Err(RenderError(what.to_owned()))
}

/// Reads the layout of a DDS file from its first bytes (128, or 148 with a DX10 header).
///
/// # Errors
///
/// Fails for a file that isn't a DDS, is truncated, or has a format the renderer doesn't support.
pub fn parse_dds_layout(b: &[u8]) -> Result<DdsLayout, RenderError> {
    let u32_at = |o: usize| match b.get(o..o + 4) {
        Some(v) => Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        None => fail("dds: truncated header"),
    };

    if b.len() < 128 || u32_at(0)? != fourcc(b"DDS ") {
        return fail("dds: bad magic");
    }

    let mut l = DdsLayout {
        height: u32_at(12)?,
        width: u32_at(16)?,
        depth: u32_at(24)?,
        mips: u32_at(28)?.max(1),
        array: 1,
        faces: 1,
        fmt: DXGI_FORMAT_UNKNOWN,
        data: 128,
        cube: false,
        volume: false,
    };

    let (pf_flags, pf_fourcc, bits, caps2) = (u32_at(80)?, u32_at(84)?, u32_at(88)?, u32_at(112)?);
    l.cube = caps2 & 0x200 != 0;
    l.volume = caps2 & 0x20_0000 != 0 && l.depth > 1;
    if pf_flags & 0x4 != 0 {
        l.fmt = match pf_fourcc {
            x if x == fourcc(b"DXT1") => DXGI_FORMAT_BC1_UNORM,
            x if x == fourcc(b"DXT2") || x == fourcc(b"DXT3") => DXGI_FORMAT_BC2_UNORM,
            x if x == fourcc(b"DXT4") || x == fourcc(b"DXT5") => DXGI_FORMAT_BC3_UNORM,
            x if x == fourcc(b"ATI1") || x == fourcc(b"BC4U") => DXGI_FORMAT_BC4_UNORM,
            x if x == fourcc(b"ATI2") || x == fourcc(b"BC5U") => DXGI_FORMAT_BC5_UNORM,
            113 => DXGI_FORMAT_R16G16B16A16_FLOAT,
            116 => DXGI_FORMAT_R32G32B32A32_FLOAT,
            x if x == fourcc(b"DX10") => {
                let dimension = u32_at(132)?;
                if dimension == 3 {
                    l.array = u32_at(140)?.max(1);
                }

                if dimension == 4 {
                    l.volume = l.depth > 1;
                }

                if u32_at(136)? & 0x4 != 0 {
                    l.cube = true;
                }

                l.data = 148;
                u32_at(128)? as DXGI_FORMAT
            }

            _ => return fail("dds: unsupported fourcc"),
        };
    } else {
        l.fmt = from_masks(bits, u32_at(92)?, u32_at(96)?, u32_at(100)?, u32_at(104)?);
        if l.fmt == DXGI_FORMAT_UNKNOWN {
            return fail("dds: unsupported uncompressed layout");
        }
    }

    if !l.volume {
        l.depth = 1;
    }

    l.faces = if l.cube { 6 * l.array } else { l.array };
    Ok(l)
}

/// The row pitch and size of a mip.
fn mip_size(l: &DdsLayout, w: u32, h: u32) -> Result<(usize, usize), RenderError> {
    if is_block(l.fmt) {
        let pitch = (w.div_ceil(4).max(1)) as usize * block_bytes(l.fmt);
        Ok((pitch, pitch * h.div_ceil(4).max(1) as usize))
    } else {
        let pb = pixel_bytes(l.fmt);
        if pb == 0 {
            return fail("dds: unsupported format");
        }

        let pitch = w as usize * pb;
        Ok((pitch, pitch * h as usize))
    }
}

fn srv_desc(format: DXGI_FORMAT, dimension: D3D_SRV_DIMENSION, mips: u32, array: u32) -> D3D11_SHADER_RESOURCE_VIEW_DESC {
    let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: format, ViewDimension: D3D11_SRV_DIMENSION(dimension), ..Default::default() };
    match dimension {
        D3D11_SRV_DIMENSION_TEXTURE3D => sd.Anonymous.Texture3D.MipLevels = mips,
        D3D11_SRV_DIMENSION_TEXTURECUBE => sd.Anonymous.TextureCube.MipLevels = mips,
        D3D11_SRV_DIMENSION_TEXTURE2DARRAY => {
            sd.Anonymous.Texture2DArray.MipLevels = mips;
            sd.Anonymous.Texture2DArray.ArraySize = array;
        }

        _ => sd.Anonymous.Texture2D.MipLevels = mips,
    }

    sd
}

/// Builds a texture from `data` (the file's bytes from `data_offset` on), leaving out the first `skip` mips of every face.
fn build_dds(dev: &ID3D11Device, l: &DdsLayout, data: &[u8], data_offset: usize, skip: u32, srgb: bool) -> Result<Texture, RenderError> {
    let mut subs = vec![];
    let mut p = l.data;
    for _ in 0..l.faces {
        let (mut w, mut h, mut d) = (l.width, l.height, l.depth);
        for m in 0..l.mips {
            let (pitch, size) = mip_size(l, w, h)?;
            if m >= skip {
                if p < data_offset || p + size * d as usize - data_offset > data.len() {
                    return fail("dds: truncated data");
                }

                subs.push(D3D11_SUBRESOURCE_DATA { pSysMem: data[p - data_offset..].as_ptr().cast(), SysMemPitch: pitch as u32, SysMemSlicePitch: size as u32 });
            }

            p += size * d as usize;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            d = (d / 2).max(1);
        }
    }

    let (width, height, mips) = ((l.width >> skip).max(1), (l.height >> skip).max(1), l.mips - skip);
    let resource_format = if srgb { to_typeless(l.fmt) } else { l.fmt };
    let format = if srgb { to_srgb(l.fmt) } else { l.fmt };
    let mut t = Texture { width, height, mips, depth: 1, format, ..Default::default() };

    // SAFETY: the subresources point into `data`, which outlives the calls, with the pitches and sizes of the description.
    unsafe {
        if l.volume {
            let vd = D3D11_TEXTURE3D_DESC {
                Width: width, Height: height, Depth: l.depth, MipLevels: mips, Format: resource_format, Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
            };

            let mut tex = None;
            if dev.CreateTexture3D(&vd, Some(subs.as_ptr()), Some(&mut tex)).is_err() {
                return fail("dds: CreateTexture3D failed");
            }

            t.depth = l.depth;
            let sd = srv_desc(format, D3D11_SRV_DIMENSION_TEXTURE3D, mips, 1);
            let resource = tex.as_ref().map(|tex| tex.cast::<ID3D11Resource>()).transpose()?;
            if dev.CreateShaderResourceView(resource.as_ref(), Some(&sd), Some(&mut t.srv)).is_err() {
                return fail("dds: CreateShaderResourceView failed");
            }

            return Ok(t);
        }

        let td = D3D11_TEXTURE2D_DESC {
            Width: width, Height: height, MipLevels: mips, ArraySize: l.faces, Format: resource_format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32,
            CPUAccessFlags: 0, MiscFlags: if l.cube { D3D11_RESOURCE_MISC_TEXTURECUBE as u32 } else { 0 },
        };

        let mut tex = None;
        if dev.CreateTexture2D(&td, Some(subs.as_ptr()), Some(&mut tex)).is_err() {
            return fail("dds: CreateTexture2D failed");
        }

        t.cube = l.cube;
        let dimension = if l.cube {
            D3D11_SRV_DIMENSION_TEXTURECUBE
        } else if l.array > 1 {
            D3D11_SRV_DIMENSION_TEXTURE2DARRAY
        } else {
            D3D11_SRV_DIMENSION_TEXTURE2D
        };

        let sd = srv_desc(format, dimension, mips, l.array);
        let resource = tex.as_ref().map(|tex| tex.cast::<ID3D11Resource>()).transpose()?;
        if dev.CreateShaderResourceView(resource.as_ref(), Some(&sd), Some(&mut t.srv)).is_err() {
            return fail("dds: CreateShaderResourceView failed");
        }
    }

    Ok(t)
}

/// Loads a DDS file. `srgb`: the view uses the _SRGB variant of the format (colour maps; the engine samples diffuse and specular
/// colour maps as sRGB, an assumption pending a capture check).
///
/// # Errors
///
/// Fails for an unsupported or truncated file, or if the device refuses the texture.
pub fn load_dds(dev: &ID3D11Device, bytes: &[u8], srgb: bool) -> Result<Texture, RenderError> {
    let l = parse_dds_layout(bytes)?;
    build_dds(dev, &l, bytes, 0, 0, srgb)
}

/// Mip streaming: DDS files store the largest mip first, so the small ones sit at the end of the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DdsStreamPlan {
    /// False for what is not worth it or not supported (cube maps, volumes, arrays, small textures): load those whole.
    pub ok: bool,

    /// Mips not loaded.
    pub skip: u32,

    /// File offset of the first loaded mip.
    pub tail_offset: usize,

    /// Of the full texture.
    pub width: u32,
    pub height: u32,
}

/// Where the mips that fit in `max_dim` start, from the first bytes of a file (at least 148).
pub fn plan_dds_stream(header: &[u8], file_size: usize, max_dim: u32) -> DdsStreamPlan {
    let mut plan = DdsStreamPlan::default();
    let Ok(l) = parse_dds_layout(header) else { return plan };
    plan.width = l.width;
    plan.height = l.height;
    if l.cube || l.volume || l.array > 1 || l.mips < 3 || l.width.max(l.height) <= max_dim {
        return plan;
    }

    let mut skip = 0;
    while skip + 1 < l.mips && (l.width >> skip).max(l.height >> skip) > max_dim {
        skip += 1;
    }

    if skip == 0 {
        return plan;
    }

    let mut p = l.data;
    let (mut w, mut h) = (l.width, l.height);
    for _ in 0..skip {
        let Ok((_, size)) = mip_size(&l, w, h) else { return plan };
        p += size;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }

    if p >= file_size {
        return plan;
    }

    DdsStreamPlan { ok: true, skip, tail_offset: p, ..plan }
}

/// Builds a texture from a file's header and the tail a [`plan_dds_stream`] plan points at (a 2048 x 2048 DXT5 texture becomes
/// 256 x 256 and 1/64 of the bytes).
///
/// # Errors
///
/// Fails for an unsupported or truncated file, or if the device refuses the texture.
pub fn load_dds_tail(dev: &ID3D11Device, header: &[u8], tail: &[u8], plan: &DdsStreamPlan, srgb: bool) -> Result<Texture, RenderError> {
    let l = parse_dds_layout(header)?;
    build_dds(dev, &l, tail, plan.tail_offset, plan.skip, srgb)
}

/// BC3 blocks of mid grey.
const GREY_BLOCK: [u8; 16] = [255, 255, 0, 0, 0, 0, 0, 0, 0xEF, 0x7B, 0xEF, 0x7B, 0, 0, 0, 0];

/// BC3 blocks of a flat two-channel normal: (alpha 127 + 1/510) * 2 - 1 = 0; green 2/3 of the way from 129.5 to 125.5 = 126.8 -> -0.001.
const FLAT_NORMAL_BLOCK: [u8; 16] = [127, 127, 0, 0, 0, 0, 0, 0, 0x00, 0x04, 0xE0, 0x03, 0xFF, 0xFF, 0xFF, 0xFF];

/// A 2D texture array from DDS files (the terrain's diffuse and normal arrays: every slice the same size, format and mip count,
/// loaded whole). A missing entry, an unreadable file or one that doesn't match the first usable file becomes a neutral BC3 slice:
/// mid grey, or a flat two-channel normal with `flat_normal`. Returns the texture (no view when no file is usable) and the number of
/// files given that were replaced.
pub fn load_dds_array(dev: &ID3D11Device, files: &[Option<&[u8]>], srgb: bool, flat_normal: bool) -> (Texture, usize) {
    let layouts = files.iter().map(|file| file.and_then(|bytes| parse_dds_layout(bytes).ok())).collect::<Vec<_>>();
    let mut usable = vec![false; files.len()];
    let mut reference: Option<DdsLayout> = None;
    for (i, layout) in layouts.iter().enumerate() {
        let Some(l) = layout else { continue };
        if l.cube || l.volume || l.array > 1 {
            continue;
        }

        match reference {
            None => {
                reference = Some(*l);
                usable[i] = true;
            }

            Some(r) => usable[i] = l.width == r.width && l.height == r.height && l.mips == r.mips && l.fmt == r.fmt,
        }
    }

    let Some(r) = reference else { return (Texture::default(), 0) };
    let mismatched = files.iter().zip(&usable).filter(|(file, usable)| file.is_some() && !**usable).count();

    // The neutral slice: every mip the same bytes per block.
    let mut neutral = vec![];
    let (mut w, mut h) = (r.width, r.height);
    for _ in 0..r.mips {
        let Ok((_, size)) = mip_size(&r, w, h) else { return (Texture::default(), mismatched) };
        neutral.push(if r.fmt == DXGI_FORMAT_BC3_UNORM {
            let block = if flat_normal { FLAT_NORMAL_BLOCK } else { GREY_BLOCK };
            block.iter().copied().cycle().take(size / 16 * 16).chain(std::iter::repeat_n(0, size % 16)).collect::<Vec<_>>()
        } else {
            vec![128u8; size]
        });

        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }

    let mut subs = vec![];
    for (i, file) in files.iter().enumerate() {
        let (mut w, mut h) = (r.width, r.height);
        let mut p = layouts[i].map_or(128, |l| l.data);
        for neutral_mip in &neutral {
            let Ok((pitch, size)) = mip_size(&r, w, h) else { return (Texture::default(), mismatched) };
            let data = match file {
                Some(bytes) if usable[i] && p + size <= bytes.len() => bytes[p..].as_ptr(),
                _ => neutral_mip.as_ptr(),
            };

            subs.push(D3D11_SUBRESOURCE_DATA { pSysMem: data.cast::<c_void>(), SysMemPitch: pitch as u32, SysMemSlicePitch: size as u32 });
            p += size;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
    }

    let td = D3D11_TEXTURE2D_DESC {
        Width: r.width, Height: r.height, MipLevels: r.mips, ArraySize: files.len() as u32, Format: if srgb { to_typeless(r.fmt) } else { r.fmt },
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32,
        CPUAccessFlags: 0, MiscFlags: 0,
    };

    let mut tex = None;
    // SAFETY: the subresources point into the files and the neutral mips, which outlive the call.
    let hr = unsafe { dev.CreateTexture2D(&td, Some(subs.as_ptr()), Some(&mut tex)) };
    if hr.is_err() {
        wce_foundation::log::warn(format!("texture array {}x{} x{}, {} mips, format {}: CreateTexture2D 0x{:08x}", r.width, r.height, files.len(), r.mips, td.Format, hr.0 as u32));
        return (Texture::default(), mismatched);
    }

    let format = if srgb { to_srgb(r.fmt) } else { r.fmt };
    let mut t = Texture { width: r.width, height: r.height, mips: r.mips, depth: 1, format, ..Default::default() };
    let sd = srv_desc(format, D3D11_SRV_DIMENSION_TEXTURE2DARRAY, r.mips, files.len() as u32);
    let resource = tex.as_ref().and_then(|tex| tex.cast::<ID3D11Resource>().ok());
    // SAFETY: the resource is the texture just created.
    if unsafe { dev.CreateShaderResourceView(resource.as_ref(), Some(&sd), Some(&mut t.srv)) }.is_err() {
        t.srv = None;
    }

    (t, mismatched)
}

/// A 1 x 1 texture of one colour (0xAABBGGRR).
pub fn make_solid_texture(dev: &ID3D11Device, rgba: u32) -> Texture {
    let td = D3D11_TEXTURE2D_DESC {
        Width: 1, Height: 1, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let sd = D3D11_SUBRESOURCE_DATA { pSysMem: (&raw const rgba).cast(), SysMemPitch: 4, SysMemSlicePitch: 4 };
    let mut t = Texture { width: 1, height: 1, mips: 1, depth: 1, format: td.Format, ..Default::default() };
    let mut tex = None;
    // SAFETY: the data is one pixel on the stack, alive during the calls.
    unsafe {
        let _ = dev.CreateTexture2D(&td, Some(&sd), Some(&mut tex));
        if let Some(resource) = tex.and_then(|tex| tex.cast::<ID3D11Resource>().ok()) {
            let _ = dev.CreateShaderResourceView(&resource, None, Some(&mut t.srv));
        }
    }

    t
}

/// The box-filtered mip chain of an RGBA8 image, down to 1 x 1: (levels, widths, heights).
pub fn rgba_mip_chain(w: u32, h: u32, rgba: &[u8]) -> (Vec<Vec<u8>>, Vec<u32>, Vec<u32>) {
    let mut levels = vec![rgba[..w as usize * h as usize * 4].to_vec()];
    let (mut lw, mut lh) = (vec![w], vec![h]);
    while lw[lw.len() - 1] > 1 || lh[lh.len() - 1] > 1 {
        let (pw, ph) = (lw[lw.len() - 1] as usize, lh[lh.len() - 1] as usize);
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let src = &levels[levels.len() - 1];
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let (x0, x1, y0, y1) = ((2 * x).min(pw - 1), (2 * x + 1).min(pw - 1), (2 * y).min(ph - 1), (2 * y + 1).min(ph - 1));
                for k in 0..4 {
                    let s = [(y0, x0), (y0, x1), (y1, x0), (y1, x1)].iter().map(|&(y, x)| u32::from(src[(y * pw + x) * 4 + k])).sum::<u32>();
                    next[(y * nw + x) * 4 + k] = ((s + 2) / 4) as u8;
                }
            }
        }

        levels.push(next);
        lw.push(nw as u32);
        lh.push(nh as u32);
    }

    (levels, lw, lh)
}

/// An RGBA8 image (w * h * 4 bytes) with a box-filtered mip chain down to 1 x 1; an sRGB view with `srgb`. No view on failure.
pub fn make_rgba_texture(dev: &ID3D11Device, w: u32, h: u32, rgba: &[u8], srgb: bool) -> Texture {
    if w == 0 || h == 0 || rgba.len() < w as usize * h as usize * 4 {
        return Texture::default();
    }

    let (levels, lw, _) = rgba_mip_chain(w, h, rgba);
    let format = if srgb { DXGI_FORMAT_R8G8B8A8_UNORM_SRGB } else { DXGI_FORMAT_R8G8B8A8_UNORM };
    let td = D3D11_TEXTURE2D_DESC {
        Width: w, Height: h, MipLevels: levels.len() as u32, ArraySize: 1, Format: format, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32, CPUAccessFlags: 0, MiscFlags: 0,
    };

    let subs = levels.iter().zip(&lw)
        .map(|(level, w)| D3D11_SUBRESOURCE_DATA { pSysMem: level.as_ptr().cast(), SysMemPitch: w * 4, SysMemSlicePitch: 0 })
        .collect::<Vec<_>>();

    let mut t = Texture::default();
    let mut tex = None;
    // SAFETY: the subresources point into `levels`, alive during the calls.
    unsafe {
        if dev.CreateTexture2D(&td, Some(subs.as_ptr()), Some(&mut tex)).is_err() {
            return t;
        }

        let Some(resource) = tex.and_then(|tex| tex.cast::<ID3D11Resource>().ok()) else { return t };
        if dev.CreateShaderResourceView(&resource, None, Some(&mut t.srv)).is_err() {
            return t;
        }
    }

    Texture { width: w, height: h, mips: levels.len() as u32, depth: 1, format, ..t }
}
