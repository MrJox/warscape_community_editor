//! A project as a historical battle (docs/plans/cinematic.md, steps 6 and 8): the battle XML, the Lua (start, declarations,
//! cutscenes, the logic), the menu's DB rows (`battles`, `historical_battles_ui_locations`), the subtitles' rows
//! (`scripted_subtitles`) and the loc. Mirrors vanilla's historical battles (`script/<key>/...`, `lib_generated_battle`,
//! `lib_cutscenes`): each clip is a `cutscene` whose actions fire on the battle's 100 ms ticks, the camera baked into `move_to`
//! steps, units given teleports and orders, and never `wait_for_vo` (it would shift every later action).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::curves::{around, point, scalar};
use crate::logic;
use crate::scene::{Clip, Gait, Interp, Project, TrackContent, sanitize_key};
use crate::support::{Support, track_support};

/// Cutscene actions run on the battle's tick (`time_increment_ms` in `lib_header.lua`).
pub const TICK_MS: u64 = 100;
/// A smooth camera move is baked into steps this long (two ticks: the steps' straight pieces stay close to the editor's curve).
pub const CAMERA_STEP_MS: u64 = 200;
/// `cam:move_to`'s flag (the scripts' "blend"): `true` in every move of vanilla's cutscenes (Catalaunian Plains, Adrianople), their
/// instant cuts and long moves alike; `false` only in the hand-over to the battle camera at their end. With `false` on each of
/// our short steps the camera stopped and started at every step in game (and lagged the curve).
const MOVE_FLAG: &str = "true";

