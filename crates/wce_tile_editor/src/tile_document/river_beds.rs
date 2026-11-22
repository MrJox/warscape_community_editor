//! The rivers' terrain in the document (docs/plans/rivers.md). A river with an id (`river_links::RIVER_ID_ATTR`) owns a height layer,
//! made by `wce_tile::river_bed` over what lies below it: its joins to the tile's river links (the ends marked `wce_link`), shaped as the
//! export shapes the ground there. The layer is derived data, as a road's: the river changing, or the ground under it, makes it again;
//! what it was made from is in its owner (`river:<id>#<signature>`, `river:<id>@<variation>#<signature>` in a combined project), so that
//! opening a tile does not make it again. The rivers' layers lie under the roads' (a road crossing a river is laid over its bed).
//!
//! Kit rivers have no id until a snap gives them one: their ground is the user's, as TEd left it.

use wce_foundation::math::Float3;
use wce_tile::river_bed::{BedSettings, Channel, Join, RiverBed};
use wce_tile::terrain_layers::{Blend, Height, Kind, Stack, TerrainLayers, order_of};

use super::Tile;
use super::river_links::{LINK_ATTR, RIVER_ID_ATTR};

/// A river cuts its bed along its line (`1`; the rivers the editor draws), or leaves the ground to the user (missing or `0`: the kit's).
pub const BED_ATTR: &str = "wce_bed";

/// The bed's depth under the water (metres; missing: `BedSettings::default_depth` of its width) and its banks' slope (degrees).
pub const BED_DEPTH_ATTR: &str = "wce_bed_depth";
pub const BANK_SLOPE_ATTR: &str = "wce_bank_slope";

/// The texture a river's bed is painted with: a blend channel (the texture group times 4 plus its variation), `none` for none; missing:
/// the tile's group named like mud, dirt, rock or sand, its first variation (`Tile::default_bed_texture`).
pub const BED_TEXTURE_ATTR: &str = "wce_bed_texture";

/// How far the ground is cut for the bed's texture to cover it whole (metres): less, it fades.
const TEXTURE_FULL_CUT: f64 = 1.5;

/// How far under the ground it is drawn on a river the editor draws runs (metres): its bed is cut round it.
pub const WATER_BELOW_BANK: f32 = 2.0;

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

/// A river's attributes the Inspector shows as rows of their own (or not at all).
pub(crate) const RIVER_ROW_ATTRS: [&str; 5] = [RIVER_ID_ATTR, BED_ATTR, BED_DEPTH_ATTR, BANK_SLOPE_ATTR, BED_TEXTURE_ATTR];

