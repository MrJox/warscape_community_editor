//! Which files the editor reads, and in which order. Uses the installed game but asserts rules, not its contents: it picks the skins and
//! mod packs it needs from what is installed and says SKIPPED when the install has none to test with.
//!
//!   cargo run --release -p wce_ui_model --example sources_check -- --game <game folder>
//!
//! Checks:
//!  * the release packs and the mod packs one can choose from are disjoint, and a mount with no mods chosen has no mod pack in it
//!    (the launcher's list of enabled mods is not used);
//!  * a chosen mod pack is mounted, as a mod, above every release pack; one that is not installed is reported missing and not mounted;
//!  * a loose folder is read before the packs: a file of it replaces the pack's file of the same path, and the pack's shows again without it;
//!  * the skin list: a loose folder of a new name is loose only, one of a name the packs have is loose and packed, an empty folder counts;
//!    and a loose file that shadows every pack file of a skin does not hide that the packs have it.

use std::path::{Path, PathBuf};

use wce_ui_model::skins;
use wce_vfs::{Config, GameProfile, Vfs};

struct T {
    failed: usize,
}

impl T {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {what}", if ok { "ok     " } else { "FAILED " });
        if !ok {
            self.failed += 1;
        }
    }
}

fn mount(game: &Path, mods: Vec<String>, overlay: Vec<PathBuf>) -> Vfs {
    let mut c = Config::new(game, GameProfile::ATTILA);
    c.use_mods = false;
    c.mod_packs = Some(mods);
    c.overlay_folders = overlay;
    Vfs::mount(&c).expect("mount")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut game = None;
    while let Some(a) = args.next() {
        if a == "--game" {
            game = args.next();
        }
    }

    let game = PathBuf::from(game.expect("--game <folder>"));
    let profile = GameProfile::ATTILA;
    let mut t = T { failed: 0 };

    // ---- which packs
    let release = profile.release_pack_names(&game);
    let available = profile.available_mod_packs(&game);
    t.check(!release.is_empty(), "the game has release packs");
    t.check(available.iter().all(|m| !release.iter().any(|r| r.eq_ignore_ascii_case(m))), "release packs and chooseable mod packs do not overlap");

    let none = mount(&game, vec![], vec![]);
    t.check(none.packs().iter().all(|p| !p.is_mod), "with no mod chosen no mod pack is mounted");
    t.check(none.packs().len() == release.len(), "with no mod chosen exactly the release packs are mounted");

    let ghost = mount(&game, vec!["no_such_mod_wce_check.pack".into()], vec![]);
    t.check(ghost.missing_mods().iter().any(|m| m == "no_such_mod_wce_check.pack"), "a chosen mod that is not installed is reported missing");
    t.check(ghost.packs().iter().all(|p| !p.is_mod), "a chosen mod that is not installed is not mounted");

    match available.first() {
        Some(m) => {
            let with = mount(&game, vec![m.clone()], vec![]);
            let at = with.packs().iter().position(|p| p.name.eq_ignore_ascii_case(m));
            t.check(at.is_some_and(|i| with.packs()[i].is_mod), "a chosen mod pack is mounted as a mod");
            let first_release = with.packs().iter().position(|p| !p.is_mod);
            t.check(matches!((at, first_release), (Some(a), Some(b)) if a < b), "a chosen mod pack is read before the release packs");
            t.check(with.packs().len() == release.len() + 1, "choosing one mod adds exactly one pack");
        }
        None => println!("SKIPPED the mod pack checks: no mod pack is installed in the data folder"),
    }

    // ---- loose folder over the packs
    let tmp = std::env::temp_dir().join(format!("wce_sources_check_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let pic = none.files().keys().find(|p| p.starts_with("ui/skins/") && !p.starts_with("ui/skins/default/") && p.ends_with(".png")).cloned().or_else(|| none.files().keys().find(|p| p.starts_with("ui/skins/") && p.ends_with(".png")).cloned());
    match pic {
        Some(pic) => {
            let packed = none.read(&pic).expect("read");
            let mine = b"loose bytes that are not the pack's".to_vec();
            let file = tmp.join(&pic);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, &mine).unwrap();
            let over = mount(&game, vec![], vec![tmp.clone()]);
            t.check(over.read(&pic).ok().as_deref() == Some(&mine[..]), "a loose file replaces the pack's file of the same path");
            t.check(over.owner(&pic).and_then(|i| over.packs().get(i)).is_some_and(|p| p.loose), "the loose file is the one that is read first");
            t.check(over.read_from(over.owners(&pic).last().copied().unwrap(), &pic).ok().as_deref() == Some(&packed[..]), "the pack's file is still there underneath");
            let again = mount(&game, vec![], vec![]);
            t.check(again.read(&pic).ok().as_deref() == Some(&packed[..]), "without the loose folder the pack's file is read again");

            // ---- skin list
            let skin_of_pic = pic.strip_prefix("ui/skins/").and_then(|r| r.split_once('/')).map(|(s, _)| s.to_string()).unwrap();
            let list = skins::list(&over, Some(&tmp));
            let s = list.iter().find(|s| s.name == skin_of_pic);
            t.check(s.is_some_and(|s| s.loose && s.packed), "a skin that is both in the packs and loose is listed as both");
            let fresh = "wce_check_new_skin";
            std::fs::create_dir_all(tmp.join("ui/skins").join(fresh)).unwrap();
            let list = skins::list(&over, Some(&tmp));
            t.check(list.iter().any(|s| s.name == fresh && s.loose && !s.packed), "a new, empty skin folder is listed as loose only");
            t.check(list.iter().filter(|s| s.name == skin_of_pic).count() == 1, "a skin is listed once");
            t.check(list.first().is_some_and(|s| s.name == "default"), "default is first");
            t.check(skins::list(&none, None).iter().all(|s| !s.loose), "without a loose folder no skin is loose");
            t.check(skins::loose_names(&tmp).contains(&fresh.to_string()), "the loose skin folders are read from disk");
        }
        None => println!("SKIPPED the loose folder checks: the packs have no skin picture"),
    }

    let _ = std::fs::remove_dir_all(&tmp);
    println!("{}", if t.failed == 0 { "sources_check: all good" } else { "sources_check: FAILED" });
    if t.failed > 0 {
        std::process::exit(1);
    }
}
