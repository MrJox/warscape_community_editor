//! Shores in the tile (docs/plans/rivers.md, step 10; `wce_tile::shores`): reeds and stones along the water of a river or a lake with
//! `wce_shores="1"` (the rivers and lakes the editor makes; the kit's are left as they are until turned on), made again off the editor's
//! thread when the water or the ground by it changes and stays so for a look, drawn locked (they are the water's, not tile.ted's) and
//! added at export in a layer of their own, as the biome areas' contents are.

use wce_foundation::math::Double3;
use wce_scene::render_scene::RenderScene;
use wce_tile::shores::{AMOUNTS, BRIDGE_MARGIN, Cover, ShoreSettings, Water, amount};
use wce_tile::ted_scene::TedScene;

use super::biomes::{BiomeItem, item_text};
use super::river_links::RIVER_ID_ATTR;
use super::{Tile, TileDocument};

/// A river or a lake has a shore (`1`), or not (`0`, missing).
pub const SHORES_ATTR: &str = "wce_shores";

/// How thick its reeds and its stones are: an amount's name (`wce_tile::shores::AMOUNTS`); missing: `some` of each.
pub const REEDS_ATTR: &str = "wce_shore_reeds";
pub const STONES_ATTR: &str = "wce_shore_stones";

/// A river's fords splash at their edges (`1`, missing) or not (`0`).
pub const SPLASHES_ATTR: &str = "wce_ford_splashes";

/// How many decals lie along a river's shore (muddy rocks over the waterline) and on its bed (mud under the water): an amount's name;
/// missing: `some`.
pub const DECALS_ATTR: &str = "wce_shore_decals";
pub const BED_DECALS_ATTR: &str = "wce_bed_decals";

/// How many rapids a river has (rocks out on its water, splashing): an amount's name; missing: `none`.
pub const RAPIDS_ATTR: &str = "wce_shore_rapids";
const DEFAULT_RAPIDS: &str = "none";

/// The attributes the Inspector shows as rows of their own.
pub(crate) const SHORE_ROW_ATTRS: [&str; 11] = [
    SHORES_ATTR,
    RAPIDS_ATTR,
    REEDS_ATTR,
    STONES_ATTR,
    SPLASHES_ATTR,
    DECALS_ATTR,
    BED_DECALS_ATTR,
    super::river_cliffs::CLIFFS_ATTR,
    super::river_point_shores::VAR_CLIFFS_ATTR,
    super::river_point_shores::VAR_AMOUNTS_ATTR,
    super::river_point_shores::VAR_SEED_ATTR,
];

const DEFAULT_DECALS: &str = "some";

/// Every amount's default (a river's or a lake's without one).
pub(crate) const DEFAULT_AMOUNT: &str = "some";

/// The layer an export puts the shores in.
pub const EXPORT_LAYER: &str = "WCE shores";

/// The reeds' exclusion polygons (`Tile::reed_exclusions`): the squares the reeds are grouped by, and how far each outline reaches
/// past its reeds (metres).
const REED_COVER_CELL_M: f64 = 12.0;
const REED_COVER_PAD_M: f64 = 1.5;

const DEFAULT_REEDS: &str = "some";
const DEFAULT_STONES: &str = "some";

/// The shores being made off the editor's thread, and the signature of what they are made from.
pub(crate) struct ShoreJob {
    job: super::jobs::Job<Vec<BiomeItem>>,
    signature: u64,
}

/// A water with a shore: its element, its water, its settings, its seed and whether its fords splash.
struct ShoreWater {
    elem: usize,
    water: Water,
    settings: ShoreSettings,
    seed: u64,
    splashes: bool,
}

