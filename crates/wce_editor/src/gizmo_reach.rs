//! The gizmo's whole reach: ImGuizmo takes a click only within a few pixels of a handle (8 of a ring, 12 of an axis), so a click between the
//! handles went through to the scene and unselected what the gizmo stood on. Here, a mouse inside the gizmo but off its handles is shown to
//! ImGuizmo at the closest point of the closest handle ([`snap`]), which so highlights and takes the drag. The drag goes on from that point
//! by the mouse's movement (`Viewport::gizmo_offset`): shown the real mouse, a click alone would move the item by the offset.
//!
//! The geometry is ImGuizmo's own (its `ComputeContext`, `ComputeTripodAxisAndVisibility`, `DrawRotationGizmo`, `GetMoveType`, `GetScaleType`,
//! `GetRotateType`): the handles are measured where it draws them, to the pixel.

use wce_foundation::math::{Float3, dot};

/// `rotationDisplayFactor`: a ring's radius in axis lengths.
const RING: f32 = 1.2;

/// `screenRotateSize`: the smallest outer ring, as a part of the view's height.
const OUTER_RING: f32 = 0.06;

/// An axis starts this far along itself (`GetMoveType`'s and `GetScaleType`'s `0.1f`).
const AXIS_START: f32 = 0.1;

/// A click within this many pixels past the outer ring is on it: ImGuizmo's own tolerance is 4, less than the line is thick to the eye.
const OUTER_MARGIN: f32 = 6.0;

/// Half the side of the square at the centre, in pixels.
const CENTRE_SQUARE: f32 = 10.0;

/// A plane's square, in axis lengths (`quadMin`, `quadMax`), and the least it has on screen to be drawn (pixels squared).
const QUAD_MIN: f32 = 0.5;
const QUAD_MAX: f32 = 0.8;
const MIN_QUAD: f32 = 16.0;

/// `mPlaneLimit`: an axis pointing at the camera, shorter than this in clip space, is not drawn.
const AXIS_LIMIT: f32 = 0.02;

/// The points a ring is measured at: closer than ImGuizmo's 8 pixels on any ring that fits the view.
const RING_SAMPLES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handles {
    /// Three axes and the squares of three planes: the translate gizmo.
    Move,

    /// Three axes: the scale gizmo.
    Scale,

    /// Three rings and the outer one that turns about the view.
    Rings,

    /// What stands on the ground: the x and z axes, the square between them and the y ring. It has no centre of its own.
    Ground,
}

/// A gizmo as it stands in the view. Positions on screen are the window's pixels, as the mouse's.
pub struct Gizmo<'a> {
    /// A point of the world on screen.
    pub project: &'a dyn Fn(Float3) -> Option<[f32; 2]>,

    /// The view's size in pixels.
    pub view: [f32; 2],
    pub centre: Float3,

    /// Its axes in the world, unit length.
    pub axes: [Float3; 3],

    /// The camera's right and view directions.
    pub right: Float3,
    pub forward: Float3,
    pub orthographic: bool,

    /// `mGizmoSizeClipSpace`.
    pub size: f32,
}

