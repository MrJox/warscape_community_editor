//! The Audio Game Data panel (docs/plans/audio.md, step 7): the game's sound data outside the banks. Animations (`.snd.meta`: the
//! sound frames on a timeline, edited and played), campaign ambience (`.amb`: emitters by event), the DB tables naming events, the
//! voice-overs, and which language packs give the voice lines.

use std::collections::BTreeMap;

use wce_audio::anim_sounds::{self, SoundFrame};
use wce_audio::sound_db::SoundDb;
use wce_audio_format::amb::Ambience;
use wce_audio_format::fnv;
use wce_audio_format::triggers::Triggers;
use wce_editor::gui::{self, col32, v2};
use wce_editor::icons;
use wce_formats::anim_meta::Meta;
use wce_vfs::files::Files;

use super::{AudioTool, Request};

/// What the panel shows, read once with the references (on a worker).
#[derive(Default)]
pub struct GameData {
    pub db: SoundDb,
    pub triggers: Option<Triggers>,
    pub animations: Vec<String>,
    pub ambience: Vec<String>,
}

impl GameData {
    pub fn read(files: &dyn Files) -> Self {
        Self {
            db: SoundDb::read(files),
            triggers: files.read("audio/sound_animation_triggers.csv").ok().and_then(|b| Triggers::read(&b).ok()),
            animations: files.list("animations/", ".snd.meta"),
            ambience: files.list("", ".amb"),
        }
    }
}

/// The panel's choices.
#[derive(Default)]
pub struct View {
    filter: String,
    animation: Option<String>,
    ambience: Option<String>,
    new_trigger: i32,
}

impl View {
    /// The animation picked in the Animations tab.
    pub fn animation(&self) -> Option<&str> {
        self.animation.as_deref()
    }
}

/// A frame row's label: the trigger's name and its battle event.
fn frame_label(triggers: Option<&Triggers>, f: &SoundFrame) -> (String, String) {
    triggers.map_or((format!("row {}", f.trigger), String::new()), |t| {
        let (name, battle, campaign) = anim_sounds::trigger_events(t, f.trigger);
        let event = if battle.is_empty() { campaign } else { battle };
        (format!("{name} (row {})", f.trigger), event)
    })
}

impl AudioTool {
    /// A file as edited (an unsaved edit first), else the game's.
    fn edited_file(&self, path: &str) -> Option<Vec<u8>> {
        self.edits.loose.get(path).cloned().or_else(|| self.files.as_ref()?.read(path).ok())
    }

