//! Fields and farmland in a tile (docs/plans/auto_detail_2.md, G): an area is a vegetation exclusion polygon with `wce_fields` (its
//! settings, `wce_tile::fields::FieldSettings`) and `wce_fields_seed`. What it lays is the editor's, as a biome area's contents: made again
//! when the area, its settings or the ground change (once an edit settled, on the editor's thread: it is a few hundred items), drawn
//! locked and added at export in a layer of its own ([`EXPORT_LAYER`]): the crops' decals, the orchards' trees, the props, the fence round
//! it (laid as a closed building spline's). The ground under it goes into the texture rules' layer (`all_texture_rules`).

use wce_foundation::math::Double3;
use wce_scene::render_scene::{EntityId, RenderScene};
use wce_tile::biomes::AREA_TYPE;
use wce_tile::building_spline::{Settings as SplineSettings, instance_text, lay as lay_spline};
use wce_tile::fields::{FIELDS_ATTR, FieldSettings, SEED_ATTR};
use wce_tile::ted_scene::TedScene;

use super::biomes::{BiomeItem, item_text};
use super::decal_rules::DecalItem;
use super::{Tile, TileDocument};

/// The layer an export puts the farmland in.
pub const EXPORT_LAYER: &str = "WCE fields";

/// A field area: its element, settings, seed and polygon (world metres, x and z).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldArea {
    pub elem: usize,
    pub settings: FieldSettings,
    pub seed: u32,
    pub points: Vec<(f32, f32)>,
}

/// What the field areas lay: their trees, props and fences, and their crops' decals; the signature they were made from and the one last
/// looked at, whether they are shown and their entities.
#[derive(Default)]
pub(crate) struct FieldsMade {
    items: Vec<BiomeItem>,
    decals: Vec<DecalItem>,
    made: u64,
    seen: u64,
    shown: bool,
    entities: Vec<EntityId>,
}

impl Tile {
    /// The tile's field areas.
    #[must_use]
    pub fn field_areas(&self) -> Vec<FieldArea> {
        let sc = self.tscene();
        sc.find_all(AREA_TYPE)
            .into_iter()
            .filter_map(|e| {
                let text = sc.get(e, FIELDS_ATTR)?;
                let seed = sc.get(e, SEED_ATTR).and_then(|s| s.trim().parse().ok()).unwrap_or(e as u32);
                let points = self.polygon_world_points(e).into_iter().map(|p| (p.x, p.z)).collect();
                Some(FieldArea { elem: e, settings: FieldSettings::from_text(&text), seed, points })
            })
            .collect()
    }

    /// What the farmland is made from, hashed: the areas, the heights, the scene (the fences' models).
    fn fields_signature(&mut self, areas: &[FieldArea]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        for a in areas {
            eat(a.settings.to_text().as_bytes());
            eat(&a.seed.to_le_bytes());
            for &(x, z) in &a.points {
                eat(&x.to_le_bytes());
                eat(&z.to_le_bytes());
            }
        }

        if let Some(hf) = self.project.height.as_ref() {
            let version = hf.version();
            if self.texture_rules_heights.0 != version {
                self.texture_rules_heights = (version, hf.region_hash(0.0, 0.0, f64::from(hf.width()), f64::from(hf.height())));
            }

            eat(&self.texture_rules_heights.1.to_le_bytes());
        }

        eat(&(self.models.len() as u64).to_le_bytes());
        h
    }

    /// Makes the farmland again when what it is made from changed and stayed so for a look; `wait`: now (an export).
    pub(super) fn refresh_fields(&mut self, wait: bool) {
        if self.stroke_open {
            return;
        }

        let areas = self.field_areas();
        if areas.is_empty() {
            if !self.fields.items.is_empty() || !self.fields.decals.is_empty() {
                self.fields.items.clear();
                self.fields.decals.clear();
                self.fields.shown = false;
            }

            self.fields.made = 0;
            return;
        }

        let signature = self.fields_signature(&areas);
        if signature == self.fields.made {
            return;
        }

        if signature != self.fields.seen && !wait {
            self.fields.seen = signature;
            return;
        }

        self.fields.seen = signature;
        self.make_fields(&areas);
        self.fields.made = signature;
        self.fields.shown = false;
    }

