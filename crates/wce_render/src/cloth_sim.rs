//! The sails' cloth: the game's verlet particle simulation (docs/re/cloth.md), run on the CPU, giving the bone palette of a sail model's
//! `WeightedCloth` meshes. A sail model's `Cloth` blocks (`sim_*`) are the particles and their links, its `Rope`s tied to a cloth particle hang
//! from it (and pull it), its `CollisionShape` spheres keep the cloth off the masts. All of it in the model's own space; the ship's motion
//! reaches it only through the wind ([`ship_wind`]).
//!
//! [`ClothRig`] is what the file says (loaded once, shared); [`ClothSim`] is one sail model's state.

use std::io::Cursor;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::materials::{MaterialType, TextureType};
use rpfm_lib::files::{Decodeable, DecodeableExtraData};

type V3 = [f32; 3];

/// The tweakers' defaults (docs/re/cloth.md, "Tweakers").
const INTERVAL: f32 = 0.05;
const MAX_DT: f32 = 0.1;
const ITERATIONS: usize = 10;
const DAMPING: f32 = 0.999;
const FORCE_SCALE: f32 = 1.0;
const HANDLE_TENSION: f32 = 0.95;
const PERTURBATION_SCALE: f32 = 1.0;
const PERTURBATION_SPACING: f32 = 3.5;
const PERTURBATION_SPEED: f32 = 1.0;
const CLOTH_MASS: f32 = 1.0;
const ROPE_MASS: f32 = 5.0;
const ROPE_FLEX: f32 = 1.0;
const ROPE_WIND: f32 = 0.01;
const COLLISION_INTERVAL: usize = 3;
const COLLISION_SCALE: f32 = 1.1;
const SPHERE_SCALE: f32 = 1.0;

/// The ship side's wind (`EmpireBattle`): `SHIP_DISPLAY_WIND_SCALE`, `SHIP_DISPLAY_MIN_FORWARD_WIND`, `SHIP_DISPLAY_MAX_LATERAL_WIND_FORWARD_SCALE`.
const WIND_SCALE: f32 = 30.0;
const MIN_FORWARD_WIND: f32 = 1.0;
const MAX_LATERAL_WIND: f32 = 5.0;
const GRAVITY: f32 = 9.8;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The engine's quick square root: the exponent halved on the bits (docs/re/cloth.md, "Skinning").
fn quick_sqrt(x: f32) -> f32 {
    f32::from_bits(((x.to_bits() as i32 - 0x3F80_0000) >> 1).wrapping_add(0x3F80_0000) as u32)
}

/// A vector over its quick length (slightly off unit length, as the game's).
fn quick_norm(v: V3) -> V3 {
    let l = quick_sqrt(dot(v, v));
    if l > 0.0 { mul(v, 1.0 / l) } else { v }
}

fn norm(v: V3) -> V3 {
    let l = dot(v, v).sqrt();
    if l > 0.0 { mul(v, 1.0 / l) } else { v }
}

/// A 3 x 3 matrix by columns.
type M3 = [V3; 3];

fn m3_mul(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..3).map(|k| a[k][r] * b[c][k]).sum()))
}

fn m3_apply(a: &M3, v: V3) -> V3 {
    std::array::from_fn(|r| a[0][r] * v[0] + a[1][r] * v[1] + a[2][r] * v[2])
}

/// The inverse (adjugate over determinant); the matrix as it is when the determinant is tiny.
fn m3_inverse(m: &M3) -> M3 {
    let (c0, c1, c2) = (m[0], m[1], m[2]);
    let det = dot(c0, cross(c1, c2));
    if det.abs() < 1e-12 {
        return *m;
    }

    // The rows of the inverse are the cross products of the columns.
    let rows = [cross(c1, c2), cross(c2, c0), cross(c0, c1)].map(|r| mul(r, 1.0 / det));
    std::array::from_fn(|c| std::array::from_fn(|r| rows[r][c]))
}

/// A particle (the engine's 0x48 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Particle {
    pinned: bool,
    w: f32,
    x: V3,
    prev: V3,
    a: V3,
    rest: V3,
}

impl Particle {
    fn at(p: V3, weight: f32) -> Self {
        Self { pinned: weight < 0.5, w: weight, x: p, prev: p, a: [0.0; 3], rest: p }
    }
}

