//! The keyed scene (docs/plans/cinematic.md, step 3): one model for both outputs. A project holds the battle's setup, the units
//! on the field, the cutscene clips (each its own timeline) and the battle logic. Every value that changes over time is a key on
//! a track: `eval::evaluate(project, clip, t)` is a pure function of them, so moving the timeline gives the same scene every time.
//!
//! Space is the battle's: metres, x across, z forward (the battle XML's `position x / y`), y up (the Lua camera's `v(x, y, z)`).
//! Times are seconds from the clip's start.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The file format's version (`project.json`).
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub name: String,
    /// The game profile's key ("attila").
    pub game: String,
    pub map: MapPlacement,
    pub battle: BattleSetup,
    pub armies: Vec<Army>,
    pub units: Vec<SceneUnit>,
    pub clips: Vec<Clip>,
    /// Text in every language: text id -> language -> text (subtitles, the battle's name and description...).
    #[serde(default)]
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// The battle logic (step 7): nodes and links, kept as written until the graph editor exists.
    #[serde(default)]
    pub logic: LogicGraph,
    /// Ids handed out so far (units, clips, tracks).
    #[serde(default)]
    pub next_id: u32,
}

/// Where the battle is: a position on the campaign map (the battle XML's `tile_map_position`, 0 to 1 of the map), or one battle tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapPlacement {
    /// The campaign map folder (`main_attila_map`).
    pub campaign_map: String,
    pub position: [f32; 2],
    /// The playable area's side, metres.
    pub playable_area: f32,
    /// A battle tile's folder (`terrain/tiles/battle/plains/8x8/8x8_aa`): the battle is fought on that tile alone, not at the map
    /// position. Empty: the map position.
    #[serde(default)]
    pub tile: String,
    /// The battle map is fought whole, as vanilla's historical battles are (`terrain/battles/catalaunian_plains/`, made in TEd): its
    /// name alone, no position.
    #[serde(default)]
    pub whole_map: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleSetup {
    /// The `battles` row's key and the script folder's name.
    pub key: String,
    /// Text ids (in `Project::texts`) of the menu's name and description.
    pub title_text: String,
    pub description_text: String,
    pub year: i32,
    /// `Summer`, `Winter`...
    pub season: String,
    /// `day`, `night`...
    pub time_of_day: String,
    /// `dry`, `rain`, `snow`...
    pub precipitation: String,
    /// `land_normal`...
    pub battle_type: String,
    pub duration_s: f32,
    /// The weather file at the start (`Weather\...\x.environment`); keys of a clip's weather track can change it in a movie.
    pub environment: String,
    pub wind: [f32; 2],
    /// The Historical Battles map's pixel position (`historical_battles_ui_locations`).
    pub menu_position: [i32; 2],
}

/// One side's army (the battle XML's `alliance` > `army`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Army {
    pub alliance: u32,
    pub faction: String,
    pub deployment_centre: [f32; 2],
    pub deployment_size: [f32; 2],
    pub deployment_orientation: f32,
    /// The player's army (exactly one).
    #[serde(default)]
    pub player: bool,
}

/// A unit on the field: what the battle XML places and what the clips animate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneUnit {
    pub id: u32,
    pub name: String,
    /// `main_units` key.
    pub unit: String,
    /// Index into `Project::armies`.
    pub army: usize,
    /// Its place and facing at the battle's start (radians, 0 = +z).
    pub start: [f32; 2],
    pub facing: f32,
    /// Formation width, metres.
    pub width: f32,
    /// Men in a movie; in a battle "start with N men" through `kill_number_of_men` (docs/plans/cinematic.md, 1.3).
    pub men: u32,
    /// The variation seed (each man's looks and animation phase).
    pub seed: u32,
    #[serde(default)]
    pub general: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u32,
    pub name: String,
    pub length: f64,
    pub fps: u32,
    pub tracks: Vec<Track>,
    #[serde(default = "yes")]
    pub skippable: bool,
    /// Movies: each frame the average of sub-frames over a 180 degree shutter (no game shader does it: a deviation the user
    /// approved, 2026-11-20). Off by default.
    #[serde(default)]
    pub motion_blur: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub muted: bool,
    pub content: TrackContent,
}

