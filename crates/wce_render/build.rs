//! Linux: the RPATH to DXVK-native and vkd3d-utils for this crate's examples and tests (wce_d3d11's own link arguments don't reach them).

fn main() {
    if let Ok(dir) = std::env::var("DEP_WCE_D3D11_LIB_DIR") {
        for kind in ["examples", "tests"] {
            println!("cargo:rustc-link-arg-{kind}=-Wl,--disable-new-dtags,-rpath,{dir}");
        }
    }
}