impl Tile {
    /// The waters with a shore, as drawn (the viewed variation's rivers, the shown lakes).
    fn shore_waters(&self) -> Vec<ShoreWater> {
        let sc = self.tscene();
        // A water in a Don't Export layer has no shore made: a baked river's are its baked layer's elements (`river_bake.rs`).
        let on = |e: usize| {
            sc.get(e, SHORES_ATTR).as_deref().map(str::trim) == Some("1")
                && self.shown(e)
                && self.of_tile_variation(e)
                && super::siege::exported(sc, e)
        };
        let settings = |e: usize| {
            let read = |a: &str, d: &str| amount(sc.get(e, a).as_deref().unwrap_or(d)).unwrap_or_else(|| amount(d).unwrap_or(0.0));
            ShoreSettings::of(read(REEDS_ATTR, DEFAULT_REEDS), read(STONES_ATTR, DEFAULT_STONES))
                .with_decals(read(DECALS_ATTR, DEFAULT_DECALS), read(BED_DECALS_ATTR, DEFAULT_DECALS))
                .with_rapids(read(RAPIDS_ATTR, DEFAULT_RAPIDS))
        };
        let seed = |e: usize| sc.get(e, RIVER_ID_ATTR).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(e as u64);
        let mut out = vec![];
        for (e, (_, samples)) in self.river_ribbons_by_elem() {
            // Its fords (its terrain patch on): their edges are marked whether or not it has a shore.
            let patch = super::river_beds::river_patch_on(
                sc.get(e, super::river_beds::PATCH_ATTR).as_deref(),
                sc.get(e, super::river_beds::BED_ATTR).as_deref(),
            );
            let fords = if patch {
                super::river_beds::river_fords(sc.get(e, super::river_beds::FORDS_ATTR).as_deref().unwrap_or_default())
            } else {
                vec![]
            };
            let shore = on(e);
            let cliffs = if patch { self.river_cliffs_on(e, &samples) } else { vec![] };
            if (!shore && fords.is_empty() && cliffs.is_empty())
                || !self.shown(e)
                || !self.of_tile_variation(e)
                || !super::siege::exported(sc, e)
                || samples.len() < 2
            {
                continue;
            }

            let mut water = Water::River {
                line: samples.iter().map(|s| [s.x as f32, s.z as f32]).collect(),
                water: samples.iter().map(|s| s.y as f32).collect(),
                half: samples.iter().map(|s| (s.width * 0.5) as f32).collect(),
                // The fords as the river's bed has them (`river_beds.rs`): their curved ends the same.
                fords: fords
                    .iter()
                    .map(|&(at, length)| wce_tile::river_bed::Ford { at, length, depth: wce_tile::river_bed::FORD_DEPTH })
                    .collect(),
                cliffs,
                stretches: vec![],
            };
            let settings = if shore { settings(e) } else { ShoreSettings::of(0.0, 0.0) };
            // Its points' own amounts (with its shore on).
            if shore && let Water::River { stretches, .. } = &mut water {
                *stretches = self.river_shore_stretches(e, &samples, &settings);
            }

            let splashes = sc.get(e, SPLASHES_ATTR).as_deref().map(str::trim) != Some("0");
            out.push(ShoreWater { elem: e, water, settings, seed: seed(e), splashes });
        }

        for e in sc.find_all("SIMPLE_MESH_INSTANCE") {
            let Some(text) = sc.get(e, super::lakes::OUTLINE_ATTR) else { continue };
            if !on(e) {
                continue;
            }

            let outline = super::lakes::lake_outline(&text).into_iter().map(|p| [p[0] as f32, p[1] as f32]).collect::<Vec<_>>();
            if outline.len() >= 3 {
                let water = Water::Lake { outline, water: self.world_position(e).y };
                out.push(ShoreWater { elem: e, water, settings: settings(e), seed: seed(e), splashes: false });
            }
        }

        out.sort_by_key(|w| w.elem);
        out
    }

