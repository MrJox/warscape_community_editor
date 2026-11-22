//! Lakes (docs/plans/rivers.md, step 5): a lake is the kit's: one `water_plane` model (`SIMPLE_MESH_INSTANCE`), freeform at the water's
//! height and scaled over it, the ground dug under it (docs/re/water.md; the kit's 22 `lakes` tiles: one plane each, unturned, water at
//! -4 .. -15 in the game's units). Drawn as an outline: the plane covers the outline's rectangle, its water just under the outline's lowest
//! ground (so that it does not stand over its shore), the outline kept on it (`wce_lake_outline`). A basin (`wce_lake_basin`, a depth) is
//! dug in the outline by the lake's own height layer (`river_beds.rs`, as a river's bed).

use wce_foundation::math::Float3;

use super::river_links::RIVER_ID_ATTR;
use super::{TileDocument, item_of};

/// The kit's lake model.
pub const WATER_PLANE_KEY: &str = "RigidModels/campaign/settlements/resources/water_plane.rigid_model_v2";

/// The model's bounds in x and z (its units; measured in the editor on `lakes/10x10`): not centred on its origin.
pub const WATER_PLANE_X: [f64; 2] = [-0.414_495_4, 0.908_746_8];
pub const WATER_PLANE_Z: [f64; 2] = [-0.631_774_3, 1.485_413_2];

/// A lake's outline (`x,z;x,z;...`, the editor's metres) and its basin's depth under the water (the editor's metres; missing or 0: none).
pub const OUTLINE_ATTR: &str = "wce_lake_outline";
pub const BASIN_ATTR: &str = "wce_lake_basin";

/// A lake shapes its ground (`1`: the height patch of its layer, its basin dug and its shore raised to keep the water in; `0`: the
/// ground as it is; missing: as its basin's depth says, the lakes made before it).
pub const GROUND_ATTR: &str = "wce_lake_ground";

/// What a new lake's height patch is: its basin's depth (metres) and how far past its outline its shore is raised (metres).
pub const DEFAULT_BASIN_M: f64 = 2.0;
pub const LAKE_SHORE_M: f64 = 12.0;

/// Whether a lake shapes its ground ([`GROUND_ATTR`]).
pub fn shapes_ground(ground: Option<&str>, basin: f64) -> bool {
    match ground.map(str::trim) {
        Some("1") => true,
        Some("0") => false,
        _ => basin > 0.0,
    }
}

/// How far under the outline's lowest ground a lake's water is put (metres), and how far past the outline its plane reaches.
pub const LAKE_FREEBOARD_M: f32 = 0.25;
pub(super) const PLANE_MARGIN_M: f64 = 8.0;

/// A lake's outline read.
pub fn lake_outline(text: &str) -> Vec<[f64; 2]> {
    text.split(';')
        .filter_map(|p| {
            let (x, z) = p.split_once(',')?;
            Some([x.trim().parse().ok()?, z.trim().parse().ok()?])
        })
        .collect()
}

/// A closed outline rounded off (two passes of Chaikin's corner cutting: each edge's quarter and three-quarter points): an area brush's
/// outline follows its grid in steps, which a lake's shore would show.
pub fn smooth_outline(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut pts = points.to_vec();
    for _ in 0..2 {
        let n = pts.len();
        if n < 3 {
            break;
        }

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            out.push((a.0 * 0.75 + b.0 * 0.25, a.1 * 0.75 + b.1 * 0.25));
            out.push((a.0 * 0.25 + b.0 * 0.75, a.1 * 0.25 + b.1 * 0.75));
        }

        pts = out;
    }

    pts
}

/// Whether a point is inside a polygon (even-odd).
pub fn inside(poly: &[[f64; 2]], x: f64, z: f64) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > z) != (b[1] > z) && x < (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }

    c
}

/// The plane over an outline's rectangle (with a margin) at a water height: where its origin goes (world metres) and its scale in x and
/// z (scene units), its lowest corner on the rectangle's (the model is not centred on its origin).
fn plane_for(outline: &[Float3], level: f32, u: f64) -> (Float3, f64, f64) {
    let xs = outline.iter().map(|p| f64::from(p.x));
    let zs = outline.iter().map(|p| f64::from(p.z));
    let (x0, x1) = (xs.clone().fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M, xs.fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M);
    let (z0, z1) = (zs.clone().fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M, zs.fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M);
    let sx = (x1 - x0) * u / (WATER_PLANE_X[1] - WATER_PLANE_X[0]);
    let sz = (z1 - z0) * u / (WATER_PLANE_Z[1] - WATER_PLANE_Z[0]);
    let origin = Float3::new((x0 - WATER_PLANE_X[0] * sx / u) as f32, level, (z0 - WATER_PLANE_Z[0] * sz / u) as f32);
    (origin, sx, sz)
}

