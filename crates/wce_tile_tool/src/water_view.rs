//! The water depth view (Phase 4d of docs/plans/tile_ted_parity.md): stage B's water map (`terrain_mesh::water_fields`, the rasterisation
//! BOB writes `hf_water_map.data` with) of the tile as it is in memory, less its final heights, handed to the document
//! (`Tile::set_water_depth`) while a view shows it, and the automatic no-go areas of the same final heights (`Tile::set_auto_nogos`) while
//! they show; worked out again once the tile has not changed for a moment after an edit.

use wce_editor::editor::Editor;
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::terrain_bake::{TerrainView, WaterDepth};
use wce_tile_editor::tile_document::{Job, JobPoll, TileDocument};
use wce_tile_io::attila::TedProject;
use wce_tile_io::attila::terrain_mesh::{WaterFields, water_fields};
use wce_tile_io::{TileFiles, TileFormat};
use wce_tile_model::Blob;

use crate::tool::TileTool;

/// How long the tile must stay unchanged after an edit before the depth is worked out again (seconds).
const SETTLE_S: f64 = 0.6;

impl TileTool {
    /// Per frame: the water depth of the open tile while a view shows it (the viewport's, the minimap's), and the automatic no-go areas
    /// while they show, worked out when the tile changed (and has settled). Stage B runs off the editor's thread (it took 1.5 s on a port,
    /// after every edit while the radar showed), one at a time; what it gives is put in when it is done.
    pub(crate) fn update_water_depth(&mut self, e: &mut Editor, id: DocumentId) {
        self.finish_water_job(e);
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        if d.tile().stroke_open() {
            return;
        }

        let wants_water = d.tile().terrain_look().view == TerrainView::WaterDepth
            || self.minimap.view() == Some(TerrainView::WaterDepth)
            || self.minimap.wants_radar();
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

        // Shown for this tile already: after an edit, once the tile has stayed as it is for a moment; else at once. The one running
        // ends first.
        let shown = |f: Option<(DocumentId, u64)>| f.is_some_and(|(doc, _)| doc == id);
        if self.water_job.is_some()
            || (!water || shown(self.water_for)) && (!nogo || shown(self.nogo_for)) && now - self.water_since < SETTLE_S
        {
            return;
        }

        let game = self.exported_game(e).ok();
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let request = StageBRequest::of(d, game.as_deref());
        let job = Job::spawn("stage B", move || request.run(game.as_deref(), nogo));
        self.water_job = Some(WaterJob { job, key, nogo, started: std::time::Instant::now() });
    }

    /// The stage B that ended: its depth and no-go areas handed to its tile (a newer version of it gets its own once it settles).
    fn finish_water_job(&mut self, e: &mut Editor) {
        let Some(j) = self.water_job.as_ref() else { return };
        let result = match j.job.poll() {
            JobPoll::Running => return,
            JobPoll::Done(r) => r,
            JobPoll::Failed => Err("it stopped".to_owned()),
        };

        let WaterJob { key, nogo, started, .. } = self.water_job.take().expect("a job");
        let (depth, nogos) = match result {
            Ok((w, n)) => (Some(w), n),
            Err(why) => {
                log::warn(format!("water depth / no-go areas: {why}"));
                (None, None)
            }
        };

        log::info(format!(
            "{} worked out in {:.2} s",
            if nogo { "water depth and no-go areas" } else { "water depth" },
            started.elapsed().as_secs_f64()
        ));
        let Some(d) = key.and_then(|(id, _)| e.docs.find_as_mut::<TileDocument>(id)) else { return };
        self.water_for = key;
        d.tile_mut().set_water_depth(&mut e.scene, depth);
        if nogo {
            self.nogo_for = key;
            d.tile_mut().set_auto_nogos(nogos);
        }
    }
}

/// Stage B running for the water depth (`TileTool::update_water_depth`).
pub(crate) struct WaterJob {
    job: Job<Result<(WaterDepth, Option<Outlines>), String>>,
    key: Option<(DocumentId, u64)>,
    nogo: bool,
    started: std::time::Instant,
}

impl TileTool {
    /// The automatic no-go areas of the open tile as it is now, worked out unless the view has them for this version already (Generate
    /// SiegeAI keeps its streets off the steep ground they cover).
    ///
    /// # Errors
    ///
    /// Stage B failed.
    pub(crate) fn refresh_auto_nogos(&mut self, e: &mut Editor, id: DocumentId) -> Result<(), String> {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return Err("no tile".to_owned()) };
        let key = Some((id, d.history().version()));
        if self.nogo_for == key && d.tile().auto_nogos().is_some() {
            return Ok(());
        }

