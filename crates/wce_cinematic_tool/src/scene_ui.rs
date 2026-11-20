//! The scene editor's panels (docs/plans/cinematic.md, steps 3 and 4): Scene (the battle's setup, armies, units, texts), Timeline
//! (clips, transport, tracks and their keys), Key (the selected key's values) and Layout (the field from above: units, paths and the
//! camera at the playhead; Ctrl+click adds a path key for the selected unit). Every change is an undoable edit of the document.

use std::collections::BTreeMap;

use wce_cinematic::eval::{SceneFrame, SpeedTable, evaluate, evaluate_battle};
use wce_cinematic::scene::*;
use wce_cinematic::support::track_support;
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};
use wce_editor::icons;

use crate::scene_doc::SceneDocument;

/// The game's languages (`subtitle_timings.language`).
pub const LANGUAGES: &[&str] = &["en", "cz", "de", "es", "fr", "it", "pl", "ru", "tr"];

pub fn scene_doc(e: &mut Editor) -> Option<&mut SceneDocument> {
    let id = e.docs.of_tool("cinematic")?;
    e.docs.find_as_mut::<SceneDocument>(id)
}

/// Which key is selected: the track, the list in it (a unit track has four), the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRef {
    pub track: u32,
    pub list: &'static str,
    pub index: usize,
}

#[derive(Default)]
pub struct SceneUi {
    pub clip: Option<u32>,
    pub time: f64,
    pub playing: bool,
    pub track: Option<u32>,
    pub key: Option<KeyRef>,
    pub unit: Option<u32>,
    new_unit: String,
    new_text: String,
    /// Layout view: centre (x, z) and metres per pixel.
    view: Option<([f32; 2], f32)>,
    /// A key being dragged on the timeline: its time when the drag started.
    dragging: Option<(KeyRef, f64)>,
    pub speeds: SpeedTable,
    /// The viewport's camera is the user's, not the keys'.
    pub free_camera: bool,
    /// The scene as a battle would play it (units under orders, movie-only tracks gone).
    pub battle_preview: bool,
    /// The language texts are edited and exported in (`en`, `de`...); English stands in where a text has none.
    pub language: String,
    last_tick: Option<std::time::Instant>,
}

/// Applies `f` to a copy of the project and pushes it as an edit called `name`.
pub fn edit(e: &mut Editor, name: &str, f: impl FnOnce(&mut Project)) {
    let Some(doc) = scene_doc(e) else { return };
    let mut p = doc.project().clone();
    f(&mut p);
    doc.push(name, p);
}

impl SceneUi {
    /// Back to a fresh state for another scene, keeping what belongs to the game rather than the scene (the units' speeds, read
    /// once a mount).
    /// The language being edited (English until one is picked).
    pub fn lang(&self) -> &str {
        if self.language.is_empty() { "en" } else { &self.language }
    }

    pub fn reset(&mut self) {
        let speeds = std::mem::take(&mut self.speeds);
        *self = Self { speeds, ..Self::default() };
    }

