//! Where data in the Assembly Kit's layout (tiles, the tile database, battle maps, prefabs, vegetation parameters) is read from. One order
//! for every loader, the first that has a thing wins: the projects' kit layout (the editor's own files), the Assembly Kits in the settings'
//! order, then the game's packs decompiled into the kit's layout (the fallback). Models and textures are not kit-layout data: they keep the
//! game's order (loose folders, then packs).

use std::path::{Path, PathBuf};

use crate::tile_project::has_tile;

/// The kit form of a custom-mesh tile's mesh (the cliffs: 3ds Max's Cas2 export) and the packs' form (BOB's output).
pub const CUSTOM_MESH_CS2: &str = "custom_mesh.cs2";
pub const CUSTOM_MESH_MODEL: &str = "custom_mesh.rigid_model_v2";

/// The tile folder of a placement under a `raw_data` folder: `<raw_data>/<location>/<variation>` (the location as the maps and the tile
/// database name it, `terrain\tiles\battle\...`).
pub fn tile_dir(raw_root: &Path, location: &str, variation: &str) -> PathBuf {
    let mut p = raw_root.to_path_buf();
    for part in location.split(['\\', '/']).filter(|s| !s.is_empty()) {
        p.push(part);
    }

    p.join(variation)
}

/// A file of a folder by its name, whatever its case (the kit has `custom_mesh.CS2` beside `custom_mesh.cs2`).
pub fn file_any_case(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }

    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// A tile with no scene of its own, a mesh instead (the `cliff_custom` set): its mesh file, the kit's CS2 first, else the packs' model.
pub fn custom_mesh(dir: &Path) -> Option<PathBuf> {
    if has_tile(dir) {
        return None;
    }

    file_any_case(dir, CUSTOM_MESH_CS2).or_else(|| file_any_case(dir, CUSTOM_MESH_MODEL))
}

/// Whether a folder holds a tile of either kind: a scene (tile.wce, tile.ted) or a custom mesh.
pub fn is_tile_folder(dir: &Path) -> bool {
    has_tile(dir) || custom_mesh(dir).is_some()
}

/// The folder of a placement's tile in the first of `roots` (`raw_data` folders in the order above) that has it. A variation a combined
/// project serves (docs/plans/tile_variations.md) is a project of its own written for it under the root's build folder
/// (`variations::materialize::served`), written again when the project changed: what reads a tile's folder reads it as any other.
pub fn find_tile(roots: &[PathBuf], location: &str, variation: &str) -> Option<PathBuf> {
    roots.iter().find_map(|r| {
        let d = tile_dir(r, location, variation);
        if is_tile_folder(&d) {
            return Some(d);
        }

        let tile = crate::variations::combined_for(&d)?;
        crate::variations::materialize::served(&tile, variation, r)
            .map_err(|e| wce_foundation::log::warn(format!("{location}\\{variation}: {e}")))
            .ok()
    })
}

/// [`find_tile`] of folders that hold a tile themselves only: a combined project's variation is not written for it.
pub fn find_tile_folder(roots: &[PathBuf], location: &str, variation: &str) -> Option<PathBuf> {
    roots.iter().map(|r| tile_dir(r, location, variation)).find(|d| is_tile_folder(d))
}

/// Whether one of `roots` has a placement's tile: a folder of its own, or a combined project's variation (nothing written).
pub fn serves(roots: &[PathBuf], location: &str, variation: &str) -> bool {
    roots.iter().map(|r| tile_dir(r, location, variation)).any(|d| is_tile_folder(&d) || crate::variations::combined_for(&d).is_some())
}

/// The `raw_data` folders kit-layout data is looked for in, in the order above: the projects' kit layout, then every Assembly Kit in the
/// settings' order (`Settings::assembly_kit_raw_dirs`, `WCE_ASSEMBLY_KIT`), each once. Every kit, whatever it holds (one with only prefabs or
/// only a map is as much a source as one with the tiles). The packs, the fallback, are the caller's: they are not in the kit's layout.
pub fn kit_raw_roots(settings: &wce_foundation::settings::Settings) -> Vec<PathBuf> {
    let mut roots = vec![];
    let projects = settings.kit_projects_dir().join("raw_data");
    if projects.is_dir() {
        roots.push(projects);
    }

    for raw in settings.assembly_kit_raw_dirs() {
        if !roots.contains(&raw) {
            roots.push(raw);
        }
    }

    roots
}

