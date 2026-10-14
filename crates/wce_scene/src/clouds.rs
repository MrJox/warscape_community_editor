//! Battle cloud planes (SKY_DOME > CLOUD_PLANE_MESH > CLOUD > CLOUD_ELEMENT), rebuilt from empire.retail.dll:
//!
//! - init (fn 0x18e5b10): every cell of a (grid_size + 2)^2 grid spawns a cloud when rand <= cloud_density;
//! - spawn (fn 0x18f0bf0): jitter, cloud rotation, template choice, per-element sprites;
//! - template (fn 0x1905f60): the first CLOUD with this_cloud_only, else weighted by probability;
//! - sprite (fn 0x18d82a0): the per-instance data cloud_plane.hlsl vs30_main reads (TEXCOORD1..5).
//!
//! Random numbers: MSVC rand (state * 214013 + 2531011, 16 high bits / 65535). The dome constants per plane type come from a memory
//! dump of the game (not derived from the XML): near: radius 608000, centre -592800, jitter 0.5, atlas 2 x 2; far: radius 972800,
//! centre -912000, jitter 0, atlas 1 x 2. The cell angle is 2 * acos(-centre / radius) / grid_size (the dome cap down to the
//! horizon), verified on both planes.

use std::f32::consts::PI;

use wce_formats::env_value::{ValueKind, parse_value};
use wce_formats::xml_doc::XmlDoc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CloudElement {
    pub enabled: bool,
    pub scale: f32,
    pub offset: [f32; 3],

    /// 0: random.
    pub texture_index: i32,
    pub texture_row: i32,
    pub texture_column: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CloudTemplate {
    pub this_cloud_only: bool,
    pub probability: f32,
    pub elements: Vec<CloudElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudPlane {
    pub far_plane: bool,
    pub grid_size: i32,
    pub height_scale: f32,
    pub velocity: f32,
    pub density: f32,
    pub flatten: f32,
    pub transmitted: [f32; 3],
    pub reflected: [f32; 3],
    pub shadow: [f32; 3],
    pub transmit_falloff: f32,
    pub shadow_gradient: f32,
    pub shadow_falloff: f32,
    pub horizon_alpha_start: f32,
    pub horizon_alpha_end: f32,
    pub height_variation: f32,
    pub rotation: f32,
    pub cloudset: String,
    pub clouds: Vec<CloudTemplate>,
}

impl Default for CloudPlane {
    fn default() -> Self {
        Self {
            far_plane: false,
            grid_size: 1,
            height_scale: 1.0,
            velocity: 0.0,
            density: 0.0,
            flatten: 0.0,
            transmitted: [1.0; 3],
            reflected: [1.0; 3],
            shadow: [0.0; 3],
            transmit_falloff: 0.0,
            shadow_gradient: 0.0,
            shadow_falloff: 0.0,
            horizon_alpha_start: 0.0,
            horizon_alpha_end: 1.0,
            height_variation: 0.0,
            rotation: 0.0,
            cloudset: "CLOUDSET_1".to_owned(),
            clouds: vec![],
        }
    }
}

/// One sprite: one instance of the cloud quad (the layout of TEXCOORD1..5 in cloud_plane.hlsl vs30_main).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CloudInstance {
    /// Atlas u, v, tile u size, tile v size.
    pub t1: [f32; 4],

    /// Dome angles x, y; dome radius; dome centre (y).
    pub t2: [f32; 4],

    /// 1, sprite rotation, angular size, cloud rotation.
    pub t3: [f32; 4],

    /// Element offset in metres x, y, z; 1.
    pub t4: [f32; 4],

    /// Atlas texture (0 / 1), cloud radius.
    pub t5: [f32; 2],
}

/// MSVC's rand.
#[derive(Clone, Copy, Debug)]
struct Rng {
    state: u32,
}

impl Rng {
    fn next(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(0x343FD).wrapping_add(0x26_9EC3);
        (self.state >> 16) as f32 * (1.0 / 65535.0)
    }
}

struct PlaneConstants {
    radius: f32,
    centre: f32,
    jitter: f32,
    u_tiles: i32,
    v_tiles: i32,
}

fn constants(p: &CloudPlane) -> PlaneConstants {
    if p.far_plane {
        PlaneConstants { radius: 972_800.0, centre: -912_000.0, jitter: 0.0, u_tiles: 1, v_tiles: 2 }
    } else {
        PlaneConstants { radius: 608_000.0, centre: -592_800.0, jitter: 0.5, u_tiles: 2, v_tiles: 2 }
    }
}

struct Geometry {
    cell_angle: f32,
    cell_m: f32,
    cells: i32,
}

fn geometry(p: &CloudPlane) -> Geometry {
    let k = constants(p);
    let cap = (-k.centre / k.radius).acos();
    let a = 2.0 * cap / p.grid_size as f32;
    Geometry { cell_angle: a, cell_m: a * k.radius, cells: p.grid_size + 2 }
}

/// A number attribute (bools as 1 / 0), or its default.
fn attr(d: &XmlDoc, e: usize, name: &str, default: f32) -> f32 {
    let Some(value) = d.get(e, name) else { return default };
    let x = parse_value(&value);
    match x.kind {
        ValueKind::Bool => if x.b { 1.0 } else { 0.0 },
        ValueKind::Float | ValueKind::Int if !x.f.is_empty() => x.f[0],
        _ => default,
    }
}

fn colour(d: &XmlDoc, e: usize, name: &str, out: &mut [f32; 3]) {
    if let Some(value) = d.get(e, name) {
        let x = parse_value(&value);
        if x.kind == ValueKind::Rgb {
            out.copy_from_slice(&x.f[..3]);
        }
    }
}

fn choose<'a>(p: &'a CloudPlane, rng: &mut Rng) -> Option<&'a CloudTemplate> {
    let valid = |c: &CloudTemplate| c.elements.iter().any(|e| e.enabled);
    let mut total = 0.0;
    let mut n = 0;
    for c in &p.clouds {
        if c.this_cloud_only {
            return Some(c);
        }

        if valid(c) {
            total += c.probability;
            n += 1;
        }
    }

    if n == 0 {
        return None;
    }

    let pick = rng.next() * total;
    let mut acc = 0.0;
    for c in p.clouds.iter().filter(|c| valid(c) && c.probability > 0.0) {
        acc += c.probability;
        if acc >= pick {
            return Some(c);
        }
    }

    None
}

