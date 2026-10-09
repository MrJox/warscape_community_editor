#pragma once
// .markers files (TEd logic export: props, particle emitters, point lights, AI hints, separators, ...).
//
// Framing shared by every file (verified on all 449 shipped files, docs/formats/markers.md):
//   STR16 "BASE_MARKER_REPOSITORY"  12 zero bytes  u32 repository_tag (0x0009d9bb)  u32 1  u32 1
//   12 zero bytes?  u32 class_tag  STR16 class_name  12 zero bytes  u32 class_tag  u32 class_version  u32 count
//   count markers (class specific, not decoded yet)
// STR16 = u16 character count + UTF-16LE characters. Until a class is decoded its marker bytes stay opaque in
// `payload`, which keeps the file byte-identical.
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

struct MarkersFile {
    std::string repository_name;          // "BASE_MARKER_REPOSITORY"
    std::vector<uint8_t> prefix;          // bytes between the repository name and the class name (incl. tags)
    std::string class_name;               // e.g. "PROP_MARKER"
    uint32_t class_tag = 0;
    uint32_t class_version = 0;
    uint32_t count = 0;
    std::vector<uint8_t> payload;         // marker data, to end of file

    static MarkersFile parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error
    std::vector<uint8_t> write() const;
};

} // namespace wce
