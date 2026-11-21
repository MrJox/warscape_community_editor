//! On Linux, the binary runs against the DXVK libraries next to wce_d3d11's: their folder goes into the RPATH.
//! On Windows (MSVC), the app's icon (packaging/windows/wce.ico, made by scripts/make_icon.sh) is embedded in wce.exe: Explorer shows it,
//! and SDL gives the window the exe's first icon (its title bar, the taskbar, Alt+Tab).

use std::path::{Path, PathBuf};

fn main() {
    if let Ok(dir) = std::env::var("DEP_WCE_D3D11_LIB_DIR") {
        println!("cargo:rustc-link-arg-bins=-Wl,--disable-new-dtags,-rpath,{dir}");
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
        let ico = manifest_dir.join("../../packaging/windows/wce.ico");
        println!("cargo:rerun-if-changed={}", ico.display());
        let res = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("wce.res");
        write_icon_res(&ico, &res);
        // link.exe (and lld-link) take a compiled resource file as an input, as rc.exe's output would be.
        println!("cargo:rustc-link-arg-bins={}", res.display());
    }
}

/// `RT_ICON` and `RT_GROUP_ICON`, the resource types of an icon's images and of its directory.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// The memory flags rc.exe gives them: moveable and discardable; the group pure too.
const ICON_FLAGS: u16 = 0x1010;
const GROUP_ICON_FLAGS: u16 = 0x1030;

/// Writes the compiled resource file (.res) that `1 ICON "wce.ico"` in an .rc file makes: each image of the ICO as an `RT_ICON` resource
/// (named 1, 2, ...), and the ICO's directory as the `RT_GROUP_ICON` resource 1, its entries naming the images instead of their offsets.
/// Done here rather than with rc.exe, which is not on the PATH outside a Visual Studio prompt, and with no crate for it.
fn write_icon_res(ico: &Path, res: &Path) {
    let data = std::fs::read(ico).unwrap_or_else(|e| panic!("reading {}: {e}", ico.display()));
    let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    assert!(data.len() >= 6 && u16_at(0) == 0 && u16_at(2) == 1, "{} is not an ICO file", ico.display());
    let count = u16_at(4);

    // The empty resource every .res file starts with.
    let mut out = vec![];
    push_resource(&mut out, 0, 0, 0, 0, &[]);

    let mut group = vec![];
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&count.to_le_bytes());
    for i in 0..count {
        let entry = 6 + 16 * usize::from(i);
        let (size, offset) = (u32_at(entry + 8) as usize, u32_at(entry + 12) as usize);
        let image = data.get(offset..offset + size).unwrap_or_else(|| panic!("{}: image {i} is past the end of the file", ico.display()));
        let id = i + 1;
        push_resource(&mut out, RT_ICON, id, ICON_FLAGS, 0, image);
        // Width, height, colour count, reserved, planes, bit count and size as in the ICO; the image's id in place of its offset.
        group.extend_from_slice(&data[entry..entry + 12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut out, RT_GROUP_ICON, 1, GROUP_ICON_FLAGS, 0, &group);

    std::fs::write(res, out).unwrap_or_else(|e| panic!("writing {}: {e}", res.display()));
}

/// Appends one resource: its header (type and name as ordinals, the language neutral) and its data, padded to 4 bytes.
fn push_resource(out: &mut Vec<u8>, kind: u16, name: u16, memory_flags: u16, language: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}
