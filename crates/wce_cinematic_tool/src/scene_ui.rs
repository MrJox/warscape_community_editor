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

use crate::pickers::{Filters, GameLists, picker};
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
    /// Layout view: centre (x, z) and metres per pixel.
    view: Option<([f32; 2], f32)>,
    /// A key being dragged on the timeline: its time when the drag started.
    dragging: Option<(KeyRef, f64)>,
    pub speeds: SpeedTable,
    /// Paused, the viewport looks through the camera keys (playing it always does; else the camera is the user's, free).
    pub look_through: bool,
    /// The keyed camera free of the battle camera's limits (off: held within them, as the game does).
    pub no_game_limits: bool,
    /// Recording the camera: the clip plays and the viewport's camera (flown by the user) is keyed as it goes.
    pub recording: bool,
    /// The scene time of the last recorded key.
    pub recorded_at: f64,
    /// The scene as a battle would play it (units under orders, movie-only tracks gone).
    pub battle_preview: bool,
    /// The language texts are edited and exported in (`en`, `de`...); English stands in where a text has none.
    pub language: String,
    last_tick: Option<std::time::Instant>,
    /// The game's units, factions, weather files... for the pickers (kept across scenes).
    pub lists: std::sync::Arc<GameLists>,
    /// Each unit's animation slots (`main_units` key -> sorted), as the preview reads them: the action keys' choices.
    pub slots: std::collections::HashMap<String, std::sync::Arc<Vec<String>>>,
    /// The units' tables, once the preview has read them: the characters and command groups.
    pub catalog: Option<std::sync::Arc<wce_render::unit_catalog::UnitCatalog>>,
    /// The characters a unit can be, for its picker (built once the catalog is there).
    pub characters: std::sync::Arc<Vec<crate::pickers::Item>>,
    pub filters: Filters,
    /// The Inspector shows the selected key (it was clicked after the selection last changed).
    pub key_focus: bool,
    /// Units hidden in the editor's preview (the Scene panel's eyes; not saved, not in the exports).
    pub hidden: std::collections::HashSet<u32>,
    /// Asked for by a panel: the camera to look at a unit, the selection deleted or duplicated.
    pub look_at: Option<u32>,
    pub request_delete: bool,
    pub request_duplicate: bool,
    /// A cut to this camera at the playhead, asked for (the Inspector's Cut to it here).
    pub request_cut: Option<u32>,
    /// The camera the viewport looks through (the Inspector's Look through it), live or not.
    pub pilot: Option<u32>,
    pub outliner_filter: String,
    pub outliner_hovered: bool,
    /// The viewport's launch chip was clicked.
    pub launch_request: Option<bool>,
    /// A sound or some music to listen to (a pick's play button): the event, or the moods.
    pub audition: Option<(Option<String>, Vec<String>)>,
    /// The Layout's picture of the ground: its render target, the view it is of, when, and its image's number.
    layout_target: wce_render::render_target::RenderTarget,
    layout_pic_key: Option<([i32; 2], i32, u32, u32)>,
    layout_pic_at: Option<std::time::Instant>,
    layout_pic_n: u64,
    /// The picture shown and the one on its way to the GPU: its number and the view it is of (centre, metres a pixel, size).
    layout_shown: Option<(u64, ([f32; 2], f32, [f32; 2]))>,
    layout_pending: Option<(u64, ([f32; 2], f32, [f32; 2]))>,
    /// The view as it is and since when (a picture is rendered once it holds still).
    layout_view_key: Option<([i32; 2], i32, u32, u32)>,
    layout_view_since: Option<std::time::Instant>,
    /// The window picking the battle's place on its map.
    pub map_pick: crate::map_pick::MapPicker,
    /// A unit being dragged on the Layout: its start when the drag began.
    drag_unit: Option<(u32, [f32; 2])>,
    /// The scenes of the projects folder (for Open), and what the start buttons asked for.
    pub scenes: Vec<String>,
    pub request: Option<SceneRequest>,
    new_scene_name: String,
}

