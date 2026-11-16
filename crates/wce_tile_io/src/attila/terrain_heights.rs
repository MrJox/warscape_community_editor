//! A tile's heights from its terrain mesh (`mesh.rigid_model_v2`), for a tile whose `hf_height_map.data` lacks them: TDD's campaign
//! tiles ship height maps whose block table stops early (`wce_formats::chmf`: the game reads a missing block as 0, the map's minimum),
//! while their meshes, which the game draws, have the ground (`lakes/22x32_rauros`: its height map -36 nearly everywhere, its mesh's
//! 8988 vertices from -38.7 to 1.35 with the lake's shores).

use rpfm_lib::files::rigidmodel::RigidModel;

/// The longest edge (x, z) of a triangle taken: the ground's are a cell or less (Rauros' 16246: all but 29 up to 128); 943 more of its
/// triangles reach across the tile from a few vertices (slivers fanning out from a corner, not ground the game shows).
const LONGEST_EDGE: f64 = 512.0;

/// The heights of the mesh's first LOD and block on a grid of `w` x `h` samples `spacing` apart (the height map's), row j at mesh z = j *
/// spacing, or at (h - j) * spacing when `flip` (the mesh's z runs from the tile's far edge, as its markers': the height map's row j is at
/// h - j, `lakes/22x32_rauros`' mesh covers 669 of its 682 cells as its mask has them so, 511 the other way); None where no triangle covers a
/// sample (the mask's left-out squares). None when the model has no such block.
pub fn mesh_heights(model: &RigidModel, w: u32, h: u32, spacing: f64, flip: bool) -> Option<Vec<Option<f32>>> {
    let block = model.lods().first()?.mesh_blocks().first()?;
    let mesh = block.mesh();
    let verts: Vec<[f64; 3]> =
        mesh.vertices().iter().map(|v| v.position()).map(|p| [f64::from(p.x), f64::from(p.y), f64::from(p.z)]).collect();
    let idx: Vec<usize> = mesh.indices().iter().map(|&i| i as usize).collect();
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![None; w * h];
    let s = spacing.max(1e-9);
    for t in idx.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) = (verts.get(t[0]), verts.get(t[1]), verts.get(t[2])) else { continue };
        let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        let edge = |p: &[f64; 3], q: &[f64; 3]| (p[0] - q[0]).hypot(p[2] - q[2]);
        if det.abs() < 1e-12 || edge(a, b).max(edge(b, c)).max(edge(c, a)) > LONGEST_EDGE {
            continue;
        }

        let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
        let (z0, z1) = (a[2].min(b[2]).min(c[2]), a[2].max(b[2]).max(c[2]));
        let (i0, i1) = (((x0 / s).ceil().max(0.0)) as usize, ((x1 / s).floor().max(-1.0) as i64).min(w as i64 - 1));
        let top = if flip { h as i64 } else { h as i64 - 1 };
        let (k0, k1) = (((z0 / s).ceil().max(0.0)) as usize, ((z1 / s).floor().max(-1.0) as i64).min(top));
        for k in k0..=k1.max(-1) as usize {
            if k1 < 0 {
                break;
            }

            let z = k as f64 * s;
            for i in i0..=i1.max(-1) as usize {
                if i1 < 0 {
                    break;
                }

                let x = i as f64 * s;
                let l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / det;
                let l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 < -1e-9 || l2 < -1e-9 || l3 < -1e-9 {
                    continue;
                }

                let row = if flip { h - k } else { k };
                if row >= h {
                    continue;
                }

                out[row * w + i] = Some((l1 * a[1] + l2 * b[1] + l3 * c[1]) as f32);
            }
        }
    }

    // Seams a few samples wide that no triangle covers (between the mesh's parts: Rauros' a stair of one or two samples across its lake)
    // take their neighbours' heights; wider gaps (the mask's left-out squares) stay uncovered.
    for _ in 0..HOLE_PASSES {
        let before = out.clone();
        for j in 0..h {
            for i in 0..w {
                if before[j * w + i].is_some() {
                    continue;
                }

                let near: Vec<f32> = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)]
                    .iter()
                    .filter_map(|&(di, dj)| {
                        let (x, y) = (i as i64 + di, j as i64 + dj);
                        (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h).then(|| before[y as usize * w + x as usize]).flatten()
                    })
                    .collect();
                // Between covered samples on two opposite sides only: a seam, not the edge of a gap.
                let across = |a: (i64, i64), b: (i64, i64)| {
                    let at = |(di, dj): (i64, i64)| {
                        let (x, y) = (i as i64 + di, j as i64 + dj);
                        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && before[y as usize * w + x as usize].is_some()
                    };
                    at(a) && at(b)
                };
                if !near.is_empty() && (across((-1, 0), (1, 0)) || across((0, -1), (0, 1)) || near.len() >= 3) {
                    out[j * w + i] = Some(near.iter().sum::<f32>() / near.len() as f32);
                }
            }
        }
    }

    Some(out)
}

/// How many passes fill a seam (each closes a sample from both sides).
const HOLE_PASSES: usize = 3;
