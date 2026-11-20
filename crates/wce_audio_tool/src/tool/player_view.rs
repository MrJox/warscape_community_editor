//! The player strip (play / pause, stop, loop, volume, the sound's waveform as its progress bar, click to seek) and the waveform
//! widget it shares with the Audio Object panel's preview (docs/plans/audio.md, step 9).

use std::sync::Arc;
use std::sync::mpsc::channel;

use wce_audio::decode::decode_wem;
use wce_audio::library::{MediaRef, SoundLibrary};
use wce_audio_format::hirc::Source;
use wce_editor::gui::{self, col32, v2};
use wce_editor::icons;

use super::{AudioTool, Request};
use crate::player::{PEAKS, peaks};

/// Minutes and seconds, with tenths.
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

/// Draws a waveform (peaks 0 to 1) across the available width; the part before `played` (0 to 1) in the accent colour, a playhead
/// at it. Returns where it was clicked (0 to 1).
pub fn waveform(id: &str, peaks: &[f32], played: Option<f32>, height: f32) -> Option<f32> {
    let width = gui::content_region_avail().x.max(60.0);
    let origin = gui::cursor_screen_pos();
    let dl = gui::window_draw_list();
    dl.rect_filled(origin, v2(origin.x + width, origin.y + height), col32(24, 26, 31, 255), 4.0);
    let mid = origin.y + height / 2.0;
    let n = peaks.len().max(1);
    let columns = (width as usize / 2).max(1);
    for c in 0..columns {
        let fraction = c as f32 / columns as f32;
        // The loudest of the peaks this column covers, so no spike falls between columns.
        let (a, b) = (c * n / columns, ((c + 1) * n / columns).max(c * n / columns + 1).min(n));
        let peak = peaks.get(a..b).map_or(0.0, |s| s.iter().fold(0.0f32, |m, p| m.max(*p)));
        let h = (peak.sqrt() * (height / 2.0 - 2.0)).max(0.5);
        let x = origin.x + fraction * width + 1.0;
        let colour = if played.is_some_and(|p| fraction < p) { col32(240, 176, 64, 255) } else { col32(110, 118, 135, 255) };
        dl.line(v2(x, mid - h), v2(x, mid + h), colour, 1.5);
    }
    if let Some(p) = played {
        let x = origin.x + p.clamp(0.0, 1.0) * width;
        dl.line(v2(x, origin.y + 1.0), v2(x, origin.y + height - 1.0), col32(255, 230, 170, 255), 1.5);
    }
    let clicked = gui::invisible_button(id, v2(width, height), gui::BUTTON_LEFT);
    if gui::is_item_hovered() {
        gui::set_tooltip("Click to play from here");
    }
    clicked.then(|| ((gui::mouse_pos().x - origin.x) / width).clamp(0.0, 1.0))
}

impl AudioTool {
    /// The player strip at the top of Audio Banks.
    pub(super) fn player_strip(&mut self, requests: &mut Vec<Request>) {
        let _ = requests;
        if let Some((label, _)) = &self.decoding {
            gui::text_disabled(&format!("Decoding {label}..."));
        }
        if !self.player.loaded() {
            if self.decoding.is_none() {
                gui::text_disabled(&format!("{} Nothing loaded: press Play on any sound.", icons::AUDIO_LINES));
            }
            return;
        }

        let position = self.player.position();
        let playing = self.player.playing();
        if gui::button(if playing { icons::PAUSE } else { icons::PLAY }) {
            self.player.toggle();
        }
        if gui::is_item_hovered() {
            gui::set_tooltip(if playing { "Pause" } else { "Play" });
        }
        gui::same_line();
        if gui::button(icons::SQUARE) {
            self.player.stop();
        }
        if gui::is_item_hovered() {
            gui::set_tooltip("Stop (back to the start)");
        }
        gui::same_line();
        let looping = self.player.looping;
        if looping {
            gui::push_style_color(gui::COL_BUTTON, gui::v4(0.75, 0.52, 0.16, 1.0));
        }
        if gui::button(icons::REPEAT) {
            self.player.looping = !looping;
        }
        if looping {
            gui::pop_style_color(1);
        }
        if gui::is_item_hovered() {
            gui::set_tooltip(if looping { "Looping: click to play once" } else { "Loop" });
        }
        gui::same_line();
        gui::text(if self.player.volume == 0.0 { icons::VOLUME_X } else { icons::VOLUME_2 });
        gui::same_line();
        let mut volume = self.player.volume * 100.0;
        gui::set_next_item_width(90.0);
        if gui::slider_float("##volume", &mut volume, 0.0, 150.0, "%.0f %%") {
            self.player.set_volume(volume / 100.0);
        }
        gui::same_line();
        let length = self.player.length();
        gui::text_disabled(&format!("{} / {}", clock(position as f64 * length), clock(length)));
        gui::text(&self.player.label);

        let peaks = self.player.peaks.clone();
        if let Some(at) = waveform("player waveform", &peaks, Some(position), 46.0) {
            self.player.seek(at);
            if !self.player.playing() && self.player.paused {
                self.player.toggle();
            }
        }
    }

