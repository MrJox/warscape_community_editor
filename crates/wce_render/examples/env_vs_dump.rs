//! Compares the editor's environment -> cbuffer mapping with live values read from a game memory dump. Port of `cli/env_vs_dump.cpp`.
//!
//!   cargo run --release -p wce_render --example env_vs_dump -- <in-pack .environment path> <dump_cbs.py output.txt> [gamma brightness]
//!   cargo run --release -p wce_render --example env_vs_dump -- --all <dump_cbs.py output.txt> [gamma brightness]
//!   cargo run --release -p wce_render --example env_vs_dump -- --emit <in-pack .environment path> <out.txt> [gamma brightness]
//!
//! Prints, for every variable the editor writes that also appears in the dump: both values and OK / DIFF. The dump file is the text written by
//! scripts/re/dump_cbs.py ("cbuffer.var  v0 v1 v2 ..."). --all ranks every .environment file in the packs by how many of those variables
//! match: the dump does not say which weather file the battle used, but its sun / sky / fog constants identify it. --emit writes the editor's
//! own values in the dump's format (a dump every variable of that environment matches: the gate's self-check).

use std::sync::Arc;

use wce_d3d11::*;
use wce_formats::c_num::{printf_g, strtod_prefix};
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::settings::Settings;
use wce_render::cbuffers::ConstantBuffers;
use wce_render::environment_apply::apply_environment;
use wce_render::shader_lib::ShaderLibrary;
use wce_scene::environment::read_environment;
use wce_vfs::{Config, GameProfile, Vfs};

struct DumpVar {
    key: String,
    cb: String,
    var: String,
    values: Vec<f32>,
}

/// Whitespace-separated: the key, then floats (as `is >> x`, stopping at the first word that is not a number).
fn read_dump(path: &str) -> Vec<DumpVar> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = vec![];
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let Some((cb, var)) = key.split_once('.') else { continue };
        let mut values = vec![];
        for w in words {
            match strtod_prefix(w) {
                Some((v, n)) if n == w.len() => values.push(v as f32),
                _ => break,
            }
        }

        out.push(DumpVar { key: key.to_owned(), cb: cb.to_owned(), var: var.to_owned(), values });
    }

    out
}

fn g6(v: f32) -> String {
    printf_g(f64::from(v))
}

/// The value of a variable as the editor wrote it.
fn mine(cbs: &ConstantBuffers, cb: &str, var: &str) -> Option<Vec<f32>> {
    let b = cbs.find(cb)?;
    let v = b.vars.get(var)?;
    Some((0..v.size as usize / 4).map(|i| f32::from_le_bytes(b.cpu[v.offset as usize + 4 * i..][..4].try_into().expect("4 bytes"))).collect())
}

/// (match, differ); prints every compared variable when verbose.
fn compare(cbs: &ConstantBuffers, dump: &[DumpVar], verbose: bool) -> (i32, i32) {
    let (mut ok, mut diff) = (0, 0);
    for d in dump {
        let Some(m) = mine(cbs, &d.cb, &d.var) else { continue };

        // An engine flag, not from the file.
        if d.cb == "lighting_VS_PS" && d.var == "g_apply_environment_specular" {
            continue;
        }

        let n = m.len().min(d.values.len());
        let same = (0..n).all(|i| (m[i] - d.values[i]).abs() <= 1e-3 * d.values[i].abs().max(1.0));
        let mine_s = m[..n].iter().map(|&x| format!("{} ", g6(x))).collect::<String>();
        let game_s = d.values[..n].iter().map(|&x| format!("{} ", g6(x))).collect::<String>();
        if same {
            ok += 1;
        } else {
            diff += 1;
        }

        if verbose {
            println!("{:<4} {:<50} editor: {mine_s}\n     {:<50} game:   {game_s}", if same { "OK" } else { "DIFF" }, d.key, "");
        }
    }

    (ok, diff)
}

fn run(args: &[String]) -> Result<i32, String> {
    // SAFETY: before other threads; valid arguments.
    let dev = unsafe {
        init_platform();
        let mut dev = None;
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&mut dev), None, None)
            .ok()
            .ok()
            .and(dev)
            .ok_or("no D3D11 device")?
    };

    let game = Settings::load_default().game_dir().ok_or("the game folder is not set (in the settings or WCE_GAME)")?;
    // WCE_MOD_PACKS (pack file names, `;` between them, first wins): mods mounted above the game's packs (a mod's environment files).
    let mut config = Config::new(game, GameProfile::from_env().clone());
    if let Ok(mods) = std::env::var("WCE_MOD_PACKS") {
        config.mod_packs = Some(mods.split(';').map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned).collect());
    }

    let vfs = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    let read = |p: &str| vfs.read(p).map_err(|e| e.to_string());
    let mut cbs = ConstantBuffers::default();
    let mut lib = ShaderLibrary::new(&dev);
    for s in [
        "dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen", "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef",
        "dx11_sm_3_0_battlefog.hlsl_ps_ps30_main", "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high",
        "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_determine_black_and_white_points", "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_luma_to_quarter_size_luma_stats",
    ] {
        let _ = lib.get(&read, &mut cbs, s);
    }

    let gamma_brightness = |first: usize| -> Option<(f32, f32)> {
        let num = |s: &String| strtod_prefix(s).map_or(0.0, |v| v.0) as f32;
        (args.len() > first + 1).then(|| (num(&args[first]), num(&args[first + 1])))
    };

    let load = |cbs: &mut ConstantBuffers, path: &str, gb: Option<(f32, f32)>| -> Result<(), String> {
        let doc = XmlDoc::parse(read(path)?).map_err(|e| e.to_string())?;
        let mut e = read_environment(&doc);
        if let Some((g, b)) = gb {
            e.output_gamma = g;
            e.output_brightness = b;
        }

        apply_environment(&e, cbs);
        Ok(())
    };

    if args[0] == "--emit" {
        load(&mut cbs, &args[1], gamma_brightness(3))?;
        let mut out = String::new();
        for b in cbs.buffers() {
            for var in b.vars.keys() {
                let v = mine(&cbs, &b.name, var).unwrap_or_default();
                out += &format!("{}.{var} {}\n", b.name, v.iter().map(|x| format!("{x:.9e}")).collect::<Vec<_>>().join(" "));
            }
        }

        std::fs::write(&args[2], out).map_err(|e| e.to_string())?;
        return Ok(0);
    }

    let dump = read_dump(&args[1]);
    let gb = gamma_brightness(2);
    if args[0] == "--all" {
        let files = vfs.list("", ".environment").into_iter().map(str::to_owned).collect::<Vec<_>>();
        let mut ranked = vec![];
        for f in files {
            if load(&mut cbs, &f, gb).is_err() {
                continue;
            }

            ranked.push((compare(&cbs, &dump, false).0, f));
        }

        ranked.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (n, f) in ranked.iter().take(15) {
            println!("{n:3}  {f}");
        }

        return Ok(0);
    }

    load(&mut cbs, &args[0], gb)?;
    let (ok, diff) = compare(&cbs, &dump, true);
    println!("\n{ok} match, {diff} differ");
    Ok(i32::from(diff != 0))
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 || (args[0] == "--emit" && args.len() < 3) {
        println!("usage: env_vs_dump <environment path | --all> <dump_cbs output> [gamma brightness]");
        std::process::exit(2);
    }

    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
