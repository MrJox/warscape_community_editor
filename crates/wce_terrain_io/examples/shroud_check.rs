//! The campaign shroud (`wce_terrain_io::attila::shroud`) against a RenderDoc capture of the game's `s_shroud` texture: builds
//! `main_attila_map`'s shroud for a faction (default the Western Roman Empire, the captures' player) from the game's files and compares
//! it texel by texel with the captured texture (exact, within 2, within 16, mean difference).
//!
//!   cargo run --release -p wce_terrain_io --example shroud_check -- [map] [faction]
//!
//! `WCE_ATTILA_DIR` the game (default this machine's Steam folder); `WCE_SHROUD_CAPTURE` the captured texture (raw A8, 1485 x 1240, row
//! 0 the south edge; default `%TEMP%/wce_scratch/fow/rd/r6628_tex6382_m0.bin`, frame 6628, turn 1); `WCE_SHROUD_REFERENCE` optionally
//! the Python reference's output (same layout; default `%TEMP%/wce_scratch/fow/ref/wre.raw` if present); `WCE_SHROUD_PNG` optionally a
//! path to write ours as a north-up grey PNG.

use std::path::PathBuf;

use wce_terrain_io::attila::shroud;
use wce_vfs::{Config, GameProfile, Vfs};

fn compare(name: &str, ours: &[u8], theirs: &[u8]) {
    if ours.len() != theirs.len() {
        println!("{name}: {} bytes, ours {}", theirs.len(), ours.len());
        return;
    }
    let n = ours.len() as f64;
    let d: Vec<u32> = ours.iter().zip(theirs).map(|(a, b)| u32::from(a.abs_diff(*b))).collect();
    let within = |k: u32| d.iter().filter(|&&v| v <= k).count() as f64 / n;
    println!(
        "{name}: exact {:.4}, within 2 {:.4}, within 16 {:.4}, mean |d| {:.3}, max {}",
        within(0),
        within(2),
        within(16),
        d.iter().map(|&v| f64::from(v)).sum::<f64>() / n,
        d.iter().max().copied().unwrap_or(0)
    );
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let map = a.first().map_or("main_attila_map", String::as_str);
    let faction = a.get(1).map_or("att_fact_western_roman_empire", String::as_str);
    let game_dir = std::env::var_os("WCE_ATTILA_DIR")
        .map_or_else(|| PathBuf::from("D:/Juegos/Steam/steamapps/common/Total War Attila"), PathBuf::from);
    let fow = std::env::temp_dir().join("wce_scratch/fow");
    let cap_path = std::env::var_os("WCE_SHROUD_CAPTURE").map_or_else(|| fow.join("rd/r6628_tex6382_m0.bin"), PathBuf::from);
    let ref_path = std::env::var_os("WCE_SHROUD_REFERENCE").map_or_else(|| fow.join("ref/wre.raw"), PathBuf::from);

    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&game_dir, game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let schema =
        rpfm_lib::schema::Schema::load(&wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema"), None).expect("schema");
    let list = |p: &str| vfs.list(p, "").iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let read = |p: &str| vfs.read(p).ok().map(|b| b.to_vec());
    let startpos = |p: &str| read(p).or_else(|| std::fs::read(game_dir.join("data").join(p)).ok());

    let t = std::time::Instant::now();
    let s = shroud::read(&schema, map, Some(faction), &list, &read, &startpos).expect("shroud");
    println!(
        "campaign {}, faction {}, {} x {} (content {:?}, padding {}, world {:?}, uv offset {:?}), built in {:.2?}",
        s.campaign,
        s.faction,
        s.width,
        s.height,
        s.content,
        s.padding,
        s.world,
        s.uv_offset(),
        t.elapsed()
    );
    for n in &s.notes {
        println!("  {n}");
    }

    match std::fs::read(&cap_path) {
        Ok(cap) => compare(&format!("vs the capture {}", cap_path.display()), &s.pixels, &cap),
        Err(e) => println!("{}: {e}", cap_path.display()),
    }
    if let Ok(r) = std::fs::read(&ref_path) {
        compare(&format!("vs the reference {}", ref_path.display()), &s.pixels, &r);
    }

    if let Some(png) = std::env::var_os("WCE_SHROUD_PNG") {
        let pixels = s.pixels.chunks(s.width).rev().flatten().copied().collect();
        let file = wce_formats::png_file::PngFile {
            width: s.width as u32,
            height: s.height as u32,
            bit_depth: 8,
            color_type: 0,
            pixels,
            ..Default::default()
        };
        std::fs::write(&png, file.encode().expect("png")).expect("write the png");
        println!("wrote {}", PathBuf::from(png).display());
    }
}