fn scale_text(sx: f64, sz: f64) -> String {
    use wce_tile::ted_scene::format_number;
    format!("vector({},1,{})", format_number(sx), format_number(sz))
}

fn outline_text(outline: &[Float3]) -> String {
    use wce_tile::ted_scene::format_number;
    outline.iter().map(|p| format!("{},{}", format_number(f64::from(p.x)), format_number(f64::from(p.z)))).collect::<Vec<_>>().join(";")
}

impl super::Tile {
    /// Whether an element is a lake: a water plane with an outline.
    pub fn is_lake(&self, e: usize) -> bool {
        let sc = self.tscene();
        sc.type_of(e) == "SIMPLE_MESH_INSTANCE" && sc.get(e, OUTLINE_ATTR).is_some()
    }

    /// A paintable area's outline (world metres, x and z): a lake's, else the polygon's points.
    pub fn area_outline(&self, e: usize) -> Vec<(f32, f32)> {
        if self.is_lake(e) {
            return lake_outline(&self.tscene().get(e, OUTLINE_ATTR).unwrap_or_default())
                .into_iter()
                .map(|p| (p[0] as f32, p[1] as f32))
                .collect();
        }

        self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect()
    }
}

impl TileDocument {
    /// The water height a lake drawn on an outline gets: just under the lowest ground along it (the editor's metres).
    pub fn lake_level(&self, outline: &[Float3]) -> Option<f32> {
        let t = self.tile();
        let n = outline.len();
        if n < 3 {
            return None;
        }

        let mut low = f32::INFINITY;
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let steps = ((b.x - a.x).hypot(b.z - a.z) / 2.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                let f = k as f32 / steps as f32;
                low = low.min(t.terrain_height(a.x + (b.x - a.x) * f, a.z + (b.z - a.z) * f));
            }
        }