/// A rope: its nodes, link rests (× stiffness), mass, radius, and for one tied to a cloth particle which end is tied.
#[derive(Clone, Debug)]
struct RopeRig {
    nodes: Vec<Particle>,
    rests: Vec<f32>,
    mass: f32,

    /// Its half width for drawing (the material's `uk_1`; not read by the simulation), and its nodes' texture coordinates (their `uk_1`).
    radius: f32,
    v: Vec<f32>,

    /// The tied node (0 or the last) and the cloth particle it is tied to; unused on a free rope.
    node: usize,
    particle: usize,
}

/// A rigid block's handle (docs/re/cloth.md, "Handle tension"): the link from the rope's particle to a pinned one, and the third particle.
#[derive(Clone, Copy, Debug)]
struct Handle {
    free: usize,
    pinned: usize,
    rest: f32,
    vertex: usize,
}

/// One cloth (a `sim_*` block) and the display mesh it skins.
#[derive(Clone, Debug)]
struct CloRig {
    particles: Vec<Particle>,
    links: Vec<(usize, usize, f32)>,

    /// Per particle, the triangles touching it, in the file's order.
    fans: Vec<Vec<[usize; 3]>>,
    mass: f32,

    /// A rigid block (one triangle, three particles): no wind perturbation, handle tension.
    rigid: bool,
    handle: Option<Handle>,
    ropes: Vec<RopeRig>,

    /// The display mesh's bones: the particle each follows, in the palette's order.
    bones: Vec<usize>,

    /// Its first bone in the model's palette.
    palette_base: usize,
}

/// A sail model's cloths, ropes and colliders, as the file has them.
#[derive(Clone, Debug, Default)]
pub struct ClothRig {
    cloths: Vec<CloRig>,

    /// The ropes tied to no cloth (both ends pinned, or loose).
    free: Vec<RopeRig>,
    spheres: Vec<(V3, f32)>,

    /// The palette's bones: all the display meshes', one after another.
    pub bones: usize,

    /// The ropes' textures (the first rope's diffuse and normal map).
    pub rope_textures: [String; 2],
}

impl ClothRig {
    /// What was built, for checks: per cloth its particles, pinned ones, links, bones and first bone; then the free ropes and spheres.
    pub fn summary(&self) -> String {
        let cloths = self.cloths.iter().map(|c| format!("{} particles ({} pinned), {} links, {} bones from {}, {} ropes", c.particles.len(), c.particles.iter().filter(|p| p.pinned).count(), c.links.len(), c.bones.len(), c.palette_base, c.ropes.len())).collect::<Vec<_>>().join("; ");
        format!("{} bones; cloths: {cloths}; {} free ropes, {} spheres", self.bones, self.free.len(), self.spheres.len())
    }

