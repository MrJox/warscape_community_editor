//! Where a river's foam goes (user 2026-11-28: only where the water meets something, in an elongated trail as it thins out down the
//! river): a map over the tile, 1 m a cell, of how much foam each place of the water has (0 to 1), made by the editor, which knows the
//! ground and what stands in the water, and saved with the project ([`FILE`]). The export's river mesh for the editor's rivers is dense
//! enough to carry it, each vertex the river's foam times the map there (`wce_tile_io::attila::river`); the editor's water too. The
//! editor's river shader draws it (docs/re/water.md, "The editor's river shader").
//!
//! The map: where the water meets the ground (the bed within [`CONTACT_M`] of the surface: banks, shallows, a ford's edge), and around
//! and behind what stands in it (rocks, piers): a ring round its footprint, then a trail down the river, as wide as the thing and
//! widening, fading to nothing over [`WAKE_M`] (at the game's flow; longer with a faster flow).

/// The map's file in a project's folder (and its exported folder: the compile reads it).
pub const FILE: &str = "wce_river_foam.bin";

/// The bed's depth (m) under which the water foams, fading to none there.
pub const CONTACT_M: f64 = 0.6;

/// The ring of foam round what stands in the water (m past its footprint).
pub const RING_M: f64 = 1.0;

/// The trail's length (m) at a flow of 0.5, and how much the flow lengthens it: twice the flow, between these.
pub const WAKE_M: f64 = 20.0;
pub const WAKE_FLOW: (f64, f64) = (0.3, 2.5);

/// How much the trail widens (m each side per m down the river), and the share of the thing's width it starts with.
pub const WAKE_SPREAD: f64 = 0.1;
pub const WAKE_START: f64 = 0.8;

/// What stands in the water deeper than this (m of water at its middle) makes a trail; on the bank (a cliff piece, a bank stone),
/// only its contact.
pub const OBSTACLE_DEPTH_M: f64 = 0.3;

const MAGIC: &[u8; 8] = b"WCEFOAM1";

/// A river as the map needs it: its middle line, each place with its surface's height, half its width and its flow (world metres).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoamRiver {
    pub samples: Vec<FoamSample>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FoamSample {
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub half: f64,
    pub flow: f64,
}

/// Something standing on the tile (world metres; its radius across).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Obstacle {
    pub x: f64,
    pub z: f64,
    pub radius: f64,
}

/// The foam over a box of the tile: cells of `cell` m from (`x0`, `z0`), `w` across x and `h` across z, 0 to 255 each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoamMap {
    pub x0: f64,
    pub z0: f64,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub v: Vec<u8>,
}

impl FoamMap {
    /// The foam at a place (world metres), 0 to 1, from the four cells round it; 0 off the map.
    #[must_use]
    pub fn at(&self, x: f64, z: f64) -> f64 {
        if self.w == 0 || self.h == 0 {
            return 0.0;
        }

        let (fx, fz) = ((x - self.x0) / self.cell - 0.5, (z - self.z0) / self.cell - 0.5);
        let (ix, iz) = (fx.floor(), fz.floor());
        let (tx, tz) = (fx - ix, fz - iz);
        let get = |cx: f64, cz: f64| {
            if cx < 0.0 || cz < 0.0 || cx >= self.w as f64 || cz >= self.h as f64 {
                0.0
            } else {
                f64::from(self.v[cz as usize * self.w + cx as usize]) / 255.0
            }
        };
        let a = get(ix, iz) * (1.0 - tx) + get(ix + 1.0, iz) * tx;
        let b = get(ix, iz + 1.0) * (1.0 - tx) + get(ix + 1.0, iz + 1.0) * tx;
        a * (1.0 - tz) + b * tz
    }

    /// The file's bytes: the magic, x0, z0, cell (f64), w, h (u32), the cells row by row (z), little endian.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for f in [self.x0, self.z0, self.cell] {
            out.extend(f.to_le_bytes());
        }

        out.extend((self.w as u32).to_le_bytes());
        out.extend((self.h as u32).to_le_bytes());
        out.extend(&self.v);
        out
    }

    /// A map read from its file's bytes; none when they aren't one.
    #[must_use]
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 40 || &b[..8] != MAGIC {
            return None;
        }

        let f = |at: usize| f64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"));
        let u = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes")) as usize;
        let (w, h) = (u(32), u(36));
        let v = b.get(40..40 + w * h)?.to_vec();
        Some(Self { x0: f(8), z0: f(16), cell: f(24), w, h, v })
    }
}

