//! A list of keys picked from the game's own (docs/plans/auto_detail.md): what a biome grows and a clutter rule lays when they are not a
//! set. The keys show as pictures (the Create panel's thumbnails) with their names, a key the game's files do not have framed red; a click
//! on one takes it out; the last cell opens a picker of the kind's keys (searched by name, buildings by category too) as a grid of
//! pictures, a click adding a key or taking it out. Typed keys can't name what does not exist.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use wce_editor::editor::Editor;
use wce_editor::images::ImageRef;
use wce_editor::{gui, icons, theme};

use crate::create_panel::Entry;
use crate::tool::{TileTool, document};

/// A cell's picture's side, pixels.
const CELL: f32 = 64.0;

/// The picker's window size.
const POPUP: gui::Vec2 = gui::Vec2 { x: 600.0, y: 480.0 };

/// The pickers' state (one popup open at a time).
#[derive(Default)]
pub(crate) struct KeyPicker {
    /// The search words and the building category shown (empty: all).
    filter: String,
    category: String,

    /// The keys of each kind with their building category ("" for trees), for the catalogue they were made from.
    keys: HashMap<String, (usize, Arc<Vec<(String, String)>>)>,

    /// The keys shown for the filter, by kind, filter and category.
    shown: Option<((String, String, String), Arc<Vec<usize>>)>,

    /// The pictures made, by `kind|key`, and the cells drawn without one this frame (asked for after).
    images: HashMap<String, ImageRef>,
    wanted: RefCell<Vec<(String, String)>>,

    /// A list whose picker a script asked to open (by its id), with the search.
    pub(crate) open_request: Option<(String, String)>,
}

/// A picture's id.
fn image_key(kind: &str, key: &str) -> String {
    format!("{kind}|{key}")
}

/// A key's name cut to a width.
fn cut(name: &str, width: f32) -> String {
    let mut s = name.to_owned();
    while s.chars().count() > 3 && gui::calc_text_width(&s) > width {
        s.pop();
        s.pop();
        s.push('…');
    }

    s
}

impl KeyPicker {
    /// A cell: the key's picture (an icon until there is one) over its name, framed by what it is (`lit`: chosen; `bad`: not in the
    /// game's files); whether it was clicked.
    fn cell(&self, id: &str, kind: &str, key: &str, lit: bool, bad: bool) -> bool {
        let line = gui::text_line_height_with_spacing();
        let at = gui::cursor_screen_pos();
        let clicked = gui::invisible_button(id, gui::v2(CELL, CELL + line), 0);
        let hovered = gui::is_item_hovered();
        let dl = gui::window_draw_list();
        let (pmin, pmax) = (at, gui::v2(at.x + CELL, at.y + CELL));
        match self.images.get(&image_key(kind, key)) {
            Some(img) => dl.image(img.id, pmin, pmax, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), 0xFFFF_FFFF),
            None => {
                if !bad && gui::is_item_visible() {
                    self.wanted.borrow_mut().push((kind.to_owned(), key.to_owned()));
                }

                dl.rect_filled(pmin, pmax, theme::u32(frame_bg()), 2.0);
                let icon = match kind {
                    "BUILDING_INSTANCE" => icons::BOX,
                    "ProjectedDecalInstance" => icons::STICKER,
                    _ => icons::TREES,
                };
                let w = gui::calc_text_width(icon);
                dl.text(gui::v2(at.x + (CELL - w) * 0.5, at.y + CELL * 0.5 - 8.0), theme::u32(theme::dim()), icon);
            }
        }

        let (frame, thick) = if bad {
            (theme::error(), 2.0)
        } else if lit {
            (accent(), 2.5)
        } else if hovered {
            (theme::text(), 1.5)
        } else {
            (theme::mix(frame_bg(), theme::text(), 0.2), 1.0)
        };
        dl.rect(pmin, pmax, theme::u32(frame), 2.0, thick);
        let colour = if bad { theme::error() } else { theme::text() };
        // A model's path (a decal's) by its file's name.
        let caption = key.rsplit(['\\', '/']).next().unwrap_or(key).trim_end_matches(".rigid_model_v2");
        dl.text(gui::v2(at.x, at.y + CELL + 1.0), theme::u32(colour), &cut(caption, CELL));
        clicked
    }
}

fn frame_bg() -> [f32; 4] {
    let c = gui::style_color(gui::COL_FRAME_BG);
    [c.x, c.y, c.z, c.w]
}

