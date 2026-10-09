#pragma once
// Small FASTBIN0 tile files: tree lists, grass lists, civilian deployment / shelter lists.
// Layouts and the corpus evidence are in docs/formats/fastbin_tile.md. All little endian.
#include <array>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>

namespace te {

struct TreeInstance { float x, y, z; uint16_t w; };
struct TreeType { std::string name; std::vector<TreeInstance> instances; };
struct TreeList {
    uint16_t version = 3;
    std::vector<TreeType> types;
    static TreeList parse(const std::vector<uint8_t>& bytes);   // throws std::runtime_error
    std::vector<uint8_t> write() const;
};

struct GrassGroup { std::string name; std::vector<uint8_t> data; }; // 6 bytes per entry, layout not decoded
struct GrassList {
    uint16_t version = 1;
    std::vector<GrassGroup> groups;
    static GrassList parse(const std::vector<uint8_t>& bytes);
    std::vector<uint8_t> write() const;
};

struct CivilianDeploymentItem { uint16_t flag; std::string kind, size; std::vector<std::array<float, 2>> points; };
struct CivilianDeployment {
    uint16_t version = 1;
    std::vector<CivilianDeploymentItem> items;
    static CivilianDeployment parse(const std::vector<uint8_t>& bytes);
    std::vector<uint8_t> write() const;
};

struct CivilianShelterItem { uint16_t flag; std::array<float, 4> rect; uint32_t capacity; };
struct CivilianShelter {
    uint16_t version = 1;
    std::vector<CivilianShelterItem> items;
    static CivilianShelter parse(const std::vector<uint8_t>& bytes);
    std::vector<uint8_t> write() const;
};

} // namespace te
