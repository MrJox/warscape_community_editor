//! The styled controls of the texture rules' and the biomes' windows (docs/plans/auto_detail.md): a label column and a value column that
//! fill the window's width and keep their places whatever is switched on, segmented choices (one of a few), check boxes over a row's
//! width, a range bar (a condition's ends as handles over the weight it gives, its fades drawn as the ramps they are) and texture
//! swatches. Drawn with the window's draw list over invisible buttons.

use wce_editor::{gui, theme};
use wce_tile::texture_rules::Range;

/// Space between segments and between swatches, pixels.
const GAP: f32 = 3.0;

/// The rounding of a chip, pixels.
const ROUNDING: f32 = 2.0;

/// The label column's width: a third of the window, 90 to 150 pixels. Asked at the start of a line.
fn label_width() -> f32 {
    (gui::content_region_avail().x * 0.32).clamp(90.0, 150.0)
}

/// A row's place: where its value column starts (window x) and how wide it is.
#[derive(Clone, Copy)]
pub struct Row {
    pub x: f32,
    pub width: f32,
}

impl Row {
    /// The cursor to the value column of a new line (a second line of the same row).
    pub fn next_line(self) {
        gui::set_cursor_pos_x(self.x);
    }
}

/// A row with a label: the label (with its tooltip) in the label column, the cursor then in the value column.
pub fn row(label: &str, tip: &str) -> Row {
    let start = gui::cursor_pos_x();
    let (lw, avail) = (label_width(), gui::content_region_avail().x);
    gui::align_text_to_frame_padding();
    gui::text_colored(theme::dim(), label);
    if !tip.is_empty() && gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    gui::same_line_at(start + lw);
    Row { x: start + lw, width: (avail - lw).max(60.0) }
}

/// A row whose label is a check box (on or off); whether it was clicked.
pub fn toggle_row(id: &str, label: &str, on: &mut bool, tip: &str) -> (Row, bool) {
    let start = gui::cursor_pos_x();
    let (lw, avail) = (label_width(), gui::content_region_avail().x);
    let clicked = toggle_chip(id, label, *on, lw - gui::item_spacing().x, tip);
    if clicked {
        *on = !*on;
    }

    gui::same_line_at(start + lw);
    (Row { x: start + lw, width: (avail - lw).max(60.0) }, clicked)
}

fn frame_bg() -> [f32; 4] {
    let c = gui::style_color(gui::COL_FRAME_BG);
    [c.x, c.y, c.z, c.w]
}

/// The editor's accent: the colour of its check marks and sliders' grabs.
fn accent() -> [f32; 4] {
    let c = gui::style_color(gui::COL_CHECK_MARK);
    [c.x, c.y, c.z, c.w]
}

/// A chip's colours: its fill and its text, lit or not, hovered or held.
fn chip_colours(on: bool, hovered: bool, held: bool) -> ([f32; 4], [f32; 4]) {
    let base = frame_bg();
    let fill = match (on, hovered || held) {
        (true, false) => theme::mix(base, accent(), 0.55),
        (true, true) => theme::mix(base, accent(), 0.7),
        (false, false) => base,
        (false, true) => theme::mix(base, theme::text(), 0.12),
    };
    (fill, theme::text())
}

/// Text centred in a box, clipped to it.
fn centred_text(dl: gui::DrawList, min: gui::Vec2, max: gui::Vec2, colour: [f32; 4], text: &str) {
    let w = gui::calc_text_width(text);
    let h = gui::text_line_height();
    let x = (min.x + (max.x - min.x - w) * 0.5).max(min.x + 4.0);
    let y = min.y + (max.y - min.y - h) * 0.5;
    dl.push_clip_rect(min, max);
    dl.text(gui::v2(x, y), theme::u32(colour), text);
    dl.pop_clip_rect();
}

