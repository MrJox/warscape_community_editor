//! Choosing game data from lists instead of typing keys: the units (with their names, and which faction fields them), factions,
//! weather files, VFX, sound and music events, animation slots and battle maps, read from the packs once a mount; and the picker
//! that shows them (a combo with a filter: typing narrows the list, names first, keys after).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;

use rpfm_lib::files::loc::Loc;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_audio::library::SoundLibrary;
use wce_audio::sound_db::table_rows;
use wce_editor::gui;
use wce_vfs::files::Files;

/// One choice: its key and what the user reads (a name, or the key again).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item {
    pub key: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct GameLists {
    pub units: Vec<Item>,
    /// Faction key -> the units it fields (`unit_variants`).
    pub units_of_faction: HashMap<String, BTreeSet<String>>,
    pub factions: Vec<Item>,
    pub environments: Vec<Item>,
    pub vfx: Vec<Item>,
    pub battle_maps: Vec<Item>,
    /// Filled once the sound library is there.
    pub sound_events: Vec<Item>,
    pub music_events: Vec<Item>,
}

/// Every slot a unit's animation fragments commonly have, for the action keys.
pub const SLOTS: &[&str] = &[
    "AUTO",
    "STAND",
    "STAND_IDLE_1",
    "WALK",
    "RUN",
    "CHARGE",
    "ATTACK_1",
    "ATTACK_2",
    "ATTACK_3",
    "DEFEND_1",
    "FIRE",
    "RELOAD",
    "TAUNT",
    "CELEBRATE",
    "CHEER",
    "SALUTE",
    "DIE",
    "DIE_FRONT",
    "DIE_BACK",
    "KNOCKED_BACK",
    "PANIC",
    "ROUT",
    "KNEEL",
    "PRAY",
];

fn loc_names(files: &dyn Files, file_part: &str, prefix: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for path in files.list("text/", ".loc").into_iter().filter(|p| p.contains(file_part)) {
        let Ok(bytes) = files.read(&path) else { continue };
        let Ok(loc) = Loc::decode(&mut Cursor::new(&bytes), &Some(DecodeableExtraData::default())) else { continue };
        for row in loc.data().iter() {
            let (Some(k), Some(v)) = (row.first(), row.get(1)) else { continue };
            if let Some(key) = k.data_to_string().strip_prefix(prefix) {
                out.insert(key.to_owned(), v.data_to_string().into_owned());
            }
        }
    }
    out
}

fn folder_items(files: &dyn Files, prefix: &str, ext: &str) -> Vec<Item> {
    files.list(prefix, ext).into_iter().map(|p| Item { name: p.clone(), key: p }).collect()
}

