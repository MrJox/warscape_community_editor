//! A sample scene to look at and learn from: two armies of vanilla Attila units facing each other (the Western Roman Empire's line
//! with archers, cavalry and a general; the Huns' horse archers, lancers and a warlord), one clip with a camera that sweeps from the
//! Roman line towards the Huns, the Hun lancers riding in, a cohort cheering, fires burning, the bars and a fade in, a subtitle.

use std::collections::BTreeMap;

use crate::scene::*;

/// The units: (army, main unit key, name, x, z, general).
const UNITS: &[(usize, &str, &str, f32, f32, bool)] = &[
    (0, "att_rom_cohors", "Cohors (left)", -70.0, -300.0, false),
    (0, "att_rom_cohors", "Cohors (centre)", 0.0, -300.0, false),
    (0, "att_rom_cohors", "Cohors (right)", 70.0, -300.0, false),
    (0, "att_rom_sagittarii", "Sagittarii", 0.0, -340.0, false),
    (0, "att_rom_equites_promoti", "Equites promoti", 160.0, -310.0, false),
    (0, "att_rom_comes", "Comes", 0.0, -380.0, true),
    (1, "att_nom_hunnic_horse_archers", "Horse archers (left)", -90.0, 300.0, false),
    (1, "att_nom_hunnic_lancers", "Hunnic lancers", 0.0, 320.0, false),
    (1, "att_nom_hunnic_horse_archers", "Horse archers (right)", 90.0, 300.0, false),
    (1, "att_nom_steppe_warlord", "Steppe warlord", 0.0, 370.0, true),
];

