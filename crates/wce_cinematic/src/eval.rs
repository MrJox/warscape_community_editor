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
    /// The run of action keys playing now (from `slot_start`): each key's animation for its time, one after the other, blending.
    pub run: Vec<RunClip>,
}

/// An action key in its run: its animation, how long it plays (to the next key, or the run's end), where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct RunClip {
    pub slot: String,
    pub length: f64,
    pub place: Option<[f32; 3]>,
}

/// The run of action keys `t` is in: its start and clips (keys back to the last `AUTO`, on to the next one or `end`); none when the unit
/// is moving or standing on its own.
pub fn action_run(actions: &[crate::scene::ActionKey], t: f64, end: f64) -> Option<(f64, Vec<RunClip>)> {
    let now = actions.iter().rposition(|k| k.t <= t)?;
    if actions[now].slot == AUTO_SLOT {
        return None;
    }
    let first = (0..=now).rev().take_while(|&i| actions[i].slot != AUTO_SLOT).last()?;
    let last = (now..actions.len()).take_while(|&i| actions[i].slot != AUTO_SLOT).last()?;
    let run = (first..=last)
        .map(|i| {
            let next = actions.get(i + 1).map_or(end, |k| k.t);
            RunClip { slot: actions[i].slot.clone(), length: (next - actions[i].t).max(1e-3), place: actions[i].place }
        })
        .collect();
    Some((actions[first].t, run))
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
    /// The live camera's view (the Camera Cuts' camera at `t`).
    pub camera: Option<CameraState>,
    /// The live camera's track, and every camera's view at `t` (by track id, in the clip's order).
    pub live_camera: Option<u32>,
    pub cameras: Vec<(u32, CameraState)>,
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
                if let Some(c) = camera_at(keys, t) {
                    f.cameras.push((track.id, c));
                }
            }
            TrackContent::Cuts { .. } => {}
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
                    // The run of keys it is in: played as one sequence from its first key.
                    if let Some((start, run)) = action_run(actions, t, clip.length) {
                        state.slot_start = start;
                        state.run = run;
                    }
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
            run: vec![],
        });
    }
    f.units.sort_by_key(|u| u.unit);
    f.live_camera = live_camera(clip, t).filter(|id| f.cameras.iter().any(|(c, _)| c == id));
    f.camera = f.live_camera.and_then(|id| f.cameras.iter().find(|(c, _)| *c == id)).map(|(_, c)| c.clone());
    f
}

/// A camera's view at `t` from its keys (none without keys).
pub fn camera_at(keys: &[crate::scene::CameraKey], t: f64) -> Option<CameraState> {
    let times: Vec<f64> = keys.iter().map(|k| k.t).collect();
    let (i, j, u) = around(&times, t)?;
    let positions: Vec<[f32; 3]> = keys.iter().map(|k| k.position).collect();
    let targets: Vec<[f32; 3]> = keys.iter().map(|k| k.target).collect();
    let interp = keys[i].interp;
    // The field of view on the same curve as the place (smoothstep between each pair of keys stopped the zoom at every key: a jolt
    // there in game).
    let fovs: Vec<[f32; 1]> = keys.iter().map(|k| [k.fov]).collect();
    Some(CameraState {
        position: point(&positions, &times, i, j, u, interp),
        target: point(&targets, &times, i, j, u, interp),
        fov: point(&fovs, &times, i, j, u, interp)[0],
    })
}

/// The cameras of a clip that can be live: its Camera tracks that are on and have keys, in order.
pub fn clip_cameras(clip: &Clip) -> Vec<u32> {
    clip.tracks
        .iter()
        .filter(|tr| !tr.muted && matches!(&tr.content, TrackContent::Camera { keys } if !keys.is_empty()))
        .map(|tr| tr.id)
        .collect()
}