/// The original's round to nearest (the SSE "add / sub 2^23" idiom with a correction step).
fn round_index(max_index: f32, r: f32) -> i32 {
    (max_index * r + 0.5).floor() as i32
}

/// The cloud planes of an environment document.
pub fn read_cloud_planes(doc: &XmlDoc) -> Vec<CloudPlane> {
    let els = doc.elements();
    let mut planes = vec![];
    for i in (0..els.len()).filter(|&i| els[i].name == "CLOUD_PLANE_MESH") {
        let mut p = CloudPlane {
            far_plane: doc.get(i, "name").unwrap_or_default().to_ascii_lowercase().contains("far"),
            grid_size: (attr(doc, i, "grid_size", 1.0) as i32).max(1),
            height_scale: attr(doc, i, "height_scale", 1.0),
            velocity: attr(doc, i, "cloud_velocity", 0.0),
            density: attr(doc, i, "cloud_density", 0.0),
            flatten: attr(doc, i, "flatten_factor", 0.0),
            transmit_falloff: attr(doc, i, "sun_transmit_falloff", 0.0),
            shadow_gradient: attr(doc, i, "cloud_shadow_gradient", 0.0),
            shadow_falloff: attr(doc, i, "cloud_shadow_falloff", 0.0),
            horizon_alpha_start: attr(doc, i, "horizon_alpha_start", 0.0),
            horizon_alpha_end: attr(doc, i, "horizon_alpha_end", 1.0),
            height_variation: attr(doc, i, "cloud_height_variation", 0.0),
            rotation: attr(doc, i, "cloud_rotation", 0.0),
            ..Default::default()
        };

        colour(doc, i, "sun_transmitted_colour", &mut p.transmitted);
        colour(doc, i, "sun_reflected_colour", &mut p.reflected);
        colour(doc, i, "cloud_shadow_colour", &mut p.shadow);
        if let Some(cloudset) = doc.get(i, "cloudset") {
            p.cloudset = cloudset;
        }

        for &c in els[i].children.iter().filter(|&&c| els[c].name == "CLOUD") {
            p.clouds.push(CloudTemplate {
                this_cloud_only: attr(doc, c, "this_cloud_only", 0.0) != 0.0,
                probability: attr(doc, c, "probability", 0.0),
                elements: els[c].children.iter().filter(|&&e| els[e].name == "CLOUD_ELEMENT").map(|&e| CloudElement {
                    enabled: attr(doc, e, "enabled", 0.0) != 0.0,
                    scale: attr(doc, e, "scale", 0.0),
                    offset: [attr(doc, e, "offset_x", 0.0), attr(doc, e, "offset_y", 0.0), attr(doc, e, "offset_z", 0.0)],
                    texture_index: attr(doc, e, "texture_index", 0.0) as i32,
                    texture_row: attr(doc, e, "texture_row", 0.0) as i32,
                    texture_column: attr(doc, e, "texture_column", 0.0) as i32,
                }).collect(),
            });
        }

        planes.push(p);
    }

    planes
}

