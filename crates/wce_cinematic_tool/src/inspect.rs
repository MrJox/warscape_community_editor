//! The Inspector of the scene, as the Tile tool's: the selected item's properties in the shared Inspector panel (its rows, hints and
//! drags; a drag is one undo entry), game data from pickers. While a unit or an effect is placed, what every copy takes (the Create
//! panel's draft); a key selected on the timeline, its values.

use std::hash::{Hash, Hasher};

use wce_cinematic::scene::{Interp, Project, TrackContent, sanitize_key};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::icons;
use wce_editor_core::inspector::Inspector;

use crate::create::{Create, Making};
use crate::items::{self, SceneItem, vfx_of_mut};
use crate::pickers::picker;
use crate::scene_ui::{LANGUAGES, SceneUi, add_track, for_track, scene_doc};

/// What the rows changed: the field last changed (a drag's edits merge by it), a picked value (an entry of its own), a drag ended.
struct Rows<'a> {
    ins: &'a mut dyn Inspector,
    dragged: Option<String>,
    picked: Option<String>,
    ended: bool,
}

impl Rows<'_> {
    fn note(&mut self, field: &str, changed: bool) {
        if changed {
            self.dragged = Some(field.to_owned());
        }
        if self.ins.gesture_ended() {
            self.ended = true;
        }
    }
}

fn merge_key(item: SceneItem, field: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (item.encode(), field).hash(&mut h);
    h.finish() | 1
}

/// Applies what the rows did to the document.
fn commit(e: &mut Editor, item: SceneItem, q: Project, r: &Rows) {
    let Some(doc) = scene_doc(e) else { return };
    if let Some(name) = &r.picked {
        doc.push(name, q);
    } else if let Some(field) = &r.dragged {
        doc.push_merged(field, q, merge_key(item, field));
    }
    if r.ended {
        doc.end_gesture();
    }
}

/// The Inspector's contents for the scene; false when there is no scene (the Inspector says so itself).
pub fn inspect(e: &mut Editor, ins: &mut dyn Inspector, ui: &mut SceneUi, create: &mut Create) -> bool {
    let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return false };
    if create.making.is_some() {
        draft(e, ins, ui, create, &p);
        return true;
    }
    if ui.key_focus && ui.key.is_some() {
        ui.key_body(e);
        return true;
    }
    let Some(item) = items::primary(e).filter(|i| i.exists(&p)) else {
        gui::text_disabled("Nothing selected");
        if gui::is_item_hovered() {
            gui::set_tooltip("Select in the Scene panel, the viewport or the Layout; the Create panel adds units and effects");
        }
        return true;
    };
    let n = items::selected(e).len();
    gui::text(&item.describe(&p, |k| ui.lists.faction_name(k)));
    if n > 1 {
        gui::same_line();
        gui::text_disabled(&format!("(and {} more: the gizmo moves them all)", n - 1));
    }
    gui::separator();
    let doc = e.docs.of_tool("cinematic").unwrap_or_default();
    ins.begin(doc, item.encode());
    let mut q = p.clone();
    let mut r = Rows { ins, dragged: None, picked: None, ended: false };
    match item {
        SceneItem::Unit(id) => unit(e, &mut r, ui, create, &mut q, id),
        SceneItem::Army(i) => army(e, &mut r, ui, create, &mut q, i),
        SceneItem::Battle => battle(&mut r, ui, &mut q),
        SceneItem::Vfx { track, index } => vfx(e, &mut r, ui, &mut q, track, index),
        SceneItem::CameraKey { track, index } | SceneItem::CameraTarget { track, index } => camera_key(e, &mut r, ui, &mut q, track, index),
        SceneItem::Camera(track) => camera(e, &mut r, ui, create, &mut q, track),
    }
    commit(e, item, q, &r);
    true
}

/// A data field: its label in the Inspector's column, then a picker.
fn pick_row(ui: &mut SceneUi, label: &str, id: &str, shown: &str, list: &[crate::pickers::Item], tip: &str) -> Option<String> {
    gui::row(label);
    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }
    let k = picker(&mut ui.filters, id, shown, list, -1.0);
    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }
    k
}

