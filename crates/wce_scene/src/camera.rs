//! The view camera.

use wce_foundation::math::{Float3, Float4x4, cross, dot, look_at_lh, normalize, ortho_lh, perspective_lh};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Float3,
    pub target: Float3,

    /// The view's up: +y, except for a camera looking straight down (the top-down view's, where it is the screen's up on the ground).
    pub up: Float3,

    /// The game's battle camera (memory dump): vertical FOV 1.13446 rad (65 degrees), near 1, far 13000.
    pub fov_y: f32,
    pub znear: f32,
    pub zfar: f32,

    /// An orthographic camera: the height of the view in metres (TEd's "Ortho Top", `orthographic_height`); `None` is a perspective one.
    pub ortho_height: Option<f32>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: Float3::new(0.0, 30.0, -80.0), target: Float3::new(0.0, 5.0, 0.0), up: Float3::new(0.0, 1.0, 0.0), fov_y: 1.13446, znear: 1.0, zfar: 13000.0,
            ortho_height: None,
        }
    }
}

impl Camera {
    pub fn is_ortho(&self) -> bool {
        self.ortho_height.is_some()
    }

    /// The view's axes in the world: right, up, forward.
    pub fn axes(&self) -> (Float3, Float3, Float3) {
        let fwd = normalize(self.target - self.eye);
        let right = normalize(cross(self.up, fwd));
        (right, cross(fwd, right), fwd)
    }

    /// The view matrix, from `eye - origin` (the renderer's origin-relative views).
    pub fn view_from(&self, origin: Float3) -> Float4x4 {
        look_at_lh(self.eye - origin, self.target - origin, self.up)
    }

    pub fn view(&self) -> Float4x4 {
        self.view_from(Float3::default())
    }

    pub fn projection(&self, aspect: f32) -> Float4x4 {
        match self.ortho_height {
            Some(h) => ortho_lh(h * aspect, h, self.znear, self.zfar),
            None => perspective_lh(self.fov_y, aspect, self.znear, self.zfar),
        }
    }

    pub fn view_projection(&self, aspect: f32) -> Float4x4 {
        self.projection(aspect) * self.view()
    }

    /// Half the view's width and height at a view depth.
    pub fn half_extents(&self, depth: f32, aspect: f32) -> (f32, f32) {
        let h = match self.ortho_height {
            Some(h) => h * 0.5,
            None => (self.fov_y * 0.5).tan() * depth,
        };

        (h * aspect, h)
    }

    /// The view depth of a z-buffer value (0..1).
    pub fn linear_depth(&self, d: f32) -> f32 {
        let (zn, zf) = (self.znear, self.zfar);
        if self.is_ortho() { zn + d * (zf - zn) } else { zn * zf / (zf - d * (zf - zn)) }
    }

    /// The ray (origin, direction normalized) through a point of the view (x right, y up, -1..1 across).
    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (Float3, Float3) {
        let (right, up, fwd) = self.axes();
        if let Some(h) = self.ortho_height {
            return (self.eye + right * (nx * h * 0.5 * aspect) + up * (ny * h * 0.5), fwd);
        }

        let th = (self.fov_y * 0.5).tan();
        (self.eye, normalize(fwd + right * (nx * th * aspect) + up * (ny * th)))
    }

    /// Where a world point is in the view (x right, y up, -1..1 across); `None` behind the near plane.
    pub fn project(&self, p: Float3, aspect: f32) -> Option<(f32, f32)> {
        let (right, up, fwd) = self.axes();
        let rel = p - self.eye;
        let z = dot(rel, fwd);
        if z <= self.znear {
            return None;
        }

        let (hw, hh) = self.half_extents(z, aspect);
        Some((dot(rel, right) / hw, dot(rel, up) / hh))
    }
}
