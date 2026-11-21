//! The scene in the 3D viewport (docs/plans/cinematic.md, step 4): a flat battle ground, every unit's men in formation posed by the
//! game's animations in the slot the evaluation gives (`wce_render::unit_animation`, drawn through the game's skinned shaders),
//! the camera following the keys, the weather blended between the keyed environments. Everything is a function of the evaluated
//! frame, so the viewport and the movie export show the same scene for the same time.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use rpfm_lib::schema::Schema;
use wce_cinematic::eval::SceneFrame;
use wce_cinematic::scene::Project;
use wce_editor::editor::Editor;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_render::unit_animation::{UnitInstance, UnitModel};
use wce_render::unit_catalog::{UnitCatalog, formation};
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, blend_environments, read_environment};
use wce_scene::render_scene::DocumentId;
use wce_scene::render_scene::{EntityId, LayerId, Payload, SourceRef, Terrain, VfxEmitter};

/// Soldiers made per unit, slot: the men take them in turn.
const VARIATIONS: u64 = 6;
/// The ground: grass, as the weather tool's sample map has it.
const GROUND: &str = "terrain/textures/battle/default/grass/0_su_diffuse.dds";

type ModelKey = (String, String, String);

#[derive(Default)]
pub struct Preview {
    catalog: Option<Arc<UnitCatalog>>,
    catalog_rx: Option<Receiver<Result<UnitCatalog, String>>>,
    pub catalog_error: Option<String>,
    models: HashMap<ModelKey, Arc<Vec<UnitModel>>>,
    loading: Option<(ModelKey, Receiver<Result<Vec<UnitModel>, String>>)>,
    failed: HashSet<ModelKey>,
    layer: Option<(DocumentId, LayerId)>,
    ground: Option<(EntityId, u32)>,
    men: Vec<EntityId>,
    /// The VFX instances shown: (effect, where, scale) and their emitters.
    vfx: Vec<((String, [i32; 4], u32), EntityId)>,
    environments: HashMap<String, Option<EnvironmentParams>>,
    /// What lights the scene now: the two environments, the blend and the depth of field (rounded), to set it again only on change.
    lit: Option<(String, String, u32, [u32; 3])>,
    /// The viewport's camera is the user's (else it follows the camera keys).
    pub free_camera: bool,
    pub status: String,
}

impl Preview {
    /// Drops everything from the scene (another scene opened, the tool closes its document).
    pub fn clear(&mut self, e: &mut Editor) {
        if let Some((_, layer)) = self.layer.take() {
            e.scene.remove_layer(layer);
        }
        self.ground = None;
        self.men.clear();
        self.vfx.clear();
        self.lit = None;
    }

    pub fn game_changed(&mut self, e: &mut Editor) {
        self.clear(e);
        *self = Self { free_camera: self.free_camera, ..Self::default() };
    }

    fn load_catalog(&mut self, e: &Editor) {
        if self.catalog.is_some() || self.catalog_rx.is_some() || self.catalog_error.is_some() {
            return;
        }
        let Some(vfs) = e.vfs().cloned() else { return };
        let (tx, rx) = channel();
        self.catalog_rx = Some(rx);
        std::thread::spawn(move || {
            let r = wce_foundation::settings::rpfm_schema_path(vfs.profile())
                .ok_or_else(|| "no RPFM schema for the game".to_owned())
                .and_then(|p| Schema::load(&p, None).map_err(|e| e.to_string()))
                .and_then(|schema| UnitCatalog::load(&vfs, &schema));
            let _ = tx.send(r);
        });
    }

