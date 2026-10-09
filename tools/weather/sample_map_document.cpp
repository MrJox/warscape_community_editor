#include "weather/sample_map_document.h"

#include "vfs/vfs.h"

#include <cmath>

namespace wce {

class PlacementCommand : public Command {
public:
    PlacementCommand(SampleMapDocument& d, uint64_t item, Placement before, Placement after, uint64_t key)
        : doc_(d), item_(item), before_(before), after_(after), key_(key) {}
    std::string name() const override { return "Move building " + std::to_string(item_); }
    void apply() override { doc_.placements_[item_] = after_; }
    void revert() override { doc_.placements_[item_] = before_; }
    uint64_t merge_key() const override { return key_; }
    bool merge_from(const Command& next) override {
        const auto* n = dynamic_cast<const PlacementCommand*>(&next);
        if (!n || n->item_ != item_) return false;
        after_ = n->after_;
        return true;
    }
    std::vector<uint64_t> dirty_items() const override { return {item_}; }

private:
    SampleMapDocument& doc_;
    uint64_t item_;
    Placement before_, after_;
    uint64_t key_;
};

SampleMapDocument::SampleMapDocument(IVfs& vfs, uint32_t seed, int count) : seed_(seed), count_(count) {
    RenderScene tmp;
    const LayerId l = tmp.add_layer(id(), "");
    info_ = publish_sample_map(vfs, tmp, l, id(), seed, count);
    for (const Entity& e : tmp.entities()) {
        base_.push_back({e.payload, e.world, e.source.item});
        ++pieces_[e.source.item];
    }
}

std::string SampleMapDocument::title() const { return "Sample map (seed " + std::to_string(seed_) + ")"; }

Placement SampleMapDocument::placement(uint64_t item) const {
    auto it = placements_.find(item);
    return it == placements_.end() ? Placement{} : it->second;
}

float4x4 SampleMapDocument::placement_matrix(uint64_t item) const {
    const Placement p = placement(item);
    if (item == 0 || item > info_.centres.size()) return float4x4{};
    const float3 c = info_.centres[item - 1];
    return translation(p.offset) * translation(c) * rotation_y(p.yaw) * translation(-c);
}

void SampleMapDocument::publish(RenderScene& scene, LayerId layer) {
    scene.clear_layer(layer);
    for (const Base& b : base_) scene.add(layer, placement_matrix(b.item) * b.world, b.payload, SourceRef{id(), b.item});
}

OutlinerNode SampleMapDocument::outline() const {
    OutlinerNode root{title(), {id(), 0}, false, {}};
    root.children.push_back({"Ground", {id(), 0}, true, {}});
    for (size_t i = 1; i <= info_.centres.size(); ++i) {
        auto it = pieces_.find(i);
        root.children.push_back({"Building " + std::to_string(i) + " (" + std::to_string(it == pieces_.end() ? 0 : it->second) + " pieces)", {id(), i}, true, {}});
    }
    return root;
}

std::string SampleMapDocument::describe(const SourceRef& item) const {
    if (item.item == 0) return "Ground";
    auto it = pieces_.find(item.item);
    return "Building " + std::to_string(item.item) + " (" + std::to_string(it == pieces_.end() ? 0 : it->second) + " pieces)";
}

void SampleMapDocument::inspect(const SourceRef& item, Inspector& ins) {
    if (item.item == 0) {
        ins.section("Ground");
        ins.text("model", "terrain", "flat, 2000 m, tiled grass");
        return;
    }
    ins.section("Placement");
    Placement p = placement(item.item);
    float off[3] = {p.offset.x, p.offset.y, p.offset.z};
    // Edits made in the inspector merge per field until the widget is released.
    const uint64_t key = (uint64_t(id()) << 40) ^ (item.item << 8) ^ 0x51;
    if (ins.edit_float3("offset", "offset (m)", off, 0.5f)) {
        Placement n = p;
        n.offset = {off[0], off[1], off[2]};
        history_.push(std::make_unique<PlacementCommand>(*this, item.item, p, n, key));
    }
    if (ins.gesture_ended()) history_.break_merge();
    float yaw_deg = p.yaw * 57.2957795f;
    if (ins.edit_float("yaw", "yaw (deg)", yaw_deg, 0.5f, -3600, 3600)) {
        Placement n = p;
        n.yaw = yaw_deg / 57.2957795f;
        history_.push(std::make_unique<PlacementCommand>(*this, item.item, p, n, key + 1));
    }
    if (ins.gesture_ended()) history_.break_merge();
    ins.section("Source");
    ins.text("pieces", "pieces", std::to_string(pieces_.count(item.item) ? pieces_.at(item.item) : 0));
    if (item.item <= info_.centres.size()) {
        const float3 c = info_.centres[item.item - 1];
        ins.text("grid", "grid centre", std::to_string(int(c.x)) + ", " + std::to_string(int(c.z)));
    }
}

bool SampleMapDocument::item_transform(const SourceRef& item, float4x4& world) const {
    if (item.item == 0 || item.item > info_.centres.size()) return false;
    const Placement p = placement(item.item);
    world = translation(p.offset + info_.centres[item.item - 1]) * rotation_y(p.yaw);
    return true;
}

void SampleMapDocument::transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) {
    for (const SourceRef& r : items) {
        float4x4 before;
        if (r.doc != id() || !item_transform(r, before)) continue;
        const float4x4 now = delta * before;
        const float3 c = info_.centres[r.item - 1];
        Placement n;
        n.offset = {now.m[0][3] - c.x, now.m[1][3] - c.y, now.m[2][3] - c.z};
        n.yaw = std::atan2(now.m[0][2], now.m[0][0]);
        set_placement(r.item, n, gesture);
    }
}

void SampleMapDocument::set_placement(uint64_t item, const Placement& p, uint64_t gesture) {
    const Placement before = placement(item);
    if (before == p) return;
    history_.push(std::make_unique<PlacementCommand>(*this, item, before, p, gesture));
}

} // namespace wce
