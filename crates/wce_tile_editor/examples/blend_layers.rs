//! Which of a tile's 16 blend layers (4 texture groups x 4 variations) its `blend_map.tif` uses, how much, and what each draws in a climate:
//! the texture's path (`ClimateList::texture_path`) and its mean colour (its 64-texel mip or the nearest).
//!
//!   cargo run --release -p wce_tile_editor --example blend_layers -- <game dir> <database _settings.xml> <tile db entry .xml> <variation folder> <climate> [season]

use wce_formats::tile_climate::ClimateList;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::new(&a[0], GameProfile::from_env().clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let climates = ClimateList::load(std::path::Path::new(&a[1])).expect("_settings.xml");
    let entry = std::fs::read_to_string(&a[2]).expect("tile entry");
    let group_line = entry.lines().find(|l| l.contains("<TEXTURE_GROUP")).unwrap_or_default().to_owned();
    let attr = |k: &str| group_line.split(&format!("{k}='")).nth(1).and_then(|s| s.split('\'').next()).unwrap_or("").to_owned();
    let groups = [attr("red"), attr("green"), attr("blue"), attr("alpha")];
    let blend = wce_tile::blend_field::BlendField::read(&std::path::Path::new(&a[3]).join("blend_map.tif")).expect("blend_map.tif");
    let (climate_name, season) = (&a[4], a.get(5).map_or("su", String::as_str));
    let climate = climates.find(climate_name);
    let folder = climate.map_or_else(|| climates.default_folder(), |c| climates.folder_of(c));
    println!("groups {groups:?}; climate {climate_name} folder {folder}; blend {} x {}", blend.width(), blend.height());
    let mut sums = [0f64; 16];
    let n = (blend.width() * blend.height()) as f64;
    for p in blend.weights().as_chunks::<16>().0 {
        let t: f64 = p.iter().map(|&v| f64::from(v)).sum::<f64>().max(1.0);
        for k in 0..16 {
            sums[k] += f64::from(p[k]) / t;
        }
    }

    for k in 0..16 {
        if sums[k] / n < 0.001 {
            continue;
        }

        let g = climates.group_of(climate, &groups[k / 4]).to_owned();
        // A campaign database's textures are under `terrain/textures/campaign/default` (docs/re/terry.md).
        let path = climates.texture_path(&folder, &g, (k % 4) as i32, season, false, |p| vfs.read(p).is_ok())
            .or_else(|| Some(format!("terrain/textures/campaign/default/{g}/{}_{season}_diffuse.dds", k % 4)).filter(|p| vfs.read(p).is_ok()));
        let mean = path.as_ref().and_then(|p| vfs.read(p).ok()).and_then(|b| {
            let info = wce_formats::dds_decode::dds_info(&b);
            let mip = (0..info.mips.max(1)).find(|&m| (info.width >> m).max(1) <= 64).unwrap_or(0);
            wce_formats::dds_decode::dds_decode_mip(&b, mip).ok()
        }).map(|img| {
            let px = &img.px;
            let c = px.len() / 4;
            [0, 1, 2].map(|i| px.as_chunks::<4>().0.iter().map(|q| u64::from(q[i])).sum::<u64>() / c.max(1) as u64)
        });
        println!("  channel {} ({}) variation {}: {:.1} % of the tile -> {g}: {path:?} mean {mean:?}", k / 4, groups[k / 4], k % 4, 100.0 * sums[k] / n);
    }
}
