//! Differential dump of whole frames: the same scenes the C++ harness renders, written as raw RGBA, with the frame stats and a grid of picks.
//!
//!   cargo run --release -p wce_render --example frame_dump -- <out dir> <models list> <extracted packs folder>

use std::path::PathBuf;
use std::sync::Arc;

use wce_d3d11::*;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::clock::Clock;
use wce_foundation::math::{Float3, Float4x4, rotation_y, translation};
use wce_render::files::{FolderFiles, Files};
use wce_render::gpu_adapter::choose_adapter;
use wce_render::render_target::RenderTarget;
use wce_render::renderer::{Renderer, View, ViewParams};
use wce_scene::camera::Camera;
use wce_scene::environment::{EnvironmentParams, read_environment};
use wce_scene::render_scene::*;

const W: u32 = 640;
const H: u32 = 360;

fn fnv(d: &[u8]) -> u64 {
    d.iter().fold(14695981039346656037, |h, &c| (h ^ u64::from(c)).wrapping_mul(1099511628211))
}

fn orbit(yaw: f32, pitch: f32, dist: f32, target: Float3) -> Camera {
    let (cp, sp) = (pitch.cos(), pitch.sin());
    Camera { target, eye: target + Float3::new(yaw.sin() * cp, sp, -yaw.cos() * cp) * dist, ..Default::default() }
}

fn hill_y(x: f32, z: f32) -> f32 {
    40.0 * (-(x * x + z * z) / (2.0 * 80.0 * 80.0)).exp()
}

fn hill() -> Terrain {
    let mut t = Terrain { nx: 129, nz: 129, step: 4.0, origin_x: -256.0, origin_z: -256.0, version: 1, ..Default::default() };
    for j in 0..t.nz {
        for i in 0..t.nx {
            let (x, z) = (t.origin_x + i as f32 * t.step, t.origin_z + j as f32 * t.step);
            std::sync::Arc::make_mut(&mut t.heights).push(hill_y(x, z));
        }
    }

    t
}

fn add_world(s: &mut RenderScene, models: &[String]) {
    let ground = s.add_layer(1, "ground");
    s.add(ground, Float4x4::IDENTITY, Payload::Terrain(hill()), SourceRef { doc: 1, item: 0 });
    let b = s.add_layer(2, "buildings");
    for (k, model) in models.iter().enumerate() {
        let (a, r) = (k as f32 * 0.7, 60.0 + k as f32 * 9.0);
        let (x, z) = (a.sin() * r, -a.cos() * r);
        let mi = MeshInstance { model: model.clone(), ..Default::default() };
        s.add(b, translation(Float3::new(x, hill_y(x, z), z)) * rotation_y(a), Payload::MeshInstance(mi), SourceRef { doc: 2, item: k as u64 + 1 });
    }

    let o = s.add_layer(3, "overlays");
    let mut up = Overlay::default();
    up.line(Float3::new(0.0, 41.0, 0.0), Float3::new(0.0, 110.0, 0.0), overlay_colour(1.0, 0.0, 1.0, 1.0));
    up.triangle(Float3::new(-20.0, 45.0, 0.0), Float3::new(20.0, 45.0, 0.0), Float3::new(0.0, 70.0, 0.0), overlay_colour(0.2, 0.6, 1.0, 0.5));
    s.add(o, Float4x4::IDENTITY, Payload::Overlay(up), SourceRef { doc: 3, item: 1 });
    let mut top = Overlay { depth_test: false, ..Default::default() };
    top.box_wire(Float3::new(-30.0, 0.0, -30.0), Float3::new(30.0, 20.0, 30.0), overlay_colour(0.0, 1.0, 0.0, 1.0));
    s.add(o, translation(Float3::new(40.0, 5.0, 0.0)), Payload::Overlay(top), SourceRef { doc: 3, item: 2 });
}

fn water_vertex([x, y, z, across, u, v, flow, fade]: [f32; 8]) -> WaterVertex {
    WaterVertex { x, y, z, across, u, v, flow, fade }
}