/// A toggle: a check box and its label over `width` (all of it clickable, lit under the mouse), as the editor's check boxes look;
/// true when clicked.
pub fn toggle_chip(id: &str, label: &str, on: bool, width: f32, tip: &str) -> bool {
    let h = gui::frame_height();
    let at = gui::cursor_screen_pos();
    let clicked = gui::invisible_button(&format!("##{id}"), gui::v2(width.max(h), h), 0);
    let (hovered, held) = (gui::is_item_hovered(), gui::is_item_active());
    if hovered && !tip.is_empty() {
        gui::set_tooltip(tip);
    }

    let dl = gui::window_draw_list();
    let max = gui::v2(at.x + width.max(h), at.y + h);
    if hovered || held {
        dl.rect_filled(at, max, theme::u32(theme::mix(theme::palette_background(), theme::text(), 0.06)), ROUNDING);
    }

    // The box, the frame's colour as a check box's; its mark in the accent.
    let (b0, b1) = (gui::v2(at.x + 2.0, at.y + 2.0), gui::v2(at.x + h - 2.0, at.y + h - 2.0));
    let (fill, _) = chip_colours(false, hovered, held);
    dl.rect_filled(b0, b1, theme::u32(fill), ROUNDING);
    if on {
        let s = b1.x - b0.x;
        let mark = [
            gui::v2(b0.x + s * 0.22, b0.y + s * 0.52),
            gui::v2(b0.x + s * 0.42, b0.y + s * 0.72),
            gui::v2(b0.x + s * 0.78, b0.y + s * 0.3),
        ];
        dl.polyline(&mark, theme::u32(accent()), false, (s * 0.12).max(1.5));
    }

    dl.push_clip_rect(at, max);
    dl.text(gui::v2(at.x + h + 4.0, at.y + (h - gui::text_line_height()) * 0.5), theme::u32(theme::text()), label);
    dl.pop_clip_rect();
    clicked
}

/// Segmented chips: one of `options`, `current` lit (none: none is), over `width`; the one clicked.
pub fn segmented(id: &str, options: &[&str], current: Option<usize>, width: f32) -> Option<usize> {
    let n = options.len().max(1);
    let h = gui::frame_height();
    let w = ((width - GAP * (n - 1) as f32) / n as f32).max(12.0);
    let dl = gui::window_draw_list();
    let mut picked = None;
    gui::push_id(id);
    for (i, label) in options.iter().enumerate() {
        if i > 0 {
            gui::same_line_spacing(GAP);
        }

        let at = gui::cursor_screen_pos();
        if gui::invisible_button(&format!("##s{i}"), gui::v2(w, h), 0) {
            picked = Some(i);
        }

        if gui::is_item_hovered() && gui::calc_text_width(label) > w - 8.0 {
            gui::set_tooltip(label);
        }

        let (fill, text) = chip_colours(current == Some(i), gui::is_item_hovered(), gui::is_item_active());
        let max = gui::v2(at.x + w, at.y + h);
        dl.rect_filled(at, max, theme::u32(fill), ROUNDING);
        centred_text(dl, at, max, text, label);
    }

    gui::pop_id();
    picked
}

/// How a range bar shows and edits its range.
pub struct Bar<'a> {
    /// The values across the bar, low to high.
    pub domain: (f32, f32),

    /// What a dragged value is rounded to.
    pub step: f32,
    pub unit: &'a str,

    /// Whether an end may be open (no limit): dragged onto the bar's end.
    pub open_ends: bool,

    /// Whether Shift + drag sets the fade.
    pub fade: bool,
}

/// What a range bar drag moves.
#[derive(Clone, Copy, PartialEq)]
enum Grip {
    Lo,
    Hi,
    FadeLo,
    FadeHi,
}

thread_local! {
    /// The range bar being dragged and what of it.
    static GRIP: std::cell::Cell<Option<(u32, Grip)>> = const { std::cell::Cell::new(None) };
}

/// A number for a bar's text: whole when the step is, else as many decimals as the step has.
pub fn number(v: f32, step: f32) -> String {
    let decimals = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else {
        2
    };
    format!("{v:.decimals$}")
}

/// A range in words: "35 to 60 deg", "35 deg and up", "up to 4 m", with its fade.
pub fn range_text(r: &Range, bar: &Bar) -> String {
    let n = |v: f32| number(v, bar.step);
    let unit = bar.unit;
    let ends = match (r.lo, r.hi) {
        (Some(lo), Some(hi)) => format!("{} to {} {unit}", n(lo), n(hi)),
        (Some(lo), None) => format!("{} {unit} and up", n(lo)),
        (None, Some(hi)) => format!("up to {} {unit}", n(hi)),
        (None, None) => "everywhere".to_owned(),
    };
    if bar.fade && r.fade > 0.0 { format!("{ends}, soft {}", n(r.fade)) } else { ends }
}