    /// A model's cloth, if it has any (`None` without a `WeightedCloth` mesh paired with its `sim_` block).
    pub fn load(bytes: Vec<u8>) -> Option<Self> {
        let model = RigidModel::decode(&mut Cursor::new(bytes), &Some(DecodeableExtraData::default())).ok()?;
        let blocks = model.lods().first()?.mesh_blocks();
        let name = |k: usize| blocks[k].material().name().to_owned();

        // The palette's order: the drawn WeightedCloth meshes in the LOD's order (`GpuMesh::palette_base`).
        let mut rig = Self::default();
        let mut sim_of_block = vec![None; blocks.len()];
        for (k, block) in blocks.iter().enumerate() {
            let (mesh, material) = (block.mesh(), block.material());
            match mesh.material_type() {
                MaterialType::CollisionShape => {
                    let c = material.sphere_center();
                    rig.spheres.push(([c.x, c.y, c.z], *material.sphere_radius()));
                }

                MaterialType::WeightedCloth if !mesh.vertices().is_empty() && !mesh.indices().is_empty() => {
                    let base = rig.bones;
                    rig.bones += material.cloth_bones().len();
                    let wanted = format!("sim_{}_LOD1", name(k).strip_prefix("display_").unwrap_or(&name(k)));
                    let Some(s) = (0..blocks.len()).find(|&j| *blocks[j].mesh().material_type() == MaterialType::Cloth && name(j) == wanted) else { continue };
                    let Some(cloth) = cloth_of(&blocks[s], material.cloth_bones().iter().map(|b| *b.vertex_index() as usize).collect(), base) else { continue };
                    sim_of_block[s] = Some(rig.cloths.len());
                    rig.cloths.push(cloth);
                }

                _ => {}
            }
        }

        // The ropes tied to a cloth particle: `attachments` = [v0, b0, v1, b1] (node 0 to particle v0 of block b0, the last node to v1 of b1).
        for block in blocks.iter().filter(|b| *b.mesh().material_type() == MaterialType::Rope) {
            let (material, rope) = (block.material(), block.material().rope());
            let n = rope.nodes().len();
            if n < 2 {
                continue;
            }

            if rig.rope_textures[0].is_empty() {
                let path = |t: TextureType| material.textures().iter().find(|x| *x.tex_type() == t).map(|x| x.path().replace('\\', "/").to_lowercase()).unwrap_or_default();
                rig.rope_textures = [path(TextureType::Diffuse), path(TextureType::Normal)];
            }

            let param = |k: i32, or: f32| material.params_f32().iter().find(|(i, _)| *i == k).map_or(or, |(_, v)| *v);
            let stiffness = ROPE_FLEX * param(3, 1.0);
            let nodes = rope.nodes().iter().map(|nd| { let p = nd.position(); Particle::at([p.x, p.y, p.z], *nd.weight()) }).collect();
            let rests = rope.links().iter().map(|l| l.length() * stiffness).collect();
            let v = rope.nodes().iter().map(|nd| *nd.uk_1()).collect();
            let mut r = RopeRig { nodes, rests, mass: ROPE_MASS * param(2, 1.0), radius: *rope.uk_1(), v, node: usize::MAX, particle: usize::MAX };

            // Tied to a cloth particle: `attachments` = [v0, b0, v1, b1] (node 0 to particle v0 of block b0, the last node to v1 of b1).
            let at = rope.attachments();
            let tie = [(at.x, at.y, 0), (at.z, at.w, n - 1)].into_iter().find(|&(v, b, _)| v >= 0 && b >= 0);
            let cloth = tie.and_then(|(v, b, node)| Some((sim_of_block.get(b as usize).copied().flatten()?, v as usize, node)));
            match cloth {
                Some((c, v, node)) if v < rig.cloths[c].particles.len() => {
                    (r.node, r.particle) = (node, v);
                    rig.cloths[c].ropes.push(r);
                }

                _ => rig.free.push(r),
            }
        }

        for cloth in &mut rig.cloths {
            cloth.handle = handle_of(cloth);
        }

        (!rig.cloths.is_empty()).then_some(rig)
    }
}

/// A `Cloth` block's particles (its first `uk_8.len()` vertices), links and triangle fans.
fn cloth_of(block: &rpfm_lib::files::rigidmodel::MeshBlock, bones: Vec<usize>, palette_base: usize) -> Option<CloRig> {
    let (mesh, material) = (block.mesh(), block.material());
    let n = material.uk_8().len();
    if n == 0 || mesh.vertices().len() < n || bones.iter().any(|&b| b >= n) {
        return None;
    }

    // The weight is the colour's alpha: the vertex's last four bytes (BGRA), which rpfm keeps raw as `uk_1` for this layout.
    let particles = mesh.vertices()[..n].iter().map(|v| { let p = v.position(); Particle::at([p.x, p.y, p.z], f32::from(v.uk_1().w) / 255.0) }).collect();
    let links = material.uk_7().iter().filter(|l| (*l.uk1() as usize) < n && (*l.uk2() as usize) < n).map(|l| (*l.uk1() as usize, *l.uk2() as usize, *l.uk3())).collect();
    let mut fans = Vec::with_capacity(n);
    let mut at = 0;
    for count in material.uk_8() {
        let count = *count.uk1() as usize;
        fans.push(material.uk_9().get(at..at + count)?.iter().map(|t| [*t.uk1() as usize, *t.uk2() as usize, *t.uk3() as usize]).filter(|t| t.iter().all(|&i| i < n)).collect());
        at += count;
    }

    let mass = material.params_f32().iter().find(|(i, _)| *i == 2).map_or(1.0, |(_, v)| *v);
    Some(CloRig { particles, links, fans, mass: CLOTH_MASS * mass, rigid: n == 3, handle: None, ropes: vec![], bones, palette_base })
}