        let game = self.exported_game(e).ok();
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (_, nogos) = stage_b_of(d, game.as_deref(), true)?;
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        d.tile_mut().set_auto_nogos(nogos);
        self.nogo_for = key;
        Ok(())
    }
}

impl TileTool {
    /// The water depth and the automatic no-go areas of the open tile worked out now when it has none (the radar's rivers and cliffs).
    ///
    /// # Errors
    ///
    /// No tile, or stage B failed.
    pub(crate) fn ensure_radar_water(&mut self, e: &mut Editor, id: DocumentId) -> Result<(), String> {
        let d = e.docs.find_as::<TileDocument>(id).ok_or("no tile")?;
        if d.tile().water_depth().is_some() && d.tile().auto_nogos().is_some() {
            return Ok(());
        }

        let game = self.exported_game(e).ok();
        let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
        let (water, nogos) = stage_b_of(d, game.as_deref(), true)?;
        let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
        d.tile_mut().set_water_depth(&mut e.scene, Some(water));
        d.tile_mut().set_auto_nogos(nogos);
        Ok(())
    }
}

/// What stage B of the tile as it is in memory needs, copied from it (cheap: [`StageBRequest::run`] does the reading and the work, on
/// any thread).
pub struct StageBRequest {
    dir: std::path::PathBuf,
    scene: Vec<u8>,
    height: Option<(wce_tile::height_field::HeightField, wce_formats::png_file::PngFile)>,
    entry: Option<(wce_formats::tile_entry::TileEntry, usize)>,
    roots: Vec<std::path::PathBuf>,
    extent: (f64, f64),
}

impl StageBRequest {
    /// The tile as it is in memory: its scene, its heights, its entry, and the raw_data folders as the export reads them (the tile's own
    /// raw_data first, then the kits').
    pub fn of(d: &TileDocument, game: Option<&wce_tile_io::attila::workflow::Game>) -> Self {
        let p = d.project();
        let roots = match (game, &p.raw_root) {
            (Some(g), Some(raw)) => g.bob_roots(raw),
            (Some(g), None) => g.kits.clone(),
            (None, raw) => raw.iter().cloned().collect(),
        };

        Self {
            dir: p.dir.clone(),
            scene: p.scene.save(),
            height: p.height.clone().zip(p.height_png.clone()),
            entry: p.entry.clone().zip(p.variation),
            roots,
            extent: (d.tile().extent_x(), d.tile().extent_z()),
        }
    }

    /// Stage B's water map of the tile less its final heights (its heights in the range the next save gives them, its entry with that
    /// range); the water planes' models from the game's packs when they are known. With `nogos`, the automatic no-go areas too.
    ///
    /// # Errors
    ///
    /// The tile's folder or scene does not read, or stage B failed.
    pub fn run(self, game: Option<&wce_tile_io::attila::workflow::Game>, nogos: bool) -> Result<(WaterDepth, Option<Outlines>), String> {
        let extent = self.extent;
        let (f, raw) = self.fields(game)?;
        Ok(depth_and_nogos(&f, &raw, nogos, extent))
    }

    /// Stage B's fields of the tile as [`Self::run`] reads it, and the raw tile they were made from.
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    pub fn fields(self, game: Option<&wce_tile_io::attila::workflow::Game>) -> Result<(WaterFields, wce_tile_model::Tile), String> {
        use wce_tile::project_files::{TED_FILE, WCE_FILE, to_ted};

        let mut files = TileFiles::from_kit_dir(&self.dir).map_err(|e| e.to_string())?;
        files.folder.remove(WCE_FILE);
        let (ted, _) = to_ted(self.scene)?;
        files.folder.insert(TED_FILE.to_owned(), Blob::Bytes(ted.into()));

        if let Some((field, mut png)) = self.height {
            let (min, scale) = field.fit_range();
            png.pixels = field.quantized(min, scale).iter().flat_map(|r| r.to_be_bytes()).collect();
            png.pixels_edited = true;
            let bytes = if png.has_ancillary { png.encode() } else { png.write() }.map_err(|e| e.to_string())?;
            files.folder.insert("height_map_0.png".to_owned(), Blob::Bytes(bytes.into()));
            if let Some((mut entry, v)) = self.entry {
                entry.set_height_range(v, min, scale);
                let name = entry.file().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                files.entry = Some((name, Blob::Bytes(entry.save().into())));
            }
        }

        let none = |_: &str| None;
        let packs = |k: &str| game.and_then(|g| g.files.read(k).ok());
        let models: &dyn Fn(&str) -> Option<Vec<u8>> = if game.is_some() { &packs } else { &none };
        stage_b_fields_of_files(&files, &self.roots, models)
    }
}

