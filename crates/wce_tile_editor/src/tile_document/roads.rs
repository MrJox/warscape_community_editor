//! Procedural roads in the document (docs/plans/procedural_roads.md). A `WceRoadSpline` owns a layer of each type, made from its line by
//! `wce_tile::roads` over what lies below them. They are derived data: a road changing, or the ground under it, makes them again, so undoing an
//! edit of the road needs nothing of its own. What a road was made from is in its layers' owner (`road:<id>#<signature>`), so that opening a
//! tile does not make its roads again.

use wce_foundation::math::{Double3, Float3};
use wce_scene::render_scene::RenderScene;
use wce_tile::ground_types::GROUND_TYPES;
use wce_tile::new_instances::{NewInstance, element_text};
use wce_tile::road_detail::{Layout, lay, placed_text, read_prefab};
use wce_tile::roads::{RoadSettings, Strip, bridge_pieces, centre_line, find_gaps, graded, over_gaps, with_bump};
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::euler_matrix;
use wce_tile::terrain_layers::{Kind, Stack, TerrainLayers};

use super::Tile;
pub use wce_tile::new_instances::ROAD_SPLINE;

/// The layer of an export that holds what stands along the roads.
pub const KIT_LAYER: &str = "WCE procedural roads";

/// A road as the scene has it.
struct Road {
    id: u32,
    line: Vec<[f64; 2]>,
    settings: RoadSettings,
    signature: u64,
}

fn owner_of(id: u32, signature: u64) -> String {
    format!("road:{id}#{signature:x}")
}

/// The road id and signature in a layer's owner.
fn parse_owner(owner: &str) -> Option<(u32, u64)> {
    let (id, signature) = owner.strip_prefix("road:")?.split_once('#')?;
    Some((id.parse().ok()?, u64::from_str_radix(signature, 16).ok()?))
}

/// What a text hashes to (the same in every run: it is saved).
fn hash_text(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
}

/// A road's line, laid out over what lies below it.
struct Plan {
    line: Vec<[f64; 2]>,
    heights: Vec<f64>,
    gaps: Vec<(usize, usize)>,
}

/// An instance generated along a road: a prefab's part or a bridge's piece.
pub(crate) struct RoadItem {
    road: usize,
    text: Vec<u8>,
    position: [f64; 3],
}

