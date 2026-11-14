//! Poses a unit's parts on the CPU and draws them flat-shaded, without the GPU: the check for a skeleton, a clip and how the parts follow
//! them, before and apart from the renderer's skinning.
//!
//!   cargo run --release -p wce_render --example unit_pose_check -- <manifest> <out.ppm> [--no-skeleton]
//!
//! The manifest has one command a line; `figure` starts another one, in the same picture and frame:
//!
//!   figure <skeleton stem>       (`<stem>.anim`, `<stem>.bone_inv_trans_mats`)
//!   clip <file.anim> <seconds>   (without one: the bind pose)
//!   part <file.rigid_model_v2>   skinned by its vertices' bones
//!   rigid <file.rigid_model_v2> <bone name> [world | skin]   static, on a bone: its world matrix, or its skinning matrix
//!   view <yaw degrees>           the picture's view (default 0: looking along -z at the front)
//!
//! Prints each figure's bounds, its bones' heights and how far the bone lengths stray from the bind pose.

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::vertices::VertexFormat;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use wce_formats::anim::{Animation, BoneLocal};
use wce_foundation::math::{Float3, Float4x4};
use wce_render::anim_pose::{inverse_binds, vertex_weights};

fn local(t: [f32; 3], q: [f32; 4]) -> Float4x4 {
    let [x, y, z, w] = q;
    let mut m = Float4x4::IDENTITY;
    m.m[0] = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), t[0]];
    m.m[1] = [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), t[1]];
    m.m[2] = [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), t[2]];
    m
}

fn world(parents: &[i32], locals: &[BoneLocal]) -> Vec<Float4x4> {
    let mut out: Vec<Float4x4> = Vec::with_capacity(locals.len());
    for (i, (t, q)) in locals.iter().enumerate() {
        let l = local(*t, *q);
        out.push(match usize::try_from(parents[i]).ok() {
            Some(p) => out[p] * l,
            None => l,
        });
    }

    out
}

struct Figure {
    bones: Vec<String>,
    parents: Vec<i32>,
    pose: Vec<Float4x4>,
    bind: Vec<Float4x4>,
    skin: Vec<Float4x4>,
}

#[derive(Default)]
struct Picture {
    tris: Vec<[[f32; 3]; 3]>,
}

fn read_model(path: &str) -> RigidModel {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut extra = DecodeableExtraData::default();
    extra.set_disk_file_path(Some(path));
    RigidModel::decode(&mut Cursor::new(data), &Some(extra)).expect("a model")
}

fn add_part(pic: &mut Picture, path: &str, f: &Figure, rigid: Option<Float4x4>) {
    let model = read_model(path);
    let Some(lod) = model.lods().first() else { return };
    for block in lod.mesh_blocks() {
        let format = block.material().vertex_format();
        if !matches!(format, VertexFormat::Weighted | VertexFormat::Cinematic | VertexFormat::Static) {
            continue;
        }

        let posed: Vec<[f32; 3]> = block
            .mesh()
            .vertices()
            .iter()
            .map(|v| {
                let p = v.position();
                let p = Float3::new(p.x, p.y, p.z);
                let q = if let Some(m) = rigid {
                    m.transform_point(p)
                } else if *format == VertexFormat::Static {
                    p
                } else {
                    let (idx, w) = (*v.bone_indices(), vertex_weights(*format == VertexFormat::Weighted, (*v.weights()).into()));
                    let mut q = Float3::new(0.0, 0.0, 0.0);
                    for k in 0..4 {
                        if w[k] > 0.0 {
                            let s = f.skin[usize::from(idx[k])].transform_point(p);
                            q = Float3::new(q.x + s.x * w[k], q.y + s.y * w[k], q.z + s.z * w[k]);
                        }
                    }

                    q
                };

                [q.x, q.y, q.z]
            })
            .collect();
        for t in block.mesh().indices().chunks_exact(3) {
            pic.tris.push([posed[t[0] as usize], posed[t[1] as usize], posed[t[2] as usize]]);
        }
    }
}

fn figure(stem: &str, clip: Option<(&str, f32)>, with_skeleton: bool) -> Figure {
    let skeleton = Animation::decode(&std::fs::read(format!("{stem}.anim")).expect("the skeleton"), 1).expect("a skeleton");
    let binds = inverse_binds(&std::fs::read(format!("{stem}.bone_inv_trans_mats")).expect("the binds")).expect("binds");
    let bind = world(&skeleton.parents, &skeleton.frames[0]);
    let locals = match clip {
        Some((path, t)) => {
            let mut a = Animation::decode(&std::fs::read(path).expect("the clip"), usize::MAX).expect("a clip");
            println!("clip {path}: skeleton {} {} bones {} frames {} fps, end {}", a.skeleton, a.bones.len(), a.frames.len(), a.fps, a.end);
            if with_skeleton {
                a.on_skeleton(&skeleton);
            }

            assert_eq!(a.bones, skeleton.bones, "the clip's bones are the skeleton's");
            a.sample(t)
        }
        None => skeleton.frames[0].clone(),
    };

    let pose = world(&skeleton.parents, &locals);
    let skin = pose.iter().zip(&binds).map(|(w, i)| *w * *i).collect();
    Figure { bones: skeleton.bones, parents: skeleton.parents, pose, bind, skin }
}

fn origin(m: &Float4x4) -> [f32; 3] {
    [m.m[0][3], m.m[1][3], m.m[2][3]]
}

