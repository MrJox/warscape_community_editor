#pragma once
// Owns the open documents, gives each a layer in the shared RenderScene, keeps the scene in step with the documents' edits, and routes
// save/close. Many documents are open at once; one is active (what the history panel and the default inspector target follow).
#include "editor/document.h"

#include <memory>
#include <string>
#include <vector>

namespace wce {

class DocumentManager {
public:
    explicit DocumentManager(RenderScene& scene) : scene_(scene) {}

    // Takes the document, creates its layer, publishes it, makes it active.
    DocumentId add(std::unique_ptr<Document> doc, const std::string& tool_id);
    bool close(DocumentId id);                       // drops the document and its layer (the caller asked about unsaved changes)
    Document* find(DocumentId id);
    const Document* find(DocumentId id) const;
    Document* active() { return find(active_); }
    DocumentId active_id() const { return active_; }
    void set_active(DocumentId id) { if (find(id)) active_ = id; }
    LayerId layer_of(DocumentId id) const;
    std::string tool_of(DocumentId id) const;
    const std::vector<DocumentId>& order() const { return order_; }   // opening order

    // Re-publishes every document whose command stack moved since its last publish. Call once per frame, before drawing.
    void update();
    bool any_modified() const;
    std::vector<DocumentId> unsaved() const;         // the modified documents, in opening order
    // Saves one document in place and marks it saved. False with the reason in *message.
    bool save_one(DocumentId id, std::string* message);
    // Saves every modified document that can be saved. Messages explain each failure; true when nothing failed.
    bool save_all(std::vector<std::string>* messages);
    // Layer visibility per document (the outliner's eye icon).
    void set_visible(DocumentId id, bool visible);
    bool visible(DocumentId id) const;
    // A document that brings its own ground (a tile) is open: stand-in scenes (the weather tool's sample map) are hidden meanwhile.
    bool shows_scene() const;

private:
    struct Entry {
        std::unique_ptr<Document> doc;
        std::string tool;
        LayerId layer = 0;
        uint64_t published_version = 0;
    };
    void apply_stand_ins();
    Entry* entry(DocumentId id);
    const Entry* entry(DocumentId id) const;

    RenderScene& scene_;
    std::vector<Entry> entries_;
    std::vector<DocumentId> order_;
    DocumentId active_ = 0;
    bool real_scene_ = false;                        // shows_scene() as of the last apply_stand_ins()
};

} // namespace wce
