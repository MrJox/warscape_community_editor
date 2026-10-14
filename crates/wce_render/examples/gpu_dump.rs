//! Differential dump of the GPU-side loaders, for comparing with the C++ code on the same device: textures (read back from the GPU,
//! every mip and slice hashed), mip-streaming plans, texture arrays, RGBA mip chains, and the reflection of every game shader.
//!
//!   gpu_dump <file with one .dds path per line> <folder with fxc/> <terrain textures folder>

use std::path::{Path, PathBuf};

use wce_d3d11::*;
use wce_render::cbuffers::ConstantBuffers;
use wce_render::shader_lib::ShaderLibrary;
use wce_render::texture::{Texture, load_dds, load_dds_array, load_dds_tail, make_rgba_texture, make_solid_texture, plan_dds_stream};
use windows_core::Interface;

fn fnv(bytes: &[u8], h: u64) -> u64 {
    bytes.iter().fold(h, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

fn is_block(f: DXGI_FORMAT) -> bool {
    (DXGI_FORMAT_BC1_TYPELESS..=DXGI_FORMAT_BC5_SNORM).contains(&f) || (DXGI_FORMAT_BC6H_TYPELESS..=DXGI_FORMAT_BC7_UNORM_SRGB).contains(&f)
}

fn row_bytes(f: DXGI_FORMAT, w: u32) -> usize {
    if is_block(f) {
        let bb = if (DXGI_FORMAT_BC1_TYPELESS..=DXGI_FORMAT_BC1_UNORM_SRGB).contains(&f) || (DXGI_FORMAT_BC4_TYPELESS..=DXGI_FORMAT_BC4_SNORM).contains(&f) { 8 } else { 16 };
        return w.div_ceil(4).max(1) as usize * bb;
    }

    w as usize * match f {
        DXGI_FORMAT_R16G16B16A16_FLOAT | DXGI_FORMAT_R16G16B16A16_UNORM => 8,
        DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
        DXGI_FORMAT_R8_UNORM | DXGI_FORMAT_A8_UNORM => 1,
        DXGI_FORMAT_R8G8_UNORM | DXGI_FORMAT_R16_FLOAT | DXGI_FORMAT_B5G6R5_UNORM | DXGI_FORMAT_B5G5R5A1_UNORM => 2,
        _ => 4,
    }
}

fn rows(f: DXGI_FORMAT, h: u32) -> u32 {
    if is_block(f) { h.div_ceil(4).max(1) } else { h }
}

unsafe fn readback(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, srv: Option<&ID3D11ShaderResourceView>) -> String {
    let Some(srv) = srv else { return "nosrv".to_owned() };
    unsafe {
        let Ok(resource) = srv.GetResource() else { return "unknown".to_owned() };
        let mut h = 0xCBF2_9CE4_8422_2325u64;
        if let Ok(t2) = resource.cast::<ID3D11Texture2D>() {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            t2.GetDesc(&mut d);
            let s = D3D11_TEXTURE2D_DESC { Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0, ..d };
            let mut st = None;
            if dev.CreateTexture2D(&s, None, Some(&mut st)).is_err() {
                return "nostaging".to_owned();
            }

            let st = st.unwrap();
            ctx.CopyResource(&st, &t2);
            for a in 0..d.ArraySize {
                for m in 0..d.MipLevels {
                    let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
                    if ctx.Map(&st, m + a * d.MipLevels, D3D11_MAP_READ, 0, Some(&mut ms)).is_err() {
                        return "nomap".to_owned();
                    }

                    let (w, hh) = ((d.Width >> m).max(1), (d.Height >> m).max(1));
                    for y in 0..rows(d.Format, hh) as usize {
                        h = fnv(std::slice::from_raw_parts(ms.pData.cast::<u8>().add(y * ms.RowPitch as usize), row_bytes(d.Format, w)), h);
                    }

                    ctx.Unmap(&st, m + a * d.MipLevels);
                }
            }

            return format!("2d {}x{} a{} m{} f{} misc{} {h:016x}", d.Width, d.Height, d.ArraySize, d.MipLevels, d.Format, d.MiscFlags);
        }

        if let Ok(t3) = resource.cast::<ID3D11Texture3D>() {
            let mut d = D3D11_TEXTURE3D_DESC::default();
            t3.GetDesc(&mut d);
            let s = D3D11_TEXTURE3D_DESC { Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0, ..d };
            let mut st = None;
            if dev.CreateTexture3D(&s, None, Some(&mut st)).is_err() {
                return "nostaging".to_owned();
            }

            let st = st.unwrap();
            ctx.CopyResource(&st, &t3);
            for m in 0..d.MipLevels {
                let mut ms = D3D11_MAPPED_SUBRESOURCE::default();
                if ctx.Map(&st, m, D3D11_MAP_READ, 0, Some(&mut ms)).is_err() {
                    return "nomap".to_owned();
                }

                let (w, hh, dd) = ((d.Width >> m).max(1), (d.Height >> m).max(1), (d.Depth >> m).max(1));
                for z in 0..dd as usize {
                    for y in 0..rows(d.Format, hh) as usize {
                        let p = ms.pData.cast::<u8>().add(z * ms.DepthPitch as usize + y * ms.RowPitch as usize);
                        h = fnv(std::slice::from_raw_parts(p, row_bytes(d.Format, w)), h);
                    }
                }

                ctx.Unmap(&st, m);
            }

            return format!("3d {}x{}x{} m{} f{} {h:016x}", d.Width, d.Height, d.Depth, d.MipLevels, d.Format);
        }

        "unknown".to_owned()
    }
}

fn show(tag: &str, t: &Texture, dev: &ID3D11Device, ctx: &ID3D11DeviceContext) {
    // SAFETY: the texture belongs to the device.
    let back = unsafe { readback(dev, ctx, t.srv.as_ref()) };
    println!("  {tag} {}x{} m{} cube{} d{} f{} | {back}", t.width, t.height, t.mips, i32::from(t.cube), t.depth, t.format);
}

fn dirs(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        if entry.path().is_dir() {
            out.push(entry.path());
            dirs(&entry.path(), out);
        }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };
    let (mut dev, mut ctx) = (None, None);
    // SAFETY: device creation with valid arguments.
    unsafe {
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, None, D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx)).unwrap();
    }

    let (dev, ctx) = (dev.unwrap(), ctx.unwrap());
    for path in std::fs::read_to_string(&args[1]).unwrap().lines() {
        let b = std::fs::read(path).unwrap();
        println!("== {path}");
        for srgb in [false, true] {
            match load_dds(&dev, &b, srgb) {
                Ok(t) => show(if srgb { "srgb" } else { "unorm" }, &t, &dev, &ctx),
                Err(error) => println!("  error {error}"),
            }
        }

        let head = &b[..b.len().min(148)];
        let plan = plan_dds_stream(head, b.len(), 256);
        println!("  plan {} {} {} {}x{}", i32::from(plan.ok), plan.skip, plan.tail_offset, plan.width, plan.height);
        if plan.ok {
            match load_dds_tail(&dev, head, &b[plan.tail_offset..], &plan, true) {
                Ok(t) => show("tail", &t, &dev, &ctx),
                Err(error) => println!("  error {error}"),
            }
        }
    }

    let terrain = PathBuf::from(&args[3]);
    let mut all = vec![];
    dirs(&terrain, &mut all);
    all.sort();
    for d in &all {
        for kind in ["_diffuse.dds", "_normal.dds"] {
            let mut files = std::fs::read_dir(d).unwrap().flatten().map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| { let n = n.to_string_lossy(); n.len() > kind.len() && n.ends_with(kind) }))
                .collect::<Vec<_>>();
            if files.is_empty() {
                continue;
            }

            files.sort();
            let data = files.iter().map(|f| std::fs::read(f).unwrap()).collect::<Vec<_>>();
            let mut slices = data.iter().map(|x| Some(x.as_slice())).collect::<Vec<_>>();
            slices.insert(1, None);
            let normal = kind == "_normal.dds";
            let (t, mismatched) = load_dds_array(&dev, &slices, !normal, normal);
            println!("== array {}{kind} n{} mism{mismatched}", d.strip_prefix(&terrain).unwrap().display(), slices.len());
            show("array", &t, &dev, &ctx);
        }
    }

    let img = (0..37 * 23 * 4).map(|i: usize| ((i * 31 + i / 7) & 0xFF) as u8).collect::<Vec<_>>();
    show("rgba", &make_rgba_texture(&dev, 37, 23, &img, true), &dev, &ctx);
    show("solid", &make_solid_texture(&dev, 0x8040_2010), &dev, &ctx);

    let root = PathBuf::from(&args[2]);
    let read = |path: &str| {
        let p = root.join(path);
        if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    };

    let mut cbs = ConstantBuffers::default();
    let mut lib = ShaderLibrary::new(&dev);
    let mut names = std::fs::read_dir(root.join("fxc")).unwrap().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "fxc"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    for n in &names {
        match lib.get(&read, &mut cbs, n) {
            Ok(s) => {
                let maps = [&s.cbuffers, &s.textures, &s.samplers, &s.uavs].iter()
                    .map(|m| format!(" |{}", m.iter().map(|(k, v)| format!(" {k}={v}")).collect::<String>()))
                    .collect::<String>();
                println!("shader {n} stage{}{maps}", match s.stage {
                    wce_render::shader_lib::Stage::Vs => 0,
                    wce_render::shader_lib::Stage::Ps => 1,
                    wce_render::shader_lib::Stage::Gs => 2,
                    wce_render::shader_lib::Stage::Cs => 3,
                    wce_render::shader_lib::Stage::Hs => 4,
                    wce_render::shader_lib::Stage::Ds => 5,
                });
            }

            Err(error) => println!("shader {n} error {error}"),
        }
    }

    for (name, b) in cbs.all() {
        let vars = b.vars.iter().map(|(k, v)| format!(" {k}@{}+{}", v.offset, v.size)).collect::<String>();
        println!("cbuffer {name} {}{vars}", b.cpu.len());
    }
}
