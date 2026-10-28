//! What the decompiler and the compiler read from a game's packs besides the tile: the tile databases (the entries), the buildings'
//! destruction levels (`<key>_tech.cs2.parsed`, or the models' `_destructNN` names), and the `battlefield_buildings` tables (category
//! and `destruct_thresholds` per key, through rpfm's schema). docs/formats/tile_decompile.md, "Buildings".

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rpfm_lib::files::db::DB;
use rpfm_lib::files::pack::Pack;
use rpfm_lib::files::tile_database::TileDatabase;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;

use super::decompile::{Context, destruction_levels};

type Lookup<T> = Box<dyn Fn(&str) -> Option<T> + Send + Sync>;

/// The game's tile data, read once from its `data` folder.
pub struct GameData {
    /// The tile databases: the mods' (packs not named `tiles*`) first, as they override the game's.
    pub databases: Vec<TileDatabase>,

    /// Each database's kind (`battle`, `campaign`: its path, `terrain/tiles/<kind>/tile_database.bin`).
    pub kinds: Vec<String>,
    levels: Lookup<u32>,
    categories: Lookup<String>,
    thresholds: Lookup<Vec<f32>>,

    /// What could not be read (no schema: the buildings' categories and thresholds are unknown).
    pub warnings: Vec<String>,
}

/// rpfm's Attila schema where rpfm keeps it (`~/.config/rpfm/schemas/schema_att.ron`; on Windows under `%APPDATA%`).
pub fn default_schema_path() -> Option<PathBuf> {
    wce_foundation::settings::default_rpfm_schema_path()
}

/// What [`GameData::load`] takes from one pack, in the pack's order.
struct PackPart {
    /// (building key, piece, its `_destructNN`) of the models' names.
    pieces: Vec<(String, String, u32)>,

    /// (building key, destruction levels) of the collision files.
    cs2: Vec<(String, u32)>,

    /// The `battlefield_buildings` rows: (a mod's, key, category, destruct thresholds).
    rows: Vec<(bool, String, String, Vec<f32>)>,

    /// The tile databases: (not the game's `tiles*` packs, kind, database).
    dbs: Vec<(bool, String, TileDatabase)>,
}

impl PackPart {
    /// A pack's part; the warning when it does not read.
    fn read(p: &Path, attila: &rpfm_lib::games::GameInfo, schema: Option<&Schema>) -> Result<Self, String> {
        let read = {
            wce_foundation::wce_prof!("game_data.pack_index");
            Pack::read_and_merge(std::slice::from_ref(&p.to_path_buf()), attila, true, false, false)
        };
        let Ok(mut pack) = read else { return Err(format!("{}: does not read", p.display())) };
        let name = p.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut out = Self { pieces: vec![], cs2: vec![], rows: vec![], dbs: vec![] };
        let mut cs2 = vec![];
        for n in pack.files().keys() {
            let Some(rest) = n.strip_prefix("rigidmodels/buildings/") else { continue };
            let Some((key, file)) = rest.split_once('/') else { continue };
            if file.ends_with(".cs2.parsed") {
                cs2.push((key.to_owned(), n.clone()));
            } else if let Some(at) = file.find("_destruct")
                && file.ends_with(".rigid_model_v2")
            {
                let digits = file[at + 9..].chars().take_while(char::is_ascii_digit).collect::<String>();
                if let Ok(level) = digits.parse::<u32>() {
                    out.pieces.push((key.to_owned(), file[..at].to_owned(), level));
                }
            }
        }

        // The collision files read in order, parsed on the thread pool, kept in order.
        let bytes: Vec<(String, Vec<u8>)> = {
            wce_foundation::wce_prof!("game_data.cs2_read");
            cs2.into_iter()
                .filter_map(|(key, n)| match pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) {
                    Some(Ok(Some(b))) => Some((key, b)),
                    _ => None,
                })
                .collect()
        };

        {
            use rayon::prelude::*;
            wce_foundation::wce_prof!("game_data.cs2_parse");
            out.cs2 = bytes.par_iter().filter_map(|(key, b)| destruction_levels(b).map(|l| (key.clone(), l))).collect();
        }