/// A range bar: the weight the range gives across the domain (1 inside, its fades as ramps), its ends as handles (an open end at the
/// bar's end, dim). Drag an end to move it, onto the bar's end to open it (when it may be); Shift + drag sets how soft an end is.
/// `enabled` false: drawn dim, and a click asks for it to be switched on (the second value). Whether the range changed, and whether a
/// dim bar was clicked.
pub fn range_bar(id: &str, r: &mut Range, bar: &Bar, width: f32, enabled: bool) -> (bool, bool) {
    let h = gui::frame_height() + 4.0;
    let at = gui::cursor_screen_pos();
    let (x0, x1) = (at.x + 6.0, at.x + width - 6.0);
    let (lo, hi) = bar.domain;
    let to_x = |v: f32| x0 + ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * (x1 - x0);
    let to_v = |x: f32| {
        let v = lo + (x - x0) / (x1 - x0) * (hi - lo);
        ((v / bar.step).round() * bar.step).clamp(lo, hi)
    };

    let key = gui::get_id(id);
    gui::invisible_button(&format!("##{id}"), gui::v2(width, h), 0);
    let (hovered, held) = (gui::is_item_hovered(), gui::is_item_active());
    let mouse = gui::mouse_pos();
    let mut changed = false;
    let mut ask_on = false;
    if !enabled {
        ask_on = gui::is_item_clicked();
    } else if held {
        let grip = match GRIP.get() {
            Some((k, g)) if k == key => g,
            _ => {
                // The end nearest the click; an open end sits at its side of the bar.
                let (lx, hx) = (r.lo.map_or(x0, to_x), r.hi.map_or(x1, to_x));
                let lo_end = (mouse.x - lx).abs() < (mouse.x - hx).abs() || (mouse.x < lx);
                let shift = gui::io().KeyShift && bar.fade;
                let g = match (lo_end, shift) {
                    (true, false) => Grip::Lo,
                    (false, false) => Grip::Hi,
                    (true, true) => Grip::FadeLo,
                    (false, true) => Grip::FadeHi,
                };
                GRIP.set(Some((key, g)));
                g
            }
        };

        let before = *r;
        match grip {
            Grip::Lo => {
                r.lo = if bar.open_ends && mouse.x <= x0 + 2.0 { None } else { Some(to_v(mouse.x).min(r.hi.unwrap_or(hi))) };
            }
            Grip::Hi => {
                r.hi = if bar.open_ends && mouse.x >= x1 - 2.0 { None } else { Some(to_v(mouse.x).max(r.lo.unwrap_or(lo))) };
            }
            Grip::FadeLo | Grip::FadeHi => {
                let end = if grip == Grip::FadeLo { r.lo } else { r.hi };
                if let Some(end) = end {
                    let d = (to_v(mouse.x) - end).abs() * 2.0;
                    r.fade = (d / bar.step).round() * bar.step;
                }
            }
        }

        changed = *r != before;
    } else if GRIP.get().is_some_and(|(k, _)| k == key) {
        GRIP.set(None);
    }

    if hovered && !held {
        gui::set_tooltip(if !enabled {
            "Off: click to use it"
        } else if bar.fade {
            "Drag an end to move it, onto the bar's end for no limit; Shift + drag an end: how softly it blends"
        } else if bar.open_ends {
            "Drag an end to move it, onto the bar's end for no limit"
        } else {
            "Drag an end to move it"
        });
    }

    // The track, the weight as a filled curve, the ends, the words.
    let dl = gui::window_draw_list();
    let max = gui::v2(at.x + width, at.y + h);
    let track = if hovered && enabled { theme::mix(frame_bg(), theme::text(), 0.06) } else { frame_bg() };
    dl.rect_filled(at, max, theme::u32(track), ROUNDING);
    let colour = if enabled { accent() } else { theme::dim() };
    let (top, bottom) = (at.y + 4.0, at.y + h - 3.0);
    let samples = 64;
    let mut curve = vec![gui::v2(x0, bottom)];
    curve.extend((0..=samples).map(|i| {
        let x = x0 + (x1 - x0) * i as f32 / samples as f32;
        let w = r.weight(f64::from(lo + (x - x0) / (x1 - x0) * (hi - lo))) as f32;
        gui::v2(x, bottom - w * (bottom - top))
    }));
    curve.push(gui::v2(x1, bottom));
    let mut fill = colour;
    fill[3] *= if enabled { 0.35 } else { 0.15 };
    dl.concave_poly_filled(&curve, theme::u32(fill));
    let mut line = colour;
    line[3] *= if enabled { 0.9 } else { 0.4 };
    dl.polyline(&curve[1..curve.len() - 1], theme::u32(line), false, 1.5);

    if enabled {
        for (end, side) in [(r.lo, x0), (r.hi, x1)] {
            let x = end.map_or(side, to_x);
            let c = if end.is_some() { theme::text() } else { theme::dim() };
            dl.rect_filled(gui::v2(x - 2.5, at.y + 2.0), gui::v2(x + 2.5, at.y + h - 2.0), theme::u32(c), 2.0);
        }
    }

    // The words beside the range's span where they fit (not under its ends), else over it.
    let words = if enabled { range_text(r, bar) } else { "off".to_owned() };
    let shadow = gui::col32(0, 0, 0, 160);
    let tw = gui::calc_text_width(&words);
    let (lx, hx) = (r.lo.map_or(x0, to_x), r.hi.map_or(x1, to_x));
    let tx = if !enabled {
        at.x + (width - tw) * 0.5
    } else if tw + 16.0 <= x1 - hx {
        hx + 8.0
    } else if tw + 16.0 <= lx - x0 {
        lx - 8.0 - tw
    } else {
        at.x + (width - tw) * 0.5
    };
    let ty = at.y + (h - gui::text_line_height()) * 0.5;
    dl.text(gui::v2(tx + 1.0, ty + 1.0), shadow, &words);
    dl.text(gui::v2(tx, ty), theme::u32(if enabled { theme::text() } else { theme::dim() }), &words);
    (changed, ask_on)
}

