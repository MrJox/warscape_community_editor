//! Shader gate + reflection dump for the game's compiled shaders. Port of `cli/fxc_dump.cpp`.
//!
//!   cargo run --release -p wce_render --example fxc_dump -- <fxc_dir> <out_dir> [asm_dir]
//!
//! For every dx11_*.fxc: parse the CA header (rpfm_lib's Fxc), reflect the DXBC, create the shader on a hardware D3D11 device. Writes:
//!   <out_dir>/index.tsv     one row per shader: name parts, stage, create result, cbuffers, resources
//!   <out_dir>/cbuffers.txt  every distinct constant buffer layout with its variables and users
//!   [asm_dir]/<file>.asm    D3DDisassemble output per shader (large; keep out of the repo)

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, c_void};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::fxc::Fxc;
use wce_d3d11::*;
use windows_core::{Interface, PCSTR};

/// The parts of a shader file name: `<prefix _sm_X_Y>_<source.hlsl>_<stage>_<entry>[.fxc]`.
#[derive(Default)]
struct FxcName {
    prefix: String,
    source: String,
    entry: String,
}

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

/// `^(.*?_sm_\d_\d)_(.+?\.hlsl)_([a-z]s)_(.+?)(\.fxc)?$`, case-insensitive, as the C++ regex: the shortest prefix and source that let the
/// rest match.
fn split_fxc_name(name: &str) -> Option<FxcName> {
    let b = name.as_bytes();
    let lower = name.to_ascii_lowercase();
    let l = lower.as_bytes();
    for i in 0..b.len() {
        // The prefix ends with "_sm_" digit "_" digit at i, then '_'.
        if i < 7 || &l[i - 7..i - 3] != b"_sm_" || !is_digit(b[i - 3]) || b[i - 2] != b'_' || !is_digit(b[i - 1]) || b.get(i) != Some(&b'_') {
            continue;
        }

        let rest = i + 1;
        for j in rest + 1..=b.len() {
            // The source ends with ".hlsl" at j, then "_" letter "s" "_" and a non-empty entry.
            if j < rest + 6 || &l[j - 5..j] != b".hlsl" || b.get(j) != Some(&b'_') {
                continue;
            }

            let st = j + 1;
            if st + 3 > b.len() || !l[st].is_ascii_lowercase() || l[st + 1] != b's' || b[st + 2] != b'_' {
                continue;
            }

            let e0 = st + 3;
            let end = if l.len() >= 4 && l.ends_with(b".fxc") && b.len() - 4 > e0 { b.len() - 4 } else { b.len() };
            if end <= e0 {
                continue;
            }

            return Some(FxcName { prefix: name[..i].to_owned(), source: name[rest..j].to_owned(), entry: name[e0..end].to_owned() });
        }
    }

    None
}

fn stage_name(ty: u32) -> &'static str {
    match ty {
        0 => "ps",
        1 => "vs",
        2 => "gs",
        3 => "hs",
        4 => "ds",
        5 => "cs",
        _ => "??",
    }
}

/// # Safety
///
/// `code` is the bytecode the reflection read.
unsafe fn create(dev: &ID3D11Device, ty: u32, code: &[u8]) -> bool {
    // SAFETY: the caller's contract.
    unsafe {
        match ty {
            0 => dev.CreatePixelShader(code, None, Some(&mut None)).is_ok(),
            1 => dev.CreateVertexShader(code, None, Some(&mut None)).is_ok(),
            2 => dev.CreateGeometryShader(code, None, Some(&mut None)).is_ok(),
            3 => dev.CreateHullShader(code, None, Some(&mut None)).is_ok(),
            4 => dev.CreateDomainShader(code, None, Some(&mut None)).is_ok(),
            5 => dev.CreateComputeShader(code, None, Some(&mut None)).is_ok(),
            _ => false,
        }
    }
}

fn c_str(p: PCSTR) -> String {
    if p.is_null() {
        return "?".to_owned();
    }

    // SAFETY: a NUL-terminated name from the reflection, alive while it is.
    unsafe { CStr::from_ptr(p.0.cast()).to_string_lossy().into_owned() }
}

fn type_string(t: &ID3D11ShaderReflectionType) -> String {
    let mut d = D3D11_SHADER_TYPE_DESC::default();
    // SAFETY: reflection queries on a live type.
    unsafe {
        let _ = t.GetDesc(&mut d);
        let mut s = c_str(d.Name);
        if d.Class == D3D_SVC_STRUCT {
            s = "struct{".to_owned();
            for m in 0..d.Members {
                let mut md = D3D11_SHADER_TYPE_DESC::default();
                if let Some(mt) = t.GetMemberTypeByIndex(m) {
                    let _ = mt.GetDesc(&mut md);
                }

                s += &format!("{}:{}@{};", c_str(t.GetMemberTypeName(m)), c_str(md.Name), md.Offset);
            }

            s += "}";
        }

        if d.Elements != 0 {
            s += &format!("[{}]", d.Elements);
        }

        s
    }
}

