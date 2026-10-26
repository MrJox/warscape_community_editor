//! The ground colour of a tile (Phase 3 of docs/plans/tile_ted_parity.md): the tile's blend map (blend_map.tif, 16 weights per pixel = the four
//! TEXTURE_GROUPs of the tile database entry x their four variations, normalised to 1) mixes the textures of the climate
//! (terrain/textures/battle/<climate>/<group>/<variation>_<season>_diffuse.dds), each repeating every `blend_pixel_scale` metres of the
//! climate's texture list. The result is one RGBA8 image over the bordered tile (the area of the height map) that the renderer cuts into the
//! terrain patches' textures. This is the CPU stand-in for the game's blend pass (docs/re/terrain.md): the same inputs and the same weights,
//! baked once per tile / climate / season instead of evaluated per pixel; it carries no normal detail and no mid-distance layers yet.

use std::path::PathBuf;

use rayon::prelude::*;
use wce_formats::dds_decode::{Rgba8Image, dds_decode_mip, dds_info};
use wce_formats::tile_climate::{ClimateList, climate_texture_path};
use wce_formats::tile_entry::TileTextureGroup;
use wce_foundation::{log, wce_prof};
use wce_tile::blend_field::BlendField;
use wce_tile::ground_type_field::GroundTypeField;
use wce_vfs::files::Files;

/// What the ground shows: the blended colour, or one of TEd's debug views of the tile's maps (Phase 3d).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainView {
    #[default]
    Colour,
    BlendGroups,
    GroundTypes,
    Protection,

    /// The water's depth over the ground (BOB's water map less its final heights), with the painted water ground types outlined.
    WaterDepth,
}

/// The water's depth over the ground on stage B's grid (`w` x `h` vertices over the tile without its border, row j along +z): water
/// surface less final height, NaN where there is no water (`wce_tile_io::attila::terrain_mesh::water_fields`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaterDepth {
    pub w: usize,
    pub h: usize,
    pub depth: Vec<f32>,
}

/// The water depth view's colour scale: shallow at 0 m, deep from this many metres.
pub const WATER_VIEW_DEEP_M: f32 = 6.0;