    /// The bridges over the tile's waters, seen from above (world metres): the kit's bridges (`bridge` buildings) by their models' bounds
    /// (none until their models load), the roads' by their decks as their pieces were made. Their shores are cleared under them.
    pub(super) fn bridge_covers(&self) -> Vec<Cover> {
        let mut out = vec![];
        for e in self.real_building_elems_of(&|c| c == "bridge") {
            let Some(m) = self.model_of(e).filter(|m| m.ok && m.resolved) else { continue };
            let w = self.world_matrix(e);
            let corner = |x: f32, z: f32| {
                let p = w.transform_point(wce_foundation::math::Float3::new(x, 0.0, z));
                [p.x, p.z]
            };
            let (a, b) = (m.bmin, m.bmax);
            out.push(Cover::Quad([corner(a.x, a.z), corner(b.x, a.z), corner(b.x, b.z), corner(a.x, b.z)]));
        }

        for (line, half) in &self.road_bridges {
            out.push(Cover::Band { line: line.iter().map(|p| [p[0] as f32, p[1] as f32]).collect(), half: *half as f32 });
        }

        out
    }

    fn shore_signature(&mut self, waters: &[ShoreWater], covers: &[Cover]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for w in waters {
            (w.elem, w.seed, w.splashes, format!("{:?}{:?}", w.water, w.settings)).hash(&mut h);
        }

        // The bridges over them: a bridge's model loading, or a road's bridge made, makes them again.
        format!("{covers:?}").hash(&mut h);

        // The ground (as the texture rules hash it) and the buildings and roads they keep off (any edit).
        if let Some(hf) = self.project.height.as_ref() {
            let version = hf.version();
            if self.texture_rules_heights.0 != version {
                self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
            }

            self.texture_rules_heights.1.hash(&mut h);
        }

        (self.history_version, self.structure_version).hash(&mut h);
        // What they are made of (a mod's table over the app's).
        format!("{:?}", wce_tile::shores::keys()).hash(&mut h);
        h.finish()
    }

    /// Makes the shores again when what they are made from changed and stayed so for a look (not during a drag); `wait`: now, waiting (an
    /// export).
    pub(super) fn refresh_shores(&mut self, wait: bool) {
        if self.stroke_open || (self.drag_open && !wait) {
            return;
        }

        self.take_shores(wait);
        let waters = self.shore_waters();
        if waters.is_empty() {
            if !self.shore_items.is_empty() {
                self.shore_items.clear();
                self.shore_items_shown = false;
            }

            self.shore_made = 0;
            return;
        }

        let covers = self.bridge_covers();
        let signature = self.shore_signature(&waters, &covers);
        if signature == self.shore_made {
            return;
        }

        if signature != self.shore_seen && !wait {
            self.shore_seen = signature;
            return;
        }

        if self.shore_job.as_ref().is_some_and(|j| j.signature == signature) {
            if wait {
                self.take_shores(true);
            }

            return;
        }

        self.shore_seen = signature;
        self.start_shores(waters, covers, signature);
        if wait {
            self.take_shores(true);
        }
    }

