//! A tile as the Prefab tool's stage (docs/plans/prefab_tool.md): a flat tile holding one group, the prefab's, marked `wce_stage_root`. The
//! prefab's height layer is a layer of the user's on it (`STAGE_LAYER`), the working one; its footprint is what is painted on its ground
//! type map (any ground type). What the stage holds becomes the prefab's layer again ([`TileDocument::stage_terrain`]): the layer's heights
//! and, as coverage, the footprint and wherever the layer holds a height, in the group's frame.

use std::collections::BTreeMap;

use wce_tile::prefab_terrain::{Grid, Placement, PrefabTerrain};
use wce_tile::terrain_layers::{Ground, Kind, Stack, TerrainLayers};
use wce_tile::world_transform::world_transform;

use super::TileDocument;

pub use wce_tile::prefab_stage::STAGE_ATTR;

/// The stage's layer of the prefab's heights.
pub const STAGE_LAYER: &str = "Prefab ground";

/// A height that is there (below it, the layer holds nothing).
const SOME_HEIGHT: f32 = 0.001;

impl TileDocument {
    /// The stage's group: the prefab.
    #[must_use]
    pub fn stage_group(&self) -> Option<usize> {
        let sc = self.tile().tscene();
        sc.find_all("UserInstanceGroup").into_iter().find(|&g| sc.doc().get(g, STAGE_ATTR).is_some_and(|v| v == "true"))
    }

    /// The stage's layer of the prefab's heights.
    #[must_use]
    pub fn stage_layer(&self) -> Option<u64> {
        let s = self.project().layers.as_ref()?.height.as_ref()?;
        s.layers.iter().find(|l| l.owner.is_empty() && l.name == STAGE_LAYER).map(|l| l.id)
    }

    /// Sets the stage up for a prefab with this height layer (none: a new one), before any edit: its layer, the working one, holding the
    /// heights, and its footprint painted where it covers. Not an undo entry: the stage starts so.
    ///
    /// # Errors
    ///
    /// The stage has no group or no height map.
    pub fn begin_stage(&mut self, terrain: Option<&PrefabTerrain>) -> Result<(), String> {
        let group = self.stage_group().ok_or("the stage has no prefab group")?;
        let w = world_transform(self.tile().tscene(), group);
        let at = Placement::from_transform(w.position, &w.rotation);
        let t = self.tile_mut();
        t.stage_layer_last = true;
        let grid = Grid::of(&t.project).ok_or("the stage has no height map")?;
        let p = &mut t.project;
        if terrain.is_some() {
            p.load_ground()?;
        }

        let hf = p.height.as_mut().ok_or("the stage has no height map")?;
        let layers = p.layers.get_or_insert_with(TerrainLayers::default);
        let id = layers.new_id();
        let height = layers.height.get_or_insert_with(|| Stack::new(hf));
        height.working = Some(height.add(id, STAGE_LAYER, ""));

        if let Some(terrain) = terrain {
            // The heights, as the layer adds them.
            let mut flat = terrain.clone();
            flat.normalize = false;
            let mut out = BTreeMap::new();
            flat.rasterize(&at, &grid, None, &|_, _| 0.0, &mut out);
            for ((x, y), v) in out {
                height.put(id, x, y, &[v]);
            }

            height.apply_all(hf);

            // The footprint: what it covers, at half or more.
            if let Some(gf) = p.ground.as_mut() {
                let (gw, gh) = (gf.width(), gf.height());
                let ground_grid = Grid { width: gw, height: gh, origin: grid.origin, spacing: [grid.spacing[0] * f64::from(grid.width) / f64::from(gw), grid.spacing[1] * f64::from(grid.height) / f64::from(gh)] };
                let mut cover = terrain.clone();
                (cover.heights, cover.normalize) = (terrain.coverage.clone(), false);
                let mut out = BTreeMap::new();
                cover.rasterize(&at, &ground_grid, None, &|_, _| 0.0, &mut out);
                let [r, g, b] = wce_tile::ground_types::GROUND_TYPES[FOOTPRINT_GROUND].1;
                for ((x, y), c) in out {
                    if c >= 0.5 {
                        Ground::write(gf, x, y, &[r, g, b, 255]);
                    }
                }
            }
        }

        if let Some(s) = p.layers.as_mut().and_then(|l| l.height.as_mut()) {
            s.remember();
        }

        if let Some(l) = p.layers.as_mut() {
            l.dirty = false;
        }

        t.layers_shown = true;
        Ok(())
    }