/// What the export wrote, and what it left out or changed.
#[derive(Clone, Debug, Default)]
pub struct BattleScripts {
    /// Pack path -> text.
    pub files: BTreeMap<String, String>,
    /// Subtitle keys (`scripted_subtitles`) -> text id in `Project::texts`.
    pub subtitles: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

pub fn script_folder(project: &Project) -> String {
    format!("script/{}", sanitize_key(&project.battle.key))
}

/// The prefix of the script files (vanilla: two letters; here the battle's key).
pub fn shortform(project: &Project) -> String {
    sanitize_key(&project.battle.key)
}

pub fn unit_script_name(id: u32) -> String {
    format!("wce_u{id}")
}

pub fn clip_function(clip: &Clip) -> String {
    format!("Play_Clip_{}", clip.id)
}

pub fn clip_ended_message(clip: &Clip) -> String {
    format!("clip_{}_ended", clip.id)
}

/// A subtitle text id's key in `scripted_subtitles` (and its loc: `scripted_subtitles_localised_text_<key>`).
pub fn subtitle_key(project: &Project, text: &str) -> String {
    format!("wce.{}.{}", sanitize_key(&project.battle.key), sanitize_key(text))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn lua_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn f(v: f32) -> String {
    format!("{v:.2}")
}

/// Battle XML bearing: radians as the XML has them; Lua orders take degrees (`d_to_r` in `lib_script_unit.lua`).
fn degrees(radians: f32) -> f32 {
    radians.to_degrees().rem_euclid(360.0)
}

fn ms(t: f64) -> u64 {
    // On the tick grid, rounded to the nearest tick.
    (((t * 1000.0) / TICK_MS as f64).round().max(0.0) as u64) * TICK_MS
}

/// The battle map's folder: a campaign map's key gives its battle tiles (`terrain/battles/<map>/`), a path is taken as it is.
pub fn battle_map_name(project: &Project) -> String {
    let m = project.map.campaign_map.trim().trim_end_matches('/');
    if m.contains('/') { format!("{m}/") } else { format!("terrain/battles/{m}/") }
}

/// The battle XML (UTF-8).
pub fn battle_xml(project: &Project) -> String {
    let b = &project.battle;
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- Warscape Community Editor: Cinematic -->\n<battle>\n");
    let alliances: std::collections::BTreeSet<u32> = project.armies.iter().map(|a| a.alliance).collect();
    for alliance in &alliances {
        let _ = writeln!(x, "\t<alliance id=\"{alliance}\">");
        for (ai, army) in project.armies.iter().enumerate().filter(|(_, a)| a.alliance == *alliance) {
            let _ = writeln!(x, "\t\t<army>\n\t\t\t<faction>{}</faction>", esc(&army.faction));
            let _ = writeln!(
                x,
                "\t\t\t<deployment_area>\n\t\t\t\t<centre x=\"{}\" y=\"{}\"/>\n\t\t\t\t<width metres=\"{}\"/>\n\t\t\t\t<height metres=\"{}\"/>\n\
                 \t\t\t\t<orientation radians=\"{}\"/>\n\t\t\t</deployment_area>",
                f(army.deployment_centre[0]),
                f(army.deployment_centre[1]),
                f(army.deployment_size[0]),
                f(army.deployment_size[1]),
                f(army.deployment_orientation)
            );
            for u in project.units.iter().filter(|u| u.army == ai) {
                let _ = writeln!(
                    x,
                    "\t\t\t<unit script_name=\"{}\">\n\t\t\t\t<unit_type type=\"{}\"/>\n\t\t\t\t<position x=\"{}\" y=\"{}\"/>\n\
                     \t\t\t\t<orientation radians=\"{}\"/>\n\t\t\t\t<width metres=\"{}\"/>\n\t\t\t\t<unit_experience level=\"0\"/>",
                    unit_script_name(u.id),
                    esc(&u.unit),
                    f(u.start[0]),
                    f(u.start[1]),
                    f(u.facing),
                    f(u.width)
                );
                if u.general {
                    x += "\t\t\t\t<general>\n\t\t\t\t\t<star_rating level=\"3\"/>\n\t\t\t\t</general>\n";
                }
                x += "\t\t\t</unit>\n";
            }
            x += "\t\t</army>\n";
        }
        x += "\t\t<victory_condition>\n\t\t\t<kill_or_rout_enemy></kill_or_rout_enemy>\n\t\t</victory_condition>\n";
        x += "\t\t<rout_position x=\"0.00\" y=\"0.00\"/>\n\t</alliance>\n";
    }
    let _ = write!(
        x,
        "\t<battle_description>\n\t\t<battle_script prepare_for_fade_in=\"true\">{}_start.lua</battle_script>\n\
         \t\t<time_of_day>{}</time_of_day>\n\t\t<season>{}</season>\n\t\t<precipitation_type>{}</precipitation_type>\n\
         \t\t<type>{}</type>\n\t\t<duration>{:.2}</duration>\n\t\t<timeout_winning_alliance_index>1</timeout_winning_alliance_index>\n\
         \t</battle_description>\n",
        shortform(project),
        esc(&b.time_of_day),
        esc(&b.season),
        esc(&b.precipitation),
        esc(&b.battle_type),
        b.duration_s
    );
    if !b.environment.is_empty() {
        let _ = write!(
            x,
            "\t<weather>\n\t\t<environment_key>{}</environment_key>\n\t\t<prevailing_wind x=\"{}\" y=\"{}\"/>\n\t</weather>\n",
            esc(&b.environment),
            f(b.wind[0]),
            f(b.wind[1])
        );
    }
    // One battle tile (its folder, as Launch in game names a tile) or a whole battle map (as vanilla's historical battles): the name
    // alone, no map position.
    let tile = project.map.tile.trim().trim_end_matches('/');
    let alone = if !tile.is_empty() {
        Some(tile.to_owned())
    } else if project.map.whole_map {
        Some(battle_map_name(project))
    } else {
        None
    };
    if let Some(name) = alone {
        let _ = write!(
            x,
            "\t<battle_map_definition>\n\t\t<name>{}</name>\n\t</battle_map_definition>\n\t<playable_area dimension=\"{}\"></playable_area>\n</battle>\n",
            esc(&name),
            project.map.playable_area.round()
        );
        return x;
    }
    let _ = write!(
        x,
        "\t<battle_map_definition>\n\t\t<name>{}</name>\n\t\t<tile_map_position x=\"{:.6}\" y=\"{:.6}\">/</tile_map_position>\n\
         \t</battle_map_definition>\n\t<playable_area dimension=\"{}\"></playable_area>\n</battle>\n",
        esc(&battle_map_name(project)),
        project.map.position[0],
        project.map.position[1],
        project.map.playable_area.round()
    );
    x
}

/// Armies' Lua names: `ga_<n>` for the generated armies, by the army's index in the project.
pub fn army_var(i: usize) -> String {
    format!("ga_{}", i + 1)
}

/// `gb:get_army(alliance, army)`: 1-based, the army's place among its alliance's.
fn army_numbers(project: &Project, i: usize) -> (usize, usize) {
    let alliances: Vec<u32> = {
        let mut a: Vec<u32> = project.armies.iter().map(|a| a.alliance).collect();
        a.sort_unstable();
        a.dedup();
        a
    };
    let al = project.armies[i].alliance;
    let alliance_no = alliances.iter().position(|a| *a == al).unwrap_or(0) + 1;
    let army_no = project.armies[..i].iter().filter(|a| a.alliance == al).count() + 1;
    (alliance_no, army_no)
}

/// Every Lua file and the subtitle keys. The intro clip is the first clip (none: the battle starts with a fade in).
pub fn battle_scripts(project: &Project) -> BattleScripts {
    battle_scripts_with(project, &crate::eval::SpeedTable::new())
}

/// [`battle_scripts`] with the units' speeds: a move runs when it is faster than its unit walks (as the preview's gait).
pub fn battle_scripts_with(project: &Project, speeds: &crate::eval::SpeedTable) -> BattleScripts {
    let mut out = BattleScripts::default();
    let folder = script_folder(project);
    let short = shortform(project);
    let player = project.armies.iter().position(|a| a.player).unwrap_or(0);

    // Declarations: the armies (generated), the units (script units by their XML script names).
    let mut d = String::from("-- Warscape Community Editor: Cinematic. Generated: edit the project, not this file.\n\n");
    d += "cam = bm:camera();\nsm = get_messager();\nAlliances = bm:alliances();\n\n";
    for (i, _) in project.armies.iter().enumerate() {
        let (al, ar) = army_numbers(project, i);
        let _ = writeln!(d, "{} = gb:get_army({al}, {ar});", army_var(i));
        let _ = writeln!(d, "Army_{} = Alliances:item({al}):armies():item({ar});", i + 1);
    }
    d += "\n";
    for u in &project.units {
        let _ = writeln!(d, "SU_{} = script_unit:new(Army_{}, {});", u.id, u.army + 1, lua_str(&unit_script_name(u.id)));
    }

    // Cutscenes: one function per clip.
    let mut c = String::from("-- Warscape Community Editor: Cinematic. Generated: edit the project, not this file.\n");
    for clip in &project.clips {
        c += &cutscene_lua(project, clip, player, speeds, &mut out);
    }

    // Start: as vanilla's (Battle_Script_Header, battle_manager, generated_battle), then the logic.
    let intro = project.clips.first();
    let mut s = String::from("-- Warscape Community Editor: Cinematic. Generated: edit the project, not this file.\n\n");
    s += "system.ClearRequiredFiles();\nrequire \"lua_scripts.Battle_Script_Header\";\n\n";
    s += "bm = battle_manager:new(empire_battle:new());\n";
    s += "battle_name, battle_shortform = get_folder_name_and_shortform();\n";
    s += "package.path = package.path .. \";data/Script/\" .. battle_name .. \"/?.lua\";\n\n";
    let _ = writeln!(s, "require ({});", lua_str(&format!("{short}_cutscenes")));
    let _ = writeln!(
        s,
        "\ngb = generated_battle:new(\n\ttrue,\t-- the screen starts black (prepare_for_fade_in)\n\ttrue,\t-- no deployment\n\t{},\n\tfalse\n);\n",
        intro.map_or("nil".to_owned(), |c| format!("function() {}() end", clip_function(c)))
    );
    let _ = writeln!(s, "require ({});\n", lua_str(&format!("{short}_declarations")));
    if intro.is_some() {
        s += "gb:fade_in_on_message(\"cutscene_ended\", 0.5);\n";
    }
    s += &logic::compile(project, &mut out.warnings);

    out.files.insert(format!("{folder}/{short}_declarations.lua"), d);
    out.files.insert(format!("{folder}/{short}_cutscenes.lua"), c);
    out.files.insert(format!("{folder}/{short}_start.lua"), s);
    out.files.insert(format!("{folder}/{short}_battle.xml"), battle_xml(project));
    out
}

fn cutscene_lua(project: &Project, clip: &Clip, player: usize, speeds: &crate::eval::SpeedTable, out: &mut BattleScripts) -> String {
    let name = clip_function(clip);
    let length = ms(clip.length);
    // Actions by time; each a line of Lua.
    let mut actions: Vec<(u64, String)> = vec![];
    // The battle starts black (`prepare_for_fade_in`): the clip fades in at once as vanilla's intros do (Catalaunian Plains:
    // `cam:fade(false, 0.5)` at 0), unless its fade track starts black. First of the tick-0 actions (the sort keeps the order).
    let starts_black = clip
        .tracks
        .iter()
        .filter(|t| !t.muted)
        .find_map(|t| match &t.content {
            TrackContent::Fade { keys } => keys.first().filter(|k| k.t <= 0.0).map(|k| k.value > 0.5),
            _ => None,
        })
        .unwrap_or(false);
    actions.push((0, if starts_black { "cam:fade(true, 0)".to_owned() } else { "cam:fade(false, 0.5)".to_owned() }));
    let mut skip: Vec<String> = vec![];
    let mut pre: Vec<String> = vec![];
    let mut sfx_count = 0;
    let mut bars = false;
    let mut skip_camera = None;

    for track in clip.tracks.iter().filter(|t| !t.muted) {
        let (support, why) = track_support(&track.content);
        if support == Support::MovieOnly {
            out.warnings.push(format!("{}: track {} left out ({why})", clip.name, track.name));
            continue;
        }
        match &track.content {
            TrackContent::Camera { keys } if !keys.is_empty() => {
                let times: Vec<f64> = keys.iter().map(|k| k.t).collect();
                let positions: Vec<[f32; 3]> = keys.iter().map(|k| k.position).collect();
                let targets: Vec<[f32; 3]> = keys.iter().map(|k| k.target).collect();
                let at = |t: f64| {
                    let (i, j, u) = around(&times, t).expect("keys");
                    let m = keys[i].interp;
                    (point(&positions, &times, i, j, u, m), point(&targets, &times, i, j, u, m), scalar(keys[i].fov, keys[j].fov, u, m))
                };
                // Snap to the first key at 0, then move from sample to sample.
                let (p, tg, fov) = at(0.0);
                actions.push((0, format!("cam:move_to({}, {}, 0, {MOVE_FLAG}, {})", v3(p), v3(tg), f(fov))));
                let mut stops: Vec<u64> = vec![];
                for (k, key) in keys.iter().enumerate() {
                    stops.push(ms(key.t));
                    if k + 1 < keys.len() && key.interp == Interp::Smooth {
                        let (a, b) = (ms(key.t), ms(keys[k + 1].t));
                        let mut s = a + CAMERA_STEP_MS;
                        while s < b {
                            stops.push(s);
                            s += CAMERA_STEP_MS;
                        }
                    }
                }
                stops.retain(|s| *s <= length);
                stops.sort_unstable();
                stops.dedup();
                for (n, w) in stops.windows(2).enumerate() {
                    let (from, mut to) = (w[0], w[1]);
                    let (i, _, _) = around(&times, from as f64 / 1000.0).expect("keys");
                    // A moving step aims a step further on (and takes as long again): the camera chases a point just ahead on the
                    // path and its speed never drops to a stop at a step's end (short straight moves met with a jolt every step).
                    // Not across a cut (a Step key next).
                    if keys[i].interp != Interp::Step
                        && let Some(&further) = stops.get(n + 2)
                    {
                        let (j, _, _) = around(&times, to as f64 / 1000.0).expect("keys");
                        if keys[j].interp != Interp::Step {
                            to = further;
                        }
                    }
                    let (p, tg, fov) = at(to as f64 / 1000.0);
                    let dur = if keys[i].interp == Interp::Step { 0.0 } else { (to - from) as f32 / 1000.0 };
                    let when = if keys[i].interp == Interp::Step { w[1] } else { from };
                    actions.push((when, format!("cam:move_to({}, {}, {}, {MOVE_FLAG}, {})", v3(p), v3(tg), f(dur), f(fov))));
                }
                let (p, tg, _) = at(clip.length);
                skip_camera = Some((p, tg));
            }
            TrackContent::Camera { .. } => {}
            TrackContent::Unit { unit, path, actions: slots, men, visible } => {
                let Some(su) = project.unit(*unit) else { continue };
                let su_var = format!("SU_{}", su.id);
                let width = f(su.width);
                if let Some(first) = path.first() {
                    let facing = path_facing(path, 0).unwrap_or(su.facing);
                    actions.push((
                        0,
                        format!(
                            "{su_var}.uc:teleport_to_location(v({}, {}), {}, {width})",
                            f(first.position[0]),
                            f(first.position[1]),
                            f(degrees(facing))
                        ),
                    ));
                }
                for k in 0..path.len().saturating_sub(1) {
                    let (a, b) = (&path[k], &path[k + 1]);
                    let facing = path_facing(path, k).unwrap_or(su.facing);
                    let dest = format!("v({}, {})", f(b.position[0]), f(b.position[1]));
                    if a.interp == Interp::Step || b.t <= a.t {
                        actions.push((ms(b.t), format!("{su_var}.uc:teleport_to_location({dest}, {}, {width})", f(degrees(facing)))));
                    } else {
                        let dist = ((b.position[0] - a.position[0]).powi(2) + (b.position[1] - a.position[1]).powi(2)).sqrt();
                        if dist < 0.01 {
                            continue;
                        }
                        let walk = speeds.get(&su.unit).copied().unwrap_or_default().walk;
                        let run = matches!(a.gait, Some(Gait::Run | Gait::Charge))
                            || (a.gait.is_none()
                                && crate::eval::slot_for_speed(
                                    dist / (b.t - a.t) as f32,
                                    crate::eval::Speeds { walk, ..Default::default() },
                                ) != "WALK");
                        actions.push((
                            ms(a.t),
                            format!("{su_var}.uc:goto_location_angle_width({dest}, {}, {width}, {run})", f(degrees(facing))),
                        ));
                    }
                }
                if let Some(last) = path.last() {
                    let facing = path_facing(path, path.len().saturating_sub(2)).unwrap_or(su.facing);
                    skip.push(format!(
                        "{su_var}.uc:teleport_to_location(v({}, {}), {}, {width});",
                        f(last.position[0]),
                        f(last.position[1]),
                        f(degrees(facing))
                    ));
                }
                for s in slots {
                    match s.slot.as_str() {
                        "TAUNT" => actions.push((ms(s.t), format!("{su_var}.uc:start_taunting()"))),
                        "AUTO" | "STAND" => {}
                        other => out.warnings.push(format!("{}: {} plays {other} in movies only", clip.name, su.name)),
                    }
                }
                let mut alive = su.men;
                for m in men {
                    if m.men < alive {
                        actions.push((ms(m.t), format!("{su_var}.unit:kill_number_of_men({})", alive - m.men)));
                        alive = m.men;
                    } else if m.men > alive {
                        out.warnings.push(format!("{}: {} can't gain men in a battle", clip.name, su.name));
                    }
                }
                for v in visible {
                    actions.push((ms(v.t), format!("{su_var}.uc:set_invisible_to_all({})", !v.visible)));
                }
            }
            TrackContent::Sound { events } => {
                for e in events {
                    sfx_count += 1;
                    let var = format!("SFX_{}_{sfx_count}", clip.id);
                    pre.push(format!("{var} = new_sfx({});", lua_str(&e.event)));
                    actions.push((ms(e.t), format!("cs:play_sound({var})")));
                    skip.push(format!("stop_sound({var});"));
                    if e.position.is_some() {
                        out.warnings.push(format!("{}: {} plays without its position in a battle", clip.name, e.event));
                    }
                }
            }
            TrackContent::Music { cues } => {
                for m in cues {
                    // By mood: the game's music system follows the states the mood events set (its own playback goes on).
                    if !m.moods.is_empty() {
                        for (k, mood) in m.moods.iter().enumerate() {
                            let var = format!("music_{}_{}_{k}", clip.id, (m.t * 1000.0).round() as u64);
                            pre.push(format!("{var} = new_sfx({});", lua_str(mood)));
                            actions.push((ms(m.t), format!("cs:play_sound({var})")));
                        }
                        continue;
                    }
                    actions.push((ms(m.t), "bm:set_music_auto_playback(false)".into()));
                    if m.fade_in > 0.0 {
                        actions.push((ms(m.t), format!("bm:play_music_custom_fade({}, {})", lua_str(&m.event), f(m.fade_in))));
                    } else {
                        actions.push((ms(m.t), format!("bm:play_music({})", lua_str(&m.event))));
                    }
                    let stop = m.stop.unwrap_or(clip.length);
                    if m.fade_out > 0.0 {
                        actions.push((ms((stop - m.fade_out as f64).max(m.t)), format!("bm:stop_music_custom_fade({})", f(m.fade_out))));
                    } else {
                        actions.push((ms(stop), "bm:stop_music()".into()));
                    }
                    actions.push((ms(stop), "bm:set_music_auto_playback(true)".into()));
                }
            }
            TrackContent::Subtitles { lines } => {
                for l in lines {
                    let key = subtitle_key(project, &l.text);
                    out.subtitles.insert(key.clone(), l.text.clone());
                    actions.push((ms(l.t), format!("subtitles:set_text({})", lua_str(&key))));
                    actions.push((ms(l.end), "subtitles:clear()".into()));
                }
            }
            TrackContent::Fade { keys } => {
                for w in keys.windows(2) {
                    let (a, b) = (&w[0], &w[1]);
                    if (b.value > 0.5) != (a.value > 0.5) {
                        let dur = if a.interp == Interp::Step { 0.0 } else { (b.t - a.t) as f32 };
                        let when = if a.interp == Interp::Step { ms(b.t) } else { ms(a.t) };
                        actions.push((when, format!("cam:fade({}, {})", b.value > 0.5, f(dur))));
                    }
                }
            }
            TrackContent::Letterbox { keys } => bars = keys.iter().any(|k| k.value > 0.5),
            TrackContent::DepthOfField { keys } => {
                for (k, key) in keys.iter().enumerate() {
                    let dur = keys.get(k + 1).filter(|_| key.interp != Interp::Step).map_or(0.0, |n| (n.t - key.t) as f32);
                    let target = keys.get(k + 1).filter(|_| key.interp != Interp::Step).unwrap_or(key);
                    actions.push((
                        ms(key.t),
                        format!(
                            "cam:change_depth_of_field({}, {}, {}, {})",
                            f(dur),
                            f(target.focus_distance),
                            f(target.focal_length),
                            f(target.blur_size)
                        ),
                    ));
                }
            }
            TrackContent::Vfx { .. } | TrackContent::Weather { .. } => {}
        }
    }
    actions.sort_by_key(|(t, _)| *t);

    let mut s = String::new();
    let _ = writeln!(s, "\n-- {}", clip.name.replace('\n', " "));
    let _ = writeln!(s, "function {name}()");
    for p in &pre {
        let _ = writeln!(s, "\t{p}");
    }
    let _ = writeln!(
        s,
        "\tcs = cutscene:new({}, {}:get_unitcontroller(), {length}, function() sm:trigger_message({}) end);",
        lua_str(&format!("wce_clip_{}", clip.id)),
        army_var(player),
        lua_str(&clip_ended_message(clip))
    );
    if clip.skippable {
        let _ = writeln!(s, "\tcs:set_skippable(true, function() Skip_{name}() end);");
        if let Some((p, tg)) = skip_camera {
            let _ = writeln!(s, "\tcs:set_skip_camera({}, {});", v3(p), v3(tg));
        }
    }
    if bars {
        s += "\tcs:set_show_cinematic_bars(true);\n";
    }
    s += "\tcam = cs:camera();\n\tsubtitles = cs:subtitles();\n\tsubtitles:set_alignment(\"bottom_centre\");\n\tsubtitles:clear();\n";
    for (t, line) in &actions {
        let _ = writeln!(s, "\tcs:action(function() {line} end, {t});");
    }
    s += "\tcs:start();\nend;\n";
    if clip.skippable {
        let _ = writeln!(s, "\nfunction Skip_{name}()");
        for line in &skip {
            let _ = writeln!(s, "\t{line}");
        }
        s += "\tsubtitles:clear();\nend;\n";
    }
    s
}

fn v3(p: [f32; 3]) -> String {
    format!("v({}, {}, {})", f(p[0]), f(p[1]), f(p[2]))
}

/// The direction from path key `k` to the next, radians (0 = +z).
fn path_facing(path: &[crate::scene::PathKey], k: usize) -> Option<f32> {
    let (a, b) = (path.get(k)?, path.get(k + 1)?);
    let (dx, dz) = (b.position[0] - a.position[0], b.position[1] - a.position[1]);
    if dx * dx + dz * dz < 1e-6 { None } else { Some(dx.atan2(dz)) }
}
