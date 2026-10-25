//! Thumbnails of the Create panel's entries (Phase 5d of docs/plans/tile_ted_parity.md; wce's, TEd has none): the models an entry places
//! (`Tile::model_parts`, what the placing preview draws), rendered alone under the default weather, seen from above at an angle, and kept as
//! PNGs in the cache folder (`thumbnails/<game folder hash>/`), so that each is made once per game. They are lit by the weather the
//! editor shows when they are made.
//!
//! The rendering has a renderer of its own, made when there is something to render and dropped a while after the last one: the editor's
//! keeps its draw lists, sizes and temporal state. A thumbnail waits for its models to load (a few frames), then renders at twice its size
//! and is box-filtered down.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_editor::images::ImageRef;
use wce_formats::png_file::PngFile;
use wce_foundation::log;
use wce_foundation::math::{Float3, Float4x4, length, normalize};
use wce_render::render_target::RenderTarget;
use wce_render::renderer::{Renderer, ViewParams};
use wce_scene::camera::Camera;
use wce_scene::environment::read_graphics_options;
use wce_scene::render_scene::{MeshInstance, Payload, RenderScene, SourceRef};

/// The size of a thumbnail, pixels.
pub const SIZE: u32 = 128;

/// How many frames a thumbnail waits for its models before it is made with what loaded (or given up when nothing did).
const MAX_WAIT_FRAMES: u32 = 600;

/// Renders of a thumbnail once its models are in (the exposure settles).
const SETTLE_RENDERS: u32 = 6;

/// How long the renderer stays without work before it is dropped (seconds).
const IDLE_DROP_S: f64 = 20.0;

/// The direction the camera looks from: south-east of the model and above it.
const VIEW_FROM: [f32; 3] = [0.75, 0.55, -1.0];

struct Job {
    id: String,
    scene: RenderScene,
    models: Vec<String>,
    waited: u32,
}

#[derive(Default)]
pub(crate) struct Thumbnails {
    renderer: Option<Renderer>,
    target: RenderTarget,
    queue: VecDeque<(String, Vec<(Float4x4, MeshInstance)>)>,
    job: Option<Job>,

    /// Ids with nothing to show (no models, or none loaded).
    failed: HashSet<String>,

    /// Ids whose file was looked for (found or not): not read again.
    looked: HashSet<String>,
    idle_since: f64,

    /// The cache folder for the game the parts came from.
    pub dir: PathBuf,

    /// Made, and asked for, since the queue was last empty (the panel's progress).
    pub made: usize,
    pub asked: usize,
}

/// A file name for an entry id: its letters, with a hash so that ids that differ only in other characters stay apart.
fn file_name(id: &str) -> String {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '.' { c } else { '_' }).take(80).collect();
    format!("{clean}_{:016x}.png", fnv(id.as_bytes()))
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The cache folder of a game folder's thumbnails.
pub(crate) fn cache_dir(e: &Editor) -> PathBuf {
    let game = e.settings_ref().game_dir().map(|d| d.display().to_string()).unwrap_or_default();
    e.settings_ref().paths().cache_dir().join("thumbnails").join(format!("{:016x}", fnv(game.to_lowercase().as_bytes())))
}

fn image_key(id: &str) -> String {
    format!("thumbnail:{id}")
}

impl Thumbnails {
    /// The thumbnail of an entry when it is on the GPU; reads its file the first time (shown from the next frame).
    pub fn get(&mut self, e: &mut Editor, id: &str) -> Option<ImageRef> {
        let key = image_key(id);
        if let Some(r) = e.images.get(&key) {
            return Some(r);
        }

        if self.looked.insert(id.to_owned())
            && let Ok(bytes) = std::fs::read(self.dir.join(file_name(id)))
            && let Ok(png) = PngFile::parse(&bytes)
            && png.bit_depth == 8
            && png.color_type == 6
        {
            e.images.submit(&key, png.width, png.height, png.pixels);
        }

        None
    }

    /// Whether the entry has a file, is queued or is being made.
    pub fn known(&self, id: &str) -> bool {
        self.failed.contains(id) || self.job.as_ref().is_some_and(|j| j.id == id) || self.queue.iter().any(|(q, _)| q == id) || self.dir.join(file_name(id)).exists()
    }

