//! Two games' shaders side by side: which `fxc/*.fxc` one has and the other doesn't, which have the same bytecode, and for the rest
//! how their reflection differs (constant buffers and their variables, bound resources, the stage's inputs and outputs). What a port
//! of the renderer to a game of the same engine has to look at.
//!
//!   cargo run --release -p wce_d3d11 --example shader_diff -- <game A folder> <key A> <game B folder> <key B> [name part]...
//!
//! Only the shaders whose names contain every part are compared; a part `@<file>` instead compares the shaders named in the file (one
//! name a line, without folder and extension).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, c_void};
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::{Decodeable, fxc::Fxc};
use windows_core::Interface;
use wce_d3d11::*;
use wce_vfs::{Config, GameProfile, Vfs};

fn name(p: windows_core::PCSTR) -> String {
    // SAFETY: the reflection's names are NUL-terminated strings alive with it.
    unsafe { CStr::from_ptr(p.0 as *const _).to_string_lossy().into_owned() }
}

/// A shader as compared: its bytecode and its reflection, one fact a line.
struct Shader {
    bytecode: Vec<u8>,
    facts: BTreeSet<String>,
}

fn read(vfs: &Vfs, path: &str) -> Option<Shader> {
    let bytes = vfs.read(path).ok()?;
    let fxc = Fxc::decode(&mut Cursor::new(&bytes), &None).ok()?;
    let bytecode = fxc.shader().clone();
    let mut facts = BTreeSet::new();

    // SAFETY: the reflection reads the bytecode, alive during the block; descriptions are locals.
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        if D3DReflect(bytecode.as_ptr() as *const c_void, bytecode.len(), &ID3D11ShaderReflection::IID, &mut raw).is_err() {
            facts.insert("reflection failed".to_owned());
            return Some(Shader { bytecode, facts });
        }

        let r = ID3D11ShaderReflection::from_raw(raw);
        let mut desc = D3D11_SHADER_DESC::default();
        if r.GetDesc(&mut desc).is_err() {
            return Some(Shader { bytecode, facts });
        }

        facts.insert(format!("version {:#x}", desc.Version));
        for i in 0..desc.BoundResources {
            let mut b = D3D11_SHADER_INPUT_BIND_DESC::default();
            if r.GetResourceBindingDesc(i, &mut b).is_ok() {
                facts.insert(format!("resource {:?} {} slot {} count {} dim {:?}", b.Type, name(b.Name), b.BindPoint, b.BindCount, b.Dimension));
            }
        }

        for i in 0..desc.ConstantBuffers {
            let Some(cb) = r.GetConstantBufferByIndex(i) else { continue };
            let mut cd = D3D11_SHADER_BUFFER_DESC::default();
            if cb.GetDesc(&mut cd).is_err() {
                continue;
            }

            let cb_name = name(cd.Name);
            facts.insert(format!("cbuffer {cb_name} {} bytes", cd.Size));
            for v in 0..cd.Variables {
                let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                if let Some(var) = cb.GetVariableByIndex(v)
                    && var.GetDesc(&mut vd).is_ok()
                {
                    facts.insert(format!("cbuffer {cb_name} +{} {} {}{}", vd.StartOffset, vd.Size, name(vd.Name), if vd.uFlags & 2 != 0 { "" } else { " (unused)" }));
                }
            }
        }

        for i in 0..desc.InputParameters {
            let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
            if r.GetInputParameterDesc(i, &mut p).is_ok() {
                facts.insert(format!("in {}{} reg {} mask {:#x} used {:#x}", name(p.SemanticName), p.SemanticIndex, p.Register, p.Mask, p.ReadWriteMask));
            }
        }

        for i in 0..desc.OutputParameters {
            let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
            if r.GetOutputParameterDesc(i, &mut p).is_ok() {
                facts.insert(format!("out {}{} reg {} mask {:#x}", name(p.SemanticName), p.SemanticIndex, p.Register, p.Mask));
            }
        }
    }

    Some(Shader { bytecode, facts })
}

fn main() {
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [dir_a, key_a, dir_b, key_b, parts @ ..] = args.as_slice() else {
        eprintln!("shader_diff <game A folder> <key A> <game B folder> <key B> [name part]...");
        std::process::exit(2);
    };

    let mount = |dir: &str, key: &str| {
        let profile = GameProfile::by_key(key).unwrap_or_else(|| panic!("no game {key}"));
        Vfs::mount(&Config::new(PathBuf::from(dir), profile.clone())).expect("the game's packs")
    };
    let (a, b) = (mount(dir_a, key_a), mount(dir_b, key_b));
    let names = |vfs: &Vfs| -> BTreeSet<String> {
        match parts.first().and_then(|p| p.strip_prefix('@')) {
            Some(file) => std::fs::read_to_string(file).expect("the list").lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect(),
            None => vfs
                .list("fxc/", ".fxc")
                .into_iter()
                .filter(|p| parts.iter().all(|part| p.contains(part.as_str())))
                .map(|p| p.trim_start_matches("fxc/").trim_end_matches(".fxc").to_owned())
                .collect(),
        }
    };
    let all: BTreeSet<String> = names(&a).union(&names(&b)).cloned().collect();

    let (mut only_a, mut only_b, mut same, mut same_reflection) = (vec![], vec![], 0, 0);
    let mut differ = BTreeMap::new();
    for n in &all {
        let path = format!("fxc/{n}.fxc");
        match (read(&a, &path), read(&b, &path)) {
            (Some(x), Some(y)) if x.bytecode == y.bytecode => same += 1,
            (Some(x), Some(y)) if x.facts == y.facts => same_reflection += 1,
            (Some(x), Some(y)) => {
                differ.insert(n.clone(), (x.facts.difference(&y.facts).cloned().collect::<Vec<_>>(), y.facts.difference(&x.facts).cloned().collect::<Vec<_>>()));
            }

            (Some(_), None) => only_a.push(n.clone()),
            (None, Some(_)) => only_b.push(n.clone()),
            (None, None) => {}
        }
    }

    println!("{} shaders: {same} the same bytecode, {same_reflection} another bytecode with the same reflection, {} another reflection, {} only in {key_a}, {} only in {key_b}",
        all.len(), differ.len(), only_a.len(), only_b.len());
    for n in &only_a {
        println!("only in {key_a}: {n}");
    }

    for n in &only_b {
        println!("only in {key_b}: {n}");
    }

    for (n, (gone, new)) in &differ {
        println!("== {n}");
        for f in gone {
            println!("  - {f}");
        }

        for f in new {
            println!("  + {f}");
        }
    }
}
