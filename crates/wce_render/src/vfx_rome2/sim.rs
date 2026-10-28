//! Rome 2's particle simulation: the billboard emitter classes of empire.retail.dll (`continuous_mesh`, `burst_mesh`), ported as
//! the game runs them on the CPU. Function addresses and field offsets are the DLL's; docs/re/rome2_vfx.md has the whole picture.
//!
//! Everything draws from one shared generator (DAT_11a46f50) in the game's order, so the order of the draws below is the game's too.
//! Not ported yet (TODO(rome2)): spawn spacing along a moving emitter (fn 0x1155f730), spawning on meshes (SpawnByMesh, fn
//! 0x1150ec20 / 0x1150de20), ribbons (`continuous_ribbon`), particle events (OnParticleSpawn child effects) and the rigid classes.

#![allow(clippy::needless_range_loop)]

use wce_assets::vfx_def::VfxEmitterDef;

/// 1 / 65535 (DAT_11786c48): a draw is `(state >> 16) * this`.
const INV_65535: f32 = f32::from_bits(0x3780_0080);

/// pi / 360 (DAT_1193b8b8): EmitterRotation's degrees to half angles.
const HALF_DEGREE: f32 = f32::from_bits(0x3c0e_fa35);

/// pi / 180 (DAT_11797980): SpawnRadius's angle.
const DEGREE: f32 = f32::from_bits(0x3c8e_fa35);

/// pi / 180 (DAT_1191c38c, one ulp above the other): SpawnSpread's minimum.
const DEGREE_SPREAD: f32 = f32::from_bits(0x3c8e_fa36);

/// (pi / 180) / 65535 (DAT_1193b8b0): SpawnSpread's range per raw draw.
const DEGREE_PER_RAW: f32 = f32::from_bits(0x348e_fac5);

/// 2 pi / 65535 (DAT_117ad974): the spread's azimuth per raw draw.
const TURN_PER_RAW: f32 = f32::from_bits(0x38c9_10a4);

/// 2 pi (DAT_11797a08): angles wrap at it.
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);

/// DAT_1193b8f4: GravityAmount 100 is 9.81 m/s^2.
const GRAVITY: f32 = f32::from_bits(0xbdc8_e8a7);

/// DAT_11784f50: the longest frame the spawning counts, and the force scale.
const TENTH: f32 = 0.1;

/// DAT_118873e8 (double).
const EPS: f64 = 1e-6;

/// VFX_ALPHA_CUTOFF: particles at or under it are not drawn.
pub const ALPHA_CUTOFF: f32 = f32::from_bits(0x3c23_d70a);

/// VFX_PRE_INITIALIZE_TIME_STEP: the steps of a negative StartTime's catch-up.
const PREWARM_STEP: f32 = f32::from_bits(0x3e4c_cccd);

/// VFX_WIND_ANGLE_DELTA.
const WIND_ANGLE_DELTA: f32 = 5.0;

/// VFX_WIND_SCALE_FACTOR: the environment's wind to the particles'.
pub const WIND_SCALE: f32 = 20.0;

/// VFX_DISTANCE_CULLING_SCALE_CAPPED.
const CULL_SCALE_CAPPED: bool = true;

/// VFX_FAR_DISTANCE_CULLING_BIAS.
const FAR_CULL_BIAS: f32 = 0.0;

/// VFX_DEFAULT_DISTANCE_FROM_CAMERA: the distance of camera-following effects.
pub const DEFAULT_CAMERA_DISTANCE: f32 = f32::from_bits(0x3c23_d70a);

/// The record (the vertex of one particle: 31 floats, POSITION0..6 and TEXCOORD1 of the generated shaders).
pub const RECORD: usize = 31;
pub const R_SPAWN: usize = 0;
pub const R_LIFE: usize = 1;
pub const R_POS: usize = 12;
pub const R_NOW: usize = 15;
pub const R_ALPHA: usize = 30;

/// The game's generator (0x343fd / 0x269ec3), shared by all emitters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    pub fn raw(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        self.0 >> 16
    }

    pub fn unit(&mut self) -> f32 {
        self.raw() as f32 * INV_65535
    }

    /// `draw * (max - min) + min`, the form of most ranged properties.
    fn range(&mut self, r: (f32, f32)) -> f32 {
        self.unit() * (r.1 - r.0) + r.0
    }

    /// FUN_115cadd0: three draws, kept last first.
    fn vec3(&mut self) -> [f32; 3] {
        let a = self.unit();
        let b = self.unit();
        let c = self.unit();
        [c, b, a]
    }
}

/// The square root of the update code: `((bits - 0x3f800000) >> 1) + 0x3f800000` (an arithmetic shift).
pub fn fast_sqrt(x: f32) -> f32 {
    f32::from_bits((x.to_bits() as i32).wrapping_sub(0x3f80_0000).wrapping_shr(1).wrapping_add(0x3f80_0000) as u32)
}

/// `_CIfmod` of an angle by 2 pi (x87 precision).
fn wrap_angle(a: f32) -> f32 {
    ((a as f64) % (TWO_PI as f64)) as f32
}

/// MSVC's float to unsigned (through a double).
fn u32_to_f32(u: u32) -> f32 {
    u as f64 as f32
}

type V3 = [f32; 3];

/// Quaternions are (x, y, z, w).
type Quat = [f32; 4];

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn len2(a: V3) -> f32 {
    a[1] * a[1] + a[0] * a[0] + a[2] * a[2]
}

