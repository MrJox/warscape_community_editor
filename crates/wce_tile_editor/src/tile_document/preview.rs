//! What the Tile tool offers to place: the valid keys of each type (from the game's files and the kit's tables), and a preview of the
//! thing being placed that follows the mouse until a click places it.
//!
//! The preview is drawn from the text that will be inserted (a new instance, a building, a prefab's group), read as a scene of its own: its
//! instances' models, decals and effects, a marker box for the others and the outlines of its polygons, the whole at the cursor (instances
//! standing on the terrain where their surface mode says so). Its entities are not pickable and are not in the document.

use wce_foundation::math::{Double3, Float3, Float4x4};
use wce_scene::render_scene::{Decal, EntityId, MeshInstance, Overlay, Payload, RenderScene, VfxEmitter, overlay_colour};
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::{WorldTransform, world_transform};

use super::index::{matrix_of, static_piece, vegetation_model};
use super::{TERRAIN, Tile, scene_key};

/// The preview's entities and what they were made from.
#[derive(Default)]
pub(crate) struct Preview {
    text: Vec<u8>,
    items: Vec<PreviewItem>,

    /// Marker boxes and polygon outlines, in metres around the preview's origin.
    lines: Vec<(Float3, Float3)>,
    entities: Vec<EntityId>,
    overlay: EntityId,
    at: Option<Float3>,
    scene_key: usize,
}

struct PreviewItem {
    /// Its matrix in metres around the preview's origin, and whether it stands on the terrain.
    local: Float4x4,
    landscape: bool,
    payload: Payload,
}

impl Tile {
    /// The keys an instance of `kind` can have, sorted: the building keys (the kit's `battlefield_buildings` and the folders of building
    /// models), the trees (`species.variant`), the decal models (as TEd writes them, with backslashes), the models of a simple mesh and the
    /// effects. Empty for the other types and without the game's files.
    pub fn key_catalogue(&self, kind: &str) -> Vec<String> {
        let Some(files) = self.files.as_deref() else { return vec![] };
        let mut keys = match kind {
            "BUILDING_INSTANCE" => {
                let prefix = "rigidmodels/buildings/";
                let mut keys = files.list(prefix, ".rigid_model_v2").into_iter().filter_map(|p| {
                    let slash = p.rfind('/')?;
                    p.get(prefix.len()..slash).filter(|f| !f.is_empty()).map(str::to_owned)
                }).collect::<Vec<_>>();
                keys.extend(self.building_db.get_or_init(|| wce_tile::building_db::BuildingDb::load_near(&self.project.dir)).keys().map(str::to_owned));
                keys
            }

            "VEGETATION_INSTANCE" => ["shrubs", "trees"].iter().flat_map(|dir| {
                let prefix = format!("battleterrain/vegetation/{dir}/");
                files.list(&prefix, ".rigid_model_v2").into_iter().filter_map(move |p| {
                    let (species, variant) = p.strip_prefix(&prefix)?.strip_suffix(".rigid_model_v2")?.split_once('/')?;
                    (!variant.contains('/')).then(|| format!("{species}.{variant}"))
                }).collect::<Vec<_>>()
            }).collect(),

            "ProjectedDecalInstance" => files.list("rigidmodels/decals/", ".rigid_model_v2").into_iter().map(|p| p.replace('/', "\\")).collect(),
            "SIMPLE_MESH_INSTANCE" => files.list("rigidmodels/", ".rigid_model_v2"),
            "VFXInstance" => files.list("vfx/", ".xml").into_iter().filter_map(|p| p.strip_prefix("vfx/")?.strip_suffix(".xml").map(str::to_owned)).collect(),
            _ => vec![],
        };

        keys.sort_unstable_by_key(|k| k.to_lowercase());
        keys.dedup();
        keys
    }

