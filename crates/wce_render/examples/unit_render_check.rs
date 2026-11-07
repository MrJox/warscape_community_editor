//! The skinned path against the CPU: renders a soldier offscreen, posed, through the game's weighted shaders, and checks the picture against the
//! same vertices skinned on the CPU (`anim_pose`, the rule `unit_pose_check` showed right): the LOD 0 vertices that project into the view must
//! land on pixels the unit changed (97 %: a cut model's open rims, at the neck and wrists, only have back faces from some sides, and culling
//! leaves nothing there), the unit must change no pixel outside those vertices' box, and its shadow must show. Writes one `.ppm` per case.
//!
//!   cargo run --release -p wce_render --example unit_render_check -- <out dir> [environment path in the packs]      (the game folder: the settings or WCE_GAME)
//!
//! `WCE_MODS` (packs, comma separated) mounts mods, `WCE_UNITS` (`unit:faction`, comma separated) runs just those whole units, `WCE_UNIT_FIND` lists
//! the units whose key has a word, `WCE_UNIT_CAM` (eye and target, x,y,z,x,y,z) places the camera, `WCE_UNIT_COUNT`
//! sets how many soldiers (over 5: ranks).

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use rpfm_lib::files::rigidmodel::RigidModel;
use rpfm_lib::files::rigidmodel::vertices::VertexFormat;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_d3d11::*;
use wce_formats::anim::Animation;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::settings::{Settings, rpfm_schema_path};
use wce_render::anim_pose::{inverse_binds, palette, vertex_weights};
use wce_render::files::Files;
use wce_render::gpu_adapter::choose_adapter;
use wce_render::render_target::RenderTarget;
use wce_render::renderer::{Renderer, ViewParams};
use wce_render::unit_animation::UnitModel;
use wce_render::unit_catalog::UnitCatalog;
use wce_scene::camera::Camera;
use wce_scene::environment::read_environment;
use wce_scene::render_scene::{MODEL_SKINNED_SUFFIX, Payload, SKINNED_NO_OFFSET, RenderScene, SkinnedInstance, SkinnedPart, SourceRef, Terrain};
use wce_vfs::{Config, GameProfile, Vfs};

const W: u32 = 512;
const H: u32 = 640;
const MAN: &str = "animations/skeletons/rome_man_game";
const BODY: [&str; 3] = [
    "variantmeshes/_variantmodels/man/skin/attila_main_body.rigid_model_v2",
    "variantmeshes/_variantmodels/man/tunics/roman_tunic_b_cut_01.rigid_model_v2",
    "variantmeshes/_variantmodels/man/helmets/roman_heavy_ridge_helmet.rigid_model_v2",
];

/// A cinematic head: four bones per vertex.
const CINEMATIC: [&str; 1] = ["variantmeshes/_variantmodels/man/skin/att_cine_germanic_06_skin01_cut_2.rigid_model_v2"];
/// A cloak: four bones, over the body's two.
const CLOAK: [&str; 1] = ["variantmeshes/_variantmodels/man/cloak/eastern_overcloak_03.rigid_model_v2"];
/// WeightedDecalDirtmap, alpha-tested: `weightedwithdecaldirt`, with no decal given.
const GREAVES: [&str; 1] = ["variantmeshes/_variantmodels/man/greaves/steppe_leather_greaves.rigid_model_v2"];
/// The whole units: infantry and cavalry, `unit:faction`. `WCE_UNITS` replaces them.
const UNITS: [&str; 2] = ["att_rom_legio_comitatenses:att_fact_western_roman_empire", "att_rom_equites_promoti:att_fact_western_roman_empire"];
const WALK: &str = "animations/rome2/human/battle/sword_and_shield/locomotion/sws_walk_124_cm.anim";

struct Gpu {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
}

fn make_device() -> Option<Gpu> {
    // SAFETY: before other threads; valid arguments.
    unsafe {
        init_platform();
        let adapter = choose_adapter();
        let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        let (mut dev, mut ctx) = (None, None);
        D3D11CreateDevice(adapter.as_ref(), driver, HMODULE::default(), 0, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx)).ok().ok()?;
        Some(Gpu { dev: dev?, ctx: ctx? })
    }
}