/// A clickable texture swatch: the picture (a placeholder while it is being made), a frame lit when selected, a caption under it.
pub fn swatch(id: &str, image: Option<u64>, size: f32, selected: bool, caption: &str, tip: &str) -> bool {
    let caption_h = if caption.is_empty() { 0.0 } else { gui::text_line_height() + 2.0 };
    let at = gui::cursor_screen_pos();
    let clicked = gui::invisible_button(&format!("##{id}"), gui::v2(size, size + caption_h), 0);
    let hovered = gui::is_item_hovered();
    if hovered && !tip.is_empty() {
        gui::set_tooltip(tip);
    }

    let dl = gui::window_draw_list();
    let max = gui::v2(at.x + size, at.y + size);
    match image {
        Some(id) => dl.image(id, at, max, gui::v2(0.0, 0.0), gui::v2(1.0, 1.0), gui::col32(255, 255, 255, 255)),
        None => dl.rect_filled(at, max, theme::u32(frame_bg()), ROUNDING),
    }

    let (frame, thick) = if selected {
        (accent(), 2.5)
    } else if hovered {
        (theme::text(), 1.5)
    } else {
        (theme::mix(frame_bg(), theme::text(), 0.2), 1.0)
    };
    dl.rect(at, max, theme::u32(frame), 3.0, thick);
    if !caption.is_empty() {
        let colour = if selected { accent() } else { theme::dim() };
        centred_text(dl, gui::v2(at.x - GAP, max.y + 1.0), gui::v2(max.x + GAP, max.y + caption_h), colour, caption);
    }

    clicked
}

/// Swatches across a width: their size so that `n` fit with gaps, at most `max`.
pub fn swatch_size(n: usize, width: f32, max: f32) -> f32 {
    ((width - GAP * 2.0 * (n.saturating_sub(1)) as f32) / n.max(1) as f32).min(max)
}

/// The space between swatches on a line.
pub fn swatch_gap() {
    gui::same_line_spacing(GAP * 2.0);
}

/// An icon button of the editor's small-button look, with a tooltip.
pub fn icon_button(icon: &str, id: &str, tip: &str) -> bool {
    let clicked = gui::small_button(&format!("{icon}##{id}"));
    if gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }

    clicked
}