/// A plane's live clouds: the initial fill (fn 0x18e5b10), the drift (fn 0x192aa40: angle x += speed * dt, the cloud dies past the
/// far edge) and new clouds entering at column 0 (fn 0x192a900).
///
/// Verified: the death limit (cells / 2) * cell_angle matches the dumped sprites' angle range; the timer restarts at 0 after a spawn.
/// ASSUMPTION: the spawn interval (object +0x8, source not decoded) is cell_angle / velocity.
#[derive(Clone, Debug)]
pub struct CloudField {
    rng: Rng,
    inst: Vec<CloudInstance>,

    /// Per instance (the sprites of one cloud share it).
    speed: Vec<f32>,
    timer: f32,
}

impl Default for CloudField {
    fn default() -> Self {
        Self { rng: Rng { state: 1 }, inst: vec![], speed: vec![], timer: 0.0 }
    }
}

impl CloudField {

    pub fn instances(&self) -> &[CloudInstance] {
        &self.inst
    }

    pub fn reset(&mut self, p: &CloudPlane, seed: u32) {
        self.inst.clear();
        self.speed.clear();
        self.timer = 0.0;
        self.rng.state = seed;
        let cells = p.grid_size + 2;
        for x in 0..cells {
            for y in 0..cells {
                if p.density >= self.rng.next() {
                    self.spawn(p, x, y);
                }
            }
        }
    }

