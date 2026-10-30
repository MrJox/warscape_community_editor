//! What a game's renderer does differently from Attila's: the shaders of its passes and how it feeds them (docs/re/rome2_renderer.md).

use wce_vfs::files::Files;

/// The renderer of a game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderGame {
    /// Attila's (and a folder of extracted files, which says no game).
    #[default]
    Attila,

    /// Rome 2's: the luminance statistics in log Y (CIE log Y xy), the single tones biases, its own terrain and ocean shaders.
    Rome2,
}

impl RenderGame {
    /// The renderer of the game the files are of.
    pub fn of(files: &dyn Files) -> Self {
        match files.game().map(|g| g.key) {
            Some("rome_2") => Self::Rome2,
            _ => Self::Attila,
        }
    }

    /// Whether the game has the fire variant of a material's G-buffer shaders. Rome 2's packs have rigidstandardfire only (no
    /// rigidtileddirtmapfiremesh, rigidwithdirtburnmesh or rigidambientmapfiremesh): its burning dirt, tiled dirt and ambient map meshes
    /// keep their plain shaders.
    pub(crate) fn has_fire_shaders(self, kind: crate::model_cache::MeshKind) -> bool {
        self == Self::Attila || kind == crate::model_cache::MeshKind::Standard
    }
}
