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

/// How far under the outline's lowest ground a lake's water is put (metres), and how far past the outline its plane reaches.
pub const LAKE_FREEBOARD_M: f32 = 0.25;
const PLANE_MARGIN_M: f64 = 8.0;

/// A lake's outline read.
pub fn lake_outline(text: &str) -> Vec<[f64; 2]> {
    text.split(';')
        .filter_map(|p| {
            let (x, z) = p.split_once(',')?;
            Some([x.trim().parse().ok()?, z.trim().parse().ok()?])
        })
        .collect()
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
        let xs = outline.iter().map(|p| f64::from(p.x));
        let zs = outline.iter().map(|p| f64::from(p.z));
        let (x0, x1) = (xs.clone().fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M, xs.fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M);
        let (z0, z1) = (zs.clone().fold(f64::INFINITY, f64::min) - PLANE_MARGIN_M, zs.fold(f64::NEG_INFINITY, f64::max) + PLANE_MARGIN_M);

        // The model's x and z over the rectangle: its scale (scene units), its origin where its lowest corner lands on the rectangle's.
        let sx = (x1 - x0) * u / (WATER_PLANE_X[1] - WATER_PLANE_X[0]);
        let sz = (z1 - z0) * u / (WATER_PLANE_Z[1] - WATER_PLANE_Z[0]);
        let origin = Float3::new((x0 - WATER_PLANE_X[0] * sx / u) as f32, level, (z0 - WATER_PLANE_Z[0] * sz / u) as f32);
        let local = t.to_local(into, origin);
        let mut element = super::edits::point_instance("SIMPLE_MESH_INSTANCE", local, WATER_PLANE_KEY, u)?;
        let text_of = |p: &Float3| format!("{},{}", format_number(f64::from(p.x)), format_number(f64::from(p.z)));
        let id = t.next_river_id();
        for (k, v) in [
            ("scale", format!("vector({},1,{})", format_number(sx), format_number(sz))),
            ("surface_mode", "MSM_FREEFORM".to_owned()),
            ("name", "Lake".to_owned()),
            (OUTLINE_ATTR, outline.iter().map(text_of).collect::<Vec<_>>().join(";")),
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
        let (ex, ez) = (t.extent_x() as f64, t.extent_z() as f64);
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
    fn an_outline_reads_back_and_holds_its_inside() {
        let poly = lake_outline("0,0;10,0;10,10;0,10");
        assert_eq!(poly.len(), 4);
        assert!(inside(&poly, 5.0, 5.0) && !inside(&poly, 11.0, 5.0));
    }
}
