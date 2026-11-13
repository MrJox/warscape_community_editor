//! Mix the open .environment file with another one into a new file (docs/editor.md, "Weather tool"): the game's cross-fade
//! (`blend_environments`, verified: (b - a) * t + a on every number) previewed live in the viewport while the Mix section is open, then
//! written as a file of the project's loose folder. The new file starts from the nearer of the two (its switches, texts and cloud layers,
//! as the cross-fade takes them) with every number the mix's (`XmlFileDocument::write_numbers_of`).

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_formats::xml_doc::XmlDoc;
use wce_scene::environment::{EnvironmentParams, blend_environments, read_environment};
use wce_scene::render_scene::DocumentId;
use wce_vfs::normalize;
use wce_weather::xml_file_document::{Source, XmlFileDocument};

/// How many files the picker lists at once (the filter narrows them).
const LISTED: usize = 300;

#[derive(Default)]
pub(crate) struct Mix {
    /// The game's .environment files (listed once).
    files: Vec<String>,
    filter: String,

    /// The file mixed with (a path in the packs) and its parameters as the packs (or the project) have it.
    pub(crate) other: String,
    other_params: Option<(String, EnvironmentParams)>,

    /// 0: the open file, 1: the other one.
    pub(crate) t: f32,

    /// The mix lights the scene (the section is open, or `weather_mix`).
    pub(crate) on: bool,
    forced: Option<bool>,

    /// The new file's path in the packs' layout.
    pub(crate) name: String,
}

impl Mix {
    /// The other file's parameters, read when it changes.
    fn other_params(&mut self, e: &Editor) -> Option<EnvironmentParams> {
        if self.other.is_empty() {
            return None;
        }

        if self.other_params.as_ref().is_none_or(|(p, _)| *p != self.other) {
            let bytes = e.vfs()?.read(&self.other).ok()?;
            let params = read_environment(&XmlDoc::parse(bytes).ok()?);
            self.other_params = Some((self.other.clone(), params));
        }

        self.other_params.as_ref().map(|(_, p)| p.clone())
    }

    /// While mixing, the other file's parameters and the mix: what the viewport cross-fades to.
    pub(crate) fn preview(&mut self, e: &Editor, doc: DocumentId) -> Option<(EnvironmentParams, f32)> {
        if !self.on || doc == 0 {
            return None;
        }

        Some((self.other_params(e)?, self.t))
    }

    /// Sets the file to mix with (a path in the packs) and the mix, and shows it.
    ///
    /// # Errors
    ///
    /// No such file in the packs.
    pub(crate) fn set(&mut self, e: &Editor, other: &str, t: f32) -> Result<(), String> {
        let other = normalize(other);
        if !e.vfs().is_some_and(|v| v.read(&other).is_ok()) {
            return Err(format!("no {other} in the packs or the project"));
        }

        self.other = other;
        self.t = t.clamp(0.0, 1.0);
        self.on = true;
        self.forced = Some(true);
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        self.on = false;
        self.forced = Some(false);
    }

    /// Writes the mix as `name` (a path in the packs' layout) into the project's loose folder; the path written.
    ///
    /// # Errors
    ///
    /// No open environment file, no file to mix with, the name taken or not an .environment, the write failed.
    pub(crate) fn save(&mut self, e: &mut Editor, doc: DocumentId, name: &str) -> Result<String, String> {
        let name = normalize(name.trim());
        if !name.ends_with(".environment") || name.contains("..") {
            return Err("the new file's path ends in .environment (e.g. weather/charlemagne/my_mix.environment)".to_owned());
        }

        if e.vfs().is_some_and(|v| v.read(&name).is_ok()) || e.loose_path(&name).exists() {
            return Err(format!("{name} exists already (in the packs or the project): pick another name"));
        }

        let b = self.other_params(e).ok_or("pick the file to mix with first")?;
        let x = e.docs.find_as::<XmlFileDocument>(doc).ok_or("no environment file open")?;
        let a = read_environment(x.xml());
        let mix = blend_environments(&a, &b, self.t);

        // The nearer file is the template, as the cross-fade takes its switches and cloud layers.
        let template = if self.t < 0.5 {
            x.xml().save()
        } else {
            e.vfs().ok_or("the packs are not mounted")?.read(&self.other).map_err(|err| err.to_string())?
        };
        let mut out = XmlFileDocument::new(Source::default(), template, std::path::PathBuf::new())?;
        out.write_numbers_of(&mix);

        let path = e.loose_path(&name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        }

        std::fs::write(&path, out.xml().save()).map_err(|err| format!("{}: {err}", path.display()))?;
        e.loose_changed(&name);
        Ok(name)
    }