/// A rigid block's handle: set up when a rope is tied to one of its three particles.
fn handle_of(c: &CloRig) -> Option<Handle> {
    let free = c.ropes.first().filter(|_| c.rigid)?.particle;
    let &(a, b, rest) = c.links.iter().find(|&&(a, b, _)| (a == free && c.particles[b].pinned) || (b == free && c.particles[a].pinned))?;
    let pinned = if a == free { b } else { a };
    let vertex = (0..3).find(|&k| k != free && k != pinned)?;
    Some(Handle { free, pinned, rest, vertex })
}

/// The frame's wind (model space, gravity included), from the ship's rotation (model to world, rows): a world wind of 30 along +Z, never less
/// than 1 forward along the model's +z, sideways at most 5 x that (docs/re/cloth.md, "The wind"; `FUN_102ba0f0`).
pub fn ship_wind(rows: [V3; 3]) -> V3 {
    let (a, b, c) = (WIND_SCALE * rows[2][0], WIND_SCALE * rows[2][1], WIND_SCALE * rows[2][2]);
    let f = c.max(MIN_FORWARD_WIND);
    let l = MAX_LATERAL_WIND * f;
    add(mul(rows[1], -GRAVITY), [a.clamp(-l, l), b.clamp(-l, l), f])
}

/// The relaxation of one link (`FUN_118157f0`): `w1`, `w2` the particles' weights (0 pinned).
fn relax(x1: &mut Particle, x2: &mut Particle, rest: f32, w1: f32, w2: f32) {
    let d = sub(x1.x, x2.x);
    let l = dot(d, d).sqrt();
    let k = (l - rest) / l.max(1e-4);
    match (x1.pinned, x2.pinned) {
        (false, false) => {
            x1.x = sub(x1.x, mul(d, w1 * 0.5 * k));
            x2.x = add(x2.x, mul(d, w2 * 0.5 * k));
        }

        (false, true) => x1.x = sub(x1.x, mul(d, w1 * k)),
        (true, false) => x2.x = add(x2.x, mul(d, w2 * k)),
        (true, true) => {}
    }
}

/// Two particles of one slice, mutably.
fn pair<T>(v: &mut [T], i: usize, j: usize) -> Option<(&mut T, &mut T)> {
    if i == j || i >= v.len() || j >= v.len() {
        return None;
    }

    if i < j {
        let (a, b) = v.split_at_mut(j);
        Some((&mut a[i], &mut b[0]))
    } else {
        let (a, b) = v.split_at_mut(i);
        Some((&mut b[0], &mut a[j]))
    }
}

fn integrate(p: &mut Particle, h: f32) {
    if p.pinned {
        return;
    }

    let x = p.x;
    p.x = add(add(x, mul(sub(x, p.prev), DAMPING)), mul(p.a, h * h));
    p.prev = x;
}

/// A free particle out of the spheres (`FUN_1186c1f0`); the push, if any.
fn push_out(x: V3, spheres: &[(V3, f32)]) -> Option<V3> {
    let mut push = None;
    for &(c, r) in spheres {
        let rho = r * SPHERE_SCALE;
        let d = sub(x, c);
        let l = quick_sqrt(dot(d, d));
        if l < rho && l > 0.0 {
            let p = mul(d, (rho - l) / l * COLLISION_SCALE);
            push = Some(add(push.unwrap_or([0.0; 3]), p));
        }
    }

    push
}

/// One cloth's state.
#[derive(Clone, Debug)]
struct Cloth {
    rig: CloRig,
    particles: Vec<Particle>,
    ropes: Vec<Vec<Particle>>,
    normals: Vec<V3>,
    frames: Vec<M3>,
    inverse_rest: Vec<M3>,

    /// The wind's per-sail phase offset (three values in 0..1000, `FUN_117f94c0`).
    phase: V3,
}

