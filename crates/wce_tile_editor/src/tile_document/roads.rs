//! Procedural roads in the document (docs/plans/procedural_roads.md). A `WceRoadSpline` owns a layer of each type, made from its line by
//! `wce_tile::roads` over what lies below them. They are derived data: a road changing, or the ground under it, makes them again, so undoing an
//! edit of the road needs nothing of its own. What a road was made from is in its layers' owner (`road:<id>#<signature>`), so that opening a
//! tile does not make its roads again.

use wce_foundation::math::{Double3, Double3x3, Float3};
use wce_scene::render_scene::RenderScene;
use wce_tile::ground_types::GROUND_TYPES;
use wce_tile::road_layout::{End, Hold, Measures, Site, Stations, fit, junctions, layout, snap_ends};
use wce_tile::road_pieces::{BASE, PIECES, Piece, key, read};
use wce_tile::roads::{RoadSettings, Strip, WaterMap, centre_line_sharp, sharp_points};
use wce_tile::ted_scene::TedScene;
use wce_tile::terrain_layers::{Kind, Stack, TerrainLayers};

use super::Tile;
pub use wce_tile::new_instances::ROAD_SPLINE;

/// The layer of an export that holds what stands along the roads.
pub const KIT_LAYER: &str = "WCE procedural roads";

/// A road as the scene has it.
struct Road {
    id: u32,
    line: Vec<[f64; 2]>,

    /// Which of its points are corners (`roads::sharp_points`).
    sharp: Vec<bool>,
    settings: RoadSettings,

    /// Its road type: the folder of `roads_proc` its pieces come from (`none`: no pieces).
    road_type: String,

    /// The base its bridges are made from (`wce_walls::bridge::BASES`).
    bridge: String,
    signature: u64,
}

/// A road type's pieces, read: each one's name, what it reads as, its prefab's key and contents.
type Pieces = Vec<(&'static str, Piece, String, Vec<u8>)>;

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

    /// The sample of each of the road's points.
    at_point: Vec<usize>,
    heights: Vec<f64>,
    gaps: Vec<(usize, usize)>,

    /// Its fords: the road on the bed, the terrain's heights left as they are.
    fords: Vec<(usize, usize)>,
}

/// An instance generated along a road: a prefab's part or a bridge's piece.
pub(crate) struct RoadItem {
    road: usize,
    text: Vec<u8>,
    position: [f64; 3],

    /// A generated building's models (a bridge's piece, in `wce_vfs::generated`): drawn as they are, not from `text`.
    meshes: Vec<String>,

    /// What an export writes for it, when not its text (a bridge: its `WceBridgeSpan` with its first piece, nothing with the others).
    export: Option<Vec<u8>>,
}

impl Tile {
    pub fn is_road(&self, elem: usize) -> bool {
        elem < self.tscene().doc().elements().len() && self.tscene().type_of(elem) == ROAD_SPLINE
    }