fn pose_matrix(t: [f32; 3], q: [f32; 4]) -> Float4x4 {
    let [x, y, z, w] = q;
    let mut m = Float4x4::IDENTITY;
    m.m[0] = [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w), t[0]];
    m.m[1] = [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w), t[1]];
    m.m[2] = [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y), t[2]];
    m
}

/// The skinning matrices of a clip at a time; the bind pose without one.
fn skin_of(vfs: &Vfs, clip: Option<(&str, f32)>) -> Vec<Float4x4> {
    let skeleton = Animation::decode(&vfs.read(&format!("{MAN}.anim")).expect("the skeleton"), 1).expect("a skeleton");
    let binds = inverse_binds(&vfs.read(&format!("{MAN}.bone_inv_trans_mats")).expect("the binds")).expect("binds");
    let locals = match clip {
        Some((path, t)) => {
            let mut a = Animation::decode(&vfs.read(path).expect("the clip"), usize::MAX).expect("a clip");
            a.on_skeleton(&skeleton);
            a.sample(t)
        }

        None => skeleton.frames[0].clone(),
    };

    let mut world: Vec<Float4x4> = Vec::with_capacity(locals.len());
    for (i, (t, q)) in locals.iter().enumerate() {
        let l = pose_matrix(*t, *q);
        world.push(match usize::try_from(skeleton.parents[i]).ok() {
            Some(p) => world[p] * l,
            None => l,
        });
    }

    world.iter().zip(&binds).map(|(w, i)| *w * *i).collect()
}

/// LOD 0's vertices skinned on the CPU.
fn cpu_vertices(vfs: &Vfs, model: &str, skin: &[Float4x4]) -> Vec<Float3> {
    let model = RigidModel::decode(&mut Cursor::new(vfs.read(model).expect("the model")), &Some(DecodeableExtraData::default())).expect("a model");
    let mut out = vec![];
    for block in model.lods()[0].mesh_blocks() {
        let format = *block.material().vertex_format();
        if !matches!(format, VertexFormat::Weighted | VertexFormat::Cinematic) {
            continue;
        }

        for v in block.mesh().vertices() {
            let p = v.position();
            let p = Float3::new(p.x, p.y, p.z);
            let (idx, w) = (*v.bone_indices(), vertex_weights(format == VertexFormat::Weighted, (*v.weights()).into()));
            let mut q = Float3::new(0.0, 0.0, 0.0);
            for k in 0..4 {
                if w[k] > 0.0 {
                    let s = skin[usize::from(idx[k])].transform_point(p);
                    q = Float3::new(q.x + s.x * w[k], q.y + s.y * w[k], q.z + s.z * w[k]);
                }
            }

            out.push(q);
        }
    }

    out
}

