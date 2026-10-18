//! The open tile in the context of a battle map (`wce_tile::battle_map::context`): the tiles around it at full detail (their terrain with the
//! game's terrain shaders, buildings, trees, decals, lights, effects, rivers) and the map's low-frequency ground beyond, in the open tile's own
//! frame. Read-only: nothing of it can be picked or edited.
//!
//! Each distinct neighbour tile is opened once and published into a scratch scene; every placement of it then copies those entities through
//! its transform (its own frame -> the open tile's). Terrain can't take a rotation in the renderer (its height lookup is by world position),
//! so a neighbour's terrain is resampled into a grid of the open frame, its blend / index maps likewise and its normal map rebuilt from the
//! resampled heights.
//!
//! Heights (ASSUMPTION, see the module docs of `context` in wce_tile for the game's formula): the game puts every tile on the map's LF
//! (`LF * 3000 - 20 + the tile's height`). The open tile is edited at its own heights, so the context is drawn relative to it: a neighbour
//! point at `its height + (LF(point) - mean LF under the open tile)`, objects lifted by the LF difference at their position. Where the LF is
//! not flat under the open tile the seams are off by up to half its range (the report says how much).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use wce_foundation::math::{Float3, Float4x4};
use wce_scene::render_scene::{Kind, Payload, RenderScene, SharedHeights, Terrain, TerrainImage, TerrainMaterial};
use wce_tile::battle_map::context::{Affine2, CELL, Context, PlacedMap, Spot};
use wce_tile::tile_project::TileProject;
use wce_vfs::files::Files;

use super::{MATERIAL_IDS, NORMAL_IDS, TileDocument, next_id};
use crate::terrain_bake::TerrainLook;
use crate::terrain_material::terrain_normal_image;

/// What to build.
#[derive(Clone)]
pub struct ContextRequest {
    pub map: Arc<PlacedMap>,
    pub spot: Spot,

    /// The open tile's size in cells and its scene units per metre.
    pub cells: (i32, i32),
    pub unit: f64,

    /// Cells of neighbours around the open tile, and cells of LF ground beyond them.
    pub radius: i32,
    pub far: i32,

    /// The kit's raw_data folder (the neighbours' tile folders).
    pub raw_root: PathBuf,
    pub look: TerrainLook,
    pub trees: bool,
}

/// The entities of a context, in the open tile's frame, and what happened.
#[derive(Default)]
pub struct ContextScene {
    pub entities: Vec<(Float4x4, Payload)>,
    pub context: Context,
    pub tiles_loaded: usize,
    pub missing: Vec<String>,
    pub report: String,
}

/// A tile location and variation.
type TileKey = (String, String);

/// One neighbour tile, published once.
struct Template {
    /// Its extent (metres of its own frame) and the scene units per metre.
    extent: (f64, f64),

    /// Heights in scene units on a regular grid of its own frame: (x0, z0, step, nx, nz, heights).
    grid: (f64, f64, f64, usize, usize, Vec<f32>),

    /// Its ground material (the blend / index maps over the material's rectangle).
    material: Option<Arc<TerrainMaterial>>,

    /// Everything but the terrain, the markers and the grass.
    entities: Vec<(Float4x4, Payload)>,
}

/// The tile folder of a placement: `<raw_data>/<location>/<variation>`.
fn tile_dir(raw_root: &Path, location: &str, variation: &str) -> PathBuf {
    let mut p = raw_root.to_path_buf();
    for part in location.split(['\\', '/']).filter(|s| !s.is_empty()) {
        p.push(part);
    }

    p.join(variation)
}

