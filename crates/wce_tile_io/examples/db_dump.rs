//! A db table of the game's packs as TSV (every file of `db/<table>/`, through rpfm's schema): the header, then the rows (at most `limit`
//! a file).
//!
//!   [WCE_GAME_KEY=<key>] cargo run --release -p wce_tile_io --example db_dump -- <game dir> <table, e.g. campaign_map_roads_tables> [limit]

use std::io::Cursor;

use rpfm_lib::files::db::DB;
use rpfm_lib::files::{Decodeable, DecodeableExtraData};
use rpfm_lib::schema::Schema;
use wce_vfs::{Config, GameProfile, Vfs};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let game = GameProfile::from_env().clone();
    let mut cfg = Config::new(&a[0], game.clone());
    cfg.mod_packs = Some(vec![]);
    let vfs = Vfs::mount(&cfg).expect("vfs");
    let table = a[1].trim_end_matches('/');
    let limit: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let path = wce_foundation::settings::rpfm_schema_path(&game).expect("an rpfm schema");
    let schema = Schema::load(&path, None).expect("the schema");
    for n in vfs.list(&format!("db/{table}/"), "") {
        let bytes = vfs.read(n).expect("read");
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(&schema));
        extra.set_table_name(Some(table));
        let db = match DB::decode(&mut Cursor::new(&bytes), &Some(extra)) {
            Ok(db) => db,
            Err(e) => {
                println!("# {n}: {e}");
                continue;
            }
        };

        println!("# {n}: {} rows", db.data().len());
        println!("{}", db.definition().fields_processed().iter().map(|f| f.name().to_owned()).collect::<Vec<_>>().join("\t"));
        for row in db.data().iter().take(limit) {
            println!("{}", row.iter().map(|c| c.data_to_string().into_owned()).collect::<Vec<_>>().join("\t"));
        }
    }
}
