//! What a tile document shows: the terrain (and its ground look), the building models, the markers, polygon outlines, water, tile links and the
//! highlight overlay, and the outline and descriptions of its items.

use std::collections::BTreeMap;
use std::sync::Arc;

use rayon::prelude::*;
use wce_editor_core::document::OutlinerNode;
use wce_formats::c_num::strtod;
use wce_foundation::math::{Double3, Float3, Float4x4, dot, translation};
use wce_foundation::{log, wce_prof};
use wce_scene::render_scene::{
    EntityId, LayerId, MeshInstance, Overlay, OverlayVertex, Payload, RenderScene, SourceRef, Terrain, TerrainMaterial, TerrainSkin, Water, WaterVertex, overlay_colour,
};
use wce_tile::ted_scene::parse_vec3;
use wce_tile::world_transform::world_transform;

use super::index::{matrix_of, style_of};
use super::{GroundInfo, LinkCell, Placement, TERRAIN, Tile, elem_of, item_of, scene_key};
use crate::terrain_bake::{BakeRequest, TerrainLook, TerrainView, bake_terrain_colour, bake_terrain_map_view};
use crate::terrain_material::{MaterialRequest, build_terrain_material, terrain_normal_image};

#[derive(Clone, Copy)]
struct RiverPoint {
    /// World metres; y = the spline's own height for freeform, terrain offset for landscape (resolved by the caller).
    x: f64,
    z: f64,
    y: f64,

    /// terrain_offset.
    offset: f64,
    width: f64,
    foam: f64,
    fade: f64,
    flow: f64,

    /// Control handles as world positions (x, z metres, y local metres).
    cin: [f64; 3],
    cout: [f64; 3],
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

fn empty_overlay() -> Payload {
    Payload::Overlay(Overlay { pickable: false, ..Default::default() })
}

impl Tile {

    fn source(&self, item: u64) -> SourceRef {
        SourceRef { doc: self.id, item }
    }

    // ---- ground look

    /// The ground's look: a view setting, not an edit (not on the undo stack and not saved).
    pub fn terrain_look(&self) -> &TerrainLook {
        &self.look
    }

