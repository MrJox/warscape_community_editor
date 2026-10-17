//! Battle wind as the engine simulates it (empire.retail.dll 1.6.0, verified against three memory dumps):
//!
//! - direction, fn 0x17958b0: (cos, sin) of WEATHER wind_dir_degrees; wind_dir_random (fn 0x175d940) replaces the degrees once per
//!   battle with rand16 * 360 / 65535;
//! - gusts, fn 0x18dfd30 (WEATHER node +0x7a0..+0x7cc): strength = wind_size_min, plus a gust shaped exp(-0.5 * ((x - 1) * wind_gust_curve)^2),
//!   x = (t - start) / duration in 0..1;
//! - shaders, fn 0x18ee0c0 (weather_shared, wind_VS, wind_PS), fn 0x1923990, fn 0x191e430 (cloud_plane_VS);
//! - VFX, fn 0x18f8b10 (wind_velocity = 20 * wind), fn 0x18e51e0 (the wind_variance_dir random walk).

use std::f32::consts::{E, TAU};

use wce_assets::weather_vfx::WeatherDefinition;
use wce_foundation::math::{Float2, Float3, dot};

/// The engine's |v| for wind_variance_dir: an integer-halving square root on the float bits (fn 0x18e51e0); e.g. 5.954 for |v| = 5.624
/// (rain dump).
fn approx_sqrt(x: f32) -> f32 {
    let i = x.to_bits() as i32;
    f32::from_bits(((i.wrapping_sub(0x3F80_0000) >> 1).wrapping_add(0x3F80_0000)) as u32)
}

