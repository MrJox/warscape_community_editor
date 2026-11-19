use super::*;
use wce_editor_core::document::{Document, SaveTarget};
use wce_terrain::lf_region::LfRegion;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let root = self.0.canonicalize().unwrap();
        assert!(
            root.starts_with(std::env::temp_dir().canonicalize().unwrap())
                && root.file_name().unwrap().to_string_lossy().starts_with("wce_lf_region_loader_")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn tiny_map() -> wce_terrain_io::MapFiles {
    let mut tif = b"II*\0".to_vec();
    tif.extend(8u32.to_le_bytes());
    tif.extend(6u16.to_le_bytes());
    for (tag, kind, count, value) in
        [(256u16, 3u16, 1u32, 130u32), (257, 3, 1, 67), (258, 3, 1, 16), (259, 3, 1, 1), (273, 4, 1, 86), (277, 3, 1, 1)]
    {
        tif.extend(tag.to_le_bytes());
        tif.extend(kind.to_le_bytes());
        tif.extend(count.to_le_bytes());
        tif.extend(value.to_le_bytes());
    }
    tif.extend(0u32.to_le_bytes());
    tif.extend((0..130 * 67).flat_map(|i| (i as u16).to_le_bytes()));
    tif.extend(b"opaque tail");
    let png =
        wce_formats::png_file::PngFile { width: 2, height: 2, bit_depth: 8, color_type: 2, pixels: vec![0; 12], ..Default::default() }
            .encode()
            .unwrap();
    wce_terrain_io::MapFiles::from_bytes([
        ("tile_map.png".into(), png),
        ("lf_heights.tif".into(), tif),
        ("normal.dds".into(), b"opaque normal".to_vec()),
        ("config.xml".into(), b"<ROOT unknown=\"kept\"/>".to_vec()),
    ])
}

#[test]
fn lf_region_production_loader_save_reopen_retains_f1_metadata_and_project_override() {
    let root = std::env::temp_dir().join(format!(
        "wce_lf_region_loader_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let cleanup = Scratch(root);
    let root = &cleanup.0;
    let (kit, project) = (root.join("kit/raw_data"), root.join("project/raw_data"));
    let roots = [project.clone(), kit.clone()];
    let settings = kit.join(database_dir(MapKind::Campaign)).join("_settings.xml");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, b"<ROOT><RENDER_PARAMS lf_height=\"712\" vertical_offset=\"38.28\"/></ROOT>").unwrap();
    for (kind, name) in [(MapKind::Battle, "tiny_battle"), (MapKind::Campaign, "campaigns/tiny_campaign")] {
        let source = kit_map_dir(&kit, name);
        for (relative, blob) in tiny_map().folder {
            let path = source.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, blob.bytes().unwrap()).unwrap();
        }
        let inputs = |raw: PathBuf| OpenInputs {
            name: name.into(),
            game_dir: "terrain/battles/main_attila_map",
            raw,
            roots: roots.to_vec(),
            db: Arc::default(),
            render_lf: campaign_render_lf(&roots, kind),
            files: None,
            packs: None,
            built: None,
            game: &wce_foundation::game::GameProfile::ATTILA,
            install: None,
            restored_from: None,
        };
        let mut doc = open_map(&inputs(kit.clone())).unwrap().document();
        let expected = if kind == MapKind::Campaign { (712., 38.28) } else { (3000., 20.) };
        assert_eq!((doc.placed.lf_height, doc.placed.vertical_offset), expected);
        let region = LfRegion { x: 63, y: 63, width: 67, height: 4 };
        let pixels: Vec<u16> = (0..268).map(|i| (i as u16).wrapping_mul(7919)).collect();
        assert!(doc.import_lf_region_png(region, &wce_formats::lf_region_png::encode(67, 4, &pixels).unwrap(), None).unwrap());
        assert_eq!(doc.history().size(), 1);
        let target = SaveTarget { path: kit_map_dir(&project, name) };
        doc.save(&target).unwrap();
        let reopened = open_map(&inputs(project.clone())).unwrap().document();
        assert_eq!((reopened.placed.lf_height, reopened.placed.vertical_offset), expected);
        assert_eq!(region.read(reopened.lf_field(LfRole::Land).unwrap()).unwrap(), pixels);
        for resource in ["tile_map.png", "normal.dds", "config.xml"] {
            assert_eq!(std::fs::read(source.join(resource)).unwrap(), std::fs::read(target.path.join(resource)).unwrap());
        }
        if kind == MapKind::Campaign {
            let override_path = project.join(database_dir(kind)).join("_settings.xml");
            std::fs::create_dir_all(override_path.parent().unwrap()).unwrap();
            std::fs::write(override_path, b"<ROOT><RENDER_PARAMS lf_height=\"1234\" vertical_offset=\"56\"/></ROOT>").unwrap();
            let overridden = open_map(&inputs(project.clone())).unwrap().document();
            assert_eq!((overridden.placed.lf_height, overridden.placed.vertical_offset), (1234., 56.));
            assert_eq!(region.read(overridden.lf_field(LfRole::Land).unwrap()).unwrap(), pixels);
        }
    }
}
