#pragma once
// Lossless XML document.
//
// The original text is never re-serialised. Elements and attributes only record byte spans into it;
// edits are span replacements applied on save. An unedited document saves byte-identically, and an
// edited one differs only inside the replaced spans. Multiple top-level elements are allowed
// (vfx files have <shader_library/>, <vfx> and <vfx_references> side by side).
#include "foundation/text_encoding.h"

#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace wce {

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
    bool modified() const { return !edits_.empty() || !appended_values_.empty() || structural_ > 0; }
    void revert_all(); // attribute edits only; structural edits need parse(origin_bytes())

    // Structural edits. Each returns a new document: pending attribute edits are baked into its text (they stop
    // showing as modified) and the elements after the edit point get new indices. The text outside the edited
    // span stays byte-identical, so the rest of the file still saves unchanged. structural_edits() counts the
    // operations since parse(); origin_bytes() is the file as first loaded.
    XmlDoc removed_element(int e) const;
    XmlDoc duplicated_element(int e, int* new_index = nullptr) const; // the copy goes right after e
    XmlDoc moved_element(int e, int dir) const;                        // dir -1 / +1 swaps with a sibling
    XmlDoc with_child(int parent, std::string_view name, int* new_index = nullptr) const; // new empty last child
    XmlDoc with_attr_added(int elem, std::string_view name, std::string_view value) const;
    XmlDoc without_attr(int elem, size_t attr) const;
    int structural_edits() const { return structural_; }
    const std::vector<uint8_t>& origin_bytes() const { return origin_; }
    int subtree_size(int e) const; // the element and all its descendants

    // Element path like "SCENE/SCENE_NODE_LIST/WEATHER_SCENE_NODE/SKY_DOME/SKYGEN"
    std::string path_of(int elem) const;

    static std::string decode_entities(std::string_view raw);
    static std::string encode_entities(std::string_view text, char quote);

private:
    struct Edit { size_t len; std::string text; };
    std::string baked_text() const;          // utf-8 text with all attribute edits applied
    XmlDoc rebuilt(std::string utf8) const;  // parse(utf8), carrying origin_ and counting one more structural edit
    XmlDoc baked() const;                    // parse(save()) with origin_ and the counter carried over
    std::string text_;
    std::vector<uint8_t> origin_;
    int structural_ = 0;
    TextEncoding encoding_ = TextEncoding::Utf8;
    std::vector<XmlElement> elems_;
    std::vector<int> roots_;
    std::map<size_t, Edit> edits_;             // keyed by original offset; spans never overlap
    std::map<std::pair<int, size_t>, std::string> appended_values_; // (elem, attr index) -> raw value
};

} // namespace wce
