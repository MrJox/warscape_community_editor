//! The battle logic graph compiled to Lua on `lib_generated_battle` (docs/plans/cinematic.md, step 7): an event node raises a
//! message (`battle_started`, `cutscene_ended`, or its own, `wce_n<id>`, through the library's `message_on_*`); an action node
//! linked to it reacts to that message (`*_on_message`, or a listener). Vanilla's Catalaunian Plains is written this way.
//!
//! Without nodes, the default: when the intro ends (or the battle starts, without one) the armies against the player's alliance
//! attack and the player's AI allies are released to their AI.

use std::fmt::Write as _;

use crate::battle_export::{army_var, clip_ended_message, clip_function};
use crate::scene::{LogicNode, Project};

/// A node type: its key, the editor's label, whether it is an event (an output) or an action (an input), and its settings.
pub struct NodeType {
    pub kind: &'static str,
    pub label: &'static str,
    pub event: bool,
    pub settings: &'static [&'static str],
}

pub const NODE_TYPES: &[NodeType] = &[
    NodeType { kind: "event.battle_started", label: "Battle starts", event: true, settings: &[] },
    NodeType { kind: "event.cutscene_ended", label: "A cutscene ends", event: true, settings: &[] },
    NodeType { kind: "event.clip_ended", label: "Clip ends", event: true, settings: &["clip"] },
    NodeType { kind: "event.time", label: "Time after battle start", event: true, settings: &["seconds"] },
    NodeType { kind: "event.proximity", label: "Army near the enemy", event: true, settings: &["army", "distance"] },
    NodeType { kind: "event.casualties", label: "Army casualties", event: true, settings: &["army", "threshold"] },
    NodeType { kind: "event.rout", label: "Army routing", event: true, settings: &["army", "threshold"] },
    NodeType { kind: "event.commander_death", label: "Army's general dies", event: true, settings: &["army"] },
    NodeType { kind: "event.seen", label: "Army seen by the enemy", event: true, settings: &["army"] },
    NodeType { kind: "event.victory", label: "Army wins", event: true, settings: &["army"] },
    NodeType { kind: "event.defeat", label: "Army loses", event: true, settings: &["army"] },
    NodeType { kind: "event.message", label: "Message", event: true, settings: &["name"] },
    NodeType { kind: "action.attack", label: "Army attacks", event: false, settings: &["army"] },
    NodeType { kind: "action.defend", label: "Army defends a spot", event: false, settings: &["army", "x", "z", "radius"] },
    NodeType { kind: "action.release", label: "Army to its AI", event: false, settings: &["army"] },
    NodeType { kind: "action.advice", label: "Advisor says", event: false, settings: &["advice", "delay_ms"] },
    NodeType { kind: "action.play_clip", label: "Play a clip", event: false, settings: &["clip"] },
    NodeType { kind: "action.fade_in", label: "Fade in", event: false, settings: &["seconds"] },
    NodeType { kind: "action.music", label: "Play music", event: false, settings: &["event", "fade"] },
    NodeType { kind: "action.message", label: "Send a message", event: false, settings: &["name"] },
    NodeType { kind: "action.lua", label: "Lua", event: false, settings: &["code"] },
];

pub fn node_type(kind: &str) -> Option<&'static NodeType> {
    NODE_TYPES.iter().find(|t| t.kind == kind)
}

fn value<'a>(n: &'a LogicNode, k: &str) -> &'a str {
    n.values.get(k).map_or("", String::as_str)
}

fn num(n: &LogicNode, k: &str, default: f64) -> f64 {
    value(n, k).trim().parse().unwrap_or(default)
}

