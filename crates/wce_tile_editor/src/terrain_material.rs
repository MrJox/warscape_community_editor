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
use wce_formats::tile_climate::{ClimateList, climate_texture_path};
use wce_formats::tile_entry::TileTextureGroup;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{TerrainImage, TerrainMaterial};
use wce_vfs::files::Files;

use crate::terrain_bake::{TerrainLook, group_names, read_blend_map};

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

/// The material and one line for the log / the panel. The normal map is left flat: set it with [`terrain_normal_image`].
///
/// # Errors
///
/// The tile has no usable blend map, or none of the climate's textures exists.
pub fn build_terrain_material(rq: &MaterialRequest, climates: &ClimateList, files: &dyn Files) -> Result<(TerrainMaterial, String), String> {
    wce_prof!("tile.terrain_material");
    let blend = read_blend_map(&rq.folder.join("blend_map.tif"))?;
    let (bw, bh) = (blend.w, blend.h);
    let mut m = TerrainMaterial { id: rq.id, x0: rq.x0, z0: rq.z0, x1: rq.x1, z1: rq.z1, y0: rq.y0 - 20.0, y1: rq.y1 + 200.0, ..Default::default() };

    // Blend / index maps (BOB's reduction of the 16 weights).
    let n = bw as usize * bh as usize;
    let mut blend_px = vec![0u8; n * 4];
    let mut index_px = vec![0u8; n * 4];
    blend_px.par_chunks_mut(4).zip(index_px.par_chunks_mut(4)).enumerate().for_each(|(i, (b, ix))| {
        let p = &blend.weights[i * 16..i * 16 + 16];
        for g in 0..4 {
            let s = f64::from(p[g * 4]) + f64::from(p[g * 4 + 1]) + f64::from(p[g * 4 + 2]) + f64::from(p[g * 4 + 3]);
            b[g] = q8(s / 65535.0);
            ix[g] = if s > 0.0 { q8((f64::from(p[g * 4 + 1]) + 2.0 * f64::from(p[g * 4 + 2]) + 3.0 * f64::from(p[g * 4 + 3])) / s / 3.0) } else { 0 };
        }
    });

    let peak = blend.peaks();
    m.blend = TerrainImage { w: bw, h: bh, rgba: Some(Arc::new(blend_px)) };
    m.index = TerrainImage { w: bw, h: bh, rgba: Some(Arc::new(index_px)) };

    // Flat until the heights are given.
    m.normal = flat_normal();

    // The textures of the climate: slot = 4 * group + variation.
    let climate = climates.find(&rq.look.climate).or_else(|| climates.find("default"));
    let folder = climate.map_or_else(|| "terrain/textures/battle/default".to_owned(), |c| c.folder());
    let def = climates.find("default");
    let groups = group_names(&rq.groups);
    let exists = |p: &str| files.exists(p);
    let (mut wanted, mut missing) = (0, 0);
    for (g, group) in groups.iter().enumerate() {
        let ct = climate.and_then(|c| c.find(group)).or_else(|| def.and_then(|d| d.find(group)));
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

            let used = peak[slot] >= 128;
            if group.is_empty() {
                continue;
            }

            m.diffuse[slot] = climate_texture_path(&folder, group, v as i32, &rq.look.season, false, exists).unwrap_or_default();
            m.normal_tex[slot] = climate_texture_path(&folder, group, v as i32, &rq.look.season, true, exists).unwrap_or_default();
            if used {
                wanted += 1;
                if m.diffuse[slot].is_empty() {
                    missing += 1;
                }
            }
        }
    }

    if m.diffuse.iter().all(String::is_empty) {
        return Err(format!("none of the climate's textures for this tile was found ({})", rq.look.climate));
    }

    // The detail maps of the climate.
    for root in [folder.as_str(), "terrain/textures/battle/default"] {
        let (colour, normal) = (format!("{root}/medium_detail_map_colour.dds"), format!("{root}/medium_detail_map_normal.dds"));
        if m.detail_colour.is_empty() && exists(&colour) {
            m.detail_colour = colour;
        }

        if m.detail_normal.is_empty() && exists(&normal) {
            m.detail_normal = normal;
        }
    }

    let missing_text = if missing != 0 { format!(", {missing} texture(s) missing") } else { String::new() };
    let report = format!(
        "{} {}: {wanted} layer(s){missing_text}, {bw} x {bh} blend map (the game's terrain shaders)", climate.map_or("default", |c| c.name.as_str()), rq.look.season
    );

    Ok((m, report))
}

/// The normal map (.a = x, .g = z of the unit normal * 0.5 + 0.5) of a height grid (nx x nz samples, `step_x` / `step_z` metres apart, row j
/// along +z), as a w x h image of the same grid. Central differences like TEd's.
pub fn terrain_normal_image(heights: &[f32], nx: i32, nz: i32, step_x: f64, step_z: f64, w: u32, h: u32) -> TerrainImage {
    if nx < 2 || nz < 2 || heights.len() < nx as usize * nz as usize || w == 0 || h == 0 {
        return flat_normal();
    }

    let height = |i: i32, j: i32| f64::from(heights[j.clamp(0, nz - 1) as usize * nx as usize + i.clamp(0, nx - 1) as usize]);
    let mut px = vec![0u8; w as usize * h as usize * 4];
    px.par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        let j = (y as f64 * f64::from(nz - 1) / f64::from((h - 1).max(1))).round() as i32;
        for (x, d) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let i = (x as f64 * f64::from(nx - 1) / f64::from((w - 1).max(1))).round() as i32;
            let dhdx = (height(i + 1, j) - height(i - 1, j)) / (2.0 * step_x);
            let dhdz = (height(i, j + 1) - height(i, j - 1)) / (2.0 * step_z);
            let len = (dhdx * dhdx + 1.0 + dhdz * dhdz).sqrt();
            *d = [255, q8((-dhdz / len) * 0.5 + 0.5), 0, q8((-dhdx / len) * 0.5 + 0.5)];
        }
    });

    TerrainImage { w, h, rgba: Some(Arc::new(px)) }
}