/// How a value goes from one key to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interp {
    /// Holds until the next key.
    Step,
    Linear,
    /// Smooth through the keys (Catmull-Rom for points, smoothstep for numbers).
    #[default]
    Smooth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TrackContent {
    /// A camera: an object of the scene (its place, aim and lens keyed). A clip may have several; the Camera Cuts say which one is
    /// live when.
    Camera {
        keys: Vec<CameraKey>,
    },
    /// The Camera Cuts (Unreal's): from each cut on, the camera it names (a Camera track's id) is the one the clip shows. Without
    /// cuts: the first camera.
    Cuts {
        cuts: Vec<CutKey>,
    },
    Unit {
        unit: u32,
        path: Vec<PathKey>,
        actions: Vec<ActionKey>,
        men: Vec<MenKey>,
        visible: Vec<VisibleKey>,
    },
    Sound {
        events: Vec<SoundEvent>,
    },
    Music {
        cues: Vec<MusicCue>,
    },
    Subtitles {
        lines: Vec<SubtitleLine>,
    },
    Vfx {
        instances: Vec<VfxInstance>,
    },
    /// 0 clear, 1 black.
    Fade {
        keys: Vec<ScalarKey>,
    },
    /// 0 none, 1 the cinematic bars.
    Letterbox {
        keys: Vec<ScalarKey>,
    },
    DepthOfField {
        keys: Vec<DofKey>,
    },
    Weather {
        keys: Vec<WeatherKey>,
    },
}

/// A cut to a camera (a Camera track's id) at `t`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CutKey {
    pub t: f64,
    pub camera: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    pub t: f64,
    pub position: [f32; 3],
    pub target: [f32; 3],
    /// Vertical field of view, degrees (the Lua `move_to`'s unit).
    pub fov: f32,
    #[serde(default)]
    pub interp: Interp,
}

/// Movement: from one key to the next along a line or a smooth path. The gait follows from the speed unless `gait` says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathKey {
    pub t: f64,
    pub position: [f32; 2],
    #[serde(default)]
    pub interp: Interp,
    #[serde(default)]
    pub gait: Option<Gait>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gait {
    Walk,
    Run,
    Charge,
}

/// An animation slot (`STAND`, `ATTACK_1`, `CELEBRATE`, `DIE`...) from `t` until the next action key; `AUTO` gives the slot back to
/// the movement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionKey {
    pub t: f64,
    /// A slot of the unit's animations (or its family), or `file:<a .anim of the packs>` (a matched combat's clip).
    pub slot: String,
    /// Where the animation's root goes in the unit's frame: x and z (metres) and a turn (degrees). A matched combat's two clips share
    /// one space; their keys place each pair where the fighters are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<[f32; 3]>,
}

/// The prefix of an action key's slot naming an animation file of the packs.
pub const FILE_SLOT: &str = "file:";

pub const AUTO_SLOT: &str = "AUTO";

/// A slot's family: its name without a trailing variant number (`CELEBRATE_3` -> `CELEBRATE`, `STAND` -> `STAND`). An action key may
/// name a family (its variants spread over the men) or one slot.
pub fn slot_family(slot: &str) -> &str {
    match slot.rsplit_once('_') {
        Some((head, n)) if !head.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => slot,
    }
}

/// The slots an action key's slot plays among a unit's: itself when the unit has it, else its family's variants in order (none: the
/// unit has neither).
pub fn slot_variants<'a>(slot: &str, own: &'a [String]) -> Vec<&'a str> {
    if let Some(s) = own.iter().find(|s| *s == slot) {
        return vec![s.as_str()];
    }
    let mut v: Vec<&str> = own.iter().map(String::as_str).filter(|s| slot_family(s) == slot && *s != slot).collect();
    v.sort_by_key(|s| s.rsplit_once('_').and_then(|(_, n)| n.parse::<u32>().ok()).unwrap_or(0));
    v
}

