//! What the decompiler and the compiler read from a game's packs besides the tile: the tile databases (the entries), the buildings'
//! destruction levels (`<key>_tech.cs2.parsed`, or the models' `_destructNN` names), and the `battlefield_buildings` tables (category
//! and `destruct_thresholds` per key, through rpfm's schema). docs/formats/tile_decompile.md, "Buildings".

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::db::DB;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_foundation::game::GameProfile;
use wce_vfs::{Config, Vfs};

use crate::conventions::TileConventions;
use crate::rome2::tile_database::{Rome2TileDatabase, SETTINGS_FILE};

use super::decompile::{Context, destruction_levels};

type Lookup<T> = Box<dyn Fn(&str) -> Option<T> + Send + Sync>;

/// The game's tile data, read once from its packs.
pub struct GameData {
    /// The tile databases: the mods' first, as they override the game's.
    pub databases: Vec<TileDatabase>,

    /// Each database's kind (`battle`, `campaign`: its path, `terrain/tiles/<kind>/tile_database.bin`).
    pub kinds: Vec<String>,

    /// The databases of a game that has them as XML in the packs (Rome 2), with their files, by kind: a saved tile goes into its set's
    /// file.
    pub xml_databases: BTreeMap<String, Rome2TileDatabase>,

    /// The game's conventions (what its files mean at a version).
    pub conventions: TileConventions,
    levels: Lookup<u32>,
    categories: Lookup<String>,
    thresholds: Lookup<Vec<f32>>,

    /// What could not be read (no schema: the buildings' categories and thresholds are unknown).
    pub warnings: Vec<String>,
}

/// rpfm's schema of a game where rpfm keeps it (`wce_foundation::settings::default_rpfm_schema_path`).
pub fn default_schema_path(game: &GameProfile) -> Option<PathBuf> {
    wce_foundation::settings::default_rpfm_schema_path(game)
}

/// The categories of the buildings (`battlefield_buildings_tables`, given as the tables' bytes, a mod's before the game's), by lower case
/// key: the first table that has a key wins. Tables that do not decode with the schema are left out.
///
/// # Errors
///
/// The schema does not load.
pub fn building_categories(tables: &[Vec<u8>], schema: &Path) -> Result<BTreeMap<String, String>, String> {
    let schema = Schema::load(schema, None).map_err(|e| format!("{}: {e}", schema.display()))?;
    let mut out = BTreeMap::new();
    for bytes in tables {
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(&schema));
        extra.set_table_name(Some("battlefield_buildings_tables"));
        let Ok(db) = DB::decode(&mut Cursor::new(bytes), &Some(extra)) else { continue };
        let (Some(k), Some(cat)) = (db.column_position_by_name("key"), db.column_position_by_name("category")) else { continue };
        for row in db.data().iter() {
            out.entry(row[k].data_to_string().to_lowercase()).or_insert_with(|| row[cat].data_to_string().into_owned());
        }
    }

    Ok(out)
}

impl GameData {
    /// Reads a game's folder: its own packs and every mod pack of its data folder (the mods first), as the checks see the game.
    ///
    /// # Errors
    ///
    /// The packs can't be mounted.
    pub fn load(game: &GameProfile, game_dir: &Path, schema: Option<&Path>) -> Result<Self, String> {
        let mut config = Config::new(game_dir, game.clone());
        config.use_mods = false;
        config.mod_packs = Some(game.available_mod_packs(game_dir));
        let vfs = Vfs::mount(&config).map_err(|e| e.to_string())?;
        Self::from_vfs(&vfs, schema)
    }

    /// Reads the mounted packs of a game (a path from the pack that wins; the tile databases from every pack that has one, a mod's
    /// first); `schema` is rpfm's schema of the game for the db tables (without it the buildings' categories and thresholds are not
    /// known). A game whose tile databases are XML in the packs (Rome 2: `_tile_database/_settings.xml` and the tile set files) has
    /// them read as rpfm's database, kept in [`Self::xml_databases`] too.
    ///
    /// # Errors
    ///
    /// None for now: what can't be read is a warning.
    pub fn from_vfs(vfs: &Vfs, schema: Option<&Path>) -> Result<Self, String> {
        let mut warnings = vec![];
        let schema = match schema {
            Some(p) => match { wce_foundation::wce_prof!("game_data.schema"); Schema::load(p, None) } {
                Ok(s) => Some(s),
                Err(e) => {
                    warnings.push(format!("{}: {e}", p.display()));
                    None
                }
            },
            None => None,
        };
        if schema.is_none() {
            warnings.push("no rpfm schema: the buildings' categories and destruction thresholds are not known".to_owned());
        }

        let is_mod = |path: &str| vfs.owner(path).is_some_and(|p| vfs.packs()[p].is_mod);

        // The destruction levels: the building's collision file lists each piece's destructs, the building's count is the most over its
        // pieces; without one, the same from the models' names (`<piece>_destructNN.rigid_model_v2`).
        let mut pieces: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
        let mut cs2 = vec![];
        for n in vfs.list("rigidmodels/buildings/", "") {
            let Some(rest) = n.strip_prefix("rigidmodels/buildings/") else { continue };
            let Some((key, file)) = rest.split_once('/') else { continue };
            if file.ends_with(".cs2.parsed") {
                cs2.push((key, n));
            } else if let Some(at) = file.find("_destruct")
                && file.ends_with(".rigid_model_v2")
            {
                let digits = file[at + 9..].chars().take_while(char::is_ascii_digit).collect::<String>();
                if let Ok(level) = digits.parse::<u32>() {
                    let e = pieces.entry(key.to_owned()).or_default().entry(file[..at].to_owned()).or_default();
                    *e = (*e).max(level);
                }
            }
        }

        // The collision files read on the thread pool (the packs lock their own reads) and parsed there; the last of a key wins, as in the
        // listing's order.
        let from_cs2: BTreeMap<String, u32> = {
            use rayon::prelude::*;
            wce_foundation::wce_prof!("game_data.cs2");
            let levels = cs2.par_iter().map(|(key, n)| vfs.read(n).ok().and_then(|bytes| destruction_levels(&bytes)).map(|l| (key.to_string(), l))).collect::<Vec<_>>();
            levels.into_iter().flatten().collect()
        };

        // The battlefield_buildings tables; a mod's rows (TDD's tables are in its main pack) before the game's.
        let mut rows: Vec<(bool, String, String, Vec<f32>)> = vec![];
        if let Some(schema) = &schema {
            wce_foundation::wce_prof!("game_data.tables");
            for n in vfs.list("db/battlefield_buildings_tables/", "") {
                let Ok(bytes) = vfs.read(n) else { continue };
                let mut extra = DecodeableExtraData::default();
                extra.set_schema(Some(schema));
                extra.set_table_name(Some("battlefield_buildings_tables"));
                let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) else {
                    warnings.push(format!("{n}: does not decode with the schema"));
                    continue;
                };

                let col = |c: &str| db.column_position_by_name(c);
                let (Some(k), Some(cat), Some(th)) = (col("key"), col("category"), col("destruct_thresholds")) else { continue };
                for row in db.data().iter() {
                    let values = row[th].data_to_string().split(',').filter_map(|v| v.trim().parse::<f32>().ok()).collect::<Vec<_>>();
                    rows.push((is_mod(n), row[k].data_to_string().to_lowercase(), row[cat].data_to_string().into_owned(), values));
                }
            }
        }