    /// What the stage holds as the prefab's height layer (none when it holds nothing): the stage layer's heights in the group's frame, one
    /// sample a pixel of the stage, covering what is painted on the footprint and wherever there is a height.
    #[must_use]
    pub fn stage_terrain(&self, normalize: bool, feather: f64) -> Option<PrefabTerrain> {
        let group = self.stage_group()?;
        let id = self.stage_layer()?;
        let p = self.project();
        let grid = Grid::of(p)?;
        let stack = p.layers.as_ref()?.height.as_ref()?;
        let w = world_transform(self.tile().tscene(), group);
        let at = Placement::from_transform(w.position, &w.rotation);

        // The stage's rectangle in the group's frame, at the stage's spacing.
        let spacing = grid.spacing[0].min(grid.spacing[1]);
        let (x1, z1) = (grid.origin[0] + f64::from(grid.width - 1) * grid.spacing[0], grid.origin[1] + f64::from(grid.height - 1) * grid.spacing[1]);
        let corners = [(grid.origin[0], grid.origin[1]), (x1, grid.origin[1]), (grid.origin[0], z1), (x1, z1)].map(|(x, z)| at.to_local(x, z));
        let (lx0, lx1) = corners.iter().fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c.0), b.max(c.0)));
        let (lz0, lz1) = corners.iter().fold((f64::MAX, f64::MIN), |(a, b), c| (a.min(c.1), b.max(c.1)));
        let (lx0, lz0) = ((lx0 / spacing).ceil() * spacing, (lz0 / spacing).ceil() * spacing);
        let (tw, th) = ((((lx1 - lx0) / spacing).floor() as u32 + 1).min(4096), (((lz1 - lz0) / spacing).floor() as u32 + 1).min(4096));
        let mut out = PrefabTerrain::empty([lx0, lz0], spacing, tw, th);
        (out.normalize, out.feather) = (normalize, feather);

        // Bilinear over the stage's pixels.
        let layer_at = |x: u32, y: u32| {
            let mut px = [0.0f32];
            if stack.get(id, x, y, &mut px) { px[0] } else { 0.0 }
        };

        let ground = p.ground.as_ref();
        let footprint_at = |sx: f64, sz: f64| {
            let Some(gf) = ground else { return false };
            let (u, v) = ((sx - grid.origin[0]) / (grid.spacing[0] * f64::from(grid.width) / f64::from(gf.width())), (sz - grid.origin[1]) / (grid.spacing[1] * f64::from(grid.height) / f64::from(gf.height())));
            let (x, y) = (u.round(), v.round());
            if x < 0.0 || y < 0.0 || x >= f64::from(gf.width()) || y >= f64::from(gf.height()) {
                return false;
            }

            let mut px = [0u8; 4];
            Ground::read(gf, x as u32, y as u32, &mut px);
            px[..3] != [0, 0, 0]
        };

        for j in 0..th {
            for i in 0..tw {
                let (lx, lz) = (lx0 + f64::from(i) * spacing, lz0 + f64::from(j) * spacing);
                let (sx, sz) = at.to_scene(lx, lz);
                let (u, v) = ((sx - grid.origin[0]) / grid.spacing[0], (sz - grid.origin[1]) / grid.spacing[1]);
                if u < 0.0 || v < 0.0 || u > f64::from(grid.width - 1) || v > f64::from(grid.height - 1) {
                    continue;
                }

                let (x0, y0) = (u.floor() as u32, v.floor() as u32);
                let (x1, y1) = ((x0 + 1).min(grid.width - 1), (y0 + 1).min(grid.height - 1));
                let (fx, fy) = ((u - f64::from(x0)) as f32, (v - f64::from(y0)) as f32);
                let top = layer_at(x0, y0) * (1.0 - fx) + layer_at(x1, y0) * fx;
                let bottom = layer_at(x0, y1) * (1.0 - fx) + layer_at(x1, y1) * fx;
                let h = top * (1.0 - fy) + bottom * fy;
                let k = (j * tw + i) as usize;
                if h.abs() >= SOME_HEIGHT {
                    out.heights[k] = h;
                    out.coverage[k] = 1.0;
                }

                if footprint_at(sx, sz) {
                    out.coverage[k] = 1.0;
                }
            }
        }

        out.cropped()
    }
}

