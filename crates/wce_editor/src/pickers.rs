//! Pickers the tools share: a combo with a filter over a long list (units, factions, slots, files of the packs).

use crate::gui;

/// Whether `item` has every word of `filter` (lower case, space separated) in it, any case.
pub fn matches(item: &str, words: &[String]) -> bool {
    let item = item.to_lowercase();
    words.iter().all(|w| item.contains(w.as_str()))
}

/// A combo listing `items` with a filter box at its top (the words of `filter`, in any order and case): the item picked, if one was.
/// `value` is what it shows closed, and the item marked in the list.
pub fn search_combo<S: AsRef<str>>(label: &str, tip: &str, filter: &mut String, value: &str, items: &[S]) -> Option<String> {
    let mut picked = None;
    let open = gui::begin_combo(label, value);
    if !tip.is_empty() && gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    if open {
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }

        gui::input_text(&format!("##{label}_filter"), Some("filter"), filter, 128, 0);
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let shown: Vec<&str> = items.iter().map(AsRef::as_ref).filter(|i| matches(i, &words)).collect();
        gui::clipped(shown.len(), |k| {
            if gui::selectable(shown[k], shown[k] == value, 0) {
                picked = Some(shown[k].to_owned());
            }
        });

        gui::end_combo();
    }

    picked
}

/// A small button that opens a filtered list of `items` (the long lists a combo is too narrow for: files of the packs): the item picked.
pub fn pick_button<S: AsRef<str>>(id: &str, tip: &str, filter: &mut String, current: &str, items: &[S]) -> Option<String> {
    let popup = format!("##pick_popup{id}");
    if gui::small_button(&format!("...##{id}")) {
        gui::open_popup(&popup);
    }

    if !tip.is_empty() && gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    let mut picked = None;
    if gui::begin_popup(&popup) {
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }

        gui::set_next_item_width(560.0);
        gui::input_text(&format!("##{id}_filter"), Some("filter: words in any order"), filter, 128, 0);
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let shown: Vec<&str> = items.iter().map(AsRef::as_ref).filter(|i| matches(i, &words)).collect();
        gui::text_colored(crate::theme::dim(), &format!("{} of {}", shown.len(), items.len()));
        if gui::begin_child(&format!("##{id}_list"), gui::v2(560.0, 320.0), 0, 0) {
            gui::clipped(shown.len(), |k| {
                if gui::selectable(shown[k], shown[k].eq_ignore_ascii_case(current), 0) {
                    picked = Some(shown[k].to_owned());
                    gui::close_current_popup();
                }
            });
        }

        gui::end_child();
        gui::end_popup();
    }

    picked
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn every_word_in_any_order_and_case() {
        let words = |f: &str| f.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
        assert!(matches("att_rom_Legio_comitatenses", &words("COMIT rom")));
        assert!(!matches("att_rom_legio", &words("rom horse")));
        assert!(matches("anything", &words("")));
    }
}