    pub fn current_clip<'a>(&mut self, p: &'a Project) -> Option<&'a Clip> {
        if self.clip.and_then(|c| p.clip(c)).is_none() {
            self.clip = p.clips.first().map(|c| c.id);
        }
        self.clip.and_then(|c| p.clip(c))
    }

    pub fn frame(&mut self, p: &Project) -> Option<SceneFrame> {
        let clip = self.current_clip(p)?.clone();
        Some(if self.battle_preview {
            evaluate_battle(p, &clip, &self.speeds, self.time)
        } else {
            evaluate(p, &clip, &self.speeds, self.time)
        })
    }

    /// The playhead: advances while playing (the fixed step in scripted runs, else the wall clock), stops at the clip's end.
    pub fn tick(&mut self, e: &Editor, length: f64) {
        let now = std::time::Instant::now();
        let dt = if e.clock.fixed_step() { e.clock.step() } else { self.last_tick.map_or(0.0, |t| now.duration_since(t).as_secs_f64()) };
        self.last_tick = Some(now);
        if self.playing {
            self.time += dt;
            if self.time >= length {
                self.time = length;
                self.playing = false;
            }
        }
    }

    // ---- Scene panel ----

    pub fn scene_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(340.0, 600.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Scene", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            gui::text_disabled("File > New cinematic scene");
            gui::end();
            return;
        };
        if gui::collapsing_header("Battle", gui::TREE_DEFAULT_OPEN) {
            let mut b = p.battle.clone();
            let mut m = p.map.clone();
            let mut changed = false;
            changed |= text_row("Key", &mut b.key, "The battles row's key and the script folder");
            changed |= text_row("Campaign map", &mut m.campaign_map, "The campaign map whose battle tiles the battle uses");
            let mut pos = m.position;
            gui::row("Map position");
            if gui::drag_float2("##mappos", &mut pos, 0.0005, "%.4f") {
                m.position = [pos[0].clamp(0.0, 1.0), pos[1].clamp(0.0, 1.0)];
                changed = true;
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("tile_map_position: 0 to 1 across the campaign map");
            }
            changed |= float_row("Playable area", &mut m.playable_area, 10.0, "Metres");
            let mut year = b.year;
            gui::row("Year");
            if gui::input_int("##year", &mut year) {
                b.year = year;
                changed = true;
            }
            changed |= text_row("Season", &mut b.season, "Summer, Winter...");
            changed |= text_row("Time of day", &mut b.time_of_day, "day, night...");
            changed |= text_row("Precipitation", &mut b.precipitation, "dry, rain, snow...");
            changed |= text_row("Environment", &mut b.environment, "The weather file the battle starts with");
            let mut menu = [b.menu_position[0], b.menu_position[1]];
            gui::row("Menu position");
            if gui::drag_int_range("##menux", &mut menu[0], 1.0, 0, 4096, "x %d") {
                b.menu_position[0] = menu[0];
                changed = true;
            }
            gui::same_line();
            if gui::drag_int_range("##menuy", &mut menu[1], 1.0, 0, 4096, "y %d") {
                b.menu_position[1] = menu[1];
                changed = true;
            }
            if changed {
                edit(e, "battle setup", |p| {
                    p.battle = b;
                    p.map = m;
                });
            }
        }
        if gui::collapsing_header("Armies", gui::TREE_DEFAULT_OPEN) {
            for (i, a) in p.armies.iter().enumerate() {
                gui::push_id_int(i as i32);
                let mut faction = a.faction.clone();
                let mut alliance = a.alliance as i32;
                gui::set_next_item_width(200.0);
                let f = gui::input_text("##faction", Some("faction key"), &mut faction, 128, gui::INPUT_ENTER_RETURNS_TRUE);
                gui::same_line();
                gui::set_next_item_width(60.0);
                let al = gui::drag_int_range("##alliance", &mut alliance, 0.05, 0, 7, "side %d");
                gui::same_line();
                let player = gui::radio_button("Player", a.player);
                if f || al || player {
                    edit(e, "army", |p| {
                        p.armies[i].faction = faction;
                        p.armies[i].alliance = alliance.max(0) as u32;
                        if player {
                            for (k, x) in p.armies.iter_mut().enumerate() {
                                x.player = k == i;
                            }
                        }
                    });
                }
                gui::pop_id();
            }
            if gui::small_button("Add army") {
                edit(e, "add army", |p| {
                    p.armies.push(Army {
                        alliance: 1,
                        faction: "att_fact_huns".into(),
                        deployment_centre: [0.0, 300.0],
                        deployment_size: [600.0, 300.0],
                        deployment_orientation: std::f32::consts::PI,
                        player: false,
                    })
                });
            }
        }
        if gui::collapsing_header("Units", gui::TREE_DEFAULT_OPEN) {
            gui::set_next_item_width(-80.0);
            gui::input_text("##newunit", Some("main_units key"), &mut self.new_unit, 128, 0);
            gui::same_line();
            if gui::button("Add") && !self.new_unit.trim().is_empty() {
                let key = self.new_unit.trim().to_owned();
                let army = self.unit.and_then(|u| p.unit(u)).map_or(0, |u| u.army);
                let mut new_id = 0;
                edit(e, "add unit", |p| {
                    let id = p.new_id();
                    let a = &p.armies[army.min(p.armies.len().saturating_sub(1))];
                    let n = p.units.iter().filter(|u| u.army == army).count() as f32;
                    p.units.push(SceneUnit {
                        id,
                        name: key.clone(),
                        unit: key.clone(),
                        army,
                        start: [a.deployment_centre[0] - 150.0 + 35.0 * (n % 9.0), a.deployment_centre[1] - 20.0 * (n / 9.0).floor()],
                        facing: a.deployment_orientation,
                        width: 30.0,
                        men: 160,
                        seed: id,
                        general: false,
                    });
                    new_id = id;
                });
                self.unit = Some(new_id);
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("Adds the unit to the selected unit's army (the first army when none)");
            }
            for u in &p.units {
                let label = format!("{} #{}  ({})##u{}", u.name, u.id, army_name(&p, u.army), u.id);
                if gui::selectable(&label, self.unit == Some(u.id), 0) {
                    self.unit = Some(u.id);
                }
            }
            if let Some(u) = self.unit.and_then(|id| p.unit(id)).cloned() {
                gui::separator_text("Selected unit");
                let mut v = u.clone();
                let mut changed = false;
                changed |= text_row("Name", &mut v.name, "The unit's name in the editor");
                changed |= text_row("Unit", &mut v.unit, "main_units key");
                let mut army = v.army as i32;
                gui::row("Army");
                if gui::drag_int_range("##army", &mut army, 0.05, 0, p.armies.len().saturating_sub(1) as i32, "%d") {
                    v.army = army.max(0) as usize;
                    changed = true;
                }
                gui::row("Start");
                changed |= gui::drag_float2("##start", &mut v.start, 0.5, "%.1f");
                let mut deg = v.facing.to_degrees();
                gui::row("Facing");
                if gui::drag_float("##facing", &mut deg, 1.0, -360.0, 360.0, "%.0f deg") {
                    v.facing = deg.to_radians();
                    changed = true;
                }
                changed |= float_row("Width", &mut v.width, 0.5, "Formation width, metres");
                let mut men = v.men as i32;
                gui::row("Men");
                if gui::drag_int_range("##men", &mut men, 1.0, 1, 1000, "%d") {
                    v.men = men.max(1) as u32;
                    changed = true;
                }
                gui::row("General");
                changed |= gui::checkbox("##general", &mut v.general);
                if changed {
                    edit(e, "unit", |p| {
                        if let Some(x) = p.units.iter_mut().find(|x| x.id == u.id) {
                            *x = v;
                        }
                    });
                }
                if gui::small_button("Delete unit") {
                    edit(e, "delete unit", |p| {
                        p.units.retain(|x| x.id != u.id);
                        for c in &mut p.clips {
                            c.tracks.retain(|t| !matches!(&t.content, TrackContent::Unit { unit, .. } if *unit == u.id));
                        }
                    });
                    self.unit = None;
                }
            }
        }
        if gui::collapsing_header("Texts", 0) {
            let lang = self.lang().to_owned();
            let mut current = LANGUAGES.iter().position(|l| *l == lang).unwrap_or(0) as i32;
            gui::set_next_item_width(70.0);
            if gui::combo("Language", &mut current, LANGUAGES) {
                self.language = LANGUAGES[current as usize].to_owned();
            }
            if gui::is_item_hovered() {
                gui::set_tooltip("The language edited here and in the Key panel, and the one the exports write");
            }
            let mut changes: Vec<(String, String)> = vec![];
            for (id, langs) in &p.texts {
                let mut en = langs.get(&lang).cloned().unwrap_or_default();
                gui::row(id);
                if gui::is_item_hovered()
                    && let Some(english) = langs.get("en")
                {
                    gui::set_tooltip(english);
                }
                gui::set_next_item_width(-1.0);
                if gui::input_text(&format!("##t_{id}"), None, &mut en, 512, gui::INPUT_ENTER_RETURNS_TRUE) {
                    changes.push((id.clone(), en));
                }
            }
            gui::set_next_item_width(-60.0);
            gui::input_text("##newtext", Some("text id"), &mut self.new_text, 64, 0);
            gui::same_line();
            if gui::button("Add##text") && !self.new_text.trim().is_empty() {
                changes.push((sanitize_key(&self.new_text), String::new()));
                self.new_text.clear();
            }
            if !changes.is_empty() {
                edit(e, "text", |p| {
                    for (id, en) in changes {
                        p.texts.entry(id).or_default().insert(lang.clone(), en);
                    }
                });
            }
        }
        gui::end();
    }

    // ---- Timeline panel ----

    pub fn timeline_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(900.0, 260.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Timeline", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            gui::text_disabled("No scene");
            gui::end();
            return;
        };
        let Some(clip) = self.current_clip(&p).cloned() else {
            if gui::button("Add clip") {
                edit(e, "add clip", |p| add_clip(p, "Clip"));
            }
            gui::end();
            return;
        };
        self.tick(e, clip.length);

        // Clip bar.
        let names: Vec<String> = p.clips.iter().map(|c| format!("{}##{}", c.name, c.id)).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut current = p.clips.iter().position(|c| c.id == clip.id).unwrap_or(0) as i32;
        gui::set_next_item_width(160.0);
        if gui::combo("##clip", &mut current, &refs) {
            self.clip = Some(p.clips[current as usize].id);
            self.time = 0.0;
            self.key = None;
        }
        gui::same_line();
        if gui::small_button("+##clip") {
            edit(e, "add clip", |p| add_clip(p, "Clip"));
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("New clip");
        }
        gui::same_line();
        gui::begin_disabled(p.clips.len() < 2);
        if gui::small_button("-##clip") {
            let id = clip.id;
            edit(e, "delete clip", |p| p.clips.retain(|c| c.id != id));
            self.clip = None;
            self.key = None;
            self.track = None;
        }
        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip("Delete this clip (a scene keeps one)");
        }
        gui::same_line();
        let mut name = clip.name.clone();
        gui::set_next_item_width(140.0);
        if gui::input_text("##clipname", Some("name"), &mut name, 64, gui::INPUT_ENTER_RETURNS_TRUE) {
            edit(e, "rename clip", |p| {
                if let Some(c) = p.clip_mut(clip.id) {
                    c.name = name;
                }
            });
        }
        gui::same_line();
        let mut length = clip.length as f32;
        gui::set_next_item_width(90.0);
        if gui::drag_float("##length", &mut length, 0.1, 0.5, 600.0, "%.1f s") {
            edit(e, "clip length", |p| {
                if let Some(c) = p.clip_mut(clip.id) {
                    c.length = length as f64;
                }
            });
        }
        gui::same_line();
        let mut blur = clip.motion_blur;
        if gui::checkbox("Blur", &mut blur) {
            edit(e, "motion blur", |p| {
                if let Some(c) = p.clip_mut(clip.id) {
                    c.motion_blur = blur;
                }
            });
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Motion blur in this clip's movie (4 sub-frames a frame, 180 degree shutter; not a game effect)");
        }
        gui::same_line();
        if gui::button(if self.playing { icons::PAUSE } else { icons::PLAY }) {
            if !self.playing && self.time >= clip.length {
                self.time = 0.0;
            }
            self.playing = !self.playing;
        }
        gui::same_line();
        let fps = clip.fps.max(1) as f64;
        gui::text(&format!("{:.2} s  frame {}", self.time, (self.time * fps).round() as i64));
        gui::same_line();
        gui::set_next_item_width(120.0);
        if gui::begin_combo("##addtrack", "Add track") {
            for kind in ["Camera", "Unit", "Sound", "Music", "Subtitles", "Fade", "Letterbox", "Depth of field", "Weather", "VFX"] {
                if gui::selectable(kind, false, 0) {
                    let unit = self.unit;
                    let env = p.battle.environment.clone();
                    let mut made = None;
                    edit(e, "add track", |p| made = add_track(p, clip.id, kind, unit, &env));
                    if made.is_none() && kind == "Unit" {
                        e.status("Cinematic: select a unit first (Scene > Units)", true);
                    }
                    self.track = made;
                }
            }
            gui::end_combo();
        }
        gui::same_line();
        gui::checkbox("Battle", &mut self.battle_preview);
        if gui::is_item_hovered() {
            gui::set_tooltip("Play it as the battle would: units under orders at their own speeds, movie-only tracks gone");
        }
        gui::same_line();
        gui::checkbox("Free camera", &mut self.free_camera);
        if gui::is_item_hovered() {
            gui::set_tooltip("Fly the viewport yourself; Add key on a camera track keys this view");
        }
        gui::same_line();
        if gui::button("Add key") {
            self.add_key(e, &p, &clip);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A key at the playhead on the selected track, holding the value there");
        }

        // Tracks and keys.
        let avail = gui::content_region_avail();
        let names_w = 190.0f32;
        let row_h = gui::frame_height() + 2.0;
        let origin = gui::cursor_screen_pos();
        let area_x = origin.x + names_w;
        let area_w = (avail.x - names_w - 8.0).max(50.0);
        let to_x = |t: f64| area_x + (t / clip.length.max(0.001)) as f32 * area_w;
        let from_x = |x: f32| (((x - area_x) / area_w) as f64 * clip.length).clamp(0.0, clip.length);
        let draw = gui::window_draw_list();
        let ruler_h = 18.0;
        // Ruler: a second's tick, a label every 5 s; a click moves the playhead.
        draw.rect_filled(v2(area_x, origin.y), v2(area_x + area_w, origin.y + ruler_h), gui::col32(40, 40, 46, 255), 0.0);
        let mut s = 0.0;
        while s <= clip.length + 1e-6 {
            let x = to_x(s);
            let major = (s as i64) % 5 == 0;
            draw.line(v2(x, origin.y + if major { 2.0 } else { 10.0 }), v2(x, origin.y + ruler_h), gui::col32(150, 150, 160, 255), 1.0);
            if major {
                draw.text(v2(x + 2.0, origin.y + 1.0), gui::col32(180, 180, 190, 255), &format!("{s:.0}"));
            }
            s += 1.0;
        }
        gui::set_cursor_screen_pos(v2(area_x, origin.y));
        gui::invisible_button("##ruler", v2(area_w, ruler_h), 0);
        if gui::is_item_active() {
            self.time = (from_x(gui::mouse_pos().x) * fps).round() / fps;
            self.playing = false;
        }
        let mut y = origin.y + ruler_h + 2.0;
        let mut pending: Option<(String, Box<dyn FnOnce(&mut Project)>)> = None;
        for track in &clip.tracks {
            let selected = self.track == Some(track.id);
            gui::set_cursor_screen_pos(v2(origin.x, y));
            gui::push_id_int(track.id as i32);
            let (support, why) = track_support(&track.content);
            let label = format!("{} {}", if track.muted { icons::VOLUME_X } else { "" }, track.name);
            if gui::selectable_sized(&label, selected, 0, v2(names_w - 30.0, row_h - 2.0)) {
                self.track = Some(track.id);
                if let TrackContent::Unit { unit, .. } = &track.content {
                    self.unit = Some(*unit);
                }
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}: {} ({why})", track.content.kind_name(), support.label()));
            }
            if gui::begin_popup_context_item("##trackmenu") {
                if gui::menu_item(if track.muted { "Unmute" } else { "Mute" }, None, false, true) {
                    let id = track.id;
                    pending =
                        Some(("mute track".into(), Box::new(move |p: &mut Project| for_track(p, clip.id, id, |t| t.muted = !t.muted))));
                }
                if gui::menu_item("Delete track", None, false, true) {
                    let id = track.id;
                    pending = Some((
                        "delete track".into(),
                        Box::new(move |p: &mut Project| {
                            if let Some(c) = p.clip_mut(clip.id) {
                                c.tracks.retain(|t| t.id != id);
                            }
                        }),
                    ));
                }
                gui::end_popup();
            }
            // A dot by the name: the battle's support (green exact, amber approximate, grey movie only).
            let col = match support {
                wce_cinematic::support::Support::Exact => gui::col32(90, 200, 120, 255),
                wce_cinematic::support::Support::Approximate => gui::col32(230, 180, 60, 255),
                wce_cinematic::support::Support::MovieOnly => gui::col32(140, 140, 140, 255),
            };
            draw.circle_filled(v2(origin.x + names_w - 18.0, y + row_h * 0.5), 4.0, col);
            gui::pop_id();
            draw.rect_filled(
                v2(area_x, y),
                v2(area_x + area_w, y + row_h - 2.0),
                if selected { gui::col32(52, 58, 72, 255) } else { gui::col32(34, 34, 40, 255) },
                2.0,
            );
            for (list, i, t) in key_times(&track.content) {
                let k = KeyRef { track: track.id, list, index: i };
                let x = to_x(t);
                let cy = y + row_h * 0.5;
                let sel = self.key.as_ref() == Some(&k);
                let c = if sel { gui::col32(255, 210, 80, 255) } else { gui::col32(200, 200, 215, 255) };
                draw.triangle_filled(v2(x, cy - 6.0), v2(x + 6.0, cy), v2(x, cy + 6.0), c);
                draw.triangle_filled(v2(x, cy - 6.0), v2(x - 6.0, cy), v2(x, cy + 6.0), c);
            }
            // Clicks on the row: a key under the mouse is selected (and dragged), else the playhead moves.
            gui::set_cursor_screen_pos(v2(area_x, y));
            gui::invisible_button(&format!("##row{}", track.id), v2(area_w, row_h - 2.0), 0);
            if gui::is_item_activated() {
                let mx = gui::mouse_pos().x;
                let hit = key_times(&track.content).into_iter().min_by(|a, b| (to_x(a.2) - mx).abs().total_cmp(&(to_x(b.2) - mx).abs()));
                self.track = Some(track.id);
                match hit {
                    Some((list, i, t)) if (to_x(t) - mx).abs() <= 7.0 => {
                        let k = KeyRef { track: track.id, list, index: i };
                        self.key = Some(k.clone());
                        self.dragging = Some((k, t));
                        self.time = t;
                    }
                    _ => {
                        self.key = None;
                        self.time = (from_x(mx) * fps).round() / fps;
                    }
                }
                self.playing = false;
            }
            if gui::is_item_deactivated()
                && let Some((k, t0)) = self.dragging.take()
            {
                let t1 = (from_x(gui::mouse_pos().x) * fps).round() / fps;
                if (t1 - t0).abs() > 1e-9 {
                    pending = Some(("move key".into(), Box::new(move |p: &mut Project| set_key_time(p, clip.id, &k, t1))));
                    self.key = None;
                }
            }
            y += row_h;
        }
        // The playhead.
        let px = to_x(self.time);
        draw.line(v2(px, origin.y), v2(px, y), gui::col32(255, 90, 70, 255), 2.0);
        gui::set_cursor_screen_pos(v2(origin.x, y + 4.0));
        gui::dummy(v2(1.0, 1.0));
        if let Some((name, f)) = pending {
            edit(e, &name, f);
        }
        gui::end();
    }

    pub fn add_key_public(&mut self, e: &mut Editor, p: &Project, clip: &Clip) {
        self.add_key(e, p, clip);
    }

    fn add_key(&mut self, e: &mut Editor, p: &Project, clip: &Clip) {
        let Some(track_id) = self.track else {
            e.status("Cinematic: select a track first", true);
            return;
        };
        let t = self.time;
        let frame = evaluate(p, clip, &self.speeds, t);
        let unit_pos = |u: u32| frame.units.iter().find(|s| s.unit == u).map(|s| s.position);
        // A camera key holds the viewport's view (in metres, the FOV in degrees).
        let units = e.scene.game_units.max(1e-3);
        let vc = e.viewport.camera();
        let cam = Some(wce_cinematic::eval::CameraState {
            position: [vc.eye.x * units, vc.eye.y * units, vc.eye.z * units],
            target: [vc.target.x * units, vc.target.y * units, vc.target.z * units],
            fov: vc.fov_y.to_degrees(),
        })
        .or(frame.camera.clone());
        let env = p.battle.environment.clone();
        let next_text =
            (1..).map(|n| format!("{}_{}", sanitize_key(&clip.name), n)).find(|id| !p.texts.contains_key(id)).unwrap_or_default();
        let mut made_text = None;
        edit(e, "add key", |p| {
            for_track(p, clip.id, track_id, |tr| match &mut tr.content {
                TrackContent::Camera { keys } => {
                    let c = cam.clone().unwrap_or(wce_cinematic::eval::CameraState {
                        position: [0.0, 60.0, -200.0],
                        target: [0.0, 0.0, 0.0],
                        fov: 35.0,
                    });
                    keys.push(CameraKey { t, position: c.position, target: c.target, fov: c.fov, interp: Interp::Smooth });
                }
                TrackContent::Unit { unit, path, .. } => {
                    let pos = unit_pos(*unit).unwrap_or([0.0, 0.0]);
                    path.push(PathKey { t, position: pos, interp: Interp::Linear, gait: None });
                }
                TrackContent::Sound { events } => events.push(SoundEvent { t, event: String::new(), position: None, volume: 1.0 }),
                TrackContent::Music { cues } => cues.push(MusicCue { t, event: String::new(), stop: None, fade_in: 0.0, fade_out: 0.0 }),
                TrackContent::Subtitles { lines } => {
                    lines.push(SubtitleLine { t, end: t + 3.0, text: next_text.clone() });
                    made_text = Some(next_text.clone());
                }
                TrackContent::Fade { keys } => keys.push(ScalarKey { t, value: frame.fade, interp: Interp::Linear }),
                TrackContent::Letterbox { keys } => keys.push(ScalarKey { t, value: frame.letterbox, interp: Interp::Step }),
                TrackContent::DepthOfField { keys } => {
                    let d = frame.dof.clone().unwrap_or(wce_cinematic::eval::DofState {
                        focus_distance: 60.0,
                        focal_length: 35.0,
                        blur_size: 2.0,
                    });
                    keys.push(DofKey {
                        t,
                        focus_distance: d.focus_distance,
                        focal_length: d.focal_length,
                        blur_size: d.blur_size,
                        interp: Interp::Linear,
                    });
                }
                TrackContent::Weather { keys } => keys.push(WeatherKey { t, environment: env.clone(), interp: Interp::Smooth }),
                TrackContent::Vfx { instances } => instances.push(VfxInstance {
                    start: t,
                    end: t + 5.0,
                    vfx: String::new(),
                    position: [0.0, 0.0, 0.0],
                    rotation_y: 0.0,
                    scale: 1.0,
                }),
            });
            if let Some(id) = &made_text {
                p.texts.entry(id.clone()).or_insert_with(|| BTreeMap::from([("en".into(), String::new())]));
            }
        });
    }

    // ---- Key panel ----

    pub fn key_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(320.0, 300.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Key", None, 0) {
            gui::end();
            return;
        }
        let (Some(p), Some(k)) = (scene_doc(e).map(|d| d.project().clone()), self.key.clone()) else {
            gui::text_disabled("Select a key on the timeline");
            gui::end();
            return;
        };
        let Some(clip) = self.current_clip(&p).cloned() else {
            gui::end();
            return;
        };
        let Some(track) = clip.tracks.iter().find(|t| t.id == k.track).cloned() else {
            gui::end();
            return;
        };
        let mut content = track.content.clone();
        let mut changed = false;
        let mut delete = false;
        gui::text(&format!("{} / {} #{}", track.name, k.list, k.index));
        macro_rules! time_row {
            ($t:expr) => {{
                let mut t = $t as f32;
                gui::row("Time");
                if gui::drag_float("##t", &mut t, 0.01, 0.0, clip.length as f32, "%.2f s") {
                    $t = t as f64;
                    changed = true;
                }
            }};
        }
        match &mut content {
            TrackContent::Camera { keys } => {
                if let Some(c) = keys.get_mut(k.index) {
                    time_row!(c.t);
                    gui::row("Position");
                    changed |= gui::drag_float3("##pos", &mut c.position, 0.5, "%.1f");
                    gui::row("Target");
                    changed |= gui::drag_float3("##tgt", &mut c.target, 0.5, "%.1f");
                    changed |= float_row("FOV", &mut c.fov, 0.2, "Vertical field of view, degrees");
                    changed |= interp_row(&mut c.interp);
                }
            }
            TrackContent::Unit { path, actions, men, visible, .. } => match k.list {
                "path" => {
                    if let Some(c) = path.get_mut(k.index) {
                        time_row!(c.t);
                        gui::row("Position");
                        changed |= gui::drag_float2("##pos", &mut c.position, 0.5, "%.1f");
                        changed |= interp_row(&mut c.interp);
                        let mut g = match c.gait {
                            None => 0,
                            Some(Gait::Walk) => 1,
                            Some(Gait::Run) => 2,
                            Some(Gait::Charge) => 3,
                        };
                        gui::row("Gait");
                        if gui::combo("##gait", &mut g, &["From speed", "Walk", "Run", "Charge"]) {
                            c.gait = [None, Some(Gait::Walk), Some(Gait::Run), Some(Gait::Charge)][g as usize];
                            changed = true;
                        }
                    }
                }
                "actions" => {
                    if let Some(c) = actions.get_mut(k.index) {
                        time_row!(c.t);
                        changed |= text_row("Slot", &mut c.slot, "STAND, ATTACK_1, CELEBRATE, TAUNT, DIE... AUTO: back to the movement");
                    }
                }
                "men" => {
                    if let Some(c) = men.get_mut(k.index) {
                        time_row!(c.t);
                        let mut m = c.men as i32;
                        gui::row("Men");
                        if gui::drag_int_range("##men", &mut m, 1.0, 0, 1000, "%d") {
                            c.men = m.max(0) as u32;
                            changed = true;
                        }
                    }
                }
                _ => {
                    if let Some(c) = visible.get_mut(k.index) {
                        time_row!(c.t);
                        gui::row("Visible");
                        changed |= gui::checkbox("##vis", &mut c.visible);
                    }
                }
            },
            TrackContent::Sound { events } => {
                if let Some(c) = events.get_mut(k.index) {
                    time_row!(c.t);
                    changed |= text_row("Event", &mut c.event, "A Wwise event's name");
                    changed |= float_row("Volume", &mut c.volume, 0.01, "0 to 2");
                }
            }
            TrackContent::Music { cues } => {
                if let Some(c) = cues.get_mut(k.index) {
                    time_row!(c.t);
                    changed |= text_row("Event", &mut c.event, "A music event's name");
                    changed |= float_row("Fade in", &mut c.fade_in, 0.05, "Seconds");
                    changed |= float_row("Fade out", &mut c.fade_out, 0.05, "Seconds");
                    let mut stop = c.stop.unwrap_or(clip.length) as f32;
                    gui::row("Stop");
                    if gui::drag_float("##stop", &mut stop, 0.05, 0.0, clip.length as f32, "%.2f s") {
                        c.stop = Some(stop as f64);
                        changed = true;
                    }
                }
            }
            TrackContent::Subtitles { lines } => {
                if let Some(c) = lines.get_mut(k.index) {
                    time_row!(c.t);
                    let mut end = c.end as f32;
                    gui::row("End");
                    if gui::drag_float("##end", &mut end, 0.01, 0.0, clip.length as f32, "%.2f s") {
                        c.end = end as f64;
                        changed = true;
                    }
                    let lang = self.lang().to_owned();
                    let mut text = p.texts.get(&c.text).and_then(|t| t.get(&lang)).cloned().unwrap_or_default();
                    gui::row(&format!("Text ({lang})"));
                    gui::set_next_item_width(-1.0);
                    if gui::input_text("##text", None, &mut text, 512, gui::INPUT_ENTER_RETURNS_TRUE) {
                        let id = c.text.clone();
                        edit(e, "subtitle text", |p| {
                            p.texts.entry(id).or_default().insert(lang, text);
                        });
                    }
                }
            }
            TrackContent::Fade { keys } | TrackContent::Letterbox { keys } => {
                if let Some(c) = keys.get_mut(k.index) {
                    time_row!(c.t);
                    gui::row("Value");
                    changed |= gui::slider_float("##v", &mut c.value, 0.0, 1.0, "%.2f");
                    changed |= interp_row(&mut c.interp);
                }
            }
            TrackContent::DepthOfField { keys } => {
                if let Some(c) = keys.get_mut(k.index) {
                    time_row!(c.t);
                    changed |= float_row("Focus distance", &mut c.focus_distance, 0.5, "Metres");
                    changed |= float_row("Focal length", &mut c.focal_length, 0.5, "mm");
                    changed |= float_row("Blur", &mut c.blur_size, 0.05, "Blur size");
                    changed |= interp_row(&mut c.interp);
                }
            }
            TrackContent::Weather { keys } => {
                if let Some(c) = keys.get_mut(k.index) {
                    time_row!(c.t);
                    changed |= text_row("Environment", &mut c.environment, "A weather file");
                    changed |= interp_row(&mut c.interp);
                }
            }
            TrackContent::Vfx { instances } => {
                if let Some(c) = instances.get_mut(k.index) {
                    time_row!(c.start);
                    let mut end = c.end as f32;
                    gui::row("End");
                    if gui::drag_float("##end", &mut end, 0.01, 0.0, clip.length as f32, "%.2f s") {
                        c.end = end as f64;
                        changed = true;
                    }
                    changed |= text_row("VFX", &mut c.vfx, "A vfx definition's key");
                    gui::row("Position");
                    changed |= gui::drag_float3("##pos", &mut c.position, 0.5, "%.1f");
                    changed |= float_row("Scale", &mut c.scale, 0.01, "Size");
                }
            }
        }
        if gui::button("Delete key") {
            delete = true;
        }
        gui::end();
        if changed || delete {
            let track_id = k.track;
            let list = k.list;
            let index = k.index;
            edit(e, if delete { "delete key" } else { "key" }, |p| {
                for_track(p, clip.id, track_id, |t| {
                    t.content = content;
                    if delete {
                        remove_key(&mut t.content, list, index);
                    }
                })
            });
            if delete {
                self.key = None;
            }
        }
    }

    // ---- Layout panel (the field from above) ----

    pub fn layout_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(500.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Layout", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            gui::end();
            return;
        };
        let frame = self.frame(&p);
        let size = gui::content_region_avail();
        let origin = gui::cursor_screen_pos();
        let (centre, mpp) = *self.view.get_or_insert(([0.0, 0.0], 2.0));
        let to_s = |w: [f32; 2]| v2(origin.x + size.x * 0.5 + (w[0] - centre[0]) / mpp, origin.y + size.y * 0.5 - (w[1] - centre[1]) / mpp);
        let to_w = |s: gui::Vec2| [centre[0] + (s.x - origin.x - size.x * 0.5) * mpp, centre[1] - (s.y - origin.y - size.y * 0.5) * mpp];
        let draw = gui::window_draw_list();
        draw.push_clip_rect(origin, v2(origin.x + size.x, origin.y + size.y));
        draw.rect_filled(origin, v2(origin.x + size.x, origin.y + size.y), gui::col32(30, 36, 30, 255), 0.0);
        // A 50 m grid, the 100 m lines brighter.
        let half = [size.x * 0.5 * mpp, size.y * 0.5 * mpp];
        let mut gx = ((centre[0] - half[0]) / 50.0).floor() * 50.0;
        while gx <= centre[0] + half[0] {
            let a = to_s([gx, centre[1] - half[1]]);
            let b = to_s([gx, centre[1] + half[1]]);
            draw.line(a, b, if (gx as i64) % 100 == 0 { gui::col32(60, 70, 60, 255) } else { gui::col32(44, 52, 44, 255) }, 1.0);
            gx += 50.0;
        }
        let mut gz = ((centre[1] - half[1]) / 50.0).floor() * 50.0;
        while gz <= centre[1] + half[1] {
            let a = to_s([centre[0] - half[0], gz]);
            let b = to_s([centre[0] + half[0], gz]);
            draw.line(a, b, if (gz as i64) % 100 == 0 { gui::col32(60, 70, 60, 255) } else { gui::col32(44, 52, 44, 255) }, 1.0);
            gz += 50.0;
        }
        let army_col = |a: usize| match p.armies.get(a).map_or(0, |x| x.alliance) {
            0 => gui::col32(90, 140, 230, 255),
            1 => gui::col32(220, 90, 80, 255),
            _ => gui::col32(220, 200, 90, 255),
        };
        // Deployment areas.
        for (i, a) in p.armies.iter().enumerate() {
            let (c, s) = (a.deployment_centre, a.deployment_size);
            draw.rect(
                to_s([c[0] - s[0] * 0.5, c[1] + s[1] * 0.5]),
                to_s([c[0] + s[0] * 0.5, c[1] - s[1] * 0.5]),
                army_col(i) & 0x80FF_FFFF,
                0.0,
                1.0,
            );
        }
        // Paths of the clip's unit tracks.
        if let Some(clip) = self.current_clip(&p) {
            for t in &clip.tracks {
                if let TrackContent::Unit { path, unit, .. } = &t.content {
                    let pts: Vec<gui::Vec2> = path.iter().map(|k| to_s(k.position)).collect();
                    let col = p.unit(*unit).map_or(0xFFFF_FFFF, |u| army_col(u.army));
                    draw.polyline(&pts, col & 0xA0FF_FFFF, false, 1.5);
                    for q in &pts {
                        draw.circle_filled(*q, 3.0, col);
                    }
                }
            }
        }
        // Units where the playhead has them: a block as wide as the formation, a tick at the front.
        if let Some(f) = &frame {
            for s in &f.units {
                let Some(u) = p.unit(s.unit) else { continue };
                let (sin, cos) = s.facing.sin_cos();
                let (hw, hd) = (u.width * 0.5, 6.0);
                let corner = |x: f32, z: f32| to_s([s.position[0] + x * cos + z * sin, s.position[1] - x * sin + z * cos]);
                let pts = [corner(-hw, -hd), corner(hw, -hd), corner(hw, hd), corner(-hw, hd)];
                let col = army_col(u.army);
                draw.concave_poly_filled(&pts, if s.visible { col & 0xC0FF_FFFF } else { col & 0x40FF_FFFF });
                if self.unit == Some(u.id) {
                    draw.polyline(&pts, gui::col32(255, 230, 120, 255), true, 2.0);
                }
                draw.line(corner(0.0, hd), corner(0.0, hd + 8.0), gui::col32(255, 255, 255, 200), 1.5);
            }
            // The camera: where it is and where it looks.
            if let Some(c) = &f.camera {
                let a = to_s([c.position[0], c.position[2]]);
                let b = to_s([c.target[0], c.target[2]]);
                draw.line(a, b, gui::col32(255, 220, 120, 200), 1.0);
                draw.circle_filled(a, 5.0, gui::col32(255, 220, 120, 255));
            }
        }
        draw.pop_clip_rect();
        gui::invisible_button("##layout", size, gui::BUTTON_LEFT | gui::BUTTON_RIGHT);
        let hovered = gui::is_item_hovered();
        if hovered {
            let wheel = gui::io().MouseWheel;
            if wheel != 0.0 {
                let v = self.view.as_mut().expect("set above");
                v.1 = (v.1 * if wheel > 0.0 { 0.8 } else { 1.25 }).clamp(0.05, 50.0);
            }
            if gui::is_mouse_down(1) {
                let d = gui::io().MouseDelta;
                let v = self.view.as_mut().expect("set above");
                v.0[0] -= d.x * mpp;
                v.0[1] += d.y * mpp;
            }
            let w = to_w(gui::mouse_pos());
            gui::set_tooltip(&format!("x {:.1}  z {:.1}", w[0], w[1]));
        }
        if gui::is_item_clicked() {
            let w = to_w(gui::mouse_pos());
            let ctrl = gui::is_key_down(gui::MOD_CTRL);
            if ctrl && let (Some(u), Some(clip)) = (self.unit, self.clip) {
                let t = self.time;
                let mut track = None;
                edit(e, "path key", |p| {
                    let id = match p
                        .clip(clip)
                        .and_then(|c| c.tracks.iter().find(|tr| matches!(&tr.content, TrackContent::Unit { unit, .. } if *unit == u)))
                    {
                        Some(tr) => tr.id,
                        None => add_track(p, clip, "Unit", Some(u), "").unwrap_or(0),
                    };
                    for_track(p, clip, id, |tr| {
                        if let TrackContent::Unit { path, .. } = &mut tr.content {
                            path.retain(|k| (k.t - t).abs() > 1e-6);
                            path.push(PathKey { t, position: w, interp: Interp::Linear, gait: None });
                        }
                    });
                    track = Some(id);
                });
                self.track = track;
            } else if let Some(f) = &frame {
                // Select the unit under the click.
                self.unit = f
                    .units
                    .iter()
                    .filter_map(|s| p.unit(s.unit).map(|u| (u, ((s.position[0] - w[0]).powi(2) + (s.position[1] - w[1]).powi(2)).sqrt())))
                    .filter(|(u, d)| *d <= u.width * 0.6 + 4.0 * mpp)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(u, _)| u.id);
            }
        }
        gui::end();
    }
}

