//! Viewport camera controllers. They turn mouse and key state into a `Camera`; the viewport feeds them and passes the result to the renderer.
//! All keep the game's battle camera lens (vertical FOV 65 degrees, near 1, far 13000, from the memory dump) unless the lens is edited.
//!
//! Five modes: the Total War camera (the game's battle camera, the default), the top-down view (TEd's "Ortho Top": orthographic, straight
//! down), the walk camera (TEd's "Walk": eye height above the ground), the fly camera (TEd's "Look": free flight) and the orbit camera (TEd's
//! "Orbit": around a target). The ground and the obstacles come from a [`CameraWorld`]; the options are the settings' ([`CameraOptions`]).

use wce_foundation::math::{Float3, length, normalize};
use wce_scene::camera::Camera;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraInput {
    /// Mouse movement in pixels this frame.
    pub dx: f32,
    pub dy: f32,

    /// Wheel notches.
    pub wheel: f32,

    /// A camera drag with the left button (off the gizmo and the tool) or the right one: turns the camera, pans the top-down view.
    pub rotate: bool,

    /// A drag with the middle button: pans the orbit camera; the others take it as `rotate`.
    pub pan: bool,
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,

    /// Q / E: turn left and right (the fly camera: down and up).
    pub turn_left: bool,
    pub turn_right: bool,

    /// Shift.
    pub fast: bool,

    /// Edge scrolling: -1..1 to the right (x) and forward (y), 0 away from the edges.
    pub edge: [f32; 2],

    /// The mouse in the view (x right, y up, -1..1 across), while it is over it.
    pub cursor: Option<[f32; 2]>,

    /// The view's width over its height, and its height in pixels.
    pub aspect: f32,
    pub view_h: f32,

    /// Seconds.
    pub dt: f32,
}

/// The settings' camera options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraOptions {
    /// 0 (off) .. 1: how long the view takes to catch up with the controls (up to 0.3 s for most of the way).
    pub smoothing: f32,

    /// The game's battle camera limits (the `battle_cameras` table's land row: 2 to 90 m above the ground).
    pub game_limits: bool,

    /// The eye stays `ground_clearance` above the terrain.
    pub respect_ground: bool,
    pub ground_clearance: f32,

    /// The eye stays out of the obstacles' bounding boxes.
    pub respect_collisions: bool,
}

impl Default for CameraOptions {
    fn default() -> Self {
        Self { smoothing: 0.5, game_limits: false, respect_ground: true, ground_clearance: 1.0, respect_collisions: false }
    }
}

/// The game's battle camera limits above the ground (`battle_cameras`, land: min_height 2, max_height_large 90).
pub const GAME_MIN_HEIGHT: f32 = 2.0;
pub const GAME_MAX_HEIGHT: f32 = 90.0;

/// The eye's radius against the obstacles: the lens's near plane, so that nothing in front of it is cut.
pub const EYE_RADIUS: f32 = 1.0;

/// What the cameras stand on and bump into.
pub trait CameraWorld {
    /// The terrain's height at a point; `None` off every terrain.
    fn ground(&mut self, x: f32, z: f32) -> Option<f32>;

    /// Where a ray (direction normalized) meets the terrain first.
    fn ray_ground(&mut self, origin: Float3, dir: Float3) -> Option<Float3>;

    /// A sphere moved out of the obstacles (their bounding boxes), the least it takes.
    fn push_out(&mut self, centre: Float3, radius: f32) -> Float3;
}

/// No ground, no obstacles.
pub struct NoWorld;

impl CameraWorld for NoWorld {
    fn ground(&mut self, _x: f32, _z: f32) -> Option<f32> {
        None
    }

    fn ray_ground(&mut self, _origin: Float3, _dir: Float3) -> Option<Float3> {
        None
    }

    fn push_out(&mut self, centre: Float3, _radius: f32) -> Float3 {
        centre
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    TotalWar,
    TopDown,
    Walk,
    Fly,
    Orbit,
}

impl CameraMode {
    pub const ALL: [Self; 5] = [Self::TotalWar, Self::TopDown, Self::Walk, Self::Fly, Self::Orbit];

