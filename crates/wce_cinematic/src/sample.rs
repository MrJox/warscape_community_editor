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
}
