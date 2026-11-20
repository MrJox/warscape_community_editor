//! Playback through SDL3: one audio stream on the default device, opened with the sound's own rate and channels (SDL converts them
//! to the device's). The player keeps the decoded sound, so it can seek, loop and replay, and its peak envelope for the waveform.
//! `tick` runs every frame, whatever panel is shown: it queues the next loop ahead of time (no gap) and handles the end.

use std::ptr::null_mut;
use std::sync::Arc;

use sdl3_sys::audio::{
    SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_F32, SDL_AudioSpec, SDL_AudioStream, SDL_ClearAudioStream, SDL_DestroyAudioStream,
    SDL_GetAudioStreamQueued, SDL_OpenAudioDeviceStream, SDL_PauseAudioStreamDevice, SDL_PutAudioStreamData, SDL_ResumeAudioStreamDevice,
    SDL_SetAudioStreamGain,
};
use sdl3_sys::init::{SDL_INIT_AUDIO, SDL_InitSubSystem};
use wce_audio::analysis::{Stats, analyse};
use wce_audio::decode::Pcm;
use wce_audio::library::MediaRef;

/// How many columns a waveform has.
pub const PEAKS: usize = 600;

/// The loudest sample of each of `columns` equal slices of a sound (all channels), 0 to 1: its amplitude envelope.
pub fn peaks(pcm: &Pcm, columns: usize) -> Vec<f32> {
    let channels = usize::from(pcm.channels.max(1));
    let frames = pcm.frames();
    if frames == 0 || columns == 0 {
        return vec![];
    }
    (0..columns)
        .map(|c| {
            let (a, b) = (c * frames / columns, ((c + 1) * frames / columns).max(c * frames / columns + 1).min(frames));
            pcm.samples[a * channels..b * channels].iter().fold(0.0f32, |m, s| m.max(s.abs())).min(1.0)
        })
        .collect()
}

/// Where a loop's next copy is queued: when less than this many seconds are left in the stream.
const LOOP_AHEAD: f64 = 0.5;

pub struct Player {
    stream: *mut SDL_AudioStream,
    pcm: Option<Arc<Pcm>>,
    /// The waveform of the loaded sound, and its levels.
    pub peaks: Vec<f32>,
    pub stats: Option<Stats>,
    pub label: String,
    /// The loaded sound's media, when it came from one.
    pub media: Option<MediaRef>,
    /// The frame the stream's data starts at (a seek moves it); the bytes put since (more than one copy when looping).
    start_frame: usize,
    bytes_put: usize,
    pub paused: bool,
    pub looping: bool,
    pub volume: f32,
    audio_ready: Option<bool>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            stream: null_mut(),
            pcm: None,
            peaks: vec![],
            stats: None,
            label: String::new(),
            media: None,
            start_frame: 0,
            bytes_put: 0,
            paused: false,
            looping: false,
            volume: 1.0,
            audio_ready: None,
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.close();
    }
}

impl Player {
    /// Loads a sound and plays it from the start.
    ///
    /// # Errors
    ///
    /// When SDL's audio can't start or the device can't be opened.
    pub fn play(&mut self, pcm: Arc<Pcm>, label: &str, media: Option<MediaRef>) -> Result<(), String> {
        if pcm.samples.is_empty() || pcm.channels == 0 {
            return Err("the sound is empty".into());
        }
        // A new stream: the old one has the previous sound's rate and channels (kept, it would play this one too fast or slow).
        self.close();
        self.peaks = peaks(&pcm, PEAKS);
        self.stats = Some(analyse(&pcm));
        self.pcm = Some(pcm);
        self.label = label.to_owned();
        self.media = media;
        self.paused = false;
        self.start_at(0)
    }

    /// True while a sound is loaded (playing, paused, or ended and ready to play again).
    pub fn loaded(&self) -> bool {
        self.pcm.is_some()
    }

    pub fn playing(&self) -> bool {
        !self.stream.is_null() && !self.paused
    }

    /// Seconds of the loaded sound.
    pub fn length(&self) -> f64 {
        self.pcm.as_ref().map_or(0.0, |p| p.seconds())
    }

    /// The loaded sound's rate and channels.
    pub fn format(&self) -> Option<(u32, u16)> {
        self.pcm.as_ref().map(|p| (p.sample_rate, p.channels))
    }

    fn close(&mut self) {
        if !self.stream.is_null() {
            unsafe { SDL_DestroyAudioStream(self.stream) };
            self.stream = null_mut();
        }
    }

