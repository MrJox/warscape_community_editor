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
use wce_scene::render_scene::{Payload, RenderScene, SourceRef};

/// The size of a thumbnail, pixels.
pub const SIZE: u32 = 128;

/// How many frames a thumbnail waits for its models before it is made with what loaded (or given up when nothing did).
const MAX_WAIT_FRAMES: u32 = 600;

/// Renders of a thumbnail once its models are in (the exposure settles).
const SETTLE_RENDERS: u32 = 6;

/// How long the renderer stays without work before it is dropped (seconds).
const IDLE_DROP_S: f64 = 20.0;

/// The direction the camera looks from: south-east of the model and 40 degrees above it (24 before: what is wide and low, decals above
/// all, was seen too flat).
const VIEW_FROM: [f32; 3] = [0.75, 1.05, -1.0];

/// The sun is put behind the camera, this far round to its side (degrees), and at least this high: under the weather's own sun a model
/// was often seen from its dark side. Its side gives the faces some shading.
const SUN_TURN: f32 = 35.0;
const SUN_LEAST_ELEVATION: f32 = 35.0;

/// Goes into a thumbnail's file name: the ones made under another light, or without the ground, are not taken for these.
const LOOK: &str = "sun_behind_grey_ground_higher_view";

/// The ground under what a thumbnail shows: grey, so that what is pale, small or see-through (effects, decals) shows against it. As
/// wide as this many times the framed radius.
const GROUND_GREY: u8 = 110;
const GROUND_SPAN: f32 = 8.0;

/// The ground's material id: one for every thumbnail, made once per renderer.
const GROUND_MATERIAL: u64 = u64::MAX - 7;

/// An effect is framed as a sphere this big (metres, times its scale) over where it is: it has no bounds to go by.
const EFFECT_RADIUS: f32 = 1.5;

/// An effect's thumbnail is made after its particles have run this long: the renders step the clock.
const EFFECT_SECONDS: f32 = 2.0;
const EFFECT_STEP: f32 = 0.05;

/// While its models load, a thumbnail is rendered every this many frames (a render asks for what it misses; one every frame cost the
/// editor a whole second renderer's frame for as long as a catalogue's thumbnails were made: user 2026-11-08, 40 fps with the Create
/// panel's grid open). Its ground likewise.
const LOAD_EVERY: u32 = 8;

/// The final renders a frame makes at most (an effect's run of them, 40, went in one frame).
const RENDERS_A_FRAME: u32 = 2;

/// Where a thumbnail is: its models loading, then its ground, then its final renders (one frame's work at a time).
enum Phase {
    Loading,
    Ground { camera: Camera, ground: wce_scene::render_scene::Terrain, waited: u32 },
    Settle { camera: Camera, done: u32, renders: u32, effects: bool },
}

struct Job {
    id: String,
    scene: RenderScene,
    waited: u32,
    phase: Phase,
}

#[derive(Default)]
pub(crate) struct Thumbnails {
    renderer: Option<Renderer>,
    target: RenderTarget,
    queue: VecDeque<(String, Vec<(Float4x4, Payload)>)>,
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
    format!("{clean}_{:016x}.png", fnv(format!("{LOOK}|{id}").as_bytes()))
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The cache folder of a game folder's thumbnails. Rome 2's are in a folder of their own below it: their ground is a skin (`add_ground`);
/// the ones made before on a baked ground came out black, and are not taken.
pub(crate) fn cache_dir(e: &Editor) -> PathBuf {
    let game = e.settings_ref().game_dir().map(|d| d.display().to_string()).unwrap_or_default();
    let dir = e.settings_ref().paths().cache_dir().join("thumbnails").join(format!("{:016x}", fnv(game.to_lowercase().as_bytes())));
    if e.game().key == wce_vfs::GameProfile::ROME_2.key { dir.join("skin_ground") } else { dir }
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
        self.failed.contains(id)
            || self.job.as_ref().is_some_and(|j| j.id == id)
            || self.queue.iter().any(|(q, _)| q == id)
            || self.dir.join(file_name(id)).exists()
    }

