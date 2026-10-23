//! Stage B's terrain mesh against a fresh BOB run: the raw tile's surface (`wce_tile_io::attila::terrain_mesh`) against the surface
//! triangles of BOB's `mesh.rigid_model_v2` (the fringe left out), compared as sets of triangles by their corners' (x, z).
//!
//!   cargo run --release -p wce_tile_io --example terrain_mesh_check -- <raw tile folder> <BOB's working tile folder> [<kit raw_data>]
//!
//! With the kit's `raw_data`, the buildings' height meshes are stamped into the CHMF (`battlefield_buildings.height_map_mesh`,
//! `models_building.cs2_file`).
//!
//! `WCE_SHOW=1` prints a few triangles found on one side only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wce_tile_io::attila::TedProject;
use wce_formats::chmf::ChmfFile;
use wce_formats::cas2::Cas2File;
use wce_tile_io::attila::bob_tile::{CELL, HeightNumbers, chmf, final_heights, influence};
use wce_tile_io::attila::compile::buildings;
use wce_tile_io::attila::terrain_mesh::{add_fringe, cos_degrees, grid_flags, grid_triangles, grid_vertices, merge, protection_map, rasterise, rasterise_max_heights, rmv2, stamp_matrix, HeightMesh};
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

/// BOB's triangles in order, corners as (x, z, below the surface).
fn bob_list(b: &[u8]) -> Vec<[(i32, i32, bool); 3]> {
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (nv, ni) = (u32_at(0xac), u32_at(0xb4));
    let vs = b.len() - ni * 2 - nv * 8;
    let f16 = |o: usize| half(u16::from_le_bytes([b[o], b[o + 1]]));
    let pos = (0..nv).map(|i| (f16(vs + i * 8) as i32, f16(vs + i * 8 + 4) as i32, f16(vs + i * 8 + 2) < -5.0)).collect::<Vec<_>>();
    (0..ni / 3).map(|t| [0, 1, 2].map(|k| pos[u16::from_le_bytes([b[vs + nv * 8 + (t * 3 + k) * 2], b[vs + nv * 8 + (t * 3 + k) * 2 + 1]]) as usize])).collect()
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
    let alpha = influence(CELL, (w - 1) / CELL, (h - 1) / CELL);
    let mut verts = grid_vertices(&heights, &alpha, w, h, n.normal_strength);
    let prot = protection_map(&project);
    let mut flags = grid_flags(w, h, prot.as_ref().map(|(p, pw, ph)| (p.as_slice(), *pw, *ph)));
    let mut grid = grid_triangles(w, h, 1);
    add_fringe(&mut verts, &mut flags, &mut grid, w, h, 1);
    let t0 = std::time::Instant::now();
    let all = merge(&verts, &flags, &grid, cos_degrees(20.0));
    let tris = all.iter().copied().filter(|t| !t.iter().any(|&i| verts[i as usize].fringe)).collect::<Vec<_>>();
    let ours_mesh = rmv2(&verts, &all, tris.len(), ((w - 1) as u32, (h - 1) as u32), w as u32);
    let theirs_mesh = std::fs::read(work.join("mesh.rigid_model_v2")).expect("BOB's mesh");
    // BOB leaves the shader name's tail uninitialised.
    let masked = |b: &[u8]| b.iter().enumerate().map(|(i, &v)| if (0xde..0xf0).contains(&i) { 0 } else { v }).collect::<Vec<_>>();
    let (om, tm) = (masked(&ours_mesh), masked(&theirs_mesh));
    let diff = om.iter().zip(&tm).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| i).collect::<Vec<_>>();
    println!("mesh.rigid_model_v2: {} bytes ours, {} BOB's; {} bytes differ (shader name tail masked){}", om.len(), tm.len(), diff.len(),
        diff.first().map(|i| format!(", first at {i:#x}")).unwrap_or_default());
    if std::env::var_os("WCE_SHOW").is_some() {
        for &i in diff.iter().filter(|&&i| i >= 0x148 && (i - 0x148) % 2 == 0).take(12) {
            let (o, t) = (u16::from_le_bytes([om[i], om[i + 1]]), u16::from_le_bytes([tm[i], tm[i + 1]]));
            let v = (i - 0x148) / 8;
            let at = |b: &[u8], k: usize| half(u16::from_le_bytes([b[0x148 + v * 8 + k], b[0x148 + v * 8 + k + 1]]));
            println!("  vertex {v} field {}: ours {o:#06x} ({}) BOB {t:#06x} ({}) at x {} z {} h {}", (i - 0x148) % 8 / 2, half(o), half(t), at(&om, 0), at(&om, 4), at(&om, 2));
        }
    }
    // The whole list in order, corners as (x, z, bottom), against BOB's.
    let ours_all = all.iter().map(|t| t.map(|i| (verts[i as usize].x as i32, verts[i as usize].z as i32, verts[i as usize].fringe))).collect::<Vec<_>>();
    let theirs_all = bob_list(&std::fs::read(work.join("mesh.rigid_model_v2")).expect("BOB's mesh"));
    let first = ours_all.iter().zip(&theirs_all).position(|(a, b)| a != b);
    println!("index list: {} triangles ours, {} BOB's; first difference at {first:?}", ours_all.len(), theirs_all.len());
    if let Some(i) = first {
        for k in i.saturating_sub(2)..(i + 3).min(ours_all.len()).min(theirs_all.len()) {
            println!("  {k}: ours {:?} BOB {:?}", ours_all[k], theirs_all[k]);
        }
    }
    println!("merged {} grid triangles to {} in {:?}", 2 * (w - 1) * (h - 1), tris.len(), t0.elapsed());
    if let Ok(path) = std::env::var("WCE_DUMP") {
        let text = tris.iter().map(|t| t.map(|i| format!("{} {}", verts[i as usize].x, verts[i as usize].z)).join(" ")).collect::<Vec<_>>().join("\n");
        std::fs::write(path, text).expect("dump");
    }

    let ours = tris.iter().map(|t| key(t.map(|i| (verts[i as usize].x as i32, verts[i as usize].z as i32)))).collect::<BTreeSet<_>>();
    let theirs = bob_surface(&std::fs::read(work.join("mesh.rigid_model_v2")).expect("BOB's mesh"));
    let (only_ours, only_theirs) = (ours.difference(&theirs).collect::<Vec<_>>(), theirs.difference(&ours).collect::<Vec<_>>());
    println!("surface: {} ours, {} BOB's; {} only ours, {} only BOB's", ours.len(), theirs.len(), only_ours.len(), only_theirs.len());
    let mut field = rasterise(&verts, &tris, w, h);
    if let Some(kit) = args.get(2) {
        let stamped = stamp_buildings(&project, kit, &mut field, w, h);
        println!("stamped {stamped} buildings");
    }

    let (ours, theirs) = (chmf(&field, w, h), std::fs::read(work.join("hf_height_map.data")).expect("BOB's CHMF"));
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

