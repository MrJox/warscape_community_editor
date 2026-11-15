//! Checks `mesh::warp_collapse` on the collapses of the game's buildings (the packs and the enabled mods): each collapse's mesh and
//! animation through three warps, posed (every block's vertices by its bone, `wce_formats::anim::RawAnim`):
//!
//! - the identity: the mesh and every frame as they were (rotations within their 16-bit rounding);
//! - a rigid motion (a turn and a shift): every frame's posed vertex the old one moved by it;
//! - a bend: the first frame's posed vertex the warp of the old one (the collapse starts where the warped standing piece stands).
//!
//!   cargo run --release -p wce_walls --example collapse_check -- <game dir> <building key>...

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim::RawAnim;
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::mesh::warp_collapse;
use wce_walls::warp::{Bend, Cuts, Identity, Warp, turn3};

/// A turn about the vertical axis and a shift.
struct Rigid {
    yaw: f32,
    shift: [f32; 3],
}

impl Warp for Rigid {
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let r = turn3(p, self.yaw);
        [r[0] + self.shift[0], r[1] + self.shift[1], r[2] + self.shift[2]]
    }

    fn yaw(&self, _: [f32; 3]) -> f32 {
        self.yaw
    }
}

fn qrot(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (u, w) = ([q[0], q[1], q[2]], q[3]);
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [v[0] + w * t[0] + ut[0], v[1] + w * t[1] + ut[1], v[2] + w * t[2] + ut[2]]
}

fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The first level of detail's triangle corners (plus their pivot), each with its block's bone: read once, posed at any frame.
fn vertices(mesh: &[u8], bones: usize) -> Vec<(usize, [f32; 3])> {
    let m = RigidModel::decode(&mut Cursor::new(mesh), &Some(DecodeableExtraData::default())).expect("mesh");
    let mut out = vec![];
    for block in m.lods()[0].mesh_blocks() {
        let k = usize::try_from(*block.material().i_matrix_index()).ok().filter(|&k| k < bones).unwrap_or(0);
        let pv = block.material().v_pivot();
        // By triangle corner (the index lists): a warp keeps the triangles, not the vertex lists (unused vertices go).
        let vs = block.mesh().vertices();
        out.extend(block.mesh().indices().iter().map(|&i| {
            let v = &vs[i as usize];
            (k, [v.position().x + pv.x, v.position().y + pv.y, v.position().z + pv.z])
        }));
    }

    out
}

/// Those vertices posed at a frame.
fn posed(vs: &[(usize, [f32; 3])], anim: &RawAnim, frame: usize) -> Vec<[f32; 3]> {
    // In the piece's frame: a parent's frame times the local one (as the editor's playback composes them).
    let mut bones: Vec<([f32; 3], [f32; 4])> = vec![];
    for k in 0..anim.bones.len() {
        let (t, q) = anim.local(frame, k);
        bones.push(match usize::try_from(anim.parents[k]).ok().filter(|&p| p < k) {
            Some(p) => {
                let (tp, qp) = bones[p];
                let r = qrot(qp, t);
                ([r[0] + tp[0], r[1] + tp[1], r[2] + tp[2]], qmul(qp, q))
            }
            None => (t, q),
        });
    }

    vs.iter()
        .map(|&(k, p)| {
            let (t, q) = bones[k];
            let r = qrot(q, p);
            [r[0] + t[0], r[1] + t[1], r[2] + t[2]]
        })
        .collect()
}

/// Least and greatest x y z.
fn bounds(ps: &[[f32; 3]]) -> [f32; 6] {
    ps.iter().fold([f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN], |b, p| {
        [b[0].min(p[0]), b[1].min(p[1]), b[2].min(p[2]), b[3].max(p[0]), b[4].max(p[1]), b[5].max(p[2])]
    })
}

