//! The ground of a tile for the game's own terrain shaders (docs/re/terrain.md, the renderer's terrain passes). The shaders read, per tile: a
//! blend map (the weights of the entry's four TEXTURE_GROUPs), an index map (which of a group's four variations), a normal map, and the
//! climate's diffuse / normal textures. BOB makes the first two from the tile's blend_map.tif (16 weights per pixel = group x variation):
//! blend = the sum of a group's four weights, index = the weighted mean variation / 3 (checked against BOB's blend.dds / index.dds of the
//! osgiliath test map: correlation 0.998, the rest is the DXT5 compression); the normal map comes from the heights (TEd's tile_normal.tga is
//! the same thing: R = -dh/dx, G = -dh/dz of the same grid, correlation 0.97 / 0.98 with the height map's gradient), so it follows a brush
//! stroke.

use std::path::PathBuf;
use std::sync::Arc;

use rayon::prelude::*;
use wce_formats::tile_climate::ClimateList;
use wce_formats::tile_entry::TileTextureGroup;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{TerrainImage, TerrainMaterial};
use wce_vfs::files::Files;

use wce_tile::blend_field::{BlendField, reduce_pixel};

use crate::terrain_bake::{TerrainLook, group_names};

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRequest {
    /// The variation folder: blend_map.tif, tile_normal.tga.
    pub folder: PathBuf,

    /// From the entry: red, green, blue, alpha group names.
    pub groups: TileTextureGroup,

    /// The area the maps cover (the bordered tile), tile frame metres.
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,

    /// Vertical extent of the terrain (for the box that draws it).
    pub y0: f64,
    pub y1: f64,
    pub look: TerrainLook,
    pub id: u64,
}

impl Default for MaterialRequest {
    fn default() -> Self {
        Self { folder: PathBuf::new(), groups: TileTextureGroup::default(), x0: 0.0, z0: 0.0, x1: 0.0, z1: 0.0, y0: -100.0, y1: 1000.0, look: TerrainLook::default(), id: 1 }
    }
}