/// What the start buttons ask the tool for.
#[derive(Clone, Debug)]
pub enum SceneRequest {
    New(String),
    Open(String),
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
        let (speeds, lists, scenes) = (std::mem::take(&mut self.speeds), self.lists.clone(), std::mem::take(&mut self.scenes));
        *self = Self { speeds, lists, scenes, ..Self::default() };
    }

    /// No scene open: name a new one, or open one of the projects folder's.
    pub fn start_buttons(&mut self) {
        gui::set_next_item_width(-110.0);
        let enter = gui::input_text("##newscene", Some("new scene's name"), &mut self.new_scene_name, 64, gui::INPUT_ENTER_RETURNS_TRUE);
        gui::same_line();
        if (gui::button("New scene") || enter) && !self.new_scene_name.trim().is_empty() {
            self.request = Some(SceneRequest::New(self.new_scene_name.trim().to_owned()));
            self.new_scene_name.clear();
        }
        let items: Vec<crate::pickers::Item> =
            self.scenes.iter().map(|s| crate::pickers::Item { key: s.clone(), name: crate::tool::scene_label(s) }).collect();
        if let Some(k) =
            picker(&mut self.filters, "openscene", if items.is_empty() { "No saved scenes" } else { "Open a scene..." }, &items, -1.0)
        {
            self.request = Some(SceneRequest::Open(k));
        }
    }

    pub fn current_clip<'a>(&mut self, p: &'a Project) -> Option<&'a Clip> {
        if self.clip.and_then(|c| p.clip(c)).is_none() {
            self.clip = p.clips.first().map(|c| c.id);
        }
        self.clip.and_then(|c| p.clip(c))
    }

    pub fn frame(&mut self, p: &Project) -> Option<SceneFrame> {
        let clip = self.current_clip(p)?.clone();
        let mut f = if self.battle_preview {
            evaluate_battle(p, &clip, &self.speeds, self.time)
        } else {
            evaluate(p, &clip, &self.speeds, self.time)
        };
        // Looking through one camera (while not playing, which shows the cuts): its view, live or not.
        if let Some(cam) = self.pilot {
            match f.cameras.iter().find(|(c, _)| *c == cam) {
                Some((_, c)) if !self.playing => f.camera = Some(c.clone()),
                Some(_) => {}
                None => self.pilot = None,
            }
        }
        Some(f)
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

    // ---- Timeline panel ----

    pub fn timeline_panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(900.0, 260.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Timeline", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            self.start_buttons();
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
        if gui::button(if self.playing { icons::PAUSE } else { icons::PLAY }) {
            self.toggle_play(clip.length);
        }
        gui::same_line();
        let fps = clip.fps.max(1) as f64;
        gui::text(&format!("{:.2} s  frame {}", self.time, (self.time * fps).round() as i64));
        gui::same_line();
        gui::set_next_item_width(120.0);
        if gui::begin_combo("##addtrack", "Add track") {
            let mut add: Option<(&str, Option<u32>)> = None;
            let mut new_camera = false;
            if gui::selectable(&format!("{}  Camera (from this view)", icons::CAMERA), false, 0) {
                new_camera = true;
            }
            if gui::selectable(&format!("{}  Camera Cuts", icons::SCISSORS), false, 0) {
                add = Some(("Camera Cuts", None));
            }
            gui::separator();
            for kind in ["Sound", "Music", "Subtitles", "Fade", "Letterbox", "Depth of field", "Weather", "VFX"] {
                if gui::selectable(kind, false, 0) {
                    add = Some((kind, None));
                }
            }
            if new_camera {
                self.new_camera(e);
            }
            // A unit's track: the units without one in this clip, by name.
            let tracked: Vec<u32> = clip
                .tracks
                .iter()
                .filter_map(|t| if let TrackContent::Unit { unit, .. } = &t.content { Some(*unit) } else { None })
                .collect();
            let free: Vec<&SceneUnit> = p.units.iter().filter(|u| !tracked.contains(&u.id)).collect();
            if !free.is_empty() {
                gui::separator_text("A unit's moves");
            }
            for u in free {
                if gui::selectable(&format!("{}  {}##addunit{}", icons::USERS, u.name, u.id), false, 0) {
                    add = Some(("Unit", Some(u.id)));
                }
            }
            if p.units.is_empty() {
                gui::text_disabled("Units: place some first (Create)");
            }
            if let Some((kind, unit)) = add {
                let env = p.battle.environment.clone();
                let mut made = None;
                edit(e, "add track", |p| made = add_track(p, clip.id, kind, unit, &env));
                self.track = made;
                if unit.is_some() {
                    self.unit = unit;
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
        gui::checkbox("Look through", &mut self.look_through);
        gui::same_line();
        let mut limits = !self.no_game_limits;
        if gui::checkbox("Game limits", &mut limits) {
            self.no_game_limits = !limits;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The camera held 2 to 90 m over the ground, as the battle camera is (off: as keyed, as a movie shows it)");
        }
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
        gui::same_line();
        let mut key_view = false;
        if gui::button(&format!("{}  Key the camera", icons::CAMERA)) {
            key_view = true;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("A camera key at the playhead holding the viewport's view (fly there first)");
        }
        gui::same_line();
        let rec_label =
            if self.recording { format!("{}  Stop recording", icons::SQUARE) } else { format!("{}  Record", icons::CIRCLE_DOT_DASHED) };
        if gui::button(&rec_label) {
            self.toggle_recording();
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("The clip plays from the playhead; fly the viewport and your camera is keyed every half second");
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
        let mut key_here: Option<(u32, f64)> = None;
        for track in &clip.tracks {
            let selected = self.track == Some(track.id);
            gui::set_cursor_screen_pos(v2(origin.x, y));
            gui::push_id_int(track.id as i32);
            let (support, why) = track_support(&track.content);
            // On or off: an off track is left out of the preview, the movie and the battle (not deleted).
            let on = !track.muted;
            if !on {
                gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
            }
            if gui::small_button(if on { icons::EYE } else { icons::EYE_OFF }) {
                let id = track.id;
                pending = Some((
                    if on { "track off".into() } else { "track on".into() },
                    Box::new(move |p: &mut Project| for_track(p, clip.id, id, |t| t.muted = !t.muted)),
                ));
            }
            if !on {
                gui::pop_style_color(1);
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(if on { "On: click to leave it out (kept, not deleted)" } else { "Off: click to use it again" });
            }
            gui::same_line();
            let label = if on { track.name.clone() } else { format!("{}  (off)", track.name) };
            let button_w = gui::item_rect().1.x - gui::item_rect().0.x + gui::item_spacing().x;
            if !on {
                gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
            }
            let clicked = gui::selectable_sized(&label, selected, 0, v2(names_w - 30.0 - button_w, row_h - 2.0));
            if !on {
                gui::pop_style_color(1);
            }
            if clicked {
                self.track = Some(track.id);
                if let TrackContent::Unit { unit, .. } = &track.content {
                    self.unit = Some(*unit);
                }
            }
            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("{}: {} ({why})", track.content.kind_name(), support.label()));
            }
            if gui::begin_popup_context_item("##trackmenu") {
                if gui::menu_item(if track.muted { "Turn on" } else { "Turn off" }, None, false, true) {
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
            // The Camera Cuts: each shot a span in its camera's colour, named.
            if matches!(track.content, TrackContent::Cuts { .. }) {
                let cams = wce_cinematic::eval::clip_cameras(&clip);
                for (from, to, cam) in wce_cinematic::eval::shots(&clip) {
                    let k = cams.iter().position(|c| *c == cam).unwrap_or(0) as u32;
                    let hue = [(90, 170, 230), (230, 170, 80), (120, 200, 120), (200, 120, 200), (220, 110, 100), (150, 150, 220)];
                    let (r, g, b) = hue[k as usize % hue.len()];
                    let (x0, x1) = (to_x(from), to_x(to));
                    draw.rect_filled(v2(x0 + 1.0, y + 3.0), v2(x1 - 1.0, y + row_h - 5.0), gui::col32(r, g, b, 90), 3.0);
                    let name = clip.tracks.iter().find(|t| t.id == cam).map_or(String::new(), |t| t.name.clone());
                    if x1 - x0 > gui::calc_text_width(&name) + 16.0 {
                        draw.text(v2(x0 + 10.0, y + row_h * 0.5 - 8.0), gui::col32(235, 235, 240, 255), &name);
                    }
                }
            }
            // An animation key the unit has no such animation for: red (it stands there).
            let missing = |list: &str, i: usize| match &track.content {
                TrackContent::Unit { unit, actions, .. } if list == "actions" => {
                    let own = p.unit(*unit).and_then(|u| self.slots.get(u.slots_key()));
                    actions.get(i).is_some_and(|a| crate::pickers::slot_missing(&a.slot, own.map(|s| s.as_slice())))
                }
                _ => false,
            };
            for (list, i, t) in key_times(&track.content) {
                let k = KeyRef { track: track.id, list, index: i };
                let x = to_x(t);
                let cy = y + row_h * 0.5;
                let sel = self.key.as_ref() == Some(&k);
                let c = if sel {
                    gui::col32(255, 210, 80, 255)
                } else if missing(list, i) {
                    gui::col32(240, 90, 80, 255)
                } else {
                    gui::col32(200, 200, 215, 255)
                };
                draw.triangle_filled(v2(x, cy - 6.0), v2(x + 6.0, cy), v2(x, cy + 6.0), c);
                draw.triangle_filled(v2(x, cy - 6.0), v2(x - 6.0, cy), v2(x, cy + 6.0), c);
            }
            // Clicks on the row: a key under the mouse is selected (and dragged), else the playhead moves.
            gui::set_cursor_screen_pos(v2(area_x, y));
            gui::invisible_button(&format!("##row{}", track.id), v2(area_w, row_h - 2.0), 0);
            if gui::is_item_hovered() {
                gui::set_tooltip("Click: the playhead here  ·  double-click: a key here  ·  drag a key: move it");
                if gui::is_mouse_double_clicked(0) {
                    key_here = Some((track.id, (from_x(gui::mouse_pos().x) * fps).round() / fps));
                }
            }
            if gui::is_item_activated() {
                let mx = gui::mouse_pos().x;
                let hit = key_times(&track.content).into_iter().min_by(|a, b| (to_x(a.2) - mx).abs().total_cmp(&(to_x(b.2) - mx).abs()));
                self.track = Some(track.id);
                match hit {
                    Some((list, i, t)) if (to_x(t) - mx).abs() <= 7.0 => {
                        let k = KeyRef { track: track.id, list, index: i };
                        self.key = Some(k.clone());
                        self.key_focus = true;
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
        if key_view {
            self.look_through = false;
            self.key_camera_view(e);
        }
        if let Some((track, t)) = key_here {
            self.track = Some(track);
            self.time = t;
            self.dragging = None;
            if let Some(p) = scene_doc(e).map(|d| d.project().clone())
                && let Some(c) = p.clip(clip.id).cloned()
            {
                self.add_key(e, &p, &c);
            }
        }
        gui::end();
    }

    /// The viewport follows the camera keys: while the clip plays (not while recording), or when looking through them.
    pub fn follows_keys(&self) -> bool {
        (self.playing && !self.recording) || self.look_through || self.pilot.is_some()
    }

    /// Play / pause; at the clip's end, play starts it over.
    pub fn toggle_play(&mut self, length: f64) {
        if !self.playing && self.time >= length - 1e-6 {
            self.time = 0.0;
        }
        self.playing = !self.playing;
    }

    /// Starts or stops recording the camera.
    pub fn toggle_recording(&mut self) {
        self.recording = !self.recording;
        if self.recording {
            self.look_through = false;
            self.playing = true;
            self.recorded_at = f64::NEG_INFINITY;
        }
    }

    /// While recording: the viewport's view keyed at the playhead every half second, the clip's camera keys it passes over replaced
    /// (one undo entry for the whole recording).
    pub fn record(&mut self, e: &mut Editor) {
        if !self.recording {
            return;
        }
        if !self.playing {
            self.recording = false;
            if let Some(d) = scene_doc(e) {
                d.end_gesture();
            }
            e.status("Cinematic: camera recorded", false);
            return;
        }
        if self.time - self.recorded_at < 0.5 && self.recorded_at.is_finite() {
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
        let Some(clip) = self.current_clip(&p).map(|c| c.id) else { return };
        let units = e.scene.game_units.max(1e-3);
        let vc = e.viewport.camera();
        let key = CameraKey {
            t: self.time,
            position: [vc.eye.x * units, vc.eye.y * units, vc.eye.z * units],
            target: [vc.target.x * units, vc.target.y * units, vc.target.z * units],
            fov: vc.fov_y.to_degrees(),
            interp: Interp::Smooth,
        };
        let from = if self.recorded_at.is_finite() { self.recorded_at } else { self.time - 1e-6 };
        let t = self.time;
        let mut q = p.clone();
        let track = q.clip(clip).and_then(|c| self.keyed_camera(e, c)).or_else(|| add_track(&mut q, clip, "Camera", None, ""));
        if let Some(track) = track {
            for_track(&mut q, clip, track, |tr| {
                if let TrackContent::Camera { keys } = &mut tr.content {
                    keys.retain(|k| k.t <= from || k.t > t + 1e-6);
                    keys.push(key.clone());
                }
            });
            self.track = Some(track);
        }
        if let Some(d) = scene_doc(e) {
            d.push_merged("record camera", q, 0x5EC0_4D00);
        }
        self.recorded_at = t;
    }

    pub fn add_key_public(&mut self, e: &mut Editor, p: &Project, clip: &Clip) {
        self.add_key(e, p, clip);
    }

    /// A camera key at the playhead holding the viewport's view (the clip's camera track, made when it has none).
    pub fn key_camera_view(&mut self, e: &mut Editor) {
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
        let Some(clip) = self.current_clip(&p).map(|c| c.id) else { return };
        let track = p.clip(clip).and_then(|c| self.keyed_camera(e, c));
        let track = match track {
            Some(t) => Some(t),
            None => {
                let mut made = None;
                edit(e, "add track", |p| made = add_track(p, clip, "Camera", None, ""));
                made
            }
        };
        let Some(track) = track else { return };
        self.track = Some(track);
        if let Some(p) = scene_doc(e).map(|d| d.project().clone())
            && let Some(c) = p.clip(clip).cloned()
        {
            self.add_key(e, &p, &c);
        }
    }

    /// The camera Key and Record write into: the camera selected (or the one of a selected key), else the Camera track selected
    /// on the Timeline, else the live one at the playhead, else the first.
    pub fn keyed_camera(&self, e: &Editor, clip: &Clip) -> Option<u32> {
        let is_cam = |id: u32| clip.tracks.iter().any(|t| t.id == id && matches!(t.content, TrackContent::Camera { .. }));
        crate::items::selected(e)
            .into_iter()
            .find_map(crate::items::SceneItem::camera)
            .filter(|&id| is_cam(id))
            .or(self.track.filter(|&id| is_cam(id)))
            .or_else(|| wce_cinematic::eval::live_camera(clip, self.time))
            .or_else(|| clip.tracks.iter().find(|t| matches!(t.content, TrackContent::Camera { .. })).map(|t| t.id))
    }

    /// A new camera where the viewport looks from (keyed at the playhead), selected. Returns its track.
    pub fn new_camera(&mut self, e: &mut Editor) -> Option<u32> {
        let p = scene_doc(e).map(|d| d.project().clone())?;
        let clip = self.current_clip(&p)?.id;
        let units = e.scene.game_units.max(1e-3);
        let vc = e.viewport.camera();
        let key = CameraKey {
            t: self.time,
            position: [vc.eye.x * units, vc.eye.y * units, vc.eye.z * units],
            target: [vc.target.x * units, vc.target.y * units, vc.target.z * units],
            fov: vc.fov_y.to_degrees(),
            interp: Interp::Smooth,
        };
        let mut made = None;
        edit(e, "new camera", |p| {
            made = add_track(p, clip, "Camera", None, "");
            if let Some(id) = made {
                for_track(p, clip, id, |tr| {
                    if let TrackContent::Camera { keys } = &mut tr.content {
                        keys.push(key);
                    }
                });
            }
        });
        if let Some(id) = made {
            self.track = Some(id);
            crate::items::select(e, crate::items::SceneItem::Camera(id), crate::items::How::Set);
        }
        made
    }

    /// A cut to `camera` at the playhead (the Camera Cuts made when the clip has none; its first cut, at 0, to the camera live
    /// until now).
    pub fn cut_to(&mut self, e: &mut Editor, camera: u32) {
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else { return };
        let Some(clip) = self.current_clip(&p).cloned() else { return };
        let t = self.time;
        let before = wce_cinematic::eval::live_camera(&clip, t);
        let had = clip.tracks.iter().find(|tr| matches!(tr.content, TrackContent::Cuts { .. })).map(|tr| tr.id);
        edit(e, "cut to camera", |p| {
            let Some(cuts) = had.or_else(|| add_track(p, clip.id, "Camera Cuts", None, "")) else { return };
            for_track(p, clip.id, cuts, |tr| {
                if let TrackContent::Cuts { cuts } = &mut tr.content {
                    if cuts.is_empty()
                        && t > 1e-6
                        && let Some(b) = before
                    {
                        cuts.push(CutKey { t: 0.0, camera: b });
                    }
                    cuts.retain(|c| (c.t - t).abs() > 1e-6);
                    cuts.push(CutKey { t, camera });
                    cuts.sort_by(|a, b| a.t.total_cmp(&b.t));
                }
            });
        });
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
        let cams = wce_cinematic::eval::clip_cameras(clip);
        let cut_camera = crate::items::selected(e)
            .into_iter()
            .find_map(crate::items::SceneItem::camera)
            .filter(|c| cams.contains(c))
            .or_else(|| {
                let live = wce_cinematic::eval::live_camera(clip, t)?;
                let at = cams.iter().position(|c| *c == live)?;
                cams.get((at + 1) % cams.len()).copied()
            })
            .or_else(|| cams.first().copied());
        edit(e, "add key", |p| {
            for_track(p, clip.id, track_id, |tr| match &mut tr.content {
                TrackContent::Cuts { cuts } => {
                    if let Some(camera) = cut_camera {
                        cuts.retain(|c| (c.t - t).abs() > 1e-6);
                        cuts.push(CutKey { t, camera });
                    }
                }
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
                TrackContent::Music { cues } => cues.push(MusicCue {
                    t,
                    event: String::new(),
                    moods: vec!["music_set_Intensity_Battle".into()],
                    stop: None,
                    fade_in: 1.0,
                    fade_out: 1.0,
                }),
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

    // ---- The selected key (in the Inspector) ----

    pub fn key_body(&mut self, e: &mut Editor) {
        let (Some(p), Some(k)) = (scene_doc(e).map(|d| d.project().clone()), self.key.clone()) else { return };
        let Some(clip) = self.current_clip(&p).cloned() else { return };
        let Some(track) = clip.tracks.iter().find(|t| t.id == k.track).cloned() else {
            self.key = None;
            return;
        };
        let mut content = track.content.clone();
        let mut changed = false;
        let mut delete = false;
        gui::text(&format!("Key: {} / {} #{}", track.name, k.list, k.index));
        gui::separator();
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
            TrackContent::Cuts { cuts } => {
                if let Some(c) = cuts.get_mut(k.index) {
                    time_row!(c.t);
                    let cams: Vec<(u32, String)> = clip
                        .tracks
                        .iter()
                        .filter(|t| matches!(t.content, TrackContent::Camera { .. }))
                        .map(|t| (t.id, t.name.clone()))
                        .collect();
                    let names: Vec<&str> = cams.iter().map(|(_, n)| n.as_str()).collect();
                    let mut at = cams.iter().position(|(id, _)| *id == c.camera).map_or(-1, |i| i as i32);
                    gui::row("Cut to");
                    if gui::combo("##cutcam", &mut at, &names) && at >= 0 {
                        c.camera = cams[at as usize].0;
                        changed = true;
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("The camera the clip shows from this cut to the next");
                    }
                }
            }
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
            TrackContent::Unit { unit, path, actions, men, visible } => match k.list {
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
                        gui::row("Animation");
                        let own = p.unit(*unit).and_then(|u| self.slots.get(u.slots_key())).cloned();
                        if let Some(k) = picker(
                            &mut self.filters,
                            "slot",
                            if c.slot.is_empty() { "Pick..." } else { &c.slot },
                            &crate::pickers::slot_items(own.as_deref().map(Vec::as_slice)),
                            -1.0,
                        ) {
                            c.slot = k;
                            changed = true;
                        }
                        if gui::is_item_hovered() {
                            gui::set_tooltip(crate::pickers::slot_tip(&c.slot));
                        }
                        // A family of variants: any (spread over the men) or one.
                        let family = wce_cinematic::scene::slot_family(&c.slot).to_owned();
                        let variants: Vec<String> = own
                            .as_deref()
                            .map(|s| wce_cinematic::scene::slot_variants(&family, s).into_iter().map(str::to_owned).collect())
                            .unwrap_or_default();
                        if variants.len() > 1 {
                            let mut names = vec!["Any (spread over the men)".to_owned()];
                            names.extend(variants.iter().map(|v| v.rsplit_once('_').map_or(v.clone(), |(_, n)| n.to_owned())));
                            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                            let mut at = variants.iter().position(|v| *v == c.slot).map_or(0, |i| i as i32 + 1);
                            gui::row("Variant");
                            if gui::combo("##variant", &mut at, &refs) {
                                c.slot = if at == 0 { family.clone() } else { variants[at as usize - 1].clone() };
                                changed = true;
                            }
                        }
                        if crate::pickers::slot_missing(&c.slot, own.as_deref().map(Vec::as_slice)) {
                            gui::text_colored(
                                [1.0, 0.45, 0.4, 1.0],
                                &format!("{}  The unit has no {}: it stands", icons::TRIANGLE_ALERT, c.slot),
                            );
                        }
                        // Where the animation's root goes in the unit's frame (a matched combat's clips are placed so).
                        let mut place = c.place.unwrap_or([0.0; 3]);
                        gui::row("Place");
                        if gui::drag_float3("##place", &mut place, 0.01, "%.2f") {
                            c.place = (place != [0.0; 3]).then_some(place);
                            changed = true;
                        }
                        if gui::is_item_hovered() {
                            gui::set_tooltip("Where the animation stands in the unit's frame: x and z (metres), a turn (degrees)");
                        }
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
                    gui::row("Sound");
                    let mut play = None;
                    if let Some(k) = crate::pickers::picker_audible(
                        &mut self.filters,
                        "sound",
                        if c.event.is_empty() { "Pick..." } else { &c.event },
                        &self.lists.sound_events,
                        -30.0,
                        &mut play,
                    ) {
                        c.event = k;
                        changed = true;
                    }
                    gui::same_line();
                    if gui::small_button(&format!("{}##playsound", icons::PLAY)) && !c.event.is_empty() {
                        play = Some(c.event.clone());
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("Listen to it");
                    }
                    if let Some(p) = play {
                        self.audition = Some((Some(p), vec![]));
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("A sound event of the game's banks");
                    }
                    changed |= float_row("Volume", &mut c.volume, 0.01, "0 to 2");
                }
            }
            TrackContent::Music { cues } => {
                if let Some(c) = cues.get_mut(k.index) {
                    time_row!(c.t);
                    // The game's music by mood: a value per group (the battle sets them; its music follows).
                    let mut groups: Vec<String> = vec![];
                    for m in &self.lists.moods {
                        if !groups.contains(&m.group) {
                            groups.push(m.group.clone());
                        }
                    }
                    if groups.is_empty() {
                        gui::text_disabled("Reading the game's music...");
                    }
                    for g in groups.iter().filter(|g| !g.eq_ignore_ascii_case("gamemode")) {
                        let values: Vec<&wce_cinematic::music::Mood> = self.lists.moods.iter().filter(|m| &m.group == g).collect();
                        let mut names: Vec<String> = vec!["(as it is)".to_owned()];
                        names.extend(values.iter().map(|m| m.value.clone()));
                        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                        let mut i = values.iter().position(|m| c.moods.contains(&m.event)).map_or(0, |k| k as i32 + 1);
                        gui::row(g);
                        gui::set_next_item_width(-1.0);
                        if gui::combo(&format!("##mood{g}"), &mut i, &refs) {
                            c.moods.retain(|e| !values.iter().any(|m| &m.event == e));
                            if i > 0 {
                                c.moods.push(values[i as usize - 1].event.clone());
                            }
                            changed = true;
                        }
                        if gui::is_item_hovered() {
                            gui::set_tooltip(&format!("The music's {g} from here (as it is: what the battle has)"));
                        }
                    }
                    if gui::small_button(&format!("{}  Listen", icons::PLAY)) {
                        self.audition = Some((None, c.moods.clone()));
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("20 seconds of the music these moods give");
                    }
                    if !c.event.is_empty() {
                        gui::row("Event");
                        gui::text_disabled(&c.event);
                    }
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
                    gui::row("Weather");
                    if let Some(k) = picker(
                        &mut self.filters,
                        "weather",
                        if c.environment.is_empty() { "Pick..." } else { &c.environment },
                        &self.lists.environments,
                        -1.0,
                    ) {
                        c.environment = k;
                        changed = true;
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("The weather from this key; it blends into the next key's");
                    }
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
                    gui::row("Effect");
                    if let Some(k) =
                        picker(&mut self.filters, "vfx", if c.vfx.is_empty() { "Pick..." } else { &c.vfx }, &self.lists.vfx, -1.0)
                    {
                        c.vfx = k;
                        changed = true;
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip("A visual effect of the game's vfx folder");
                    }
                    gui::row("Position");
                    changed |= gui::drag_float3("##pos", &mut c.position, 0.5, "%.1f");
                    changed |= float_row("Scale", &mut c.scale, 0.01, "Size");
                }
            }
        }
        if gui::button(&format!("{}  Delete key", icons::TRASH_2)) {
            delete = true;
        }
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

    /// The Layout's picture of the ground: a top-down orthographic render at the view's centre and scale. While the view moves the
    /// last picture is drawn where its ground now is (`layout_shown`); a new one is rendered once the view has held still a moment (and
    /// every second and a half: the ground streams in, the units move), and takes the old one's place only once it is on the GPU.
    fn layout_picture(&mut self, e: &mut Editor, centre: [f32; 2], mpp: f32, size: gui::Vec2) {
        let (w, h) = ((size.x.max(16.0)) as u32, (size.y.max(16.0)) as u32);
        let key = (centre.map(|v| (v * 10.0).round() as i32), (mpp * 1000.0).round() as i32, w, h);
        // The newer picture is up: the older one goes.
        if let Some((n, view)) = self.layout_pending
            && e.images.get(&format!("cinematic/layout/{n}")).is_some()
        {
            if let Some((old, _)) = self.layout_shown.replace((n, view)) {
                e.images.forget(&format!("cinematic/layout/{old}"));
            }
            self.layout_pending = None;
        }
        if self.layout_pending.is_some() {
            return;
        }
        if self.layout_view_key != Some(key) {
            self.layout_view_key = Some(key);
            self.layout_view_since = Some(std::time::Instant::now());
        }
        let still = e.scripted_input() || self.layout_view_since.is_some_and(|t| t.elapsed().as_secs_f32() > 0.25);
        let stale = self.layout_pic_at.is_none_or(|t| t.elapsed().as_secs_f32() > 1.5);
        if !still || (self.layout_pic_key == Some(key) && !stale) || (e.scripted_input() && self.layout_pic_key == Some(key)) {
            return;
        }
        let units = e.scene.game_units.max(1e-3);
        let (cx, cz) = (centre[0] / units, centre[1] / units);
        let cam = wce_scene::camera::Camera {
            eye: wce_foundation::math::Float3::new(cx, 6000.0 / units, cz),
            target: wce_foundation::math::Float3::new(cx, 0.0, cz),
            up: wce_foundation::math::Float3::new(0.0, 0.0, 1.0),
            fov_y: 0.6,
            znear: 1.0 / units,
            zfar: 13000.0 / units,
            ortho_height: Some(size.y * mpp / units),
        };
        let Some(rgba) = wce_editor::viewport::render_offscreen(e, &mut self.layout_target, w, h, cam, e.clock.time() as f32, true) else {
            return;
        };
        self.layout_pic_n += 1;
        e.images.submit(&format!("cinematic/layout/{}", self.layout_pic_n), w, h, rgba);
        self.layout_pending = Some((self.layout_pic_n, (centre, mpp, [size.x, size.y])));
        self.layout_pic_key = Some(key);
        self.layout_pic_at = Some(std::time::Instant::now());
    }

    pub fn layout_panel(&mut self, e: &mut Editor, create: &mut crate::create::Create) {
        gui::set_next_window_size(v2(500.0, 500.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Layout", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            self.start_buttons();
            gui::end();
            return;
        };
        let frame = self.frame(&p);
        let size = gui::content_region_avail();
        let origin = gui::cursor_screen_pos();
        // First look: the player's deployment zone, close enough to tell the units apart.
        let home = p.armies.iter().find(|a| a.player).or(p.armies.first()).map_or([0.0, 0.0], |a| a.deployment_centre);
        let (centre, mpp) = *self.view.get_or_insert((home, 0.8));
        let to_s = |w: [f32; 2]| v2(origin.x + size.x * 0.5 + (w[0] - centre[0]) / mpp, origin.y + size.y * 0.5 - (w[1] - centre[1]) / mpp);
        let to_w = |s: gui::Vec2| [centre[0] + (s.x - origin.x - size.x * 0.5) * mpp, centre[1] - (s.y - origin.y - size.y * 0.5) * mpp];
        let draw = gui::window_draw_list();
        draw.push_clip_rect(origin, v2(origin.x + size.x, origin.y + size.y));
        draw.rect_filled(origin, v2(origin.x + size.x, origin.y + size.y), gui::col32(30, 36, 30, 255), 0.0);
        // The ground under it: the scene seen from straight above (the map, the tile, the men), rendered again when the view moves
        // and every second and a half meanwhile (the ground streams in, the units move).
        self.layout_picture(e, centre, mpp, size);
        if let Some((n, (pc, pmpp, psize))) = self.layout_shown
            && let Some(img) = e.images.get(&format!("cinematic/layout/{n}"))
        {
            // Where the picture's ground is in the view as it is now.
            let (hw, hh) = (psize[0] * 0.5 * pmpp, psize[1] * 0.5 * pmpp);
            let (a, b) = (to_s([pc[0] - hw, pc[1] + hh]), to_s([pc[0] + hw, pc[1] - hh]));
            draw.image(img.id, a, b, v2(0.0, 0.0), v2(1.0, 1.0), 0xFFFF_FFFF);
            // Dimmed a little, the marks over it stand out.
            draw.rect_filled(origin, v2(origin.x + size.x, origin.y + size.y), gui::col32(0, 0, 0, 25), 0.0);
        }
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
                let (hw, hd) = (u.width * 0.5, crate::view::formation_depth(s.men, u.width) * 0.5);
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
            // The unit under the mouse, by name.
            let under = frame.as_ref().and_then(|f| {
                f.units
                    .iter()
                    .filter_map(|s| p.unit(s.unit).map(|u| (u, ((s.position[0] - w[0]).powi(2) + (s.position[1] - w[1]).powi(2)).sqrt())))
                    .filter(|(u, d)| *d <= u.width * 0.6 + 4.0 * mpp)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(u, _)| u.name.clone())
            });
            if create.making.is_none() {
                gui::set_tooltip(&match under {
                    Some(n) => format!("{n}  ·  drag to move, Ctrl+click: a path key at the playhead"),
                    None => format!("x {:.0}  z {:.0}  ·  right-drag: pan, wheel: zoom", w[0], w[1]),
                });
            }
        }
        // Dragging the selected unit moves where it starts; kept when the mouse is released.
        if let Some((id, from)) = self.drag_unit {
            if gui::is_mouse_down(0) {
                let w = to_w(gui::mouse_pos());
                let draw = gui::window_draw_list();
                draw.circle_filled(to_s(w), 6.0, gui::col32(255, 230, 120, 200));
                draw.line(to_s(from), to_s(w), gui::col32(255, 230, 120, 200), 1.5);
            } else {
                let w = to_w(gui::mouse_pos());
                self.drag_unit = None;
                if (w[0] - from[0]).abs() + (w[1] - from[1]).abs() > 0.5 {
                    // As the gizmo moves it: a unit with a path in the clip is keyed at the playhead.
                    if let Some(d) = scene_doc(e) {
                        let units = d.view.units.max(1e-3);
                        let delta = wce_foundation::math::translation(wce_foundation::math::Float3::new(
                            (w[0] - from[0]) / units,
                            0.0,
                            (w[1] - from[1]) / units,
                        ));
                        d.end_gesture();
                        d.move_items(&[crate::items::SceneItem::Unit(id)], &delta, 0);
                        d.end_gesture();
                    }
                }
            }
        }
        if hovered && create.making.is_some() {
            let w = to_w(gui::mouse_pos());
            create.ghost = Some(w);
            gui::set_tooltip("Click where it goes  ·  R / Shift+R: turn  ·  Esc or right click: stop");
            // Where it will stand.
            if let Some(crate::create::Making::Unit(_)) = &create.making {
                let army = create.draft.army.min(p.armies.len().saturating_sub(1));
                let facing = p.armies.get(army).map_or(0.0, |a| a.deployment_orientation) + create.draft.turn.to_radians();
                let (sin, cos) = facing.sin_cos();
                let (hw, hd) = (create.draft.width * 0.5, crate::view::formation_depth(create.draft.men, create.draft.width) * 0.5);
                let corner = |x: f32, z: f32| to_s([w[0] + x * cos + z * sin, w[1] - x * sin + z * cos]);
                let pts = [corner(-hw, -hd), corner(hw, -hd), corner(hw, hd), corner(-hw, hd)];
                let draw = gui::window_draw_list();
                draw.polyline(&pts, gui::col32(255, 230, 120, 220), true, 1.5);
                draw.line(corner(0.0, hd), corner(0.0, hd + 8.0), gui::col32(255, 230, 120, 220), 1.5);
            }
            if gui::is_mouse_clicked(1) {
                create.stop();
            }
        }
        if gui::is_item_clicked()
            && let Some(m) = create.making.clone()
        {
            let w = to_w(gui::mouse_pos());
            let lists = self.lists.clone();
            if let Some(item) = create.place(e, &lists, &p, &m, w, self.clip, self.time) {
                create.requests.push(crate::create::CreateRequest::Placed(item));
            }
        } else if gui::is_item_clicked() {
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
                // Held: a drag that moves it.
                self.drag_unit = self.unit.and_then(|id| f.units.iter().find(|s| s.unit == id)).map(|s| (s.unit, s.position));
            }
        }
        gui::end();
    }
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
    if kind == "Camera Cuts"
        && let Some(id) = p.clip(clip)?.tracks.iter().find(|t| matches!(t.content, TrackContent::Cuts { .. })).map(|t| t.id)
    {
        return Some(id);
    }
    let content = match kind {
        "Camera" => TrackContent::Camera { keys: vec![] },
        "Camera Cuts" => TrackContent::Cuts { cuts: vec![] },
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
        // Cameras numbered: the first name no camera of the clip has.
        (TrackContent::Camera { .. }, _) => {
            let taken: Vec<String> = p.clip(clip)?.tracks.iter().map(|t| t.name.clone()).collect();
            (1..).map(|n| format!("Camera {n}")).find(|n| !taken.contains(n)).unwrap_or_default()
        }
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
        TrackContent::Cuts { cuts } => list("cuts", cuts.iter().map(|k| k.t).collect()).collect(),
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
        (TrackContent::Cuts { cuts }, _) => cuts.get_mut(k.index).iter_mut().for_each(|x| x.t = t),
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
        (TrackContent::Cuts { cuts }, _) => rm(cuts, i),
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

/// Adds a unit of `key` to `army`: at `at`, else in a row of the army's deployment zone. Returns its id.
pub fn add_unit(e: &mut Editor, lists: &GameLists, key: &str, army: usize, at: Option<[f32; 2]>) -> u32 {
    let name = lists.unit_name(key);
    let mut made = 0;
    edit(e, "add unit", |p| {
        let id = p.new_id();
        let a = p.armies[army.min(p.armies.len().saturating_sub(1))].clone();
        let n = p.units.iter().filter(|u| u.army == army).count() as f32;
        let start = at.unwrap_or([a.deployment_centre[0] - 150.0 + 35.0 * (n % 9.0), a.deployment_centre[1] - 20.0 * (n / 9.0).floor()]);
        p.units.push(SceneUnit {
            id,
            name,
            unit: key.to_owned(),
            army,
            start,
            facing: a.deployment_orientation,
            width: 30.0,
            men: 160,
            seed: id,
            general: false,
            command: true,
            character: String::new(),
        });
        made = id;
    });
    made
}