fn accent() -> [f32; 4] {
    let c = gui::style_color(gui::COL_CHECK_MARK);
    [c.x, c.y, c.z, c.w]
}

impl TileTool {
    /// The keys of a kind with their building categories, for the open tile.
    fn picker_keys(&mut self, e: &mut Editor, kind: &str) -> Arc<Vec<(String, String)>> {
        let catalogue = self.catalogue(e, kind);
        let stamp = Arc::as_ptr(&catalogue) as usize;
        if let Some((s, keys)) = self.key_picker.keys.get(kind)
            && *s == stamp
        {
            return keys.clone();
        }

        let d = document(e);
        let keys = Arc::new(
            catalogue
                .iter()
                .map(|k| {
                    let category = if kind == "BUILDING_INSTANCE" {
                        d.as_ref().and_then(|d| d.tile().building_category(k)).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    (k.clone(), category)
                })
                .collect::<Vec<_>>(),
        );
        self.key_picker.keys.insert(kind.to_owned(), (stamp, keys.clone()));
        self.key_picker.shown = None;
        keys
    }

    /// A list of keys of a kind (`BUILDING_INSTANCE` or `VEGETATION_INSTANCE`) as pictures across `width`, and the picker to add to it;
    /// `category`: the building category the picker shows first (empty: all). Whether the list changed.
    pub(crate) fn key_list(&mut self, e: &mut Editor, id: &str, kind: &str, keys: &mut Vec<String>, width: f32, category: &str) -> bool {
        let all = self.picker_keys(e, kind);
        let exists = |k: &str| all.iter().any(|(c, _)| c.eq_ignore_ascii_case(k));
        let mut changed = false;
        let spacing = gui::item_spacing().x;
        let across = (((width + spacing) / (CELL + spacing)) as usize).max(1);
        let x0 = gui::cursor_pos_x();
        let popup = format!("Pick keys##{id}");
        let (mut remove, mut open) = (None, false);
        gui::push_id(id);
        for i in 0..=keys.len() {
            if i % across != 0 {
                gui::same_line();
            } else if i > 0 {
                gui::set_cursor_pos_x(x0);
            }

            match keys.get(i) {
                Some(k) => {
                    let bad = !exists(k);
                    if self.key_picker.cell(&format!("##key{i}"), kind, k, false, bad) {
                        remove = Some(i);
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip(&if bad {
                            format!("{k}\nNot in the game's files: nothing is laid for it. Click to take it out.")
                        } else {
                            format!("{k}\nClick to take it out")
                        });
                    }
                }

                None => {
                    let line = gui::text_line_height_with_spacing();
                    let asked = self.key_picker.open_request.as_ref().is_some_and(|(r, _)| r == id);
                    if gui::button_sized(&format!("{}##add", icons::PLUS), gui::v2(CELL, CELL + line)) || asked {
                        let request = self.key_picker.open_request.take().filter(|(r, _)| r == id);
                        self.key_picker.filter = request.map(|r| r.1).unwrap_or_default();
                        self.key_picker.category = category.to_owned();
                        open = true;
                    }

                    if gui::is_item_hovered() {
                        gui::set_tooltip("Pick keys from the game's");
                    }
                }
            }
        }

        gui::pop_id();

        // Opened where it is begun (outside the cells' id scope).
        if open {
            gui::open_popup(&popup);
        }

        if let Some(i) = remove {
            keys.remove(i);
            changed = true;
        }

        changed |= self.key_popup(&popup, kind, keys, &all);
        self.make_key_pictures(e);
        changed
    }

    /// The picker: a search, the building categories, the kind's keys as a grid of pictures (the chosen lit); a click adds a key or takes
    /// it out. Whether the list changed.
    fn key_popup(&mut self, popup: &str, kind: &str, keys: &mut Vec<String>, all: &Arc<Vec<(String, String)>>) -> bool {
        gui::set_next_window_size(POPUP, gui::COND_ALWAYS);
        if !gui::begin_popup(popup) {
            return false;
        }

        let mut changed = false;
        let p = &mut self.key_picker;
        let buildings = kind == "BUILDING_INSTANCE";
        let combo_w = if buildings { 180.0 } else { 0.0 };
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }

        gui::set_next_item_width(gui::content_region_avail().x - combo_w - if buildings { gui::item_spacing().x } else { 0.0 });
        gui::input_text("##search", Some("search the keys"), &mut p.filter, 256, 0);
        if buildings {
            gui::same_line();
            gui::set_next_item_width(combo_w);
            let shown = if p.category.is_empty() { "every category".to_owned() } else { p.category.clone() };
            if gui::begin_combo("##category", &shown) {
                if gui::selectable("every category", p.category.is_empty(), 0) {
                    p.category.clear();
                }

                let mut cats = all.iter().map(|(_, c)| c.as_str()).filter(|c| !c.is_empty()).collect::<Vec<_>>();
                cats.sort_unstable();
                cats.dedup();
                for c in cats {
                    if gui::selectable(c, p.category == c, 0) {
                        p.category = c.to_owned();
                    }
                }

                gui::end_combo();
            }
        }

        // The keys passing the search and the category, kept until they change.
        let want = (kind.to_owned(), p.filter.to_lowercase(), p.category.clone());
        if p.shown.as_ref().is_none_or(|(w, _)| *w != want) {
            let words = want.1.split_whitespace().collect::<Vec<_>>();
            let rows = all
                .iter()
                .enumerate()
                .filter(|(_, (_, c))| p.category.is_empty() || *c == p.category)
                .filter(|(_, (k, c))| {
                    let text = format!("{} {}", k.to_lowercase(), c.to_lowercase());
                    words.iter().all(|w| text.contains(w))
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            p.shown = Some((want, Arc::new(rows)));
        }

        let rows = p.shown.as_ref().map(|s| s.1.clone()).unwrap_or_default();
        gui::text_disabled(&format!("{} of {} keys; a click adds one or takes it out", rows.len(), all.len()));
        gui::begin_child("##keygrid", gui::v2(0.0, 0.0), 0, 0);
        let spacing = gui::item_spacing().x;
        let across = (((gui::content_region_avail().x + spacing) / (CELL + spacing)) as usize).max(1);
        let mut toggle = None;
        let mut cell = |p: &KeyPicker, i: usize| {
            let (key, category) = &all[i];
            let lit = keys.iter().any(|k| k.eq_ignore_ascii_case(key));
            if p.cell(&format!("##pick{i}"), kind, key, lit, false) {
                toggle = Some(key.clone());
            }

            if gui::is_item_hovered() {
                let what = if lit { "chosen: click to take it out" } else { "click to add it" };
                gui::set_tooltip(&if category.is_empty() { format!("{key}\n{what}") } else { format!("{key} ({category})\n{what}") });
            }
        };

        if buildings {
            // Many: by lines in view only.
            gui::clipped(rows.len().div_ceil(across), |l| {
                for (n, &i) in rows[l * across..((l + 1) * across).min(rows.len())].iter().enumerate() {
                    if n > 0 {
                        gui::same_line();
                    }

                    cell(p, i);
                }
            });
        } else {
            // Trees and shrubs by their species (oak, acacia ...), each under its name.
            let species = |i: usize| all[i].0.split_once('.').map_or("", |s| s.0);
            let mut sorted = rows.to_vec();
            sorted.sort_by(|&a, &b| species(a).cmp(species(b)).then(all[a].0.cmp(&all[b].0)));
            for group in sorted.chunk_by(|&a, &b| species(a) == species(b)) {
                gui::separator_text(species(group[0]));
                for (n, &i) in group.iter().enumerate() {
                    if n % across != 0 {
                        gui::same_line();
                    }

                    cell(p, i);
                }
            }
        }
        gui::end_child();
        gui::end_popup();

        if let Some(key) = toggle {
            match keys.iter().position(|k| k.eq_ignore_ascii_case(&key)) {
                Some(i) => {
                    keys.remove(i);
                }
                None => keys.push(key),
            }

            changed = true;
        }

        changed
    }

    /// Asks for the pictures of the cells drawn without one (those in view).
    fn make_key_pictures(&mut self, e: &mut Editor) {
        let wanted = std::mem::take(&mut *self.key_picker.wanted.borrow_mut());
        for (kind, key) in wanted {
            let id = image_key(&kind, &key);
            if self.key_picker.images.contains_key(&id) {
                continue;
            }

            if let Some(img) = self.thumbnail(e, &Entry::placing(&kind, &key), false) {
                self.key_picker.images.insert(id, img);
            }
        }
    }
}