impl Gizmo<'_> {
    /// `GetSegmentLengthClipSpace`.
    fn clip_length(&self, a: Float3, b: Float3) -> Option<f32> {
        let (a, b) = ((self.project)(a)?, (self.project)(b)?);
        let ratio = self.view[0] / self.view[1];
        let (mut x, mut y) = ((b[0] - a[0]) * 2.0 / self.view[0], (b[1] - a[1]) * 2.0 / self.view[1]);
        if ratio < 1.0 {
            x *= ratio;
        } else {
            y /= ratio;
        }

        Some((x * x + y * y).sqrt())
    }

    /// `mScreenFactor`: the world length of an axis at the gizmo's distance.
    fn axis_length(&self) -> Option<f32> {
        let len = self.clip_length(self.centre, self.centre + self.right)?;
        (len > 1e-6).then(|| self.size / len)
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The point of a segment closest to `p`.
fn on_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 > 0.0 { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
    [a[0] + d[0] * t, a[1] + d[1] * t]
}

/// Whether `p` is inside a convex quad on screen.
fn in_quad(p: [f32; 2], quad: &[[f32; 2]; 4]) -> bool {
    let side = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let sides = [side(quad[0], quad[1]), side(quad[1], quad[2]), side(quad[2], quad[3]), side(quad[3], quad[0])];
    sides.iter().all(|&s| s >= 0.0) || sides.iter().all(|&s| s <= 0.0)
}

fn quad_area(q: &[[f32; 2]; 4]) -> f32 {
    let tri = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
    tri(q[0], q[1], q[2]) + tri(q[0], q[2], q[3])
}

/// Where to show ImGuizmo a mouse at `mouse`: the closest point of the closest handle, when the mouse is inside the gizmo, off the handles
/// that ImGuizmo takes as they are. `None`: the real mouse.
///
/// This depends on the mouse and the gizmo alone, never on what ImGuizmo said (`IsOver` is about the last frame, where it was shown this
/// point: asking it made the handle come and go, frame by frame).
pub fn snap(handles: Handles, mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    match handles {
        Handles::Move => snap_axes(mouse, g, true, false),

        // ImGuizmo's scale is the distance from the centre over the one the drag began at: grabbed near the centre, a few pixels were many
        // times the size, and past the centre nothing. From the axis's tip it is the same wherever the click was.
        Handles::Scale => snap_axes(mouse, g, false, true),
        Handles::Rings => snap_rings(mouse, g),
        Handles::Ground => snap_ground(mouse, g),
    }
}

fn snap_ground(mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    let length = g.axis_length()?;
    let centre = (g.project)(g.centre)?;
    let longer = |d: Float3| Some(if g.clip_length(g.centre, g.centre + d)? < g.clip_length(g.centre, g.centre - d)? { -d } else { d });
    let (x, z) = (longer(g.axes[0])?, longer(g.axes[2])?);

    let corner = |a: f32, b: f32| (g.project)(g.centre + z * (length * a) + x * (length * b));
    let quad = [corner(QUAD_MIN, QUAD_MIN)?, corner(QUAD_MIN, QUAD_MAX)?, corner(QUAD_MAX, QUAD_MAX)?, corner(QUAD_MAX, QUAD_MIN)?];
    if quad_area(&quad) > MIN_QUAD && in_quad(mouse, &quad) {
        return None;
    }

    let mut best: Option<(f32, [f32; 2])> = None;
    let mut offer = |p: [f32; 2]| {
        let dist = distance(mouse, p);
        if best.is_none_or(|(n, _)| dist < n) {
            best = Some((dist, p));
        }
    };

    for d in [x, z] {
        if g.clip_length(g.centre, g.centre + d * length)? > AXIS_LIMIT {
            offer(on_segment(mouse, (g.project)(g.centre + d * (length * AXIS_START))?, (g.project)(g.centre + d * length)?));
        }
    }

    // Only the ring's half towards the camera is drawn and hit (`GetRotateType`).
    let radius = length * RING;
    let mut reach = 0.0f32;
    for k in 0..RING_SAMPLES {
        let (s, c) = (k as f32 / RING_SAMPLES as f32 * std::f32::consts::TAU).sin_cos();
        let offset = (g.axes[2] * c + g.axes[0] * s) * radius;
        let Some(p) = (g.project)(g.centre + offset) else { continue };
        reach = reach.max(distance(centre, p));
        if dot(offset, g.forward) <= 1e-4 {
            offer(p);
        }
    }

    best.filter(|_| distance(mouse, centre) <= reach + OUTER_MARGIN).map(|(_, p)| p)
}

fn snap_axes(mouse: [f32; 2], g: &Gizmo, planes: bool, tips: bool) -> Option<[f32; 2]> {
    let length = g.axis_length()?;
    let centre = (g.project)(g.centre)?;

    // The square at the centre (`mScreenSquareMin`, `mScreenSquareMax`).
    if (mouse[0] - centre[0]).abs() <= CENTRE_SQUARE && (mouse[1] - centre[1]).abs() <= CENTRE_SQUARE {
        return None;
    }

    // Each axis points to the side that is longer on screen (`mulAxis`).
    let mut dirs = g.axes;
    for d in &mut dirs {
        if g.clip_length(g.centre, g.centre + *d)? < g.clip_length(g.centre, g.centre - *d)? {
            *d = -*d;
        }
    }

    // The planes' squares (`quadUV`).
    for i in (0..3).filter(|_| planes) {
        let (u, v) = (dirs[(i + 1) % 3] * length, dirs[(i + 2) % 3] * length);
        let corner = |a: f32, b: f32| (g.project)(g.centre + u * a + v * b);
        let quad = [corner(QUAD_MIN, QUAD_MIN)?, corner(QUAD_MIN, QUAD_MAX)?, corner(QUAD_MAX, QUAD_MAX)?, corner(QUAD_MAX, QUAD_MIN)?];
        if quad_area(&quad) > MIN_QUAD && in_quad(mouse, &quad) {
            return None;
        }
    }

    let (mut best, mut reach): (Option<(f32, [f32; 2])>, f32) = (None, 0.0);
    for d in dirs {
        if g.clip_length(g.centre, g.centre + d * length)? <= AXIS_LIMIT {
            continue;
        }

        let (a, b) = ((g.project)(g.centre + d * (length * AXIS_START))?, (g.project)(g.centre + d * length)?);
        reach = reach.max(distance(centre, b));
        let p = on_segment(mouse, a, b);
        let dist = distance(mouse, p);
        if best.is_none_or(|(n, _)| dist < n) {
            best = Some((dist, if tips { b } else { p }));
        }
    }

    best.filter(|_| distance(mouse, centre) <= reach).map(|(_, p)| p)
}

/// The radius of the white outer ring on screen, as ImGuizmo makes it: the widest of the rings measured where each is first drawn, on the
/// edge of its half towards the camera (`DrawRotationGizmo`'s `circlePos[0]`). In perspective a ring's nearer points are wider than that.
fn outer_ring(g: &Gizmo, radius: f32, centre: [f32; 2]) -> f32 {
    let mut outer = OUTER_RING * g.view[1];
    let to_camera = [dot(-g.forward, g.axes[0]), dot(-g.forward, g.axes[1]), dot(-g.forward, g.axes[2])];
    for axis in 0..3 {
        let start = to_camera[(4 - axis) % 3].atan2(to_camera[(3 - axis) % 3])
            + std::f32::consts::FRAC_PI_2 * if g.orthographic { 3.0 } else { 1.0 };
        let (sin, cos) = start.sin_cos();
        let on_ring = [cos, sin, 0.0];
        let at = g.axes[0] * on_ring[axis] + g.axes[1] * on_ring[(axis + 1) % 3] + g.axes[2] * on_ring[(axis + 2) % 3];
        if let Some(p) = (g.project)(g.centre + at * radius) {
            outer = outer.max(distance(centre, p));
        }
    }

    outer
}

fn snap_rings(mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    let radius = g.axis_length()? * RING;
    let centre = (g.project)(g.centre)?;
    let outer = outer_ring(g, radius, centre);
    let from_centre = distance(mouse, centre);
    if from_centre > outer + OUTER_MARGIN {
        return None;
    }

    let mut best: Option<(f32, [f32; 2])> = None;
    for i in 0..3 {
        let (u, v) = (g.axes[(i + 1) % 3], g.axes[(i + 2) % 3]);
        for k in 0..RING_SAMPLES {
            let (s, c) = (k as f32 / RING_SAMPLES as f32 * std::f32::consts::TAU).sin_cos();
            let offset = (u * c + v * s) * radius;

            // Only a ring's half towards the camera is drawn and hit (`GetRotateType`).
            if dot(offset, g.forward) > 1e-4 {
                continue;
            }

            let Some(p) = (g.project)(g.centre + offset) else { continue };
            let dist = distance(mouse, p);
            if best.is_none_or(|(n, _)| dist < n) {
                best = Some((dist, p));
            }
        }
    }

    if from_centre > 1e-3 && best.is_none_or(|(n, _)| (outer - from_centre).abs() < n) {
        let k = outer / from_centre;
        return Some([centre[0] + (mouse[0] - centre[0]) * k, centre[1] + (mouse[1] - centre[1]) * k]);
    }

    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view down +z of 800 x 600 pixels, 10 pixels a metre, the centre of the world at (400, 300).
    fn with<R>(f: impl FnOnce(&Gizmo) -> R) -> R {
        let project = |p: Float3| Some([400.0 + p.x * 10.0, 300.0 - p.y * 10.0]);
        let g = Gizmo {
            project: &project,
            view: [800.0, 600.0],
            centre: Float3::default(),
            axes: [Float3::new(1.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 0.0, 1.0)],
            right: Float3::new(1.0, 0.0, 0.0),
            forward: Float3::new(0.0, 0.0, 1.0),
            orthographic: true,
            size: 0.25,
        };

        f(&g)
    }

    #[test]
    fn inside_the_axes_a_click_goes_to_the_closest_one() {
        // An axis is 0.25 of the view's half width: 100 pixels.
        with(|g| {
            assert_eq!(snap(Handles::Move, [460.0, 290.0], g), Some([460.0, 300.0]), "the x axis, 10 pixels up");
            assert_eq!(snap(Handles::Move, [413.0, 240.0], g), Some([400.0, 240.0]), "the y axis");
        });
    }

    #[test]
    fn outside_its_reach_a_click_goes_through() {
        with(|g| {
            assert_eq!(snap(Handles::Scale, [520.0, 300.0], g), None);
            assert_eq!(snap(Handles::Rings, [540.0, 300.0], g), None);
        });
    }

    #[test]
    fn an_axis_at_the_camera_is_not_a_handle() {
        // The z axis has no length on this screen: a click at the centre's side goes to x, not to it.
        with(|g| {
            let p = snap(Handles::Move, [430.0, 298.0], g).unwrap();
            assert_eq!(p, [430.0, 300.0]);
        });
    }

    #[test]
    fn a_scale_is_grabbed_at_the_tip_of_its_axis() {
        with(|g| {
            assert_eq!(snap(Handles::Scale, [425.0, 295.0], g), Some([500.0, 300.0]), "wherever it is clicked: it scales alike");
            assert_eq!(snap(Handles::Scale, [405.0, 250.0], g), Some([400.0, 200.0]));
        });
    }

    #[test]
    fn its_own_handles_keep_the_real_mouse() {
        with(|g| {
            assert_eq!(snap(Handles::Scale, [405.0, 296.0], g), None, "the centre's square");
            assert_eq!(snap(Handles::Move, [405.0, 296.0], g), None);

            // The x-y plane's square, 50 to 80 pixels along both axes; only the move gizmo has it.
            assert_eq!(snap(Handles::Move, [465.0, 235.0], g), None);
            assert!(snap(Handles::Scale, [465.0, 235.0], g).is_some());
        });
    }

    #[test]
    fn the_answer_is_the_same_every_time() {
        // Shown a snapped point, the next frame's real mouse is the same: nothing may depend on the last answer.
        with(|g| {
            for mouse in [[460.0, 290.0], [430.0, 298.0], [481.0, 381.0]] {
                let first = (snap(Handles::Move, mouse, g), snap(Handles::Rings, mouse, g));
                assert_eq!(first, (snap(Handles::Move, mouse, g), snap(Handles::Rings, mouse, g)));
            }
        });
    }

    /// A view from above of 800 x 600 pixels, 10 pixels a metre: x to the right, z up the screen.
    fn from_above<R>(f: impl FnOnce(&Gizmo) -> R) -> R {
        let project = |p: Float3| Some([400.0 + p.x * 10.0, 300.0 - p.z * 10.0]);
        let g = Gizmo {
            project: &project,
            view: [800.0, 600.0],
            centre: Float3::default(),
            axes: [Float3::new(1.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 0.0, 1.0)],
            right: Float3::new(1.0, 0.0, 0.0),
            forward: Float3::new(0.0, -1.0, 0.0),
            orthographic: true,
            size: 0.25,
        };

        f(&g)
    }

    #[test]
    fn on_the_ground_a_click_goes_to_the_closest_of_its_axes_and_ring() {
        from_above(|g| {
            assert_eq!(snap(Handles::Ground, [460.0, 292.0], g), Some([460.0, 300.0]), "the x axis");
            assert_eq!(snap(Handles::Ground, [392.0, 240.0], g), Some([400.0, 240.0]), "the z axis");

            // Past the axes: the ring, 120 pixels out.
            let p = snap(Handles::Ground, [400.0 + 112.0 * 0.6, 300.0 + 112.0 * 0.8], g).unwrap();
            assert!((distance(p, [400.0, 300.0]) - 120.0).abs() < 1.0, "{p:?}");
            let p = snap(Handles::Ground, [400.0 + 124.0 * 0.6, 300.0 + 124.0 * 0.8], g).unwrap();
            assert!((distance(p, [400.0, 300.0]) - 120.0).abs() < 1.0, "{p:?}");

            // The square between the axes is ImGuizmo's; past the ring the click goes through.
            assert_eq!(snap(Handles::Ground, [465.0, 235.0], g), None);
            assert_eq!(snap(Handles::Ground, [400.0 + 130.0 * 0.6, 300.0 + 130.0 * 0.8], g), None);
        });
    }

    #[test]
    fn the_edge_of_the_outer_ring_is_on_it() {
        with(|g| {
            // 120 pixels: on the line, and just past it where the line still looks hit.
            for r in [118.0_f32, 120.0, 124.0] {
                let [x, y] = snap(Handles::Rings, [400.0 + r * 0.6, 300.0 + r * 0.8], g).unwrap();
                assert!((distance([x, y], [400.0, 300.0]) - 120.0).abs() < 1.0, "{r}: {x} {y}");
            }

            assert_eq!(snap(Handles::Rings, [400.0 + 130.0 * 0.6, 300.0 + 130.0 * 0.8], g), None);
        });
    }

    #[test]
    fn inside_the_rings_a_click_goes_to_the_closest_one() {
        // The rings are 120 pixels wide here; the z ring faces the camera (all of it is drawn), y and x are seen edge on.
        with(|g| {
            let p = snap(Handles::Rings, [430.0, 300.0], g).unwrap();
            assert!((distance(p, [400.0, 300.0]) - 120.0).abs() < 2.0 || distance(p, [430.0, 300.0]) < 3.0, "{p:?}");

            let near_the_rim = snap(Handles::Rings, [481.0, 381.0], g).unwrap();
            assert!((distance(near_the_rim, [400.0, 300.0]) - 120.0).abs() < 1.0, "{near_the_rim:?}");
        });
    }
}