fn q8(v: f64) -> u8 {
    (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn flat_normal() -> TerrainImage {
    TerrainImage { w: 1, h: 1, rgba: Some(Arc::new(vec![255, 127, 0, 127])) }
}

/// The material and one line for the log / the panel, from the tile's blend map. The normal map is left flat: set it with
/// [`terrain_normal_image`].
///
/// # Errors
///
/// None of the climate's textures exists.
pub fn build_terrain_material(rq: &MaterialRequest, blend: &BlendField, climates: &ClimateList, files: &dyn Files) -> Result<(TerrainMaterial, String), String> {
    wce_prof!("tile.terrain_material");
    let (bw, bh) = (blend.width(), blend.height());
    let mut m = TerrainMaterial {
        id: rq.id, x0: rq.x0, z0: rq.z0, x1: rq.x1, z1: rq.z1, y0: rq.y0 - 20.0, y1: rq.y1 + 200.0, campaign: climates.kind == "campaign", ..Default::default()
    };

    // Blend / index maps (BOB's reduction of the 16 weights).
    let n = bw as usize * bh as usize;
    let mut blend_px = vec![0u8; n * 4];
    let mut index_px = vec![0u8; n * 4];
    let weights = blend.weights();
    blend_px.par_chunks_mut(4).zip(index_px.par_chunks_mut(4)).enumerate().for_each(|(i, (b, ix))| {
        let (bb, ii) = reduce_pixel(&weights[i * 16..i * 16 + 16]);
        b.copy_from_slice(&bb);
        ix.copy_from_slice(&ii);
    });

    let peak = blend.peaks();
    m.blend = TerrainImage { w: bw, h: bh, rgba: Some(Arc::new(blend_px)) };
    m.index = TerrainImage { w: bw, h: bh, rgba: Some(Arc::new(index_px)) };

    // Flat until the heights are given.
    m.normal = flat_normal();

    let (wanted, missing) = apply_climate(&mut m, &rq.groups, &rq.look, Some(&peak), climates, files);
    if m.diffuse.iter().all(String::is_empty) {
        return Err(format!("none of the climate's textures for this tile was found ({})", rq.look.climate));
    }

    let climate = climates.find(&rq.look.climate).or_else(|| climates.find("default"));
    let missing_text = if missing != 0 { format!(", {missing} texture(s) missing") } else { String::new() };
    let report = format!(
        "{} {}: {wanted} layer(s){missing_text}, {bw} x {bh} blend map (the game's terrain shaders)", climate.map_or("default", |c| c.name.as_str()), rq.look.season
    );

    Ok((m, report))
}

/// The blend / index maps of a material made from `blend` again over a rectangle of its pixels (exclusive end), after a paint stroke. The
/// maps must be the blend map's size.
pub fn patch_blend_maps(m: &mut TerrainMaterial, blend: &BlendField, (x0, y0, x1, y1): (u32, u32, u32, u32)) {
    let w = blend.width();
    if m.blend.w != w || m.blend.h != blend.height() || m.index.w != w || m.index.h != blend.height() {
        return;
    }

    let (Some(bp), Some(ip)) = (m.blend.rgba.as_mut(), m.index.rgba.as_mut()) else { return };
    let (bp, ip) = (Arc::make_mut(bp), Arc::make_mut(ip));
    for y in y0..y1.min(blend.height()) {
        for x in x0..x1.min(w) {
            let (b, i) = reduce_pixel(blend.pixel(x, y));
            let at = (y as usize * w as usize + x as usize) * 4;
            bp[at..at + 4].copy_from_slice(&b);
            ip[at..at + 4].copy_from_slice(&i);
        }
    }
}

/// A material with another climate's (or season's) textures: its maps kept, its slots filled again for `look` (the groups are the
/// tile entry's red, green, blue and alpha ones, as the material was built with). A new id for the renderer.
pub fn retexture(base: &TerrainMaterial, groups: &TileTextureGroup, look: &TerrainLook, climates: &ClimateList, files: &dyn Files, id: u64) -> TerrainMaterial {
    let mut m = base.clone();
    (m.diffuse, m.normal_tex) = Default::default();
    (m.detail_colour, m.detail_normal) = (String::new(), String::new());
    m.id = id;
    apply_climate(&mut m, groups, look, None, climates, files);
    m
}

/// The textures and per-layer values of a look's climate into a material's slots (slot = 4 * group + variation), and its detail maps;
/// with the blend map's peaks, how many of the used slots there are and how many have no texture.
fn apply_climate(m: &mut TerrainMaterial, groups: &TileTextureGroup, look: &TerrainLook, peak: Option<&[u32; 16]>, climates: &ClimateList, files: &dyn Files) -> (i32, i32) {
    let climate = climates.find(&look.climate).or_else(|| climates.find("default"));
    let default_folder = climates.default_folder();
    let folder = climate.map_or_else(|| default_folder.clone(), |c| climates.folder_of(c));
    let def = climates.find("default");
    let groups = group_names(groups);
    let exists = |p: &str| files.exists(p);
    let (mut wanted, mut missing) = (0, 0);

    // Rome 2's climates list only the textures that differ: the others repeat every RENDER_PARAMS blend_pixel_scale (fn 0x115bfb40).
    let rome2 = files.game().is_some_and(|g| g.key == wce_vfs::GameProfile::ROME_2.key);
    for (g, group) in groups.iter().enumerate() {
        let ct = climate.and_then(|c| c.find(group)).or_else(|| def.and_then(|d| d.find(group)));
        if rome2 && ct.is_none() {
            m.pixel_scale[g] = climates.blend_pixel_scale.unwrap_or(9.0) as f32;
        }

        if let Some(ct) = ct {
            m.pixel_scale[g] = (if ct.blend_pixel_scale > 0.5 { ct.blend_pixel_scale } else { 40.1 }) as f32;
            m.outfield_scale[g] = ct.outfield_blend_pixel_scale as f32;
            m.mid_strength[g] = ct.mid_distance_strength as f32;
        }

        for v in 0..4 {
            let slot = 4 * g + v;
            if let Some(ct) = ct {
                m.smoothness[slot] = ct.lerp_smoothness[v] as f32;
                m.reflectivity[slot] = ct.lerp_reflectivity[v] as f32;
            }

            let used = peak.is_some_and(|p| p[slot] >= 128);
            if group.is_empty() {
                continue;
            }

            m.diffuse[slot] = climates.texture_path(&folder, group, v as i32, &look.season, false, exists).unwrap_or_default();
            m.normal_tex[slot] = climates.texture_path(&folder, group, v as i32, &look.season, true, exists).unwrap_or_default();
            if used {
                wanted += 1;
                if m.diffuse[slot].is_empty() {
                    missing += 1;
                }
            }
        }
    }

    // The detail maps of the climate.
    for root in [folder.as_str(), default_folder.as_str()] {
        let (colour, normal) = (format!("{root}/medium_detail_map_colour.dds"), format!("{root}/medium_detail_map_normal.dds"));
        if m.detail_colour.is_empty() && exists(&colour) {
            m.detail_colour = colour;
        }

        if m.detail_normal.is_empty() && exists(&normal) {
            m.detail_normal = normal;
        }
    }

    // Rome 2's climates have one detail map, a normal map.
    for root in [folder.as_str(), default_folder.as_str()] {
        let normal = format!("{root}/medium_detail_map.dds");
        if m.detail_normal.is_empty() && exists(&normal) {
            m.detail_normal = normal;
        }
    }

    (wanted, missing)
}

/// The normal map (.a = x, .g = z of the unit normal * 0.5 + 0.5) of a height grid (nx x nz samples, `step_x` / `step_z` metres apart, row j
/// along +z), as a w x h image of the same grid. Central differences like TEd's.
pub fn terrain_normal_image(heights: &[f32], nx: i32, nz: i32, step_x: f64, step_z: f64, w: u32, h: u32) -> TerrainImage {
    if nx < 2 || nz < 2 || heights.len() < nx as usize * nz as usize || w == 0 || h == 0 {
        return flat_normal();
    }

    let mut px = vec![0u8; w as usize * h as usize * 4];
    px.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        for (x, d) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *d = normal_pixel(heights, nx, nz, step_x, step_z, w, h, x as u32, y as u32);
        }
    });

    TerrainImage { w, h, rgba: Some(Arc::new(px)) }
}

