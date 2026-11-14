//! Picking: the instances in a grid of ground cells for the ray test of a click or a hover, and the ray against the terrain.

use wce_foundation::math::{Float3, dot};
use wce_foundation::wce_prof;

use super::{PickGrid, Tile};

impl Tile {
    /// The polygon or line a click on the ground at `at` (x, z) picks, the last resort of a click (after the models and the markers): the
    /// one whose drawn outline (`polygon_outline`) is nearest within `reach` metres, else the smallest closed one it is inside. Shown and
    /// selectable ones only, as the polygons are drawn (the display's Polygons on, their type not hidden). So a holder's shapes (a siege
    /// AI node's Bounding Box and Boundary, a deployment zone's regions) are picked by a click on them.
    pub fn pick_polygon(&self, at: Float3, reach: f32) -> Option<usize> {
        wce_prof!("tile.pick_polygon");
        if !self.display.show_polygons {
            return None;
        }

        let (x, z) = (at.x, at.z);
        let mut near: Option<(f32, usize)> = None;
        let mut inside: Option<(f32, usize)> = None;
        for &e in self.instances.iter().filter(|&&e| self.is_polygon(e) && self.shown(e) && self.selectable(e)) {
            if self.display.hidden_types.contains(self.tscene().type_of(e)) {
                continue;
            }

            let (pts, closed) = self.polygon_outline(e);
            let n = pts.len();
            if n < 2 {
                continue;
            }

            // The outline's bounding box, grown by the reach: most polygons are far from the click.
            let (lo, hi) = pts
                .iter()
                .fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p.x), lo[1].min(p.z)], [hi[0].max(p.x), hi[1].max(p.z)]));
            if x < lo[0] - reach || x > hi[0] + reach || z < lo[1] - reach || z > hi[1] + reach {
                continue;
            }

            let segments = if closed && n > 2 { n } else { n - 1 };
            let mut d = f32::MAX;
            let mut crossings = false;
            for s in 0..segments {
                let (a, b) = (pts[s], pts[(s + 1) % n]);
                let (dx, dz) = (b.x - a.x, b.z - a.z);
                let l2 = dx * dx + dz * dz;
                let t = if l2 > 1e-12 { (((x - a.x) * dx + (z - a.z) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                d = d.min((x - a.x - dx * t).hypot(z - a.z - dz * t));
                if closed && (a.z > z) != (b.z > z) && x < a.x + (z - a.z) / (b.z - a.z) * dx {
                    crossings = !crossings;
                }
            }

            if d <= reach && near.is_none_or(|(nd, _)| d < nd) {
                near = Some((d, e));
            }

            if crossings {
                let area = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].z - pts[(i + 1) % n].x * pts[i].z).sum::<f32>().abs() * 0.5;
                if inside.is_none_or(|(a, _)| area < a) {
                    inside = Some((area, e));
                }
            }
        }

        near.or(inside).map(|(_, e)| e)
    }

    fn build_pick_grid(&self) {
        wce_prof!("tile.pick_grid");
        let mut g = PickGrid { valid: true, ..Default::default() };

        // Shown and selectable instances.
        let pickable = self.instances.iter().copied().filter(|&e| self.shown(e) && self.selectable(e)).collect::<Vec<_>>();
        let (mut minx, mut maxx, mut minz, mut maxz) = (1e30f32, -1e30f32, 1e30f32, -1e30f32);
        for &e in &pickable {
            let p = self.world_position(e);
            g.elem.push(e);
            g.x.push(p.x);
            g.y.push(p.y);
            g.z.push(p.z);
            g.r.push(self.marker_radius(e));
            g.type_.push(self.idx.type_[e]);
            minx = minx.min(p.x);
            maxx = maxx.max(p.x);
            minz = minz.min(p.z);
            maxz = maxz.max(p.z);
        }

        // Each soldier of a reference is a sphere of it.
        let spots = self.ref_spots.borrow();
        let picked = if spots.is_empty() { Default::default() } else { pickable.iter().copied().collect::<std::collections::HashSet<_>>() };
        for &(e, p, r) in spots.iter().filter(|(e, ..)| picked.contains(e)) {
            g.elem.push(e);
            g.x.push(p.x);
            g.y.push(p.y);
            g.z.push(p.z);
            g.r.push(r);
            g.type_.push(self.idx.type_[e]);
            minx = minx.min(p.x);
            maxx = maxx.max(p.x);
            minz = minz.min(p.z);
            maxz = maxz.max(p.z);
        }

        drop(spots);
        let m = g.elem.len();
        g.stamp = vec![0; m];
        if m == 0 {
            g.start = vec![0];
            *self.grid.borrow_mut() = g;
            return;
        }

        // The largest pick radius is 80 m.
        let pad = 90.0f32;
        g.cell = 64.0;
        while ((maxx - minx + 2.0 * pad) / g.cell + 1.0) * ((maxz - minz + 2.0 * pad) / g.cell + 1.0) > 1.0e6 {
            g.cell *= 2.0;
        }

        g.x0 = minx - pad;
        g.z0 = minz - pad;
        g.nx = ((maxx + pad - g.x0) / g.cell) as i32 + 1;
        g.nz = ((maxz + pad - g.z0) / g.cell) as i32 + 1;
        let range = |g: &PickGrid, i: usize| {
            (
                (((g.x[i] - g.r[i] - g.x0) / g.cell) as i32).clamp(0, g.nx - 1),
                (((g.x[i] + g.r[i] - g.x0) / g.cell) as i32).clamp(0, g.nx - 1),
                (((g.z[i] - g.r[i] - g.z0) / g.cell) as i32).clamp(0, g.nz - 1),
                (((g.z[i] + g.r[i] - g.z0) / g.cell) as i32).clamp(0, g.nz - 1),
            )
        };

        let nx = g.nx as usize;
        g.start = vec![0; nx * g.nz as usize + 1];
        for i in 0..m {
            let (cx0, cx1, cz0, cz1) = range(&g, i);
            for cz in cz0..=cz1 {
                for cx in cx0..=cx1 {
                    g.start[cz as usize * nx + cx as usize + 1] += 1;
                }
            }
        }

        for c in 0..g.start.len() - 1 {
            g.start[c + 1] += g.start[c];
        }

        g.items = vec![0; *g.start.last().expect("cells") as usize];
        let mut cursor = g.start[..g.start.len() - 1].to_vec();
        for i in 0..m {
            let (cx0, cx1, cz0, cz1) = range(&g, i);
            for cz in cz0..=cz1 {
                for cx in cx0..=cx1 {
                    let c = cz as usize * nx + cx as usize;
                    g.items[cursor[c] as usize] = i as u32;
                    cursor[c] += 1;
                }
            }
        }

        *self.grid.borrow_mut() = g;
    }

    /// What a click on the soldiers of the references finds: element, centre and radius of a sphere per soldier.
    pub fn set_ref_spots(&self, spots: Vec<(usize, Float3, f32)>) {
        if *self.ref_spots.borrow() != spots {
            *self.ref_spots.borrow_mut() = spots;
            self.grid_dirty.set(true);
        }
    }

    /// The nearest instance along a ray (spheres around the instances): a walk over the grid cells the ray crosses, stopping at the first cell
    /// that starts beyond the best hit.
    pub fn pick_instance(&self, o: Float3, d: Float3, visible_only: bool) -> Option<usize> {
        wce_prof!("tile.pick_instance");

        // Right after a structural edit, until the republish rebuilds the index.
        if !self.idx.valid {
            return None;
        }

        if self.grid_dirty.get() || !self.grid.borrow().valid {
            self.build_pick_grid();
            self.grid_dirty.set(false);
        }

        let mut g = self.grid.borrow_mut();
        if g.nx == 0 {
            return None;
        }

        let types = self.idx.type_names.len();
        let mut hidden = vec![false; types];

        // 0: no draw distance.
        let mut lim2 = vec![0.0f32; types];
        let mut building = None;
        for (t, name) in self.idx.type_names.iter().enumerate() {
            if name == "BUILDING_INSTANCE" {
                building = Some(t as u16);
            }

            if visible_only && self.display.hidden_types.contains(name) {
                hidden[t] = true;
            }

            if visible_only && self.display.lod && name != "BUILDING_INSTANCE" {
                lim2[t] = self.lod_distance(name) * self.lod_distance(name);
            }
        }

        let (mut t0, mut t1) = (0.0f32, 20000.0f32);
        let mut clip = |oo: f32, dd: f32, lo: f32, hi: f32| {
            if dd.abs() < 1e-9 {
                return oo >= lo && oo <= hi;
            }

            let (mut a, mut b) = ((lo - oo) / dd, (hi - oo) / dd);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }

            t0 = t0.max(a);
            t1 = t1.min(b);
            t0 <= t1
        };

        if !clip(o.x, d.x, g.x0, g.x0 + g.nx as f32 * g.cell) || !clip(o.z, d.z, g.z0, g.z0 + g.nz as f32 * g.cell) {
            return None;
        }

        let mut cx = (((o.x + d.x * t0 - g.x0) / g.cell) as i32).clamp(0, g.nx - 1);
        let mut cz = (((o.z + d.z * t0 - g.z0) / g.cell) as i32).clamp(0, g.nz - 1);
        let (sx, sz) = (if d.x > 0.0 { 1 } else { -1 }, if d.z > 0.0 { 1 } else { -1 });
        let inf = 1e30f32;
        let mut tmx = if d.x != 0.0 { ((cx + i32::from(sx > 0)) as f32 * g.cell + g.x0 - o.x) / d.x } else { inf };
        let mut tmz = if d.z != 0.0 { ((cz + i32::from(sz > 0)) as f32 * g.cell + g.z0 - o.z) / d.z } else { inf };
        let tdx = if d.x != 0.0 { g.cell / d.x.abs() } else { inf };
        let tdz = if d.z != 0.0 { g.cell / d.z.abs() } else { inf };
        g.stamp_counter += 1;
        let stamp = g.stamp_counter;
        let (mut best, mut best_t) = (None, 1e30f32);
        loop {
            let cell = cz as usize * g.nx as usize + cx as usize;
            for k in g.start[cell]..g.start[cell + 1] {
                let i = g.items[k as usize] as usize;
                if g.stamp[i] == stamp {
                    continue;
                }

                g.stamp[i] = stamp;
                let type_ = g.type_[i];
                if hidden[type_ as usize] {
                    continue;
                }

                let mut c = Float3::new(g.x[i], g.y[i], g.z[i]);
                let r = g.r[i];
                if Some(type_) == building {
                    c.y += r * 0.5;
                }

                if lim2[type_ as usize] > 0.0 {
                    let dd = c - o;
                    if dot(dd, dd) > lim2[type_ as usize] {
                        continue;
                    }
                }

                let oc = o - c;
                let (b, cc) = (dot(oc, d), dot(oc, oc) - r * r);
                let disc = b * b - cc;
                if disc < 0.0 {
                    continue;
                }

                let t = -b - disc.sqrt();
                if t > 0.0 && t < best_t {
                    best_t = t;
                    best = Some(g.elem[i]);
                }
            }

            let t_exit = tmx.min(tmz);
            if best_t <= t_exit || t_exit > t1 {
                break;
            }

            if tmx < tmz {
                cx += sx;
                tmx += tdx;
                if cx < 0 || cx >= g.nx {
                    break;
                }
            } else {
                cz += sz;
                tmz += tdz;
                if cz < 0 || cz >= g.nz {
                    break;
                }
            }
        }

        best
    }

    /// Where a ray meets the terrain: steps of a height map pixel, then a bisection.
    pub fn pick_terrain(&self, o: Float3, d: Float3) -> Option<Float3> {
        if !self.project.has_height() {
            return None;
        }

        let b = self.project.border_m();
        let (lo_x, hi_x, lo_z, hi_z) = (-b, self.extent_x() + b, -b, self.extent_z() + b);
        let step = 0.5f32.max(self.project.mpp_x() as f32);
        let tmax = 20000.0f32;
        let (mut prev_t, mut prev_above) = (0.0f32, true);
        let mut t = 0.0f32;
        while t < tmax {
            let p = o + d * t;
            if f64::from(p.x) < lo_x || f64::from(p.z) < lo_z || f64::from(p.x) > hi_x || f64::from(p.z) > hi_z {
                prev_t = t;
                prev_above = true;
                t += step;
                continue;
            }

            let above = p.y > self.terrain_height(p.x, p.z);
            if !above && prev_above && t > 0.0 {
                let (mut a, mut bb) = (prev_t, t);
                for _ in 0..20 {
                    let mid = (a + bb) * 0.5;
                    let q = o + d * mid;
                    if q.y > self.terrain_height(q.x, q.z) {
                        a = mid;
                    } else {
                        bb = mid;
                    }
                }

                return Some(o + d * bb);
            }

            prev_above = above;
            prev_t = t;
            t += step;
        }

        None
    }
}
