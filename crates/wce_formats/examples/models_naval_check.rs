//! Gate of the `models_naval` reader: decodes every `db/models_naval_tables/*` of the games' vanilla packs, checks that each encodes back
//! byte for byte, and compares the rows against the kit's ship logic XMLs (by default every
//! `assembly_kit/raw_data/art/battle/naval/models/ships/*/logic.xml` of the games; a row matches an XML when its display folder ends in
//! the XML's folder name). Exits non-zero on any failure.
//!
//!   cargo run --release -p wce_formats --example models_naval_check -- [<game key>=<game dir>]... [--xml <logic.xml>]...
//!
//! Without games, `WCE_ATTILA_DIR` and `WCE_ROME2_DIR` (default this machine's Steam folders) are used, the ones that exist.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rpfm_lib::files::pack::Pack;
use rpfm_lib::games::supported_games::SupportedGames;
use wce_formats::XmlDoc;
use wce_formats::models_naval::{self, ModelsNaval, ShipLogic};

const TOLERANCE: f32 = 2e-3;

fn default_games() -> Vec<(String, PathBuf)> {
    let steam = "D:/Juegos/Steam/steamapps/common";
    [("attila", "WCE_ATTILA_DIR", "Total War Attila"), ("rome_2", "WCE_ROME2_DIR", "Total War Rome II")]
        .into_iter()
        .map(|(key, var, folder)| (key.to_owned(), std::env::var_os(var).map_or_else(|| Path::new(steam).join(folder), PathBuf::from)))
        .filter(|(_, dir)| dir.is_dir())
        .collect()
}

/// The vanilla `models_naval` table files of a game: (path, bytes).
fn tables(key: &str, dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let games = SupportedGames::default();
    let game = games.game(key).ok_or_else(|| format!("rpfm doesn't know {key}"))?;
    let mut pack = Pack::read_and_merge_ca_packs(game, dir).map_err(|e| e.to_string())?;
    let mut paths: Vec<String> = pack.files().keys().filter(|p| p.to_lowercase().starts_with("db/models_naval_tables/")).cloned().collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let file = pack.files_mut().get_mut(&p).ok_or("vanished")?;
            let bytes = file.encode(&None, false, false, true).map_err(|e| e.to_string())?.ok_or("no data")?;
            Ok((p, bytes))
        })
        .collect()
}

fn kit_xmls(dir: &Path) -> Vec<PathBuf> {
    let ships = dir.join("assembly_kit/raw_data/art/battle/naval/models/ships");
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(ships).into_iter().flatten().flatten().map(|e| e.path().join("logic.xml")).filter(|p| p.is_file()).collect();
    out.sort();
    out
}

/// The XML of a ship logic, as elements with children by name.
struct Xml(XmlDoc);

impl Xml {
    fn children<'a>(&'a self, e: usize, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.0.elements()[e].children.iter().copied().filter(move |&c| self.0.elements()[c].name == name)
    }

    fn child(&self, e: usize, name: &str) -> Option<usize> {
        self.children(e, name).next()
    }

    fn all(&self, e: usize, section: &str, name: &str) -> Vec<usize> {
        self.child(e, section).map(|s| self.children(s, name).collect()).unwrap_or_default()
    }

    fn num(&self, e: usize, name: &str) -> f32 {
        self.0.get(e, name).and_then(|v| v.trim().parse().ok()).unwrap_or(f32::NAN)
    }

    /// An XML point in model space: x, z, y.
    fn point(&self, e: usize) -> [f32; 3] {
        [self.num(e, "x"), self.num(e, "z"), self.num(e, "y")]
    }

    fn text(&self, e: usize, name: &str) -> String {
        self.0.get(e, name).unwrap_or_default()
    }
}

