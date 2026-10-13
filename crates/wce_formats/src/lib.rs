//! The editor's own file formats: the ones it edits in place without re-serialising them (XML), and the ones rpfm_lib doesn't cover.

pub mod c_num;
pub mod env_value;
pub mod text_encoding;
pub mod tile_climate;
pub mod tile_entry;
pub mod tile_index;
pub mod xml_doc;

pub use self::text_encoding::{TextEncoding, decode_text, encode_text};
pub use self::xml_doc::{XmlAttr, XmlDoc, XmlElement, XmlError, XmlSplice};
