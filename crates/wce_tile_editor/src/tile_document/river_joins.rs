//! Rivers meeting rivers (docs/plans/rivers.md, step 4): a river split in two at a point, two rivers whose ends meet joined into one, and
//! a tributary: a river's end on another river's middle line, at its water. Their beds merge by themselves (each only cuts).

use wce_foundation::math::{Double3, Float3};

use super::river_links::{LINK_ATTR, RIVER_ID_ATTR};
use super::{TileDocument, item_of};

/// How close two rivers' ends are to be joined (metres).
pub const JOIN_REACH_M: f32 = 2.0;

impl TileDocument {
    /// Splits a river at a point (not an end): the river keeps its points up to it, a copy of it (its settings, a new id) takes the point
    /// and those after it. One undo entry; returns the new river.
    ///
    /// # Errors
    ///
    /// Not a river, an end, or the scene could not be edited.
    pub fn split_river(&mut self, river: usize, at: usize) -> Result<usize, String> {
        let t = self.tile();
        let sc = t.tscene();
        if sc.type_of(river) != "SplineRiverInstance" {
            return Err("not a river".to_owned());
        }

        let n = sc.polygon_points(river).len();
        if at == 0 || at + 1 >= n {
            return Err("split at a point between its ends".to_owned());
        }

        let id = t.next_river_id();
        self.end_stroke();
        let mark = self.begin()?;
        let copy = match self.tscene_mut().duplicate(river) {
            Ok(c) => c,
            Err(e) => return self.rolled_back(mark, e),
        };

        // The copy (after the original in the text): from `at`, its first one's inner handle unused; then the original: its points up to
        // `at`, its last one's outer handle unused. Its points going move the copy's element: found again after it.
        let sc = self.tscene_mut();
        for _ in 0..at {
            if let Err(e) = sc.remove_polygon_point(copy, 0) {
                return self.rolled_back(mark, e);
            }
        }

        for i in (at + 1..n).rev() {
            if let Err(e) = sc.remove_polygon_point(river, i) {
                return self.rolled_back(mark, e);
            }
        }

        let Some(copy) = sc.find_all("SplineRiverInstance").into_iter().find(|&e| e > river) else {
            return self.rolled_back(mark, "the copy went missing");
        };

        let (last, first) = (sc.polygon_points(river)[at], sc.polygon_points(copy)[0]);
        sc.set_attr(last, "controlOut", "0,0,0");
        sc.set_attr(first, "controlIn", "0,0,0");
        // The point split at belongs to no link (the river's ends at the edge do).
        sc.set_attr(last, LINK_ATTR, "");
        sc.set_attr(first, LINK_ATTR, "");
        if sc.get(copy, RIVER_ID_ATTR).is_some() {
            sc.set_attr(copy, RIVER_ID_ATTR, &id.to_string());
        }

        let name = sc.name(river);
        sc.set_attr(copy, "name", &format!("{name} 2"));
        self.push_splice_structure("Split river".to_owned(), mark, vec![item_of(river)], vec![item_of(copy)]);
        Ok(copy)
    }

