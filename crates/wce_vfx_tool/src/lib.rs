//! The VFX tool (docs/plans/vfx_tool.md): `vfx/*.xml` effects edited visually. The effect plays on the sample map (or the open tile)
//! while it is edited, with playback, solo / mute and handles in the viewport; the Inspector edits its emitters, behaviour chains,
//! render stacks and references, curves included. Files are written byte-identically outside the edited spans.

pub mod commands;
pub mod curves;
mod gizmos;
mod inspect;
pub mod model;
pub mod ops;
pub mod props;
pub mod tool;

pub use tool::VfxTool;
