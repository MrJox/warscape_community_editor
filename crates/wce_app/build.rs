//! The binary runs against the DXVK libraries next to wce_d3d11's: their folder goes into the RPATH.

fn main() {
    if let Ok(dir) = std::env::var("DEP_WCE_D3D11_LIB_DIR") {
        println!("cargo:rustc-link-arg-bins=-Wl,--disable-new-dtags,-rpath,{dir}");
    }
}
