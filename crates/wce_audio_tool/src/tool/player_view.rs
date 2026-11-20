//! The Audio tool's Inspector: the player (play / pause, stop, loop, volume, the sound's waveform as its progress bar, click to
//! seek) with the playing sound's info (format, size, bitrate, peak, RMS, loudness), and the selected sound's waveform and info;
//! the waveforms read for the trees (docs/plans/audio.md, steps 9 and 10).

use std::sync::Arc;
use std::sync::mpsc::channel;

use wce_audio::analysis::{Stats, analyse};
use wce_audio::decode::decode_wem;
use wce_audio::library::{MediaRef, SoundLibrary};
use wce_audio::setup_allocs::setup_kind;
use wce_audio_format::hirc::Object;
use wce_audio_format::wem::Wem;
use wce_editor::gui::{self, col32, v2};
use wce_editor::icons;

use super::{AudioTool, Request};
use crate::player::{PEAKS, peaks};

/// What is known of a media file: its waveform, levels and format, read on a worker.
#[derive(Default)]
pub struct Preview {
    pub peaks: Vec<f32>,
    pub stats: Option<Stats>,
    pub file: Option<FileInfo>,
}

pub struct FileInfo {
    pub codec: String,
    pub channels: u16,
    /// The WAVE speaker mask (Vorbis WEMs), naming the channels.
    pub channel_mask: Option<u32>,
    pub rate: u32,
    /// Samples per channel.
    pub frames: u64,
    pub bits: u16,
    pub bytes: usize,
    pub seconds: f64,
    pub vorbis: Option<VorbisInfo>,
}

pub struct VorbisInfo {
    pub block_sizes: (u32, u32),
    pub max_packet: u16,
    pub setup_hash: u32,
}

fn codec_name(codec: u16) -> String {
    match codec {
        0xFFFF => "Vorbis (Wwise)".to_owned(),
        0xFFFE => "PCM".to_owned(),
        0x0002 => "ADPCM".to_owned(),
        0x0001 => "PCM".to_owned(),
        c => format!("{c:#06x}"),
    }
}

/// Reads a media file's preview (on a worker).
pub fn read_preview(bytes: Option<Vec<u8>>) -> Preview {
    let Some(bytes) = bytes else { return Preview::default() };
    let wem = Wem::read(&bytes).ok();
    let format = wem.as_ref().and_then(Wem::format);
    let vorbis = wem.as_ref().and_then(Wem::vorbis);
    let mut file = format.map(|f| FileInfo {
        codec: codec_name(f.codec),
        channels: f.channels,
        channel_mask: vorbis.map(|v| v.channel_mask),
        rate: f.sample_rate,
        frames: vorbis.map_or(0, |v| u64::from(v.total_frames)),
        bits: f.bits_per_sample,
        bytes: bytes.len(),
        seconds: wem.as_ref().and_then(Wem::duration).unwrap_or(0.0),
        vorbis: vorbis.map(|v| VorbisInfo {
            block_sizes: (1 << v.blocksize_0_exp, 1 << v.blocksize_1_exp),
            max_packet: v.max_packet_size,
            setup_hash: v.codebook_hash,
        }),
    });
    match decode_wem(&bytes) {
        Ok(pcm) => {
            if let Some(f) = &mut file {
                f.seconds = pcm.seconds();
                f.frames = pcm.frames() as u64;
            }
            Preview { peaks: peaks(&pcm, PEAKS), stats: Some(analyse(&pcm)), file }
        }
        Err(_) => Preview { file, ..Preview::default() },
    }
}

/// The browsers' Play buttons: a row's button shows Stop while the sound it started is being read or played.
#[derive(Default)]
pub struct PlayButtons {
    /// The row (a key unique over the panels) whose Play started the sound.
    source: Option<String>,
    /// Something is being read or played (set every frame).
    pub active: bool,
}

impl PlayButtons {
    /// Play, or Stop for the row playing: true when Play was clicked (the caller asks for its sound); Stop asks for the stop.
    pub fn button(&mut self, key: &str, requests: &mut Vec<Request>) -> bool {
        let playing = self.active && self.source.as_deref() == Some(key);
        let clicked = gui::small_button(&format!("{}##play{key}", if playing { icons::SQUARE } else { icons::PLAY }));
        if playing && gui::is_item_hovered() {
            gui::set_tooltip("Stop");
        }
        if !clicked {
            return false;
        }
        if playing {
            requests.push(Request::Stop);
            return false;
        }
        self.source = Some(key.to_owned());
        true
    }
}

/// Minutes and seconds, with tenths.
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

fn level(db: f64, unit: &str) -> String {
    if db.is_finite() { format!("{db:.1} {unit}") } else { "silent".to_owned() }
}

fn row(name: &str, value: &str, tip: &str) {
    gui::text_disabled(name);
    gui::same_line_at(120.0);
    gui::text(value);
    if !tip.is_empty() && gui::is_item_hovered() {
        gui::set_tooltip(tip);
    }
}