/// A place of a river's water: how far to the left of its middle (m), half its width there, its surface, its flow, and its way down the
/// river (unit, x and z).
#[derive(Clone, Copy, Debug)]
struct Place {
    across: f64,
    half: f64,
    y: f64,
    flow: f64,
    dir: (f64, f64),
}

/// The nearest place of a river's middle line to a point (`None` for a river of fewer than two samples).
fn place_of(r: &FoamRiver, x: f64, z: f64) -> Option<Place> {
    let mut best: Option<(f64, Place)> = None;
    for w in r.samples.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let l2 = dx * dx + dz * dz;
        if l2 < 1e-12 {
            continue;
        }

        let t = (((x - a.x) * dx + (z - a.z) * dz) / l2).clamp(0.0, 1.0);
        let (px, pz) = (a.x + dx * t, a.z + dz * t);
        let d2 = (x - px).powi(2) + (z - pz).powi(2);
        if best.as_ref().is_none_or(|(bd, _)| d2 < *bd) {
            let l = l2.sqrt();
            let side = if dx * (z - a.z) - dz * (x - a.x) < 0.0 { -1.0 } else { 1.0 };
            let lerp = |p: f64, q: f64| p + (q - p) * t;
            best = Some((
                d2,
                Place {
                    across: side * d2.sqrt(),
                    half: lerp(a.half, b.half),
                    y: lerp(a.y, b.y),
                    flow: lerp(a.flow, b.flow),
                    dir: (dx / l, dz / l),
                },
            ));
        }
    }

    best.map(|(_, p)| p)
}