/// The engine's LCG: s * 0x343fd + 0x269ec3, (s >> 16) / 65535 (DAT 0x1afdce4).
fn rand01(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(0x343FD).wrapping_add(0x26_9EC3);
    (*s >> 16) as f32 * 1.525_902_2e-5
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindSim {
    /// wind_VS.wind_direction, wind_PS.wind_direction_PS.
    pub direction: Float2,

    /// WEATHER node +0x7a8.
    pub strength: f32,

    /// wind_VS.gust_start_time (= start + duration of the last gust).
    pub gust_end: f32,

    /// weather_shared.g_wind (running average over SMOOTHING_PERIOD, length <= 1).
    pub smoothed: Float2,

    /// weather_shared.g_wind_offset (the integral of g_wind).
    pub offset: Float2,

    /// cb_vfx_game_simulation_data.
    pub vfx_velocity: Float3,
    pub vfx_variance: Float3,

    // Gust state (WEATHER node fields).
    pub gust_size: f32,
    pub gust_duration: f32,
    pub gust_start: f32,
    pub cooldown: f32,

    gust_rng: u32,
    variance_rng: u32,
    variance_angle: f32,
    accum: Float2,
    previous: Float2,
    accum_time: f32,
}

impl Default for WindSim {
    fn default() -> Self {
        Self {
            direction: Float2 { x: 1.0, y: 0.0 },
            strength: 0.0,
            gust_end: 0.0,
            smoothed: Float2::default(),
            offset: Float2::default(),
            vfx_velocity: Float3::new(2.0, 0.0, 0.0),
            vfx_variance: Float3::new(2.0, 0.0, 0.0),
            gust_size: 0.0,
            gust_duration: 0.0,
            gust_start: 0.0,
            cooldown: 0.0,
            gust_rng: 0,
            variance_rng: 0,
            variance_angle: 0.0,
            accum: Float2::default(),
            previous: Float2::default(),
            accum_time: 0.0,
        }
    }
}

impl WindSim {

    // Engine variables read from the dumps (the same in all three).

    /// dll var +0x2a787c8.
    pub const VFX_WIND_SCALE: f32 = 20.0;

    /// dll var +0x2a776a8 (radians per second, random sign).
    pub const VARIANCE_RATE: f32 = 5.0;

    /// WEATHER node +0x80c.
    pub const SMOOTHING_PERIOD: f32 = 60.0;

    /// wind_VS.wind_vegetation_hstrength.
    pub const VEGETATION_H_STRENGTH: f32 = 0.91;

    /// wind_VS.wind_vegetation_vstrength.
    pub const VEGETATION_V_STRENGTH: f32 = 0.08;

    /// The seed the editor resets with.
    pub const DEFAULT_SEED: u32 = 0x2545_F491;

    /// Starts again from the engine's initial state.
    pub fn reset(&mut self, seed: u32) {
        *self = Self { gust_rng: seed, variance_rng: seed ^ 0x9E37_79B9, ..Self::default() };
    }

    /// A random battle direction for wind_dir_random (fn 0x175d940).
    pub fn random_direction(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(0x343FD).wrapping_add(0x26_9EC3);
        (*seed >> 16) as f32 * 0.005_493_248
    }

    /// cloud_plane_VS.g_wind_alignment = atan2(direction.y, direction.x).
    pub fn alignment(&self) -> f32 {
        self.direction.y.atan2(self.direction.x)
    }

    /// Advances the wind. `t` is the battle clock in seconds, `dt` the frame time. Without a weather definition the VFX wind is (0.1, 0, 0) * 20.
    pub fn update(&mut self, def: Option<&WeatherDefinition>, direction_degrees: f32, t: f32, dt: f32) {
        let a = direction_degrees * 0.017_453_294;
        self.direction = Float2 { x: a.cos(), y: a.sin() };

        // Gusts (fn 0x18dfd30).
        if let Some(def) = def {
            self.strength = def.wind_size_min;
            let end = self.gust_start + self.gust_duration;
            if t > end && self.cooldown <= 0.0 {
                if rand01(&mut self.gust_rng) <= def.wind_gust_probability {
                    let (sv, dv) = (def.wind_gust_size_variation, def.wind_gust_duration_variation);
                    self.gust_size = (rand01(&mut self.gust_rng) * sv + (1.0 - sv)) * (def.wind_size_max - def.wind_size_min);
                    self.gust_duration = (rand01(&mut self.gust_rng) * dv + (1.0 - dv)) * def.wind_gust_duration;
                    self.gust_start = t;
                }

                self.cooldown = def.wind_gust_interval;
            } else if self.gust_start < t && t < end {
                // The engine doubles x when its "gust centred" flag (var +0x2a74990) is set; it is 0 in every dump.
                let x = ((t - self.gust_start) / self.gust_duration - 1.0) * def.wind_gust_curve;
                self.strength = def.wind_size_min + E.powf(x * x * -0.5) * self.gust_size;
            } else if self.cooldown > 0.0 {
                self.cooldown -= dt;
            }

            self.gust_end = self.gust_start + self.gust_duration;
        } else {
            self.strength = 0.0;
        }

        let wind = Float2 { x: self.direction.x * self.strength, y: self.direction.y * self.strength };

        // weather_shared (fn 0x18ee0c0): the running average of the wind over SMOOTHING_PERIOD.
        self.accum.x += wind.x * dt;
        self.accum.y += wind.y * dt;
        self.accum_time += dt;
        let mut w = wind;
        if self.accum_time >= Self::SMOOTHING_PERIOD {
            self.previous = Float2 { x: self.accum.x / self.accum_time, y: self.accum.y / self.accum_time };
            w = self.previous;
            self.accum = Float2::default();
            self.accum_time = 0.0;
        } else if self.accum_time > 0.0 {
            let f = self.accum_time / Self::SMOOTHING_PERIOD;
            w = Float2 {
                x: self.accum.x / self.accum_time * f + (1.0 - f) * self.previous.x,
                y: self.accum.y / self.accum_time * f + (1.0 - f) * self.previous.y,
            };
        }

        let len = (w.x * w.x + w.y * w.y).sqrt();
        if len > 0.0 {
            let k = len.min(1.0) / len;
            w = Float2 { x: w.x * k, y: w.y * k };
        }

        self.smoothed = w;
        self.offset.x += w.x * dt;
        self.offset.y += w.y * dt;

        // VFX (fn 0x18f8b10, fn 0x18e51e0).
        self.vfx_velocity = if def.is_some() { Float3::new(wind.x, 0.0, wind.y) } else { Float3::new(0.1, 0.0, 0.0) } * Self::VFX_WIND_SCALE;
        let r = rand01(&mut self.variance_rng);
        self.variance_angle = (self.variance_angle + (r + r - 1.0) * dt * Self::VARIANCE_RATE) % TAU;
        let m = approx_sqrt(dot(self.vfx_velocity, self.vfx_velocity));
        self.vfx_variance = Float3::new(self.variance_angle.cos() * m, 0.0, self.variance_angle.sin() * m);
    }
}