/// Renders `n` frames at one time of day and clock: what differs between two calls is the scene.
fn frames(r: &mut Renderer, scene: &RenderScene, cam: &Camera, target: &mut RenderTarget, n: usize) {
    r.settings.deterministic = true;
    for _ in 0..n {
        r.render(scene, &ViewParams { camera: *cam, time_s: 1.0 }, target);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

/// Renders until the parts are loaded.
fn wait_for(r: &mut Renderer, scene: &RenderScene, cam: &Camera, target: &mut RenderTarget, parts: &[&str]) -> Result<(), String> {
    for _ in 0..400 {
        frames(r, scene, cam, target, 1);
        if parts.iter().all(|p| r.models().model(&format!("{p}{MODEL_SKINNED_SUFFIX}")).is_some()) && r.models().pending() == 0 {
            return Ok(());
        }
    }

    Err(format!("the models did not load: {parts:?}"))
}

/// Renders `n` frames with the clock running: the auto exposure adapts by the time between frames, so at one time (`frames`) it stays as the first
/// frame left it, with nothing loaded. The pictures to compare with the game need this one.
fn frames_running(r: &mut Renderer, scene: &RenderScene, cam: &Camera, target: &mut RenderTarget, n: usize, clock: &mut f32) {
    r.settings.deterministic = true;
    for _ in 0..n {
        *clock += 1.0 / 30.0;
        r.render(scene, &ViewParams { camera: *cam, time_s: *clock }, target);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn write_ppm(path: &Path, rgba: &[u8]) {
    write_ppm_sized(path, rgba, W, H);
}

fn write_ppm_sized(path: &Path, rgba: &[u8], w: u32, h: u32) {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend(rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
    std::fs::write(path, out).expect("written");
}

struct Case {
    name: &'static str,
    parts: &'static [&'static str],
    clip: Option<(&'static str, f32)>,
    yaw: f32,
}

/// Whole units from the DB, side by side: `(unit, faction)`, a few soldiers of each, standing, each with his mount. Checks that they load and that
/// the seeds differ, and leaves pictures to compare with the game's. Returns the failures.
fn legion(gpu: &Gpu, vfs: &Arc<Vfs>, files: &Arc<dyn Files>, env: &wce_scene::environment::EnvironmentParams, out: &Path) -> Result<usize, String> {
    let read = |p: &str| vfs.read(p).ok();
    let schema_path = rpfm_schema_path(vfs.profile()).ok_or("no rpfm schema")?;
    let schema = Schema::load(&schema_path, None).map_err(|e| e.to_string())?;
    let catalog = UnitCatalog::load(vfs, &schema)?;
    println!("catalog: {} units, {} land units, {} mounts, {} factions, {} tables", catalog.units().len(), catalog.land_units.len(), catalog.mounts.len(), catalog.colours.len(), catalog.tables.len());
    if let Ok(find) = std::env::var("WCE_UNIT_FIND") {
        let found: Vec<&str> = catalog.units().into_iter().filter(|u| u.contains(&find)).take(30).collect();
        println!("units with '{find}': {found:?}");
    }

    let units: Vec<String> = std::env::var("WCE_UNITS").map_or_else(|_| UNITS.iter().map(|u| (*u).to_owned()).collect(), |v| v.split(',').map(str::to_owned).collect());
    let mut failures = 0;
    for (shot, unit) in units.iter().enumerate() {
        let (unit, faction) = unit.split_once(':').unwrap_or((unit, ""));
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(1, "Unit");
        scene.add(layer, Float4x4::IDENTITY, Payload::Terrain(Terrain::flat(200.0, "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0)), SourceRef { doc: 1, item: 0 });
        scene.environment.active = Some(env.clone());
        let count = std::env::var("WCE_UNIT_COUNT").ok().and_then(|g| g.parse().ok()).unwrap_or(5);
        let mut wanted: Vec<String> = vec![];
        let mut different = std::collections::BTreeSet::new();
        let mut extent = 2.0f32;
        for k in 0..count {
            let model = match UnitModel::load(&read, &catalog, unit, faction, 1000 + k as u64) {
                Ok(m) => m,
                Err(e) => {
                    println!("FAIL: {unit}: {e}");
                    failures += 1;
                    break;
                }
            };

            if k == 0 {
                let e = catalog.entry(unit, faction).expect("the unit");
                println!("{unit}: variant {}, skeleton {}, mount {}, {} parts, clip {} ({} s), missing {:?}", e.variant, model.man.rig.skeleton.skeleton, model.mount.as_ref().map_or("none", |m| m.rig.skeleton.skeleton.as_str()), model.man.parts.parts.len(), model.man.clip.anim.bones.len(), model.man.clip.anim.duration(), model.man.parts.missing);
            }

            wanted.extend(model.models());
            different.insert(model.man.parts.parts.iter().map(|p| p.model.clone()).collect::<Vec<_>>().join("|"));
            if model.mount.is_some() {
                extent = 3.2;
            }

            for (scale, instance) in model.instances(0.3, true) {
                let mut world = Float4x4::IDENTITY;
                for r in 0..3 {
                    world.m[r][r] = scale;
                }

                world.m[0][3] = (k as f32 - (count - 1) as f32 * 0.5) * extent * 0.65;
                world.m[2][3] = (k % 2) as f32 * 1.1;
                if count > 5 {
                    world.m[0][3] = ((k % 8) as f32 - 3.5) * 1.15 + ((k / 8) % 2) as f32 * 0.5;
                    world.m[2][3] = 1.2 - (k / 8) as f32 * 1.4;
                }
                scene.add(layer, world, Payload::SkinnedInstance(instance), SourceRef { doc: 1, item: 1 + k as u64 });
            }
        }

        if wanted.is_empty() {
            continue;
        }

        let (w, h) = (1280, 640);
        let target = Float3::new(0.0, extent * 0.5, 0.0);
        let mut cam = Camera { target, eye: Float3::new(-extent * 0.6, extent * 1.15, extent * 2.6), ..Default::default() };
        if let Ok(view) = std::env::var("WCE_UNIT_CAM") {
            let v: Vec<f32> = view.split(',').filter_map(|c| c.parse().ok()).collect();
            if let [ex, ey, ez, tx, ty, tz] = v[..] {
                (cam.eye, cam.target) = (Float3::new(ex, ey, ez), Float3::new(tx, ty, tz));
            }
        }

        let mut r = Renderer::new(&gpu.dev, &gpu.ctx, files.clone()).map_err(|e| e.0)?;
        let mut rt = RenderTarget::new(&gpu.dev, w, h);
        wanted.sort();
        wanted.dedup();
        let parts: Vec<&str> = wanted.iter().map(String::as_str).collect();
        wait_for(&mut r, &scene, &cam, &mut rt, &parts)?;
        let mut clock = 1.0;
        frames_running(&mut r, &scene, &cam, &mut rt, 150, &mut clock);
        println!("{unit}: tone points {:?}", r.points_readback().0);
        write_ppm_sized(&out.join(format!("unit{shot}.ppm")), &rt.read_rgba8(&gpu.dev, &gpu.ctx).unwrap_or_default(), w, h);
        println!("{unit}: {} of {count} soldiers differ", different.len());
        failures += usize::from(different.is_empty());
    }

    Ok(failures)
}

fn run(out: &Path, env_path: &str) -> Result<usize, String> {
    let gpu = make_device().ok_or("no Direct3D 11 device")?;
    let game = Settings::load_default().game_dir().ok_or("the game folder is not set (in the settings or WCE_GAME)")?;
    let mut config = Config::new(game, GameProfile::from_env().clone());
    config.use_mods = std::env::var("WCE_MODS").is_ok();
    config.mod_packs = std::env::var("WCE_MODS").ok().map(|m| m.split(',').map(str::to_owned).collect());
    let vfs: Arc<Vfs> = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    let files: Arc<dyn Files> = vfs.clone();
    let env_doc = XmlDoc::parse(vfs.read(env_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let env = read_environment(&env_doc);
    let mut failures = 0;
    let only_units = std::env::var("WCE_UNITS").is_ok();
    for case in [
        Case { name: "bind", parts: &BODY, clip: None, yaw: 0.5 },
        Case { name: "walk", parts: &BODY, clip: Some((WALK, 0.3)), yaw: 1.2 },
        Case { name: "cinematic", parts: &CINEMATIC, clip: Some((WALK, 0.3)), yaw: 0.5 },
        Case { name: "cloak", parts: &CLOAK, clip: Some((WALK, 0.3)), yaw: 2.6 },
        Case { name: "greaves", parts: &GREAVES, clip: Some((WALK, 0.3)), yaw: 0.5 },
    ] {
        if only_units {
            break;
        }

        let mut scene = RenderScene::default();
        let layer = scene.add_layer(1, "Unit");
        scene.add(layer, Float4x4::IDENTITY, Payload::Terrain(Terrain::flat(200.0, "terrain/textures/battle/default/grass/0_su_diffuse.dds", 8.0)), SourceRef { doc: 1, item: 0 });
        scene.environment.active = Some(env.clone());
        let target = Float3::new(0.0, 0.95, 0.0);
        let (cp, sp) = (0.2f32.cos(), 0.2f32.sin());
        let cam = Camera { target, eye: target + Float3::new(case.yaw.sin() * cp, sp, -case.yaw.cos() * cp) * 3.2, ..Default::default() };

        let mut r = Renderer::new(&gpu.dev, &gpu.ctx, files.clone()).map_err(|e| e.0)?;
        let mut rt = RenderTarget::new(&gpu.dev, W, H);
        let skin = skin_of(&vfs, case.clip);
        let mut instance = SkinnedInstance {
            parts: case.parts.iter().map(|p| SkinnedPart { model: (*p).to_owned(), tint: [0.0; 4], decal_diffuse: None, decal_normal: None, masks: [0, 1, 2], bone: None, offset: SKINNED_NO_OFFSET }).collect(),
            palette: Arc::new(palette(&skin)), bones: Arc::new(vec![]), casts_shadow: false,
            faction_colours: [[0.7, 0.1, 0.1], [0.8, 0.7, 0.3], [0.2, 0.2, 0.6], [0.0; 3]], faction_scale: 0.0,
        };

        let unit = scene.add(layer, Float4x4::IDENTITY, Payload::SkinnedInstance(instance.clone()), SourceRef { doc: 1, item: 1 });
        wait_for(&mut r, &scene, &cam, &mut rt, case.parts)?;
        frames(&mut r, &scene, &cam, &mut rt, 40);
        let without_shadow = rt.read_rgba8(&gpu.dev, &gpu.ctx).unwrap_or_default();
        instance.casts_shadow = true;
        scene.update_payload(unit, Payload::SkinnedInstance(instance));
        frames(&mut r, &scene, &cam, &mut rt, 40);
        let full = rt.read_rgba8(&gpu.dev, &gpu.ctx).unwrap_or_default();
        write_ppm(&out.join(format!("{}.ppm", case.name)), &full);
        if let Some(e) = scene.find_mut(unit) {
            e.visible = false;
        }

        frames(&mut r, &scene, &cam, &mut rt, 40);
        let empty = rt.read_rgba8(&gpu.dev, &gpu.ctx).unwrap_or_default();
        let differ = |a: &[u8], b: &[u8]| -> Vec<bool> { a.chunks_exact(4).zip(b.chunks_exact(4)).map(|(a, b)| (0..3).any(|k| a[k].abs_diff(b[k]) > 6)).collect() };
        let changed = differ(&without_shadow, &empty);
        let shadow = differ(&full, &without_shadow).iter().filter(|c| **c).count();

        let aspect = W as f32 / H as f32;
        let (mut hit, mut seen) = (0usize, 0usize);
        let (mut lo, mut hi) = ([i32::MAX; 2], [i32::MIN; 2]);
        for part in case.parts {
            for v in cpu_vertices(&vfs, part, &skin) {
                let Some((x, y)) = cam.project(v, aspect) else { continue };
                let (px, py) = (((x + 1.0) * 0.5 * W as f32) as i32, ((1.0 - y) * 0.5 * H as f32) as i32);
                if px < 1 || py < 1 || px >= W as i32 - 1 || py >= H as i32 - 1 {
                    continue;
                }

                (lo[0], lo[1], hi[0], hi[1]) = (lo[0].min(px), lo[1].min(py), hi[0].max(px), hi[1].max(py));
                seen += 1;

                // A pixel of slack: a vertex on the silhouette rounds either way.
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| changed[((py + dy) * W as i32 + px + dx) as usize]));
                hit += usize::from(near);
            }
        }

        // Rendered without a shadow, the unit changes nothing outside its vertices' box but the ambient occlusion it puts on the ground around
        // and under it: the band of pixels nearest the feet is left out, and below the feet.
        let ground = hi[1] - 60;
        let stray = changed.iter().enumerate().filter(|(i, c)| {
            let (x, y) = ((*i % W as usize) as i32, (*i / W as usize) as i32);
            **c && y < ground && (x < lo[0] - 3 || x > hi[0] + 3 || y < lo[1] - 3)
        }).count();
        let total = changed.iter().filter(|c| **c).count();
        let share = hit as f32 / seen.max(1) as f32;
        println!("{}: {hit} of {seen} vertices on the unit's pixels ({:.2} %), {total} pixels changed, {stray} stray, {shadow} by the shadow", case.name, share * 100.0);
        if seen < 100 || share < 0.97 || total < 500 || stray > 40 || shadow < 100 {
            println!("FAIL: {}", case.name);
            failures += 1;
        }
    }

    failures += legion(&gpu, &vfs, &files, &env, out)?;
    Ok(failures)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(out) = args.first() else {
        eprintln!("unit_render_check <out dir> [environment path in the packs]");
        std::process::exit(2);
    };

    let env = args.get(1).map_or("weather/charlemagne/med_sp_land_day_dry.environment", String::as_str);
    std::fs::create_dir_all(out).expect("the out dir");
    match run(Path::new(out), env) {
        Ok(0) => println!("all passed"),
        Ok(n) => {
            println!("{n} cases FAILED");
            std::process::exit(1);
        }

        Err(e) => {
            println!("error: {e}");
            std::process::exit(if e == "no Direct3D 11 device" { 2 } else { 1 });
        }
    }
}