/// `terrain_normal_image`'s pixel (x, y).
#[allow(clippy::too_many_arguments, reason = "the grid, its spacing, the image size and the pixel")]
fn normal_pixel(heights: &[f32], nx: i32, nz: i32, step_x: f64, step_z: f64, w: u32, h: u32, x: u32, y: u32) -> [u8; 4] {
    let height = |i: i32, j: i32| f64::from(heights[j.clamp(0, nz - 1) as usize * nx as usize + i.clamp(0, nx - 1) as usize]);
    let j = (f64::from(y) * f64::from(nz - 1) / f64::from((h - 1).max(1))).round() as i32;
    let i = (f64::from(x) * f64::from(nx - 1) / f64::from((w - 1).max(1))).round() as i32;
    let dhdx = (height(i + 1, j) - height(i - 1, j)) / (2.0 * step_x);
    let dhdz = (height(i, j + 1) - height(i, j - 1)) / (2.0 * step_z);
    let len = (dhdx * dhdx + 1.0 + dhdz * dhdz).sqrt();
    [255, q8((-dhdz / len) * 0.5 + 0.5), 0, q8((-dhdx / len) * 0.5 + 0.5)]
}

/// `terrain_normal_image`'s image made again where the grid changed in `cells` (i0, j0, i1, j1; end exclusive): the pixels whose samples
/// or their neighbours are there. The pixels written (x0, y0, x1, y1; end exclusive), none when the image is not one of this grid.
#[allow(clippy::too_many_arguments, reason = "the image, the grid, its spacing and the cells")]
pub fn patch_terrain_normal(img: &mut TerrainImage, heights: &[f32], nx: i32, nz: i32, step_x: f64, step_z: f64, cells: [i32; 4]) -> Option<[u32; 4]> {
    let (w, h) = (img.w, img.h);
    if nx < 2 || nz < 2 || heights.len() < nx as usize * nz as usize || w < 2 || h < 2 {
        return None;
    }

    // A pixel reads the samples one either side of its own: the cells widened by one, then to the pixels (rounded outwards).
    let (sx, sz) = (f64::from(w - 1) / f64::from(nx - 1), f64::from(h - 1) / f64::from(nz - 1));
    let x0 = ((f64::from(cells[0] - 1) - 0.5) * sx).floor().max(0.0) as u32;
    let y0 = ((f64::from(cells[1] - 1) - 0.5) * sz).floor().max(0.0) as u32;
    let x1 = (((f64::from(cells[2]) + 0.5) * sx).ceil() as u32 + 1).min(w);
    let y1 = (((f64::from(cells[3]) + 0.5) * sz).ceil() as u32 + 1).min(h);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }

    let px = Arc::make_mut(img.rgba.as_mut()?);
    for y in y0..y1 {
        for x in x0..x1 {
            let at = (y as usize * w as usize + x as usize) * 4;
            px[at..at + 4].copy_from_slice(&normal_pixel(heights, nx, nz, step_x, step_z, w, h, x, y));
        }
    }

    Some([x0, y0, x1, y1])
}