fn unit(e: &mut Editor, r: &mut Rows, ui: &mut SceneUi, create: &mut Create, q: &mut Project, id: u32) {
    let Some(ix) = q.units.iter().position(|u| u.id == id) else { return };
    let faction = q.armies.get(q.units[ix].army).map(|a| a.faction.clone()).unwrap_or_default();
    // The unit's card beside its kind's name.
    if let Some(card) = ui.lists.card(&q.units[ix].unit, &faction)
        && let Some(img) = create.card(e, &card)
    {
        let h = 72.0;
        gui::image(img.id, v2(h * img.w as f32 / img.h.max(1) as f32, h));
        gui::same_line();
    }
    gui::begin_group();
    gui::text(&ui.lists.unit_name(&q.units[ix].unit));
    gui::text_disabled(&q.units[ix].unit);
    if let Some(m) = ui.lists.men.get(&q.units[ix].unit) {
        gui::text_disabled(&format!("{m} men at the largest unit size"));
    }
    gui::end_group();

    let armies: Vec<String> =
        q.armies.iter().enumerate().map(|(i, a)| format!("{}: {}", i + 1, ui.lists.faction_name(&a.faction))).collect();
    let u = &mut q.units[ix];
    let c = r.ins.edit_string("name", "Name", &mut u.name);
    r.note("name", c);
    let lists = ui.lists.clone();
    if let Some(k) = pick_row(ui, "Unit", "unitkind", &lists.unit_name(&u.unit), &lists.units, "Which of the game's units it is") {
        u.unit = k;
        r.picked = Some("unit kind".into());
    }
    let mut a = u.army as i32;
    r.ins.hint("The army it fights in");
    if r.ins.edit_choice("army", "Army", &mut a, &armies) {
        u.army = a.max(0) as usize;
        r.picked = Some("unit army".into());
    }
    r.ins.hint("Where it stands when the battle starts, metres (move it with the gizmo or in the Layout)");
    let c = r.ins.edit_float2("start", "Start", &mut u.start, 0.5);
    r.note("start", c);
    let mut deg = u.facing.to_degrees();
    r.ins.hint("Where it faces at the start, degrees (0: north)");
    if r.ins.edit_slider("facing", "Facing", &mut deg, -180.0, 180.0) {
        u.facing = deg.to_radians();
        r.note("facing", true);
    } else {
        r.note("facing", false);
    }
    let mut men = i64::from(u.men);
    r.ins.hint("Men in a movie; a battle starts it with this many (kill_number_of_men)");
    let c = r.ins.edit_int_slider("men", "Men", &mut men, 1, 1000);
    u.men = men.clamp(1, 10_000) as u32;
    r.note("men", c);
    r.ins.hint("The formation's width, metres");
    let c = r.ins.edit_float("width", "Width", &mut u.width, 0.5, 2.0, 1000.0);
    r.note("width", c);
    let c = r.ins.edit_bool("general", "General", &mut u.general);
    r.note("general", c);

    // Who is drawn: its men and their command group, or one character in its place (a movie's; a battle has the unit).
    let group = ui.catalog.as_ref().and_then(|c| c.command_of(&u.unit)).map(|g| {
        g.men()
            .map(|k| {
                let kind = ui.catalog.as_ref().and_then(|c| c.personalities.get(k)).map_or("", |p| p.kind.as_str());
                (format!("{}{k}", wce_cinematic::scene::CHARACTER_PERSONALITY), crate::pickers::personality_kind(kind).to_owned())
            })
            .collect::<Vec<_>>()
    });
    if u.character.is_empty() && u.men > 1 {
        let tip = match &group {
            Some(men) => {
                format!("Its {} at their places in a movie", men.iter().map(|(_, k)| k.to_lowercase()).collect::<Vec<_>>().join(", "))
            }
            None => "Its officers at their places in a movie (this unit has none)".to_owned(),
        };
        r.ins.hint(&tip);
        let c = r.ins.edit_bool("command", "Command group", &mut u.command);
        r.note("command", c);
    }
    // The unit's own command group first, then everyone.
    let mut characters: Vec<crate::pickers::Item> = group
        .unwrap_or_default()
        .into_iter()
        .map(|(key, kind)| crate::pickers::Item { name: format!("{kind} of this unit"), key })
        .collect();
    characters.extend(ui.characters.iter().cloned());
    let shown = crate::pickers::character_name(ui.catalog.as_deref(), &u.character);
    if let Some(k) = pick_row(
        ui,
        "Character",
        "character",
        &shown,
        &characters,
        "One character alone in its place in a movie, on its mount; a battle has the unit",
    ) {
        u.character = k;
        r.picked = Some("unit character".into());
    }

    // Its part in the clip shown: keys at the playhead.
    let (clip, t) = (ui.clip, ui.time);
    if let Some(c) = clip.and_then(|c| q.clip(c)).cloned() {
        r.ins.section(&format!("In {} at {t:.2} s", c.name));
        let track = c.tracks.iter().find(|tr| matches!(&tr.content, TrackContent::Unit { unit, .. } if *unit == id)).map(|t| t.id);
        let keys = c
            .tracks
            .iter()
            .find_map(|tr| match &tr.content {
                TrackContent::Unit { unit, path, actions, .. } if *unit == id => Some((path.len(), actions.len())),
                _ => None,
            })
            .unwrap_or((0, 0));
        r.ins.text("keys", "Keys", &format!("{} on its path, {} animations", keys.0, keys.1));
        let at = scene_doc(e).and_then(|d| d.view.placed.get(&id).copied()).map_or(q.units[ix].start, |(p, _)| p);
        r.ins.hint("A path key here, where it stands now: then move it with the gizmo to key where it goes");
        if r.ins.button("keyhere", &format!("{}  Key its place here", icons::CROSSHAIR), true) {
            let tid = track.or_else(|| add_track(q, c.id, "Unit", Some(id), ""));
            if let Some(tid) = tid {
                for_track(q, c.id, tid, |tr| {
                    if let TrackContent::Unit { path, .. } = &mut tr.content {
                        path.retain(|k| (k.t - t).abs() > 1e-6);
                        path.push(wce_cinematic::scene::PathKey { t, position: at, interp: Interp::Linear, gait: None });
                    }
                });
            }
            r.picked = Some("path key".into());
        }
        if let Some(slot) = pick_row(
            ui,
            "Animate",
            "animhere",
            "Play an animation here...",
            &crate::pickers::slot_items(ui.slots.get(q.units[ix].slots_key()).map(|s| s.as_slice())),
            "The whole unit plays it from the playhead (AUTO: back to walking / standing); in a battle only a taunt plays",
        ) {
            let tid = track.or_else(|| add_track(q, c.id, "Unit", Some(id), ""));
            if let Some(tid) = tid {
                for_track(q, c.id, tid, |tr| {
                    if let TrackContent::Unit { actions, .. } = &mut tr.content {
                        actions.retain(|k| (k.t - t).abs() > 1e-6);
                        actions.push(wce_cinematic::scene::ActionKey { t, slot, place: None });
                    }
                });
            }
            r.picked = Some("animation key".into());
        }
    }
    r.ins.section("");
    if r.ins.button("dup", &format!("{}  Duplicate", icons::COPY), true) {
        ui.request_duplicate = true;
    }
    if r.ins.button("del", &format!("{}  Delete", icons::TRASH_2), true) {
        ui.request_delete = true;
    }
}