/// The quaternion product a * b.
fn qmul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] + a[1] * b[3] + a[2] * b[0] - a[0] * b[2],
        a[3] * b[2] + a[2] * b[3] + a[0] * b[1] - a[1] * b[0],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The rotation matrix of a unit quaternion (rows; v' = M v).
fn qmatrix(q: Quat) -> [[f32; 3]; 3] {
    let (x2, y2, z2, w2) = (q[0] + q[0], q[1] + q[1], q[2] + q[2], q[3] + q[3]);
    [
        [1.0 - y2 * q[1] - z2 * q[2], x2 * q[1] - w2 * q[2], x2 * q[2] + w2 * q[1]],
        [x2 * q[1] + w2 * q[2], 1.0 - x2 * q[0] - z2 * q[2], y2 * q[2] - w2 * q[0]],
        [x2 * q[2] - w2 * q[1], y2 * q[2] + w2 * q[0], 1.0 - x2 * q[0] - y2 * q[1]],
    ]
}

fn mul(m: &[[f32; 3]; 3], v: V3) -> V3 {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

fn rotate(q: Quat, v: V3) -> V3 {
    mul(&qmatrix(q), v)
}

/// The class of a billboard emitter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Continuous,
    Burst,
}

/// An emitter definition: the properties the billboard classes read, with the constructors' defaults (fn 0x114b2b80, and
/// 0x114b1aa0 / 0x114b14d0 for the classes). Ranged properties are (min, max).
#[derive(Clone, Debug, PartialEq)]
pub struct Desc {
    pub class: Class,
    pub enable: bool,
    pub visible: bool,
    pub wind_variance: f32,
    pub follow: bool,
    pub local_space: bool,
    pub cull: [f32; 4],
    pub particle_cull: [f32; 4],
    pub spawn_by_mesh: bool,
    pub particle_life: (f32, f32),
    pub start_time: (f32, f32),

    /// InfiniteLife (continuous) or InfiniteBursts (burst): the same field (+0x18c).
    pub infinite: bool,
    pub deceleration: bool,
    pub wind: bool,
    pub decel_start: (f32, f32),
    pub decel_amount: (f32, f32),
    pub gravity: (f32, f32),
    pub rotational: (f32, f32),
    pub central: (f32, f32),
    pub speed: (f32, f32),
    pub spread: (f32, f32),
    pub spread_method: bool,
    pub single_scale: bool,
    pub scale_start: [f32; 2],
    pub scale_mid: [f32; 2],
    pub scale_end: [f32; 2],
    pub mid_point: f32,
    pub scale_variation: f32,
    pub spawn_position: V3,
    pub spawn_area: V3,

    /// Radius min, max, angle min, max (degrees).
    pub spawn_radius: [f32; 4],
    pub rotation: V3,
    pub spin: f32,

    /// Continuous: LifeTime, ParticlesPerSecond, SpawnSpacing, SpawnSpacingMaxSpeed, ParticlesMax.
    pub life_time: (f32, f32),
    pub rate: (f32, f32),
    pub spacing: f32,
    pub spacing_max_speed: f32,
    pub particles_max: u32,

    /// Burst: Bursts, BurstInterval, BurstIntervalLength, ParticlesPerBurst.
    pub bursts: (f32, f32),
    pub interval: (f32, f32),
    pub length: (f32, f32),
    pub per_burst: (f32, f32),
    pub recycle: bool,
    pub turbulence: (f32, f32),
    pub speed_max: f32,
}

impl Desc {
    /// The definition of a billboard emitter; None for the other classes.
    pub fn from_def(d: &VfxEmitterDef) -> Option<Self> {
        let class = match d.kind.as_str() {
            "continuous_mesh" => Class::Continuous,
            "burst_mesh" => Class::Burst,
            _ => return None,
        };

        let v2 = |name: &str, default: [f32; 2]| {
            let mut v = default;
            d.vec(name, &mut v);
            v
        };

        let v3 = |name: &str| {
            let mut v = [0.0; 3];
            d.vec(name, &mut v);
            v
        };

        let v4 = |name: &str, default: [f32; 4]| {
            let mut v = default;
            d.vec(name, &mut v);
            v
        };

        let r = |name: &str, lo: f32, hi: f32| d.range(name, lo, hi);
        let burst = class == Class::Burst;
        Some(Self {
            class,
            enable: d.b("Enable", true),
            visible: d.b("Visible", true),
            wind_variance: d.f("WindVarianceFactor", 0.0),
            follow: d.b("ParticlesFollowEmitter", false),
            local_space: d.b("ParticlesLocalSpace", false),
            cull: v4("CullDistances", [0.0; 4]),
            particle_cull: v4("ParticleCullDistances", [0.0; 4]),
            spawn_by_mesh: d.b("SpawnByMesh", false),
            particle_life: r("ParticleLifeTime", 0.0, 0.0),
            start_time: r("StartTime", 0.0, 0.0),
            infinite: d.b(if burst { "InfiniteBursts" } else { "InfiniteLife" }, false),
            deceleration: d.b("Deceleration", false),
            wind: d.b("EffectedByWind", false),
            decel_start: r("DecelerationStartTime", 0.0, 0.0),
            decel_amount: r("DecelerationAmount", 0.0, 0.0),
            gravity: r("GravityAmount", 0.0, 0.0),
            rotational: r("RotationalForce", 0.0, 0.0),
            central: r("CentralForce", 0.0, 0.0),
            speed: r("SpawnSpeed", 0.0, 0.0),
            spread: r("SpawnSpread", 90.0, 90.0),
            spread_method: d.b("SpreadMethod", true),
            single_scale: d.b("ParticleScaleUseSingleScale", false),
            scale_start: v2("ParticleScaleStart", [1.0; 2]),
            scale_mid: v2("ParticleScaleMid", [1.0; 2]),
            scale_end: v2("ParticleScaleEnd", [1.0; 2]),
            mid_point: d.f("ParticleScaleMidPointTime", 0.5),
            scale_variation: d.f("ParticleScaleVariation", 0.0),
            spawn_position: v3("SpawnPosition"),
            spawn_area: v3("SpawnArea"),
            spawn_radius: v4("SpawnRadius", [0.0, 0.0, 0.0, 360.0]),
            rotation: v3("EmitterRotation"),
            spin: d.f("EmitterSpinSpeed", 0.0),
            life_time: r("LifeTime", 50.0, 50.0),
            rate: r("ParticlesPerSecond", 10.0, 10.0),
            spacing: d.f("SpawnSpacing", 0.0),
            spacing_max_speed: d.f("SpawnSpacingMaxSpeed", 80.0),
            particles_max: d.i("ParticlesMax", 0).max(0) as u32,
            bursts: r("Bursts", 10.0, 10.0),
            interval: r("BurstInterval", 2.0, 2.0),
            length: r("BurstIntervalLength", 0.0, 0.0),
            per_burst: r("ParticlesPerBurst", 10.0, 10.0),
            recycle: d.b("RecycleClipped", false),
            turbulence: (d.f("TurbulenceStart", 0.0), d.f("TurbulenceEnd", 0.0)),
            speed_max: d.f("ParticleSpeedMax", 1000.0),
        })
    }
}

/// What an emitter reads of its effect instance: the scale (+0x84), the colour (+0x18) and the reference it came from (+0x58:
/// start_time, infinite_life), if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Owner {
    pub scale: f32,
    pub colour: [f32; 4],
    pub reference: Option<(f32, bool)>,
}

