//! The Tile panel's Weather section: which environment files the game picks for the tile (the kit's environment_map.xml under the filters; for
//! a tile outside a kit, like an exported one decompiled into the workspace, or with a mod pack that overrides the variation database, the
//! packs' variation database),
//! searchable by text, and the one that lights the tile. Nothing here needs the Weather tool: the file is read from the packs (see
//! `Editor::set_weather_preview`); when the Weather tool has the same file open, the tile follows its edits.
//!
//! Each tile keeps its choice and its filters in the settings (`[tools.tile] weathers`, by the tile's folder). A tile without a weather file
//! gets the first dry one without snow the filters allow.
//!
//! The climate and the season are set in one place, the climate and season chips: they are the tile's terrain look (the ground's textures,
//! its trees and grass), and the weather's climate and season filters follow it (`follow_look`), as the game lights a battle with its
//! climate's and season's weather. The Weather panel shows those two filters without changing them.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use rpfm_lib::files::Decodeable;
use rpfm_lib::files::asset_variation_db::AssetVariationDb;
use wce_editor::chips::{begin_chip_popup, chip};
use wce_editor::editor::Editor;
use wce_editor::{gui, icons, theme};
use wce_editor_core::document::Document;
use wce_foundation::log;
use wce_scene::render_scene::DocumentId;
use wce_tile_editor::tile_document::TileDocument;
use wce_vfs::normalize;

use crate::settings::{SETTINGS_ID, TileSettings, TileWeather};

/// The game's variation database in the packs (what the Weather tool's picker reads too).
const PACKS_DB: &str = "warscape_asset_variation_db/warscape_asset_variation_db.bin";

/// The filters: as the panel labels them, their categories in environment_map.xml, and what the game sends for them when it picks a
/// battle's weather (docs/re/variation_db.md, "What the engine asks for").
pub const FILTERS: [(&str, &str, &str); 8] = [
    (
        "campaign",
        "map",
        "The campaign the battle is fought in (its map's name): each has its own set of weathers. The game asks again with default_map \
         when its map has none, so default_map is every other campaign (Attila's main one among them) and custom battles. Follows the \
         Map context's map while it shows one.",
    ),
    ("climate", "climate", "The tile's climate, set by the climate chip over the viewport (the ground follows it too)."),
    ("season", "season", "The tile's season, set by the season chip over the viewport (the ground follows it too)."),
    (
        "climate change",
        "climate_change_index",
        "The campaign's climate change phase (cc0 at its start; Attila's darkening world raises it with the turns, or with the \
         leading player's imperium). cc0 outside a campaign.",
    ),
    ("precipitation", "precipitation", "Dry, rain, snow or fog: the battle's weather as the campaign rolled it."),
    ("time of day", "time_of_day", "A day or a night battle."),
    ("battle type", "battle_types", "Land or sea. A tile is a land battle, so it is always land here."),
    (
        "snow attrition",
        "snow_attrition",
        "Whether the battle is where the campaign has snow attrition: on picks among the snow weathers (snowfall in any season, dry \
         winter days), off among the others. (What sets it in the game is not traced; the files say so much.)",
    ),
];

/// The filters that follow the tile's terrain look (set by the climate and season chips), not set on their own.
const LOOK_FILTERS: [&str; 2] = ["climate", "season"];

/// The filters fixed for a tile: (category, the value it always has).
const FIXED_FILTERS: [(&str, &str); 1] = [("battle_types", "land")];

