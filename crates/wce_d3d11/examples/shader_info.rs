//! The reflection of a game's shaders (constant buffers and their variables, bound resources, the stage's inputs and outputs), read
//! from the game's packs: what a renderer pass binds, for any game.
//!
//!   cargo run --release -p wce_d3d11 --example shader_info -- <game folder> <name part>...
//!
//! `$WCE_GAME_KEY` chooses the game (`rome_2`; Attila by default). Every `fxc/*.fxc` whose name contains all the parts is shown; a part
//! `@<file>` instead shows the shaders named in the file (one name a line, without folder and extension; a missing one is said so).

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

fn main() {
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (Some(game), parts) = (args.first(), &args[1.min(args.len())..]) else {
        eprintln!("shader_info <game folder> <name part>...");
        std::process::exit(2);
    };

    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::from_env().clone())).expect("the game's packs");
    let paths = match parts.first().and_then(|p| p.strip_prefix('@')) {
        Some(file) => std::fs::read_to_string(file).expect("the list").lines().map(str::trim).filter(|l| !l.is_empty()).map(|n| format!("fxc/{n}.fxc")).collect::<Vec<_>>(),
        None => vfs.list("fxc/", ".fxc").into_iter().filter(|p| parts.iter().all(|part| p.contains(part.as_str()))).map(str::to_owned).collect(),
    };
    for path in &paths {
        let path = path.as_str();
        if vfs.owner(path).is_none() {
            println!("== {path}: missing");
            continue;
        }

        let Ok(bytes) = vfs.read(path) else { continue };
        let Ok(fxc) = Fxc::decode(&mut Cursor::new(&bytes), &None) else {
            println!("== {path}: does not decode");
            continue;
        };

        println!("== {path}");
        // SAFETY: the reflection reads the bytecode, alive during the block; descriptions are locals.
        unsafe {
            let dxbc = fxc.shader();
            let mut raw: *mut c_void = std::ptr::null_mut();
            if D3DReflect(dxbc.as_ptr() as *const c_void, dxbc.len(), &ID3D11ShaderReflection::IID, &mut raw).is_err() {
                println!("  reflection failed");
                continue;
            }

            let r = ID3D11ShaderReflection::from_raw(raw);
            let mut desc = D3D11_SHADER_DESC::default();
            if r.GetDesc(&mut desc).is_err() {
                continue;
            }

            for i in 0..desc.BoundResources {
                let mut b = D3D11_SHADER_INPUT_BIND_DESC::default();
                if r.GetResourceBindingDesc(i, &mut b).is_ok() {
                    println!("  resource {:?} slot {} count {} dim {:?}: {}", b.Type, b.BindPoint, b.BindCount, b.Dimension, name(b.Name));
                }
            }

            for i in 0..desc.ConstantBuffers {
                let Some(cb) = r.GetConstantBufferByIndex(i) else { continue };
                let mut cd = D3D11_SHADER_BUFFER_DESC::default();
                if cb.GetDesc(&mut cd).is_err() {
                    continue;
                }

                println!("  cbuffer {} ({} bytes)", name(cd.Name), cd.Size);
                for v in 0..cd.Variables {
                    let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                    if let Some(var) = cb.GetVariableByIndex(v)
                        && var.GetDesc(&mut vd).is_ok()
                    {
                        println!("    +{}\t{}\t{}{}", vd.StartOffset, vd.Size, name(vd.Name), if vd.uFlags & 2 != 0 { "" } else { "  (unused)" });
                    }
                }
            }

            for i in 0..desc.InputParameters {
                let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
                if r.GetInputParameterDesc(i, &mut p).is_ok() {
                    println!("  in  {}{} reg {} mask {:#x}", name(p.SemanticName), p.SemanticIndex, p.Register, p.Mask);
                }
            }

            for i in 0..desc.OutputParameters {
                let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
                if r.GetOutputParameterDesc(i, &mut p).is_ok() {
                    println!("  out {}{} reg {} mask {:#x}", name(p.SemanticName), p.SemanticIndex, p.Register, p.Mask);
                }
            }
        }
    }
}