/// The info rows of a sound: where its media is, format, size, bitrate, levels.
fn info(place: Option<&str>, file: Option<&FileInfo>, stats: Option<&Stats>) {
    let row = |name: &str, value: String, tip: &str| row(name, &value, tip);
    if let Some(place) = place {
        row("Media", place.to_owned(), "Where the game reads this sound from.");
    }
    if let Some(f) = file {
        row("Codec", f.codec.clone(), "");
        row("Sample rate", format!("{} Hz", f.rate), "");
        row("Channels", format!("{} ({})", f.channels, channels(f.channels)), "");
        if let Some(mask) = f.channel_mask {
            row("Speakers", speakers(mask), "The WAVE speaker mask the file declares.");
        }
        row("Length", format!("{}  ({:.3} s)", clock(f.seconds), f.seconds), "");
        if f.frames > 0 {
            row("Samples", format!("{} per channel ({} in all)", f.frames, f.frames * u64::from(f.channels)), "");
        }
        if f.bits > 0 && f.vorbis.is_none() {
            row("Bit depth", format!("{} bits", f.bits), "");
        }
        row("File size", format!("{:.1} KB ({} bytes)", f.bytes as f64 / 1024.0, f.bytes), "");
        if f.seconds > 0.0 {
            row(
                "Bitrate",
                format!("{:.0} kbps", f.bytes as f64 * 8.0 / f.seconds / 1000.0),
                "The file's size over its length (all channels).",
            );
        }
        if let Some(v) = &f.vorbis {
            row("Vorbis blocks", format!("{} / {} samples", v.block_sizes.0, v.block_sizes.1), "Short and long block sizes.");
            row("Largest packet", format!("{} bytes", v.max_packet), "");
            row("Encoding", encoding(v), &format!("Vorbis setup {:#010x}", v.setup_hash));
        }
    }
    if let Some(s) = stats {
        row("Peak", level(s.peak_db, "dBFS"), "The loudest sample; 0 dBFS is full scale.");
        if s.channel_peaks_db.len() > 1 {
            let each: Vec<String> =
                s.channel_peaks_db.iter().map(|p| if p.is_finite() { format!("{p:.1}") } else { "silent".to_owned() }).collect();
            row("Peak by channel", format!("{} dBFS", each.join(" / ")), "The loudest sample of each channel, in the file's order.");
        }
        row("RMS", level(s.rms_db, "dBFS"), "The average level (root mean square of every sample).");
        row(
            "Loudness",
            s.loudness.map_or_else(|| "too short to measure".to_owned(), |l| format!("{l:.1} LUFS")),
            "Integrated loudness (ITU-R BS.1770), as streaming services and broadcast measure it; game dialogue sits around -23 to -16.",
        );
        if s.dc_offset.abs() > 0.001 {
            row("DC offset", format!("{:+.4}", s.dc_offset), "The mean sample: a constant shift off zero, worth removing.");
        }
        if s.clipped > 0 {
            gui::text_colored([1.0, 0.6, 0.3, 1.0], &format!("{} samples at full scale (clipping)", s.clipped));
        }
    }
}

fn channels(n: u16) -> String {
    match n {
        1 => "mono",
        2 => "stereo",
        3 => "3.0",
        4 => "quad",
        5 => "5.0",
        6 => "5.1",
        8 => "7.1",
        _ => "multichannel",
    }
    .to_owned()
}

/// The speakers of a WAVE channel mask: "front left, front right".
fn speakers(mask: u32) -> String {
    const NAMES: [&str; 11] = [
        "front left",
        "front right",
        "centre",
        "LFE",
        "back left",
        "back right",
        "front left of centre",
        "front right of centre",
        "back centre",
        "side left",
        "side right",
    ];
    let names: Vec<&str> = (0..NAMES.len()).filter(|b| mask & (1 << b) != 0).map(|b| NAMES[b]).collect();
    if names.is_empty() { format!("{mask:#x}") } else { names.join(", ") }
}

/// The quality a file was encoded at, from its Vorbis setup: "Vorbis quality 4", "Vorbis quality 3, tuned for 32 kHz".
fn encoding(v: &VorbisInfo) -> String {
    let Some(kind) = setup_kind(v.setup_hash) else {
        return "Vorbis, a setup of its own".to_owned();
    };
    let (lo, hi) = kind.qualities;
    let quality = if lo == hi { format!("Vorbis quality {lo}") } else { format!("Vorbis quality {lo} to {hi}") };
    match kind.rates {
        None => quality,
        Some((r, r2)) if r == r2 => format!("{quality}, tuned for {} kHz", f64::from(r) / 1000.0),
        Some((r, r2)) => format!("{quality}, tuned for {} to {} kHz", f64::from(r) / 1000.0, f64::from(r2) / 1000.0),
    }
}

