//! Gate of Phase 3 (docs/plans/tile_ted_parity.md): the data side of the battle terrain colour. Port of `cli/terrain_check.cpp`.
//!
//!   cargo run --release -p wce_tile_editor --example terrain_check -- <game dir> [tile folder]
//!
//!  1. DXT decoding: hand-made BC1 / BC3 blocks decode to the expected colours (the 4-colour and the 3-colour + transparent BC1 modes, both
//!     BC3 alpha ramps); a DDS built around them decodes per mip, with sizes that are not a multiple of 4.
//!  2. Climates: the kit's _settings.xml has the eight climates, ROME_Continental lists mud with 9 m per repeat; the texture path falls back
//!     from the climate to `default` and from a season to summer.
//!  3. The bake: the given tile (default: tdd_custom_lothlorien/1x1/aa) bakes with the game's textures: the image covers the bordered tile at
//!     4 texels per metre, all layers of that tile are found, it is not flat, two bakes are byte-identical, another climate and season give
//!     another image, and a climate without a group falls back to `default`. Needs the game's packs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wce_formats::dds_decode::{decode_bc1_block, decode_bc3_alpha_block, dds_decode_mip, dds_info};
use wce_formats::tile_climate::{ClimateList, climate_texture_path};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::terrain_bake::{BakeRequest, bake_terrain_colour};
use wce_vfs::{Config, GameProfile, Vfs};

#[derive(Default)]
struct Gate {
    failed: usize,
}

impl Gate {
    fn check(&mut self, ok: bool, what: &str) {
        if !ok {
            self.failed += 1;
            println!("FAILED: {what}");
        }
    }
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// A DDS with the given fourcc and the blocks of every mip equal to `blk`.
fn make_dds(fourcc: &[u8; 4], w: u32, h: u32, mips: u32, blk: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..4].copy_from_slice(b"DDS ");
    put32(&mut b, 4, 124);
    put32(&mut b, 12, h);
    put32(&mut b, 16, w);
    put32(&mut b, 28, mips);
    put32(&mut b, 76, 32);
    put32(&mut b, 80, 4);
    b[84..88].copy_from_slice(fourcc);
    let (mut mw, mut mh) = (w, h);
    for _ in 0..mips {
        let blocks = mw.div_ceil(4).max(1) as usize * mh.div_ceil(4).max(1) as usize;
        for _ in 0..blocks {
            b.extend_from_slice(blk);
        }

        mw = (mw / 2).max(1);
        mh = (mh / 2).max(1);
    }

    b
}

fn test_dxt(g: &mut Gate) {
    // BC1: c0 = 0xF800 (red), c1 = 0x001F (blue), indices 0, 1, 2, 3 in the first row.
    let px = decode_bc1_block(&[0x00, 0xF8, 0x1F, 0x00, 0xE4, 0x00, 0x00, 0x00], true);
    g.check(px[0] == [255, 0, 0, 255], "BC1 index 0 is c0 (red)");
    g.check(px[1][0] == 0 && px[1][2] == 255, "BC1 index 1 is c1 (blue)");
    g.check(px[2][0] == 170 && px[2][2] == 85, "BC1 index 2 is 2/3 c0 + 1/3 c1");
    g.check(px[3][0] == 85 && px[3][2] == 170, "BC1 index 3 is 1/3 c0 + 2/3 c1");

    // 3-colour mode: c0 <= c1, index 3 transparent.
    let px = decode_bc1_block(&[0x1F, 0x00, 0x00, 0xF8, 0xE4, 0x00, 0x00, 0x00], true);
    g.check(px[2][0] == 127 && px[2][2] == 127 && px[2][3] == 255, "BC1 3-colour mode: index 2 is the middle");
    g.check(px[3][3] == 0, "BC1 3-colour mode: index 3 is transparent");

    // BC3 alpha: a0 > a1: 8 values; a0 <= a1: 6 values + 0 and 255 (indices 0 1 2 3 4 5 6 7 in the first row, 3 bits each).
    let a = decode_bc3_alpha_block(&[255, 0, 0x88, 0xC6, 0xFA, 0, 0, 0]);
    g.check(a[0] == 255 && a[1] == 0 && a[2] == 218 && a[7] == 36, "BC3 alpha, 8-value ramp");
    let a = decode_bc3_alpha_block(&[50, 100, 0x88, 0xC6, 0xFA, 0, 0, 0]);
    g.check(a[0] == 50 && a[1] == 100 && a[6] == 0 && a[7] == 255, "BC3 alpha, 6-value ramp with 0 and 255");

    // A whole DDS: 10 x 6 (not a multiple of 4), 3 mips, DXT5 all red opaque.
    let mut blk = vec![0u8; 16];

    // Alpha 255 / 255: every index gives 255; c0 and c1 red.
    blk[0] = 255;
    blk[1] = 255;
    blk[9] = 0xF8;
    blk[11] = 0xF8;
    let dds = make_dds(b"DXT5", 10, 6, 3, &blk);
    let info = dds_info(&dds);
    g.check(info.format == "DXT5" && info.width == 10 && info.height == 6 && info.mips == 3, "dds_info of a DXT5 file");
    let m0 = dds_decode_mip(&dds, 0);
    g.check(m0.as_ref().is_ok_and(|im| im.w == 10 && im.h == 6), "decode mip 0 (10 x 6)");
    g.check(m0.as_ref().is_ok_and(|im| im.w == 10 && im.at(9, 5)[0] == 255 && im.at(9, 5)[1] == 0 && im.at(9, 5)[3] == 255), "the last pixel of a partial block is decoded");
    g.check(dds_decode_mip(&dds, 1).is_ok_and(|im| im.w == 5 && im.h == 3), "decode mip 1 (5 x 3)");
    g.check(dds_decode_mip(&dds, 9).is_ok_and(|im| im.w == 2 && im.h == 1), "a mip past the end is the last one");
    g.check(dds_decode_mip(&[0u8; 40], 0).is_err_and(|why| !why.is_empty()), "garbage is refused with a reason");
}

