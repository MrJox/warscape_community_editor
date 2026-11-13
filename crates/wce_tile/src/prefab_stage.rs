//! The Prefab tool's stage (docs/plans/prefab_tool.md): a flat tile, made as a new tile is (`new_tile`) in a folder of its own, never a
//! tile of the user's, holding one group at its middle, right under the root: the prefab's, marked [`STAGE_ATTR`]. What is edited there is saved as the prefab,
//! never as the tile.

use std::path::Path;

use wce_foundation::math::Double3;

use crate::new_tile::{self, NewTile};
use crate::tile_project::TileProject;

/// The mark of the stage's group.
pub const STAGE_ATTR: &str = "wce_stage_root";

/// The stage group's attributes for the prefab's height layer while it is edited (saved into the prefab's `WceTerrain`): whether it
/// normalizes the ground, and its feather.
pub const NORMALIZE_ATTR: &str = "wce_stage_normalize";
pub const FEATHER_ATTR: &str = "wce_stage_feather";

/// The stage's tile name.
pub const STAGE_NAME: &str = "prefab_stage";

/// The stage's size and ground.
#[derive(Clone, Debug, PartialEq)]
pub struct StageSettings {
    /// Cells a side (of 128 world units, as a new tile's).
    pub cells: u32,

    /// The ground's height.
    pub height: f64,

    /// Height map pixels a cell.
    pub density: u32,
}

impl Default for StageSettings {
    fn default() -> Self {
        Self { cells: 2, height: 10.0, density: 128 }
    }
}

impl StageSettings {
    /// The middle of the stage in the scene (x, z), where the group goes.
    #[must_use]
    pub fn middle(&self) -> (f64, f64) {
        let c = f64::from(self.cells) * 128.0;
        (c, c)
    }
}

/// Makes a stage in `root` (emptied first: it is the stage's alone) with the prefab's group text (`PrefabLibrary::load`'s `group_text`,
/// references filled in) at its middle, and opens it.
///
/// # Errors
///
/// The folder can't be emptied or written, or the group text is not one element.
pub fn make(root: &Path, settings: &StageSettings, group_text: &[u8]) -> Result<TileProject, String> {
    if root.exists() {
        std::fs::remove_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    }

    let t = NewTile {
        name: STAGE_NAME.to_owned(),
        cells_x: settings.cells,
        cells_y: settings.cells,
        height: settings.height,
        density: settings.density,
        seed: 1,
    };
    let dir = new_tile::create(&t, root, None)?;
    let mut project = TileProject::open(&dir).map_err(|e| e.to_string())?;
    let graph = project.scene.scene_graph().ok_or("the stage has no scene graph")?;
    let group = project.scene.insert_element_text(graph, group_text).map_err(|e| e.to_string())?;
    let (x, z) = settings.middle();
    project.scene.set_position(group, Double3::new(x, settings.height, z));
    project.scene.set_euler(group, Double3::default());
    project.scene.set_attr(group, STAGE_ATTR, "true");
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stage_holds_the_group_at_its_middle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("stage");
        std::fs::create_dir_all(root.join("left over")).unwrap();
        let group = b"<UserInstanceGroup flags=\" OIF_VISIBLE\" position=\"vector(1,2,3)\" name=\"rocks\" orientation=\"euler(0.5,0,0)\">\n    <children>\n        <BUILDING_INSTANCE position=\"vector(1,0,0)\" key=\"rock\" name=\"rock\"/>\n    </children>\n</UserInstanceGroup>\n";
        let s = StageSettings { cells: 1, density: 32, ..Default::default() };
        let p = make(&root, &s, group).unwrap();
        assert!(!root.join("left over").exists());
        let g = p.scene.find_all("UserInstanceGroup")[0];
        assert_eq!(p.scene.doc().get(g, STAGE_ATTR).as_deref(), Some("true"));
        assert_eq!(p.scene.position(g), Some(Double3::new(128.0, 10.0, 128.0)));
        assert_eq!(p.scene.euler(g), Some(Double3::default()));
        assert_eq!(p.scene.find_all("BUILDING_INSTANCE").len(), 1);

        // Right under the root: no layer between them.
        assert_eq!(p.scene.children(p.scene.scene_graph().unwrap()), vec![g]);

        // Made again, the same.
        let again = make(&root, &s, group).unwrap();
        assert_eq!(again.scene.save(), p.scene.save());
    }
}
