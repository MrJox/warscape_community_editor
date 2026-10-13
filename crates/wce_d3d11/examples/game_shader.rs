//! A game shader of Attila, read from its packs with rpfm_lib, reflected with the D3D compiler and drawn with D3D11:
//! the check that the whole chain works (on Linux, DXVK-native and vkd3d-utils).
//!
//!   WCE_GAME=<Total War Attila folder> cargo run -p wce_d3d11 --example game_shader

use std::ffi::{CStr, c_void};
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::{Decodeable, fxc::Fxc, pack::Pack};
use rpfm_lib::games::supported_games::{KEY_ATTILA, SupportedGames};
use windows_core::{Interface, PCSTR};

use wce_d3d11::*;

const SHADER: &str = "fxc/dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen.fxc";
const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;

const FULLSCREEN_VS: &str = "struct O { float4 p : SV_Position; float4 t : TEXCOORD0; };
O main(uint id : SV_VertexID) { O o; float2 uv = float2((id << 1) & 2, id & 2); o.p = float4(uv * float2(2, -2) + float2(-1, 1), 0, 1); o.t = float4(uv, 0, 0); return o; }";

fn main() -> windows_core::Result<()> {
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };

    let started = std::time::Instant::now();
    let games = SupportedGames::default();
    let game = games.game(KEY_ATTILA).unwrap();
    let mut pack = Pack::read_and_merge_ca_packs(game, &PathBuf::from(std::env::var("WCE_GAME").expect("WCE_GAME must be the game folder"))).unwrap();
    let file = pack.files_mut().get_mut(SHADER).unwrap();
    let bytes = file.encode(&None, false, false, true).unwrap().unwrap();
    let fxc = Fxc::decode(&mut Cursor::new(&bytes), &None).unwrap();
    let dxbc = fxc.shader();
    println!("read {SHADER} from the packs in {:.2} s ({} bytes of DXBC)", started.elapsed().as_secs_f32(), dxbc.len());

    unsafe {
        reflect(dxbc)?;

        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, None, D3D11_SDK_VERSION as u32, Some(&mut device), None, Some(&mut context)).ok()?;
        let (device, context) = (device.unwrap(), context.unwrap());

        let vs_code = compile(FULLSCREEN_VS, "vs_4_0")?;
        let mut vs = None;
        device.CreateVertexShader(blob_bytes(&vs_code), None, Some(&mut vs)).ok()?;
        let mut ps = None;
        device.CreatePixelShader(dxbc, None, Some(&mut ps)).ok()?;

        // camera_VS_PS (cb0): identity matrices are enough for the sky generation. skydome_PS (cb1): a blue sky, a warm sun up and ahead.
        let mut camera = [0f32; 576 / 4];
        for matrix in [16, 80, 144, 208, 272, 336] {
            for i in 0..4 { camera[matrix / 4 + i * 5] = 1.0; }
        }
        let mut skydome = [0f32; 112 / 4];
        skydome[0..3].copy_from_slice(&[0.10, 0.25, 0.60]);   // g_linear_sky_colour_scaled
        skydome[4..7].copy_from_slice(&[1.00, 0.80, 0.50]);   // g_linear_circumsolar_color_scaled
        skydome[8..11].copy_from_slice(&[0.50, 0.55, 0.60]);  // g_linear_horizon_colour_scaled
        skydome[12..15].copy_from_slice(&[0.30, 0.35, 0.45]); // g_linear_horizon_opposite_colour_scaled
        skydome[16..19].copy_from_slice(&[0.20, 0.15, 0.10]); // g_linear_backscatter_colour_scaled
        skydome[19] = 3.0;                                     // g_horizon_decay
        skydome[20] = 8.0;                                     // g_circumsolar_decay
        skydome[21] = 2.0;                                     // g_backscatter_decay
        skydome[24..27].copy_from_slice(&[0.0, 0.5, 0.866]);  // g_sun_direction
        let cb0 = constant_buffer(&device, &camera)?;
        let cb1 = constant_buffer(&device, &skydome)?;

        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: WIDTH, Height: HEIGHT, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };
        let mut target = None;
        device.CreateTexture2D(&desc, None, Some(&mut target)).ok()?;
        let target = target.unwrap();
        let mut rtv = None;
        device.CreateRenderTargetView(&target, None, Some(&mut rtv)).ok()?;

        context.OMSetRenderTargets(Some(&[rtv.clone()]), None);
        context.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: WIDTH as f32, Height: HEIGHT as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
        context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST));
        context.VSSetShader(vs.as_ref(), None);
        context.PSSetShader(ps.as_ref(), None);
        context.PSSetConstantBuffers(0, Some(&[Some(cb0), Some(cb1)]));
        context.Draw(3, 0);

        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ as u32;
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging)).ok()?;
        let staging = staging.unwrap();
        context.CopyResource(&staging, &target);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).ok()?;
        let row = |y: u32| std::slice::from_raw_parts((mapped.pData as *const u8).add((y * mapped.RowPitch) as usize), (WIDTH * 4) as usize).to_vec();
        for y in [0, HEIGHT / 2, HEIGHT - 1] {
            let pixels = row(y);
            println!("row {y:2}: left {:?} middle {:?} right {:?}", &pixels[0..4], &pixels[(WIDTH * 2) as usize..(WIDTH * 2 + 4) as usize], &pixels[((WIDTH - 1) * 4) as usize..]);
        }
        context.Unmap(&staging, 0);
    }

    Ok(())
}

