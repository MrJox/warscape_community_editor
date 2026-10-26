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

/// The first bytes of a pack of a type (`PFHFileType`: 1 release, 2 patch).
fn pack_header(kind: u8) -> String {
    String::from_utf8(vec![b'P', b'F', b'H', b'4', kind, 0, 0, 0]).unwrap()
}

#[test]
fn patch_packs_come_before_the_release_packs() {
    let (release, patch) = (pack_header(1), pack_header(2));
    let game = game_folder(&[
        ("data/data.pack", &release), ("data/data_rome2.pack", &patch), ("data/tiles.pack", &release), ("data/tiles_rome2.pack", &patch),
        ("data/my_mod.pack", &pack_header(3)),
    ]);

    let order = GameProfile::ROME_2.pack_order_with(game.path(), &["my_mod.pack".to_owned()]);
    let names = order.packs.iter().map(|pack| pack.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["my_mod.pack", "data_rome2.pack", "tiles_rome2.pack", "data.pack", "tiles.pack"]);
    assert_eq!(GameProfile::ROME_2.available_mod_packs(game.path()), ["my_mod.pack"]);
}

#[test]
fn an_empty_file_removes_the_file_of_the_packs_below() {
    use rpfm_lib::files::pack::Pack;
    use rpfm_lib::files::{Container, FileType, RFile};
    use rpfm_lib::games::pfh_file_type::PFHFileType;
    use rpfm_lib::games::supported_games::SupportedGames;

    let game = game_folder(&[]);
    let games = SupportedGames::default();
    let info = games.game(GameProfile::ROME_2.key).unwrap();
    let write = |name: &str, kind: PFHFileType, files: &[(&str, &[u8])]| {
        let mut pack = Pack::new_with_name_and_version(name, info.pfh_version_by_file_type(kind));
        pack.set_pfh_file_type(kind);
        for (path, data) in files {
            pack.insert(RFile::new_from_vec(data, FileType::Unknown, 0, path)).unwrap();
        }

        fs::create_dir_all(game.path().join("data")).unwrap();
        pack.save(Some(&game.path().join("data").join(name)), info, &None).unwrap();
    };

    write("data.pack", PFHFileType::Release, &[("fxc/a.fxc", b"old"), ("fxc/b.fxc", b"kept"), ("fxc/c.fxc", b"old")]);
    write("data_rome2.pack", PFHFileType::Patch, &[("fxc/a.fxc", b""), ("fxc/c.fxc", b"new")]);

    let vfs = Vfs::mount(&Config::new(game.path(), GameProfile::ROME_2)).unwrap();
    assert_eq!(vfs.profile().key, "rome_2");
    assert_eq!(vfs.packs().iter().map(|pack| pack.name.as_str()).collect::<Vec<_>>(), ["data_rome2.pack", "data.pack"]);
    assert!(vfs.read("fxc/a.fxc").is_err());
    assert_eq!(vfs.list("fxc/", ".fxc"), ["fxc/b.fxc", "fxc/c.fxc"]);
    assert_eq!(vfs.read("fxc/c.fxc").unwrap(), b"new");
    assert_eq!(vfs.read_from(1, "fxc/a.fxc").unwrap(), b"old");
}

#[test]
fn files_go_into_a_mod_pack_replacing_their_folder() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("data/my_tile.pack");
    let game = &GameProfile::ROME_2;
    let file = |p: &str, d: &[u8]| (p.to_owned(), d.to_vec());
    wce_vfs::mod_pack::write_files(game, &pack, &[], &[file("terrain/tiles/battle/a/aa/old.markers", b"1"), file("terrain/tiles/battle/b/aa/x", b"2")]).unwrap();
    wce_vfs::mod_pack::write_files(game, &pack, &["terrain/tiles/battle/a/aa"], &[file("terrain/tiles/battle/a/aa/new.markers", b"3")]).unwrap();

    let mut config = Config::new(dir.path(), game.clone());
    config.mod_packs = Some(vec!["my_tile.pack".to_owned()]);
    let vfs = Vfs::mount(&config).unwrap();
    assert_eq!(vfs.list("terrain/tiles/", ""), ["terrain/tiles/battle/a/aa/new.markers", "terrain/tiles/battle/b/aa/x"]);
    assert!(vfs.packs()[0].is_mod);
}