impl GameLists {
    /// Reads the lists (a second or two: on a worker).
    pub fn load(files: &dyn Files) -> Self {
        let mut g = Self::default();
        let schema = files.game().and_then(wce_foundation::settings::rpfm_schema_path).and_then(|p| Schema::load(&p, None).ok());
        if let Some(schema) = &schema {
            let unit_names = loc_names(files, "land_units", "land_units_onscreen_name_");
            let land_of: HashMap<String, String> = table_rows(schema, files, "main_units_tables")
                .into_iter()
                .filter_map(|r| Some((r.get("unit")?.clone(), r.get("land_unit")?.clone())))
                .collect();
            let mut units: Vec<Item> = land_of
                .iter()
                .map(|(unit, land)| Item { key: unit.clone(), name: unit_names.get(land).cloned().unwrap_or_else(|| unit.clone()) })
                .collect();
            units.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.key.cmp(&b.key)));
            g.units = units;
            for r in table_rows(schema, files, "unit_variants_tables") {
                if let (Some(u), Some(f)) = (r.get("unit"), r.get("faction")) {
                    g.units_of_faction.entry(f.clone()).or_default().insert(u.clone());
                }
            }
            let faction_names = loc_names(files, "factions", "factions_screen_name_");
            let mut factions: Vec<Item> = table_rows(schema, files, "factions_tables")
                .into_iter()
                .filter_map(|r| r.get("key").cloned())
                .map(|k| Item { name: faction_names.get(&k).cloned().unwrap_or_else(|| k.clone()), key: k })
                .collect();
            factions.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            factions.dedup_by(|a, b| a.key == b.key);
            g.factions = factions;
        }
        g.environments = folder_items(files, "weather/", ".environment");
        g.vfx = files
            .list("vfx/", ".xml")
            .into_iter()
            .filter(|p| p.matches('/').count() == 1)
            .map(|p| {
                let k = p.trim_start_matches("vfx/").trim_end_matches(".xml").to_owned();
                Item { name: k.clone(), key: k }
            })
            .collect();
        let maps: BTreeSet<String> = files
            .list("terrain/battles/", "")
            .into_iter()
            .filter_map(|p| p.strip_prefix("terrain/battles/").and_then(|r| r.split('/').next()).map(str::to_owned))
            .collect();
        g.battle_maps = maps.into_iter().map(|m| Item { name: m.clone(), key: m }).collect();
        g
    }

    /// The named events of the sound banks (sound and music split by name).
    pub fn add_events(&mut self, lib: &SoundLibrary) {
        let mut events: BTreeMap<String, ()> = BTreeMap::new();
        for b in &lib.banks {
            for o in b.bank.objects().iter().filter(|o| o.kind == 4) {
                if let Some(name) = lib.names.get(o.id) {
                    events.insert(name.to_owned(), ());
                }
            }
        }
        let (music, sound): (Vec<String>, Vec<String>) = events.into_keys().partition(|n| n.to_lowercase().contains("music"));
        self.sound_events = sound.into_iter().map(|n| Item { name: n.clone(), key: n }).collect();
        self.music_events = music.into_iter().map(|n| Item { name: n.clone(), key: n }).collect();
    }

    pub fn unit_name(&self, key: &str) -> String {
        self.units.iter().find(|i| i.key == key).map_or_else(|| key.to_owned(), |i| i.name.clone())
    }

    pub fn faction_name(&self, key: &str) -> String {
        self.factions.iter().find(|i| i.key == key).map_or_else(|| key.to_owned(), |i| i.name.clone())
    }
}

/// The filters typed into the pickers, by picker id (kept between frames).
#[derive(Default)]
pub struct Filters(HashMap<String, String>);

/// A combo of `items` showing `shown` while closed; open, a filter on top narrows it (names and keys). The item picked, if any.
pub fn picker(filters: &mut Filters, id: &str, shown: &str, items: &[Item], width: f32) -> Option<String> {
    let mut picked = None;
    gui::set_next_item_width(width);
    if gui::begin_combo(&format!("##{id}"), shown) {
        let filter = filters.0.entry(id.to_owned()).or_default();
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }
        gui::set_next_item_width(-1.0);
        gui::input_text(&format!("##{id}_filter"), Some("type to search"), filter, 128, 0);
        let words: Vec<String> = filter.to_lowercase().split_whitespace().map(str::to_owned).collect();
        let shown_items: Vec<&Item> = items
            .iter()
            .filter(|i| {
                let hay = format!("{} {}", i.name.to_lowercase(), i.key.to_lowercase());
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        if shown_items.is_empty() {
            gui::text_disabled(if items.is_empty() { "Loading..." } else { "Nothing matches" });
        }
        gui::clipped(shown_items.len(), |k| {
            let i = shown_items[k];
            let label = if i.name == i.key { i.key.clone() } else { format!("{}  ({})", i.name, i.key) };
            if gui::selectable(&format!("{label}##{k}"), i.key == shown, 0) {
                picked = Some(i.key.clone());
            }
        });
        gui::end_combo();
    }
    picked
}

/// The labels of a short fixed list, as items.
pub fn items_of(list: &[&str]) -> Vec<Item> {
    list.iter().map(|s| Item { key: (*s).to_owned(), name: (*s).to_owned() }).collect()
}
