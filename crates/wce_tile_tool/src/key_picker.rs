//! A combo of every valid key of a type (`TileDocument::key_catalogue`: buildings, trees, decals, models, effects), with a filter: the
//! list can hold tens of thousands of keys, so only the rows on screen are drawn.

use std::sync::Arc;

use wce_editor::{gui, icons};

/// The filter of one picker and the keys it let through (made again when the filter or the list changes).
#[derive(Default)]
pub struct KeyPicker {
    filter: String,
    shown: Vec<usize>,
    shown_for: Option<(String, usize)>,
}

impl KeyPicker {
    /// The combo `label` showing `value`; choosing a key sets it. Returns whether it changed.
    pub fn combo(&mut self, label: &str, value: &mut String, keys: &Arc<Vec<String>>, width: f32) -> bool {
        gui::set_next_item_width(width);
        let preview = if value.is_empty() { format!("(choose one of {})", keys.len()) } else { value.clone() };
        if !gui::begin_combo(label, &preview) {
            return false;
        }

        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }

        gui::set_next_item_width(-1.0);
        gui::input_text("##keyfilter", Some(&format!("{}  filter (words are and-ed)", icons::SEARCH)), &mut self.filter, 128, 0);
        let stamp = (self.filter.clone(), Arc::as_ptr(keys) as usize);
        if self.shown_for.as_ref() != Some(&stamp) {
            let words = self.filter.to_lowercase().split_whitespace().map(str::to_owned).collect::<Vec<_>>();
            self.shown = keys.iter().enumerate().filter(|(_, k)| {
                let k = k.to_lowercase();
                words.iter().all(|w| k.contains(w))
            }).map(|(i, _)| i).collect();
            self.shown_for = Some(stamp);
        }

        gui::text_disabled(&format!("{} of {}", self.shown.len(), keys.len()));
        let mut changed = false;

        // A key the files don't have (a mod's): as typed.
        let typed = self.filter.trim();
        if !typed.is_empty() && !keys.iter().any(|k| k == typed) && gui::selectable(&format!("use \"{typed}\" as typed"), false, 0) {
            *value = typed.to_owned();
            changed = true;
        }

        if gui::begin_child("##keys", gui::v2(width.max(360.0), 320.0), 0, 0) {
            gui::clipped(self.shown.len(), |row| {
                let key = &keys[self.shown[row]];
                if gui::selectable(key, key == value, 0) {
                    value.clone_from(key);
                    changed = true;
                }
            });
        }

        gui::end_child();
        if changed {
            gui::close_current_popup();
        }

        gui::end_combo();
        changed
    }
}
