//! Differential dump of the wind simulation and of the environment -> constant buffer mapping, for comparing with the C++ code.
//!
//!   render_cpu_dump <folder with vfx/ and weather/> <file with one .environment path per line> <file of "buffer variable size" lines>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_assets::weather_vfx::{WeatherDefinition, load_weather_vfx};
use wce_formats::xml_doc::XmlDoc;
use wce_render::cbuffers::{ConstantBuffers, Var};
use wce_render::environment_apply::apply_environment;
use wce_render::wind::WindSim;
use wce_scene::environment::read_environment;

fn b(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0100_0000_01B3))
}

fn wind_run(tag: &str, def: Option<&WeatherDefinition>) {
    let mut w = WindSim::default();
    w.reset(WindSim::DEFAULT_SEED);
    let mut t = 0.0f32;
    for i in 0..3000 {
        let dt = if i % 13 == 0 { 0.25 } else { 1.0 / 60.0 };
        t += dt;
        w.update(def, (i % 720) as f32 * 0.5, t, dt);
        if i % 150 == 0 {
            println!("{tag} {i} dir={},{} s={} ge={} sm={},{} off={},{} vel={},{},{} var={},{},{} g={},{},{},{} al={}", b(w.direction.x), b(w.direction.y), b(w.strength),
                b(w.gust_end), b(w.smoothed.x), b(w.smoothed.y), b(w.offset.x), b(w.offset.y), b(w.vfx_velocity.x), b(w.vfx_velocity.y), b(w.vfx_velocity.z),
                b(w.vfx_variance.x), b(w.vfx_variance.y), b(w.vfx_variance.z), b(w.gust_size), b(w.gust_duration), b(w.gust_start), b(w.cooldown), b(w.alignment()));
        }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let root = PathBuf::from(&args[1]);
    let reader = |path: &str| {
        let p = root.join(path);
        if p.exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    };

    let defs = XmlDoc::parse(std::fs::read(root.join("weather/weather_definitions.xml")).unwrap()).unwrap();
    wind_run("none", None);
    for i in (0..defs.elements().len()).filter(|&i| defs.elements()[i].name == "WEATHER_DEFINITION") {
        let name = defs.get(i, "name").unwrap_or_default();
        let vfx = load_weather_vfx(&reader, &name).unwrap();
        wind_run(&name, Some(&vfx.definition));
    }

    let mut seed = 12345;
    for _ in 0..5 {
        println!("random {}", b(WindSim::random_direction(&mut seed)));
    }

    let mut layout = BTreeMap::<String, BTreeMap<String, Var>>::new();
    let mut used = BTreeMap::<String, u32>::new();
    let vars = std::fs::read_to_string(&args[3]).unwrap();
    for line in vars.lines() {
        let [cb, var, size] = line.split_whitespace().collect::<Vec<_>>()[..] else { continue };
        let size = size.parse::<u32>().unwrap();
        let offset = used.entry(cb.to_owned()).or_default();
        layout.entry(cb.to_owned()).or_default().insert(var.to_owned(), Var { offset: *offset, size });
        *offset += (size + 15) & !15;
    }

    let mut cbs = ConstantBuffers::default();
    for (name, vars) in &layout {
        cbs.declare(name, used[name] - 3, vars);
    }

    for path in std::fs::read_to_string(&args[2]).unwrap().lines() {
        let Ok(doc) = XmlDoc::parse(std::fs::read(path).unwrap()) else { continue };
        apply_environment(&read_environment(&doc), &mut cbs);
        let buffers = cbs.all().iter().map(|(name, buf)| format!(" {name}={}/{:016x}/{}", buf.cpu.len(), fnv(&buf.cpu), i32::from(buf.dirty))).collect::<String>();
        println!("== {path}{buffers}");
        let names = cbs.all().keys().cloned().collect::<Vec<_>>();
        for name in names {
            if let Some(buf) = cbs.find_mut(&name) {
                buf.dirty = false;
            }
        }
    }

    for missing in cbs.missing() {
        println!("missing {missing}");
    }
}