    /// The entry's file, when it was made.
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        Some(self.dir.join(file_name(id))).filter(|p| p.exists())
    }

    pub fn failed(&self, id: &str) -> bool {
        self.failed.contains(id)
    }

    /// Queues an entry's models (`first`: before the others, what the mouse is on).
    pub fn request(&mut self, id: &str, parts: Vec<(Float4x4, Payload)>, first: bool) {
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
                    r.settings.apply_graphics_options(&e.graphics_options());
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
            let mut env = e.scene.environment.active.clone().unwrap_or_default();
            env.sun_azimuth = VIEW_FROM[0].atan2(VIEW_FROM[2]).to_degrees() + SUN_TURN;
            env.sun_elevation = env.sun_elevation.max(SUN_LEAST_ELEVATION);
            scene.environment.active = Some(env);
            scene.game_units = e.scene.game_units;
            let layer = scene.add_layer(0, "thumbnail");
            for (m, p) in parts {
                scene.add(layer, m, p, SourceRef::default());
            }

            self.job = Some(Job { id, scene, waited: 0, phase: Phase::Loading });
        }

        let (Some(r), Some(job)) = (self.renderer.as_mut(), self.job.as_mut()) else { return };
        match &mut job.phase {
            // Loading: a render now and then asks for the models.
            Phase::Loading => {
                job.waited += 1;
                if (job.waited - 1) % LOAD_EVERY != 0 {
                    return;
                }

                r.render(&job.scene, &ViewParams { camera: Camera::default(), time_s: 0.0 }, &mut self.target);
                if r.draw_stats().waiting_models > 0 && job.waited < MAX_WAIT_FRAMES {
                    return;
                }

                let Some((camera, floor, radius)) = frame(r, job) else {
                    let job = self.job.take().expect("a job");
                    self.failed.insert(job.id);
                    return;
                };

                // The ground's GPU side is made on a loading thread.
                let ground = add_ground(&mut job.scene, floor, radius, r.render_game() == wce_render::renderer::game::RenderGame::Rome2);
                job.phase = Phase::Ground { camera, ground, waited: 0 };
                return;
            }

            // Its ground: renders now and then until it is there.
            Phase::Ground { camera, ground, waited } => {
                let camera = *camera;
                if !r.terrain_ready(ground) && *waited < MAX_WAIT_FRAMES {
                    if *waited % LOAD_EVERY == 0 {
                        r.render(&job.scene, &ViewParams { camera, time_s: 0.0 }, &mut self.target);
                    }

                    *waited += 1;
                    return;
                }

                r.forget_scene();
                r.restart_temporal();
                let effects = job.scene.entities().iter().any(|en| matches!(en.payload, Payload::VfxEmitter(_)));
                let renders = if effects { (EFFECT_SECONDS / EFFECT_STEP) as u32 } else { SETTLE_RENDERS };
                job.phase = Phase::Settle { camera, done: 0, renders, effects };
                return;
            }

            // The final renders, a few a frame (the exposure settles; an effect's particles run).
            Phase::Settle { camera, done, renders, effects } => {
                let end = (*done + RENDERS_A_FRAME).min(*renders);
                for k in *done..end {
                    let time_s = if *effects { k as f32 * EFFECT_STEP } else { 0.0 };
                    r.render(&job.scene, &ViewParams { camera: *camera, time_s }, &mut self.target);
                }

                *done = end;
                if *done < *renders {
                    return;
                }
            }
        }

        let job = self.job.take().expect("a job");
        let Some(rgba) = self.target.read_rgba8(e.device(), e.context()) else {
            self.failed.insert(job.id);
            return;
        };

        let small = halve(&rgba, SIZE * 2, SIZE * 2);
        let png = PngFile {
            width: SIZE,
            height: SIZE,
            bit_depth: 8,
            color_type: 6,
            pixels: small.clone(),
            pixels_edited: true,
            ..Default::default()
        };
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

