//! The rivers' terrain in the document (docs/plans/rivers.md). A river with an id (`river_links::RIVER_ID_ATTR`) owns a height layer,
//! made by `wce_tile::river_bed` over what lies below it: its joins to the tile's river links (the ends marked `wce_link`), shaped as the
//! export shapes the ground there. The layer is derived data, as a road's: the river changing, or the ground under it, makes it again;
//! what it was made from is in its owner (`river:<id>#<signature>`, `river:<id>@<variation>#<signature>` in a combined project), so that
//! opening a tile does not make it again. The rivers' layers lie under the roads' (a road crossing a river is laid over its bed).
//!
//! Kit rivers have no id until a snap gives them one: their ground is the user's, as TEd left it.

use wce_foundation::math::Float3;
use wce_tile::river_bed::{Join, RiverBed};
use wce_tile::terrain_layers::{Height, Stack, TerrainLayers, order_of};

use super::Tile;
use super::river_links::{LINK_ATTR, RIVER_ID_ATTR};

/// The owner prefix of the rivers' layers.
pub(super) const OWNER: &str = "river:";

fn owner_of(id: u32, variation: Option<&str>, signature: u64) -> String {
    match variation {
        Some(v) => format!("{OWNER}{id}@{v}#{signature:x}"),
        None => format!("{OWNER}{id}#{signature:x}"),
    }
}

/// A river layer's owner read: the river's id, the variation it was made for, its signature.
#[derive(Clone, Debug, PartialEq)]
struct Owner {
    id: u32,
    variation: Option<String>,
    signature: u64,
}

fn parse_owner(owner: &str) -> Option<Owner> {
    let (head, signature) = owner.strip_prefix(OWNER)?.split_once('#')?;
    let (id, variation) = match head.split_once('@') {
        Some((id, v)) => (id, Some(v.to_owned())),
        None => (head, None),
    };

    Some(Owner { id: id.parse().ok()?, variation, signature: u64::from_str_radix(signature, 16).ok()? })
}

/// Whether a layer's owner is a river's (the roads' layers go over them: `roads.rs`).
pub(super) fn is_river_owner(owner: &str) -> bool {
    owner.starts_with(OWNER)
}

/// A river as its layer is made from it: its id, what it does to the ground, the signature of that.
#[derive(Clone, Debug)]
struct River {
    id: u32,
    bed: RiverBed,
    signature: u64,
}

/// The rivers' layer being made off the editor's thread, what it is made from, and the stack as it was taken.
pub(crate) struct RiverTerrainJob {
    job: super::jobs::Job<Stack<Height>>,
    all: u64,
    before: Stack<Height>,
}

/// What making the rivers' layers needs: the rivers, the layers' new ids, a copy of the stack, the height map's size and how it lies
/// (unit, border, metres a pixel across and down).
struct RiverInput {
    rivers: Vec<River>,
    variation: Option<String>,
    new_ids: Vec<u64>,
    height: Stack<Height>,
    dims: (u32, u32),
    frame: (f64, f64, f64, f64),
}

fn signature_of(bed: &RiverBed) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for j in &bed.joins {
        for v in [j.at[0], j.at[1], j.inward[0], j.inward[1], j.connection.water, j.connection.width] {
            v.to_bits().hash(&mut h);
        }

        for v in j.connection.profile {
            v.to_bits().hash(&mut h);
        }
    }

    h.finish()
}

impl Tile {
    /// The rivers that own a layer, in the document's order: those with an id, of the variation viewed, shown, with what they do to the
    /// ground (a river doing nothing has no layer).
    fn river_beds(&self) -> Vec<River> {
        let sc = self.tscene();
        let links = self.river_link_points();
        let mut out = vec![];
        for e in sc.find_all("SplineRiverInstance") {
            let Some(id) = sc.get(e, RIVER_ID_ATTR).and_then(|v| v.parse::<u32>().ok()) else { continue };
            if !self.shown(e) || !self.of_tile_variation(e) {
                continue;
            }

            let points = sc.polygon_points(e);
            let world = self.polygon_world_points(e);
            let mut joins = vec![];
            for (i, &p) in points.iter().enumerate() {
                let Some(cell) = sc.get(p, LINK_ATTR).and_then(|v| {
                    let (x, y) = v.split_once(',')?;
                    Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?))
                }) else {
                    continue;
                };
                let Some(at) = world.get(i) else { continue };
                // The link of that cell nearest the end (a corner cell has two sides on the edge).
                let near = links.iter().filter(|l| l.cell == cell).min_by(|a, b| {
                    let d = |q: Float3| (q.x - at.x).hypot(q.z - at.z);
                    d(a.on_edge).total_cmp(&d(b.on_edge))
                });
                if let Some(l) = near {
                    joins.push(Join {
                        at: [f64::from(l.on_edge.x), f64::from(l.on_edge.z)],
                        inward: [f64::from(l.inward[0]), f64::from(l.inward[1])],
                        connection: *l.connection,
                    });
                }
            }

