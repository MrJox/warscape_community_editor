//! What the DB says about a unit: its definitions by faction, its size, animation table, mount and colours. Read through rpfm_lib's DB tables, so
//! a mod's tables and files count.
//!
//! `unit_variants` (`unit`, `faction`, `variant`, sizes) is the list of units and gives the definition, by faction when a row names one: the game
//! doesn't find it by name. `main_units.land_unit` -> `land_units` (`man_animation`, `mount`) -> `mounts` (`animation`, `variant`). A whole unit:
//! `main_units` (`num_men`, `unit_scaling`), `land_units` (`rank_depth`, `spacing`) -> `unit_spacings`.

use std::collections::HashMap;
use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_formats::anim_tables::{AnimationTable, parse_tables};
use wce_vfs::Vfs;

/// One `unit_variants` row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitEntry {
    pub unit: String,

    /// Empty: any faction.
    pub faction: String,
    pub variant: String,
    pub height_scale: f32,
    pub height_variation: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LandUnit {
    pub man_animation: String,
    pub mount: String,

    /// The men at the largest unit size.
    pub num_men: u32,

    /// The game's unit size setting changes its men.
    pub scales: bool,

    /// Its ranks.
    pub rank_depth: u32,

    /// Metres between files and between ranks, in close formation.
    pub spacing: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mount {
    pub animation: String,
    pub variant: String,
}

/// The tables.
#[derive(Clone, Debug, Default)]
pub struct UnitCatalog {
    pub entries: Vec<UnitEntry>,

    /// By main unit.
    pub land_units: HashMap<String, LandUnit>,
    pub mounts: HashMap<String, Mount>,

    /// By faction: primary, secondary and uniform colours, linear, as the shaders multiply them (**ASSUMPTION**: the tables' 0..255 are sRGB).
    pub colours: HashMap<String, [[f32; 3]; 4]>,
    pub tables: HashMap<String, AnimationTable>,
}

/// The rows of a table, every file of it, the columns as text, in `columns`' order. A key that comes twice keeps the first: the packs are read
/// in priority order only per file, which is what a mod overriding a table's rows expects of the file order.
fn rows(vfs: &Vfs, schema: &Schema, table: &str, columns: &[&str]) -> Result<Vec<Vec<String>>, String> {
    rows_with(vfs, schema, table, columns, &[])
}

/// [`rows`] with `optional` columns after `columns`, empty in the tables whose version lacks them.
fn rows_with(vfs: &Vfs, schema: &Schema, table: &str, columns: &[&str], optional: &[&str]) -> Result<Vec<Vec<String>>, String> {
    let name = format!("{table}_tables");
    let mut out = vec![];
    for path in vfs.list(&format!("db/{name}/"), "") {
        let bytes = vfs.read(path).map_err(|e| e.to_string())?;
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(&name));
        let db = DB::decode(&mut Cursor::new(bytes), &Some(extra)).map_err(|e| format!("{path}: {e}"))?;
        let at = columns.iter().map(|c| db.column_position_by_name(c).ok_or(format!("{path}: no column '{c}'"))).collect::<Result<Vec<_>, _>>()?;
        let maybe: Vec<Option<usize>> = optional.iter().map(|c| db.column_position_by_name(c)).collect();
        out.extend(db.data().iter().map(|row| {
            let cell = |k: usize| row[k].data_to_string().to_string();
            at.iter().map(|&k| cell(k)).chain(maybe.iter().map(|k| k.map(cell).unwrap_or_default())).collect::<Vec<_>>()
        }));
    }

    Ok(out)
}

