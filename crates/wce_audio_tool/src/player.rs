//! Playback through SDL3: one audio stream on the default device per sound. The stream takes the sound's own rate and channels and
//! SDL converts them to the device's.

use std::ptr::null_mut;

use sdl3_sys::audio::{
    SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_F32, SDL_AudioSpec, SDL_AudioStream, SDL_DestroyAudioStream, SDL_GetAudioStreamQueued,
    SDL_OpenAudioDeviceStream, SDL_PutAudioStreamData, SDL_ResumeAudioStreamDevice,
};
use sdl3_sys::init::{SDL_INIT_AUDIO, SDL_InitSubSystem};
use wce_audio::decode::Pcm;

pub struct Player {
    stream: *mut SDL_AudioStream,
    total_bytes: usize,
    pub label: String,
    pub seconds: f64,
    audio_ready: Option<bool>,
}

impl Default for Player {
    fn default() -> Self {
        Self { stream: null_mut(), total_bytes: 0, label: String::new(), seconds: 0.0, audio_ready: None }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Player {
    /// Starts playing, stopping what played before.
    ///
    /// # Errors
    ///
    /// When SDL's audio can't start or the device can't be opened.
    pub fn play(&mut self, pcm: &Pcm, label: &str) -> Result<(), String> {
        self.stop();
        let ready = *self.audio_ready.get_or_insert_with(|| unsafe { SDL_InitSubSystem(SDL_INIT_AUDIO) });
        if !ready {
            return Err(format!("SDL audio did not start: {}", sdl_error()));
        }

        if pcm.samples.is_empty() || pcm.channels == 0 {
            return Err("the sound is empty".into());
        }

        let spec = SDL_AudioSpec { format: SDL_AUDIO_F32, channels: i32::from(pcm.channels), freq: pcm.sample_rate as i32 };
        let bytes = std::mem::size_of_val(pcm.samples.as_slice());
        unsafe {
            let stream = SDL_OpenAudioDeviceStream(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, &spec, None, null_mut());
            if stream.is_null() {
                return Err(format!("no audio device: {}", sdl_error()));
            }

            if !SDL_PutAudioStreamData(stream, pcm.samples.as_ptr().cast(), bytes as i32) {
                SDL_DestroyAudioStream(stream);
                return Err(format!("SDL refused the sound: {}", sdl_error()));
            }

            SDL_ResumeAudioStreamDevice(stream);
            self.stream = stream;
        }

        self.total_bytes = bytes;
        self.label = label.to_owned();
        self.seconds = pcm.seconds();
        Ok(())
    }

    pub fn stop(&mut self) {
        if !self.stream.is_null() {
            unsafe { SDL_DestroyAudioStream(self.stream) };
            self.stream = null_mut();
        }
    }

    /// While playing: how far it is (0 to 1).
    pub fn progress(&mut self) -> Option<f32> {
        if self.stream.is_null() {
            return None;
        }

        let queued = unsafe { SDL_GetAudioStreamQueued(self.stream) }.max(0) as usize;
        if queued == 0 {
            self.stop();
            return None;
        }

        Some(1.0 - queued as f32 / self.total_bytes.max(1) as f32)
    }
}

fn sdl_error() -> String {
    unsafe { std::ffi::CStr::from_ptr(sdl3_sys::error::SDL_GetError()).to_string_lossy().into_owned() }
}
