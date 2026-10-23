//! Stage B's terrain mesh against a fresh BOB run: the raw tile's surface (`wce_tile_io::attila::terrain_mesh`) against the surface
//! triangles of BOB's `mesh.rigid_model_v2` (the fringe left out), compared as sets of triangles by their corners' (x, z).
//!
//!   cargo run --release -p wce_tile_io --example terrain_mesh_check -- <raw tile folder> <BOB's working tile folder>
//!
//! `WCE_SHOW=1` prints a few triangles found on one side only.

use std::collections::BTreeSet;
use std::path::PathBuf;

use wce_tile_io::attila::TedProject;
use wce_formats::chmf::ChmfFile;
use wce_tile_io::attila::bob_tile::{HeightNumbers, chmf, final_heights};
use wce_tile_io::attila::terrain_mesh::{cos_degrees, grid_flags, grid_triangles, grid_vertices, merge, protection_map, rasterise};
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::{MapKind, Samples};

type Tri = [(i32, i32); 3];

fn key(mut t: Tri) -> Tri {
    t.sort();
    t
}

/// BOB's surface triangles: the mesh's vertices (8 bytes each: x, y, z, w as f16) and u16 indices end the file; counts at 0xac, 0xb4.
fn bob_surface(b: &[u8]) -> BTreeSet<Tri> {
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (nv, ni) = (u32_at(0xac), u32_at(0xb4));
    let vs = b.len() - ni * 2 - nv * 8;
    let f16 = |o: usize| half(u16::from_le_bytes([b[o], b[o + 1]]));
    let pos = (0..nv).map(|i| (f16(vs + i * 8), f16(vs + i * 8 + 4))).collect::<Vec<_>>();
    let idx = (0..ni).map(|i| u16::from_le_bytes([b[vs + nv * 8 + i * 2], b[vs + nv * 8 + i * 2 + 1]]) as usize).collect::<Vec<_>>();
    let mut out = BTreeSet::new();
    for t in idx.chunks(3) {
        let p = [pos[t[0]], pos[t[1]], pos[t[2]]];
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
        if area != 0.0 {
            out.insert(key(p.map(|(x, z)| (x as i32, z as i32))));
        }
    }

    out
}

fn half(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f32);
    let v = if e == 0 { m * 2f32.powi(-24) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if s == 1 { -v } else { v }
}

fn main() {
    let args = std::env::args().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    let (raw, work) = (&args[0], &args[1]);
    let files = TileFiles::from_kit_dir(raw).expect("the raw tile");
    let (project, _) = TedProject.read(&files).expect("the project");
    let hm = project.map(MapKind::Height, 0).expect("a height map");
    let Samples::U16(hs) = &hm.raster.samples else { panic!("16-bit heights") };
    let n = HeightNumbers::of(&project);
    let (heights, w, h) = final_heights(hs, hm.raster.width as usize, hm.raster.height as usize, n.scale, n.min_height);
    let verts = grid_vertices(&heights, w, h, n.normal_strength);
    let prot = protection_map(&project);
    let flags = grid_flags(w, h, prot.as_ref().map(|(p, pw, ph)| (p.as_slice(), *pw, *ph)));
    let t0 = std::time::Instant::now();
    let tris = merge(&verts, &flags, &grid_triangles(w, h, 1), cos_degrees(20.0));
    println!("merged {} grid triangles to {} in {:?}", 2 * (w - 1) * (h - 1), tris.len(), t0.elapsed());
    let ours = tris.iter().map(|t| key(t.map(|i| (verts[i as usize].x as i32, verts[i as usize].z as i32)))).collect::<BTreeSet<_>>();
    let theirs = bob_surface(&std::fs::read(work.join("mesh.rigid_model_v2")).expect("BOB's mesh"));
    let (only_ours, only_theirs) = (ours.difference(&theirs).collect::<Vec<_>>(), theirs.difference(&ours).collect::<Vec<_>>());
    println!("surface: {} ours, {} BOB's; {} only ours, {} only BOB's", ours.len(), theirs.len(), only_ours.len(), only_theirs.len());
    let (ours, theirs) = (chmf(&rasterise(&verts, &tris, w, h), w, h), std::fs::read(work.join("hf_height_map.data")).expect("BOB's CHMF"));
    println!("hf_height_map.data: {}", if ours == theirs { "identical" } else { "differs" });
    let (ours, theirs) = (ChmfFile::parse(&ours).expect("our CHMF"), ChmfFile::parse(&theirs).expect("BOB's CHMF"));
    let ((_, _, os), (_, _, ts)) = (ours.region_samples(0).expect("ours"), theirs.region_samples(0).expect("BOB's"));
    let diff = os.iter().zip(&ts).enumerate().filter(|(_, (a, b))| a != b).collect::<Vec<_>>();
    println!("CHMF: min {} / {}, max {} / {}; {} of {} samples differ", ours.min, theirs.min, ours.max, theirs.max, diff.len(), os.len());
    if std::env::var_os("WCE_SHOW").is_some() {
        for (i, (a, b)) in diff.iter().take(8) {
            println!("  ({}, {}): ours {a}, BOB {b}", i % w, i / w);
        }
        for t in only_ours.iter().take(8) {
            println!("  ours {t:?}");
        }
        for t in only_theirs.iter().take(8) {
            println!("  BOB  {t:?}");
        }
    }
}