impl Cloth {
    fn new(rig: &CloRig, seed: u64) -> Self {
        let particles = rig.particles.clone();
        let (normals, frames) = frames_of(&rig.fans, &particles);
        let inverse_rest = frames.iter().map(m3_inverse).collect();
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ((s >> 33) % 1000) as f32
        };
        let phase = [next(), next(), next()];
        Self { rig: rig.clone(), ropes: rig.ropes.iter().map(|r| r.nodes.clone()).collect(), particles, normals, frames, inverse_rest, phase }
    }

    /// The frame's accelerations (`FUN_117fef10`).
    fn forces(&mut self, wind: V3, t: f32) {
        let w = mul(wind, FORCE_SCALE);
        let hl = (w[0] * w[0] + w[2] * w[2]).sqrt();
        let n = if hl > 0.0 { [w[0] / hl, 0.0, w[2] / hl] } else { [0.0; 3] };
        let (side, along) = ([n[2], 0.0, -n[0]], [n[0], 0.0, n[2]]);
        let k = std::f32::consts::TAU / PERTURBATION_SPACING.max(0.01);
        let tau = PERTURBATION_SPEED * t;
        for (p, normal) in self.particles.iter_mut().zip(&self.normals) {
            let wp = if self.rig.rigid {
                [w[0], 0.0, w[2]]
            } else {
                let q = add(p.x, self.phase);
                let m = 1.0 + PERTURBATION_SCALE * ((k * (dot(side, q) + tau)).sin() + (k * (q[1] + tau)).sin() + (k * (dot(along, q) + tau)).sin()) / 3.0;
                [w[0] * m, 0.0, w[2] * m]
            };

            let f = add(mul(*normal, dot(wp, *normal)), [0.0, w[1], 0.0]);
            p.a = mul(f, 1.0 / self.rig.mass);
        }

        for (rope, rig) in self.ropes.iter_mut().zip(&self.rig.ropes) {
            for (k, node) in rope.iter_mut().enumerate() {
                if k != rig.node {
                    node.a = mul(w, 1.0 / rig.mass);
                }
            }
        }
    }

    /// One step of `h` seconds (`FUN_1188d310`).
    fn step(&mut self, h: f32, spheres: &[(V3, f32)]) {
        for p in &mut self.particles {
            integrate(p, h);
        }

        for (rope, rig) in self.ropes.iter_mut().zip(&self.rig.ropes) {
            for (k, node) in rope.iter_mut().enumerate() {
                if k != rig.node {
                    integrate(node, h);
                }
            }

            rope[rig.node] = self.particles[rig.particle];
        }

        for i in 0..ITERATIONS {
            let s = (1.0 - i as f32 / ITERATIONS as f32).powi(2);
            if i % COLLISION_INTERVAL == 0 && !spheres.is_empty() {
                self.collide(spheres);
            }

            for &(a, b, rest) in &self.rig.links {
                if let Some((p1, p2)) = pair(&mut self.particles, a, b) {
                    let (w1, w2) = (p1.w, p2.w);
                    relax(p1, p2, rest, w1, w2);
                }
            }

            if self.rig.rigid && s > 0.5 && let Some(hd) = self.rig.handle {
                self.handle(hd);
            }

            for (rope, rig) in self.ropes.iter_mut().zip(&self.rig.ropes) {
                let mut tied = self.particles[rig.particle];
                if self.rig.rigid {
                    tied.w = s;
                }

                for (k, &rest) in rig.rests.iter().enumerate() {
                    let (a, b) = (k, k + 1);
                    if a == rig.node || b == rig.node {
                        let other = if a == rig.node { b } else { a };
                        let mut o = rope[other];
                        let (w1, w2) = (tied.w, o.w);
                        if a == rig.node { relax(&mut tied, &mut o, rest, w1, w2) } else { relax(&mut o, &mut tied, rest, w2, w1) }
                        rope[other] = o;
                    } else if let Some((p1, p2)) = pair(rope, a, b) {
                        let (w1, w2) = (p1.w, p2.w);
                        relax(p1, p2, rest, w1, w2);
                    }
                }

                let w = self.particles[rig.particle].w;
                self.particles[rig.particle] = Particle { w, ..tied };
                rope[rig.node] = self.particles[rig.particle];
            }
        }
    }

    fn collide(&mut self, spheres: &[(V3, f32)]) {
        for p in self.particles.iter_mut().filter(|p| !p.pinned) {
            if let Some(push) = push_out(p.x, spheres) {
                p.prev = p.x;
                p.x = add(p.x, push);
            }
        }

        for &(a, b, _) in &self.rig.links {
            let Some((p1, p2)) = pair(&mut self.particles, a, b) else { continue };
            if p1.pinned || p2.pinned {
                continue;
            }

            if let Some(push) = push_out(mul(add(p1.x, p2.x), 0.5), spheres) {
                (p1.prev, p2.prev) = (p1.x, p2.x);
                (p1.x, p2.x) = (add(p1.x, push), add(p2.x, push));
            }
        }
    }

    /// The rigid block's handle (`FUN_118159d0`).
    fn handle(&mut self, hd: Handle) {
        let (anchor, free) = (self.particles[hd.pinned].x, self.particles[hd.free].x);
        let d = norm(sub(free, anchor));
        self.particles[hd.free].x = add(anchor, mul(d, hd.rest));
        if HANDLE_TENSION != 0.0 {
            let mut home = Particle { pinned: true, w: 0.0, ..self.particles[hd.vertex] };
            home.x = home.rest;
            let v = &mut self.particles[hd.vertex];
            let current = dot(sub(v.x, home.x), sub(v.x, home.x)).sqrt();
            let w = v.w;
            relax(v, &mut home, HANDLE_TENSION * current, w, 0.0);
        }
    }

    /// The particles' frames and normals again (`FUN_11814990`), after stepping.
    fn skin(&mut self) {
        (self.normals, self.frames) = frames_of(&self.rig.fans, &self.particles);
    }
}

