//! Differential dump of the particle spawner, for comparing with the C++ code: every weather definition, 300 frames of CPU emission
//! (the spawned vertices hashed per frame).
//!
//!   spawn_dump <folder with vfx/ and weather/>

use std::path::PathBuf;

use wce_d3d11::*;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::Float3;
use wce_render::particles::{Frame, ParticleSystem};

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_default());
    // SAFETY: no other thread exists yet.
    unsafe { init_platform() };
    let (mut dev, mut ctx) = (None, None);
    // SAFETY: device creation with valid arguments.
    unsafe {
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), 0, None, D3D11_SDK_VERSION as u32, Some(&mut dev), None, Some(&mut ctx)).unwrap();
    }

    let (dev, ctx) = (dev.unwrap(), ctx.unwrap());
    let defs = XmlDoc::parse(std::fs::read(root.join("weather/weather_definitions.xml")).unwrap()).unwrap();
    for i in (0..defs.elements().len()).filter(|&i| defs.elements()[i].name == "WEATHER_DEFINITION") {
        let name = defs.get(i, "name").unwrap_or_default();
        let folder = root.clone();
        let read = Box::new(move |path: &str| {
            let p = folder.join(wce_vfs::normalize(path));
            if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
        });

        let mut ps = ParticleSystem::new(&dev, &ctx, read);
        ps.set_weather(&name, 0);
        println!("== {name} emitters {}", ps.effect().emitters.len());
        if ps.effect().emitters.is_empty() {
            continue;
        }

        let mut all = 0xCBF2_9CE4_8422_2325u64;
        let mut total = 0;
        for frame in 0..300 {
            let f = Frame {
                eye: Float3::new(frame as f32 * 0.7, 30.0 + (frame % 17) as f32, frame as f32 * -0.4),
                dt: if frame % 23 == 0 { 0.25 } else { 1.0 / 60.0 },
                ..Default::default()
            };

            ps.advance(f.dt);
            let verts = ps.spawn(&f);
            for v in &verts {
                for x in v.id_seed_steps.iter().chain(&v.pos_time).chain(&v.vel_life) {
                    for c in x.to_le_bytes() {
                        all = (all ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3);
                    }
                }
            }

            total += verts.len();
            if frame % 50 == 0 {
                println!("  frame {frame} spawned {} alive {}", verts.len(), ps.alive());
            }
        }

        println!("  total {total} hash {all:016x} rng {:08x}", ps.rng_state());
        for (e, emitter) in ps.effect().emitters.iter().enumerate() {
            println!("  emitter {e} alive {} mask {} {}", ps.alive_of(e), emitter.k.behaviour_mask[1], emitter.k.behaviour_mask[0]);
        }

        for e in ps.errors() {
            println!("  error {e}");
        }
    }
}
