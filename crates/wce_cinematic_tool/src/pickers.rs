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
use wce_cinematic::scene::AUTO_SLOT;
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
    /// Faction key -> the units (`main_units` keys) it fields (`unit_variants`, by land unit).
    pub units_of_faction: HashMap<String, BTreeSet<String>>,
    /// Unit -> its cards by faction (`unit_variants.unit_card`; faction "" the generic one): `ui/units/icons/<card>.png`.
    pub cards: HashMap<String, Vec<(String, String)>>,
    /// Unit -> its men at the largest unit size (`main_units.num_men`) and its caste (infantry, cavalry...).
    pub men: HashMap<String, u32>,
    pub caste: HashMap<String, String>,
    pub factions: Vec<Item>,
    pub environments: Vec<Item>,
    pub vfx: Vec<Item>,
    pub battle_maps: Vec<Item>,
    /// The battle tiles of the packs (their folders: `terrain/tiles/battle/<set>/<tile>/<variation>`), named by their path's parts.
    pub battle_tiles: Vec<Item>,
    /// Filled once the sound library is there.
    pub sound_events: Vec<Item>,
    pub music_events: Vec<Item>,
    /// The game's music moods (`music_set_*` events), by group.
    pub moods: Vec<wce_cinematic::music::Mood>,
}

/// A unit's animation slots as picker items, grouped (idle, movement, combat, death, morale and gestures, other) and named for
/// reading, `AUTO` first. Without the unit's own (its men not loaded yet): the common ones.
pub fn slot_items(own: Option<&[String]>) -> Vec<Item> {
    let slots: Vec<String> = match own {
        Some(s) if !s.is_empty() => s.to_vec(),
        _ => SLOTS.iter().map(|s| (*s).to_owned()).collect(),
    };
    // By family: a numbered set of variants is one entry (its variants spread over the men; the key can name one).
    let mut families: BTreeMap<String, usize> = BTreeMap::new();
    for s in slots.iter().filter(|s| *s != AUTO_SLOT && !s.starts_with("HAND_POSE")) {
        *families.entry(wce_cinematic::scene::slot_family(s).to_owned()).or_default() += 1;
    }
    let mut list: Vec<(String, usize)> = families.into_iter().collect();
    list.sort_by(|a, b| slot_group(&a.0).0.cmp(&slot_group(&b.0).0).then_with(|| a.0.cmp(&b.0)));
    let mut out = vec![Item { key: AUTO_SLOT.to_owned(), name: "Back to moving / standing".to_owned() }];
    out.extend(list.into_iter().map(|(f, n)| {
        let more = if n > 1 { format!(", {n} variants") } else { String::new() };
        Item { name: format!("{}: {}{more}", slot_group(&f).1, slot_label(&f)), key: f }
    }));
    out
}

/// The group a slot is listed under (its order, its name).
fn slot_group(slot: &str) -> (u8, &'static str) {
    let s = slot.strip_prefix("RIDER_").unwrap_or(slot);
    let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
    if has(&["STAND", "IDLE"]) {
        (0, "Idle")
    } else if has(&["WALK", "RUN", "CHARGE", "TURN", "MOVE", "JOG", "SPRINT", "STRAFE"]) {
        (1, "Movement")
    } else if has(&["ATTACK", "COMBAT", "DEFEND", "BLOCK", "PARRY", "FIRE", "SHOOT", "RELOAD", "AIM", "THROW", "HIT", "SPEAR", "SWORD"]) {
        (2, "Combat")
    } else if has(&["DIE", "DEATH", "DEAD", "DYING", "KNOCKED", "FALL"]) {
        (3, "Death")
    } else if has(&["TAUNT", "CELEBRATE", "CHEER", "SALUTE", "PANIC", "ROUT", "KNEEL", "PRAY", "CHANT", "SHOUT", "FLEE", "MORALE", "WAVE"])
    {
        (4, "Morale and gestures")
    } else {
        (5, "Other")
    }
}

