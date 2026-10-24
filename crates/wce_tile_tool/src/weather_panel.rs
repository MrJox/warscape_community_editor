//! The Tile panel's Weather section: which environment files the game picks for the tile (the kit's environment_map.xml under the filters),
//! searchable by text, and the one that lights the tile. Nothing here needs the Weather tool: the file is read from the packs (see
//! `Editor::set_weather_preview`); when the Weather tool has the same file open, the tile follows its edits.
//!
//! Each tile keeps its choice and its filters in the settings (`[tools.tile] weathers`, by the tile's folder). A tile seen for the first time
//! starts with the filters of its terrain look (climate, season) and the default light.

use std::collections::BTreeMap;
use std::path::Path;

use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_vfs::normalize;

use crate::settings::{SETTINGS_ID, TileSettings, TileWeather};

/// The filters, as the panel labels them, and their categories in environment_map.xml.
pub const FILTERS: [(&str, &str); 8] = [
    ("map", "map"), ("climate", "climate"), ("season", "season"), ("climate change", "climate_change_index"), ("precipitation", "precipitation"),
    ("time of day", "time_of_day"), ("battle type", "battle_types"), ("snow attrition", "snow_attrition"),
];

#[derive(Default)]
pub(crate) struct WeatherUi {
    /// The kit's environment_map.xml.
    env: Option<AssetVariationDb>,
    status: String,

    /// The tile shown (its folder, the document's `source_key`); its choice and filters are in the settings.
    tile: String,
    choice: TileWeather,

    /// Words (and-ed) the listed files must contain.
    search: String,
}

/// True when `file` holds every word of `search` (case-insensitive).
fn matches_words(file: &str, search: &str) -> bool {
    let file = file.to_lowercase();
    search.to_lowercase().split_whitespace().all(|w| file.contains(w))
}

impl WeatherUi {
    /// A tile became the one the tool works on: read its kit's environment map, take up its remembered weather (or the filters of its look),
    /// and light it.
    pub(crate) fn tile_changed(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let dir = d.project().dir.clone();
        let look = d.tile().terrain_look().clone();
        self.tile = d.source_key();
        self.load_environment_map(&dir);

        let remembered = e.tool_settings::<TileSettings>(SETTINGS_ID).weathers.get(&self.tile).cloned();
        self.choice = remembered.unwrap_or_else(|| {
            let mut query = BTreeMap::from([("map".to_owned(), "default_map".to_owned())]);
            for (category, value) in [("climate", &look.climate), ("season", &look.season)] {
                if let Some(v) = self.value_of(category, value) {
                    query.insert(category.to_owned(), v);
                }
            }

            TileWeather { file: String::new(), query }
        });

        let file = self.choice.file.clone();
        if let Err(m) = e.set_weather_preview(Some(&file).filter(|f| !f.is_empty()).map(String::as_str)) {
            e.status(&format!("the tile's weather: {m}"), true);
        }
    }

    /// The database's spelling of a value of a category (case-insensitive), if it has it.
    fn value_of(&self, category: &str, value: &str) -> Option<String> {
        let values = self.env.as_ref()?.category(category)?.values().clone();
        values.into_iter().find(|v| v.eq_ignore_ascii_case(value))
    }

    /// The kit's environment_map.xml: which environment file the game picks for this tile's map.
    fn load_environment_map(&mut self, tile_dir: &Path) {
        self.env = None;
        let g = tile_dir.display().to_string().replace('\\', "/");
        let Some(at) = g.find("/raw_data/") else {
            self.status = "no raw_data folder in the tile path".to_owned();
            return;
        };

        let p = Path::new(&g[..at + 10]).join("warscape_asset_variation_db").join("environment_map.xml");
        let loaded = std::fs::read(&p)
            .map_err(|_| format!("cannot read {}", p.display()))
            .and_then(|b| AssetVariationDb::from_environment_map(&String::from_utf8_lossy(&b)).map_err(|err| err.to_string()));
        match loaded {
            Ok(db) => {
                self.status = format!("{} environment files", db.entries().first().map_or(0, |entry| entry.variations().len()));
                self.env = Some(db);
            }

            Err(err) => self.status = format!("environment_map.xml: {err}"),
        }
    }

    /// The files the game picks under the filters that hold the search's words, and whether the game asked again with `default_map`.
    pub(crate) fn matching(&self) -> (Vec<String>, bool) {
        let Some(env) = &self.env else { return (vec![], false) };
        let query = self.choice.query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        let res = env.matches_listed_map("environment", &query);
        let files = res.variations.iter().map(|v| v.filename().clone()).filter(|f| matches_words(f, &self.search)).collect();
        (files, res.used_default_map)
    }

    fn remember(&self, e: &mut Editor) {
        if self.tile.is_empty() {
            return;
        }

        let (tile, choice) = (self.tile.clone(), self.choice.clone());
        e.update_tool_settings(SETTINGS_ID, |s: &mut TileSettings| {
            s.weathers.insert(tile, choice);
        });
    }

