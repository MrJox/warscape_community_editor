//! What the Tile tool's minimap draws (docs/plans/editor_todo.md): the tile from above as pictures (its ground's colour, its heights) and
//! its siege AI and deployment zones as shapes. The pictures are in the tile frame's metres, rows along +z (as `BakedColour`); the siege
//! shapes are in scene units (`MinimapSiege::unit` to the metre).

use std::sync::Arc;

use rayon::prelude::*;
use wce_formats::tile_climate::ClimateList;
use wce_siege::{Deployment, Input, Layout};
use wce_tile::blend_field::BlendField;
use wce_vfs::files::Files;

use super::{Tile, TileDocument};
use crate::terrain_bake::{BakeRequest, BakedColour, TerrainView, bake_terrain_colour};

/// A picture of the tile from above: sRGB, `w * h * 4`, rows along +z, over `x0, z0, w_m, h_m` (metres).
#[derive(Clone, Debug, Default)]
pub struct MinimapImage {
    pub x0: f64,
    pub z0: f64,
    pub w_m: f64,
    pub h_m: f64,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,

    /// The lowest and highest ground (height pictures).
    pub range: (f64, f64),
}

/// The siege AI the tile exports now and what it is made around (the generator's input), with every deployment zone group.
#[derive(Clone, Debug, Default)]
pub struct MinimapSiege {
    /// Scene units per metre.
    pub unit: f64,
    pub input: Input,
    pub layout: Layout,

    /// (alliance_id, zone), every group.
    pub zones: Vec<(String, Deployment)>,
}

/// The ground's colour to bake away from the UI thread (it reads the blend map and the climate's textures: seconds on a big tile).
pub struct ColourBake {
    pub request: BakeRequest,
    /// The blend map in memory (painted), else the file is read.
    pub blend: Option<BlendField>,
    pub climates: ClimateList,
    pub files: Arc<dyn Files>,
}

impl ColourBake {
    /// # Errors
    ///
    /// The blend map or the textures do not read.
    pub fn run(&self) -> Result<BakedColour, String> {
        bake_terrain_colour(&self.request, self.blend.as_ref(), &self.climates, self.files.as_ref())
    }
}

impl Tile {
    /// What the minimap shows (metres, x0 z0 w h): the bordered tile when the height map has its border, else the tile.
    pub fn minimap_area(&self) -> [f64; 4] {
        let b = self.project.border_m();
        [-b, -b, self.extent_x() + 2.0 * b, self.extent_z() + 2.0 * b]
    }

    /// Changes whenever the terrain is published again (a height stroke, a new look).
    pub fn terrain_version(&self) -> u64 {
        self.terrain_version
    }

    /// The heights from above, at most `max_dim` pixels a side: a colour ramp from the lowest ground to the highest, shaded by a light from
    /// the north-west, the ground under the sea (0 m) blue. `None` without a height map.
    pub fn minimap_heights(&self, max_dim: u32) -> Option<MinimapImage> {
        if !self.project.has_height() {
            return None;
        }

        let [x0, z0, w_m, h_m] = self.minimap_area();
        let ppm = f64::from(max_dim) / w_m.max(h_m);
        let (w, h) = (((w_m * ppm).round() as u32).max(2), ((h_m * ppm).round() as u32).max(2));
        let (cx, cz) = (w_m / f64::from(w), h_m / f64::from(h));
        let project = &self.project;
        let heights: Vec<f64> = (0..w * h).into_par_iter().map(|k| {
            let (i, j) = (k % w, k / w);
            project.height_at(x0 + (f64::from(i) + 0.5) * cx, z0 + (f64::from(j) + 0.5) * cz)
        }).collect();

        let (lo, hi) = heights.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        let land_lo = lo.max(0.0);
        let span = (hi - land_lo).max(1.0);
        let at = |i: i64, j: i64| heights[(j.clamp(0, i64::from(h) - 1) * i64::from(w) + i.clamp(0, i64::from(w) - 1)) as usize];

        // The light: from the north-west (x west, z north), 45 degrees up.
        let light = {
            let l = [-1.0f64, 1.0, 1.0];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n]
        };

