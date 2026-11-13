//! The selected building spline (`wce_tile::building_spline`) in the Inspector, below the grid: its lists of buildings and of joiners
//! (remove, move up and down), a search to add one to each, and the keys of the buildings selected in the scene added in one click.

use wce_editor::editor::Editor;
use wce_editor::{gui, icons};
use wce_scene::render_scene::DocumentId;
use wce_tile::building_spline::{JOINERS, KEYS, keys_text, parse_keys};
use wce_tile_editor::tile_document::{BUILDING_SPLINE, TileDocument};

use crate::tool::{TileTool, primary, selected};

/// How many matches of a search are listed.
const MATCHES: usize = 50;

/// The searches' state (the buildings', the joiners'), and the building keys they search (read once a tile is open).
#[derive(Default)]
pub(crate) struct BuildingSplineUi {
    search: [String; 2],
    catalogue: Option<(DocumentId, Vec<String>)>,
}

/// One list of keys: its rows, the button adding the selected buildings, its search. Whether it changed.
fn key_list(list: &mut Vec<String>, picked: &[String], search: &mut String, all: &[String], id: &str, empty: &str) -> bool {
    let mut changed = false;
    if list.is_empty() {
        gui::text_disabled(empty);
    }

    let mut remove = None;
    let mut swap = None;
    for (i, key) in list.iter().enumerate() {
        gui::push_id(&format!("{id}{i}"));
        if gui::small_button(icons::X) {
            remove = Some(i);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Take it off the list");
        }

        gui::same_line();
        gui::begin_disabled(i == 0);
        if gui::small_button(icons::ARROW_BIG_UP) {
            swap = Some((i - 1, i));
        }

        gui::end_disabled();
        gui::same_line();
        gui::begin_disabled(i + 1 == list.len());
        if gui::small_button(icons::ARROW_BIG_DOWN) {
            swap = Some((i, i + 1));
        }

        gui::end_disabled();
        gui::same_line();
        gui::text(key);
        gui::pop_id();
    }

    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }

    if let Some((a, b)) = swap {
        list.swap(a, b);
        changed = true;
    }

    let picked = picked.iter().filter(|k| !list.contains(k)).cloned().collect::<Vec<_>>();
    if !picked.is_empty() {
        if gui::button(&format!("{} Add the {} selected building(s)##{id}", icons::PLUS, picked.len())) {
            list.extend(picked.iter().cloned());
            changed = true;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(&picked.join(", "));
        }
    }

    gui::set_next_item_width(-1.0);
    gui::input_text(&format!("##search{id}"), Some("Search buildings to add..."), search, 256, 0);
    let words = search.to_lowercase().split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    if !words.is_empty() {
        let matches = all.iter().filter(|k| words.iter().all(|w| k.to_lowercase().contains(w.as_str()))).take(MATCHES).collect::<Vec<_>>();
        if matches.is_empty() {
            gui::text_disabled("No building matches.");
        } else {
            if gui::begin_child(&format!("##matches{id}"), gui::v2(0.0, 160.0), gui::CHILD_BORDERS, 0) {
                for k in matches {
                    let on = list.contains(k);
                    if gui::selectable(&format!("{k}##m{id}"), on, 0) && !on {
                        list.push(k.clone());
                        changed = true;
                    }
                }
            }

            gui::end_child();
        }
    }

    changed
}

impl TileTool {
    /// The Inspector below the grid: the selected building spline's buildings and joiners.
    pub(crate) fn inspect_building_spline(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(elem) = primary(e) else { return };
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let t = d.tile();
        if t.tscene().type_of(elem) != BUILDING_SPLINE {
            return;
        }

        let get = |attr: &str| parse_keys(&t.tscene().doc().get(elem, attr).unwrap_or_default());
        let (mut keys, mut joiners) = (get(KEYS), get(JOINERS));
        let laid = t.building_spline_count(Some(elem));

        // The keys of the buildings selected besides the spline.
        let mut picked = selected(e)
            .into_iter()
            .filter(|&el| t.tscene().type_of(el) == "BUILDING_INSTANCE")
            .filter_map(|el| t.tscene().doc().get(el, "key"))
            .filter(|k| !k.is_empty())
            .collect::<Vec<_>>();
        picked.sort();
        picked.dedup();

        if self.bspline_ui.catalogue.as_ref().is_none_or(|(d, _)| *d != id) {
            let mut all = t.key_catalogue("BUILDING_INSTANCE");
            all.sort();
            all.dedup();
            self.bspline_ui.catalogue = Some((id, all));
        }

        let ui = &mut self.bspline_ui;
        let all = ui.catalogue.as_ref().map_or(&[][..], |(_, a)| a.as_slice());
        let [search_keys, search_joiners] = &mut ui.search;
        gui::separator_text("Buildings along the line");
        let keys_changed =
            key_list(&mut keys, &picked, search_keys, all, "b", "None yet: add one below. The line lays nothing without buildings.");
        gui::separator_text("Joiners (optional)");
        let joiners_changed = key_list(
            &mut joiners,
            &picked,
            search_joiners,
            all,
            "j",
            "None: the buildings meet with nothing between them. A joiner (a post, a pillar) goes on each joint.",
        );

        gui::push_style_color(gui::COL_TEXT, gui::style_color(gui::COL_TEXT_DISABLED));
        gui::text_wrapped(&format!(
            "{laid} building(s) laid, joiners included. Each one goes with its long side along the line, as long as its model (its logic \
             file's box), the gap between them; a joiner in the middle of each gap. Written as plain buildings in the layer '{}' when the \
             tile is exported.",
            wce_tile_editor::tile_document::building_splines::KIT_LAYER
        ));
        gui::pop_style_color(1);

        if let Some(d) = e.docs.find_as_mut::<TileDocument>(id) {
            if keys_changed {
                d.set_attribute(elem, KEYS, &keys_text(&keys));
            }

            if joiners_changed {
                d.set_attribute(elem, JOINERS, &keys_text(&joiners));
            }
        }
    }
}