impl Owner {
    /// FUN_11515d80: the emitter lives forever (its own flag, unless a reference says otherwise).
    fn infinite(&self, d: &Desc) -> bool {
        self.reference.is_none_or(|(_, infinite)| infinite) && d.infinite
    }
}

/// A particle: its record (the vertex) and the rest of the game's particle object (0x48 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub rec: [f32; RECORD],

    /// Start, mid and end sizes (x, y) with the variation (+0xc..+0x20).
    size: [f32; 6],
    vel: V3,
    decel_start: f32,
    decel_amount: f32,
    gravity: f32,
    rotational: f32,
    central: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Self { rec: [0.0; RECORD], size: [1.0; 6], vel: [0.0; 3], decel_start: 0.0, decel_amount: 0.0, gravity: 0.0, rotational: 0.0, central: 0.0 }
    }
}

/// The frame's inputs of the emitters (the effect update's arguments).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub wind: V3,
    pub camera: V3,
    pub prev_camera: V3,

    /// This frame's random turn of the wind's variance (fn 0x115639a0).
    pub phase_step: f32,
}

/// An emitter instance (the game's billboard emitter object).
#[derive(Clone, Debug)]
pub struct Emitter {
    pub desc: Desc,

    /// The generated shaders' branch (FUN_1155fbd0) and the texture slots (FUN_11550a80: diffuse + 10 * normal; -1 for none).
    pub index: u32,
    pub packed: u32,
    pub tint: [f32; 4],

    /// The view its render stack draws in (scene_view_type: 0 DeferredAlpha, 1 Distortion, 2 SeaColourOverlay).
    pub view: i32,

    /// +0x8c.
    pub scale: f32,
    pub pos: V3,
    pub quat: Quat,
    pub vel: V3,
    prev_pos: V3,
    prev_quat: Quat,
    rot_quat: Quat,
    spin_angle: f32,
    wind_phase: f32,
    last: f32,
    state: u8,
    delay: f32,
    start: f32,

    /// Continuous: the emitter's life left. Burst: the next burst's time (negative before the first).
    life: f32,
    bursts_left: i32,
    rate: f32,
    accum: f32,
    fade: f32,
    lod: f32,
    pub visible: bool,
    pub done: bool,
    pub hidden: bool,
    stopped: bool,
    alive: bool,
    halted: bool,

    /// The pool (fn 0x114f1dc0): live particles are `first..`; the records are in this order.
    pub parts: Vec<Particle>,
    pub first: usize,
}

impl Emitter {
    pub fn new(desc: Desc, index: u32, packed: u32, tint: [f32; 4]) -> Self {
        Self {
            desc, index, packed, tint, view: 0, scale: 1.0, pos: [0.0; 3], quat: [0.0, 0.0, 0.0, 1.0], vel: [0.0; 3], prev_pos: [0.0; 3], prev_quat: [0.0, 0.0, 0.0, 1.0],
            rot_quat: [0.0, 0.0, 0.0, 1.0], spin_angle: 0.0, wind_phase: 0.0, last: 0.0, state: 0, delay: 0.0, start: 0.0, life: 0.0, bursts_left: 0, rate: 0.0,
            accum: 0.0, fade: 1.0, lod: 1.0, visible: true, done: false, hidden: false, stopped: false, alive: false, halted: false, parts: Vec::new(), first: 0,
        }
    }

    fn s(&self, o: &Owner) -> f32 {
        o.scale * self.scale
    }

    /// The class init (vtable +4): FUN_11528bb0 (continuous) / FUN_115288a0 (burst).
    pub fn init(&mut self, o: &Owner, rng: &mut Rng) {
        self.begin(o, rng);
        let d = &self.desc;
        let n = match d.class {
            Class::Continuous => {
                self.accum = 1.0;
                self.life = rng.range(d.life_time);
                self.rate = rng.range(d.rate);
                if d.spacing > 0.0 {
                    let s = o.scale * self.scale;
                    self.rate = (d.spacing_max_speed * s) / (d.spacing * s);
                }

                let n1 = (d.particle_life.1 * self.rate).round_ties_even() as u32;
                let mut n = if o.infinite(d) { n1 } else { ((self.life * self.rate) as u32).min(n1) };
                n += 1;
                if d.particles_max != 0 && d.particles_max <= n {
                    n = d.particles_max;
                }

                n
            }

            Class::Burst => {
                self.bursts_left = rng.range(d.bursts) as i32;
                self.life = -d.particle_life.1;
                if o.infinite(d) {
                    self.bursts_left = 1;
                }

                let per_burst = d.per_burst.1;
                let span = d.length.1 + d.interval.1;
                let mut k = 1.0;
                if span > 0.0 {
                    let overlap = (d.particle_life.1 + span) / span;
                    k = if o.infinite(d) { overlap } else { overlap.min(self.bursts_left as f32) };
                }

                (per_burst * k).round_ties_even().max(0.0) as u32
            }
        };

        self.parts = vec![Particle::default(); n as usize];
        self.first = n as usize;
    }

    /// FUN_11529480: every class's init starts with it.
    fn begin(&mut self, o: &Owner, rng: &mut Rng) {
        self.alive = true;
        self.state = 0;
        self.halted = false;
        self.last = 0.0;
        let start = rng.range(self.desc.start_time) + o.reference.map_or(0.0, |(start, _)| start);
        self.start = start;
        self.delay = start;

        // FUN_115676b0: EmitterRotation (degrees) as a quaternion; the spin restarts.
        self.visible = self.desc.visible;
        let r = self.desc.rotation;
        let (s1, c1) = (r[0] * HALF_DEGREE).sin_cos();
        let (s2, c2) = (r[1] * HALF_DEGREE).sin_cos();
        let (s3, c3) = (r[2] * HALF_DEGREE).sin_cos();
        let (a, b, c, e) = (c3 * c2, s3 * s2, s3 * c2, c3 * s2);
        self.spin_angle = 0.0;
        self.rot_quat = [a * s1 - b * c1, c * s1 + e * c1, c * c1 - e * s1, b * s1 + a * c1];
    }

