//! What a tile document shows: the terrain (and its ground look), the building models, the markers, polygon outlines, water, tile links and the
//! highlight overlay, and the outline and descriptions of its items.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use rayon::prelude::*;
use wce_editor_core::document::OutlinerNode;
use wce_formats::c_num::strtod;
use wce_foundation::math::{Double3, Float3, Float4x4, dot, translation};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{TerrainImage, Decal, 
    EntityId, LayerId, LfMap, MeshInstance, Overlay, OverlayVertex, Payload, PointLight, RenderScene, SourceRef, Terrain, TerrainMaterial, TerrainSkin, VfxEmitter, Water, WaterVertex, overlay_colour,
};
use wce_tile::blend_field::BlendField;
use wce_tile::ground_type_field::GroundTypeField;
use wce_tile::ted_scene::parse_vec3;
use wce_tile::world_transform::{WorldTransform, euler_matrix, world_transform};

use super::index::{matrix_of, style_of};
use super::{GroundInfo, LinkCell, MATERIAL_IDS, NORMAL_IDS, PieceProps, SKIN_IDS, TERRAIN, Tile, elem_of, item_of, next_id, piece_item, piece_of, scene_key};
use crate::terrain_bake::{BakeRequest, TerrainLook, TerrainView, bake_terrain_colour, bake_terrain_map_view};
use crate::terrain_material::{MaterialRequest, build_terrain_material, patch_blend_maps, terrain_normal_image};

#[derive(Clone, Copy)]
struct RiverPoint {
    /// World metres; y = the spline's own height.
    x: f64,
    z: f64,
    y: f64,

    width: f64,
    foam: f64,
    fade: f64,
    flow: f64,

    /// Control handles as world positions (x, z metres, y local metres).
    cin: [f64; 3],
    cout: [f64; 3],
}

/// A piece's own transform inside its building: the position (scene units), the euler angles (radians) and the scale.
pub(crate) fn piece_local(p: &PieceProps) -> Float4x4 {
    let w = WorldTransform { position: Double3::new(f64::from(p.position[0]), f64::from(p.position[1]), f64::from(p.position[2])), rotation: euler_matrix(Double3::new(f64::from(p.euler[0]), f64::from(p.euler[1]), f64::from(p.euler[2]))) };
    matrix_of(&w, 1.0, Some(Double3::new(f64::from(p.scale[0]), f64::from(p.scale[1]), f64::from(p.scale[2]))))
}

/// The model of a piece at a destruction level of a building that has `levels`: the highest level is the intact piece (`..._destruct01`, what the
/// tile draws), each lower one the next more damaged file (`..._destruct02` ...), level 0 the last.
/// The piece file of a destruction level: 0 is the intact `destruct01` (the vanilla tiles store 0 on almost every building), level `n` is
/// `destruct{n + 1}`, up to the building's last level.
pub(crate) fn piece_model(base: &str, level: i64, levels: u32) -> String {
    let top = i64::from(levels.max(1)) - 1;
    if level <= 0 || !base.contains("destruct01") {
        return base.to_owned();
    }

    base.replacen("destruct01", &format!("destruct{:02}", level.min(top) + 1), 1)
}

fn bits_equal(a: &[OverlayVertex], b: &[OverlayVertex]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.rgba == y.rgba && x.p.x.to_bits() == y.p.x.to_bits() && x.p.y.to_bits() == y.p.y.to_bits() && x.p.z.to_bits() == y.p.z.to_bits())
}

fn link_colour(set: &str, alpha: f32) -> u32 {
    const TABLE: [(&str, f32, f32, f32); 8] = [
        ("sea", 0.15, 0.35, 0.95), ("river", 0.1, 0.85, 0.95), ("roads", 0.9, 0.75, 0.35), ("sea_coast", 0.95, 0.9, 0.2), ("roads_stone", 0.7, 0.7, 0.75),
        ("roads_mountain", 0.6, 0.4, 0.2), ("river_shallow", 0.5, 0.95, 0.8), ("cliff_custom", 0.95, 0.35, 0.2),
    ];

    if let Some(&(_, r, g, b)) = TABLE.iter().find(|c| c.0 == set) {
        return overlay_colour(r, g, b, alpha);
    }

    let h = set.bytes().fold(2_166_136_261u32, |h, c| (h ^ u32::from(c)).wrapping_mul(16_777_619));
    overlay_colour(0.35 + (h & 255) as f32 / 400.0, 0.35 + ((h >> 8) & 255) as f32 / 400.0, 0.35 + ((h >> 16) & 255) as f32 / 400.0, alpha)
}

/// Cuts a material's blend / index maps down to a rectangle inside the one they cover (whole pixels; the rectangle follows them).
pub(crate) fn crop_material(m: &mut TerrainMaterial, x0: f64, z0: f64, x1: f64, z1: f64) {
    let (w, h) = (m.blend.w, m.blend.h);
    if w == 0 || h == 0 || m.x1 <= m.x0 || m.z1 <= m.z0 {
        return;
    }

    let (ppx, ppz) = (f64::from(w) / (m.x1 - m.x0), f64::from(h) / (m.z1 - m.z0));
    let (i0, j0) = (((x0 - m.x0) * ppx).round().clamp(0.0, f64::from(w - 1)) as u32, ((z0 - m.z0) * ppz).round().clamp(0.0, f64::from(h - 1)) as u32);
    let (i1, j1) = (((x1 - m.x0) * ppx).round().clamp(f64::from(i0 + 1), f64::from(w)) as u32, ((z1 - m.z0) * ppz).round().clamp(f64::from(j0 + 1), f64::from(h)) as u32);
    let crop = |img: &TerrainImage| -> TerrainImage {
        let Some(px) = &img.rgba else { return img.clone() };
        if img.w != w || img.h != h {
            return img.clone();
        }

        let mut out = Vec::with_capacity(((i1 - i0) * (j1 - j0) * 4) as usize);
        for j in j0..j1 {
            let row = (j * w) as usize * 4;
            out.extend_from_slice(&px[row + i0 as usize * 4..row + i1 as usize * 4]);
        }

        TerrainImage { w: i1 - i0, h: j1 - j0, rgba: Some(Arc::new(out)) }
    };

    m.blend = crop(&m.blend);
    m.index = crop(&m.index);
    let (nx0, nz0) = (m.x0 + f64::from(i0) / ppx, m.z0 + f64::from(j0) / ppz);
    let (nx1, nz1) = (m.x0 + f64::from(i1) / ppx, m.z0 + f64::from(j1) / ppz);
    (m.x0, m.z0, m.x1, m.z1) = (nx0, nz0, nx1, nz1);
}

fn empty_overlay() -> Payload {
    Payload::Overlay(Overlay { pickable: false, ..Default::default() })
}

impl Tile {

    pub(crate) fn source(&self, item: u64) -> SourceRef {
        SourceRef { doc: self.id, item }
    }

    // ---- ground look

