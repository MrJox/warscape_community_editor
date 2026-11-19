//! A campaign map frame read from a RenderDoc capture by `scripts/re/rd_campaign_frame.py`, named and summarised
//! (docs/plans/campaign_tilemap.md, Phase 1):
//! - `shader_names.csv`: each shader of the capture named by the game's `fxc/*.fxc` file whose DXBC checksum it has;
//! - `passes.txt`: the frame's draws in runs of the same targets and shaders, with the shaders' names;
//! - `mask.png`: the pixels the skinned draws (armies, agents) change in the G-buffer, white (out of the comparison's scope);
//! - `screen.png`, `gbuffer0.png`: the screen before the UI and the G-buffer's first target (8-bit targets as RGB, to look at);
//! - with `--textures <prefix>...`: `texture_names.csv`, the capture's textures named by the packs' DDS files under the prefixes whose
//!   mip of the capture's top size has the same bytes (SHA-1; the texture setting may drop the top mips).
//!
//!   cargo run --release -p wce_terrain_io --example campaign_frame -- <the script's folder> [--textures <prefix>...]
//!
//! `WCE_ATTILA_DIR` the game whose shaders name the capture's (default this machine's Steam folder); the profile from `WCE_GAME`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;
use wce_vfs::{Config, GameProfile, Vfs};

fn read_csv(path: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines().skip(1).filter(|l| !l.is_empty()).map(|l| l.split(',').map(str::to_owned).collect()).collect()
}

/// Every DXBC checksum in the game's fxc files and the files that hold it.
fn fxc_names(vfs: &Vfs) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let files: Vec<String> = vfs.list("fxc/", "fxc").iter().map(|s| (*s).to_owned()).collect();
    for path in &files {
        let Ok(b) = vfs.read(path) else { continue };
        let name = path.trim_start_matches("fxc/").trim_end_matches(".fxc").to_owned();
        let mut i = 0;
        while let Some(k) = b[i..].windows(4).position(|w| w == b"DXBC") {
            let at = i + k;
            if at + 20 <= b.len() {
                let chk: String = b[at + 4..at + 20].iter().map(|x| format!("{x:02x}")).collect();
                out.entry(chk).or_default().push(name.clone());
            }
            i = at + 4;
        }
    }
    println!("{} fxc files, {} shaders", files.len(), out.len());
    out
}

/// An 8-bit RGBA or BGRA target as an RGB PNG (its DDS's DXGI format: 28 R8G8B8A8, 87 B8G8R8A8; a legacy header by its red mask).
fn rgb_png(dds: &Path, png: &Path) {
    let Ok(b) = std::fs::read(dds) else { return };
    let Some((w, h, px)) = dds_pixels(dds) else { return };
    if px.len() != (w * h * 4) as usize {
        println!("{}: not an 8-bit target", dds.display());
        return;
    }
    let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let bgra = if &b[84..88] == b"DX10" { matches!(u(128), 87 | 91) } else { u(92) == 0x00ff_0000 };
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for p in px.chunks_exact(4) {
        if bgra {
            rgb.extend_from_slice(&[p[2], p[1], p[0]]);
        } else {
            rgb.extend_from_slice(&p[..3]);
        }
    }
    let png_file = PngFile { width: w, height: h, bit_depth: 8, color_type: 2, pixels: rgb, ..Default::default() };
    std::fs::write(png, png_file.encode().expect("png")).expect("png");
}

/// A DDS's size and pixel bytes (RenderDoc's save: one mip, the target's format).
fn dds_pixels(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 128 || &b[..4] != b"DDS " {
        return None;
    }
    let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let (h, w) = (u(12), u(16));
    let start = if &b[84..88] == b"DX10" { 148 } else { 128 };
    Some((w, h, b[start..].to_vec()))
}