    /// The curve a road follows, on the terrain: what its outline draws, so that the line is where the road is.
    pub(super) fn road_world_curve(&self, elem: usize) -> Vec<Float3> {
        let points = self.polygon_world_points(elem).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect::<Vec<_>>();
        let line = centre_line_sharp(&points, &self.road_sharp(elem, &points)).0;
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
        let owned = self.project.layers.as_ref().map_or([0; 3], |l| {
            let count = |n: Option<Vec<&str>>| n.map_or(0, |v| v.iter().filter(|o| o.starts_with("road:")).count());
            [
                count(l.height.as_ref().map(|s| s.layers.iter().map(|x| x.owner.as_str()).collect())),
                count(l.blend.as_ref().map(|s| s.layers.iter().map(|x| x.owner.as_str()).collect())),
                count(l.ground.as_ref().map(|s| s.layers.iter().map(|x| x.owner.as_str()).collect())),
            ]
        });

        // The road pieces laid, by their prefab (a piece's group is named after its key).
        let mut pieces = std::collections::BTreeMap::<String, usize>::new();
        for i in &self.road_items {
            let t = text(&i.text);
            if t.starts_with("<UserInstanceGroup") && let Some(name) = t.split("name=\"").nth(1).and_then(|n| n.split('"').next()) {
                *pieces.entry(name.to_owned()).or_default() += 1;
            }
        }

        let pieces = pieces.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", ");
        format!("{} instances, road layers {owned:?}, pieces: {pieces}; {bridge} bridge pieces: {}", self.road_items.len(), list.join("; "))
    }

    /// Whether a road is at a place (world units), by its ground type layer.
    pub(super) fn road_at(&self, x: f64, z: f64) -> bool {
        let p = &self.project;
        let Some(s) = p.layers.as_ref().and_then(|l| l.ground.as_ref()) else { return false };
        let Some(f) = p.ground.as_ref() else { return false };
        let border = p.border_m();
        let (u, v) = ((x + border) * f64::from(f.width()) / (p.extent_x() + 2.0 * border), (z + border) * f64::from(f.height()) / (p.extent_z() + 2.0 * border));
        u >= 0.0 && v >= 0.0 && s.owned_at("road:", u as u32, v as u32, |px| px[3] == 0)
    }

    /// An id for a new road.
    pub(super) fn next_road_id(&self) -> u32 {
        let doc = self.tscene().doc();
        (0..doc.elements().len()).filter(|&e| self.is_road(e)).filter_map(|e| doc.get(e, "road_id")?.parse::<u32>().ok()).max().unwrap_or(0) + 1
    }

    /// A road's corners: its points marked `sharp`, and those it turns at by 90 degrees or more.
    fn road_sharp(&self, elem: usize, points: &[[f64; 2]]) -> Vec<bool> {
        let sc = self.tscene();
        let marked = sc.polygon_points(elem).iter().map(|&p| sc.doc().get(p, "sharp").is_some_and(|v| wce_tile::properties::parse_bool(&v))).collect::<Vec<_>>();
        sharp_points(points, &marked)
    }

    /// The roads, their ends taken onto the roads they stop near (`road_layout::snap_ends`): what their terrain and their pieces are made
    /// from, so a road reaches the junction it makes.
    fn roads(&self) -> Vec<Road> {
        let doc = self.tscene().doc();
        let number = |e: usize, name: &str, or: f64| doc.get(e, name).and_then(|v| wce_formats::c_num::strtod(&v)).unwrap_or(or);
        let elems = self.road_elems();
        let mut lines = elems.iter().map(|&e| self.polygon_world_points(e).iter().map(|p| [f64::from(p.x), f64::from(p.z)]).collect::<Vec<_>>()).collect::<Vec<_>>();
        let centres = elems.iter().zip(&lines).map(|(&e, l)| centre_line_sharp(l, &self.road_sharp(e, l)).0).collect::<Vec<_>>();
        snap_ends(&mut lines, &centres);

        // The water a road crosses is in what it is made from: a river or a lake moved makes it again.
        let sc = self.tscene();
        let wet = |e: usize| sc.type_of(e) == "SplineRiverInstance" || (sc.type_of(e) == "SIMPLE_MESH_INSTANCE" && sc.doc().get(e, "key").is_some_and(|k| k.to_lowercase().contains("water_plane")));
        let water = (0..sc.doc().elements().len()).filter(|&e| wet(e)).fold(0u64, |h, e| h.rotate_left(5) ^ hash_text(&String::from_utf8_lossy(&sc.element_text(e))));
        elems.into_iter().zip(lines).map(|(e, line)| {
            let ground = doc.get(e, "road_ground").and_then(|g| GROUND_TYPES.iter().find(|t| t.0 == g.as_str())).map_or([255, 96, 0], |t| t.1);
            let settings = RoadSettings {
                width: number(e, "road_width", 6.0).max(0.5), shoulder: number(e, "road_shoulder", 3.0).max(0.5), bump: number(e, "road_bump", 0.25),
                max_grade: number(e, "road_max_grade", 10.0).clamp(0.5, 60.0), max_side: number(e, "road_max_side", 30.0).clamp(1.0, 80.0),
                channel: (number(e, "road_group", 0.0) as usize).min(3) * 4 + (number(e, "road_variation", 0.0) as usize).min(3), ground,
                bridges: doc.get(e, "road_bridges").is_none_or(|v| v != "false"), clearance: number(e, "road_bridge_clearance", 2.0).max(0.0),
                feather: number(e, "road_feather", 2.0).max(0.0), seed: number(e, "road_seed", 1.0) as u32,
            };

            let sharp = self.road_sharp(e, &line);
            let road_type = doc.get(e, "road_type").filter(|t| !t.is_empty()).unwrap_or_else(|| wce_tile::road_dirt::TYPE.to_owned());
            let bridge = doc.get(e, "road_bridge").filter(|b| wce_walls::bridge::base(b).is_some()).unwrap_or_else(|| wce_walls::bridge::BASES[0].name.to_owned());
            let signature = hash_text(&format!("{line:?} {sharp:?} {settings:?} {road_type} {bridge} {water:x}"));
            Road { id: number(e, "road_id", 0.0) as u32, line, sharp, settings, road_type, bridge, signature }
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
        let on_top = self.project.layers.as_ref().is_none_or(|l| {
            l.height.as_ref().is_none_or(Stack::owned_on_top) && l.blend.as_ref().is_none_or(Stack::owned_on_top) && l.ground.as_ref().is_none_or(Stack::owned_on_top)
        });

        let stale = self.roads_stale || !on_top || roads.iter().any(|r| made(r.id) != Some(r.signature));
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

        // Grass stands by the blend map and on the heights, and neither is what it was.
        self.reload_grass();
    }

    /// The road over a ground (`sample`, world units) and the water on it (`roads::plan`): fords where it is shallow, bridges where it is
    /// deep or the ground drops away, graded with those taken out so that it does not dip into them.
    fn plan_road(road: &Road, sample: &dyn Fn(f64, f64) -> f64, water: &WaterMap, on_other: &dyn Fn(f64, f64) -> bool) -> Plan {
        let (line, at_point) = centre_line_sharp(&road.line, &road.sharp);
        let raw = line.iter().map(|p| sample(p[0], p[1])).collect::<Vec<_>>();
        let wet = line.iter().map(|p| water.level(p[0], p[1])).collect::<Vec<_>>();
        let p = wce_tile::roads::plan(&line, &raw, &wet, &road.settings, on_other);
        Plan { line, at_point, heights: p.heights, gaps: p.gaps, fords: p.fords }
    }

    /// The tile's water, as a road crossing it finds it (world metres): its rivers' ribbons as they are drawn, its lakes' water planes, and
    /// the sea at 0 m in a tile of the sea's sets (`sea`, `sea_coast`, `sea_coast_landing`, `river_mouth`: docs/re/water.md).
    pub(super) fn water_map(&self) -> WaterMap {
        let rivers = self.river_ribbons().into_iter().map(|(_, samples)| samples.iter().map(|s| [s.x, s.z, s.y, s.width * 0.5]).collect()).collect();
        let sc = self.tscene();
        let planes = sc.find_all("SIMPLE_MESH_INSTANCE").into_iter().filter(|&e| sc.doc().get(e, "key").is_some_and(|k| k.to_lowercase().contains("water_plane"))).filter_map(|e| {
            let m = self.model_of(e).filter(|m| m.ok && m.resolved)?;
            let w = self.world_matrix(e);
            let corner = |x: f32, z: f32| { let p = w.transform_point(Float3::new(x, 0.0, z)); [f64::from(p.x), f64::from(p.z)] };
            let y = f64::from(w.transform_point(Float3::new(0.0, 0.0, 0.0)).y);
            Some(([corner(m.bmin.x, m.bmin.z), corner(m.bmax.x, m.bmin.z), corner(m.bmax.x, m.bmax.z), corner(m.bmin.x, m.bmax.z)], y))
        }).collect();

        let dir = self.project.dir.to_string_lossy().replace('\\', "/").to_lowercase();
        let set = dir.split("/battle/").nth(1).and_then(|r| r.split('/').next()).unwrap_or_default().to_owned();
        let sea = matches!(set.as_str(), "sea" | "sea_coast" | "sea_coast_landing" | "river_mouth");
        WaterMap { rivers, planes, sea }
    }

    fn make_roads(&mut self, roads: &[Road]) -> Result<(), String> {
        wce_foundation::wce_prof!("tile.make_roads");
        let water = self.water_map();
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
        height.put_owned_on_top();
        blend.put_owned_on_top();
        ground.put_owned_on_top();

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

            let plan = Self::plan_road(road, &sample, &water, &|x, z| earlier.iter().any(|s| s.on_road(x, z)));
            let mut strip = Strip::new(plan.line, plan.heights, road.settings.clone());
            strip.open_gaps(&plan.gaps);
            strip.keep_heights(&plan.fords);
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

        // Each road's line and bridges first: where roads meet is found on all their lines.
        let water = self.water_map();
        let mut planned = vec![];
        let mut decks = vec![];
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

            let plan = Self::plan_road(road, &sample, &water, &|_, _| false);
            // Each bridge's deck from rim to rim: the road's line and height over its gap (scene units).
            let st = Stations::new(&plan.line);
            let mut spans = vec![];
            // Each crossing's AI hint, as vanilla's bridges and fords have one (`road_layout::crossing_hint`), for an export.
            let width = road.settings.width;
            let crossings = plan.gaps.iter().map(|&g| (g, width * 0.5 + wce_tile::road_layout::HINT_SIDE))
                .chain(plan.fords.iter().map(|&g| (g, (width * 0.5 + wce_tile::road_layout::HINT_SIDE).max(wce_tile::road_layout::FORD_HINT_HALF))));
            for ((a, b), half) in crossings {
                let ends = wce_tile::road_layout::HINT_ENDS;
                let outline = wce_tile::road_layout::strip_outline(&plan.line, st.of_sample(a) - ends, st.of_sample(b) + ends, half, unit);
                match wce_tile::road_layout::crossing_hint(&outline) {
                    Ok(hint) => items.push(RoadItem { road: elem, text: vec![], position: [0.0; 3], meshes: vec![], export: Some(hint) }),
                    Err(why) => wce_foundation::log::warn(format!("road {}: {why}", road.id)),
                }
            }

            for &(a, b) in &plan.gaps {
                spans.push((st.of_sample(a), st.of_sample(b)));
                let path = (a..=b).map(|i| [plan.line[i][0] * unit, plan.heights[i] * unit, plan.line[i][1] * unit]).collect::<Vec<_>>();
                decks.push((elem, road.id, road.bridge.clone(), road.settings.width * unit, path));
            }

            planned.push((elem, road, plan, spans));
        }

        // The pieces of each road's type, its measures, and how far its junctions reach.
        let kit = planned.iter().map(|(_, road, _, _)| {
            let pieces = self.road_pieces_of(&road.road_type);
            let arm = |name: &str| pieces.iter().find(|p| p.0 == name).and_then(|p| p.1.arms.first()).map(|a| a.length);
            let defaults = Measures::default();
            let measures = Measures {
                straight: arm("straight").unwrap_or(defaults.straight),
                end: arm("end").unwrap_or(defaults.end),
                corner_arm: arm("corner").unwrap_or(defaults.corner_arm),
            };

            let width = pieces.iter().find(|p| p.0 == "straight").map_or(wce_tile::road_pieces::WIDTH, |p| p.1.width).max(1e-6);
            let scale = road.settings.width / width;
            let half = arm("x_junction").or_else(|| arm("t_junction")).unwrap_or(defaults.corner_arm) * scale;
            (pieces, measures, scale, half)
        }).collect::<Vec<_>>();

        let lines = planned.iter().map(|p| p.2.line.clone()).collect::<Vec<_>>();
        let meet = junctions(&lines, &kit.iter().map(|k| k.3).collect::<Vec<_>>());
        for note in &meet.notes {
            wce_foundation::log::warn(format!("roads: {note}"));
        }

        // Each road's pieces, from end to end, but over its bridges and the junctions of other roads.
        let (ex, ez) = (self.project.extent_x(), self.project.extent_z());
        let end_of = |p: [f64; 2], joined: bool| if joined { End::Joined } else if p[0] < 1.0 || p[1] < 1.0 || p[0] > ex - 1.0 || p[1] > ez - 1.0 { End::Edge } else { End::Free };
        let terrain = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
        for (r, ((elem, road, plan, spans), (pieces, measures, scale, _))) in planned.iter().zip(&kit).enumerate() {
            if pieces.is_empty() {
                continue;
            }

            let piece = |name: &str| pieces.iter().find(|p| p.0 == name);
            let corners = plan.at_point.iter().zip(&road.sharp).filter(|(_, s)| **s).map(|(i, _)| *i).collect::<Vec<_>>();
            let joined = meet.joined[r];
            let ends = [end_of(road.line[0], joined[0]), end_of(road.line[road.line.len() - 1], joined[1])];
            let mut holds = spans.iter().map(|&(from, to)| Hold { from, to, laid: None }).collect::<Vec<_>>();

            // A ford: ford pieces from bank to bank, as long as straights.
            let st = Stations::new(&plan.line);
            for &(a, b) in &plan.fords {
                let (from, to) = (st.of_sample(a), st.of_sample(b));
                let n = (((to - from) / measures.straight).round() as usize).max(1);
                for k in 0..n {
                    let (s0, s1) = (from + (to - from) * k as f64 / n as f64, from + (to - from) * (k + 1) as f64 / n as f64);
                    holds.push(Hold { from: s0, to: s1, laid: Some(wce_tile::road_layout::Laid { piece: "ford", frame: wce_tile::road_layout::Frame::Along { from: s0, to: s1 } }) });
                }
            }

            holds.extend(meet.holds[r].iter().cloned());
            let laid = layout(&plan.line, &corners, ends, &holds, measures, *scale, road.settings.seed);
            for note in &laid.notes {
                wce_foundation::log::warn(format!("road {}: {note}", road.id));
            }

            let width = road.settings.width;
            for l in &laid.laid {
                let Some((_, p, key, children)) = piece(l.piece) else { continue };
                match fit(p, children, key, l, &Site { line: &plan.line, width, unit, ground: &terrain }) {
                    Ok(f) => items.push(RoadItem { road: *elem, text: f.text, position: f.position, meshes: vec![], export: None }),
                    Err(why) => wce_foundation::log::warn(format!("road {}: {key}: {why}", road.id)),
                }
            }
        }

        // The bridges, generated: what was made for the last ones goes first.
        for key in std::mem::take(&mut self.road_generated) {
            wce_vfs::generated::remove_prefix(&wce_walls::building::folder(&key));
        }

        for (elem, id, base, width, path) in decks {
            items.extend(self.bridge_items(elem, id, &base, width, path));
        }

        self.road_items = items;
        self.road_items_shown = false;
    }

    /// A road's bridge over a gap (`path`: its deck from rim to rim, scene units), made from its base as walls are (`wce_walls::bridge`):
    /// its pieces' files put among the generated ones under keys of this making, drawn as they are; an export writes the bridge's span
    /// instead, to be made again there (`wce_walls::export::lower_walls`).
    fn bridge_items(&mut self, elem: usize, road: u32, base: &str, width: f64, path: Vec<[f64; 3]>) -> Vec<RoadItem> {
        let Some(key) = wce_walls::bridge::base(base).map(|b| b.key) else { return vec![] };
        let Some(prepared) = self.bridge_base(key) else {
            wce_foundation::log::warn(format!("road {road}: the bridge base {key} is in no pack: no bridge"));
            return vec![];
        };

        let unit = self.unit();
        let deck = wce_walls::bridge::Deck::new(path.clone());
        let across = width / (prepared.z.1 - prepared.z.0).max(1e-6);
        self.road_making += 1;
        let making = self.road_making;
        let ground = |x: f64, z: f64| f64::from(self.terrain_height((x / unit) as f32, (z / unit) as f32)) * unit;
        let mut out = vec![];
        let mut keys = vec![];
        for (k, (s, origin)) in wce_walls::bridge::piece_stretches(&deck, &prepared).into_iter().enumerate() {
            let key = format!("wce_preview_road{road}_{making}_{k:02}");
            let warp = wce_walls::bridge::BridgeWarp { deck: &deck, base: &prepared, s, across, origin, ground: &ground };
            let made = match wce_walls::bridge::make_piece(&prepared, &key, &warp) {
                Ok(m) => m,
                Err(err) => {
                    wce_foundation::log::warn(format!("road {road}: bridge piece {k}: {err}"));
                    continue;
                }
            };

            let mut meshes = vec![];
            for (path, bytes) in made.into_pack_files() {
                if path.ends_with(".rigid_model_v2") && path.contains("_destruct01") && !path.contains("_anim") {
                    meshes.push(path.clone());
                }

                wce_vfs::generated::put(&path, bytes);
            }

            let export = Some(if k == 0 { wce_walls::bridge::span_text(base, width, &path) } else { vec![] });
            out.push(RoadItem { road: elem, text: vec![], position: origin, meshes, export });
            keys.push(key);
        }

        self.road_generated.extend(keys);
        out
    }

    /// A bridge base, turned and measured (`wce_walls::bridge::prepare`), read once.
    fn bridge_base(&mut self, key: &str) -> Option<std::sync::Arc<wce_walls::bridge::Prepared>> {
        if let Some(b) = self.bridge_bases.get(key) {
            return b.clone();
        }

        let made = self.files.as_deref().and_then(|f| wce_walls::building::BuildingFiles::read_files(f, key).ok()).and_then(|b| match wce_walls::bridge::prepare(&b) {
            Ok(p) => Some(std::sync::Arc::new(p)),
            Err(err) => {
                wce_foundation::log::warn(format!("bridge base {key}: {err}"));
                None
            }
        });
        self.bridge_bases.insert(key.to_owned(), made.clone());
        made
    }

    /// Whether the roads have generated bridges: an export makes them as walls are made.
    #[must_use]
    pub fn has_road_bridges(&self) -> bool {
        self.road_items.iter().any(|i| i.export.as_ref().is_some_and(|t| t.starts_with(format!("<{}", wce_walls::bridge::BRIDGE_SPAN).as_bytes())))
    }

    fn road_elems(&self) -> Vec<usize> {
        (0..self.tscene().doc().elements().len()).filter(|&e| self.is_road(e)).collect()
    }

    /// A road type's pieces (`roads_proc/<type>`), each its type's or, missing there, the base's (`none`: none).
    fn road_pieces_of(&self, road_type: &str) -> Pieces {
        let Some(lib) = &self.road_prefabs else { return vec![] };
        if road_type == "none" {
            return vec![];
        }

        PIECES.iter().filter_map(|&name| {
            let found = [key(road_type, name), key(BASE, name)].into_iter().find(|k| lib.get(k).is_some());
            let Some(k) = found else {
                wce_foundation::log::warn(format!("roads: no {} piece for the road type {road_type}, nor a base one", name));
                return None;
            };

            if k != key(road_type, name) {
                wce_foundation::log::warn(format!("roads: the road type {road_type} has no {name}: the base's is laid (guides only)"));
            }

            let prefab = lib.load(&k).map_err(|e| wce_foundation::log::warn(format!("roads: {k}: {e}"))).ok()?;
            let piece = read(&prefab.group_text).map_err(|e| wce_foundation::log::warn(format!("roads: {k} is not a road piece: {e}"))).ok()?;
            Some((name, piece, k, prefab.children_text))
        }).collect()
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
        for id in std::mem::take(&mut self.road_entities) {
            scene.remove(id);
        }

        let unit = self.unit();
        let mut ids = vec![];
        for item in &self.road_items {
            let source = self.source(super::item_of(item.road));
            let [x, y, z] = item.position;
            if !item.meshes.is_empty() {
                let wt = wce_tile::world_transform::WorldTransform { position: Double3::new(x, y, z), rotation: Double3x3::default() };
                let world = super::index::matrix_of(&wt, unit, Some(Double3::new(1.0 / unit, 1.0 / unit, 1.0 / unit)));
                for model in &item.meshes {
                    let mi = wce_scene::render_scene::MeshInstance { model: model.clone(), lod: true, ..Default::default() };
                    ids.push(scene.add(self.layer, world, wce_scene::render_scene::Payload::MeshInstance(mi), source));
                }

                continue;
            }

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
            match &item.export {
                Some(t) if t.is_empty() => {}
                Some(t) => {
                    scene.insert_element_text(layer, t)?;
                }

                None => {
                    scene.insert_element_text(layer, &item.text)?;
                }
            }
        }

        Ok(scene.save())
    }
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
