//! Viewport camera controllers. They turn mouse and key state into a `Camera`; the viewport feeds them and passes the result to the renderer.
//! Both keep the game's battle camera lens (vertical FOV 65 degrees, near 1, far 13000, from the memory dump) unless the lens is edited.

use wce_foundation::math::{Float3, length, normalize};
use wce_scene::camera::Camera;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraInput {
    /// Mouse movement in pixels this frame.
    pub dx: f32,
    pub dy: f32,

    /// Wheel notches.
    pub wheel: f32,

    /// Left or right button held over the viewport.
    pub rotate: bool,

    /// Middle button.
    pub pan: bool,
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,

    /// Shift.
    pub fast: bool,

    /// Seconds.
    pub dt: f32,
}

/// The lens of a controller's camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    pub fov_y: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Default for Lens {

    /// The game's battle camera.
    fn default() -> Self {
        Self { fov_y: 1.13446, znear: 1.0, zfar: 13000.0 }
    }
}

pub trait CameraController {
    fn name(&self) -> &'static str;
    fn update(&mut self, input: &CameraInput);
    fn camera(&self) -> Camera;

    /// Continue from this view (switching controllers).
    fn set_camera(&mut self, cam: &Camera);

    /// Frame a sphere.
    fn focus(&mut self, center: Float3, radius: f32);
    fn lens(&self) -> &Lens;
    fn lens_mut(&mut self) -> &mut Lens;
}

/// Turns around a target: rotate with the mouse, pan with the middle button, wheel zooms, WASD moves the target along the ground (Q / E
/// down / up, faster with Shift, at a speed that follows the distance). The formulas of the weather tool's viewport, so existing `--cam`
/// values mean the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub lens: Lens,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Float3,

    /// Stay above the ground plane when looking up.
    pub min_eye_height: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self { lens: Lens::default(), yaw: 0.6, pitch: 0.35, dist: 120.0, target: Float3::new(0.0, 5.0, 0.0), min_eye_height: 2.0 }
    }
}

impl OrbitCamera {
    pub fn set_orbit(&mut self, yaw: f32, pitch: f32, dist: f32, target: Float3) {
        self.yaw = yaw;
        self.pitch = pitch;
        self.dist = dist;
        self.target = target;
    }
}

impl CameraController for OrbitCamera {
    fn name(&self) -> &'static str {
        "Orbit"
    }

    fn update(&mut self, input: &CameraInput) {
        if input.rotate {
            self.yaw += input.dx * 0.006;
            self.pitch = (self.pitch + input.dy * 0.006).clamp(-1.2, 1.5);
        }

        if input.pan {
            let right = Float3::new(self.yaw.cos(), 0.0, self.yaw.sin());
            let fwd = Float3::new(-self.yaw.sin(), 0.0, self.yaw.cos());
            let k = self.dist * 0.0015;
            self.target = self.target - right * (input.dx * k) + fwd * (input.dy * k);
        }

        if input.wheel != 0.0 {
            self.dist = (self.dist * 0.9f32.powf(input.wheel)).clamp(2.0, 6000.0);
        }

        // Forward is where the camera looks, flattened: the eye sits at target + (sin yaw, ., -cos yaw) * dist.
        let fwd = Float3::new(-self.yaw.sin(), 0.0, self.yaw.cos());
        let right = Float3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let mut m = Float3::new(0.0, 0.0, 0.0);
        for (on, d) in [(input.forward, fwd), (input.back, -fwd), (input.right, right), (input.left, -right)] {
            if on {
                m = m + d;
            }
        }

        if input.up {
            m.y += 1.0;
        }

        if input.down {
            m.y -= 1.0;
        }

        if m != Float3::default() {
            let speed = self.dist.max(10.0) * if input.fast { 4.0 } else { 1.0 };
            self.target = self.target + normalize(m) * (speed * input.dt);
        }
    }

    fn camera(&self) -> Camera {
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        let mut eye = self.target + Float3::new(self.yaw.sin() * cp, sp, -self.yaw.cos() * cp) * self.dist;
        eye.y = eye.y.max(self.min_eye_height);
        Camera { eye, target: self.target, fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar }
    }

    fn set_camera(&mut self, cam: &Camera) {
        let d = cam.eye - cam.target;
        self.dist = length(d).max(1.0);
        self.target = cam.target;
        self.pitch = (d.y / self.dist).clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(-d.z);
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        self.target = center;
        self.dist = (radius / (self.lens.fov_y * 0.5).tan() * 1.3).clamp(5.0, 6000.0);
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}

/// Free flight: right button looks around, WASD moves, Q / E down and up, the wheel changes the speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyCamera {
    pub lens: Lens,
    pub eye: Float3,
    pub yaw: f32,
    pub pitch: f32,

    /// Metres per second.
    pub speed: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self { lens: Lens::default(), eye: Float3::new(0.0, 30.0, -80.0), yaw: 0.0, pitch: -0.2, speed: 40.0 }
    }
}

impl CameraController for FlyCamera {
    fn name(&self) -> &'static str {
        "Fly"
    }

    fn update(&mut self, input: &CameraInput) {
        if input.rotate {
            self.yaw += input.dx * 0.004;
            self.pitch = (self.pitch - input.dy * 0.004).clamp(-1.5, 1.5);
        }

        if input.wheel != 0.0 {
            self.speed = (self.speed * 1.2f32.powf(input.wheel)).clamp(1.0, 2000.0);
        }

        let cp = self.pitch.cos();
        let fwd = Float3::new(self.yaw.sin() * cp, self.pitch.sin(), self.yaw.cos() * cp);
        let right = Float3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let up = Float3::new(0.0, 1.0, 0.0);
        let mut m = Float3::new(0.0, 0.0, 0.0);
        if input.forward {
            m = m + fwd;
        }

        if input.back {
            m = m - fwd;
        }

        if input.right {
            m = m + right;
        }

        if input.left {
            m = m - right;
        }

        if input.up {
            m = m + up;
        }

        if input.down {
            m = m - up;
        }

        self.eye = self.eye + m * (self.speed * if input.fast { 4.0 } else { 1.0 } * input.dt);
    }

    fn camera(&self) -> Camera {
        let cp = self.pitch.cos();
        let target = self.eye + Float3::new(self.yaw.sin() * cp, self.pitch.sin(), self.yaw.cos() * cp) * 10.0;
        Camera { eye: self.eye, target, fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar }
    }

    fn set_camera(&mut self, cam: &Camera) {
        self.eye = cam.eye;
        let d = normalize(cam.target - cam.eye);
        self.pitch = d.y.clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(d.z);
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        let d = normalize(Float3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos()));
        self.eye = center - d * (radius / (self.lens.fov_y * 0.5).tan() * 1.3).clamp(5.0, 6000.0);
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}
