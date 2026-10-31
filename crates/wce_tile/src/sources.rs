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

    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
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

/// The folder of a placement's tile in the first of `roots` (`raw_data` folders in the order above) that has it.
pub fn find_tile(roots: &[PathBuf], location: &str, variation: &str) -> Option<PathBuf> {
    roots.iter().map(|r| tile_dir(r, location, variation)).find(|d| is_tile_folder(d))
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
}
