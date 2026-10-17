//! Linux: the D3D11 and DXGI functions come from DXVK-native, and the shader compiler from vkd3d-utils, both built by
//! cmake/linux_deps.cmake into <build>/_deps/prefix. WCE_DEPS_PREFIX overrides that folder. Windows links the system DLLs by itself.

fn main() {
    println!("cargo:rerun-if-env-changed=WCE_DEPS_PREFIX");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let prefix = std::env::var("WCE_DEPS_PREFIX").unwrap_or_else(|_| format!("{}/../../build/_deps/prefix", env!("CARGO_MANIFEST_DIR")));
    let lib = std::path::Path::new(&prefix).join("lib");
    if !lib.join("libdxvk_d3d11.so").exists() {
        panic!("DXVK-native is not in {}: configure the CMake build first (README, Linux), or set WCE_DEPS_PREFIX", lib.display());
    }

    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=dxvk_d3d11");
    println!("cargo:rustc-link-lib=dylib=dxvk_dxgi");
    println!("cargo:rustc-link-lib=dylib=vkd3d-utils");

    // An RPATH, not a RUNPATH: libdxvk_d3d11.so finds libdxvk_dxgi.so through it too. Link arguments only reach this crate's own
    // binaries: crates with binaries read the folder from `DEP_WCE_D3D11_LIB_DIR` (see `rpath` in this crate) and add it to theirs.
    println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags,-rpath,{}", lib.display());
    println!("cargo:lib_dir={}", lib.display());
}
