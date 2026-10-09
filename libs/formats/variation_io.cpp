// FASTBIN0 writer, XML reader and writer, validation and value-list editing of VariationDb (see variation_db.h).
#include "formats/variation_db.h"
#include "formats/xml_doc.h"

#include <algorithm>
#include <iterator>
#include <sstream>
#include <stdexcept>

namespace wce {

std::vector<uint8_t> VariationDb::to_bytes() const {
    std::vector<uint8_t> b = {'F', 'A', 'S', 'T', 'B', 'I', 'N', '0'};
    auto u16 = [&](uint32_t v) { b.push_back(uint8_t(v)); b.push_back(uint8_t(v >> 8)); };
    auto u32 = [&](uint32_t v) { for (int i = 0; i < 4; ++i) b.push_back(uint8_t(v >> (8 * i))); };
    auto str = [&](const std::string& s) {
        if (s.size() > 0xffff) throw std::runtime_error("variation db: string longer than 65535 bytes");
        u16(uint32_t(s.size()));
        b.insert(b.end(), s.begin(), s.end());
    };
    u16(1);
    u32(uint32_t(metas.size()));
    for (auto& m : metas) {
        u16(1);
        str(m.name);
        u32(uint32_t(m.values.size()));
        for (auto& v : m.values) str(v);
    }
    u32(uint32_t(entries.size()));
    for (auto& e : entries) {
        str(e.key);
        u16(1);
        u32(uint32_t(e.variations.size()));
        for (auto& v : e.variations) {
            u16(1);
            str(v.filename);
            u32(v.flags);
        }
    }
    return b;
}

namespace {

std::string esc(const std::string& s) { return XmlDoc::encode_entities(s, '\''); }

} // namespace

std::string VariationDb::to_xml() const {
    std::string o = "<warscape_asset_variation_db serialise_version='1'>\r\n\t<meta_data_entries>\r\n";
    for (auto& m : metas) {
        o += "\t\t<meta_data_entry serialise_version='1' name='" + esc(m.name) + "'>\r\n\t\t\t<values";
        for (auto& v : m.values) o += " value='" + esc(v) + "'";
        o += "/>\r\n\t\t</meta_data_entry>\r\n";
    }
    o += "\t</meta_data_entries>\r\n\t<entries>\r\n";
    for (auto& e : entries) {
        o += "\t\t<entry key='" + esc(e.key) + "'>\r\n\t\t\t<value serialise_version='1'>\r\n\t\t\t\t<variations>\r\n";
        for (auto& v : e.variations)
            o += "\t\t\t\t\t<variation serialise_version='1' filename='" + esc(v.filename) + "' flags='" + std::to_string(v.flags) + "'/>\r\n";
        o += "\t\t\t\t</variations>\r\n\t\t\t</value>\r\n\t\t</entry>\r\n";
    }
    o += "\t</entries>\r\n</warscape_asset_variation_db>\r\n";
    return o;
}

VariationDb VariationDb::parse_xml(const std::vector<uint8_t>& xml) {
    XmlDoc doc = XmlDoc::parse(xml);
    auto& el = doc.elements();
    auto fail = [](const std::string& m) { throw std::runtime_error("variation xml: " + m); };
    if (doc.roots().size() != 1 || el[doc.roots()[0]].name != "warscape_asset_variation_db") fail("the root element is not warscape_asset_variation_db");
    auto attr = [&](int e, const char* name) {
        auto v = doc.get(e, name);
        if (!v) fail("<" + el[e].name + "> has no " + name + " attribute");
        return *v;
    };
    auto version = [&](int e) {
        if (attr(e, "serialise_version") != "1") fail("<" + el[e].name + "> serialise_version is not 1");
    };
    auto child = [&](int e, const char* name) {
        int found = -1;
        for (int c : el[e].children)
            if (el[c].name == name) {
                if (found >= 0) fail(std::string("more than one <") + name + "> in <" + el[e].name + ">");
                found = c;
            }
        if (found < 0) fail(std::string("missing <") + name + "> in <" + el[e].name + ">");
        return found;
    };
    const int root = doc.roots()[0];
    version(root);
    VariationDb db;
    for (int m : el[child(root, "meta_data_entries")].children) {
        if (el[m].name != "meta_data_entry") fail("unexpected <" + el[m].name + "> in meta_data_entries");
        version(m);
        Meta meta;
        meta.name = attr(m, "name");
        const int values = child(m, "values");
        for (size_t a = 0; a < el[values].attrs.size(); ++a) {
            if (el[values].attrs[a].name != "value") fail("<values> attribute '" + el[values].attrs[a].name + "' is not 'value'");
            meta.values.push_back(XmlDoc::decode_entities(doc.attr_raw(values, a)));
        }
        db.metas.push_back(std::move(meta));
    }
    for (int e : el[child(root, "entries")].children) {
        if (el[e].name != "entry") fail("unexpected <" + el[e].name + "> in entries");
        Entry entry;
        entry.key = attr(e, "key");
        const int value = child(e, "value");
        version(value);
        for (int v : el[child(value, "variations")].children) {
            if (el[v].name != "variation") fail("unexpected <" + el[v].name + "> in variations");
            version(v);
            Variation var;
            var.filename = attr(v, "filename");
            const std::string f = attr(v, "flags");
            if (f.empty() || f.size() > 10 || f.find_first_not_of("0123456789") != std::string::npos || std::stoull(f) > 0xffffffffull)
                fail("flags '" + f + "' of " + var.filename + " is not an unsigned 32-bit number");
            var.flags = uint32_t(std::stoull(f));
            entry.variations.push_back(std::move(var));
        }
        db.entries.push_back(std::move(entry));
    }
    return db;
}

bool VariationDb::operator==(const VariationDb& o) const {
    if (metas.size() != o.metas.size() || entries.size() != o.entries.size()) return false;
    for (size_t i = 0; i < metas.size(); ++i)
        if (metas[i].name != o.metas[i].name || metas[i].values != o.metas[i].values) return false;
    for (size_t i = 0; i < entries.size(); ++i) {
        if (entries[i].key != o.entries[i].key || entries[i].variations.size() != o.entries[i].variations.size()) return false;
        for (size_t j = 0; j < entries[i].variations.size(); ++j)
            if (entries[i].variations[j].filename != o.entries[i].variations[j].filename || entries[i].variations[j].flags != o.entries[i].variations[j].flags) return false;
    }
    return true;
}

int VariationDb::value_count() const {
    int n = 0;
    for (auto& m : metas) n += int(m.values.size());
    return n;
}

std::vector<std::string> VariationDb::validate() const {
    std::vector<std::string> out;
    const int total = value_count();
    if (total > 32) out.push_back(std::to_string(total) + " values in all categories: the flags have 32 bits, so bits would collide");
    for (size_t i = 0; i < metas.size(); ++i) {
        for (size_t j = i + 1; j < metas.size(); ++j)
            if (metas[i].name == metas[j].name) out.push_back("category '" + metas[i].name + "' appears twice");
        for (size_t a = 0; a < metas[i].values.size(); ++a)
            for (size_t b = a + 1; b < metas[i].values.size(); ++b)
                if (metas[i].values[a] == metas[i].values[b]) out.push_back("value '" + metas[i].values[a] + "' appears twice in '" + metas[i].name + "'");
    }
    if (const Meta* map = meta("map"); map && std::find(map->values.begin(), map->values.end(), "default_map") == map->values.end())
        out.push_back("category 'map' has no value 'default_map': the game asks again with it when nothing matched");
    const uint32_t used = total >= 32 ? 0xffffffffu : ((1u << total) - 1);
    for (auto& e : entries)
        for (auto& v : e.variations) {
            if (v.flags & ~used) out.push_back(v.filename + ": flags have bits that belong to no value");
            if (v.flags == 0) continue; // unconstrained
            for (auto& [cat, allowed] : decode(v.flags))
                if (allowed.empty()) {
                    out.push_back(v.filename + ": no " + cat + " allowed, it can never match");
                    break;
                }
        }
    return out;
}

namespace {

// Moves every flag bit with the index map (-1 drops the bit). Flags 0 stay 0.
uint32_t remap_flags(uint32_t flags, const std::vector<int>& to) {
    if (flags == 0) return 0;
    uint32_t out = 0;
    for (size_t i = 0; i < to.size() && i < 32; ++i)
        if ((flags >> i & 1) && to[i] >= 0) out |= 1u << to[i];
    return out;
}

} // namespace

bool VariationDb::add_value(const std::string& category, const std::string& value, std::string* why) {
    auto no = [&](const std::string& r) { if (why) *why = r; return false; };
    auto it = std::find_if(metas.begin(), metas.end(), [&](const Meta& m) { return m.name == category; });
    if (it == metas.end()) return no("no category '" + category + "'");
    if (value.empty()) return no("the value is empty");
    if (std::find(it->values.begin(), it->values.end(), value) != it->values.end()) return no("'" + value + "' already exists in '" + category + "'");
    if (value_count() >= 32) return no("all 32 flag bits are in use: remove a value of any category first");
    const int first = first_bit(category), count = int(it->values.size()), at = first + count; // index of the new value
    std::vector<int> to(value_count());
    for (int i = 0; i < int(to.size()); ++i) to[i] = i < at ? i : i + 1;
    uint32_t category_bits = 0;
    for (int i = 0; i < count; ++i) category_bits |= 1u << (first + i);
    for (auto& e : entries)
        for (auto& v : e.variations) {
            const bool all_allowed = v.flags != 0 && count > 0 && (v.flags & category_bits) == category_bits;
            v.flags = remap_flags(v.flags, to);
            if (all_allowed) v.flags |= 1u << at;
        }
    it->values.push_back(value);
    return true;
}

bool VariationDb::remove_value(const std::string& category, const std::string& value, std::string* why) {
    auto no = [&](const std::string& r) { if (why) *why = r; return false; };
    auto it = std::find_if(metas.begin(), metas.end(), [&](const Meta& m) { return m.name == category; });
    if (it == metas.end()) return no("no category '" + category + "'");
    auto vi = std::find(it->values.begin(), it->values.end(), value);
    if (vi == it->values.end()) return no("no value '" + value + "' in '" + category + "'");
    if (category == "map" && value == "default_map") return no("the game asks again with map 'default_map' when nothing matched, so that value must stay");
    const int at = first_bit(category) + int(vi - it->values.begin());
    std::vector<int> to(value_count());
    for (int i = 0; i < int(to.size()); ++i) to[i] = i < at ? i : (i == at ? -1 : i - 1);
    for (auto& e : entries)
        for (auto& v : e.variations)
            if (v.flags != 0 && remap_flags(v.flags, to) == 0)
                return no(v.filename + " allows nothing else, removing '" + value + "' would turn it into an unconstrained variation (flags 0)");
    for (auto& e : entries)
        for (auto& v : e.variations) v.flags = remap_flags(v.flags, to);
    it->values.erase(vi);
    return true;
}

bool VariationDb::rename_value(const std::string& category, const std::string& from, const std::string& to, std::string* why) {
    auto no = [&](const std::string& r) { if (why) *why = r; return false; };
    auto it = std::find_if(metas.begin(), metas.end(), [&](const Meta& m) { return m.name == category; });
    if (it == metas.end()) return no("no category '" + category + "'");
    auto vi = std::find(it->values.begin(), it->values.end(), from);
    if (vi == it->values.end()) return no("no value '" + from + "' in '" + category + "'");
    if (to.empty()) return no("the new name is empty");
    if (from != to && std::find(it->values.begin(), it->values.end(), to) != it->values.end()) return no("'" + to + "' already exists in '" + category + "'");
    *vi = to;
    return true;
}

uint32_t VariationDb::flags_for(const std::map<std::string, std::vector<std::string>>& allowed) const {
    uint32_t flags = 0;
    int index = 0;
    for (auto& m : metas) {
        auto it = allowed.find(m.name);
        for (size_t i = 0; i < m.values.size(); ++i)
            if (it == allowed.end() || std::find(it->second.begin(), it->second.end(), m.values[i]) != it->second.end()) flags |= 1u << ((index + int(i)) & 31);
        index += int(m.values.size());
    }
    return flags;
}

VariationDb VariationDb::parse_environment_map(const std::vector<uint8_t>& xml) {
    static const char* const kCategories[] = {"map", "climate", "season", "climate_change_index", "precipitation", "time_of_day", "battle_types", "snow_attrition"};
    XmlDoc doc = XmlDoc::parse(xml);
    const auto& el = doc.elements();
    auto split = [](const std::string& s) {
        std::vector<std::string> out;
        std::stringstream ss(s);
        std::string item;
        while (std::getline(ss, item, ',')) {
            size_t a = item.find_first_not_of(" 	"), b = item.find_last_not_of(" 	");
            if (a != std::string::npos) out.push_back(item.substr(a, b - a + 1));
        }
        return out;
    };
    // per file and category, the listed values
    std::vector<std::string> names;
    std::vector<std::vector<std::vector<std::string>>> lists;
    for (size_t i = 0; i < el.size(); ++i) {
        if (el[i].name != "environment_file") continue;
        const int e = int(i);
        names.push_back(doc.get(e, "filename").value_or(""));
        auto& l = lists.emplace_back();
        for (const char* c : kCategories) l.push_back(split(doc.get(e, c).value_or("")));
    }
    if (names.empty()) throw std::runtime_error("no <environment_file> entries");
    VariationDb db;
    for (size_t c = 0; c < std::size(kCategories); ++c) {
        Meta m;
        m.name = kCategories[c];
        for (auto& l : lists) m.values.insert(m.values.end(), l[c].begin(), l[c].end());
        std::sort(m.values.begin(), m.values.end());
        m.values.erase(std::unique(m.values.begin(), m.values.end()), m.values.end());
        db.metas.push_back(std::move(m));
    }
    if (db.value_count() > 32) throw std::runtime_error("environment_map: more than 32 category values, the flags would collide");
    Entry entry;
    entry.key = "environment";
    for (size_t f = 0; f < names.size(); ++f) {
        uint32_t flags = 0;
        for (size_t c = 0; c < std::size(kCategories); ++c) {
            const Meta& m = db.metas[c];
            const int first = db.first_bit(m.name);
            if (lists[f][c].empty()) { // nothing listed accepts every value of the category
                for (size_t v = 0; v < m.values.size(); ++v) flags |= 1u << ((first + int(v)) & 31);
            } else {
                for (auto& v : lists[f][c]) flags |= db.bit(m.name, v);
            }
        }
        entry.variations.push_back({names[f], flags});
    }
    db.entries.push_back(std::move(entry));
    return db;
}

} // namespace wce
