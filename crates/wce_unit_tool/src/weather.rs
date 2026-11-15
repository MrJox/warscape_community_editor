//! The stage's weather, set as the Tile tool sets a tile's: chips at the top left of the viewport, beside the compass. Climate and season
//! (they pick the ground's grass too), precipitation, time of day and climate change filter the weather files of the packs' variation
//! database; the cloud lists the files they let through (searchable, with the campaign and snow attrition filters) and a click lights the
//! stage with one; the pencil opens it in the Weather tool. A filter that the file lighting the stage no longer passes gives way to the
//! first dry one without snow that does. The filters and the file are the tool's settings (`[tools.unit] weather`, `weather_query`).

use std::collections::BTreeMap;
use std::io::Cursor;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use wce_editor::chips::{begin_chip_popup, chip};
use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_vfs::normalize;

use super::{UNIT_TOOL_ID, UnitSettings, UnitTool};

/// The game's variation database in the packs.
const PACKS_DB: &str = "warscape_asset_variation_db/warscape_asset_variation_db.bin";

/// The chips' filters: icon, category, label.
const CHIPS: [(&str, &str, &str); 5] = [
    (icons::GLOBE, "climate", "climate"),
    (icons::LEAF, "season", "season"),
    (icons::CLOUD_RAIN, "precipitation", "precipitation"),
    (icons::CLOCK, "time_of_day", "time of day"),
    (icons::CLOUD_FOG, "climate_change_index", "climate change"),
];

/// The filters in the cloud chip's popup.
const POPUP_FILTERS: [(&str, &str); 2] = [("campaign", "map"), ("snow attrition", "snow_attrition")];

/// What a stage always is: a land battle.
const FIXED: (&str, &str) = ("battle_types", "land");

/// A filter's value as the chips show it.
fn value_name(category: &str, value: &str) -> String {
    if category == "climate_change_index"
        && let Some(n) = value.strip_prefix("cc")
    {
        return format!("phase {n} ({value})");
    }

    let named = match (category, value) {
        ("map", "default_map") => "any other: the main campaign, custom battles",
        ("season", "sp") => "spring",
        ("season", "su") => "summer",
        ("season", "au") => "autumn",
        ("season", "wi") => "winter",
        ("snow_attrition", "snow_attrition_on") => "on",
        ("snow_attrition", "snow_attrition_off") => "off",
        _ => return value.to_owned(),
    };
    format!("{named} ({value})")
}

/// The stage's weather database, read once a game.
#[derive(Default)]
pub(super) struct StageWeather {
    env: Option<AssetVariationDb>,
    status: String,
    loaded: bool,
    search: String,

    /// The chips' width last frame (they go beside the compass when there is room).
    pub width: f32,
}

impl UnitTool {
    fn weather_db(&mut self, e: &Editor) {
        if self.weather.loaded {
            return;
        }

        let Some(vfs) = e.vfs() else { return };
        self.weather.loaded = true;
        let read = vfs
            .read(PACKS_DB)
            .map_err(|err| err.to_string())
            .and_then(|b| AssetVariationDb::decode(&mut Cursor::new(b), &None).map_err(|err| err.to_string()));
        match read {
            Ok(db) => self.weather.env = Some(db),
            Err(err) => self.weather.status = format!("no weather database ({PACKS_DB}: {err}): every weather file is listed"),
        }
    }

    /// The filters set (any left out), with the stage's fixed one.
    fn weather_query(&self) -> BTreeMap<String, String> {
        let mut q: BTreeMap<String, String> =
            self.settings.weather_query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(v) = self.value_of(FIXED.0, FIXED.1) {
            q.insert(FIXED.0.to_owned(), v);
        }

        // The campaign until one is picked: `default_map` (the main campaign, custom battles), as a tile's.
        if !self.settings.weather_query.contains_key("map")
            && let Some(v) = self.value_of("map", "default_map")
        {
            q.insert("map".to_owned(), v);
        }

        q
    }

    /// The database's spelling of a value (case-insensitive).
    fn value_of(&self, category: &str, value: &str) -> Option<String> {
        let values = self.weather.env.as_ref()?.category(category)?.values().clone();
        values.into_iter().find(|v| v.eq_ignore_ascii_case(value))
    }

    fn files_of(&self, query: &BTreeMap<String, String>) -> Vec<String> {
        let Some(env) = &self.weather.env else { return vec![] };
        let mut out: Vec<String> = vec![];
        for f in env.matches("environment", query).variations.iter().map(|v| v.filename()) {
            if !out.iter().any(|o| normalize(o) == normalize(f)) {
                out.push(f.clone());
            }
        }

        out
    }

    /// Whether some file passes the filters with `category` set to `value`.
    fn weather_passes(&self, category: &str, value: &str) -> bool {
        let mut q = self.weather_query();
        q.insert(category.to_owned(), value.to_owned());
        !self.files_of(&q).is_empty()
    }

