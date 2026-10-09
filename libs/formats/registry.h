#pragma once
// Registry of file formats the round-trip gate knows about.
//
// A format claims files by predicate (extension / magic) and provides one function: parse the bytes into the
// format's model and write them back. The gate compares the result with the input. Formats that are not yet
// implemented are simply not registered; the gate reports their files as "no format".
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace wce {

struct RoundTripResult {
    bool ok = false;
    std::string error;                 // parse failure or first difference
    std::vector<uint8_t> output;       // what the writer produced (empty when it threw)
};

struct Format {
    std::string name;
    std::function<bool(const std::string& path, const std::vector<uint8_t>& head)> claims;
    std::function<RoundTripResult(const std::vector<uint8_t>& bytes)> round_trip;
};

std::vector<Format>& formats();                // populated by register_all_formats()
void register_all_formats();
const Format* find_format(const std::string& path, const std::vector<uint8_t>& head);

// First differing offset message for two byte strings.
std::string describe_diff(const std::vector<uint8_t>& a, const std::vector<uint8_t>& b);

} // namespace wce