fn load_template(dir: &Path, rq: &ContextRequest, files: Option<Arc<dyn Files>>) -> Result<Template, String> {
    let project = TileProject::open(dir)?;
    let mut doc = TileDocument::new(project, files);
    let t = doc.tile_mut();
    t.look = rq.look.clone();
    t.display.show_grass = false;
    t.display.show_links = false;
    t.display.show_polygons = false;
    t.display.show_border = false;
    t.display.lod = false;
    if !rq.trees {
        t.display.tree_list.clear();
    }

    let mut scratch = RenderScene::default();
    let layer = scratch.add_layer(t.id, "context");
    t.publish_items(&mut scratch, layer, false);
    t.update_view(&mut scratch, Float3::new(0.0, 0.0, 0.0));

    let material = scratch.find(t.terrain_entity).and_then(|e| match &e.payload {
        Payload::Terrain(tr) => tr.material.clone(),
        _ => None,
    });

    let entities = scratch
        .entities()
        .iter()
        .filter(|e| e.id != t.terrain_entity && !matches!(e.kind(), Kind::Overlay | Kind::Grass | Kind::Terrain | Kind::CameraRig))
        .map(|e| (e.world, e.payload.clone()))
        .collect();

    // The heights over the tile without its border, every 2 m at most.
    let (ex, ez) = (t.extent_x(), t.extent_z());
    let step = NEIGHBOUR_STEP;
    let (nx, nz) = ((ex / step).round() as usize + 1, (ez / step).round() as usize + 1);
    let (project, unit) = (&t.project, t.unit());
    let mut heights = vec![0.0f32; nx * nz];
    heights.par_chunks_mut(nx).enumerate().for_each(|(j, row)| {
        for (i, h) in row.iter_mut().enumerate() {
            *h = (project.height_at(i as f64 * step, j as f64 * step) / unit) as f32;
        }
    });

    Ok(Template { extent: (ex, ez), grid: (0.0, 0.0, step, nx, nz, heights), material, entities })
}

/// A 4 x 4 of the frame transform: x' = a x + b z + c, z' = d x + e z + f, y + dy.
fn matrix(a: &Affine2, dy: f32) -> Float4x4 {
    let m = &a.m;
    let mut r = Float4x4::IDENTITY;
    r.m[0] = [m[0][0] as f32, 0.0, m[0][1] as f32, m[0][2] as f32];
    r.m[1][3] = dy;
    r.m[2] = [m[1][0] as f32, 0.0, m[1][1] as f32, m[1][2] as f32];
    r
}

/// The bounding rectangle of a frame rectangle's image: x0, z0, x1, z1.
fn bounds(a: &Affine2, x0: f64, z0: f64, x1: f64, z1: f64) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (x, z) in [(x0, z0), (x1, z0), (x0, z1), (x1, z1)] {
        let (px, pz) = a.apply(x, z);
        b = [b[0].min(px), b[1].min(pz), b[2].max(px), b[3].max(pz)];
    }

    b
}

/// Bilinear in a grid (x0, z0, step, nx, nz), clamped.
fn grid_sample(g: &(f64, f64, f64, usize, usize, Vec<f32>), x: f64, z: f64) -> f32 {
    let (x0, z0, step, nx, nz, h) = g;
    let fx = ((x - x0) / step).clamp(0.0, (*nx - 1) as f64);
    let fz = ((z - z0) / step).clamp(0.0, (*nz - 1) as f64);
    let (i, j) = (fx.floor() as usize, fz.floor() as usize);
    let (i1, j1) = ((i + 1).min(nx - 1), (j + 1).min(nz - 1));
    let (tx, tz) = ((fx - i as f64) as f32, (fz - j as f64) as f32);
    let v = |i: usize, j: usize| h[j * nx + i];
    let a = v(i, j) + (v(i1, j) - v(i, j)) * tx;
    let b = v(i, j1) + (v(i1, j1) - v(i, j1)) * tx;
    a + (b - a) * tz
}

/// A material image (row 0 at z0) resampled into a frame rectangle: `back` maps the frame to the image's own frame.
fn resample_image(img: &TerrainImage, src: [f64; 4], back: &Affine2, dst: [f64; 4], w: u32, h: u32) -> TerrainImage {
    let Some(rgba) = &img.rgba else { return img.clone() };
    if img.w == 0 || img.h == 0 {
        return img.clone();
    }

    let mut out = vec![0u8; w as usize * h as usize * 4];
    out.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        let fz = dst[1] + (y as f64 + 0.5) / f64::from(h) * (dst[3] - dst[1]);
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let fx = dst[0] + (x as f64 + 0.5) / f64::from(w) * (dst[2] - dst[0]);
            let (lx, lz) = back.apply(fx, fz);
            let u = (((lx - src[0]) / (src[2] - src[0])) * f64::from(img.w)).floor().clamp(0.0, f64::from(img.w - 1)) as usize;
            let v = (((lz - src[1]) / (src[3] - src[1])) * f64::from(img.h)).floor().clamp(0.0, f64::from(img.h - 1)) as usize;
            let s = (v * img.w as usize + u) * 4;
            px.copy_from_slice(&rgba[s..s + 4]);
        }
    });

    TerrainImage { w, h, rgba: Some(Arc::new(out)) }
}