    pub(super) fn game_data_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(600.0, 500.0), gui::COND_FIRST_USE_EVER);
        if self.bank_dock != 0 {
            gui::set_next_window_dock_id(self.bank_dock, gui::COND_FIRST_USE_EVER);
        }
        if !gui::begin("Audio Game Data", None, 0) {
            gui::end();
            return;
        }
        let Some(data) = self.game_data.clone() else {
            gui::text_disabled("Reading the game's animations, ambience and tables...");
            gui::end();
            return;
        };
        if let Some(part) = self.pending_animation.take() {
            let part = part.to_lowercase();
            self.data_view.animation = data.animations.iter().find(|p| p.contains(&part)).cloned();
            gui::select_dock_tab("Audio Game Data");
        }

        if gui::begin_tab_bar("game data") {
            if gui::begin_tab_item(&format!("Animations ({})###anims", data.animations.len()), 0) {
                self.animations_tab(&data, requests);
                gui::end_tab_item();
            }
            if gui::begin_tab_item(&format!("Campaign ambience ({})###amb", data.ambience.len()), 0) {
                self.ambience_tab(&data, requests);
                gui::end_tab_item();
            }
            if gui::begin_tab_item("DB tables###db", 0) {
                db_tab(&data.db, requests);
                gui::end_tab_item();
            }
            if gui::begin_tab_item("Languages###lang", 0) {
                gui::text_wrapped(
                    "Voice lines come from the game's language packs (local_*.pack): the banks and sound files of the language the game is \
                     set to are read with the others, and show in Audio Banks and Audio Search like every bank.",
                );
                if let Some(lib) = &self.library {
                    let vo: Vec<&str> =
                        lib.banks.iter().map(|b| b.name.as_str()).filter(|n| n.contains("_vo") || n.contains("advice")).collect();
                    gui::text_disabled(&format!("{} voice banks: {}", vo.len(), vo.join(", ")));
                }
                gui::end_tab_item();
            }
            gui::end_tab_bar();
        }
        gui::end();
    }

    fn animations_tab(&mut self, data: &GameData, requests: &mut Vec<Request>) {
        gui::set_next_item_width(-1.0);
        gui::input_text("##animfilter", Some("Filter animations (e.g. boar, crossbow, footstep)"), &mut self.data_view.filter, 128, 0);
        let filter = self.data_view.filter.to_lowercase();
        let shown: Vec<&String> = data.animations.iter().filter(|p| filter.is_empty() || p.contains(&filter)).collect();
        if gui::begin_child("anim list", v2(0.0, 160.0), gui::CHILD_BORDERS, 0) {
            gui::clipped(shown.len(), |row| {
                let path = shown[row];
                let edited = self.edits.loose.contains_key(path.as_str());
                let label = format!("{}{}##{row}", path.trim_start_matches("animations/"), if edited { "  (edited)" } else { "" });
                if gui::selectable(&label, self.data_view.animation.as_deref() == Some(path.as_str()), 0) {
                    self.data_view.animation = Some(path.clone());
                }
            });
        }
        gui::end_child();

        let Some(path) = self.data_view.animation.clone() else {
            gui::text_disabled("Pick an animation: its sound frames show on a timeline.");
            return;
        };
        let Some(mut meta) = self.edited_file(&path).and_then(|b| Meta::decode(&b)) else {
            gui::text_disabled("This .snd.meta does not read.");
            return;
        };
        let frames = anim_sounds::frames(&meta);
        let triggers = data.triggers.as_ref();

        // The timeline: the clip's sound frames as marks, their names below.
        let end = frames.iter().map(|f| f.time).fold(0.0f32, f32::max).max(0.5) * 1.15;
        let width = gui::content_region_avail().x.max(100.0);
        let origin = gui::cursor_screen_pos();
        let dl = gui::window_draw_list();
        let height = 46.0;
        dl.rect_filled(origin, v2(origin.x + width, origin.y + height), col32(30, 32, 38, 255), 4.0);
        let ticks = (end * 10.0).ceil() as i32;
        for k in 0..=ticks {
            let x = origin.x + width * (k as f32 / 10.0) / end;
            let tall = k % 5 == 0;
            dl.line(v2(x, origin.y + height - if tall { 12.0 } else { 6.0 }), v2(x, origin.y + height), col32(90, 95, 110, 255), 1.0);
            if tall {
                dl.text(v2(x + 2.0, origin.y + height - 14.0), col32(120, 125, 140, 255), &format!("{:.1}s", k as f32 / 10.0));
            }
        }
        for f in &frames {
            let x = origin.x + width * f.time / end;
            dl.line(v2(x, origin.y + 4.0), v2(x, origin.y + height - 16.0), col32(240, 180, 70, 255), 2.0);
            dl.circle_filled(v2(x, origin.y + 6.0), 4.0, col32(240, 180, 70, 255));
        }
        gui::invisible_button("timeline", v2(width, height), gui::BUTTON_LEFT);
        if gui::is_item_hovered() {
            gui::set_tooltip("The animation's sound frames (seconds into the clip). Edit them in the list below.");
        }

        // The frames, editable.
        let mut changed = false;
        let mut remove = None;
        for f in &frames {
            gui::push_id(&format!("f{}", f.entry));
            let (name, event) = frame_label(triggers, f);
            if gui::small_button(icons::PLAY) && !event.is_empty() {
                requests.push(Request::PlayObject(fnv::id(&event)));
            }
            gui::same_line();
            let mut time = f.time;
            gui::set_next_item_width(80.0);
            gui::drag_float("##t", &mut time, 0.005, 0.0, 600.0, "%.3f s");
            if gui::is_item_deactivated_after_edit() {
                anim_sounds::move_frame(&mut meta, f.entry, time);
                changed = true;
            }
            gui::same_line();
            let mut row = f.trigger as i32;
            gui::set_next_item_width(80.0);
            if gui::input_int("##row", &mut row) && row >= 0 {
                anim_sounds::set_trigger(&mut meta, f.entry, row as usize);
                changed = true;
            }
            gui::same_line();
            if gui::selectable(&format!("{name}: {event}##sel"), false, 0) && !event.is_empty() {
                requests.push(Request::Select(fnv::id(&event)));
            }
            gui::same_line();
            if gui::small_button("Remove") {
                remove = Some(f.entry);
            }
            gui::pop_id();
        }
        if let Some(entry) = remove {
            meta.entries.remove(entry);
            changed = true;
        }

        gui::separator();
        gui::set_next_item_width(80.0);
        gui::input_int("##newrow", &mut self.data_view.new_trigger);
        gui::same_line();
        let preview = triggers.map(|t| anim_sounds::trigger_events(t, self.data_view.new_trigger.max(0) as usize)).unwrap_or_default();
        if gui::button("Add a sound frame at 0 s") {
            anim_sounds::add_frame(&mut meta, 0.0, self.data_view.new_trigger.max(0) as usize);
            changed = true;
        }
        gui::same_line();
        gui::text_disabled(&format!("row {}: {} ({})", self.data_view.new_trigger, preview.0, preview.1));

        if changed {
            requests.push(Request::WriteLoose(path.clone(), meta.encode(), format!("edited the sound frames of {path}")));
        }
    }

    fn ambience_tab(&mut self, data: &GameData, requests: &mut Vec<Request>) {
        for path in &data.ambience {
            if gui::selectable(path, self.data_view.ambience.as_deref() == Some(path.as_str()), 0) {
                self.data_view.ambience = Some(path.clone());
            }
        }
        gui::separator();
        let Some(path) = self.data_view.ambience.clone() else {
            gui::text_disabled(
                "Pick a campaign map's ambience: its emitters by sound (placing them on the map comes with the campaign editor).",
            );
            return;
        };
        let Some(amb) = self.edited_file(&path).and_then(|b| Ambience::read(&b).ok()) else {
            gui::text_disabled("This .amb does not read.");
            return;
        };
        let mut by_event: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for e in &amb.emitters {
            let entry = by_event.entry(e.event_id).or_default();
            entry.0 += 1;
            entry.1 += e.points.len();
        }
        let Some(lib) = self.library.clone() else { return };
        let mut rows: Vec<(String, u32, usize, usize)> = by_event.into_iter().map(|(id, (n, p))| (lib.label(id, 4), id, n, p)).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.2));
        gui::text_disabled(&format!("{} emitters, {} sounds", amb.emitters.len(), rows.len()));
        for (label, id, emitters, points) in rows {
            gui::push_id(&format!("a{id}"));
            if gui::small_button(icons::PLAY) {
                requests.push(Request::PlayObject(id));
            }
            gui::same_line();
            if gui::selectable(&format!("{label}  ({emitters} emitters, {points} points)"), false, 0) {
                requests.push(Request::Select(id));
            }
            gui::pop_id();
        }
    }
}