fn army_name(p: &Project, i: usize) -> String {
    p.armies.get(i).map_or("?".into(), |a| format!("side {} {}", a.alliance, a.faction))
}

fn text_row(label: &str, v: &mut String, tip: &str) -> bool {
    gui::row(label);
    gui::set_next_item_width(-1.0);
    let r = gui::input_text(&format!("##{label}"), None, v, 256, gui::INPUT_ENTER_RETURNS_TRUE);
    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }
    r
}

fn float_row(label: &str, v: &mut f32, speed: f32, tip: &str) -> bool {
    gui::row(label);
    gui::set_next_item_width(-1.0);
    let r = gui::drag_float(&format!("##{label}"), v, speed, -1.0e6, 1.0e6, "%.2f");
    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }
    r
}

fn interp_row(i: &mut Interp) -> bool {
    let mut k = match i {
        Interp::Step => 0,
        Interp::Linear => 1,
        Interp::Smooth => 2,
    };
    gui::row("To the next");
    let r = gui::combo("##interp", &mut k, &["Hold", "Linear", "Smooth"]);
    if r {
        *i = [Interp::Step, Interp::Linear, Interp::Smooth][k as usize];
    }
    r
}

pub fn add_clip(p: &mut Project, name: &str) {
    let id = p.new_id();
    let cam = p.new_id();
    p.clips.push(Clip {
        id,
        name: format!("{name} {}", p.clips.len() + 1),
        length: 10.0,
        fps: 30,
        tracks: vec![Track { id: cam, name: "Camera".into(), muted: false, content: TrackContent::Camera { keys: vec![] } }],
        skippable: true,
        motion_blur: false,
    });
}

