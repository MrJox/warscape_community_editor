//! What one game's exported tiles mean that another's don't, at the same file versions, and the versions its TEd writes: the decompiler
//! and the compiler read them from their contexts (docs/formats/rome2_tiles.md).

use wce_foundation::game::GameProfile;

/// The fields of a building list block past `position_type`, by game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildingListLayout {
    /// Attila's version 12: `toggleable` and `lite` (bools).
    #[default]
    Attila,

    /// Rome 2's version 12: `burn_amount` (an f32).
    Rome2,
}

/// How a game's terrain meshes store the river surface: a block of 8 f32 vertices after the terrain's (`attila::river`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RiverFormat {
    /// Attila's type 90: x, y, z, flow speed, the width across (0 on the strip's first edge), the distance along, alpha fade, the
    /// position across (0 to 1).
    #[default]
    Attila,

    /// Rome 2's type 48: x, y, z, flow speed, the position across (0 to 1), the position along (0 to 1), alpha fade, foam amount (the
    /// packs' blocks: the same rows as Attila's tessellation, these values in TEd's defaults, flow 0.5 and foam 0.1, and others).
    Rome2,
}

impl RiverFormat {
    /// The block's type.
    pub fn block_type(self) -> u32 {
        match self {
            Self::Attila => 0x5a,
            Self::Rome2 => 0x30,
        }
    }

    /// The format of a block type.
    pub fn of_block(kind: u32) -> Option<Self> {
        [Self::Attila, Self::Rome2].into_iter().find(|f| f.block_type() == kind)
    }
}

/// The conventions of a game's exported tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileConventions {
    /// The prop markers version from which a decal's scale fields are signed and its rows' lengths their magnitudes (Attila: 6); before
    /// it (and always, when `None`: Rome 2, whose version 6 only adds the seasons) the fields are magnitudes and the rows carry the sign.
    pub signed_decal_fields_from: Option<i64>,

    /// The layout of a version 12 building list.
    pub building_list: BuildingListLayout,

    /// The version the game's TEd writes each markers file in (by file name), for the files the compile makes without the decompiled
    /// file's version.
    pub marker_versions: &'static [(&'static str, i64)],

    /// The version of the building lists the game's TEd writes.
    pub building_list_version: i64,

    /// The files of BOB's processors the game's tiles have none of (Rome 2's have no `tile.agf`): the compile does not write them.
    pub not_shipped: &'static [&'static str],

    /// The building list versions the game's TEd wrote, first and last (the others are legacy lists, carried as files).
    pub building_list_versions: (i64, i64),

    /// The river block of the terrain meshes.
    pub river: RiverFormat,
}

impl Default for TileConventions {
    fn default() -> Self {
        Self::ATTILA
    }
}

impl TileConventions {
    /// Attila's.
    pub const ATTILA: Self = Self {
        signed_decal_fields_from: Some(6),
        building_list: BuildingListLayout::Attila,
        marker_versions: &[
            ("prop_marker.markers", 9), ("particle_emitter_marker.markers", 9), ("point_light_marker.markers", 3), ("ai_hint_polyline_marker.markers", 1),
            ("ai_separator_marker.markers", 1), ("ai_hint_polyline_list_marker.markers", 1),
        ],
        building_list_version: 12,
        not_shipped: &[],
        building_list_versions: (12, 12),
        river: RiverFormat::Attila,
    };

    /// Rome 2's: the highest versions of its packs (prop 6, particle emitter 5, point light 2).
    pub const ROME_2: Self = Self {
        signed_decal_fields_from: None,
        building_list: BuildingListLayout::Rome2,
        marker_versions: &[
            ("prop_marker.markers", 6), ("particle_emitter_marker.markers", 5), ("point_light_marker.markers", 2), ("ai_hint_polyline_marker.markers", 1),
            ("ai_separator_marker.markers", 1), ("ai_hint_polyline_list_marker.markers", 1),
        ],
        building_list_version: 12,
        not_shipped: &["tile.agf"],
        building_list_versions: (10, 12),
        river: RiverFormat::Rome2,
    };

    /// A game's conventions (Attila's for a game without its own).
    pub fn of(game: &GameProfile) -> Self {
        match game.key {
            "rome_2" => Self::ROME_2,
            _ => Self::ATTILA,
        }
    }

    /// Whether a building list of a version is one the game's TEd wrote (not a legacy list).
    pub fn current_building_list(&self, version: i64) -> bool {
        (self.building_list_versions.0..=self.building_list_versions.1).contains(&version)
    }

    /// Whether a decal record of a prop markers version has signed scale fields (and its rows' lengths their magnitudes).
    pub fn signed_decal_fields(&self, version: i64) -> bool {
        self.signed_decal_fields_from.is_some_and(|from| version >= from)
    }

    /// The version the game's TEd writes a markers file in (a per-climate file, `<climate>particle_emitter_marker.markers`, as its
    /// kind's), 1 for one not listed.
    pub fn marker_version(&self, name: &str) -> i64 {
        let name = name.to_lowercase();
        self.marker_versions.iter().find(|(n, _)| name.ends_with(n)).map_or(1, |(_, v)| *v)
    }
}