        Some(low - LAKE_FREEBOARD_M)
    }

    /// A lake on a drawn outline (3 points or more) in `into`: a freeform water plane over the outline's rectangle at `level` (the editor's
    /// metres; none: [`Self::lake_level`]), the outline kept on it, an id for its basin's layer. One undo entry; returns the plane.
    ///
    /// # Errors
    ///
    /// Fewer than 3 points, or the scene could not be edited.
    pub fn create_lake(&mut self, into: usize, outline: &[Float3], level: Option<f32>) -> Result<usize, String> {
        use wce_tile::new_instances::element_text;
        use wce_tile::ted_scene::format_number;

        let Some(level) = level.or_else(|| self.lake_level(outline)) else {
            return Err("draw a lake's outline: 3 points or more".to_owned());
        };
        self.check_structure(&[], Some(into))?;
        let t = self.tile();
        let u = t.unit();
        let (origin, sx, sz) = plane_for(outline, level, u);
        let local = t.to_local(into, origin);
        let mut element = super::edits::point_instance("SIMPLE_MESH_INSTANCE", local, WATER_PLANE_KEY, u)?;
        let id = t.next_river_id();
        for (k, v) in [
            ("scale", scale_text(sx, sz)),
            ("surface_mode", "MSM_FREEFORM".to_owned()),
            ("name", "Lake".to_owned()),
            (OUTLINE_ATTR, outline_text(outline)),
            (GROUND_ATTR, "1".to_owned()),
            (BASIN_ATTR, format_number(DEFAULT_BASIN_M)),
            (RIVER_ID_ATTR, id.to_string()),
        ] {
            element.attrs.insert(k.to_owned(), v);
        }

        self.check_fits([element.kind.as_str()], into)?;
        let text = element_text(&element)?;
        let mark = self.begin()?;
        match self.tscene_mut().insert_element_text(into, &text) {
            Ok(e) => {
                self.push_splice_added("Add lake".to_owned(), mark, vec![], vec![item_of(e)], &[e]);
                Ok(e)
            }

            Err(err) => self.rolled_back(mark, err),
        }
    }

    /// A lake's outline and its plane set to `outline` (world metres), its water where it is: inside an edit begun (`begin`), as the
    /// area brush makes them.
    ///
    /// # Errors
    ///
    /// Fewer than 3 points, or the scene could not be edited.
    pub(super) fn set_lake_outline(&mut self, lake: usize, outline: &[Float3]) -> Result<(), String> {
        use wce_tile::new_instances::vector;
        if outline.len() < 3 {
            return Err("a lake's outline needs 3 points or more".to_owned());
        }

        let t = self.tile();
        let u = t.unit();
        let (origin, sx, sz) = plane_for(outline, self.lake_water(lake), u);
        // The plane's own place, in its parent's frame.
        let pw = super::edits::parent_transform(t.tscene(), lake);
        let local = pw.rotation.transpose() * (super::edits::scene_point(origin, u) - pw.position);
        let sc = self.tscene_mut();
        sc.set_attr(lake, "position", &vector(local));
        sc.set_attr(lake, "scale", &scale_text(sx, sz));
        sc.set_attr(lake, OUTLINE_ATTR, &outline_text(outline));
        Ok(())
    }

    /// A lake's water height (the editor's metres): its plane's.
    pub fn lake_water(&self, lake: usize) -> f32 {
        self.tile().world_position(lake).y
    }

    /// Sets a lake's water height (the editor's metres): its plane moved up or down. One undo entry per `gesture`.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn set_lake_water(&mut self, lake: usize, level: f32, gesture: u64) -> Result<(), String> {
        let at = self.tile().world_position(lake);
        let local = self.tile().local_for_world_y(lake, f64::from(level));
        if (f64::from(at.y) - f64::from(level)).abs() < 1e-6 {
            return Ok(());
        }

        if self.set_local(lake, Some(local), None, gesture) { Ok(()) } else { Err("the lake's plane could not be moved".to_owned()) }
    }

    /// The tile's lakes: its water planes with an outline.
    pub fn lakes(&self) -> Vec<usize> {
        let sc = self.tile().tscene();
        sc.find_all("SIMPLE_MESH_INSTANCE").into_iter().filter(|&e| sc.get(e, OUTLINE_ATTR).is_some()).collect()
    }

    /// Where a lake's water stands over ground outside its outline (it floods past its shore): places of its plane's rectangle outside the
    /// outline whose ground is under its water, every 4 m; how many, and the lowest of them.
    pub fn lake_spills(&self, lake: usize) -> (usize, Option<Float3>) {
        let t = self.tile();
        let poly = lake_outline(&t.tscene().get(lake, OUTLINE_ATTR).unwrap_or_default());
        if poly.len() < 3 {
            return (0, None);
        }

        let water = self.lake_water(lake);
        let (ex, ez) = (t.extent_x(), t.extent_z());
        let x0 = poly.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M;
        let x1 = poly.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M;
        let z0 = poly.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M;
        let z1 = poly.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M;
        let (mut n, mut worst): (usize, Option<(f32, Float3)>) = (0, None);
        let mut z = z0;
        while z <= z1 {
            let mut x = x0;
            while x <= x1 {
                if x >= 0.0 && z >= 0.0 && x <= ex && z <= ez && !inside(&poly, x, z) {
                    let g = t.terrain_height(x as f32, z as f32);
                    if g < water - 0.05 {
                        n += 1;
                        if worst.is_none_or(|w| g < w.0) {
                            worst = Some((g, Float3::new(x as f32, g, z as f32)));
                        }
                    }
                }

                x += 4.0;
            }

            z += 4.0;
        }

        (n, worst.map(|w| w.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brushed_outline_is_rounded_off() {
        let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let round = smooth_outline(&square);
        assert_eq!(round.len(), 16);
        // No corner left: every point inside the square, none on its corners.
        assert!(round.iter().all(|p| p.0 > 0.0 && p.0 < 10.0 || p.1 > 0.0 && p.1 < 10.0));
        assert!(!round.contains(&(0.0, 0.0)));
    }

    #[test]
    fn an_outline_reads_back_and_holds_its_inside() {
        let poly = lake_outline("0,0;10,0;10,10;0,10");
        assert_eq!(poly.len(), 4);
        assert!(inside(&poly, 5.0, 5.0) && !inside(&poly, 11.0, 5.0));
    }
}