fn mask(dir: &Path) {
    let t = dir.join("targets");
    let mut masked: Option<(u32, u32, Vec<bool>)> = None;
    for i in 0.. {
        let (Some(a), Some(b)) =
            (dds_pixels(&t.join(format!("skinned_before{i}.dds"))), dds_pixels(&t.join(format!("skinned_after{i}.dds"))))
        else {
            break;
        };
        let (w, h) = (a.0, a.1);
        let n = (w * h) as usize;
        let bpp = a.2.len() / n.max(1);
        if bpp == 0 || a.2.len() != b.2.len() {
            println!("mask: target {i} unreadable ({} / {} bytes for {w} x {h})", a.2.len(), b.2.len());
            continue;
        }
        let m = masked.get_or_insert_with(|| (w, h, vec![false; n]));
        for (k, px) in m.2.iter_mut().enumerate() {
            *px |= a.2[k * bpp..(k + 1) * bpp] != b.2[k * bpp..(k + 1) * bpp];
        }
    }
    let Some((w, h, m)) = masked else {
        println!("mask: no skinned draws' targets (nothing masked)");
        return;
    };
    let count = m.iter().filter(|&&x| x).count();
    let png = PngFile {
        width: w,
        height: h,
        bit_depth: 8,
        color_type: 0,
        pixels: m.iter().map(|&x| if x { 255 } else { 0 }).collect(),
        ..Default::default()
    };
    std::fs::write(dir.join("mask.png"), png.encode().expect("png")).expect("mask.png");
    println!("mask: {count} pixels ({:.2} %) changed by the skinned draws", 100.0 * count as f64 / m.len() as f64);
}

/// SHA-1 of the bytes as 40 hex digits (FIPS 180-4; the capture script's `hashlib.sha1`).
fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Bytes per 4 x 4 block (compressed) or per pixel (8-bit RGBA / BGRA) of a DXGI format's name; none: not matched.
fn format_size(name: &str) -> Option<(bool, usize)> {
    match name.split('_').next()? {
        "BC1" | "BC4" => Some((true, 8)),
        "BC2" | "BC3" | "BC5" | "BC6H" | "BC7" => Some((true, 16)),
        "R8G8B8A8" | "B8G8R8A8" | "B8G8R8X8" => Some((false, 4)),
        _ => None,
    }
}

fn mip_bytes(w: u32, h: u32, (blocks, size): (bool, usize)) -> usize {
    if blocks { (w.div_ceil(4).max(1) * h.div_ceil(4).max(1)) as usize * size } else { (w * h) as usize * size }
}