/// The row of one shader after its name parts; the stage on success.
///
/// # Safety
///
/// `dev` is a live device.
unsafe fn shader_row(dev: &ID3D11Device, path: &Path, asm_dir: Option<&Path>, source: &str, layouts: &mut BTreeMap<String, BTreeSet<String>>) -> Result<(String, &'static str, bool), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let fxc = Fxc::decode(&mut Cursor::new(&bytes), &None).map_err(|e| e.to_string())?;
    let code = fxc.shader();
    // SAFETY: the reflection reads `code`, alive during the block; descriptions are locals.
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        D3DReflect(code.as_ptr().cast(), code.len(), &ID3D11ShaderReflection::IID, &mut raw).ok().map_err(|_| "D3DReflect failed".to_owned())?;
        let refl = ID3D11ShaderReflection::from_raw(raw);
        let mut sd = D3D11_SHADER_DESC::default();
        refl.GetDesc(&mut sd).ok().map_err(|e| e.to_string())?;
        let ty = sd.Version >> 16;
        let created = create(dev, ty, code);
        if let Some(dir) = asm_dir
            && let Ok(text) = D3DDisassemble(code.as_ptr().cast(), code.len(), 0, PCSTR::null())
        {
            let t = std::slice::from_raw_parts(text.GetBufferPointer().cast::<u8>(), text.GetBufferSize());
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let _ = std::fs::write(dir.join(format!("{stem}.asm")), &t[..t.len().saturating_sub(1)]);
        }

        let mut cbs = vec![];
        for c in 0..sd.ConstantBuffers {
            let Some(cb) = refl.GetConstantBufferByIndex(c) else { continue };
            let mut bd = D3D11_SHADER_BUFFER_DESC::default();
            let _ = cb.GetDesc(&mut bd);
            let name = c_str(bd.Name);
            cbs.push(format!("{name}:{}", bd.Size));
            let mut lay = format!("cbuffer {name} ({} bytes)\n", bd.Size);
            for v in 0..bd.Variables {
                let Some(var) = cb.GetVariableByIndex(v) else { continue };
                let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                let _ = var.GetDesc(&mut vd);
                let ty = var.GetType().map(|t| type_string(&t)).unwrap_or_default();
                lay += &format!("  +{}\t{}\t{ty}\t{}\n", vd.StartOffset, vd.Size, c_str(vd.Name));
            }

            layouts.entry(lay).or_default().insert(source.to_owned());
        }

        const KINDS: [&str; 12] = ["cb", "tb", "t", "s", "u", "st", "u", "u", "u", "u", "u", "u"];
        let mut res = vec![];
        for r in 0..sd.BoundResources {
            let mut bd = D3D11_SHADER_INPUT_BIND_DESC::default();
            let _ = refl.GetResourceBindingDesc(r, &mut bd);
            let kind = usize::try_from(bd.Type).ok().and_then(|k| KINDS.get(k)).copied().unwrap_or("?");
            res.push(format!("{}={kind}{}", c_str(bd.Name), bd.BindPoint));
        }

        let mut inputs = vec![];
        for p in 0..sd.InputParameters {
            let mut pd = D3D11_SIGNATURE_PARAMETER_DESC::default();
            let _ = refl.GetInputParameterDesc(p, &mut pd);
            inputs.push(format!("{}{}", c_str(pd.SemanticName), pd.SemanticIndex));
        }

        let version = format!("{}.{}", (sd.Version >> 4) & 0xF, sd.Version & 0xF);
        Ok((format!("{version}\t{}\t{}\t{}\t{}", if created { "ok" } else { "FAIL" }, cbs.join(","), res.join(","), inputs.join(",")), stage_name(ty), created))
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        println!("usage: fxc_dump <fxc_dir> <out_dir>");
        std::process::exit(2);
    }

    let (input, out) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let asm_dir = args.get(2).map(PathBuf::from);
    let _ = std::fs::create_dir_all(&out);
    if let Some(dir) = &asm_dir {
        let _ = std::fs::create_dir_all(dir);
    }

    // SAFETY: before other threads; the device outlives its uses.
    let dev = unsafe {
        init_platform();
        let mut dev = None;
        match D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&mut dev), None, None).ok() {
            Ok(()) if dev.is_some() => dev.expect("checked"),
            _ => {
                println!("no D3D11 hardware device");
                std::process::exit(1);
            }
        }
    };

    let mut index = "file\tprefix\tsource\tstage\tentry\tversion\tcreate\tcbuffers\tresources\tinputs\n".to_owned();
    let mut layouts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut total, mut failed) = (0, 0);
    let mut per_stage: BTreeMap<&str, i32> = BTreeMap::new();

    // In directory order, as the C++ tool listed them.
    for de in std::fs::read_dir(&input).into_iter().flatten().flatten() {
        let path = de.path();
        let fname = de.file_name().to_string_lossy().into_owned();
        if !fname.starts_with("dx11") || path.extension().is_none_or(|x| x != "fxc") {
            continue;
        }

        total += 1;
        let nm = split_fxc_name(&fname).unwrap_or_default();
        let mut row = format!("{fname}\t{}\t{}\t", nm.prefix, nm.source);
        // SAFETY: the device is live.
        match unsafe { shader_row(&dev, &path, asm_dir.as_deref(), &nm.source, &mut layouts) } {
            Ok((rest, stage, created)) => {
                if !created {
                    failed += 1;
                }

                *per_stage.entry(stage).or_default() += 1;
                let (version, tail) = rest.split_once('\t').unwrap_or((&rest, ""));
                row += &format!("{stage}\t{}\t{version}\t{tail}", nm.entry);
            }

            Err(e) => {
                failed += 1;
                row += &format!("?\t{}\t\tERROR {e}\t\t\t", nm.entry);
            }
        }

        index += &row;
        index.push('\n');
    }

    let _ = std::fs::write(out.join("index.tsv"), index);
    let mut cbo = String::new();
    for (lay, users) in &layouts {
        cbo += lay;
        cbo += "  used by:";
        for u in users {
            cbo += " ";
            cbo += u;
        }

        cbo += "\n\n";
    }

    let _ = std::fs::write(out.join("cbuffers.txt"), cbo);
    print!("{total} shaders, {failed} failed; ");
    for (s, n) in &per_stage {
        print!("{s}={n} ");
    }

    println!("\n{} distinct cbuffer layouts", layouts.len());

    // Released before the exit: DXVK may still be compiling the shaders in the background.
    drop(dev);
    std::process::exit(i32::from(failed != 0));
}