fn close(a: &[f32], b: &[f32], tolerance: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

/// Compares a row against its logic.xml; the differences, at most a few per section.
fn compare(ship: &ShipLogic, x: &Xml, root: usize) -> Vec<String> {
    let mut errors = vec![];
    let mut fail = |section: &str, msg: String| {
        if errors.iter().filter(|e: &&String| e.starts_with(section)).count() < 5 {
            errors.push(format!("{section}: {msg}"));
        }
    };

    let bobbies = x.all(root, "buoyancy", "bobby");
    if bobbies.len() != ship.bobbies.len() {
        fail("bobbies", format!("{} in the table, {} in the XML", ship.bobbies.len(), bobbies.len()));
    }
    for (b, &e) in ship.bobbies.iter().zip(&bobbies) {
        let (breach_radius, breach) = x.child(e, "breach").map_or((0.0, [0.0; 3]), |br| (x.num(br, "radius"), x.point(br)));
        let connections: Vec<u32> = x.children(e, "connection").map(|c| x.num(c, "bobby") as u32).collect();
        let want = [[x.num(e, "mass"), x.num(e, "radius")].as_slice(), &x.point(e), &[breach_radius], &breach].concat();
        let have = [[b.mass, b.radius].as_slice(), &b.position, &[b.breach_radius], &b.breach_offset].concat();
        if b.id != x.num(e, "id") as u32 || !close(&have, &want, 0.01) || b.connections != connections {
            fail("bobbies", format!("bobby {} differs", b.id));
        }
    }

    let decks = x.all(root, "decks", "deck");
    if decks.len() != ship.decks.len() {
        fail("decks", format!("{} in the table, {} in the XML", ship.decks.len(), decks.len()));
    }
    for (d, &e) in ship.decks.iter().zip(&decks) {
        let pieces: Vec<usize> = x.children(e, "piece").collect();
        if d.name != x.text(e, "id") || d.pieces.len() != pieces.len() {
            fail("decks", format!("deck {}: {} pieces, XML {} with {}", d.name, d.pieces.len(), x.text(e, "id"), pieces.len()));
        }
        for (p, &pe) in d.pieces.iter().zip(&pieces) {
            let corners: Vec<usize> = x.children(pe, "corner").collect();
            let same = p.id == x.num(pe, "id") as u32
                && p.corners.len() == corners.len()
                && p.corners
                    .iter()
                    .zip(&corners)
                    .all(|(c, &ce)| close(&c.position, &x.point(ce), TOLERANCE) && c.hard == (x.text(ce, "type") == "hard"));
            if !same {
                fail("decks", format!("deck {} piece {} differs", d.name, p.id));
            }
            let off_plane =
                p.corners.iter().any(|c| (c.position.iter().zip(&p.plane).map(|(a, b)| a * b).sum::<f32>() + p.plane[3]).abs() > TOLERANCE);
            if off_plane {
                fail("decks", format!("deck {} piece {}: a corner is off the plane", d.name, p.id));
            }
        }
    }

    let pipes = x.all(root, "pipes", "pipe");
    if pipes.len() != ship.pipes.len() {
        fail("pipes", format!("{} in the table, {} in the XML", ship.pipes.len(), pipes.len()));
    }
    for (p, &e) in ship.pipes.iter().zip(&pipes) {
        let segments: Vec<usize> = x.children(e, "segment").collect();
        let piece = |s: Option<&usize>| s.map_or(f32::NAN, |&s| x.num(s, "deck_piece")) as i32;
        let same = p.points.len() == segments.len()
            && p.points.iter().zip(&segments).all(|(v, &s)| close(v, &x.point(s), TOLERANCE))
            && p.start_deck_piece == piece(segments.first())
            && p.end_deck_piece == piece(segments.last());
        if !same {
            fail("pipes", format!("pipe {} ({}) differs", p.id, x.text(e, "id")));
        }
    }

    let parts = x.all(root, "damage_model", "part");
    if parts.len() != ship.damage_parts.len() {
        fail("damage", format!("{} parts in the table, {} in the XML", ship.damage_parts.len(), parts.len()));
    }
    for (part, &e) in ship.damage_parts.iter().zip(&parts) {
        let panels: Vec<usize> = x.children(e, "panel").collect();
        let same = part.name == x.text(e, "name")
            && part.panels.len() == panels.len()
            && part.panels.iter().zip(&panels).all(|(panel, &pe)| {
                let vertices: Vec<usize> = x.children(pe, "vertex").collect();
                panel.len() == vertices.len() && panel.iter().zip(&vertices).all(|(v, &ve)| close(v, &x.point(ve), TOLERANCE))
            });
        if !same {
            fail(
                "damage",
                format!(
                    "part {} ({} panels) differs from the XML's {} ({} panels)",
                    part.name,
                    part.panels.len(),
                    x.text(e, "name"),
                    panels.len()
                ),
            );
        }
    }

    let faces = x.all(root, "collision", "face");
    if faces.len() != ship.collision.len() {
        fail("collision", format!("{} faces in the table, {} in the XML", ship.collision.len(), faces.len()));
    }
    for (f, &e) in ship.collision.iter().zip(&faces) {
        let vertices: Vec<usize> = x.children(e, "vertex").collect();
        let same = f.name == x.text(e, "name")
            && f.index == x.num(e, "index") as u32
            && vertices.len() == 3
            && f.vertices.iter().zip(&vertices).all(|(v, &ve)| close(v, &x.point(ve), TOLERANCE));
        if !same {
            fail("collision", format!("face {} differs", f.index));
        }
    }

    let lines = x.all(root, "ef_lines", "ef_line");
    if lines.len() != ship.ef_lines.len() {
        fail("ef_lines", format!("{} in the table, {} in the XML", ship.ef_lines.len(), lines.len()));
    }
    for (l, &e) in ship.ef_lines.iter().zip(&lines) {
        let ends: Vec<[f32; 3]> = x.children(e, "end").map(|c| x.point(c)).collect();
        let d = x.child(e, "direction").map_or([f32::NAN; 3], |c| x.point(c));
        let len = d.iter().map(|v| v * v).sum::<f32>().sqrt();
        let same = l.name == x.text(e, "name")
            && l.purpose_name() == Some(x.text(e, "action").as_str())
            && ends.len() == 2
            && close(&l.ends.concat(), &ends.concat(), TOLERANCE)
            && close(&l.direction, &d.map(|v| v / len), 3e-3);
        if !same {
            fail(
                "ef_lines",
                format!("{} ({:?}) differs from the XML's {} ({})", l.name, l.purpose_name(), x.text(e, "name"), x.text(e, "action")),
            );
        }
    }

    let positions = x.all(root, "custom_positions", "custom_position");
    if positions.len() != ship.custom_positions.len() {
        fail("custom_positions", format!("{} in the table, {} in the XML", ship.custom_positions.len(), positions.len()));
    }
    for (c, &e) in ship.custom_positions.iter().zip(&positions) {
        let p = x.child(e, "position").map_or([f32::NAN; 3], |pe| x.point(pe));
        if c.name != x.text(e, "name") || !close(&c.position, &p, TOLERANCE) {
            fail("custom_positions", format!("{} differs", c.name));
        }
    }

    for section in ["cannons", "ef_artillery_info"] {
        if x.child(root, section).is_some_and(|s| !x.0.elements()[s].children.is_empty()) {
            fail(section, "the XML has entries; the table's layout for them is unknown".to_owned());
        }
    }

    errors
}

fn summary(table: &ModelsNaval) -> String {
    let rows = &table.rows;
    let sum = |f: fn(&ShipLogic) -> usize| rows.iter().map(f).sum::<usize>();
    format!(
        "version {:?}, {} rows: {} cameras, {} bobbies, {} decks / {} pieces, {} pipes, {} damage parts / {} panels, {} collision faces, {} ef lines, {} custom positions",
        table.version,
        rows.len(),
        sum(|s| s.cameras.len()),
        sum(|s| s.bobbies.len()),
        sum(|s| s.decks.len()),
        sum(|s| s.decks.iter().map(|d| d.pieces.len()).sum()),
        sum(|s| s.pipes.len()),
        sum(|s| s.damage_parts.len()),
        sum(|s| s.damage_parts.iter().map(|p| p.panels.len()).sum()),
        sum(|s| s.collision.len()),
        sum(|s| s.ef_lines.len()),
        sum(|s| s.custom_positions.len()),
    )
}

fn main() -> ExitCode {
    let mut games = vec![];
    let mut xmls = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--xml" {
            xmls.push(PathBuf::from(args.next().expect("--xml takes a path")));
        } else if let Some((key, dir)) = arg.split_once('=') {
            games.push((key.to_owned(), PathBuf::from(dir)));
        } else {
            eprintln!("usage: models_naval_check [<game key>=<game dir>]... [--xml <logic.xml>]...");
            return ExitCode::FAILURE;
        }
    }

    if games.is_empty() {
        games = default_games();
    }
    if xmls.is_empty() {
        xmls = games.iter().flat_map(|(_, dir)| kit_xmls(dir)).collect();
    }

    let mut failures = 0;
    let mut decoded = vec![];
    for (key, dir) in &games {
        let files = match tables(key, dir) {
            Ok(files) if !files.is_empty() => files,
            Ok(_) => {
                println!("{key}: no models_naval table");
                continue;
            }
            Err(e) => {
                println!("{key}: FAIL reading the packs: {e}");
                failures += 1;
                continue;
            }
        };

        for (path, bytes) in files {
            match models_naval::decode(&bytes) {
                Ok(table) => {
                    let same = models_naval::encode(&table).is_ok_and(|b| b == bytes);
                    println!(
                        "{key} {path} ({} bytes): {}; round trip {}",
                        bytes.len(),
                        summary(&table),
                        if same { "identical" } else { "DIFFERS" }
                    );
                    failures += usize::from(!same);
                    decoded.push((key.clone(), table));
                }
                Err(e) => {
                    println!("{key} {path}: FAIL {e}");
                    failures += 1;
                }
            }
        }
    }

    for path in &xmls {
        let folder = path.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        let xml = match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| XmlDoc::parse(b).map_err(|e| e.to_string())) {
            Ok(doc) => Xml(doc),
            Err(e) => {
                println!("{}: FAIL {e}", path.display());
                failures += 1;
                continue;
            }
        };
        let Some(&root) = xml.0.roots().iter().find(|&&r| xml.0.elements()[r].name == "ship_logic") else {
            println!("{}: FAIL no <ship_logic>", path.display());
            failures += 1;
            continue;
        };

        let mut matched = 0;
        for (key, table) in &decoded {
            for ship in table.rows.iter().filter(|s| s.display_folder.to_lowercase().rsplit(['\\', '/']).next() == Some(folder.as_str())) {
                matched += 1;
                let errors = compare(ship, &xml, root);
                if errors.is_empty() {
                    println!(
                        "{folder} vs {key} {}: matches ({} bobbies, {} deck pieces, {} pipes, {} panels, {} faces, {} ef lines, {} custom positions)",
                        ship.key,
                        ship.bobbies.len(),
                        ship.decks.iter().map(|d| d.pieces.len()).sum::<usize>(),
                        ship.pipes.len(),
                        ship.damage_parts.iter().map(|p| p.panels.len()).sum::<usize>(),
                        ship.collision.len(),
                        ship.ef_lines.len(),
                        ship.custom_positions.len()
                    );
                } else {
                    failures += 1;
                    println!("{folder} vs {key} {}: FAIL", ship.key);
                    errors.iter().for_each(|e| println!("    {e}"));
                }
            }
        }
        if matched == 0 {
            println!("{folder}: no row uses this folder");
        }
    }

    if failures == 0 {
        println!("ok");
        ExitCode::SUCCESS
    } else {
        println!("{failures} failures");
        ExitCode::FAILURE
    }
}
