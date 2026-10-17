//! Layouts that belong together, drawn as read-only reference behind and over the layout being edited.
//!
//! The layout format has no record of which layout another one is loaded into: the game's code (and its scripts) create a component from a
//! layout file and put it under a component of another layout. What the data does show is the naming: a layout `ui/common ui/land_unit_card`
//! is used under a component called `land_unit_card`. So:
//!
//! * a **child** of layout L is a layout whose file name is the name of a component in L (drawn at that component);
//! * a **parent** of L is a layout that has a component named like L's file (L is drawn at that component).
//!
//! This is a guess made from names, not something the game stores, so a few names are too common to mean anything (`tooltip`, `background`)
//! and are marked `generic`; callers leave those out unless asked.

use std::collections::HashMap;

use crate::draw::{DrawItem, DrawKind, DrawOptions};
use crate::model::EntryKind;
use crate::UiModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelKind {
    /// A layout used under a component of this one.
    Child,
    /// A layout that has a component this one is used under.
    Parent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub kind: RelKind,
    /// The other layout's path in the packs.
    pub path: String,
    /// The component name that links them (the file name of the child).
    pub via: String,
    /// The name is common (`tooltip`, `background`, `layout`): probably not a real link.
    pub generic: bool,
}

/// Names that many layouts have as components or as file names without being each other's children.
const GENERIC: [&str; 12] = ["tooltip", "background", "layout", "main", "frontend", "battle", "campaign", "root", "icon", "frame", "title", "button"];

pub fn is_generic(name: &str) -> bool {
    GENERIC.contains(&name.to_ascii_lowercase().as_str())
}

#[derive(Debug, Clone)]
struct Info {
    path: String,
    /// Lower case file name.
    base: String,
    /// Lower case component names, root excluded.
    names: Vec<String>,
}

/// The component names of every layout in the packs, read once.
#[derive(Debug, Clone, Default)]
pub struct RelationIndex {
    layouts: Vec<Info>,
}

/// The `<s>` of every `title` line of the XML: the component names.
fn component_names(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in xml.lines() {
        let l = line.trim_start();
        if let Some(rest) = l.strip_prefix("<s>") {
            if let Some(end) = rest.find("</s><!-- title -->") {
                out.push(rest[..end].to_ascii_lowercase());
            }
        }
    }

    // the first is the root
    if !out.is_empty() {
        out.remove(0);
    }

    out
}

pub fn file_base(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase()
}

impl RelationIndex {
    pub fn build(vfs: &wce_vfs::Vfs) -> Self {
        let mut layouts = Vec::new();
        for path in vfs.files().keys() {
            let name = path.rsplit('/').next().unwrap_or(path);
            if !path.starts_with("ui/") || name.contains('.') {
                continue;
            }

            let Ok(data) = vfs.read(path) else { continue };
            if !wce_ui_format::looks_like_ui(&data) {
                continue;
            }

            let Ok(x) = wce_ui_format::ui_to_xml(&data) else { continue };
            if !x.xml.starts_with("<ui ") {
                continue;
            }

            let mut names = component_names(&x.xml);
            names.sort();
            names.dedup();
            layouts.push(Info { path: path.clone(), base: file_base(path), names });
        }

        Self { layouts }
    }

    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    /// Every layout that links to the one at `path`, children first.
    pub fn relations(&self, path: &str) -> Vec<Relation> {
        let mut out = Vec::new();
        let me = file_base(path);
        let Some(mine) = self.layouts.iter().find(|l| l.path == path) else { return out };
        let by_base: HashMap<&str, Vec<&Info>> = self.layouts.iter().fold(HashMap::new(), |mut m, l| {
            m.entry(l.base.as_str()).or_default().push(l);
            m
        });

        // A layout is named after its own top component, and layouts with the same file name in other folders are variants of it:
        // neither is a parent or a child.
        for n in mine.names.iter().filter(|n| **n != me) {
            if let Some(files) = by_base.get(n.as_str()) {
                for f in files.iter().filter(|f| f.path != path && f.base != me) {
                    out.push(Relation { kind: RelKind::Child, path: f.path.clone(), via: n.clone(), generic: is_generic(n) });
                }
            }
        }

        for l in self.layouts.iter().filter(|l| l.path != path && l.base != me) {
            if l.names.binary_search(&me).is_ok() {
                out.push(Relation { kind: RelKind::Parent, path: l.path.clone(), via: me.clone(), generic: is_generic(&me) });
            }
        }

        out
    }
}

/// The component of a layout that stands for the layout as a whole: the first child of the root, else the root.
fn top_entry(m: &UiModel) -> usize {
    let ents = m.entries();
    if ents.len() > 1 && ents[1].parent == Some(0) { 1 } else { 0 }
}

/// The first component called `name` (case-insensitive), as an entry number.
fn find_named(m: &UiModel, name: &str) -> Option<usize> {
    m.entries().iter().position(|e| e.kind == EntryKind::Entry && e.depth > 0 && e.name.eq_ignore_ascii_case(name))
}

/// The pictures and text of `other`, moved to where it belongs next to `main` and faded by `alpha` (0..1), to draw as a reference layer.
/// Children are placed at the component of `main` they are named after; a parent is placed so that `main` falls on its component.
/// `None` when the component the link goes through is not in the layout.
pub fn layer(main: &UiModel, main_opts: &DrawOptions, rel: &Relation, other: &UiModel, alpha: f32) -> Option<Vec<DrawItem>> {
    let other_opts = DrawOptions { screen: None, ..Default::default() };
    let other_ents = other.entries();
    let other_rects = other.geometry(&other_ents, &other_opts);
    let main_ents = main.entries();
    let main_rects = main.geometry(&main_ents, main_opts);
    let (dx, dy) = match rel.kind {
        RelKind::Child => {
            let at = find_named(main, &rel.via)?;
            let top = top_entry(other);
            (main_rects[at].x - other_rects[top].x, main_rects[at].y - other_rects[top].y)
        }

        RelKind::Parent => {
            let at = find_named(other, &rel.via)?;
            let top = top_entry(main);
            (main_rects[top].x - other_rects[at].x, main_rects[top].y - other_rects[at].y)
        }
    };

    let mut items = other.draw_list_with(&other_opts);
    for it in &mut items {
        it.x += dx;
        it.y += dy;
        match &mut it.kind {
            DrawKind::Image { tint, .. } => tint[3] = (f32::from(tint[3]) * alpha).round() as u8,
            DrawKind::Text { color, .. } => color[3] = (f32::from(color[3]) * alpha).round() as u8,
        }
    }

    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_skip_the_root() {
        let xml = "<ui version=\"096\">\n  <uientry>\n    <s>root</s><!-- title -->\n    <s></s><!-- title2 -->\n    <uientry>\n      <s>Card</s><!-- title -->\n";
        assert_eq!(component_names(xml), vec!["card".to_string()]);
    }

    #[test]
    fn generic_names() {
        assert!(is_generic("Tooltip"));
        assert!(!is_generic("land_unit_card"));
    }
}
