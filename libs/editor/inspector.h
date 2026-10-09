#pragma once
// The inspector framework. A document describes the properties of one of its items by calling an Inspector; the ImGui implementation
// draws widgets, tests use a recording one. Every property has a stable PropertyPath {document, item, field}: edits, undo entries and the
// future Cinematic editor's animation tracks all name properties this way.
#include "scene/render_scene.h"

#include <string>
#include <vector>

namespace wce {

struct PropertyPath {
    DocumentId doc = 0;
    uint64_t item = 0;
    std::string field;

    std::string to_string() const { return std::to_string(doc) + "/" + std::to_string(item) + "/" + field; }
    bool operator==(const PropertyPath& o) const { return doc == o.doc && item == o.item && field == o.field; }
};

class Inspector {
public:
    virtual ~Inspector() = default;

    void begin(DocumentId doc, uint64_t item) { doc_ = doc; item_ = item; }
    PropertyPath path(const std::string& field) const { return {doc_, item_, field}; }

    virtual void section(const std::string& title) = 0;
    virtual void text(const std::string& field, const std::string& label, const std::string& value) = 0;   // read-only
    // The edit_* calls return true on every frame the value changed (a drag produces many); gesture_ended() is true once, on the frame
    // the drag or typing ends, which is where merged commands are closed (CommandStack::break_merge).
    virtual bool edit_float(const std::string& field, const std::string& label, float& v, float speed = 0.1f, float lo = -1e9f, float hi = 1e9f) = 0;
    virtual bool edit_float3(const std::string& field, const std::string& label, float v[3], float speed = 0.1f) = 0;
    virtual bool edit_bool(const std::string& field, const std::string& label, bool& v) = 0;
    virtual bool edit_string(const std::string& field, const std::string& label, std::string& v) = 0;
    virtual bool edit_choice(const std::string& field, const std::string& label, int& index, const std::vector<std::string>& choices) = 0;
    virtual bool gesture_ended() = 0;

protected:
    DocumentId doc_ = 0;
    uint64_t item_ = 0;
};

} // namespace wce