    /// A sound's waveform in Audio Object (decoded on a worker the first time); a click plays it from there.
    pub(super) fn sound_preview(&mut self, lib: &SoundLibrary, source: &Source, label: &str, requests: &mut Vec<Request>) {
        let Some(files) = self.files.clone() else { return };
        let media = lib.media_of(source, files.as_ref());
        let id = media.id();
        match self.peaks_of(&media, true) {
            Some(peaks) if !peaks.is_empty() => {
                if let Some(at) = waveform(&format!("preview{id}"), &peaks, None, 34.0) {
                    requests.push(Request::PlayFrom(media, label.to_owned(), at));
                }
            }
            Some(_) => gui::text_disabled("(no audio to show)"),
            None => gui::text_disabled("Reading the waveform..."),
        }
    }

    /// A media file's waveform, or None until it is read: asked for here (each frame it is shown), read on a worker one at a
    /// time, `first` before the others.
    pub(super) fn peaks_of(&mut self, media: &MediaRef, first: bool) -> Option<Arc<Vec<f32>>> {
        let id = media.id();
        if let Some(peaks) = self.previews.get(&id) {
            return Some(peaks.clone());
        }
        if self.preview_loading.as_ref().is_none_or(|(loading, _)| *loading != id) && !self.preview_wanted.iter().any(|m| m.id() == id) {
            if first {
                self.preview_wanted.insert(0, media.clone());
            } else {
                self.preview_wanted.push(media.clone());
            }
        }
        None
    }

    /// Starts reading the first waveform asked for last frame (when none is being read), and forgets the others: what is still
    /// shown asks again.
    pub(super) fn start_preview(&mut self) {
        if self.preview_loading.is_none()
            && let Some(media) = self.preview_wanted.first().cloned()
        {
            let bytes = self.media_bytes(&media);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let peaks = bytes.and_then(|b| decode_wem(&b).ok()).map(|p| peaks(&p, PEAKS)).unwrap_or_default();
                let _ = tx.send(peaks);
            });
            self.preview_loading = Some((media.id(), rx));
        }
        self.preview_wanted.clear();
    }

    /// Edits change what a media ID plays: the waveforms are read again.
    pub(super) fn forget_previews(&mut self) {
        self.previews.clear();
        self.preview_loading = None;
        self.preview_wanted.clear();
    }
}

/// A small waveform on a tree row (or the space for it while it is read).
pub fn mini_waveform(peaks: Option<&[f32]>) {
    let size = v2(56.0, gui::text_line_height());
    let origin = gui::cursor_screen_pos();
    if let Some(peaks) = peaks.filter(|p| !p.is_empty()) {
        let dl = gui::window_draw_list();
        let mid = origin.y + size.y / 2.0;
        let (n, columns) = (peaks.len(), (size.x / 2.0) as usize);
        for c in 0..columns {
            let (a, b) = (c * n / columns, ((c + 1) * n / columns).max(c * n / columns + 1).min(n));
            let peak = peaks[a..b].iter().fold(0.0f32, |m, p| m.max(*p));
            let h = (peak.sqrt() * (size.y / 2.0 - 1.0)).max(0.5);
            let x = origin.x + c as f32 * 2.0 + 0.5;
            dl.line(v2(x, mid - h), v2(x, mid + h), col32(110, 118, 135, 255), 1.0);
        }
    }
    gui::dummy(size);
}