/// Adds a track of a kind to a clip; a unit track needs a unit. Returns its id.
pub fn add_track(p: &mut Project, clip: u32, kind: &str, unit: Option<u32>, environment: &str) -> Option<u32> {
    let content = match kind {
        "Camera" => TrackContent::Camera { keys: vec![] },
        "Unit" => TrackContent::Unit { unit: unit?, path: vec![], actions: vec![], men: vec![], visible: vec![] },
        "Sound" => TrackContent::Sound { events: vec![] },
        "Music" => TrackContent::Music { cues: vec![] },
        "Subtitles" => TrackContent::Subtitles { lines: vec![] },
        "Fade" => TrackContent::Fade { keys: vec![] },
        "Letterbox" => TrackContent::Letterbox { keys: vec![] },
        "Depth of field" => TrackContent::DepthOfField { keys: vec![] },
        "Weather" => {
            TrackContent::Weather { keys: vec![WeatherKey { t: 0.0, environment: environment.to_owned(), interp: Interp::Smooth }] }
        }
        "VFX" => TrackContent::Vfx { instances: vec![] },
        _ => return None,
    };
    let name = match (&content, unit.and_then(|u| p.unit(u))) {
        (TrackContent::Unit { .. }, Some(u)) => u.name.clone(),
        _ => kind.to_owned(),
    };
    let id = p.new_id();
    p.clip_mut(clip)?.tracks.push(Track { id, name, muted: false, content });
    Some(id)
}

