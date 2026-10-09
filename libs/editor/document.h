#pragma once
// A Document is one thing a tool edits (an environment file, a tile, a sample map). It owns the lossless model of that thing, accepts
// edits as Commands on its own stack, and publishes what it shows into the shared RenderScene (one layer per document). The editor core
// (outliner, inspector, gizmo, save) only talks to this interface; what a click on item N means is the document's business.
#include "editor/command.h"
#include "editor/inspector.h"
#include "scene/render_scene.h"

#include <filesystem>
#include <string>
#include <vector>

namespace wce {

struct SaveTarget {
    std::filesystem::path path;      // empty: back to where the document came from
};

struct OutlinerNode {
    std::string label;
    SourceRef ref;                   // what a click selects
    bool selectable = true;
    std::vector<OutlinerNode> children;
};

class Document {
public:
    Document();
    virtual ~Document() = default;
    Document(const Document&) = delete;
    Document& operator=(const Document&) = delete;

    DocumentId id() const { return id_; }
    virtual std::string title() const = 0;
    virtual std::string kind() const = 0;                // "sample_map", "environment", ...
    virtual bool modified() const { return history_.modified(); }
    virtual bool can_save() const { return false; }
    // Writes the document as minimal edits of its source bytes. Returns false with the reason in *message.
    virtual bool save(const SaveTarget& target, std::string* message) {
        (void)target;
        if (message) *message = title() + " cannot be saved";
        return false;
    }

    // Puts everything the document shows into `layer` (called on open and when the whole document changed), or only what the last command touched.
    virtual void publish(RenderScene& scene, LayerId layer) = 0;
    virtual void republish(RenderScene& scene, LayerId layer, const CommandStack::Dirty& dirty) { (void)dirty; publish(scene, layer); }

    virtual OutlinerNode outline() const { return {title(), {id_, 0}, false, {}}; }
    virtual void inspect(const SourceRef& item, Inspector& inspector) { (void)item; (void)inspector; }
    virtual std::string describe(const SourceRef& item) const { (void)item; return {}; }
    // Identifies the file a document came from (a normalized in-pack path); an open request for the same key activates the open document instead.
    virtual std::string source_key() const { return {}; }

    // Gizmo support: the world transform of an item (the gizmo's pivot), and moving items by a world-space delta. `gesture` is
    // constant during one drag so that the commands merge into one undo entry.
    virtual bool item_transform(const SourceRef& item, float4x4& world) const { (void)item; (void)world; return false; }
    virtual void transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) { (void)items; (void)delta; (void)gesture; }

    CommandStack& history() { return history_; }
    const CommandStack& history() const { return history_; }

protected:
    CommandStack history_;

private:
    DocumentId id_;
};

} // namespace wce