impl Tile {
    pub fn is_road(&self, elem: usize) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == ROAD_SPLINE
    }

    /// The curve a road follows, on the terrain: what its outline draws, so that the line is where the road is.
    pub(super) fn road_world_curve(&self, elem: usize) -> Vec<Float3> {
        let points = self.polygon_world_points(elem).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect::<Vec<_>>();
        let line = centre_line(&points);
        let every = (line.len() / 400).max(2);
        let mut curve: Vec<_> = line.iter().step_by(every).chain(line.last()).map(|p| Float3::new(p[0] as f32, self.terrain_height(p[0] as f32, p[1] as f32), p[1] as f32)).collect();
        curve.dedup();
        curve
    }

    /// What stands along the roads, for a report.
    #[must_use]
    pub fn road_items_report(&self) -> String {
        let text = |t: &[u8]| String::from_utf8_lossy(t).into_owned();
        let bridge = self.road_items.iter().filter(|i| text(&i.text).contains("key=\"bridge_")).count();
        let list = self.road_items.iter().filter(|i| text(&i.text).contains("key=\"bridge_")).map(|i| {
            let t = text(&i.text);
            let key = t.split("key=\"").nth(1).and_then(|k| k.split('"').next()).unwrap_or("");
            let m = self.preview_payloads(&i.text).len();
            format!("{key} at {:.1} {:.1} {:.1} parts {m}", i.position[0], i.position[1], i.position[2])
        }).collect::<Vec<_>>();
        format!("{} instances, {bridge} of them bridge pieces: {}", self.road_items.len(), list.join("; "))
    }

    /// An id for a new road.
    pub(super) fn next_road_id(&self) -> u32 {
        let doc = self.tscene().doc();
        (0..doc.elements().len()).filter(|&e| self.is_road(e)).filter_map(|e| doc.get(e, "road_id")?.parse::<u32>().ok()).max().unwrap_or(0) + 1
    }

    fn roads(&self) -> Vec<Road> {
        let doc = self.tscene().doc();
        let number = |e: usize, name: &str, or: f64| doc.get(e, name).and_then(|v| wce_formats::c_num::strtod(&v)).unwrap_or(or);
        (0..doc.elements().len()).filter(|&e| self.is_road(e)).map(|e| {
            let line = self.polygon_world_points(e).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect::<Vec<_>>();
            let ground = doc.get(e, "road_ground").and_then(|g| GROUND_TYPES.iter().find(|t| t.0 == g.as_str())).map_or([255, 96, 0], |t| t.1);
            let settings = RoadSettings {
                width: number(e, "road_width", 6.0).max(0.5), shoulder: number(e, "road_shoulder", 3.0).max(0.5), bump: number(e, "road_bump", 0.25),
                max_grade: number(e, "road_max_grade", 10.0).clamp(0.5, 60.0), max_side: number(e, "road_max_side", 30.0).clamp(1.0, 80.0),
                channel: (number(e, "road_group", 0.0) as usize).min(3) * 4 + (number(e, "road_variation", 0.0) as usize).min(3), ground,
                bridges: doc.get(e, "road_bridges").is_none_or(|v| v != "false"),
                feather: number(e, "road_feather", 2.0).max(0.0), seed: number(e, "road_seed", 1.0) as u32,
            };

            let signature = hash_text(&format!("{line:?} {settings:?} {:?}", doc.get(e, "road_detail")));
            Road { id: number(e, "road_id", 0.0) as u32, line, settings, signature }
        }).collect()
    }

    /// Makes again the layers of the roads that changed, and takes out those of roads that went. Waits for an open stroke and for a line that
    /// stopped moving: a drag makes its road once, at its end. `sync_terrain_layers` has run.
    pub(super) fn refresh_roads(&mut self) {
        if self.stroke_open {
            return;
        }

        let any = self.idx.valid && self.idx.type_names.iter().any(|t| t == ROAD_SPLINE);
        let owned = |p: &wce_tile::tile_project::TileProject| p.layers.as_ref().and_then(|l| l.height.as_ref()).is_some_and(|s| s.layers.iter().any(|l| l.owner.starts_with("road:")));
        if !any && !owned(&self.project) {
            return;
        }

        let roads = self.roads();
        let stacks = self.project.layers.as_ref().and_then(|l| l.height.as_ref());

        // Each road against what its layers were made from.
        let made = |id: u32| stacks.and_then(|s| s.layers.iter().find_map(|l| parse_owner(&l.owner).filter(|o| o.0 == id).map(|o| o.1)));
        let stale = self.roads_stale || roads.iter().any(|r| made(r.id) != Some(r.signature));
        let gone = stacks.is_some_and(|s| s.layers.iter().filter_map(|l| parse_owner(&l.owner)).any(|o| roads.iter().all(|r| r.id != o.0)));
        if !stale && !gone {
            return;
        }

        // A line still moving waits for the next look.
        let all = roads.iter().fold(0u64, |h, r| h.rotate_left(7) ^ r.signature);
        if all != self.roads_seen {
            self.roads_seen = all;
            return;
        }

        if let Err(why) = self.make_roads(&roads) {
            wce_foundation::log::warn(format!("roads: {why}"));
        }

        self.roads_stale = false;
        self.layers_shown = true;
    }

    /// The road over a ground (`sample`, world units): graded with its gaps taken out, so that it does not dip into them.
    fn plan_road(road: &Road, sample: &dyn Fn(f64, f64) -> f64, on_other: &dyn Fn(f64, f64) -> bool) -> Plan {
        let s = &road.settings;
        let line = centre_line(&road.line);
        let raw = line.iter().map(|p| sample(p[0], p[1])).collect::<Vec<_>>();
        let mut gaps = vec![];
        if s.bridges {
            gaps = find_gaps(&line, &raw, &graded(&line, &raw, s), s);
            for _ in 0..3 {
                let found = find_gaps(&line, &raw, &graded(&line, &over_gaps(&raw, &gaps), s), s);
                if found == gaps {
                    break;
                }

                gaps = found;
            }
        }

        let heights = with_bump(&line, &graded(&line, &over_gaps(&raw, &gaps), s), s, on_other);
        Plan { line, heights, gaps }
    }

    fn make_roads(&mut self, roads: &[Road]) -> Result<(), String> {
        wce_foundation::wce_prof!("tile.make_roads");
        let p = &mut self.project;
        p.load_blend()?;
        p.load_ground()?;
        let unit = p.unit_scale;
        let (border, ex, ez, mx, mz) = (p.border_m(), p.extent_x(), p.extent_z(), p.mpp_x(), p.mpp_z());
        let (Some(hf), Some(bf), Some(gf)) = (p.height.as_mut(), p.blend.as_mut(), p.ground.as_mut()) else { return Err("the tile has no height map".to_owned()) };
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let new_ids = roads.iter().map(|_| [layers.new_id(), layers.new_id(), layers.new_id()]).collect::<Vec<_>>();
        layers.dirty = true;
        let height = layers.height.get_or_insert_with(|| Stack::new(hf));
        let blend = layers.blend.get_or_insert_with(|| Stack::new(bf));
        let ground = layers.ground.get_or_insert_with(|| Stack::new(gf));
        let alive = |o: &str| parse_owner(o).is_none_or(|(id, _)| roads.iter().any(|r| r.id == id));
        height.layers.retain(|l| alive(&l.owner));
        blend.layers.retain(|l| alive(&l.owner));
        ground.layers.retain(|l| alive(&l.owner));

        let (hw, hh) = (hf.width(), hf.height());
        let (bw, bh) = (bf.width(), bf.height());
        let (gw, gh) = (gf.width(), gf.height());
        let (bpx, bpz) = (f64::from(bw) / (ex + 2.0 * border), f64::from(bh) / (ez + 2.0 * border));
        let (gpx, gpz) = (f64::from(gw) / (ex + 2.0 * border), f64::from(gh) / (ez + 2.0 * border));
        let mut earlier: Vec<Strip> = vec![];
        for (road, ids) in roads.iter().zip(&new_ids) {
            if road.line.len() < 2 {
                continue;
            }

            let owner = owner_of(road.id, road.signature);
            let name = format!("Road {}", road.id);
            let height_id = road_layer(height, road.id, &owner, &name, ids[0]);
            let blend_id = road_layer(blend, road.id, &owner, &name, ids[1]);
            let ground_id = road_layer(ground, road.id, &owner, &name, ids[2]);

            // What lies below the road, in world units.
            let under = |x: u32, y: u32| {
                let mut px = [0.0f32];
                height.below(height_id, x.min(hw - 1), y.min(hh - 1), &mut px);
                f64::from(px[0]) / unit
            };

            let sample = |x: f64, z: f64| {
                let (u, v) = (((x + border) / mx).clamp(0.0, f64::from(hw - 1)), ((z + border) / mz).clamp(0.0, f64::from(hh - 1)));
                let (x0, y0) = (u.floor() as u32, v.floor() as u32);
                let (fx, fy) = (u - f64::from(x0), v - f64::from(y0));
                let top = under(x0, y0) + (under(x0 + 1, y0) - under(x0, y0)) * fx;
                let bottom = under(x0, y0 + 1) + (under(x0 + 1, y0 + 1) - under(x0, y0 + 1)) * fx;
                top + (bottom - top) * fy
            };

            let plan = Self::plan_road(road, &sample, &|x, z| earlier.iter().any(|s| s.on_road(x, z)));
            let mut strip = Strip::new(plan.line, plan.heights, road.settings.clone());
            strip.open_gaps(&plan.gaps);
            let b = strip.bounds();
            let rect = |pp: (f64, f64), off: f64, w: u32, h: u32| {
                let lo = |v: f64, n: u32| (v.floor().max(0.0) as u32).min(n);
                [lo((b[0] + off) * pp.0, w), lo((b[1] + off) * pp.1, h), lo(((b[2] + off) * pp.0).ceil(), w), lo(((b[3] + off) * pp.1).ceil(), h)]
            };

            let mut writes = vec![];
            let [x0, y0, x1, y1] = rect((1.0 / mx, 1.0 / mz), border, hw, hh);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (wx, wz) = (f64::from(x) * mx - border, f64::from(y) * mz - border);
                    let g = under(x, y);
                    if let Some(h) = strip.height_at(wx, wz, g) {
                        writes.push((x, y, ((h - g) * unit) as f32));
                    }
                }
            }

            for (x, y, v) in writes {
                height.put(height_id, x, y, &[v]);
            }

            let [x0, y0, x1, y1] = rect((bpx, bpz), border, bw, bh);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (wx, wz) = ((f64::from(x) + 0.5) / bpx - border, (f64::from(y) + 0.5) / bpz - border);
                    let w = strip.texture_at(wx, wz);
                    if w > 0.0 {
                        let mut px = [0u16; 16];
                        px[road.settings.channel] = (w * 65535.0).round() as u16;
                        blend.put(blend_id, x, y, &px);
                    }
                }
            }

            let [c0, c1, c2] = road.settings.ground;
            let [x0, y0, x1, y1] = rect((gpx, gpz), border, gw, gh);
            for y in y0..y1 {
                for x in x0..x1 {
                    if strip.on_road((f64::from(x) + 0.5) / gpx - border, (f64::from(y) + 0.5) / gpz - border) {
                        ground.put(ground_id, x, y, &[c0, c1, c2, 255]);
                    }
                }
            }

            earlier.push(strip);
        }

        height.apply_all(hf);
        blend.apply_all(bf);
        ground.apply_all(gf);
        height.remember();
        blend.remember();
        ground.remember();
        self.road_items_stale = true;
        Ok(())
    }

    /// The instances along the roads: the prefabs, and the bridges over their gaps. Derived as their layers are, so made again, not saved.
    pub(super) fn make_road_items(&mut self) {
        wce_foundation::wce_prof!("tile.make_road_items");
        self.road_items_stale = false;
        let unit = self.project.unit_scale;
        let roads = self.roads();
        let mut items = vec![];
        for (elem, road) in self.road_elems().into_iter().zip(&roads) {
            if road.line.len() < 2 {
                continue;
            }

            // Gaps are the road's own: found on what lies below its layer.
            let below = self.project.layers.as_ref().and_then(|l| l.height.as_ref()).and_then(|s| s.layers.iter().find(|l| parse_owner(&l.owner).is_some_and(|o| o.0 == road.id)).map(|l| (s, l.id)));
            let (w, h) = self.project.height.as_ref().map_or((1, 1), |f| (f.width(), f.height()));
            let (border, mx, mz) = (self.project.border_m(), self.project.mpp_x(), self.project.mpp_z());
            let sample = |x: f64, z: f64| match below {
                Some((s, id)) => {
                    let mut px = [0.0f32];
                    let (u, v) = (((x + border) / mx).round().clamp(0.0, f64::from(w - 1)) as u32, ((z + border) / mz).round().clamp(0.0, f64::from(h - 1)) as u32);
                    s.below(id, u, v, &mut px);
                    f64::from(px[0]) / unit
                }

                None => self.project.height_at(x, z) / unit,
            };

            let plan = Self::plan_road(road, &sample, &|_, _| false);
            let mut spans = vec![];
            let along = |i: usize| plan.line[..=i].windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>();
            for &(a, b) in &plan.gaps {
                spans.push((along(a), along(b)));
                let ((p, q), (ya, yb)) = ((plan.line[a], plan.line[b]), (plan.heights[a], plan.heights[b]));
                let (dx, dz) = (q[0] - p[0], q[1] - p[1]);
                let length = dx.hypot(dz);
                let (tx, tz) = (dx / length.max(1e-9), dz / length.max(1e-9));
                let yaw = tx.atan2(tz);
                for (key, offset, back) in bridge_pieces(length * unit) {
                    let f = 0.5 + offset / (length * unit);
                    let (x, z) = (p[0] + dx * 0.5 + tx * offset / unit, p[1] + dz * 0.5 + tz * offset / unit);
                    let y = ya + (yb - ya) * f.clamp(0.0, 1.0);
                    let position = [x * unit, y * unit, z * unit];
                    if let Some(text) = bridge_text(key, position, if back { yaw + std::f64::consts::PI } else { yaw }) {
                        items.push(RoadItem { road: elem, text, position });
                    }
                }
            }

            let layouts = self.road_layouts(self.road_detail_of(elem).as_str());
            let half = road.settings.width * 0.5 + road.settings.shoulder;
            let terrain = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
            let skip = |s: f64| spans.iter().any(|&(a, b): &(f64, f64)| s > a - 1.0 && s < b + 1.0);
            for laid in lay(&layouts, &plan.line, unit, half, &terrain, &skip, road.settings.seed) {
                items.push(RoadItem { road: elem, text: laid.text, position: laid.position });
            }

        }

        self.road_items = items;
        self.road_items_shown = false;
    }

    fn road_elems(&self) -> Vec<usize> {
        (0..self.tscene().doc().elements().len()).filter(|&e| self.is_road(e)).collect()
    }

    fn road_detail_of(&self, elem: usize) -> String {
        self.tscene().doc().get(elem, "road_detail").unwrap_or_else(|| "dirt".to_owned())
    }

    /// The prefabs of a road's detail, read once.
    fn road_layouts(&self, detail: &str) -> Vec<Layout> {
        let Some(lib) = &self.road_prefabs else { return vec![] };
        let prefix = match detail {
            "dirt" => "global_dirt_road",
            "path" => "global_path",
            _ => return vec![],
        };

        let mut keys = lib.entries().filter(|e| e.key.starts_with(prefix)).map(|e| e.key.clone()).collect::<Vec<_>>();
        keys.sort();
        keys.iter().filter_map(|k| lib.load(k).ok()).filter_map(|p| read_prefab(&p.group_text)).collect()
    }

    /// Puts the instances along the roads on the scene.
    pub(super) fn show_road_items(&mut self, scene: &mut RenderScene) {
        if self.road_items_stale && self.road_prefabs.is_some() {
            self.make_road_items();
        }

        if self.road_items_shown {
            return;
        }

        self.road_items_shown = true;
        for id in self.road_entities.drain(..).collect::<Vec<_>>() {
            scene.remove(id);
        }

        let unit = self.unit();
        let mut ids = vec![];
        for item in &self.road_items {
            let source = self.source(super::item_of(item.road));
            let [x, y, z] = item.position;
            for (local, payload) in self.preview_payloads(&item.text) {
                let mut world = local;
                world.m[0][3] += (x / unit) as f32;
                world.m[1][3] += (y / unit) as f32;
                world.m[2][3] += (z / unit) as f32;
                ids.push(scene.add(self.layer, world, payload, source));
            }
        }

        self.road_entities = ids;
    }

    /// The layer of generated instances an export adds: what stands along the roads, as TEd and the game read it.
    pub(super) fn with_road_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.road_items.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), KIT_LAYER)?;
        for item in &self.road_items {
            scene.insert_element_text(layer, &item.text)?;
        }

        Ok(scene.save())
    }
}

/// A piece of a bridge, as the kit's tiles place them: freeform, level, along its z.
fn bridge_text(key: &str, position: [f64; 3], yaw: f64) -> Option<Vec<u8>> {
    let n = NewInstance::new("BUILDING_INSTANCE").position(Double3::new(position[0], position[1], position[2])).attr("key", key).attr("name", key).attr("surface_mode", "MSM_FREEFORM");
    placed_text(&element_text(&n).ok()?, position, &euler_matrix(Double3::new(yaw, 0.0, 0.0)))
}

/// A road's layer in a stack, emptied: the one it has, else a new one.
fn road_layer<K: Kind>(s: &mut Stack<K>, road: u32, owner: &str, name: &str, id: u64) -> u64 {
    if let Some(l) = s.layers.iter_mut().find(|l| parse_owner(&l.owner).is_some_and(|o| o.0 == road)) {
        l.owner = owner.to_owned();
        let id = l.id;
        s.clear_layer(id);
        return id;
    }

    s.add(id, name, owner)
}
