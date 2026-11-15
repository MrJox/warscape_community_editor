//! Combine and Separate (docs/plans/tile_variations.md, "Migration: Combine variations"): variations of a tile, each a project of its own,
//! the kit's or the packs' (decompiled), made one combined project in the projects; and one variation of a combined project made a
//! project of its own again.
//!
//! Combine checks before it writes anything: the combined project is written to a staging folder, each variation is written back from it
//! as a project of its own (`TileProject::materialize`) and compared with what it was made from (its scene through `to_ted`, its maps,
//! its files). Only then are the projects that went in moved to `<projects>/backup/combine/<when>/` and the combined project put in their
//! tile's folder. The kit and the packs are never written.

use std::path::{Path, PathBuf};

use wce_formats::png_file::PngFile;
use wce_formats::tile_entry::TileEntry;
use wce_formats::xml_doc::XmlDoc;

use super::merge::{Source, Stats, check, merge};
use super::{
    GROUP_ATTR, VARIATIONS_ATTR, close_tag, indent_len, line_end, line_start, list_text, own_variations, parse_list, project_variations,
    to_variation, whole_lines,
};
use crate::blend_field::BlendField;
use crate::ground_type_field::GroundTypeField;
use crate::height_field::HeightField;
use crate::project_files::{TED_FILE, WCE_FILE, mark_wce_text, to_ted, to_wce};
use crate::terrain_layers::{Blend, Ground, Height, Stack, TerrainLayers};
use crate::tile_project::TileProject;

/// Where a variation that goes into a combination comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A project of its own in the projects (moved to the backup when combined).
    Project,

    /// An Assembly Kit's tile (read, never written).
    Kit,

    /// The game's packs, decompiled (read, never written).
    Packs,
}

impl Origin {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Kit => "kit",
            Self::Packs => "packs",
        }
    }
}

/// A variation to combine: its name (the entry's row), the folder it is read from, where that is.
#[derive(Clone, Debug)]
pub struct Input {
    pub variation: String,
    pub dir: PathBuf,
    pub origin: Origin,
}

/// BOB's intermediates: not inputs, made again by every export (left out of a combined project; the backup keeps a project's).
const BOB_FILES: [&str; 3] = ["final_heights.dds", "final_alpha.dds", "tile_normal.tga"];

/// The maps a combined project keeps as its terrain layers.
const MAPS: [&str; 3] = ["height_map_0.png", "blend_map.tif", "ground_type_map.png"];

/// A variation read for a combination.
struct Read {
    text: Vec<u8>,
    layers: TerrainLayers,

    /// Its maps as they are on disk: the height map's samples, the blend and ground type maps' bytes.
    height: Vec<u8>,
    blend: Option<Vec<u8>>,
    ground: Option<Vec<u8>>,

    /// Its other files (name, bytes), and its rivers' files (kept for their factors, `river_files`).
    files: Vec<(String, Vec<u8>)>,

    /// The files in its folders (by their path from its folder: a decompiled tile's `decompiled_terrain/...`), always its own.
    below: Vec<(String, Vec<u8>)>,
    rivers: Vec<(String, Vec<u8>)>,
}

/// A variation's scene as tile.wce text: its tile.wce, else its tile.ted read as one.
///
/// # Errors
///
/// It has neither, or it does not read.
pub fn scene_text(dir: &Path) -> Result<Vec<u8>, String> {
    let wce = dir.join(WCE_FILE);
    if wce.is_file() {
        return std::fs::read(&wce).map_err(|e| format!("{}: {e}", wce.display()));
    }

    let ted = dir.join(TED_FILE);
    let bytes = std::fs::read(&ted).map_err(|e| format!("{}: {e}", ted.display()))?;
    to_wce(bytes)
}