    /// FUN_115186a0: EmitterRotation turned by the spin about its own Y.
    fn spin_quat(&self) -> Quat {
        let q = self.rot_quat;
        let axis = [2.0 * q[0] * q[1] - 2.0 * q[3] * q[2], 1.0 - 2.0 * q[0] * q[0] - 2.0 * q[2] * q[2], 2.0 * q[1] * q[2] + 2.0 * q[3] * q[0]];
        let (s, c) = (self.spin_angle * 0.5).sin_cos();
        qmul([axis[0] * s, axis[1] * s, axis[2] * s, c], q)
    }

    /// FUN_11506600: the distance culling of the emitter (every frame, before its state machine).
    fn cull(&mut self, d: f32, o: &Owner) {
        let s = self.s(o);
        let far = if CULL_SCALE_CAPPED && s <= 1.0 { 1.0 } else { s };
        let near = s.min(1.0);
        let c = self.desc.cull;
        let z = c[2] * far + FAR_CULL_BIAS;
        let w = c[3] * far + FAR_CULL_BIAS;
        let x = near * c[0];
        let y = c[1] * near;
        self.stopped = w < d;
        self.hidden = d < x || w < d;
        let r = d / if x == 0.0 { w } else { x };
        self.lod = if r <= 1.0 { 1.0 - r * r * r * r } else { 0.0 };
        self.fade = if x <= d && d <= w {
            if y < d {
                if z <= d {
                    if w - z != 0.0 { 1.0 - (d - z) / (w - z) } else { 0.0 }
                } else {
                    1.0
                }
            } else if y - x != 0.0 {
                (d - x) / (y - x)
            } else {
                0.0
            }
        } else {
            0.0
        };
    }

    /// The state machine (vtable +0x10: FUN_11562cf0 continuous, FUN_11562870 burst).
    #[allow(clippy::too_many_arguments)]
    pub fn machine(&mut self, t: f32, active: bool, distance: f32, i: &Inputs, o: &Owner, rng: &mut Rng) {
        if !active {
            self.last = t;
            self.state = 2;
            self.stopped = true;
        } else {
            self.cull(distance, o);
        }

        match self.state {
            0 => {
                if self.last == 0.0 {
                    self.prev_pos = self.pos;
                    self.last = t;
                    self.prev_quat = self.quat;
                }

                if self.start >= 0.0 {
                    self.delay -= t - self.last;
                    if self.delay < 0.0 {
                        self.delay = 0.0;
                        self.state = 1;
                    }

                    self.last = t;
                    return;
                }

                // A negative StartTime: the emitter catches up in steps (as if started that long ago).
                let mut s = self.start + t;
                while s < t {
                    self.step(s, i, o, rng);
                    s += PREWARM_STEP;
                }

                self.delay = 0.0;
                self.state = 1;
            }

            1 => {
                if self.stopped {
                    self.state = 2;
                }

                self.step(t, i, o, rng);
            }

            _ => {
                if o.infinite(&self.desc) && self.alive {
                    // FUN_115007c0: the particles go; a running emitter starts again (vtable +8 is the init).
                    self.parts.clear();
                    self.first = 0;
                    if !self.stopped {
                        self.init(o, rng);
                    }
                } else {
                    self.done = true;
                    self.state = 1;
                }

                if self.halted {
                    self.done = true;
                }
            }
        }
    }

    fn step(&mut self, t: f32, i: &Inputs, o: &Owner, rng: &mut Rng) {
        match self.desc.class {
            Class::Continuous => self.step_continuous(t, i, o, rng),
            Class::Burst => self.step_burst(t, i, o, rng),
        }
    }

    /// The emitter's own motion, the shared start of the steps.
    fn advance(&mut self, t: f32) -> f32 {
        let dt = t - self.last;
        for k in 0..3 {
            self.pos[k] += self.vel[k] * dt;
        }

        if self.desc.spin != 0.0 {
            self.spin_angle = wrap_angle(self.desc.spin * dt + self.spin_angle);
        }

        dt
    }

    /// The shared end of the steps.
    fn finish(&mut self, t: f32) {
        self.last = t;
        self.prev_pos = self.pos;
        self.prev_quat = self.quat;
    }

    /// FUN_11566350.
    fn step_continuous(&mut self, t: f32, i: &Inputs, o: &Owner, rng: &mut Rng) {
        let dt = self.advance(t);
        self.retire(t);
        self.life -= dt;
        let spawning = (self.life > 0.0 || o.infinite(&self.desc)) && !self.halted;
        if !self.stopped && dt > 0.0 {
            self.update(t, dt, i, o, rng);
        }

        // TODO(rome2): with SpawnSpacing, FUN_1155f730 spawns along the emitter's path instead; the placed effects do not move.
        if !self.stopped && spawning && dt > 0.0 && self.alive && self.s(o) * self.desc.spacing <= 0.0 {
            let a = dt.min(TENTH) * self.rate + self.accum;
            self.accum = a;
            if a >= 1.0 {
                let n = (a.floor() as u32).min(self.first as u32);
                for k in 0..n {
                    let time = if self.rate != 0.0 { u32_to_f32(k) / self.rate + t } else { t };
                    let life = rng.range(self.desc.particle_life);
                    if let Some(slot) = self.alloc() {
                        let (pos, dir) = self.spawn_point(o, rng);
                        self.spawn(slot, time, life, pos, dir, o, rng);
                    }

                    self.accum -= 1.0;
                }
            }
        }

        let over = (self.life <= 0.0 && !o.infinite(&self.desc)) || self.halted;
        if over && self.first == self.parts.len() {
            self.done = true;
        }

        self.finish(t);
    }