fn srgb_to_linear(byte: f32) -> f32 {
    let v = byte / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// `RRGGBB`, with or without its `#`, as linear colour; black when it isn't one.
fn hex_colour(text: &str) -> [f32; 3] {
    let text = text.trim().trim_start_matches('#');
    std::array::from_fn(|i| text.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map_or(0.0, |b| srgb_to_linear(f32::from(b))))
}

impl UnitCatalog {
    /// Reads the tables and every `animation_tables` file.
    ///
    /// # Errors
    ///
    /// Returns what could not be read: a table, or a column the schema doesn't have.
    pub fn load(vfs: &Vfs, schema: &Schema) -> Result<Self, String> {
        let mut c = Self::default();
        for r in rows(vfs, schema, "unit_variants", &["unit", "faction", "variant", "height_scale", "height_variation"])? {
            let number = |s: &String| s.trim().parse().unwrap_or(0.0);
            c.entries.push(UnitEntry { height_scale: number(&r[3]), height_variation: number(&r[4]), unit: r[0].clone(), faction: r[1].clone(), variant: r[2].clone() });
        }

        let number = |s: &String| s.trim().parse::<f32>().unwrap_or(0.0);
        let spacings: HashMap<String, [f32; 2]> = rows(vfs, schema, "unit_spacings", &["key", "close_formation_spacing_horizontal", "close_formation_spacing_vertical"])?
            .into_iter().map(|r| (r[0].clone(), [number(&r[1]), number(&r[2])])).collect();
        let land: HashMap<String, LandUnit> = rows(vfs, schema, "land_units", &["key", "man_animation", "mount", "rank_depth", "spacing"])?.into_iter()
            .map(|r| (r[0].clone(), LandUnit {
                man_animation: r[1].clone(),
                mount: r[2].clone(),
                rank_depth: number(&r[3]) as u32,
                spacing: spacings.get(&r[4]).copied().unwrap_or_default(),
                ..LandUnit::default()
            })).collect();
        // Attila's table has no `unit_scaling`: there, everything scales.
        for r in rows_with(vfs, schema, "main_units", &["unit", "land_unit", "num_men"], &["unit_scaling"])? {
            if let Some(l) = land.get(&r[1]) {
                let row = LandUnit { num_men: number(&r[2]) as u32, scales: !matches!(r[3].trim(), "false" | "0"), ..l.clone() };
                c.land_units.entry(r[0].clone()).or_insert(row);
            }
        }

        for r in rows(vfs, schema, "mounts", &["key", "animation", "variant"])? {
            c.mounts.entry(r[0].clone()).or_insert(Mount { animation: r[1].clone(), variant: r[2].clone() });
        }

        // rpfm_lib merges each colour's three columns into one, `RRGGBB`.
        for r in rows(vfs, schema, "factions", &["key", "primary_colour_hex", "secondary_colour_hex", "uniform_colour_hex"])? {
            c.colours.entry(r[0].clone()).or_insert([hex_colour(&r[1]), hex_colour(&r[2]), hex_colour(&r[3]), [0.0; 3]]);
        }

        for path in vfs.list("animations/animation_tables/", ".txt") {
            let text = String::from_utf8_lossy(&vfs.read(path).map_err(|e| e.to_string())?).into_owned();
            for table in parse_tables(&text).ok_or_else(|| format!("{path}: not an animation table"))? {
                c.tables.entry(table.name.clone()).or_insert(table);
            }
        }

        Ok(c)
    }

    /// A unit's row: the faction's when it has one, else the row of any faction, else the first.
    pub fn entry(&self, unit: &str, faction: &str) -> Option<&UnitEntry> {
        let of_unit = || self.entries.iter().filter(|e| e.unit == unit);
        of_unit().find(|e| e.faction == faction).or_else(|| of_unit().find(|e| e.faction.is_empty())).or_else(|| of_unit().next())
    }

    /// The units, each once, sorted.
    pub fn units(&self) -> Vec<&str> {
        let mut units: Vec<&str> = self.entries.iter().map(|e| e.unit.as_str()).collect();
        units.sort_unstable();
        units.dedup();
        units
    }
}

/// The men of a unit of `num_men` at the game's `gfx_unit_size` (0 small .. 3 ultra). **ASSUMPTION** (from what the game's size names are known to give,
/// not traced): a quarter of the table's men per step, ultra the table's; an odd count rounds up.
pub fn men_at_size(unit: &LandUnit, size: u32) -> u32 {
    let men = unit.num_men.max(1);
    if !unit.scales {
        return men;
    }

    (men * (size.min(3) + 1)).div_ceil(4).max(1)
}

/// Where the men of a unit stand: `(x, z)` in metres, about its middle, the front rank towards +z, where it faces. `ranks` ranks of equal files, the
/// last short and centred. The close formation of the tables; **ASSUMPTION**: the game's deployment isn't traced.
pub fn formation(men: u32, ranks: u32, spacing: [f32; 2]) -> Vec<[f32; 2]> {
    let ranks = ranks.clamp(1, men.max(1));
    let files = men.div_ceil(ranks);
    let mut out = Vec::with_capacity(men as usize);
    for k in 0..men {
        let (rank, file) = (k / files, k % files);
        let in_rank = (men - rank * files).min(files);
        let x = (file as f32 - (in_rank - 1) as f32 * 0.5) * spacing[0];
        let z = ((ranks - 1) as f32 * 0.5 - rank as f32) * spacing[1];
        out.push([x, z]);
    }

    out
}

/// The file of a variant.
pub fn variant_path(variant: &str) -> String {
    format!("variantmeshes/variantmeshdefinitions/{variant}.variantmeshdefinition")
}

#[cfg(test)]
mod tests {
    use super::{LandUnit, UnitCatalog, UnitEntry, formation, hex_colour, men_at_size, srgb_to_linear};

    fn entry(unit: &str, faction: &str, variant: &str) -> UnitEntry {
        UnitEntry { unit: unit.to_owned(), faction: faction.to_owned(), variant: variant.to_owned(), ..UnitEntry::default() }
    }

    #[test]
    fn a_faction_has_its_own_row_or_the_general_one() {
        let c = UnitCatalog { entries: vec![entry("a", "x", "ax"), entry("a", "", "any"), entry("b", "y", "by")], ..UnitCatalog::default() };
        assert_eq!(c.entry("a", "x").map(|e| e.variant.as_str()), Some("ax"));
        assert_eq!(c.entry("a", "z").map(|e| e.variant.as_str()), Some("any"));
        assert_eq!(c.entry("b", "z").map(|e| e.variant.as_str()), Some("by"));
        assert!(c.entry("c", "").is_none());
        assert_eq!(c.units(), ["a", "b"]);
    }

    #[test]
    fn the_size_setting_scales_what_may_scale() {
        let mut unit = LandUnit { num_men: 160, scales: true, ..LandUnit::default() };
        assert_eq!([0, 1, 2, 3, 9].map(|s| men_at_size(&unit, s)), [40, 80, 120, 160, 160]);
        unit.num_men = 2;
        assert_eq!(men_at_size(&unit, 0), 1);
        unit.scales = false;
        assert_eq!(men_at_size(&unit, 0), 2);
    }

    #[test]
    fn a_formation_is_centred_with_the_last_rank_short() {
        let f = formation(5, 2, [1.0, 2.0]);
        assert_eq!(f, [[-1.0, 1.0], [0.0, 1.0], [1.0, 1.0], [-0.5, -1.0], [0.5, -1.0]]);
        assert_eq!(formation(1, 4, [1.0, 1.0]), [[0.0, 0.0]]);
        assert_eq!(formation(160, 4, [1.0, 1.0]).len(), 160);
    }

    #[test]
    fn colours_are_linear() {
        assert!((srgb_to_linear(255.0) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(143.0) - 0.2747).abs() < 1e-3);
        assert_eq!(hex_colour("#FF0000"), [1.0, 0.0, 0.0]);
        assert_eq!(hex_colour("00ff00"), [0.0, 1.0, 0.0]);
    }
}
