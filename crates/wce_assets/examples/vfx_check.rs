//! Builds the particle emitters of a weather definition and compares their constants with a game memory dump. Port of `cli/vfx_check.cpp`.
//!
//!   cargo run --release -p wce_assets --example vfx_check -- "<weather definition name>" [dump_cbs.py --array=512 output.txt]
//!
//! Without a dump file: lists the active emitters and their constant rows. With one: for every emitter finds the dumped row (index into the
//! [512] arrays) that matches best and prints OK / DIFF per variable.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wce_assets::vfx_constants::EmitterConstants;
use wce_assets::weather_vfx::load_weather_vfx_from_vfs;
use wce_formats::c_num::{printf_g, printf_g_prec, strtod_prefix};
use wce_foundation::settings::Settings;
use wce_vfs::{Config, GameProfile, Vfs};

#[derive(Clone, Copy, Default)]
struct Row {
    f: [f32; 4],
    u: [u32; 4],
}

/// index -> variable -> values
type DumpRows = BTreeMap<i64, BTreeMap<String, Row>>;

fn read_rows(path: &str) -> DumpRows {
    let mut rows = DumpRows::new();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let (Some(lb), Some(rb), Some(dot)) = (line.find('['), line.find(']'), line.find('.')) else { continue };
        if dot > lb {
            continue;
        }

        let var = line[dot + 1..lb].to_owned();
        let idx = wce_formats::c_num::strtol(&line[lb + 1..]).unwrap_or(0);
        let mut r = Row::default();
        for (i, tok) in line[rb + 1..].split_whitespace().take(4).enumerate() {
            if let Some(n) = tok.strip_prefix('#') {
                r.u[i] = n.parse::<u64>().unwrap_or(0) as u32;
                r.f[i] = f32::from_bits(r.u[i]);
            } else {
                r.f[i] = strtod_prefix(tok).map_or(0.0, |v| v.0) as f32;
                r.u[i] = r.f[i].to_bits();
            }
        }

        rows.entry(idx).or_default().insert(var, r);
    }

    rows
}

/// A constant row of an emitter: floats or unsigned integers.
enum Values<'a> {
    F(&'a [f32; 4]),
    U(&'a [u32; 4]),
}

fn mine(k: &EmitterConstants) -> Vec<(&'static str, Values<'_>)> {
    use Values::{F, U};
    vec![
        ("behaviour_mask", U(&k.behaviour_mask)), ("vs_translation_params", F(&k.vs_translation_params)), ("vs_translation_params1", F(&k.vs_translation_params1)),
        ("ps_sim_game_logic_params", F(&k.ps_sim_game_logic_params)), ("ps_sim_gravity_params", F(&k.ps_sim_gravity_params)),
        ("ps_sim_deceleration_params", F(&k.ps_sim_deceleration_params)), ("ps_sim_following_emitter_params", F(&k.ps_sim_following_emitter_params)),
        ("ps_sim_turbulence_params", F(&k.ps_sim_turbulence_params)), ("ps_lighting_legacy_params", F(&k.ps_lighting_legacy_params)),
        ("ps_lighting_params", F(&k.ps_lighting_params)), ("ps_soft_edge_params", F(&k.ps_soft_edge_params)), ("vs_culling_distances", F(&k.vs_culling_distances)),
        ("vs_rotation_params", F(&k.vs_rotation_params)), ("vs_atlas_texture_params", F(&k.vs_atlas_texture_params)),
        ("vs_cell_texture_params", U(&k.vs_cell_texture_params)), ("vs_animated_texture_params", U(&k.vs_animated_texture_params)),
        ("vs_animated_texture_params1", F(&k.vs_animated_texture_params1)),
    ]
}

fn same(m: &Values, r: &Row, i: usize) -> bool {
    match m {
        Values::U(u) => u[i] == r.u[i],
        Values::F(f) => (f[i] - r.f[i]).abs() <= 1e-4 * r.f[i].abs().max(1.0),
    }
}

fn components(m: &Values, row: Option<&Row>) -> String {
    (0..4)
        .map(|i| match (m, row) {
            (Values::U(u), None) => format!(" #{}", u[i]),
            (Values::F(f), None) => format!(" {}", printf_g(f64::from(f[i]))),
            (Values::U(_), Some(r)) => format!(" #{}", r.u[i]),
            (Values::F(_), Some(r)) => format!(" {}", printf_g(f64::from(r.f[i]))),
        })
        .collect()
}

fn run(args: &[String]) -> Result<i32, String> {
    let game = Settings::load(&Settings::default_dir()).get("game_dir", Some("WCE_GAME")).ok_or("the game folder is not set (game_dir in wce.ini or WCE_GAME)")?;
    let vfs = Vfs::mount(&Config::new(PathBuf::from(game), GameProfile::ATTILA)).map_err(|e| e.to_string())?;
    let w = load_weather_vfx_from_vfs(&vfs, &args[0])?;
    let d = &w.definition;
    println!(
        "definition '{}' vfx1 '{}' type {} wind {}..{}: {} emitters", d.name, d.vfx1, d.kind, printf_g_prec(f64::from(d.wind_size_min), 3),
        printf_g_prec(f64::from(d.wind_size_max), 3), w.emitters.len(),
    );
    for n in &w.notes {
        println!("  note: {n}");
    }

    let mut rows = args.get(1).map(|p| read_rows(p)).unwrap_or_default();
    let mut total_diff = 0;
    for e in &w.emitters {
        let vars = mine(&e.k);
        println!("\n{} / {} / {}  diffuse={}", e.library, e.instance, e.name, e.k.diffuse);
        if rows.is_empty() {
            for (name, m) in &vars {
                println!("  {name:<34}{}", components(m, None));
            }

            continue;
        }

        let (mut best, mut best_score) = (-1i64, -1i32);
        for (&idx, rv) in &rows {
            let score = vars.iter().map(|(name, m)| {
                let r = rv.get(*name).copied().unwrap_or_default();
                (0..4).filter(|&i| same(m, &r, i)).count() as i32
            }).sum::<i32>();
            if score > best_score {
                best_score = score;
                best = idx;
            }
        }

        println!("  best dump row {best} ({best_score} of {} components)", vars.len() * 4);
        let best_row = rows.entry(best).or_default();
        for (name, m) in &vars {
            let r = best_row.get(*name).copied().unwrap_or_default();
            if (0..4).all(|i| same(m, &r, i)) {
                continue;
            }

            total_diff += 1;
            println!("  DIFF {name:<32} mine:{}   game:{}", components(m, None), components(m, Some(&r)));
        }
    }

    Ok(i32::from(total_diff != 0))
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        println!("usage: vfx_check \"<weather definition>\" [dump rows.txt]");
        std::process::exit(2);
    }

    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}