/// A slot for reading: `RIDER_ATTACK_1` -> "Attack 1 (riding)".
fn slot_label(slot: &str) -> String {
    let (s, riding) = match slot.strip_prefix("RIDER_") {
        Some(s) => (s, true),
        None => (slot, false),
    };
    let mut words = s.to_lowercase().replace('_', " ");
    if let Some(c) = words.get(..1) {
        words = c.to_uppercase() + &words[1..];
    }
    if riding { format!("{words} (riding)") } else { words }
}

/// Where an action key's animation plays, for its tooltip.
pub fn slot_tip(slot: &str) -> &'static str {
    if wce_cinematic::scene::slot_family(slot) == "TAUNT" || slot == AUTO_SLOT || slot == "STAND" {
        "The animation the whole unit plays from here; plays in the editor, exported clips and the battle"
    } else {
        "The animation the whole unit plays from here; plays in the editor and exported clips (a battle's Lua can't)"
    }
}

/// A key's slot the unit hasn't (its men loaded; `AUTO` always has a meaning).
pub fn slot_missing(slot: &str, own: Option<&[String]>) -> bool {
    !slot.is_empty()
        && slot != AUTO_SLOT
        && !slot.starts_with(wce_cinematic::scene::FILE_SLOT)
        && own.is_some_and(|s| {
            // A rider's list is his `RIDER_` slots alone.
            let mounted = s.iter().all(|x| x.starts_with("RIDER_"));
            !s.is_empty() && wce_cinematic::scene::resolve_slot(slot, s, mounted).is_empty()
        })
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
            let mains = table_rows(schema, files, "main_units_tables");
            let land_of: HashMap<String, String> =
                mains.iter().filter_map(|r| Some((r.get("unit")?.clone(), r.get("land_unit")?.clone()))).collect();
            for r in &mains {
                let Some(u) = r.get("unit") else { continue };
                if let Some(n) = r.get("num_men").and_then(|n| n.parse::<u32>().ok()) {
                    g.men.insert(u.clone(), n);
                }
                if let Some(c) = r.get("caste") {
                    g.caste.insert(u.clone(), c.clone());
                }
            }
            let mut mains_of_land: HashMap<&str, Vec<&str>> = HashMap::new();
            for (u, l) in &land_of {
                mains_of_land.entry(l.as_str()).or_default().push(u.as_str());
            }
            let mut units: Vec<Item> = land_of
                .iter()
                .map(|(unit, land)| Item { key: unit.clone(), name: unit_names.get(land).cloned().unwrap_or_else(|| unit.clone()) })
                .collect();
            units.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.key.cmp(&b.key)));
            g.units = units;
            // Who fields what: the custom battles' permissions (faction, main unit), then the variants below.
            for r in table_rows(schema, files, "units_custom_battle_permissions_tables") {
                if let (Some(f), Some(u)) = (r.get("faction"), r.get("unit")) {
                    g.units_of_faction.entry(f.clone()).or_default().insert(u.clone());
                }
            }
            for r in table_rows(schema, files, "unit_variants_tables") {
                let (Some(land), f) = (r.get("unit"), r.get("faction").cloned().unwrap_or_default()) else { continue };
                for &u in mains_of_land.get(land.as_str()).into_iter().flatten() {
                    if !f.is_empty() {
                        g.units_of_faction.entry(f.clone()).or_default().insert(u.to_owned());
                    }
                    if let Some(card) = r.get("unit_card").filter(|c| !c.is_empty() && *c != "placeholder") {
                        g.cards.entry(u.to_owned()).or_default().push((f.clone(), card.clone()));
                    }
                }
            }
            let faction_names = loc_names(files, "factions", "factions_screen_name_");
            let mut factions: Vec<Item> = table_rows(schema, files, "factions_tables")
                .into_iter()
                .filter_map(|r| r.get("key").cloned())
                .map(|k| Item { name: faction_names.get(&k).cloned().unwrap_or_else(|| k.clone()), key: k })
                .collect();
            factions.sort_by_key(|a| a.name.to_lowercase());
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
        let mut tiles: Vec<String> = files
            .list("terrain/tiles/battle/", "/definition.xml")
            .into_iter()
            .filter_map(|p| p.strip_suffix("/definition.xml").map(str::to_owned))
            .filter(|f| !f.contains("/_"))
            .collect();
        tiles.sort();
        tiles.dedup();
        g.battle_tiles =
            tiles.into_iter().map(|f| Item { name: f.trim_start_matches("terrain/tiles/battle/").replace('/', " / "), key: f }).collect();
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
        self.moods = wce_cinematic::music::moods(lib);
    }

    pub fn unit_name(&self, key: &str) -> String {
        self.units.iter().find(|i| i.key == key).map_or_else(|| key.to_owned(), |i| i.name.clone())
    }

    /// The picture of a unit as `faction` fields it (its generic card, else any).
    pub fn card(&self, unit: &str, faction: &str) -> Option<String> {
        let cards = self.cards.get(unit)?;
        cards
            .iter()
            .find(|(f, _)| f == faction)
            .or_else(|| cards.iter().find(|(f, _)| f.is_empty()))
            .or_else(|| cards.first())
            .map(|(_, c)| format!("ui/units/icons/{c}.png"))
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

/// [`picker`] with a play button on every row (sounds): the row whose button was clicked goes in `play`.
pub fn picker_audible(
    filters: &mut Filters,
    id: &str,
    shown: &str,
    items: &[Item],
    width: f32,
    play: &mut Option<String>,
) -> Option<String> {
    let mut picked = None;
    gui::set_next_item_width(width);
    if gui::begin_combo(&format!("##{id}"), shown) {
        let filter = filters.0.entry(id.to_owned()).or_default();
        if gui::is_window_appearing() {
            gui::set_keyboard_focus_here(0);
        }
        gui::set_next_item_width(-1.0);
        gui::input_text(&format!("##{id}_filter"), Some("type to search; the triangle plays it"), filter, 128, 0);
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
            if gui::small_button(&format!("{}##play{k}", wce_editor::icons::PLAY)) {
                *play = Some(i.key.clone());
            }
            gui::same_line();
            if gui::selectable(&format!("{}##{k}", i.name), i.key == shown, 0) {
                picked = Some(i.key.clone());
            }
        });
        gui::end_combo();
    }
    picked
}

