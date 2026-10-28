//! The shadow cascades' light fit, as the game's ShadowMapView does it (fn 0x117329a0, with the light basis of fn 0x11758dd0): a tight
//! light-space box around the slice's corners, pulled back towards the sun, warped by a light-space perspective (LiSPSM) when the view
//! is not along the light (`warp_shadows`, default on).

use wce_foundation::math::{Float3, Float4x4, cross, dot, length, normalize};

/// `g_battle_terrain_on_shadow_cam_depth_offset_battle`: how far the light camera is pulled back behind the slice (a battle with its
/// terrain on; 150 m without, 14 m on the campaign map).
pub(crate) const DEPTH_OFFSET: f32 = 5000.0;

/// The fit of one slice: the light's view-projection (clip x, y in -1..1, depth 0..1) and the width of the light box (metres).
pub(crate) struct LightFit {
    pub view_proj: Float4x4,
    pub width: f32,
}

/// The light-space axes (fn 0x11758dd0): x = view x light, y = light x x (the view direction made perpendicular to the light), z = the
/// light's direction. With the view (nearly) along the light, `side` (the camera's side vector) takes the view's place.
fn light_basis(light: Float3, view: Float3, side: Float3) -> [Float3; 3] {
    let d = dot(view, light);
    if d.abs() <= 0.999 {
        let x = normalize(cross(view, light));
        let y = normalize(cross(light, x));
        [x, y, light]
    } else {
        let s = if d >= 0.0 { 1.0 } else { -1.0 };
        let y = normalize(cross(light, side) * s);
        let x = normalize(cross(y, light));
        [x, y, light]
    }
}

/// A matrix row `(a, w)`: `a . p + w`.
fn row(a: Float3, w: f32) -> [f32; 4] {
    [a.x, a.y, a.z, w]
}

