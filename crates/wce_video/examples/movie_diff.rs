//! Compares two movies (files or packs) frame by frame: the first frames that differ and by how much.
//!
//!   movie_diff <a> <b>

use std::path::Path;

use wce_video::{Decoder, Movie};

fn load(p: &str) -> Movie {
    let bytes = if p.ends_with(".pack") {
        let files = wce_walls::pack::read_pack_files(Path::new(p), "attila").expect("pack");
        files.into_iter().find(|(p, _)| p.ends_with(".ca_vp8")).map(|(_, b)| b).expect("movie")
    } else {
        std::fs::read(p).expect("movie")
    };
    Movie::read(&bytes).expect("movie")
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (ma, mb) = (load(&a[0]), load(&a[1]));
    let (mut da, mut db) = (Decoder::new(1).unwrap(), Decoder::new(1).unwrap());
    let mut shown = 0;
    for i in 0..ma.frame_count().min(mb.frame_count()) {
        let same_bits = ma.frame(i).unwrap() == mb.frame(i).unwrap();
        let pa = da.decode(ma.frame(i).unwrap()).unwrap();
        let pb = db.decode(mb.frame(i).unwrap()).unwrap();
        if let (Some(pa), Some(pb)) = (pa, pb) {
            let diff = pa.y.iter().zip(&pb.y).filter(|(x, y)| x != y).count();
            if (diff > 0 || !same_bits) && shown < 12 {
                println!("frame {i}: bits {} luma pixels differing {diff}", if same_bits { "same" } else { "differ" });
                shown += 1;
            }
        }
    }
    println!("frames {} / {}", ma.frame_count(), mb.frame_count());
}