    pub fn label(self) -> &'static str {
        match self {
            Self::TotalWar => "Total War camera",
            Self::TopDown => "Top-down camera",
            Self::Walk => "Walk camera",
            Self::Fly => "Fly camera",
            Self::Orbit => "Orbit camera",
        }
    }

    /// The settings' and the `camera` command's name.
    pub fn key(self) -> &'static str {
        match self {
            Self::TotalWar => "total_war",
            Self::TopDown => "top_down",
            Self::Walk => "walk",
            Self::Fly => "fly",
            Self::Orbit => "orbit",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }
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
    fn mode(&self) -> CameraMode;
    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld);
    fn camera(&self) -> Camera;

    /// The view another controller continues from: the camera, except for the top-down view (a perspective view over its centre).
    fn handoff(&self) -> Camera {
        self.camera()
    }

    /// Continue from this view (switching controllers, saved views). At once, without smoothing.
    fn set_camera(&mut self, cam: &Camera);

    /// The view of the orbit convention (`cam yaw pitch dist`, the weather tool's `--cam`): the eye at
    /// target + (sin yaw cos pitch, sin pitch, -cos yaw cos pitch) * dist, looking at the target.
    fn set_orbit(&mut self, yaw: f32, pitch: f32, dist: f32, target: Float3) {
        let (cp, sp) = (pitch.cos(), pitch.sin());
        let eye = target + Float3::new(yaw.sin() * cp, sp, -yaw.cos() * cp) * dist;
        self.set_camera(&Camera { eye, target, up: Float3::new(0.0, 1.0, 0.0), ortho_height: None, ..self.camera() });
    }

    /// The point the view is about: what the camera looks at on the ground (the walker's own place).
    fn pivot(&self) -> Float3 {
        self.camera().target
    }

    /// Moves the view so that its pivot is at `p`, its angles and distance kept.
    fn move_pivot(&mut self, p: Float3) {
        let c = self.handoff();
        let d = p - self.pivot();
        self.set_camera(&Camera { eye: c.eye + d, target: c.target + d, ..c });
    }

    /// Frame a sphere (smoothly when smoothing is on).
    fn focus(&mut self, center: Float3, radius: f32);
    fn lens(&self) -> &Lens;
    fn lens_mut(&mut self) -> &mut Lens;
}

/// How far the shown view moves towards the controls this frame.
fn smooth_step(opts: &CameraOptions, dt: f32) -> f32 {
    if opts.smoothing <= 0.0 || dt <= 0.0 { 1.0 } else { 1.0 - (-dt / (opts.smoothing.min(1.0) * 0.3)).exp() }
}

/// `a` moved `t` of the way to `b`; `b` itself at 1 (no smoothing: exactly the controls' view).
fn lerp3(a: Float3, b: Float3, t: f32) -> Float3 {
    if t >= 1.0 { b } else { a + (b - a) * t }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    if t >= 1.0 { b } else { a + (b - a) * t }
}

/// An eye kept out of the obstacles and above the ground, as the options say.
fn constrain(p: Float3, opts: &CameraOptions, world: &mut dyn CameraWorld) -> Float3 {
    let mut p = p;
    if opts.respect_collisions {
        p = world.push_out(p, EYE_RADIUS);
    }

    if opts.respect_ground
        && let Some(g) = world.ground(p.x, p.z)
    {
        p.y = p.y.max(g + opts.ground_clearance);
    }

    p
}

/// The view direction of a heading (0 north, +z; turning right towards +x) and a tilt (negative down).
fn direction(yaw: f32, pitch: f32) -> Float3 {
    let cp = pitch.cos();
    Float3::new(yaw.sin() * cp, pitch.sin(), yaw.cos() * cp)
}

