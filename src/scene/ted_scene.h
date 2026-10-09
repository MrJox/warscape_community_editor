#pragma once
// tile.ted scene graph on top of the lossless XmlDoc.
//
// The file is the source of truth of a TDD tile: layers (LayerInstance, UserInstanceGroup, INSTANCE_GROUP) holding
// instances (BUILDING_INSTANCE, VEGETATION_INSTANCE, ProjectedDecalInstance, VFXInstance, POINT_LIGHT_INSTANCE, polygons,
// AI hints, siege AI, deployment, capture locations, rivers ...). Every instance carries the base attributes
// flags, lf_offset, name, position, orientation, surface_mode; the rest is type specific (docs/formats/tile_ted.md).
//
// Edits are attribute edits on the XmlDoc, so everything outside the edited values stays byte-identical, and an edit
// that does not change a value (as float32) leaves the text alone. Numbers are written the way TEd writes them
// (format_number): "%.6f" rounded half away from zero, trailing zeros removed.
#include "formats/xml_doc.h"

#include <array>
#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace te {

using Vec3 = std::array<double, 3>;

std::string format_number(double v);                    // 1.000000 -> "1", -0.000000 -> "-0", 12.5 -> "12.5"
std::string format_general(double v);                   // "%g" (6 significant digits), used for <point position="x,y,z">
std::optional<Vec3> parse_vec3(const std::string& text, const char* prefix); // "vector(1,2,3)" / "euler(..)" / "x,y,z" with prefix ""

class TedScene {
public:
    static TedScene parse(const std::vector<uint8_t>& bytes);   // throws std::runtime_error
    std::vector<uint8_t> save() const { return doc_.save(); }

    int project_version() const;                                // <project version="17">
    int scene_graph() const;                                    // element index of SCENE_GRAPH, -1 if absent
    const we::XmlDoc& doc() const { return doc_; }

    // Instances below `elem` without the `children` wrappers: direct instance children.
    std::vector<int> children(int elem) const;
    std::string type(int elem) const { return doc_.elements()[elem].name; }
    bool is_layer(int elem) const;                              // LayerInstance, UserInstanceGroup, INSTANCE_GROUP, SCENE_GRAPH
    std::string name(int elem) const { return doc_.get(elem, "name").value_or(""); }
    std::optional<Vec3> position(int elem) const;
    std::optional<Vec3> euler(int elem) const;                  // orientation="euler(x,y,z)" (radians)
    std::optional<Vec3> scale(int elem) const;                  // scale="vector(..)" or a plain number (uniform)
    bool visible(int elem) const;                               // OIF_VISIBLE in flags
    bool active(int elem) const;                                // OIF_ACTIVE in flags

    // Attribute edits. A value equal (as float32) to the current one is a no-op.
    void set_position(int elem, const Vec3& v) { set_vector(elem, "position", "vector", v); }
    void set_euler(int elem, const Vec3& v) { set_vector(elem, "orientation", "euler", v); }
    void translate(int elem, const Vec3& delta);
    void set_attr(int elem, const char* name, const std::string& value) { doc_.set(elem, name, value); }
    void set_flag(int elem, const char* flag, bool on);        // flags=" OIF_VISIBLE|OIF_ACTIVE"
    bool modified() const { return doc_.modified(); }
    void revert_all() { doc_.revert_all(); }

    // Structural edits. Element indices after the edit point shift: re-resolve any stored index.
    int duplicate(int elem);                                   // copy placed right after `elem`; returns the copy's index
    void remove(int elem);
    using Snapshot = std::shared_ptr<const we::XmlDoc>;        // whole-document snapshot for undo of structural edits
    Snapshot snapshot() const { return std::make_shared<we::XmlDoc>(doc_); }
    void restore(const Snapshot& s) { doc_ = *s; }

    // All instances (depth first) of a given element name, e.g. "BUILDING_INSTANCE".
    std::vector<int> find_all(const std::string& type) const;
    // Layer path "Logic/Go Zones" of an element.
    std::string layer_path(int elem) const;

private:
    void set_vector(int elem, const char* attr, const char* prefix, const Vec3& v);
    we::XmlDoc doc_;
};

} // namespace te