        // The battlefield_buildings tables; a mod's rows (TDD's tables are in its main pack) before the game's.
        let is_mod = !(name.starts_with("data") || name.starts_with("tiles"));
        if let Some(schema) = schema {
            wce_foundation::wce_prof!("game_data.tables");
            let tables = pack.files().keys().filter(|n| n.starts_with("db/battlefield_buildings_tables/")).cloned().collect::<Vec<_>>();
            for n in tables {
                let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { continue };
                let mut extra = DecodeableExtraData::default();
                extra.set_schema(Some(schema));
                extra.set_table_name(Some("battlefield_buildings_tables"));
                let Ok(db) = DB::decode(&mut Cursor::new(&bytes), &Some(extra)) else { continue };
                let col = |c: &str| db.column_position_by_name(c);
                let (Some(k), Some(cat), Some(th)) = (col("key"), col("category"), col("destruct_thresholds")) else { continue };
                for row in db.data().iter() {
                    let values = row[th].data_to_string().split(',').filter_map(|v| v.trim().parse::<f32>().ok()).collect::<Vec<_>>();
                    out.rows.push((is_mod, row[k].data_to_string().to_lowercase(), row[cat].data_to_string().into_owned(), values));
                }
            }
        }

        wce_foundation::wce_prof!("game_data.tile_databases");
        let names = pack.files().keys().filter(|n| n.starts_with("terrain/tiles/") && n.ends_with("/tile_database.bin")).cloned().collect::<Vec<_>>();
        for n in names {
            let Some(Ok(Some(bytes))) = pack.files_mut().get_mut(&n).map(|f| f.encode(&None, false, false, true)) else { continue };
            if let Ok(db) = TileDatabase::decode(&mut Cursor::new(&bytes), &None) {
                let kind = n.trim_start_matches("terrain/tiles/").split('/').next().unwrap_or_default().to_owned();
                out.dbs.push((!name.starts_with("tiles"), kind, db));
            }
        }

        Ok(out)
    }
}

impl GameData {
    /// Reads every pack of a game's `data` folder (in name order); `schema` is rpfm's Attila schema for the db tables (without it the
    /// buildings' categories and thresholds are not known).
    ///
    /// # Errors
    ///
    /// The folder cannot be listed.
    pub fn load(data: &Path, schema: Option<&Path>) -> Result<Self, String> {
        let games = rpfm_lib::games::supported_games::SupportedGames::default();
        let attila = games.game("attila").ok_or("rpfm knows no attila")?;
        let mut packs = std::fs::read_dir(data)
            .map_err(|e| format!("{}: {e}", data.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pack"))
            .collect::<Vec<_>>();
        packs.sort();

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

        // Each pack on the thread pool (what it holds doesn't depend on the others), then everything taken in the packs' order.
        let parts: Vec<Result<PackPart, String>> = {
            use rayon::prelude::*;
            packs.par_iter().map(|p| PackPart::read(p, attila, schema.as_ref())).collect()
        };

        let mut dbs: Vec<(bool, String, TileDatabase)> = vec![];
        // The destruction levels: the building's collision file lists each piece's destructs, the building's count is the most over its
        // pieces; without one, the same from the models' names (`<piece>_destructNN.rigid_model_v2`).
        let mut pieces: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
        let mut from_cs2: BTreeMap<String, u32> = BTreeMap::new();
        let mut rows: Vec<(bool, String, String, Vec<f32>)> = vec![];
        for part in parts {
            let part = match part {
                Ok(part) => part,
                Err(w) => {
                    warnings.push(w);
                    continue;
                }
            };

            for (key, piece, level) in part.pieces {
                let e = pieces.entry(key).or_default().entry(piece).or_default();
                *e = (*e).max(level);
            }

            from_cs2.extend(part.cs2);
            rows.extend(part.rows);
            dbs.extend(part.dbs);
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
        super::compile::CompileContext { building_levels: Some(&*self.levels), building_thresholds: Some(&*self.thresholds), ..Default::default() }
    }
}