    /// The entry's file, when it was made.
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        Some(self.dir.join(file_name(id))).filter(|p| p.exists())
    }

    pub fn failed(&self, id: &str) -> bool {
        self.failed.contains(id)
    }

    /// Queues an entry's models (`first`: before the others, what the mouse is on).
    pub fn request(&mut self, id: &str, parts: Vec<(Float4x4, MeshInstance)>, first: bool) {
        if self.known(id) {
            return;
        }

        if parts.is_empty() {
            self.failed.insert(id.to_owned());
            return;
        }

        self.asked += 1;
        if first {
            self.queue.push_front((id.to_owned(), parts));
        } else {
            self.queue.push_back((id.to_owned(), parts));
        }
    }

    /// Forgets the queue (the thumbnails made stay).
    pub fn cancel(&mut self) {
        self.queue.clear();
        self.job = None;
        self.made = 0;
        self.asked = 0;
    }

    pub fn busy(&self) -> bool {
        self.job.is_some() || !self.queue.is_empty()
    }

    /// Per frame: works on the current thumbnail (one render while its models load, the final renders once they are in).
    pub fn update(&mut self, e: &mut Editor) {
        let now = e.clock.time();
        if !self.busy() {
            if self.renderer.is_some() && now - self.idle_since > IDLE_DROP_S {
                self.renderer = None;
                self.target = RenderTarget::default();
            }

            self.made = 0;
            self.asked = 0;
            return;
        }

        self.idle_since = now;
        if self.renderer.is_none() {
            let Some(vfs) = e.vfs().cloned() else { return self.cancel() };
            match Renderer::new(e.device(), e.context(), vfs) {
                Ok(mut r) => {
                    r.settings.apply_graphics_options(&read_graphics_options(e.game()));
                    r.settings.deterministic = true;
                    self.renderer = Some(r);
                }

                Err(err) => {
                    log::warn(format!("thumbnails: no renderer ({})", err.0));
                    return self.cancel();
                }
            }
        }

        if !self.target.valid() && !self.target.create(e.device(), SIZE * 2, SIZE * 2) {
            return self.cancel();
        }

        if self.job.is_none() {
            let Some((id, parts)) = self.queue.pop_front() else { return };
            let mut scene = RenderScene::default();
            scene.environment.active = Some(e.scene.environment.active.clone().unwrap_or_default());
            scene.game_units = e.scene.game_units;
            let layer = scene.add_layer(0, "thumbnail");
            let models = parts.iter().map(|(_, mi)| mi.model.clone()).collect();
            for (m, mi) in parts {
                scene.add(layer, m, Payload::MeshInstance(mi), SourceRef::default());
            }

            self.job = Some(Job { id, scene, models, waited: 0 });
        }

        let (Some(r), Some(job)) = (self.renderer.as_mut(), self.job.as_mut()) else { return };
        let params = |camera| ViewParams { camera, time_s: 0.0 };

        // Loading: a render asks for the models.
        r.render(&job.scene, &params(Camera::default()), &mut self.target);
        job.waited += 1;
        if r.draw_stats().waiting_models > 0 && job.waited < MAX_WAIT_FRAMES {
            return;
        }

        let job = self.job.take().expect("a job");
        let Some(camera) = frame(r, &job) else {
            self.failed.insert(job.id);
            return;
        };

        r.restart_temporal();
        for _ in 0..SETTLE_RENDERS {
            r.render(&job.scene, &params(camera), &mut self.target);
        }

        let Some(rgba) = self.target.read_rgba8(e.device(), e.context()) else {
            self.failed.insert(job.id);
            return;
        };

        let small = halve(&rgba, SIZE * 2, SIZE * 2);
        let png = PngFile { width: SIZE, height: SIZE, bit_depth: 8, color_type: 6, pixels: small.clone(), pixels_edited: true, ..Default::default() };
        match png.write() {
            Ok(bytes) => {
                let path = self.dir.join(file_name(&job.id));
                if let Err(err) = std::fs::create_dir_all(&self.dir).and_then(|()| std::fs::write(&path, bytes)) {
                    log::warn(format!("thumbnails: {}: {err}", path.display()));
                }
            }

            Err(err) => log::warn(format!("thumbnails: {err}")),
        }

        e.images.forget(&image_key(&job.id));
        e.images.submit(&image_key(&job.id), SIZE, SIZE, small);
        self.made += 1;
    }
}

/// The camera that frames the job's models (their bounds once loaded), `None` when none loaded.
fn frame(r: &mut Renderer, job: &Job) -> Option<Camera> {
    let mut lo = Float3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Float3::new(f32::MIN, f32::MIN, f32::MIN);
    for (en, path) in job.scene.entities().iter().zip(&job.models) {
        let Some(m) = r.models().model(path) else { continue };
        for k in 0..8 {
            let c = Float3::new(if k & 1 != 0 { m.bmax.x } else { m.bmin.x }, if k & 2 != 0 { m.bmax.y } else { m.bmin.y }, if k & 4 != 0 { m.bmax.z } else { m.bmin.z });
            let p = en.world.transform_point(c);
            lo = Float3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Float3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }

    if lo.x > hi.x {
        return None;
    }

    let centre = (lo + hi) * 0.5;
    let radius = length((hi - lo) * 0.5).max(0.25);
    let cam = Camera::default();
    let d = normalize(Float3::new(VIEW_FROM[0], VIEW_FROM[1], VIEW_FROM[2]));
    // The bounding sphere of the box is loose for long, low models: a little closer than it asks.
    let dist = radius / (cam.fov_y * 0.5).sin() * 0.85;
    Some(Camera { eye: centre + d * dist, target: centre, znear: (dist - radius * 1.5).max(0.05), zfar: dist + radius * 4.0, ..cam })
}

/// Box-filters RGBA8 pixels to half their size, alpha 255.
fn halve(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut out = Vec::with_capacity(w * h);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            for c in 0..3 {
                let at = |xx: usize, yy: usize| u32::from(rgba[(yy * w + xx) * 4 + c]);
                out.push(((at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1) + 2) / 4) as u8);
            }

            out.push(255);
        }
    }

    out
}