    fn start_shores(&mut self, waters: Vec<ShoreWater>, covers: Vec<Cover>, signature: u64) {
        let unit = self.unit();
        let p = &self.project;
        let (mx, mz) = (p.mpp_x(), p.mpp_z());
        let fields = p.height.as_ref().map(|hf| wce_tile::texture_rules::Fields::from_height_field(hf, (mx, mz), unit));
        let border = p.border_m();
        // The buildings but the bridges (round, they would clear the banks far up and down the river): their footprints instead, below.
        let keep_off = self.keep_off_with(false);
        // What they are made of: the tables' (`tables/<game>/shores.toml`).
        let keys = wce_tile::shores::keys().clone();
        let job = super::jobs::Job::spawn("shores", move || {
            wce_foundation::wce_prof!("tile.shores");
            let height = |x: f32, z: f32| fields.as_ref().map_or(0.0, |f| f.height(f64::from(x) + border, f64::from(z) + border) as f32);
            let mut out = vec![];
            for w in &waters {
                let mut shore = wce_tile::shores::place(&w.water, &w.settings, &keys, &height, &|x, z| keep_off.blocked(x, z), w.seed);
                // Nothing of the river's own under a bridge over it: its reeds, stones and rocks would poke through the deck.
                shore.clear_under(&covers, BRIDGE_MARGIN);
                // The bank's decals: projected on the ground, at their size (the kit's ford decals are freeform, as these).
                for p in &shore.decals {
                    let at = Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit);
                    let size = f64::from(p.scale) * unit;
                    let n = wce_tile::new_instances::NewInstance::new("ProjectedDecalInstance")
                        .position(at)
                        .attr("key", &p.key)
                        // 10 m tall, centred on the ground: it lands on the bank even if the ground under it moved a little since.
                        .attr("scale", wce_tile::new_instances::vector(Double3::new(size, 10.0 * unit, -size)))
                        .attr("surface_mode", "MSM_FREEFORM");
                    let Ok(text) = wce_tile::new_instances::element_text(&n) else { continue };
                    let text = super::draft::turned(&text, p.yaw);
                    out.push(BiomeItem { area: w.elem, text, position: [at.x, at.y, at.z] });
                }

                let splashes = if w.splashes { shore.splashes.as_slice() } else { &[] };
                for (kind, list) in [
                    ("VEGETATION_INSTANCE", shore.reeds.as_slice()),
                    ("BUILDING_INSTANCE", &shore.stones),
                    ("BUILDING_INSTANCE", &shore.bank_pieces),
                    ("BUILDING_INSTANCE", &shore.rapids),
                    ("VFXInstance", splashes),
                    ("VFXInstance", &shore.rapid_splashes),
                ] {
                    for p in list {
                        let at = Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit);
                        let Some(mut text) = item_text(kind, &p.key, at, p.scale, p.yaw, unit) else { continue };
                        // A lily and a splash float: at the water's height, not on the bed.
                        if kind == "VFXInstance" || keys.lilies.contains(&p.key) {
                            text = super::draft::with_root_attr(&text, "surface_mode", "MSM_FREEFORM");
                        }

                        out.push(BiomeItem { area: w.elem, text, position: [at.x, at.y, at.z] });
                    }
                }

                // The cliff banks' pieces: freeform at their height, leaning with their face's top at its ends.
                for c in &shore.cliffs {
                    let p = &c.placed;
                    let at = Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit);
                    let Some(mut text) = item_text("BUILDING_INSTANCE", &p.key, at, p.scale, p.yaw, unit) else { continue };
                    if c.up[1] < 1.0 - 1e-6 {
                        text = super::draft::tilted(&text, Double3::new(f64::from(c.up[0]), f64::from(c.up[1]), f64::from(c.up[2])));
                    }

                    out.push(BiomeItem { area: w.elem, text, position: [at.x, at.y, at.z] });
                }
            }