/// Per particle, its normal and frame (docs/re/cloth.md, "Skinning").
fn frames_of(fans: &[Vec<[usize; 3]>], particles: &[Particle]) -> (Vec<V3>, Vec<M3>) {
    fans.iter().map(|fan| {
        let mut sum = [0.0; 3];
        let mut single = None;
        for &[i0, i1, i2] in fan {
            let (x0, x1, x2) = (particles[i0].x, particles[i1].x, particles[i2].x);
            let t = quick_norm(sub(x1, x0));
            let e = quick_norm(sub(x2, x0));
            let n = quick_norm(cross(t, e));
            let b = quick_norm(cross(n, t));
            sum = add(sum, n);
            single = Some([t, b, n]);
        }

        let normal = quick_norm(sum);
        let frame = match (fan.len(), single) {
            (1, Some(f)) => f,
            _ => {
                let r = if normal[1].abs() > 0.95 { [0.099_503_7, 0.995_037_3, 0.0] } else { [0.0, 1.0, 0.0] };
                let u = norm(cross(normal, r));
                let v = norm(cross(normal, u));
                [u, v, normal]
            }
        };

        (normal, frame)
    }).unzip()
}

/// One sail model's simulated cloth.
#[derive(Clone, Debug)]
pub struct ClothSim {
    cloths: Vec<Cloth>,
    free: Vec<(RopeRig, Vec<Particle>)>,
    spheres: Vec<(V3, f32)>,
    acc: f32,
    bones: usize,
}

/// A free rope's frame forces (`FUN_117fed50`): the horizontal wind × `rigid_rope_wind_pressure_scalar`, gravity whole, over its mass.
fn free_rope_forces(rig: &RopeRig, nodes: &mut [Particle], w: V3) {
    let wind = [w[0] * ROPE_WIND, w[1], w[2] * ROPE_WIND];
    for p in nodes.iter_mut() {
        p.a = mul(wind, 1.0 / rig.mass);
    }
}

/// A free rope's step: as a cloth's, its links its constraints, no ropes of its own.
fn free_rope_step(rig: &RopeRig, nodes: &mut [Particle], h: f32, spheres: &[(V3, f32)]) {
    for p in nodes.iter_mut() {
        integrate(p, h);
    }

    for i in 0..ITERATIONS {
        if i % COLLISION_INTERVAL == 0 && !spheres.is_empty() {
            for p in nodes.iter_mut().filter(|p| !p.pinned) {
                if let Some(push) = push_out(p.x, spheres) {
                    p.prev = p.x;
                    p.x = add(p.x, push);
                }
            }

            for k in 0..rig.rests.len() {
                let Some((p1, p2)) = pair(nodes, k, k + 1) else { continue };
                if p1.pinned || p2.pinned {
                    continue;
                }

                if let Some(push) = push_out(mul(add(p1.x, p2.x), 0.5), spheres) {
                    (p1.prev, p2.prev) = (p1.x, p2.x);
                    (p1.x, p2.x) = (add(p1.x, push), add(p2.x, push));
                }
            }
        }

        for (k, &rest) in rig.rests.iter().enumerate() {
            if let Some((p1, p2)) = pair(nodes, k, k + 1) {
                let (w1, w2) = (p1.w, p2.w);
                relax(p1, p2, rest, w1, w2);
            }
        }
    }
}