/// The heights of a tile resampled under a frame transform onto a grid of the frame over the tile's image: (bounds, nx, nz, heights).
fn resample_heights(t: &Template, a: &Affine2, lift: &(dyn Fn(f64, f64) -> f32 + Sync)) -> ([f64; 4], usize, usize, Vec<f32>) {
    let (ex, ez) = t.extent;
    let b = bounds(a, 0.0, 0.0, ex, ez);
    let step = t.grid.2;
    let (nx, nz) = (((b[2] - b[0]) / step).round() as usize + 1, ((b[3] - b[1]) / step).round() as usize + 1);
    let back = a.inverse();
    let mut heights = vec![0.0f32; nx * nz];
    heights.par_chunks_mut(nx).enumerate().for_each(|(j, row)| {
        for (i, h) in row.iter_mut().enumerate() {
            let (fx, fz) = (b[0] + i as f64 * step, b[1] + j as f64 * step);
            let (lx, lz) = back.apply(fx, fz);
            *h = grid_sample(&t.grid, lx, lz) + lift(fx, fz);
        }
    });

    (b, nx, nz, heights)
}

/// The rotation part of a frame transform (no translation).
fn rotation_of(a: &Affine2) -> Affine2 {
    let mut r = *a;
    r.m[0][2] = 0.0;
    r.m[1][2] = 0.0;
    r
}

/// A tile's material under a rotation, shared by its placements with that rotation (one copy of the maps on the GPU): the blend / index maps
/// resampled over the tile without its border (the borders would blend twice), the normal map from the tile's own heights. ASSUMPTION / known
/// gap: the LF slope under a placement is in its geometry but not in this normal map.
fn shared_material(t: &Template, rot: &Affine2) -> Option<Arc<TerrainMaterial>> {
    let m = t.material.as_ref()?;
    let (b, nx, nz, heights) = resample_heights(t, rot, &|_, _| 0.0);
    let back = rot.inverse();
    let src = [m.x0, m.z0, m.x1, m.z1];
    let ppm = f64::from(m.blend.w) / (m.x1 - m.x0);
    let (w, h) = (((b[2] - b[0]) * ppm).round().max(1.0) as u32, ((b[3] - b[1]) * ppm).round().max(1.0) as u32);
    let step = t.grid.2;
    let mut mm = (**m).clone();
    mm.blend = resample_image(&m.blend, src, &back, b, w, h);
    mm.index = resample_image(&m.index, src, &back, b, w, h);
    mm.normal = terrain_normal_image(&heights, nx as i32, nz as i32, step, step, w, h);
    (mm.x0, mm.z0, mm.x1, mm.z1) = (b[0], b[1], b[2], b[3]);
    mm.id = next_id(&MATERIAL_IDS);
    mm.normal_id = next_id(&NORMAL_IDS);
    Some(Arc::new(mm))
}

/// A neighbour's terrain in the frame, lifted by `lift(x, z)` (scene units), with its rotation's shared material moved to its place.
fn place_terrain(t: &Template, a: &Affine2, lift: &(dyn Fn(f64, f64) -> f32 + Sync), material: Option<&Arc<TerrainMaterial>>) -> Terrain {
    let (b, nx, nz, heights) = resample_heights(t, a, lift);
    let step = t.grid.2;
    let mut terrain = Terrain { nx: nx as i32, nz: nz as i32, origin_x: b[0] as f32, origin_z: b[1] as f32, step: step as f32, heights, lod: true, ..Default::default() };
    if let Some(m) = material {
        let (dx, dz) = (a.m[0][2], a.m[1][2]);
        let mut mm = (**m).clone();
        (mm.x0, mm.z0, mm.x1, mm.z1) = (m.x0 + dx, m.z0 + dz, m.x1 + dx, m.z1 + dz);
        let (lo, hi) = terrain.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        (mm.y0, mm.y1) = (f64::from(lo) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
        terrain.material = Some(Arc::new(mm));
    }

    terrain
}

/// The vertical extent of a context terrain's blend box around its heights (scene units): the box is drawn over the screen and every pixel
/// of it runs the blend shader, so it stays close to the ground (enough for the buildings' terrain skirts).
const BOX_BELOW: f64 = 5.0;
const BOX_ABOVE: f64 = 30.0;

/// The neighbours' height spacing (scene units): the shared map's.
const NEIGHBOUR_STEP: f64 = 2.0;

/// A shared height map over a frame rectangle: square from min(x0, z0) to max(x1, z1) (the shader's one uv offset), `lift` everywhere, then
/// the given terrains' heights where they are (on the same step).
fn shared_map<'a>(b: [f64; 4], step: f64, lift: &(dyn Fn(f64, f64) -> f32 + Sync), terrains: impl Iterator<Item = &'a Terrain>) -> Arc<SharedHeights> {
    let origin = b[0].min(b[1]);
    let n = ((b[2].max(b[3]) - origin) / step).ceil() as usize + 1;
    let mut heights = vec![0.0f32; n * n];
    heights.par_chunks_mut(n).enumerate().for_each(|(j, row)| {
        for (i, h) in row.iter_mut().enumerate() {
            *h = lift(origin + i as f64 * step, origin + j as f64 * step);
        }
    });

    for t in terrains {
        let (i0, j0) = (((f64::from(t.origin_x) - origin) / step).round() as i64, ((f64::from(t.origin_z) - origin) / step).round() as i64);
        for j in 0..t.nz as i64 {
            for i in 0..t.nx as i64 {
                let (x, z) = (i0 + i, j0 + j);
                if x >= 0 && z >= 0 && (x as usize) < n && (z as usize) < n {
                    heights[z as usize * n + x as usize] = t.heights[(j * i64::from(t.nx) + i) as usize];
                }
            }
        }
    }

    Arc::new(SharedHeights { origin: origin as f32, step: step as f32, n: n as i32, heights, id: next_id(&NORMAL_IDS) })
}

