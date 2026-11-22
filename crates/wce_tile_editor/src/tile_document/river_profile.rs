//! A river along its length (docs/plans/rivers.md, step 3): its water and the ground under its middle as the Inspector's long profile
//! shows them, its points' heights set from there (a drag is one undo entry), *Make downhill*, and its points' values ramped between two of
//! them. Heights are the file's (the game's), widths too.

use wce_foundation::math::Double3;

use super::TileDocument;
use super::commands::TedPointAttrsCommand;
use super::river_links::LINK_ATTR;

/// A place along a river's profile: where it is (x, z), how far along (metres), its water, the ground under its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileSample {
    pub x: f64,
    pub z: f64,
    pub along: f64,
    pub water: f64,
    pub ground: f64,
}

/// A river's point on its profile: its index, how far along, its water, and whether it is joined to a link (its height is the link's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfilePoint {
    pub index: usize,
    pub along: f64,
    pub water: f64,
    pub locked: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverProfile {
    pub length: f64,
    pub samples: Vec<ProfileSample>,
    pub points: Vec<ProfilePoint>,

    /// Its fords: each one's middle along it and its length (metres).
    pub fords: Vec<(f64, f64)>,
}

/// The values a ramp sets on a river's points: the attribute and its label (`width` in the file's metres).
pub const RAMPS: [(&str, &str); 4] =
    [("width", "width (m)"), ("flow_speed", "flow speed"), ("foam_amount", "foam"), ("terrain_offset", "terrain offset (m)")];

impl TileDocument {
    /// A river's profile, none when it is not drawn (fewer than 2 points, another variation's).
    pub fn river_profile(&self, river: usize) -> Option<RiverProfile> {
        let t = self.tile();
        let (_, samples) = t.river_ribbons_by_elem().remove(&river)?;
        let mut along = 0.0;
        let mut out = vec![];
        for (i, s) in samples.iter().enumerate() {
            if i > 0 {
                along += (s.x - samples[i - 1].x).hypot(s.z - samples[i - 1].z);
            }

            out.push(ProfileSample { x: s.x, z: s.z, along, water: s.y, ground: f64::from(t.terrain_height(s.x as f32, s.z as f32)) });
        }

        // Each point at the sample nearest it (the ribbon passes through every point).
        let sc = t.tscene();
        let factor = t.river_height_factor(river) as f32;
        let world = t.polygon_world_points(river);
        let points = sc
            .polygon_points(river)
            .iter()
            .zip(&world)
            .enumerate()
            .map(|(index, (&p, w))| {
                let near = samples
                    .iter()
                    .zip(&out)
                    .min_by(|a, b| {
                        (a.0.x - f64::from(w.x))
                            .hypot(a.0.z - f64::from(w.z))
                            .total_cmp(&(b.0.x - f64::from(w.x)).hypot(b.0.z - f64::from(w.z)))
                    })
                    .map_or(0.0, |(_, o)| o.along);
                ProfilePoint {
                    index,
                    along: near,
                    water: f64::from(w.y / factor),
                    locked: sc.get(p, LINK_ATTR).is_some_and(|v| !v.is_empty()),
                }
            })
            .collect();
        let fords = super::river_beds::river_fords(sc.get(river, super::river_beds::FORDS_ATTR).as_deref().unwrap_or_default());
        Some(RiverProfile { length: along, samples: out, points, fords })
    }

    /// Sets river points' water heights (the file's metres), their handles moving with them: one undo entry per `gesture` (a drag's steps
    /// merge).
    ///
    /// # Errors
    ///
    /// Not a river, or no such point.
    pub fn set_river_heights(&mut self, river: usize, heights: &[(usize, f64)], name: &str, gesture: u64) -> Result<(), String> {
        use wce_tile::new_instances::point_text;

        let t = self.tile();
        let sc = t.tscene();
        if sc.type_of(river) != "SplineRiverInstance" {
            return Err("not a river".to_owned());
        }

        let points = sc.polygon_points(river);
        let factor = t.river_height_factor(river);
        let unit = t.unit();
        let world = t.polygon_world_points(river);
        let mut steps = vec![];
        for &(i, y) in heights {
            let (Some(&p), Some(w)) = (points.get(i), world.get(i)) else { return Err(format!("no point {i}")) };
            // The local change of a world one (the instance turned only about y: heights go straight through).
            let dy = (y * factor - f64::from(w.y)) * unit;
            if dy.abs() < 1e-9 {
                continue;
            }

            for attr in ["position", "controlIn", "controlOut"] {
                let Some(text) = sc.get(p, attr) else { continue };
                let Some(v) = wce_tile::ted_scene::parse_vec3(&text, "") else { continue };
                // An unused handle stays 0,0,0.
                if attr != "position" && v == Double3::ZERO {
                    continue;
                }

                steps.push((i, attr.to_owned(), text, point_text(v + Double3::new(0.0, dy, 0.0))));
            }
        }

        if steps.is_empty() {
            return Ok(());
        }

        self.end_stroke();
        self.history.push(Box::new(TedPointAttrsCommand { name: name.to_owned(), elem: river, steps, gesture }));
        Ok(())
    }