    /// (Re)opens the stream and queues the sound from `frame`.
    fn start_at(&mut self, frame: usize) -> Result<(), String> {
        let Some(pcm) = self.pcm.clone() else { return Ok(()) };
        let ready = *self.audio_ready.get_or_insert_with(|| unsafe { SDL_InitSubSystem(SDL_INIT_AUDIO) });
        if !ready {
            return Err(format!("SDL audio did not start: {}", sdl_error()));
        }
        let frame = frame.min(pcm.frames());
        unsafe {
            if self.stream.is_null() {
                let spec = SDL_AudioSpec { format: SDL_AUDIO_F32, channels: i32::from(pcm.channels), freq: pcm.sample_rate as i32 };
                self.stream = SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, None, null_mut());
                if self.stream.is_null() {
                    return Err(format!("no audio device: {}", sdl_error()));
                }
            } else {
                SDL_ClearAudioStream(self.stream);
            }
            SDL_SetAudioStreamGain(self.stream, self.volume);
        }
        self.bytes_put = 0;
        self.start_frame = frame;
        self.put(frame)?;
        unsafe {
            if self.paused {
                SDL_PauseAudioStreamDevice(self.stream);
            } else {
                SDL_ResumeAudioStreamDevice(self.stream);
            }
        }
        Ok(())
    }

    /// Queues the sound from `frame` to its end.
    fn put(&mut self, frame: usize) -> Result<(), String> {
        let Some(pcm) = self.pcm.clone() else { return Ok(()) };
        let channels = usize::from(pcm.channels);
        let data = &pcm.samples[frame * channels..];
        let bytes = std::mem::size_of_val(data);
        if !unsafe { SDL_PutAudioStreamData(self.stream, data.as_ptr().cast(), bytes as i32) } {
            return Err(format!("SDL refused the sound: {}", sdl_error()));
        }
        self.bytes_put += bytes;
        Ok(())
    }

    /// Stops and goes back to the start (the sound stays loaded).
    pub fn stop(&mut self) {
        self.close();
        self.paused = false;
        self.start_frame = 0;
        self.bytes_put = 0;
    }

    /// Plays or pauses; plays again from the start after the end.
    pub fn toggle(&mut self) {
        if self.stream.is_null() {
            self.paused = false;
            let _ = self.start_at(0);
            return;
        }
        self.paused = !self.paused;
        unsafe {
            if self.paused {
                SDL_PauseAudioStreamDevice(self.stream);
            } else {
                SDL_ResumeAudioStreamDevice(self.stream);
            }
        }
    }

    /// Jumps to a point (0 to 1) of the sound and plays from there.
    pub fn seek(&mut self, fraction: f32) {
        let Some(pcm) = &self.pcm else { return };
        let frame = (fraction.clamp(0.0, 1.0) as f64 * pcm.frames() as f64) as usize;
        self.paused = false;
        let _ = self.start_at(frame);
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 2.0);
        if !self.stream.is_null() {
            unsafe { SDL_SetAudioStreamGain(self.stream, self.volume) };
        }
    }

    fn queued(&self) -> usize {
        if self.stream.is_null() { 0 } else { unsafe { SDL_GetAudioStreamQueued(self.stream) }.max(0) as usize }
    }

    /// Every frame: queues the next copy of a looping sound before the stream runs dry, and at the end of a sound played once,
    /// goes back to the start.
    pub fn tick(&mut self) {
        let Some(pcm) = self.pcm.clone() else { return };
        if self.stream.is_null() || self.paused {
            return;
        }
        let queued = self.queued();
        let frame_bytes = usize::from(pcm.channels) * 4;
        let ahead = (LOOP_AHEAD * f64::from(pcm.sample_rate)) as usize * frame_bytes;
        if self.looping && queued < ahead {
            let _ = self.put(0);
        } else if !self.looping && queued == 0 {
            self.stop();
        }
    }

    /// Where playback is (0 to 1).
    pub fn position(&self) -> f32 {
        let Some(pcm) = &self.pcm else { return 0.0 };
        if self.stream.is_null() {
            return 0.0;
        }
        let frames = pcm.frames().max(1);
        let played = self.bytes_put.saturating_sub(self.queued()) / (usize::from(pcm.channels) * 4);
        // The first copy starts at the seek point, the next ones (a loop) at 0.
        let first = frames.saturating_sub(self.start_frame);
        let frame = if played < first { self.start_frame + played } else { (played - first) % frames };
        (frame as f64 / frames as f64) as f32
    }
}

fn sdl_error() -> String {
    unsafe { std::ffi::CStr::from_ptr(sdl3_sys::error::SDL_GetError()).to_string_lossy().into_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope() {
        let pcm = Pcm { channels: 2, sample_rate: 10, samples: vec![0.0, 0.1, -0.5, 0.2, 0.9, 0.0, 0.0, 0.3] };
        assert_eq!(peaks(&pcm, 2), vec![0.5, 0.9]);
        assert_eq!(peaks(&pcm, 4), vec![0.1, 0.5, 0.9, 0.3]);
    }
}
