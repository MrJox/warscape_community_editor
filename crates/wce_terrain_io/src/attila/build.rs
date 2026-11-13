//! A battle map's built files (BOB_Terrain's `Tilemap` and `Heights & Normals` outputs, docs/re/bob.md) made from the Terrain tool's
//! model: what the game reads under `terrain/battles/<map>/`:
//!
//! - `tile_map.index`, `tile_map.tiles` (the final tile map) and `terrain.tile_list` (the battle field): BOB's fill of the tile map,
//!   climate map, explicit tiles and LF (`wce_tile::battle_map::build_images`);
//! - `lf_height_map.dds`, `lf_height_map.data`, `lf_sea_height_map.dds` ([`super::heights`]);
//! - `locations.blm`, `.mask0`, `.mask1` and `deployment/deployment_NNNN.xml` from the `.terry`, the tile map and the map's snow, culture
//!   and nogo images (`wce_terrain::locations`, `wce_terrain::deployment`);
//! - `tile_map.png` as the map has it (the game's packs carry it).
//!
//! - `normals/normal_N.dds`, the LF normal chunks an LF edit reaches ([`super::normals`]; the others stay the game's).

use wce_terrain::locations::{Image, MaskInputs};
use wce_terrain::{LayerRole, LfRole, MapModel};
use wce_tile::battle_map::fill::MapImages;
use wce_tile::battle_map::tile_db::TileDatabase;

use super::heights;

/// What the build reads besides the map: the battle tile database, the game's `environment_map.xml`
/// (`raw_data/warscape_asset_variation_db/`) and `tile_upgrades.xml` (`raw_data/terrain/tiles/battle/`).
pub struct BuildInputs<'a> {
    pub db: &'a TileDatabase,
    pub environment_map: &'a [u8],
    pub tile_upgrades: &'a [u8],
}

/// The built files: (path under the map's folder, bytes), and BOB's messages.
pub struct Built {
    pub files: Vec<(String, Vec<u8>)>,
    pub messages: Vec<String>,
}

fn rgb(l: &wce_terrain::cells::CellLayer) -> Vec<[u8; 3]> {
    l.cells()
        .iter()
        .map(|&i| {
            let c = l.palette()[usize::from(i)];
            [c[0], c[1], c[2]]
        })
        .collect()
}

fn rgba(l: &wce_terrain::cells::CellLayer) -> (u32, u32, Vec<[u8; 4]>) {
    let px = l
        .cells()
        .iter()
        .map(|&i| {
            let c = l.palette()[usize::from(i)];
            if l.alpha { c } else { [c[0], c[1], c[2], 255] }
        })
        .collect();
    (l.width(), l.height(), px)
}

/// An uncompressed 32-bit DDS (`nogo.dds`) as RGBA.
fn dds32(b: &[u8]) -> Option<(u32, u32, Vec<[u8; 4]>)> {
    let u = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..4) != Some(b"DDS ") || u(88)? != 32 {
        return None;
    }

    let (h, w) = (u(12)?, u(16)?);
    let px = b.get(128..128 + (w * h * 4) as usize)?;
    Some((w, h, px.chunks_exact(4).map(|c| [c[2], c[1], c[0], c[3]]).collect()))
}

