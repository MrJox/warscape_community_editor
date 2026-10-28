//! Differential dump of the editor core's numbers and text, in the format of the C++ harness: the camera controllers over random input sequences
//! and the command tokenizer on fuzzed lines. The tokenizer's lines are compared with the C++ dump (the random numbers are drawn as before); the
//! cameras' (Total War, top-down, walk, fly: `o`, `t`, `w`, `f`), which now switch between controllers the C++ editor does not have, between
//! runs of this dump.

use wce_editor_core::camera_controller::{CameraController, CameraInput, CameraOptions, FlyCamera, NoWorld, TopDownCamera, TotalWarCamera, WalkCamera};
use wce_editor_core::command_registry::{CommandHost, CommandRegistry};
use wce_foundation::math::Float3;
use wce_scene::camera::Camera;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() % 100000) as f32 / 100000.0
    }

    fn input(&mut self) -> CameraInput {
        let f = self.next();
        let (dx, dy) = (self.f(-40.0, 40.0), self.f(-40.0, 40.0));
        let wheel = if f & 1 != 0 { ((self.next() % 7) as i32 - 3) as f32 } else { 0.0 };
        let bit = |b: u32| f & b != 0;
        CameraInput {
            dx, dy, wheel, rotate: bit(2), pan: bit(4), forward: bit(8), back: bit(16), left: bit(32), right: bit(64), turn_right: bit(128), turn_left: bit(256),
            fast: bit(512), cursor: Some([0.25, -0.5]), aspect: 16.0 / 9.0, view_h: 720.0, dt: self.f(0.0, 0.1), ..Default::default()
        }
    }
}

fn cam(tag: &str, c: &Camera) {
    println!("{tag} {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}", c.eye.x.to_bits(), c.eye.y.to_bits(), c.eye.z.to_bits(), c.target.x.to_bits(), c.target.y.to_bits(), c.target.z.to_bits());
}

struct NoCommands(CommandRegistry<NoCommands>);

impl CommandHost for NoCommands {
    fn commands(&self) -> &CommandRegistry<Self> {
        &self.0
    }
}

fn main() {
    let mut r = Rng(88172645463325252);
    for _ in 0..200 {
        let (mut o, mut t, mut w, mut f) = (TotalWarCamera::default(), TopDownCamera::default(), WalkCamera::default(), FlyCamera::default());
        let exact = CameraOptions { smoothing: 0.0, respect_ground: false, ..Default::default() };
        let opts = CameraOptions { smoothing: 0.3, respect_ground: false, ..Default::default() };
        let (y, p, d, tx, ty, tz) = (r.f(-3.0, 3.0), r.f(-1.0, 1.4), r.f(5.0, 900.0), r.f(-500.0, 500.0), r.f(-10.0, 60.0), r.f(-500.0, 500.0));
        o.set_orbit(y, p, d, Float3::new(tx, ty, tz));
        for s in 0..300 {
            let input = r.input();
            o.update(&input, &opts, &mut NoWorld);
            t.update(&input, &opts, &mut NoWorld);
            w.update(&input, &opts, &mut NoWorld);
            f.update(&input, &exact, &mut NoWorld);
            if s % 37 == 0 {
                t.set_camera(&o.handoff());
                w.set_camera(&o.handoff());
                f.set_camera(&o.handoff());
            }

            if s % 53 == 0 {
                o.set_camera(&t.handoff());
            }

            if s % 71 == 0 {
                let (x, y, z, radius) = (r.f(-300.0, 300.0), r.f(0.0, 50.0), r.f(-300.0, 300.0), r.f(0.5, 400.0));
                o.focus(Float3::new(x, y, z), radius);
                t.focus(Float3::new(x, y, z), radius);
                f.focus(Float3::new(x, y, z), radius);
            }

            if s % 10 == 0 {
                cam("o", &o.camera());
                cam("t", &t.camera());
                cam("w", &w.camera());
                cam("f", &f.camera());
                println!("p {:08x} {:08x} {:08x} {:08x} {:08x} {:08x}", o.yaw.to_bits(), o.pitch.to_bits(), o.focus_dist.to_bits(), t.heading.to_bits(), t.height.to_bits(), w.speed.to_bits());
            }
        }
    }

    let alphabet = b"ab \t\"#\r-x";
    for _ in 0..20000 {
        let n = r.next() % 24;
        let line = (0..n).map(|_| alphabet[(r.next() % alphabet.len() as u32) as usize] as char).collect::<String>();
        let t = CommandRegistry::<NoCommands>::tokenize(&line);
        print!("t {}", t.len());
        for s in &t {
            print!(" [{s}]");
        }

        println!();
    }
}