/// The capture's textures named by the packs' DDS files under the prefixes (`textures.csv`'s top-mip SHA-1 against each file's mip of
/// that size).
fn name_textures(dir: &Path, vfs: &Vfs, prefixes: &[String]) {
    let rows = read_csv(&dir.join("textures.csv"));
    let mut want: HashMap<(u32, u32, String), Vec<(String, String)>> = HashMap::new();
    for r in &rows {
        if r.len() >= 8 && r[7].len() == 40 && r[6] == "0" {
            let (w, h) = (r[1].parse().unwrap_or(0), r[2].parse().unwrap_or(0));
            want.entry((w, h, r[3].clone())).or_default().push((r[0].clone(), r[7].clone()));
        }
    }
    let mut found: HashMap<String, String> = HashMap::new();
    for prefix in prefixes {
        let files: Vec<String> = vfs.list(prefix, "dds").iter().map(|s| (*s).to_owned()).collect();
        for path in &files {
            let Ok(b) = vfs.read(path) else { continue };
            if b.len() < 128 || &b[..4] != b"DDS " {
                continue;
            }
            let u = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            let (h, w, mips) = (u(12), u(16), u(28).max(1));
            let start = if &b[84..88] == b"DX10" { 148 } else { 128 };
            for ((cw, ch, format), list) in &want {
                let Some(fs) = format_size(format) else { continue };
                let mut at = start;
                for m in 0..mips {
                    let (mw, mh) = ((w >> m).max(1), (h >> m).max(1));
                    let n = mip_bytes(mw, mh, fs);
                    if (mw, mh) == (*cw, *ch) {
                        if let Some(bytes) = b.get(at..at + n) {
                            let sha = sha1_hex(bytes);
                            for (id, s) in list {
                                if *s == sha {
                                    found.entry(id.clone()).or_insert_with(|| path.clone());
                                }
                            }
                        }
                        break;
                    }
                    at += n;
                }
            }
        }
        println!("textures under {prefix}: {} files read", files.len());
    }
    let mut out = String::from(
        "id,file
",
    );
    for r in &rows {
        if let Some(f) = found.get(&r[0]) {
            out += &format!(
                "{},{f}
",
                r[0]
            );
        }
    }
    std::fs::write(dir.join("texture_names.csv"), out).expect("texture_names.csv");
    println!("textures named: {} of {}", found.len(), rows.len());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().expect("usage: campaign_frame <folder> [--textures <prefix>...]"));
    let prefixes: Vec<String> = args.iter().skip_while(|a| *a != "--textures").skip(1).cloned().collect();
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&game_dir, game);
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");

    // The shaders' names.
    let fxc = fxc_names(&vfs);
    let mut names: HashMap<String, String> = HashMap::new();
    let mut out = String::from("id,stage,dxbc,name,files\n");
    let (mut named, mut total) = (0, 0);
    for r in read_csv(&dir.join("shaders.csv")) {
        let (id, stage, chk) = (&r[0], &r[1], &r[2]);
        total += 1;
        let files = fxc.get(chk.as_str());
        // The shortest of the files with that bytecode (the variants with fewer defines).
        let name = files.and_then(|f| f.iter().min_by_key(|n| n.len())).cloned().unwrap_or_else(|| format!("?{stage}{id}"));
        if files.is_some() {
            named += 1;
        }
        out += &format!("{id},{stage},{chk},{name},{}\n", files.map_or(0, Vec::len));
        names.insert(id.clone(), name);
    }
    std::fs::write(dir.join("shader_names.csv"), out).expect("shader_names.csv");
    println!("shaders named: {named} of {total}");

    // The passes: runs of draws with the same targets and shaders.
    let name = |id: &str| if id == "0" { "-".to_owned() } else { names.get(id).cloned().unwrap_or_else(|| format!("#{id}")) };
    let mut lines = Vec::new();
    let mut run: Option<(String, String, u32, u32, u32)> = None; // key, text, first, last, count
    for r in read_csv(&dir.join("events.csv")) {
        if r.len() < 17 || r[1] == "clear" {
            continue;
        }
        let ev: u32 = r[0].parse().unwrap_or(0);
        let text = if r[1] == "dispatch" {
            format!("CS {}", name(&r[9]))
        } else {
            format!("RT [{}] D {} | VS {} | PS {} | depth {} blend {}", r[5], r[6], name(&r[7]), name(&r[8]), r[15], r[16])
        };
        let key = text.clone();
        match &mut run {
            Some((k, _, _, last, n)) if *k == key => {
                *last = ev;
                *n += 1;
            }
            _ => {
                if let Some((_, t, a, b, n)) = run.take() {
                    lines.push(format!("{a}-{b} x{n} {t}"));
                }
                run = Some((key, text, ev, ev, 1));
            }
        }
    }
    if let Some((_, t, a, b, n)) = run {
        lines.push(format!("{a}-{b} x{n} {t}"));
    }
    std::fs::write(dir.join("passes.txt"), lines.join("\n") + "\n").expect("passes.txt");
    println!("passes.txt: {} runs", lines.len());

    mask(&dir);
    rgb_png(&dir.join("targets/screen.dds"), &dir.join("screen.png"));
    rgb_png(&dir.join("targets/gbuffer0.dds"), &dir.join("gbuffer0.png"));
    if !prefixes.is_empty() {
        name_textures(&dir, &vfs, &prefixes);
    }
}