            out
        });
        self.shore_job = Some(ShoreJob { job, signature });
    }

    fn take_shores(&mut self, wait: bool) {
        let Some(j) = &self.shore_job else { return };
        let made = match if wait { j.job.wait() } else { j.job.poll() } {
            super::jobs::Poll::Running => return,
            super::jobs::Poll::Done(m) => Some(m),
            super::jobs::Poll::Failed => None,
        };

        let j = self.shore_job.take().expect("a job");
        let Some(items) = made else {
            wce_foundation::log::warn("shores: they could not be made");
            return;
        };

        self.shore_items = items;
        self.shore_made = j.signature;
        self.shore_items_shown = false;
    }

    /// The shores in the viewport, again when they were made again (locked).
    pub(super) fn show_shore_items(&mut self, scene: &mut RenderScene) {
        if self.shore_items_shown {
            return;
        }

        self.shore_items_shown = true;
        for id in std::mem::take(&mut self.shore_entities) {
            scene.remove(id);
        }

        self.shore_entities = self.show_items(scene, &self.shore_items);
    }

    /// The layer of the shores an export adds, as TEd and the game read it.
    pub(super) fn with_shore_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.shore_items.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), EXPORT_LAYER)?;
        let mut texts = self.shore_items.iter().map(|i| i.text.clone()).collect::<Vec<_>>();
        texts.extend(self.reed_exclusions(None));
        scene.insert_elements_text(layer, &texts)?;

        Ok(scene.save())
    }

    /// Vegetation exclusion polygons round the reeds (`wce_tile::vegetation::cover_polygons`): BOB keeps a placed plant only inside
    /// one, so without them no reed reached the game (user 2026-11-28). Small shapes per patch of reeds, so little of the game's own
    /// vegetation beside the river is kept off.
    pub(super) fn reed_exclusions(&self, only: Option<usize>) -> Vec<Vec<u8>> {
        use wce_tile::new_instances::{NewInstance, NewPoint, element_text};
        let unit = self.unit();
        let reeds = self
            .shore_items
            .iter()
            .filter(|i| i.text.starts_with(b"<VEGETATION_INSTANCE") && only.is_none_or(|o| i.area == o))
            .map(|i| [i.position[0], i.position[2]])
            .collect::<Vec<_>>();
        wce_tile::vegetation::cover_polygons(&reeds, REED_COVER_CELL_M * unit, REED_COVER_PAD_M * unit)
            .into_iter()
            .filter_map(|poly| {
                let c = poly.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / poly.len() as f64, a[1] + p[1] / poly.len() as f64]);
                let local = poly.iter().map(|p| Double3::new(p[0] - c[0], 0.0, p[1] - c[1])).collect::<Vec<_>>();
                let mut g = NewInstance::new("VegetationExclusionPolygonInstance")
                    .position(Double3::new(c[0], 0.0, c[1]))
                    .attr("name", "River reeds")
                    .attr("polygon_closed", "1")
                    .attr("surface_mode", "MSM_LANDSCAPE")
                    .attr("area", wce_tile::ted_scene::format_number(wce_tile::new_instances::polygon_area(&local)));
                g.points = local.into_iter().map(NewPoint::at).collect();
                element_text(&g).ok()
            })
            .collect()
    }

    /// How many reeds and stones the shores hold now.
    #[must_use]
    pub fn shore_counts(&self) -> (usize, usize) {
        let reeds = self.shore_items.iter().filter(|i| i.text.starts_with(b"<VEGETATION_INSTANCE")).count();
        let stones = self.shore_items.iter().filter(|i| i.text.starts_with(b"<BUILDING_INSTANCE")).count();
        (reeds, stones)
    }

    /// The roads' bridges as the shores keep off them: each deck's centre line and half its width (world metres).
    #[must_use]
    pub fn road_bridge_lines(&self) -> Vec<(Vec<[f64; 2]>, f64)> {
        self.road_bridges.clone()
    }

    /// The shore items within a box (x0, z0, x1, z1, world metres): each one's element type, key and place (a script's look).
    #[must_use]
    pub fn shore_items_in(&self, b: [f32; 4]) -> Vec<String> {
        let unit = self.unit();
        self.shore_items
            .iter()
            .filter_map(|i| {
                let (x, z) = ((i.position[0] / unit) as f32, (i.position[2] / unit) as f32);
                if x < b[0] || x > b[2] || z < b[1] || z > b[3] {
                    return None;
                }

                let text = String::from_utf8_lossy(&i.text);
                let kind = text.trim_start_matches('<').split(' ').next().unwrap_or("").to_owned();
                let key = ["key=\"", "vfx=\""]
                    .iter()
                    .find_map(|k| text.split(k).nth(1)?.split('"').next().map(str::to_owned))
                    .unwrap_or_default();
                let (y, ground) = ((i.position[1] / unit) as f32, self.terrain_height(x, z));
                let extra = if std::env::var_os("WCE_SHORE_TEXT").is_some() { format!("\n    {text}") } else { String::new() };
                Some(format!("{kind} {key} at ({x:.1}, {z:.1}) y {y:.2}, ground {ground:.2}{extra}"))
            })
            .collect()
    }

    /// How many decals the banks have now.
    #[must_use]
    pub fn shore_decals(&self) -> usize {
        self.shore_items.iter().filter(|i| i.text.starts_with(b"<ProjectedDecalInstance")).count()
    }

    /// How many splashes the fords have now.
    #[must_use]
    pub fn shore_splashes(&self) -> usize {
        self.shore_items.iter().filter(|i| i.text.starts_with(b"<VFXInstance")).count()
    }
}

