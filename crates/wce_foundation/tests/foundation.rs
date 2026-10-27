//! Tests of the foundation. `math_dump` compares the math with the C++ code bit for bit over random inputs.

use wce_foundation::clock::Clock;
use wce_foundation::log::{self, LogLevel};
use wce_foundation::math::*;
use wce_foundation::profile;
use wce_foundation::settings::{Paths, Settings};
use wce_foundation::wce_prof;

#[test]
fn math() {
    let m = translation(Float3::new(1.0, 2.0, 3.0)) * rotation_y(0.5) * scaling(2.0);
    let p = m.transform_point(Float3::new(1.0, 0.0, 0.0));
    let back = inverse(&m).transform_point(p);
    assert!(length(back - Float3::new(1.0, 0.0, 0.0)) < 1e-6);
    assert_eq!(inverse(&Float4x4 { m: [[0.0; 4]; 4] }), Float4x4 { m: [[0.0; 4]; 4] });
    assert_eq!(normalize(Float3::default()), Float3::default());
    assert_eq!(cross(Float3::new(1.0, 0.0, 0.0), Float3::new(0.0, 1.0, 0.0)), Float3::new(0.0, 0.0, 1.0));

    let view = look_at_lh(Float3::new(0.0, 0.0, -10.0), Float3::default(), Float3::new(0.0, 1.0, 0.0));
    assert_eq!(view.transform_point(Float3::default()), Float3::new(0.0, 0.0, 10.0));
    let proj = perspective_lh(1.0, 1.0, 1.0, 100.0);
    assert!((proj.transform_point(Float3::new(0.0, 0.0, 100.0)).z - 1.0).abs() < 1e-6);
    assert_eq!(Float4x4::IDENTITY.data()[5], 1.0);
}

#[test]
fn fixed_clock() {
    let mut clock = Clock::fixed(0.25);
    clock.tick();
    assert_eq!((clock.time(), clock.dt(), clock.frames()), (0.0, 0.0, 1));
    clock.tick();
    clock.tick();
    assert_eq!((clock.time(), clock.dt(), clock.frames()), (0.5, 0.25, 3));
    clock.restart();
    assert_eq!(clock.frames(), 0);
    assert!(Clock::fixed(-1.0).fixed_step() && !Clock::live().fixed_step());
}

#[test]
fn log_ring() {
    let before = log::sequence();
    log::warn("first");
    log::error(String::from("second"));
    assert!(log::sequence() >= before + 2);
    let entries = log::entries();
    assert!(entries.iter().any(|entry| entry.level == LogLevel::Error && entry.text == "second"));
    assert!(log::count(LogLevel::Warning) >= 1);
}

#[test]
fn settings_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    let mut settings = Settings::load(paths.clone());
    assert_eq!(settings.frame_cap, 60);
    assert_eq!(settings.game_dir(), None);

    settings.game_mut().dir = Some("/games/attila".into());
    settings.game_mut().mod_packs = vec!["a.pack".to_owned(), "b.pack".to_owned()];
    settings.frame_cap = 144;
    settings.set_tool("tile", &ToolSettings { kits: vec!["/kit".to_owned()], show_sea: false });
    settings.save().unwrap();
    assert!(!dir.path().join("settings.toml.tmp").exists());

    let loaded = Settings::load(paths.clone());
    assert_eq!(loaded, settings);
    assert_eq!(loaded.game_dir().as_deref(), Some(std::path::Path::new("/games/attila")));
    assert_eq!(loaded.mod_packs(), ["a.pack", "b.pack"]);
    assert_eq!(loaded.tool::<ToolSettings>("tile"), ToolSettings { kits: vec!["/kit".to_owned()], show_sea: false });
    assert_eq!(loaded.tool::<ToolSettings>("none"), ToolSettings::default());

    // A file that does not read is kept aside and gives the defaults.
    std::fs::write(paths.settings_file(), "frame_cap = \"many\"").unwrap();
    assert_eq!(Settings::load(paths.clone()).frame_cap, 60);
    assert!(dir.path().join("settings.toml.bad").exists());
}

