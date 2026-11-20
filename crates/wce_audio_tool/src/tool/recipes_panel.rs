//! The Audio Recipes panel (docs/plans/audio.md, step 8): the common jobs, each one place doing every edit it needs. Music tracks
//! (playlists named by how the music switches choose them), a voice line, a culture's voice lines, an animation sound.

use std::sync::Arc;

use wce_audio::recipes;
use wce_audio::sound_db::SELECTION_COLUMNS;
use wce_audio::unit_voices::{self, CultureActors};
use wce_audio_format::fnv;
use wce_audio_format::hirc::Object;
use wce_editor::gui::{self, v2};

use super::{AudioTool, Request, UnitLine};

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
    /// A unit's voice: the order voices, the filter, the unit (index into the DB's units), the actor's order switch, the
    /// lines' filter, a new line's name and which selection column it fills.
    cultures: Option<Arc<Vec<CultureActors>>>,
    unit_filter: String,
    unit: Option<usize>,
    actor: Option<u32>,
    line_filter: String,
    new_line: String,
    column: usize,
    /// The voice set picked by hand for a unit whose voice-over names none.
    set_override: Option<u32>,
    new_culture: String,
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
        if self.pending_unit.is_some() && self.game_data.is_some() {
            gui::set_next_item_open(false, gui::COND_ALWAYS);
        }
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
                    // The engine's cultures without lines of their own in this event (no other name is valid in game).
                    let have: Vec<String> =
                        recipes::group_values(&self.edits, &lib, dialogue, group).iter().map(|v| lib.label(*v, 0).to_lowercase()).collect();
                    let free: Vec<&str> =
                        recipes::ENGINE_VOICEOVER_CULTURES.iter().copied().filter(|c| !have.iter().any(|h| h == c)).collect();
                    if !free.contains(&self.recipes_view.new_culture.as_str()) {
                        self.recipes_view.new_culture = free.first().map_or_else(String::new, |c| (*c).to_owned());
                    }
                    gui::set_next_item_width(200.0);
                    if gui::begin_combo("##newculture", &self.recipes_view.new_culture) {
                        for c in &free {
                            if gui::selectable(c, self.recipes_view.new_culture == *c, 0) {
                                self.recipes_view.new_culture = (*c).to_owned();
                            }
                        }
                        gui::end_combo();
                    }
                    gui::same_line();
                    let name = self.recipes_view.new_culture.clone();
                    gui::begin_disabled(name.is_empty());
                    if gui::button(&format!("New culture like {}", lib.label(culture, 0))) {
                        requests.push(Request::NewCulture(name.clone(), culture));
                    }
                    gui::end_disabled();
                    if gui::is_item_hovered() {
                        gui::set_tooltip(
                            "Gives the chosen culture (one the engine knows that has no lines of its own here) this one's lines in every\nbattle voice event: new containers, the same sound files (change them with Add variation / Remove). Give it to\na subculture and religion in the DB's subculture_religion_voiceover_junctions.",
                        );
                    }
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

        // A unit's voice.
        let unit_asked = self.pending_unit.is_some() && self.game_data.is_some();
        if unit_asked {
            gui::set_next_item_open(true, gui::COND_ALWAYS);
        }
        if gui::collapsing_header("A unit's voice (battle orders and selection)", 0) {
            self.unit_voice(&lib, requests);
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

    /// A unit's order voices: its culture and actors (from the DB), the actor's lines with Play, Add a take and select, the unit's
    /// selection lines first, and a new selection line for the unit (under every actor of its culture and family, its DB column
    /// set to it).
    fn unit_voice(&mut self, lib: &Arc<wce_audio::library::SoundLibrary>, requests: &mut Vec<Request>) {
        let Some(data) = self.game_data.clone() else {
            gui::text_disabled("Reading the game's tables...");
            return;
        };
        let units = &data.db.units;
        if units.is_empty() {
            gui::text_disabled("No land units read (the game's DB schema is not set?).");
            return;
        }
        let cultures = self.recipes_view.cultures.get_or_insert_with(|| Arc::new(unit_voices::cultures(&self.edits, lib))).clone();
        if let Some(key) = self.pending_unit.take() {
            self.recipes_view.unit = units.iter().position(|u| u.land_unit == key.trim());
            if let Some(u) = self.recipes_view.unit.map(|i| &units[i]) {
                self.recipes_view.new_line = format!("selected_{}", u.land_unit);
                self.recipes_view.actor = unit_voices::culture_of(lib, &cultures, &u.voiceover)
                    .and_then(|c| cultures.iter().find(|x| x.culture == c))
                    .and_then(|c| unit_voices::family_actors(lib, c, unit_voices::mounted(&u.category)).first().map(|(_, s)| *s));
            }
        }
        gui::text_wrapped(
            "Order lines: the culture (main_units.voiceover) picks the voice set, the game gives each unit one of its culture's actors \
             (foot: Inf, mounted: Mtd), and land_units.selection_vo / selected_vo_secondary / selected_vo_tertiary name its selection lines.",
        );
        gui::set_next_item_width(-1.0);
        gui::input_text("##units", Some("Filter units (e.g. comitatenses, cavalry)"), &mut self.recipes_view.unit_filter, 128, 0);
        let filter = self.recipes_view.unit_filter.to_lowercase();
        let shown: Vec<usize> = (0..units.len()).filter(|i| filter.is_empty() || units[*i].land_unit.contains(&filter)).collect();
        if gui::begin_child("unit list", v2(0.0, 110.0), gui::CHILD_BORDERS, 0) {
            gui::clipped(shown.len(), |row| {
                let u = &units[shown[row]];
                let text = format!("{}  ({}, {})##u{}", u.land_unit, u.voiceover, u.category, shown[row]);
                if gui::selectable(&text, self.recipes_view.unit == Some(shown[row]), 0) {
                    self.recipes_view.unit = Some(shown[row]);
                    self.recipes_view.actor = None;
                    self.recipes_view.new_line = format!("selected_{}", u.land_unit);
                }
            });
        }
        gui::end_child();

        let Some(unit) = self.recipes_view.unit.and_then(|i| units.get(i)) else { return };
        let mounted = unit_voices::mounted(&unit.category);
        let found = unit_voices::culture_of(lib, &cultures, &unit.voiceover);
        if found.is_none() {
            let sets: Vec<String> = cultures.iter().map(|c| lib.label(c.culture, 0)).collect();
            gui::text_colored(
                [1.0, 0.6, 0.3, 1.0],
                &format!("The voice-over {} names no order voice set ({}); pick the one to look at:", unit.voiceover, sets.join(", ")),
            );
            for c in cultures.iter() {
                if gui::radio_button(&lib.label(c.culture, 0), self.recipes_view.set_override == Some(c.culture)) {
                    self.recipes_view.set_override = Some(c.culture);
                    self.recipes_view.actor = None;
                }
                gui::same_line();
            }
            gui::new_line();
        }
        let Some(culture) = found.or(self.recipes_view.set_override).and_then(|c| cultures.iter().find(|x| x.culture == c)) else {
            return;
        };
        let actors = unit_voices::family_actors(lib, culture, mounted);
        gui::text_wrapped(&format!(
            "{}: voice set {}{}, {} actors ({})",
            unit.land_unit,
            lib.label(culture.culture, 0),
            if found.is_some_and(|f| f == culture.culture)
                && lib.label(culture.culture, 0).to_lowercase() != unit.voiceover.split('_').next().unwrap_or("")
            {
                " (the default: no set is named for this voice-over)"
            } else {
                ""
            },
            actors.len(),
            if mounted { "mounted" } else { "foot" }
        ));
        for (i, column) in SELECTION_COLUMNS.iter().enumerate() {
            gui::text_disabled(&format!("{column}:"));
            gui::same_line();
            gui::text(if unit.selection[i].is_empty() { "(none)" } else { &unit.selection[i] });
        }
        for (actor, switch) in &actors {
            if gui::small_button(&format!("{}##a{switch}", lib.label(*actor, 0))) {
                self.recipes_view.actor = Some(*switch);
            }
            gui::same_line();
        }
        gui::new_line();

        // The chosen actor's lines: the unit's selection lines first, then the others matching the filter.
        if let Some(switch) = self.recipes_view.actor.filter(|s| actors.iter().any(|(_, x)| x == s)) {
            let lines = unit_voices::lines(&self.edits, lib, switch);
            gui::set_next_item_width(-1.0);
            gui::input_text("##lines", Some("Filter lines (e.g. attack, halt, selected_)"), &mut self.recipes_view.line_filter, 128, 0);
            let filter = self.recipes_view.line_filter.to_lowercase();
            let own: Vec<u32> = unit.selection.iter().filter(|s| !s.is_empty()).map(|s| wce_audio_format::fnv::id(s)).collect();
            let mut shown: Vec<(u32, u32)> = lines
                .iter()
                .copied()
                .filter(|(v, _)| own.contains(v) || filter.is_empty() || lib.label(*v, 0).to_lowercase().contains(&filter))
                .collect();
            shown.sort_by_key(|(v, _)| !own.contains(v));
            if gui::begin_child("actor lines", v2(0.0, 180.0), gui::CHILD_BORDERS, 0) {
                gui::clipped(shown.len(), |row| {
                    let (value, line) = shown[row];
                    gui::push_id(&format!("ol{line}"));
                    if self.play.button(&format!("ol{switch}.{line}"), requests) {
                        requests.push(Request::PlayObject(line));
                    }
                    gui::same_line();
                    if matches!(self.edits.object(lib, line), Some(Object::RandomSequence(_))) && gui::small_button("Add a take...") {
                        requests.push(Request::AddVariation(line));
                    }
                    gui::same_line();
                    let takes = self.edits.object(lib, line).map_or(0, |o| o.children().len());
                    let mark = if own.contains(&value) { "  (this unit's)" } else { "" };
                    if gui::selectable(&format!("{}  ({takes} takes){mark}", lib.label(value, 0)), self.selected == Some(line), 0) {
                        requests.push(Request::Select(line));
                    }
                    gui::pop_id();
                });
            }
            gui::end_child();
        } else {
            gui::text_disabled("Pick an actor to hear and edit its lines.");
        }

        // A new selection line for this unit: all three columns (the game picks among them when the unit is selected: tested in
        // game, 2026-11-20, a line set in selection_vo alone plays only part of the time), or one of them.
        gui::separator_text("A new selection line for this unit");
        if gui::radio_button("all three (always this line)", self.recipes_view.column == 0) {
            self.recipes_view.column = 0;
        }
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The game picks among selection_vo, selected_vo_secondary and selected_vo_tertiary each time the unit is selected.",
            );
        }
        for (i, column) in SELECTION_COLUMNS.iter().enumerate() {
            gui::same_line();
            if gui::radio_button(column, self.recipes_view.column == i + 1) {
                self.recipes_view.column = i + 1;
            }
        }
        gui::set_next_item_width(300.0);
        gui::input_text("##newline", Some("the line's name (letters, digits, _)"), &mut self.recipes_view.new_line, 128, 0);
        let columns: Vec<&'static str> =
            if self.recipes_view.column == 0 { SELECTION_COLUMNS.to_vec() } else { vec![SELECTION_COLUMNS[self.recipes_view.column - 1]] };
        let column = columns.join(", ");
        let like = [unit.selection[self.recipes_view.column.saturating_sub(1)].as_str(), unit.selection[0].as_str(), "selected_generic"]
            .into_iter()
            .find(|s| !s.is_empty())
            .map_or(0, wce_audio_format::fnv::id);
        let name = self.recipes_view.new_line.trim().to_lowercase();
        gui::begin_disabled(name.is_empty());
        if gui::button("Pick a sound file and add it...") {
            requests.push(Request::UnitLine(UnitLine {
                switches: actors.iter().map(|(_, s)| *s).collect(),
                name: name.clone(),
                like,
                unit: Some((unit.land_unit.clone(), columns.clone())),
            }));
        }
        gui::end_disabled();
        if gui::is_item_hovered() {
            gui::set_tooltip(&format!(
                "Adds the line {name} under the {} {} actors of {} (one take each, the same sound file), set up like the unit's\ncurrent line, and sets {}'s {column} to it (a land_units row written into db/land_units_tables/wce_audio).",
                actors.len(),
                if mounted { "mounted" } else { "foot" },
                lib.label(culture.culture, 0),
                unit.land_unit
            ));
        }
    }
}
