//! What a layout looks like, as a list of things to paint in order. The canvas and the CLI's renderer both paint it.
//!
//! Positions are in layout pixels from the layout origin. What is drawn is one state per component (by default its default state):
//! its image uses (the pictures, cut into nine parts when they have margins) and its text. Behaviour (hover and click, animations,
//! scripts) is not shown.
//!
//! Visibility: the seventh flag of a component says whether it is visible when the layout loads (checked against the game's own
//! screens: tab pages, unit cards and timers that the game shows later all have it off). A hidden component hides what is below it.
//! Names that end in `template` are filled in by the game's code (list items, player slots) and are not drawn in place unless asked.

use std::collections::HashMap;

use crate::dom::{Element, Node};
use crate::layout::{Rect, place_images, state_size};
use crate::model::{Entry, EntryKind, UiModel, by_label, color_of};

#[derive(Debug, Clone, PartialEq)]
pub enum DrawKind {
    Image {
        /// The path as the layout spells it, normalised to lower case with `/`.
        path: String,
        tiled: bool,
        flip_x: bool,
        flip_y: bool,
        tint: [u8; 4],
        /// Fixed borders of the picture in its own pixels: top, right, bottom, left. Zero: the whole picture scales.
        margins: [f32; 4],
    },
    Text {
        text: String,
        color: [u8; 4],
        /// Font size in layout pixels.
        size: f32,
        /// 0 left, 1 centre, 2 right: centred when the first text value of the state is 4, else the third (checked on the Custom Battle screen).
        halign: i32,
        /// 0 top, 1 middle (the second text value is 4).
        valign: i32,
        font: String,
        /// The state's font category (`Default Font Category`: none).
        category: String,
        /// Space between the text's box and the component's edge: left, top, right, bottom.
        inset: [i32; 4],
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawItem {
    /// Index into `UiModel::entries()` of the component this belongs to.
    pub entry: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: DrawKind,
}

/// Which parts of the layout to draw, and in which state.
#[derive(Debug, Clone, Default)]
pub struct DrawOptions {
    /// Draw components the layout hides when it loads.
    pub show_hidden: bool,
    /// Draw components the game fills in at runtime (names ending in `template`).
    pub show_templates: bool,
    /// Where a component has a state of this name, draw that one (others keep their default state).
    pub state: Option<String>,
    /// A state chosen for one component (index into its states), by path.
    pub entry_state: HashMap<Vec<usize>, usize>,
    /// Visibility chosen for one component, by path (overrides everything above, children follow their parent).
    pub visibility: HashMap<Vec<usize>, bool>,
    /// Size of the screen the root is sized to (the game's `SizeToScreen`); docked components move with it. `None`: the layout's own size.
    pub screen: Option<(i32, i32)>,
    /// An animation caught at some moment (see `UiModel::pose_at`): positions, sizes, opacity, colour and states it gives components.
    pub pose: Option<std::rc::Rc<crate::anim::Pose>>,
}

pub fn normalize_image_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

fn int(e: &Element, name: &str, n: usize) -> i64 {
    e.elems().filter(|c| c.name == name).nth(n).and_then(|c| c.text().trim().parse().ok()).unwrap_or(0)
}

fn yes(e: &Element, n: usize) -> bool {
    e.elems().filter(|c| c.name == "yes" || c.name == "no").nth(n).is_some_and(|c| c.name == "yes")
}

/// True for a name the game's code fills in (`player_template`, `item_template`).
pub fn is_runtime_template(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with("template")
}

fn state_title(s: &Element) -> String {
    s.elems().find(|c| c.name == "s").map(|c| c.text()).unwrap_or_default()
}

/// The margins of an image use (top, right, bottom, left), from its last floats.
fn margins_of(u: &Element, version: u32) -> [f32; 4] {
    if version < 74 {
        return [0.0; 4];
    }

    let f: Vec<f32> = u.elems().filter(|c| c.name == "flt").filter_map(|c| c.text().trim().parse().ok()).collect();
    if version >= 92 {
        if f.len() >= 4 {
            let n = f.len();
            return [f[n - 4], f[n - 3], f[n - 2], f[n - 1]];
        }
    } else if f.len() >= 2 {
        let n = f.len();
        let (tb, lr) = (f[n - 2], f[n - 1]);
        return [tb, lr, tb, lr];
    }

    [0.0; 4]
}

/// One textured rectangle of a picture that is cut into parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

const MAX_PIECES: usize = 6000;

/// Cuts a picture of `iw` x `ih` pixels over the rectangle `x, y, w, h`. With margins (top, right, bottom, left) the four corners keep
/// their size and the edges and the middle stretch, or repeat when `tiled`; without margins the picture stretches, or repeats when `tiled`.
pub fn pieces(x: f32, y: f32, w: f32, h: f32, iw: f32, ih: f32, margins: [f32; 4], tiled: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    if w <= 0.0 || h <= 0.0 || iw <= 0.0 || ih <= 0.0 {
        return out;
    }

    // Source columns / rows and the destination ones.
    let clamp = |a: f32, b: f32, total: f32| {
        let (mut a, mut b) = (a.max(0.0), b.max(0.0));
        if a + b > total {
            let k = total / (a + b);
            a *= k;
            b *= k;
        }

        (a, b)
    };

    let (ml, mr) = clamp(margins[3], margins[1], iw);
    let (mt, mb) = clamp(margins[0], margins[2], ih);
    let (dl, dr) = clamp(ml, mr, w);
    let (dt, db) = clamp(mt, mb, h);
    let cols = [(0.0, ml, 0.0, dl, false), (ml, iw - mr, dl, w - dr, true), (iw - mr, iw, w - dr, w, false)];
    let rows = [(0.0, mt, 0.0, dt, false), (mt, ih - mb, dt, h - db, true), (ih - mb, ih, h - db, h, false)];
    for &(sy0, sy1, dy0, dy1, ystretch) in &rows {
        for &(sx0, sx1, dx0, dx1, xstretch) in &cols {
            let (sw, sh) = (sx1 - sx0, sy1 - sy0);
            let (dw, dh) = (dx1 - dx0, dy1 - dy0);
            if sw <= 0.0 || sh <= 0.0 || dw <= 0.0 || dh <= 0.0 {
                continue;
            }

            let (nx, ny) = (if tiled && xstretch { (dw / sw).ceil().max(1.0) } else { 1.0 }, if tiled && ystretch { (dh / sh).ceil().max(1.0) } else { 1.0 });
            if (nx * ny) as usize + out.len() > MAX_PIECES {
                out.push(Piece { x: x + dx0, y: y + dy0, w: dw, h: dh, u0: sx0 / iw, v0: sy0 / ih, u1: sx1 / iw, v1: sy1 / ih });
                continue;
            }

            let (tw, th) = (if nx > 1.0 { sw } else { dw }, if ny > 1.0 { sh } else { dh });
            for ty in 0..ny as i32 {
                for tx in 0..nx as i32 {
                    let (px, py) = (dx0 + tx as f32 * tw, dy0 + ty as f32 * th);
                    let (pw, ph) = (tw.min(dx1 - px), th.min(dy1 - py));
                    if pw <= 0.0 || ph <= 0.0 {
                        continue;
                    }

                    let (fu, fv) = (if nx > 1.0 { pw / sw } else { 1.0 }, if ny > 1.0 { ph / sh } else { 1.0 });
                    out.push(Piece { x: x + px, y: y + py, w: pw, h: ph, u0: sx0 / iw, v0: sy0 / ih, u1: (sx0 + sw * fu) / iw, v1: (sy0 + sh * fv) / ih });
                }
            }
        }
    }

    // Without margins and not tiled the loop above produced the single middle piece: the whole picture.
    out
}

impl UiModel {
    /// The state's nine patch flag: the fourth byte of the word that follows the text margins (from version 80).
    fn nine_patch(&self, state: &Element) -> bool {
        if self.version < 80 {
            return false;
        }

        let Some(i) = by_label(state, "bottom") else { return false };
        match state.children.get(i + 1) {
            Some(Node::Elem(c)) if c.name == "i" => c.text().trim().parse::<i64>().map(|v| (v as u32) >> 24 != 0).unwrap_or(false),
            _ => false,
        }
    }

    /// Names of the states in the layout, with how many components have each, most common first.
    pub fn state_names(&self) -> Vec<(String, usize)> {
        let mut m: HashMap<String, usize> = HashMap::new();
        for en in self.entries().iter().filter(|e| e.kind == EntryKind::Entry) {
            if let Some(states) = self.dom.at(&en.path).and_then(|el| el.find("states")) {
                for s in states.elems().filter(|s| s.name == "state") {
                    *m.entry(state_title(s)).or_default() += 1;
                }
            }
        }

        let mut v: Vec<(String, usize)> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// The names of a component's states, in file order.
    pub fn entry_state_names(&self, path: &[usize]) -> Vec<String> {
        self.dom.at(path).and_then(|el| el.find("states")).map(|st| st.elems().filter(|s| s.name == "state").map(state_title).collect()).unwrap_or_default()
    }

    /// Index of the component's default state among its states.
    pub fn default_state_index(&self, path: &[usize]) -> usize {
        let Some(el) = self.dom.at(path) else { return 0 };
        let Some(d) = self.default_state(el) else { return 0 };
        el.find("states").and_then(|st| st.elems().filter(|s| s.name == "state").position(|s| std::ptr::eq(s, d))).unwrap_or(0)
    }

    /// Per component: is it drawn (own visibility and every parent's).
    pub fn shown(&self, entries: &[Entry], o: &DrawOptions) -> Vec<bool> {
        let mut out: Vec<bool> = Vec::with_capacity(entries.len());
        for en in entries {
            let parent = en.parent.map(|p| out[p]).unwrap_or(true);
            let own = match o.visibility.get(&en.path) {
                Some(v) => *v,
                None => o.show_hidden || ((en.visible || o.show_hidden) && (o.show_templates || !is_runtime_template(&en.name))),
            };
            out.push(parent && own);
        }

        out
    }

    pub fn draw_list(&self) -> Vec<DrawItem> {
        self.draw_list_with(&DrawOptions::default())
    }

    pub fn draw_list_with(&self, o: &DrawOptions) -> Vec<DrawItem> {
        let entries = self.entries();
        let shown = self.shown(&entries, o);
        let rects = self.geometry(&entries, o);
        let mut out = Vec::new();
        // colour and opacity an animation gives a component, passed on to its descendants when it propagates
        type Fx = (Option<[u8; 3]>, Option<f32>);
        let mut pass: Vec<Option<Fx>> = Vec::with_capacity(entries.len());
        for (ei, en) in entries.iter().enumerate() {
            let inherited = en.parent.and_then(|p| pass[p]);
            let own = o.pose.as_ref().and_then(|p| p.by_entry.get(&en.path)).filter(|p| p.rgb.is_some() || p.opacity.is_some());
            let eff: Option<Fx> = match own {
                Some(p) => Some((p.rgb.or(inherited.and_then(|i| i.0)), p.opacity.or(inherited.and_then(|i| i.1)))),
                None => inherited,
            };

            pass.push(if own.is_some_and(|p| p.propagate) { eff } else { inherited });
            if en.kind != EntryKind::Entry || !shown[ei] {
                continue;
            }

            let Some(el) = self.dom.at(&en.path) else { continue };
            let from = out.len();
            self.draw_entry(ei, en, el, rects[ei], o, &mut out);
            if let Some((rgb, opacity)) = eff {
                for it in &mut out[from..] {
                    match &mut it.kind {
                        DrawKind::Image { tint, .. } => {
                            if let Some(c) = rgb {
                                tint[..3].copy_from_slice(&c);
                            }

                            if let Some(a) = opacity {
                                tint[3] = tint[3].min(a.round().clamp(0.0, 255.0) as u8);
                            }
                        }

                        DrawKind::Text { color, .. } => {
                            if let Some(c) = rgb {
                                color[..3].copy_from_slice(&c);
                            }

                            if let Some(a) = opacity {
                                color[3] = a.round().clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                }
            }
        }

        out
    }

    pub(crate) fn chosen_state<'a>(&self, el: &'a Element, en: &Entry, o: &DrawOptions) -> Option<&'a Element> {
        let states: Vec<&Element> = el.find("states")?.elems().filter(|s| s.name == "state").collect();
        if let Some(name) = o.pose.as_ref().and_then(|p| p.by_entry.get(&en.path)).and_then(|p| p.state.as_ref()) {
            if let Some(s) = states.iter().find(|s| state_title(s) == *name) {
                return Some(s);
            }
        }

        if let Some(i) = o.entry_state.get(&en.path) {
            if let Some(s) = states.get(*i) {
                return Some(s);
            }
        }

        if let Some(name) = &o.state {
            if let Some(s) = states.iter().find(|s| state_title(s) == *name) {
                return Some(s);
            }
        }

        self.default_state(el)
    }

    fn draw_entry(&self, ei: usize, en: &Entry, el: &Element, rect: Rect, o: &DrawOptions, out: &mut Vec<DrawItem>) {
        let Some(state) = self.chosen_state(el, en, o) else { return };
        let images: Vec<(u32, String)> = el
            .find("images")
            .map(|a| {
                a.elems()
                    .filter(|i| i.name == "image")
                    .map(|i| (int(i, "u", 0) as u32, i.elems().find(|c| c.name == "s").map(|c| c.text()).unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();

        if let Some(uses) = state.find("image_uses") {
            let list: Vec<&Element> = uses.elems().filter(|u| u.name == "image_use").collect();
            let design = state_size(state);
            let placed = place_images(&list, self.version, design, (rect.w, rect.h), self.nine_patch(state));
            for (u, p) in list.iter().zip(placed) {
                let id = int(u, "u", 0) as u32;
                let Some((_, path)) = images.iter().find(|(i, _)| *i == id) else { continue };
                if path.is_empty() {
                    continue;
                }

                let tint = u.find("color").map(color_of).unwrap_or([255; 4]);
                out.push(DrawItem {
                    entry: ei,
                    x: rect.x + p.x,
                    y: rect.y + p.y,
                    w: p.w,
                    h: p.h,
                    kind: DrawKind::Image { path: normalize_image_path(path), tiled: yes(u, 0), flip_x: yes(u, 1), flip_y: yes(u, 2), tint, margins: margins_of(u, self.version) },
                });
            }
        }

        let text = state.elems().find(|c| c.name == "unicode").map(|c| c.text()).unwrap_or_default();
        if !text.is_empty() {
            let size = by_label(state, "font size").and_then(|i| if let Some(Node::Elem(c)) = state.children.get(i) { c.text().trim().parse::<f32>().ok() } else { None }).unwrap_or(10.0);
            let strings: Vec<String> = state.elems().filter(|c| c.name == "s").map(|c| c.text()).collect();
            let font = by_label(state, "font").and_then(|i| if let Some(Node::Elem(c)) = state.children.get(i) { Some(c.text()) } else { None }).unwrap_or_else(|| strings.get(1).cloned().unwrap_or_default());
            let category = by_label(state, "font category").and_then(|i| if let Some(Node::Elem(c)) = state.children.get(i) { Some(c.text()) } else { None }).unwrap_or_default();
            let margin = |label: &str| by_label(state, label).and_then(|i| if let Some(Node::Elem(c)) = state.children.get(i) { c.text().trim().parse::<i32>().ok() } else { None }).unwrap_or(0);
            out.push(DrawItem {
                entry: ei,
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                kind: DrawKind::Text {
                    text,
                    color: state.find("color").map(color_of).unwrap_or([255; 4]),
                    size: if size < 4.0 { 10.0 } else { size },
                    // the game: right and centre are 3 and 4 of the horizontal value, bottom and centre 1 and 4 of the vertical; anything else is left / top
                    halign: match int(state, "i", 2) {
                        3 => 2,
                        4 => 1,
                        _ => 0,
                    },
                    valign: match int(state, "i", 3) {
                        1 => 2,
                        4 => 1,
                        _ => 0,
                    },
                    font,
                    category,
                    inset: [margin("left"), margin("top"), margin("right"), margin("bottom")],
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_picture_is_one_piece() {
        let p = pieces(10.0, 20.0, 100.0, 50.0, 64.0, 64.0, [0.0; 4], false);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].x, p[0].y, p[0].w, p[0].h, p[0].u1, p[0].v1), (10.0, 20.0, 100.0, 50.0, 1.0, 1.0));
    }

    #[test]
    fn caps_keep_their_size_and_the_middle_repeats() {
        // the title plaque: 219 wide picture, 52 pixel caps, over 414 pixels, tiled
        let p = pieces(0.0, 0.0, 414.0, 50.0, 219.0, 50.0, [0.0, 52.0, 0.0, 52.0], true);
        assert_eq!((p[0].x, p[0].w), (0.0, 52.0));
        let last = p.last().unwrap();
        assert_eq!((last.x, last.w), (362.0, 52.0));
        // 310 pixels of middle from a 115 pixel piece: 2 full tiles and a part
        assert_eq!(p.len(), 5);
        assert!((p[3].w - 80.0).abs() < 1e-3 && (p[3].u1 - (52.0 + 80.0) / 219.0).abs() < 1e-4);
    }

    #[test]
    fn stretched_middle_is_one_piece() {
        let p = pieces(0.0, 0.0, 300.0, 40.0, 100.0, 40.0, [0.0, 10.0, 0.0, 10.0], false);
        assert_eq!(p.len(), 3);
        assert_eq!((p[1].x, p[1].w), (10.0, 280.0));
    }
}