    /// FUN_11564ec0.
    fn step_burst(&mut self, t: f32, i: &Inputs, o: &Owner, rng: &mut Rng) {
        let dt = self.advance(t);
        self.retire(t);
        if !self.stopped && dt > 0.0 {
            self.update(t, dt, i, o, rng);
        }

        if self.bursts_left != 0 && !self.halted && dt > 0.0 {
            let d = self.desc.clone();
            // (time, length, interval) of the bursts due.
            let mut events: Vec<(f32, f32, f32)> = Vec::new();
            if self.life >= 0.0 {
                if t - self.life > 0.0 {
                    // FUN_1150d710.
                    let end = self.life + (t - self.life);
                    let mut at = self.life;
                    loop {
                        let length = rng.range(d.length);
                        let interval = rng.range(d.interval);
                        events.push((at, length, interval));
                        at += interval + length;
                        if at >= end {
                            break;
                        }
                    }
                }
            } else {
                let length = rng.range(d.length);
                let interval = rng.range(d.interval);
                events.push((t, length, interval));
            }

            if self.alive && !events.is_empty() && !self.stopped {
                let density = 1.0;
                let counts: Vec<u32> = events.iter().map(|_| ((rng.range(d.per_burst) * density) as i32).max(0) as u32).collect();
                let n = events.len().min(self.bursts_left.max(0) as usize);
                for (&(time, length, interval), &count) in events.iter().zip(&counts).take(n) {
                    let mut pending = Vec::with_capacity(count as usize);
                    if count != 0 {
                        let inv = 1.0 / u32_to_f32(count);
                        for k in 0..count {
                            let at = u32_to_f32(k) * length * inv + time;
                            let life = rng.range(d.particle_life);
                            pending.push((at, life));
                        }
                    }

                    // Unlike the continuous class, the position and motion draws come before the slot is taken.
                    for (at, life) in pending {
                        let (pos, dir) = self.spawn_point(o, rng);
                        let (vel, ds, da, g) = self.motion_draws(o, rng, dir);
                        if let Some(slot) = self.alloc() {
                            let rv = rng.vec3();
                            self.create(slot, at, life, rv, pos, vel, ds, da, g, o, rng);
                        }
                    }

                    if !o.infinite(&d) {
                        self.bursts_left -= 1;
                    }

                    self.life = time + length + interval;
                }
            }
        }

        let over = self.bursts_left == 0 || self.halted || self.stopped;
        if over && self.first == self.parts.len() {
            self.done = true;
        }

        self.finish(t);
    }

    /// vtable +0x38 (FUN_115185a0): a free slot, taken from the front.
    fn alloc(&mut self) -> Option<usize> {
        if self.first == 0 {
            return None;
        }

        self.first -= 1;
        Some(self.first)
    }

    /// FUN_1150a4d0: the particles past their life go (vtable +0x34, FUN_1155c7e0: swapped with the front one, which leaves).
    fn retire(&mut self, t: f32) {
        let mut k = self.first;
        while k < self.parts.len() {
            let r = &self.parts[k].rec;
            let spawn = r[R_SPAWN];
            if spawn <= t && r[R_LIFE] + spawn < t {
                // TODO(rome2): the death events (+0x35e with EventEnable).
                if k != self.first {
                    self.parts.swap(k, self.first);
                }

                self.first += 1;
            }

            k += 1;
        }
    }

    /// FUN_11519380 (without meshes): the spawn position (world) and direction.
    fn spawn_point(&self, o: &Owner, rng: &mut Rng) -> (V3, V3) {
        let d = &self.desc;
        let s = self.s(o);
        let p = self.spin_quat();

        // FUN_11518eb0: the SpawnRadius ring (angle from the first draw, radius from the second) and the SpawnArea box.
        let a = rng.raw();
        let b = rng.raw();
        let radius = b as f32 * INV_65535 * (d.spawn_radius[1] - d.spawn_radius[0]) + d.spawn_radius[0];
        let angle = (a as f32 * (d.spawn_radius[3] - d.spawn_radius[2]) * INV_65535 + d.spawn_radius[2]) * DEGREE;
        let (sin, cos) = angle.sin_cos();
        let rv = rng.vec3();
        let raw = [
            (cos * radius + (rv[0] - 0.5) * d.spawn_area[0]) * s,
            (rv[1] - 0.5) * d.spawn_area[1] * s,
            (sin * radius + (rv[2] - 0.5) * d.spawn_area[2]) * s,
        ];

        let mut v = rotate(p, raw);
        for k in 0..3 {
            v[k] += d.spawn_position[k] * s;
        }

        let offset = rotate(self.quat, v);

        // FUN_11518a90: SpawnSpread's cone (elevation from the second draw, azimuth from the first), or away from SpawnPosition.
        let dir = if d.spread_method {
            let a = rng.raw();
            let b = rng.raw();
            let elevation = b as f32 * DEGREE_PER_RAW * (d.spread.1 - d.spread.0) + d.spread.0 * DEGREE_SPREAD;
            let azimuth = a as f32 * TURN_PER_RAW;
            let (se, ce) = elevation.sin_cos();
            let (sa, ca) = azimuth.sin_cos();
            [ce * ca, se, -(ce * sa)]
        } else {
            let mut v = [raw[0] - d.spawn_position[0], raw[1] - d.spawn_position[1], raw[2] - d.spawn_position[2]];
            let l2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
            if l2 > 0.0 {
                let inv = 1.0 / l2.sqrt();
                v = [v[0] * inv, v[1] * inv, v[2] * inv];
            }

            v
        };

        let dir = rotate(qmul(self.quat, p), dir);
        ([offset[0] + self.pos[0], offset[1] + self.pos[1], offset[2] + self.pos[2]], dir)
    }

