#pragma once
// warscape_asset_variation_db/warscape_asset_variation_db.bin (FASTBIN0) and its XML form: which environment file a battle uses.
//
// FASTBIN0 layout (little endian, verified byte for byte against the files in the game's packs):
//   "FASTBIN0", u16 version (1), u32 meta count, then per meta: u16 serialise (1), str name, u32 value count, str values;
//   u32 entry count, per entry: str key ("environment"), u16 serialise (1), u32 variation count,
//   per variation: u16 serialise (1), str filename, u32 flags.   (str = u16 length + bytes)
//
// XML form (what the TDD team keeps next to the .bin in the pack): UTF-8, CRLF, tab indentation, single-quoted attributes,
// serialise_version='1' on the root, every meta_data_entry, value and variation. parse_xml ignores whitespace and blank
// lines; to_xml writes the canonical layout (the vanilla file is byte-identical to it).
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

namespace wce {

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

    static VariationDb parse(const std::vector<uint8_t>& bytes); // FASTBIN0; throws std::runtime_error
    std::vector<uint8_t> to_bytes() const;                        // FASTBIN0
    static VariationDb parse_xml(const std::vector<uint8_t>& xml); // throws std::runtime_error
    // The assembly kit's source form, raw_data/warscape_asset_variation_db/environment_map.xml (<environment_file> elements with
    // comma separated lists per category, an empty or missing list accepts every value), as the single entry "environment".
    // BOB_Terrain assigns the bits when it builds the FASTBIN; here the metas follow the order map, climate, season,
    // climate_change_index, precipitation, time_of_day, battle_types, snow_attrition with sorted values (the matching result does
    // not depend on the bit order). Throws std::runtime_error, also when the values do not fit in 32 bits.
    static VariationDb parse_environment_map(const std::vector<uint8_t>& xml);
    std::string to_xml() const;                                    // canonical layout

    bool operator==(const VariationDb& o) const;
    int value_count() const;
    // Problems that make the data wrong for the game: more than 32 values (bits would collide), duplicate names or
    // values, flag bits that belong to no value, a category with no allowed value in a variation (it can never match).
    std::vector<std::string> validate() const;

    // Editing the value lists. Every variation's flags are remapped so that it keeps allowing the same values.
    // add_value appends to the category; a variation that allowed every existing value of that category (and is not
    // unconstrained) also allows the new one. remove_value refuses when a variation would end up with flags 0, which means
    // "any". Both return false with the reason in *why.
    bool add_value(const std::string& category, const std::string& value, std::string* why = nullptr);
    bool remove_value(const std::string& category, const std::string& value, std::string* why = nullptr);
    bool rename_value(const std::string& category, const std::string& from, const std::string& to, std::string* why = nullptr);
    // Flags allowing exactly these values per category (a category left out allows all of its values).
    uint32_t flags_for(const std::map<std::string, std::vector<std::string>>& allowed) const;

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
    // candidates() for tools that pick from a source list: a map name the database does not list is treated as unmatched, so the
    // first round is skipped and the "default_map" round runs (the game's own query adds no bit for an unlisted value and so
    // matches every map in the first round; use candidates() for that behaviour).
    Result candidates_listed_map(const std::string& key, Query q) const;

    // Per category, the values a variation allows (all values when its flags are 0).
    std::vector<std::pair<std::string, std::vector<std::string>>> decode(uint32_t flags) const;
};

} // namespace wce