/// The built files of a battle map's model.
///
/// # Errors
///
/// The model has no tile map, a carried file can't be read, or a BLM name/count cannot be encoded.
pub fn build(m: &MapModel, inputs: &BuildInputs) -> crate::Result<Built> {
    let tiles = m.layer(&LayerRole::TileMap).map(|l| l.get()).ok_or_else(|| crate::file_error("tile_map.png", "no tile map"))?;
    let (w, h) = (tiles.width(), tiles.height());
    let tile_rgb = rgb(tiles);
    let climate = m.layer(&LayerRole::Climate).map(|l| l.get()).filter(|l| l.width() == w && l.height() == h).map(rgb);
    let explicit: Option<String> = m.explicit.as_ref().map(|x| x.get().write().iter().map(|&c| char::from(c)).collect());
    let land = m.lf(LfRole::Land).map(|f| f.get());
    let lf: Option<(Vec<u16>, u32, u32)> =
        land.map(|f| (f.to_le_rows().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect(), f.width(), f.height()));
    let images = MapImages {
        width: w,
        height: h,
        tile_map: &tile_rgb,
        climate_map: climate.as_deref(),
        explicit_tiles: explicit.as_deref(),
        lf: None,
        lf_u16: lf.as_ref().map(|(v, fw, fh)| (v.as_slice(), *fw, *fh)),
        campaign: m.kind == wce_terrain::MapKind::Campaign,
    };

    let (tile_map, tile_list, mut messages) = wce_tile::battle_map::build_images(inputs.db, &images);
    let mut files = vec![];
    let (index, tiles_file) = tile_map.write();
    files.push(("tile_map.index".to_owned(), index));
    files.push(("tile_map.tiles".to_owned(), tiles_file));
    files.push(("terrain.tile_list".to_owned(), tile_list.write()));
    if let Some(f) = land {
        if m.kind == wce_terrain::MapKind::Campaign {
            // The campaign's LF halved (and halved again for the low-quality one).
            files.push(("lf_height_map.dds".to_owned(), heights::campaign_lf_dds(f, 1)));
            files.push(("lf_height_map_lq.dds".to_owned(), heights::campaign_lf_dds(f, 2)));
            files.push(("lf_height_map.data".to_owned(), heights::campaign_lf_data(f, (w, h))));
        } else {
            files.push(("lf_height_map.dds".to_owned(), heights::lf_height_dds(f)));
            files.push(("lf_height_map.data".to_owned(), heights::lf_height_data(f, (w, h))));
        }
    }

    // The LF normal chunks an LF edit reaches (none for an LF as the map's file has it: the game's stay).
    if let Some(item) = m.lf(LfRole::Land).filter(|i| i.edited()) {
        let original = item.origin.bytes().ok().and_then(|b| super::normals::tiff_samples(&b)).unwrap_or_default();
        let base = m
            .carried
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("normal.dds"))
            .and_then(|(_, b)| b.bytes().ok())
            .and_then(|b| super::normals::NormalImage::from_dds(&b));
        files.extend(super::normals::chunks(item.get(), &original, base.as_ref(), (w, h)));
    }

    if let Some(f) = m.lf(LfRole::Sea) {
        files.push(("lf_sea_height_map.dds".to_owned(), heights::lf_sea_height_dds(f.get())));
    }

    // A campaign map (BOB's Tilemap skips its masks and climates when the path names the campaigns): an empty locations.blm and masks,
    // no deployment files; the props, VFX and environment volumes of its .terry instead.
    if m.kind == wce_terrain::MapKind::Campaign {
        let mut blm = b"BLMF".to_vec();
        for v in [8u32, 0, w, h, 0, 0] {
            blm.extend_from_slice(&v.to_le_bytes());
        }

        files.push(("locations.blm".to_owned(), blm));
        files.push(("locations.blm.mask0".to_owned(), vec![]));
        files.push(("locations.blm.mask1".to_owned(), vec![]));
        if let Some(p) = m.project.as_ref().map(|p| p.get()) {
            let err = |e: String| crate::file_error("markers", e);
            files.push(("prop_marker.markers".to_owned(), super::campaign::prop_markers(p).map_err(err)?));
            files.push((
                "particle_emitter_marker.markers".to_owned(),
                super::campaign::particle_markers(p).map_err(|e| crate::file_error("markers", e))?,
            ));
            files.push(("environment_collection.xml".to_owned(), super::campaign::environment_collection(p)));
        }

        // Not BOB's: the game's snow masks, from the map folder's art (`snow_mask/*.tga`).
        for (path, blob) in &m.carried {
            if let Some(name) = super::campaign::snow_mask_name(path)
                && let Ok(bytes) = blob.bytes()
            {
                match super::campaign::snow_mask_dds(&bytes) {
                    Ok(dds) => files.push((name, dds)),
                    Err(e) => messages.push(format!("{path}: {e}")),
                }
            }
        }
    } else if let Some(p) = m.project.as_ref().map(|p| p.get()) {
        let climates = wce_terrain::locations::climate_environments(inputs.db, inputs.environment_map);
        let locations = wce_terrain::locations::build(p, w, h, &climates).write().map_err(|e| crate::file_error("locations.blm", e))?;
        files.push(("locations.blm".to_owned(), locations));

        let layer = |r: LayerRole| m.layer(&r).map(|l| rgba(l.get()));
        let snow = layer(LayerRole::Snow(String::new()));
        let seasons: Vec<_> = ["sp", "su", "au", "wi"].iter().map(|s| layer(LayerRole::Snow((*s).to_owned()))).collect();
        let culture = layer(LayerRole::Culture);
        let nogo =
            m.carried.iter().find(|(k, _)| k.eq_ignore_ascii_case("nogo.dds")).and_then(|(_, b)| b.bytes().ok()).and_then(|b| dds32(&b));
        fn img(i: &Option<(u32, u32, Vec<[u8; 4]>)>) -> Option<Image<'_>> {
            i.as_ref().map(|(w, h, px)| Image { width: *w, height: *h, pixels: px })
        }

        let mask_inputs = MaskInputs {
            snow: img(&snow),
            culture: img(&culture),
            culture_colours: wce_terrain::locations::culture_colours(inputs.tile_upgrades),
            nogo: img(&nogo),
            seasons: [img(&seasons[0]), img(&seasons[1]), img(&seasons[2]), img(&seasons[3])],
        };
        let (m0, m1) = wce_terrain::locations::masks(inputs.db, &tile_map, &mask_inputs);
        files.push(("locations.blm.mask0".to_owned(), wce_terrain::locations::mask_bytes(&m0)));
        files.push(("locations.blm.mask1".to_owned(), wce_terrain::locations::mask_bytes(&m1)));
        for (name, bytes) in wce_terrain::deployment::build(p) {
            files.push((format!("deployment/{name}"), bytes));
        }
    }

    // tile_map.png as the map's folder has it (edited: written as the folder's writer writes it).
    let folder = super::write(m)?;
    if let Some((_, b)) = folder.folder.iter().find(|(k, _)| k.eq_ignore_ascii_case("tile_map.png")) {
        files.push(("tile_map.png".to_owned(), b.bytes().map_err(|err| crate::file_error("tile_map.png", err))?.to_vec()));
    }

    Ok(Built { files, messages })
}