/// Polygons in metres (tile frame).
pub type Outlines = Vec<Vec<[f32; 2]>>;

/// [`StageBRequest::run`] of the tile as it is now, on this thread.
///
/// # Errors
///
/// As [`StageBRequest::run`].
pub fn stage_b_of(
    d: &TileDocument,
    game: Option<&wce_tile_io::attila::workflow::Game>,
    nogos: bool,
) -> Result<(WaterDepth, Option<Outlines>), String> {
    StageBRequest::of(d, game).run(game, nogos)
}

/// Metres of water over each vertex of stage B's grid; NaN where there is none.
pub(crate) fn water_depth(f: &WaterFields) -> Vec<f32> {
    match &f.water {
        Some(water) => water.iter().zip(&f.heights).map(|(&w, &h)| if w <= -999.0 { f32::NAN } else { w - h }).collect(),
        None => vec![f32::NAN; f.w * f.h],
    }
}

/// Metres between the vertices of stage B's grid over a tile of `extent` without its border (metres), along x and z.
fn spacing(f: &WaterFields, extent: (f64, f64)) -> (f32, f32) {
    (extent.0 as f32 / (f.w - 1).max(1) as f32, extent.1 as f32 / (f.h - 1).max(1) as f32)
}

/// The water depth of stage B's fields, and with `nogos` the automatic no-go areas (in metres, the tile's frame).
fn depth_and_nogos(f: &WaterFields, raw: &wce_tile_model::Tile, nogos: bool, extent: (f64, f64)) -> (WaterDepth, Option<Outlines>) {
    // The grid's vertices over the tile without its border, row j along +z.
    let nogos = nogos.then(|| {
        let (sx, sz) = spacing(f, extent);
        f.no_go_outlines(raw).into_iter().map(|o| o.into_iter().map(|(x, y)| [x * sx, y * sz]).collect()).collect()
    });

    (WaterDepth { w: f.w, h: f.h, depth: water_depth(f) }, nogos)
}

/// A project folder's files as stage B reads them: its tile.wce as TEd's tile.ted (a combined project's variation written as a project of
/// its own, `TileDocument::materialize_variations`).
///
/// # Errors
///
/// The folder or its scene does not read.
pub fn project_files_for_stage_b(dir: &std::path::Path) -> Result<TileFiles, String> {
    use wce_tile::project_files::{TED_FILE, WCE_FILE, to_ted};

    let mut files = TileFiles::from_kit_dir(dir).map_err(|e| e.to_string())?;
    if let Some(wce) = files.folder.remove(WCE_FILE) {
        let (ted, _) = to_ted(wce.bytes().map_err(|e| e.to_string())?.to_vec())?;
        files.folder.insert(TED_FILE.to_owned(), Blob::Bytes(ted.into()));
    }

    Ok(files)
}

/// [`stage_b_of`] of a project's files: `roots` the raw_data folders the linked tiles are read from (the tile's own first), `models` the
/// water planes' models from the packs; `extent` the tile's size without its border (metres).
///
/// # Errors
///
/// The project does not read, or stage B failed.
pub fn stage_b_of_files(
    files: &TileFiles,
    roots: &[std::path::PathBuf],
    models: &dyn Fn(&str) -> Option<Vec<u8>>,
    nogos: bool,
    extent: (f64, f64),
) -> Result<(WaterDepth, Option<Outlines>), String> {
    let (f, raw) = stage_b_fields_of_files(files, roots, models)?;
    Ok(depth_and_nogos(&f, &raw, nogos, extent))
}

/// Stage B's fields of a project's files (as [`stage_b_of_files`]) and the raw tile they were made from.
///
/// # Errors
///
/// The project does not read, or stage B failed.
fn stage_b_fields_of_files(
    files: &TileFiles,
    roots: &[std::path::PathBuf],
    models: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<(WaterFields, wce_tile_model::Tile), String> {
    let (raw, _) = TedProject.read(files).map_err(|e| e.to_string())?;
    let f = water_fields(&raw, roots, models)?;
    Ok((f, raw))
}