fn add_water(s: &mut RenderScene) {
    let l = s.add_layer(4, "water");
    let mut sea = Water { river: false, version: 1, ..Default::default() };
    let n = 16;
    for j in 0..=n {
        for i in 0..=n {
            let (x, z) = (-256.0 + 512.0 * i as f32 / n as f32, -256.0 + 512.0 * j as f32 / n as f32);
            sea.verts.push(water_vertex([x, 9.0, z, 0.0, x, z, 0.0, 1.0]));
        }
    }

    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let (b, c) = (a + 1, a + n as u32 + 1);
            sea.indices.extend([a, c, b, b, c, c + 1]);
        }
    }

    s.add(l, Float4x4::IDENTITY, Payload::Water(sea), SourceRef { doc: 4, item: 1 });
    let mut river = Water { version: 1, ..Default::default() };
    for k in 0..=40 {
        let (z, xc) = (-200.0 + 10.0 * k as f32, 70.0 + 15.0 * (k as f32 * 0.3).sin());
        for side in [-1.0f32, 1.0] {
            let x = xc + 6.0 * side;
            river.verts.push(water_vertex([x, hill_y(x, z) + 0.4, z, 6.0 * side, 6.0 * side, 10.0 * k as f32, 1.5, 1.0]));
        }
    }

    for k in 0..40u32 {
        let a = 2 * k;
        river.indices.extend([a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }

    s.add(l, Float4x4::IDENTITY, Payload::Water(river), SourceRef { doc: 4, item: 2 });
}

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> TerrainImage {
    let px = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect();
    TerrainImage { w, h, rgba: Some(Arc::new(px)) }
}

fn add_material(s: &mut RenderScene) {
    let mut t = hill();
    let mut m = TerrainMaterial { x0: -256.0, z0: -256.0, x1: 256.0, z1: 256.0, normal_id: 1, id: 1, ..Default::default() };
    m.blend = image(64, 64, |x, _| [(255 - x * 4) as u8, (x * 4) as u8, 0, 0]);
    m.index = image(64, 64, |_, y| [(y * 4) as u8, 85, 0, 0]);
    m.normal = image(64, 64, |x, y| [0, (96 + y) as u8, 0, (96 + x) as u8]);
    m.diffuse[0] = "terrain/textures/battle/default/grass/0_su_diffuse.dds".into();
    m.diffuse[1] = "terrain/textures/battle/default/agri/1_su_diffuse.dds".into();
    m.diffuse[4] = "terrain/textures/battle/default/agri/0_su_diffuse.dds".into();
    m.diffuse[5] = "terrain/textures/battle/default/agri/2_su_diffuse.dds".into();
    m.normal_tex[0] = "terrain/textures/battle/default/grass/0_su_normal.dds".into();
    m.normal_tex[1] = "terrain/textures/battle/default/agri/1_su_normal.dds".into();
    m.normal_tex[4] = "terrain/textures/battle/default/agri/0_su_normal.dds".into();
    m.normal_tex[5] = "terrain/textures/battle/default/agri/2_su_normal.dds".into();
    m.detail_colour = "terrain/textures/battle/default/grass/1_sp_diffuse.dds".into();
    m.detail_normal = "terrain/textures/battle/default/grass/1_sp_normal.dds".into();
    for k in 0..16 {
        m.smoothness[k] = 0.1 * (k % 4) as f32;
        m.reflectivity[k] = 0.05 * (k % 3) as f32;
    }

    t.material = Some(Arc::new(m));
    let l = s.add_layer(1, "ground");
    s.add(l, Float4x4::IDENTITY, Payload::Terrain(t), SourceRef { doc: 1, item: 0 });
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let out = PathBuf::from(&args[0]);
    let models = std::fs::read_to_string(&args[1]).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let files: Arc<dyn Files> = Arc::new(FolderFiles(PathBuf::from(&args[2])));

    // SAFETY: no other thread exists yet; device creation with valid arguments.
    let (dev, ctx) = unsafe {
        init_platform();
        let adapter = choose_adapter();
        let (mut dev, mut ctx) = (None, None);
        let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        D3D11CreateDevice(adapter.as_ref(), driver, HMODULE::default(), 0, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx)).unwrap();
        (dev.unwrap(), ctx.unwrap())
    };

    let env_of = |p: &str| read_environment(&XmlDoc::parse(files.read(p).unwrap()).unwrap());
    let dry = env_of("weather/charlemagne/med_sp_land_day_dry.environment");
    let rain = env_of("weather/charlemagne/med_sp_land_day_rain.environment");
    let night = env_of("weather/charlemagne/at_su_land_night_fog.environment");
    let dump = |r: &Renderer, t: &RenderTarget, name: &str| {
        let px = t.read_rgba8(&dev, &ctx).unwrap_or_default();
        std::fs::write(out.join(format!("{name}.rgba")), &px).unwrap();
        let d = r.draw_stats();
        println!("{name} {:016x} items {} batches {} models {} waiting {} views {} inst {} calls {} tris {} cam {}/{} shadow {}/{} patches {}", fnv(&px), d.items, d.batches, d.models,
            d.waiting_models, d.views, d.instances, d.draw_calls, d.triangles, d.camera_instances, d.camera_triangles, d.shadow_instances, d.shadow_triangles, d.terrain_patches);
        println!("  luma {:08x} {:08x} sun {:08x} clouds {} wind {:08x} amb {}", r.luma_black().to_bits(), r.luma_white().to_bits(), r.sun_visibility().to_bits(), r.cloud_sprites(),
            r.wind_direction_degrees(&dry).to_bits(), r.engine_ambient_faces());
    };

    let run = |name: &str, env: &EnvironmentParams, what: u32, cam: Camera, frames: u32| {
        let mut s = RenderScene::default();
        if what & 1 != 0 {
            add_world(&mut s, &models);
        }

        if what & 2 != 0 {
            add_water(&mut s);
        }

        if what & 4 != 0 {
            add_material(&mut s);
        }

        s.environment.active = Some(env.clone());
        let mut r = Renderer::new(&dev, &ctx, files.clone()).unwrap();
        let mut t = RenderTarget::new(&dev, W, H);
        r.settings.deterministic = true;
        if what & 8 != 0 {
            r.settings.show_grid = true;
            r.settings.selection = vec![2, 5];
        }

        let mut clock = Clock::fixed(1.0 / 60.0);
        for _ in 0..frames {
            clock.tick();
            r.render(&s, &ViewParams { camera: cam, time_s: clock.time() as f32 }, &mut t);
        }

        dump(&r, &t, name);
        if what & 16 != 0 {
            let views = [
                (View::HdrRaw, "hdr"), (View::Albedo, "albedo"), (View::Normal, "normal"), (View::Specular, "specular"), (View::Depth, "depth"), (View::SkyCube, "skycube"),
                (View::Bloom, "bloom"), (View::Quarter, "quarter"), (View::ToneMapped, "tonemapped"), (View::Ao, "ao"),
            ];

            for (v, view_name) in views {
                r.settings.view = v;
                clock.tick();
                r.render(&s, &ViewParams { camera: cam, time_s: clock.time() as f32 }, &mut t);
                dump(&r, &t, &format!("{name}_{view_name}"));
            }

            r.settings.view = View::Final;
            print!("  picks");
            for y in (20..H as i32).step_by(40) {
                for x in (20..W as i32).step_by(50) {
                    print!(" {}", r.pick(&s, &ViewParams { camera: cam, time_s: 0.0 }, &t, x, y));
                }
            }

            println!();
        }

        for e in r.errors() {
            println!("  error {e}");
        }

        for e in r.shader_failures() {
            println!("  shader {e}");
        }
    };

    run("sky", &dry, 0, orbit(0.6, 0.15, 100.0, Float3::new(0.0, 20.0, 0.0)), 4);
    run("world", &dry, 1 | 8 | 16, orbit(0.6, 0.45, 230.0, Float3::new(0.0, 20.0, 0.0)), 4);
    run("night", &night, 1, orbit(2.2, 0.3, 200.0, Float3::new(0.0, 20.0, 0.0)), 4);
    run("water", &dry, 1 | 2, orbit(1.2, 0.35, 220.0, Float3::new(40.0, 10.0, 0.0)), 4);
    run("rain", &rain, 1, orbit(0.3, 0.25, 150.0, Float3::new(0.0, 25.0, 0.0)), 30);
    run("material", &dry, 4, orbit(0.9, 0.5, 260.0, Float3::new(0.0, 10.0, 0.0)), 4);
}
