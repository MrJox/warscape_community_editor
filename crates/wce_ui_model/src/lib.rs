//! The editable model of a Total War UI layout, shared by the editor's canvas, the CLI and the MCP server.
//!
//! `UiModel` holds the XML of a layout (see `wce_ui_format`) as a tree that prints back to the same text, so the
//! visual editor and the raw XML pane work on one thing: an edit in either shows in the other.

pub mod anim;
pub mod cuf;
pub mod dom;
pub mod draw;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod render;
pub mod skins;
pub mod model;
pub mod named;
pub mod pack;
pub mod props;
pub mod related;

pub use dom::{Dom, Element, Form, Node};
pub use draw::{DrawItem, DrawKind, DrawOptions};
pub use model::{Entry, EntryKind, Field, Place, UiModel};