fn army(_e: &mut Editor, r: &mut Rows, ui: &mut SceneUi, create: &mut Create, q: &mut Project, i: usize) {
    let lists = ui.lists.clone();
    let units = q.units.iter().filter(|u| u.army == i).count();
    let Some(a) = q.armies.get_mut(i) else { return };
    if let Some(k) = pick_row(
        ui,
        "Faction",
        "armyfaction",
        &lists.faction_name(&a.faction),
        &lists.factions,
        "The faction it fights as (its units, colours and banners)",
    ) {
        a.faction = k;
        r.picked = Some("army faction".into());
    }
    let mut side = a.alliance.min(3) as i32;
    r.ins.hint("Armies of one side fight together");
    if r.ins.edit_choice("side", "Side", &mut side, &["Side 1".into(), "Side 2".into(), "Side 3".into(), "Side 4".into()]) {
        a.alliance = side.max(0) as u32;
        r.picked = Some("army side".into());
    }
    let mut player = a.player;
    r.ins.hint("The army the player commands (one)");
    if r.ins.edit_bool("player", "Player's", &mut player) && player {
        for (k, x) in q.armies.iter_mut().enumerate() {
            x.player = k == i;
        }
        r.picked = Some("player army".into());
    }
    let Some(a) = q.armies.get_mut(i) else { return };
    r.ins.section("Deployment zone");
    r.ins.hint("Its centre, metres (move it with the gizmo)");
    let c = r.ins.edit_float2("zone", "Centre", &mut a.deployment_centre, 1.0);
    r.note("zone", c);
    r.ins.hint("Its width and depth, metres");
    let c = r.ins.edit_float2("zonesize", "Size", &mut a.deployment_size, 1.0);
    r.note("zonesize", c);
    let mut deg = a.deployment_orientation.to_degrees();
    if r.ins.edit_slider("zonefacing", "Facing", &mut deg, -180.0, 180.0) {
        a.deployment_orientation = deg.to_radians();
        r.note("zonefacing", true);
    } else {
        r.note("zonefacing", false);
    }
    r.ins.section("");
    r.ins.text("units", "Units", &units.to_string());
    r.ins.hint("The Create panel's units, this army's faction's first: click one, then where it stands");
    if r.ins.button("placeunits", &format!("{}  Place units...", icons::PLUS), true) {
        create.draft.army = i;
        create.category = 0;
        gui::set_window_focus("Create");
    }
    r.ins.hint("Deletes the army and its units");
    if r.ins.button("delarmy", &format!("{}  Delete army", icons::TRASH_2), q.armies.len() > 1) {
        ui.request_delete = true;
    }
}

