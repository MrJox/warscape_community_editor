#pragma once
// BATTLE_MAP_DEFINITION_DATA, binary form (FASTBIN0 version 6): *_procedural_bmd_data.bin and
// bmd_data_escalation{1,2,3}.bin. Layout in docs/formats/fastbin_tile.md. Sections TEd never fills in the
// shipped data (AI hint lists, deployment manager, far buildings, EF lines, zones, prefabs, BMD outlines,
// terrain/lite outlines, civilian lists) must be empty; a non-empty one makes parse() throw so nothing is
// silently dropped.
#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

using Transform16 = std::array<float, 16>; // row-major m00..m33

struct BmdProp {
    std::string key;
    uint16_t marker_version = 2;
    Transform16 transform{};
    uint8_t clamp_to_surface = 0;
    uint32_t type = 0;
    uint8_t allow_in_outfield = 0;
    std::array<float, 4> decal_params{}; // parallax, scale x, y, z
    std::array<uint8_t, 4> flags{};      // tiled, is_fauna, <byte not present in the xml>, 0
};

struct BmdEmitter {
    std::string key;
    uint16_t marker_version = 4;
    float emission_rate = 1;
    Transform16 transform{};
    std::string instance_name;
    uint8_t autoplay = 1;
    float scale = 1;
    std::array<uint8_t, 5> flags{}; // use_in_outfield, snow_in, snow_out, destruction_in, destruction_out
};

struct BmdBuilding {
    uint16_t version = 3;
    std::string key, position_type;
    std::array<float, 3> position{};
    std::array<float, 4> orientation{}; // i j k w
    std::array<float, 3> scale{};
    float starting_damage = 0;
    // on_fire, start_disabled, weak_point, ai_breachable, indestructible, dockable, toggleable, lite
    std::array<uint8_t, 8> flags{};
};

struct BmdCapture {
    std::array<float, 2> location{};
    float radius = 0;
    uint32_t min_players = 0, max_players = 0;
    std::string type, database_key;
    std::vector<std::array<float, 2>> points;
    std::vector<uint32_t> building_indices;
    std::array<float, 2> flag_facing{};
};

using Outline = std::vector<std::array<float, 2>>;

struct BmdBin {
    uint16_t version = 6;
    std::vector<BmdProp> props;
    std::vector<BmdEmitter> emitters;
    uint16_t buildings_version = 1;
    std::vector<BmdBuilding> buildings;
    uint16_t capture_version = 1;
    std::vector<std::vector<BmdCapture>> capture_sets;
    std::vector<Outline> go_outlines, non_terrain_outlines;

    static BmdBin parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error
    std::vector<uint8_t> write() const;
};

} // namespace wce
