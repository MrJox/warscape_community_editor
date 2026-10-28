//! The water depth view (Phase 4d of docs/plans/tile_ted_parity.md): stage B's water map (`terrain_mesh::water_fields`, the rasterisation
//! BOB writes `hf_water_map.data` with) of the tile as it is in memory, less its final heights, handed to the document
//! (`Tile::set_water_depth`) while a view shows it, and the automatic no-go areas of the same final heights (`Tile::set_auto_nogos`) while
//! they show; worked out again once the tile has not changed for a moment after an edit.

use std::path::Path;

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::terrain_bake::{TerrainView, WaterDepth};
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::terrain_mesh::water_fields;
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::Blob;

use crate::tool::TileTool;

/// How long the tile must stay unchanged after an edit before the depth is worked out again (seconds).
const SETTLE_S: f64 = 0.6;

impl TileTool {
    /// Per frame: the water depth of the open tile while a view shows it (the viewport's, the minimap's), and the automatic no-go areas
    /// while they show, worked out when the tile changed (and has settled).
    pub(crate) fn update_water_depth(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if d.tile().stroke_open() {
            return;
        }

        let wants_water = d.tile().terrain_look().view == TerrainView::WaterDepth || self.minimap.view() == Some(TerrainView::WaterDepth);
        let wants_nogo = d.tile().display().show_auto_nogo || self.minimap.shows_auto_nogos();
        let version = d.history().version();
        let key = Some((id, version));
        let (water, nogo) = (wants_water && self.water_for != key, wants_nogo && self.nogo_for != key);
        if !water && !nogo {
            return;
        }

        let now = e.clock.time();
        if self.water_seen != key {
            self.water_seen = key;
            self.water_since = now;
        }

        // Shown for this tile already: after an edit, once the tile has stayed as it is for a moment; else at once.
        let shown = |f: Option<(DocumentId, u64)>| f.is_some_and(|(doc, _)| doc == id);
        if (!water || shown(self.water_for)) && (!nogo || shown(self.nogo_for)) && now - self.water_since < SETTLE_S {
            return;
        }

        let game = self.exported_game(e).ok();
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let started = std::time::Instant::now();
        let (depth, nogos) = match stage_b_of(d, game.as_deref(), nogo) {
            Ok((w, n)) => (Some(w), n),
            Err(why) => {
                log::warn(format!("water depth / no-go areas: {why}"));
                (None, None)
            }
        };

        log::info(format!("{} worked out in {:.2} s", if nogo { "water depth and no-go areas" } else { "water depth" }, started.elapsed().as_secs_f64()));
        self.water_for = key;
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        d.tile_mut().set_water_depth(&mut e.scene, depth);
        if nogo {
            self.nogo_for = key;
            d.tile_mut().set_auto_nogos(nogos);
        }
    }
}

/// The tile as it is in memory (its scene, its heights in the range the next save gives them, its entry with that range) through stage B's
/// water map; the water planes' models from the game's packs when they are known.
/// Polygons in metres (tile frame).
type Outlines = Vec<Vec<[f32; 2]>>;

/// With `nogos`, the automatic no-go areas too.
fn stage_b_of(d: &TileDocument, game: Option<&wce_tile_io::attila::workflow::Game>, nogos: bool) -> Result<(WaterDepth, Option<Outlines>), String> {
    use wce_tile::project_files::{TED_FILE, WCE_FILE, to_ted};

    let p = d.project();
    let mut files = TileFiles::from_kit_dir(&p.dir).map_err(|e| e.to_string())?;
    files.folder.remove(WCE_FILE);
    let (ted, _) = to_ted(p.scene.save())?;
    files.folder.insert(TED_FILE.to_owned(), Blob::Bytes(ted.into()));

    if let (Some(field), Some(base)) = (&p.height, &p.height_png) {
        let (min, scale) = field.fit_range();
        let mut png = base.clone();
        png.pixels = field.quantized(min, scale).iter().flat_map(|r| r.to_be_bytes()).collect();
        png.pixels_edited = true;
        let bytes = if png.has_ancillary { png.encode() } else { png.write() }.map_err(|e| e.to_string())?;
        files.folder.insert("height_map_0.png".to_owned(), Blob::Bytes(bytes.into()));
        if let (Some(entry), Some(v)) = (&p.entry, p.variation) {
            let mut entry = entry.clone();
            entry.set_height_range(v, min, scale);
            let name = entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            files.entry = Some((name, Blob::Bytes(entry.save().into())));
        }
    }

    let (raw, _) = TedProject.read(&files).map_err(|e| e.to_string())?;
    let kit_raw = game.and_then(|g| g.kit_raw.clone()).or_else(|| p.raw_root.clone()).unwrap_or_default();
    let none = |_: &str| None;
    let packs = |k: &str| game.and_then(|g| g.files.read(k).ok());
    let models: &dyn Fn(&str) -> Option<Vec<u8>> = if game.is_some() { &packs } else { &none };
    let f = water_fields(&raw, Path::new(&kit_raw), models)?;
    let depth = match &f.water {
        Some(water) => water.iter().zip(&f.heights).map(|(&w, &h)| if w <= -999.0 { f32::NAN } else { w - h }).collect(),
        None => vec![f32::NAN; f.w * f.h],
    };

    // The grid's vertices over the tile without its border, row j along +z.
    let nogos = nogos.then(|| {
        let (sx, sz) = (d.tile().extent_x() as f32 / (f.w - 1).max(1) as f32, d.tile().extent_z() as f32 / (f.h - 1).max(1) as f32);
        f.no_go_outlines(&raw).into_iter().map(|o| o.into_iter().map(|(x, y)| [x * sx, y * sz]).collect()).collect()
    });

    Ok((WaterDepth { w: f.w, h: f.h, depth }, nogos))
}