    /// The first dry weather without snow the filters let through, else the first.
    fn first_dry(&self) -> Option<String> {
        let query = self.weather_query();
        let mut dry = self
            .value_of("precipitation", "dry")
            .map(|v| {
                let mut q = query.clone();
                q.insert("precipitation".to_owned(), v);
                self.files_of(&q)
            })
            .unwrap_or_default();
        if dry.is_empty() {
            dry = self.files_of(&query);
        }

        dry.iter().find(|f| !f.to_lowercase().contains("snow")).or_else(|| dry.first()).cloned()
    }

    /// Lights the stage with a file (empty: the game's default light).
    pub(super) fn set_weather_file(&mut self, e: &mut Editor, file: &str) {
        let file = normalize(file);
        e.update_tool_settings::<UnitSettings>(UNIT_TOOL_ID, |s| s.weather.clone_from(&file));
        self.settings.weather = file;
    }

    /// Sets a filter (empty: any); a file lighting the stage that it no longer lets through gives way to the first dry one that it does.
    pub(super) fn set_weather_filter(&mut self, e: &mut Editor, category: &str, value: &str) -> Result<String, String> {
        self.weather_db(e);
        let value = if value.is_empty() || self.weather.env.is_none() {
            value.to_owned()
        } else {
            let values = self.weather.env.as_ref().and_then(|env| env.category(category)).map(|m| m.values().clone());
            let values = values.ok_or_else(|| format!("no filter '{category}'"))?;
            values
                .iter()
                .find(|v| v.eq_ignore_ascii_case(value))
                .cloned()
                .ok_or_else(|| format!("{category} has no '{value}' (it has: {})", values.join(", ")))?
        };
        let (c, v) = (category.to_owned(), value.clone());
        e.update_tool_settings::<UnitSettings>(UNIT_TOOL_ID, |s| {
            s.weather_query.insert(c, v);
        });
        self.settings = e.tool_settings(UNIT_TOOL_ID);
        if self.weather.env.is_none() {
            return Ok(String::new());
        }

        let files = self.files_of(&self.weather_query());
        let current = self.settings.weather.clone();
        if !current.is_empty() && files.iter().any(|f| normalize(f) == current) {
            return Ok(current);
        }

        match self.first_dry() {
            Some(f) => {
                self.set_weather_file(e, &f);
                Ok(normalize(&f))
            }

            None => Err(format!("no weather file for {category} {value}")),
        }
    }

    /// The grass the stage stands on: the climate's and season's, else the climate's summer, else the default climate's.
    pub(super) fn ground_texture(&self, e: &Editor) -> String {
        let climate = self.settings.weather_query.get("climate").cloned().unwrap_or_default();
        let season = self.settings.weather_query.get("season").cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| "su".to_owned());
        let path = |c: &str, s: &str| format!("terrain/textures/battle/{c}/grass/0_{s}_diffuse.dds");
        let exists = |p: &String| e.vfs().is_some_and(|v| v.owner(p).is_some());
        let climates: Vec<&str> = if climate.is_empty() { vec!["default"] } else { vec![climate.as_str(), "default"] };
        for c in climates {
            for s in [season.as_str(), "su"] {
                let p = path(c, s);
                if exists(&p) {
                    return p;
                }
            }
        }