    fn poll(&mut self, scripted: bool) {
        if let Some(rx) = &self.catalog_rx {
            let got = if scripted { rx.recv().ok() } else { rx.try_recv().ok() };
            if let Some(r) = got {
                self.catalog_rx = None;
                match r {
                    Ok(c) => self.catalog = Some(Arc::new(c)),
                    Err(err) => self.catalog_error = Some(err),
                }
            }
        }
        if let Some((key, rx)) = &self.loading {
            let got = if scripted { rx.recv().ok() } else { rx.try_recv().ok() };
            if let Some(r) = got {
                let key = key.clone();
                self.loading = None;
                match r {
                    Ok(m) if !m.is_empty() => {
                        self.models.insert(key, Arc::new(m));
                    }
                    Ok(_) => {
                        self.failed.insert(key);
                    }
                    Err(err) => {
                        self.status = format!("{}: {err}", key.0);
                        self.failed.insert(key);
                    }
                }
            }
        }
    }

    fn load_models(&mut self, e: &Editor, key: ModelKey, seed: u64) {
        let (Some(vfs), Some(catalog)) = (e.vfs().cloned(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        let k = key.clone();
        std::thread::spawn(move || {
            let read = |p: &str| vfs.read(p).ok();
            let models: Result<Vec<UnitModel>, String> = (0..VARIATIONS)
                .map(|v| {
                    let mut m = UnitModel::load(&read, &catalog, &k.0, &k.1, seed.wrapping_add(v))?;
                    if k.2 != "STAND" {
                        // A slot the unit's animations don't have keeps its standing pose.
                        let _ = m.play(&read, &k.2);
                    }
                    Ok(m)
                })
                .collect();
            let _ = tx.send(models);
        });
        self.loading = Some((key, rx));
    }

    fn environment(&mut self, e: &Editor, path: &str) -> Option<EnvironmentParams> {
        if path.is_empty() {
            return None;
        }
        self.environments
            .entry(path.to_owned())
            .or_insert_with(|| {
                let bytes = e.vfs()?.read(&path.replace('\\', "/").to_lowercase()).ok()?;
                wce_formats::xml_doc::XmlDoc::parse(bytes).ok().map(|d| read_environment(&d))
            })
            .clone()
    }

    /// The scene at `frame` in the viewport: units, ground, weather and (when following) the camera.
    /// `ground`: the real ground's height at a point (metres; none: the flat grass, at 0).
    pub fn update(
        &mut self,
        e: &mut Editor,
        doc: DocumentId,
        project: &Project,
        frame: &SceneFrame,
        ground: Option<&dyn Fn(f32, f32) -> f32>,
    ) {
        let scripted = e.scripted_input();
        self.load_catalog(e);
        self.poll(scripted);

        let layer = match self.layer {
            Some((d, l)) if d == doc => l,
            _ => {
                self.clear(e);
                let l = e.scene.add_layer(doc, "cinematic preview");
                self.layer = Some((doc, l));
                l
            }
        };
        let units = e.scene.game_units.max(1e-3);

        // The ground: as wide as the playable area (at least 1 km).
        let half = (project.map.playable_area * 0.5).max(1000.0) as u32;
        if ground.is_some() {
            if let Some((id, _)) = self.ground.take() {
                e.scene.remove(id);
            }
        } else if self.ground.is_none_or(|(id, h)| h != half || e.scene.find(id).is_none()) {
            if let Some((id, _)) = self.ground.take() {
                e.scene.remove(id);
            }
            let ground = Terrain::flat(half as f32 / units, GROUND, 8.0 / units);
            let id = e.scene.add(layer, Float4x4::IDENTITY, Payload::Terrain(ground), SourceRef { doc, item: 0 });
            self.ground = Some((id, half));
        }

        // The weather: the keyed environments blended, else the battle's.
        let (from, to, blend) = match &frame.weather {
            Some(w) => (w.from.clone(), w.to.clone(), w.blend),
            None => (project.battle.environment.clone(), String::new(), 0.0),
        };
        let dof =
            frame.dof.as_ref().map_or([0; 3], |d| [d.focus_distance, d.focal_length, d.blur_size].map(|v| (v * 100.0).round() as u32));
        let lit = (from.clone(), to.clone(), (blend * 1000.0).round() as u32, dof);
        if self.lit.as_ref() != Some(&lit) {
            let a = self.environment(e, &from);
            let b = if to.is_empty() || to == from { None } else { self.environment(e, &to) };
            if let Some(a) = a {
                let mut params = match b {
                    Some(b) if blend > 0.0 => blend_environments(&a, &b, blend),
                    _ => a,
                };
                // Keyed depth of field: the game's DOF constants are the battle camera's change_depth_of_field values
                // (focus distance, focal length, blur kernel scale; renderer/post.rs).
                if let Some(d) = &frame.dof {
                    params.use_dof = true;
                    params.dof_focus = [d.focus_distance, d.focal_length];
                    params.dof_blur_size = d.blur_size;
                }
                // At once (a movie renders many frames in one editor frame) and for the editor's own lighting update.
                if let Some(old) = &e.scene.environment.active {
                    (params.output_gamma, params.output_brightness) = (old.output_gamma, old.output_brightness);
                }
                e.scene.environment.active = Some(params.clone());
                e.scene.environment.blend = None;
                e.scene.environment.blend_t = 0.0;
                e.set_weather_params(&format!("cinematic:{from}"), params);
            }
            self.lit = Some(lit);
        }

        // The men: each unit's formation at its place and facing, each man a soldier of the unit's slot, posed at the slot's time.
        let mut figures: Vec<(Float4x4, UnitInstance)> = vec![];
        let mut wanted: Option<(ModelKey, u64)> = None;
        let mut poses: HashMap<(ModelKey, usize), Vec<UnitInstance>> = HashMap::new();
        if let Some(catalog) = self.catalog.clone() {
            for s in frame.units.iter().filter(|s| s.visible && s.men > 0) {
                let Some(u) = project.unit(s.unit) else { continue };
                let faction = project.armies.get(u.army).map_or(String::new(), |a| a.faction.clone());
                let key: ModelKey = (u.unit.clone(), faction.clone(), s.slot.clone());
                let stand: ModelKey = (u.unit.clone(), faction, "STAND".into());
                let models = match self.models.get(&key).or_else(|| self.models.get(&stand)) {
                    Some(m) => m.clone(),
                    None => {
                        if wanted.is_none() && !self.failed.contains(&stand) {
                            wanted = Some((stand, u.seed as u64));
                        }
                        continue;
                    }
                };
                if !self.models.contains_key(&key) && !self.failed.contains(&key) && wanted.is_none() {
                    wanted = Some((key.clone(), u.seed as u64));
                }
                let used = if self.models.contains_key(&key) { key.clone() } else { stand.clone() };
                let Some(land) = catalog.land_units.get(&u.unit) else { continue };
                // A moving slot's clips play by the ground covered (their own travel over their length), so feet don't slide; other
                // slots by the time since they started.
                let anim_t = match natural_speed(&models[0]) {
                    Some(v) if used.2 == s.slot && s.speed > 0.05 => s.slot_distance / v,
                    _ => (frame.t - s.slot_start).max(0.0) as f32,
                };
                let place = translation(Float3::new(s.position[0] / units, 0.0, s.position[1] / units)) * rotation_y(s.facing);
                for (k, [x, z]) in formation(s.men, land.rank_depth, land.spacing).into_iter().enumerate() {
                    let variation = (k * 7) % models.len();
                    let pose = poses.entry((used.clone(), variation)).or_insert_with(|| models[variation].instances(anim_t, true));
                    let mut man = place * translation(Float3::new(x / units, 0.0, z / units));
                    if let Some(h) = ground {
                        man.m[1][3] = h(man.m[0][3] * units, man.m[2][3] * units) / units;
                    }
                    for f in pose.iter() {
                        let mut world = man;
                        let scale = f.scale / units;
                        for row in 0..3 {
                            for col in 0..3 {
                                world.m[row][col] *= scale;
                            }
                        }
                        figures.push((world * f.local, f.clone()));
                    }
                }
            }
        }
        if self.loading.is_none()
            && let Some((key, seed)) = wanted
        {
            self.status = format!("loading {} ({})...", key.0, key.2);
            self.load_models(e, key, seed);
        } else if self.loading.is_none() {
            self.status.clear();
        }

        if self.men.len() != figures.len() || self.men.iter().any(|&id| e.scene.find(id).is_none()) {
            for id in self.men.drain(..) {
                e.scene.remove(id);
            }
            self.men = figures
                .iter()
                .map(|_| e.scene.add(layer, Float4x4::IDENTITY, Payload::CameraRig(Default::default()), SourceRef { doc, item: 1 }))
                .collect();
        }
        for (&id, (world, f)) in self.men.iter().zip(figures) {
            e.scene.update_pose(id, world, f.instance);
        }

        // VFX instances alive at the playhead: an emitter each, made when one starts (particles live on from there; scrubbing back
        // does not rewind them), moved when only its place changes.
        let shown: Vec<&wce_cinematic::eval::VfxState> = frame.vfx.iter().filter(|v| !v.vfx.is_empty()).collect();
        let wanted_vfx: Vec<(String, [i32; 4], u32)> = shown
            .iter()
            .map(|v| {
                let at = [v.position[0], v.position[1], v.position[2], v.rotation_y].map(|x| (x * 100.0).round() as i32);
                (v.vfx.clone(), at, (v.scale * 1000.0).round() as u32)
            })
            .collect();
        if self.vfx.iter().map(|(k, _)| k).ne(wanted_vfx.iter()) || self.vfx.iter().any(|(_, id)| e.scene.find(*id).is_none()) {
            for (_, id) in self.vfx.drain(..) {
                e.scene.remove(id);
            }
            for (k, v) in wanted_vfx.into_iter().zip(&shown) {
                let lift = ground.map_or(0.0, |h| h(v.position[0], v.position[2]));
                let world = translation(Float3::new(v.position[0] / units, (v.position[1] + lift) / units, v.position[2] / units))
                    * rotation_y(v.rotation_y);
                let payload = Payload::VfxEmitter(VfxEmitter { effect: v.vfx.clone(), scale: v.scale / units, mesh: None });
                let id = e.scene.add(layer, world, payload, SourceRef { doc, item: 2 });
                self.vfx.push((k, id));
            }
        }

        if !self.free_camera
            && let Some(c) = &frame.camera
        {
            let cam = camera_of(c, units);
            e.viewport.controller().set_camera(&cam);
            e.viewport.controller().lens_mut().fov_y = cam.fov_y;
        }
    }

    /// True while the units' models or tables are still loading (a movie waits for them).
    pub fn busy(&self) -> bool {
        self.catalog_rx.is_some() || self.loading.is_some()
    }
}

/// How fast a soldier's first clip covers the ground (metres a second at its size): a walk or run cycle's own speed. None for a clip
/// that stays on the spot.
fn natural_speed(m: &UnitModel) -> Option<f32> {
    let seg = m.man.segments.first()?;
    let length = seg.clip.anim.duration();
    let [x, z] = seg.clip.travel(length);
    let metres = (x * x + z * z).sqrt() * m.scale;
    (length > 0.0 && metres > 0.1).then(|| metres / length)
}

/// The renderer's camera for a keyed camera (metres to the scene's units, the FOV from degrees).
pub fn camera_of(c: &wce_cinematic::eval::CameraState, units: f32) -> Camera {
    Camera {
        eye: Float3::new(c.position[0] / units, c.position[1] / units, c.position[2] / units),
        target: Float3::new(c.target[0] / units, c.target[1] / units, c.target[2] / units),
        up: Float3::new(0.0, 1.0, 0.0),
        fov_y: c.fov.to_radians().clamp(0.05, 3.0),
        znear: 1.0 / units,
        zfar: 13000.0 / units,
        ortho_height: None,
    }
}
