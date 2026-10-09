#pragma once
// Sample map: randomly picked building models on a flat ground, so that environments can be judged on something with shape.
// This is the weather tool's default view (the former TestScene); it publishes into the shared RenderScene like any document.
// A building is a folder of pieces; "<name>_pieceNN_destruct01" is each piece's intact state. Piece placement nodes in
// <name>_tech.cs2.parsed are identity in every file checked, so the pieces are drawn at the origin of the building (docs/re/renderer.md).
#include "scene/render_scene.h"

#include <string>
#include <vector>

namespace wce {

class IVfs;

struct SampleMapResult {
    int buildings = 0;
    float3 bounds_min, bounds_max;          // footprint of the grid, tallest building as y
    std::vector<std::string> errors;
};

// Fills `layer` (cleared first) with the ground and up to `count` houses chosen by `seed`; returns what it placed.
SampleMapResult publish_sample_map(IVfs& vfs, RenderScene& scene, LayerId layer, DocumentId doc, uint32_t seed, int count,
                                   const std::string& prefix = "rigidmodels/buildings/");

} // namespace wce