/// A material of one colour: `base` with its blend and index maps averaged into one pixel.
fn plain_material(base: &TerrainMaterial) -> Arc<TerrainMaterial> {
    let average = |img: &TerrainImage| -> TerrainImage {
        let Some(px) = &img.rgba else { return img.clone() };
        let mut sum = [0u64; 4];
        for p in px.as_chunks::<4>().0 {
            for k in 0..4 {
                sum[k] += u64::from(p[k]);
            }
        }

        let n = (px.len() / 4).max(1) as u64;
        TerrainImage { w: 1, h: 1, rgba: Some(Arc::new(sum.iter().map(|v| (v / n) as u8).collect())) }
    };

    let mut m = base.clone();
    m.blend = average(&base.blend);
    m.index = average(&base.index);
    Arc::new(m)
}

/// The rotation of a frame transform as a key.
fn rot_key(a: &Affine2) -> [i64; 4] {
    [a.m[0][0], a.m[0][1], a.m[1][0], a.m[1][1]].map(|v| v.round() as i64)
}

/// The y of an entity's position.
fn position(m: &Float4x4) -> (f64, f64) {
    (f64::from(m.m[0][3]), f64::from(m.m[2][3]))
}

/// Builds the context (slow: it opens every neighbour tile; run it away from the UI thread).
pub fn build_context(rq: &ContextRequest, files: Option<Arc<dyn Files>>) -> ContextScene {
    let t0 = std::time::Instant::now();
    let map = &rq.map;
    let context = map.context(&rq.spot, rq.cells.0, rq.cells.1, rq.radius);
    let to_map = context.to_map.unwrap_or(Affine2::IDENTITY);
    let lf_mean = context.lf_mean;
    let unit = rq.unit;
    let lift = |x: f64, z: f64| -> f32 {
        let (mx, my) = to_map.apply(x, z);
        ((map.lf_metres(mx, my) - lf_mean) / unit) as f32
    };

    // The distinct tiles, opened in parallel.
    let mut keys: Vec<TileKey> = context.neighbours.iter().map(|(i, _)| (map.placements[*i].location.clone(), map.placements[*i].variation.clone())).collect();
    keys.sort();
    keys.dedup();
    // Opening a tile holds its whole scene and height map: a few at a time.
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().map_err(|e| e.to_string());
    let open = || -> Vec<(TileKey, Result<Template, String>)> {
        keys.into_par_iter().map(|k| { let r = load_template(&tile_dir(&rq.raw_root, &k.0, &k.1), rq, files.clone()); (k, r) }).collect()
    };
    let loaded = match pool {
        Ok(pool) => pool.install(open),
        Err(_) => open(),
    };
    let mut missing = vec![];
    let mut templates = HashMap::new();
    for (k, r) in loaded {
        match r {
            Ok(t) => {
                templates.insert(k, t);
            }

            Err(e) => missing.push(format!("{}\\{}: {e}", k.0, k.1)),
        }
    }

    // One material per tile and rotation.
    let mut rotations: Vec<(TileKey, [i64; 4])> = vec![];
    for (i, a) in &context.neighbours {
        let p = &map.placements[*i];
        let key = ((p.location.clone(), p.variation.clone()), rot_key(a));
        if !rotations.contains(&key) {
            rotations.push(key);
        }
    }

    let materials: HashMap<(TileKey, [i64; 4]), Arc<TerrainMaterial>> = rotations
        .into_par_iter()
        .filter_map(|(k, r)| {
            let t = templates.get(&k)?;
            let a = context.neighbours.iter().find(|(i, a)| rot_key(a) == r && map.placements[*i].location == k.0 && map.placements[*i].variation == k.1)?.1;
            shared_material(t, &rotation_of(&a)).map(|m| ((k, r), m))
        })
        .collect();

    let t_load = t0.elapsed().as_secs_f64();
    let mut out = ContextScene { tiles_loaded: templates.len(), missing, ..Default::default() };
    let loaded = |i: usize| templates.contains_key(&(map.placements[i].location.clone(), map.placements[i].variation.clone()));
    let mut drawn = context.clone();
    drawn.neighbours.retain(|(i, _)| loaded(*i));
    let from_map = to_map.inverse();

    let mut placed: Vec<Vec<(Float4x4, Payload)>> = drawn
        .neighbours
        .par_iter()
        .filter_map(|(i, a)| {
            let p = &map.placements[*i];
            let key = (p.location.clone(), p.variation.clone());
            let t = templates.get(&key)?;
            let material = materials.get(&(key, rot_key(a)));
            let mut v = vec![(Float4x4::IDENTITY, Payload::Terrain(place_terrain(t, a, &lift, material)))];
            let frame = matrix(a, 0.0);
            for (w, payload) in &t.entities {
                // The renderer draws water vertices as they are (no world matrix): moved into the frame here, lifted per vertex.
                if let Payload::Water(water) = payload {
                    // The tiles' sea planes give way to the context's one sea level.
                    if !water.river {
                        continue;
                    }

                    let m = frame * *w;
                    let mut water = water.clone();
                    for v in &mut water.verts {
                        let q = m.transform_point(Float3::new(v.x, v.y, v.z));
                        (v.x, v.y, v.z) = (q.x, q.y + lift(f64::from(q.x), f64::from(q.z)), q.z);
                    }

                    v.push((Float4x4::IDENTITY, Payload::Water(water)));
                    continue;
                }

                let world = frame * *w;
                let (x, z) = position(&world);
                let mut world = world;
                world.m[1][3] += lift(x, z);
                v.push((world, payload.clone()));
            }

            Some(v)
        })
        .collect();

    // The game's terrain vertex shader can only place a height texture that starts at the same world x and z: the neighbours (and the
    // cells nobody covers inside them) all read one shared map, square and on the diagonal, over everything they cover.
    let mut near_cells = context.open;
    for (i, _) in &drawn.neighbours {
        let r = map.placements[*i].rect();
        near_cells = [near_cells[0].min(r[0]), near_cells[1].min(r[1]), near_cells[2].max(r[2]), near_cells[3].max(r[3])];
    }

    let near_b = bounds(&from_map, f64::from(near_cells[0]), f64::from(near_cells[1]), f64::from(near_cells[2]), f64::from(near_cells[3]));
    let near = shared_map(near_b, NEIGHBOUR_STEP, &lift, placed.iter().filter_map(|v| match &v[0].1 {
        Payload::Terrain(t) => Some(t),
        _ => None,
    }));

    for v in &mut placed {
        if let Payload::Terrain(t) = &mut v[0].1 {
            t.shared = Some(near.clone());
        }
    }

    for v in placed {
        out.entities.extend(v);
    }

    // A plain ground for the cells no tile draws: a neighbour's material with its average weights.
    let plain = materials.values().next().map(|m| plain_material(m));
    let rect_terrain = |s: [i32; 4], shared: &Arc<SharedHeights>| -> Terrain {
        let b = bounds(&from_map, f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), f64::from(s[3]));
        let step = f64::from(shared.step);
        let (nx, nz) = (((b[2] - b[0]) / step).round() as usize + 1, ((b[3] - b[1]) / step).round() as usize + 1);
        let mut heights = vec![0.0f32; nx * nz];
        heights.par_chunks_mut(nx).enumerate().for_each(|(j, row)| {
            for (i, h) in row.iter_mut().enumerate() {
                *h = lift(b[0] + i as f64 * step, b[1] + j as f64 * step);
            }
        });

        let mut t = Terrain {
            nx: nx as i32, nz: nz as i32, origin_x: b[0] as f32, origin_z: b[1] as f32, step: step as f32, heights, lod: true, tile_metres: 32.0,
            shared: Some(shared.clone()), ..Default::default()
        };

        if let Some(base) = &plain {
            let mut m = (**base).clone();
            let (w, h) = ((nx as u32).min(1024), (nz as u32).min(1024));
            m.normal = terrain_normal_image(&t.heights, t.nx, t.nz, step, step, w, h);
            (m.x0, m.z0, m.x1, m.z1) = (b[0], b[1], b[2], b[3]);
            let (lo, hi) = t.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            (m.y0, m.y1) = (f64::from(lo) - BOX_BELOW, f64::from(hi) + BOX_ABOVE);
            m.id = next_id(&MATERIAL_IDS);
            m.normal_id = next_id(&NORMAL_IDS);
            t.material = Some(Arc::new(m));
        }

        t
    };

    // Inside the neighbours' rectangle: the cells of tiles that could not be opened.
    let holes: Vec<Terrain> = drawn.uncovered(map, &[context.open], near_cells).into_par_iter().map(|s| rect_terrain(s, &near)).collect();
    out.entities.extend(holes.into_iter().map(|t| (Float4x4::IDENTITY, Payload::Terrain(t))));

    // The LF ground beyond, within `far` cells, on its own shared map of LF pixels.
    if let Some(lf) = &map.lf {
        let w = context.window;
        let area = [(w[0] - rq.far).max(0), (w[1] - rq.far).max(0), (w[2] + rq.far).min(map.width as i32), (w[3] + rq.far).min(map.height as i32)];
        let px_per_cell = f64::from(lf.w) / f64::from(map.width);
        // A whole number of LF pixels per cell (4 on the campaign map, 8 on the settlement maps): the rectangles' sides are whole steps.
        let step = CELL / px_per_cell.round().clamp(1.0, 16.0);
        let far_b = bounds(&from_map, f64::from(area[0]), f64::from(area[1]), f64::from(area[2]), f64::from(area[3]));
        let far = shared_map(far_b, step, &lift, std::iter::empty());
        let ground: Vec<Terrain> = drawn.uncovered(map, &[context.open, near_cells], area).into_par_iter().map(|s| rect_terrain(s, &far)).collect();
        out.entities.extend(ground.into_iter().map(|t| (Float4x4::IDENTITY, Payload::Terrain(t))));

        // The sea: one plane at 0 m over everything (ASSUMPTION: the battle sea is at 0 m; the LF's lowest value is -vertical_offset, -20 m,
        // under the open sea), drawn relative to the open tile like the rest. Where the ground is higher it hides it.
        let y = (-lf_mean / unit) as f32;
        let v = |x: f64, z: f64| wce_scene::render_scene::WaterVertex { x: x as f32, y, z: z as f32, u: x as f32, v: z as f32, fade: 1.0, ..Default::default() };
        let sea = wce_scene::render_scene::Water {
            river: false, foam: 0.0, verts: vec![v(far_b[0], far_b[1]), v(far_b[2], far_b[1]), v(far_b[0], far_b[3]), v(far_b[2], far_b[3])], indices: vec![0, 2, 1, 1, 2, 3],
            ..Default::default()
        };

        out.entities.push((Float4x4::IDENTITY, Payload::Water(sea)));
    }

    out.report = format!(
        "{}: {} neighbours ({} tiles, {} missing) in cells {:?}; LF under the tile {:.1} m mean, {:.1} m range{} ({:.1} s, tiles {t_load:.1} s)",
        map.name, context.neighbours.len(), out.tiles_loaded, out.missing.len(), context.window, context.lf_mean, context.lf_range,
        if context.lf_range > 2.0 { format!(": the seams may be off by up to {:.1} m", context.lf_range * 0.5 / unit) } else { String::new() },
        t0.elapsed().as_secs_f64()
    );
    out.context = context;
    out
}

/// Puts a built context on its own layer of the scene (cleared first); its entities can't be picked.
pub fn publish_context(scene: &mut RenderScene, layer: wce_scene::render_scene::LayerId, built: &mut ContextScene, doc: wce_scene::render_scene::DocumentId) {
    scene.clear_layer(layer);
    scene.reserve(built.entities.len());
    for (world, payload) in built.entities.drain(..) {
        let e = scene.add(layer, world, payload, wce_scene::render_scene::SourceRef { doc, item: u64::MAX });
        if let Some(en) = scene.find_mut(e) {
            en.locked = true;
        }
    }
}