/// Prints the reflection of a shader, in the layout of WCE's fxc_dump.
unsafe fn reflect(dxbc: &[u8]) -> windows_core::Result<()> {
    unsafe {
        let mut reflection: *mut c_void = std::ptr::null_mut();
        D3DReflect(dxbc.as_ptr() as *const c_void, dxbc.len(), &ID3D11ShaderReflection::IID, &mut reflection).ok()?;
        let reflection = ID3D11ShaderReflection::from_raw(reflection);
        let mut desc = D3D11_SHADER_DESC::default();
        reflection.GetDesc(&mut desc).ok()?;

        for index in 0..desc.ConstantBuffers {
            let buffer = reflection.GetConstantBufferByIndex(index).unwrap();
            let mut buffer_desc = D3D11_SHADER_BUFFER_DESC::default();
            buffer.GetDesc(&mut buffer_desc).ok()?;
            println!("cbuffer {} ({} bytes)", CStr::from_ptr(buffer_desc.Name.0 as *const _).to_string_lossy(), buffer_desc.Size);
            for variable in 0..buffer_desc.Variables {
                let mut variable_desc = D3D11_SHADER_VARIABLE_DESC::default();
                buffer.GetVariableByIndex(variable).unwrap().GetDesc(&mut variable_desc).ok()?;
                println!("  +{}\t{}\t{}", variable_desc.StartOffset, variable_desc.Size, CStr::from_ptr(variable_desc.Name.0 as *const _).to_string_lossy());
            }
        }

        let mut bindings = vec![];
        for index in 0..desc.BoundResources {
            let mut bind = D3D11_SHADER_INPUT_BIND_DESC::default();
            reflection.GetResourceBindingDesc(index, &mut bind).ok()?;
            bindings.push(format!("{}=slot{}", CStr::from_ptr(bind.Name.0 as *const _).to_string_lossy(), bind.BindPoint));
        }
        println!("resources: {} | inputs: {}", bindings.join(","), desc.InputParameters);
    }

    Ok(())
}

/// Compiles helper HLSL with vkd3d.
unsafe fn compile(source: &str, target: &str) -> windows_core::Result<ID3D10Blob> {
    unsafe {
        let mut code = None;
        let mut errors = None;
        let entry = c"main";
        let target = std::ffi::CString::new(target).unwrap();
        let result = D3DCompile(source.as_ptr() as *const c_void, source.len(), PCSTR::null(), None, None, PCSTR(entry.as_ptr() as *const u8), PCSTR(target.as_ptr() as *const u8), 0, 0, &mut code, &mut errors);
        if let Some(errors) = errors {
            eprintln!("{}", String::from_utf8_lossy(blob_bytes(&errors)));
        }

        result.ok()?;
        Ok(code.unwrap())
    }
}

unsafe fn blob_bytes(blob: &ID3D10Blob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }
}

unsafe fn constant_buffer(device: &ID3D11Device, data: &[f32]) -> windows_core::Result<ID3D11Buffer> {
    unsafe {
        let desc = D3D11_BUFFER_DESC { ByteWidth: (data.len() * 4) as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
        let init = D3D11_SUBRESOURCE_DATA { pSysMem: data.as_ptr() as *const c_void, SysMemPitch: 0, SysMemSlicePitch: 0 };
        let mut buffer = None;
        device.CreateBuffer(&desc, Some(&init), Some(&mut buffer)).ok()?;
        Ok(buffer.unwrap())
    }
}
