//! Tests of the renderer's CPU side. `render_cpu_dump` compares the wind and the environment mapping with the C++ code over the game's files.

use std::collections::BTreeMap;

use wce_assets::weather_vfx::WeatherDefinition;
use wce_render::cbuffers::{ConstantBuffers, Var};
use wce_render::environment_apply::apply_environment;
use wce_render::wind::WindSim;
use wce_scene::environment::EnvironmentParams;

#[test]
fn constant_buffers() {
    let mut cbs = ConstantBuffers::default();
    let vars = BTreeMap::from([("a".to_owned(), Var { offset: 0, size: 8 }), ("b".to_owned(), Var { offset: 16, size: 4 })]);
    cbs.declare("cb", 20, &vars);
    cbs.declare("cb", 8, &BTreeMap::from([("a".to_owned(), Var { offset: 4, size: 4 })]));
    assert_eq!(cbs.find("cb").unwrap().cpu.len(), 32);
    assert!(cbs.has("cb", "a") && !cbs.has("cb", "c"));

    cbs.find_mut("cb").unwrap().dirty = false;
    cbs.set_f("cb", "a", &[1.0, 2.0, 3.0]);
    let buffer = cbs.find("cb").unwrap();
    assert!(buffer.dirty);
    assert_eq!(&buffer.cpu[..12], &[0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 0, 0]);

    cbs.find_mut("cb").unwrap().dirty = false;
    cbs.set_f("cb", "a", &[1.0, 2.0]);
    assert!(!cbs.find("cb").unwrap().dirty);

    cbs.set_i("cb", "c", 1);
    cbs.set_i("cb", "c", 2);
    cbs.set_i("other", "x", 2);
    assert_eq!(cbs.missing(), ["cb.c", "other.x"]);
}

#[test]
fn environment_mapping() {
    let mut cbs = ConstantBuffers::default();
    cbs.declare("fog_VS_PS", 16, &BTreeMap::from([("g_fog_height_top".to_owned(), Var { offset: 0, size: 4 })]));
    apply_environment(&EnvironmentParams::default(), &mut cbs);
    assert_eq!(&cbs.find("fog_VS_PS").unwrap().cpu[..4], &0.001f32.to_le_bytes());
    assert!(cbs.missing().contains(&"hdr_to_screen_PS.colour_matrix".to_owned()));
}

#[test]
fn wind() {
    let def = WeatherDefinition { wind_size_min: 0.2, wind_size_max: 0.8, wind_gust_interval: 1.0, ..Default::default() };
    let run = |def: Option<&WeatherDefinition>| {
        let mut w = WindSim::default();
        w.reset(WindSim::DEFAULT_SEED);
        let mut strongest = 0.0f32;
        for i in 1..=1200 {
            w.update(def, 90.0, i as f32 / 60.0, 1.0 / 60.0);
            strongest = strongest.max(w.strength);
        }

        (w, strongest)
    };

    let (w, strongest) = run(Some(&def));
    assert!(strongest > 0.2 && strongest <= 0.8 + 1e-6);
    assert!((w.direction.y - 1.0).abs() < 1e-6 && w.smoothed.y > 0.0);
    assert_eq!(run(Some(&def)).0, w);

    let (calm, _) = run(None);
    assert_eq!((calm.strength, calm.vfx_velocity.x), (0.0, 2.0));
}