    /// Lights the tile with `file` (a path in the packs; empty: the default light) and remembers it for the tile.
    ///
    /// # Errors
    ///
    /// No tile, or the file can't be read.
    pub(crate) fn set_file(&mut self, e: &mut Editor, file: &str) -> Result<String, String> {
        if self.tile.is_empty() {
            return Err("no tile open".to_owned());
        }

        let msg = e.set_weather_preview(Some(file).filter(|f| !f.is_empty()))?;
        self.choice.file = e.weather_preview().unwrap_or_default().to_owned();
        self.remember(e);
        Ok(msg)
    }

    /// Sets a filter (empty: any) and remembers it for the tile.
    ///
    /// # Errors
    ///
    /// No tile, or no such category or value.
    pub(crate) fn set_filter(&mut self, e: &mut Editor, category: &str, value: &str) -> Result<(), String> {
        if self.tile.is_empty() {
            return Err("no tile open".to_owned());
        }

        let env = self.env.as_ref().ok_or_else(|| self.status.clone())?;
        let values = env.category(category).ok_or_else(|| format!("no filter '{category}'"))?.values().clone();
        let value = if value.is_empty() { String::new() } else { values.into_iter().find(|v| v.eq_ignore_ascii_case(value)).ok_or_else(|| format!("{category} has no '{value}'"))? };
        self.choice.query.insert(category.to_owned(), value);
        self.remember(e);
        Ok(())
    }

    pub(crate) fn set_search(&mut self, search: &str) {
        search.clone_into(&mut self.search);
    }

    pub(crate) fn draw(&mut self, e: &mut Editor) {
        if !gui::collapsing_header("Weather / climate (which environment the game picks)", 0) {
            return;
        }

        // The light: the chosen file, followed live while the Weather tool edits it.
        let live = e.weather_preview_document().and_then(|id| e.docs.find(id)).map(|d| d.modified());
        let lit_by = e.weather_preview().map_or_else(|| "the default light".to_owned(), str::to_owned);
        gui::text_disabled("lit by");
        gui::same_line();
        gui::text(&lit_by);
        if e.weather_preview().is_some() {
            gui::same_line();
            if gui::small_button(&format!("{}##noweather", icons::X)) {
                self.apply(e, "");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Back to the default light");
            }

            match live {
                Some(true) => gui::text_colored(theme::modified(), "with the unsaved edits of its open document"),
                Some(false) => gui::text_disabled("from its open document"),
                None => {}
            }
        }

        let Some(env) = &self.env else {
            gui::text_disabled(&self.status);
            return;
        };

        let mut filter_changed = None;
        for (label, category) in FILTERS {
            let value = self.choice.query.get(category).cloned().unwrap_or_default();
            let vals = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
            gui::set_next_item_width(220.0);
            if gui::begin_combo(label, if value.is_empty() { "(any)" } else { &value }) {
                if gui::selectable("(any)", value.is_empty(), 0) {
                    filter_changed = Some((category, String::new()));
                }

                for v in &vals {
                    if gui::selectable(v, *v == value, 0) {
                        filter_changed = Some((category, v.clone()));
                    }
                }

                gui::end_combo();
            }
        }

        if let Some((category, value)) = filter_changed {
            let _ = self.set_filter(e, category, &value);
        }

        gui::set_next_item_width(-1.0);
        gui::input_text("##weather_search", Some(&format!("{}  search the files (words are and-ed)", icons::SEARCH)), &mut self.search, 256, 0);

        let (files, used_default_map) = self.matching();
        let map = self.choice.query.get("map").cloned().unwrap_or_default();
        if used_default_map {
            gui::text_colored(theme::warning(), &format!("Nothing matched map '{map}', so the game asks again with map 'default_map'."));
        }

        gui::text_disabled(&format!("{} files{}; the game picks one at random. Click one to light the tile with it.", files.len(), if self.search.trim().is_empty() { "" } else { " found" }));
        gui::begin_child("envlist", gui::v2(0.0, 180.0), gui::CHILD_BORDERS, 0);
        let chosen = e.weather_preview().map(str::to_owned);
        let mut pick = None;
        let mut open = None;
        for file in &files {
            let key = normalize(file);
            if gui::selectable(file, chosen.as_deref() == Some(key.as_str()), 0) {
                pick = Some(file.clone());
            }

            if e.can_open(file) && gui::begin_popup_context_item(&format!("##ctx{file}")) {
                if gui::menu_item("Open in its tool", None, false, true) {
                    open = Some(file.clone());
                }

                gui::end_popup();
            }
        }

        gui::end_child();
        if let Some(file) = pick {
            self.apply(e, &file);
        }

        if let Some(file) = open {
            match e.open_path(&file, true, true) {
                Ok(m) => e.status(&m, false),
                Err(m) => e.status(&m, true),
            }
        }
    }

    /// `set_file` with the result in the status line.
    fn apply(&mut self, e: &mut Editor, file: &str) {
        match self.set_file(e, file) {
            Ok(m) => e.status(&m, false),
            Err(m) => e.status(&m, true),
        }
    }
}
