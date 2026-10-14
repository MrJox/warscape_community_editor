//! render_check: the renderer without a window. Builds render scenes, renders them offscreen with a fixed-step clock and checks what the
//! editor depends on: determinism, entity picking (meshes, terrain, overlays, locked and hidden layers), the interim terrain pass, the overlay
//! pass (colour, depth test, selection outline, grid) and PNG capture. Port of `cli/render_check.cpp`.
//!
//!   cargo run --release -p wce_render --example render_check -- [environment path in the packs]      (the game folder: wce.ini or WCE_GAME)

use std::path::PathBuf;
use std::sync::Arc;

use wce_d3d11::*;
use wce_formats::png_file::PngFile;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::clock::Clock;
use wce_foundation::math::{Float3, Float4x4};
use wce_foundation::settings::Settings;
use wce_render::files::Files;
use wce_render::gpu_adapter::choose_adapter;
use wce_render::render_target::RenderTarget;
use wce_render::renderer::{Renderer, ViewParams};
use wce_scene::camera::Camera;
use wce_scene::environment::read_environment;
use wce_scene::render_scene::{Overlay, Payload, RenderScene, SourceRef, Terrain, overlay_colour};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_weather::sample_map::publish_sample_map;

const W: u32 = 640;
const H: u32 = 360;

#[derive(Default)]
struct Gate {
    failures: usize,
}

impl Gate {
    fn check(&mut self, ok: bool, msg: &str) {
        if !ok {
            self.failures += 1;
            println!("FAIL: {msg}");
        }
    }
}

struct Gpu {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
}

fn make_device() -> Option<Gpu> {
    // Needs the Windows "Graphics Tools" feature.
    let debug = std::env::var("WCE_D3D_DEBUG").is_ok_and(|d| d.starts_with('1'));
    // SAFETY: before other threads; valid arguments.
    unsafe {
        init_platform();
        let adapter = choose_adapter();
        let driver = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        let (mut dev, mut ctx) = (None, None);
        let flags = if debug { D3D11_CREATE_DEVICE_DEBUG as u32 } else { 0 };
        D3D11CreateDevice(adapter.as_ref(), driver, HMODULE::default(), flags, Some(&[D3D_FEATURE_LEVEL_11_0]), D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx))
            .ok()
            .ok()?;
        Some(Gpu { dev: dev?, ctx: ctx? })
    }
}

fn orbit(yaw: f32, pitch: f32, dist: f32, target: Float3) -> Camera {
    let (cp, sp) = (pitch.cos(), pitch.sin());
    Camera { target, eye: target + Float3::new(yaw.sin() * cp, sp, -yaw.cos() * cp) * dist, ..Default::default() }
}

/// Renders `frames` fixed-step frames; the last one stays in `target`.
fn render_frames(r: &mut Renderer, scene: &RenderScene, cam: &Camera, target: &mut RenderTarget, frames: usize) {
    let mut clock = Clock::fixed(1.0 / 60.0);
    r.deterministic = true;
    for _ in 0..frames {
        clock.tick();
        r.render(scene, &ViewParams { camera: *cam, time_s: clock.time() as f32 }, target);
    }
}

struct Pixels {
    rgba: Vec<u8>,
    w: i32,
    h: i32,
}

impl Pixels {
    fn at(&self, x: i32, y: i32) -> &[u8] {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        &self.rgba[i..i + 4]
    }

    /// Pixels within `tol` of the colour (r, g, b) in the box around (cx, cy).
    fn count_near(&self, cx: i32, cy: i32, radius: i32, rgb: [i32; 3], tol: i32) -> usize {
        let mut n = 0;
        for y in (cy - radius).max(0)..=(cy + radius).min(self.h - 1) {
            for x in (cx - radius).max(0)..=(cx + radius).min(self.w - 1) {
                let p = self.at(x, y);
                if (i32::from(p[0]) - rgb[0]).abs() <= tol && (i32::from(p[1]) - rgb[1]).abs() <= tol && (i32::from(p[2]) - rgb[2]).abs() <= tol {
                    n += 1;
                }
            }
        }

        n
    }