    /// Shows `text` (an element as `insert_element_text` takes it) at `at` (world metres, on the ground), or nothing (`None`). Reading the
    /// text again only when it changed; moving it is cheap.
    pub fn set_preview(&mut self, scene: &mut RenderScene, text: Option<&[u8]>, at: Float3) {
        let key = scene_key(scene);
        let gone = self.preview.scene_key != key || self.preview.entities.iter().chain([&self.preview.overlay]).any(|&e| e != 0 && scene.find(e).is_none());
        let Some(text) = text else {
            self.clear_preview(scene);
            return;
        };

        if gone || self.preview.text != text {
            self.clear_preview(scene);
            self.build_preview(text);
            self.preview.text = text.to_vec();
            self.preview.scene_key = key;
            for item in &self.preview.items {
                let e = scene.add(self.layer, item.local, item.payload.clone(), self.source(TERRAIN));
                if let Some(en) = scene.find_mut(e) {
                    en.locked = true;
                }

                self.preview.entities.push(e);
            }

            let o = Overlay { pickable: false, ..Default::default() };
            self.preview.overlay = scene.add(self.layer, Float4x4::IDENTITY, Payload::Overlay(o), self.source(TERRAIN));
            self.preview.at = None;
        }

        if self.preview.at == Some(at) {
            return;
        }

        self.preview.at = Some(at);
        for (item, &e) in self.preview.items.iter().zip(&self.preview.entities) {
            let mut m = item.local;
            let (x, z) = (m.m[0][3] + at.x, m.m[2][3] + at.z);
            m.m[0][3] = x;
            m.m[2][3] = z;
            m.m[1][3] += if item.landscape { self.terrain_height(x, z) } else { at.y };
            scene.update_world(e, m);
        }

        let mut o = Overlay { pickable: false, ..Default::default() };
        let gold = overlay_colour(1.0, 0.8, 0.2, 1.0);
        // On the terrain under each end (the parts of a polygon follow the ground; a marker's box stands on it).
        let on_ground = |p: Float3| {
            let (x, z) = (p.x + at.x, p.z + at.z);
            Float3::new(x, self.terrain_height(x, z) + p.y + 0.2, z)
        };

        for &(a, b) in &self.preview.lines {
            o.line(on_ground(a), on_ground(b), gold);
        }

        scene.update_payload(self.preview.overlay, Payload::Overlay(o));
    }

    fn clear_preview(&mut self, scene: &mut RenderScene) {
        for e in self.preview.entities.drain(..).chain([self.preview.overlay]) {
            if e != 0 && scene.find(e).is_some() {
                scene.remove(e);
            }
        }

        self.preview = super::preview::Preview::default();
    }