fn heading_and_tilt(d: Float3) -> (f32, f32) {
    let d = normalize(d);
    (d.x.atan2(d.z), d.y.clamp(-1.0, 1.0).asin())
}

/// The Total War camera: WASD moves over the ground (faster higher up, Shift faster still), Q / E turn, the wheel moves along the view
/// and tilts it flatter lower down, a drag turns and tilts. Smoothed as the options say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TotalWarCamera {
    pub lens: Lens,

    /// Where the controls put the camera.
    pub eye: Float3,
    pub yaw: f32,
    pub pitch: f32,

    /// The distance to what it looks at (the ground along the view when it meets it).
    pub focus_dist: f32,

    /// What is shown: the controls' view, smoothed.
    shown_eye: Float3,
    shown_yaw: f32,
    shown_pitch: f32,
}

impl Default for TotalWarCamera {
    fn default() -> Self {
        let mut c = Self {
            lens: Lens::default(), eye: Float3::default(), yaw: 0.0, pitch: 0.0, focus_dist: 120.0, shown_eye: Float3::default(), shown_yaw: 0.0,
            shown_pitch: 0.0,
        };

        c.set_orbit(0.6, 0.35, 120.0, Float3::new(0.0, 5.0, 0.0));
        c
    }
}

impl TotalWarCamera {
    const MIN_PITCH: f32 = -1.55;
    const MAX_PITCH: f32 = 1.2;

    fn snap(&mut self) {
        (self.shown_eye, self.shown_yaw, self.shown_pitch) = (self.eye, self.yaw, self.pitch);
    }
}

impl CameraController for TotalWarCamera {
    fn mode(&self) -> CameraMode {
        CameraMode::TotalWar
    }

    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld) {
        let before = (self.shown_eye, self.shown_yaw, self.shown_pitch);
        if input.rotate || input.pan {
            self.yaw += input.dx * 0.004;
            self.pitch = (self.pitch - input.dy * 0.004).clamp(Self::MIN_PITCH, Self::MAX_PITCH);
        }

        let turn = f32::from(u8::from(input.turn_right)) - f32::from(u8::from(input.turn_left));
        self.yaw += turn * 1.5 * input.dt * if input.fast { 2.0 } else { 1.0 };

        // The speeds follow the height above the ground (the sea's level off the terrain).
        let height = |world: &mut dyn CameraWorld, p: Float3| p.y - world.ground(p.x, p.z).unwrap_or(0.0);
        let h = height(world, self.eye);
        let (fwd, right) = (Float3::new(self.yaw.sin(), 0.0, self.yaw.cos()), Float3::new(self.yaw.cos(), 0.0, -self.yaw.sin()));
        let mut m = fwd * input.edge[1] + right * input.edge[0];
        for (on, d) in [(input.forward, fwd), (input.back, -fwd), (input.right, right), (input.left, -right)] {
            if on {
                m = m + d;
            }
        }

        let ml = length(m);
        if ml > 0.0 {
            let speed = h.max(5.0) * 1.2 * if input.fast { 4.0 } else { 1.0 };
            self.eye = self.eye + m * (speed * input.dt / ml.max(1.0));
        }

        // The wheel moves along the view; lower down the view is flatter (the tilt scales with the height's fourth root).
        if input.wheel != 0.0 {
            let step = h.max(2.0) * 0.2 * input.wheel;
            self.eye = self.eye + direction(self.yaw, self.pitch) * step;
            self.focus_dist = (self.focus_dist - step).max(1.0);
            let h2 = height(world, self.eye);
            if self.pitch < 0.0 && h > 0.0 && h2 > 0.0 {
                self.pitch = (self.pitch * (h2 / h).powf(0.25)).clamp(Self::MIN_PITCH, -0.02);
            }
        }

        if opts.game_limits
            && let Some(g) = world.ground(self.eye.x, self.eye.z)
        {
            self.eye.y = self.eye.y.clamp(g + GAME_MIN_HEIGHT, g + GAME_MAX_HEIGHT);
        }

        self.eye = constrain(self.eye, opts, world);

        let t = smooth_step(opts, input.dt);
        self.shown_eye = constrain(lerp3(self.shown_eye, self.eye, t), opts, world);
        self.shown_yaw = lerp(self.shown_yaw, self.yaw, t);
        self.shown_pitch = lerp(self.shown_pitch, self.pitch, t);

        // What it looks at: the ground along the view, when the view moved.
        if (self.shown_eye, self.shown_yaw, self.shown_pitch) != before
            && let Some(hit) = world.ray_ground(self.shown_eye, direction(self.shown_yaw, self.shown_pitch))
        {
            self.focus_dist = length(hit - self.shown_eye).max(1.0);
        }
    }

    fn camera(&self) -> Camera {
        let target = self.shown_eye + direction(self.shown_yaw, self.shown_pitch) * self.focus_dist.max(1.0);
        Camera { eye: self.shown_eye, target, up: Float3::new(0.0, 1.0, 0.0), fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar, ortho_height: None }
    }

    fn set_camera(&mut self, cam: &Camera) {
        self.eye = cam.eye;
        let (yaw, pitch) = heading_and_tilt(cam.target - cam.eye);
        self.yaw = yaw;
        self.pitch = pitch.clamp(Self::MIN_PITCH, Self::MAX_PITCH);
        self.focus_dist = length(cam.target - cam.eye).max(1.0);
        self.snap();
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        if self.pitch > -0.35 {
            self.pitch = -0.6;
        }

        let dist = (radius / (self.lens.fov_y * 0.5).tan() * 1.3).clamp(5.0, 6000.0);
        self.eye = center - direction(self.yaw, self.pitch) * dist;
        self.focus_dist = dist;
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}