/// The Assembly Kits among them (`kit_raw_roots` without the projects' layout): what a whole kit is needed for (both tile databases,
/// TEd's own files).
pub fn assembly_kit_roots(settings: &wce_foundation::settings::Settings) -> Vec<PathBuf> {
    let projects = settings.kit_projects_dir().join("raw_data");
    kit_raw_roots(settings).into_iter().filter(|r| *r != projects).collect()
}

/// A file below `raw_data` (`/` or `\` separated) in the first of `roots` that has it, its name's case aside (BOB runs on Windows, the kit's
/// names are not all one case).
pub fn first_file(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let (dir, name) = rel.rsplit_once('/').unwrap_or(("", rel.as_str()));
    roots.iter().find_map(|r| file_any_case(&r.join(dir), name))
}

/// A folder below `raw_data` in the first of `roots` that has it.
pub fn first_dir(roots: &[PathBuf], rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    roots.iter().map(|r| r.join(&rel)).find(|d| d.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_root_with_the_tile_wins() {
        let base = std::env::temp_dir().join(format!("wce_sources_{}", std::process::id()));
        let (project, kit, other) = (base.join("project"), base.join("kit"), base.join("other"));
        let loc = "terrain\\tiles\\battle\\plains\\1x1";
        for (root, file) in [(&kit, "tile.ted"), (&project, "tile.wce"), (&other, "custom_mesh.CS2")] {
            let d = tile_dir(root, loc, "aa");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(file), b"x").unwrap();
        }

        assert_eq!(find_tile(&[project.clone(), kit.clone()], loc, "aa"), Some(tile_dir(&project, loc, "aa")));
        assert_eq!(find_tile(&[kit.clone(), project.clone()], loc, "aa"), Some(tile_dir(&kit, loc, "aa")));
        assert_eq!(find_tile(&[base.join("none"), kit.clone()], loc, "aa"), Some(tile_dir(&kit, loc, "aa")));
        assert_eq!(find_tile(&[project.clone()], loc, "ab"), None);
        // A custom-mesh tile, its file's case aside.
        assert!(custom_mesh(&tile_dir(&other, loc, "aa")).is_some());
        assert_eq!(find_tile(&[other.clone()], loc, "aa"), Some(tile_dir(&other, loc, "aa")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_first_root_with_a_file_or_folder_wins_its_names_case_aside() {
        let base = std::env::temp_dir().join(format!("wce_sources_files_{}", std::process::id()));
        let (project, kit) = (base.join("project"), base.join("kit"));
        let settings = "terrain/tiles/battle/_tile_database/_settings.xml";
        for root in [&project, &kit] {
            std::fs::create_dir_all(root.join("terrain/tiles/battle/_tile_database")).unwrap();
        }

        std::fs::write(kit.join(settings), b"kit").unwrap();
        std::fs::write(kit.join("terrain/tiles/battle/_tile_database/_SETTINGS.XML.other"), b"x").unwrap();
        let roots = [project.clone(), kit.clone()];
        assert_eq!(first_file(&roots, settings), Some(kit.join(settings)), "only the kit has it");
        std::fs::write(project.join("terrain/tiles/battle/_tile_database/_Settings.xml"), b"project").unwrap();
        let found = first_file(&roots, "terrain\\tiles\\battle\\_tile_database\\_settings.xml").unwrap();
        assert_eq!(std::fs::read(found).unwrap(), b"project", "the projects' first, its name's case aside");
        assert_eq!(first_file(&roots, "terrain/none.xml"), None);
        assert_eq!(first_dir(&roots, "terrain/tiles/battle"), Some(project.join("terrain/tiles/battle")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
