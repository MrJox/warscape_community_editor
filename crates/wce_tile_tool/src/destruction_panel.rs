//! The Destruction mode (docs/plans/building_destruction.md): a selected building opened in a panel of its own, the camera framed on it,
//! its damage levels, collapses, materials and effects set by hand, previewed in the viewport as they are made (a preview has the first
//! LOD only, made on a thread a moment after the last change), and on Confirm written as a new building (every LOD) into the mod pack and
//! the TEd polyfill pack (`@wce_proc_data.pack`), the selected instance switched to it (one undo entry).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use rpfm_lib::games::pfh_file_type::PFHFileType;
use wce_destruction::generate::{Generated, Options, generate};
use wce_destruction::params::{Interior, MaterialClass, MaterialOverride, Params, Preset, Volume};
use wce_destruction::source::{Source, Templates};
use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_formats::anim_meta::MetaBody;
use wce_foundation::log;
use wce_foundation::math::Float3;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_tile_editor::tile_document::destruction::{DestructionRequest, VolumeBox};
use wce_vfs::files::Files;

use crate::tool::{TileTool, document_id, selected};

/// How long the parameters are left alone before the preview is made again.
const PREVIEW_DELAY: Duration = Duration::from_millis(400);

/// A generation running on a thread.
struct Job {
    rx: Receiver<Result<Generated, String>>,
    started: Instant,
    revision: u64,
}

/// The building open in the Destruction mode.
pub(crate) struct Session {
    doc: DocumentId,
    elem: usize,
    source: Arc<Source>,
    templates: Arc<Templates>,
    files: Arc<dyn Files>,
    category: String,
    pub(crate) params: Params,

    revision: u64,
    changed: Option<Instant>,
    job: Option<Job>,
    confirm: Option<Job>,
    preview: Option<Arc<Generated>>,
    level: usize,
    volume: (usize, usize),
    status: String,
    took: f32,
}

fn preset_index(p: Preset) -> i32 {
    Preset::ALL.iter().position(|x| *x == p).unwrap_or(0) as i32
}

fn class_index(c: Option<MaterialClass>) -> i32 {
    c.map_or(0, |c| MaterialClass::ALL.iter().position(|x| *x == c).unwrap_or(0) as i32 + 1)
}

impl TileTool {
    /// Opens the Destruction mode on the selected building.
    ///
    /// # Errors
    ///
    /// No tile, no single building selected, or the building has nothing to break.
    pub fn open_destruction(&mut self, e: &mut Editor) -> Result<String, String> {
        let doc = document_id(e).ok_or("no tile is open")?;
        let sel = selected(e);
        let [elem] = sel.as_slice() else { return Err("select one building".into()) };
        let elem = *elem;
        let d = e.docs.find_as::<TileDocument>(doc).ok_or("no tile is open")?;
        if d.tile().tscene().type_of(elem) != "BUILDING_INSTANCE" {
            return Err("the selection is not a building".into());
        }

        let key = d.tile().tscene().doc().get(elem, "key").ok_or("the building has no key")?;
        let world = d.tile().world_matrix(elem);
        let files = e.files().ok_or("no game files are mounted")?;
        let category = crate::tool::walls_schema_for(e).ok().map(|s| wce_destruction::rows::category_of(files.as_ref(), &s, &key)).unwrap_or_default();
        let source = Arc::new(Source::read(files.as_ref(), &key, &category)?);
        let templates = match &self.destruction_templates {
            Some(t) => t.clone(),
            None => {
                let t = Arc::new(Templates::load(files.as_ref()));
                self.destruction_templates = Some(t.clone());
                t
            }
        };

        let base = key.strip_prefix("wce_").unwrap_or(&key);
        let params = Params::defaults(&format!("wce_{base}_destroyed"), &source.bounds, &category);
        let b = source.bounds;
        let centre = world.transform_point(Float3::new(b.centre().x, b.centre().y, b.centre().z));
        let size = b.size().max();
        e.viewport.controller().set_orbit(0.6, 0.35, size * 2.2, centre);

        self.destruction = Some(Session {
            doc, elem, source, templates, files, category, params,
            revision: 0, changed: Some(Instant::now() - PREVIEW_DELAY), job: None, confirm: None, preview: None, level: 0, volume: (0, 0),
            status: String::new(), took: 0.0,
        });
        self.destruction_open = true;
        self.send_volumes(e);
        Ok(format!("destruction: {key}"))
    }