    /// Makes a river's water run downhill: from its higher end to its lower one, no point higher than the one before it. Points joined
    /// to a link keep their height; the others between are evened out to them (a river between two links at the same height runs level).
    /// One undo entry; returns how many points moved.
    ///
    /// # Errors
    ///
    /// Not a river.
    pub fn make_river_downhill(&mut self, river: usize) -> Result<usize, String> {
        let Some(p) = self.river_profile(river) else { return Err("not a river drawn".to_owned()) };
        let mut y = p.points.iter().map(|q| q.water).collect::<Vec<_>>();
        let locked = p.points.iter().map(|q| q.locked).collect::<Vec<_>>();
        let n = y.len();
        if n < 2 {
            return Ok(0);
        }

        // Along the flow: from the higher end.
        let order = if y[0] >= y[n - 1] { (0..n).collect::<Vec<_>>() } else { (0..n).rev().collect() };
        for w in order.windows(2) {
            if !locked[w[1]] {
                y[w[1]] = y[w[1]].min(y[w[0]]);
            }
        }

        for w in order.windows(2).rev() {
            if !locked[w[0]] {
                y[w[0]] = y[w[0]].max(y[w[1]]);
            }
        }

        let moves = p.points.iter().zip(&y).filter(|(q, y)| (q.water - **y).abs() > 1e-6).map(|(q, &y)| (q.index, y)).collect::<Vec<_>>();
        if moves.is_empty() {
            return Ok(0);
        }

        let g = self.next_gesture();
        self.set_river_heights(river, &moves, "Make river downhill", g)?;
        Ok(moves.len())
    }

    /// Ramps a value over a river's points `from` ..= `to` (either order), from `a` at the first to `b` at the last, by their distance
    /// along the river. `width` is in the file's metres (the game's). One undo entry.
    ///
    /// # Errors
    ///
    /// Not a river, an attribute no ramp sets, or no such points.
    pub fn ramp_river(&mut self, river: usize, attr: &str, from: usize, to: usize, a: f64, b: f64) -> Result<(), String> {
        use wce_tile::ted_scene::format_number;

        if !RAMPS.iter().any(|r| r.0 == attr) {
            return Err(format!("no ramp of {attr}"));
        }

        let Some(p) = self.river_profile(river) else { return Err("not a river drawn".to_owned()) };
        let (lo, hi) = (from.min(to), from.max(to));
        if hi >= p.points.len() || lo == hi {
            return Err("pick two different points of the river".to_owned());
        }

        let (start, end) = (p.points[from].along, p.points[to].along);
        let scale = if attr == "width" { 1.0 / self.tile().river_width_factor(river) } else { 1.0 };
        let sc = self.tile().tscene();
        let points = sc.polygon_points(river);
        let mut steps = vec![];
        for q in &p.points[lo..=hi] {
            let t = if (end - start).abs() > 1e-9 { ((q.along - start) / (end - start)).clamp(0.0, 1.0) } else { 0.0 };
            let v = format_number((a + (b - a) * t) * scale);
            let old = sc.get(points[q.index], attr).unwrap_or_default();
            if old != v {
                steps.push((q.index, attr.to_owned(), old, v));
            }
        }

        if steps.is_empty() {
            return Ok(());
        }

        self.end_stroke();
        let g = self.next_gesture();
        self.history.push(Box::new(TedPointAttrsCommand { name: format!("Ramp river {attr}"), elem: river, steps, gesture: g }));
        Ok(())
    }
}
