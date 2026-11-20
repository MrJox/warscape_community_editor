//! Writes one frame of a `.ca_vp8` movie (a file, or one inside a pack) as a PNG: a look at what a movie export made.
//!
//!   video_frame_png <movie.ca_vp8 | mod.pack> <frame> <out.png>

use std::path::Path;

use wce_formats::png_file::PngFile;
use wce_video::{Decoder, Movie, yuv};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        eprintln!("video_frame_png <movie.ca_vp8 | mod.pack> <frame> <out.png>");
        std::process::exit(2);
    }
    let bytes = if a[0].ends_with(".pack") {
        let files = wce_walls::pack::read_pack_files(Path::new(&a[0]), "attila").expect("the pack");
        files.into_iter().find(|(p, _)| p.ends_with(".ca_vp8")).map(|(_, b)| b).expect("a movie in the pack")
    } else {
        std::fs::read(&a[0]).expect("the movie")
    };
    let movie = Movie::read(&bytes).expect("a movie");
    let want: usize = a[1].parse().expect("a frame number");
    let mut dec = Decoder::new(2).expect("decoder");
    let mut last = None;
    for i in movie.key_frame_before(want)..=want.min(movie.frame_count() - 1) {
        if let Some(p) = dec.decode(movie.frame(i).unwrap()).expect("decode") {
            last = Some(p);
        }
    }
    let p = last.expect("a picture");
    let rgba = yuv::to_rgba(&p);
    let png = PngFile {
        width: p.width,
        height: p.height,
        bit_depth: 8,
        color_type: 2,
        pixels: rgba.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect(),
        ..Default::default()
    };
    std::fs::write(&a[2], png.encode().expect("png")).expect("write");
    println!("{} x {}, frame {want} of {}, {:.3} fps", p.width, p.height, movie.frame_count(), movie.fps());
}