fn read_input(input: &Input, entry: &TileEntry) -> Result<Read, String> {
    let dir = &input.dir;
    let v = &input.variation;
    let rows = entry.variations();
    let row = rows.iter().find(|r| r.location.eq_ignore_ascii_case(v)).ok_or_else(|| format!("the entry has no variation '{v}'"))?;
    let text = scene_text(dir)?;
    let file = |name: &str| crate::sources::file_any_case(dir, name);
    let read = |p: &Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));

    let hp = file(MAPS[0]).ok_or_else(|| format!("{v}: no {}", MAPS[0]))?;
    let png = PngFile::parse(&read(&hp)?).map_err(|e| format!("{}: {e}", hp.display()))?;
    let height = png.pixels.clone();
    let hf = HeightField::from_png(&png, row.min_height, row.scale)?;
    let blend = file(MAPS[1]);
    let ground = file(MAPS[2]);
    let mut layers = match file(crate::terrain_layers::FILE) {
        Some(p) => TerrainLayers::from_bytes(&read(&p)?)?,
        None => TerrainLayers::default(),
    };

    if layers.height.is_none() {
        layers.height = Some(Stack::<Height>::new(&hf));
    }

    if layers.blend.is_none()
        && let Some(p) = &blend
    {
        layers.blend = Some(Stack::<Blend>::new(&BlendField::read(p)?));
    }

    if layers.ground.is_none()
        && let Some(p) = &ground
    {
        layers.ground = Some(Stack::<Ground>::new(&GroundTypeField::read(p)?));
    }

    let mut files = vec![];
    let mut rivers = vec![];
    let mut names = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|f| f.path().is_file())
        .map(|f| f.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    for name in names {
        let lower = name.to_lowercase();
        if lower == WCE_FILE
            || lower == TED_FILE
            || lower == crate::terrain_layers::FILE
            || lower.ends_with(".bak")
            || MAPS.contains(&lower.as_str())
            || BOB_FILES.contains(&lower.as_str())
        {
            continue;
        }

        let bytes = read(&dir.join(&name))?;
        if lower.starts_with("river_spline") {
            rivers.push((name, bytes));
        } else {
            files.push((name, bytes));
        }
    }

    // Its folders' files (a decompiled tile's carried terrain and vegetation, what the export writes back while it is unchanged).
    let below = super::materialize::files_below(dir)
        .into_iter()
        .filter(|n| n.contains('/') && !n.ends_with(".bak"))
        .map(|n| read(&dir.join(&n)).map(|b| (n, b)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Read {
        text,
        below,
        layers,
        height,
        blend: blend.as_deref().map(read).transpose()?,
        ground: ground.as_deref().map(read).transpose()?,
        files,
        rivers,
    })
}

/// What the variations share, for the dialog: their scenes merged (nothing written), in the inputs' order.
///
/// # Errors
///
/// A scene does not read, or the merge fails.
pub fn shares(inputs: &[Input]) -> Result<Stats, String> {
    let sources = inputs
        .iter()
        .map(|i| scene_text(&i.dir).map(|text| Source { variation: i.variation.clone(), text }))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(merge(&sources)?.stats)
}

/// What a combination did.
#[derive(Clone, Debug)]
pub struct Report {
    /// The combined project's tile folder.
    pub tile_dir: PathBuf,

    /// Where the projects that went in were moved.
    pub backup: Option<PathBuf>,
    pub stats: Stats,
    pub message: String,
}

/// The projects folder of a `raw_data` in its kit layout (`<projects>/assembly_kit/raw_data`).
fn projects_of(projects_raw: &Path) -> PathBuf {
    projects_raw.parent().and_then(Path::parent).unwrap_or(projects_raw).to_path_buf()
}

/// Now as a folder name, `2026-11-15_143005` (UTC).
#[must_use]
pub fn now_folder() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}_{:02}{:02}{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The entry's file under a `raw_data`.
fn entry_file(entry: &TileEntry, raw: &Path) -> Result<PathBuf, String> {
    let name = entry.file().file_name().ok_or("the entry has no file name")?;
    Ok(raw.join("terrain/tiles").join(entry.kind()).join("_tile_database/TILES").join(name))
}

/// Writes the entry under a `raw_data` (and the tile sets' `_settings.xml` beside it from the entry's own kit, when it has none).
fn put_entry(entry: &TileEntry, raw: &Path) -> Result<(), String> {
    let target = entry_file(entry, raw)?;
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    wce_foundation::files::write_replacing(&target, entry.save()).map_err(|e| format!("{}: {e}", target.display()))?;
    let database = |r: &Path| r.join("terrain/tiles").join(entry.kind()).join("_tile_database");
    let settings = database(raw).join("_settings.xml");
    if !settings.is_file()
        && let Some(from) = entry.raw_root().map(|r| database(&r).join("_settings.xml")).filter(|p| p.is_file())
    {
        std::fs::copy(&from, &settings).map_err(|e| format!("{}: {e}", settings.display()))?;
    }

    wce_formats::tile_index::TileIndex::forget_shared();
    Ok(())
}

/// Moves a file or folder, its parent made; across volumes, copied and removed.
fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }

    copy_tree(from, to)?;
    if from.is_dir() { std::fs::remove_dir_all(from) } else { std::fs::remove_file(from) }.map_err(|e| format!("{}: {e}", from.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_file() {
        std::fs::copy(from, to).map_err(|e| format!("{}: {e}", from.display()))?;
        return Ok(());
    }

    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for f in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        copy_tree(&f.path(), &to.join(f.file_name()))?;
    }

    Ok(())
}

