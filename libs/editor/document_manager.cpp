#include "editor/document_manager.h"

#include "foundation/log.h"

#include <algorithm>

namespace wce {

namespace {
DocumentId g_next_document = 1;
}

Document::Document() : id_(g_next_document++) {}

DocumentManager::Entry* DocumentManager::entry(DocumentId id) {
    for (Entry& e : entries_)
        if (e.doc->id() == id) return &e;
    return nullptr;
}
const DocumentManager::Entry* DocumentManager::entry(DocumentId id) const { return const_cast<DocumentManager*>(this)->entry(id); }

DocumentId DocumentManager::add(std::unique_ptr<Document> doc, const std::string& tool_id) {
    Entry e;
    e.layer = scene_.add_layer(doc->id(), doc->title());
    doc->publish(scene_, e.layer);
    e.published_version = doc->history().version();
    e.tool = tool_id;
    const DocumentId id = doc->id();
    e.doc = std::move(doc);
    entries_.push_back(std::move(e));
    order_.push_back(id);
    active_ = id;
    return id;
}

bool DocumentManager::close(DocumentId id) {
    Entry* e = entry(id);
    if (!e) return false;
    scene_.remove_layer(e->layer);
    entries_.erase(entries_.begin() + (e - entries_.data()));
    order_.erase(std::remove(order_.begin(), order_.end(), id), order_.end());
    if (active_ == id) active_ = order_.empty() ? 0 : order_.back();
    return true;
}

Document* DocumentManager::find(DocumentId id) {
    Entry* e = entry(id);
    return e ? e->doc.get() : nullptr;
}
const Document* DocumentManager::find(DocumentId id) const { return const_cast<DocumentManager*>(this)->find(id); }

LayerId DocumentManager::layer_of(DocumentId id) const {
    const Entry* e = entry(id);
    return e ? e->layer : 0;
}

std::string DocumentManager::tool_of(DocumentId id) const {
    const Entry* e = entry(id);
    return e ? e->tool : std::string();
}

void DocumentManager::update() {
    for (Entry& e : entries_) {
        const uint64_t v = e.doc->history().version();
        if (v == e.published_version) continue;
        e.doc->republish(scene_, e.layer, e.doc->history().last_dirty());
        e.published_version = v;
    }
}

bool DocumentManager::any_modified() const {
    for (const Entry& e : entries_)
        if (e.doc->modified()) return true;
    return false;
}

bool DocumentManager::save_all(std::vector<std::string>* messages) {
    bool ok = true;
    for (Entry& e : entries_) {
        if (!e.doc->modified() || !e.doc->can_save()) continue;
        std::string msg;
        if (e.doc->save({}, &msg)) {
            e.doc->history().mark_saved();
            if (messages) messages->push_back("saved " + e.doc->title());
        } else {
            ok = false;
            if (messages) messages->push_back("cannot save " + e.doc->title() + ": " + msg);
        }
    }
    return ok;
}

void DocumentManager::set_visible(DocumentId id, bool visible) {
    if (const Entry* e = entry(id)) scene_.set_layer_visible(e->layer, visible);
}

} // namespace wce