/// The records of a kit table (`raw_data/db/<table>.xml`): per record its fields.
fn table(kit: &Path, name: &str) -> Vec<Vec<(String, String)>> {
    let text = std::fs::read_to_string(kit.join("db").join(format!("{name}.xml"))).unwrap_or_default();
    let open = format!("<{name} ");
    text.split(&open).skip(1).map(|rec| {
        let rec = rec.split(&format!("</{name}>")).next().unwrap_or_default();
        rec.split('<').filter_map(|t| {
            let (tag, value) = t.split_once('>')?;
            (!tag.starts_with('/') && !tag.contains(' ')).then(|| (tag.to_owned(), value.to_owned()))
        }).collect()
    }).collect()
}

/// A file of the kit by its path, whatever the case of its name (BOB runs on Windows).
fn find(kit: &Path, rel: &str) -> Option<PathBuf> {
    let p = kit.join(rel.replace('\\', "/"));
    let dir = p.parent()?;
    let name = p.file_name()?.to_str()?.to_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|e| e.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase() == name))
}

fn stamp_buildings(project: &wce_tile_model::Tile, kit: &Path, field: &mut [f32], w: usize, h: usize) -> usize {
    let get = |r: &Vec<(String, String)>, k: &str| r.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let with_mesh = table(kit, "battlefield_buildings").iter().filter(|r| get(r, "height_map_mesh") == "1").map(|r| get(r, "key")).collect::<BTreeSet<_>>();
    let cs2 = table(kit, "models_building").iter().map(|r| (get(r, "key"), get(r, "cs2_file"))).collect::<std::collections::HashMap<_, _>>();
    let mut meshes = std::collections::HashMap::new();
    let mut n = 0;
    for (key, world) in buildings(project) {
        if !with_mesh.contains(&key) {
            continue;
        }

        let mesh = meshes.entry(key.clone()).or_insert_with(|| {
            let path = cs2.get(&key).and_then(|c| find(kit, c))?;
            Some(HeightMesh::of(&Cas2File::parse(&std::fs::read(path).ok()?).ok()?))
        });
        match mesh {
            Some(m) => {
                rasterise_max_heights(m, &stamp_matrix(&world), field, w, h);
                n += 1;
            }
            None => println!("  {key}: no height mesh file"),
        }
    }

    n
}
