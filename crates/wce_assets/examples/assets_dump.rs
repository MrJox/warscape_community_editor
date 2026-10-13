//! Differential dump of the vfx definitions, emitter constants and weather effects, for comparing with the C++ code fed the same files.
//!
//!   assets_dump <folder with vfx/ and weather/>

use std::fmt::Write as _;
use std::path::Path;

use wce_assets::vfx_constants::{EmitterConstants, build_emitter_constants};
use wce_assets::vfx_def::{VfxProps, parse_vfx};
use wce_assets::weather_vfx::load_weather_vfx;
use wce_formats::xml_doc::XmlDoc;

fn b(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn bs(f: &[f32]) -> String {
    f.iter().map(|f| format!("{},", b(*f))).collect()
}

fn us(f: &[u32]) -> String {
    f.iter().map(|f| format!("{f},")).collect()
}

fn props(p: &VfxProps) {
    for (name, x) in &p.props {
        println!("    prop {name} value={} v={} comps={} min={} max={} ranged={} mt={} Mt={}", x.value, bs(&x.v), x.components, b(x.min), b(x.max), i32::from(x.ranged), x.min_text, x.max_text);
        for (curve_name, c) in &x.curves {
            let mut line = format!("      curve {curve_name}:");
            for (t, v) in &c.keys {
                let _ = write!(line, " {}/{}", b(*t), b(*v));
            }

            println!("{line} eval {} {}", b(c.eval(0.37)), b(c.eval(1.5)));
        }
    }
}

fn consts(k: &EmitterConstants) {
    println!("    k mask={} gs={}{} sim={}{}{}{}{} ps={}{}{}{}{}{}{} vs={}{}{}{} cell={} anim={}{} tex={}|{} add={}", us(&k.behaviour_mask),
        bs(&k.vs_translation_params), bs(&k.vs_translation_params1), bs(&k.ps_sim_game_logic_params), bs(&k.ps_sim_gravity_params),
        bs(&k.ps_sim_deceleration_params), bs(&k.ps_sim_following_emitter_params), bs(&k.ps_sim_turbulence_params),
        bs(&k.ps_specular_params), bs(&k.ps_specular_params1), bs(&k.ps_receive_shadows_high_quality_params), bs(&k.ps_lighting_legacy_params),
        bs(&k.ps_lighting_params), bs(&k.ps_alpha_crush_params), bs(&k.ps_soft_edge_params), bs(&k.vs_culling_distances),
        bs(&k.vs_rotation_params), bs(&k.vs_scroll_uvs_params), bs(&k.vs_atlas_texture_params), us(&k.vs_cell_texture_params),
        us(&k.vs_animated_texture_params), bs(&k.vs_animated_texture_params1), k.diffuse, k.normal, i32::from(k.additive));
    let hash = k.curves.iter().flatten().flatten().flat_map(|f| f.to_le_bytes())
        .fold(0xCBF2_9CE4_8422_2325u64, |h, byte| (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3));
    println!("    curves {hash:016x}");
}

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_default());
    let mut files = std::fs::read_dir(root.join("vfx")).unwrap().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "xml")).collect::<Vec<_>>();
    files.sort();
    for p in &files {
        println!("== {}", p.file_name().unwrap().to_string_lossy());
        let doc = match XmlDoc::parse(std::fs::read(p).unwrap()) {
            Ok(doc) => doc,
            Err(error) => {
                println!("error {error}");
                continue;
            }
        };

        let f = parse_vfx(&doc);
        for (k, v) in &f.vfx_attrs {
            println!("  attr {k}={v}");
        }

        println!("  camera_following {}", i32::from(f.camera_following()));
        for e in &f.emitters {
            println!("  emitter {} {}", e.id, e.kind);
            props(e);
            consts(&build_emitter_constants(e, None));
            for c in &f.chains {
                consts(&build_emitter_constants(e, Some(c)));
            }
        }

        for c in &f.chains {
            println!("  chain {}", c.id);
            for m in &c.modifiers {
                println!("   modifier {}", m.fragment);
                props(m);
            }
        }

        for s in &f.stacks {
            let emitters = s.emitters.iter().map(|e| format!(" {e}")).collect::<String>();
            let chains = s.chains.iter().map(|c| format!(" {c}")).collect::<String>();
            println!("  stack {} svt={} shadows={} e:{emitters} c:{chains}", s.id, s.scene_view_type, i32::from(s.cast_shadows));
        }

        for r in &f.refs {
            println!("  ref {} {} {} {} {} {} {}", r.inst_name, r.library, b(r.scale_min), b(r.scale_max), bs(&r.pos), b(r.start_time), i32::from(r.infinite_life));
            for e in &r.emitters {
                println!("   ref_emitter {} {} {} {} {} {}", e.name, i32::from(e.enable), bs(&e.tint_min), bs(&e.tint_max), bs(&e.particle_lifetime_bias), bs(&e.emitter_lifetime_bias));
            }
        }
    }

    let reader = |path: &str| {
        let p = root.join(path);
        if Path::new(&p).exists() { std::fs::read(&p).map_err(|error| error.to_string()) } else { Err(format!("missing {path}")) }
    };

    let defs = XmlDoc::parse(std::fs::read(root.join("weather/weather_definitions.xml")).unwrap()).unwrap();
    let mut names = vec![String::new(), "no_such_weather".to_owned()];
    names.extend((0..defs.elements().len()).filter(|&i| defs.elements()[i].name == "WEATHER_DEFINITION").map(|i| defs.get(i, "name").unwrap_or_default()));
    for n in &names {
        let w = load_weather_vfx(&reader, n).unwrap();
        let d = &w.definition;
        println!("== weather {n}: {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}", d.name, d.kind, d.vfx1, d.vfx2, d.severity, d.type_index(),
            b(d.wind_size_min), b(d.wind_size_max), b(d.wind_gust_size_variation), b(d.wind_gust_duration), b(d.wind_gust_duration_variation),
            b(d.wind_gust_interval), b(d.wind_gust_probability), b(d.wind_gust_curve), b(d.wind_vegetation_multiplier));
        for a in &w.emitters {
            println!("  active {} {} {} chain={} tint={}{} life={} scale={} pos={} cam={}", a.library, a.instance, a.name, a.chain.id, bs(&a.tint_min),
                bs(&a.tint_max), bs(&a.particle_lifetime_bias), b(a.scale), bs(&a.pos), i32::from(a.camera_following));
            consts(&a.k);
        }

        for note in &w.notes {
            println!("  note {note}");
        }
    }
}