/// The top-down view (TEd's "Ortho Top"): orthographic, straight down, north up until Q / E turn it. A drag or WASD pans, the wheel zooms
/// about the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopDownCamera {
    pub lens: Lens,

    /// The point at the view's centre (its height is the ground's there).
    pub centre: Float3,

    /// The direction of the screen's up on the ground: 0 north (+z), turning right towards +x.
    pub heading: f32,

    /// The view's height in metres.
    pub height: f32,

    /// The eye's height: above the ground the view covers.
    eye_y: f32,
    shown_centre: Float3,
    shown_heading: f32,
    shown_height: f32,
}

impl Default for TopDownCamera {
    fn default() -> Self {
        Self {
            lens: Lens::default(), centre: Float3::default(), heading: 0.0, height: 400.0, eye_y: 100.0, shown_centre: Float3::default(), shown_heading: 0.0,
            shown_height: 400.0,
        }
    }
}

impl TopDownCamera {
    /// How far above the highest ground in view the eye is: above the tallest building, low enough that the game's fog (by the distance
    /// from the eye) does not veil the ground.
    const ABOVE: f32 = 100.0;

    fn axes(heading: f32) -> (Float3, Float3) {
        (Float3::new(heading.cos(), 0.0, -heading.sin()), Float3::new(heading.sin(), 0.0, heading.cos()))
    }

    fn snap(&mut self) {
        (self.shown_centre, self.shown_heading, self.shown_height) = (self.centre, self.heading, self.height);
    }
}