    fn count_near_all(&self, rgb: [i32; 3], tol: i32) -> usize {
        self.count_near(self.w / 2, self.h / 2, self.w.max(self.h), rgb, tol)
    }
}

fn read(g: &Gpu, t: &RenderTarget) -> Pixels {
    Pixels { rgba: t.read_rgba8(&g.dev, &g.ctx).unwrap_or_default(), w: t.width() as i32, h: t.height() as i32 }
}

fn run(g: &mut Gate, env_path: &str) -> Result<(), String> {
    let gpu = make_device().ok_or("no Direct3D 11 device")?;
    let game = Settings::load(&Settings::default_dir()).get("game_dir", Some("WCE_GAME")).ok_or("the game folder is not set (game_dir in wce.ini or WCE_GAME)")?;
    let vfs: Arc<Vfs> = Arc::new(Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).map_err(|e| e.to_string())?);
    let files: Arc<dyn Files> = vfs.clone();
    let env_doc = XmlDoc::parse(vfs.read(env_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let env = read_environment(&env_doc);
    let renderer = || Renderer::new(&gpu.dev, &gpu.ctx, files.clone()).map_err(|e| e.0);

    // ---- scene A: the sample map (flat ground and houses)
    {
        let mut scene = RenderScene::default();
        let layer = scene.add_layer(1, "Sample map");
        let sm = publish_sample_map(&*vfs, &mut scene, layer, 1, 1, 8, "rigidmodels/buildings/");
        scene.environment.active = Some(env.clone());
        g.check(sm.buildings > 0, "the sample map placed buildings");
        let cam = orbit(0.6, 0.35, 120.0, Float3::new(0.0, 5.0, 0.0));
        let (mut r1, mut r2) = (renderer()?, renderer()?);
        let (mut t1, mut t2) = (RenderTarget::new(&gpu.dev, W, H), RenderTarget::new(&gpu.dev, W, H));
        render_frames(&mut r1, &scene, &cam, &mut t1, 4);
        render_frames(&mut r2, &scene, &cam, &mut t2, 4);
        let (a, b) = (read(&gpu, &t1), read(&gpu, &t2));
        g.check(!a.rgba.is_empty() && a.rgba == b.rgba, "two renderers give the same pixels for the same fixed-step frames");
        g.check(r1.draw_count() == scene.entities().len(), "every mesh and terrain entity has a draw");

        // Picking: sky misses, ground hits the terrain entity, house centres hit their house.
        let terrain_id = scene.entities().iter().rev().find(|e| matches!(e.payload, Payload::Terrain(_))).map_or(0, |e| e.id);
        let vp = ViewParams { camera: cam, time_s: 0.0 };
        g.check(r1.pick(&scene, &vp, &t1, 10, 2) == 0, "picking the sky finds nothing");
        let ground_hits = [W as i32 - 8, W as i32 - 30, W as i32 - 60].into_iter().filter(|&x| r1.pick(&scene, &vp, &t1, x, H as i32 - 8) == terrain_id).count();
        g.check(ground_hits >= 2, "picking the ground at the bottom of the view finds the terrain entity");
        let (mut house_hits, mut houses_in_view) = (0, 0);
        for item in 1..=sm.buildings as u64 {
            let mut lo = Float3::new(1e30, 1e30, 1e30);
            let mut hi = Float3::new(-1e30, -1e30, -1e30);
            for e in scene.entities() {
                let Payload::MeshInstance(mi) = &e.payload else { continue };
                if e.source.item != item {
                    continue;
                }

                let Some(m) = r1.models().model(&mi.model) else { continue };
                for k in 0..8 {
                    let c = Float3::new(
                        if k & 1 != 0 { m.bmax.x } else { m.bmin.x }, if k & 2 != 0 { m.bmax.y } else { m.bmin.y }, if k & 4 != 0 { m.bmax.z } else { m.bmin.z },
                    );
                    let w = e.world.transform_point(c);
                    lo = Float3::new(lo.x.min(w.x), lo.y.min(w.y), lo.z.min(w.z));
                    hi = Float3::new(hi.x.max(w.x), hi.y.max(w.y), hi.z.max(w.z));
                }
            }

            if lo.x > hi.x {
                continue;
            }

            let Some((px, py)) = Renderer::project(&cam, W, H, (lo + hi) * 0.5) else { continue };
            if px < 4.0 || py < 4.0 || px > W as f32 - 4.0 || py > H as f32 - 4.0 {
                continue;
            }

            houses_in_view += 1;
            let id = r1.pick(&scene, &vp, &t1, px as i32, py as i32);
            if scene.find(id).is_some_and(|e| matches!(e.payload, Payload::MeshInstance(_)) && e.source.item == item) {
                house_hits += 1;
            }
        }

        println!("houses in view {houses_in_view}, picked right {house_hits}");

        // The centre of a bounding box can be empty air (an arch) or hidden behind another house, so only a share has to match.
        g.check(houses_in_view > 0 && house_hits >= 2 && house_hits * 3 >= houses_in_view, "picking the centre of a house often finds a piece of that house");

        // A locked layer is not pickable, a hidden one is not drawn.
        let (bx, by) = (W as i32 - 8, H as i32 - 8);
        scene.layer_mut(layer).expect("the layer").locked = true;
        g.check(r1.pick(&scene, &vp, &t1, bx, by) == 0, "a locked layer is not picked");
        scene.layer_mut(layer).expect("the layer").locked = false;
        scene.set_layer_visible(layer, false);
        render_frames(&mut r1, &scene, &cam, &mut t1, 1);
        g.check(r1.draw_count() == 0, "a hidden layer has no draws");
        g.check(r1.pick(&scene, &vp, &t1, bx, by) == 0, "a hidden layer is not picked");
        scene.set_layer_visible(layer, true);
    }

    // ---- scene B: a hill (3 x 3 terrain patches at most) with overlays on and inside it
    {
        let mut scene = RenderScene::default();
        let ground = scene.add_layer(1, "Hill");
        let mut hill = Terrain { nx: 129, nz: 129, step: 4.0, origin_x: -256.0, origin_z: -256.0, ..Default::default() };
        hill.heights = vec![0.0; (hill.nx * hill.nz) as usize];
        for j in 0..hill.nz {
            for i in 0..hill.nx {
                let x = hill.origin_x + i as f32 * hill.step;
                let z = hill.origin_z + j as f32 * hill.step;
                hill.heights[(j * hill.nx + i) as usize] = 40.0 * (-(x * x + z * z) / (2.0 * 80.0 * 80.0)).exp();
            }
        }

        let peak_side = hill.heights[(64 * hill.nx + (64 - 15)) as usize];
        let hill_id = scene.add(ground, Float4x4::IDENTITY, Payload::Terrain(hill), SourceRef { doc: 1, item: 0 });
        let marks = scene.add_layer(2, "Overlays");

        // Above the peak: always visible.
        let mut up = Overlay { lines: vec![], triangles: vec![], depth_test: true, pickable: true };
        up.line(Float3::new(0.0, 41.0, 0.0), Float3::new(0.0, 110.0, 0.0), overlay_colour(1.0, 0.0, 1.0, 1.0));
        let up_id = scene.add(marks, Float4x4::IDENTITY, Payload::Overlay(up), SourceRef { doc: 2, item: 1 });

        // Inside the hill: hidden by the depth test, visible without it.
        let mut buried = Overlay { lines: vec![], triangles: vec![], depth_test: true, pickable: true };
        buried.line(Float3::new(0.0, 2.0, 0.0), Float3::new(0.0, 30.0, 0.0), overlay_colour(0.0, 1.0, 0.0, 1.0));
        let buried_id = scene.add(marks, Float4x4::IDENTITY, Payload::Overlay(buried), SourceRef { doc: 2, item: 2 });
        scene.environment.active = Some(env.clone());
        let cam = orbit(0.6, 0.5, 220.0, Float3::new(0.0, 25.0, 0.0));
        let vp = ViewParams { camera: cam, time_s: 0.0 };
        let mut r = renderer()?;
        let mut t = RenderTarget::new(&gpu.dev, W, H);
        render_frames(&mut r, &scene, &cam, &mut t, 3);
        let base = read(&gpu, &t);
        let up_px = Renderer::project(&cam, W, H, Float3::new(0.0, 76.0, 0.0));
        g.check(up_px.is_some(), "the overlay is in front of the camera");
        let (mx, my) = up_px.map_or((0, 0), |(x, y)| (x as i32, y as i32));
        g.check(base.count_near(mx, my, 2, [255, 0, 255], 2) > 0, "an overlay line is drawn in its own colour (no tone mapping)");
        let (bx, by) = Renderer::project(&cam, W, H, Float3::new(0.0, 16.0, 0.0)).map_or((0, 0), |(x, y)| (x as i32, y as i32));
        g.check(base.count_near(bx, by, 3, [0, 255, 0], 2) == 0, "an overlay line inside the hill is hidden by the depth test");

        // The terrain mesh: its peak is where the heights say (the id pass sees the surface, not the overlay); 60 m left of the peak.
        let (tx, ty) = Renderer::project(&cam, W, H, Float3::new(-60.0, peak_side, 0.0)).map_or((0, 0), |(x, y)| (x as i32, y as i32));
        g.check(r.pick(&scene, &vp, &t, tx, ty) == hill_id, "picking the hill surface finds the terrain entity");
        g.check(r.pick(&scene, &vp, &t, mx, my) == up_id, "an overlay line is picked in screen space and wins over what is behind it");
        g.check(r.pick(&scene, &vp, &t, 3, 3) == 0, "picking empty sky finds nothing");

        // Selection outline, grid, depth-test off.
        r.selection = vec![hill_id];
        render_frames(&mut r, &scene, &cam, &mut t, 1);
        g.check(read(&gpu, &t).count_near_all([255, 217, 51], 4) > 20, "a selected entity is outlined");
        r.selection.clear();
        r.show_grid = true;
        render_frames(&mut r, &scene, &cam, &mut t, 1);
        g.check(read(&gpu, &t).rgba != base.rgba, "the grid changes the image");
        r.show_grid = false;
        if let Some(Payload::Overlay(b)) = scene.find_mut(buried_id).map(|e| &mut e.payload) {
            b.depth_test = false;
        }

        scene.touch();
        render_frames(&mut r, &scene, &cam, &mut t, 1);
        g.check(read(&gpu, &t).count_near(bx, by, 3, [0, 255, 0], 2) > 0, "without the depth test the buried line shows");

        // Capture: the PNG decodes to what the target holds.
        let png = std::env::temp_dir().join("wce_render_check.png");
        g.check(t.save_png(&gpu.dev, &gpu.ctx, &png), "save_png");
        let decoded = std::fs::read(&png).map_err(|e| e.to_string()).and_then(|b| PngFile::parse(&b).map_err(|e| e.0))?;
        g.check(decoded.width == W && decoded.height == H, "the PNG has the target's size");
        let now = read(&gpu, &t);
        let n = (W * H) as usize;
        let same = decoded.color_type == 2
            && decoded.pixels.len() == n * 3
            && (0..n).all(|i| decoded.pixels[i * 3..i * 3 + 3] == now.rgba[i * 4..i * 4 + 3]);
        g.check(same, "the PNG holds the target's pixels");
        let _ = std::fs::remove_file(&png);
        for e in r.errors() {
            println!("renderer: {e}");
        }
    }

    Ok(())
}

fn main() {
    let env_path = std::env::args().nth(1).unwrap_or_else(|| "weather/charlemagne/med_sp_land_day_dry.environment".to_owned());
    let mut g = Gate::default();
    if let Err(e) = run(&mut g, &env_path) {
        println!("error: {e}");
        std::process::exit(if e == "no Direct3D 11 device" { 2 } else { 1 });
    }

    println!("{}", if g.failures != 0 { "FAILED" } else { "all passed" });
    std::process::exit(i32::from(g.failures != 0));
}

