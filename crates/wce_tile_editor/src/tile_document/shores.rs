//! Shores in the tile (docs/plans/rivers.md, step 10; `wce_tile::shores`): reeds and stones along the water of a river or a lake with
//! `wce_shores="1"` (the rivers and lakes the editor makes; the kit's are left as they are until turned on), made again off the editor's
//! thread when the water or the ground by it changes and stays so for a look, drawn locked (they are the water's, not tile.ted's) and
//! added at export in a layer of their own, as the biome areas' contents are.

use wce_foundation::math::Double3;
use wce_scene::render_scene::RenderScene;
use wce_tile::shores::{AMOUNTS, ShoreSettings, Water, amount};
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

/// The attributes the Inspector shows as rows of their own.
pub(crate) const SHORE_ROW_ATTRS: [&str; 4] = [SHORES_ATTR, REEDS_ATTR, STONES_ATTR, SPLASHES_ATTR];

/// The layer an export puts the shores in.
pub const EXPORT_LAYER: &str = "WCE shores";

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
        let on = |e: usize| sc.get(e, SHORES_ATTR).as_deref().map(str::trim) == Some("1") && self.shown(e) && self.of_tile_variation(e);
        let settings = |e: usize| {
            let read = |a: &str, d: &str| amount(sc.get(e, a).as_deref().unwrap_or(d)).unwrap_or_else(|| amount(d).unwrap_or(0.0));
            ShoreSettings::of(read(REEDS_ATTR, DEFAULT_REEDS), read(STONES_ATTR, DEFAULT_STONES))
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
            if (!shore && fords.is_empty()) || !self.shown(e) || !self.of_tile_variation(e) || samples.len() < 2 {
                continue;
            }

            let water = Water::River {
                line: samples.iter().map(|s| [s.x as f32, s.z as f32]).collect(),
                water: samples.iter().map(|s| s.y as f32).collect(),
                half: samples.iter().map(|s| (s.width * 0.5) as f32).collect(),
                // The fords as the river's bed has them (`river_beds.rs`): their curved ends the same.
                fords: fords
                    .iter()
                    .map(|&(at, length)| wce_tile::river_bed::Ford { at, length, depth: wce_tile::river_bed::FORD_DEPTH })
                    .collect(),
            };
            let settings = if shore { settings(e) } else { ShoreSettings::of(0.0, 0.0) };
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

    fn shore_signature(&mut self, waters: &[ShoreWater]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for w in waters {
            (w.elem, w.seed, w.splashes, format!("{:?}{:?}", w.water, w.settings)).hash(&mut h);
        }

        // The ground (as the texture rules hash it) and the buildings and roads they keep off (any edit).
        if let Some(hf) = self.project.height.as_ref() {
            let version = hf.version();
            if self.texture_rules_heights.0 != version {
                self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
            }

            self.texture_rules_heights.1.hash(&mut h);
        }

        (self.history_version, self.structure_version).hash(&mut h);
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

        let signature = self.shore_signature(&waters);
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
        self.start_shores(waters, signature);
        if wait {
            self.take_shores(true);
        }
    }

    fn start_shores(&mut self, waters: Vec<ShoreWater>, signature: u64) {
        let unit = self.unit();
        let p = &self.project;
        let (mx, mz) = (p.mpp_x(), p.mpp_z());
        let fields = p.height.as_ref().map(|hf| wce_tile::texture_rules::Fields::from_height_field(hf, (mx, mz), unit));
        let border = p.border_m();
        let keep_off = self.keep_off();
        let job = super::jobs::Job::spawn("shores", move || {
            wce_foundation::wce_prof!("tile.shores");
            let height = |x: f32, z: f32| fields.as_ref().map_or(0.0, |f| f.height(f64::from(x) + border, f64::from(z) + border) as f32);
            let mut out = vec![];
            for w in &waters {
                let shore = wce_tile::shores::place(&w.water, &w.settings, &height, &|x, z| keep_off.blocked(x, z), w.seed);
                let splashes = if w.splashes { shore.splashes.as_slice() } else { &[] };
                for (kind, list) in
                    [("VEGETATION_INSTANCE", shore.reeds.as_slice()), ("BUILDING_INSTANCE", &shore.stones), ("VFXInstance", splashes)]
                {
                    for p in list {
                        let at = Double3::new(f64::from(p.x) * unit, f64::from(p.y) * unit, f64::from(p.z) * unit);
                        let Some(mut text) = item_text(kind, &p.key, at, p.scale, p.yaw, unit) else { continue };
                        // A lily and a splash float: at the water's height, not on the bed.
                        if kind == "VFXInstance" || wce_tile::shores::LILIES.contains(&p.key.as_str()) {
                            text = super::draft::with_root_attr(&text, "surface_mode", "MSM_FREEFORM");
                        }

                        out.push(BiomeItem { area: w.elem, text, position: [at.x, at.y, at.z] });
                    }
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
        for item in &self.shore_items {
            scene.insert_element_text(layer, &item.text)?;
        }

        Ok(scene.save())
    }

    /// How many reeds and stones the shores hold now.
    #[must_use]
    pub fn shore_counts(&self) -> (usize, usize) {
        let reeds = self.shore_items.iter().filter(|i| i.text.starts_with(b"<VEGETATION_INSTANCE")).count();
        let stones = self.shore_items.iter().filter(|i| i.text.starts_with(b"<BUILDING_INSTANCE")).count();
        (reeds, stones)
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

    /// A river's or a lake's shore in the Inspector: on or off, and how thick its reeds and stones are.
    pub(crate) fn shore_rows(&mut self, e: usize, ins: &mut dyn wce_editor_core::inspector::Inspector) {
        let sc = self.tile().tscene();
        let on = sc.get(e, SHORES_ATTR).as_deref().map(str::trim) == Some("1");
        let has_fords = !super::river_beds::river_fords(sc.get(e, super::river_beds::FORDS_ATTR).as_deref().unwrap_or_default()).is_empty();
        let splashing = sc.get(e, SPLASHES_ATTR).as_deref().map(str::trim) != Some("0");
        ins.section("Shores");
        if has_fords {
            let mut s = splashing;
            ins.hint(
                "Splashes over the fords' edges (the game's battle water splash, a loop on the water). The fords' edges are lined \
                 with stones whether or not the river has a shore.",
            );
            if ins.edit_bool(SPLASHES_ATTR, "ford splashes", &mut s) && s != splashing {
                self.set_attribute(e, SPLASHES_ATTR, if s { "1" } else { "0" });
            }
        }

        let mut shores = on;
        ins.hint(
            "Reed beds in the shallows and on the wet bank, water lilies, small stones along the waterline; made again as the water or \
             the ground changes and written in their own layer at export. On for the rivers and lakes the editor makes.",
        );
        if ins.edit_bool(SHORES_ATTR, "shores", &mut shores) && shores != on {
            self.set_attribute(e, SHORES_ATTR, if shores { "1" } else { "0" });
        }

        if !shores {
            return;
        }

        let names = AMOUNTS.iter().map(|a| a.0.to_owned()).collect::<Vec<_>>();
        for (attr, label, default, hint) in [
            (
                REEDS_ATTR,
                "reeds",
                DEFAULT_REEDS,
                "How much of the shore its reed beds take (bulrush, calamus, marsh plants; in the shallows and on the wet bank), with a \
                 few water lilies further out.",
            ),
            (
                STONES_ATTR,
                "stones",
                DEFAULT_STONES,
                "How much of the waterline small stones line, with a small boulder on the bank now and then.",
            ),
        ] {
            let sc = self.tile().tscene();
            let current = sc.get(e, attr).unwrap_or_else(|| default.to_owned());
            let mut idx = AMOUNTS.iter().position(|a| a.0 == current.trim()).unwrap_or(0) as i32;
            let before = idx;
            ins.hint(hint);
            if ins.edit_choice(attr, label, &mut idx, &names) && idx != before {
                self.set_attribute(e, attr, AMOUNTS[idx.clamp(0, AMOUNTS.len() as i32 - 1) as usize].0);
            }
        }
    }
}
