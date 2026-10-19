//! What the tile tool needs to know about a building from the `battlefield_buildings` table: its category, which decides whether TEd shows
//! `weak_wall`, `ai_breachable` and `dockable` for an instance (`fort_wall`: all three, `unbreachable_fort_wall`: `dockable`).
//!
//! The table is read from the assembly kit's export next to the tile (`<kit>/raw_data/db/battlefield_buildings.xml`, and the mod's
//! `TWaD_battlefield_buildings.xml` beside it); without the kit the category is unknown and the grid shows those properties for the instances
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
        let mut db = Self::default();
        for dir in tile_dir.ancestors() {
            let tables = dir.join("raw_data").join("db");
            if !tables.is_dir() {
                continue;
            }

            for name in ["battlefield_buildings.xml", "TWaD_battlefield_buildings.xml"] {
                if let Ok(bytes) = std::fs::read(tables.join(name)) {
                    db.add_xml(&String::from_utf8_lossy(&bytes));
                }
            }

            break;
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
}
