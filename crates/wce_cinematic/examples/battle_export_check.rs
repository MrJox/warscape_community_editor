//! Gate of the historical battle export (docs/plans/cinematic.md, steps 6 to 8): a sample project (two armies of vanilla units, an
//! intro clip with every kind of track, a logic graph) exported with the game's schema. Checks: the battle XML parses and places
//! every unit; the tables and the loc decode back with their rows; the Lua names every unit, clip and message it uses; movie-only
//! tracks are reported, not exported; the export is the same twice. Writes the files and a pack to `<out>` for the Lua checker.
//!
//!   battle_export_check <game folder> <out folder>

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::loc::Loc;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_cinematic::battle_export::{battle_scripts, battle_xml, unit_script_name};
use wce_cinematic::battle_tables::{battle_files, write_battle_pack};
use wce_cinematic::scene::*;
use wce_vfs::files::Files;
use wce_vfs::{Config, GameProfile, Vfs};

pub fn sample() -> Project {
    let mut p = Project::new("WCE Test Battle", "attila");
    // At a place on the campaign map (the other two kinds are checked below).
    p.map.campaign_map = "main_attila_map".into();
    p.map.whole_map = false;
    p.battle.environment = "Weather\\default\\default\\land\\day\\dry\\evening_north_3.environment".into();
    p.texts.insert("battle_description".into(), BTreeMap::from([("en".into(), "A test of the Cinematic tool.".into())]));
    p.texts.insert("intro_1".into(), BTreeMap::from([("en".into(), "June 20, 451 AD.".into())]));
    let unit = |p: &mut Project, name: &str, key: &str, army: usize, x: f32, z: f32, facing: f32| {
        let id = p.new_id();
        p.units.push(SceneUnit {
            id,
            name: name.into(),
            unit: key.into(),
            army,
            start: [x, z],
            facing,
            width: 30.0,
            men: 160,
            seed: id,
            general: false,
        });
        id
    };
    let general = unit(&mut p, "General", "att_rom_magister_militum", 0, -20.0, -360.0, 0.0);
    p.units.iter_mut().find(|u| u.id == general).unwrap().general = true;
    let cohors = unit(&mut p, "Cohors", "att_rom_cohors", 0, 20.0, -340.0, 0.0);
    unit(&mut p, "Huns", "att_hun_horse_archers", 1, 0.0, 360.0, std::f32::consts::PI);
    let clip = p.clips[0].id;
    let ids: Vec<u32> = (0..9).map(|_| p.new_id()).collect();
    let c = p.clip_mut(clip).unwrap();
    c.length = 12.0;
    if let TrackContent::Camera { keys } = &mut c.tracks[0].content {
        keys.push(CameraKey { t: 0.0, position: [-150.0, 60.0, -500.0], target: [0.0, 0.0, -340.0], fov: 35.0, interp: Interp::Smooth });
        keys.push(CameraKey { t: 6.0, position: [80.0, 40.0, -420.0], target: [20.0, 0.0, -330.0], fov: 40.0, interp: Interp::Smooth });
        keys.push(CameraKey { t: 12.0, position: [120.0, 80.0, -200.0], target: [0.0, 0.0, 0.0], fov: 45.0, interp: Interp::Linear });
    }
    let t = |id: u32, name: &str, content: TrackContent| Track { id, name: name.into(), muted: false, content };
    c.tracks.push(t(
        ids[0],
        "Cohors",
        TrackContent::Unit {
            unit: cohors,
            path: vec![
                PathKey { t: 0.0, position: [20.0, -340.0], interp: Interp::Linear, gait: None },
                PathKey { t: 8.0, position: [20.0, -300.0], interp: Interp::Linear, gait: None },
            ],
            actions: vec![ActionKey { t: 8.5, slot: "TAUNT".into() }, ActionKey { t: 10.0, slot: "CELEBRATE".into() }],
            men: vec![MenKey { t: 0.0, men: 120 }],
            visible: vec![VisibleKey { t: 0.0, visible: true }],
        },
    ));
    c.tracks.push(t(
        ids[1],
        "Narration",
        TrackContent::Sound { events: vec![SoundEvent { t: 1.0, event: "Play_Att_HB_CP_Intro_02_1".into(), position: None, volume: 1.0 }] },
    ));
    c.tracks.push(t(
        ids[2],
        "Music",
        TrackContent::Music {
            cues: vec![MusicCue { t: 0.0, event: "Play_Music_Battle_Hun".into(), stop: Some(11.0), fade_in: 1.0, fade_out: 1.0 }],
        },
    ));
    c.tracks.push(t(
        ids[3],
        "Subtitles",
        TrackContent::Subtitles { lines: vec![SubtitleLine { t: 1.0, end: 5.0, text: "intro_1".into() }] },
    ));
    c.tracks.push(t(
        ids[4],
        "Fade",
        TrackContent::Fade {
            keys: vec![
                ScalarKey { t: 0.0, value: 1.0, interp: Interp::Linear },
                ScalarKey { t: 1.0, value: 0.0, interp: Interp::Linear },
                ScalarKey { t: 11.0, value: 0.0, interp: Interp::Linear },
                ScalarKey { t: 12.0, value: 1.0, interp: Interp::Linear },
            ],
        },
    ));
    c.tracks.push(t(ids[5], "Bars", TrackContent::Letterbox { keys: vec![ScalarKey { t: 0.0, value: 1.0, interp: Interp::Step }] }));
    c.tracks.push(t(
        ids[6],
        "Focus",
        TrackContent::DepthOfField {
            keys: vec![
                DofKey { t: 0.0, focus_distance: 50.0, focal_length: 35.0, blur_size: 2.0, interp: Interp::Linear },
                DofKey { t: 6.0, focus_distance: 120.0, focal_length: 50.0, blur_size: 1.0, interp: Interp::Linear },
            ],
        },
    ));
    c.tracks.push(t(
        ids[7],
        "Smoke",
        TrackContent::Vfx {
            instances: vec![VfxInstance {
                start: 0.0,
                end: 12.0,
                vfx: "fire_smoke_large_loop".into(),
                position: [0.0, 0.0, 0.0],
                rotation_y: 0.0,
                scale: 1.0,
            }],
        },
    ));
    c.tracks.push(t(
        ids[8],
        "Dusk",
        TrackContent::Weather { keys: vec![WeatherKey { t: 0.0, environment: "a".into(), interp: Interp::Smooth }] },
    ));
    // Logic: when the intro ends the Huns attack; when the Romans come near, an advice line.
    let n = |id, kind: &str, values: &[(&str, &str)]| LogicNode {
        id,
        kind: kind.into(),
        values: values.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
        position: [0.0, 0.0],
    };
    p.logic.nodes = vec![
        n(1, "event.cutscene_ended", &[]),
        n(2, "action.attack", &[("army", "1")]),
        n(3, "event.proximity", &[("army", "0"), ("distance", "150")]),
        n(4, "action.advice", &[("advice", "Att.HB.CP.001"), ("delay_ms", "2000")]),
    ];
    p.logic.links = vec![
        LogicLink { from: 1, from_pin: "out".into(), to: 2, to_pin: "in".into() },
        LogicLink { from: 3, from_pin: "out".into(), to: 4, to_pin: "in".into() },
    ];
    p
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(game), Some(out)) = (args.next(), args.next()) else {
        eprintln!("battle_export_check <game folder> <out folder>");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    let config = Config::new(PathBuf::from(game), GameProfile::from_env().clone());
    let vfs = Vfs::mount(&config).expect("the game's packs");
    let files: &dyn Files = &vfs;
    let schema =
        files.game().and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok()).expect("the schema");
    let mut failed: Vec<String> = vec![];

    let project = sample();
    let (written, warnings) = battle_files(&project, files, &schema, "en").expect("export");
    let (again, _) = battle_files(&project, files, &schema, "en").expect("export");
    if written != again {
        failed.push("two exports differ".into());
    }
    for w in &warnings {
        println!("warning: {w}");
    }
    for expected in ["Smoke", "Dusk", "CELEBRATE"] {
        if !warnings.iter().any(|w| w.contains(expected)) {
            failed.push(format!("no warning about {expected}"));
        }
    }

    std::fs::create_dir_all(&out).expect("out folder");
    for (path, bytes) in &written {
        let p = out.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        println!("{path}: {} bytes", bytes.len());
    }

    // The battle XML parses and holds every unit by its script name.
    let xml = written.iter().find(|(p, _)| p.ends_with("_battle.xml")).map(|(_, b)| b.clone()).unwrap_or_default();
    if let Err(e) = wce_formats::xml_doc::XmlDoc::parse(xml.clone()) {
        failed.push(format!("battle XML: {e:?}"));
    }
    let text = String::from_utf8_lossy(&xml);
    for u in &project.units {
        if !text.contains(&format!("script_name=\"{}\"", unit_script_name(u.id))) {
            failed.push(format!("battle XML: no unit {}", u.id));
        }
    }
    if !text.contains("<tile_map_position x=\"0.500000\" y=\"0.500000\">") || !text.contains("terrain/battles/main_attila_map/") {
        failed.push("battle XML: no campaign map position".into());
    }
    // On one battle tile instead: its folder, no map position.
    {
        let mut on_tile = project.clone();
        on_tile.map.tile = "terrain/tiles/battle/plains/8x8/8x8_aa".into();
        let t = battle_xml(&on_tile);
        if !t.contains("<name>terrain/tiles/battle/plains/8x8/8x8_aa</name>") || t.contains("tile_map_position") {
            failed.push(format!("battle XML on a tile: {t}"));
        }
        // A whole battle map, as vanilla's historical battles.
        let mut whole = project.clone();
        whole.map.campaign_map = "catalaunian_plains".into();
        whole.map.whole_map = true;
        let t = battle_xml(&whole);
        if !t.contains("<name>terrain/battles/catalaunian_plains/</name>") || t.contains("tile_map_position") {
            failed.push(format!("battle XML on a whole map: {t}"));
        }
    }

    // The Lua: every unit declared, the clip's function, the logic's messages.
    let scripts = battle_scripts(&project);
    let all: String = scripts.files.values().cloned().collect();
    for needle in [
        "SU_",
        "Play_Clip_",
        "cs:action(",
        "cam:move_to(",
        "goto_location_angle_width(",
        "kill_number_of_men(40)",
        "start_taunting()",
        "subtitles:set_text(\"wce.wce_test_battle.intro_1\")",
        "bm:play_music_custom_fade(",
        "cam:change_depth_of_field(",
        "cam:fade(",
        "set_show_cinematic_bars(true)",
        "ga_2:attack_on_message(\"cutscene_ended\")",
        "ga_1:message_on_proximity_to_enemy(\"wce_n3\", 150)",
        "gb:advice_on_message(\"wce_n3\", \"Att.HB.CP.001\", 2000)",
    ] {
        if !all.contains(needle) {
            failed.push(format!("Lua: no {needle}"));
        }
    }
    if all.contains("wait_for_vo") {
        failed.push("Lua: wait_for_vo".into());
    }
    // Every action on the tick grid.
    for line in all.lines().filter(|l| l.contains("cs:action(")) {
        let ms: u64 = line.rsplit(',').next().and_then(|s| s.trim().trim_end_matches(");").trim().parse().ok()).unwrap_or(1);
        if !ms.is_multiple_of(100) {
            failed.push(format!("Lua: action off the tick grid: {line}"));
        }
    }

    // Tables and loc decode back.
    for (path, bytes) in written.iter().filter(|(p, _)| p.starts_with("db/")) {
        let table = path.split('/').nth(1).unwrap();
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(&schema));
        extra.set_table_name(Some(table));
        match DB::decode(&mut Cursor::new(bytes), &Some(extra)) {
            Ok(db) if !db.data().is_empty() => println!("{table}: {} rows", db.data().len()),
            Ok(_) => failed.push(format!("{table}: no rows")),
            Err(e) => failed.push(format!("{table}: {e}")),
        }
    }
    let loc = written.iter().find(|(p, _)| p.ends_with(".loc")).map(|(_, b)| b.clone()).unwrap_or_default();
    match Loc::decode(&mut Cursor::new(&loc), &Some(DecodeableExtraData::default())) {
        Ok(l) => {
            let keys: Vec<String> = l.data().iter().map(|r| r[0].data_to_string().into_owned()).collect();
            println!("loc: {}", keys.join(", "));
            for k in ["battles_localised_name_wce_test_battle", "scripted_subtitles_localised_text_wce.wce_test_battle.intro_1"] {
                if !keys.iter().any(|x| x == k) {
                    failed.push(format!("loc: no {k}"));
                }
            }
        }
        Err(e) => failed.push(format!("loc: {e}")),
    }

    match write_battle_pack(&out.join("wce_test_battle.pack"), &project, files, &schema, "en") {
        Ok(_) => println!("pack written"),
        Err(e) => failed.push(format!("pack: {e}")),
    }
    // The project's file round-trips.
    if Project::from_json(&project.to_json()).ok().as_ref() != Some(&project) {
        failed.push("project.json does not round-trip".into());
    }
    std::fs::write(out.join("project.json"), project.to_json()).unwrap();

    if failed.is_empty() {
        println!("OK");
    } else {
        for f in &failed {
            println!("FAIL {f}");
        }
        std::process::exit(1);
    }
}
