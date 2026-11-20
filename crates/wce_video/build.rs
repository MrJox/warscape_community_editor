//! Builds libvpx's VP8 encoder and decoder (third_party/libvpx, plain C: scripts/vendor_libvpx.py) and the crate's shim with cc.
//! Always optimised: an unoptimised encoder is too slow even for a debug editor.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let vpx = manifest.join("../../third_party/libvpx");
    let sources = std::fs::read_to_string(vpx.join("sources.txt")).expect("third_party/libvpx/sources.txt");

    let mut build = cc::Build::new();
    build.include(&vpx).opt_level(2).warnings(false);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // vpx_util/vpx_pthread.h's Windows threads need Vista or later.
        build.define("_WIN32_WINNT", "0x0601").define("WIN32_LEAN_AND_MEAN", None);
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // libvpx's C wants the conforming preprocessor and no deprecation noise.
        build.flag("/Zc:preprocessor").define("_CRT_SECURE_NO_WARNINGS", None);
    }
    for line in sources.lines().map(str::trim).filter(|l| !l.is_empty()) {
        build.file(vpx.join(line));
    }
    build.file(manifest.join("src/shim.c"));
    build.compile("wce_vpx");

    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=../../third_party/libvpx/sources.txt");
}