    /// FUN_114f9e00 (continuous): the draws after the slot, then the particle.
    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, slot: usize, time: f32, life: f32, pos: V3, dir: V3, o: &Owner, rng: &mut Rng) {
        let (vel, ds, da, g) = self.motion_draws(o, rng, dir);
        let rv = rng.vec3();
        self.create(slot, time, life, rv, pos, vel, ds, da, g, o, rng);
    }

    /// The speed, deceleration and gravity draws of a new particle.
    fn motion_draws(&self, o: &Owner, rng: &mut Rng, dir: V3) -> (V3, f32, f32, f32) {
        let d = &self.desc;
        let s = self.s(o);
        let speed = rng.range(d.speed);
        let vel = [dir[0] * speed * s, dir[1] * speed * s, dir[2] * speed * s];
        let ds = (rng.unit() * (d.decel_start.1 - d.decel_start.0) + d.decel_start.0) * s;
        let da = rng.range(d.decel_amount);
        let g = rng.range(d.gravity) * s;
        (vel, ds, da, g)
    }

    /// FUN_11506080: the record and the particle; then RotationalForce and CentralForce, and the size variation (FUN_114ed360).
    #[allow(clippy::too_many_arguments)]
    fn create(&mut self, slot: usize, time: f32, life: f32, rv: V3, pos: V3, vel: V3, ds: f32, da: f32, g: f32, o: &Owner, rng: &mut Rng) {
        let s = self.s(o);
        let d = &self.desc;
        let mut r = [1.0f32; RECORD];
        r[0] = time;
        r[1] = life;
        r[2] = u32_to_f32(self.index);
        r[3] = u32_to_f32(self.packed);
        r[4..7].copy_from_slice(&rv);
        r[7] = s;
        for base in [8, 12, 16, 20] {
            r[base..base + 3].copy_from_slice(&pos);
        }

        r[11] = 0.0;
        r[15] = 0.0;
        r[19] = 0.0;
        let rotational = rng.range(d.rotational) * o.scale * self.scale;
        let central = rng.range(d.central) * o.scale * self.scale;
        let k = d.scale_variation + 1.0;
        let vary = |v: f32, x: f32| v + (k * v - v) * x;
        let mut size = [1.0; 6];
        size[0] = vary(d.scale_start[0], rv[0]);
        size[1] = vary(d.scale_start[1], rv[0]);
        if !d.single_scale {
            size[2] = vary(d.scale_mid[0], rv[1]);
            size[3] = vary(d.scale_mid[1], rv[1]);
            size[4] = vary(d.scale_end[0], rv[2]);
            size[5] = vary(d.scale_end[1], rv[2]);
        }

        self.parts[slot] = Particle { rec: r, size, vel, decel_start: ds, decel_amount: da, gravity: g, rotational, central };
    }

    /// FUN_11568640: the particles' update.
    fn update(&mut self, t: f32, dt: f32, i: &Inputs, o: &Owner, rng: &mut Rng) {
        let d = self.desc.clone();
        let mut wind = [0.0; 3];
        if d.wind {
            wind = i.wind;
            self.wind_phase = wrap_angle(self.wind_phase + i.phase_step);
            let (sin, cos) = self.wind_phase.sin_cos();
            let k = d.wind_variance * fast_sqrt(len2(wind));
            wind[0] += cos * k;
            wind[2] += sin * k;
        }

        let delta = [self.pos[0] - self.prev_pos[0], self.pos[1] - self.prev_pos[1], self.pos[2] - self.prev_pos[2]];
        let frame = if d.local_space {
            // The emitter's turn of this frame: R(now) R(before)^T.
            let (a, b) = (qmatrix(self.quat), qmatrix(self.prev_quat));
            std::array::from_fn(|r| std::array::from_fn(|c| a[r][0] * b[c][0] + a[r][1] * b[c][1] + a[r][2] * b[c][2]))
        } else {
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        };

        let s = self.s(o);
        let far = if CULL_SCALE_CAPPED && s <= 1.0 { 1.0 } else { s };
        let near = s.min(1.0);
        let pc = d.particle_cull;
        let (x, y) = (pc[0] * near, pc[1] * near);
        let (z, w) = (pc[2] * far + FAR_CULL_BIAS, pc[3] * far + FAR_CULL_BIAS);
        let force_q = qmul(self.quat, self.spin_quat());
        for k in self.first..self.parts.len() {
            let spawn = self.parts[k].rec[R_SPAWN];
            if t <= spawn {
                if t < spawn {
                    self.follow(k, delta, &frame);
                }

                continue;
            }

            let p = &mut self.parts[k];
            p.rec[R_NOW] = t;
            let tt = (((t - p.rec[R_SPAWN]) as f64 / (p.rec[R_LIFE] as f64 + EPS)) as f32).clamp(0.0, 1.0);
            let (mut sx, mut sy) = (p.size[0], p.size[1]);
            if !d.single_scale {
                let m = d.mid_point;
                if m <= tt || m == 0.0 {
                    if m == 1.0 {
                        (sx, sy) = (p.size[4], p.size[5]);
                    } else {
                        let f = (tt - m) / (1.0 - m);
                        sx = p.size[2] + f * (p.size[4] - p.size[2]);
                        sy = p.size[3] + f * (p.size[5] - p.size[3]);
                    }
                } else {
                    sx = p.size[0] + (tt / m) * (p.size[2] - p.size[0]);
                    sy = p.size[1] + (tt / m) * (p.size[3] - p.size[1]);
                }
            }

            p.rec[25] = s * sx;
            p.rec[26] = s * sy;
            velocity(p, t, dt, wind, &d, s, force_q, self.pos, rng);
            p.rec[20..23].copy_from_slice(&p.vel);
            let pos = [p.rec[12], p.rec[13], p.rec[14]];
            p.rec[8..11].copy_from_slice(&pos);
            for c in 0..3 {
                p.rec[12 + c] += p.vel[c] * dt;
            }

            if d.recycle {
                // Kept within CullDistances.w of the camera (horizontally).
                let dx = p.rec[12] - i.camera[0];
                let dz = p.rec[14] - i.camera[2];
                if d.cull[3] < (dx * dx + dz * dz).sqrt() {
                    let c = i.prev_camera;
                    p.rec[12] = (c[0] - p.rec[12]) + c[0] + (i.camera[0] - c[0]);
                    p.rec[14] = (c[2] - p.rec[14]) + c[2] + (i.camera[2] - c[2]);
                }
            }

            self.follow(k, delta, &frame);
            let p = &mut self.parts[k];
            p.rec[27] = o.colour[0] * self.tint[0];
            p.rec[28] = self.tint[1] * o.colour[1];
            p.rec[29] = self.tint[2] * o.colour[2];
            p.rec[30] = self.fade * self.tint[3] * o.colour[3];

            // ParticleCullDistances: fades in from x to y, out from z to w.
            let e = [p.rec[12] - i.camera[0], p.rec[13] - i.camera[1], p.rec[14] - i.camera[2]];
            let dist = fast_sqrt(e[0] * e[0] + e[1] * e[1] + e[2] * e[2]);
            let a = &mut p.rec[R_ALPHA];
            if dist < x || w < dist {
                *a *= 0.0;
            } else if dist < x || y < dist {
                if dist < z || w < dist {
                    *a *= 1.0;
                } else if w - z == 0.0 {
                    *a *= 0.0;
                } else {
                    *a *= 1.0 - (dist - z) / (w - z);
                }
            } else if y - x == 0.0 {
                *a *= 0.0;
            } else {
                *a *= (dist - x) / (y - x);
            }
        }
    }

    /// FUN_11568220: ParticlesFollowEmitter / ParticlesLocalSpace.
    fn follow(&mut self, k: usize, delta: V3, frame: &[[f32; 3]; 3]) {
        let (follow, local, at) = (self.desc.follow, self.desc.local_space, self.pos);
        let p = &mut self.parts[k];
        if follow {
            for c in 0..3 {
                p.rec[12 + c] += delta[c];
                p.rec[8 + c] = delta[c] + p.rec[12 + c];
            }
        } else if local {
            let r = mul(frame, [p.rec[12] - at[0], p.rec[13] - at[1], p.rec[14] - at[2]]);
            p.vel = mul(frame, p.vel);
            for c in 0..3 {
                p.rec[12 + c] = at[c] + r[c] + delta[c];
                p.rec[8 + c] = at[c] + r[c] + delta[c];
            }
        }
    }

    /// FUN_11541a30: the slots to draw (live and visible enough), in record order.
    pub fn drawn(&self, t: f32, out: &mut Vec<usize>) {
        for k in self.first..self.parts.len() {
            let r = &self.parts[k].rec;
            if !(t < r[R_SPAWN] || r[R_LIFE] + r[R_SPAWN] < t || r[R_ALPHA] <= ALPHA_CUTOFF) {
                out.push(k);
            }
        }
    }

    /// Visible (+0x80) and not culled (+0x91): drawn at all (fn 0x1151f260).
    pub fn shown(&self) -> bool {
        self.visible && !self.hidden
    }
}