    /// The ground's look: a view setting, not an edit (not on the undo stack and not saved).
    /// Draws the terrain over the tile's own cells only (the map context draws its neighbours where the border was), or all of it again.
    pub fn set_clip_to_tile(&mut self, scene: &mut RenderScene, on: bool) {
        if self.clip_to_tile == on {
            return;
        }

        self.clip_to_tile = on;

        // The tile's own sea gives way to the map context's.
        self.water_dirty = true;
        if self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
            self.update_terrain(scene, false);
        }
    }

    /// The tile's ground material (its whole blend map), if it has one: the map context makes its plain ground from it.
    pub fn ground_material_base(&self) -> Option<Arc<TerrainMaterial>> {
        self.material_base.clone()
    }

    /// The map context's LF map under the tile (its ground is drawn on it as the game does, relative to the mean LF under it), or none.
    pub fn set_context_lf(&mut self, scene: &mut RenderScene, lf: Option<Arc<LfMap>>) {
        if self.context_lf.as_ref().map(|l| l.id) == lf.as_ref().map(|l| l.id) {
            return;
        }

        self.context_lf = lf;
        if self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
            self.update_terrain(scene, false);
        }
    }

    pub fn terrain_look(&self) -> &TerrainLook {
        &self.look
    }

    /// The water depth view's raster (the tool works it out from stage B's water map); the view is drawn again when it shows.
    pub fn set_water_depth(&mut self, scene: &mut RenderScene, depth: Option<crate::terrain_bake::WaterDepth>) {
        self.water_depth = depth.map(Arc::new);
        if self.look.view == TerrainView::WaterDepth {
            self.skin = None;
            if self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
                self.update_terrain(scene, false);
            }
        }
    }

    pub fn water_depth(&self) -> Option<&crate::terrain_bake::WaterDepth> {
        self.water_depth.as_deref()
    }

    pub fn set_terrain_look(&mut self, scene: &mut RenderScene, look: TerrainLook) {
        if look == self.look {
            return;
        }

        let season_changed = look.season != self.look.season;
        self.look = look;
        if self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
            self.update_terrain(scene, false);
        }

        // The placed effects play in their seasons only: their entities are made again.
        if season_changed && self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
            let layer = self.layer;
            self.rebuild_items(true);
            self.publish_items(scene, layer, true);
        }
    }

    /// Whether a VFXInstance plays in the terrain look's season (`seasons`, all when missing).
    fn in_season(&self, elem: usize) -> bool {
        let Some(text) = self.tscene().doc().get(elem, "seasons") else { return true };
        let on = wce_tile::properties::parse_seasons(&text);
        match self.look.season.as_str() {
            "wi" => on[0],
            "sp" => on[1],
            "au" => on[3],
            _ => on[2],
        }
    }

    /// What the colour is, or why it is the plain texture.
    pub fn terrain_note(&self) -> &str {
        &self.terrain_note
    }

    /// The debug view's image (blend groups, ground types, protection).
    pub fn baked_colour(&self) -> Option<&crate::terrain_bake::BakedColour> {
        self.skin_baked.as_deref()
    }

    pub fn ground_info(&self) -> &GroundInfo {
        &self.ground_info
    }

    /// The bake / material area: the bordered tile.
    fn blend_area(&self) -> (f64, f64, f64, f64) {
        let b = self.project.border_m();
        (-b, -b, self.extent_x() + b, self.extent_z() + b)
    }

    fn has_blend_area(&self) -> bool {
        self.project.has_height() && self.project.has_entry() && self.project.bordered()
    }

    /// The skin of the terrain payload for the current look: baked once per (climate, season) and kept until the look changes. The debug views
    /// of the tile's maps (TEd's blend groups, ground types, protection) are images over the ground; the game-look colour is the material.
    fn ground_skin(&mut self) -> Option<Arc<TerrainSkin>> {
        if !self.look.blended {
            self.terrain_note = "plain tiled texture (blend off)".to_owned();
            return None;
        }

        // The material's business (ground_material); no CPU stand-in for the game's blend.
        if self.look.view == TerrainView::Colour {
            return None;
        }

        // Painted since the view was baked: the ground types at every step of a stroke, the blend groups (a whole bake) when it ends.
        let stale = match self.look.view {
            TerrainView::GroundTypes | TerrainView::WaterDepth => self.project.ground.as_mut().and_then(GroundTypeField::take_changed).is_some(),
            TerrainView::BlendGroups if !self.stroke_open => self.project.blend.as_mut().and_then(BlendField::take_changed).is_some(),
            _ => false,
        };

        if self.skin.is_some() && self.skin_look == self.look && !stale {
            return self.skin.clone();
        }

        self.skin = None;
        self.skin_baked = None;
        self.skin_look = self.look.clone();
        if !self.has_blend_area() {
            self.terrain_note = "plain tiled texture (the tile has no database entry or no height map: no blend map area)".to_owned();
            return None;
        }

        let p = &self.project;
        let (x0, z0, x1, z1) = self.blend_area();
        let rq = BakeRequest {
            blend_map: p.dir.join("blend_map.tif"), folder: p.dir.clone(), groups: p.entry.as_ref().expect("an entry").texture_group(), x0, z0, w_m: x1 - x0,
            h_m: z1 - z0, look: self.look.clone(), ..Default::default()
        };

        let Some(files) = self.files.clone() else {
            self.terrain_note = "plain tiled texture (no game files)".to_owned();
            return None;
        };

        // The maps in memory when read (painted), else their files.
        if matches!(self.look.view, TerrainView::GroundTypes | TerrainView::WaterDepth) {
            let _ = self.project.load_ground();
        }

        if self.look.view == TerrainView::WaterDepth {
            let Some(water) = self.water_depth.clone() else {
                self.terrain_note = "water depth: being worked out (stage B's water map)".to_owned();
                self.skin_look = TerrainLook::default();
                return None;
            };

            let baked = Arc::new(crate::terrain_bake::bake_water_depth(&rq, &water, self.project.ground.as_ref()));
            let k = self.project.mpp_z() / self.project.mpp_x();
            self.skin_id = next_id(&SKIN_IDS);
            let skin = Arc::new(TerrainSkin { x0: baked.x0, z0: baked.z0 / k, w_m: baked.w_m, h_m: baked.h_m / k, w: baked.w, h: baked.h, rgba: baked.rgba.clone(), id: self.skin_id });
            self.skin = Some(skin.clone());
            self.terrain_note = format!("debug view, {}", baked.report);
            self.skin_baked = Some(baked);
            return Some(skin);
        }

        let map_view = matches!(self.look.view, TerrainView::GroundTypes | TerrainView::Protection);
        let baked = if map_view {
            bake_terrain_map_view(&rq, self.project.ground.as_ref())
        } else {
            bake_terrain_colour(&rq, self.project.blend.as_ref(), self.climates(), files.as_ref())
        };
        let baked = match baked {
            Ok(b) => Arc::new(b),
            Err(why) => {
                self.terrain_note = format!("plain tiled texture: {why}");
                log::info(format!("terrain: {}", self.terrain_note));
                return None;
            }
        };

        // The terrain entity's z axis is scaled by k (rows of the height map vs columns): the skin is in the terrain's own coordinates.
        let k = self.project.mpp_z() / self.project.mpp_x();
        self.skin_id = next_id(&SKIN_IDS);
        let skin = Arc::new(TerrainSkin {
            x0: baked.x0, z0: baked.z0 / k, w_m: baked.w_m, h_m: baked.h_m / k, w: baked.w, h: baked.h, rgba: baked.rgba.clone(), id: self.skin_id,
        });

        self.skin = Some(skin.clone());
        self.terrain_note = format!("{}{}", if self.look.view == TerrainView::Colour { "blended ground: " } else { "debug view, " }, baked.report);
        self.skin_baked = Some(baked);
        log::info(format!("terrain: {}", self.terrain_note));
        Some(skin)
    }

    /// The ground for the game's terrain shaders: the blend / index maps and the climate's texture list are made once per look, the normal map
    /// from the current heights every time (a stroke changes it).
    fn ground_material(&mut self, t: &Terrain) -> Option<Arc<TerrainMaterial>> {
        self.ground_info = GroundInfo::default();
        if !self.look.blended || self.look.view != TerrainView::Colour {
            self.material_base = None;
            return None;
        }

        if self.material_base.is_none() || self.material_look != self.look {
            self.material_base = None;
            self.material_clip_id = 0;
            self.material_look = self.look.clone();
            if !self.has_blend_area() {
                self.terrain_note = "plain tiled texture (the tile has no database entry or no height map: no blend map area)".to_owned();
                return None;
            }

            let Some(files) = self.files.clone() else {
                self.terrain_note = "plain tiled texture (no game files)".to_owned();
                return None;
            };

            if let Err(why) = self.project.load_blend() {
                self.terrain_note = format!("plain tiled texture: {why}");
                return None;
            }

            let p = &self.project;
            let h = p.height.as_ref().expect("a height map");
            let blend = p.blend.as_ref().expect("read above");
            let (x0, z0, x1, z1) = self.blend_area();
            self.material_id = next_id(&MATERIAL_IDS);
            let rq = MaterialRequest {
                folder: p.dir.clone(), groups: p.entry.as_ref().expect("an entry").texture_group(), x0, z0, x1, z1, y0: h.min_height() / p.unit_scale, y1: (h.min_height() + h.scale()) / p.unit_scale,
                look: self.look.clone(), id: self.material_id,
            };

            let built = build_terrain_material(&rq, blend, self.climates(), files.as_ref());
            if let Some(b) = self.project.blend.as_mut() {
                b.take_changed();
            }

            match built {
                Ok((m, report)) => {
                    self.terrain_note = format!("blended ground: {report}");
                    log::info(format!("terrain: {}", self.terrain_note));
                    let climate = self.climates().find(&self.look.climate).map_or_else(|| "default".to_owned(), |c| c.name.clone());
                    let layers = m.diffuse.iter().filter(|d| !d.is_empty()).count() as i32;
                    self.ground_info_base = GroundInfo { game_shaders: true, layers, climate, season: self.look.season.clone() };
                    self.material_base = Some(Arc::new(m));
                }

                Err(why) => {
                    self.terrain_note = format!("plain tiled texture: {why}");
                    log::info(format!("terrain: {}", self.terrain_note));
                    return None;
                }
            }
        }

        // Painted since: the maps again over what changed.
        if let Some(b) = self.project.blend.as_mut()
            && let Some(rect) = b.take_changed()
            && let Some(base) = self.material_base.as_mut()
        {
            let m = Arc::make_mut(base);
            patch_blend_maps(m, self.project.blend.as_ref().expect("taken above"), rect);
            m.blend_id = next_id(&NORMAL_IDS);
        }

        self.ground_info = self.ground_info_base.clone();
        let mut m = (**self.material_base.as_ref()?).clone();
        if self.clip_to_tile {
            // Other maps: another material for the renderer (it keeps a material's maps by its id).
            crop_material(&mut m, 0.0, 0.0, self.extent_x(), self.extent_z());
            if self.material_clip_id == 0 {
                self.material_clip_id = next_id(&MATERIAL_IDS);
            }

            m.id = self.material_clip_id;
        }

        let k = self.project.mpp_z() / self.project.mpp_x();
        m.normal = terrain_normal_image(&t.heights, t.nx, t.nz, f64::from(t.step), f64::from(t.step) * k, m.blend.w, m.blend.h);
        self.normal_id = next_id(&NORMAL_IDS);
        m.normal_id = self.normal_id;
        Some(Arc::new(m))
    }

    // ---- water

    pub fn water_pieces(&self) -> usize {
        self.water_entities.len()
    }

    pub fn water_note(&self) -> &str {
        &self.water_note
    }

    /// Rivers: every SplineRiverInstance is a chain of Bezier segments (point i's position and controlOut, point i+1's controlIn and position,
    /// all local to the instance in scene units); the ribbon follows it with the point's width, at the spline's own height, landscape or freeform:
    /// the exported river_spline*.xml the game reads has the points' stored heights and no terrain_offset. terrain_offset is the point's height
    /// over the ground under it when TEd placed it (osgiliath minor: 23 of 26 points to the millimetre; up to 0.8 m off where the ground changed
    /// since). Adding it to the ground under every sample lifted the water of osgiliath minor's second river over the ford it starts on.
    /// The sea plane: TEd draws the sea at 0 m over every tile, whatever its tile set, as context (it is not in the file); so does this, over
    /// the bordered tile, unless the map context is shown (it draws the sea over the whole area, at 0 m relative to the open tile's place).
    /// The depth test cuts it where the ground is higher.
    fn rebuild_water(&mut self, scene: &mut RenderScene, layer: LayerId) {
        for &e in &self.water_entities {
            scene.remove(e);
        }

        self.water_entities.clear();
        self.water_dirty = false;
        self.water_note.clear();
        if !self.display.show_water && !self.display.show_sea {
            self.water_note = "water hidden".to_owned();
            return;
        }

        let u = self.unit();
        let (mut rivers, mut ribbons) = (0, 0);
        let mut new_water = vec![];
        let sc = self.tscene();
        let els = sc.doc().elements();
        for e in 0..els.len() {
            if els[e].name != "SplineRiverInstance" || !self.display.show_water {
                continue;
            }

            let w = world_transform(sc, e);
            let mut pts = vec![];
            for &c in els[e].children.iter().filter(|&&c| els[c].name == "points") {
                for &p in els[c].children.iter().filter(|&&p| els[p].name == "point") {
                    let vec = |name: &str| sc.doc().get(p, name).and_then(|v| parse_vec3(&v, ""));
                    let Some(pos) = vec("position") else { continue };
                    let world = |q: Double3| {
                        let p = w.position + w.rotation * q;
                        [p.x / u, p.y / u, p.z / u]
                    };

                    let num = |n: &str, d: f64| sc.doc().get(p, n).filter(|v| !v.is_empty()).map_or(d, |v| strtod(&v).unwrap_or(0.0));
                    let wp = world(pos);
                    pts.push(RiverPoint {
                        x: wp[0], y: wp[1], z: wp[2], cin: world(vec("controlIn").unwrap_or(pos)), cout: world(vec("controlOut").unwrap_or(pos)),
                        width: num("width", 1.0), foam: num("foam_amount", 0.1), fade: num("alpha_fade", 1.0), flow: num("flow_speed", 0.5),
                    });
                }
            }

            if pts.len() < 2 {
                continue;
            }

            rivers += 1;
            let bez = |a: f64, b: f64, c: f64, d: f64, t: f64| {
                let s = 1.0 - t;
                s * s * s * a + 3.0 * s * s * t * b + 3.0 * s * t * t * c + t * t * t * d
            };

            struct Sample {
                x: f64,
                z: f64,
                y: f64,
                width: f64,
                flow: f64,
                foam: f64,
            }

            let mut samples = vec![];
            for i in 0..pts.len() - 1 {
                let (a, b) = (&pts[i], &pts[i + 1]);
                let chord = (b.x - a.x).hypot(b.z - a.z);
                let n = ((chord / 2.0f64.max(a.width.min(b.width) * 0.2)).ceil() as i32).clamp(2, 40);
                for k in (if i == 0 { 0 } else { 1 })..=n {
                    let t = f64::from(k) / f64::from(n);
                    let x = bez(a.x, a.cout[0], b.cin[0], b.x, t);
                    let z = bez(a.z, a.cout[2], b.cin[2], b.z, t);
                    let y = bez(a.y, a.cout[1], b.cin[1], b.y, t);
                    samples.push(Sample {
                        x, z, y, width: a.width + (b.width - a.width) * t, flow: a.flow + (b.flow - a.flow) * t, foam: a.foam + (b.foam - a.foam) * t,
                    });
                }
            }

            if samples.len() < 2 {
                continue;
            }

            let mut wt = Water { river: true, ..Default::default() };
            let mut foam_sum = 0.0;
            let mut cum = vec![0.0f64; samples.len()];
            for i in 1..samples.len() {
                cum[i] = cum[i - 1] + (samples[i].x - samples[i - 1].x).hypot(samples[i].z - samples[i - 1].z);
            }

            let total = *cum.last().expect("samples");
            let fade_ends = pts[0].fade > 0.01 || pts[pts.len() - 1].fade > 0.01;
            for (i, s) in samples.iter().enumerate() {
                let p0 = &samples[i.saturating_sub(1)];
                let p1 = &samples[if i + 1 < samples.len() { i + 1 } else { i }];
                let (mut tx, mut tz) = (p1.x - p0.x, p1.z - p0.z);
                let len = tx.hypot(tz);
                if len < 1e-9 {
                    (tx, tz) = (1.0, 0.0);
                } else {
                    tx /= len;
                    tz /= len;
                }

                let (nx, nz, hw) = (-tz, tx, s.width * 0.5);
                let fadelen = 8.0f64.max(s.width);
                let fade = if fade_ends { (cum[i].min(total - cum[i]) / fadelen).clamp(0.0, 1.0) as f32 } else { 1.0 };
                let y = s.y as f32;
                let vertex = |sign: f64| WaterVertex {
                    x: (s.x + sign * nx * hw) as f32, y, z: (s.z + sign * nz * hw) as f32, across: (sign * hw) as f32, u: (sign * hw) as f32, v: cum[i] as f32,
                    flow: s.flow as f32, fade,
                };

                wt.verts.push(vertex(-1.0));
                wt.verts.push(vertex(1.0));
                foam_sum += s.foam;
                if i != 0 {
                    let a = (2 * (i - 1)) as u32;
                    wt.indices.extend([a, a + 2, a + 1, a + 1, a + 2, a + 3]);
                }
            }

            wt.foam = (foam_sum / samples.len() as f64) as f32;
            ribbons += samples.len();
            new_water.push(wt);
        }

        // The sea plane. Lakes are not the sea: a lakes tile has its water as SIMPLE_MESH_INSTANCE water planes (water_plane models scaled
        // over the lake, at the lake's own height: -6.9 m on lakes/10x10/aa), drawn as models.
        let sea = self.display.show_sea && !self.clip_to_tile;
        if sea {
            let b = self.project.border_m() as f32;
            let (x0, z0, x1, z1) = (-b, -b, self.extent_x() as f32 + b, self.extent_z() as f32 + b);
            let v = |x: f32, z: f32| WaterVertex { x, y: 0.0, z, across: 0.0, u: x, v: z, flow: 0.0, fade: 1.0 };
            new_water.push(Water { river: false, foam: 0.0, verts: vec![v(x0, z0), v(x1, z0), v(x0, z1), v(x1, z1)], indices: vec![0, 2, 1, 1, 2, 3], ..Default::default() });
        }

        for mut wt in new_water {
            self.water_version += 1;
            wt.version = self.water_version;
            let e = scene.add(layer, Float4x4::IDENTITY, Payload::Water(wt), self.source(TERRAIN));
            self.water_entities.push(e);
        }

        self.water_note = if rivers != 0 || sea {
            format!("{rivers} river spline(s) ({ribbons} samples){}", if sea { ", the sea at 0 m" } else { "" })
        } else {
            "no water in this tile".to_owned()
        };
    }

    // ---- tile links (read-only view)

    /// World metres of an entry cell: the cell row y counts from the +z edge (y = cells_y - 1 - floor(z / 128), measured against the river
    /// targets of osgiliath).
    pub fn cell_rect(&self, x: i32, y: i32) -> (f32, f32, f32, f32) {
        let rows = if self.project.info.cells_y != 0 { self.project.info.cells_y as i32 } else { 1 };
        let x0 = x as f32 * 128.0;
        let z0 = (rows - 1 - y) as f32 * 128.0;
        (x0, z0, x0 + 128.0, z0 + 128.0)
    }

    /// The entry's TILE_LINK rules and TILE_LINK_TARGETs: targets first, then the rules, in the entry's order.
    pub fn link_cells(&self) -> Vec<LinkCell> {
        let Some(entry) = &self.project.entry else { return vec![] };
        let mut out = entry.link_targets().into_iter().map(|t| LinkCell {
            x: t.x, y: t.y, text: format!("offers a {} join at cell ({}, {})", t.tile_set, t.x, t.y), set: t.tile_set, equals: true, target: true, base_x: t.x, base_y: t.y,
        }).collect::<Vec<_>>();

        for l in entry.links() {
            let equals = l.test != "TLT_NOT_EQUALS";
            let text = format!(
                "cell ({}, {}) {}{}; belongs to cell ({}, {}){}, blend {} px{}", l.x, l.y, if equals { "must be " } else { "must not be " }, l.link_set, l.base_x, l.base_y,
                if l.is_entry { ", entry" } else { "" }, l.blend_size, if l.no_offline_blend { ", no offline blend" } else { "" }
            );

            out.push(LinkCell { x: l.x, y: l.y, set: l.link_set, text, equals, target: false, base_x: l.base_x, base_y: l.base_y });
        }

        out
    }

    pub fn links_drawn(&self) -> bool {
        self.links_entity != 0
    }

    fn rebuild_links(&mut self, scene: &mut RenderScene, layer: LayerId) {
        if self.links_entity != 0 {
            scene.remove(self.links_entity);
        }

        self.links_entity = 0;
        self.links_dirty = false;
        if !self.display.show_links {
            return;
        }

        let cells = self.link_cells();
        if cells.is_empty() {
            return;
        }

        let mut o = Overlay { pickable: false, ..Default::default() };
        let sub_n = 8;
        for c in &cells {
            let (x0, z0, x1, z1) = self.cell_rect(c.x, c.y);
            let fill = link_colour(&c.set, if c.target { 0.55 } else if c.equals { 0.30 } else { 0.20 });
            let line = link_colour(&c.set, 1.0);
            let at = |x: f32, z: f32| Float3::new(x, self.terrain_height(x, z) + 0.6, z);

            // The cell draped over the ground: sub_n x sub_n quads.
            for j in 0..sub_n {
                for i in 0..sub_n {
                    let xa = x0 + (x1 - x0) * i as f32 / sub_n as f32;
                    let xb = x0 + (x1 - x0) * (i + 1) as f32 / sub_n as f32;
                    let za = z0 + (z1 - z0) * j as f32 / sub_n as f32;
                    let zb = z0 + (z1 - z0) * (j + 1) as f32 / sub_n as f32;
                    o.triangle(at(xa, za), at(xa, zb), at(xb, zb), fill);
                    o.triangle(at(xa, za), at(xb, zb), at(xb, za), fill);
                }
            }

            // Outline; a rule that must NOT hold gets a cross as well, a target a pole at the cell's centre.
            let seg = 12;
            for s in 0..seg {
                let (a, b) = (s as f32 / seg as f32, (s + 1) as f32 / seg as f32);
                o.line(at(x0 + (x1 - x0) * a, z0), at(x0 + (x1 - x0) * b, z0), line);
                o.line(at(x0 + (x1 - x0) * a, z1), at(x0 + (x1 - x0) * b, z1), line);
                o.line(at(x0, z0 + (z1 - z0) * a), at(x0, z0 + (z1 - z0) * b), line);
                o.line(at(x1, z0 + (z1 - z0) * a), at(x1, z0 + (z1 - z0) * b), line);
            }

            if !c.equals && !c.target {
                for s in 0..seg {
                    let (a, b) = (s as f32 / seg as f32, (s + 1) as f32 / seg as f32);
                    o.line(at(x0 + (x1 - x0) * a, z0 + (z1 - z0) * a), at(x0 + (x1 - x0) * b, z0 + (z1 - z0) * b), line);
                    o.line(at(x0 + (x1 - x0) * a, z1 - (z1 - z0) * a), at(x0 + (x1 - x0) * b, z1 - (z1 - z0) * b), line);
                }
            }

            if c.target {
                let m = at((x0 + x1) * 0.5, (z0 + z1) * 0.5);
                o.line(m, Float3::new(m.x, m.y + 40.0, m.z), line);
                // The C++ code's 1.5707963f, which is not FRAC_PI_2 as a float.
                #[allow(clippy::approx_constant, reason = "the C++ code's constant: the float below FRAC_PI_2")]
                const QUARTER: f32 = 1.570_796_3;
                for s in 0..4 {
                    let (a, b) = (s as f32 * QUARTER, (s + 1) as f32 * QUARTER);
                    o.line(Float3::new(m.x + a.cos() * 8.0, m.y + 40.0, m.z + a.sin() * 8.0), Float3::new(m.x + b.cos() * 8.0, m.y + 40.0, m.z + b.sin() * 8.0), line);
                }
            } else if c.base_x != c.x || c.base_y != c.y {
                // The rule's own cell: a line from its centre to the cell it constrains.
                let (bx0, bz0, bx1, bz1) = self.cell_rect(c.base_x, c.base_y);
                let (from, to) = (at((bx0 + bx1) * 0.5, (bz0 + bz1) * 0.5), at((x0 + x1) * 0.5, (z0 + z1) * 0.5));
                o.line(Float3::new(from.x, from.y + 6.0, from.z), Float3::new(to.x, to.y + 6.0, to.z), line);
            }
        }

        self.links_entity = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(o), self.source(TERRAIN));
    }

    /// The automatic no-go areas, given by the tool (stage B's final heights, `WaterFields::no_go_outlines` in metres); `None` until worked
    /// out.
    pub fn set_auto_nogos(&mut self, nogos: Option<Vec<Vec<[f32; 2]>>>) {
        self.auto_nogos = nogos.map(Arc::new);
        self.nogo_dirty = true;
    }

    pub fn auto_nogos(&self) -> Option<&[Vec<[f32; 2]>]> {
        self.auto_nogos.as_deref().map(Vec::as_slice)
    }

    /// The automatic no-go areas' outlines draped over the ground (red, as the NOGO polygons), while they show.
    fn rebuild_auto_nogos(&mut self, scene: &mut RenderScene, layer: LayerId) {
        if self.nogo_entity != 0 {
            scene.remove(self.nogo_entity);
        }

        self.nogo_entity = 0;
        self.nogo_dirty = false;
        let Some(nogos) = self.auto_nogos.clone().filter(|_| self.display.show_auto_nogo) else { return };
        let mut o = Overlay { pickable: false, ..Default::default() };
        let line = overlay_colour(1.0, 0.25, 0.15, 1.0);
        let at = |x: f32, z: f32| Float3::new(x, self.terrain_height(x, z) + 0.6, z);
        for poly in nogos.iter() {
            for (k, a) in poly.iter().enumerate() {
                let b = poly[(k + 1) % poly.len()];
                // Long edges in steps, to follow the ground.
                let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / 4.0).ceil().max(1.0) as usize;
                for s in 0..n {
                    let (t0, t1) = (s as f32 / n as f32, (s + 1) as f32 / n as f32);
                    let p = |t: f32| at(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
                    o.line(p(t0), p(t1), line);
                }
            }
        }

        if !o.lines.is_empty() {
            self.nogo_entity = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(o), self.source(TERRAIN));
        }
    }

    // ---- publishing

    pub(crate) fn update_terrain(&mut self, scene: &mut RenderScene, first: bool) {
        // Landscape rivers lie on the terrain; the link cells are draped over it.
        self.water_dirty = true;
        self.links_dirty = true;
        self.nogo_dirty = true;
        let (world, t) = if let Some(hf) = &self.project.height {
            // The renderer cuts the grid into patches with LODs, the full resolution costs nothing far away.
            self.terrain_stride = 1;
            let stride = self.terrain_stride;
            let p = &self.project;
            // The pixels drawn: all, or the tile's own cells (the border cut off).
            let (px0, pz0, px1, pz1) = if self.clip_to_tile && p.bordered() {
                let (bx, bz) = ((p.border_m() / p.mpp_x()).round() as i32, (p.border_m() / p.mpp_z()).round() as i32);
                let (ix, iz) = (((p.border_m() + self.extent_x()) / p.mpp_x()).round() as i32, ((p.border_m() + self.extent_z()) / p.mpp_z()).round() as i32);
                (bx, bz, ix.min(hf.width() as i32 - 1), iz.min(hf.height() as i32 - 1))
            } else {
                (0, 0, hf.width() as i32 - 1, hf.height() as i32 - 1)
            };

            let mut t = Terrain { nx: ((px1 - px0) / stride + 1).max(2), nz: ((pz1 - pz0) / stride + 1).max(2), ..Default::default() };
            let k = (p.mpp_z() / p.mpp_x()) as f32;
            t.step = (p.mpp_x() * f64::from(stride)) as f32;
            t.origin_x = (f64::from(px0) * p.mpp_x() - p.border_m()) as f32;
            t.origin_z = (f64::from(pz0) * p.mpp_z() - p.border_m()) as f32 / k;
            let (nx, unit) = (t.nx, p.unit_scale);
            let mut heights = vec![0.0; t.nx as usize * t.nz as usize];
            heights.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
                for (i, h) in row.iter_mut().enumerate() {
                    let x = (px0 + i as i32 * stride).min(px1) as u32;
                    let y = (pz0 + j as i32 * stride).min(pz1) as u32;
                    *h = (hf.metres(x, y) / unit) as f32;
                }
            });

            t.heights = Arc::new(heights);
            t.lod = true;
            t.lf = self.context_lf.clone();
            t.material = self.ground_material(&t);
            if t.material.is_none() {
                t.skin = self.ground_skin();
            }

            let mut w = Float4x4::IDENTITY;
            w.m[2][2] = k;
            (w, t)
        } else {
            let t = Terrain::flat((self.extent_x().max(self.extent_z()) * 0.5) as f32, "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0);
            (translation(Float3::new((self.extent_x() * 0.5) as f32, 0.0, (self.extent_z() * 0.5) as f32)), t)
        };

        let mut t = t;
        self.terrain_version += 1;
        t.version = self.terrain_version;
        if first {
            self.terrain_entity = scene.add(self.layer, world, Payload::Terrain(t), self.source(TERRAIN));
        } else {
            scene.update_payload(self.terrain_entity, Payload::Terrain(t));
        }
    }

    fn add_item_entities(&mut self, scene: &mut RenderScene, layer: LayerId, elem: usize) {
        if !self.shown(elem) {
            return;
        }

        let pieces = if self.tscene().type_of(elem) == "VEGETATION_INSTANCE" {
            self.model_of(elem).filter(|m| m.ok).and_then(|_| self.vegetation_instance_model(elem)).map(|m| vec![m])
        } else {
            self.model_of(elem).filter(|m| m.ok).map(|m| m.pieces.clone())
        };

        if let Some(pieces) = pieces {
            let w = self.drawn_matrix(elem);

            // Trees (rigidleaf_v3 / rigidbranch_v3) read POSITION4 as (tint and wind seed, 1 - dissolve, burn): a seed per instance, nothing
            // dissolved (0 discards every leaf).
            let p4 = if self.tscene().type_of(elem) == "VEGETATION_INSTANCE" {
                let seed = ((elem as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0;
                [seed, 1.0, 0.0, 1000.0]
            } else {
                [0.0, 0.0, 0.0, self.opacity_w(elem)]
            };

            // The building's saved destruction level, unless a piece was given its own this session.
            let levels = self.model_of(elem).map_or(1, |m| m.levels);
            let saved = self.saved_destruction_level(elem, levels);
            let (mut first, mut count) = (0, 0u8);
            for (i, piece) in pieces.into_iter().enumerate() {
                let props = self.piece_props.get(&(elem, i));
                let world = props.map_or(w, |p| w * piece_local(p));
                let model = self.piece_file(&piece, props.map_or(saved, |p| if p.destruction_level < 0 { saved } else { p.destruction_level }), self.piece_levels(elem, i));
                // POSITION5.w > 0 lets the rigid shaders put snow on the instance while it snows (they gate it on g_snowing_intensity too).
                // ASSUMPTION: every placed building and prop takes snow (the engine's per-instance value was not traced).
                let p5 = [0.0, 0.0, 0.0, 1.0];
                // A building's or a mesh's look (its piece's own once edited): the opacity, and on fire the burn amount (POSITION6.x).
                let (p4, p6, burning) = if self.tscene().type_of(elem) == "VEGETATION_INSTANCE" {
                    (p4, [0.0; 4], false)
                } else {
                    let look = self.piece_props_or_building(elem, i);
                    ([p4[0], p4[1], p4[2], opacity_w(look.opacity)], burn_row(&look), look.burn)
                };

                let mi = MeshInstance { model, lod: true, p4, p5, p6, burning, ..Default::default() };
                let e = scene.add(layer, world, Payload::MeshInstance(mi), self.source(item_of(elem)));
                if props.is_some_and(|p| !p.visible) && let Some(en) = scene.find_mut(e) {
                    en.visible = false;
                }

                // The pieces of a building get consecutive ids.
                if count == 0 {
                    first = e;
                }

                count = count.wrapping_add(1);
            }

            self.first_entity[elem] = first;
            self.entity_count[elem] = count;
            if !self.selectable(elem) {
                for k in 0..u64::from(count) {
                    if let Some(en) = scene.find_mut(first + k as EntityId) {
                        en.locked = true;
                    }
                }
            }
        }

        // A projected decal paints the ground (and anything else) inside its box; its key is the decal's model.
        if self.tscene().type_of(elem) == "ProjectedDecalInstance"
            && let Some(model) = self.tscene().doc().get(elem, "key").filter(|k| !k.is_empty())
        {
            let tiled = self.tscene().doc().get(elem, "tiled").is_some_and(|v| v == "true");
            let parallax = self.tscene().doc().get(elem, "parallax_scale").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0);
            let decal = Decal { model: model.replace('\\', "/"), tiled, opacity: 1.0, parallax };
            let e = scene.add(layer, self.world_matrix(elem), Payload::Decal(decal), self.source(item_of(elem)));
            self.first_entity[elem] = e;
            self.entity_count[elem] = 1;
            if !self.selectable(elem) && let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }
        }

        // A point light (point_light).
        if self.tscene().type_of(elem) == "POINT_LIGHT_INSTANCE" {
            let light = self.point_light(elem);
            let e = scene.add(layer, self.world_matrix(elem), Payload::PointLight(light), self.source(item_of(elem)));
            self.first_entity[elem] = e;
            self.entity_count[elem] = 1;
            if !self.selectable(elem) && let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }
        }

        // A placed effect (autoplay="false" ones wait for a script: not drawn; out of the look's season: not drawn; BOB writes the seasons as a
        // mask into the markers, +0x60). The scale is in metres as BOB writes it (tile scale / unit).
        if let Some(vfx) = self.vfx_emitter(elem) {
            let e = scene.add(layer, self.world_matrix(elem), Payload::VfxEmitter(vfx), self.source(item_of(elem)));
            self.first_entity[elem] = e;
            self.entity_count[elem] = 1;
            if !self.selectable(elem) && let Some(en) = scene.find_mut(e) {
                en.locked = true;
            }
        }

        if self.display.show_polygons && self.is_polygon(elem) {
            self.rebuild_polygon(scene, layer, elem);
        }
    }

    /// The phase the game gives this light: the tile's lights in document order (hidden ones too) are the markers file's order (BOB's).
    fn light_phase(&self, elem: usize) -> f32 {
        light_phase(self.type_ordinal.get(elem).copied().unwrap_or(0))
    }

    fn rebuild_polygon(&mut self, scene: &mut RenderScene, layer: LayerId, elem: usize) {
        let mut o = Overlay { pickable: false, ..Default::default() };
        let st = if self.idx.valid { self.idx.style[self.idx.type_[elem] as usize] } else { style_of(self.tscene().type_of(elem)) };
        let spline = wce_tile::splines::is_spline(self.tscene().type_of(elem));
        let closed = if spline {
            self.tscene().doc().get(elem, "closed").is_some_and(|v| v == "1")
        } else {
            self.tscene().doc().get(elem, "polygon_closed").is_none_or(|v| v != "0")
        };

        let points = if spline { self.spline_world_curve(elem) } else { self.polygon_world_points(elem) };
        let lifted = points.into_iter().map(|p| Float3::new(p.x, p.y + 0.3, p.z)).collect::<Vec<_>>();
        let closed = closed && !spline;
        o.polygon_outline(&lifted, closed, st.color);
        if self.is_wall(elem) {
            self.wall_overlay(elem, &mut o);
        }
        match self.outline_entity.get(&elem).copied().filter(|&e| scene.find(e).is_some()) {
            Some(e) => scene.update_payload(e, Payload::Overlay(o)),
            None => {
                let e = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(o), self.source(item_of(elem)));
                self.outline_entity.insert(elem, e);
            }
        }
    }

    fn rebuild_batch(&mut self, scene: &mut RenderScene, type_: usize) {
        wce_prof!("tile.rebuild_batch");
        if type_ >= self.batch_entity.len() || self.batch_entity[type_] == 0 {
            return;
        }

        let mut o = Overlay { pickable: false, ..Default::default() };
        let name = &self.idx.type_names[type_];
        let st = self.idx.style[type_];
        if !self.display.hidden_types.contains(name) {
            let lim = self.lod_distance(name);
            let lim2 = lim * lim;
            let eye = self.lod_eye;
            let cull = self.display.lod && eye.x < 1e29;
            let mk = &self.markers;
            let (b, e) = (mk.group_begin[type_] as usize, mk.group_begin[type_ + 1] as usize);
            let near = |i: usize| {
                let (dx, dy, dz) = (mk.x[i] - eye.x, mk.y[i] - eye.y, mk.z[i] - eye.z);
                dx * dx + dy * dy + dz * dz <= lim2
            };

            let visible = if cull { (b..e).filter(|&i| near(i)).count() } else { e - b };
            o.lines.reserve(visible * if st.cross { 6 } else { 24 });
            let h = st.size * 0.5;
            for i in b..e {
                let (px, py, pz) = (mk.x[i], mk.y[i], mk.z[i]);
                if cull && !near(i) {
                    continue;
                }

                if st.cross {
                    o.line(Float3::new(px - h, py + 0.4, pz), Float3::new(px + h, py + 0.4, pz), st.color);
                    o.line(Float3::new(px, py + 0.4, pz - h), Float3::new(px, py + 0.4, pz + h), st.color);
                    o.line(Float3::new(px, py, pz), Float3::new(px, py + st.size, pz), st.color);
                } else {
                    o.box_wire(Float3::new(px - h, py - h, pz - h), Float3::new(px + h, py + h, pz + h), st.color);
                }
            }
        }

        scene.update_payload(self.batch_entity[type_], Payload::Overlay(o));
    }

    /// Puts the tile into `layer`. `keep_terrain`: the heights did not change (a structural edit of the instances): the terrain entity stays,
    /// so its patches are not built again.
    pub(crate) fn publish_items(&mut self, scene: &mut RenderScene, layer: LayerId, keep_terrain: bool) {
        wce_prof!("tile.publish");
        let keep_terrain = keep_terrain && self.scene_key == scene_key(scene) && self.terrain_entity != 0 && scene.find(self.terrain_entity).is_some();
        if keep_terrain {
            scene.clear_layer_except(layer, self.terrain_entity);
        } else {
            scene.clear_layer(layer);
        }

        self.scene_key = scene_key(scene);
        self.layer = layer;

        // Cleared with the layer; rebuilt by the next update_view.
        self.water_entities.clear();
        self.reset_columns();
        self.reset_collapses();
        self.water_dirty = true;
        self.links_entity = 0;
        self.links_dirty = true;
        self.nogo_entity = 0;
        self.nogo_dirty = true;
        let n = self.idx.type_.len();
        self.first_entity = vec![0; n];
        self.entity_count = vec![0; n];
        self.outline_entity.clear();
        self.wall_preview_cleared();
        self.last_highlight = None;
        let pieces = self.instances.iter().filter_map(|&e| self.model_of(e).filter(|m| m.ok)).map(|m| m.pieces.len()).sum::<usize>();
        scene.reserve(pieces + self.instances.len() / 16 + 64);
        if !keep_terrain {
            self.update_terrain(scene, true);
        }

        for e in self.instances.clone() {
            self.add_item_entities(scene, layer, e);
        }

        self.update_list_trees(scene, layer, true);
        self.update_grass(scene, layer, true);

        let types = self.idx.type_names.len();
        self.batch_entity = vec![0; types];
        for t in 0..types {
            if self.markers.group_begin[t + 1] != self.markers.group_begin[t] {
                self.batch_entity[t] = scene.add(layer, Float4x4::IDENTITY, empty_overlay(), self.source(TERRAIN));
            }
        }

        self.dirty_type = vec![0; types];
        let h = Overlay { depth_test: false, pickable: false, ..Default::default() };
        self.overlay_entity = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(h), self.source(TERRAIN));
        let mask = Overlay { pickable: false, selection_mask: true, ..Default::default() };
        self.mask_entity = scene.add(layer, Float4x4::IDENTITY, Payload::Overlay(mask), self.source(TERRAIN));
        self.last_mask = None;
        self.meshed_for = (vec![], u64::MAX);
        self.markers_dirty = true;
        self.hidden_signature = u64::MAX;
        self.lod_eye = Float3::new(1e30, 1e30, 1e30);
    }

    /// A point light, as the game builds it from point_light_marker.markers (fn 0x10407e90): radius, colour, colour_scale, colour_min,
    /// the animation and its speeds; the falloff is always smooth there (falloff_type is not read) and the phase comes from the game's
    /// random generator, not random_offset.
    fn point_light(&self, elem: usize) -> PointLight {
        point_light_of(self.tscene().doc(), elem, self.light_phase(elem))
    }
}