    /// Leaves the Destruction mode: the building's own pieces again.
    pub fn close_destruction(&mut self, e: &mut Editor) {
        if let Some(s) = self.destruction.take() && let Some(d) = e.docs.find_as_mut::<TileDocument>(s.doc) {
            d.tile_mut().destruction_request(DestructionRequest::Clear);
        }

        self.destruction_open = false;
    }

    fn request(&self, e: &mut Editor, r: DestructionRequest) {
        if let Some(s) = &self.destruction && let Some(d) = e.docs.find_as_mut::<TileDocument>(s.doc) {
            d.tile_mut().destruction_request(r);
        }
    }

    fn send_volumes(&self, e: &mut Editor) {
        let Some(s) = &self.destruction else { return };
        let boxes = s.params.levels.iter().enumerate().flat_map(|(k, l)| l.volumes.iter().enumerate().map(move |(i, v)| VolumeBox { centre: v.centre, half: v.half, yaw: v.yaw, selected: (k, i) == s.volume })).collect();
        self.request(e, DestructionRequest::Volumes(boxes));
    }

    /// Plays the collapse into damage level `level` (1 the first).
    pub fn destruction_play(&mut self, e: &mut Editor, level: usize) -> Result<String, String> {
        let n = self.destruction.as_ref().ok_or("no building in the Destruction mode")?.params.levels.len();
        if level == 0 || level > n {
            return Err(format!("no damage level {level} (1 to {n})"));
        }
        self.request(e, DestructionRequest::Play(level));
        Ok(format!("playing the collapse into level {level}"))
    }

    /// Shows a level (0 intact).
    pub fn destruction_show(&mut self, e: &mut Editor, level: usize) -> Result<String, String> {
        let s = self.destruction.as_mut().ok_or("no building in the Destruction mode")?;
        s.level = level.min(s.params.levels.len());
        let l = s.level;
        self.request(e, DestructionRequest::Level(l));
        Ok(format!("showing level {l}"))
    }

    /// Starts making the building for the game (every LOD); [`Self::update_destruction`] writes it when done.
    pub fn destruction_confirm(&mut self, _e: &mut Editor) -> Result<String, String> {
        let s = self.destruction.as_mut().ok_or("no building in the Destruction mode")?;
        if s.confirm.is_some() {
            return Err("already being made".into());
        }
        if !s.params.key.starts_with("wce_") || s.params.key.contains(['/', '\\', ' ']) {
            return Err("the new key must start with wce_ and have no spaces or slashes".into());
        }

        s.confirm = Some(spawn(s, Options::default(), s.params.key.clone()));
        s.status = "making the building (every LOD)...".into();
        Ok("making the building".into())
    }