fn battle(r: &mut Rows, ui: &mut SceneUi, q: &mut Project) {
    let lists = ui.lists.clone();
    let lang = ui.lang().to_owned();
    r.ins.hint("The battle's key: its script folder and its menu entry");
    let mut key = q.battle.key.clone();
    if r.ins.edit_string("key", "Key", &mut key) {
        q.battle.key = sanitize_key(&key);
        r.note("key", true);
    } else {
        r.note("key", false);
    }
    for (field, label, id) in [("title", "Title", q.battle.title_text.clone()), ("desc", "Description", q.battle.description_text.clone())]
    {
        let mut v = q.texts.get(&id).and_then(|t| t.get(&lang)).cloned().unwrap_or_default();
        r.ins.hint(&format!("The Historical Battles menu's {} ({lang})", label.to_lowercase()));
        if r.ins.edit_string(field, label, &mut v) {
            q.texts.entry(id).or_default().insert(lang.clone(), v);
            r.note(field, true);
        } else {
            r.note(field, false);
        }
    }
    r.ins.section("Where");
    let kinds = ["A whole battle map".to_owned(), "A place on a campaign map".to_owned(), "One battle tile".to_owned()];
    let mut kind = if !q.map.tile.is_empty() {
        2
    } else if q.map.whole_map {
        0
    } else {
        1
    };
    r.ins.hint("A whole battle map (as vanilla's historical battles, made in TEd), a place on a campaign map, or one battle tile alone");
    if r.ins.edit_choice("where", "Fought on", &mut kind, &kinds) {
        q.map.whole_map = kind == 0;
        q.map.tile = if kind == 2 { lists.battle_tiles.first().map(|t| t.key.clone()).unwrap_or_default() } else { String::new() };
        r.picked = Some("battle ground".into());
    }
    if q.map.tile.is_empty() {
        let tip = if q.map.whole_map { "The battle map it is fought on, whole" } else { "The map whose battle tiles it is fought on" };
        if let Some(k) = pick_row(ui, "Battle map", "map", &q.map.campaign_map, &lists.battle_maps, tip) {
            q.map.campaign_map = k;
            r.picked = Some("battle map".into());
        }
        if !q.map.whole_map {
            r.ins.hint("Where on that map, 0 to 1 across and down");
            let c = r.ins.edit_float2("mappos", "Map position", &mut q.map.position, 0.0005);
            q.map.position = q.map.position.map(|v| v.clamp(0.0, 1.0));
            r.note("mappos", c);
            r.ins.hint("A picture of the map's heights: click where the battle is");
            if r.ins.button("mappick", &format!("{}  Pick on the map...", icons::MAP), true) {
                ui.map_pick.open(q.map.position);
            }
        }
    } else {
        let shown = q.map.tile.trim_start_matches("terrain/tiles/battle/").replace('/', " / ");
        if let Some(k) = pick_row(
            ui,
            "Battle tile",
            "tile",
            &shown,
            &lists.battle_tiles,
            "The game's battle tile it is fought on (type to search: plains, 8x8, river...)",
        ) {
            q.map.tile = k;
            r.picked = Some("battle tile".into());
        }
    }
    r.ins.hint("The battlefield's side, metres");
    let c = r.ins.edit_float("area", "Playable area", &mut q.map.playable_area, 10.0, 200.0, 10_000.0);
    r.note("area", c);
    r.ins.section("When");
    let shown = q.battle.environment.replace('\\', "/").to_lowercase();
    let shown = shown.rsplit('/').next().unwrap_or("").trim_end_matches(".environment").to_owned();
    if let Some(k) = pick_row(
        ui,
        "Weather",
        "env",
        if shown.is_empty() { "Pick a weather" } else { &shown },
        &lists.environments,
        "The sky, light and fog at the start (Weather keys change it in a clip)",
    ) {
        q.battle.environment = k;
        r.picked = Some("weather".into());
    }
    for (field, label, v, options) in [
        ("season", "Season", &mut q.battle.season, &["Spring", "Summer", "Autumn", "Winter"][..]),
        ("tod", "Time of day", &mut q.battle.time_of_day, &["day", "night"][..]),
        ("precip", "Rain / snow", &mut q.battle.precipitation, &["dry", "rain", "snow"][..]),
    ] {
        let choices: Vec<String> = options.iter().map(|s| (*s).to_owned()).collect();
        let mut k = options.iter().position(|o| o.eq_ignore_ascii_case(v)).map_or(0, |k| k as i32);
        if r.ins.edit_choice(field, label, &mut k, &choices) {
            *v = options[k.max(0) as usize].to_owned();
            r.picked = Some(label.to_lowercase());
        }
    }
    let mut year = i64::from(q.battle.year);
    let c = r.ins.edit_int("year", "Year", &mut year, 0.2);
    q.battle.year = year as i32;
    r.note("year", c);
    r.ins.section("Menu");
    let mut menu = [q.battle.menu_position[0] as f32, q.battle.menu_position[1] as f32];
    r.ins.hint("Where it sits on the Historical Battles map, pixels");
    let c = r.ins.edit_float2("menu", "Menu position", &mut menu, 1.0);
    q.battle.menu_position = [menu[0].clamp(0.0, 4096.0) as i32, menu[1].clamp(0.0, 4096.0) as i32];
    r.note("menu", c);
    if r.ins.open_section("Texts##texts") {
        let mut current = LANGUAGES.iter().position(|l| *l == lang).unwrap_or(0) as i32;
        let langs: Vec<String> = LANGUAGES.iter().map(|s| (*s).to_owned()).collect();
        r.ins.hint("The language edited here and on keys, and the one the exports write (English stands in)");
        if r.ins.edit_choice("lang", "Language", &mut current, &langs) {
            ui.language = LANGUAGES[current.max(0) as usize].to_owned();
        }
        let ids: Vec<String> = q.texts.keys().cloned().collect();
        for id in ids {
            let mut v = q.texts.get(&id).and_then(|t| t.get(&lang)).cloned().unwrap_or_default();
            if let Some(en) = q.texts.get(&id).and_then(|t| t.get("en")).filter(|_| lang != "en") {
                r.ins.hint(en);
            }
            if r.ins.edit_string(&format!("text.{id}"), &id, &mut v) {
                q.texts.entry(id.clone()).or_default().insert(lang.clone(), v);
                r.note(&format!("text.{id}"), true);
            } else {
                r.note(&format!("text.{id}"), false);
            }
        }
    }
}

