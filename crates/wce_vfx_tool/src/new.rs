//! New effects, written into the project's loose folder (`vfx/<name>.xml`) and opened: a copy of the open effect (its edits included),
//! or an effect that plays a library, as the game's own reference files are (`burning_entity` plays `library_burning_entity`): an empty
//! `<vfx>` and one reference enabling every emitter of the library, in their text layout (UTF-16 with a BOM, CRLF, tabs).

use wce_editor::editor::Editor;
use wce_formats::xml_doc::XmlDoc;
use wce_formats::{TextEncoding, encode_text};

use crate::model::VfxIndex;
use crate::tool::document;

/// What a new effect starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// The open effect, as edited.
    Copy,

    /// A reference to this library (its name, without `vfx/` and `.xml`).
    Plays(String),
}

/// A name the game's files use: lower case letters, digits, `_` (and `/` for a sub-folder).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.ends_with('/')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '/')
}

/// The text of an effect that plays `library`, enabling each of its `emitters` with no tint or lifetime change.
pub fn reference_text(library: &str, emitters: &[String]) -> String {
    let mut t = String::from(
        "<?xml version=\"1.0\"?>\r\n<shader_library file=\"fx/particle_vfx_library.hlsl\"/><vfx construction_type=\"2\" enable_in_ted=\"false\" \
         camera_following=\"false\" affected_by_wind=\"false\">\r\n\t<!-- EMITTERS-->\r\n\t<emitters></emitters>\r\n\t<!-- BEHAVIOUR_CHAINS-->\r\n\
         \t<behaviour_chains></behaviour_chains>\r\n\t<!-- EFFECT RENDER STACKS-->\r\n\t<effect_render_stacks></effect_render_stacks>\r\n\
         </vfx><vfx_references>\r\n",
    );
    t += &format!(
        "\t<vfx inst_name=\"vfxref_1\" vfx_ref=\"{library}\" scale_min=\"1.00\" scale_max=\"1.00\" pos_x=\"0.00\" pos_y=\"0.00\" \
         pos_z=\"0.00\" start_time=\"0.00\" infinite_life=\"true\">\r\n"
    );
    for em in emitters {
        t += &format!(
            "\t\t<emitter name=\"{em}\" enable=\"true\" tint_min_r=\"1.00\" tint_min_g=\"1.00\" tint_min_b=\"1.00\" tint_min_a=\"1.00\" \
             tint_max_r=\"1.00\" tint_max_g=\"1.00\" tint_max_b=\"1.00\" tint_max_a=\"1.00\" particle_lifetime_bias_min=\"0.00\" \
             particle_lifetime_bias_max=\"0.00\" emitter_lifetime_bias_min=\"0.00\" emitter_lifetime_bias_max=\"0.00\"/>\r\n"
        );
    }

    t + "\t</vfx>\r\n</vfx_references>"
}

/// The emitters of a library effect (the open document's when it is that file).
fn library_emitters(e: &Editor, library: &str) -> Result<Vec<String>, String> {
    let path = format!("vfx/{library}.xml");
    let ix = match document(e).filter(|d| d.source().vfs_path == path) {
        Some(d) => VfxIndex::new(d.xml()),
        None => {
            let bytes = e.vfs().ok_or("the game packs are not mounted")?.read(&path).map_err(|err| format!("{path}: {err}"))?;
            VfxIndex::new(&XmlDoc::parse(bytes).map_err(|err| format!("{path}: {err}"))?)
        }
    };

    if ix.emitters.is_empty() {
        return Err(format!("{path} has no emitters of its own: not a library effect"));
    }

    Ok(ix.emitters.iter().map(|em| em.id.clone()).collect())
}

/// Writes the new effect `vfx/<name>.xml` into the project's loose folder. Returns its path in the packs.
///
/// # Errors
///
/// A bad or taken name, nothing to copy, not a library, or the file could not be written.
pub fn create(e: &mut Editor, name: &str, start: &Start) -> Result<String, String> {
    let name = name.trim().trim_end_matches(".xml").trim_start_matches("vfx/").to_owned();
    if !valid_name(&name) {
        return Err(format!("'{name}': use lower case letters, digits and _"));
    }

    let path = format!("vfx/{name}.xml");
    if e.vfs().is_some_and(|v| v.owner(&path).is_some()) || e.loose_path(&path).exists() {
        return Err(format!("{path} exists already: choose another name"));
    }

    let bytes = match start {
        Start::Copy => document(e).ok_or("no effect open to copy")?.xml().save(),
        Start::Plays(library) => {
            let emitters = library_emitters(e, library)?;
            encode_text(reference_text(library, &emitters).as_bytes(), TextEncoding::Utf16LeBom)
        }
    };

    let to = e.loose_path(&path);
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }

    std::fs::write(&to, bytes).map_err(|err| format!("cannot write {}: {err}", to.display()))?;
    e.loose_changed(&path);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_file_reads_as_one_reference_of_the_library() {
        let text = reference_text("library_x", &["fire".to_owned(), "smoke".to_owned()]);
        let doc = XmlDoc::parse(encode_text(text.as_bytes(), TextEncoding::Utf16LeBom)).unwrap();
        let ix = VfxIndex::new(&doc);
        assert!(ix.emitters.is_empty());
        assert_eq!(ix.refs.len(), 1);
        assert_eq!(ix.refs[0].library, "library_x");
        assert_eq!(ix.refs[0].emitters.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["fire", "smoke"]);
        let f = wce_assets::vfx_def::parse_vfx(&doc);
        assert_eq!(f.refs[0].emitters.len(), 2);
        assert!(f.refs[0].emitters.iter().all(|e| e.enable && e.tint_max == [1.0; 4]));
    }

    #[test]
    fn names() {
        assert!(valid_name("my_fire_2"));
        assert!(valid_name("mods/my_fire"));
        assert!(!valid_name("My Fire"));
        assert!(!valid_name(""));
    }
}