impl CameraController for TopDownCamera {
    fn mode(&self) -> CameraMode {
        CameraMode::TopDown
    }

    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld) {
        let (right, up) = Self::axes(self.heading);
        let aspect = if input.aspect > 0.0 { input.aspect } else { 1.0 };
        if (input.rotate || input.pan) && input.view_h > 0.0 {
            let per_pixel = self.height / input.view_h;
            self.centre = self.centre - right * (input.dx * per_pixel) + up * (input.dy * per_pixel);
        }

        let mut m = right * input.edge[0] + up * input.edge[1];
        for (on, d) in [(input.forward, up), (input.back, -up), (input.right, right), (input.left, -right)] {
            if on {
                m = m + d;
            }
        }

        let ml = length(m);
        if ml > 0.0 {
            self.centre = self.centre + m * (self.height * 0.6 * input.dt * if input.fast { 4.0 } else { 1.0 } / ml.max(1.0));
        }

        let turn = f32::from(u8::from(input.turn_right)) - f32::from(u8::from(input.turn_left));
        self.heading += turn * 1.5 * input.dt * if input.fast { 2.0 } else { 1.0 };

        // The wheel zooms about the point under the mouse.
        if input.wheel != 0.0 {
            let h2 = (self.height * 0.85f32.powf(input.wheel)).clamp(5.0, 20000.0);
            if let Some([nx, ny]) = input.cursor {
                let (r, u) = Self::axes(self.heading);
                let off = |h: f32| r * (nx * h * 0.5 * aspect) + u * (ny * h * 0.5);
                self.centre = self.centre + off(self.height) - off(h2);
            }

            self.height = h2;
        }

        let t = smooth_step(opts, input.dt);
        self.shown_centre = lerp3(self.shown_centre, self.centre, t);
        self.shown_heading = lerp(self.shown_heading, self.heading, t);
        self.shown_height = lerp(self.shown_height, self.height, t);

        // The eye above the highest ground at the view's centre and corners.
        let (r, u) = Self::axes(self.shown_heading);
        let (hw, hh) = (self.shown_height * 0.5 * aspect, self.shown_height * 0.5);
        let c = self.shown_centre;
        let mut top = f32::MIN;
        for p in [c, c + r * hw + u * hh, c - r * hw + u * hh, c + r * hw - u * hh, c - r * hw - u * hh] {
            if let Some(g) = world.ground(p.x, p.z) {
                top = top.max(g);
            }
        }

        if let Some(g) = world.ground(c.x, c.z) {
            self.centre.y = g;
            self.shown_centre.y = g;
        }

        self.eye_y = if top > f32::MIN { top } else { self.shown_centre.y } + Self::ABOVE;
    }

    fn camera(&self) -> Camera {
        let c = self.shown_centre;
        let (_, up) = Self::axes(self.shown_heading);
        Camera {
            eye: Float3::new(c.x, self.eye_y, c.z), target: Float3::new(c.x, self.eye_y - 100.0, c.z), up, fov_y: self.lens.fov_y, znear: self.lens.znear,
            zfar: self.lens.zfar, ortho_height: Some(self.shown_height),
        }
    }

    /// A steep perspective view over the centre that shows about as much of the ground.
    fn handoff(&self) -> Camera {
        let dist = self.shown_height * 0.5 / (self.lens.fov_y * 0.5).tan();
        let target = self.shown_centre;
        Camera { eye: target - direction(self.shown_heading, -1.2) * dist, target, up: Float3::new(0.0, 1.0, 0.0), ortho_height: None, ..self.camera() }
    }

    fn set_camera(&mut self, cam: &Camera) {
        if let Some(h) = cam.ortho_height {
            self.centre = cam.target;
            self.height = h;
            let (_, up, _) = cam.axes();
            self.heading = up.x.atan2(up.z);
        } else {
            // Over what the view looks at, as much of the ground as it shows there, north up.
            self.centre = cam.target;
            let d = cam.target - cam.eye;
            self.height = (2.0 * (cam.fov_y * 0.5).tan() * length(d)).clamp(5.0, 20000.0);
            self.heading = 0.0;
        }

        self.eye_y = self.centre.y + Self::ABOVE;
        self.snap();
    }

    fn pivot(&self) -> Float3 {
        self.shown_centre
    }

    fn move_pivot(&mut self, p: Float3) {
        self.centre = p;
        self.snap();
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        self.centre = center;
        self.height = (radius * 2.4).clamp(5.0, 20000.0);
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}