/// The water depth view (`TerrainView::WaterDepth`) over the tile without its border: dark where there is no water, cyan (shallow) to
/// dark blue (`WATER_VIEW_DEEP_M` and deeper) over the water; the edges of the areas `ground_type_map.png` paints as deep water in white,
/// as shallow water in yellow. The ground type map is the bordered tile at the same density (one pixel per vertex).
pub fn bake_water_depth(req: &BakeRequest, water: &WaterDepth, ground: Option<&GroundTypeField>) -> BakedColour {
    const DEEP: [u8; 3] = [0, 0, 255];
    const SHALLOW: [u8; 3] = [93, 255, 255];
    let (w, h) = (water.w, water.h);
    let border = ground.map_or(0, |g| (g.width() as usize).saturating_sub(w.saturating_sub(1)) / 2);
    let painted = |i: usize, j: usize| -> u8 {
        let Some(g) = ground else { return 0 };
        let (x, y) = ((i + border) as u32, (j + border) as u32);
        if x >= g.width() || y >= g.height() {
            return 0;
        }

        match g.rgb(x, y) {
            DEEP => 1,
            SHALLOW => 2,
            _ => 0,
        }
    };

    let mut rgba = vec![255u8; w * h * 4];
    let mut wet = 0;
    for j in 0..h {
        for i in 0..w {
            let d = water.depth[j * w + i];
            let c = if d.is_finite() && d > 0.0 {
                wet += 1;
                let f = (d / WATER_VIEW_DEEP_M).clamp(0.0, 1.0);
                let mix = |a: f32, b: f32| (a + (b - a) * f) as u8;
                [mix(140.0, 15.0), mix(235.0, 40.0), mix(245.0, 150.0)]
            } else {
                [55, 62, 55]
            };

            // The painted water's edges: a painted pixel with a neighbour painted otherwise.
            let p = painted(i, j);
            let edge = p != 0 && [(0, 1), (2, 1), (1, 0), (1, 2)].iter().any(|&(di, dj)| {
                let (x, y) = ((i + di).wrapping_sub(1), (j + dj).wrapping_sub(1));
                x < w && y < h && painted(x, y) != p
            });

            let c = match (edge, p) {
                (true, 1) => [255, 255, 255],
                (true, _) => [255, 220, 40],
                _ => c,
            };

            rgba[(j * w + i) * 4..(j * w + i) * 4 + 3].copy_from_slice(&c);
        }
    }

    let report = if water.depth.iter().all(|d| !d.is_finite()) {
        "water depth: the tile has no river or water plane".to_owned()
    } else {
        format!("water depth: {wet} wet points (cyan shallow .. blue {WATER_VIEW_DEEP_M} m and deeper); painted deep water edged white, shallow yellow")
    };

    BakedColour {
        x0: 0.0, z0: 0.0, w_m: req.w_m - 2.0 * (-req.x0), h_m: req.h_m - 2.0 * (-req.z0), w: w as u32, h: h as u32, rgba, climate: "water depth".to_owned(), report,
        ..Default::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainLook {
    /// False: the interim tiled single texture.
    pub blended: bool,
    pub climate: String,

    /// su | au | wi.
    pub season: String,

    /// Anything but Colour needs `blended`.
    pub view: TerrainView,
}

impl Default for TerrainLook {
    fn default() -> Self {
        Self { blended: true, climate: "ROME_Continental".to_owned(), season: "su".to_owned(), view: TerrainView::Colour }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakedColour {
    /// The world rectangle the image covers, metres (tile frame).
    pub x0: f64,
    pub z0: f64,
    pub w_m: f64,
    pub h_m: f64,
    pub w: u32,
    pub h: u32,

    /// sRGB, w * h * 4, rows along +z.
    pub rgba: Vec<u8>,

    /// What was used.
    pub climate: String,
    pub season: String,

    /// (group, variation) layers with any weight.
    pub layers: i32,

    /// Of those, the textures that were not found (drawn as the weighted rest).
    pub missing: i32,

    /// One line for the log / the panel.
    pub report: String,
}

impl BakedColour {
    pub fn texels_per_metre(&self) -> f64 {
        if self.w_m > 0.0 { f64::from(self.w) / self.w_m } else { 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BakeRequest {
    /// blend_map.tif of the variation folder.
    pub blend_map: PathBuf,

    /// From the entry: red, green, blue, alpha group names.
    pub groups: TileTextureGroup,

    /// The variation folder (ground_type_map.png, protection_map.dds for the debug views).
    pub folder: PathBuf,

    /// The area the blend map covers (the bordered tile).
    pub x0: f64,
    pub z0: f64,
    pub w_m: f64,
    pub h_m: f64,
    pub look: TerrainLook,

    /// Largest side of the baked image.
    pub max_dim: u32,
    pub max_texels_per_metre: f64,
}

impl Default for BakeRequest {
    fn default() -> Self {
        Self {
            blend_map: PathBuf::new(), groups: TileTextureGroup::default(), folder: PathBuf::new(), x0: 0.0, z0: 0.0, w_m: 0.0, h_m: 0.0, look: TerrainLook::default(),
            max_dim: 2048, max_texels_per_metre: 4.0,
        }
    }
}

struct Layer {
    /// 0..15 = group * 4 + variation.
    channel: usize,

    /// The mip used.
    img: Rgba8Image,

    /// Metres per repeat.
    scale: f64,
    ok: bool,
}

/// Bilinear sample with wrap (u, v in repeats), bytes r g b.
fn sample_wrap(im: &Rgba8Image, u: f64, v: f64) -> [f32; 3] {
    let u = (u - u.floor()) * f64::from(im.w) - 0.5;
    let v = (v - v.floor()) * f64::from(im.h) - 0.5;
    let (fu, fv) = (u.floor(), v.floor());
    let (tx, ty) = ((u - fu) as f32, (v - fv) as f32);
    let (w, h) = (i64::from(im.w), i64::from(im.h));
    let (x0, y0) = (((fu as i64) % w + w) % w, ((fv as i64) % h + h) % h);
    let (x1, y1) = ((x0 + 1) % w, (y0 + 1) % h);
    let (a, b, c, d) = (im.at(x0 as u32, y0 as u32), im.at(x1 as u32, y0 as u32), im.at(x0 as u32, y1 as u32), im.at(x1 as u32, y1 as u32));
    std::array::from_fn(|k| {
        let top = f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * tx;
        let bot = f32::from(c[k]) + (f32::from(d[k]) - f32::from(c[k])) * tx;
        top + (bot - top) * ty
    })
}

/// The group names in channel order.
pub(crate) fn group_names(g: &TileTextureGroup) -> [&str; 4] {
    [&g.red, &g.green, &g.blue, &g.alpha]
}

/// The baked ground colour of a tile.
///
/// # Errors
///
/// The tile has no usable blend map, or no texture was found at all.
pub fn bake_terrain_colour(req: &BakeRequest, blend: Option<&BlendField>, climates: &ClimateList, files: &dyn Files) -> Result<BakedColour, String> {
    wce_prof!("tile.terrain_bake");
    // The map in memory (painted), else the file.
    let read;
    let blend = match blend {
        Some(b) => b,
        None => {
            read = if req.blend_map.exists() { BlendField::read(&req.blend_map)? } else { return Err("no blend_map.tif".to_owned()) };
            &read
        }
    };

    let (bw, bh) = (blend.width(), blend.height());
    let weights = blend.weights();

    // The layers with weight somewhere: (group g, variation v) = channel 4g + v.
    let peak = blend.peaks();

    // Output size.
    let ppm = req.max_texels_per_metre.min(f64::from(req.max_dim) / req.w_m.max(req.h_m));
    let ow = ((req.w_m * ppm).round() as u32).max(2);
    let oh = ((req.h_m * ppm).round() as u32).max(2);
    let (out_ppm_x, out_ppm_z) = (f64::from(ow) / req.w_m, f64::from(oh) / req.h_m);

    let climate = climates.find(&req.look.climate).or_else(|| climates.find("default"));
    let folder = climate.map_or_else(|| "terrain/textures/battle/default".to_owned(), |c| c.folder());
    let groups = group_names(&req.groups);
    let exists = |p: &str| files.exists(p);

    let mut layers = vec![];
    let (mut missing, mut wanted) = (0, 0);
    for (c, &p) in peak.iter().enumerate() {
        // Under 0.2 %: nothing to draw.
        if p < 128 {
            continue;
        }

        wanted += 1;
        let group = groups[c / 4];
        if req.look.view == TerrainView::BlendGroups {
            // The debug view: every group one flat colour (red, green, blue, yellow).
            const COL: [[u8; 3]; 4] = [[230, 60, 60], [60, 200, 70], [70, 110, 240], [235, 210, 60]];
            let [r, g, b] = COL[c / 4];
            layers.push(Layer { channel: c, img: Rgba8Image { w: 1, h: 1, px: vec![r, g, b, 255] }, scale: 40.1, ok: true });
            continue;
        }

        let ct = climate.and_then(|cl| cl.find(group)).or_else(|| climates.find("default").and_then(|d| d.find(group)));
        let scale = ct.filter(|ct| ct.blend_pixel_scale > 0.5).map_or(40.1, |ct| ct.blend_pixel_scale);
        let path = if group.is_empty() { None } else { climate_texture_path(&folder, group, (c % 4) as i32, &req.look.season, false, exists) };
        let Some(path) = path else {
            missing += 1;
            continue;
        };

        let mut layer = Layer { channel: c, img: Rgba8Image::default(), scale, ok: false };
        match files.read(&path) {
            Ok(bytes) => {
                // The mip whose texel density is nearest the output's (so the bake neither aliases nor blurs more than it has to).
                let info = dds_info(&bytes);
                let density = f64::from(info.width) / scale;
                let mut mip = 0;
                while mip + 1 < info.mips && density / f64::from(1u32 << (mip + 1)) >= out_ppm_x.max(0.5) {
                    mip += 1;
                }

                match dds_decode_mip(&bytes, mip) {
                    Ok(img) => {
                        layer.img = img;
                        layer.ok = true;
                    }

                    Err(err) => {
                        log::warn(format!("terrain bake: {path}: {err}"));
                        missing += 1;
                        continue;
                    }
                }
            }

            Err(e) => {
                log::warn(format!("terrain bake: {path}: {e}"));
                missing += 1;
            }
        }

        layers.push(layer);
    }

    if req.look.view == TerrainView::BlendGroups {
        missing = 0;
    }

    if !layers.iter().any(|l| l.ok) {
        return Err(format!("none of the climate's textures for this tile was found ({})", req.look.climate));
    }

    let mut out = BakedColour {
        x0: req.x0, z0: req.z0, w_m: req.w_m, h_m: req.h_m, w: ow, h: oh, rgba: vec![255; ow as usize * oh as usize * 4],
        climate: climate.map_or_else(|| "default".to_owned(), |c| c.name.clone()), season: req.look.season.clone(), layers: wanted, missing, report: String::new(),
    };

    // The weights of the layers that could not be read are dropped and the rest renormalised, so a missing texture shows as its neighbours.
    out.rgba.par_chunks_mut(ow as usize * 4).enumerate().for_each(|(row, line)| {
        let mut wgt = vec![0.0f32; layers.len()];
        let z = req.z0 + (row as f64 + 0.5) / out_ppm_z;

        // Blend map sample row (pixel centres).
        let by = (z - req.z0) / req.h_m * f64::from(bh) - 0.5;
        let y0 = (by.floor() as i32).clamp(0, bh as i32 - 1);
        let y1 = (y0 + 1).min(bh as i32 - 1);
        let ty = (by - by.floor()).clamp(0.0, 1.0) as f32;
        for (col, o) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = req.x0 + (col as f64 + 0.5) / out_ppm_x;
            let bx = (x - req.x0) / req.w_m * f64::from(bw) - 0.5;
            let x0 = (bx.floor() as i32).clamp(0, bw as i32 - 1);
            let x1 = (x0 + 1).min(bw as i32 - 1);
            let tx = (bx - bx.floor()).clamp(0.0, 1.0) as f32;
            let px = |x: i32, y: i32| y as usize * bw as usize + x as usize;
            let mut sum = 0.0f32;
            for (k, l) in layers.iter().enumerate() {
                if !l.ok {
                    wgt[k] = 0.0;
                    continue;
                }

                let c = l.channel;
                let (a, b) = (f32::from(weights[px(x0, y0) * 16 + c]), f32::from(weights[px(x1, y0) * 16 + c]));
                let (cc, d) = (f32::from(weights[px(x0, y1) * 16 + c]), f32::from(weights[px(x1, y1) * 16 + c]));
                let (top, bot) = (a + (b - a) * tx, cc + (d - cc) * tx);
                wgt[k] = top + (bot - top) * ty;
                sum += wgt[k];
            }

            let mut rgb = [0.0f32; 3];
            if sum > 1e-3 {
                for (k, l) in layers.iter().enumerate() {
                    if wgt[k] <= 0.0 {
                        continue;
                    }

                    let s = sample_wrap(&l.img, x / l.scale, z / l.scale);
                    let f = wgt[k] / sum;
                    rgb[0] += s[0] * f;
                    rgb[1] += s[1] * f;
                    rgb[2] += s[2] * f;
                }
            } else {
                rgb = [96.0; 3];
            }

            for k in 0..3 {
                o[k] = (rgb[k] + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    });

    if req.look.view == TerrainView::BlendGroups {
        out.climate = "blend groups".to_owned();
    }

    let missing_text = if missing != 0 { format!(", {missing} texture(s) missing") } else { String::new() };
    out.report = format!("{} {}: {wanted} layer(s){missing_text}, {ow} x {oh} texels ({out_ppm_x:.2} / m)", out.climate, out.season);
    Ok(out)
}

/// The tile's ground type map (ground_type_map.png, RGBA over the bordered tile) and protection map (protection_map.dds, R32F over the tile
/// without border; 1 = protected) as images over the same area, for TEd's debug views (view GroundTypes / Protection).
///
/// # Errors
///
/// The file is missing or unreadable.
pub fn bake_terrain_map_view(req: &BakeRequest, ground: Option<&GroundTypeField>) -> Result<BakedColour, String> {
    if req.look.view == TerrainView::GroundTypes {
        // The map in memory (painted), else the file.
        let read;
        let ground = match ground {
            Some(g) => g,
            None => {
                let p = req.folder.join("ground_type_map.png");
                read = if p.exists() { GroundTypeField::read(&p)? } else { return Err("no ground_type_map.png".to_owned()) };
                &read
            }
        };

        let rgba = ground.rgba();
        let nonzero = rgba.as_chunks::<4>().0.iter().filter(|p| p[0] | p[1] | p[2] != 0).count();
        let report = if nonzero != 0 {
            format!("ground types: the map's RGB as stored ({nonzero} pixels set)")
        } else {
            "ground types: the map is empty (all pixels 0: no ground types painted)".to_owned()
        };

        return Ok(BakedColour {
            x0: req.x0, z0: req.z0, w_m: req.w_m, h_m: req.h_m, w: ground.width(), h: ground.height(), rgba, climate: "ground types".to_owned(), report, ..Default::default()
        });
    }

    let p = req.folder.join("protection_map.dds");
    if !p.exists() {
        return Err("no protection_map.dds".to_owned());
    }

    let bytes = std::fs::read(&p).unwrap_or_default();
    if bytes.len() < 132 || &bytes[..4] != b"DDS " {
        return Err("protection_map.dds is not a DDS file".to_owned());
    }

    let le = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (h, w) = (le(12), le(16));
    let n = w as usize * h as usize;
    if w == 0 || h == 0 || bytes.len() < 128 + n * 4 {
        return Err("protection_map.dds is not an R32F image".to_owned());
    }

    // The map covers the tile itself (no border): it sits at 0 .. extent in the bordered area's frame.
    let mut rgba = vec![255u8; n * 4];
    let mut set = 0;
    for (d, s) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(bytes[128..].as_chunks::<4>().0) {
        if f32::from_le_bytes(*s) > 0.5 {
            d[..3].copy_from_slice(&[220, 50, 50]);
            set += 1;
        } else {
            d[..3].copy_from_slice(&[70, 120, 70]);
        }
    }

    Ok(BakedColour {
        x0: 0.0, z0: 0.0, w_m: req.w_m - 2.0 * (-req.x0), h_m: req.h_m - 2.0 * (-req.z0), w, h, rgba, climate: "protection".to_owned(),
        report: format!("protection: {set} of {n} cells protected (red)"), ..Default::default()
    })
}
