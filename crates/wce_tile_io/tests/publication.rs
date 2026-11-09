use std::{collections::BTreeMap,sync::Arc};
use wce_tile_io::TileFiles;
use wce_tile_model::Blob;
#[test]fn step_c_tile_folder_and_entry_stage_together_and_exclude_leftovers(){
    let root=std::env::temp_dir().join(format!("wce-tile-io-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));let folder=root.join("tile");let entry=root.join("entries");std::fs::create_dir_all(&folder).unwrap();std::fs::write(folder.join("old.wce_tmp"),b"leftover").unwrap();
    assert!(TileFiles::from_dir(&folder).unwrap().folder.is_empty());
    let mut files=TileFiles{folder:BTreeMap::from([("native.bin".into(),Blob::Bytes(Arc::from(&b"new"[..])))]),entry:Some(("entry.xml".into(),Blob::File(root.join("missing")))),..Default::default()};
    assert!(files.write_to(&folder,Some(&entry)).is_err());assert!(!folder.join("native.bin").exists());
    files.entry=Some(("entry.xml".into(),Blob::Bytes(Arc::from(&b"entry"[..]))));files.write_to(&folder,Some(&entry)).unwrap();assert_eq!(std::fs::read(folder.join("native.bin")).unwrap(),b"new");assert_eq!(std::fs::read(entry.join("entry.xml")).unwrap(),b"entry");
    files.folder.insert("escape.wce_tmp".into(),Blob::Bytes(Arc::from(&b"reserved"[..])));assert!(files.write_to(&folder,None).is_err());
    let canonical=root.canonicalize().unwrap();assert!(canonical.starts_with(std::env::temp_dir().canonicalize().unwrap())&&canonical.file_name().unwrap().to_string_lossy().starts_with("wce-tile-io-"));std::fs::remove_dir_all(canonical).unwrap();
}