/// The walk camera (TEd's "Walk"): the eye at a man's height above the ground; WASD walks (Shift runs), Q / E and a drag turn, the wheel
/// changes the walking speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkCamera {
    pub lens: Lens,

    /// The eye.
    pub eye: Float3,
    pub yaw: f32,
    pub pitch: f32,

    /// Metres per second.
    pub speed: f32,

    /// The eye above the ground (TEd's `ground_height`).
    pub eye_height: f32,
    shown_eye: Float3,
    shown_yaw: f32,
    shown_pitch: f32,
}

impl Default for WalkCamera {
    fn default() -> Self {
        Self {
            lens: Lens::default(), eye: Float3::new(0.0, 1.75, 0.0), yaw: 0.0, pitch: 0.0, speed: 5.0, eye_height: 1.75, shown_eye: Float3::new(0.0, 1.75, 0.0),
            shown_yaw: 0.0, shown_pitch: 0.0,
        }
    }
}

impl WalkCamera {
    fn snap(&mut self) {
        (self.shown_eye, self.shown_yaw, self.shown_pitch) = (self.eye, self.yaw, self.pitch);
    }

    fn stand(&self, p: Float3, world: &mut dyn CameraWorld) -> Float3 {
        Float3::new(p.x, world.ground(p.x, p.z).map_or(p.y, |g| g + self.eye_height), p.z)
    }
}

impl CameraController for WalkCamera {
    fn mode(&self) -> CameraMode {
        CameraMode::Walk
    }

    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld) {
        if input.rotate || input.pan {
            self.yaw += input.dx * 0.004;
            self.pitch = (self.pitch - input.dy * 0.004).clamp(-1.5, 1.5);
        }

        let turn = f32::from(u8::from(input.turn_right)) - f32::from(u8::from(input.turn_left));
        self.yaw += turn * 1.5 * input.dt;

        if input.wheel != 0.0 {
            self.speed = (self.speed * 1.2f32.powf(input.wheel)).clamp(0.5, 200.0);
        }

        let (fwd, right) = (Float3::new(self.yaw.sin(), 0.0, self.yaw.cos()), Float3::new(self.yaw.cos(), 0.0, -self.yaw.sin()));
        let mut m = fwd * input.edge[1] + right * input.edge[0];
        for (on, d) in [(input.forward, fwd), (input.back, -fwd), (input.right, right), (input.left, -right)] {
            if on {
                m = m + d;
            }
        }

        let ml = length(m);
        if ml > 0.0 {
            self.eye = self.eye + m * (self.speed * input.dt * if input.fast { 4.0 } else { 1.0 } / ml.max(1.0));
        }

        // On the ground, out of the obstacles (the ground option does not apply: the walker always stands on it).
        let walk = CameraOptions { respect_ground: false, ..*opts };
        self.eye = self.stand(constrain(self.eye, &walk, world), world);
        let t = smooth_step(opts, input.dt);
        self.shown_eye = lerp3(self.shown_eye, self.eye, t);
        self.shown_yaw = lerp(self.shown_yaw, self.yaw, t);
        self.shown_pitch = lerp(self.shown_pitch, self.pitch, t);
    }

    fn camera(&self) -> Camera {
        let target = self.shown_eye + direction(self.shown_yaw, self.shown_pitch) * 10.0;
        Camera { eye: self.shown_eye, target, up: Float3::new(0.0, 1.0, 0.0), fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar, ortho_height: None }
    }

    /// Stands where the view looked (its target), facing the same way, looking ahead.
    fn set_camera(&mut self, cam: &Camera) {
        let (yaw, _) = heading_and_tilt(if cam.is_ortho() { cam.axes().1 } else { cam.target - cam.eye });
        self.yaw = yaw;
        self.pitch = 0.0;
        self.eye = Float3::new(cam.target.x, cam.target.y + self.eye_height, cam.target.z);
        self.snap();
    }

    fn pivot(&self) -> Float3 {
        self.shown_eye - Float3::new(0.0, self.eye_height, 0.0)
    }

    fn move_pivot(&mut self, p: Float3) {
        self.eye = p + Float3::new(0.0, self.eye_height, 0.0);
        self.snap();
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        let back = Float3::new(self.yaw.sin(), 0.0, self.yaw.cos()) * (radius * 1.5 + 2.0);
        self.eye = Float3::new(center.x - back.x, center.y + self.eye_height, center.z - back.z);
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}