pub fn for_track(p: &mut Project, clip: u32, track: u32, f: impl FnOnce(&mut Track)) {
    if let Some(t) = p.clip_mut(clip).and_then(|c| c.tracks.iter_mut().find(|t| t.id == track)) {
        f(t);
    }
}

/// Every key of a track: (list, index, time).
pub fn key_times(c: &TrackContent) -> Vec<(&'static str, usize, f64)> {
    let list = |name: &'static str, ts: Vec<f64>| ts.into_iter().enumerate().map(move |(i, t)| (name, i, t));
    match c {
        TrackContent::Camera { keys } => list("keys", keys.iter().map(|k| k.t).collect()).collect(),
        TrackContent::Unit { path, actions, men, visible, .. } => list("path", path.iter().map(|k| k.t).collect())
            .chain(list("actions", actions.iter().map(|k| k.t).collect()))
            .chain(list("men", men.iter().map(|k| k.t).collect()))
            .chain(list("visible", visible.iter().map(|k| k.t).collect()))
            .collect(),
        TrackContent::Sound { events } => list("events", events.iter().map(|k| k.t).collect()).collect(),
        TrackContent::Music { cues } => list("cues", cues.iter().map(|k| k.t).collect()).collect(),
        TrackContent::Subtitles { lines } => list("lines", lines.iter().map(|k| k.t).collect()).collect(),
        TrackContent::Vfx { instances } => list("instances", instances.iter().map(|k| k.start).collect()).collect(),
        TrackContent::Fade { keys } | TrackContent::Letterbox { keys } => list("keys", keys.iter().map(|k| k.t).collect()).collect(),
        TrackContent::DepthOfField { keys } => list("keys", keys.iter().map(|k| k.t).collect()).collect(),
        TrackContent::Weather { keys } => list("keys", keys.iter().map(|k| k.t).collect()).collect(),
    }
}