/// Combines variations of the tile of `entry` into one project in the projects (`projects_raw`, `<projects>/assembly_kit/raw_data`):
/// `reference` (of `inputs`) gives the terrain's base. Checked before anything is written (see the module); the projects that went in
/// are moved to the backup.
///
/// # Errors
///
/// Fewer than two variations, the tile is combined already, a variation does not read or does not come back as it was (nothing is
/// written then), or a file can't be written.
pub fn combine(inputs: &[Input], reference: usize, entry: &TileEntry, projects_raw: &Path) -> Result<Report, String> {
    if inputs.len() < 2 {
        return Err("tick at least two variations to combine".to_owned());
    }

    let tile_dir = entry.tile_folder(projects_raw);
    if tile_dir.join(WCE_FILE).is_file() {
        return Err(format!("{} is a combined project already", tile_dir.display()));
    }

    // The reference first (the project's first variation: what the editor draws an element several have as, with everything shown),
    // then the others in their order (each one's terrain patches build on those before it).
    let mut ordered = inputs.to_vec();
    let first = ordered.remove(reference.min(inputs.len() - 1));
    ordered.insert(0, first);
    let (inputs, reference) = (&ordered[..], 0);

    let names = inputs.iter().map(|i| i.variation.clone()).collect::<Vec<_>>();
    let reads = inputs.iter().map(|i| read_input(i, entry).map_err(|e| format!("{}: {e}", i.variation))).collect::<Result<Vec<_>, _>>()?;
    let sources = names.iter().zip(&reads).map(|(v, r)| Source { variation: v.clone(), text: r.text.clone() }).collect::<Vec<_>>();
    let merged = merge(&sources)?;
    check(&merged.text, &sources)?;
    let layer_sources = names.iter().zip(&reads).map(|(v, r)| (v.as_str(), &r.layers)).collect::<Vec<_>>();
    // Heights compared as each variation stores them (its row's range): a level stored in another range still shares its patches.
    let rows = entry.variations();
    let ranges = names
        .iter()
        .map(|v| rows.iter().find(|r| r.location.eq_ignore_ascii_case(v)).map_or((0.0, 1.0), |r| (r.min_height, r.scale)))
        .collect::<Vec<_>>();
    let layers = TerrainLayers::combine_in(&layer_sources, reference, Some(&ranges))?;

    // The files: those every variation has the same go in the tile's folder, the others in each one's own.
    let mut shared = vec![];
    let mut own = vec![vec![]; reads.len()];
    let mut all_names = reads.iter().flat_map(|r| r.files.iter().map(|f| f.0.clone())).collect::<Vec<_>>();
    all_names.sort();
    all_names.dedup();
    for name in all_names {
        let each = reads.iter().map(|r| r.files.iter().find(|f| f.0 == name).map(|f| &f.1)).collect::<Vec<_>>();
        if each.iter().all(|b| b.is_some() && *b == each[0]) {
            shared.push((name, each[0].cloned().unwrap_or_default()));
        } else {
            for (k, b) in each.iter().enumerate() {
                if let Some(b) = b {
                    own[k].push((name.clone(), (*b).clone()));
                }
            }
        }
    }

    for (k, r) in reads.iter().enumerate() {
        own[k].extend(r.rivers.iter().cloned());
        own[k].extend(r.below.iter().cloned());
    }

    // Staged: the combined project as it will be, in a kit layout of its own (with the entry), to read back.
    let projects = projects_of(projects_raw);
    let staging = projects.join("build").join("combine");
    let checking = projects.join("build").join("combine_check");
    for d in [&staging, &checking] {
        if d.exists() {
            std::fs::remove_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
    }

    let result = (|| {
        let staging_raw = staging.join("raw_data");
        let staged = entry.tile_folder(&staging_raw);
        let put = |p: PathBuf, b: &[u8]| -> Result<(), String> {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            }

            std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))
        };
        put(staged.join(WCE_FILE), &mark_wce_text(merged.text.clone()))?;
        put(staged.join(crate::terrain_layers::FILE), &layers.to_bytes()?)?;
        for (name, b) in &shared {
            put(staged.join(name), b)?;
        }

        for (k, files) in own.iter().enumerate() {
            for (name, b) in files {
                put(staged.join(super::materialize::OWN_DIR).join(&names[k]).join(name), b)?;
            }
        }

        put_entry(entry, &staging_raw)?;

        // Each variation back from it, compared with what it was made from.
        let project = TileProject::open(&staged.join(&names[0]))?;
        let back_raw = checking.join("raw_data");
        let mut bad = vec![];
        for (k, read) in reads.iter().enumerate() {
            let v = &names[k];
            let m = project.materialize(v, &project.scene.save(), &back_raw)?;
            let file = |name: &str| std::fs::read(m.dir.join(name)).ok();
            let scene = file(WCE_FILE).ok_or("no tile.wce written")?;
            if to_ted(scene)?.0 != to_ted(read.text.clone())?.0 {
                bad.push(format!("{v}: its scene"));
            }

            let back_height = file(MAPS[0]).and_then(|b| PngFile::parse(&b).ok()).map(|p| p.pixels);
            if m.refit || back_height.as_ref() != Some(&read.height) {
                bad.push(format!("{v}: its height map"));
            }

            if read.blend.is_some() && file(MAPS[1]) != read.blend {
                bad.push(format!("{v}: its blend map"));
            }

            if read.ground.is_some() && file(MAPS[2]) != read.ground {
                bad.push(format!("{v}: its ground type map"));
            }

            for (name, b) in read.files.iter().chain(&read.below) {
                if file(name).as_ref() != Some(b) {
                    bad.push(format!("{v}: {name}"));
                }
            }
        }

        if !bad.is_empty() {
            return Err(format!("the combined project does not give back {}: nothing was written", bad.join(", ")));
        }

        // In place: the projects that went in to the backup, the combined project in their tile's folder, the entry in the projects.
        let location = entry.tile_folder(Path::new(""));
        let backup = projects.join("backup").join("combine").join(now_folder()).join(&location);
        let mut moved = false;
        for i in inputs.iter().filter(|i| i.origin == Origin::Project) {
            move_path(&i.dir, &backup.join(&i.variation))?;
            moved = true;
        }

        std::fs::create_dir_all(&tile_dir).map_err(|e| format!("{}: {e}", tile_dir.display()))?;
        for f in std::fs::read_dir(&staged).map_err(|e| format!("{}: {e}", staged.display()))?.flatten() {
            move_path(&f.path(), &tile_dir.join(f.file_name()))?;
        }

        if !entry_file(entry, projects_raw)?.is_file() {
            put_entry(entry, projects_raw)?;
        }

        wce_formats::tile_index::TileIndex::forget_shared();
        let message = format!(
            "{} combined into {} ({}){}",
            names.join(", "),
            tile_dir.display(),
            merged.stats.summary(),
            if moved { format!("; the projects that went in are in {}", backup.display()) } else { String::new() }
        );
        Ok(Report { tile_dir: tile_dir.clone(), backup: moved.then_some(backup), stats: merged.stats.clone(), message })
    })();

    for d in [&staging, &checking] {
        let _ = std::fs::remove_dir_all(d);
    }

    result
}