/// Free flight (TEd's "Look"): a drag looks around, WASD moves along the view, Q / E down and up, the wheel changes the speed. Without
/// smoothing, the ground and the obstacles (all off) it moves exactly as the C++ editor's (`editor_dump`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyCamera {
    pub lens: Lens,
    pub eye: Float3,
    pub yaw: f32,
    pub pitch: f32,

    /// Metres per second.
    pub speed: f32,
    shown_eye: Float3,
    shown_yaw: f32,
    shown_pitch: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        let eye = Float3::new(0.0, 30.0, -80.0);
        Self { lens: Lens::default(), eye, yaw: 0.0, pitch: -0.2, speed: 40.0, shown_eye: eye, shown_yaw: 0.0, shown_pitch: -0.2 }
    }
}

impl FlyCamera {
    fn snap(&mut self) {
        (self.shown_eye, self.shown_yaw, self.shown_pitch) = (self.eye, self.yaw, self.pitch);
    }
}

impl CameraController for FlyCamera {
    fn mode(&self) -> CameraMode {
        CameraMode::Fly
    }

    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld) {
        if input.rotate || input.pan {
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
        for (on, d) in [(input.forward, fwd), (input.back, -fwd), (input.right, right), (input.left, -right), (input.turn_right, up), (input.turn_left, -up)] {
            if on {
                m = m + d;
            }
        }

        m = m + right * input.edge[0] + Float3::new(fwd.x, 0.0, fwd.z) * input.edge[1];
        self.eye = constrain(self.eye + m * (self.speed * if input.fast { 4.0 } else { 1.0 } * input.dt), opts, world);
        let t = smooth_step(opts, input.dt);
        self.shown_eye = constrain(lerp3(self.shown_eye, self.eye, t), opts, world);
        self.shown_yaw = lerp(self.shown_yaw, self.yaw, t);
        self.shown_pitch = lerp(self.shown_pitch, self.pitch, t);
    }

    fn camera(&self) -> Camera {
        let target = self.shown_eye + direction(self.shown_yaw, self.shown_pitch) * 10.0;
        Camera { eye: self.shown_eye, target, up: Float3::new(0.0, 1.0, 0.0), fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar, ortho_height: None }
    }

    fn set_camera(&mut self, cam: &Camera) {
        self.eye = cam.eye;
        let d = normalize(cam.target - cam.eye);
        self.pitch = d.y.clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(d.z);
        self.snap();
    }

    /// The eye: a free camera is about where it is.
    fn pivot(&self) -> Float3 {
        self.shown_eye
    }

    fn move_pivot(&mut self, p: Float3) {
        self.eye = p;
        self.snap();
    }

    fn focus(&mut self, center: Float3, radius: f32) {
        let d = direction(self.yaw, self.pitch);
        self.eye = center - d * (radius / (self.lens.fov_y * 0.5).tan() * 1.3).clamp(5.0, 6000.0);
    }

    fn lens(&self) -> &Lens {
        &self.lens
    }

    fn lens_mut(&mut self) -> &mut Lens {
        &mut self.lens
    }
}

/// Around a target (TEd's "Orbit"): a drag turns around it, the middle button pans it, the wheel goes nearer and farther, WASD moves it
/// over the ground (faster farther away, Shift faster still), Q / E turn around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub lens: Lens,

    /// The eye at target + (sin yaw cos pitch, sin pitch, -cos yaw cos pitch) * dist (the `cam` command's convention).
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Float3,
    shown: (f32, f32, f32, Float3),

    /// The eye where the ground or an obstacle stopped it (`None`: where the orbit puts it).
    eye_override: Option<Float3>,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let target = Float3::new(0.0, 5.0, 0.0);
        Self { lens: Lens::default(), yaw: 0.6, pitch: 0.35, dist: 120.0, target, shown: (0.6, 0.35, 120.0, target), eye_override: None }
    }
}

