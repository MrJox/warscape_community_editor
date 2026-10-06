#pragma once
// Lossless XML document.
//
// The original text is never re-serialised. Elements and attributes only record byte spans into it;
// edits are span replacements applied on save. An unedited document saves byte-identically, and an
// edited one differs only inside the replaced spans. Multiple top-level elements are allowed
// (vfx files have <shader_library/>, <vfx> and <vfx_references> side by side).
#include "core/text_encoding.h"

#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace we {

struct XmlAttr {
    std::string name;
    size_t value_off = 0; // span of the raw value text, without quotes
    size_t value_len = 0;
    char quote = '\'';
    bool appended = false; // added by an edit; raw value lives in XmlDoc, inserted at the element's attrs_end
};

struct XmlElement {
    std::string name;
    int parent = -1;
    std::vector<int> children;
    std::vector<XmlAttr> attrs;
    size_t start_off = 0;   // offset of '<'
    size_t attrs_end = 0;   // offset of '/>' or '>' that ends the start tag (insertion point for attributes)
    size_t start_end = 0;   // offset just after the start tag
    size_t end_off = 0;     // offset just after the matching end tag (== start_end if self-closing)
    bool self_closing = false;
};

class XmlDoc {
public:
    static XmlDoc parse(const std::vector<uint8_t>& bytes); // throws std::runtime_error with offset
    std::vector<uint8_t> save() const;

    const std::vector<XmlElement>& elements() const { return elems_; }
    const std::vector<int>& roots() const { return roots_; }
    TextEncoding encoding() const { return encoding_; }
    const std::string& original_text() const { return text_; }

    // Current raw value (edits applied), still entity-encoded.
    std::string_view attr_raw(int elem, size_t attr) const;
    // Value as loaded (empty for attributes added by an edit).
    std::string_view original_raw(int elem, size_t attr) const;
    bool attr_modified(int elem, size_t attr) const { return attr_raw(elem, attr) != original_raw(elem, attr) || elems_[elem].attrs[attr].appended; }
    const XmlAttr* find_attr(int elem, std::string_view name, size_t* index = nullptr) const;
    std::optional<std::string> get(int elem, std::string_view name) const; // entity-decoded

    // Replaces the raw value text. Setting the original text back removes the edit.
    void set_attr_raw(int elem, size_t attr, std::string raw);
    void set(int elem, std::string_view name, std::string_view value); // entity-encodes; appends if missing
    bool modified() const { return !edits_.empty() || !appended_values_.empty(); }
    void revert_all();

    // Element path like "SCENE/SCENE_NODE_LIST/WEATHER_SCENE_NODE/SKY_DOME/SKYGEN"
    std::string path_of(int elem) const;

    static std::string decode_entities(std::string_view raw);
    static std::string encode_entities(std::string_view text, char quote);

private:
    struct Edit { size_t len; std::string text; };
    std::string text_;
    TextEncoding encoding_ = TextEncoding::Utf8;
    std::vector<XmlElement> elems_;
    std::vector<int> roots_;
    std::map<size_t, Edit> edits_;             // keyed by original offset; spans never overlap
    std::map<std::pair<int, size_t>, std::string> appended_values_; // (elem, attr index) -> raw value
};

} // namespace we
