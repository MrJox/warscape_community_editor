//! The simulation clock.
//!
//! Live: wall time since [`Clock::restart`]. Fixed-step: every [`Clock::tick`] advances time by exactly `step` seconds, whatever the
//! machine does, so wind, clouds, particles and tone-map adaptation (all driven by the clock's time and dt) give the same frame N
//! every run. Screenshots, the render regression gate and the cinematic recorder use the fixed-step clock.

use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Clock {
    step: f64,
    time: f64,
    dt: f32,
    frame: u64,
    start: Instant,
    last: Instant,
}

impl Clock {

    /// A clock that follows the wall time.
    pub fn live() -> Self {
        Self::new(0.0)
    }

    /// A clock that advances `step_seconds` per tick (1/60 s for a step that is not positive).
    pub fn fixed(step_seconds: f64) -> Self {
        Self::new(if step_seconds > 0.0 { step_seconds } else { 1.0 / 60.0 })
    }

    fn new(step: f64) -> Self {
        let now = Instant::now();
        Self { step, time: 0.0, dt: 0.0, frame: 0, start: now, last: now }
    }

    pub fn fixed_step(&self) -> bool {
        self.step > 0.0
    }

    pub fn step(&self) -> f64 {
        self.step
    }

    /// Starts counting from now and from zero (time goes backwards for the clock's users, who reset their simulations).
    pub fn restart(&mut self) {
        *self = Self::new(self.step);
    }

    /// Call once per rendered frame, before the frame uses [`Self::time`] and [`Self::dt`]. The first tick gives time 0.
    pub fn tick(&mut self) {
        if self.fixed_step() {
            self.time = self.frame as f64 * self.step;
            self.dt = if self.frame == 0 { 0.0 } else { self.step as f32 };
        } else {
            let now = Instant::now();
            self.time = now.duration_since(self.start).as_secs_f64();
            self.dt = if self.frame == 0 { 0.0 } else { now.duration_since(self.last).as_secs_f64() as f32 };
            self.last = now;
        }

        self.frame += 1;
    }

    /// Seconds since the restart.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Seconds since the previous tick (0 on the first).
    pub fn dt(&self) -> f32 {
        self.dt
    }

    /// Ticks so far.
    pub fn frames(&self) -> u64 {
        self.frame
    }
}
