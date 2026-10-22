//! Decodes a DDS mip to raw RGBA: a header of width and height (u32 little endian each), then the rows (for scripts that compare maps).
//!
//!   cargo run --release -p wce_formats --example dds_to_rgba -- <in.dds> <out.rgba> [mip]

use wce_formats::dds_decode::dds_decode_mip;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(out)) = (a.first(), a.get(1)) else {
        eprintln!("dds_to_rgba <in.dds> <out.rgba> [mip]");
        std::process::exit(2);
    };

    let mip = a.get(2).and_then(|m| m.parse().ok()).unwrap_or(0);
    let img = dds_decode_mip(&std::fs::read(input).expect("the DDS file"), mip).expect("a DDS this decoder reads");
    let mut bytes = Vec::with_capacity(8 + img.px.len());
    bytes.extend(img.w.to_le_bytes());
    bytes.extend(img.h.to_le_bytes());
    bytes.extend(&img.px);
    std::fs::write(out, bytes).expect("written");
    println!("{input}: {} x {} -> {out}", img.w, img.h);
}
