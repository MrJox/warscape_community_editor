//! What the tile tool needs to know about a building from the `battlefield_buildings` table: its category, which decides whether TEd shows
//! `weak_wall`, `ai_breachable` and `dockable` for an instance (`fort_wall`: all three, `unbreachable_fort_wall`: `dockable`).
//!
//! The table is read from the Assembly Kits' exports (`<kit>/raw_data/db/battlefield_buildings.xml`, and the mod's
//! `TWaD_battlefield_buildings.xml` beside it): the kit next to the tile first, then the projects' kit layout and the settings' kits
//! ([`BuildingDb::load_for_tile`]); without a kit the category is unknown and the grid shows those properties for the instances
//! that already have them in the file (TEd writes them for exactly those buildings).

use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct BuildingDb {
    categories: HashMap<String, String>,
}

impl BuildingDb {
    /// The tables of the kit that holds `tile_dir` (any folder below its `raw_data`); empty when there is none.
    pub fn load_near(tile_dir: &Path) -> Self {
        Self::load_raw_dirs(&Self::raw_dir_near(tile_dir).into_iter().collect::<Vec<_>>())
    }

    /// The raw_data folder with tables (`raw_data/db`) of the kit that holds `tile_dir`, if there is one.
    pub fn raw_dir_near(tile_dir: &Path) -> Option<std::path::PathBuf> {
        tile_dir.ancestors().map(|dir| dir.join("raw_data")).find(|raw| raw.join("db").is_dir())
    }

    /// The tables of a tile: its own kit's (a kit's tile), then those of `raw_dirs` (the projects' kit layout, the Assembly Kits in the
    /// settings' order), the first ones' records winning.
    pub fn load_for_tile(tile_dir: &Path, raw_dirs: &[std::path::PathBuf]) -> Self {
        let mut raws = Self::raw_dir_near(tile_dir).into_iter().collect::<Vec<_>>();
        for raw in raw_dirs {
            if !raws.contains(raw) {
                raws.push(raw.clone());
            }
        }

        Self::load_raw_dirs(&raws)
    }

    /// The tables of these raw_data folders (`<raw_data>/db`), the first ones' records winning: the projects' kit layout, then the Assembly
    /// Kits in the settings' order.
    pub fn load_raw_dirs(raw_dirs: &[std::path::PathBuf]) -> Self {
        let mut db = Self::default();
        for raw in raw_dirs.iter().rev() {
            for name in ["battlefield_buildings.xml", "TWaD_battlefield_buildings.xml"] {
                if let Ok(bytes) = std::fs::read(raw.join("db").join(name)) {
                    db.add_xml(&String::from_utf8_lossy(&bytes));
                }
            }
        }

        db
    }

    /// Reads the `<battlefield_buildings>` records of an export: the text of `<key>` and `<category>` of each. Records of a later table win
    /// (the mod's table overrides the game's).
    pub fn add_xml(&mut self, xml: &str) {
        const OPEN: &str = "<battlefield_buildings ";
        const CLOSE: &str = "</battlefield_buildings>";
        let mut rest = xml;
        while let Some(start) = rest.find(OPEN) {
            let after = &rest[start..];
            let Some(end) = after.find(CLOSE) else { break };
            let record = &after[..end];
            if let (Some(key), Some(category)) = (field(record, "key"), field(record, "category")) {
                self.categories.insert(key.to_owned(), category.to_owned());
            }

            rest = &after[end + CLOSE.len()..];
        }
    }

    pub fn category(&self, key: &str) -> Option<&str> {
        self.categories.get(key).map(String::as_str)
    }

    /// Every building key of the tables.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.categories.keys().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.categories.is_empty()
    }
}

/// The text of `<name>...</name>` in a record.
fn field<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let from = record.find(&open)? + open.len();
    let to = record[from..].find(&format!("</{name}>"))?;
    Some(&record[from..from + to])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_key_and_category() {
        let mut db = BuildingDb::default();
        db.add_xml(
            "<dataroot>\n<battlefield_buildings record_uuid=\"{1}\">\n<category>fort_wall</category>\n<key>wall_a</key>\n</battlefield_buildings>\n\
             <battlefield_buildings record_uuid=\"{2}\">\n<category>generic</category>\n<key>30_30_10</key>\n</battlefield_buildings>\n</dataroot>",
        );
        assert_eq!(db.category("wall_a"), Some("fort_wall"));
        assert_eq!(db.category("30_30_10"), Some("generic"));
        assert_eq!(db.category("nope"), None);
        db.add_xml("<battlefield_buildings x=\"1\"><category>gate</category><key>wall_a</key></battlefield_buildings>");
        assert_eq!(db.category("wall_a"), Some("gate"));
    }

    #[test]
    fn a_tile_reads_its_kit_then_the_others() {
        let table = |raw: &Path, key: &str, category: &str| {
            std::fs::create_dir_all(raw.join("db")).unwrap();
            let xml = format!("<battlefield_buildings x=\"1\"><category>{category}</category><key>{key}</key></battlefield_buildings>");
            std::fs::write(raw.join("db").join("battlefield_buildings.xml"), xml).unwrap();
        };
        let dir = tempfile::tempdir().unwrap();
        let (own, projects, kit) = (dir.path().join("own/raw_data"), dir.path().join("projects/raw_data"), dir.path().join("kit/raw_data"));
        table(&own, "a", "gate");
        table(&projects, "a", "generic");
        table(&kit, "b", "fort_wall");
        let tile = own.join("terrain/tiles/battle/x/aa");
        std::fs::create_dir_all(&tile).unwrap();

        let db = BuildingDb::load_for_tile(&tile, &[projects.clone(), kit.clone()]);
        assert_eq!((db.category("a"), db.category("b")), (Some("gate"), Some("fort_wall")));

        // A project tile (no kit of its own): the projects' table, then the kits'.
        let project_tile = dir.path().join("elsewhere/aa");
        let db = BuildingDb::load_for_tile(&project_tile, &[projects, kit]);
        assert_eq!((db.category("a"), db.category("b")), (Some("generic"), Some("fort_wall")));
    }
}