/// The sample, for `game` (its key in the project).
pub fn sample_project(game: &str) -> Project {
    let mut p = Project::new("WCE Sample Battle", game);
    p.battle.key = "wce_sample_battle".into();
    // Fought on vanilla's Catalaunian Plains map, whole, as its historical battle is.
    p.map.campaign_map = "catalaunian_plains".into();
    p.map.whole_map = true;
    p.texts.insert("battle_title".into(), BTreeMap::from([("en".into(), "The Sample Battle".to_owned())]));
    p.texts.insert(
        "battle_description".into(),
        BTreeMap::from([("en".into(), "The Roman line waits as the Huns ride in from the east.".to_owned())]),
    );
    let mut ids = vec![];
    for &(army, key, name, x, z, general) in UNITS {
        let id = p.new_id();
        let facing = p.armies[army].deployment_orientation;
        p.units.push(SceneUnit {
            id,
            name: name.into(),
            unit: key.into(),
            army,
            start: [x, z],
            facing,
            width: if general { 16.0 } else { 34.0 },
            men: if general { 32 } else { 120 },
            seed: id,
            general,
        });
        ids.push(id);
    }
    let lancers = ids[7];
    let centre = ids[1];
    let archers_left = ids[6];

    let clip = p.clips[0].id;
    let mut tracks = vec![];
    let mut track = |p: &mut Project, name: &str, content: TrackContent| {
        let id = p.new_id();
        tracks.push(Track { id, name: name.into(), muted: false, content });
    };
    // The camera: behind the Roman line, over it, then towards the Huns. Heights are the map's, from the sea (its ground is at
    // about 460 to 480 m here), as the battle's `cam:move_to` takes them.
    track(
        &mut p,
        "Camera 1",
        TrackContent::Camera {
            keys: vec![
                CameraKey { t: 0.0, position: [-60.0, 474.0, -420.0], target: [0.0, 470.0, -300.0], fov: 35.0, interp: Interp::Smooth },
                CameraKey { t: 5.0, position: [40.0, 490.0, -330.0], target: [0.0, 470.0, -280.0], fov: 40.0, interp: Interp::Smooth },
                CameraKey { t: 12.0, position: [45.0, 505.0, 130.0], target: [0.0, 479.0, 230.0], fov: 38.0, interp: Interp::Smooth },
            ],
        },
    );
    // Camera 2: close on the centre cohort as it celebrates, turning slowly round its front.
    track(
        &mut p,
        "Camera 2",
        TrackContent::Camera {
            keys: vec![
                CameraKey { t: 5.5, position: [-14.0, 475.0, -281.0], target: [0.0, 471.5, -300.0], fov: 30.0, interp: Interp::Smooth },
                CameraKey { t: 8.5, position: [12.0, 474.5, -282.0], target: [0.0, 471.5, -300.0], fov: 28.0, interp: Interp::Smooth },
            ],
        },
    );
    // The Hun lancers ride in; the left horse archers walk forward; the centre cohort cheers.
    track(
        &mut p,
        "Hunnic lancers",
        TrackContent::Unit {
            unit: lancers,
            path: vec![
                PathKey { t: 0.0, position: [0.0, 320.0], interp: Interp::Linear, gait: None },
                PathKey { t: 3.0, position: [0.0, 300.0], interp: Interp::Linear, gait: None },
                PathKey { t: 12.0, position: [0.0, 200.0], interp: Interp::Linear, gait: None },
            ],
            actions: vec![],
            men: vec![],
            visible: vec![],
        },
    );
    track(
        &mut p,
        "Horse archers (left)",
        TrackContent::Unit {
            unit: archers_left,
            path: vec![
                PathKey { t: 2.0, position: [-90.0, 300.0], interp: Interp::Linear, gait: None },
                PathKey { t: 12.0, position: [-110.0, 270.0], interp: Interp::Linear, gait: None },
            ],
            actions: vec![],
            men: vec![],
            visible: vec![],
        },
    );
    track(
        &mut p,
        "Cohors (centre)",
        TrackContent::Unit {
            unit: centre,
            path: vec![],
            actions: vec![ActionKey { t: 6.0, slot: "CELEBRATE".into() }, ActionKey { t: 9.0, slot: "AUTO".into() }],
            men: vec![],
            visible: vec![],
        },
    );
    track(
        &mut p,
        "Fires",
        TrackContent::Vfx {
            instances: vec![
                VfxInstance {
                    start: 0.0,
                    end: 12.0,
                    vfx: "cine_vfx_fire_smoke_loop".into(),
                    position: [-200.0, 0.0, -180.0],
                    rotation_y: 0.0,
                    scale: 1.0,
                },
                VfxInstance {
                    start: 0.0,
                    end: 12.0,
                    vfx: "enviro_vfx_fire_smoke_heavy_loop".into(),
                    position: [150.0, 0.0, 40.0],
                    rotation_y: 0.0,
                    scale: 1.5,
                },
            ],
        },
    );
    track(&mut p, "Subtitles", TrackContent::Subtitles { lines: vec![SubtitleLine { t: 2.0, end: 6.0, text: "sample_line_1".into() }] });
    p.texts.insert("sample_line_1".into(), BTreeMap::from([("en".into(), "They are coming. Hold the line!".to_owned())]));
    track(
        &mut p,
        "Fade",
        TrackContent::Fade {
            keys: vec![ScalarKey { t: 0.0, value: 1.0, interp: Interp::Linear }, ScalarKey { t: 1.5, value: 0.0, interp: Interp::Linear }],
        },
    );
    track(&mut p, "Letterbox", TrackContent::Letterbox { keys: vec![ScalarKey { t: 0.0, value: 1.0, interp: Interp::Step }] });
    // The cuts: the wide shot, close on the cohort from 5.5 s, back to the wide one at 8.5 s.
    let cams: Vec<u32> = tracks.iter().filter(|t| matches!(t.content, TrackContent::Camera { .. })).map(|t| t.id).collect();
    let cuts = vec![CutKey { t: 0.0, camera: cams[0] }, CutKey { t: 5.5, camera: cams[1] }, CutKey { t: 8.5, camera: cams[0] }];
    let id = p.new_id();
    tracks.push(Track { id, name: "Camera Cuts".into(), muted: false, content: TrackContent::Cuts { cuts } });
    if let Some(c) = p.clip_mut(clip) {
        // The new project's empty camera track gives way to the keyed one.
        c.tracks = tracks;
        c.length = 12.0;
    }
    p.normalize();
    p
}

/// The duel's fight: (seconds, the Roman's animation, the Hun's), each held until the next. One strikes, the other blocks; a shield
/// bash knocks the Hun back; families (`ATTACK` ...) would spread variants over men, so each key names one.
const DUEL: &[(f64, &str, &str)] = &[
    (0.0, "COMBAT_READY", "COMBAT_READY"),
    (1.0, "ATTACK_1", "DEFEND_2"),
    (2.3, "COMBAT_READY", "COMBAT_READY"),
    (2.8, "DEFEND_1", "ATTACK_2"),
    (4.1, "ATTACK_3", "DEFEND_1"),
    (5.4, "BASH_SHIELD_1", "KNOCKBACK_1"),
    (6.6, "COMBAT_READY", "COMBAT_READY"),
    (7.0, "DEFEND_3", "ATTACK_4"),
    (8.3, "ATTACK_2", "DEFEND_4"),
    (9.6, "ATTACK_5", "DEFEND_2"),
    (11.0, "COMBAT_READY", "COMBAT_READY"),
];