    /// Joins two rivers whose ends meet (within [`JOIN_REACH_M`]) into the first: the second's points after the shared end are added to
    /// it (either river turned round as needed), and the second goes. One undo entry.
    ///
    /// # Errors
    ///
    /// Not two rivers, their ends do not meet, or the scene could not be edited.
    pub fn join_rivers(&mut self, a: usize, b: usize) -> Result<(), String> {
        use wce_tile::new_instances::point_text;

        let t = self.tile();
        let sc = t.tscene();
        if a == b || sc.type_of(a) != "SplineRiverInstance" || sc.type_of(b) != "SplineRiverInstance" {
            return Err("select two rivers".to_owned());
        }

        let (pa, pb) = (t.polygon_world_points(a), t.polygon_world_points(b));
        let (Some(&a0), Some(&a1), Some(&b0), Some(&b1)) = (pa.first(), pa.last(), pb.first(), pb.last()) else {
            return Err("a river without points".to_owned());
        };
        let near = |p: Float3, q: Float3| (p.x - q.x).hypot(p.z - q.z) <= JOIN_REACH_M;
        // Which ends meet: a's last and b's first is the join; the others turn a river round first.
        let (flip_a, flip_b) = if near(a1, b0) {
            (false, false)
        } else if near(a1, b1) {
            (false, true)
        } else if near(a0, b0) {
            (true, false)
        } else if near(a0, b1) {
            (true, true)
        } else {
            return Err(format!("their ends do not meet (within {JOIN_REACH_M} m)"));
        };

        use wce_editor_core::document::Document;
        let before = self.history().position();
        if flip_a {
            self.flip_spline(a)?;
        }

        if flip_b {
            self.flip_spline(b)?;
        }

        // b's points after its first, in a's frame: positions and handles through the world, the other values as they are.
        let t = self.tile();
        let sc = t.tscene();
        let (wa, wb) = (wce_tile::world_transform::world_transform(sc, a), wce_tile::world_transform::world_transform(sc, b));
        let to_a = |v: Double3| wa.rotation.transpose() * (wb.position + wb.rotation * v - wa.position);
        let b_points = sc.polygon_points(b);
        let b_attrs = b_points
            .iter()
            .map(|&p| {
                let doc = sc.doc();
                doc.elements()[p].attrs.iter().filter_map(|a| Some((a.name.clone(), doc.get(p, &a.name)?))).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let a_count = sc.polygon_points(a).len();
        let rank = sc.find_all("SplineRiverInstance").iter().position(|&r| r == a).unwrap_or(0);
        let a_last = *sc.polygon_points(a).last().expect("points");
        let b_first_out = sc.get(b_points[0], "controlOut");
        self.end_stroke();
        let mark = self.begin()?;
        let sc = self.tscene_mut();
        // The later of the two first, so that the other's element stays where it is: b removed, or a's points added.
        if b > a
            && let Err(e) = sc.remove(b)
        {
            return self.rolled_back(mark, e);
        }

        // The shared end takes b's outer handle (a's last point is inside now).
        if let Some(h) = b_first_out.and_then(|v| wce_tile::ted_scene::parse_vec3(&v, "")).filter(|v| *v != Double3::ZERO) {
            sc.set_attr(a_last, "controlOut", &point_text(to_a(h)));
        }

        sc.set_attr(a_last, LINK_ATTR, "");
        for (k, attrs) in b_attrs.iter().enumerate().skip(1) {
            let at = a_count + k - 2;
            let pos =
                attrs.iter().find(|(n, _)| n == "position").and_then(|(_, v)| wce_tile::ted_scene::parse_vec3(v, "")).unwrap_or_default();
            if let Err(e) = sc.insert_polygon_point(a, at, to_a(pos)) {
                return self.rolled_back(mark, e);
            }

            let p = sc.polygon_points(a)[at + 1];
            for (name, v) in attrs {
                match name.as_str() {
                    "position" => {}
                    "controlIn" | "controlOut" => {
                        let h = wce_tile::ted_scene::parse_vec3(v, "").unwrap_or_default();
                        sc.set_attr(p, name, &if h == Double3::ZERO { "0,0,0".to_owned() } else { point_text(to_a(h)) });
                    }
                    _ => sc.set_attr(p, name, v),
                }
            }
        }

        if b < a
            && let Err(e) = sc.remove(b)
        {
            return self.rolled_back(mark, e);
        }

        // a found again among the rivers (b before it, it moved up).
        let rank = rank - usize::from(b < a);
        let Some(a) = sc.find_all("SplineRiverInstance").get(rank).copied() else {
            return self.rolled_back(mark, "the joined river went missing");
        };
        self.push_splice_structure("Join rivers".to_owned(), mark, vec![item_of(a), item_of(b)], vec![item_of(a)]);
        let n = self.history().position() - before;
        if n > 1 {
            self.join_last(n, "Join rivers");
        }

        Ok(())
    }

    /// Where a river's point goes onto another river (a tributary): the nearest place on another river's middle line within half its
    /// width of `p`, at its water; none when no river is that near. `skip`: the river being edited.
    pub fn river_join_point(&self, p: Float3, skip: Option<usize>) -> Option<Float3> {
        let t = self.tile();
        let mut best: Option<(f64, Float3)> = None;
        for (e, (_, s)) in t.river_ribbons_by_elem() {
            if Some(e) == skip {
                continue;
            }

            for w in s.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let l2 = dx * dx + dz * dz;
                let (px, pz) = (f64::from(p.x), f64::from(p.z));
                let f = if l2 > 1e-12 { (((px - a.x) * dx + (pz - a.z) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let (x, z) = (a.x + dx * f, a.z + dz * f);
                let d = (px - x).hypot(pz - z);
                let half = (a.width + (b.width - a.width) * f) * 0.5;
                if d <= half && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, Float3::new(x as f32, (a.y + (b.y - a.y) * f) as f32, z as f32)));
                }
            }
        }

        best.map(|(_, q)| q)
    }

    /// Where a bridge goes over a river at `p`: the nearest place on a river's middle line within half its width (at its water), and the
    /// way the river runs there (radians, from +x towards +z); none away from rivers.
    pub fn river_crossing(&self, p: Float3) -> Option<(Float3, f64)> {
        let t = self.tile();
        let mut best: Option<(f64, Float3, f64)> = None;
        for (_, (_, s)) in t.river_ribbons_by_elem() {
            for w in s.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let l2 = dx * dx + dz * dz;
                if l2 < 1e-12 {
                    continue;
                }

                let (px, pz) = (f64::from(p.x), f64::from(p.z));
                let f = (((px - a.x) * dx + (pz - a.z) * dz) / l2).clamp(0.0, 1.0);
                let (x, z) = (a.x + dx * f, a.z + dz * f);
                let d = (px - x).hypot(pz - z);
                if d <= (a.width + (b.width - a.width) * f) * 0.5 && best.is_none_or(|(bd, ..)| d < bd) {
                    best = Some((d, Float3::new(x as f32, (a.y + (b.y - a.y) * f) as f32, z as f32), dz.atan2(dx)));
                }
            }
        }

        best.map(|(_, q, dir)| (q, dir))
    }

    /// After a river's end was put on another river (`river_join_point`, where the live snap puts it): the end at that river's water.
    /// One undo entry, none when nothing changes; whether it did.
    ///
    /// # Errors
    ///
    /// The scene could not be edited.
    pub fn settle_river_on_rivers(&mut self, river: usize) -> Result<bool, String> {
        let t = self.tile();
        let pts = t.polygon_world_points(river);
        if pts.len() < 2 {
            return Ok(false);
        }

        let factor = t.river_height_factor(river);
        let mut heights = vec![];
        for i in [0, pts.len() - 1] {
            let at = pts[i];
            if let Some(q) = self.river_join_point(at, Some(river)).filter(|q| (q.x - at.x).hypot(q.z - at.z) < 0.5) {
                // The other river's drawn water (its file's) in this river's file's.
                let y = f64::from(q.y);
                if (f64::from(at.y) / factor - y).abs() > 1e-3 {
                    heights.push((i, y));
                }
            }
        }

        if heights.is_empty() {
            return Ok(false);
        }

        let g = self.next_gesture();
        self.set_river_heights(river, &heights, "Join river to river", g)?;
        Ok(true)
    }
}