/// The grey ground under a thumbnail: a flat terrain at the bottom of what it shows, of one colour. `skin`: coloured by a skin, drawn as the
/// legacy patches (Rome 2's terrain pass has no baked materials: the ground came out black), without the decals on it.
fn add_ground(scene: &mut RenderScene, floor: Float3, radius: f32, skin: bool) -> wce_scene::render_scene::Terrain {
    use wce_scene::render_scene::{Terrain, TerrainImage, TerrainMaterial, TerrainSkin};

    let half = (radius * GROUND_SPAN).max(10.0);
    let layer = scene.add_layer(0, "thumbnail ground");
    let mut world = Float4x4::IDENTITY;
    (world.m[0][3], world.m[1][3], world.m[2][3]) = (floor.x, floor.y - 0.01, floor.z);
    if skin {
        let skin = TerrainSkin {
            x0: f64::from(-half),
            z0: f64::from(-half),
            w_m: f64::from(2.0 * half),
            h_m: f64::from(2.0 * half),
            w: 2,
            h: 2,
            rgba: [GROUND_GREY, GROUND_GREY, GROUND_GREY, 255].repeat(4),
            id: 1,
            dirty: None,
        };
        let ground = Terrain {
            nx: 2,
            nz: 2,
            origin_x: -half,
            origin_z: -half,
            step: 2.0 * half,
            heights: std::sync::Arc::new(vec![0.0; 4]),
            skin: Some(std::sync::Arc::new(skin)),
            ..Default::default()
        };
        scene.add(layer, world, Payload::Terrain(ground.clone()), SourceRef::default());
        return ground;
    }

    // A baked material (one colour, drawn by the terrain pass): the terrain pass marks what it draws as taking decals, which a plain ground
    // is not (the `DECAL_RECEIVER` stencil bit), and a prefab's decals were not seen.
    let grey = [GROUND_GREY, GROUND_GREY, GROUND_GREY, 0];
    let material = TerrainMaterial {
        baked: true,
        x0: f64::from(-half),
        z0: f64::from(-half),
        x1: f64::from(half),
        z1: f64::from(half),
        y0: -5.0,
        y1: 5.0,
        blend: TerrainImage { w: 2, h: 2, rgba: Some(std::sync::Arc::new(grey.repeat(4))) },
        normal: TerrainImage { w: 1, h: 1, rgba: Some(std::sync::Arc::new(vec![255, 127, 0, 127])) },
        id: GROUND_MATERIAL,
        normal_id: GROUND_MATERIAL,
        ..Default::default()
    };
    let ground = Terrain {
        nx: 2,
        nz: 2,
        origin_x: -half,
        origin_z: -half,
        step: 2.0 * half,
        heights: std::sync::Arc::new(vec![0.0; 4]),
        legacy: false,
        material: Some(std::sync::Arc::new(material)),
        ..Default::default()
    };
    scene.add(layer, world, Payload::Terrain(ground.clone()), SourceRef::default());
    ground
}

/// Where the ground is under what a thumbnail shows: at the middle height of its models' origins (a model stands on its origin), else of its
/// decals' (their boxes reach across the ground), else at the origin. The bottom of the bounds was under it wherever a model had foundations,
/// a skirt or a decal box below the ground, and what stood on it floated; the origin alone is not it either: a prefab's parts keep the heights
/// they were saved at (the kit's path prefabs stand 2 m over theirs).
fn ground_height(job: &Job) -> f32 {
    let median = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied()
    };

    let origins = |decal: bool| {
        job.scene
            .entities()
            .iter()
            .filter(|en| match &en.payload {
                Payload::MeshInstance(_) => !decal,
                Payload::Decal(_) => decal,
                _ => false,
            })
            .map(|en| en.world.m[1][3])
            .collect::<Vec<_>>()
    };
    median(origins(false)).or_else(|| median(origins(true))).unwrap_or(0.0)
}

/// The camera that frames the job's models (their bounds once loaded) and effects, the point under their middle at their bottom, and their
/// radius. `None` when nothing loaded.
fn frame(r: &mut Renderer, job: &Job) -> Option<(Camera, Float3, f32)> {
    let mut lo = Float3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Float3::new(f32::MIN, f32::MIN, f32::MIN);
    for en in job.scene.entities() {
        // A decal is a box, the unit one under its matrix.
        if matches!(en.payload, Payload::Decal(_)) {
            for k in 0..8 {
                let c = Float3::new(
                    if k & 1 != 0 { 0.5 } else { -0.5 },
                    if k & 2 != 0 { 0.5 } else { -0.5 },
                    if k & 4 != 0 { 0.5 } else { -0.5 },
                );
                let p = en.world.transform_point(c);
                lo = Float3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
                hi = Float3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
            }

            continue;
        }

        let Payload::MeshInstance(mi) = &en.payload else { continue };
        let Some(m) = r.models().model(&mi.model) else { continue };
        for k in 0..8 {
            let c = Float3::new(
                if k & 1 != 0 { m.bmax.x } else { m.bmin.x },
                if k & 2 != 0 { m.bmax.y } else { m.bmin.y },
                if k & 4 != 0 { m.bmax.z } else { m.bmin.z },
            );
            let p = en.world.transform_point(c);
            lo = Float3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = Float3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
    }

    for en in job.scene.entities() {
        if let Payload::VfxEmitter(v) = &en.payload {
            let p = en.world.transform_point(Float3::new(0.0, 0.0, 0.0));
            let r = EFFECT_RADIUS * v.scale.max(0.2);
            lo = Float3::new(lo.x.min(p.x - r), lo.y.min(p.y), lo.z.min(p.z - r));
            hi = Float3::new(hi.x.max(p.x + r), hi.y.max(p.y + 2.0 * r), hi.z.max(p.z + r));
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
    let camera = Camera {
        eye: centre + d * dist,
        target: centre,
        znear: (dist - radius * 1.5).max(0.05),
        zfar: dist + radius * (GROUND_SPAN + 4.0),
        ..cam
    };
    Some((camera, Float3::new(centre.x, ground_height(job), centre.z), radius))
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