            let bed = RiverBed { joins };
            if bed.bounds().is_none() {
                continue;
            }

            let signature = signature_of(&bed);
            out.push(River { id, bed, signature });
        }

        out
    }

    /// Makes again the layers of the rivers that changed, and takes out those of rivers that went. Waits for an open stroke and for a
    /// river that stopped changing. `sync_terrain_layers` has run.
    pub(super) fn refresh_rivers(&mut self) {
        self.refresh_rivers_with(false);
    }

    /// [`Self::refresh_rivers`]; `wait`: what is being made is waited for, and what is to be made is made now (an export).
    fn refresh_rivers_with(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        self.take_river_terrain(wait);
        let stacks = self.project.layers.as_ref().and_then(|l| l.height.as_ref());
        let owned = stacks.is_some_and(|s| s.layers.iter().any(|l| is_river_owner(&l.owner)));
        let rivers = self.river_beds();
        if rivers.is_empty() && !owned {
            self.rivers_stale = false;
            return;
        }

        let cur = self.road_variation();
        let mine = |o: &Owner| o.variation == cur;
        let made = |id: u32| {
            stacks.and_then(|s| s.layers.iter().find_map(|l| parse_owner(&l.owner).filter(|o| o.id == id && mine(o)).map(|o| o.signature)))
        };
        let stale = self.rivers_stale || rivers.iter().any(|r| made(r.id) != Some(r.signature));
        let gone = stacks.is_some_and(|s| {
            s.layers
                .iter()
                .filter_map(|l| parse_owner(&l.owner))
                .any(|o| (mine(&o) && rivers.iter().all(|r| r.id != o.id)) || (cur.is_some() && o.variation.is_none()))
        });
        if !stale && !gone {
            return;
        }

        // A river still changing waits for the next look; the rivers being made as they are wait for their job.
        let all = rivers.iter().fold(0u64, |h, r| h.rotate_left(7) ^ r.signature ^ u64::from(r.id));
        if all != self.rivers_seen && !wait {
            self.rivers_seen = all;
            return;
        }

        if self.river_terrain_job.as_ref().is_some_and(|j| j.all == all) {
            return;
        }

        self.rivers_seen = all;
        if let Err(why) = self.start_river_terrain(rivers, all) {
            wce_foundation::log::warn(format!("rivers: {why}"));
            return;
        }

        if wait {
            self.take_river_terrain(true);
        }
    }

    fn start_river_terrain(&mut self, rivers: Vec<River>, all: u64) -> Result<(), String> {
        let variation = self.road_variation();
        let p = &mut self.project;
        let unit = p.unit_scale;
        let (border, mx, mz) = (p.border_m(), p.mpp_x(), p.mpp_z());
        let Some(hf) = p.height.as_ref() else { return Err("the tile has no height map".to_owned()) };
        let dims = (hf.width(), hf.height());
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let new_ids = rivers.iter().map(|_| layers.new_id()).collect::<Vec<_>>();
        let height = layers.height.get_or_insert_with(|| Stack::new(hf)).clone();
        let before = height.clone();
        let input = RiverInput { rivers, variation, new_ids, height, dims, frame: (unit, border, mx, mz) };
        let job = super::jobs::Job::spawn("river terrain", move || river_terrain(input));
        self.river_terrain_job = Some(RiverTerrainJob { job, all, before });
        Ok(())
    }

    /// The rivers' layers made off the editor's thread, put in place of the stack they were made from and composed again over what they
    /// changed, when the stack is still as it was taken (edited meanwhile, they are made again). The roads lie over them: made again too.
    fn take_river_terrain(&mut self, wait: bool) {
        let Some(j) = &self.river_terrain_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => Some(m),
            super::jobs::Poll::Failed => None,
        };

        let j = self.river_terrain_job.take().expect("a job");
        let Some(height) = made else {
            wce_foundation::log::warn("rivers: their terrain could not be made");
            return;
        };

        let p = &mut self.project;
        let (Some(hf), Some(layers)) = (p.height.as_mut(), p.layers.as_mut()) else { return };
        let Some(h0) = layers.height.as_ref() else { return };
        if !h0.unchanged_since(&j.before) {
            self.rivers_stale = true;
            return;
        }

        let rect = h0
            .layers
            .iter()
            .chain(&height.layers)
            .filter(|l| is_river_owner(&l.owner))
            .filter_map(|l| l.extent())
            .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
        p.height_dirty |= rect.is_some();
        layers.height = Some(height);
        layers.dirty = true;
        if let Some(s) = layers.height.as_mut() {
            if let Some(r) = rect {
                s.apply(hf, r);
            }

            s.remember();
        }

        self.rivers_stale = false;
        self.roads_stale = true;
        self.layers_shown = true;
        self.reload_grass();
        self.reload_tree_list();
    }

    /// The rivers' layers as an export reads them: made, waiting for what is being made off the editor's thread.
    pub fn finish_river_jobs(&mut self) {
        self.refresh_rivers_with(true);
    }
}

