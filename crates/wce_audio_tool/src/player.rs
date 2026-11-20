//! Playback through SDL3: one audio stream on the default device, taking the sound's own rate and channels (SDL converts them to the
//! device's). The player keeps the decoded sound, so it can seek, loop and replay, and its peak envelope for the waveform.

use std::ptr::null_mut;
use std::sync::Arc;

use sdl3_sys::audio::{
    SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_F32, SDL_AudioSpec, SDL_AudioStream, SDL_ClearAudioStream, SDL_DestroyAudioStream,
    SDL_GetAudioStreamQueued, SDL_OpenAudioDeviceStream, SDL_PauseAudioStreamDevice, SDL_PutAudioStreamData, SDL_ResumeAudioStreamDevice,
    SDL_SetAudioStreamGain,
};
use sdl3_sys::init::{SDL_INIT_AUDIO, SDL_InitSubSystem};
use wce_audio::decode::Pcm;

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

pub struct Player {
    stream: *mut SDL_AudioStream,
    pcm: Option<Arc<Pcm>>,
    /// The waveform of the loaded sound.
    pub peaks: Vec<f32>,
    pub label: String,
    /// The frame the stream's data starts at (a seek moves it).
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
            label: String::new(),
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
    pub fn play(&mut self, pcm: Arc<Pcm>, label: &str) -> Result<(), String> {
        if pcm.samples.is_empty() || pcm.channels == 0 {
            return Err("the sound is empty".into());
        }
        self.peaks = peaks(&pcm, PEAKS);
        self.pcm = Some(pcm);
        self.label = label.to_owned();
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
        let channels = usize::from(pcm.channels);
        let frame = frame.min(pcm.frames());
        let data = &pcm.samples[frame * channels..];
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
            let bytes = std::mem::size_of_val(data);
            if !SDL_PutAudioStreamData(self.stream, data.as_ptr().cast(), bytes as i32) {
                return Err(format!("SDL refused the sound: {}", sdl_error()));
            }
            self.bytes_put = bytes;
            if self.paused {
                SDL_PauseAudioStreamDevice(self.stream);
            } else {
                SDL_ResumeAudioStreamDevice(self.stream);
            }
        }
        self.start_frame = frame;
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

    /// Jumps to a point (0 to 1) of the sound; plays from there unless paused.
    pub fn seek(&mut self, fraction: f32) {
        let Some(pcm) = &self.pcm else { return };
        let frame = (fraction.clamp(0.0, 1.0) as f64 * pcm.frames() as f64) as usize;
        let _ = self.start_at(frame);
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 2.0);
        if !self.stream.is_null() {
            unsafe { SDL_SetAudioStreamGain(self.stream, self.volume) };
        }
    }

    /// Where playback is (0 to 1), and the end handled: looped, or back to the start. Call every frame.
    pub fn position(&mut self) -> f32 {
        let Some(pcm) = self.pcm.clone() else { return 0.0 };
        if self.stream.is_null() {
            return 0.0;
        }
        let queued = unsafe { SDL_GetAudioStreamQueued(self.stream) }.max(0) as usize;
        if queued == 0 && !self.paused {
            if self.looping {
                let _ = self.start_at(0);
            } else {
                self.stop();
            }
            return 0.0;
        }
        let frame_bytes = usize::from(pcm.channels) * 4;
        let played = self.start_frame + self.bytes_put.saturating_sub(queued) / frame_bytes;
        (played as f64 / pcm.frames().max(1) as f64) as f32
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