/// The clip's cuts (its Camera Cuts track's, on), by time; only cuts to cameras that can be live.
pub fn clip_cuts(clip: &Clip) -> Vec<crate::scene::CutKey> {
    let cams = clip_cameras(clip);
    let mut cuts: Vec<crate::scene::CutKey> = clip
        .tracks
        .iter()
        .filter(|tr| !tr.muted)
        .find_map(|tr| match &tr.content {
            TrackContent::Cuts { cuts } => Some(cuts.clone()),
            _ => None,
        })
        .unwrap_or_default();
    cuts.retain(|c| cams.contains(&c.camera));
    cuts.sort_by(|a, b| a.t.total_cmp(&b.t));
    cuts
}

/// The live camera at `t`: the last cut's at or before it (before the first cut, the first cut's), else the first camera.
pub fn live_camera(clip: &Clip, t: f64) -> Option<u32> {
    let cuts = clip_cuts(clip);
    if let Some(first) = cuts.first() {
        return Some(cuts.iter().rev().find(|c| c.t <= t + 1e-9).unwrap_or(first).camera);
    }
    clip_cameras(clip).first().copied()
}

/// The clip's shots: (from, to, camera) spans where one camera is live, in order, covering the clip.
pub fn shots(clip: &Clip) -> Vec<(f64, f64, u32)> {
    let cuts = clip_cuts(clip);
    let end = clip.length.max(0.0);
    if cuts.is_empty() {
        return clip_cameras(clip).first().map(|&c| vec![(0.0, end, c)]).unwrap_or_default();
    }
    let mut out: Vec<(f64, f64, u32)> = vec![];
    for (k, c) in cuts.iter().enumerate() {
        let from = if k == 0 { 0.0 } else { c.t.clamp(0.0, end) };
        let to = cuts.get(k + 1).map_or(end, |n| n.t.clamp(0.0, end));
        if to <= from + 1e-9 {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.2 == c.camera => last.1 = to,
            _ => out.push((from, to, c.camera)),
        }
    }
    out
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
        run: vec![],
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

/// The clip as a historical battle would play it (docs/plans/cinematic.md, "Battle gap"): what `battle_export` writes, run the way
/// the engine runs it, roughly. A unit is teleported to its first path key, then each later key is an order given when its segment
/// starts: the unit goes there in a straight line at its own walk or run speed (the export's choice) until it arrives or a newer
/// order comes, so it can be late or early against its keys; a Hold segment teleports at its end. Animation slots other than a
/// taunt, VFX and keyed weather are movie only: dropped. Men can only be fewer. The camera, sounds, music, subtitles, fades and
/// depth of field are as keyed.
pub fn evaluate_battle(project: &Project, clip: &Clip, speeds: &SpeedTable, t: f64) -> SceneFrame {
    let mut f = evaluate(project, clip, speeds, t);
    f.vfx.clear();
    f.weather = None;
    for track in clip.tracks.iter().filter(|tr| !tr.muted) {
        let TrackContent::Unit { unit, path, actions, men, .. } = &track.content else { continue };
        let Some(su) = project.unit(*unit) else { continue };
        let Some(state) = f.units.iter_mut().find(|s| s.unit == *unit) else { continue };
        let sp = speeds.get(&su.unit).copied().unwrap_or_default();
        let ordered = ordered_move(su.start, su.facing, path, sp, t.max(0.0));
        state.position = ordered.position;
        state.facing = ordered.facing;
        state.speed = ordered.speed;
        state.slot = ordered.slot.into();
        state.slot_start = ordered.since;
        state.slot_distance = ordered.covered;
        if let Some(k) = actions.iter().rev().find(|k| k.t <= t)
            && crate::scene::slot_family(&k.slot) == "TAUNT"
        {
            state.slot = "TAUNT".into();
            state.slot_start = k.t;
            state.slot_distance = 0.0;
        }
        let mut alive = su.men;
        for m in men.iter().filter(|m| m.t <= t) {
            alive = alive.min(m.men);
        }
        state.men = alive;
    }
    f
}

struct Ordered {
    position: [f32; 2],
    facing: f32,
    speed: f32,
    slot: &'static str,
    since: f64,
    covered: f32,
}

/// A unit under orders: teleported to the first key, then sent to each next key when its segment starts (straight, at its gait's
/// speed), stopping where it gets to.
fn ordered_move(start: [f32; 2], start_facing: f32, path: &[PathKey], sp: Speeds, t: f64) -> Ordered {
    let mut o = Ordered { position: start, facing: start_facing, speed: 0.0, slot: "STAND", since: 0.0, covered: 0.0 };
    let Some(first) = path.first() else { return o };
    o.position = first.position;
    // Orders: (when, where to, speed; 0 = teleport at that time).
    let mut orders: Vec<(f64, [f32; 2], f32, &'static str)> = vec![];
    for w in path.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.interp == Interp::Step || b.t <= a.t {
            orders.push((b.t, b.position, 0.0, "STAND"));
            continue;
        }
        let dist = ((b.position[0] - a.position[0]).powi(2) + (b.position[1] - a.position[1]).powi(2)).sqrt();
        if dist < 0.01 {
            continue;
        }
        let keyed = dist / (b.t - a.t) as f32;
        let run = matches!(a.gait, Some(Gait::Run | Gait::Charge)) || (a.gait.is_none() && slot_for_speed(keyed, sp) != "WALK");
        let (v, slot) = if run { (sp.run, "RUN") } else { (sp.walk, "WALK") };
        orders.push((a.t, b.position, v.max(0.1), slot));
    }
    orders.sort_by(|a, b| a.0.total_cmp(&b.0));
    // From each order to the next (or to t), move towards its target.
    for (i, &(when, target, v, slot)) in orders.iter().enumerate() {
        if when > t {
            break;
        }
        if v == 0.0 {
            o.position = target;
            (o.speed, o.slot, o.since, o.covered) = (0.0, "STAND", when, 0.0);
            continue;
        }
        let until = orders.get(i + 1).map_or(t, |n| n.0.min(t));
        let (dx, dz) = (target[0] - o.position[0], target[1] - o.position[1]);
        let left = (dx * dx + dz * dz).sqrt();
        if left < 1e-4 {
            continue;
        }
        o.facing = dx.atan2(dz);
        let step = (v as f64 * (until - when)) as f32;
        if step >= left {
            // Arrived on the way: standing since then.
            o.position = target;
            let arrived = when + (left / v) as f64;
            (o.speed, o.slot, o.since, o.covered) = (0.0, "STAND", arrived, 0.0);
        } else {
            o.position = [o.position[0] + dx / left * step, o.position[1] + dz / left * step];
            if o.slot != slot {
                (o.since, o.covered) = (when, 0.0);
            }
            (o.speed, o.slot) = (v, slot);
            o.covered += step;
        }
    }
    o
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
            command: true,
            character: String::new(),
        });
        (p, id)
    }

    /// A clip of two cameras (ids 1 and 2, each still at its own place) and cuts.
    fn two_cameras(cuts: Vec<crate::scene::CutKey>) -> Clip {
        let cam = |id: u32, x: f32| Track {
            id,
            name: format!("Camera {id}"),
            muted: false,
            content: TrackContent::Camera {
                keys: vec![CameraKey { t: 0.0, position: [x, 10.0, 0.0], target: [x, 0.0, 10.0], fov: 35.0, interp: Interp::Smooth }],
            },
        };
        let mut tracks = vec![cam(1, 0.0), cam(2, 100.0)];
        if !cuts.is_empty() {
            tracks.push(Track { id: 3, name: "Camera Cuts".into(), muted: false, content: TrackContent::Cuts { cuts } });
        }
        Clip { id: 9, name: "c".into(), length: 10.0, fps: 30, tracks, skippable: true, motion_blur: false }
    }

    #[test]
    fn a_run_of_keys_plays_as_one() {
        use crate::scene::ActionKey;
        let k = |t: f64, slot: &str| ActionKey { t, slot: slot.into(), place: None };
        let keys = vec![k(1.0, "COMBAT_READY"), k(2.0, "file:a.anim"), k(2.8, "file:b.anim"), k(4.0, AUTO_SLOT), k(5.0, "TAUNT")];
        // Within the first run: from its first key, each clip to the next key, the last to the AUTO.
        let (start, run) = action_run(&keys, 2.5, 10.0).unwrap();
        assert_eq!(start, 1.0);
        assert_eq!(
            run.iter().map(|c| (c.slot.as_str(), (c.length * 10.0).round())).collect::<Vec<_>>(),
            [("COMBAT_READY", 10.0), ("file:a.anim", 8.0), ("file:b.anim", 12.0)]
        );
        // AUTO: none; the last run goes on to the clip's end.
        assert!(action_run(&keys, 4.5, 10.0).is_none());
        assert_eq!(action_run(&keys, 6.0, 10.0).unwrap().1[0].length, 5.0);
        assert!(action_run(&keys, 0.5, 10.0).is_none());
    }

    #[test]
    fn the_cuts_choose_the_live_camera() {
        use crate::scene::CutKey;
        let p = Project::new("Test", "attila");
        // No cuts: the first camera all along.
        let clip = two_cameras(vec![]);
        assert_eq!(evaluate(&p, &clip, &SpeedTable::new(), 5.0).live_camera, Some(1));
        assert_eq!(shots(&clip), [(0.0, 10.0, 1)]);
        // Cut to 2 at 4 s, back to 1 at 7 s; a cut to a camera that isn't there is left out.
        let clip = two_cameras(vec![
            CutKey { t: 7.0, camera: 1 },
            CutKey { t: 4.0, camera: 2 },
            CutKey { t: 8.0, camera: 99 },
            CutKey { t: 0.0, camera: 1 },
        ]);
        let f = evaluate(&p, &clip, &SpeedTable::new(), 5.0);
        assert_eq!(f.live_camera, Some(2));
        assert_eq!(f.camera.map(|c| c.position[0]), Some(100.0));
        assert_eq!(f.cameras.len(), 2);
        assert_eq!(evaluate(&p, &clip, &SpeedTable::new(), 3.9).live_camera, Some(1));
        assert_eq!(evaluate(&p, &clip, &SpeedTable::new(), 7.0).live_camera, Some(1));
        assert_eq!(shots(&clip), [(0.0, 4.0, 1), (4.0, 7.0, 2), (7.0, 10.0, 1)]);
        // Before the first cut: the first cut's camera.
        let clip = two_cameras(vec![CutKey { t: 2.0, camera: 2 }]);
        assert_eq!(evaluate(&p, &clip, &SpeedTable::new(), 1.0).live_camera, Some(2));
        assert_eq!(shots(&clip), [(0.0, 10.0, 2)]);
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

    #[test]
    fn battle_orders_drift_from_keys() {
        let (mut p, unit) = project_with_unit();
        let clip_id = p.clips[0].id;
        let track = p.new_id();
        // 10 m in 10 s: a 1 m/s walk keyed; the unit walks at 1.6 m/s in battle, so it arrives at 6.25 s and stands.
        let path = vec![
            PathKey { t: 0.0, position: [0.0, 0.0], interp: Interp::Linear, gait: None },
            PathKey { t: 10.0, position: [0.0, 10.0], interp: Interp::Linear, gait: None },
        ];
        p.clip_mut(clip_id).unwrap().tracks.push(Track {
            id: track,
            name: "u".into(),
            muted: false,
            content: TrackContent::Unit { unit, path, actions: vec![], men: vec![], visible: vec![] },
        });
        let clip = p.clip(clip_id).unwrap().clone();
        let speeds = SpeedTable::new();
        let keyed = evaluate(&p, &clip, &speeds, 5.0);
        let battle = evaluate_battle(&p, &clip, &speeds, 5.0);
        assert!((keyed.units[0].position[1] - 5.0).abs() < 1e-3);
        assert!((battle.units[0].position[1] - 8.0).abs() < 1e-3, "{:?}", battle.units[0].position);
        assert_eq!(battle.units[0].slot, "WALK");
        let later = evaluate_battle(&p, &clip, &speeds, 8.0);
        assert_eq!(later.units[0].position, [0.0, 10.0]);
        assert_eq!(later.units[0].slot, "STAND");
        assert!((later.units[0].slot_start - 6.25).abs() < 1e-6);
    }
}