/// FUN_115691f0: the velocity of a particle this frame.
#[allow(clippy::too_many_arguments)]
fn velocity(p: &mut Particle, t: f32, dt: f32, wind: V3, d: &Desc, s: f32, q: Quat, at: V3, rng: &mut Rng) {
    // FUN_11567ea0: RotationalForce turns about the emitter's axis, CentralForce pulls towards it.
    if p.rotational != 0.0 || p.central != 0.0 {
        let m = qmatrix(q);
        let axis = [m[0][1], m[1][1], m[2][1]];
        let mut r = [p.rec[12] - at[0], p.rec[13] - at[1], p.rec[14] - at[2]];
        let along = r[1] * axis[1] + r[0] * axis[0] + r[2] * axis[2];
        for c in 0..3 {
            r[c] -= along * axis[c];
        }

        let l2 = r[1] * r[1] + r[0] * r[0] + r[2] * r[2];
        if l2 > 0.0 {
            let inv = 1.0 / l2.sqrt();
            r = [r[0] * inv, r[1] * inv, r[2] * inv];
        }

        let k = p.rotational * TENTH;
        let c = p.central * TENTH;
        p.vel[0] += ((axis[1] * k * r[2] - axis[2] * k * r[1]) + -(c * r[0])) * dt;
        p.vel[1] += ((axis[2] * k * r[0] - axis[0] * k * r[2]) + -(c * r[1])) * dt;
        p.vel[2] += ((axis[0] * k * r[1] - axis[1] * k * r[0]) + -(c * r[2])) * dt;
    }

    // Turbulence: a random push, from TurbulenceStart to TurbulenceEnd over the life.
    let (ts, te) = d.turbulence;
    if ts + te > 0.0 && p.rec[R_LIFE] > 0.0 {
        let f = ((t - p.rec[R_SPAWN]) / p.rec[R_LIFE]).clamp(0.0, 1.0);
        let a = rng.unit();
        let b = rng.unit();
        let c = rng.unit();
        let e = rng.unit();
        let g = rng.unit();
        let h = rng.unit();
        let (dx, dy, dz) = (h - g, c - e, a - b);
        let amp = (te - ts) * f + ts;
        let inv = 1.0 / (dy * dy + dx * dx + dz * dz).sqrt();
        p.vel[0] += dx * inv * amp * s * dt;
        p.vel[1] += amp * inv * dy * s * dt;
        p.vel[2] += inv * dz * amp * s * dt;
    }

    // Deceleration: drag towards the wind, from DecelerationStartTime on.
    if d.deceleration {
        let mut f = ((t - (p.decel_start + p.rec[R_SPAWN])) as f64 / (p.rec[R_LIFE] as f64 + EPS)) as f32;
        if f <= 0.0 {
            f = 0.0;
        }

        f = f.min(1.0);
        if f > 0.0 {
            let inv_s = 1.0 / s;
            let rel = [p.vel[0] * inv_s - wind[0], p.vel[1] * inv_s - wind[1], p.vel[2] * inv_s - wind[2]];
            let m = fast_sqrt(len2(rel));
            let r = 1.0 / ((m as f64 + EPS) as f32);
            let k = (m * m * p.decel_amount * f * dt).min(m);
            for c in 0..3 {
                p.vel[c] -= r * rel[c] * k * s;
            }
        }
    }

    p.vel[1] += p.gravity * GRAVITY * dt;
    if d.speed_max > 0.0 {
        let m = fast_sqrt(p.vel[0] * p.vel[0] + p.vel[1] * p.vel[1] + p.vel[2] * p.vel[2]);
        if d.speed_max < m && m > 0.0 {
            let k = d.speed_max / m;
            p.vel = [k * p.vel[0], p.vel[1] * k, p.vel[2] * k];
        }
    }
}

/// An effect instance: one effect file's emitters at a place (the game's effect instance; fn 0x115639a0 is its update).
#[derive(Clone, Debug)]
pub struct Instance {
    pub owner: Owner,
    pub pos: V3,
    pub quat: Quat,
    pub emitters: Vec<Emitter>,
    pub camera_following: bool,
    last_t: f32,
    started: bool,
    pub done: bool,
}

impl Instance {
    pub fn new(owner: Owner, pos: V3, quat: Quat, emitters: Vec<Emitter>, camera_following: bool) -> Self {
        Self { owner, pos, quat, emitters, camera_following, last_t: 0.0, started: false, done: false }
    }