/// A combined tile.wce without a variation: what only it has goes, its name leaves every list, a group that is a group in it only gives
/// its place to its contents (as `to_variation` does for the others). Every other variation is as it was ([`separate`] checks it).
///
/// # Errors
///
/// The text does not read, the project does not have that variation, or a group is not laid out one element per line.
pub fn without_variation(text: &[u8], variation: &str) -> Result<Vec<u8>, String> {
    let doc = XmlDoc::parse(text.to_vec()).map_err(|e| e.to_string())?;
    let all = project_variations(&doc);
    if !all.iter().any(|v| v == variation) {
        return Err(format!("the project has no variation '{variation}' (it has {})", list_text(&all)));
    }

    let text = doc.original_text();
    let els = doc.elements();
    let rest = |list: &[String]| list.iter().filter(|v| *v != variation).cloned().collect::<Vec<_>>();
    let mut cuts: Vec<(usize, usize, Vec<u8>)> = vec![];
    let mut dedent = std::collections::BTreeMap::<usize, usize>::new();
    let set = |e: usize, name: &str, list: &[String], cuts: &mut Vec<(usize, usize, Vec<u8>)>| {
        if let Some(i) = doc.find_attr(e, name) {
            let a = &els[e].attrs[i];
            cuts.push((a.value_off, a.value_off + a.value_len, list_text(list).into_bytes()));
        }
    };

    let Some(&root) = doc.roots().first() else { return Err("an empty document".to_owned()) };
    set(root, VARIATIONS_ATTR, &rest(&all), &mut cuts);
    let mut stack = els[root].children.iter().rev().copied().collect::<Vec<_>>();
    while let Some(e) = stack.pop() {
        let el = &els[e];
        if let Some(own) = own_variations(&doc, e)
            && own.iter().any(|v| v == variation)
        {
            let left = rest(&own);
            if left.is_empty() {
                let (s, t) = whole_lines(text, el.start_off, el.end_off).unwrap_or((el.start_off, el.end_off));
                cuts.push((s, t, vec![]));
                continue;
            }

            set(e, VARIATIONS_ATTR, &left, &mut cuts);
        }

        if let Some(groups) = doc.get(e, GROUP_ATTR) {
            let list = parse_list(&groups);
            if list.iter().any(|v| v == variation) {
                let left = rest(&list);
                if left.is_empty() {
                    // A group in this variation only: in the others its contents are in its place already.
                    let bad = || format!("a group at offset {} with {GROUP_ATTR} is not laid out one element per line", el.start_off);
                    let &c = el.children.iter().find(|&&c| els[c].name == "children").ok_or_else(bad)?;
                    let group_line = line_start(text, el.start_off).ok_or_else(bad)?;
                    let after_open = line_end(text, els[c].start_end).ok_or_else(bad)?;
                    let close = close_tag(&doc, text, c).ok_or_else(bad)?;
                    let (close_line, after) = whole_lines(text, close, el.end_off).ok_or_else(bad)?;
                    cuts.push((group_line, after_open, vec![]));
                    cuts.push((close_line, after, vec![]));
                    if let Some(&first) = els[c].children.first() {
                        let unit = indent_len(text, line_start(text, els[first].start_off).ok_or_else(bad)?)
                            .saturating_sub(indent_len(text, group_line));
                        let mut at = after_open;
                        while at < close_line {
                            *dedent.entry(at).or_default() += unit;
                            at = text[at..close_line].iter().position(|&c| c == b'\n').map_or(close_line, |i| at + i + 1);
                        }
                    }

                    stack.extend(els[c].children.iter().rev().copied());
                    continue;
                }

                set(e, GROUP_ATTR, &left, &mut cuts);
            }
        }

        stack.extend(el.children.iter().rev().copied());
    }

    for (at, n) in dedent {
        if text[at..].iter().take(n).any(|&c| c != b' ' && c != b'\t') {
            return Err(format!("a line in a group at offset {at} has less indent than the rest of its contents"));
        }

        cuts.push((at, at + n, vec![]));
    }

    cuts.sort_unstable();
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for (s, t, with) in cuts {
        if t <= at && s < at {
            continue;
        }

        if s >= at {
            out.extend_from_slice(&text[at..s]);
            out.extend_from_slice(&with);
        }

        at = at.max(t);
    }

    out.extend_from_slice(&text[at..]);
    Ok(out)
}

