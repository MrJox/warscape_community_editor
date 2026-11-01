//! Differential dump of the draw set on synthetic models, items and views, for comparing with the C++ code (the same generator).

use std::sync::{Arc, RwLock};

use wce_foundation::math::*;
use wce_render::draw_set::{DrawSet, Frustum, View, Visible};
use wce_render::model_cache::{GpuLod, GpuMesh, GpuModel, RigidInstance};
use wce_render::texture::Texture;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() % 100_000) as f32 / 100_000.0
    }
}

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xCBF2_9CE4_8422_2325, |h, c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

fn words(v: &[u32]) -> u64 {
    fnv(v.iter().flat_map(|x| x.to_le_bytes()))
}

fn floats<'a>(v: impl IntoIterator<Item = &'a f32>) -> u64 {
    fnv(v.into_iter().flat_map(|x| x.to_le_bytes()))
}

fn show(tag: &str, v: &Visible) {
    println!("{tag} n{} start={:016x} count={:016x} active={:016x} order={:016x} batch={:016x}", v.instances(), words(&v.start), words(&v.count), words(&v.active),
        words(&v.order), words(&v.batch_of));
}

fn mesh(rng: &mut Rng, y: bool) -> GpuMesh {
    let tex = Arc::new(RwLock::new(Texture::default()));
    let has_offset = rng.next() % 2 == 1;
    let (a, b) = (rng.f(-5.0, 5.0), if y { rng.f(-1.0, 1.0) } else { 0.0 });
    let c = rng.f(-5.0, 5.0);
    GpuMesh {
        vb: None,
        ib: None,
        index_count: 0,
        alpha_mode: 0,
        stride: 32,
        kind: Default::default(),
        uv2_tile: [0.0; 2],
        tint: [1.0; 12],
        offset: Float3::new(a, b, c),
        has_offset,
        tex: std::array::from_fn(|_| tex.clone()),
        diffuse_path: String::new(),
        normal_path: String::new(),
        displacement: None,
        bmin: Float3::default(),
        bmax: Float3::default(),
        snow: false,
    }
}

// 6.28 is the generator's own number, shared with the C++ harness, not an approximation of tau.
#[allow(clippy::approx_constant)]
fn main() {
    let mut rng = Rng(88_172_645_463_325_252);
    let mut models = vec![];
    for m in 0..30 {
        let (a, b, c) = (rng.f(-10.0, 0.0), rng.f(-2.0, 0.0), rng.f(-10.0, 0.0));
        let bmin = Float3::new(a, b, c);
        let (a, b, c) = (rng.f(1.0, 12.0), rng.f(1.0, 30.0), rng.f(1.0, 12.0));
        let bmax = Float3::new(a, b, c);
        let distance = if m % 3 == 0 { 0.0 } else { rng.f(20.0, 80.0) };
        let nm = 1 + rng.next() % 3;
        let meshes = (0..nm).map(|_| mesh(&mut rng, true)).collect();
        let nl = rng.next() % 4;
        let lods = (0..nl).map(|l| {
            let distance = if l + 1 == nl { 0.0 } else { rng.f(100.0, 400.0) * (l + 1) as f32 };
            let count = 1 + rng.next() % 2;
            GpuLod { meshes: (0..count).map(|_| mesh(&mut rng, false)).collect(), distance }
        }).collect();

        models.push(Arc::new(GpuModel { path: String::new(), meshes, distance, lods, bmin, bmax, decals: vec![], top: None, ..Default::default() }));
    }

    let mut ds = DrawSet::default();
    ds.reserve(20000);
    for i in 0..20000u32 {
        let m = models[(rng.next() % models.len() as u32) as usize].clone();
        let (tx, ty, tz, ry, sc) = (rng.f(-2000.0, 2000.0), rng.f(-10.0, 50.0), rng.f(-2000.0, 2000.0), rng.f(0.0, 6.28), rng.f(0.5, 3.0));
        let w = translation(Float3::new(tx, ty, tz)) * rotation_y(ry) * scaling(sc);
        let params: [f32; 16] = std::array::from_fn(|_| if rng.next().is_multiple_of(3) { rng.f(-1.0, 1.0) } else { 0.0 });
        let any_params = rng.next() % 2 == 1;
        let shadow = !rng.next().is_multiple_of(4);
        let lod = !rng.next().is_multiple_of(5);
        ds.add(&m, &w, any_params.then_some(&params), shadow, lod, i + 1);
    }

    for _ in 0..500 {
        let item = rng.next() % 20000;
        let (tx, tz, rx) = (rng.f(-500.0, 500.0), rng.f(-500.0, 500.0), rng.f(-0.3, 0.3));
        ds.set_world(item, &(translation(Float3::new(tx, 0.0, tz)) * rotation_x(rx)));
    }

    println!("items {} batches {} models {}", ds.items(), ds.batches(), ds.models());
    for v in 0..5 {
        let (ex, ey, ez, tx, tz) = (rng.f(-500.0, 500.0), rng.f(20.0, 200.0), rng.f(-500.0, 500.0), rng.f(-500.0, 500.0), rng.f(-500.0, 500.0));
        let (eye, target) = (Float3::new(ex, ey, ez), Float3::new(tx, 0.0, tz));
        let proj = if v == 2 { ortho_lh(800.0, 800.0, -2000.0, 2000.0) } else { perspective_lh(if v == 1 { 0.6 } else { 1.13446 }, 16.0 / 9.0, 1.0, 13000.0) };
        let vp = proj * look_at_lh(eye, target, Float3::new(0.0, 1.0, 0.0));
        let view = View {
            frustum: Frustum::from_view_projection(&vp),
            eye,
            shadow: v == 2,
            lod_scale: if v == 3 { 0.5 } else { 1.0 },
            coarser: if v == 3 { 1 } else { 0 },
            min_ratio: if v == 4 { 0.01 } else { 0.0 },
            min_radius: if v == 2 { 3.0 } else { 0.0 },
            max_distance: 0.0,
            quality: Default::default(),
        };

        println!("frustum {:016x}", floats(view.frustum.p.iter().flat_map(|p| p.n.iter().chain(std::iter::once(&p.d)))));
        let mut vis = Visible::default();
        ds.cull(&view, &mut vis);
        show("visible", &vis);
        let mut inst = vec![RigidInstance::default(); vis.instances()];
        ds.gather(&vis, &mut inst, 1.0, [0.0; 3]);
        println!("gather {:016x}", floats(inst.iter().flat_map(|r| r.world.iter().flatten().chain(&r.p4).chain(&r.p5).chain(&r.p6).chain(&r.p7))));
        println!("ids {:016x}", words(&ds.gather_ids(&vis)));
        ds.retain(&mut vis, |e| e % 3 != 0);
        show("retained", &vis);
        let mut lods = [0; 4];
        for i in (0..20000).step_by(7) {
            lods[ds.lod_of(i, eye, view.lod_scale).min(3)] += 1;
        }

        println!("lods {} {} {} {}", lods[0], lods[1], lods[2], lods[3]);
    }

    let (w, a, b) = ds.item_box(1234).unwrap();
    println!("box {:016x} {:016x} {:016x}", floats(w.m.iter().flatten()), floats(&[a.x, a.y, a.z]), floats(&[b.x, b.y, b.z]));
}