impl OrbitCamera {
    fn snap(&mut self) {
        self.shown = (self.yaw, self.pitch, self.dist, self.target);
        self.eye_override = None;
    }

    fn orbit_eye(yaw: f32, pitch: f32, dist: f32, target: Float3) -> Float3 {
        let (cp, sp) = (pitch.cos(), pitch.sin());
        target + Float3::new(yaw.sin() * cp, sp, -yaw.cos() * cp) * dist
    }
}

impl CameraController for OrbitCamera {
    fn mode(&self) -> CameraMode {
        CameraMode::Orbit
    }

    fn update(&mut self, input: &CameraInput, opts: &CameraOptions, world: &mut dyn CameraWorld) {
        if input.rotate {
            self.yaw += input.dx * 0.006;
            self.pitch = (self.pitch + input.dy * 0.006).clamp(-1.2, 1.5);
        }

        let turn = f32::from(u8::from(input.turn_right)) - f32::from(u8::from(input.turn_left));
        self.yaw -= turn * 1.5 * input.dt * if input.fast { 2.0 } else { 1.0 };

        // Forward is where the camera looks, flattened.
        let fwd = Float3::new(-self.yaw.sin(), 0.0, self.yaw.cos());
        let right = Float3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        if input.pan {
            let k = self.dist * 0.0015;
            self.target = self.target - right * (input.dx * k) + fwd * (input.dy * k);
        }

        if input.wheel != 0.0 {
            self.dist = (self.dist * 0.9f32.powf(input.wheel)).clamp(2.0, 6000.0);
        }

        let mut m = fwd * input.edge[1] + right * input.edge[0];
        for (on, d) in [(input.forward, fwd), (input.back, -fwd), (input.right, right), (input.left, -right)] {
            if on {
                m = m + d;
            }
        }

        let ml = length(m);
        if ml > 0.0 {
            let speed = self.dist.max(10.0) * if input.fast { 4.0 } else { 1.0 };
            self.target = self.target + m * (speed * input.dt / ml.max(1.0));

            // The target stays on the ground as it moves over it.
            if let Some(g) = world.ground(self.target.x, self.target.z) {
                self.target.y = g;
            }
        }

        let t = smooth_step(opts, input.dt);
        let (y, p, d, tg) = self.shown;
        self.shown = (lerp(y, self.yaw, t), lerp(p, self.pitch, t), lerp(d, self.dist, t), lerp3(tg, self.target, t));
        let eye = Self::orbit_eye(self.shown.0, self.shown.1, self.shown.2, self.shown.3);
        let kept = constrain(eye, opts, world);
        self.eye_override = (kept != eye).then_some(kept);
    }

    fn camera(&self) -> Camera {
        let (yaw, pitch, dist, target) = self.shown;
        let eye = self.eye_override.unwrap_or_else(|| Self::orbit_eye(yaw, pitch, dist, target));
        Camera { eye, target, up: Float3::new(0.0, 1.0, 0.0), fov_y: self.lens.fov_y, znear: self.lens.znear, zfar: self.lens.zfar, ortho_height: None }
    }

    fn set_camera(&mut self, cam: &Camera) {
        let d = cam.eye - cam.target;
        self.dist = length(d).max(1.0);
        self.target = cam.target;
        self.pitch = (d.y / self.dist).clamp(-1.0, 1.0).asin();
        self.yaw = d.x.atan2(-d.z);
        self.snap();
    }

    fn set_orbit(&mut self, yaw: f32, pitch: f32, dist: f32, target: Float3) {
        (self.yaw, self.pitch, self.dist, self.target) = (yaw, pitch, dist, target);
        self.snap();
    }

    fn move_pivot(&mut self, p: Float3) {
        self.target = p;
        self.snap();
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
