//! Opt-in production-loader regression reusing an existing pristine Attila Assembly Kit.
//! WCE_DIAG_ROOT is a fresh isolated output; WCE_DIAG_RAW is read-only kit raw_data.
use super::*;
use std::io::Read;
use wce_editor_core::document::{Document, SaveTarget};
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::SourceRef;

fn equal_files(a: &std::path::Path, b: &std::path::Path) -> bool {
    if std::fs::metadata(a).unwrap().len() != std::fs::metadata(b).unwrap().len() {
        return false;
    }
    let (mut a, mut b) = (std::fs::File::open(a).unwrap(), std::fs::File::open(b).unwrap());
    let (mut x, mut y) = (vec![0; 1024 * 1024], vec![0; 1024 * 1024]);
    loop {
        let n = a.read(&mut x).unwrap();
        let m = b.read(&mut y).unwrap();
        if n != m || x[..n] != y[..m] {
            return false;
        }
        if n == 0 {
            return true;
        }
    }
}

fn compare(source: &std::path::Path, saved: &std::path::Path) -> Vec<String> {
    let files = wce_terrain_io::MapFiles::from_dir(source).unwrap();
    let saved_files = wce_terrain_io::MapFiles::from_dir(saved).unwrap();
    assert_eq!(files.folder.keys().collect::<Vec<_>>(), saved_files.folder.keys().collect::<Vec<_>>());
    files.folder.keys().filter_map(|name| (!equal_files(&source.join(name), &saved.join(name))).then(|| name.clone())).collect()
}

fn inputs(raw: PathBuf, roots: &[PathBuf]) -> OpenInputs {
    let folders: Vec<_> = roots.iter().map(|r| r.join(database_dir(MapKind::Campaign))).filter(|r| r.is_dir()).collect();
    OpenInputs {
        name: "campaigns/main_attila_map".into(),
        game_dir: "terrain/battles/main_attila_map",
        raw,
        db: Arc::new(TileDatabase::load_layered(&folders.iter().map(PathBuf::as_path).collect::<Vec<_>>()).unwrap()),
        render_lf: campaign_render_lf(roots, MapKind::Campaign),
        files: None,
        packs: None,
        built: None,
        game: &wce_foundation::game::GameProfile::ATTILA,
        install: None,
    }
}

fn heights(d: &MapDocument) -> Vec<f64> {
    [[50., 177.], [200., 200.], [335., 234.], [500., 300.]].into_iter().map(|[x, z]| d.lf_metres_at(x, z)).collect()
}

