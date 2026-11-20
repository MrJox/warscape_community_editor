//! Districts in a tile (docs/plans/auto_detail_2.md, I): a district is a vegetation exclusion polygon (no trees in a town) with
//! `wce_district` (its settings, `wce_tile::districts::DistrictSettings`) and `wce_district_seed`. Filling it lays plain buildings (the
//! user moves them after) in a group of its own beside it, `<its name> buildings`, as one undo entry; filling it again replaces that
//! group.

use wce_foundation::math::Double3;
use wce_tile::biomes::AREA_TYPE;
use wce_tile::districts::{DISTRICT_ATTR, DistrictSettings, Ground, Lot, RoadLine, SEED_ATTR};
use wce_tile::world_transform::set_world_position;

use super::biomes::item_text;
use super::{Tile, TileDocument};

/// A district: its element, name, settings, seed and polygon (world metres, x and z).
#[derive(Clone, Debug, PartialEq)]
pub struct DistrictArea {
    pub elem: usize,
    pub name: String,
    pub settings: DistrictSettings,
    pub seed: u32,
    pub points: Vec<(f32, f32)>,
}

/// Metres a side of a building whose size is not known (no logic file, no model to measure): a house's.
pub const GUESS: f64 = 8.0;

/// The most different keys a district takes from the tile's own houses.
const DEFAULT_KEYS: usize = 8;

impl Tile {
    /// The tile's districts.
    #[must_use]
    pub fn district_areas(&self) -> Vec<DistrictArea> {
        let sc = self.tscene();
        sc.find_all(AREA_TYPE)
            .into_iter()
            .filter_map(|e| {
                let text = sc.get(e, DISTRICT_ATTR)?;
                let seed = sc.get(e, SEED_ATTR).and_then(|s| s.trim().parse().ok()).unwrap_or(e as u32);
                let points = self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect();
                Some(DistrictArea { elem: e, name: sc.name(e), settings: DistrictSettings::from_text(&text), seed, points })
            })
            .collect()
    }

    /// The buildings a district without keys is filled with: the tile's own houses (its `generic` buildings whose key says house, home,
    /// hut or dwelling), the most used first; else the kit's of that kind.
    #[must_use]
    pub fn district_default_keys(&self) -> Vec<String> {
        if let Some((v, keys)) = self.district_keys.borrow().as_ref()
            && *v == self.structure_version
        {
            return keys.clone();
        }

        let keys = self.find_district_default_keys();
        *self.district_keys.borrow_mut() = Some((self.structure_version, keys.clone()));
        keys
    }