    /// The preview's parts from the text: every instance of it, relative to its root element.
    fn build_preview(&mut self, text: &[u8]) {
        let Ok(mini) = TedScene::parse_any(text.to_vec()) else { return };
        let unit = self.unit();
        let els = mini.doc().elements();
        let Some(root) = (0..els.len()).find(|&e| els[e].parent.is_none()) else { return };
        let origin = world_transform(&mini, root).position;
        let mut items = vec![];
        let mut lines = vec![];
        for (e, el) in els.iter().enumerate() {
            if matches!(el.name.as_str(), "point" | "points" | "children") || mini.position(e).is_none() {
                continue;
            }

            let w = world_transform(&mini, e);
            let w = WorldTransform { position: w.position - origin, ..w };
            let scale = mini.scale(e).unwrap_or(Double3::new(1.0, 1.0, 1.0));
            let local = matrix_of(&w, unit, Some(scale * (1.0 / unit)));
            let at = Float3::new(local.m[0][3], local.m[1][3], local.m[2][3]);
            let landscape = mini.doc().get(e, "surface_mode").is_none_or(|m| m == "MSM_LANDSCAPE");
            let get = |k: &str| mini.doc().get(e, k).filter(|v| !v.is_empty());

            // Polygons: their outline.
            let points = mini.polygon_points(e);
            if !points.is_empty() {
                let world = points.iter().filter_map(|&p| mini.point_position(p)).map(|v| {
                    let p = w.position + w.rotation * v;
                    Float3::new((p.x / unit) as f32, (p.y / unit) as f32, (p.z / unit) as f32)
                }).collect::<Vec<_>>();
                let closed = mini.doc().get(e, "polygon_closed").is_none_or(|v| v != "0") && !wce_tile::splines::is_spline(mini.type_of(e));
                for i in 0..world.len() {
                    if i + 1 < world.len() || (closed && world.len() > 2) {
                        lines.push((world[i], world[(i + 1) % world.len()]));
                    }
                }

                continue;
            }

            let models = match mini.type_of(e) {
                "BUILDING_INSTANCE" => get("key").map(|k| self.preview_building(&k)).unwrap_or_default(),
                "VEGETATION_INSTANCE" => get("key").and_then(|k| vegetation_model(self.files.as_deref(), &k)).into_iter().collect(),
                "SIMPLE_MESH_INSTANCE" => get("key").map(|k| k.replace('\\', "/").to_lowercase()).filter(|p| self.files.as_deref().is_some_and(|f| f.exists(p))).into_iter().collect(),
                _ => vec![],
            };

            let payloads = match mini.type_of(e) {
                "ProjectedDecalInstance" => get("key").map(|k| Payload::Decal(Decal { model: k.replace('\\', "/"), tiled: false, opacity: 1.0, parallax: 0.0 })).into_iter().collect(),
                "POINT_LIGHT_INSTANCE" => vec![Payload::PointLight(super::view::point_light_of(mini.doc(), e, 0.0))],
                "VFXInstance" => get("vfx").map(|effect| {
                    let scale = get("scale").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(1.0) / unit;
                    Payload::VfxEmitter(VfxEmitter { effect, scale: scale as f32, mesh: None })
                }).into_iter().collect(),
                kind => models.into_iter().map(|model| {
                    // A tree's leaves need "nothing dissolved" (see `add_item_entities`).
                    let p4 = if kind == "VEGETATION_INSTANCE" { [0.5, 1.0, 0.0, 1000.0] } else { MeshInstance::default().p4 };
                    Payload::MeshInstance(MeshInstance { model, lod: true, p4, ..Default::default() })
                }).collect::<Vec<_>>(),
            };

            // A marker where nothing is drawn, and around an effect (a torch's flame is a few centimetres) and a light (it lights, it has no shape).
            if payloads.is_empty() || matches!(mini.type_of(e), "VFXInstance" | "POINT_LIGHT_INSTANCE") {
                let h = 1.0;
                for (a, b) in box_edges(Float3::new(at.x - h, at.y, at.z - h), Float3::new(at.x + h, at.y + 2.0 * h, at.z + h)) {
                    lines.push((a, b));
                }
            }

            for payload in payloads {
                items.push(PreviewItem { local, landscape, payload });
            }
        }

        self.preview.items = items;
        self.preview.lines = lines;
    }

    /// A building's standing pieces: the document's when it uses the key, else its folder's intact pieces.
    fn preview_building(&self, key: &str) -> Vec<String> {
        if let Some(m) = self.models.get(key).filter(|m| m.ok) {
            return m.pieces.clone();
        }

        let Some(files) = self.files.as_deref() else { return vec![] };
        files.list(&format!("rigidmodels/buildings/{}/", key.to_lowercase()), ".rigid_model_v2").into_iter().filter(|p| {
            let file = &p[p.rfind('/').map_or(0, |s| s + 1)..];
            static_piece(file) && (!file.contains("destruct") || file.contains("destruct01"))
        }).collect()
    }
}

/// The 12 edges of a box.
fn box_edges(lo: Float3, hi: Float3) -> Vec<(Float3, Float3)> {
    let c = |i: u8| Float3::new(if i & 1 != 0 { hi.x } else { lo.x }, if i & 2 != 0 { hi.y } else { lo.y }, if i & 4 != 0 { hi.z } else { lo.z });
    let mut out = vec![];
    for i in 0..8u8 {
        for bit in [1u8, 2, 4] {
            if i & bit == 0 {
                out.push((c(i), c(i | bit)));
            }
        }
    }

    out
}
