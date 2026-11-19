//! The headless audio layer (docs/plans/audio.md): it decodes the game's sound files to samples, knows the names of sounds, and
//! resolves what an event plays. No UI and no output device: the Audio tool plays what this gives it, and the Cinematic tool will.
//!
//! - [`decode`]: a WEM (Wwise Vorbis) to interleaved f32 samples.
//! - [`codebooks`]: Wwise's built-in Vorbis codebook library.
//! - [`bits`]: LSB-first bit reading and writing, as Vorbis packs its bits.

pub mod bits;
pub mod codebooks;
pub mod decode;

/// Why a sound could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("{0}")]
    Format(#[from] wce_audio_format::FormatError),

    #[error("not supported: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Invalid(String),

    #[error("the Vorbis decoder: {0}")]
    Vorbis(String),
}

pub type Result<T> = std::result::Result<T, AudioError>;