    fn find_district_default_keys(&self) -> Vec<String> {
        let sc = self.tscene();
        let homes = |k: &str| {
            let k = k.to_lowercase();
            ["house", "home", "hut", "dwelling"].iter().any(|w| k.contains(w))
                && !["wall", "door", "gate", "fence", "tower", "ruin", "dead", "destroyed"].iter().any(|w| k.contains(w))
        };
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for &e in self.instances() {
            if sc.type_of(e) != "BUILDING_INSTANCE" {
                continue;
            }

            if let Some(k) = sc.get(e, "key").filter(|k| homes(k) && self.building_category(k).as_deref() == Some("generic")) {
                *counts.entry(k).or_default() += 1;
            }
        }

        let mut keys = counts.into_iter().collect::<Vec<_>>();
        keys.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut out = keys.into_iter().take(DEFAULT_KEYS).map(|(k, _)| k).collect::<Vec<_>>();
        // None of its own: one culture's of the kits' (the key's first word: the one with the most houses), in order.
        if out.is_empty() {
            // Only those the game's files have models for (the kits' tables name buildings it has none of).
            let db = self.building_db();
            let mut all = db
                .keys()
                .filter(|k| homes(k) && db.category(k) == Some("generic"))
                .filter(|k| self.files.is_none() || !self.preview_building(k, 0).is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            all.sort();
            let mut by_culture = std::collections::BTreeMap::<String, Vec<String>>::new();
            for k in all {
                let culture = k.split('_').next().unwrap_or_default().to_owned();
                by_culture.entry(culture).or_default().push(k);
            }

            out = by_culture.into_values().max_by_key(Vec::len).unwrap_or_default().into_iter().take(DEFAULT_KEYS).collect();
        }

        out
    }

    /// The group a district's buildings are in: in its layer, by name.
    fn district_group(&self, area: &DistrictArea) -> Option<usize> {
        let sc = self.tscene();
        let layer = self.layer_of(area.elem)?;
        let name = group_name(&area.name);
        sc.children(layer).into_iter().find(|&c| sc.type_of(c) == "UserInstanceGroup" && sc.name(c) == name)
    }

    /// Where a district's buildings go: its lots, from its settings (its keys, else the tile's houses) and the ground.
    fn district_lots(&self, area: &DistrictArea) -> Vec<Lot> {
        let mut s = area.settings.clone();
        if s.keys.is_empty() {
            s.keys = self.district_default_keys();
        }

        let roads = self.road_middle_lines().into_iter().map(|(line, half_width)| RoadLine { line, half_width }).collect::<Vec<_>>();
        // The buildings there but its own (filling again replaces them).
        let own = self.district_group(area);
        let sc = self.tscene();
        let blockers = self
            .texture_rules_buildings_elems()
            .into_iter()
            .filter(|&(e, _)| own.is_none_or(|g| !sc.is_descendant(e, g)))
            .map(|(_, b)| b)
            .collect::<Vec<_>>();
        let height = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
        let footprint =
            |k: &str| self.building_extent_found(k).unwrap_or_else(|| wce_tile::building_spline::Footprint::centred(GUESS, GUESS));
        let g = Ground { roads: &roads, blockers: &blockers, height: &height, footprint: &footprint };
        let poly = area.points.iter().map(|&(x, z)| [f64::from(x), f64::from(z)]).collect::<Vec<_>>();
        wce_tile::districts::fill(&poly, &g, &s, area.seed)
    }
}

/// The name of a district's group of buildings.
fn group_name(district: &str) -> String {
    format!("{} buildings", if district.is_empty() { "District" } else { district })
}

impl TileDocument {
    /// Fills a district with buildings: its group (`<its name> buildings`, in its layer) made or emptied and filled, as one undo entry.
    /// How many buildings.
    ///
    /// # Errors
    ///
    /// No such district, no buildings to fill it with, or the scene's edit failed.
    pub fn fill_district(&mut self, elem: usize) -> Result<usize, String> {
        let area = self.tile().district_areas().into_iter().find(|a| a.elem == elem).ok_or("not a district")?;
        let lots = self.tile().district_lots(&area);
        let unit = self.tile().unit();
        let layer = self.tile().layer_of(elem).ok_or("the district is in no layer")?;
        let texts = lots
            .iter()
            .filter_map(|l| {
                let y = f64::from(self.tile().terrain_height(l.x as f32, l.z as f32));
                let at = Double3::new(l.x * unit, y * unit, l.z * unit);
                item_text("BUILDING_INSTANCE", &l.key, at, 1.0, l.yaw.to_degrees() as f32, unit).map(|t| (t, at))
            })
            .collect::<Vec<_>>();
        if texts.is_empty() {
            return Err("no building fits the district (its keys, its slope, the roads and buildings there)".to_owned());
        }

        let existing = self.tile().district_group(&area);
        let mark = self.begin()?;
        let outcome = (|| -> Result<(), String> {
            let group = match existing {
                Some(g) => {
                    self.tscene_mut().set_children_text(g, b"")?;
                    g
                }
                None => {
                    let mid = area
                        .points
                        .iter()
                        .fold((0.0, 0.0), |a, p| (a.0 + p.0 / area.points.len() as f32, a.1 + p.1 / area.points.len() as f32));
                    let local = Double3::new(f64::from(mid.0) * unit, 0.0, f64::from(mid.1) * unit);
                    self.tscene_mut().add_group(Some(layer), &group_name(&area.name), local)?
                }
            };

            for (text, at) in &texts {
                let sc = self.tscene_mut();
                let e = sc.insert_element_text(group, text)?;
                set_world_position(sc, e, *at);
            }

            Ok(())
        })();

        match outcome {
            Ok(()) => {
                self.push_splice("Fill district".to_owned(), mark, vec![], vec![]);
                Ok(texts.len())
            }
            Err(err) => self.rolled_back(mark, err),
        }
    }

    /// A building's footprint about its pivot (metres), as the district filler takes it: none when it is not known (the filler takes
    /// [`GUESS`] metres a side).
    #[must_use]
    pub fn building_footprint(&self, key: &str) -> Option<wce_tile::building_spline::Footprint> {
        self.tile().building_extent_found(key)
    }

    /// How many buildings a district's group holds.
    #[must_use]
    pub fn district_buildings(&self, elem: usize) -> usize {
        let t = self.tile();
        let Some(area) = t.district_areas().into_iter().find(|a| a.elem == elem) else { return 0 };
        t.district_group(&area).map_or(0, |g| t.tscene().children(g).len())
    }
}