/// The outline of where the stage's prefab covers (orange) and, normalized, of how far its feather reaches (yellow).
const COVER_RGBA: u32 = 0xFF00_80FF;
const FEATHER_RGBA: u32 = 0xFF00_E0FF;

impl TileDocument {
    /// Shows on the stage where the prefab's ground covers and, normalized, how far its feather reaches: outlines on the terrain, found again
    /// when `key` changes (none: no outlines).
    pub fn show_stage_outline(&mut self, scene: &mut wce_scene::render_scene::RenderScene, normalize: bool, feather: f64, key: Option<u64>) {
        // Not while a stroke goes on: its edit reaches the layer when it ends.
        if (key.is_some() && key == self.tile().stage_outline.1) || self.tile().stroke_open {
            return;
        }

        let lines = key.and_then(|_| self.stage_outline_lines(normalize, feather));
        let t = self.tile_mut();
        if t.stage_outline.0 != 0 {
            scene.remove(t.stage_outline.0);
            t.stage_outline.0 = 0;
        }

        t.stage_outline.1 = key;
        let Some(lines) = lines.filter(|l| !l.is_empty()) else { return };
        let o = wce_scene::render_scene::Overlay { lines, depth_test: false, pickable: false, ..Default::default() };
        let source = t.source(super::TERRAIN);
        t.stage_outline.0 = scene.add(t.layer, wce_foundation::math::Float4x4::IDENTITY, wce_scene::render_scene::Payload::Overlay(o), source);
    }

    /// The outlines' lines: the edges between samples in and out of each area, on the terrain.
    fn stage_outline_lines(&self, normalize: bool, feather: f64) -> Option<Vec<wce_scene::render_scene::OverlayVertex>> {
        let group = self.stage_group()?;
        let terrain = self.stage_terrain(true, feather)?;
        let w = world_transform(self.tile().tscene(), group);
        let at = Placement::from_transform(w.position, &w.rotation);
        let unit = self.tile().unit() as f32;
        let (origin, gw, gh, weights) = terrain.weight_grid(Some(feather));
        let s = terrain.spacing;
        let point = |x: f64, z: f64| {
            let (sx, sz) = at.to_scene(x, z);
            let (wx, wz) = (sx as f32 / unit, sz as f32 / unit);
            wce_foundation::math::Float3::new(wx, self.tile().terrain_height(wx, wz) + 0.3, wz)
        };

        let mut out = wce_scene::render_scene::Overlay::default();
        let mut outline = |inside: &dyn Fn(f32) -> bool, rgba: u32| {
            let at_ij = |i: i64, j: i64| if i < 0 || j < 0 || i >= i64::from(gw) || j >= i64::from(gh) { 0.0 } else { weights[(j * i64::from(gw) + i) as usize] };
            for j in -1..=i64::from(gh) {
                for i in -1..=i64::from(gw) {
                    let here = inside(at_ij(i, j));
                    let (x, z) = (origin[0] + i as f64 * s, origin[1] + j as f64 * s);
                    // The edge between this sample's cell and the next one across, then down (cells centred on the samples).
                    if here != inside(at_ij(i + 1, j)) {
                        out.line(point(x + s * 0.5, z - s * 0.5), point(x + s * 0.5, z + s * 0.5), rgba);
                    }

                    if here != inside(at_ij(i, j + 1)) {
                        out.line(point(x - s * 0.5, z + s * 0.5), point(x + s * 0.5, z + s * 0.5), rgba);
                    }
                }
            }
        };

        outline(&|v| v >= 0.999, COVER_RGBA);
        if normalize {
            outline(&|v| v > 0.0, FEATHER_RGBA);
        }

        Some(out.lines)
    }
}

/// The ground type the footprint is painted with (road: it shows well).
pub const FOOTPRINT_GROUND: usize = 8;
