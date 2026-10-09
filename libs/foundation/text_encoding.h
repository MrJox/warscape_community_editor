#pragma once
#include <cstdint>
#include <string>
#include <vector>

namespace wce {

enum class TextEncoding { Utf8, Utf8Bom, Utf16LeBom };

struct DecodedText {
    std::string utf8;
    TextEncoding encoding = TextEncoding::Utf8;
};

// Game XML files are either 8-bit (ASCII/UTF-8, optionally with BOM) or UTF-16 LE with BOM (vfx).
// Decoding is lossless for valid input; encode(decode(x)) == x is verified by the round-trip tool.
DecodedText decode_text(const std::vector<uint8_t>& bytes);
std::vector<uint8_t> encode_text(const std::string& utf8, TextEncoding encoding);

std::vector<uint8_t> read_file(const std::string& path);
bool write_file(const std::string& path, const std::vector<uint8_t>& bytes);

} // namespace wce
