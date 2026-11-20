//! Video for the Cinematic tool (docs/plans/cinematic.md): VP8 decoding and encoding through libvpx (`vp8`), the game's movies
//! (`movie`: the `.ca_vp8` container through rpfm_lib) and the picture conversions between I420 and RGBA (`yuv`).

pub mod movie;
pub mod vp8;
pub mod yuv;

pub use movie::{Movie, MovieError};
pub use vp8::{Decoder, EncodedFrame, Encoder, EncoderSettings, I420, Vp8Error};