    pub fn set_terrain_look(&mut self, scene: &mut RenderScene, look: TerrainLook) {
        if look == self.look {
            return;
        }

        self.look = look;
        if self.scene_key == scene_key(scene) && self.terrain_entity != 0 {
            self.update_terrain(scene, false);
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

        if self.skin.is_some() && self.skin_look == self.look {
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

        let map_view = matches!(self.look.view, TerrainView::GroundTypes | TerrainView::Protection);
        let baked = if map_view { bake_terrain_map_view(&rq) } else { bake_terrain_colour(&rq, self.climates(), files.as_ref()) };
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
        self.skin_id += 1;
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
            self.material_look = self.look.clone();
            if !self.has_blend_area() {
                self.terrain_note = "plain tiled texture (the tile has no database entry or no height map: no blend map area)".to_owned();
                return None;
            }

            let Some(files) = self.files.clone() else {
                self.terrain_note = "plain tiled texture (no game files)".to_owned();
                return None;
            };

            let p = &self.project;
            let h = p.height.as_ref().expect("a height map");
            let (x0, z0, x1, z1) = self.blend_area();
            self.material_id += 1;
            let rq = MaterialRequest {
                folder: p.dir.clone(), groups: p.entry.as_ref().expect("an entry").texture_group(), x0, z0, x1, z1, y0: h.min_height() / p.unit_scale, y1: (h.min_height() + h.scale()) / p.unit_scale,
                look: self.look.clone(), id: self.material_id,
            };

            match build_terrain_material(&rq, self.climates(), files.as_ref()) {
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

        self.ground_info = self.ground_info_base.clone();
        let mut m = (**self.material_base.as_ref()?).clone();
        let k = self.project.mpp_z() / self.project.mpp_x();
        m.normal = terrain_normal_image(&t.heights, t.nx, t.nz, f64::from(t.step), f64::from(t.step) * k, m.blend.w, m.blend.h);
        self.normal_id += 1;
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
    /// all local to the instance in scene units); the ribbon follows it with the point's width. A landscape spline lies terrain_offset above the
    /// terrain under each sample (measured on osgiliath: the bed is carved, the offset is the water's depth over it), a freeform one at the height
    /// it stores. The sea plane: a tile of a sea tile set whose terrain goes below 0 gets a plane at y = 0 over the bordered tile; the depth test
    /// cuts it where the ground is higher.
    fn rebuild_water(&mut self, scene: &mut RenderScene, layer: LayerId) {
        for &e in &self.water_entities {
            scene.remove(e);
        }

        self.water_entities.clear();
        self.water_dirty = false;
        self.water_note.clear();
        if !self.display.show_water {
            self.water_note = "water hidden".to_owned();
            return;
        }

        let u = self.unit();
        let (mut rivers, mut ribbons) = (0, 0);
        let mut new_water = vec![];
        let sc = self.tscene();
        let els = sc.doc().elements();
        for e in 0..els.len() {
            if els[e].name != "SplineRiverInstance" {
                continue;
            }

            let w = world_transform(sc, e);
            let landscape = self.placement_of_attr(e) == Placement::Landscape;
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
                        offset: num("terrain_offset", 0.0), width: num("width", 1.0), foam: num("foam_amount", 0.1), fade: num("alpha_fade", 1.0), flow: num("flow_speed", 0.5),
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
                    let off = a.offset + (b.offset - a.offset) * t;
                    let y = if landscape { f64::from(self.terrain_height(x as f32, z as f32)) + off / u } else { bez(a.y, a.cout[1], b.cin[1], b.y, t) };
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

        // The sea / lake plane. Which tiles have open water is a matter of the tile set: a river tile's terrain also goes below 0 (the ford's
        // heights are all <= 0) and has no sea.
        let p = &self.project;
        let set = p.entry.as_ref().map(|e| e.tile_set()).unwrap_or_default();
        let sea_set = matches!(set.as_str(), "sea" | "sea_coast" | "sea_coast_landing" | "lakes" | "river_mouth");
        let sea = sea_set && p.has_height() && p.info.found && p.info.min_height < -0.5;
        if sea {
            let b = p.border_m() as f32;
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
            format!("{rivers} river spline(s) ({ribbons} samples){}", if sea { ", a sea / lake plane at 0 m" } else { "" })
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

    // ---- publishing

    pub(crate) fn update_terrain(&mut self, scene: &mut RenderScene, first: bool) {
        // Landscape rivers lie on the terrain; the link cells are draped over it.
        self.water_dirty = true;
        self.links_dirty = true;
        let (world, t) = if let Some(hf) = &self.project.height {
            // The renderer cuts the grid into patches with LODs, the full resolution costs nothing far away.
            self.terrain_stride = 1;
            let stride = self.terrain_stride;
            let p = &self.project;
            let mut t = Terrain { nx: ((hf.width() as i32 - 1) / stride + 1).max(2), nz: ((hf.height() as i32 - 1) / stride + 1).max(2), ..Default::default() };
            let k = (p.mpp_z() / p.mpp_x()) as f32;
            t.step = (p.mpp_x() * f64::from(stride)) as f32;
            t.origin_x = -p.border_m() as f32;
            t.origin_z = -p.border_m() as f32 / k;
            let (nx, unit) = (t.nx, p.unit_scale);
            t.heights = vec![0.0; t.nx as usize * t.nz as usize];
            t.heights.par_chunks_mut(nx as usize).enumerate().for_each(|(j, row)| {
                for (i, h) in row.iter_mut().enumerate() {
                    let x = (i as i32 * stride).min(hf.width() as i32 - 1) as u32;
                    let y = (j as i32 * stride).min(hf.height() as i32 - 1) as u32;
                    *h = (hf.metres(x, y) / unit) as f32;
                }
            });

            t.lod = true;
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

        if let Some(pieces) = self.model_of(elem).filter(|m| m.ok).map(|m| m.pieces.clone()) {
            let w = self.world_matrix(elem);
            let (mut first, mut count) = (0, 0u8);
            for piece in pieces {
                let e = scene.add(layer, w, Payload::MeshInstance(MeshInstance { model: piece, lod: true, ..Default::default() }), self.source(item_of(elem)));

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

        if self.display.show_polygons && self.is_polygon(elem) {
            self.rebuild_polygon(scene, layer, elem);
        }
    }

    fn rebuild_polygon(&mut self, scene: &mut RenderScene, layer: LayerId, elem: usize) {
        let mut o = Overlay { pickable: false, ..Default::default() };
        let st = if self.idx.valid { self.idx.style[self.idx.type_[elem] as usize] } else { style_of(self.tscene().type_of(elem)) };
        let closed = self.tscene().doc().get(elem, "polygon_closed").is_none_or(|v| v != "0");
        let lifted = self.polygon_world_points(elem).into_iter().map(|p| Float3::new(p.x, p.y + 0.3, p.z)).collect::<Vec<_>>();
        o.polygon_outline(&lifted, closed, st.color);
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
        self.water_dirty = true;
        self.links_entity = 0;
        self.links_dirty = true;
        let n = self.idx.type_.len();
        self.first_entity = vec![0; n];
        self.entity_count = vec![0; n];
        self.outline_entity.clear();
        self.last_highlight = None;
        let pieces = self.instances.iter().filter_map(|&e| self.model_of(e).filter(|m| m.ok)).map(|m| m.pieces.len()).sum::<usize>();
        scene.reserve(pieces + self.instances.len() / 16 + 64);
        if !keep_terrain {
            self.update_terrain(scene, true);
        }

        for e in self.instances.clone() {
            self.add_item_entities(scene, layer, e);
        }

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
        self.markers_dirty = true;
        self.hidden_signature = u64::MAX;
        self.lod_eye = Float3::new(1e30, 1e30, 1e30);
    }

    pub(crate) fn refresh_item(&mut self, scene: &mut RenderScene, elem: usize) {
        if !self.is_instance(elem) {
            return;
        }

        self.index_edited(elem);
        if self.first_entity.get(elem).is_some_and(|&f| f != 0) {
            let w = self.world_matrix(elem);
            for k in 0..u64::from(self.entity_count[elem]) {
                scene.update_world(self.first_entity[elem] + k as EntityId, w);
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
    }

    /// The display settings changed: everything shown is rebuilt at the next update.
    pub fn display_changed(&mut self) {
        self.markers_dirty = true;
        self.polygons_dirty = true;
        self.water_dirty = true;
        self.links_dirty = true;
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
    }

    /// The highlight overlay: the selected instances (a box, the axes, the points of a polygon with `polygon_point` hot) and the active tool's
    /// overlay.
    pub fn set_highlight(&mut self, scene: &mut RenderScene, selected: &[usize], polygon_point: Option<usize>, handle_size: f32, tool_overlay: &Overlay) {
        wce_prof!("tile.set_highlight");
        if self.scene_key != scene_key(scene) || self.overlay_entity == 0 {
            return;
        }

        let mut h = Overlay { depth_test: false, pickable: false, ..Default::default() };
        let (yellow, hot, white) = (overlay_colour(1.0, 1.0, 0.2, 1.0), overlay_colour(1.0, 0.9, 0.1, 1.0), overlay_colour(1.0, 1.0, 1.0, 1.0));
        for &e in selected {
            if !self.is_instance(e) {
                continue;
            }

            let p = self.world_position(e);
            let r = self.marker_radius(e) / 0.75 * 0.5 * 1.2;
            h.box_wire(Float3::new(p.x - r, p.y, p.z - r), Float3::new(p.x + r, p.y + 2.0 * r, p.z + r), yellow);
            let a = 8.0f32.max(r * 1.5);
            h.line(p, Float3::new(p.x + a, p.y, p.z), overlay_colour(1.0, 0.2, 0.2, 1.0));
            h.line(p, Float3::new(p.x, p.y + a, p.z), overlay_colour(0.2, 1.0, 0.2, 1.0));
            h.line(p, Float3::new(p.x, p.y, p.z + a), overlay_colour(0.3, 0.5, 1.0, 1.0));
            if self.is_polygon(e) {
                for (i, q) in self.polygon_world_points(e).into_iter().enumerate() {
                    let (c, r) = (if Some(i) == polygon_point { hot } else { white }, handle_size);
                    h.line(Float3::new(q.x - r, q.y + 0.4, q.z), Float3::new(q.x + r, q.y + 0.4, q.z), c);
                    h.line(Float3::new(q.x, q.y + 0.4, q.z - r), Float3::new(q.x, q.y + 0.4, q.z + r), c);
                    h.line(q, Float3::new(q.x, q.y + r * 1.5, q.z), c);
                }
            }
        }

        h.lines.extend_from_slice(&tool_overlay.lines);
        h.triangles.extend_from_slice(&tool_overlay.triangles);
        if self.last_highlight.as_ref().is_some_and(|l| bits_equal(&l.lines, &h.lines) && bits_equal(&l.triangles, &h.triangles)) {
            return;
        }

        self.last_highlight = Some(h.clone());
        scene.update_payload(self.overlay_entity, Payload::Overlay(h));
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