/// Where a media file is, for the info rows.
pub(super) fn place(media: &MediaRef, lib: Option<&SoundLibrary>) -> String {
    match media {
        MediaRef::InBank { bank, id } => {
            format!("#{id} in {}.bnk", lib.and_then(|l| l.banks.get(*bank)).map_or("?", |b| b.name.as_str()))
        }
        MediaRef::Loose { path, .. } => format!("{path} (streamed)"),
        MediaRef::Missing { id } => format!("#{id}: in no bank and no pack"),
    }
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
    /// The Inspector: the player, then the selected sound when it is not the one loaded.
    pub(super) fn inspector(&mut self, requests: &mut Vec<Request>) {
        gui::separator_text(&format!("{} Player", icons::AUDIO_LINES));
        self.player_section();

        let selected = self.selected_sound();
        if let Some((media, label)) = &selected
            && self.player.media.as_ref() != Some(media)
        {
            gui::spacing();
            gui::separator_text("Selected sound");
            gui::text(label);
            if gui::button(&format!("{} Play", icons::PLAY)) {
                requests.push(Request::Play(media.clone(), label.clone()));
            }
            match self.peaks_of(media, true) {
                Some(p) => {
                    if !p.peaks.is_empty()
                        && let Some(at) = waveform(&format!("preview{}", media.id()), &p.peaks, None, 40.0)
                    {
                        requests.push(Request::PlayFrom(media.clone(), label.clone(), at));
                    }
                    info(Some(&place(media, self.library.as_deref())), p.file.as_ref(), p.stats.as_ref());
                    if p.file.is_none() {
                        gui::text_disabled("Its media is in no bank and no pack.");
                    }
                }
                None => gui::text_disabled("Reading the sound..."),
            }
        } else if selected.is_none() && !self.player.loaded() {
            gui::text_disabled("Select a sound in Audio Bank or Audio Object to see it here.");
        }
    }

    /// The selected object when it is a sound (as edited): its media and label.
    fn selected_sound(&mut self) -> Option<(MediaRef, String)> {
        let (lib, files, id) = (self.library.clone()?, self.files.clone()?, self.selected?);
        let Some(Object::Sound(s)) = self.edits.object(&lib, id).or_else(|| lib.object(id).and_then(|(_, _, t)| t.cloned())) else {
            return None;
        };
        let label = self.edits.names.get(&id).cloned().unwrap_or_else(|| lib.label(id, 2));
        Some((lib.media_of(&s.source, files.as_ref()), label))
    }

    fn player_section(&mut self) {
        if let Some((label, _, _)) = &self.decoding {
            gui::text_disabled(&format!("Decoding {label}..."));
        }
        if !self.player.loaded() {
            if self.decoding.is_none() {
                gui::text_disabled("Nothing loaded: press Play on any sound.");
            }
            return;
        }

        gui::text(&self.player.label);
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

        let peaks = self.player.peaks.clone();
        if let Some(at) = waveform("player waveform", &peaks, Some(position), 56.0) {
            self.player.seek(at);
        }
        // The file's info from its preview (read like a selected sound's), the levels from the decoded sound.
        let file = self.player.media.clone().and_then(|m| self.peaks_of(&m, false));
        let stats = self.player.stats.clone();
        let where_ = self.player.media.as_ref().map(|m| place(m, self.library.as_deref()));
        match file.as_ref().and_then(|p| p.file.as_ref()) {
            Some(f) => info(where_.as_deref(), Some(f), stats.as_ref()),
            None => {
                if let Some((rate, channels_n)) = self.player.format() {
                    row("Format", &format!("{rate} Hz, {channels_n} ({})", channels(channels_n)), "");
                }
                info(where_.as_deref(), None, stats.as_ref());
            }
        }
    }

    /// A media file's preview, or None until it is read: asked for here (each frame it is shown), read on a worker one at a
    /// time, `first` before the others.
    pub(super) fn peaks_of(&mut self, media: &MediaRef, first: bool) -> Option<Arc<Preview>> {
        let id = media.id();
        if let Some(p) = self.previews.get(&id) {
            return Some(p.clone());
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

    /// Starts reading the first preview asked for last frame (when none is being read), and forgets the others: what is still
    /// shown asks again.
    pub(super) fn start_preview(&mut self) {
        if self.preview_loading.is_none()
            && let Some(media) = self.preview_wanted.first().cloned()
        {
            let bytes = self.media_bytes(&media);
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(read_preview(bytes));
            });
            self.preview_loading = Some((media.id(), rx));
        }
        self.preview_wanted.clear();
    }

    /// Edits change what a media ID plays: the previews are read again.
    pub(super) fn forget_previews(&mut self) {
        // The trees too: they show the edits.
        self.trees.clear();
        self.lists.clear();
        self.previews.clear();
        self.preview_loading = None;
        self.preview_wanted.clear();
    }
}
