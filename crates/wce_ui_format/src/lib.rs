//! Total War UI layout files to XML and back.
//!
//! The binary is a version-gated stream of primitives; the XML mirrors it one to one (`<u>`, `<i>`, `<s>`, `<flt>`,
//! `<yes />`, ... under structure tags), so an untouched file converts to XML and back to the same bytes. The
//! layout knowledge is a port of the `ui` converter of taw/etwng (MIT, see THIRD_PARTY.md). The XML may change
//! between versions of the editor, as the original warns; hand edits that keep the shape are fine.

mod reader;
mod ruby_float;
mod writer;
pub mod xml_lite;

pub use reader::xml_escape;
pub use writer::is_array_tag;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiError(pub String);

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UiError {}

/// What a successful conversion tells about the file.
pub struct UiXml {
    pub xml: String,
    /// The `VersionNNN` number of the file.
    pub version: u32,
}

/// True if the bytes start with `Version` and three digits.
pub fn looks_like_ui(data: &[u8]) -> bool {
    data.len() >= 10 && &data[..7] == b"Version" && data[7..10].iter().all(|c| c.is_ascii_digit())
}

/// Binary UI file (a `Version0xx` layout, `.cml`, `.fc` or `.twui.images`) to XML.
pub fn ui_to_xml(data: &[u8]) -> Result<UiXml, UiError> {
    reader::convert(data).map(|c| UiXml { xml: c.xml, version: c.version }).map_err(UiError)
}

/// XML of `ui_to_xml` (or edited by hand or by the editor) back to the binary file.
pub fn xml_to_ui(xml: &str) -> Result<Vec<u8>, UiError> {
    writer::convert(xml).map_err(UiError)
}

/// Convert to XML and back, and say whether the bytes are the same.
pub fn round_trip(data: &[u8]) -> Result<bool, UiError> {
    let x = ui_to_xml(data)?;
    Ok(xml_to_ui(&x.xml)? == data)
}
