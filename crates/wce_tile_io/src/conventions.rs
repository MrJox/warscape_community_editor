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

    /// The building list versions the game's TEd wrote, first and last (the others are legacy lists, carried as files).
    pub building_list_versions: (i64, i64),
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
        building_list_versions: (12, 12),
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
        building_list_versions: (10, 12),
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