impl TileDocument {
    /// The shores as an export reads them: made now.
    pub fn finish_shores(&mut self) {
        self.tile_mut().refresh_shores(true);
    }

    /// A river's or a lake's shore in the Inspector: on or off, how thick its reeds and stones are; a river's cliff banks and shore
    /// variation.
    pub(crate) fn shore_rows(&mut self, e: usize, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        let sc = self.tile().tscene();
        let river = sc.type_of(e) == "SplineRiverInstance";
        let on = sc.get(e, SHORES_ATTR).as_deref().map(str::trim) == Some("1");
        let has_fords = !super::river_beds::river_fords(sc.get(e, super::river_beds::FORDS_ATTR).as_deref().unwrap_or_default()).is_empty();
        let splashing = sc.get(e, SPLASHES_ATTR).as_deref().map(str::trim) != Some("0");
        ins.section("Shores");
        if has_fords {
            let mut s = splashing;
            ins.hint("Splashes over the fords' edges (the game's water splash). Their edges are lined with stones either way.");
            if ins.edit_bool(SPLASHES_ATTR, "ford splashes", &mut s) && s != splashing {
                self.set_attribute(e, SPLASHES_ATTR, if s { "1" } else { "0" });
            }
        }

        let mut shores = on;
        ins.hint(
            "Reed beds, water lilies, stones and decals along the water, made again as the water or the ground changes and written in \
             their own layer at export. A river's points can have their own amounts (the point's rows).",
        );
        if ins.edit_bool(SHORES_ATTR, "shores", &mut shores) && shores != on {
            self.set_attribute(e, SHORES_ATTR, if shores { "1" } else { "0" });
        }

        if shores {
            let names = AMOUNTS.iter().map(|a| a.0.to_owned()).collect::<Vec<_>>();
            for (attr, label, default, hint) in [
                (REEDS_ATTR, "reeds", DEFAULT_REEDS, "How much of the shore reed beds take, with a few water lilies further out."),
                (
                    STONES_ATTR,
                    "stones",
                    DEFAULT_STONES,
                    "How much of the waterline small stones line, with a boulder on the bank now and then and, rarely, a flat rock or a \
                     mound along a river's bank.",
                ),
                (DECALS_ATTR, "shore decals", DEFAULT_DECALS, "How many muddy rock decals lie over a river's waterline."),
                (BED_DECALS_ATTR, "bed decals", DEFAULT_DECALS, "How many mud decals lie on a river's bed by its edges."),
                (
                    RAPIDS_ATTR,
                    "rapids",
                    DEFAULT_RAPIDS,
                    "How many rapids a river has: big rocks out on its water, alone or in clumps, splashing (none by its fords).",
                ),
            ] {
                if attr == RAPIDS_ATTR && !river {
                    continue;
                }

                let sc = self.tile().tscene();
                let current = sc.get(e, attr).unwrap_or_else(|| default.to_owned());
                let mut idx = AMOUNTS.iter().position(|a| a.0 == current.trim()).unwrap_or(0) as i32;
                let before = idx;
                ins.hint(hint);
                if ins.edit_choice(attr, label, &mut idx, &names) && idx != before {
                    self.set_attribute(e, attr, AMOUNTS[idx.clamp(0, AMOUNTS.len() as i32 - 1) as usize].0);
                }
            }

            if river {
                self.river_cliff_and_variation_rows(e, ins);
            }
        }
    }