fn worst(a: &[[f32; 3]], b: &[[f32; 3]]) -> f32 {
    if a.len() != b.len() {
        return f32::INFINITY;
    }

    a.iter().zip(b).map(|(p, q)| (0..3).map(|c| (p[c] - q[c]).abs()).fold(0.0, f32::max)).fold(0.0, f32::max)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::new(PathBuf::from(&a[0]), GameProfile::from_env().clone());
    let vfs = Vfs::mount(&config).expect("game files");
    // No cut at all (a finite step still cuts at 0, which splits triangles and breaks the corner-by-corner comparison).
    let none = Cuts::every(f32::INFINITY);
    let mut failed = 0;
    for key in &a[1..] {
        let folder = format!("rigidmodels/buildings/{key}/");
        let meshes =
            vfs.list(&folder, "_anim.rigid_model_v2").into_iter().filter(|p| wce_walls::building::is_collapse(p)).collect::<Vec<_>>();
        if meshes.is_empty() {
            println!("{key}: no collapse");
        }

        for mesh_path in meshes {
            let name = mesh_path.rsplit('/').next().unwrap_or(mesh_path);
            let anim_path = mesh_path.replace(".rigid_model_v2", ".anim");
            let (Ok(mesh), Ok(anim_bytes)) = (vfs.read(mesh_path), vfs.read(&anim_path)) else {
                println!("{mesh_path}: no animation");
                continue;
            };
            let Some(anim) = RawAnim::decode(&anim_bytes) else {
                println!("{mesh_path}: not a version 5 animation");
                continue;
            };
            let frames = anim.translations.len();

            // The identity: as it was.
            let (m1, a1) = match warp_collapse(&mesh, &anim_bytes, &Identity, &none) {
                Ok(w) => w,
                Err(why) => {
                    println!("skip {name}: {why} (dropped by the generators)");
                    continue;
                }
            };
            // The editor's playback reads it (wce_formats::anim::Animation), every frame.
            if wce_formats::anim::Animation::decode(&a1, usize::MAX).is_none_or(|p| p.frames.len() != frames) {
                println!("FAIL {name}: the playback does not read the warped animation");
                failed += 1;
                continue;
            }

            let a1 = RawAnim::decode(&a1).expect("anim back");
            let n = anim.bones.len();
            let (old, v1) = (vertices(&mesh, n), vertices(&m1, n));
            let id = (0..frames).map(|f| worst(&posed(&old, &anim, f), &posed(&v1, &a1, f))).fold(0.0, f32::max);

            // A rigid motion: every frame moved by it.
            let rigid = Rigid { yaw: 0.7, shift: [12.0, 3.0, -5.0] };
            let (m2, a2) = warp_collapse(&mesh, &anim_bytes, &rigid, &none).expect("rigid");
            let a2 = RawAnim::decode(&a2).expect("anim back");
            let v2 = vertices(&m2, n);
            let rg = (0..frames)
                .map(|f| {
                    let want = posed(&old, &anim, f).into_iter().map(|p| rigid.point(p)).collect::<Vec<_>>();
                    worst(&want, &posed(&v2, &a2, f))
                })
                .fold(0.0, f32::max);

            // A bend: the first frame where the warp puts the standing piece.
            let bend = Bend { x0: 0.0, radius: 25.0 };
            let (m3, a3) = warp_collapse(&mesh, &anim_bytes, &bend, &none).expect("bend");
            let a3 = RawAnim::decode(&a3).expect("anim back");
            let want = posed(&old, &anim, 0).into_iter().map(|p| bend.point(p)).collect::<Vec<_>>();
            let bd = worst(&want, &posed(&vertices(&m3, n), &a3, 0));

            // The first frame against the standing piece it starts from (`_destructNN_anim` from `_destructNN`): their bounds.
            let standing = mesh_path.replace("_anim.rigid_model_v2", ".rigid_model_v2");
            let start = match vfs.read(&standing) {
                Ok(m) => {
                    let n2 = 1usize << 20;
                    let s = bounds(&vertices(&m, n2).into_iter().map(|(_, p)| p).collect::<Vec<_>>());
                    let f = bounds(&posed(&old, &anim, 0));
                    format!(
                        "; frame 0 bounds off the standing piece's by {:.3} m (standing {s:.1?}, frame 0 {f:.1?})",
                        (0..6).map(|c| (s[c] - f[c]).abs()).fold(0.0, f32::max)
                    )
                }
                Err(_) => String::new(),
            };

            // Rotations are 16-bit: a vertex 20 m from its bone's origin moves by up to about 20 * 3e-5 * 2 m.
            let ok = id < 0.01 && rg < 0.01 && bd < 0.01;
            failed += usize::from(!ok);
            println!(
                "{} {name}: {} bones, {frames} frames; worst offset identity {id:.4} m, rigid {rg:.4} m, bend frame 0 {bd:.4} m{start}",
                if ok { "ok  " } else { "FAIL" },
                anim.bones.len()
            );
        }
    }

    if failed > 0 {
        std::process::exit(1);
    }
}