        let mut dbs: Vec<(bool, String, TileDatabase)> = vec![];
        let mut xml_databases = BTreeMap::new();
        wce_foundation::wce_prof!("game_data.tile_databases");
        for kind in ["battle", "campaign"] {
            let bin = format!("terrain/tiles/{kind}/tile_database.bin");
            if vfs.owner(&bin).is_some() {
                for &pack in vfs.owners(&bin) {
                    let Ok(bytes) = vfs.read_from(pack, &bin) else { continue };
                    match TileDatabase::decode(&mut Cursor::new(&bytes), &None) {
                        Ok(db) => dbs.push((vfs.packs()[pack].is_mod, kind.to_owned(), db)),
                        Err(e) => warnings.push(format!("{bin} of {}: {e}", vfs.packs()[pack].name)),
                    }
                }
            } else if vfs.owner(&format!("{}{}", Rome2TileDatabase::folder(kind), SETTINGS_FILE)).is_some() {
                match Rome2TileDatabase::from_files(vfs, kind) {
                    Ok(db) => {
                        dbs.push((false, kind.to_owned(), db.database.clone()));
                        xml_databases.insert(kind.to_owned(), db);
                    }
                    Err(e) => warnings.push(e),
                }
            }
        }

        dbs.sort_by_key(|(is_mod, _, _)| !is_mod);
        let mut levels = pieces.into_iter().map(|(k, p)| (k, p.values().copied().max().unwrap_or(1))).collect::<BTreeMap<_, _>>();
        levels.extend(from_cs2);
        rows.sort_by_key(|r| !r.0);
        let (mut categories, mut thresholds): (BTreeMap<String, String>, BTreeMap<String, Vec<f32>>) = (BTreeMap::new(), BTreeMap::new());
        for (_, key, category, values) in rows {
            categories.entry(key.clone()).or_insert(category);
            thresholds.entry(key).or_insert(values);
        }

        Ok(Self {
            kinds: dbs.iter().map(|(_, k, _)| k.clone()).collect(),
            databases: dbs.into_iter().map(|(_, _, d)| d).collect(),
            xml_databases,
            conventions: TileConventions::of(vfs.profile()),
            levels: Box::new(move |k| levels.get(&k.to_lowercase()).copied()),
            categories: Box::new(move |k| categories.get(&k.to_lowercase()).cloned()),
            thresholds: Box::new(move |k| thresholds.get(&k.to_lowercase()).filter(|t| !t.is_empty()).cloned()),
            warnings,
        })
    }

    /// The decompiler's context over this data; `tree_list` picks the list that becomes the decompiled trees (`Context::tree_list`).
    pub fn context<'a>(&'a self, tree_list: Option<&'a str>) -> Context<'a> {
        Context {
            databases: &self.databases,
            building_levels: Some(&*self.levels),
            building_category: Some(&*self.categories),
            building_thresholds: Some(&*self.thresholds),
            conventions: self.conventions,
            tree_list,
        }
    }

    /// The database in effect for a kind of tile (`battle`, `campaign`): the first pack's that has one, a mod's before the game's.
    pub fn database(&self, kind: &str) -> Option<&TileDatabase> {
        self.kinds.iter().position(|k| k.eq_ignore_ascii_case(kind)).map(|i| &self.databases[i])
    }

    /// The destruction levels of a building key.
    pub fn levels_of(&self, key: &str) -> Option<u32> {
        (self.levels)(key)
    }

    /// The compiler's lookups over this data (destruction levels and thresholds).
    pub fn compile_context(&self) -> super::compile::CompileContext<'_> {
        super::compile::CompileContext {
            building_levels: Some(&*self.levels),
            building_thresholds: Some(&*self.thresholds),
            conventions: self.conventions,
            ..Default::default()
        }
    }
}
