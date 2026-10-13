//! Tests of the file system that don't need the game: a fake game folder with empty packs, and overlay folders.

use std::fs;
use std::path::Path;

use wce_vfs::{Config, GameProfile, Vfs, normalize};

/// A game folder with the given files under it, each with some content.
fn game_folder(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    dir
}

#[test]
fn pack_order_follows_mods_then_extra_vanilla_then_manifest() {
    let game = game_folder(&[
        ("used_mods.txt", "mod \"my_mod.pack\";\r\nmod \"missing.pack\";\r\n  mod \"second_mod.pack\" ;\r\nmodx \"not_a_mod.pack\";\r\n"),
        ("data/manifest.txt", "data.pack\t123\r\nBLOOD.pack\t1\r\nmovies/intro.pack\t2\r\nlocal_en.pack\t3\r\nnotes.txt\t4\r\n"),
        ("data/my_mod.pack", ""), ("data/second_mod.pack", ""), ("data/not_a_mod.pack", ""),
        ("data/data.pack", ""), ("data/blood.pack", ""), ("data/local_en.pack", ""), ("data/charlemagne.pack", ""),
    ]);

    let order = GameProfile::ATTILA.pack_order(game.path(), true);
    let names = order.packs.iter().map(|pack| (pack.name.as_str(), pack.is_mod)).collect::<Vec<_>>();
    assert_eq!(names, [
        ("my_mod.pack", true), ("second_mod.pack", true),
        ("charlemagne.pack", false), ("blood.pack", false),
        ("data.pack", false), ("local_en.pack", false),
    ]);
    assert_eq!(order.missing_mods, ["missing.pack"]);

    let without_mods = GameProfile::ATTILA.pack_order(game.path(), false);
    assert!(without_mods.packs.iter().all(|pack| !pack.is_mod));
    assert!(without_mods.missing_mods.is_empty());
}

#[test]
fn overlay_folders_go_first_and_paths_are_normalized() {
    let game = game_folder(&[("data/manifest.txt", "")]);
    let high = game_folder(&[("Terrain/Tiles/Tile.TED", "high"), ("only_high.xml", "a")]);
    let low = game_folder(&[("terrain/tiles/tile.ted", "low"), ("only_low.xml", "b")]);

    let mut config = Config::new(game.path(), GameProfile::ATTILA);
    config.overlay_folders = vec![high.path().to_path_buf(), low.path().to_path_buf()];
    let vfs = Vfs::mount(&config).unwrap();

    assert_eq!(vfs.packs().len(), 2);
    assert!(vfs.packs().iter().all(|pack| pack.loose));
    assert_eq!(vfs.owners("TERRAIN/TILES/TILE.TED"), [0, 1]);
    assert_eq!(vfs.read("terrain/tiles/tile.ted").unwrap(), b"high");
    assert_eq!(vfs.read_from(1, "terrain\\tiles\\tile.ted").unwrap(), b"low");
    assert_eq!(vfs.list("only_", ".xml"), ["only_high.xml", "only_low.xml"]);
    assert!(vfs.read("missing.xml").is_err());
    assert!(vfs.read_from(1, "only_high.xml").is_err());
}

#[test]
fn mounting_needs_the_game_folder() {
    assert!(Vfs::mount(&Config::new(Path::new("/does/not/exist"), GameProfile::ATTILA)).is_err());
    assert_eq!(normalize("Data\\Fxc/A.FXC"), "data/fxc/a.fxc");
}
