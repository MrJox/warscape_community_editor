#pragma once
// The weather tool's sandbox document until the environment documents arrive (Phase 5): the sample map with houses that can be selected,
// moved and rotated like any document's items. Item 0 is the ground, items 1..N the houses. Placements are edits on top of the generated
// layout; they go through the command stack (undo, redo, merged drags) but are not saved, the layout is generated from the seed.
#include "editor/document.h"
#include "weather/sample_map.h"

#include <map>
#include <vector>

namespace wce {

class IVfs;

struct Placement {
    float3 offset{0, 0, 0};      // metres, world space
    float yaw = 0;               // radians about the house's centre
    bool operator==(const Placement& o) const { return offset.x == o.offset.x && offset.y == o.offset.y && offset.z == o.offset.z && yaw == o.yaw; }
};

class SampleMapDocument : public Document {
public:
    SampleMapDocument(IVfs& vfs, uint32_t seed, int count);

    std::string title() const override;
    std::string kind() const override { return "sample_map"; }
    void publish(RenderScene& scene, LayerId layer) override;
    OutlinerNode outline() const override;
    void inspect(const SourceRef& item, Inspector& inspector) override;
    std::string describe(const SourceRef& item) const override;
    bool item_transform(const SourceRef& item, float4x4& world) const override;
    void transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) override;

    const SampleMapResult& info() const { return info_; }
    Placement placement(uint64_t item) const;
    void set_placement(uint64_t item, const Placement& p, uint64_t gesture);   // one undo entry (merged per gesture)

private:
    friend class PlacementCommand;
    struct Base {
        Payload payload;
        float4x4 world;
        uint64_t item = 0;
    };
    float4x4 placement_matrix(uint64_t item) const;   // what is applied to the generated world transform of the item's entities

    uint32_t seed_;
    int count_;
    SampleMapResult info_;
    std::vector<Base> base_;
    std::map<uint64_t, Placement> placements_;
    std::map<uint64_t, int> pieces_;
};

} // namespace wce
