//! What the unit tables say about a unit's looks against what `UnitCatalog` reads: `variants` (a variant's file, scales), `unit_variants`
//! and `unit_variants_colours`. A report, not a gate: it lists the differences and exits 0.
//!
//!   cargo run --release -p wce_render --example unit_tables_check      (the game folder: the settings or WCE_GAME; WCE_GAME_KEY=rome_2)
//!
//! `WCE_MODS` (packs, comma separated) mounts mods.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::sync::Arc;

use rpfm_lib::files::{Decodeable, DecodeableExtraData, db::DB};
use rpfm_lib::schema::Schema;
use wce_foundation::settings::{Settings, rpfm_schema_path};
use wce_vfs::{Config, GameProfile, Vfs};

/// Every row of a table, by column name; the file each row came from in `__file`.
fn table(vfs: &Vfs, schema: &Schema, table: &str) -> Vec<HashMap<String, String>> {
    let name = format!("{table}_tables");
    let mut out = vec![];
    for path in vfs.list(&format!("db/{name}/"), "") {
        let Ok(bytes) = vfs.read(path) else { continue };
        let mut extra = DecodeableExtraData::default();
        extra.set_schema(Some(schema));
        extra.set_table_name(Some(&name));
        let db = match DB::decode(&mut Cursor::new(bytes), &Some(extra)) {
            Ok(db) => db,
            Err(e) => {
                println!("  {path}: {e}");
                continue;
            }
        };
        let fields = db.definition().fields_processed();
        for row in db.data().iter() {
            let mut r: HashMap<String, String> =
                fields.iter().zip(row.iter()).map(|(f, c)| (f.name().to_owned(), c.data_to_string().to_string())).collect();
            r.insert("__file".into(), path.to_owned());
            out.push(r);
        }
    }

    out
}

fn get<'a>(r: &'a HashMap<String, String>, c: &str) -> &'a str {
    r.get(c).map_or("", String::as_str)
}

/// A `variants` row's file: `variant_filename`, or the name where the table has no such column or leaves it empty (Attila).
fn filename(r: &HashMap<String, String>) -> &str {
    let f = get(r, "variant_filename");
    if f.is_empty() { get(r, "variant_name") } else { f }
}

fn vmd(vfs: &Vfs, name: &str) -> bool {
    !name.is_empty() && vfs.read(&format!("variantmeshes/variantmeshdefinitions/{name}.variantmeshdefinition")).is_ok()
}

fn show<T: std::fmt::Display>(title: &str, items: &[T]) {
    println!("{title}: {}", items.len());
    for i in items.iter().take(12) {
        println!("    {i}");
    }
}