impl super::TileDocument {
    /// A river's bed in the Inspector: whether it cuts one, how deep under the water and how steep its banks.
    pub(crate) fn river_bed_rows(&mut self, e: usize, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        let sc = self.tile().tscene();
        if sc.type_of(e) != "SplineRiverInstance" {
            return;
        }

        let on = sc.get(e, BED_ATTR).as_deref() == Some("1");
        let num = |a: &str| sc.get(e, a).and_then(|v| v.trim().parse::<f32>().ok());
        let width = {
            let pts = sc.polygon_points(e);
            let w = pts.iter().filter_map(|&p| sc.get(p, "width")?.parse::<f64>().ok()).collect::<Vec<_>>();
            w.iter().sum::<f64>() / w.len().max(1) as f64 * self.tile().river_width_factor(e)
        };
        let (depth, slope) = (
            num(BED_DEPTH_ATTR).unwrap_or(BedSettings::default_depth(width) as f32),
            num(BANK_SLOPE_ATTR).unwrap_or(BedSettings::DEFAULT_BANK_SLOPE as f32),
        );
        ins.section("River bed");
        let mut carve = on;
        ins.hint(
            "The river cuts its bed along its line: the kit's river bed, scaled to its width and depth, its banks climbing to the ground; \
             where the ground is lower it stays. Its joins to the tile's river links are made whether or not. The rivers the editor draws \
             cut one; the kit's leave the ground as it was sculpted.",
        );
        if ins.edit_bool("wce_bed", "cut its bed", &mut carve) && carve != on {
            self.set_attribute(e, BED_ATTR, if carve { "1" } else { "0" });
            if carve && self.tile().tscene().get(e, RIVER_ID_ATTR).is_none_or(|v| v.is_empty()) {
                let id = self.tile().next_river_id();
                self.set_attribute(e, RIVER_ID_ATTR, &id.to_string());
            }
        }

        if !carve {
            return;
        }

        let mut d = depth;
        ins.hint("How deep the bed's middle is under the water (metres). The kit's rivers: 5.7 for 45 m wide.");
        if ins.edit_float(BED_DEPTH_ATTR, "bed depth (m)", &mut d, 0.05, 0.0, 30.0) && (d - depth).abs() > 1e-4 {
            self.set_attribute(e, BED_DEPTH_ATTR, &wce_tile::ted_scene::format_number(f64::from(d)));
        }

        let mut s = slope;
        ins.hint("How steep the banks climb past the bed to the ground (degrees).");
        if ins.edit_float(BANK_SLOPE_ATTR, "bank slope (deg)", &mut s, 0.5, 5.0, 85.0) && (s - slope).abs() > 1e-4 {
            self.set_attribute(e, BANK_SLOPE_ATTR, &wce_tile::ted_scene::format_number(f64::from(s)));
        }

        // The bed's texture: none, or one of the tile's texture groups (its first variation), the default named.
        let t = self.tile();
        let groups = t.texture_group_names();
        let default = t.default_bed_texture();
        let current = match t.tscene().get(e, BED_TEXTURE_ATTR).as_deref().map(str::trim) {
            Some("none") => None,
            Some(v) => v.parse::<usize>().ok().map(|c| c.min(15)).or(default),
            None => default,
        };
        let mut names = vec!["none".to_owned()];
        names.extend(groups.iter().map(|g| if g.is_empty() { "(unnamed group)".to_owned() } else { g.clone() }));
        let mut idx = current.map_or(0, |c| (c / 4) as i32 + 1);
        let before = idx;
        ins.hint("The texture the bed and its banks are painted with where the ground was cut: one of the tile's texture groups.");
        if ins.edit_choice(BED_TEXTURE_ATTR, "bed texture", &mut idx, &names) && idx != before {
            let value = if idx <= 0 { "none".to_owned() } else { ((idx as usize - 1) * 4).to_string() };
            self.set_attribute(e, BED_TEXTURE_ATTR, &value);
        }
    }
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

    /// The blend channel its bed is painted with.
    texture: Option<usize>,
    signature: u64,
}

/// The rivers' layer being made off the editor's thread, what it is made from, and the stack as it was taken.
pub(crate) struct RiverTerrainJob {
    job: super::jobs::Job<(Stack<Height>, Option<Stack<Blend>>)>,
    all: u64,
    before: (Stack<Height>, Option<Stack<Blend>>),
}

/// What making the rivers' layers needs: the rivers, the layers' new ids (height, blend), copies of the stacks, the height and blend maps'
/// sizes and how they lie (unit, border, metres a height pixel across and down, the extent x and z).
struct RiverInput {
    rivers: Vec<River>,
    variation: Option<String>,
    new_ids: Vec<[u64; 2]>,
    height: Stack<Height>,

    /// None when the tile has no blend map: the beds are not painted.
    blend: Option<Stack<Blend>>,
    dims: [(u32, u32); 2],
    frame: (f64, f64, f64, f64, f64, f64),
}