/// The foam map of the rivers over the box (`x0`, `z0`) to (`x1`, `z1`), world metres, a cell a metre: their contact with the ground
/// (`ground`: its height at a place) and the rings and trails of the obstacles that stand in their water.
#[must_use]
pub fn foam_map(rivers: &[FoamRiver], obstacles: &[Obstacle], ground: &dyn Fn(f64, f64) -> f64, bounds: [f64; 4]) -> FoamMap {
    let cell = 1.0;
    let [x0, z0, x1, z1] = bounds;
    let (w, h) = (((x1 - x0) / cell).ceil().max(0.0) as usize, ((z1 - z0) / cell).ceil().max(0.0) as usize);
    let mut v = vec![0.0f64; w * h];
    let centre = |i: usize, j: usize| (x0 + (i as f64 + 0.5) * cell, z0 + (j as f64 + 0.5) * cell);
    let cell_of = |x: f64, z: f64| {
        let (i, j) = ((x - x0) / cell, (z - z0) / cell);
        (i >= 0.0 && j >= 0.0 && (i as usize) < w && (j as usize) < h).then_some((i as usize, j as usize))
    };

    for r in rivers.iter().filter(|r| r.samples.len() >= 2) {
        // The water's cells (under the river's ribbon) and where each is along and across it.
        let reach = r.samples.iter().map(|s| s.half).fold(0.0, f64::max) + 1.0;
        let (lo, hi) = r.samples.iter().fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), s| {
            ([lo[0].min(s.x - reach), lo[1].min(s.z - reach)], [hi[0].max(s.x + reach), hi[1].max(s.z + reach)])
        });
        let (Some((i0, j0)), Some((i1, j1))) = (cell_of(lo[0].max(x0), lo[1].max(z0)), cell_of(hi[0].min(x1 - 1e-6), hi[1].min(z1 - 1e-6)))
        else {
            continue;
        };
        let mut water: Vec<Option<Place>> = vec![None; (i1 - i0 + 1) * (j1 - j0 + 1)];
        let stride = i1 - i0 + 1;
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (x, z) = centre(i, j);
                let Some(p) = place_of(r, x, z).filter(|p| p.across.abs() <= p.half) else { continue };
                water[(j - j0) * stride + (i - i0)] = Some(p);
                // Contact with the ground: the bed near the surface (or above it: the bank under the ribbon's edge).
                let depth = p.y - ground(x, z);
                let contact = (1.0 - depth / CONTACT_M).clamp(0.0, 1.0);
                let k = j * w + i;
                v[k] = v[k].max(contact);
            }
        }

        // What stands in the water: a ring round it, and a trail down the river.
        for o in obstacles {
            let Some(p) = place_of(r, o.x, o.z) else { continue };
            if p.across.abs() > p.half + o.radius || p.y - ground(o.x, o.z) < OBSTACLE_DEPTH_M {
                continue;
            }

            let down = if p.flow < 0.0 { (-p.dir.0, -p.dir.1) } else { p.dir };
            let side = (-down.1, down.0);
            let length = WAKE_M * (2.0 * p.flow.abs()).clamp(WAKE_FLOW.0, WAKE_FLOW.1);
            let span = o.radius + RING_M + length;
            let (Some((a0, b0)), Some((a1, b1))) =
                (cell_of((o.x - span).max(x0), (o.z - span).max(z0)), cell_of((o.x + span).min(x1 - 1e-6), (o.z + span).min(z1 - 1e-6)))
            else {
                continue;
            };
            for j in b0.max(j0)..=b1.min(j1) {
                for i in a0.max(i0)..=a1.min(i1) {
                    if water[(j - j0) * stride + (i - i0)].is_none() {
                        continue;
                    }

                    let (x, z) = centre(i, j);
                    let (dx, dz) = (x - o.x, z - o.z);
                    let ring = (1.0 - ((dx.hypot(dz) - o.radius) / RING_M).max(0.0)).clamp(0.0, 1.0);
                    let (d, lateral) = (dx * down.0 + dz * down.1, (dx * side.0 + dz * side.1).abs());
                    let trail = if d > 0.0 && d < length {
                        let width = o.radius * WAKE_START + d * WAKE_SPREAD;
                        let fade = 1.0 - d / length;
                        fade * (1.0 - lateral / width.max(1e-6)).clamp(0.0, 1.0).sqrt()
                    } else {
                        0.0
                    };
                    let k = j * w + i;
                    v[k] = v[k].max(ring).max(trail);
                }
            }
        }
    }

    FoamMap { x0, z0, cell, w, h, v: v.into_iter().map(|f| (f.clamp(0.0, 1.0) * 255.0).round() as u8).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight() -> FoamRiver {
        // Along x from 0 to 100, 20 m wide, its surface at 0, flowing towards +x.
        FoamRiver { samples: (0..=10).map(|k| FoamSample { x: f64::from(k) * 10.0, z: 50.0, y: 0.0, half: 10.0, flow: 0.5 }).collect() }
    }

    #[test]
    fn foam_trails_behind_a_rock_and_lines_the_banks() {
        // A bed 3 m deep in the middle, rising to the surface at the edges (half 10 m).
        let ground = |_x: f64, z: f64| -3.0 + 3.0 * ((z - 50.0).abs() / 10.0).powi(2);
        let rock = Obstacle { x: 30.0, z: 50.0, radius: 2.0 };
        let map = foam_map(&[straight()], &[rock], &ground, [0.0, 0.0, 120.0, 100.0]);
        // Round the rock and behind it (down the river: +x), not in front; fading with the distance.
        assert!(map.at(30.0, 52.5) > 0.4, "ring {}", map.at(30.0, 52.5));
        assert!(map.at(36.0, 50.0) > 0.35, "near trail {}", map.at(36.0, 50.0));
        assert!(map.at(40.0, 50.0) > 0.1 && map.at(40.0, 50.0) < map.at(36.0, 50.0), "far trail {}", map.at(40.0, 50.0));
        assert!(map.at(24.0, 50.0) < 0.05, "up the river {}", map.at(24.0, 50.0));
        assert!(map.at(36.0, 56.0) < 0.05, "beside the trail {}", map.at(36.0, 56.0));
        // The banks foam (the bed near the surface), the middle away from the rock doesn't.
        assert!(map.at(70.0, 59.5) > 0.5, "bank {}", map.at(70.0, 59.5));
        assert!(map.at(70.0, 50.0) < 0.05, "middle {}", map.at(70.0, 50.0));

        // A faster river: a longer trail. Its file reads back the same.
        let mut fast = straight();
        fast.samples.iter_mut().for_each(|s| s.flow = 1.0);
        let longer = foam_map(&[fast], &[rock], &ground, [0.0, 0.0, 120.0, 100.0]);
        assert!(longer.at(52.0, 50.0) > map.at(52.0, 50.0), "{} vs {}", longer.at(52.0, 50.0), map.at(52.0, 50.0));
        assert_eq!(FoamMap::from_bytes(&map.to_bytes()), Some(map));
    }

    #[test]
    fn a_rock_on_the_bank_makes_no_trail() {
        let ground = |_x: f64, z: f64| if (z - 50.0).abs() > 8.0 { 1.0 } else { -3.0 };
        let on_bank = Obstacle { x: 30.0, z: 59.0, radius: 2.0 };
        let map = foam_map(&[straight()], &[on_bank], &ground, [0.0, 0.0, 120.0, 100.0]);
        assert!(map.at(40.0, 57.0) < 0.05, "{}", map.at(40.0, 57.0));
    }
}
