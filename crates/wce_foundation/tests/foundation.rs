//! Tests of the foundation. `math_dump` compares the math with the C++ code bit for bit over random inputs.

use wce_foundation::clock::Clock;
use wce_foundation::log::{self, LogLevel};
use wce_foundation::math::*;
use wce_foundation::profile;
use wce_foundation::settings::Settings;
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
    std::fs::write(dir.path().join("wce.ini"), "# comment\r\ngame_dir=/games/attila\r\nempty=\r\nno equals sign\r\nkey=a=b").unwrap();
    let mut settings = Settings::load(dir.path());
    assert_eq!(settings.get("game_dir", None).as_deref(), Some("/games/attila"));
    assert_eq!(settings.get("key", None).as_deref(), Some("a=b"));
    assert_eq!(settings.get("empty", None), None);
    assert_eq!(settings.get("game_dir", Some("WCE_TEST_VARIABLE_THAT_IS_NOT_SET")).as_deref(), Some("/games/attila"));

    settings.set("cache_dir", "/tmp/cache");
    settings.save().unwrap();
    let saved = std::fs::read_to_string(dir.path().join("wce.ini")).unwrap();
    assert_eq!(saved, "cache_dir=/tmp/cache\nempty=\ngame_dir=/games/attila\nkey=a=b\n");
    assert_eq!(Settings::load(dir.path()), settings);
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