/// The slots a mounted man plays for a key's slot, in order of preference: his `RIDER_` ones (the movement's gaits as the horse's:
/// walk, canter, gallop), then the slot itself.
pub fn rider_slots(slot: &str) -> Vec<String> {
    if slot.starts_with("RIDER_") {
        return vec![slot.to_owned()];
    }
    let gait: &[&str] = match slot_family(slot) {
        "STAND" => &["RIDER_STAND"],
        "WALK" => &["RIDER_WALK"],
        "RUN" => &["RIDER_CANTER", "RIDER_TROT"],
        "CHARGE" => &["RIDER_CHARGE", "RIDER_GALLOP"],
        _ => &[],
    };
    let mut out: Vec<String> = gait.iter().map(|s| (*s).to_owned()).collect();
    out.push(format!("RIDER_{slot}"));
    out.push(slot.to_owned());
    out
}

/// What a key's slot plays among a man's slots: [`slot_variants`] of the slot, or for a rider of the first of [`rider_slots`] he has.
pub fn resolve_slot<'a>(slot: &str, own: &'a [String], mounted: bool) -> Vec<&'a str> {
    if !mounted {
        return slot_variants(slot, own);
    }
    rider_slots(slot).iter().map(|s| slot_variants(s, own)).find(|v| !v.is_empty()).unwrap_or_default()
}

#[cfg(test)]
mod slot_tests {
    use super::{resolve_slot, slot_family, slot_variants};

    #[test]
    fn a_rider_plays_his_own() {
        let own: Vec<String> =
            ["RUN_1", "RIDER_CANTER", "RIDER_CELEBRATE_1", "RIDER_CELEBRATE_2", "CELEBRATE_1"].map(str::to_owned).to_vec();
        assert_eq!(resolve_slot("RUN", &own, true), ["RIDER_CANTER"]);
        assert_eq!(resolve_slot("CELEBRATE", &own, true), ["RIDER_CELEBRATE_1", "RIDER_CELEBRATE_2"]);
        assert_eq!(resolve_slot("RUN", &own, false), ["RUN_1"]);
        assert!(resolve_slot("CHARGE", &own, true).is_empty());
    }

    #[test]
    fn families_and_variants() {
        assert_eq!(slot_family("CELEBRATE_10"), "CELEBRATE");
        assert_eq!(slot_family("STAND"), "STAND");
        assert_eq!(slot_family("RAMP_JUMP_TAKEOFF3"), "RAMP_JUMP_TAKEOFF3");
        let own: Vec<String> = ["CELEBRATE_10", "CELEBRATE_2", "CELEBRATE_1", "STAND", "TAUNT_1"].map(str::to_owned).to_vec();
        assert_eq!(slot_variants("CELEBRATE", &own), ["CELEBRATE_1", "CELEBRATE_2", "CELEBRATE_10"]);
        assert_eq!(slot_variants("CELEBRATE_2", &own), ["CELEBRATE_2"]);
        assert_eq!(slot_variants("TAUNT", &own), ["TAUNT_1"]);
        assert!(slot_variants("CHEER", &own).is_empty());
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MenKey {
    pub t: f64,
    pub men: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisibleKey {
    pub t: f64,
    pub visible: bool,
}

/// A Wwise event played at `t`, in the world when it has a position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundEvent {
    pub t: f64,
    pub event: String,
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    #[serde(default = "one")]
    pub volume: f32,
}

fn one() -> f32 {
    1.0
}

/// Music: a Wwise event from `t`, until `stop` (or the clip's end), with fades in seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MusicCue {
    pub t: f64,
    /// An event that plays music by itself (a mod's); empty with moods.
    pub event: String,
    /// The game's music by mood (`music::Mood`: `music_set_*` events, a group each): the battle plays them and its music follows;
    /// the editor renders the branch they lead to.
    #[serde(default)]
    pub moods: Vec<String>,
    #[serde(default)]
    pub stop: Option<f64>,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
}

/// A subtitle line: a text id of `Project::texts`, shown from `t` to `end`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubtitleLine {
    pub t: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VfxInstance {
    pub start: f64,
    pub end: f64,
    /// The vfx definition's key.
    pub vfx: String,
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_y: f32,
    #[serde(default = "one")]
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalarKey {
    pub t: f64,
    pub value: f32,
    #[serde(default)]
    pub interp: Interp,
}

/// Depth of field: the battle camera's `change_depth_of_field` values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DofKey {
    pub t: f64,
    pub focus_distance: f32,
    pub focal_length: f32,
    pub blur_size: f32,
    #[serde(default)]
    pub interp: Interp,
}

/// The weather from `t`: between two keys the scene blends from one environment to the next (the editor's own rule; the game has
/// no such blend: movie only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherKey {
    pub t: f64,
    pub environment: String,
    #[serde(default)]
    pub interp: Interp,
}

