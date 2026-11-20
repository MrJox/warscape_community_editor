//! The Audio Recipes panel (docs/plans/audio.md, step 8): the common jobs, each one place doing every edit it needs. Music tracks
//! (playlists named by how the music switches choose them), a voice line, a culture's voice lines, an animation sound.

use std::sync::Arc;

use wce_audio::recipes;
use wce_audio_format::fnv;
use wce_audio_format::hirc::Object;
use wce_editor::gui::{self, v2};

use super::{AudioTool, Request};

/// The voice-over culture group the battle voice trees decide on (its values are the DB's voice-over cultures).
const CULTURE_GROUP: &str = "vo_battle_subculture";

#[derive(Default)]
pub struct View {
    music: Option<Arc<Vec<(u32, Vec<String>)>>>,
    music_filter: String,
    vo_filter: String,
    dialogue: Option<u32>,
    culture: Option<u32>,
    time: f32,
    event: String,
    like: String,
    name: String,
}

impl AudioTool {
    pub(super) fn recipes_panel(&mut self, requests: &mut Vec<Request>) {
        gui::set_next_window_size(v2(600.0, 500.0), gui::COND_FIRST_USE_EVER);
        if self.bank_dock != 0 {
            gui::set_next_window_dock_id(self.bank_dock, gui::COND_FIRST_USE_EVER);
        }
        if !gui::begin("Audio Recipes", None, 0) {
            gui::end();
            return;
        }
        let (Some(lib), Some(files)) = (self.library.clone(), self.files.clone()) else {
            gui::text_disabled("Reading the game's sound banks...");
            gui::end();
            return;
        };

        // Music tracks.
        if gui::collapsing_header("Music tracks", gui::TREE_DEFAULT_OPEN) {
            gui::text_wrapped("Add a track to a playlist, or pick one to see its entries (Replace music..., weights) in Audio Object.");
            let music = self.recipes_view.music.get_or_insert_with(|| Arc::new(recipes::music_playlists(&lib, files.as_ref()))).clone();
            gui::set_next_item_width(-1.0);
            gui::input_text("##music", Some("Filter (e.g. Hunnic, battle, prebattle)"), &mut self.recipes_view.music_filter, 128, 0);
            let filter = self.recipes_view.music_filter.to_lowercase();
            let shown: Vec<&(u32, Vec<String>)> =
                music.iter().filter(|(_, names)| filter.is_empty() || names.iter().any(|n| n.to_lowercase().contains(&filter))).collect();
            if gui::begin_child("music list", v2(0.0, 150.0), gui::CHILD_BORDERS, 0) {
                gui::clipped(shown.len(), |row| {
                    let (id, names) = shown[row];
                    gui::push_id(&format!("m{id}"));
                    if gui::small_button("Add a track...") {
                        let first_leaf = match self.edits.object(&lib, *id) {
                            Some(Object::MusicPlaylist(p)) => {
                                p.items.iter().find(|i| i.children == 0 && i.segment_id != 0).map(|i| i.item_id)
                            }
                            _ => None,
                        };
                        if let Some(leaf) = first_leaf {
                            requests.push(Request::AddMusic(*id, leaf));
                        }
                    }
                    gui::same_line();
                    let more = if names.len() > 1 { format!("  (+{} more)", names.len() - 1) } else { String::new() };
                    if gui::selectable(&format!("{}{more}", names[0]), self.selected == Some(*id), 0) {
                        requests.push(Request::Select(*id));
                    }
                    gui::pop_id();
                });
            }
            gui::end_child();
        }

        // A voice line.
        if gui::collapsing_header("A voice line (advisor, orders, speeches)", 0) {
            gui::text_wrapped("Find the event, select it: Audio Object lists what it plays; a sound's Replace audio... swaps the line.");
            gui::set_next_item_width(-1.0);
            gui::input_text("##vo", Some("Filter voice events (e.g. advice, charge, retreat)"), &mut self.recipes_view.vo_filter, 128, 0);
            let filter = self.recipes_view.vo_filter.to_lowercase();
            if filter.len() >= 2 && gui::begin_child("vo list", v2(0.0, 150.0), gui::CHILD_BORDERS, 0) {
                let mut shown = 0;
                for b in lib.banks.iter().filter(|b| b.name.contains("vo") || b.name.contains("advice")) {
                    for o in b.bank.objects().iter().filter(|o| matches!(o.kind, 4 | 15)) {
                        let label = lib.label(o.id, o.kind);
                        if shown >= 200 || !label.to_lowercase().contains(&filter) {
                            continue;
                        }
                        shown += 1;
                        gui::push_id(&format!("v{}", o.id));
                        if self.play.button(&format!("v{}", o.id), requests) {
                            requests.push(Request::PlayObject(o.id));
                        }
                        gui::same_line();
                        if gui::selectable(&format!("{label}  ({})", b.name), self.selected == Some(o.id), 0) {
                            requests.push(Request::Select(o.id));
                        }
                        gui::pop_id();
                    }
                }
                gui::end_child();
            }
        }

        // A culture's voice lines.
        if gui::collapsing_header("A culture's voice lines", 0) {
            gui::text_wrapped(
                "Battle voice lines are chosen by voice-over culture (vo_battle_subculture: the DB gives each subculture and religion one). \
                 Pick a voice event and a culture to see and replace its lines.",
            );
            let group = fnv::id(CULTURE_GROUP);
            let events: Vec<(String, u32)> = lib
                .banks
                .iter()
                .flat_map(|b| b.bank.objects().iter().zip(&b.typed))
                .filter_map(|(o, t)| match t {
                    Some(Object::DialogueEvent(d)) if d.arguments.iter().any(|(g, _)| *g == group) => Some((lib.label(o.id, 15), o.id)),
                    _ => None,
                })
                .collect();
            if gui::begin_child("dialogue list", v2(0.0, 110.0), gui::CHILD_BORDERS, 0) {
                for (label, id) in &events {
                    if gui::selectable(&format!("{label}##d{id}"), self.recipes_view.dialogue == Some(*id), 0) {
                        self.recipes_view.dialogue = Some(*id);
                        self.recipes_view.culture = None;
                    }
                }
            }
            gui::end_child();
            if let Some(dialogue) = self.recipes_view.dialogue {
                for value in recipes::group_values(&self.edits, &lib, dialogue, group) {
                    if gui::small_button(&format!("{}##c{value}", lib.label(value, 0))) {
                        self.recipes_view.culture = Some(value);
                    }
                    gui::same_line();
                }
                gui::new_line();
                if let Some(culture) = self.recipes_view.culture {
                    let lines = recipes::culture_lines(&self.edits, &lib, dialogue, group, culture);
                    gui::text_disabled(&format!("{} lines for {}", lines.len(), lib.label(culture, 0)));
                    for (keys, node) in lines.iter().take(300) {
                        gui::push_id(&format!("l{node}{}", keys.iter().map(u32::to_string).collect::<String>()));
                        if self.play.button(&format!("l{node}{}", keys.iter().map(u32::to_string).collect::<String>()), requests) {
                            requests.push(Request::PlayObject(*node));
                        }
                        gui::same_line();
                        if let Some(Object::Sound(s)) = self.edits.object(&lib, *node) {
                            if gui::small_button("Replace audio...") {
                                requests.push(Request::Replace(s.source.source_id));
                            }
                            gui::same_line();
                        }
                        let path: Vec<String> = keys.iter().filter(|k| **k != 0 && **k != culture).map(|k| lib.label(*k, 0)).collect();
                        let kind = lib.object(*node).map_or(0, |(_, o, _)| o.kind);
                        if gui::selectable(&format!("{}: {}", path.join(" / "), lib.label(*node, kind)), false, 0) {
                            requests.push(Request::Select(*node));
                        }
                        gui::pop_id();
                    }
                }
            }
        }

        // An animation sound.
        if gui::collapsing_header("An animation sound", 0) {
            match self.data_view.animation().map(str::to_owned) {
                None => gui::text_wrapped("Pick an animation in Audio Game Data > Animations first."),
                Some(path) => {
                    gui::text(&path);
                    gui::set_next_item_width(120.0);
                    gui::drag_float("Time", &mut self.recipes_view.time, 0.005, 0.0, 600.0, "%.3f s");
                    gui::separator_text("Fire an existing event");
                    gui::set_next_item_width(260.0);
                    gui::input_text("##event", Some("event name (e.g. battle_footstep_walk_01)"), &mut self.recipes_view.event, 128, 0);
                    gui::same_line();
                    if gui::button("Add the frame") && !self.recipes_view.event.trim().is_empty() {
                        requests.push(Request::AnimationEvent(
                            path.clone(),
                            self.recipes_view.time,
                            self.recipes_view.event.trim().to_owned(),
                        ));
                    }
                    gui::separator_text("A new sound");
                    gui::set_next_item_width(260.0);
                    gui::input_text(
                        "##like",
                        Some("set up like the event (e.g. battle_footstep_walk_01)"),
                        &mut self.recipes_view.like,
                        128,
                        0,
                    );
                    gui::set_next_item_width(260.0);
                    gui::input_text("##name", Some("the new event's name (e.g. my_mod_footstep)"), &mut self.recipes_view.name, 128, 0);
                    let ready = !self.recipes_view.like.trim().is_empty() && !self.recipes_view.name.trim().is_empty();
                    gui::begin_disabled(!ready);
                    if gui::button("Pick a sound file and add it...") {
                        requests.push(Request::AnimationNewSound(
                            path.clone(),
                            self.recipes_view.time,
                            self.recipes_view.like.trim().to_owned(),
                            self.recipes_view.name.trim().to_owned(),
                        ));
                    }
                    gui::end_disabled();
                    if gui::is_item_hovered() {
                        gui::set_tooltip(
                            "Makes a new sound (set up like the given event's), a new event playing it, a trigger row firing the event,\nand the frame in this animation.",
                        );
                    }
                }
            }
        }
        gui::end();
    }
}