/// Makes again the layer each river owns in a copy of the stack, over what lies below it, and takes out those of rivers that went. The
/// rivers' layers go first among the owned ones (under the roads'). Off the editor's thread.
fn river_terrain(i: RiverInput) -> Stack<Height> {
    wce_foundation::wce_prof!("tile.make_rivers");
    let RiverInput { rivers, variation, new_ids, mut height, dims: (hw, hh), frame: (unit, border, mx, mz) } = i;
    let alive = |o: &str| {
        parse_owner(o).is_none_or(|o| match (&o.variation, &variation) {
            (Some(a), Some(b)) if a != b => true,
            (None, Some(_)) => false,
            _ => rivers.iter().any(|r| r.id == o.id),
        })
    };
    height.layers.retain(|l| alive(&l.owner));

    for (river, &new_id) in rivers.iter().zip(&new_ids) {
        let owner = owner_of(river.id, variation.as_deref(), river.signature);
        let name = match &variation {
            Some(v) => format!("River {} ({v})", river.id),
            None => format!("River {}", river.id),
        };
        let id =
            match height.layers.iter_mut().find(|l| parse_owner(&l.owner).is_some_and(|o| o.id == river.id && o.variation == variation)) {
                Some(l) => {
                    l.owner.clone_from(&owner);
                    let id = l.id;
                    height.clear_layer(id);
                    id
                }
                None => {
                    let id = height.add(new_id, &name, &owner);
                    if let (Some(v), Some(i)) = (&variation, height.index_of(id)) {
                        height.layers[i].variations = vec![v.clone()];
                    }

                    id
                }
            };

        let Some(b) = river.bed.bounds() else { continue };
        let px = |v: f64, m: f64, n: u32| (((v + border) / m).floor().max(0.0) as u32).min(n);
        let (x0, y0, x1, y1) = (px(b[0], mx, hw), px(b[1], mz, hh), (px(b[2], mx, hw) + 1).min(hw), (px(b[3], mz, hh) + 1).min(hh));
        let mut writes = vec![];
        for y in y0..y1 {
            for x in x0..x1 {
                let mut v = [0.0f32];
                height.below(id, x, y, &mut v);
                let g = f64::from(v[0]) / unit;
                let (wx, wz) = (f64::from(x) * mx - border, f64::from(y) * mz - border);
                if let Some(h) = river.bed.height_at(wx, wz, g) {
                    writes.push((x, y, ((h - g) * unit) as f32));
                }
            }
        }

        for (x, y, v) in writes {
            height.put(id, x, y, &[v]);
        }
    }

    // The rivers' first among the owned layers, the others as they were.
    height.layers.sort_by_key(|l| (order_of(&l.owner), u8::from(!is_river_owner(&l.owner))));
    height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_reads_back() {
        let o = owner_of(3, Some("b"), 0xabc);
        assert_eq!(parse_owner(&o), Some(Owner { id: 3, variation: Some("b".to_owned()), signature: 0xabc }));
        assert_eq!(parse_owner(&owner_of(4, None, 1)).map(|o| o.id), Some(4));
        assert!(parse_owner("road:3#1").is_none());
    }
}