impl ClothSim {
    /// At rest (the file's positions, still), the wind's phases from `seed`.
    pub fn new(rig: &ClothRig, seed: u64) -> Self {
        let cloths = rig.cloths.iter().enumerate().map(|(k, c)| Cloth::new(c, seed.wrapping_add(k as u64 * 7919))).collect();
        let free = rig.free.iter().map(|r| (r.clone(), r.nodes.clone())).collect();
        Self { cloths, free, spheres: rig.spheres.clone(), acc: 0.0, bones: rig.bones }
    }

    /// The frame (`FUN_118996e0`): the forces of `wind` (model space, [`ship_wind`]) at time `t`, then as many fixed steps as `dt` holds.
    pub fn update(&mut self, dt: f32, t: f32, wind: V3) {
        for c in &mut self.cloths {
            c.forces(wind, t);
        }

        let w = mul(wind, FORCE_SCALE);
        for (rig, nodes) in &mut self.free {
            free_rope_forces(rig, nodes, w);
        }

        self.acc += dt.clamp(0.0, MAX_DT);
        let mut stepped = false;
        while self.acc >= INTERVAL {
            for c in &mut self.cloths {
                c.step(INTERVAL, &self.spheres);
            }

            for (rig, nodes) in &mut self.free {
                free_rope_step(rig, nodes, INTERVAL, &self.spheres);
            }

            self.acc -= INTERVAL;
            stepped = true;
        }

        if stepped {
            for c in &mut self.cloths {
                c.skin();
            }
        }
    }

    /// The display meshes' palette (3 x 4 rows a bone): bone i on particle k moves a vertex v to `X_k (v - p0_k) + x_k`, `X_k = F_k F0_k⁻¹`.
    pub fn palette(&self) -> Vec<[f32; 12]> {
        let mut out = vec![[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]; self.bones];
        for c in &self.cloths {
            for (i, &k) in c.rig.bones.iter().enumerate() {
                let x = m3_mul(&c.frames[k], &c.inverse_rest[k]);
                let t = sub(c.particles[k].x, m3_apply(&x, c.particles[k].rest));
                if let Some(slot) = out.get_mut(c.rig.palette_base + i) {
                    *slot = [x[0][0], x[1][0], x[2][0], t[0], x[0][1], x[1][1], x[2][1], t[1], x[0][2], x[1][2], x[2][2], t[2]];
                }
            }
        }

        out
    }

