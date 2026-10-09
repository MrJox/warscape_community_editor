#pragma once
// The selection: a set of SourceRefs (document + item), shared by the viewport, outliner, inspector and gizmo. It names document
// elements, not scene entities; the scene entities that show them come from RenderScene::entities_of.
#include "scene/render_scene.h"

#include <algorithm>
#include <vector>

namespace wce {

class Selection {
public:
    const std::vector<SourceRef>& items() const { return items_; }
    bool empty() const { return items_.empty(); }
    uint64_t revision() const { return revision_; }

    bool contains(const SourceRef& r) const { return std::find_if(items_.begin(), items_.end(), [&](const SourceRef& s) { return same(s, r); }) != items_.end(); }
    void clear() { if (!items_.empty()) { items_.clear(); ++revision_; } }
    void set(const SourceRef& r) {
        if (items_.size() == 1 && same(items_[0], r)) return;
        items_.assign(1, r);
        ++revision_;
    }
    void add(const SourceRef& r) { if (!contains(r)) { items_.push_back(r); ++revision_; } }
    void toggle(const SourceRef& r) {
        auto it = std::find_if(items_.begin(), items_.end(), [&](const SourceRef& s) { return same(s, r); });
        if (it == items_.end()) items_.push_back(r);
        else items_.erase(it);
        ++revision_;
    }
    void remove_document(DocumentId doc) {
        const size_t n = items_.size();
        items_.erase(std::remove_if(items_.begin(), items_.end(), [&](const SourceRef& s) { return s.doc == doc; }), items_.end());
        if (items_.size() != n) ++revision_;
    }
    // The scene entities that show the selected items.
    std::vector<EntityId> entities(const RenderScene& scene) const {
        std::vector<EntityId> out;
        for (const SourceRef& r : items_)
            for (EntityId id : scene.entities_of(r)) out.push_back(id);
        return out;
    }

private:
    static bool same(const SourceRef& a, const SourceRef& b) { return a.doc == b.doc && a.item == b.item; }

    std::vector<SourceRef> items_;
    uint64_t revision_ = 0;
};

} // namespace wce