    /// One frame (fn 0x115639a0): the emitters go where the instance is, then run their state machines.
    pub fn update(&mut self, t: f32, wind: V3, camera: V3, prev_camera: V3, rng: &mut Rng) {
        if !self.started {
            self.started = true;
            for e in &mut self.emitters {
                e.init(&self.owner, rng);
            }
        }

        let u = rng.unit();
        let phase_step = ((u + u) - 1.0) * (t - self.last_t) * WIND_ANGLE_DELTA;
        let mut distance = DEFAULT_CAMERA_DISTANCE;
        for e in &mut self.emitters {
            e.pos = self.pos;
            e.quat = self.quat;
            if !self.camera_following {
                let d = [camera[0] - self.pos[0], camera[1] - self.pos[1], camera[2] - self.pos[2]];
                distance = fast_sqrt(len2(d));
            }
        }

        let inputs = Inputs { wind, camera, prev_camera, phase_step };
        let mut done = true;
        for e in &mut self.emitters {
            e.machine(t, true, distance, &inputs, &self.owner, rng);
            done &= e.done;
        }

        self.done = done && !self.emitters.is_empty();
        self.last_t = t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitter_def(kind: &str, props: &str) -> VfxEmitterDef {
        let xml = format!(r#"<vfx><emitters><emitter type="{kind}" id="e">{props}</emitter></emitters></vfx>"#);
        let doc = wce_formats::xml_doc::XmlDoc::parse(xml.into_bytes()).unwrap();
        wce_assets::vfx_def::parse_vfx(&doc).emitters.remove(0)
    }

    #[test]
    fn generator_and_square_root() {
        let mut r = Rng(1);
        assert_eq!(r.raw(), (1u32.wrapping_mul(0x343fd).wrapping_add(0x269ec3)) >> 16);
        assert_eq!(fast_sqrt(1.0), 1.0);
        assert_eq!(fast_sqrt(4.0), 2.0);
        assert!((fast_sqrt(2.0) - 1.5).abs() < 1e-6, "the bit trick is linear between powers of 4");
    }

    #[test]
    fn candle_flame_burns() {
        // action_vfx_candle_flame_loop's emitter_fire_loop: one particle a second, 1.1 s each, on the spot.
        let d = Desc::from_def(&emitter_def("continuous_mesh", r#"
            <property name="CullDistances" x="0.00" y="0.00" z="25.00" w="40.00"/>
            <property name="ParticleCullDistances" x="0.00" y="0.00" z="500.00" w="550.00"/>
            <ranged_property name="SpawnSpeed" min="0.00" max="0.00"/>
            <ranged_property name="SpawnSpread" min="0.00" max="0.00"/>
            <property name="ParticleScaleStart" x="0.10" y="0.07"/>
            <property name="ParticleScaleMid" x="0.10" y="0.08"/>
            <property name="ParticleScaleEnd" x="0.10" y="0.07"/>
            <ranged_property name="LifeTime" min="50.00" max="50.00"/>
            <property name="InfiniteLife" value="true"/>
            <ranged_property name="ParticlesPerSecond" min="1.00" max="1.00"/>
            <ranged_property name="ParticleLifeTime" min="1.10" max="1.10"/>"#)).unwrap();
        let owner = Owner { scale: 1.0, colour: [1.0; 4], reference: None };
        let mut inst = Instance::new(owner, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], vec![Emitter::new(d, 0, 0, [1.0; 4])], false);
        let mut rng = Rng(12345);
        let cam = [0.0, 1.0, 5.0];
        let mut t = 0.0;
        let mut seen = Vec::new();
        for _ in 0..600 {
            t += 1.0 / 60.0;
            inst.update(t, [0.0; 3], cam, cam, &mut rng);
            let mut drawn = Vec::new();
            inst.emitters[0].drawn(t, &mut drawn);
            seen.push(drawn.len());
        }

        // Capacity round(1.1 * 1) + 1 = 2; one or two alive at a time once running, never more.
        assert_eq!(inst.emitters[0].parts.len(), 2);
        assert!(seen[120..].iter().all(|&n| (1..=2).contains(&n)), "{:?}", &seen[120..180]);
        let e = &inst.emitters[0];
        let r = &e.parts[e.first].rec;
        assert_eq!(&r[12..15], &[0.0, 0.0, 0.0], "no speed: the flame stays on the spot");
        assert!((r[25] - 0.1).abs() < 1e-6 && r[26] > 0.069 && r[26] < 0.081, "size {} x {}", r[25], r[26]);
        assert_eq!(r[R_ALPHA], 1.0);
    }

    #[test]
    fn bursts_spawn_their_counts() {
        let d = Desc::from_def(&emitter_def("burst_mesh", r#"
            <property name="CullDistances" x="0.00" y="0.00" z="100.00" w="200.00"/>
            <property name="ParticleCullDistances" x="0.00" y="0.00" z="100.00" w="200.00"/>
            <ranged_property name="Bursts" min="3.00" max="3.00"/>
            <ranged_property name="BurstInterval" min="1.00" max="1.00"/>
            <ranged_property name="BurstIntervalLength" min="0.10" max="0.10"/>
            <ranged_property name="ParticlesPerBurst" min="4.00" max="4.00"/>
            <ranged_property name="ParticleLifeTime" min="0.50" max="0.50"/>
            <ranged_property name="SpawnSpeed" min="2.00" max="2.00"/>"#)).unwrap();
        let owner = Owner { scale: 1.0, colour: [1.0; 4], reference: None };
        let mut inst = Instance::new(owner, [0.0; 3], [0.0, 0.0, 0.0, 1.0], vec![Emitter::new(d, 0, 0, [1.0; 4])], false);
        let mut rng = Rng(7);
        let cam = [0.0, 0.0, 10.0];
        let mut t = 0.0;
        let mut max_live = 0;
        for _ in 0..600 {
            t += 1.0 / 60.0;
            inst.update(t, [0.0; 3], cam, cam, &mut rng);
            let e = &inst.emitters[0];
            max_live = max_live.max(e.parts.len() - e.first);
        }

        let e = &inst.emitters[0];
        assert_eq!(e.bursts_left, 0, "all three bursts fired");
        assert_eq!(max_live, 4, "a burst's particles at once (capacity {})", e.parts.len());
        assert!(inst.done, "the emitter ends once its last particles die");
    }
}
