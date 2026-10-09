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
    const bool stand_in = e.doc->stand_in();
    const LayerId layer = e.layer;
    entries_.push_back(std::move(e));
    order_.push_back(id);
    active_ = id;
    if (stand_in && real_scene_) scene_.set_layer_visible(layer, false);   // added while a tile is open
    apply_stand_ins();
    return id;
}

bool DocumentManager::close(DocumentId id) {
    Entry* e = entry(id);
    if (!e) return false;
    scene_.remove_layer(e->layer);
    entries_.erase(entries_.begin() + (e - entries_.data()));
    order_.erase(std::remove(order_.begin(), order_.end(), id), order_.end());
    if (active_ == id) active_ = order_.empty() ? 0 : order_.back();
    apply_stand_ins();
    return true;
}

bool DocumentManager::shows_scene() const {
    for (const Entry& e : entries_)
        if (e.doc->shows_scene()) return true;
    return false;
}

// Stand-in scenes follow the first and last tile: hidden when one opens, shown when none is left.
void DocumentManager::apply_stand_ins() {
    const bool real = shows_scene();
    if (real == real_scene_) return;
    real_scene_ = real;
    for (const Entry& e : entries_)
        if (e.doc->stand_in()) scene_.set_layer_visible(e.layer, !real);
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
        e.doc->republish(scene_, e.layer, e.doc->history().take_dirty());
        e.published_version = v;
    }
}

bool DocumentManager::any_modified() const {
    for (const Entry& e : entries_)
        if (e.doc->modified()) return true;
    return false;
}

std::vector<DocumentId> DocumentManager::unsaved() const {
    std::vector<DocumentId> out;
    for (DocumentId id : order_)
        if (const Entry* e = entry(id); e && e->doc->modified()) out.push_back(id);
    return out;
}

bool DocumentManager::save_one(DocumentId id, std::string* message) {
    Entry* e = entry(id);
    if (!e) {
        if (message) *message = "no such document";
        return false;
    }
    if (!e->doc->can_save()) {
        if (message) *message = e->doc->title() + " cannot be saved";
        return false;
    }
    if (!e->doc->save({}, message)) return false;
    e->doc->history().mark_saved();
    if (message) *message = "saved " + e->doc->title();
    return true;
}

bool DocumentManager::save_all(std::vector<std::string>* messages) {
    bool ok = true;
    for (DocumentId id : order_) {
        const Entry* e = entry(id);
        if (!e || !e->doc->modified() || !e->doc->can_save()) continue;
        std::string msg;
        if (save_one(id, &msg)) {
            if (messages) messages->push_back(msg);
        } else {
            ok = false;
            if (messages) messages->push_back("cannot save " + e->doc->title() + ": " + msg);
        }
    }
    return ok;
}

void DocumentManager::set_visible(DocumentId id, bool visible) {
    if (const Entry* e = entry(id)) scene_.set_layer_visible(e->layer, visible);
}

bool DocumentManager::visible(DocumentId id) const {
    const Entry* e = entry(id);
    const Layer* l = e ? scene_.layer(e->layer) : nullptr;
    return l ? l->visible : true;
}

} // namespace wce