    /// The Weather scene panel's Mix section; a file to open (the mix just saved) when the open one has no unsaved edits.
    pub(crate) fn draw(&mut self, e: &mut Editor, doc: DocumentId) -> Option<String> {
        if let Some(open) = self.forced.take() {
            gui::set_next_item_open(open, 0);
        }

        let open = doc != 0 && gui::collapsing_header("Mix with another file", 0);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "Cross-fades the open file with another, as the game does between two weathers, and saves the mix as a new file. The \
                 viewport shows the mix while this section is open.",
            );
        }

        self.on = open;
        if !open {
            return None;
        }

        if self.files.is_empty()
            && let Some(vfs) = e.vfs()
        {
            self.files = vfs.list("", ".environment").into_iter().map(str::to_owned).collect();
            self.files.sort();
        }

        let here = e.docs.find(doc).map(|d| d.source_key()).unwrap_or_default();
        gui::set_next_item_width(-1.0);
        if gui::begin_combo("##mixwith", if self.other.is_empty() { "(the file to mix with)" } else { &self.other }) {
            gui::set_next_item_width(-1.0);
            gui::input_text("##mixfilter", Some("filter (words are and-ed)"), &mut self.filter, 128, 0);
            let words = self.filter.to_lowercase();
            let mut picked = None;
            for f in self.files.iter().filter(|f| **f != here && words.split_whitespace().all(|w| f.contains(w))).take(LISTED) {
                if gui::selectable(f, *f == self.other, 0) {
                    picked = Some(f.clone());
                }
            }

            gui::end_combo();
            if let Some(f) = picked {
                self.other = f;
            }
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("The other .environment file of the mix (from the packs or the project).");
        }

        gui::set_next_item_width(-1.0);
        gui::slider_float("##mixt", &mut self.t, 0.0, 1.0, "mix %.2f");
        if gui::is_item_hovered() {
            gui::set_tooltip("0: the open file, 1: the other one; in between, every number is mixed (the sun along the shorter arc).");
        }

        if self.name.is_empty() && !here.is_empty() {
            let stem = here.strip_suffix(".environment").unwrap_or(&here);
            self.name = format!("{stem}_mix.environment");
        }

        gui::set_next_item_width(-110.0);
        gui::input_text("##mixname", Some("weather/.../new.environment"), &mut self.name, 260, 0);
        if gui::is_item_hovered() {
            gui::set_tooltip("The new file's path in the game's layout; it is written into the project's loose folder.");
        }

        gui::same_line();
        gui::begin_disabled(self.other.is_empty());
        let save = gui::button("Save the mix");
        gui::end_disabled();
        if gui::is_item_hovered_allow_disabled() {
            gui::set_tooltip(
                "Writes the mix as a new file, starting from the nearer of the two (its switches, texts and cloud layers) with every \
                 number mixed. Battles pick weather files through the game's variation database: give it an entry (Selection > Weather \
                 picker) for the game to use it.",
            );
        }

        if !save {
            return None;
        }

        let name = self.name.clone();
        match self.save(e, doc, &name) {
            Ok(path) => {
                let unsaved = e.docs.find(doc).is_some_and(|d| d.modified());
                if unsaved {
                    e.status(&format!("the mix is saved as {path} (open it from Files: the open file has unsaved edits)"), false);
                    None
                } else {
                    self.stop();
                    e.status(&format!("the mix is saved as {path}"), false);
                    Some(path)
                }
            }

            Err(m) => {
                e.status(&m, true);
                None
            }
        }
    }
}