/// Makes a variation of the combined project in `tile_dir` a project of its own again, in its folder (`<tile>/<variation>`), and takes it
/// out of the combined project: its elements and the layers only it has go (Tidy after), its own files go with it. The other variations
/// are checked to be as they were (scene and terrain) before anything of the combined project is written; its old files go to
/// `<projects>/backup/separate/<when>/`. The last variation separated leaves no combined project.
///
/// # Errors
///
/// Not a combined project, no such variation, its folder has a tile already, the others would not stay as they were, or a file can't be
/// written.
pub fn separate(tile_dir: &Path, variation: &str) -> Result<String, String> {
    let wce = tile_dir.join(WCE_FILE);
    let old = std::fs::read(&wce).map_err(|e| format!("{}: {e}", wce.display()))?;
    let all = project_variations(&XmlDoc::parse(old.clone()).map_err(|e| e.to_string())?);
    let name = all
        .iter()
        .find(|v| v.eq_ignore_ascii_case(variation))
        .cloned()
        .ok_or_else(|| format!("{} has no variation '{variation}' (it has {})", tile_dir.display(), all.join(", ")))?;
    let project = TileProject::open(&tile_dir.join(&name))?;
    let raw = project.raw_root.clone().ok_or("the combined project is not in a kit layout")?;
    let entry = project.entry.clone().ok_or("the combined project has no tile database entry")?;
    let row = entry.find_variation(&name).ok_or_else(|| format!("the entry has no variation '{name}'"))?;
    let own_folder = entry.variation_folder(&raw, row);
    if crate::tile_project::has_tile(&own_folder) {
        return Err(format!("{} has a tile already", own_folder.display()));
    }

    let remaining = all.iter().filter(|v| **v != name).cloned().collect::<Vec<_>>();

    // The combined project without it, checked: every other variation as it was.
    let (new_text, new_layers) = if remaining.is_empty() {
        (None, None)
    } else {
        let cut = without_variation(&old, &name)?;
        let mut sc = crate::ted_scene::TedScene::parse(cut)?;
        super::edit::tidy(&mut sc, &remaining)?;
        let text = sc.save();
        for w in &remaining {
            if to_variation(&text, w)? != to_variation(&old, w)? {
                return Err(format!("{w} would not stay as it is without {name}: nothing was written"));
            }
        }

        let layers_file = tile_dir.join(crate::terrain_layers::FILE);
        let layers = if layers_file.is_file() {
            let bytes = std::fs::read(&layers_file).map_err(|e| format!("{}: {e}", layers_file.display()))?;
            let before = TerrainLayers::from_bytes(&bytes)?;
            let mut after = TerrainLayers::from_bytes(&bytes)?;
            let old_keys = super::layer_key_variations(&XmlDoc::parse(old.clone()).map_err(|e| e.to_string())?, &all);
            let new_keys = super::layer_key_variations(&XmlDoc::parse(text.clone()).map_err(|e| e.to_string())?, &remaining);
            let gone = |d: &str| !d.is_empty() && old_keys.contains_key(d) && !new_keys.contains_key(d);
            fn strip<K: crate::terrain_layers::Kind>(s: &mut Stack<K>, v: &str, gone: &dyn Fn(&str) -> bool) {
                s.layers.retain(|l| !(l.variations.len() == 1 && l.variations[0] == v) && !gone(&l.depends));
                for l in &mut s.layers {
                    l.variations.retain(|x| x != v);
                }
            }

            if let Some(s) = after.height.as_mut() {
                strip(s, &name, &gone);
            }

            if let Some(s) = after.blend.as_mut() {
                strip(s, &name, &gone);
            }

            if let Some(s) = after.ground.as_mut() {
                strip(s, &name, &gone);
            }

            for w in &remaining {
                let has = |keys: &std::collections::HashMap<String, Vec<String>>, d: &str| {
                    d.is_empty() || keys.get(d).is_none_or(|l| l.iter().any(|x| x == w))
                };
                let same = before.height.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&old_keys, &l.depends)))
                    == after.height.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&new_keys, &l.depends)))
                    && before.blend.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&old_keys, &l.depends)))
                        == after.blend.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&new_keys, &l.depends)))
                    && before.ground.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&old_keys, &l.depends)))
                        == after.ground.as_ref().map(|s| s.composite(&|l| l.in_variation(w) && has(&new_keys, &l.depends)));
                if !same {
                    return Err(format!("{w}'s terrain would not stay as it is without {name}: nothing was written"));
                }
            }

            Some(after)
        } else {
            None
        };

        (Some(text), layers)
    };

    // It, as a project of its own in its folder (the entry's row with the range its heights are stored with).
    let m = project.materialize(&name, &old, &raw)?;

    // The combined project's old files to the backup; its new ones in place (none after the last).
    let projects = projects_of(&raw);
    let backup = projects.join("backup").join("separate").join(now_folder()).join(entry.tile_folder(Path::new("")));
    std::fs::create_dir_all(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    for n in [WCE_FILE, crate::terrain_layers::FILE] {
        let p = tile_dir.join(n);
        if p.is_file() {
            std::fs::copy(&p, backup.join(n)).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }

    let own = tile_dir.join(super::materialize::OWN_DIR).join(&name);
    if own.is_dir() {
        move_path(&own, &backup.join(super::materialize::OWN_DIR).join(&name))?;
    }

    match (new_text, new_layers) {
        (Some(text), layers) => {
            wce_foundation::files::write_replacing(&wce, mark_wce_text(text)).map_err(|e| format!("{}: {e}", wce.display()))?;
            if let Some(l) = layers {
                let p = tile_dir.join(crate::terrain_layers::FILE);
                wce_foundation::files::write_replacing(&p, l.to_bytes()?).map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }

        (None, _) => {
            // The last: the combined project's files go (its shared files are in the new project too).
            let files = std::fs::read_dir(tile_dir)
                .map_err(|e| format!("{}: {e}", tile_dir.display()))?
                .flatten()
                .filter(|f| f.path().is_file())
                .map(|f| f.path())
                .collect::<Vec<_>>();
            for f in files {
                if let Some(n) = f.file_name() {
                    move_path(&f, &backup.join(n))?;
                }
            }

            let rest = tile_dir.join(super::materialize::OWN_DIR);
            if rest.is_dir() {
                move_path(&rest, &backup.join(super::materialize::OWN_DIR))?;
            }
        }
    }

    wce_formats::tile_index::TileIndex::forget_shared();
    Ok(format!(
        "{name} is a project of its own again in {}{}; the combined project's old files are in {}",
        m.dir.display(),
        if remaining.is_empty() {
            " (it was the last: no combined project is left)".to_owned()
        } else {
            format!(" ({} left combined)", remaining.join(", "))
        },
        backup.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMBINED: &str = "<project wce_variations=\"minor,small,medium\">\r\n    <scene>\r\n        <SCENE_GRAPH name=\"root\">\r\n            <children>\r\n                <LayerInstance name=\"walls\" wce_variations=\"small\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"wall\"/>\r\n                    </children>\r\n                </LayerInstance>\r\n                <LayerInstance name=\"keep\" wce_variations=\"small,medium\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"keep\"/>\r\n                    </children>\r\n                </LayerInstance>\r\n                <BUILDING_INSTANCE key=\"house\"/>\r\n                <UserInstanceGroup name=\"g\" wce_group_variations=\"small\">\r\n                    <children>\r\n                        <BUILDING_INSTANCE key=\"shed\"/>\r\n                    </children>\r\n                </UserInstanceGroup>\r\n            </children>\r\n        </SCENE_GRAPH>\r\n    </scene>\r\n</project>\r\n";

    #[test]
    fn without_a_variation_the_others_are_as_they_were() {
        let out = without_variation(COMBINED.as_bytes(), "small").unwrap();
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(s.starts_with("<project wce_variations=\"minor,medium\">"), "{s}");
        assert!(!s.contains("key=\"wall\""), "what only small has goes");
        assert!(s.contains("<LayerInstance name=\"keep\" wce_variations=\"medium\">"), "{s}");
        assert!(!s.contains("UserInstanceGroup"), "a group in small only gives its place to its contents");
        assert!(s.contains("\r\n                <BUILDING_INSTANCE key=\"shed\"/>\r\n"), "{s}");
        for w in ["minor", "medium"] {
            assert_eq!(to_variation(&out, w).unwrap(), to_variation(COMBINED.as_bytes(), w).unwrap(), "{w}");
        }

        assert!(without_variation(COMBINED.as_bytes(), "large").is_err());
    }

    #[test]
    fn a_folder_name_is_a_date_and_time() {
        let n = now_folder();
        assert_eq!(n.len(), 17, "{n}");
        assert!(n.starts_with("20") && n.as_bytes()[10] == b'_', "{n}");
    }
}