/// A filter's value as the panel shows it.
fn value_name(category: &str, value: &str) -> String {
    let named = match (category, value) {
        ("map", "cha_attila_map") => "Age of Charlemagne",
        ("map", "bel_attila_map") => "The Last Roman",
        ("map", "default_map") => "any other, custom battles",
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

/// The weather's own chips over the viewport (after the climate and season ones): icon, category, what it is.
const CHIPS: [(&str, &str, &str); 2] =
    [(icons::CLOUD_RAIN, "precipitation", "precipitation"), (icons::CLOCK, "time_of_day", "time of day")];

/// The game's four battle seasons, as the terrain look and the weather database spell them.
const SEASONS: [&str; 4] = ["sp", "su", "au", "wi"];

#[derive(Default)]
pub(crate) struct WeatherUi {
    /// The kit's environment_map.xml, else the packs' variation database.
    env: Option<AssetVariationDb>,
    status: String,

    /// The tile shown (its folder, the document's `source_key`); its choice and filters are in the settings.
    tile: String,
    choice: TileWeather,

    /// Words (and-ed) the listed files must contain.
    search: String,

    /// The terrain look's climate and season the filters were last set from (`follow_look`).
    look: (String, String),

    /// The Map context's map the campaign filter was last set from (`follow_campaign`).
    campaign: Option<String>,
}

/// True when `file` holds every word of `search` (case-insensitive).
fn matches_words(file: &str, search: &str) -> bool {
    let file = file.to_lowercase();
    search.to_lowercase().split_whitespace().all(|w| file.contains(w))
}

impl WeatherUi {
    /// The open tile's weather: its file and its list's filters.
    pub(crate) fn choice(&self) -> &TileWeather {
        &self.choice
    }

    /// The tile the choice is for (the document's `source_key`).
    pub(crate) fn tile(&self) -> &str {
        &self.tile
    }

    /// A tile became the one the tool works on: read its kit's environment map, take up its remembered weather (or the filters of its look),
    /// and light it.
    pub(crate) fn tile_changed(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let dir = d.project().dir.clone();
        let look = d.tile().terrain_look().clone();
        self.tile = d.source_key();
        self.load_database(e, &dir);

        self.look = Default::default();
        self.campaign = None;
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

        for (category, value) in FIXED_FILTERS {
            let value = self.value_of(category, value).unwrap_or_default();
            self.choice.query.insert(category.to_owned(), value);
        }

        if self.choice.file.is_empty()
            && let Some(file) = self.first_dry()
        {
            match self.set_file(e, &file) {
                Ok(_) => log::info(format!("the tile's weather: {file} (the first dry one)")),
                Err(m) => e.status(&format!("the tile's weather: {m}"), true),
            }

            return;
        }

        let file = self.choice.file.clone();
        if let Err(m) = e.set_weather_preview(Some(&file).filter(|f| !f.is_empty()).map(String::as_str)) {
            e.status(&format!("the tile's weather: {m}"), true);
        }
    }

    /// The tile's climate or season changed (the chips, its map context's placement, `tile_terrain`), or the tile was opened: the
    /// weather's climate and season filters become the terrain look's (any when the database has no such value), and a weather they no
    /// longer let through gives way to the first dry one without snow they do, as the game lights a battle with its climate's and season's
    /// weather.
    pub(crate) fn follow_look(&mut self, e: &mut Editor, climate: &str, season: &str) {
        if self.tile.is_empty() || self.env.is_none() || (self.look.0 == climate && self.look.1 == season) {
            return;
        }

        self.look = (climate.to_owned(), season.to_owned());
        let mut changed = false;
        for (category, value) in [("climate", climate), ("season", season)] {
            let value = self.value_of(category, value).unwrap_or_default();
            if self.choice.query.get(category) != Some(&value) {
                self.choice.query.insert(category.to_owned(), value);
                changed = true;
            }
        }

        if !changed {
            return;
        }

        self.remember(e);
        self.relight(e, &format!("{climate}, {}", crate::ground_views::season_name(season)));
    }

    /// The Map context's map changed (`None`: it shows none): the campaign filter becomes that map, else `default_map` when the database
    /// has no such map (as the game asks again with it). Only on a change, so a campaign picked by hand stays until the next map.
    pub(crate) fn follow_campaign(&mut self, e: &mut Editor, map: Option<&str>) {
        if self.tile.is_empty() || self.env.is_none() || self.campaign.as_deref() == map {
            return;
        }

        self.campaign = map.map(str::to_owned);
        let Some(map) = map else { return };
        let value = self.value_of("map", map).or_else(|| self.value_of("map", "default_map")).unwrap_or_default();
        if self.choice.query.get("map") == Some(&value) {
            return;
        }

        self.choice.query.insert("map".to_owned(), value.clone());
        self.remember(e);
        self.relight(e, &format!("campaign {value}"));
    }

    /// The filters changed: the file lighting the tile stays if they still let it through, else (precipitation any) the first dry one
    /// without snow, else the first one; `what` names the change in the status line when nothing passes.
    fn relight(&mut self, e: &mut Editor, what: &str) {
        let files = self.passing();
        let current = e.weather_preview().map(str::to_owned);
        if current.as_deref().is_some_and(|c| files.iter().any(|f| normalize(f) == c)) {
            return;
        }

        let precipitation = self.choice.query.get("precipitation").is_some_and(|v| !v.is_empty());
        let pick = if precipitation { None } else { self.first_dry() }.or_else(|| files.first().cloned());
        match pick {
            Some(file) => self.apply(e, &file),
            None => e.status(&format!("no weather file for {what} on this map"), true),
        }
    }

    /// The files the filters let through (the search is not applied).
    fn passing(&self) -> Vec<String> {
        let Some(env) = &self.env else { return vec![] };
        let query = self.choice.query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        env.matches("environment", &query).variations.iter().map(|v| v.filename().clone()).collect()
    }

    /// The first dry weather without snow under the tile's filters: among the files the game picks with precipitation `dry` (else the files
    /// named dry, else all of them), the first one not named snow, else the first one (the search is not applied).
    fn first_dry(&self) -> Option<String> {
        let env = self.env.as_ref()?;
        let query: BTreeMap<String, String> =
            self.choice.query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        let files = |query: &BTreeMap<String, String>| {
            env.matches("environment", query).variations.iter().map(|v| v.filename().clone()).collect::<Vec<_>>()
        };
        let named = |f: &String, word: &str| f.to_lowercase().contains(word);
        let mut dry = self
            .value_of("precipitation", "dry")
            .map(|value| {
                let mut q = query.clone();
                q.insert("precipitation".to_owned(), value);
                files(&q)
            })
            .unwrap_or_default();

        if dry.is_empty() {
            let all = files(&query);
            dry = all.iter().filter(|f| named(f, "dry")).cloned().collect();
            if dry.is_empty() {
                dry = all;
            }
        }

        dry.iter().find(|f| !named(f, "snow")).or_else(|| dry.first()).cloned()
    }

    /// The database's spelling of a value of a category (case-insensitive), if it has it.
    fn value_of(&self, category: &str, value: &str) -> Option<String> {
        let values = self.env.as_ref()?.category(category)?.values().clone();
        values.into_iter().find(|v| v.eq_ignore_ascii_case(value))
    }

    /// Which environment files the game picks: the packs' variation database when a mod pack overrides it (the kit's environment_map.xml is
    /// the vanilla one, without the mod's weather), else the kit's environment_map.xml when the tile is in a kit that has one, else the packs'
    /// database (an exported tile decompiled into the workspace has no kit around it).
    fn load_database(&mut self, e: &Editor, tile_dir: &Path) {
        let kit = Self::kit_environment_map(tile_dir).filter(|_| !Self::mod_overrides_database(e));
        let (loaded, source) = match kit {
            Some(Ok(db)) => (Ok(db), "the kit's environment_map.xml"),
            Some(Err(err)) => (Err(err), "the kit's environment_map.xml"),
            None => match Self::packs_database(e) {
                Ok(db) => (Ok(db), "the packs' variation database"),
                // Rome 2 has none: the weather's folders say the same.
                Err(_) => (Self::folders_database(e), "the packs' weather folders"),
            },
        };

        match loaded {
            Ok(mut db) => {
                // The game's .bin names the category `battle_type`, the kit's environment_map.xml `battle_types`: the panel uses the
                // latter (the names change nothing in the matching, which goes by the values' bits; this copy is not saved).
                if db.category("battle_types").is_none() && db.category("battle_type").is_some() {
                    let _ = db.rename_category("battle_type", "battle_types");
                }

                let files = db.entry("environment").map_or(0, |entry| entry.variations().len());
                self.status = format!("{files} environment files ({source})");
                self.env = Some(db);
            }

            Err(err) => {
                self.status = format!("{source}: {err}");
                self.env = None;
            }
        }
    }

    /// The environment_map.xml of the kit around the tile; `None` when the tile is in no kit or the kit has none.
    fn kit_environment_map(tile_dir: &Path) -> Option<Result<AssetVariationDb, String>> {
        let g = tile_dir.display().to_string().replace('\\', "/");
        let at = g.find("/raw_data/")?;
        let p = Path::new(&g[..at + 10]).join("warscape_asset_variation_db").join("environment_map.xml");
        let bytes = std::fs::read(&p).ok()?;
        Some(AssetVariationDb::from_environment_map(&String::from_utf8_lossy(&bytes)).map_err(|err| err.to_string()))
    }

    /// If the winning variation database is a mod pack's (the vanilla one is what the kit's environment_map.xml was made from).
    fn mod_overrides_database(e: &Editor) -> bool {
        e.vfs().is_some_and(|vfs| vfs.owner(PACKS_DB).and_then(|i| vfs.packs().get(i)).is_some_and(|pack| pack.is_mod))
    }

    /// The variation database the game uses: the winning pack's .bin.
    fn packs_database(e: &Editor) -> Result<AssetVariationDb, String> {
        let vfs = e.vfs().ok_or("the game packs are not mounted")?;
        let bytes = vfs.read(PACKS_DB).map_err(|err| format!("{PACKS_DB}: {err}"))?;
        AssetVariationDb::decode(&mut Cursor::new(bytes), &None).map_err(|err| format!("{PACKS_DB}: {err}"))
    }

    /// A variation database made from the packs' weather folders, for a game without one (Rome 2), as the game chooses (fn
    /// 0x114ecc90): it lists `weather/<climate>/<season>/land/<day|night>/<precipitation>/*.environment` with the battle's season, and only
    /// when that folder has none the `default` season's (and last `weather/default/default/...`), then picks one at random. So a `default`
    /// season file is listed for the seasons whose own folder lacks its kind; the paths without a season level are never read by the game.
    /// The climate folder is the climate (`default`: every climate).
    fn folders_database(e: &Editor) -> Result<AssetVariationDb, String> {
        let vfs = e.vfs().ok_or("the game packs are not mounted")?;
        const SEASONS: [&str; 4] = ["sp", "su", "au", "wi"];

        // (climate, season, land|sea, day|night, precipitation, path)
        let mut files = Vec::new();
        for path in vfs.list("weather/", ".environment") {
            let parts = path.split('/').collect::<Vec<_>>();
            let [_, climate, season, battle, time, precipitation, _] = parts[..] else { continue };
            if !["land", "sea"].contains(&battle) || !["day", "night"].contains(&time) {
                continue;
            }

            files.push((climate.to_owned(), season.to_owned(), battle, time, precipitation, path.replace('/', "\\")));
        }

        let own: std::collections::BTreeSet<(String, String, &str, &str, &str)> =
            files.iter().filter(|f| SEASONS.contains(&f.1.as_str())).map(|f| (f.0.clone(), f.1.clone(), f.2, f.3, f.4)).collect();
        let mut xml = String::from("<environments serialise_version='1'>\n\t<environment_files>\n");
        let mut n = 0;
        for (climate, season, battle, time, precipitation, path) in &files {
            let seasons: Vec<&str> = if SEASONS.contains(&season.as_str()) {
                vec![season.as_str()]
            } else {
                SEASONS.into_iter().filter(|s| !own.contains(&(climate.clone(), (*s).to_owned(), *battle, *time, *precipitation))).collect()
            };

            if seasons.is_empty() {
                continue;
            }

            xml += &format!(
                "\t\t<environment_file serialise_version='1' filename='{path}' map='default_map' climate='{climate}' season='{}' climate_change_index='cc0' precipitation='{precipitation}' time_of_day='{time}' battle_types='{battle}' snow_attrition='snow_attrition_off,snow_attrition_on'/>\n",
                seasons.join(",")
            );
            n += 1;
        }

        if n == 0 {
            return Err("no weather files in the packs".to_owned());
        }

        xml += "\t</environment_files>\n</environments>\n";
        AssetVariationDb::from_environment_map(&xml).map_err(|err| err.to_string())
    }

    /// The files the game picks under the filters that hold the search's words, each once with the number of its variations (a file the
    /// database lists twice, for different maps or climates, is picked twice as often), and whether the game asked again with `default_map`.
    pub(crate) fn matching(&self) -> (Vec<(String, usize)>, bool) {
        let Some(env) = &self.env else { return (vec![], false) };
        let query = self.choice.query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        let res = env.matches("environment", &query);
        let mut files: Vec<(String, usize)> = vec![];
        for file in res.variations.iter().map(|v| v.filename()).filter(|f| matches_words(f, &self.search)) {
            let key = normalize(file);
            match files.iter_mut().find(|(f, _)| normalize(f) == key) {
                Some((_, n)) => *n += 1,
                None => files.push((file.clone(), 1)),
            }
        }

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

        if LOOK_FILTERS.contains(&category) {
            return Err(format!(
                "the {category} is the tile's: set it with the climate and season chips (or tile_terrain), the weather follows"
            ));
        }

        if let Some((_, fixed)) = FIXED_FILTERS.iter().find(|(c, _)| *c == category) {
            return Err(format!("the {category} of a tile is always {fixed}"));
        }

        let env = self.env.as_ref().ok_or_else(|| self.status.clone())?;
        let values = env.category(category).ok_or_else(|| format!("no filter '{category}'"))?.values().clone();
        let value = if value.is_empty() {
            String::new()
        } else {
            values.into_iter().find(|v| v.eq_ignore_ascii_case(value)).ok_or_else(|| format!("{category} has no '{value}'"))?
        };
        self.choice.query.insert(category.to_owned(), value);
        self.remember(e);
        Ok(())
    }

    pub(crate) fn set_search(&mut self, search: &str) {
        search.clone_into(&mut self.search);
    }

    /// The Weather panel: the light, every filter, the files they let through (searchable); a click lights the tile with one.
    pub(crate) fn draw(&mut self, e: &mut Editor) {
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
        for (label, category, tip) in FILTERS {
            let value = self.choice.query.get(category).cloned().unwrap_or_default();
            let vals = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
            gui::set_next_item_width(260.0);
            let set_elsewhere = LOOK_FILTERS.contains(&category) || FIXED_FILTERS.iter().any(|(c, _)| *c == category);
            gui::begin_disabled(set_elsewhere);
            let shown = if value.is_empty() { "(any)".to_owned() } else { value_name(category, &value) };
            let open = gui::begin_combo(label, &shown);
            if open {
                if gui::selectable("(any)", value.is_empty(), 0) {
                    filter_changed = Some((category, String::new()));
                }

                for v in &vals {
                    if gui::selectable(&value_name(category, v), *v == value, 0) {
                        filter_changed = Some((category, v.clone()));
                    }
                }

                gui::end_combo();
            }

            gui::end_disabled();
            if gui::is_item_hovered_allow_disabled() {
                gui::set_tooltip(&format!("{tip}\n(any): the files of every {label}."));
            }
        }

        if let Some((category, value)) = filter_changed {
            let _ = self.set_filter(e, category, &value);
        }

        gui::set_next_item_width(-1.0);
        gui::input_text(
            "##weather_search",
            Some(&format!("{}  search the files (words are and-ed)", icons::SEARCH)),
            &mut self.search,
            256,
            0,
        );

        let (files, used_default_map) = self.matching();
        let map = self.choice.query.get("map").cloned().unwrap_or_default();
        if used_default_map {
            gui::text_colored(theme::warning(), &format!("Nothing matched map '{map}', so the game asks again with map 'default_map'."));
        }

        let variations = files.iter().map(|(_, n)| n).sum::<usize>();
        gui::text_disabled(&format!(
            "{} files{} ({variations} variations); the game picks a variation at random. Click a file to light the tile with it.",
            files.len(),
            if self.search.trim().is_empty() { "" } else { " found" },
        ));
        gui::begin_child("envlist", gui::v2(0.0, 180.0), gui::CHILD_BORDERS, 0);
        let chosen = e.weather_preview().map(str::to_owned);
        let mut pick = None;
        let mut open = None;
        for (i, (file, n)) in files.iter().enumerate() {
            gui::push_id_int(i as i32);
            let key = normalize(file);
            let label = if *n > 1 { format!("{file}  (x{n})") } else { file.clone() };
            if gui::selectable(&label, chosen.as_deref() == Some(key.as_str()), 0) {
                pick = Some(file.clone());
            }

            if *n > 1 && gui::is_item_hovered() {
                gui::set_tooltip(&format!(
                    "The database lists this file {n} times (for different maps or climates): the game picks it {n} times as often."
                ));
            }

            if e.can_open(file) && gui::begin_popup_context_item("##ctx") {
                if gui::menu_item("Open in its tool", None, false, true) {
                    open = Some(file.clone());
                }

                if gui::is_item_hovered() {
                    gui::set_tooltip("Opens the file in the Weather tool.");
                }

                gui::end_popup();
            }

            gui::pop_id();
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

    /// Sets a filter from a chip and lights the tile with a file it lets through (`relight`).
    fn chip_changed(&mut self, e: &mut Editor, category: &str, value: &str) {
        if let Err(m) = self.set_filter(e, category, value) {
            e.status(&m, true);
            return;
        }

        self.relight(e, &format!("{category} {value}"));
    }

    /// Whether some file passes the filters with `category` set to `value`.
    fn passes(&self, category: &str, value: &str) -> bool {
        let Some(env) = &self.env else { return false };
        let mut query: BTreeMap<String, String> =
            self.choice.query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        if value.is_empty() {
            query.remove(category);
        } else {
            query.insert(category.to_owned(), value.to_owned());
        }

        !env.matches("environment", &query).variations.is_empty()
    }

    /// A climate or season chip: its value's name in the tooltip, its popup listing `items` (value, name); a value no weather file has is
    /// marked, and can be picked all the same (the ground has it). The value picked.
    fn look_chip(&self, icon: &str, category: &str, current: &str, items: &[(String, String)]) -> Option<String> {
        let name = items.iter().find(|(v, _)| v.eq_ignore_ascii_case(current)).map_or(current, |(_, n)| n.as_str());
        let tip = format!(
            "{category}: {name}\nThe tile's {category}: the ground's textures, its trees and grass, and the weather files the game picks.\nClick: change it"
        );
        if chip(false, icon, &format!("button{category}"), &tip) {
            gui::open_popup(&format!("chip{category}"));
        }

        gui::same_line_touching();
        if !begin_chip_popup(&format!("chip{category}")) {
            return None;
        }

        let mut picked = None;
        gui::text_disabled(category);
        for (value, name) in items {
            let weather = self.env.is_none() || self.value_of(category, value).is_some_and(|v| self.passes(category, &v));
            let label = if weather { name.clone() } else { format!("{name}  (no weather)") };
            if gui::selectable(&label, value.eq_ignore_ascii_case(current), 0) {
                picked = Some(value.clone());
            }

            if !weather && gui::is_item_hovered() {
                gui::set_tooltip(
                    "No weather file of this map has it (with the other filters): the ground takes it, the weather is not filtered by it.",
                );
            }
        }

        gui::end_popup();
        picked
    }

    /// The climate and season chips: the tile's terrain look (the climates of its tile database, the four battle seasons); the weather
    /// follows a change (`follow_look`).
    fn draw_look_chips(&mut self, e: &mut Editor) {
        let Some(id) = crate::tool::document_id(e) else { return };
        let Some(d) = e.docs.find_as::<TileDocument>(id) else { return };
        let mut look = d.tile().terrain_look().clone();
        let mut climates = d.tile().climates().climates.iter().map(|c| (c.name.clone(), c.name.clone())).collect::<Vec<_>>();
        if climates.is_empty() {
            let values = self.env.as_ref().and_then(|env| env.category("climate")).map(|m| m.values().clone()).unwrap_or_default();
            climates = values.into_iter().map(|v| (v.clone(), v)).collect();
        }

        let seasons = SEASONS.map(|s| (s.to_owned(), crate::ground_views::season_name(s).to_owned()));
        let climate = self.look_chip(icons::GLOBE, "climate", &look.climate, &climates);
        let season = self.look_chip(icons::LEAF, "season", &look.season, &seasons);
        if climate.is_none() && season.is_none() {
            return;
        }

        look.climate = climate.unwrap_or(look.climate);
        look.season = season.unwrap_or(look.season);
        let Some(d) = e.docs.find_as_mut::<TileDocument>(id) else { return };
        d.tile_mut().set_terrain_look(&mut e.scene, look.clone());
        self.follow_look(e, &look.climate, &look.season);
    }

    /// The chips over the viewport: climate and season (the tile's, `draw_look_chips`), the weather's precipitation and time of day (a
    /// value no file passes is greyed), then the file lighting the tile (icons, the values in their tooltips; a click on the cloud opens
    /// the Weather panel) and a pencil that edits it in the Weather tool (the tile follows the edits).
    pub(crate) fn draw_chips(&mut self, e: &mut Editor) {
        self.draw_look_chips(e);
        let Some(env) = &self.env else { return };
        let mut changed = None;
        for (icon, category, label) in CHIPS {
            let value = self.choice.query.get(category).cloned().unwrap_or_default();
            let values = env.category(category).map(|m| m.values().clone()).unwrap_or_default();
            // Icon only (lit while it filters); the value in the tooltip.
            let tip = format!("{label}: {}", if value.is_empty() { "any" } else { &value });
            if chip(!value.is_empty(), icon, &format!("button{category}"), &tip) {
                gui::open_popup(&format!("chip{category}"));
            }

            gui::same_line_touching();
            if begin_chip_popup(&format!("chip{category}")) {
                gui::text_disabled(label);
                if gui::selectable("any", value.is_empty(), 0) {
                    changed = Some((category, String::new()));
                }

                for v in &values {
                    let ok = self.passes(category, v);
                    gui::begin_disabled(!ok);
                    if gui::selectable(v, *v == value, 0) {
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

        let file = e.weather_preview().map(str::to_owned);
        let tip = format!("Lit by {}\nClick: every filter and file (the Weather panel)", file.as_deref().unwrap_or("the default light"));
        if chip(false, icons::CLOUD_SUN, "weatherfile", &tip) {
            e.pending_focus = Some("Weather".to_owned());
        }

        if let Some(f) = file {
            gui::same_line_touching();
            if chip(
                false,
                icons::PENCIL,
                "editweather",
                "Edit it in the Weather tool: the tile follows the edits; Save writes the file into the project, which every tile reads from then on",
            ) {
                self.edit(e, &f);
            }
        }

        if let Some((category, value)) = changed {
            self.chip_changed(e, category, &value);
        }
    }

    /// Opens the weather file in the Weather tool (the project's copy when it has one): the tile follows its edits, and Save writes it
    /// into the project's loose folder, which every tool reads before the packs.
    pub(crate) fn edit(&mut self, e: &mut Editor, file: &str) {
        match e.open_path(file, true, true) {
            Ok(m) => e.status(&m, false),
            Err(m) => e.status(&m, true),
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
