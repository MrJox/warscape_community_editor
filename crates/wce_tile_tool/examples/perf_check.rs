//! Gate for the performance machinery (no window, no GPU): parallel loops and the job pool, the frustum / LOD / size culling of the draw set,
//! the DDS mip stream plan and the tile document's element index against the plain per-element transform walk. Port of `cli/perf_check.cpp`
//! (the C++ thread pool's parallel_for is rayon in the Rust port; its job queue is `JobPool`).
//!
//!   cargo run --release -p wce_tile_tool --example perf_check -- <tile folder> [<tile folder> ...]

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use rayon::prelude::*;
use wce_foundation::jobs::JobPool;
use wce_foundation::math::{Float3, dot, look_at_lh, normalize, perspective_lh, translation};
use wce_foundation::std_random::Mt19937;
use wce_render::draw_set::{DrawSet, Frustum, View, Visible};
use wce_render::model_cache::{GpuLod, GpuMesh, GpuModel, RigidInstance};
use wce_render::texture::{Texture, plan_dds_stream};
use wce_tile::tile_project::TileProject;
use wce_tile::world_transform::world_transform;
use wce_tile_editor::tile_document::TileDocument;

#[derive(Default)]
struct Gate {
    failed: usize,
}

impl Gate {
    fn check(&mut self, ok: bool, what: &str) {
        if !ok {
            println!("FAIL: {what}");
            self.failed += 1;
        }
    }
}