fn set_key_time(p: &mut Project, clip: u32, k: &KeyRef, t: f64) {
    for_track(p, clip, k.track, |tr| match (&mut tr.content, k.list) {
        (TrackContent::Camera { keys }, _) => keys.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Unit { path, .. }, "path") => path.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Unit { actions, .. }, "actions") => actions.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Unit { men, .. }, "men") => men.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Unit { visible, .. }, _) => visible.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Sound { events }, _) => events.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Music { cues }, _) => cues.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Subtitles { lines }, _) => lines.get_mut(k.index).iter_mut().for_each(|x| {
            let len = x.end - x.t;
            x.t = t;
            x.end = t + len;
        }),
        (TrackContent::Vfx { instances }, _) => instances.get_mut(k.index).iter_mut().for_each(|x| {
            let len = x.end - x.start;
            x.start = t;
            x.end = t + len;
        }),
        (TrackContent::Fade { keys } | TrackContent::Letterbox { keys }, _) => keys.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::DepthOfField { keys }, _) => keys.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
        (TrackContent::Weather { keys }, _) => keys.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
    });
}

fn remove_key(c: &mut TrackContent, list: &str, i: usize) {
    fn rm<T>(v: &mut Vec<T>, i: usize) {
        if i < v.len() {
            v.remove(i);
        }
    }
    match (c, list) {
        (TrackContent::Camera { keys }, _) => rm(keys, i),
        (TrackContent::Unit { path, .. }, "path") => rm(path, i),
        (TrackContent::Unit { actions, .. }, "actions") => rm(actions, i),
        (TrackContent::Unit { men, .. }, "men") => rm(men, i),
        (TrackContent::Unit { visible, .. }, _) => rm(visible, i),
        (TrackContent::Sound { events }, _) => rm(events, i),
        (TrackContent::Music { cues }, _) => rm(cues, i),
        (TrackContent::Subtitles { lines }, _) => rm(lines, i),
        (TrackContent::Vfx { instances }, _) => rm(instances, i),
        (TrackContent::Fade { keys } | TrackContent::Letterbox { keys }, _) => rm(keys, i),
        (TrackContent::DepthOfField { keys }, _) => rm(keys, i),
        (TrackContent::Weather { keys }, _) => rm(keys, i),
    }
}