#[test]
fn assembly_kits() {
    let dir = tempfile::tempdir().unwrap();
    let make = |name: &str| {
        let kit = dir.path().join(name);
        std::fs::create_dir_all(kit.join("raw_data")).unwrap();
        kit
    };
    let (a, b, battle) = (make("a"), make("b"), make("assembly_kit_battle_tilemap"));
    let campaign = make("assembly_kit_campaign_tilemap");

    // The old single kit becomes the first of the list, once.
    let paths = Paths::in_dir(&dir.path().join("config"));
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    std::fs::write(paths.settings_file(), format!("[games.attila]\nassembly_kit = {:?}\nassembly_kits = [{:?}]\n", a.display().to_string(), b.display().to_string())).unwrap();
    let mut settings = Settings::load(paths.clone());
    assert_eq!(settings.assembly_kit_dirs(), [a.clone(), b.clone()]);
    settings.save().unwrap();
    let text = std::fs::read_to_string(paths.settings_file()).unwrap();
    assert!(text.contains("assembly_kits") && !text.contains("assembly_kit ="), "{text}");
    assert_eq!(Settings::load(paths.clone()).assembly_kit_dirs(), [a.clone(), b.clone()]);

    // A kit with no raw_data is skipped; the battle tile map kit brings the campaign one; no kit is listed twice.
    let none = dir.path().join("none");
    settings.game_mut().assembly_kits = vec![none, battle.clone(), b.clone(), b.clone()];
    assert_eq!(settings.assembly_kit_raw_dirs(), [battle.join("raw_data"), campaign.join("raw_data"), b.join("raw_data")]);

    // Nothing listed: the kit in the game folder.
    let game = dir.path().join("game");
    std::fs::create_dir_all(game.join("assembly_kit/raw_data")).unwrap();
    settings.game_mut().assembly_kits.clear();
    assert!(settings.assembly_kit_dirs().is_empty());
    settings.game_mut().dir = Some(game.clone());
    assert_eq!(settings.assembly_kit_dirs(), [game.join("assembly_kit")]);
    settings.game_mut().assembly_kits = vec![a.clone()];
    assert_eq!(settings.assembly_kit_dirs(), [a]);
}

// TODO(pre-release): remove with the settings' `legacy` module.
#[test]
fn settings_from_the_old_ini() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("wce.ini"), "# comment\r\ngame_dir=/games/attila\r\nmod_packs=a.pack;b.pack\nframe_cap=30\ntile_show_sea=0\ntile_kits=/k1;/k2\nempty=\n").unwrap();
    std::fs::create_dir_all(dir.path().join("layouts")).unwrap();
    let s = Settings::load(Paths::in_dir(dir.path()));
    assert_eq!(s.game_dir().as_deref(), Some(std::path::Path::new("/games/attila")));
    assert_eq!(s.mod_packs(), ["a.pack", "b.pack"]);
    assert_eq!(s.frame_cap, 30);
    assert_eq!(s.tool::<ToolSettings>("tile"), ToolSettings { kits: vec!["/k1".to_owned(), "/k2".to_owned()], show_sea: false });
    assert!(dir.path().join("settings.toml").is_file());
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ToolSettings {
    kits: Vec<String>,
    show_sea: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self { kits: vec![], show_sea: true }
    }
}

#[test]
fn profile_slots() {
    for _ in 0..3 {
        wce_prof!("test.scope");
        std::hint::black_box(0);
    }

    let slot = profile::slot("test.scope");
    assert_eq!(slot.calls.load(std::sync::atomic::Ordering::Relaxed), 3);
    assert!(profile::report(false, 0.0).contains("test.scope"));
}

#[test]
fn job_pool_order() {
    use std::sync::{Arc, Mutex, mpsc};
    use wce_foundation::jobs::JobPool;

    let pool = JobPool::new(1);
    let order = Arc::new(Mutex::new(vec![]));
    let (release, gate) = mpsc::channel::<()>();

    // The first job holds the only thread while the others queue up.
    pool.post(move || { let _ = gate.recv(); }, 0.0);
    while pool.running() == 0 {
        std::thread::yield_now();
    }

    let job = |name: &'static str| {
        let order = order.clone();
        move || order.lock().unwrap().push((name, JobPool::current_priority()))
    };

    let late = pool.post(job("late"), 5.0);
    pool.post(job("first"), 1.0);
    let cancelled = pool.post(job("cancelled"), 0.5);
    pool.post(job("second"), 1.0);
    pool.post(|| panic!("a failing job"), 2.0);
    pool.set_priority(late, 0.1);
    assert!(pool.cancel(cancelled) && !pool.cancel(cancelled));
    assert_eq!(pool.queued(), 4);

    release.send(()).unwrap();
    pool.wait_idle();
    assert_eq!(*order.lock().unwrap(), [("late", 0.1), ("first", 1.0), ("second", 1.0)]);
    assert_eq!(JobPool::current_priority(), 0.0);
}