        let rgba = (0..w * h).into_par_iter().flat_map_iter(|k| {
            let (i, j) = (i64::from(k % w), i64::from(k / w));
            let v = at(i, j);
            let (dx, dz) = ((at(i + 1, j) - at(i - 1, j)) / (2.0 * cx), (at(i, j + 1) - at(i, j - 1)) / (2.0 * cz));
            let n = (dx * dx + dz * dz + 1.0).sqrt();
            let shade = ((-dx * light[0] - dz * light[1] + light[2]) / n).clamp(0.0, 1.0);
            let c = if v < 0.0 { sea_colour(-v) } else { land_colour((v - land_lo) / span) };
            let k = 0.45 + 0.75 * shade;
            [(c[0] * k).min(255.0) as u8, (c[1] * k).min(255.0) as u8, (c[2] * k).min(255.0) as u8, 255]
        }).collect();

        Some(MinimapImage { x0, z0, w_m, h_m, w, h, rgba, range: (lo, hi) })
    }

    /// The bake of the ground's colour for the minimap (the CPU stand-in of the game's blend, at most `max_dim` pixels a side) with the
    /// current climate and season.
    ///
    /// # Errors
    ///
    /// The tile has no blend map area (no database entry or no bordered height map) or no game files.
    pub fn minimap_colour_bake(&self, max_dim: u32) -> Result<ColourBake, String> {
        let p = &self.project;
        if !(p.has_height() && p.has_entry() && p.bordered()) {
            return Err("no blend map area (the tile has no database entry or no bordered height map)".to_owned());
        }

        let files = self.files.clone().ok_or("no game files")?;
        let [x0, z0, w_m, h_m] = self.minimap_area();
        let look = crate::terrain_bake::TerrainLook { blended: true, view: TerrainView::Colour, ..self.look.clone() };
        let request = BakeRequest {
            blend_map: p.dir.join("blend_map.tif"), folder: p.dir.clone(), groups: p.entry.as_ref().expect("an entry").texture_group(), x0, z0, w_m, h_m,
            look, max_dim, ..Default::default()
        };

        Ok(ColourBake { request, blend: p.blend.clone(), climates: self.climates().clone(), files })
    }

    /// A picture of the ground (`BakedColour`'s layout) with the sea over it: the ground under 0 m blue, darker the deeper.
    pub fn minimap_sea_over(&self, img: &BakedColour) -> Vec<u8> {
        let (w, h) = (img.w, img.h);
        let (cx, cz) = (img.w_m / f64::from(w), img.h_m / f64::from(h));
        let project = &self.project;
        img.rgba.par_chunks(4).enumerate().flat_map_iter(|(k, px)| {
            let (i, j) = (k as u32 % w, k as u32 / w);
            let depth = -project.height_at(img.x0 + (f64::from(i) + 0.5) * cx, img.z0 + (f64::from(j) + 0.5) * cz);
            if depth <= 0.0 {
                return [px[0], px[1], px[2], px[3]];
            }

            let (sea, a) = (sea_colour(depth), 0.55 + 0.35 * (depth / 20.0).clamp(0.0, 1.0));
            let mix = |c: u8, s: f64| (f64::from(c) * (1.0 - a) + s * a) as u8;
            [mix(px[0], sea[0]), mix(px[1], sea[1]), mix(px[2], sea[2]), px[3]]
        }).collect()
    }

    /// The siege AI and the deployment zones for the minimap.
    pub fn minimap_siege(&self) -> MinimapSiege {
        MinimapSiege { unit: self.unit(), input: self.siege_input_with(false), layout: self.current_siege_layout(), zones: self.deployment_zones() }
    }
}

impl TileDocument {
    /// Changes whenever the tile is edited, undone or redone.
    pub fn edit_version(&self) -> u64 {
        use wce_editor_core::command::History as _;
        self.history.version()
    }
}

/// Land at `t` (0 the lowest, 1 the highest): green lowlands, olive, brown hills, grey-white tops.
fn land_colour(t: f64) -> [f64; 3] {
    const STOPS: [(f64, [f64; 3]); 5] = [(0.0, [72.0, 112.0, 62.0]), (0.3, [128.0, 140.0, 80.0]), (0.6, [140.0, 112.0, 76.0]), (0.85, [150.0, 140.0, 130.0]), (1.0, [235.0, 235.0, 235.0])];
    let t = t.clamp(0.0, 1.0);
    let k = STOPS.iter().position(|s| s.0 >= t).unwrap_or(STOPS.len() - 1).max(1);
    let ((t0, a), (t1, b)) = (STOPS[k - 1], STOPS[k]);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]
}

/// Water `depth` metres deep: lighter by the shore.
fn sea_colour(depth: f64) -> [f64; 3] {
    let f = (depth / 30.0).clamp(0.0, 1.0);
    [70.0 - 40.0 * f, 130.0 - 60.0 * f, 170.0 - 40.0 * f]
}
