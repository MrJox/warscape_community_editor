//! The view camera.

use wce_foundation::math::Float3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Float3,
    pub target: Float3,

    /// The game's battle camera (memory dump): vertical FOV 1.13446 rad (65 degrees), near 1, far 13000.
    pub fov_y: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self { eye: Float3::new(0.0, 30.0, -80.0), target: Float3::new(0.0, 5.0, 0.0), fov_y: 1.13446, znear: 1.0, zfar: 13000.0 }
    }
}