fn vfx(_e: &mut Editor, r: &mut Rows, ui: &mut SceneUi, q: &mut Project, track: u32, index: usize) {
    let lists = ui.lists.clone();
    let Some(v) = vfx_of_mut(q, track, index) else { return };
    if let Some(k) = pick_row(
        ui,
        "Effect",
        "vfxkind",
        if v.vfx.is_empty() { "Pick an effect" } else { &v.vfx },
        &lists.vfx,
        "A visual effect of the game's vfx folder",
    ) {
        v.vfx = k;
        r.picked = Some("effect".into());
    }
    let (mut s, mut en) = (v.start as f32, v.end as f32);
    r.ins.hint("When it starts in the clip, seconds");
    let c = r.ins.edit_float("start", "Start", &mut s, 0.01, 0.0, 3600.0);
    r.note("start", c);
    r.ins.hint("When it stops, seconds");
    let c2 = r.ins.edit_float("end", "End", &mut en, 0.01, 0.0, 3600.0);
    r.note("end", c2);
    (v.start, v.end) = (f64::from(s), f64::from(en.max(s)));
    r.ins.hint("Where, metres (move it with the gizmo)");
    let c = r.ins.edit_float3("pos", "Position", &mut v.position, 0.5);
    r.note("pos", c);
    let mut deg = v.rotation_y.to_degrees();
    if r.ins.edit_slider("turn", "Turn", &mut deg, -180.0, 180.0) {
        v.rotation_y = deg.to_radians();
        r.note("turn", true);
    } else {
        r.note("turn", false);
    }
    let c = r.ins.edit_float("scale", "Scale", &mut v.scale, 0.01, 0.01, 100.0);
    r.note("scale", c);
    r.ins.section("");
    if r.ins.button("dup", &format!("{}  Duplicate", icons::COPY), true) {
        ui.request_duplicate = true;
    }
    if r.ins.button("del", &format!("{}  Delete", icons::TRASH_2), true) {
        ui.request_delete = true;
    }
}

/// Focal length (mm) of a vertical field of view (degrees), on a full-frame sensor (24 mm high), as Unreal's cine cameras give it.
pub fn focal_length(fov: f32) -> f32 {
    12.0 / (fov.to_radians() * 0.5).tan().max(1e-4)
}

pub fn fov_of_focal_length(mm: f32) -> f32 {
    (2.0 * (12.0 / mm.max(1.0)).atan()).to_degrees()
}