fn thread_pool_checks(g: &mut Gate) {
    let v = vec![1i64; 100_000];
    let sum = AtomicI64::new(0);
    v.par_chunks(1000).for_each(|c| {
        sum.fetch_add(c.iter().sum::<i64>(), Ordering::Relaxed);
    });
    g.check(sum.load(Ordering::Relaxed) == 100_000, "parallel_for covers every element exactly once");

    // Nested: the inner loops run even when every worker is busy with the outer one.
    let inner = AtomicUsize::new(0);
    (0..16).into_par_iter().for_each(|_| {
        (0..8).into_par_iter().for_each(|_| {
            inner.fetch_add(1, Ordering::Relaxed);
        });
    });
    g.check(inner.load(Ordering::Relaxed) == 16 * 8, "nested parallel_for completes");

    // Jobs run in priority order (one thread: the order is exact), a cancelled job never runs. JobPool runs smaller priorities first, the C++
    // pool larger ones: the priorities are negated.
    let one = JobPool::new(1);
    let order = Arc::new(Mutex::new(vec![]));
    let gate = Arc::new(AtomicBool::new(false));

    // Keeps the single worker busy while the others are queued.
    let busy = gate.clone();
    one.post(
        move || {
            while !busy.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        },
        1.0,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let note = |id: i32| {
        let order = order.clone();
        move || order.lock().expect("the order").push(id)
    };

    one.post(note(3), -3.0);
    let h1 = one.post(note(1), -1.0);
    one.post(note(2), -2.0);
    let h9 = one.post(note(9), -9.0);

    // 1 is now the last of the kept ones.
    one.set_priority(h1, -5.0);
    g.check(one.cancel(h9), "a queued job can be cancelled");
    gate.store(true, Ordering::Release);
    one.wait_idle();
    g.check(*order.lock().expect("the order") == vec![1, 3, 2], "jobs run by priority, a changed priority counts, a cancelled one is gone");
}

/// An empty mesh (the draw set only reads the model's bounds and LOD distances).
fn mesh() -> GpuMesh {
    let tex = Arc::new(RwLock::new(Texture::default()));
    GpuMesh {
        vb: None, ib: None, index_count: 0, alpha_mode: 0, stride: 32, kind: Default::default(), uv2_tile: [0.0; 2], tint: [1.0; 12], offset: Float3::default(), has_offset: false, tex: std::array::from_fn(|_| tex.clone()), diffuse_path: String::new(),
        displacement: None, bmin: Float3::default(), bmax: Float3::default(), snow: false,
    }
}

fn make_model(lods: i32, lo: Float3, hi: Float3) -> Arc<GpuModel> {
    let mut m = GpuModel { bmin: lo, bmax: hi, meshes: vec![mesh()], distance: 100.0, ..Default::default() };
    for l in 1..lods {
        m.lods.push(GpuLod { meshes: vec![mesh()], distance: 100.0 * (1 << l) as f32 });
    }

    Arc::new(m)
}

fn draw_set_checks(g: &mut Gate) {
    // Frustum planes: a point inside a perspective frustum is inside, one behind the camera or far off to the side is outside.
    let view = look_at_lh(Float3::new(0.0, 10.0, -50.0), Float3::new(0.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0));
    let proj = perspective_lh(1.0, 16.0 / 9.0, 1.0, 500.0);
    let f = Frustum::from_view_projection(&(proj * view));
    g.check(!f.outside(&[0.0, 5.0, 0.0], 1.0), "a point in front of the camera is inside");
    g.check(f.outside(&[0.0, 10.0, -80.0], 1.0), "a point behind the camera is outside");
    g.check(f.outside(&[400.0, 0.0, 0.0], 1.0), "a point far to the side is outside");
    g.check(f.outside(&[0.0, 0.0, 600.0], 1.0), "a point beyond the far plane is outside");
    g.check(!f.outside(&[0.0, 0.0, 600.0], 200.0), "a big sphere that reaches into the frustum is kept");

    // 4 LOD levels; items on a line away from the eye.
    let model = make_model(4, Float3::new(-1.0, 0.0, -1.0), Float3::new(1.0, 2.0, 1.0));
    let mut set = DrawSet::default();
    let eye = Float3::new(0.0, 10.0, -50.0);
    let at = |z: f32| translation(Float3::new(0.0, 0.0, z));

    // Distance ~10, ~150 (LOD 1), ~350 (LOD 2), beyond the far plane, and one that is not a LOD item (always LOD 0).
    set.add(&model, &at(-40.0), None, true, true, 1);
    set.add(&model, &at(100.0), None, true, true, 2);
    set.add(&model, &at(300.0), None, true, true, 3);
    set.add(&model, &at(900.0), None, true, true, 4);
    set.add(&model, &at(-40.0), None, true, false, 5);
    g.check(set.items() == 5 && set.models() == 1 && set.batches() == 4, "one model entry, one batch per LOD");
    g.check(set.lod_of(0, eye, 1.0) == 0 && set.lod_of(1, eye, 1.0) == 1 && set.lod_of(2, eye, 1.0) == 2, "LOD by distance");
    let mut v = View { frustum: f, eye, ..Default::default() };
    let mut vis = Visible::default();
    set.cull(&v, &mut vis);
    g.check(vis.instances() == 4, "four items are inside the frustum");
    g.check(vis.count[0] == 2 && vis.count[1] == 1 && vis.count[2] == 1, "instances grouped by the LOD batch they use");

    // The same culled set always gives the same order.
    let mut again = Visible::default();
    set.cull(&v, &mut again);
    g.check(again.order == vis.order, "culling is deterministic");

    // Too small to see: skipped for LOD items only (radius / distance below 5 %).
    v.min_ratio = 0.05;
    set.cull(&v, &mut vis);
    g.check(vis.instances() == 2, "small distant LOD items are skipped, the fixed one stays");
    v.min_ratio = 0.0;
    v.shadow = true;
    set.cull(&v, &mut vis);
    g.check(vis.instances() == 4, "shadow views keep casters");

    // Gather writes the world matrix of every instance.
    let mut inst = vec![RigidInstance::default(); vis.instances()];
    set.gather(&vis, &mut inst, 1.0, [0.0; 3]);
    g.check(inst.len() == 4 && (inst[0].world[2][3] - (-40.0)).abs() < 1e-4, "gather writes the world matrix of the first instance");
}

fn dds_checks(g: &mut Gate) {
    // A DXT1 1024 x 1024 texture with a full mip chain: header + 11 mips.
    let mut hdr = vec![0u8; 148];
    let mut put = |off: usize, v: u32| hdr[off..off + 4].copy_from_slice(&v.to_le_bytes());
    put(0, 0x2053_4444);
    put(4, 124);
    put(12, 1024);
    put(16, 1024);
    put(28, 11);
    put(76, 32);
    put(80, 0x4);
    put(84, 0x3154_5844);
    let mut total = 128;
    let mut w = 1024usize;
    for _ in 0..11 {
        total += (w / 4).max(1) * (w / 4).max(1) * 8;
        w = (w / 2).max(1);
    }

    let p = plan_dds_stream(&hdr, total, 256);
    g.check(p.ok && p.skip == 2, "a 1024 texture streams from the 256 mip (skips 2)");

    // The tail starts after the two large mips: 1024^2/2 + 512^2/2 bytes.
    g.check(p.tail_offset == 128 + 1024 * 1024 / 2 + 512 * 512 / 2, "the tail starts at the right file offset");
    g.check(!plan_dds_stream(&hdr, total, 2048).ok, "a texture that already fits is loaded whole");

    // caps2: cube map
    hdr[112..116].copy_from_slice(&0x200u32.to_le_bytes());
    g.check(!plan_dds_stream(&hdr, total, 256).ok, "cube maps are loaded whole");
}

fn tile_index_checks(g: &mut Gate, dir: &std::path::Path) {
    let project = match TileProject::open(dir) {
        Ok(p) => p,
        Err(e) => {
            g.check(false, &format!("open {}: {e}", dir.display()));
            return;
        }
    };

    let mut doc = TileDocument::new(project, None);
    let t = doc.tile();
    let sc = t.tscene();
    let u = doc.project().unit_scale;
    let (mut checked, mut bad) = (0, 0);
    for e in 0..sc.doc().elements().len() {
        // The plain rules the index replaces.
        let ty = &sc.doc().elements()[e].name;
        let plain = ty == "point" || ty == "points" || ty == "children" || ty == "camera" || sc.is_layer(e);
        let instance = !plain && sc.position(e).is_some();
        if t.is_instance(e) != instance {
            bad += 1;
            continue;
        }

        if !instance {
            continue;
        }

        checked += 1;
        let w = world_transform(sc, e);
        let p = t.world_position(e);
        if (p.x - (w.position.x / u) as f32).abs() > 1e-3 || (p.y - w.position.y as f32).abs() > 1e-3 || (p.z - (w.position.z / u) as f32).abs() > 1e-3 {
            bad += 1;
        }

        let m = t.world_matrix(e);
        let s = sc.scale(e);
        for r in 0..3 {
            for c in 0..3 {
                if (m.m[r][c] - (w.rotation.m[r][c] * s.map_or(1.0, |s| s[c])) as f32).abs() > 1e-4 {
                    bad += 1;
                }
            }
        }
    }

    let name = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    println!("{name}: {checked} instances checked against the transform walk, {bad} mismatches");
    g.check(bad == 0, "the element index agrees with the transform walk");
    g.check(checked == t.instances().len(), "the index lists every instance");

    // Ray picking through the grid finds the nearest instance sphere that a brute-force test finds.
    let mut rng = Mt19937::new(7);
    let (mut tested, mut mismatches) = (0, 0);
    let instances = t.instances().to_vec();
    for _ in 0..200 {
        if instances.is_empty() {
            break;
        }

        let t = doc.tile();
        let target = instances[rng.next_u32() as usize % instances.len()];
        let c = t.world_position(target);
        let ox = c.x + ((rng.next_u32() % 200) as i32 - 100) as f32;
        let oy = c.y + 60.0 + (rng.next_u32() % 100) as f32;
        let oz = c.z + ((rng.next_u32() % 200) as i32 - 100) as f32;
        let origin = Float3::new(ox, oy, oz);
        let dir = normalize(Float3::new(c.x, c.y + t.marker_radius(target) * 0.3, c.z) - origin);
        let mut best = None;
        let mut best_t = 1e30f32;
        for &e in &instances {
            // The grid only holds what can be picked (hidden / locked layers are left out).
            if !t.shown(e) || !t.selectable(e) {
                continue;
            }

            let mut q = t.world_position(e);
            let r = t.marker_radius(e);
            if t.tscene().type_of(e) == "BUILDING_INSTANCE" {
                q.y += r * 0.5;
            }

            let oc = origin - q;
            let (b, cc) = (dot(oc, dir), dot(oc, oc) - r * r);
            let disc = b * b - cc;
            if disc < 0.0 {
                continue;
            }

            let tt = -b - disc.sqrt();
            if tt > 0.0 && tt < best_t {
                best_t = tt;
                best = Some(e);
            }
        }

        // The brute force knows no draw distances.
        doc.tile_mut().display_mut().lod = false;
        let hit = doc.tile().pick_instance(origin, dir, false);
        tested += 1;
        if hit != best {
            mismatches += 1;
        }
    }

    println!("{name}: {tested} pick rays, {mismatches} differ from the brute-force test");
    g.check(mismatches == 0, "the pick grid finds the same instance as testing every instance");
}

fn main() {
    let mut g = Gate::default();
    thread_pool_checks(&mut g);
    draw_set_checks(&mut g);
    dds_checks(&mut g);
    for dir in std::env::args().skip(1) {
        tile_index_checks(&mut g, std::path::Path::new(&dir));
    }

    println!("{}", if g.failed != 0 { "FAILED" } else { "ok" });
    std::process::exit(i32::from(g.failed != 0));
}