        path("default", "su")
    }

    /// The chips over the viewport, at its top left beside the compass (under it when the image is too narrow for both), as the Tile tool's.
    pub(super) fn draw_weather_chips(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        if size.x < 200.0 {
            return;
        }

        self.weather_db(e);
        let compass_left = origin.x + size.x * 0.5 - (size.x - 16.0).min(360.0) * 0.5;
        let beside = !e.viewport.show_compass || origin.x + 8.0 + self.weather.width + 8.0 <= compass_left;
        gui::set_cursor_screen_pos(gui::v2(origin.x + 8.0, origin.y + if beside { 6.0 } else { 58.0 }));
        gui::push_style_color(gui::COL_CHILD_BG, gui::v4(0.08, 0.08, 0.09, 0.78));
        if gui::begin_pill("##unitweatherchips") {
            self.weather_chips(e);
        }

        gui::end_child();
        let (lo, hi) = gui::item_rect();
        self.weather.width = hi.x - lo.x;
        gui::pop_style_color(1);
    }

    fn weather_chips(&mut self, e: &mut Editor) {
        let mut changed = None;
        if let Some(env) = &self.weather.env {
            for (icon, category, label) in CHIPS {
                let value = self.settings.weather_query.get(category).cloned().unwrap_or_default();
                let values = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
                let ground = if matches!(category, "climate" | "season") { " (the ground's grass follows it)" } else { "" };
                let tip = format!(
                    "{label}: {}{ground}\nClick: pick one",
                    if value.is_empty() { "any".to_owned() } else { value_name(category, &value) }
                );
                if chip(!value.is_empty(), icon, &format!("unitbutton{category}"), &tip) {
                    gui::open_popup(&format!("unitchip{category}"));
                }

                gui::same_line_touching();
                if begin_chip_popup(&format!("unitchip{category}")) {
                    gui::text_disabled(label);
                    if gui::selectable("any", value.is_empty(), 0) {
                        changed = Some((category, String::new()));
                    }

                    for v in &values {
                        let ok = self.weather_passes(category, v);
                        gui::begin_disabled(!ok);
                        if gui::selectable(&value_name(category, v), *v == value, 0) {
                            changed = Some((category, v.clone()));
                        }

                        if !ok && gui::is_item_hovered_allow_disabled() {
                            gui::set_tooltip("No weather file has this value with the other filters as they are.");
                        }

                        gui::end_disabled();
                    }

                    gui::end_popup();
                }
            }
        }

        let file = if self.settings.weather.is_empty() { None } else { Some(self.settings.weather.clone()) };
        let tip = format!(
            "Lit by {}\nClick: the files the filters let through (search, campaign, snow attrition); a click lights the stage with one",
            file.as_deref().unwrap_or("the game's default light")
        );
        if chip(false, icons::CLOUD_SUN, "unitweatherfile", &tip) {
            gui::open_popup("unitchipweatherfile");
        }

        if begin_chip_popup("unitchipweatherfile") {
            if let Some(c) = self.files_popup(e) {
                changed = Some(c);
            }

            gui::end_popup();
        }

        if let Some(f) = &file {
            gui::same_line_touching();
            if chip(false, icons::PENCIL, "uniteditweather", "Edit it in the Weather tool") {
                match e.open_path(f, true, true) {
                    Ok(m) => e.status(&m, false),
                    Err(m) => e.status(&m, true),
                }
            }
        }

        if let Some((category, value)) = changed {
            match self.set_weather_filter(e, category, &value) {
                Ok(f) if !f.is_empty() => e.status(&format!("the stage's weather: {f}"), false),
                Ok(_) => {}
                Err(m) => e.status(&m, true),
            }
        }
    }

    /// The cloud chip's popup: the light, the rarer filters, the files they let through (searchable); a click lights the stage with one.
    fn files_popup(&mut self, e: &mut Editor) -> Option<(&'static str, String)> {
        let lit_by = if self.settings.weather.is_empty() { "the game's default light".to_owned() } else { self.settings.weather.clone() };
        gui::text_disabled("lit by");
        gui::same_line();
        gui::text(&lit_by);
        if !self.settings.weather.is_empty() {
            gui::same_line();
            if gui::small_button(&format!("{}##unitnoweather", icons::X)) {
                self.set_weather_file(e, "");
            }

            if gui::is_item_hovered() {
                gui::set_tooltip("Back to the game's default light");
            }
        }

        let mut changed = None;
        if let Some(env) = &self.weather.env {
            let query = self.weather_query();
            for (label, category) in POPUP_FILTERS {
                let value = query.get(category).cloned().unwrap_or_default();
                let values = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
                gui::set_next_item_width(260.0);
                let shown = if value.is_empty() { "(any)".to_owned() } else { value_name(category, &value) };
                if gui::begin_combo(label, &shown) {
                    if gui::selectable("(any)", value.is_empty(), 0) {
                        changed = Some((category, String::new()));
                    }

                    for v in &values {
                        if gui::selectable(&value_name(category, v), *v == value, 0) {
                            changed = Some((category, v.clone()));
                        }
                    }

                    gui::end_combo();
                }
            }
        } else {
            gui::text_colored(theme::warning(), &self.weather.status);
        }

        gui::set_next_item_width(560.0);
        gui::input_text(
            "##unit_weather_search",
            Some(&format!("{}  search the files (words are and-ed)", icons::SEARCH)),
            &mut self.weather.search,
            256,
            0,
        );
        let words: Vec<String> = self.weather.search.to_lowercase().split_whitespace().map(str::to_owned).collect();
        let all = if self.weather.env.is_some() { self.files_of(&self.weather_query()) } else { self.listings(e).weathers.clone() };
        let files: Vec<String> = all.into_iter().filter(|f| words.iter().all(|w| f.to_lowercase().contains(w))).collect();
        gui::text_disabled(&format!("{} files; click one to light the stage with it", files.len()));
        gui::begin_child("unitenvlist", gui::v2(560.0, 260.0), gui::CHILD_BORDERS, 0);
        let mut pick = None;
        for (i, f) in files.iter().enumerate() {
            gui::push_id_int(i as i32);
            if gui::selectable(f, normalize(f) == self.settings.weather, 0) {
                pick = Some(f.clone());
            }

            gui::pop_id();
        }

        gui::end_child();
        if let Some(f) = pick {
            self.set_weather_file(e, &f);
        }

        changed
    }
}