/// The game's pictures of units (their cards), read from the packs on a worker as the panels ask for them and kept as images.
pub struct Cards {
    files: std::sync::Arc<dyn Files>,
    asked: std::collections::HashSet<String>,
    requests: Option<std::sync::mpsc::Sender<String>>,
    done: Option<std::sync::mpsc::Receiver<(String, Option<(u32, u32, Vec<u8>)>)>>,
}

impl Cards {
    pub fn new(files: std::sync::Arc<dyn Files>) -> Self {
        Self { files, asked: Default::default(), requests: None, done: None }
    }

    fn key(path: &str) -> String {
        format!("cinematic/{path}")
    }

    /// The picture at `path` (`ui/units/icons/...png`) once read; asks for it the first time.
    pub fn get(&mut self, images: &mut wce_editor::images::ImageTextures, path: &str) -> Option<wce_editor::images::ImageRef> {
        if let Some(rx) = &self.done {
            while let Ok((p, img)) = rx.try_recv() {
                if let Some((w, h, px)) = img {
                    images.submit(&Self::key(&p), w, h, px);
                }
            }
        }
        if let Some(r) = images.get(&Self::key(path)) {
            return Some(r);
        }
        if self.asked.insert(path.to_owned()) {
            if self.requests.is_none() {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                let (dtx, drx) = std::sync::mpsc::channel();
                let files = self.files.clone();
                std::thread::spawn(move || {
                    for p in rx {
                        let img = files.read(&p).ok().and_then(|b| decode_png(&b));
                        if dtx.send((p, img)).is_err() {
                            break;
                        }
                    }
                });
                self.requests = Some(tx);
                self.done = Some(drx);
            }
            if let Some(tx) = &self.requests {
                let _ = tx.send(path.to_owned());
            }
        }
        None
    }
}

/// An 8-bit RGB or RGBA PNG as RGBA.
fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let png = wce_formats::png_file::PngFile::parse(bytes).ok()?;
    let rgba = match (png.bit_depth, png.color_type) {
        (8, 6) => png.pixels,
        (8, 2) => png.pixels.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    Some((png.width, png.height, rgba))
}
