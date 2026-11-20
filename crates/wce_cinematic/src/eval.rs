//! A clip's scene at a time (docs/plans/cinematic.md, step 3): a pure function of the project, so a timeline can scrub it and a
//! movie render it frame by frame. The renderer, the audio player and the exporters all read a `SceneFrame`.

use std::collections::HashMap;

use crate::curves::{around, point, scalar, smoothstep};
use crate::scene::{AUTO_SLOT, Clip, Gait, Interp, PathKey, Project, TrackContent};

/// A unit type's speeds, metres per second (`battle_entities` walk, run and charge speed of its man entity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speeds {
    pub walk: f32,
    pub run: f32,
    pub charge: f32,
}

impl Default for Speeds {
    /// An infantryman's, when the unit's are not known.
    fn default() -> Self {
        Self { walk: 1.6, run: 4.0, charge: 5.5 }
    }
}

/// Speeds by `main_units` key.
pub type SpeedTable = HashMap<String, Speeds>;

#[derive(Clone, Debug, PartialEq)]
pub struct CameraState {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitState {
    pub unit: u32,
    pub position: [f32; 2],
    /// Radians, 0 = +z.
    pub facing: f32,
    /// Metres per second along the path.
    pub speed: f32,
    /// The animation slot playing, since `slot_start`; `slot_distance` metres walked in it (a moving clip's phase follows the
    /// ground covered, so feet don't slide).
    pub slot: String,
    pub slot_start: f64,
    pub slot_distance: f32,
    pub men: u32,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveSound {
    pub event: String,
    pub started: f64,
    pub position: Option<[f32; 3]>,
    pub volume: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveMusic {
    pub event: String,
    pub started: f64,
    /// The fades' gain at the time, 0 to 1.
    pub gain: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DofState {
    pub focus_distance: f32,
    pub focal_length: f32,
    pub blur_size: f32,
}

/// The weather: environment `from`, blending towards `to` by `blend` (0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherState {
    pub from: String,
    pub to: String,
    pub blend: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VfxState {
    pub vfx: String,
    /// Seconds since it started.
    pub age: f64,
    pub position: [f32; 3],
    pub rotation_y: f32,
    pub scale: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneFrame {
    pub t: f64,
    pub camera: Option<CameraState>,
    pub units: Vec<UnitState>,
    /// Sound events started so far (the player decides whether they are still heard).
    pub sounds: Vec<ActiveSound>,
    pub music: Vec<ActiveMusic>,
    /// The subtitle's text id.
    pub subtitle: Option<String>,
    pub fade: f32,
    pub letterbox: f32,
    pub dof: Option<DofState>,
    pub weather: Option<WeatherState>,
    pub vfx: Vec<VfxState>,
}

/// The clip's scene at `t` seconds (clamped to the clip).
pub fn evaluate(project: &Project, clip: &Clip, speeds: &SpeedTable, t: f64) -> SceneFrame {
    let t = t.clamp(0.0, clip.length.max(0.0));
    let mut f = SceneFrame { t, ..SceneFrame::default() };
    let mut animated = vec![];
    for track in clip.tracks.iter().filter(|tr| !tr.muted) {
        match &track.content {
            TrackContent::Camera { keys } => {
                let times: Vec<f64> = keys.iter().map(|k| k.t).collect();
                if let Some((i, j, u)) = around(&times, t) {
                    let positions: Vec<[f32; 3]> = keys.iter().map(|k| k.position).collect();
                    let targets: Vec<[f32; 3]> = keys.iter().map(|k| k.target).collect();
                    let interp = keys[i].interp;
                    f.camera = Some(CameraState {
                        position: point(&positions, &times, i, j, u, interp),
                        target: point(&targets, &times, i, j, u, interp),
                        fov: scalar(keys[i].fov, keys[j].fov, u, interp),
                    });
                }
            }
            TrackContent::Unit { unit, path, actions, men, visible } => {
                let Some(su) = project.unit(*unit) else { continue };
                let sp = speeds.get(&su.unit).copied().unwrap_or_default();
                let mut state = unit_on_path(su.id, su.start, su.facing, path, sp, t);
                if let Some(k) = actions.iter().rev().find(|k| k.t <= t)
                    && k.slot != AUTO_SLOT
                {
                    state.slot = k.slot.clone();
                    state.slot_start = k.t;
                    state.slot_distance = 0.0;
                }
                state.men = men.iter().rev().find(|k| k.t <= t).map_or(su.men, |k| k.men);
                state.visible = visible.iter().rev().find(|k| k.t <= t).is_none_or(|k| k.visible);
                animated.push(su.id);
                f.units.push(state);
            }
            TrackContent::Sound { events } => {
                for e in events.iter().filter(|e| e.t <= t) {
                    f.sounds.push(ActiveSound { event: e.event.clone(), started: e.t, position: e.position, volume: e.volume });
                }
            }
            TrackContent::Music { cues } => {
                for c in cues.iter().filter(|c| c.t <= t) {
                    let stop = c.stop.unwrap_or(clip.length);
                    if t >= stop {
                        continue;
                    }
                    let fade_in = if c.fade_in > 0.0 { ((t - c.t) / c.fade_in as f64).clamp(0.0, 1.0) } else { 1.0 };
                    let fade_out = if c.fade_out > 0.0 { ((stop - t) / c.fade_out as f64).clamp(0.0, 1.0) } else { 1.0 };
                    f.music.push(ActiveMusic { event: c.event.clone(), started: c.t, gain: (fade_in * fade_out) as f32 });
                }
            }
            TrackContent::Subtitles { lines } => {
                if let Some(l) = lines.iter().rev().find(|l| l.t <= t && t < l.end) {
                    f.subtitle = Some(l.text.clone());
                }
            }
            TrackContent::Vfx { instances } => {
                for v in instances.iter().filter(|v| v.start <= t && t < v.end) {
                    f.vfx.push(VfxState {
                        vfx: v.vfx.clone(),
                        age: t - v.start,
                        position: v.position,
                        rotation_y: v.rotation_y,
                        scale: v.scale,
                    });
                }
            }
            TrackContent::Fade { keys } => f.fade = scalar_track(keys.iter().map(|k| (k.t, k.value, k.interp)), t).unwrap_or(0.0),
            TrackContent::Letterbox { keys } => f.letterbox = scalar_track(keys.iter().map(|k| (k.t, k.value, k.interp)), t).unwrap_or(0.0),
            TrackContent::DepthOfField { keys } => {
                let times: Vec<f64> = keys.iter().map(|k| k.t).collect();
                if let Some((i, j, u)) = around(&times, t) {
                    let (a, b, m) = (&keys[i], &keys[j], keys[i].interp);
                    f.dof = Some(DofState {
                        focus_distance: scalar(a.focus_distance, b.focus_distance, u, m),
                        focal_length: scalar(a.focal_length, b.focal_length, u, m),
                        blur_size: scalar(a.blur_size, b.blur_size, u, m),
                    });
                }
            }
            TrackContent::Weather { keys } => {
                let times: Vec<f64> = keys.iter().map(|k| k.t).collect();
                if let Some((i, j, u)) = around(&times, t) {
                    let w = match keys[i].interp {
                        Interp::Step => 0.0,
                        Interp::Linear => u,
                        Interp::Smooth => smoothstep(u),
                    };
                    f.weather = Some(WeatherState { from: keys[i].environment.clone(), to: keys[j].environment.clone(), blend: w as f32 });
                }
            }
        }
    }
    // Units without a track stand where the battle starts them.
    for su in project.units.iter().filter(|u| !animated.contains(&u.id)) {
        f.units.push(UnitState {
            unit: su.id,
            position: su.start,
            facing: su.facing,
            speed: 0.0,
            slot: "STAND".into(),
            slot_start: 0.0,
            slot_distance: 0.0,
            men: su.men,
            visible: true,
        });
    }
    f.units.sort_by_key(|u| u.unit);
    f
}

fn scalar_track(keys: impl Iterator<Item = (f64, f32, Interp)>, t: f64) -> Option<f32> {
    let keys: Vec<(f64, f32, Interp)> = keys.collect();
    let times: Vec<f64> = keys.iter().map(|k| k.0).collect();
    let (i, j, u) = around(&times, t)?;
    Some(scalar(keys[i].1, keys[j].1, u, keys[i].2))
}

/// Samples per path segment for its length and direction.
const SAMPLES: usize = 16;
/// Slower than this, a unit stands.
const STANDING: f32 = 0.05;

struct Segment {
    t0: f64,
    t1: f64,
    length: f32,
    gait: Option<&'static str>,
}

fn gait_slot(g: Gait) -> &'static str {
    match g {
        Gait::Walk => "WALK",
        Gait::Run => "RUN",
        Gait::Charge => "CHARGE",
    }
}

/// The slot a speed asks for: walking up to 15 % over the walk speed, running up to 10 % over the run speed, then charging.
pub fn slot_for_speed(speed: f32, sp: Speeds) -> &'static str {
    if speed < STANDING {
        "STAND"
    } else if speed <= sp.walk * 1.15 {
        "WALK"
    } else if speed <= sp.run * 1.1 {
        "RUN"
    } else {
        "CHARGE"
    }
}

fn path_point(path: &[PathKey], times: &[f64], i: usize, j: usize, u: f64) -> [f32; 2] {
    let points: Vec<[f32; 2]> = path.iter().map(|k| k.position).collect();
    point(&points, times, i, j, u, path[i].interp)
}

/// Where a unit is on its path at `t`, its facing, speed and the gait's slot (with when it started and the ground covered since).
fn unit_on_path(id: u32, start: [f32; 2], start_facing: f32, path: &[PathKey], sp: Speeds, t: f64) -> UnitState {
    let mut state = UnitState {
        unit: id,
        position: start,
        facing: start_facing,
        speed: 0.0,
        slot: "STAND".into(),
        slot_start: 0.0,
        slot_distance: 0.0,
        men: 0,
        visible: true,
    };
    if path.is_empty() {
        return state;
    }
    let times: Vec<f64> = path.iter().map(|k| k.t).collect();
    let (i, j, u) = around(&times, t).expect("not empty");
    state.position = path_point(path, &times, i, j, u);

    // Every segment's length (sampled) and gait, to find the slot's start and the ground covered in it.
    let mut segments = Vec::with_capacity(path.len().saturating_sub(1));
    for a in 0..path.len().saturating_sub(1) {
        let mut length = 0.0f32;
        let mut prev = path_point(path, &times, a, a + 1, 0.0);
        for s in 1..=SAMPLES {
            let p = path_point(path, &times, a, a + 1, s as f64 / SAMPLES as f64);
            length += ((p[0] - prev[0]).powi(2) + (p[1] - prev[1]).powi(2)).sqrt();
            prev = p;
        }
        let gait = path[a].gait.map(gait_slot);
        segments.push(Segment { t0: times[a], t1: times[a + 1], length, gait });
    }
    let slot_of = |s: &Segment| -> &'static str {
        let speed = if s.t1 > s.t0 { s.length / (s.t1 - s.t0) as f32 } else { 0.0 };
        if speed < STANDING { "STAND" } else { s.gait.unwrap_or_else(|| slot_for_speed(speed, sp)) }
    };

    // Facing: along the motion now, else where the last motion before now ended, else the start's.
    let dir_at = |a: usize, uu: f64| -> Option<f32> {
        let p0 = path_point(path, &times, a, a + 1, (uu - 0.01).max(0.0));
        let p1 = path_point(path, &times, a, a + 1, (uu + 0.01).min(1.0));
        let (dx, dz) = (p1[0] - p0[0], p1[1] - p0[1]);
        if dx * dx + dz * dz > 1e-8 { Some(dx.atan2(dz)) } else { None }
    };
    let moving_now = i != j && segments.get(i).is_some_and(|s| slot_of(s) != "STAND");
    if moving_now {
        let s = &segments[i];
        state.speed = s.length / (s.t1 - s.t0).max(1e-9) as f32;
        if let Some(f) = dir_at(i, u) {
            state.facing = f;
        }
    } else {
        let last = if i == j { i.min(segments.len()) } else { i };
        if let Some(a) = (0..last).rev().find(|&a| slot_of(&segments[a]) != "STAND")
            && let Some(f) = dir_at(a, 1.0)
        {
            state.facing = f;
        }
    }

    // The slot and its run: back over the segments with the same slot.
    if moving_now {
        let slot = slot_of(&segments[i]);
        let mut first = i;
        while first > 0 && slot_of(&segments[first - 1]) == slot && segments[first - 1].t1 == segments[first].t0 {
            first -= 1;
        }
        let mut covered: f32 = segments[first..i].iter().map(|s| s.length).sum();
        // Within the current segment: the ground up to u (sampled).
        let mut prev = path_point(path, &times, i, j, 0.0);
        let steps = ((u * SAMPLES as f64).ceil() as usize).max(1);
        for s in 1..=steps {
            let p = path_point(path, &times, i, j, u * s as f64 / steps as f64);
            covered += ((p[0] - prev[0]).powi(2) + (p[1] - prev[1]).powi(2)).sqrt();
            prev = p;
        }
        state.slot = slot.into();
        state.slot_start = segments[first].t0;
        state.slot_distance = covered;
    } else {
        // Standing since the last moving segment ended (or the start).
        let last = if i == j { i.min(segments.len()) } else { i };
        state.slot_start = (0..last).rev().find(|&a| slot_of(&segments[a]) != "STAND").map_or(0.0, |a| segments[a].t1);
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CameraKey, SceneUnit, Track};

    fn project_with_unit() -> (Project, u32) {
        let mut p = Project::new("Test", "attila");
        let id = p.new_id();
        p.units.push(SceneUnit {
            id,
            name: "Cohors".into(),
            unit: "att_rom_cohors".into(),
            army: 0,
            start: [0.0, 0.0],
            facing: 0.0,
            width: 20.0,
            men: 120,
            seed: 7,
            general: false,
        });
        (p, id)
    }

    #[test]
    fn walk_then_stand() {
        let (mut p, unit) = project_with_unit();
        let path = vec![
            PathKey { t: 1.0, position: [0.0, 0.0], interp: Interp::Linear, gait: None },
            PathKey { t: 6.0, position: [0.0, 7.5], interp: Interp::Linear, gait: None },
        ];
        let track = p.new_id();
        let clip_id = p.clips[0].id;
        p.clip_mut(clip_id).unwrap().tracks.push(Track {
            id: track,
            name: "Cohors".into(),
            muted: false,
            content: TrackContent::Unit { unit, path, actions: vec![], men: vec![], visible: vec![] },
        });
        let clip = p.clip(clip_id).unwrap().clone();
        let speeds = SpeedTable::new();
        let f = evaluate(&p, &clip, &speeds, 3.5);
        let u = &f.units[0];
        assert_eq!(u.slot, "WALK");
        assert!((u.position[1] - 3.75).abs() < 1e-4);
        assert!((u.speed - 1.5).abs() < 1e-3);
        assert!((u.slot_distance - 3.75).abs() < 1e-2);
        assert_eq!(u.slot_start, 1.0);
        let f = evaluate(&p, &clip, &speeds, 8.0);
        assert_eq!(f.units[0].slot, "STAND");
        assert_eq!(f.units[0].slot_start, 6.0);
        assert_eq!(f.units[0].position, [0.0, 7.5]);
        // The same time twice gives the same frame.
        assert_eq!(evaluate(&p, &clip, &speeds, 4.2), evaluate(&p, &clip, &speeds, 4.2));
    }

    #[test]
    fn camera_keys() {
        let mut p = Project::new("Test", "attila");
        let clip_id = p.clips[0].id;
        if let TrackContent::Camera { keys } = &mut p.clip_mut(clip_id).unwrap().tracks[0].content {
            keys.push(CameraKey { t: 0.0, position: [0.0, 10.0, 0.0], target: [0.0, 0.0, 10.0], fov: 35.0, interp: Interp::Linear });
            keys.push(CameraKey { t: 2.0, position: [10.0, 10.0, 0.0], target: [10.0, 0.0, 10.0], fov: 45.0, interp: Interp::Linear });
        }
        let clip = p.clip(clip_id).unwrap().clone();
        let c = evaluate(&p, &clip, &SpeedTable::new(), 1.0).camera.unwrap();
        assert_eq!(c.position, [5.0, 10.0, 0.0]);
        assert_eq!(c.fov, 40.0);
    }

    #[test]
    fn json_round_trip() {
        let (p, _) = project_with_unit();
        let text = p.to_json();
        assert_eq!(Project::from_json(&text).unwrap(), p);
    }
}
