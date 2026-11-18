//! What a property looks like in the game's own effects, for adding one a file lacks: no schema of the engine's defaults is decoded
//! (docs/plans/weather_current_state.md item 5), so the defaults are the vanilla files' most common text of each property, per emitter
//! type and per modifier fragment (base quality only). Read once from the packs on a thread of its own, the first time it is asked for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wce_formats::xml_doc::XmlDoc;
use wce_vfs::Vfs;

use crate::model::VfxIndex;

/// A property as vanilla writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub name: String,

    /// The element's text (`<property name="Visible" quality="0" value="true"/>`), its most common one.
    pub text: String,

    /// How many vanilla emitters (or modifiers) of the scope have the property, and how many have that text.
    pub uses: usize,
    pub same: usize,
}

/// Templates by scope (`emitter:<type>`, `modifier:<fragment>`), most used first.
pub type Table = HashMap<String, Vec<Template>>;

pub fn emitter_scope(kind: &str) -> String {
    format!("emitter:{kind}")
}

pub fn modifier_scope(fragment: &str) -> String {
    format!("modifier:{fragment}")
}

/// An element's text as the file has it (edits applied).
pub fn element_text(doc: &XmlDoc, e: usize) -> String {
    let el = &doc.elements()[e];
    String::from_utf8_lossy(&doc.text_of_range(el.start_off, el.end_off)).into_owned()
}

/// Gathers the table from documents (the vanilla effects).
pub fn gather<'a>(docs: impl Iterator<Item = &'a XmlDoc>) -> Table {
    // scope -> name -> text -> count
    let mut counts = HashMap::<String, HashMap<String, HashMap<String, usize>>>::new();
    for doc in docs {
        let ix = VfxIndex::new(doc);
        let scoped = ix
            .emitters
            .iter()
            .map(|em| (emitter_scope(&em.kind), &em.props))
            .chain(ix.chains.iter().flat_map(|c| c.modifiers.iter().map(|m| (modifier_scope(&m.fragment), &m.props))));
        for (scope, props) in scoped {
            let names = counts.entry(scope).or_default();
            for p in props.iter().filter(|p| p.quality == 0) {
                *names.entry(p.name.clone()).or_default().entry(element_text(doc, p.elem)).or_default() += 1;
            }
        }
    }

    counts
        .into_iter()
        .map(|(scope, names)| {
            let mut list = names
                .into_iter()
                .map(|(name, texts)| {
                    let uses = texts.values().sum();
                    let (text, same) = texts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).unwrap_or_default();
                    Template { name, text, uses, same }
                })
                .collect::<Vec<_>>();
            list.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
            (scope, list)
        })
        .collect()
}

/// The table, read once on a thread of its own.
#[derive(Default)]
pub struct Defaults {
    table: Arc<Mutex<Option<Table>>>,
    started: bool,
}

impl Defaults {
    /// The table when it is read; the first call starts reading it.
    pub fn get(&mut self, vfs: Option<&Arc<Vfs>>) -> Option<Table> {
        if let Some(t) = self.table.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            return Some(t.clone());
        }

        if !self.started
            && let Some(vfs) = vfs.cloned()
        {
            self.started = true;
            let slot = self.table.clone();
            std::thread::spawn(move || {
                // The game's own packs only: not the mods' or the project's files.
                let docs = vfs
                    .list("vfx/", ".xml")
                    .into_iter()
                    .filter(|p| vfs.owners(p).iter().any(|&o| !vfs.packs()[o].is_mod))
                    .filter_map(|p| vfs.read(p).ok())
                    .filter_map(|b| XmlDoc::parse(b).ok())
                    .collect::<Vec<_>>();
                let table = gather(docs.iter());
                *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(table);
            });
        }

        None
    }
}

/// `text` with its `quality` attribute set to `q` (for a variant of a property).
pub fn with_quality(text: &str, q: i32) -> String {
    match text.find("quality=\"") {
        Some(i) => {
            let start = i + "quality=\"".len();
            let end = text[start..].find('"').map_or(start, |k| start + k);
            format!("{}{q}{}", &text[..start], &text[end..])
        }

        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(emitters: &str) -> XmlDoc {
        XmlDoc::parse(format!("<vfx><emitters>{emitters}</emitters></vfx>").into_bytes()).unwrap()
    }

    #[test]
    fn the_most_common_text_per_type() {
        let a = doc("<emitter type=\"continuous_mesh\" id=\"a\"><property name=\"Visible\" quality=\"0\" value=\"true\"/>\
             <property name=\"Visible\" quality=\"1\" value=\"false\"/></emitter>");
        let b = doc("<emitter type=\"continuous_mesh\" id=\"b\"><property name=\"Visible\" quality=\"0\" value=\"true\"/></emitter>");
        let c = doc("<emitter type=\"continuous_mesh\" id=\"c\"><property name=\"Visible\" quality=\"0\" value=\"false\"/></emitter>");
        let t = gather([&a, &b, &c].into_iter());
        let list = &t["emitter:continuous_mesh"];
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].text, "<property name=\"Visible\" quality=\"0\" value=\"true\"/>");
        assert_eq!((list[0].uses, list[0].same), (3, 2));
        assert_eq!(with_quality(&list[0].text, 2), "<property name=\"Visible\" quality=\"2\" value=\"true\"/>");
    }
}