#[test]
#[ignore = "requires pristine Attila kit raw_data and a fresh isolated WCE_DIAG_ROOT"]
fn corrected_attila_real_save_reopen() {
    let root = PathBuf::from(std::env::var_os("WCE_DIAG_ROOT").unwrap());
    let kit = PathBuf::from(std::env::var_os("WCE_DIAG_RAW").unwrap());
    assert!(root.is_absolute() && kit.is_absolute());
    std::fs::create_dir_all(root.join("reports")).unwrap();
    let project = root.join("projects/verified/assembly_kit/raw_data");
    let saved = kit_map_dir(&project, "campaigns/main_attila_map");
    assert!(!saved.exists(), "fresh case required; never overwrite prior evidence");
    assert!(saved.starts_with(&root) && !kit.starts_with(&root));
    let roots = [project.clone(), kit.clone()];
    let expected = campaign_render_lf(&roots, MapKind::Campaign).unwrap();
    let source = kit_map_dir(&kit, "campaigns/main_attila_map");
    let mut rows = vec![format!("database_scale\t{expected:?}")];
    let mut doc = open_map(&inputs(kit.clone(), &roots)).unwrap().document();
    assert_eq!((doc.placed.lf_height, doc.placed.vertical_offset), expected);
    let original_heights = heights(&doc);
    rows.push(format!("pristine_heights\t{original_heights:?}"));
    let count = doc.project().unwrap().entities.len();
    rows.push(format!("entity_count\t{count}"));
    let prop = doc
        .project()
        .unwrap()
        .entities
        .iter()
        .find(|e| matches!(doc.project().unwrap().kind(e), wce_terrain::terry::EntityKind::Prop(_)))
        .unwrap()
        .id;
    let before = doc.project().unwrap().world_position(prop);
    doc.save_dir = Some(saved.clone());
    let target = SaveTarget { path: saved.clone() };
    doc.save(&target).unwrap();
    let changed = compare(&source, &saved);
    assert!(changed.is_empty());
    rows.push(format!("noop_changed\t{changed:?}"));
    assert!(!project.join(database_dir(MapKind::Campaign)).join("_settings.xml").exists());
    let reopened = open_map(&inputs(project.clone(), &roots)).unwrap().document();
    assert_eq!((reopened.placed.lf_height, reopened.placed.vertical_offset), expected);
    assert_eq!(heights(&reopened), original_heights);
    rows.push(format!("noop_reopen_scale\t{expected:?}\nnoop_reopen_heights\t{:?}", heights(&reopened)));
    drop(reopened);
    let src = SourceRef { doc: doc.core().id(), item: prop };
    let mut delta = Float4x4::IDENTITY;
    delta.m[0][3] = 20.;
    doc.transform_items(&[src], &delta, 1234);
    let moved = doc.project().unwrap().world_position(prop);
    assert!(moved[0] > before[0]);
    assert_eq!(doc.history().size(), 1);
    doc.history_mut().undo();
    assert_eq!(doc.project().unwrap().world_position(prop), before);
    doc.history_mut().redo();
    assert_eq!(doc.project().unwrap().world_position(prop), moved);
    assert_eq!(heights(&doc), original_heights);
    doc.save(&target).unwrap();
    doc.save(&target).unwrap();
    let changed = compare(&source, &saved);
    assert_eq!(changed, vec!["main_attila_map.terry"]);
    rows.push(format!("prop_id\t{prop:x}\nprop_before\t{before:?}\nprop_moved\t{moved:?}\nprop_changed\t{changed:?}"));
    let reopened = open_map(&inputs(project.clone(), &roots)).unwrap().document();
    assert_eq!((reopened.placed.lf_height, reopened.placed.vertical_offset), expected);
    assert_eq!(heights(&reopened), original_heights);
    assert_eq!(reopened.project().unwrap().world_position(prop), moved);
    rows.push(format!("prop_reopen_scale\t{expected:?}\nprop_reopened_position\t{moved:?}\nundo_redo\tpassed"));
    drop(reopened);
    // A genuine project database settings override: retain every kit setting, replace only render parameters.
    let settings = kit.join(database_dir(MapKind::Campaign)).join("_settings.xml");
    let mut xml = wce_formats::xml_doc::XmlDoc::parse(std::fs::read(settings).unwrap()).unwrap();
    let e = xml.elements().iter().position(|e| e.name == "RENDER_PARAMS").unwrap();
    xml.set(e, "lf_height", "1234");
    xml.set(e, "vertical_offset", "56");
    let override_path = project.join(database_dir(MapKind::Campaign)).join("_settings.xml");
    std::fs::create_dir_all(override_path.parent().unwrap()).unwrap();
    std::fs::write(&override_path, xml.save()).unwrap();
    let overridden = open_map(&inputs(project.clone(), &roots)).unwrap().document();
    assert_eq!((overridden.placed.lf_height, overridden.placed.vertical_offset), (1234., 56.));
    for (&actual, &original) in heights(&overridden).iter().zip(&original_heights) {
        let wanted = (original + expected.1) / expected.0 * 1234. - 56.;
        assert!((actual - wanted).abs() < 1e-10);
    }
    assert_eq!(overridden.project().unwrap().world_position(prop), moved);
    assert_eq!(compare(&source, &saved), vec!["main_attila_map.terry"]);
    rows.push(format!("project_override_scale\t1234\t56\nproject_override_heights\t{:?}", heights(&overridden)));
    // Keep the verified project ready for an owner check with the ordinary kit metadata.
    std::fs::rename(&override_path, override_path.with_file_name("_settings.override-tested.xml")).unwrap();
    let text = rows.join("\n");
    println!("{text}");
    std::fs::write(root.join("reports/attila-real-save-reopen.txt"), text).unwrap();
}