    pub fn update(&mut self, p: &CloudPlane, dt: f32) {
        if (p.velocity * 0.1).partial_cmp(&0.001) != Some(std::cmp::Ordering::Greater) || dt <= 0.0 {
            return;
        }

        let g = geometry(p);
        let limit = g.cells as f32 * 0.5 * g.cell_angle;
        let mut i = 0;
        while i < self.inst.len() {
            self.inst[i].t2[0] += self.speed[i] * dt;
            if self.inst[i].t2[0] > limit {
                self.inst.swap_remove(i);
                self.speed.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // fn 0x1192a900: one roll per column, then the timer restarts from 0 (it isn't decremented).
        self.timer += dt;
        if self.timer >= g.cell_angle / p.velocity {
            self.timer = 0.0;
            for y in 0..g.cells {
                if p.density >= self.rng.next() {
                    self.spawn(p, 0, y);
                }
            }
        }
    }

    /// fn 0x18f0bf0.
    fn spawn(&mut self, p: &CloudPlane, x: i32, y: i32) {
        let k = constants(p);
        let Geometry { cell_angle, cell_m, cells } = geometry(p);
        let rot_range = p.rotation * PI;
        let rng = &mut self.rng;
        let (mut jx, mut jy) = (0.0, 0.0);
        if k.jitter > 0.0 {
            jx = (rng.next() - 0.5) * k.jitter;
            jy = (rng.next() - 0.5) * k.jitter;
        }

        let cloud_rot = rng.next() * rot_range * 2.0 - rot_range;
        let ax = (x as f32 + jx - cells as f32 * 0.5 + 0.5) * cell_angle;
        let ay = (y as f32 + jy - cells as f32 * 0.5 + 0.5) * cell_angle;

        // 1 +- 2.5%.
        let speed = if p.velocity * 0.1 > 0.001 { (1.0 + rng.next() * 65535.0 * 7.629_511e-7 - 0.025) * p.velocity } else { 0.0 };
        let Some(t) = choose(p, rng) else { return };

        let mut cloud_radius = 0.0f32;
        for e in t.elements.iter().filter(|e| e.enabled) {
            let s = e.scale.min(4.0);
            let r = (e.offset[0] * cell_m * e.offset[0] * cell_m + e.offset[2] * cell_m * e.offset[2] * cell_m).sqrt() + s * cell_m * 0.5;
            cloud_radius = cloud_radius.max((r as i32) as f32);
        }

        let hv = (rng.next() - 0.5) * p.height_variation * 2.0 * cell_m;
        for e in &t.elements {
            let (r6, r5, r7, r4) = (rng.next(), rng.next(), rng.next(), rng.next());
            if !e.enabled {
                continue;
            }

            let map = if k.v_tiles > 1 {
                if e.texture_index > 0 { (e.texture_index - 1).min(k.v_tiles - 1) } else { round_index((k.v_tiles - 1) as f32, r6) }
            } else {
                0
            };

            let du = 1.0 / k.u_tiles as f32;
            let (mut u, mut v) = (0.0, 0.0);
            if k.u_tiles > 1 {
                let pick = |chosen: i32, r: f32| if chosen > 0 { (chosen - 1) as f32 } else { round_index((k.u_tiles - 1) as f32, r) as f32 };
                u = pick(e.texture_column, r7) * du;
                v = pick(e.texture_row, r4) * du;
            }

            self.inst.push(CloudInstance {
                t1: [u, v, du, du],
                t2: [ax, ay, k.radius, k.centre],
                t3: [1.0, r5 * rot_range * 2.0 - rot_range, e.scale.min(4.0) * cell_angle, cloud_rot],
                t4: [e.offset[0] * cell_m, e.offset[1] * cell_m + hv, e.offset[2] * cell_m, 1.0],
                t5: [map as f32, cloud_radius],
            });

            self.speed.push(speed);
        }
    }
}

/// A plane's initial clouds; the same for the same seed (the game's state is per session, so positions differ; the distribution matches).
pub fn generate_clouds(p: &CloudPlane, seed: u32) -> Vec<CloudInstance> {
    let mut field = CloudField::default();
    field.reset(p, seed);
    field.inst
}

/// `rigidmodels/skybox/cloudsets/cloudset_2/near_set1.dds` and so on.
pub fn cloud_texture(p: &CloudPlane, map: i32) -> String {
    let file = match (p.far_plane, map != 0) {
        (true, true) => "far2.dds",
        (true, false) => "far1.dds",
        (false, true) => "near_set2.dds",
        (false, false) => "near_set1.dds",
    };

    format!("rigidmodels/skybox/cloudsets/{}/{file}", p.cloudset.to_ascii_lowercase())
}
