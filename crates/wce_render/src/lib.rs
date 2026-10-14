//! The renderer: the game's own shaders on D3D11, fed from the scene and the environment.

pub mod cbuffers;
pub mod environment_apply;
pub mod gpu_adapter;
pub mod gpu_timer;
pub mod image_io;
pub mod model_cache;
pub mod render_target;
pub mod shader_lib;
pub mod texture;
pub mod wind;

/// An error of the renderer: what failed, in words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct RenderError(pub String);

impl From<windows_core::Error> for RenderError {
    fn from(error: windows_core::Error) -> Self {
        Self(error.to_string())
    }
}