/// A second sample: two men fighting, a Roman soldier (a cohort's man) and a Hun warrior (a dismounted warband's), sword and shield
/// both, face to face on a flat plains tile; one camera fixed on them from the side; the bars and a fade in.
pub fn duel_project(game: &str) -> Project {
    let mut p = Project::new("WCE Sample Duel", game);
    p.battle.key = "wce_sample_duel".into();
    // A flat battle tile (its ground about 0.4 m): the camera's heights below are over it.
    p.map.tile = "terrain/tiles/battle/plains/8x8/8x8_aa".into();
    p.map.whole_map = false;
    p.texts.insert("battle_title".into(), BTreeMap::from([("en".into(), "The Sample Duel".to_owned())]));
    p.texts.insert("battle_description".into(), BTreeMap::from([("en".into(), "A Roman soldier and a Hun warrior fight.".to_owned())]));
    for a in &mut p.armies {
        a.deployment_size = [120.0, 60.0];
    }
    p.armies[0].deployment_centre = [0.0, -40.0];
    p.armies[1].deployment_centre = [0.0, 40.0];
    let man = |p: &mut Project, army: usize, key: &str, name: &str, z: f32, facing: f32| {
        let id = p.new_id();
        p.units.push(SceneUnit {
            id,
            name: name.into(),
            unit: key.into(),
            army,
            start: [0.0, z],
            facing,
            width: 1.0,
            men: 1,
            seed: id,
            general: false,
        });
        id
    };
    let roman = man(&mut p, 0, "att_rom_cohors", "Roman soldier", -0.85, 0.0);
    let hun = man(&mut p, 1, "att_nom_hunnic_dismounted_warband", "Hun warrior", 0.85, std::f32::consts::PI);
    let clip = p.clips[0].id;
    let mut tracks = vec![];
    let mut track = |p: &mut Project, name: &str, content: TrackContent| {
        let id = p.new_id();
        tracks.push(Track { id, name: name.into(), muted: false, content });
    };
    // The camera: fixed, from the side at chest height, both men in the frame.
    track(
        &mut p,
        "Camera 1",
        TrackContent::Camera {
            keys: vec![CameraKey { t: 0.0, position: [5.0, 2.5, 0.0], target: [0.0, 1.3, 0.0], fov: 32.0, interp: Interp::Smooth }],
        },
    );
    let keys = |hun_side: bool| -> Vec<ActionKey> {
        DUEL.iter().map(|&(t, r, h)| ActionKey { t, slot: (if hun_side { h } else { r }).into() }).collect()
    };
    for (unit, name, hun_side) in [(roman, "Roman soldier", false), (hun, "Hun warrior", true)] {
        track(&mut p, name, TrackContent::Unit { unit, path: vec![], actions: keys(hun_side), men: vec![], visible: vec![] });
    }
    track(
        &mut p,
        "Fade",
        TrackContent::Fade {
            keys: vec![ScalarKey { t: 0.0, value: 1.0, interp: Interp::Linear }, ScalarKey { t: 1.0, value: 0.0, interp: Interp::Linear }],
        },
    );
    track(&mut p, "Letterbox", TrackContent::Letterbox { keys: vec![ScalarKey { t: 0.0, value: 1.0, interp: Interp::Step }] });
    if let Some(c) = p.clip_mut(clip) {
        c.tracks = tracks;
        c.length = 12.0;
    }
    p.normalize();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_reads_back_and_plays() {
        let p = sample_project("attila");
        let back = Project::from_json(&p.to_json()).unwrap();
        assert_eq!(back, p);
        let clip = &p.clips[0];
        let f = crate::eval::evaluate(&p, clip, &crate::eval::SpeedTable::default(), 6.0);
        assert_eq!(f.units.len(), UNITS.len());
        assert!(f.camera.is_some());
        assert_eq!(f.vfx.len(), 2);
        assert_eq!(f.subtitle.as_deref(), None, "the line ends at 6 s");
        let ids: std::collections::BTreeSet<u32> =
            p.units.iter().map(|u| u.id).chain(p.clips.iter().flat_map(|c| c.tracks.iter().map(|t| t.id))).collect();
        assert_eq!(ids.len(), p.units.len() + clip.tracks.len(), "ids unique");
    }

    #[test]
    fn the_duel_fights_under_a_fixed_camera() {
        let p = duel_project("attila");
        assert_eq!(Project::from_json(&p.to_json()).unwrap(), p);
        let clip = &p.clips[0];
        let at = |t: f64| crate::eval::evaluate(&p, clip, &crate::eval::SpeedTable::default(), t);
        let (a, b) = (at(1.5), at(10.0));
        assert_eq!(a.camera, b.camera, "the camera doesn't move");
        let slots = |f: &crate::eval::SceneFrame| f.units.iter().map(|u| u.slot.clone()).collect::<Vec<_>>();
        assert_eq!(slots(&a), ["ATTACK_1", "DEFEND_2"]);
        assert_eq!(slots(&at(5.5)), ["BASH_SHIELD_1", "KNOCKBACK_1"]);
        assert!(a.units.iter().all(|u| u.men == 1));
    }
}