/// A camera: its name, its lens and where it is at the playhead (edits key it there), whether it is live, and what to do with it.
fn camera(_e: &mut Editor, r: &mut Rows, ui: &mut SceneUi, create: &mut Create, q: &mut Project, track: u32) {
    let t = ui.time;
    let Some(clip) = q.clips.iter().find(|c| c.tracks.iter().any(|tr| tr.id == track)).cloned() else { return };
    let live = wce_cinematic::eval::live_camera(&clip, t) == Some(track);
    let Some(tr) = q.clips.iter_mut().flat_map(|c| &mut c.tracks).find(|tr| tr.id == track) else { return };
    r.ins.hint("What the Scene panel and the Camera Cuts call it");
    let c = r.ins.edit_string("name", "Name", &mut tr.name);
    r.note("name", c);
    let TrackContent::Camera { keys } = &mut tr.content else { return };
    r.ins.text("live", "Shown", if live { "Live at the playhead" } else { "Not live at the playhead: cut to it to show it" });
    r.ins.hint("Moving or turning it with the gizmo keys it at the playhead");
    r.ins.text("keys", "Keys", &keys.len().to_string());
    let Some(now) = wce_cinematic::eval::camera_at(keys, t) else { return };
    // Edits at the playhead: into the key there, else a new key.
    let (mut pos, mut target, mut fov) = (now.position, now.target, now.fov);
    let mut mm = focal_length(fov);
    r.ins.section("At the playhead");
    r.ins.hint("Focal length, mm (a full-frame camera's: 24 mm wide angle, 50 mm normal, 135 mm telephoto)");
    let c_mm = r.ins.edit_float("mm", "Focal length", &mut mm, 0.5, 8.0, 600.0);
    r.note("mm", c_mm);
    if c_mm {
        fov = fov_of_focal_length(mm);
    }
    r.ins.hint("Vertical field of view, degrees (the battle's move_to takes it)");
    let c_fov = r.ins.edit_slider("fov", "Field of view", &mut fov, 2.0, 120.0);
    r.note("fov", c_fov);
    r.ins.hint("Where it is, metres (or move it with the gizmo)");
    let c_pos = r.ins.edit_float3("pos", "Position", &mut pos, 0.5);
    r.note("pos", c_pos);
    r.ins.hint("The point it looks at, metres (or turn it with the gizmo)");
    let c_tg = r.ins.edit_float3("target", "Aim", &mut target, 0.5);
    r.note("target", c_tg);
    if c_mm || c_fov || c_pos || c_tg {
        match keys.iter_mut().find(|k| (k.t - t).abs() < 1e-3) {
            Some(k) => {
                k.position = pos;
                k.target = target;
                k.fov = fov;
            }
            None => {
                keys.push(wce_cinematic::scene::CameraKey { t, position: pos, target, fov, interp: Interp::Smooth });
                keys.sort_by(|a, b| a.t.total_cmp(&b.t));
            }
        }
    }
    r.ins.section("");
    let piloting = ui.pilot == Some(track);
    let look = if piloting { format!("{}  Stop looking through it", icons::EYE_OFF) } else { format!("{}  Look through it", icons::EYE) };
    r.ins.hint("The viewport shows what this camera sees at the playhead (live or not)");
    if r.ins.button("look", &look, true) {
        ui.pilot = if piloting { None } else { Some(track) };
    }
    r.ins.hint("Keys the viewport's view into this camera at the playhead");
    if r.ins.button("keyview", &format!("{}  Key this view into it", icons::CAMERA), true) {
        create.requests.push(crate::create::CreateRequest::KeyView);
    }
    r.ins.hint("From the playhead on the clip shows this camera (a Camera Cuts key)");
    if r.ins.button("cut", &format!("{}  Cut to it here", icons::SCISSORS), !live) {
        ui.request_cut = Some(track);
    }
    if r.ins.button("del", &format!("{}  Delete camera", icons::TRASH_2), true) {
        ui.request_delete = true;
    }
}