fn signature_of(bed: &RiverBed, texture: Option<usize>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    texture.hash(&mut h);
    if let Some(c) = &bed.channel {
        for v in c.line.iter().flatten().chain(&c.water).chain(&c.width).chain([&c.depth, &c.bank_slope]) {
            v.to_bits().hash(&mut h);
        }
    }

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
        // The ribbons as drawn (`river_ribbons`: the viewed variation's rivers, in the document's order) by their element.
        let ribbon_elems = sc
            .find_all("SplineRiverInstance")
            .into_iter()
            .filter(|&e| sc.in_variation(e) && self.polygon_world_points(e).len() >= 2)
            .collect::<Vec<_>>();
        let mut ribbons = ribbon_elems.into_iter().zip(self.river_ribbons()).collect::<std::collections::HashMap<_, _>>();
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

            // Its bed along the ribbon's samples (the file's widths, the game's: the kit's profile is of those), when it cuts one.
            let channel = (sc.get(e, BED_ATTR).as_deref() == Some("1")).then(|| ribbons.remove(&e)).flatten().map(|(_, samples)| {
                let width = samples.iter().map(|s| s.width).sum::<f64>() / samples.len().max(1) as f64;
                let num = |a: &str| sc.get(e, a).and_then(|v| v.trim().parse::<f64>().ok());
                Channel {
                    line: samples.iter().map(|s| [s.x, s.z]).collect(),
                    water: samples.iter().map(|s| s.y).collect(),
                    width: samples.iter().map(|s| s.width).collect(),
                    depth: num(BED_DEPTH_ATTR).unwrap_or_else(|| BedSettings::default_depth(width)).max(0.0),
                    bank_slope: num(BANK_SLOPE_ATTR).unwrap_or(BedSettings::DEFAULT_BANK_SLOPE).clamp(5.0, 85.0),
                }
            });
            let bed = RiverBed { channel, joins };
            if bed.bounds().is_none() {
                continue;
            }

            let texture = match sc.get(e, BED_TEXTURE_ATTR).as_deref().map(str::trim) {
                Some("none") => None,
                Some(v) => v.parse::<usize>().ok().map(|c| c.min(15)).or_else(|| self.default_bed_texture()),
                None => self.default_bed_texture(),
            };
            let signature = signature_of(&bed, texture);
            out.push(River { id, bed, texture, signature });
        }

        out
    }

    /// The texture a river's bed takes unless it names one: the first of the tile's texture groups named like mud, dirt, rock or sand
    /// (in that order), its first variation; none when none is.
    pub(crate) fn default_bed_texture(&self) -> Option<usize> {
        let g = self.project.entry.as_ref()?.texture_group();
        let groups = [g.red, g.green, g.blue, g.alpha].map(|n| n.to_lowercase());
        ["mud", "dirt", "rock", "sand"].iter().find_map(|w| groups.iter().position(|n| n.contains(w))).map(|i| i * 4)
    }

    /// The tile's texture groups' names (red, green, blue, alpha), for a choice of the bed's texture.
    pub(crate) fn texture_group_names(&self) -> [String; 4] {
        let g = self.project.entry.as_ref().map(|e| e.texture_group()).unwrap_or_default();
        [g.red, g.green, g.blue, g.alpha]
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
        // Without a blend map the beds are cut, not painted.
        let painted = p.load_blend().is_ok();
        let unit = p.unit_scale;
        let (border, mx, mz, ex, ez) = (p.border_m(), p.mpp_x(), p.mpp_z(), p.extent_x(), p.extent_z());
        let Some(hf) = p.height.as_ref() else { return Err("the tile has no height map".to_owned()) };
        let bf = p.blend.as_ref().filter(|_| painted);
        let dims = [(hf.width(), hf.height()), bf.map_or((0, 0), |b| (b.width(), b.height()))];
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let new_ids = rivers.iter().map(|_| [layers.new_id(), layers.new_id()]).collect::<Vec<_>>();
        let height = layers.height.get_or_insert_with(|| Stack::new(hf)).clone();
        let blend = bf.map(|bf| layers.blend.get_or_insert_with(|| Stack::new(bf)).clone());
        let before = (height.clone(), blend.clone());
        let input = RiverInput { rivers, variation, new_ids, height, blend, dims, frame: (unit, border, mx, mz, ex, ez) };
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
        let Some((height, blend)) = made else {
            wce_foundation::log::warn("rivers: their terrain could not be made");
            return;
        };

        let p = &mut self.project;
        let (Some(hf), Some(layers)) = (p.height.as_mut(), p.layers.as_mut()) else { return };
        let Some(h0) = layers.height.as_ref() else { return };
        let blend_same = match (&j.before.1, layers.blend.as_ref()) {
            (Some(b), Some(b0)) => b0.unchanged_since(b),
            (None, _) => true,
            (Some(_), None) => false,
        };
        if !h0.unchanged_since(&j.before.0) || !blend_same {
            self.rivers_stale = true;
            return;
        }

        fn rivers_rect<K: Kind>(old: &Stack<K>, new: &Stack<K>) -> Option<[u32; 4]> {
            old.layers
                .iter()
                .chain(&new.layers)
                .filter(|l| is_river_owner(&l.owner))
                .filter_map(|l| l.extent())
                .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
        }

        let rect = rivers_rect(h0, &height);
        let brect = blend.as_ref().zip(layers.blend.as_ref()).and_then(|(b, b0)| rivers_rect(b0, b));
        p.height_dirty |= rect.is_some();
        p.blend_dirty |= brect.is_some();
        layers.height = Some(height);
        if blend.is_some() {
            layers.blend = blend;
        }

        layers.dirty = true;
        if let Some(s) = layers.height.as_mut() {
            if let Some(r) = rect {
                s.apply(hf, r);
            }

            s.remember();
        }

        if let (Some(s), Some(bf), Some(r)) = (layers.blend.as_mut(), p.blend.as_mut(), brect) {
            s.apply(bf, r);
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

/// A river's layer in a stack, emptied: the one it has (of this variation), else a new one with the id given.
fn river_layer<K: Kind>(s: &mut Stack<K>, river: u32, variation: &Option<String>, owner: &str, name: &str, new_id: u64) -> u64 {
    if let Some(l) = s.layers.iter_mut().find(|l| parse_owner(&l.owner).is_some_and(|o| o.id == river && o.variation == *variation)) {
        l.owner = owner.to_owned();
        let id = l.id;
        s.clear_layer(id);
        return id;
    }

    let id = s.add(new_id, name, owner);
    if let (Some(v), Some(i)) = (variation, s.index_of(id)) {
        s.layers[i].variations = vec![v.clone()];
    }

    id
}

/// Makes again the layers each river owns (its height, and its bed's texture) in copies of the stacks, over what lies below them, and
/// takes out those of rivers that went. The rivers' layers go first among the owned ones (under the roads'). Off the editor's thread.
fn river_terrain(i: RiverInput) -> (Stack<Height>, Option<Stack<Blend>>) {
    wce_foundation::wce_prof!("tile.make_rivers");
    let RiverInput { rivers, variation, new_ids, mut height, mut blend, dims: [(hw, hh), (bw, bh)], frame: (unit, border, mx, mz, ex, ez) } =
        i;
    let alive = |o: &str| {
        parse_owner(o).is_none_or(|o| match (&o.variation, &variation) {
            (Some(a), Some(b)) if a != b => true,
            (None, Some(_)) => false,
            _ => rivers.iter().any(|r| r.id == o.id),
        })
    };
    height.layers.retain(|l| alive(&l.owner));
    if let Some(b) = blend.as_mut() {
        b.layers.retain(|l| alive(&l.owner));
    }

    let (bpx, bpz) = (f64::from(bw) / (ex + 2.0 * border), f64::from(bh) / (ez + 2.0 * border));

    for (river, ids) in rivers.iter().zip(&new_ids) {
        let owner = owner_of(river.id, variation.as_deref(), river.signature);
        let name = match &variation {
            Some(v) => format!("River {} ({v})", river.id),
            None => format!("River {}", river.id),
        };
        let id = river_layer(&mut height, river.id, &variation, &owner, &name, ids[0]);
        let Some(b) = river.bed.bounds() else { continue };
        let px = |v: f64, m: f64, n: u32| (((v + border) / m).floor().max(0.0) as u32).min(n);
        let (x0, y0, x1, y1) = (px(b[0], mx, hw), px(b[1], mz, hh), (px(b[2], mx, hw) + 1).min(hw), (px(b[3], mz, hh) + 1).min(hh));
        // The bed over the rectangle's pixels, then the joins over it.
        let (gw, gh) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let origin = [f64::from(x0) * mx - border, f64::from(y0) * mz - border];
        let bed = river.bed.channel.as_ref().map(|c| c.fill(origin, [mx, mz], (gw, gh)));
        let mut writes = vec![];
        for y in y0..y1 {
            for x in x0..x1 {
                let mut v = [0.0f32];
                height.below(id, x, y, &mut v);
                let g = f64::from(v[0]) / unit;
                let (wx, wz) = (f64::from(x) * mx - border, f64::from(y) * mz - border);
                let b = bed.as_ref().map_or(f64::INFINITY, |b| b[(y - y0) as usize * gw + (x - x0) as usize]);
                if let Some(h) = river.bed.height_with(wx, wz, g, b) {
                    writes.push((x, y, ((h - g) * unit) as f32));
                }
            }
        }

        // The texture where the ground was cut (fading in over its first metres), from the cut at the nearest height pixel.
        let painted = river.texture.filter(|_| blend.is_some()).map(|ch| {
            // The writes by pixel, for the lookups.
            let mut grid = vec![0.0f64; gw * gh];
            for &(x, y, v) in &writes {
                grid[(y - y0) as usize * gw + (x - x0) as usize] = -f64::from(v) / unit;
            }

            let at = |x: f64, z: f64| {
                let (u, v) = (((x + border) / mx).round(), ((z + border) / mz).round());
                if u < f64::from(x0) || v < f64::from(y0) || u >= f64::from(x1) || v >= f64::from(y1) {
                    return 0.0;
                }

                grid[(v as u32 - y0) as usize * gw + (u as u32 - x0) as usize]
            };
            let bx = |v: f64, n: u32| (((v + border) * if n == bw { bpx } else { bpz }).floor().max(0.0) as u32).min(n);
            let (bx0, by0, bx1, by1) = (bx(b[0], bw), bx(b[1], bh), (bx(b[2], bw) + 1).min(bw), (bx(b[3], bh) + 1).min(bh));
            let mut out = vec![];
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (wx, wz) = ((f64::from(x) + 0.5) / bpx - border, (f64::from(y) + 0.5) / bpz - border);
                    let c = at(wx, wz);
                    if c > 0.05 {
                        let t = (c / TEXTURE_FULL_CUT).min(1.0);
                        let w = t * t * (3.0 - 2.0 * t);
                        let mut p = [0u16; 16];
                        p[ch] = (w * 65535.0).round() as u16;
                        out.push((x, y, p));
                    }
                }
            }

            out
        });

        for (x, y, v) in writes {
            height.put(id, x, y, &[v]);
        }

        if let Some(blend) = blend.as_mut() {
            let bid = river_layer(blend, river.id, &variation, &owner, &name, ids[1]);
            for (x, y, p) in painted.into_iter().flatten() {
                blend.put(bid, x, y, &p);
            }
        }
    }

    // The rivers' first among the owned layers, the others as they were.
    height.layers.sort_by_key(|l| (order_of(&l.owner), u8::from(!is_river_owner(&l.owner))));
    if let Some(b) = blend.as_mut() {
        b.layers.sort_by_key(|l| (order_of(&l.owner), u8::from(!is_river_owner(&l.owner))));
    }

    (height, blend)
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