/// `corners`: the slice's eight corners (world); `light`: the direction the light travels; `view`: the camera's view direction; `side`:
/// its side vector; `near`: the slice's near distance.
pub(crate) fn fit(corners: &[Float3; 8], light: Float3, view: Float3, side: Float3, near: f32) -> LightFit {
    let [ax, ay, az] = light_basis(light, view, side);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in corners {
        for (k, a) in [ax, ay, az].into_iter().enumerate() {
            let v = dot(*p, a);
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }

    let depth = DEPTH_OFFSET + hi[2] - lo[2];
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    let (cx, cy) = ((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5);
    let eye = ax * cx + ay * cy + az * (lo[2] - DEPTH_OFFSET);

    // Not warped: the box's ortho projection (x, y centred, depth from the pulled-back eye).
    let warp = dot(light, view).abs() < 0.999;
    if !warp {
        let mut m = Float4x4::IDENTITY;
        m.m[0] = row(ax * (2.0 / w), -dot(ax, eye) * (2.0 / w));
        m.m[1] = row(ay * (2.0 / h), -dot(ay, eye) * (2.0 / h));
        m.m[2] = row(az * (1.0 / depth), -dot(az, eye) / depth);
        return LightFit { view_proj: m, width: w };
    }

    // The perspective's near distance (`warp_distance` 0: LiSPSM's optimum for the slice) and its centre, that far in front of the
    // box's near y face, half way down the box.
    let sin_gamma = (1.0 - dot(view, az).powi(2)).max(0.0).sqrt().max(1e-6);
    let n = (((sin_gamma * depth + near) * near).sqrt() + near) / sin_gamma;
    let c = eye + az * (depth * 0.5) - ay * (h * 0.5) - ay * n;

    // The half angle of the corners around the y axis (`warp_angle` 0: measured).
    let (mut amin, mut amax) = (f32::MAX, f32::MIN);
    for p in corners {
        let d = *p - c;
        let perp = d - az * dot(az, d);
        let len = length(perp);
        if len <= 0.0 {
            continue;
        }

        let perp = perp * (1.0 / len);
        let sign = if dot(perp, ax) < 0.0 { -1.0 } else { 1.0 };
        let a = dot(perp, ay).clamp(-1.0, 1.0).acos() * sign;
        amin = amin.min(a);
        amax = amax.max(a);
    }

    let theta = (-amin).max(amax);
    let f = h + n;
    let sx = 1.0 / theta.sin();
    let sy = 2.0 * n / depth;
    let sz = f / (f - n);

    // The perspective (looking down y from c, -z up) and the game's remap of its rows (clip y = the perspective depth, clip depth = the
    // height across the light, 0.5 - 0.5 * r1 / w).
    let r0 = row(ax * sx, -dot(ax, c) * sx);
    let r1 = row(az * -sy, dot(az, c) * sy);
    let r2 = row(ay * sz, f * n / (n - f) - dot(ay, c) * sz);
    let r3 = row(ay, -dot(ay, c));
    let mut m = Float4x4::IDENTITY;
    m.m[0] = r0;
    m.m[1] = std::array::from_fn(|k| 2.0 * r2[k] - r3[k]);
    m.m[2] = std::array::from_fn(|k| 0.5 * r3[k] - 0.5 * r1[k]);
    m.m[3] = r3;
    LightFit { view_proj: m, width: w }
}

/// Rome 2's fit (fn 0x11423af0, its light basis with `shadowmap_view_quantize` on in fn 0x114765e0): no warp. The light axes are x =
/// (L.z, 0, -L.x) (horizontal), y = L x x, z = L; the box is a square as wide as the slice's longest corner to corner distance (rounded
/// up, plus two texels), its centre snapped to whole texels of a `res` wide tile; its depth is the slice's, at least
/// `shadowmap_min_depth` (1024), ending at the slice's far side, the near plane `shadowmap_near_clip` (1) closer to the sun. Units are the
/// corners' (the game: game units). Clip x, y in -1..1, depth 0..1.
pub(crate) fn fit_rome2(corners: &[Float3; 8], light: Float3, res: f32) -> LightFit {
    let ax = normalize(Float3::new(light.z, 0.0, -light.x));
    let ay = normalize(cross(light, ax));
    let az = light;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in corners {
        for (k, a) in [ax, ay, az].into_iter().enumerate() {
            let v = dot(*p, a);
            lo[k] = lo[k].min(v);
            hi[k] = hi[k].max(v);
        }
    }

    let mut span = 0.0f32;
    for p in corners {
        for q in corners {
            span = span.max(length(*p - *q));
        }
    }

    let span = span.ceil();
    let size = span + 2.0 * span / res;
    let texel = size / res;
    let depth = (hi[2] - lo[2]).max(1024.0);
    let zs = hi[2] - depth;
    let cx = ((lo[0] + hi[0]) * 0.5 / texel).floor() * texel;
    let cy = ((lo[1] + hi[1]) * 0.5 / texel).floor() * texel;

    // The projection's near bias (the default depth format's): depth = (p . L - zs + nb) / depth.
    let nb = 6.0e-7;
    let mut m = Float4x4::IDENTITY;
    m.m[0] = row(ax * (2.0 / size), -cx * (2.0 / size));
    m.m[1] = row(ay * (2.0 / size), -cy * (2.0 / size));
    m.m[2] = row(az * (1.0 / depth), (nb - zs) / depth);
    LightFit { view_proj: m, width: size }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rome 2's box: every corner inside, w 1, and the centre on the texel grid.
    #[test]
    fn rome2_corners_fit() {
        let light = normalize(Float3::new(0.8, -0.4, -0.44));
        let view = normalize(Float3::new(-0.7, -0.5, -0.4));
        let corners = slice(Float3::new(-1225.0, 77.0, 1283.0), view, 15.0, 50.0);
        let fit = fit_rome2(&corners, light, 1024.0);
        assert_eq!(fit.view_proj.m[3], [0.0, 0.0, 0.0, 1.0]);
        for p in corners {
            let [x, y, z, _] = clip(&fit.view_proj, p);
            assert!(x.abs() <= 1.0 && y.abs() <= 1.0 && (0.0..=1.0).contains(&z), "{x} {y} {z}");
        }

        let texel = fit.width / 1024.0;
        let cx = -fit.view_proj.m[0][3] * fit.width / 2.0;
        assert!((cx / texel - (cx / texel).round()).abs() < 1e-2);
    }

    fn clip(m: &Float4x4, p: Float3) -> [f32; 4] {
        std::array::from_fn(|r| m.m[r][0] * p.x + m.m[r][1] * p.y + m.m[r][2] * p.z + m.m[r][3])
    }

    fn slice(eye: Float3, view: Float3, near: f32, far: f32) -> [Float3; 8] {
        let side = normalize(cross(Float3::new(0.0, 1.0, 0.0), view));
        let up = cross(view, side);
        let (tx, ty) = (0.7f32, 0.4f32);
        std::array::from_fn(|k| {
            let d = if k & 4 != 0 { far } else { near };
            let sx = if k & 1 != 0 { 1.0 } else { -1.0 };
            let sy = if k & 2 != 0 { 1.0 } else { -1.0 };
            eye + view * d + side * (sx * tx * d) + up * (sy * ty * d)
        })
    }

    /// The structure of the dumped g_amHardSplit (docs/re/renderer.md): the w row is a unit vector across the light, the y row's
    /// direction is parallel to it, and the depth row minus half of it is along the light.
    #[test]
    fn rows_have_the_dumped_structure() {
        let light = normalize(Float3::new(-0.882881, -0.438358, 0.168413));
        let view = normalize(Float3::new(0.3, -0.35, 0.89));
        let eye = Float3::new(100.0, 80.0, -50.0);
        let side = normalize(cross(Float3::new(0.0, 1.0, 0.0), view));
        let fit = fit(&slice(eye, view, 10.0, 130.0), light, view, side, 10.0);
        let m = fit.view_proj.m;
        let r3 = Float3::new(m[3][0], m[3][1], m[3][2]);
        assert!((length(r3) - 1.0).abs() < 1e-4);
        assert!(dot(r3, light).abs() < 1e-4);
        let r1 = Float3::new(m[1][0], m[1][1], m[1][2]);
        assert!(length(cross(normalize(r1), r3)) < 1e-3);
        let r2 = Float3::new(m[2][0], m[2][1], m[2][2]) - r3 * 0.5;
        assert!(length(cross(normalize(r2), light)) < 1e-3);
    }

    /// Every corner lands in front of the projection, inside the depth range and the y range; x may pass 1 by the secant of the half
    /// angle, as the game's 1 / sin scale gives.
    #[test]
    fn corners_fit() {
        let light = normalize(Float3::new(0.3, -0.8, 0.5));
        let view = normalize(Float3::new(0.0, -0.4, 1.0));
        let side = normalize(cross(Float3::new(0.0, 1.0, 0.0), view));
        let corners = slice(Float3::new(0.0, 50.0, 0.0), view, 5.0, 200.0);
        let fit = fit(&corners, light, view, side, 5.0);
        for p in corners {
            let [x, y, z, w] = clip(&fit.view_proj, p);
            assert!(w > 0.0);
            assert!((-1.001..=1.001).contains(&(y / w)), "y {}", y / w);
            assert!((-0.001..=1.001).contains(&(z / w)), "z {}", z / w);
            assert!((x / w).abs() < 1.2, "x {}", x / w);
        }
    }

    /// Looking straight along the light: no warp, a plain ortho box.
    #[test]
    fn along_the_light_is_ortho() {
        let light = Float3::new(0.0, -1.0, 0.0);
        let view = light;
        let side = Float3::new(1.0, 0.0, 0.0);
        let corners = slice(Float3::new(0.0, 500.0, 0.0), Float3::new(0.0, -1.0, 0.0001), 10.0, 100.0);
        let fit = fit(&corners, light, view, side, 10.0);
        assert_eq!(fit.view_proj.m[3], [0.0, 0.0, 0.0, 1.0]);
        for p in corners {
            let [x, y, z, _] = clip(&fit.view_proj, p);
            assert!(x.abs() <= 1.0001 && y.abs() <= 1.0001 && (0.0..=1.0001).contains(&z));
        }
    }
}