fn camera_key(e: &mut Editor, r: &mut Rows, ui: &mut SceneUi, q: &mut Project, track: u32, index: usize) {
    let Some(k) = items::camera_key_of_mut(q, track, index) else { return };
    let mut t = k.t as f32;
    r.ins.hint("When the camera is here, seconds");
    let c = r.ins.edit_float("t", "Time", &mut t, 0.01, 0.0, 3600.0);
    k.t = f64::from(t);
    r.note("t", c);
    r.ins.hint("Where the camera is, metres (move it with the gizmo: its target goes along)");
    let c = r.ins.edit_float3("pos", "Position", &mut k.position, 0.5);
    r.note("pos", c);
    r.ins.hint("The point it looks at, metres");
    let c = r.ins.edit_float3("target", "Target", &mut k.target, 0.5);
    r.note("target", c);
    r.ins.hint("Vertical field of view, degrees");
    let c = r.ins.edit_slider("fov", "Field of view", &mut k.fov, 5.0, 120.0);
    r.note("fov", c);
    let names: Vec<String> = ["Step", "Linear", "Smooth"].iter().map(|s| (*s).to_owned()).collect();
    let mut i = match k.interp {
        Interp::Step => 0,
        Interp::Linear => 1,
        _ => 2,
    };
    r.ins.hint("How it goes to the next key");
    if r.ins.edit_choice("interp", "To the next", &mut i, &names) {
        k.interp = [Interp::Step, Interp::Linear, Interp::Smooth][i.clamp(0, 2) as usize];
        r.picked = Some("camera key interpolation".into());
    }
    let key = k.clone();
    // The way to the next key: its time and the speed it makes (both change the next keys' times).
    let keys: Vec<wce_cinematic::scene::CameraKey> = q
        .clips
        .iter()
        .flat_map(|c| &c.tracks)
        .find(|t| t.id == track)
        .and_then(|t| if let TrackContent::Camera { keys } = &t.content { Some(keys.clone()) } else { None })
        .unwrap_or_default();
    if let Some(next) = keys.get(index + 1) {
        r.ins.section("To the next key");
        let dist = (0..3).map(|i| (next.position[i] - key.position[i]).powi(2)).sum::<f32>().sqrt();
        let mut dur = (next.t - key.t) as f32;
        r.ins.hint("Seconds to the next key (the keys after it move with it)");
        let c1 = r.ins.edit_float("dur", "Duration", &mut dur, 0.01, 0.05, 600.0);
        r.note("dur", c1);
        let mut speed = if dur > 0.0 { dist / dur } else { 0.0 };
        r.ins.hint(&format!("Metres a second over the {dist:.0} m to the next key (sets the duration)"));
        let c2 = r.ins.edit_float("speed", "Speed", &mut speed, 0.1, 0.1, 1000.0);
        r.note("speed", c2);
        let new_dur = if c2 && speed > 0.0 { dist / speed } else { dur };
        if (c1 || c2) && (f64::from(new_dur) - (next.t - key.t)).abs() > 1e-4 {
            let shift = f64::from(new_dur.max(0.05)) - (next.t - key.t);
            for c in &mut q.clips {
                for t in &mut c.tracks {
                    if t.id == track
                        && let TrackContent::Camera { keys } = &mut t.content
                    {
                        for k in keys.iter_mut().skip(index + 1) {
                            k.t += shift;
                        }
                    }
                }
            }
        }
        r.ins.hint("A key halfway to the next, where the camera passes then");
        if r.ins.button("insert", &format!("{}  Insert a key after", icons::PLUS), true) {
            let mid = (key.t + next.t) * 0.5;
            if let Some(clip) = q.clips.iter().find(|c| c.tracks.iter().any(|t| t.id == track)).cloned() {
                let f = wce_cinematic::eval::evaluate(q, &clip, &wce_cinematic::eval::SpeedTable::default(), mid);
                if let Some(cam) = f.camera {
                    for t in q.clips.iter_mut().flat_map(|c| &mut c.tracks) {
                        if t.id == track
                            && let TrackContent::Camera { keys } = &mut t.content
                        {
                            keys.push(wce_cinematic::scene::CameraKey {
                                t: mid,
                                position: cam.position,
                                target: cam.target,
                                fov: cam.fov,
                                interp: key.interp,
                            });
                        }
                    }
                    r.picked = Some("insert camera key".into());
                }
            }
        }
        r.ins.hint("It looks towards the next key (as far as it looks now)");
        if r.ins.button("along", &format!("{}  Look along the path", icons::ROUTE), true) {
            let d: Vec<f32> = (0..3).map(|i| next.position[i] - key.position[i]).collect();
            let len = dist.max(1e-3);
            let reach = (0..3).map(|i| (key.target[i] - key.position[i]).powi(2)).sum::<f32>().sqrt().max(20.0);
            if let Some(k) = items::camera_key_of_mut(q, track, index) {
                k.target = [0, 1, 2].map(|i| key.position[i] + d[i] / len * reach);
            }
            r.picked = Some("camera look along".into());
        }
    }
    r.ins.section("");
    r.ins.hint("The viewport sees what this key sees (the free camera: fly from there and key again)");
    if r.ins.button("lookthrough", &format!("{}  Look through it", icons::CAMERA), true) {
        let units = e.scene.game_units.max(1e-3);
        let cam = crate::preview::camera_of(
            &wce_cinematic::eval::CameraState { position: key.position, target: key.target, fov: key.fov },
            units,
        );
        ui.look_through = false;
        ui.time = key.t;
        e.viewport.controller().set_camera(&cam);
        e.viewport.controller().lens_mut().fov_y = cam.fov_y;
    }
    r.ins.hint("This key takes the viewport's view");
    if r.ins.button("takeview", &format!("{}  Take the view", icons::CROSSHAIR), true) {
        let units = e.scene.game_units.max(1e-3);
        let vc = e.viewport.camera();
        if let Some(k) = items::camera_key_of_mut(q, track, index) {
            k.position = [vc.eye.x * units, vc.eye.y * units, vc.eye.z * units];
            k.target = [vc.target.x * units, vc.target.y * units, vc.target.z * units];
            k.fov = vc.fov_y.to_degrees();
        }
        r.picked = Some("camera key view".into());
    }
    if r.ins.button("del", &format!("{}  Delete", icons::TRASH_2), true) {
        ui.request_delete = true;
    }
}

