//! The Unit tool's DB files (`db/<table>_tables/!wce_unit_tool`): every game row of the three tables put in the project unchanged reads
//! back as it was, and the files written read back and write again to the same bytes. Exits non-zero on a difference.
//!
//!   cargo run --release -p wce_unit_tool --example unit_db_check -- <scratch dir>      (the game folder: the settings or WCE_GAME)

use std::sync::Arc;

use rpfm_lib::schema::Schema;
use wce_foundation::settings::{Settings, rpfm_schema_path};
use wce_unit_tool::db::{LooksTables, TABLES, project_path};
use wce_vfs::{Config, GameProfile, Vfs};

fn run(scratch: &std::path::Path) -> Result<usize, String> {
    let game = Settings::load_default().game_dir().ok_or("the game folder is not set (in the settings or WCE_GAME)")?;
    let config = Config::new(game, GameProfile::from_env().clone());
    let vfs: Arc<Vfs> = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    let schema = Schema::load(&rpfm_schema_path(vfs.profile()).ok_or("no rpfm schema")?, None).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(scratch);
    let mut looks = LooksTables::load(&vfs, &schema, scratch)?;
    let mut failures = 0;
    for table in TABLES {
        let rows = looks.table(table).game_rows().to_vec();
        for row in &rows {
            looks.table_mut(table).set(row, &[])?;
        }

        let back = looks.table(table).project_rows();
        let same = rows.iter().filter(|r| back.contains(r)).count();
        println!("{table}: {} game rows, {} in the project, {same} read back as they were", rows.len(), back.len());

        // Rows of one key collapse into one: as many as the table has keys.
        failures += usize::from(same != back.len());
    }

    let written = looks.save(vfs.profile().key, scratch)?;
    println!("written {written:?}");
    let first: Vec<Vec<u8>> = TABLES.iter().map(|t| std::fs::read(scratch.join(project_path(t))).unwrap_or_default()).collect();
    let mut again = LooksTables::load(&vfs, &schema, scratch)?;
    for table in TABLES {
        let before = looks.table(table).project_rows();
        failures += usize::from(again.table(table).project_rows() != before);
        // Saved again unchanged: written as it was, so nothing to write.
        let row = before.first().cloned().unwrap_or_default();
        again.table_mut(table).set(&row, &[])?;
    }

    let rewritten = again.save(vfs.profile().key, scratch)?;
    let second: Vec<Vec<u8>> = TABLES.iter().map(|t| std::fs::read(scratch.join(project_path(t))).unwrap_or_default()).collect();
    println!("read back: {}; written again: {rewritten:?}", if first == second { "same bytes" } else { "DIFFERENT bytes" });
    failures += usize::from(first != second || !rewritten.is_empty());
    Ok(failures)
}

fn main() {
    let scratch = std::env::args().nth(1).map_or_else(|| std::env::temp_dir().join("unit_db_check"), std::path::PathBuf::from);
    match run(&scratch) {
        Ok(0) => println!("unit_db_check: OK"),
        Ok(n) => {
            println!("unit_db_check: {n} failures");
            std::process::exit(1);
        }

        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