fn db_tab(db: &SoundDb, requests: &mut Vec<Request>) {
    let events = db.events();
    if gui::collapsing_header(&format!("Events named by the tables ({})", events.len()), gui::TREE_DEFAULT_OPEN) {
        for (table, key, column, event) in &events {
            gui::push_id(&format!("{table}{key}{column}"));
            if gui::small_button(icons::PLAY) {
                requests.push(Request::PlayObject(fnv::id(event)));
            }
            gui::same_line();
            if gui::selectable(&format!("{event}##e"), false, 0) {
                requests.push(Request::Select(fnv::id(event)));
            }
            gui::same_line();
            gui::text_disabled(&format!("{} {key}: {column}", table.trim_end_matches("_tables")));
            gui::pop_id();
        }
    }
    if gui::collapsing_header(&format!("Unit voice-overs ({})", db.unit_voiceovers.len()), 0) {
        for (vo, units) in &db.unit_voiceovers {
            gui::text(&format!("{vo}: {} units", units.len()));
        }
    }
    if gui::collapsing_header(&format!("Voice-over by subculture and religion ({})", db.voiceovers.len()), 0) {
        for (sub, religion, vo) in &db.voiceovers {
            gui::text_disabled(&format!("{sub} + {religion} -> {vo}"));
        }
    }
}
