//! The Unit tool (docs/plans/unit_editor.md): a variant mesh definition (VMD) as the document, worn by a unit of the DB in the renderer.

pub mod db;
mod document;
mod problems;
mod tool;

pub use document::VmdDocument;
pub use tool::{UNIT_TOOL_ID, UnitTool};