/// The battle logic graph (step 7).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LogicGraph {
    pub nodes: Vec<LogicNode>,
    pub links: Vec<LogicLink>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicNode {
    pub id: u32,
    /// The node's type (`event.battle_start`, `action.play_clip`...).
    pub kind: String,
    /// Its settings, by name.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    pub position: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogicLink {
    pub from: u32,
    pub from_pin: String,
    pub to: u32,
    pub to_pin: String,
}

impl Project {
    /// A new project: one army a side, no units, one empty intro clip of 10 s at 30 fps with a camera track.
    pub fn new(name: &str, game: &str) -> Self {
        let mut p = Self {
            format: FORMAT,
            name: name.to_owned(),
            game: game.to_owned(),
            map: MapPlacement {
                campaign_map: "main_attila_map".into(),
                position: [0.5, 0.5],
                playable_area: 1924.0,
                tile: String::new(),
                whole_map: false,
            },
            battle: BattleSetup {
                key: sanitize_key(name),
                title_text: "battle_title".into(),
                description_text: "battle_description".into(),
                year: 451,
                season: "Summer".into(),
                time_of_day: "day".into(),
                precipitation: "dry".into(),
                battle_type: "land_normal".into(),
                duration_s: 3600.0,
                // Vanilla's Catalaunian Plains weather: a sky and light from the start.
                environment: r"Weather\default\default\land\day\dry\evening_north_3.environment".into(),
                wind: [0.0, 9.0],
                menu_position: [300, 300],
            },
            armies: vec![
                Army {
                    alliance: 0,
                    faction: "att_fact_western_roman_empire".into(),
                    deployment_centre: [0.0, -368.0],
                    deployment_size: [832.0, 352.0],
                    deployment_orientation: 0.0,
                    player: true,
                },
                Army {
                    alliance: 1,
                    faction: "att_fact_hunni".into(),
                    deployment_centre: [0.0, 368.0],
                    deployment_size: [832.0, 352.0],
                    deployment_orientation: std::f32::consts::PI,
                    player: false,
                },
            ],
            units: vec![],
            clips: vec![],
            texts: BTreeMap::new(),
            logic: LogicGraph::default(),
            next_id: 1,
        };
        // Attila: vanilla's Catalaunian Plains map, whole (the campaign map's centre is the sea).
        if game == "attila" {
            p.map.campaign_map = "catalaunian_plains".into();
            p.map.whole_map = true;
        }
        p.texts.insert("battle_title".into(), BTreeMap::from([("en".into(), name.to_owned())]));
        p.texts.insert("battle_description".into(), BTreeMap::from([("en".into(), String::new())]));
        let camera = p.new_id();
        let clip = p.new_id();
        p.clips.push(Clip {
            id: clip,
            name: "Intro".into(),
            length: 10.0,
            fps: 30,
            tracks: vec![Track { id: camera, name: "Camera".into(), muted: false, content: TrackContent::Camera { keys: vec![] } }],
            skippable: true,
            motion_blur: false,
        });
        p
    }

