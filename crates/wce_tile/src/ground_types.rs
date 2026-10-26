//! The ground types a battle tile's `ground_type_map.png` paints (docs/re/terrain.md, `ground_types.dds`): the `ground_types` table's
//! `enum_value`s, each with the colour TEd paints it with (`EMPIREUTILITY::ground_type_to_colour`); black is no ground type (the blend
//! map's strongest group decides there).

/// Name and colour per `enum_value`.
pub const GROUND_TYPES: [(&str, [u8; 3]); 13] = [
    ("forest", [64, 225, 231]), ("grass", [0, 255, 0]), ("mud", [150, 75, 0]), ("sand", [244, 164, 96]), ("scrub", [172, 225, 175]),
    ("rock", [170, 170, 170]), ("deep water", [0, 0, 255]), ("shallow water", [93, 255, 255]), ("road", [255, 96, 0]),
    ("wooden floor", [255, 100, 100]), ("snow", [255, 255, 255]), ("sharp stones", [255, 255, 0]), ("burnt", [64, 0, 0]),
];

/// The colours alone, by `enum_value`.
pub const GROUND_COLOURS: [[u8; 3]; 13] = {
    let mut out = [[0u8; 3]; 13];
    let mut i = 0;
    while i < 13 {
        out[i] = GROUND_TYPES[i].1;
        i += 1;
    }

    out
};

/// No ground type painted.
pub const NO_GROUND_TYPE: [u8; 3] = [0, 0, 0];