/// While placing: what is placed and what every copy takes.
fn draft(e: &mut Editor, ins: &mut dyn Inspector, ui: &mut SceneUi, create: &mut Create, p: &Project) {
    let Some(m) = create.making.clone() else { return };
    let faction = p.armies.get(create.draft.army).map(|a| a.faction.clone()).unwrap_or_default();
    match &m {
        Making::Unit(k) => {
            if let Some(card) = ui.lists.card(k, &faction)
                && let Some(img) = create.card(e, &card)
            {
                gui::image(img.id, v2(48.0 * img.w as f32 / img.h.max(1) as f32, 48.0));
                gui::same_line();
            }
            gui::begin_group();
            gui::text(&format!("Placing {}", ui.lists.unit_name(k)));
            gui::text_disabled("click where it stands");
            gui::end_group();
        }
        Making::Vfx(k) => {
            gui::text(&format!("Placing {k}"));
            gui::text_disabled("click where it plays");
        }
        Making::CameraPath => {
            gui::text("Laying a camera path");
            gui::text_disabled("click its points on the ground");
        }
    }
    gui::separator();
    let d = &mut create.draft;
    match &m {
        Making::Unit(k) => {
            let armies: Vec<String> =
                p.armies.iter().enumerate().map(|(i, a)| format!("{}: {}", i + 1, ui.lists.faction_name(&a.faction))).collect();
            let mut a = d.army.min(p.armies.len().saturating_sub(1)) as i32;
            ins.hint("The army every unit placed joins");
            if ins.edit_choice("draft.army", "Army", &mut a, &armies) {
                d.army = a.max(0) as usize;
            }
            let own = ui.lists.men.get(k).copied().unwrap_or(160);
            let mut men = i64::from(d.men);
            ins.hint(&format!("Men in each copy (the unit's own: {own})"));
            ins.edit_int_slider("draft.men", "Men", &mut men, 1, 1000);
            d.men = men.clamp(1, 10_000) as u32;
            ins.edit_float("draft.width", "Width", &mut d.width, 0.5, 2.0, 1000.0);
            ins.hint("Degrees from the army's facing (R / Shift+R)");
            ins.edit_slider("draft.turn", "Turn", &mut d.turn, -180.0, 180.0);
            ins.edit_bool("draft.general", "General", &mut d.general);
            ins.hint("One in a row of the army's deployment zone, without a click");
            if ins.button("draft.zone", &format!("{}  Add one in its zone", icons::PLUS), true) {
                let army = d.army.min(p.armies.len().saturating_sub(1));
                let n = p.units.iter().filter(|u| u.army == army).count() as f32;
                if let Some(a) = p.armies.get(army) {
                    let at = [a.deployment_centre[0] - 150.0 + 35.0 * (n % 9.0), a.deployment_centre[1] - 20.0 * (n / 9.0).floor()];
                    let lists = ui.lists.clone();
                    let item = create.place(e, &lists, p, &m, at, ui.clip, ui.time);
                    create.requests.extend(item.map(crate::create::CreateRequest::Placed));
                }
            }
        }
        Making::CameraPath => {
            ins.hint("Metres above the ground at each point");
            ins.edit_float("draft.camh", "Height", &mut d.cam_height, 0.5, 1.0, 500.0);
            ins.hint("Seconds from one point to the next");
            ins.edit_float("draft.camstep", "Seconds apart", &mut d.cam_step, 0.05, 0.2, 60.0);
            ins.edit_slider("draft.camfov", "Field of view", &mut d.cam_fov, 5.0, 120.0);
        }
        Making::Vfx(_) => {
            ins.hint("How long each plays, seconds");
            ins.edit_float("draft.len", "Length", &mut d.vfx_length, 0.05, 0.1, 600.0);
            ins.edit_float("draft.scale", "Scale", &mut d.vfx_scale, 0.01, 0.01, 100.0);
            ins.edit_slider("draft.turn", "Turn", &mut d.turn, -180.0, 180.0);
        }
    }
    if ins.button("draft.stop", "Stop placing", true) {
        create.stop();
    }
}