    /// Where every rope is (model space): its nodes, their texture coordinates and its half width; the cloths' ropes first, then the free ones.
    pub fn ropes(&self) -> Vec<(Vec<V3>, Vec<f32>, f32)> {
        let tied = self.cloths.iter().flat_map(|c| c.ropes.iter().zip(&c.rig.ropes).map(|(r, rig)| (r.iter().map(|p| p.x).collect(), rig.v.clone(), rig.radius)));
        tied.chain(self.free.iter().map(|(rig, nodes)| (nodes.iter().map(|p| p.x).collect(), rig.v.clone(), rig.radius))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{CloRig, ClothRig, ClothSim, Particle, frames_of, m3_inverse, m3_mul, quick_sqrt, relax, ship_wind};

    /// A square of two triangles, its top edge pinned.
    fn square() -> ClothRig {
        let p = |x: f32, y: f32, w: f32| Particle::at([x, y, 0.0], w);
        let particles = vec![p(0.0, 1.0, 0.0), p(1.0, 1.0, 0.0), p(0.0, 0.0, 1.0), p(1.0, 0.0, 1.0)];
        let tris = [[0, 2, 1], [1, 2, 3]];
        let fans = (0..4).map(|k| tris.iter().filter(|t| t.contains(&k)).copied().collect()).collect();
        let links = vec![(0, 1, 1.0), (0, 2, 1.0), (1, 2, 2f32.sqrt()), (1, 3, 1.0), (2, 3, 1.0), (0, 3, 2f32.sqrt())];
        let cloth = CloRig { particles, links, fans, mass: 1.0, rigid: false, handle: None, ropes: vec![], bones: vec![0, 1, 2, 3], palette_base: 0 };
        ClothRig { cloths: vec![cloth], spheres: vec![], bones: 4, ..ClothRig::default() }
    }

    #[test]
    fn at_rest_the_palette_is_the_identity() {
        let sim = ClothSim::new(&square(), 1);
        for m in sim.palette() {
            let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            assert!(m.iter().zip(identity).all(|(a, b)| (a - b).abs() < 1e-4), "{m:?}");
        }
    }

    #[test]
    fn the_wind_pushes_the_free_edge_and_the_links_hold_it() {
        let mut sim = ClothSim::new(&square(), 1);
        for k in 0..200 {
            sim.update(1.0 / 30.0, k as f32 / 30.0, [0.0, -9.8, 30.0]);
        }

        let c = &sim.cloths[0];
        assert_eq!((c.particles[0].x, c.particles[1].x), ([0.0, 1.0, 0.0], [1.0, 1.0, 0.0]), "pinned");
        assert!(c.particles[2].x[2] > 0.3, "blown along +z: {:?}", c.particles[2].x);
        let l = |a: usize, b: usize| { let d = super::sub(c.particles[a].x, c.particles[b].x); super::dot(d, d).sqrt() };
        assert!((l(0, 2) - 1.0).abs() < 0.05 && (l(2, 3) - 1.0).abs() < 0.05, "{} {}", l(0, 2), l(2, 3));
    }

    #[test]
    fn a_free_rope_hangs_between_its_pinned_ends() {
        let nodes: Vec<Particle> = (0..5).map(|k| Particle::at([k as f32, 0.0, 0.0], if k == 0 || k == 4 { 0.0 } else { 1.0 })).collect();
        let rope = super::RopeRig { nodes, rests: vec![1.1; 4], mass: 5.0, radius: 0.03, v: vec![0.0; 5], node: usize::MAX, particle: usize::MAX };
        let rig = ClothRig { free: vec![rope], ..ClothRig::default() };
        let mut sim = ClothSim::new(&rig, 1);
        for k in 0..300 {
            sim.update(1.0 / 30.0, k as f32 / 30.0, [0.0, -9.8, 30.0]);
        }

        let ropes = sim.ropes();
        let nodes = &ropes[0].0;
        assert_eq!((nodes[0], nodes[4]), ([0.0; 3], [4.0, 0.0, 0.0]), "pinned ends stay");
        assert!(nodes[2][1] < -0.3 && nodes[2][2] > 0.0 && nodes[2][2] < 0.2, "sags, barely blown: {:?}", nodes[2]);
    }

    #[test]
    fn a_link_pulls_a_free_end_to_its_rest() {
        let (mut a, mut b) = (Particle::at([0.0; 3], 0.0), Particle::at([2.0, 0.0, 0.0], 1.0));
        relax(&mut a, &mut b, 1.0, 0.0, 1.0);
        assert_eq!((a.x, b.x), ([0.0; 3], [1.0, 0.0, 0.0]));
        let (mut a, mut b) = (Particle::at([0.0; 3], 1.0), Particle::at([2.0, 0.0, 0.0], 1.0));
        relax(&mut a, &mut b, 1.0, 1.0, 1.0);
        assert_eq!((a.x, b.x), ([0.5, 0.0, 0.0], [1.5, 0.0, 0.0]));
    }

    #[test]
    fn the_wind_follows_the_ship() {
        // Heading +Z: the full wind; heading -Z: forward 1, and the sails sag.
        let ahead = ship_wind([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(ahead, [0.0, -9.8, 30.0]);
        let astern = ship_wind([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]);
        assert_eq!(astern, [0.0, -9.8, 1.0]);
        // Abeam: +Z is the model's -x, clamped to 5 x the forward minimum.
        let abeam = ship_wind([[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]]);
        assert_eq!(abeam, [-5.0, -9.8, 1.0]);
    }

    #[test]
    fn helpers() {
        assert!((quick_sqrt(4.0) - 2.0).abs() < 0.1 && (quick_sqrt(2.0) - 1.414).abs() < 0.1);
        let m = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [1.0, 0.0, 1.0]];
        let i = m3_mul(&m, &m3_inverse(&m));
        assert!((0..3).all(|c| (0..3).all(|r| (i[c][r] - f32::from(u8::from(c == r))).abs() < 1e-5)), "{i:?}");
        let (n, f) = frames_of(&[vec![[0, 1, 2]]], &[Particle::at([0.0; 3], 1.0), Particle::at([1.0, 0.0, 0.0], 1.0), Particle::at([0.0, 1.0, 0.0], 1.0)]);
        assert!((n[0][2] - 1.0).abs() < 0.1 && (f[0][0][0] - 1.0).abs() < 0.1, "{n:?} {f:?}");
    }
}