fn report(f: &Figure) {
    let dist = |a: [f32; 3], b: [f32; 3]| (0..3).map(|c| (a[c] - b[c]) * (a[c] - b[c])).sum::<f32>().sqrt();
    let stray = f
        .parents
        .iter()
        .enumerate()
        .filter_map(|(i, &p)| {
            let p = usize::try_from(p).ok()?;
            Some(((dist(origin(&f.pose[i]), origin(&f.pose[p])) - dist(origin(&f.bind[i]), origin(&f.bind[p]))).abs(), i))
        })
        .fold((0.0f32, 0), |a, b| if b.0 > a.0 { b } else { a });
    println!("  largest change of a bone's length: {:.5} ({})", stray.0, f.bones[stray.1]);
    for name in [
        "bn_hips",
        "bn_head",
        "bn_lefthand",
        "bn_righthand",
        "bn_weapon_01",
        "bn_weapon_02",
        "bn_weapon_03",
        "bn_lefttoebase",
        "bn_righttoebase",
    ] {
        if let Some(i) = f.bones.iter().position(|b| b == name) {
            let [x, y, z] = origin(&f.pose[i]);
            println!("  {name:<18} {x:7.3} {y:7.3} {z:7.3}");
        }
    }
}

fn draw(pic: &Picture, yaw: f32, path: &str) {
    let (w, h) = (640usize, 720usize);
    let (s, c) = yaw.to_radians().sin_cos();
    let view = |p: [f32; 3]| [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]];
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let tris: Vec<[[f32; 3]; 3]> = pic.tris.iter().map(|t| t.map(view)).collect();
    for p in tris.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }

    println!("bounds {lo:?} {hi:?}");
    let scale = ((w - 20) as f32 / (hi[0] - lo[0]).max(1e-3)).min((h - 20) as f32 / (hi[1] - lo[1]).max(1e-3));
    let mx = |x: f32| (x - (lo[0] + hi[0]) * 0.5) * scale + w as f32 * 0.5;
    let my = |y: f32| h as f32 - 10.0 - (y - lo[1]) * scale;
    let mut depth = vec![f32::MIN; w * h];
    let mut rgb = vec![40u8; w * h * 3];
    for t in &tris {
        let (a, b) = ([0, 1, 2].map(|k| t[1][k] - t[0][k]), [0, 1, 2].map(|k| t[2][k] - t[0][k]));
        let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        let shade = (0.25 + 0.75 * (n[2] / len).abs()).min(1.0);
        let (x, y) = (t.map(|p| mx(p[0])), t.map(|p| my(p[1])));
        let area = (x[1] - x[0]) * (y[2] - y[0]) - (x[2] - x[0]) * (y[1] - y[0]);
        if area.abs() < 1e-6 {
            continue;
        }

        let (x0, x1) = (x[0].min(x[1]).min(x[2]).floor().max(0.0) as usize, (x[0].max(x[1]).max(x[2]).ceil() as usize).min(w - 1));
        let (y0, y1) = (y[0].min(y[1]).min(y[2]).floor().max(0.0) as usize, (y[0].max(y[1]).max(y[2]).ceil() as usize).min(h - 1));
        for py in y0..=y1 {
            for px in x0..=x1 {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let u = ((x[1] - fx) * (y[2] - fy) - (x[2] - fx) * (y[1] - fy)) / area;
                let v = ((x[2] - fx) * (y[0] - fy) - (x[0] - fx) * (y[2] - fy)) / area;
                let k = 1.0 - u - v;
                if u < 0.0 || v < 0.0 || k < 0.0 {
                    continue;
                }

                let z = u * t[0][2] + v * t[1][2] + k * t[2][2];
                if z > depth[py * w + px] {
                    depth[py * w + px] = z;
                    let g = (shade * 235.0) as u8;
                    rgb[(py * w + px) * 3..(py * w + px) * 3 + 3].copy_from_slice(&[g, g, g]);
                }
            }
        }
    }

    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend(rgb);
    std::fs::write(path, out).expect("written");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(manifest), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("unit_pose_check <manifest> <out.ppm> [--no-skeleton]");
        std::process::exit(2);
    };

    let with_skeleton = !args.iter().any(|a| a == "--no-skeleton");
    let text = std::fs::read_to_string(manifest).expect("the manifest");
    let lines: Vec<Vec<&str>> =
        text.lines().map(|l| l.split_whitespace().collect()).filter(|l: &Vec<&str>| !l.is_empty() && !l[0].starts_with('#')).collect();
    let mut pic = Picture::default();
    let mut yaw = 0.0;
    let mut current: Option<Figure> = None;
    for (i, l) in lines.iter().enumerate() {
        match l[0] {
            "view" => yaw = l[1].parse().expect("degrees"),
            "figure" => {
                let clip = lines[i + 1..]
                    .iter()
                    .take_while(|n| n[0] != "figure")
                    .find(|n| n[0] == "clip")
                    .map(|n| (n[1], n[2].parse().expect("seconds")));
                println!("figure {}", l[1]);
                let f = figure(l[1], clip, with_skeleton);
                report(&f);
                current = Some(f);
            }
            "clip" => {}
            "part" => add_part(&mut pic, l[1], current.as_ref().expect("a figure"), None),
            "rigid" => {
                let f = current.as_ref().expect("a figure");
                let b = f.bones.iter().position(|b| b == l[2]).expect("the bone");
                let m = if l.get(3) == Some(&"skin") { f.skin[b] } else { f.pose[b] };
                add_part(&mut pic, l[1], f, Some(m));
            }
            other => panic!("unknown command {other}"),
        }
    }

    draw(&pic, yaw, out);
}