    pub fn new_id(&mut self) -> u32 {
        self.next_id = self.next_id.max(1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn unit(&self, id: u32) -> Option<&SceneUnit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn clip(&self, id: u32) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clip_mut(&mut self, id: u32) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    /// The text in a language, English when it has none.
    pub fn text(&self, id: &str, language: &str) -> Option<&str> {
        let t = self.texts.get(id)?;
        t.get(language).or_else(|| t.get("en")).map(String::as_str)
    }

    /// Sorts every track's keys by time (an edit may leave them out of order; evaluation expects them sorted).
    pub fn normalize(&mut self) {
        for clip in &mut self.clips {
            for track in &mut clip.tracks {
                track.content.sort();
            }
        }
    }

    /// The project as written to `project.json`.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("a project always serialises");
        s.push('\n');
        s
    }

    /// # Errors
    ///
    /// Not a project, or of a later format.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut p: Project = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if p.format > FORMAT {
            return Err(format!("a project of format {} (this editor reads up to {FORMAT})", p.format));
        }
        let max = p
            .units
            .iter()
            .map(|u| u.id)
            .chain(p.clips.iter().map(|c| c.id))
            .chain(p.clips.iter().flat_map(|c| c.tracks.iter().map(|t| t.id)))
            .max();
        p.next_id = p.next_id.max(max.map_or(1, |m| m + 1));
        p.normalize();
        Ok(p)
    }
}

impl TrackContent {
    pub fn kind_name(&self) -> &'static str {
        match self {
            TrackContent::Camera { .. } => "Camera",
            TrackContent::Cuts { .. } => "Camera Cuts",
            TrackContent::Unit { .. } => "Unit",
            TrackContent::Sound { .. } => "Sound",
            TrackContent::Music { .. } => "Music",
            TrackContent::Subtitles { .. } => "Subtitles",
            TrackContent::Vfx { .. } => "VFX",
            TrackContent::Fade { .. } => "Fade",
            TrackContent::Letterbox { .. } => "Letterbox",
            TrackContent::DepthOfField { .. } => "Depth of field",
            TrackContent::Weather { .. } => "Weather",
        }
    }

    /// Every key's or event's time, sorted (the timeline's marks).
    pub fn times(&self) -> Vec<f64> {
        let mut out: Vec<f64> = match self {
            TrackContent::Camera { keys } => keys.iter().map(|k| k.t).collect(),
            TrackContent::Cuts { cuts } => cuts.iter().map(|k| k.t).collect(),
            TrackContent::Unit { path, actions, men, visible, .. } => path
                .iter()
                .map(|k| k.t)
                .chain(actions.iter().map(|k| k.t))
                .chain(men.iter().map(|k| k.t))
                .chain(visible.iter().map(|k| k.t))
                .collect(),
            TrackContent::Sound { events } => events.iter().map(|k| k.t).collect(),
            TrackContent::Music { cues } => cues.iter().map(|k| k.t).collect(),
            TrackContent::Subtitles { lines } => lines.iter().map(|k| k.t).collect(),
            TrackContent::Vfx { instances } => instances.iter().map(|k| k.start).collect(),
            TrackContent::Fade { keys } | TrackContent::Letterbox { keys } => keys.iter().map(|k| k.t).collect(),
            TrackContent::DepthOfField { keys } => keys.iter().map(|k| k.t).collect(),
            TrackContent::Weather { keys } => keys.iter().map(|k| k.t).collect(),
        };
        out.sort_by(f64::total_cmp);
        out
    }

    fn sort(&mut self) {
        fn by<T>(v: &mut [T], t: impl Fn(&T) -> f64) {
            v.sort_by(|a, b| t(a).total_cmp(&t(b)));
        }
        match self {
            TrackContent::Camera { keys } => by(keys, |k| k.t),
            TrackContent::Cuts { cuts } => by(cuts, |k| k.t),
            TrackContent::Unit { path, actions, men, visible, .. } => {
                by(path, |k| k.t);
                by(actions, |k| k.t);
                by(men, |k| k.t);
                by(visible, |k| k.t);
            }
            TrackContent::Sound { events } => by(events, |k| k.t),
            TrackContent::Music { cues } => by(cues, |k| k.t),
            TrackContent::Subtitles { lines } => by(lines, |k| k.t),
            TrackContent::Vfx { instances } => by(instances, |k| k.start),
            TrackContent::Fade { keys } | TrackContent::Letterbox { keys } => by(keys, |k| k.t),
            TrackContent::DepthOfField { keys } => by(keys, |k| k.t),
            TrackContent::Weather { keys } => by(keys, |k| k.t),
        }
    }
}

/// A key usable as a DB key and a folder name: lower case, `_` for anything but letters and digits.
pub fn sanitize_key(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if s.is_empty() { "wce_battle".into() } else { s }
}