    /// A river's cliff banks and its shore variation in the Inspector.
    fn river_cliff_and_variation_rows(&mut self, e: usize, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        use super::river_cliffs::{CLIFFS_ATTR, RIVER_CLIFFS};
        use super::river_point_shores::{ShoreVariation, VAR_AMOUNTS_ATTR, VAR_CLIFFS_ATTR, VAR_SEED_ATTR};
        let labels = ["none", "outer bends", "left bank", "right bank", "both banks"].map(str::to_owned);
        let mode = self.tile().river_cliffs_mode(e);
        let mut idx = RIVER_CLIFFS.iter().position(|m| *m == mode).unwrap_or(0) as i32;
        let before = idx;
        ins.hint(
            "Rock cliff pieces along a bank, the ground behind raised to their top, nothing else of the shore under them: none, the \
             outer bank where the river bends sharply, or a side (as the river runs) along it all. A point's own cliff setting (its \
             rows) goes over this, from halfway to the point before to halfway to the one after.",
        );
        if ins.edit_choice(CLIFFS_ATTR, "cliffs", &mut idx, &labels) && idx != before {
            self.set_river_cliffs_mode(e, RIVER_CLIFFS[idx.clamp(0, RIVER_CLIFFS.len() as i32 - 1) as usize]);
        }

        // The shore variation: its settings kept on the river, drawn again as one is changed (when its drag ends: one undo entry).
        let saved = self.tile().river_shore_variation(e);
        let mut v = self.shore_var_draft.filter(|(r, _)| *r == e).map_or(saved, |(_, v)| v);
        let mut changed = false;
        let mut ended = false;
        let mut cliffs = v.cliffs.round() as i64;
        ins.hint(
            "How likely each point starts a run of two or three points unlike the river's cliffs above: where it has none, a cliff on \
             the outer bank of the bend; where it has one, a break in it. 0: the river's cliffs as they are. The points are drawn \
             again as it changes; those set by hand keep their own.",
        );
        if ins.edit_int_slider(VAR_CLIFFS_ATTR, "cliff runs (%)", &mut cliffs, 0, 100) {
            v.cliffs = cliffs as f64;
            changed = true;
        }

        ended |= ins.gesture_ended();
        let mut amounts = v.amounts.round() as i64;
        ins.hint(
            "How likely each point's reeds, stones and decals are a step more or less than the river's, each, in percent. The points \
             are drawn again as it changes; those set by hand keep their own.",
        );
        if ins.edit_int_slider(VAR_AMOUNTS_ATTR, "amount changes (%)", &mut amounts, 0, 100) {
            v.amounts = amounts as f64;
            changed = true;
        }

        ended |= ins.gesture_ended();
        let mut seed = v.seed as i64;
        ins.hint("The draw: another seed, other points (drawn again as it changes).");
        if ins.edit_int(VAR_SEED_ATTR, "variation seed", &mut seed, 0.2) {
            v.seed = seed.max(0) as u64;
            changed = true;
        }

        ended |= ins.gesture_ended();
        // Lucide's refresh (`wce_editor::icons::REFRESH_CW`).
        let mut apply = None;
        if ins.row_buttons(&[("shore_var_reroll", "\u{e145}", "Another seed, and randomize with it", true)], "") == Some(0) {
            apply = Some(ShoreVariation { seed: (v.seed.wrapping_mul(2_654_435_761).wrapping_add(1)) % 1_000_000, ..v });
        }

        if changed {
            self.shore_var_draft = Some((e, v));
        }

        if ended && self.shore_var_draft.is_some_and(|(r, _)| r == e) {
            apply = Some(v);
        }

        match ins.wide_buttons(&[
            (
                "shore_var_apply",
                "Randomize shore",
                "Gives the points cliffs and amounts of their own by the settings above, in place of those it gave before; the points set \
                 by hand keep theirs. One undo entry.",
                true,
            ),
            ("shore_var_reset", "Reset variation", "Takes the values the variation gave the points away (those set by hand stay).", true),
        ]) {
            Some(0) => apply = Some(v),
            Some(_) => {
                self.shore_var_draft = None;
                let _ = self.reset_river_shore(e);
            }

            None => {}
        }

        if let Some(v) = apply {
            self.shore_var_draft = None;
            let _ = self.randomize_river_shore(e, v);
        }
    }
}
