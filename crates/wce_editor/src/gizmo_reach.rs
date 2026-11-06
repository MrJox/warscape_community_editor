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

/// `mPlaneLimit`: an axis pointing at the camera, shorter than this in clip space, is not drawn.
const AXIS_LIMIT: f32 = 0.02;

/// The points a ring is measured at: closer than ImGuizmo's 8 pixels on any ring that fits the view.
const RING_SAMPLES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handles {
    /// Three axes: the translate and scale gizmos.
    Axes,

    /// Three rings and the outer one that turns about the view.
    Rings,
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

/// Where to show ImGuizmo a mouse at `mouse`: the closest point of the closest handle, when the mouse is inside the gizmo.
pub fn snap(handles: Handles, mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    match handles {
        Handles::Axes => snap_axes(mouse, g),
        Handles::Rings => snap_rings(mouse, g),
    }
}

fn snap_axes(mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    let length = g.axis_length()?;
    let centre = (g.project)(g.centre)?;
    let (mut best, mut reach): (Option<(f32, [f32; 2])>, f32) = (None, 0.0);
    for axis in g.axes {
        // An axis points to the side that is longer on screen (`mulAxis`).
        let plus = g.clip_length(g.centre, g.centre + axis)?;
        let minus = g.clip_length(g.centre, g.centre - axis)?;
        let d = if plus < minus { -axis } else { axis };
        if g.clip_length(g.centre, g.centre + d * length)? <= AXIS_LIMIT {
            continue;
        }

        let (a, b) = ((g.project)(g.centre + d * (length * AXIS_START))?, (g.project)(g.centre + d * length)?);
        reach = reach.max(distance(centre, b));
        let p = on_segment(mouse, a, b);
        let dist = distance(mouse, p);
        if best.is_none_or(|(n, _)| dist < n) {
            best = Some((dist, p));
        }
    }

    best.filter(|_| distance(mouse, centre) <= reach).map(|(_, p)| p)
}

fn snap_rings(mouse: [f32; 2], g: &Gizmo) -> Option<[f32; 2]> {
    let radius = g.axis_length()? * RING;
    let centre = (g.project)(g.centre)?;
    let mut outer = OUTER_RING * g.view[1];
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
            outer = outer.max(distance(centre, p));
            let dist = distance(mouse, p);
            if best.is_none_or(|(n, _)| dist < n) {
                best = Some((dist, p));
            }
        }
    }

    let from_centre = distance(mouse, centre);
    if from_centre > outer {
        return None;
    }

    if from_centre > 1e-3 && best.is_none_or(|(n, _)| outer - from_centre < n) {
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
            project: &project, view: [800.0, 600.0], centre: Float3::default(), axes: [Float3::new(1.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0), Float3::new(0.0, 0.0, 1.0)],
            right: Float3::new(1.0, 0.0, 0.0), forward: Float3::new(0.0, 0.0, 1.0), size: 0.25,
        };

        f(&g)
    }

    #[test]
    fn inside_the_axes_a_click_goes_to_the_closest_one() {
        // An axis is 0.25 of the view's half width: 100 pixels.
        with(|g| {
            assert_eq!(snap(Handles::Axes, [460.0, 290.0], g), Some([460.0, 300.0]), "the x axis, 10 pixels up");
            assert_eq!(snap(Handles::Axes, [410.0, 240.0], g), Some([400.0, 240.0]), "the y axis");
        });
    }

    #[test]
    fn outside_its_reach_a_click_goes_through() {
        with(|g| {
            assert_eq!(snap(Handles::Axes, [520.0, 300.0], g), None);
            assert_eq!(snap(Handles::Rings, [540.0, 300.0], g), None);
        });
    }

    #[test]
    fn an_axis_at_the_camera_is_not_a_handle() {
        // The z axis has no length on this screen: a click at the centre's side goes to x, not to it.
        with(|g| {
            let p = snap(Handles::Axes, [430.0, 298.0], g).unwrap();
            assert_eq!(p, [430.0, 300.0]);
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