    /// Per frame: starts the preview when the parameters have rested, takes in finished jobs.
    pub(crate) fn update_destruction(&mut self, e: &mut Editor) {
        let Some(s) = self.destruction.as_mut() else { return };
        if e.docs.find_as::<TileDocument>(s.doc).is_none() {
            self.destruction = None;
            self.destruction_open = false;
            return;
        }

        // A finished preview: shown.
        if let Some(job) = &s.job && let Ok(result) = job.rx.try_recv() {
            let (revision, took) = (job.revision, job.started.elapsed().as_secs_f32());
            s.job = None;
            match result {
                Ok(g) => {
                    s.took = took;
                    s.status = format!("preview {revision}: {} collapse(s), {:.1} s", g.collapses.len(), took);
                    let pieces = s.source.pieces.iter().map(|p| p.number).collect();
                    let req = DestructionRequest::Show { elem: s.elem, key: g.key.clone(), files: g.files.clone(), pieces, level: s.level.min(g.levels - 1) };
                    s.preview = Some(Arc::new(g));
                    if let Some(d) = e.docs.find_as_mut::<TileDocument>(s.doc) {
                        d.tile_mut().destruction_request(req);
                    }
                }
                Err(err) => s.status = format!("preview failed: {err}"),
            }
        }

        if s.job.is_none() && s.confirm.is_none() && s.changed.is_some_and(|t| t.elapsed() >= PREVIEW_DELAY) {
            s.changed = None;
            s.revision += 1;
            let key = format!("wce_destruction_preview_{}", s.revision);
            s.job = Some(spawn(s, Options { lods: 1 }, key));
        }

        // A finished building: written, and the instance switched to it.
        if let Some(job) = &s.confirm && let Ok(result) = job.rx.try_recv() {
            s.confirm = None;
            let msg = match result.and_then(|g| self.write_destroyed(e, g)) {
                Ok(m) => m,
                Err(err) => format!("not written: {err}"),
            };
            log::info(format!("destruction: {msg}"));
            if let Some(s) = self.destruction.as_mut() {
                s.status = msg.clone();
            }
            self.status = msg;
        }
    }

    /// Writes a made building into the mod pack and the polyfill pack, puts it in the editor's files and switches the instance to it.
    fn write_destroyed(&mut self, e: &mut Editor, g: Generated) -> Result<String, String> {
        let s = self.destruction.as_ref().ok_or("the mode was left")?;
        let (doc, elem, files, source) = (s.doc, s.elem, s.files.clone(), s.source.clone());
        let schema = crate::tool::walls_schema_for(e)?;
        let (tables, notes) = wce_destruction::rows::rows(files.as_ref(), &schema, &source.key, &g.key, source.logic().map(Vec::as_slice), g.logic.as_deref(), g.levels)?;
        for n in notes.iter().chain(&g.notes) {
            log::info(format!("destruction {}: {n}", g.key));
        }

        let folder = wce_walls::building::folder(&g.key);
        let pack_files = g.files.iter().map(|(n, b)| (format!("{folder}{n}"), b.clone())).collect::<Vec<_>>();
        let game = e.game().clone();
        let mod_pack = Self::default_mod_pack_for(e);
        let data = e.settings_ref().game_dir().unwrap_or_default().join(game.data_dir);
        let proc_pack: PathBuf = data.join(wce_walls::pack::PROC_PACK);
        wce_destruction::pack::put_building(&mod_pack, game.key, PFHFileType::Mod, &g.key, pack_files.clone(), tables.clone())?;
        let proc_note = match wce_destruction::pack::put_building(&proc_pack, game.key, PFHFileType::Movie, &g.key, pack_files.clone(), tables) {
            Ok(_) => String::new(),
            Err(err) => format!(" (the TEd pack was not written: {err})"),
        };

        for (p, b) in pack_files {
            wce_vfs::generated::put(&p, b);
        }

        let Some(d) = e.docs.find_as_mut::<TileDocument>(doc) else { return Err("the tile was closed".into()) };
        d.tile_mut().destruction_request(DestructionRequest::Clear);
        d.set_attribute(elem, "key", &g.key);
        self.destruction = None;
        self.destruction_open = false;
        Ok(format!("{} ({} levels, {} collapses) written into {}{proc_note}; the building now uses it", g.key, g.levels, g.collapses.len(), mod_pack.display()))
    }