fn run() -> Result<(), String> {
    let game = Settings::load_default().game_dir().ok_or("the game folder is not set (in the settings or WCE_GAME)")?;
    let mut config = Config::new(game, GameProfile::from_env().clone());
    config.use_mods = std::env::var("WCE_MODS").is_ok();
    config.mod_packs = std::env::var("WCE_MODS").ok().map(|m| m.split(',').map(str::to_owned).collect());
    let vfs: Arc<Vfs> = Arc::new(Vfs::mount(&config).map_err(|e| e.to_string())?);
    let schema = Schema::load(&rpfm_schema_path(vfs.profile()).ok_or("no rpfm schema")?, None).map_err(|e| e.to_string())?;
    println!("game: {}", vfs.profile().key);

    // variants
    let variants = table(&vfs, &schema, "variants");
    let mut by_name: HashMap<String, &HashMap<String, String>> = HashMap::new();
    for r in &variants {
        by_name.entry(get(r, "variant_name").to_owned()).or_insert(r);
    }
    println!(
        "\nvariants: {} rows, columns {:?}",
        variants.len(),
        variants.first().map(|r| r.keys().filter(|k| *k != "__file").collect::<Vec<_>>())
    );
    let renamed: Vec<String> = variants
        .iter()
        .filter(|r| filename(r) != get(r, "variant_name"))
        .map(|r| format!("{} -> {} ({})", get(r, "variant_name"), filename(r), get(r, "__file")))
        .collect();
    show("  variant_filename != variant_name", &renamed);
    let file_missing_name_ok: Vec<String> = variants
        .iter()
        .filter(|r| filename(r) != get(r, "variant_name"))
        .map(|r| {
            format!("{}: by name {}, by filename {}", get(r, "variant_name"), vmd(&vfs, get(r, "variant_name")), vmd(&vfs, filename(r)))
        })
        .collect();
    show("  of those, which VMD exists", &file_missing_name_ok);
    for col in ["scale", "scale_variation", "mount_scale"] {
        let mut values: BTreeMap<String, usize> = BTreeMap::new();
        for r in &variants {
            *values.entry(get(r, col).to_owned()).or_default() += 1;
        }
        println!("  {col} values: {values:?}");
    }
    let mut folders: BTreeMap<String, usize> = BTreeMap::new();
    for r in &variants {
        *folders.entry(get(r, "tech_folder").to_owned()).or_default() += 1;
    }
    println!("  tech_folder values: {folders:?}");
    let low: Vec<String> = variants
        .iter()
        .filter(|r| !get(r, "low_poly_filename").is_empty())
        .map(|r| format!("{} low {}", get(r, "variant_name"), get(r, "low_poly_filename")))
        .collect();
    show("  low_poly_filename set", &low);

    // unit_variants
    let uv = table(&vfs, &schema, "unit_variants");
    println!(
        "\nunit_variants: {} rows, columns {:?}",
        uv.len(),
        uv.first().map(|r| r.keys().filter(|k| *k != "__file").collect::<Vec<_>>())
    );
    let not_in_variants: Vec<String> = uv
        .iter()
        .filter(|r| !by_name.contains_key(get(r, "variant")))
        .map(|r| format!("{} {}: {}", get(r, "unit"), get(r, "faction"), get(r, "variant")))
        .collect();
    show("  variant not a variants row", &not_in_variants);
    let mut catalog_ok = 0;
    let mut catalog_wrong = vec![];
    let mut both_missing = vec![];
    for r in &uv {
        let v = get(r, "variant");
        let filename = by_name.get(v).map_or(v, |row| filename(row));
        let (a, b) = (vmd(&vfs, v), vmd(&vfs, filename));
        match (a, b, v == filename) {
            (_, _, true) if a => catalog_ok += 1,
            (_, true, false) => {
                catalog_wrong.push(format!("{} {}: {} -> {} (by name: {a})", get(r, "unit"), get(r, "faction"), v, filename))
            }
            _ => both_missing.push(format!("{} {}: {} -> {}", get(r, "unit"), get(r, "faction"), v, filename)),
        }
    }
    println!("  catalog's file (variant as name) is the table's: {catalog_ok}");
    show("  the table's file differs from the catalog's", &catalog_wrong);
    show("  no VMD either way", &both_missing);
    let scaled: Vec<String> = uv
        .iter()
        .filter_map(|r| {
            let row = by_name.get(get(r, "variant"))?;
            let s: f32 = get(row, "scale").parse().unwrap_or(1.0);
            let sv: f32 = get(row, "scale_variation").parse().unwrap_or(0.0);
            ((s - 1.0).abs() > 1e-4 || sv.abs() > 1e-4).then(|| {
                format!(
                    "{} {}: variants scale {s} var {sv}, unit_variants height {} var {}",
                    get(r, "unit"),
                    get(r, "faction"),
                    get(r, "height_scale"),
                    get(r, "height_variation")
                )
            })
        })
        .collect();
    show("  units whose variant has a scale of its own", &scaled);

    // unit_variants_colours
    let colours = table(&vfs, &schema, "unit_variants_colours");
    println!(
        "\nunit_variants_colours: {} rows, columns {:?}",
        colours.len(),
        colours.first().map(|r| r.keys().filter(|k| *k != "__file").collect::<Vec<_>>())
    );
    let mut per_unit: BTreeMap<String, usize> = BTreeMap::new();
    for r in &colours {
        *per_unit.entry(format!("{} / {}", get(r, "unit_variant"), get(r, "faction"))).or_default() += 1;
    }
    println!("  (unit_variant, faction) pairs: {}", per_unit.len());
    let uv_keys: HashMap<&str, &HashMap<String, String>> = uv.iter().map(|r| (get(r, "name"), r)).collect();
    let sample: Vec<String> = colours
        .iter()
        .map(|r| {
            let unit = uv_keys.get(get(r, "unit_variant")).map_or("?", |u| get(u, "unit"));
            format!(
                "{} (unit {unit}) faction '{}' soldier_type '{}': {} {} {} | rgb {},{},{} {},{},{} {},{},{}",
                get(r, "unit_variant"),
                get(r, "faction"),
                get(r, "soldier_type"),
                get(r, "primary_colour_hex"),
                get(r, "secondary_colour_hex"),
                get(r, "tertiary_colour_hex"),
                get(r, "primary_colour_r"),
                get(r, "primary_colour_g"),
                get(r, "primary_colour_b"),
                get(r, "secondary_colour_r"),
                get(r, "secondary_colour_g"),
                get(r, "secondary_colour_b"),
                get(r, "tertiary_colour_r"),
                get(r, "tertiary_colour_g"),
                get(r, "tertiary_colour_b"),
            )
        })
        .collect();
    show("  rows", &sample);
    let mut soldier_types: BTreeMap<String, usize> = BTreeMap::new();
    for r in &colours {
        *soldier_types.entry(get(r, "soldier_type").to_owned()).or_default() += 1;
    }
    println!("  soldier_type values: {soldier_types:?}");
    let mut uv_names = 0;
    for r in &colours {
        uv_names += usize::from(uv_keys.contains_key(get(r, "unit_variant")));
    }
    println!("  rows whose unit_variant is a unit_variants name: {uv_names}");

    // mounts
    let mounts = table(&vfs, &schema, "mounts");
    let mount_files: Vec<String> = mounts
        .iter()
        .filter_map(|r| {
            let v = get(r, "variant");
            let row = by_name.get(v)?;
            (filename(row) != v).then(|| format!("{}: {} -> {}", get(r, "key"), v, filename(row)))
        })
        .collect();
    show("\nmounts whose variant's file differs from its name", &mount_files);

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