fn lua_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The army setting as the Lua variable of that army (an index into the project's armies, 0-based).
fn army(project: &Project, n: &LogicNode, warnings: &mut Vec<String>) -> Option<String> {
    match value(n, "army").trim().parse::<usize>() {
        Ok(i) if i < project.armies.len() => Some(army_var(i)),
        _ => {
            warnings.push(format!("logic node {}: no army {:?}", n.id, value(n, "army")));
            None
        }
    }
}

/// The message an event node raises, and the Lua that raises it.
fn event_message(project: &Project, n: &LogicNode, warnings: &mut Vec<String>) -> Option<(String, String)> {
    let own = format!("wce_n{}", n.id);
    let m = |s: &str| lua_str(s);
    Some(match n.kind.as_str() {
        "event.battle_started" => ("battle_started".into(), String::new()),
        "event.cutscene_ended" => ("cutscene_ended".into(), String::new()),
        "event.clip_ended" => {
            let id: u32 = value(n, "clip").trim().parse().ok()?;
            let clip = project.clip(id)?;
            (clip_ended_message(clip), String::new())
        }
        "event.message" => (value(n, "name").to_owned(), String::new()),
        "event.time" => {
            let ms = (num(n, "seconds", 0.0) * 1000.0).round() as i64;
            (own.clone(), format!("gb:message_on_time_offset({}, {ms});\n", m(&own)))
        }
        "event.proximity" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_proximity_to_enemy({}, {});\n", m(&own), num(n, "distance", 150.0)))
        }
        "event.casualties" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_casualties({}, {});\n", m(&own), num(n, "threshold", 0.5)))
        }
        "event.rout" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_rout_proportion({}, {});\n", m(&own), num(n, "threshold", 0.5)))
        }
        "event.commander_death" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_commander_death({});\n", m(&own)))
        }
        "event.seen" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_seen_by_enemy({});\n", m(&own)))
        }
        "event.victory" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_victory({});\n", m(&own)))
        }
        "event.defeat" => {
            let a = army(project, n, warnings)?;
            (own.clone(), format!("{a}:message_on_defeat({});\n", m(&own)))
        }
        _ => return None,
    })
}

/// The Lua of an action node reacting to `message`.
fn action(project: &Project, n: &LogicNode, message: &str, warnings: &mut Vec<String>) -> Option<String> {
    let msg = lua_str(message);
    let listen = |body: String| format!("gb:add_listener({msg}, function() {body} end);\n");
    Some(match n.kind.as_str() {
        "action.attack" => format!("{}:attack_on_message({msg});\n", army(project, n, warnings)?),
        "action.release" => format!("{}:release_on_message({msg});\n", army(project, n, warnings)?),
        "action.defend" => format!(
            "{}:defend_on_message({msg}, {}, {}, {});\n",
            army(project, n, warnings)?,
            num(n, "x", 0.0),
            num(n, "z", 0.0),
            num(n, "radius", 100.0)
        ),
        "action.advice" => format!("gb:advice_on_message({msg}, {}, {});\n", lua_str(value(n, "advice")), num(n, "delay_ms", 0.0) as i64),
        "action.fade_in" => format!("gb:fade_in_on_message({msg}, {});\n", num(n, "seconds", 0.5)),
        "action.play_clip" => {
            let id: u32 = value(n, "clip").trim().parse().ok()?;
            let Some(clip) = project.clip(id) else {
                warnings.push(format!("logic node {}: no clip {id}", n.id));
                return None;
            };
            listen(format!("{}()", clip_function(clip)))
        }
        "action.music" => {
            let fade = num(n, "fade", 0.0);
            listen(format!("bm:set_music_auto_playback(false); bm:play_music_custom_fade({}, {fade})", lua_str(value(n, "event"))))
        }
        "action.message" => listen(format!("sm:trigger_message({})", lua_str(value(n, "name")))),
        "action.lua" => listen(value(n, "code").to_owned()),
        _ => return None,
    })
}

/// The logic's Lua (appended to the start script, after the declarations).
pub fn compile(project: &Project, warnings: &mut Vec<String>) -> String {
    let mut s = String::from("\n-- The battle logic.\n");
    let g = &project.logic;
    if g.nodes.is_empty() {
        let start = if project.clips.is_empty() { "battle_started" } else { "cutscene_ended" };
        let player = project.armies.iter().position(|a| a.player).unwrap_or(0);
        let player_alliance = project.armies.get(player).map_or(0, |a| a.alliance);
        for (i, a) in project.armies.iter().enumerate() {
            if i == player {
                continue;
            }
            if a.alliance == player_alliance {
                let _ = writeln!(s, "{}:release_on_message({});", army_var(i), lua_str(start));
            } else {
                let _ = writeln!(s, "{}:attack_on_message({});", army_var(i), lua_str(start));
            }
        }
        return s;
    }
    let mut messages = std::collections::BTreeMap::new();
    for n in g.nodes.iter().filter(|n| n.kind.starts_with("event.")) {
        match event_message(project, n, warnings) {
            Some((message, lua)) => {
                s += &lua;
                messages.insert(n.id, message);
            }
            None => warnings.push(format!("logic node {} ({}): not compiled", n.id, n.kind)),
        }
    }
    for l in &g.links {
        let (Some(message), Some(to)) = (messages.get(&l.from), g.nodes.iter().find(|n| n.id == l.to)) else {
            warnings.push(format!("logic link {} -> {}: not from an event to an action", l.from, l.to));
            continue;
        };
        match action(project, to, message, warnings) {
            Some(lua) => s += &lua,
            None => warnings.push(format!("logic node {} ({}): not compiled", to.id, to.kind)),
        }
    }
    s
}