    /// The Destruction window.
    pub(crate) fn draw_destruction(&mut self, e: &mut Editor) {
        if !self.destruction_open {
            return;
        }

        self.update_destruction(e);
        let Some(s) = self.destruction.as_mut() else {
            self.destruction_open = false;
            return;
        };

        let mut open = true;
        gui::set_next_window_size(gui::v2(420.0, 640.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Destruction", Some(&mut open), 0) {
            gui::end();
            if !open {
                self.close_destruction(e);
            }
            return;
        }

        let mut changed = false;
        let mut volumes_changed = false;
        let mut action: Option<Action> = None;
        let b = s.source.bounds;
        gui::text(&format!("{} ({}), {} piece(s), {:.0} x {:.0} x {:.0} m", s.source.key, if s.category.is_empty() { "no row" } else { &s.category }, s.source.pieces.len(), b.size().x, b.size().y, b.size().z));
        gui::input_text("New key", None, &mut s.params.key, 128, 0);
        if gui::is_item_hovered() {
            gui::set_tooltip("The new building's key (and folder): the original is never changed.");
        }

        // Preview and playback.
        gui::separator_text("Preview");
        let n = s.params.levels.len();
        for l in 0..=n {
            if l > 0 {
                gui::same_line();
            }
            let label = if l == 0 { "Intact".to_owned() } else { format!("Level {l}") };
            if gui::radio_button(&label, s.level == l) {
                s.level = l;
                action = Some(Action::Show(l));
            }
        }

        for l in 1..=n {
            if l > 1 {
                gui::same_line();
            }
            if gui::button(&format!("Play {}->{l}", l - 1)) {
                action = Some(Action::Play(l));
            }
        }

        if s.job.is_some() || s.confirm.is_some() {
            gui::same_line();
            gui::spinner(6.0, 2.0, [0.9, 0.7, 0.2, 1.0]);
        }

        gui::text_wrapped(&s.status);
        if let Some(g) = &s.preview {
            for c in &g.collapses {
                gui::text_disabled(&format!("piece{:02} -> level {}: {} fragments, {:.1} s", c.piece, c.level, c.fragments, c.duration));
            }
        }

        // The levels.
        gui::separator_text("Damage levels");
        let mut count = n as i32;
        if gui::slider_int("Damage levels", &mut count, 1, 3) {
            let (bounds, category) = (s.source.bounds, s.category.clone());
            s.params.set_level_count(count as usize, &bounds, &category);
            s.level = s.level.min(count as usize);
            changed = true;
            volumes_changed = true;
        }

        for k in 0..s.params.levels.len() {
            gui::push_id_int(k as i32);
            if gui::collapsing_header(&format!("Level {}###level", k + 1), gui::TREE_DEFAULT_OPEN) {
                let mut preset = preset_index(s.params.levels[k].preset);
                let labels = Preset::ALL.map(Preset::label);
                if gui::combo("Preset", &mut preset, &labels) {
                    let bounds = s.source.bounds;
                    s.params.set_preset(k, Preset::ALL[preset as usize], &bounds);
                    changed = true;
                    volumes_changed = true;
                }

                let l = &mut s.params.levels[k];
                changed |= gui::slider_float("Fragment size (m)", &mut l.fragment_size, 0.4, 6.0, "%.2f");
                let mut max = l.max_fragments as i32;
                if gui::slider_int("Most fragments", &mut max, 1, 200) {
                    l.max_fragments = max as usize;
                    changed = true;
                }
                changed |= gui::slider_float("Ragged edge", &mut l.roughness, 0.0, 1.0, "%.2f");
                changed |= gui::drag_float3("Direction", &mut l.direction, 0.02, "%.2f");
                changed |= gui::slider_float("Force (m/s)", &mut l.force, 0.0, 15.0, "%.1f");
                changed |= gui::slider_float("Spread", &mut l.spread, 0.0, 1.0, "%.2f");
                changed |= gui::slider_float("Stagger (s)", &mut l.stagger, 0.0, 5.0, "%.2f");
                changed |= gui::slider_float("Spin", &mut l.spin, 0.0, 5.0, "%.2f");

                gui::text("Volumes (the building's frame, m)");
                let mut remove = None;
                for i in 0..l.volumes.len() {
                    gui::push_id_int(i as i32);
                    let v = &mut l.volumes[i];
                    let mut vc = false;
                    vc |= gui::drag_float3("Centre", &mut v.centre, 0.05, "%.2f");
                    vc |= gui::drag_float3("Half size", &mut v.half, 0.05, "%.2f");
                    let mut deg = v.yaw.to_degrees();
                    if gui::slider_float("Turn", &mut deg, -180.0, 180.0, "%.0f deg") {
                        v.yaw = deg.to_radians();
                        vc = true;
                    }
                    vc |= gui::checkbox("Ellipsoid", &mut v.ellipsoid);
                    if gui::is_item_active() || vc {
                        s.volume = (k, i);
                    }
                    gui::same_line();
                    if gui::small_button("Remove") {
                        remove = Some(i);
                    }
                    if vc {
                        l.preset = Preset::Custom;
                        changed = true;
                        volumes_changed = true;
                    }
                    gui::separator();
                    gui::pop_id();
                }

                if let Some(i) = remove {
                    l.volumes.remove(i);
                    l.preset = Preset::Custom;
                    changed = true;
                    volumes_changed = true;
                }

                if gui::button("Add a volume") {
                    let c = b.centre();
                    l.volumes.push(Volume { centre: [c.x, b.max.y - 2.0, c.z], half: [2.0, 2.0, 2.0], yaw: 0.0, ellipsoid: true });
                    l.preset = Preset::Custom;
                    changed = true;
                    volumes_changed = true;
                }
            }
            gui::pop_id();
        }

        // The building.
        gui::separator_text("Building");
        let mut interior = match s.params.interior { Interior::Auto => 0, Interior::Solid => 1, Interior::Hollow => 2 };
        if gui::combo("Interior", &mut interior, &["Auto (by kind)", "Solid", "Hollow shell"]) {
            s.params.interior = [Interior::Auto, Interior::Solid, Interior::Hollow][interior as usize];
            changed = true;
        }
        changed |= gui::slider_float("Wall thickness (m)", &mut s.params.thickness, 0.1, 2.0, "%.2f");
        let mut walk = match s.params.walkable { None => 0, Some(true) => 1, Some(false) => 2 };
        if gui::combo("Rubble", &mut walk, &["By category", "Walkable (a breach)", "Blocks units"]) {
            s.params.walkable = [None, Some(true), Some(false)][walk as usize];
            changed = true;
        }
        changed |= gui::slider_float("Rubble amount", &mut s.params.rubble, 0.0, 2.0, "%.2f");
        changed |= gui::slider_float("Longest collapse (s)", &mut s.params.max_duration, 2.0, 15.0, "%.1f");
        let cultures = std::iter::once("Auto").chain(wce_destruction::materials::cultures()).collect::<Vec<_>>();
        let mut culture = s.params.culture.as_deref().and_then(|c| cultures.iter().position(|x| *x == c)).unwrap_or(0) as i32;
        if gui::combo("Culture", &mut culture, &cultures) {
            s.params.culture = (culture > 0).then(|| cultures[culture as usize].to_owned());
            changed = true;
        }
        let mut seed = s.params.seed as i32;
        if gui::input_int("Seed", &mut seed) {
            s.params.seed = seed.max(0) as u64;
            changed = true;
        }
        gui::same_line();
        if gui::small_button("Another") {
            s.params.seed = s.params.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 33;
            changed = true;
        }

        // Materials.
        if gui::collapsing_header("Materials", 0) {
            let mut diffuse = s.source.pieces.iter().flat_map(|p| p.model.lods().first().into_iter().flat_map(|l| l.mesh_blocks().iter().map(|b| wce_destruction::materials::diffuse(b.material())))).collect::<Vec<_>>();
            diffuse.sort();
            diffuse.dedup();
            let classes = std::iter::once("Auto").chain(MaterialClass::ALL.map(MaterialClass::label)).collect::<Vec<_>>();
            for (i, tex) in diffuse.iter().enumerate() {
                gui::push_id_int(i as i32);
                let name = tex.rsplit('/').next().unwrap_or(tex);
                let at = s.params.materials.iter().position(|o| o.diffuse.eq_ignore_ascii_case(tex));
                let mut class = class_index(at.and_then(|i| s.params.materials[i].class));
                gui::set_next_item_width(110.0);
                if gui::combo("##class", &mut class, &classes) {
                    let o = match at {
                        Some(i) => &mut s.params.materials[i],
                        None => {
                            s.params.materials.push(MaterialOverride { diffuse: tex.clone(), ..Default::default() });
                            s.params.materials.last_mut().expect("pushed")
                        }
                    };
                    o.class = (class > 0).then(|| MaterialClass::ALL[class as usize - 1]);
                    changed = true;
                }
                gui::same_line();
                gui::text(name);
                gui::pop_id();
            }
        }

        // Effects.
        if gui::collapsing_header("Effects", 0) {
            for k in 0..s.params.levels.len() {
                gui::push_id_int(k as i32);
                let generated = s.preview.as_ref().and_then(|g| g.collapses.iter().find(|c| c.level == k + 1).map(|c| c.effects.clone()));
                let edited = s.params.effects.get(k).cloned().flatten();
                let mut list = edited.clone().or(generated).unwrap_or_default();
                gui::text(&format!("Collapse into level {}{}", k + 1, if edited.is_some() { " (edited)" } else { "" }));
                let mut remove = None;
                let mut edit = false;
                for (i, entry) in list.iter_mut().enumerate() {
                    gui::push_id_int(i as i32);
                    let what = match &entry.body {
                        MetaBody::Effect { effect, .. } | MetaBody::Volumetric { effect, .. } => effect.clone(),
                        MetaBody::Sound(raw) => format!("sound {:08x}", u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
                    };
                    gui::set_next_item_width(80.0);
                    if gui::drag_float("##t", &mut entry.start, 0.01, 0.0, 30.0, "%.2f s") {
                        entry.end = entry.start;
                        edit = true;
                    }
                    gui::same_line();
                    if gui::small_button("x") {
                        remove = Some(i);
                    }
                    gui::same_line();
                    gui::text(&what);
                    gui::pop_id();
                }
                if let Some(i) = remove {
                    list.remove(i);
                    edit = true;
                }
                if edit {
                    s.params.effects[k] = Some(list);
                    changed = true;
                }
                if edited.is_some() && gui::small_button("Back to the generated effects") {
                    s.params.effects[k] = None;
                    changed = true;
                }
                gui::pop_id();
            }
        }

        gui::separator();
        let busy = s.confirm.is_some();
        gui::begin_disabled(busy);
        if gui::button("Confirm: write the building") {
            action = Some(Action::Confirm);
        }
        gui::end_disabled();
        if gui::is_item_hovered() {
            gui::set_tooltip("Makes the building with every LOD, writes it (its folder and rows) into the mod pack and the TEd pack, and switches the selected instance to it.");
        }
        gui::same_line();
        if gui::button("Cancel") {
            action = Some(Action::Close);
        }

        if changed {
            s.changed = Some(Instant::now());
        }

        gui::end();
        if volumes_changed {
            self.send_volumes(e);
        }

        let result = match action {
            Some(Action::Show(l)) => self.destruction_show(e, l),
            Some(Action::Play(l)) => self.destruction_play(e, l),
            Some(Action::Confirm) => self.destruction_confirm(e),
            Some(Action::Close) => {
                self.close_destruction(e);
                Ok(String::new())
            }
            None => Ok(String::new()),
        };

        if let Err(err) = result {
            self.status = err;
        }

        if !open {
            self.close_destruction(e);
        }
    }
}

enum Action {
    Show(usize),
    Play(usize),
    Confirm,
    Close,
}

/// A generation on a thread, of the session's building and parameters under `key`.
fn spawn(s: &Session, options: Options, key: String) -> Job {
    let (tx, rx) = channel();
    let (source, templates, files) = (s.source.clone(), s.templates.clone(), s.files.clone());
    let mut params = s.params.clone();
    params.key = key;
    std::thread::spawn(move || {
        let exists = |p: &str| files.exists(p);
        let _ = tx.send(generate(&source, &params, &templates, &exists, options));
    });
    Job { rx, started: Instant::now(), revision: s.revision }
}