    fn make_fields(&mut self, areas: &[FieldArea]) {
        let unit = self.unit();
        let ground = |x: f64, z: f64| f64::from(self.terrain_height(x as f32, z as f32));
        let mut items = vec![];
        let mut decals = vec![];
        for a in areas {
            let farm = wce_tile::fields::lay(&a.points, &a.settings, a.seed);
            for d in &farm.decals {
                let decal = wce_tile::decal_rules::Decal {
                    rule: 0,
                    key: wce_tile::decal_rules::key_of(&d.key),
                    x: d.x,
                    z: d.z,
                    yaw: d.yaw,
                    across: d.across,
                    length: d.length,
                };
                decals.extend(self.decal_item(&decal));
            }

            for (kind, list) in [("VEGETATION_INSTANCE", &farm.trees), ("BUILDING_INSTANCE", &farm.props)] {
                for t in list {
                    let y = f64::from(self.terrain_height(t.x, t.z));
                    let at = Double3::new(f64::from(t.x) * unit, y * unit, f64::from(t.z) * unit);
                    if let Some(text) = item_text(kind, &t.key, at, t.scale, t.yaw, unit) {
                        items.push(BiomeItem::new(a.elem, text, [at.x, at.y, at.z]));
                    }
                }
            }

            // The fence: a closed building spline round the area.
            if !a.settings.fence.is_empty() && a.points.len() > 2 {
                let mut line = a.points.iter().map(|&(x, z)| [f64::from(x), f64::from(z)]).collect::<Vec<_>>();
                line.push(line[0]);
                let settings = SplineSettings {
                    keys: a.settings.fence.clone(),
                    random: true,
                    seed: a.seed,
                    align: true,
                    closed: true,
                    ..SplineSettings::default()
                };
                let levels = |key: &str| self.building_levels(key);
                for laid in lay_spline(&line, &settings, &|k| self.building_extent(k), &levels, &ground) {
                    if let Ok(text) = instance_text(&laid, unit) {
                        let [x, y, z] = laid.position;
                        items.push(BiomeItem::new(a.elem, text, [x * unit, y * unit, z * unit]));
                    }
                }
            }
        }

        self.fields.items = items;
        self.fields.decals = decals;
    }

    /// The farmland in the viewport, again when it was made again.
    pub(super) fn show_field_items(&mut self, scene: &mut RenderScene) {
        if self.fields.shown {
            return;
        }

        self.fields.shown = true;
        for id in std::mem::take(&mut self.fields.entities) {
            scene.remove(id);
        }

        let mut ids = self.show_items(scene, &self.fields.items);
        ids.extend(self.show_decals(scene, &self.fields.decals));
        self.fields.entities = ids;
    }

    /// The scene was cleared: the entities went with it.
    pub(super) fn fields_cleared(&mut self) {
        self.fields.entities.clear();
        self.fields.shown = false;
    }

    /// The layer of the farmland an export adds, as TEd and the game read it.
    pub(super) fn with_field_items(&self, text: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.fields.items.is_empty() && self.fields.decals.is_empty() {
            return Ok(text);
        }

        let mut scene = TedScene::parse_any(text)?;
        let layer = scene.add_layer(scene.scene_graph(), EXPORT_LAYER)?;
        for t in self.fields.items.iter().map(BiomeItem::text).chain(self.fields.decals.iter().map(|d| &d.text[..])) {
            scene.insert_element_text(layer, t)?;
        }

        Ok(scene.save())
    }

    /// The field areas' texture rules (the ground under each, fading in over its edge): rules and their areas' masks, after `first` masks.
    pub(super) fn field_texture_rules(&self, first: usize) -> (Vec<wce_tile::texture_rules::Rule>, Vec<wce_tile::texture_rules::AreaMask>) {
        let border = self.project.border_m() as f32;
        let (mut rules, mut masks) = (vec![], vec![]);
        for a in self.field_areas() {
            let s = &a.settings;
            if s.ground.trim().is_empty() || s.ground_strength <= 0.0 {
                continue;
            }

            let i = first + masks.len();
            masks.push(wce_tile::texture_rules::AreaMask {
                points: a.points.iter().map(|&(x, z)| (x + border, z + border)).collect(),
                edge: 2.0,
            });
            rules.push(wce_tile::texture_rules::Rule {
                name: "Fields".to_owned(),
                group: s.ground.clone(),
                variation: s.ground_variation,
                strength: s.ground_strength,
                breakup: Some(wce_tile::texture_rules::Breakup { scale: 6.0, amount: 0.5 }),
                seed: a.seed,
                area: Some(i),
                ..wce_tile::texture_rules::Rule::default()
            });
        }

        (rules, masks)
    }
}

impl TileDocument {
    /// The farmland laid now (scripts).
    pub fn finish_fields(&mut self) {
        self.tile_mut().refresh_fields(true);
    }

    /// How many crop decals, trees, and props and fence pieces the field areas lay.
    #[must_use]
    pub fn field_counts(&self) -> (usize, usize, usize) {
        let t = self.tile();
        let trees = t.fields.items.iter().filter(|i| i.text().starts_with(b"<VEGETATION_INSTANCE")).count();
        (t.fields.decals.len(), trees, t.fields.items.len() - trees)
    }
}