fn test_climates(g: &mut Gate, settings: &Path) {
    let cl = ClimateList::load(settings).unwrap_or_default();
    g.check(cl.climates.len() == 8, &format!("the kit has 8 climates ({})", cl.climates.len()));
    let c = cl.find("rome_continental");
    g.check(c.is_some(), "ROME_Continental is found, in any case");
    if let Some(c) = c {
        let mud = c.find("mud");
        g.check(mud.is_some_and(|m| m.blend_pixel_scale > 8.9 && m.blend_pixel_scale < 9.1), "ROME_Continental: mud repeats every 9 m");
        g.check(c.folder() == "terrain/textures/battle/rome_continental", "climate folder");
    }

    let have = BTreeSet::from(["terrain/textures/battle/default/rock/0_su_diffuse.dds", "terrain/textures/battle/rome_med/grass/1_su_diffuse.dds"]);
    let ex = |p: &str| have.contains(p);
    g.check(
        climate_texture_path("terrain/textures/battle/rome_med", "grass", 1, "wi", false, ex).as_deref() == Some("terrain/textures/battle/rome_med/grass/1_su_diffuse.dds"),
        "season falls back to summer",
    );
    g.check(
        climate_texture_path("terrain/textures/battle/rome_med", "rock", 0, "su", false, ex).as_deref() == Some("terrain/textures/battle/default/rock/0_su_diffuse.dds"),
        "a group the climate lacks falls back to default",
    );
    g.check(climate_texture_path("terrain/textures/battle/rome_med", "snow", 0, "su", false, ex).is_none(), "nothing found gives an empty path");
}

fn test_bake(g: &mut Gate, game: &Path, tile_dir: &Path) -> Result<(), String> {
    let vfs = Vfs::mount(&Config::new(game.to_path_buf(), GameProfile::ATTILA)).map_err(|e| e.to_string())?;
    let project = TileProject::open(tile_dir).map_err(|e| e.to_string())?;
    g.check(project.has_entry() && project.bordered(), "the tile has an entry and a bordered height map");
    let Some(entry) = &project.entry else { return Ok(()) };
    let settings = entry.file().parent().and_then(Path::parent).map(|p| p.join("_settings.xml")).unwrap_or_default();
    let cl = ClimateList::load(&settings).unwrap_or_default();
    let b = project.border_m();
    let rq = BakeRequest {
        blend_map: project.dir.join("blend_map.tif"), groups: entry.texture_group(), x0: -b, z0: -b, w_m: project.extent_x() + 2.0 * b,
        h_m: project.extent_z() + 2.0 * b, ..Default::default()
    };

    let a = bake_terrain_colour(&rq, &cl, &vfs);
    g.check(a.is_ok(), &format!("the bake succeeds ({})", a.as_ref().err().cloned().unwrap_or_default()));
    let Ok(a) = a else { return Ok(()) };
    g.check(
        a.w == (rq.w_m * 4.0).round() as u32 || a.w.max(a.h) == rq.max_dim,
        "the image covers the bordered tile at 4 texels per metre (or the size cap)",
    );
    g.check(a.missing == 0 && a.layers >= 4, &format!("all layers' textures were found ({} layers, {} missing)", a.layers, a.missing));
    let mut seen = BTreeSet::new();
    let mut i = 0;
    while i < a.rgba.len() && seen.len() < 64 {
        seen.insert(u32::from_le_bytes(a.rgba[i..i + 4].try_into().expect("4 bytes")));
        i += 4 * 997;
    }

    g.check(seen.len() > 20, &format!("the ground colour is not flat ({} distinct sample values)", seen.len()));
    let again = bake_terrain_colour(&rq, &cl, &vfs);
    g.check(again.is_ok_and(|x| x.rgba == a.rgba), "two bakes are byte-identical");

    // Another climate and season give another image.
    let mut rq2 = rq.clone();
    rq2.look.climate = "ROME_Desert".to_owned();
    rq2.look.season = "au".to_owned();
    g.check(bake_terrain_colour(&rq2, &cl, &vfs).is_ok_and(|o| o.rgba != a.rgba && o.w == a.w), "another climate and season give another image");

    // An unknown climate falls back to the default textures instead of failing.
    let mut rq3 = rq.clone();
    rq3.look.climate = "no_such_climate".to_owned();
    let fallback = bake_terrain_colour(&rq3, &cl, &vfs);
    g.check(fallback.is_ok(), &format!("an unknown climate falls back to default ({})", fallback.err().unwrap_or_default()));
    println!("bake: {}", a.report);
    Ok(())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(game) = args.first().map(PathBuf::from) else {
        println!("usage: terrain_check <game dir> [tile folder]");
        std::process::exit(2);
    };

    let kit = game.join("assembly_kit_battle_tilemap").join("raw_data").join("terrain").join("tiles").join("battle");
    let mut g = Gate::default();
    test_dxt(&mut g);
    test_climates(&mut g, &kit.join("_tile_database").join("_settings.xml"));
    let tile = args.get(1).map_or_else(|| kit.join("tdd_custom_lothlorien").join("1x1").join("aa"), PathBuf::from);
    if let Err(e) = test_bake(&mut g, &game, &tile) {
        println!("error: {e}");
        std::process::exit(1);
    }

    println!("{}", if g.failed != 0 { "terrain_check: FAILED" } else { "terrain_check: ok" });
    std::process::exit(i32::from(g.failed != 0));
}
