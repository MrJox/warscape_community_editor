//! The game's movies (`.ca_vp8`, the CAMV container) through rpfm_lib's `files::video::Video`, which gives every Attila movie back
//! byte for byte (docs/plans/cinematic.md, "Movies"). New movies are written as IVF, read by rpfm_lib and saved as CAMV, so rpfm_lib
//! stays the one CAMV writer.

use std::io::Cursor;

use rpfm_lib::files::video::{SupportedFormats, Video};
use rpfm_lib::files::{Decodeable, Encodeable};

use crate::vp8::EncodedFrame;

#[derive(Debug, thiserror::Error)]
pub enum MovieError {
    #[error("not a movie: {0}")]
    Read(String),
    #[error("the movie could not be written: {0}")]
    Write(String),
    #[error("frame {0} is outside the movie's data")]
    Frame(usize),
}

/// Vanilla's movies store 33.333 ms per frame for 30 fps (f32 0x420554FE), not 1000 / 30.
pub const VANILLA_MS_PER_FRAME_30: f32 = 33.333;

/// A movie: its header and its compressed VP8 frames.
#[derive(Clone, Debug)]
pub struct Movie {
    video: Video,
    /// Each frame's start in `frame_data` (rpfm_lib's offsets are relative to it).
    starts: Vec<(usize, usize, bool)>,
}

impl Movie {
    pub fn read(bytes: &[u8]) -> Result<Self, MovieError> {
        let video = Video::decode(&mut Cursor::new(bytes), &None).map_err(|e| MovieError::Read(e.to_string()))?;
        Ok(Self::from_video(video))
    }

    fn from_video(video: Video) -> Self {
        let starts = video.frame_table().iter().map(|f| (*f.offset() as usize, *f.size() as usize, *f.is_key_frame())).collect();
        Self { video, starts }
    }

    pub fn width(&self) -> u32 {
        *self.video.width() as u32
    }

    pub fn height(&self) -> u32 {
        *self.video.height() as u32
    }

    pub fn fps(&self) -> f32 {
        *self.video.framerate()
    }

    pub fn frame_count(&self) -> usize {
        self.starts.len()
    }

    pub fn seconds(&self) -> f64 {
        self.frame_count() as f64 / self.fps().max(1.0) as f64
    }

    pub fn is_key_frame(&self, index: usize) -> bool {
        self.starts.get(index).is_some_and(|f| f.2)
    }

    /// The key frame at or before `index` (decoding starts there).
    pub fn key_frame_before(&self, index: usize) -> usize {
        (0..=index.min(self.frame_count().saturating_sub(1))).rev().find(|&i| self.is_key_frame(i)).unwrap_or(0)
    }

    /// Frame `index`'s compressed bytes.
    pub fn frame(&self, index: usize) -> Result<&[u8], MovieError> {
        let (start, size, _) = *self.starts.get(index).ok_or(MovieError::Frame(index))?;
        self.video.frame_data().get(start..start + size).ok_or(MovieError::Frame(index))
    }

    /// The movie as `.ca_vp8` bytes.
    pub fn to_ca_vp8(&self) -> Result<Vec<u8>, MovieError> {
        let mut video = self.video.clone();
        video.set_format(SupportedFormats::CaVp8);
        let mut out = vec![];
        video.encode(&mut out, &None).map_err(|e| MovieError::Write(e.to_string()))?;
        Ok(out)
    }

    /// A new movie from an encoder's frames, in their order, as the game's version-0 CAMV at `fps`.
    pub fn from_frames(width: u32, height: u32, fps: u32, frames: &[EncodedFrame]) -> Result<Self, MovieError> {
        let ivf = ivf(width, height, fps, frames);
        let mut video = Video::decode(&mut Cursor::new(&ivf), &None).map_err(|e| MovieError::Write(e.to_string()))?;
        video.set_format(SupportedFormats::CaVp8);
        video.set_version(0);
        if fps == 30 {
            video.set_framerate(1000.0 / VANILLA_MS_PER_FRAME_30);
        }
        Ok(Self::from_video(video))
    }
}

/// An IVF file (the standard VP8 container): 32-byte header, then per frame its size, its timestamp and its bytes.
pub fn ivf(width: u32, height: u32, fps: u32, frames: &[EncodedFrame]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + frames.iter().map(|f| f.data.len() + 12).sum::<usize>());
    out.extend_from_slice(b"DKIF");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"VP80");
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.extend_from_slice(&fps.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (i, f) in frames.iter().enumerate() {
        out.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(i as u64).to_le_bytes());
        out.extend_from_slice(&f.data);
    }
    out
}
