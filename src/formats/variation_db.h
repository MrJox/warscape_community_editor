#pragma once
// warscape_asset_variation_db/warscape_asset_variation_db.bin: which environment file a battle uses.
//
// File layout (FASTBIN0, little endian, verified byte for byte against the XML export of the same file):
//   "FASTBIN0", u16 version (1), u32 meta count, then per meta: u16 serialise (1), str name, u32 value count, str values;
//   u32 entry count, per entry: str key ("environment"), u16 serialise (1), u32 variation count,
//   per variation: u16 serialise (1), str filename, u32 flags.   (str = u16 length + bytes)
//
// Selection, from empire.retail.dll (fns 0x17d9f10 query, 0x1183c740 / 0x1183c690 bits, 0x1862300 match,
// 0x1180c090 pick, 0x10693bf0 random):
//   * every meta value has bit (1 << (index & 31)), the index counting through all metas in file order;
//   * the query ORs the bit of each category's current value (a value missing from the lists adds no bit);
//   * a variation matches when flags == 0 or (flags & query) == query;
//   * the game collects all matches for the battle's map name; if there are none it queries again with map
//     "default_map"; it then picks one match uniformly (unbiased rejection sampling on the 0x343fd LCG).
#include <cstdint>
#include <map>
#include <string>
#include <vector>

namespace we {

struct VariationDb {
    struct Meta {
        std::string name;
        std::vector<std::string> values;
    };
    struct Variation {
        std::string filename; // as stored, backslashes
        uint32_t flags = 0;
    };
    struct Entry {
        std::string key;
        std::vector<Variation> variations;
    };

    std::vector<Meta> metas;
    std::vector<Entry> entries;

    static VariationDb parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error

    const Meta* meta(const std::string& name) const;
    const Entry* entry(const std::string& key) const;
    // Bit of a category's value, or 0 when the category or the value does not exist.
    uint32_t bit(const std::string& category, const std::string& value) const;
    // First bit index of a category (sum of the value counts of the metas before it).
    int first_bit(const std::string& category) const;

    // category -> chosen value. Categories left out add no bit (unconstrained).
    using Query = std::map<std::string, std::string>;
    uint32_t query_mask(const Query& q) const;

    struct Match {
        const Variation* variation;
        int allowed_values; // popcount of the flags, 0 for an unconstrained variation (what the game's "most specific" mode ranks by)
    };
    struct Result {
        std::vector<Match> candidates;
        bool used_default_map = false; // nothing matched the chosen map, so the game asked again with "default_map" (candidates may still be empty)
    };
    // The candidate list the game builds for `key` ("environment").
    Result candidates(const std::string& key, Query q) const;

    // Per category, the values a variation allows (all values when its flags are 0).
    std::vector<std::pair<std::string, std::vector<std::string>>> decode(uint32_t flags) const;
};

} // namespace we