/// POSITION6 of a piece: x the burn amount, which the fire shaders char the colour by (TEd hands `burn_amount` to the renderer, the
/// engine puts it there; in a battle the engine ramps it up while the building burns). The plain shaders do not read x.
fn burn_row(p: &PieceProps) -> [f32; 4] {
    [p.burn_amount.clamp(0.0, 1.0), 0.0, 0.0, 0.0]
}

/// `set_highlight`'s hot point that means a light's radius handle (it is being dragged).
pub const LIGHT_RADIUS_HANDLE: usize = usize::MAX;

/// `set_highlight`'s hot point that means a civilian shelter's target handle (it is being dragged).
pub const SHELTER_TARGET_HANDLE: usize = usize::MAX - 1;

/// POSITION4.w of an opacity (`Tile::opacity_w`).
fn opacity_w(opacity: f32) -> f32 {
    (opacity.clamp(0.0, 1.0) * 1000.0).round()
}

/// A POINT_LIGHT_INSTANCE's light from its attributes (`phase`: its animation's, see `light_phase`); the preview draws one too.
pub(crate) fn point_light_of(doc: &wce_formats::xml_doc::XmlDoc, elem: usize, phase: f32) -> PointLight {
    let num = |k: &str, d: f32| doc.get(elem, k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
    let c = doc.get(elem, "colour").and_then(|v| wce_tile::properties::parse_colour(&v)).unwrap_or([1.0; 4]);
    let speed = doc.get(elem, "animation_speed_scale").and_then(|v| wce_tile::properties::parse_vec2(&v)).unwrap_or([0.0; 2]);
    let animation = match doc.get(elem, "animation_type").as_deref() {
        Some("LAT_RADIUS_SIN") => 1,
        Some("LAT_RADIUS_SIN_SIN") => 2,
        _ => 0,
    };

    PointLight {
        colour: [c[0], c[1], c[2]],
        colour_scale: num("colour_scale", 1.0),
        colour_min: num("colour_min", 0.0),
        radius: num("radius", 1.0),
        smooth: true,
        animation,
        speed,
        phase,
    }
}

impl Tile {
    /// A VFXInstance's effect, when it plays (an effect is set, autoplay is on and it is in the terrain look's season).
    fn vfx_emitter(&self, elem: usize) -> Option<VfxEmitter> {
        let doc = self.tscene().doc();
        if self.tscene().type_of(elem) != "VFXInstance" || doc.get(elem, "autoplay").is_some_and(|v| v == "false") || !self.in_season(elem) {
            return None;
        }

        let effect = doc.get(elem, "vfx").filter(|v| !v.is_empty())?;
        let scale = doc.get(elem, "scale").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(1.0) / self.unit();
        Some(VfxEmitter { effect, scale: scale as f32, mesh: None })
    }

    /// A light's or an effect's entity follows its edited attributes (an effect that stops or starts playing is removed or added).
    fn refresh_payload(&mut self, scene: &mut RenderScene, elem: usize) {
        let first = self.first_entity.get(elem).copied().unwrap_or(0);
        match self.tscene().type_of(elem) {
            "POINT_LIGHT_INSTANCE" if first != 0 => scene.update_payload(first, Payload::PointLight(self.point_light(elem))),
            "VFXInstance" => match (first != 0, self.vfx_emitter(elem)) {
                (true, Some(vfx)) => scene.update_payload(first, Payload::VfxEmitter(vfx)),
                (true, None) => {
                    scene.remove(first);
                    self.first_entity[elem] = 0;
                    self.entity_count[elem] = 0;
                }
                (false, Some(vfx)) if self.shown(elem) => {
                    let e = scene.add(self.layer, self.world_matrix(elem), Payload::VfxEmitter(vfx), self.source(item_of(elem)));
                    self.first_entity[elem] = e;
                    self.entity_count[elem] = 1;
                    if !self.selectable(elem) && let Some(en) = scene.find_mut(e) {
                        en.locked = true;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// POSITION4.w of a building's or mesh's pieces: its `opacity` as the engine packs it (TEd hands the opacity to the renderer, which
    /// writes round(opacity * 1000) there, the opacity clamped to 0..1; 1000 is opaque). The alpha-tested G-buffer shaders multiply the
    /// diffuse alpha by floor(w) * 0.001 before the test. TEd files a building below 1 apart (render flag 0x2000), for the blended forward
    /// shaders (`ps30_alpha_blend_40` multiplies the alpha the same way); the renderer has no such pass yet, so opaque materials show no fade.
    fn opacity_w(&self, elem: usize) -> f32 {
        opacity_w(self.tscene().doc().get(elem, "opacity").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0))
    }

    /// The entities of an element's model pieces (consecutive ids; empty when it has none on the scene).
    pub fn piece_entities(&self, elem: usize) -> std::ops::Range<EntityId> {
        let first = self.first_entity.get(elem).copied().unwrap_or(0);
        let n = if first == 0 { 0 } else { u64::from(self.entity_count[elem]) };
        first..first + n as EntityId
    }

    pub(crate) fn refresh_item(&mut self, scene: &mut RenderScene, elem: usize) {
        if !self.is_instance(elem) {
            return;
        }

        self.index_edited(elem);
        self.edits_seen = self.edits_seen.wrapping_add(1);
        self.refresh_payload(scene, elem);
        if self.first_entity.get(elem).is_some_and(|&f| f != 0) {
            let w = self.drawn_matrix(elem);
            for k in 0..u64::from(self.entity_count[elem]) {
                let world = self.piece_props.get(&(elem, k as usize)).map_or(w, |p| w * piece_local(p));
                scene.update_world(self.first_entity[elem] + k as EntityId, world);
            }
        }

        if let Some(s) = self.markers.slot.get(elem).copied().flatten() {
            let p = self.world_position(elem);
            let s = s as usize;
            self.markers.x[s] = p.x;
            self.markers.y[s] = p.y;
            self.markers.z[s] = p.z;
            self.dirty_type[self.idx.type_[elem] as usize] = 1;
        }

        if self.is_polygon(elem) && self.display.show_polygons && self.shown(elem) {
            self.rebuild_polygon(scene, self.layer, elem);
        }

        if self.tscene().type_of(elem) == "SplineRiverInstance" {
            self.water_dirty = true;
        }

        // A moved building can be what a wall's end is attached to.
        if self.tscene().type_of(elem) == "BUILDING_INSTANCE" && self.has_walls() {
            self.polygons_dirty = true;
        }

        if self.is_wall(elem) {
            self.wall_changed(elem);
        }
    }

    /// The pieces whose properties were edited: their entities follow (the position, the visibility, the destruction level).
    /// The `destruction_level` the tile stores for a building (0, intact, when it has one level or none is written).
    /// The file of a piece at a level: `piece_model`'s, or, for a working animation's piece whose level has no animated file (the ram's
    /// broken body), the plain file of that level.
    pub(crate) fn piece_file(&self, base: &str, level: i64, levels: u32) -> String {
        let model = piece_model(base, level, levels);
        if model == base || self.files.as_deref().is_none_or(|f| f.exists(&model)) {
            return model;
        }

        super::index::plain_piece(&model)
    }

    pub(crate) fn saved_destruction_level(&self, elem: usize, levels: u32) -> i64 {
        if levels <= 1 {
            return 0;
        }

        self.tscene().doc().get(elem, "destruction_level").and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0)
    }

    /// How many destruction levels piece `k` of a building has (its own files; the building's when not known).
    fn piece_levels(&self, elem: usize, k: usize) -> u32 {
        self.model_of(elem).map_or(1, |m| m.piece_levels.get(k).copied().unwrap_or(m.levels))
    }

    pub(crate) fn flush_pieces(&mut self, scene: &mut RenderScene) {
        if self.piece_pending.is_empty() || self.scene_key != scene_key(scene) {
            return;
        }

        self.columns.dirty = true;
        for (elem, k) in std::mem::take(&mut self.piece_pending) {
            let Some(first) = self.first_entity.get(elem).copied().filter(|&f| f != 0) else { continue };
            if k >= usize::from(self.entity_count[elem]) {
                continue;
            }

            let p = self.piece_props_or_building(elem, k);
            let id = first + k as EntityId;
            scene.update_world(id, self.world_matrix(elem) * piece_local(&p));
            let Some(base) = self.model_of(elem).and_then(|m| m.pieces.get(k)).cloned() else { continue };
            let levels = self.model_of(elem).map_or(1, |m| m.levels);
            let level = if p.destruction_level < 0 { self.saved_destruction_level(elem, levels) } else { p.destruction_level };
            let piece_levels = self.piece_levels(elem, k);
            let model = self.piece_file(&base, level, piece_levels);

            // A level going up plays the collapse (collapse.rs): the standing piece hides meanwhile. Going down is instant.
            let shown = level.clamp(0, i64::from(piece_levels.max(1)) - 1);
            let before = self.collapse_target(elem, k).or_else(|| Self::shown_level(scene, id));
            let collapses = p.visible && before.is_some_and(|b| shown > b);
            if collapses {
                self.begin_collapse(elem, k, before.unwrap_or(0), shown);
            } else {
                self.cancel_collapse(scene, elem, k);
            }

            // A collapsing piece's entity shows what the collapse says (collapse.rs).
            if self.collapse_target(elem, k).is_some() {
                scene.touch();
                continue;
            }

            if let Some(en) = scene.find_mut(id) {
                en.visible = p.visible;
                if let Payload::MeshInstance(mi) = &mut en.payload {
                    mi.model = model;
                    mi.p4[3] = opacity_w(p.opacity);
                    mi.p6 = burn_row(&p);
                    mi.burning = p.burn;
                }
            }

            scene.touch();
        }
    }

    /// The display settings changed: everything shown is rebuilt at the next update.
    pub fn display_changed(&mut self) {
        self.columns.dirty = true;
        self.markers_dirty = true;
        self.polygons_dirty = true;
        self.water_dirty = true;
        self.links_dirty = true;
        self.nogo_dirty = true;
    }

    pub fn display(&self) -> &super::TileDisplay {
        &self.display
    }

    pub fn display_mut(&mut self) -> &mut super::TileDisplay {
        &mut self.display
    }

    /// Per frame (the tool's update): rebuilds the marker batches when the camera moved enough or the display settings changed, and the water
    /// and links when they are stale.
    pub fn update_view(&mut self, scene: &mut RenderScene, eye: Float3) {
        wce_prof!("tile.update_view");
        if self.scene_key != scene_key(scene) {
            return;
        }

        let layer = self.layer;
        if self.water_dirty && !self.stroke_open {
            self.rebuild_water(scene, layer);
        }

        if self.links_dirty && !self.stroke_open {
            self.rebuild_links(scene, layer);
        }

        if self.nogo_dirty && !self.stroke_open {
            self.rebuild_auto_nogos(scene, layer);
        }

        if !self.stroke_open {
            self.update_columns(scene, layer);
        }

        // The blend groups view is baked again once a paint stroke ends.
        if !self.stroke_open && self.look.view == TerrainView::BlendGroups && self.project.blend.as_ref().is_some_and(BlendField::changed) {
            self.update_terrain(scene, false);
        }

        self.update_collapses(scene, layer);

        self.update_list_trees(scene, layer, false);
        self.update_grass(scene, layer, false);

        // A type that has markers now but had none when the tile was published (buildings whose models turned out not to load) needs its
        // overlay entity.
        let types = self.idx.type_names.len();
        if self.batch_entity.len() < types {
            self.batch_entity.resize(types, 0);
        }

        for t in 0..types {
            if t + 1 < self.markers.group_begin.len() && self.batch_entity[t] == 0 && self.markers.group_begin[t + 1] != self.markers.group_begin[t] {
                self.batch_entity[t] = scene.add(layer, Float4x4::IDENTITY, empty_overlay(), self.source(TERRAIN));
                self.markers_dirty = true;
            }
        }

        let mut all = self.markers_dirty;
        if !all && self.display.lod {
            let d = eye - self.lod_eye;

            // The markers pop at their draw distance by about that much.
            let hysteresis = 20.0f32.max(0.05 * self.display.veg_distance.min(self.display.decal_distance));
            if self.lod_eye.x > 1e29 || dot(d, d) > hysteresis * hysteresis {
                all = true;
            }
        }

        if all {
            self.lod_eye = eye;
            for t in 0..self.batch_entity.len() {
                self.rebuild_batch(scene, t);
            }

            // The model pieces of buildings follow the type visibility.
            let sig = self.display.hidden_types.iter().fold(0u64, |sig, h| sig.wrapping_mul(1_000_003).wrapping_add(h.bytes().fold(14_695_981_039_346_656_037u64, |a, c| (a ^ u64::from(c)).wrapping_mul(1_099_511_628_211))));
            if sig != self.hidden_signature {
                self.hidden_signature = sig;
                self.columns.dirty = true;
                let building = self.idx.type_names.iter().position(|t| t == "BUILDING_INSTANCE");
                let hidden = self.display.hidden_types.contains("BUILDING_INSTANCE");
                for &e in &self.instances {
                    if Some(self.idx.type_[e] as usize) != building || self.first_entity[e] == 0 {
                        continue;
                    }

                    for k in 0..u64::from(self.entity_count[e]) {
                        if let Some(en) = scene.find_mut(self.first_entity[e] + k as EntityId) {
                            en.visible = !hidden;
                        }
                    }
                }

                scene.touch();
            }

            self.markers_dirty = false;
            self.dirty_type.fill(0);
        }

        for t in 0..self.dirty_type.len() {
            if self.dirty_type[t] != 0 {
                self.rebuild_batch(scene, t);
                self.dirty_type[t] = 0;
            }
        }

        if self.polygons_dirty {
            self.polygons_dirty = false;
            for e in self.instances.clone() {
                if self.is_wall(e) {
                    self.wall_changed(e);
                }

                if !self.is_polygon(e) || !self.shown(e) {
                    continue;
                }

                if self.display.show_polygons {
                    self.rebuild_polygon(scene, layer, e);
                } else if let Some(&id) = self.outline_entity.get(&e) {
                    // Hide: an empty outline.
                    scene.update_payload(id, empty_overlay());
                }
            }
        }

        self.update_wall_previews(scene);
    }

    /// A civilian shelter's target (TEd's TERRAIN_LINE_INSTANCE: where the civilians run to) in world metres: the file's point (in the
    /// shelter's parent frame, as its position), on the ground when the shelter stands on it. `None` for anything else.
    pub fn shelter_target(&self, elem: usize) -> Option<Float3> {
        let t = self;
        if t.tscene().type_of(elem) != "CivilianShelter" {
            return None;
        }

        let v = t.tscene().doc().get(elem, "target").and_then(|v| wce_tile::ted_scene::parse_vec3(&v, "vector"))?;
        let pw = super::edits::parent_transform(t.tscene(), elem);
        let w = pw.position + pw.rotation * v;
        let u = t.unit();
        let (x, z) = ((w.x / u) as f32, (w.z / u) as f32);
        let y = if t.placement(elem) == super::Placement::Landscape { t.terrain_height(x, z) } else { (w.y / u) as f32 };
        Some(Float3::new(x, y, z))
    }

    /// The siege overlay of the selection (`set_highlight`): a selected edge's nodes, a selected node's edges. Worked out again only when
    /// the selection, the structure, an element or the terrain changed.
    fn siege_highlight(&mut self, selected: &[usize], handle_size: f32) -> Vec<OverlayVertex> {
        let key = (selected.to_vec(), self.structure_version ^ self.edits_seen.rotate_left(20) ^ self.terrain_version.rotate_left(40), handle_size.to_bits());
        if let Some((k, lines)) = &self.siege_lines
            && *k == key
        {
            return lines.clone();
        }

        let mut h = Overlay::default();
        let lift = |p: Float3| Float3::new(p.x, self.terrain_height(p.x, p.z) + 0.6, p.z);
        let nodes = if selected.iter().any(|&e| matches!(self.tscene().type_of(e), "SiegeAIEdgeInstance" | "SiegeAINodeInstance")) {
            self.siege_nodes()
        } else {
            vec![]
        };

        for &e in selected {
            // A siege edge: the nodes it joins, as the game joins them (the start's rectangle green, the end's blue), a red cross at an end
            // in no node (not for wall edges: see `SiegeEdgeEnds::walls`).
            if let Some(ends) = self.siege_edge_ends_among(e, &nodes) {
                for (k, colour) in [(0, overlay_colour(0.2, 1.0, 0.3, 1.0)), (1, overlay_colour(0.3, 0.6, 1.0, 1.0))] {
                    match ends.nodes[k].and_then(|n| nodes.iter().find(|x| x.0 == n)) {
                        Some((_, _, r)) => {
                            for i in 0..4 {
                                h.line(lift(r[i]), lift(r[(i + 1) % 4]), colour);
                            }

                            let c = (r[0] + r[1] + r[2] + r[3]) * 0.25;
                            h.line(lift(ends.at[k]), lift(c), colour);
                        }

                        None if ends.walls => {}
                        None => {
                            let (p, s) = (lift(ends.at[k]), handle_size * 2.0);
                            let red = overlay_colour(1.0, 0.15, 0.1, 1.0);
                            h.line(Float3::new(p.x - s, p.y, p.z - s), Float3::new(p.x + s, p.y, p.z + s), red);
                            h.line(Float3::new(p.x - s, p.y, p.z + s), Float3::new(p.x + s, p.y, p.z - s), red);
                        }
                    }
                }
            }

            // A siege node: the Streets of the edges joining it.
            if self.tscene().type_of(e) == "SiegeAINodeInstance" {
                let joined = overlay_colour(1.0, 0.85, 0.2, 1.0);
                for edge in self.tscene().find_all("SiegeAIEdgeInstance") {
                    if self.siege_edge_ends_among(edge, &nodes).is_some_and(|en| en.nodes.contains(&Some(e)))
                        && let Some(line) = self.siege_polygon_world(edge)
                    {
                        for w in line.windows(2) {
                            h.line(lift(w[0]), lift(w[1]), joined);
                        }
                    }
                }
            }
        }

        self.siege_lines = Some((key, h.lines.clone()));
        h.lines
    }

    /// A selected point light's radius handle: where it is (world metres, on the +x side of the light at its height) and the radius (the
    /// file's value, which the light is drawn with); `None` for anything else.
    pub fn light_radius_handle(&self, elem: usize) -> Option<(Float3, f32)> {
        if self.tscene().type_of(elem) != "POINT_LIGHT_INSTANCE" {
            return None;
        }

        let r = self.tscene().doc().get(elem, "radius").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0).max(0.0);
        let p = self.world_position(elem);
        Some((Float3::new(p.x + r, p.y, p.z), r))
    }

    /// The highlight overlay (the points of a selected polygon, with `polygon_point` hot, and the active tool's overlay) and the selection
    /// outline's mask: the shapes of the selected markers (instances without meshes of their own). The renderer outlines the rest of the
    /// selection from its meshes and overlays.
    pub fn set_highlight(&mut self, scene: &mut RenderScene, selected: &[usize], polygon_point: Option<usize>, handle_size: f32, tool_overlay: &Overlay) {
        wce_prof!("tile.set_highlight");
        self.flush_pieces(scene);
        if self.scene_key != scene_key(scene) || self.overlay_entity == 0 {
            return;
        }

        // The selected elements with meshes of their own: one pass over the scene, when the selection or the scene's structure changed.
        if self.meshed_for.0 != selected || self.meshed_for.1 != scene.structure_revision() {
            let wanted = selected.iter().map(|&e| self.source(item_of(e))).collect::<HashSet<_>>();
            self.meshed = scene.entities().iter().filter(|en| matches!(en.payload, Payload::MeshInstance(_)) && wanted.contains(&en.source)).map(|en| en.source.item).collect();
            self.meshed_for = (selected.to_vec(), scene.structure_revision());
        }

        let mut h = Overlay { depth_test: false, pickable: false, ..Default::default() };
        let mut mask = Overlay { pickable: false, selection_mask: true, ..Default::default() };
        let (hot, white) = (overlay_colour(1.0, 0.9, 0.1, 1.0), overlay_colour(1.0, 1.0, 1.0, 1.0));
        for &e in selected {
            if !self.is_instance(e) {
                continue;
            }

            // A marker as it is drawn (see `rebuild_marker_batch`): a box around the point, or a cross standing on it. A polygon's outline is
            // an overlay of its own, which the renderer outlines.
            if !self.is_polygon(e) && !self.meshed.contains(&item_of(e)) {
                let p = self.world_position(e);
                let st = if self.idx.valid { self.idx.style[self.idx.type_[e] as usize] } else { style_of(self.tscene().type_of(e)) };
                let h2 = st.size * 0.5;
                let (lo_y, hi_y) = if st.cross { (p.y, p.y + st.size) } else { (p.y - h2, p.y + h2) };
                mask.box_solid(Float3::new(p.x - h2, lo_y, p.z - h2), Float3::new(p.x + h2, hi_y, p.z + h2), white);
            }

            // A civilian shelter's run to its target: a line over the ground from colour_1 at the shelter to colour_2 at the target, and the
            // target's handle (TEd drags it over the ground).
            if let Some(to) = self.shelter_target(e) {
                let from = self.world_position(e);
                let colour = |k: &str, d: [f32; 4]| self.tscene().doc().get(e, k).and_then(|v| wce_tile::properties::parse_colour(&v)).unwrap_or(d);
                let (c1, c2) = (colour("colour_1", [0.0, 1.0, 0.0, 0.8]), colour("colour_2", [1.0, 0.0, 0.0, 0.8]));
                let landscape = self.placement(e) == super::Placement::Landscape;
                let at = |f: f32| {
                    let p = from + (to - from) * f;
                    Float3::new(p.x, if landscape { self.terrain_height(p.x, p.z) } else { p.y } + 0.3, p.z)
                };

                const STEPS: usize = 32;
                for k in 0..STEPS {
                    let (f0, f1) = (k as f32 / STEPS as f32, (k + 1) as f32 / STEPS as f32);
                    let m = (f0 + f1) * 0.5;
                    let c: [f32; 4] = std::array::from_fn(|i| c1[i] + (c2[i] - c1[i]) * m);
                    h.line(at(f0), at(f1), overlay_colour(c[0], c[1], c[2], c[3].max(0.5)));
                }

                let k = Float3::new(to.x, to.y + 0.3, to.z);
                let (col, s) = (if polygon_point == Some(SHELTER_TARGET_HANDLE) { hot } else { white }, handle_size);
                h.line(Float3::new(k.x - s, k.y, k.z), Float3::new(k.x + s, k.y, k.z), col);
                h.line(Float3::new(k.x, k.y, k.z - s), Float3::new(k.x, k.y, k.z + s), col);
                h.line(k, Float3::new(k.x, k.y + s * 1.5, k.z), col);
            }

            // A point light's radius handle, on the +x side of its reach (the renderer draws the reach of a selected light).
            if let Some((c, r)) = self.light_radius_handle(e).map(|(h, r)| (Float3::new(h.x - r, h.y, h.z), r)) {
                let reach = overlay_colour(1.0, 0.85, 0.4, 0.8);
                let k = Float3::new(c.x + r, c.y, c.z);
                let (col, s) = (if polygon_point == Some(LIGHT_RADIUS_HANDLE) { hot } else { white }, handle_size * 0.8);
                h.line(c, k, reach);
                for (a, b) in [((-s, 0.0), (0.0, -s)), ((0.0, -s), (s, 0.0)), ((s, 0.0), (0.0, s)), ((0.0, s), (-s, 0.0))] {
                    h.line(Float3::new(k.x + a.0, k.y + a.1, k.z), Float3::new(k.x + b.0, k.y + b.1, k.z), col);
                    h.line(Float3::new(k.x + a.0, k.y, k.z + a.1), Float3::new(k.x + b.0, k.y, k.z + b.1), col);
                }
            }

            if self.is_polygon(e) {
                for (i, q) in self.polygon_world_points(e).into_iter().enumerate() {
                    let (c, r) = (if Some(i) == polygon_point { hot } else { white }, handle_size);
                    h.line(Float3::new(q.x - r, q.y + 0.4, q.z), Float3::new(q.x + r, q.y + 0.4, q.z), c);
                    h.line(Float3::new(q.x, q.y + 0.4, q.z - r), Float3::new(q.x, q.y + 0.4, q.z + r), c);
                    h.line(q, Float3::new(q.x, q.y + r * 1.5, q.z), c);
                }

                // A spline's handles: a line from the point, a small diamond at the end.
                if wce_tile::splines::is_spline(self.tscene().type_of(e)) {
                    let points = self.polygon_world_points(e);
                    let handle = overlay_colour(0.4, 0.8, 1.0, 1.0);
                    for (q, (hin, hout)) in points.into_iter().zip(self.spline_world_handles(e)) {
                        for k in [hin, hout].into_iter().flatten() {
                            let (q, k) = (Float3::new(q.x, q.y + 0.4, q.z), Float3::new(k.x, q.y + 0.4, k.z));
                            let r = handle_size * 0.6;
                            h.line(q, k, handle);
                            h.line(Float3::new(k.x - r, k.y, k.z), Float3::new(k.x, k.y, k.z - r), handle);
                            h.line(Float3::new(k.x, k.y, k.z - r), Float3::new(k.x + r, k.y, k.z), handle);
                            h.line(Float3::new(k.x + r, k.y, k.z), Float3::new(k.x, k.y, k.z + r), handle);
                            h.line(Float3::new(k.x, k.y, k.z + r), Float3::new(k.x - r, k.y, k.z), handle);
                        }
                    }
                }
            }
        }

        let siege = self.siege_highlight(selected, handle_size);
        h.lines.extend(siege);
        h.lines.extend_from_slice(&tool_overlay.lines);
        h.triangles.extend_from_slice(&tool_overlay.triangles);

        // Only what changed goes to the scene; the structure revision this moves does not make the meshed elements be looked up again.
        let current = self.meshed_for.1 == scene.structure_revision();
        if self.mask_entity != 0 && !self.last_mask.as_ref().is_some_and(|l| bits_equal(&l.triangles, &mask.triangles)) {
            self.last_mask = Some(mask.clone());
            scene.update_payload(self.mask_entity, Payload::Overlay(mask));
        }

        if !self.last_highlight.as_ref().is_some_and(|l| bits_equal(&l.lines, &h.lines) && bits_equal(&l.triangles, &h.triangles)) {
            self.last_highlight = Some(h.clone());
            scene.update_payload(self.overlay_entity, Payload::Overlay(h));
        }

        if current {
            self.meshed_for.1 = scene.structure_revision();
        }
    }

    // ---- outline and descriptions

    pub(crate) fn outline(&self) -> OutlinerNode {
        let sc = self.tscene();
        fn build(t: &Tile, e: usize) -> OutlinerNode {
            let sc = t.tscene();
            let ty = sc.type_of(e);
            let name = sc.name(e);
            let label = if name.is_empty() { ty.to_owned() } else { name };
            let kids = sc.children(e);
            let count = if kids.is_empty() { String::new() } else { format!(", {}", kids.len()) };
            let mut n = OutlinerNode { label: format!("{label}  [{ty}{count}]"), source: t.source(item_of(e)), selectable: ty != "SCENE_GRAPH", children: vec![] };

            // TEd lists the meshes of a building as its children, "building piece" each.
            if ty == "BUILDING_INSTANCE" && let Some(m) = t.model_of(e).filter(|m| m.ok) {
                for i in 0..m.pieces.len().min(0xffff) {
                    n.children.push(OutlinerNode { label: "building piece".to_owned(), source: t.source(piece_item(e, i)), selectable: true, children: vec![] });
                }
            }

            for (shown, &k) in kids.iter().enumerate() {
                if shown + 1 > 300 {
                    n.children.push(OutlinerNode { label: format!("... {} more", kids.len() - 300), source: t.source(TERRAIN), selectable: false, children: vec![] });
                    break;
                }

                n.children.push(build(t, k));
            }

            n
        }

        let mut root = OutlinerNode { label: self.title(), source: self.source(TERRAIN), selectable: true, children: vec![] };
        root.children.push(OutlinerNode { label: "Terrain".to_owned(), source: self.source(TERRAIN), selectable: true, children: vec![] });
        if let Some(g) = sc.scene_graph() {
            root.children.push(build(self, g));
        }

        root
    }

    pub(crate) fn describe(&self, item: SourceRef) -> String {
        if let Some((e, k)) = piece_of(item.item) {
            return format!("building piece {k} of '{}'", self.tscene().name(e));
        }

        let Some(e) = elem_of(item.item) else { return "Terrain".to_owned() };
        if e >= self.tscene().doc().elements().len() {
            return String::new();
        }

        let n = self.tscene().name(e);
        format!("{}{}", self.tscene().type_of(e), if n.is_empty() { String::new() } else { format!("  '{n}'") })
    }

    /// The gizmo's pivot (no scale); a hidden or locked item has none.
    pub(crate) fn item_transform(&self, item: SourceRef) -> Option<Float4x4> {
        if item.doc != self.id {
            return None;
        }

        let e = elem_of(item.item)?;
        if !self.is_instance(e) || !self.shown(e) || !self.selectable(e) {
            return None;
        }

        Some(matrix_of(&world_transform(self.tscene(), e), self.unit(), None))
    }

    /// World positions of the points of a polygon.
    /// A spline's curve in the world (metres), sampled.
    pub fn spline_world_curve(&self, elem: usize) -> Vec<Float3> {
        let pts = wce_tile::splines::read(self.tscene(), elem);
        let closed = self.tscene().doc().get(elem, "closed").is_some_and(|v| v == "1");
        self.local_to_world(elem, &wce_tile::splines::curve(&pts, closed, 16))
    }

    /// A spline's handles in the world (metres): each point's `controlIn` and `controlOut`, `None` where no segment uses it.
    pub fn spline_world_handles(&self, elem: usize) -> Vec<(Option<Float3>, Option<Float3>)> {
        let pts = wce_tile::splines::read(self.tscene(), elem);
        let n = pts.len();
        let ins = self.local_to_world(elem, &pts.iter().map(|p| p.control_in).collect::<Vec<_>>());
        let outs = self.local_to_world(elem, &pts.iter().map(|p| p.control_out).collect::<Vec<_>>());
        (0..n).map(|i| ((i > 0).then(|| ins[i]), (i + 1 < n).then(|| outs[i]))).collect()
    }

    fn local_to_world(&self, elem: usize, local: &[Double3]) -> Vec<Float3> {
        let w = world_transform(self.tscene(), elem);
        let u = self.unit();
        local.iter().map(|&v| {
            let p = w.position + w.rotation * v;
            Float3::new((p.x / u) as f32, (p.y / u) as f32, (p.z / u) as f32)
        }).collect()
    }

    pub fn polygon_world_points(&self, elem: usize) -> Vec<Float3> {
        let w = world_transform(self.tscene(), elem);
        let u = self.unit();
        self.tscene().polygon_points(elem).into_iter().filter_map(|p| self.tscene().point_position(p)).map(|v| {
            let p = w.position + w.rotation * v;
            Float3::new((p.x / u) as f32, (p.y / u) as f32, (p.z / u) as f32)
        }).collect()
    }

    /// A summary of what the document shows (the debug panel).
    pub fn diagnostics(&self, scene: Option<&RenderScene>) -> String {
        let p = &self.project;
        let (w, h) = p.height.as_ref().map_or((0, 0), |h| (h.width(), h.height()));
        let mut d = format!("tile {}\n", self.title());
        d += &format!(
            "instances {}, markers {}, building models {} of {} keys\n", self.instances.len(), self.markers.elem.len(), self.models.len() - self.fallbacks.len(), self.models.len()
        );

        d += &format!("terrain {w} x {h} px, stride {}, border {} m\n", self.terrain_stride, p.border_m() as i32);
        d += &self.load_info;
        d += "\n";
        if let Some(scene) = scene.filter(|s| scene_key(s) == self.scene_key) {
            for (t, &id) in self.batch_entity.iter().enumerate() {
                if let Some(Payload::Overlay(o)) = (id != 0).then(|| scene.find(id)).flatten().map(|e| &e.payload) {
                    let count = self.markers.group_begin[t + 1] - self.markers.group_begin[t];
                    d += &format!("markers of {}: {count}, drawn as {} line vertices\n", self.idx.type_names[t], o.lines.len());
                }
            }
        }

        for k in &self.fallbacks {
            d += &format!("no model: {k}\n");
        }

        d
    }

    /// How many building keys each instance type has, for tests.
    pub fn models(&self) -> &BTreeMap<String, super::BuildingModel> {
        &self.models
    }
}

#[cfg(test)]
mod tests {
    use super::piece_model;

    #[test]
    fn light_phase_steps_the_generator() {
        let mut state = 0x1234_4321u32;
        for n in 0..2000 {
            state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            assert_eq!(super::light_phase(n), (state >> 16) as f32 * 1.525_902_2e-5, "light {n}");
        }
    }

    #[test]
    fn level_zero_is_the_intact_piece() {
        let base = "rigidmodels/buildings/house/house_piece01_destruct01.rigid_model_v2";
        assert_eq!(piece_model(base, -1, 3), base);
        assert_eq!(piece_model(base, 0, 3), base, "level 0 of 3");
        assert_eq!(piece_model(base, 1, 3), base.replace("destruct01", "destruct02"));
        assert_eq!(piece_model(base, 2, 3), base.replace("destruct01", "destruct03"));
        assert_eq!(piece_model(base, 5, 3), base.replace("destruct01", "destruct03"), "clamped to the last level");
        assert_eq!(piece_model(base, 1, 1), base, "one level: only the intact piece");
    }
}

/// The phase of the n-th light (from 0) of a map's point_light_marker.markers (fn 0x10407e90): the game's generator seeded with
/// 0x12344321, stepped once per light (state * 0x343fd + 0x269ec3), (state >> 16) * (1 / 65535) radians.
pub(crate) fn light_phase(n: u32) -> f32 {
    // n + 1 steps: the affine step's power by squaring.
    let (mut a, mut c) = (1u32, 0u32);
    let (mut ba, mut bc) = (0x343fdu32, 0x269ec3u32);
    let mut k = n + 1;
    while k != 0 {
        if k & 1 != 0 {
            (a, c) = (ba.wrapping_mul(a), ba.wrapping_mul(c).wrapping_add(bc));
        }

        (ba, bc) = (ba.wrapping_mul(ba), ba.wrapping_mul(bc).wrapping_add(bc));
        k >>= 1;
    }

    let state = a.wrapping_mul(0x1234_4321).wrapping_add(c);
    (state >> 16) as f32 * 1.525_902_2e-5
}
