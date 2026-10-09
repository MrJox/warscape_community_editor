#pragma once
// rigid_model_v2 (version 6, Attila) reader. See file_format_specs/rigid_model_v2_spec.md.
//
// Verified layout of the MESH_HEADER_V5 material block (2026-10-06, all vanilla building models):
// the counts are at +0x2C8 and the dynamic arrays start at +0x35C (the spec's "660 bytes" total is wrong;
// its field offsets are right). 80 (common header) + material block == VertexOffset.
#include <array>
#include <cstdint>
#include <string>
#include <vector>

namespace we {

enum RigidShader : uint16_t {
    RS_STANDARD_TILED_DIRTMAP_V5 = 63,
    RS_STANDARD_AMBIENT_V5 = 64,
    RS_STANDARD_V5 = 68,
    RS_TERRAIN_BLEND_V5 = 86,
};

enum TextureParam : int32_t {
    TP_DIFFUSE = 0, TP_NORMAL = 1, TP_DETAIL_NORMAL = 2, TP_FACTION_MASK = 3, TP_MATERIAL_MAP = 4,
    TP_AO_UV2 = 5, TP_DISPLACEMENT = 6, TP_DIRTMAP_UV2 = 7, TP_ALPHA_MASK = 8, TP_DISSOLVE = 9,
    TP_SKIN_MASK = 10, TP_SPECULAR = 11, TP_GLOSS = 12, TP_DECAL_DIRTMAP = 13, TP_DECAL_DIRTMASK = 14,
    TP_DECAL_MASK = 15, TP_DIFFUSE_BURN = 16, TP_DIFFUSE_DAMAGE = 17,
};

struct Rmv2Mesh {
    uint16_t shader = 0;
    uint16_t render_flags = 0;
    uint16_t vertex_format = 0;
    std::string name, texture_dir, lighting_constants;
    float aabb[6] = {};
    std::vector<std::pair<int32_t, std::string>> textures; // (TextureParam, path)
    std::vector<std::pair<int32_t, int32_t>> int_params;   // INT_PARAM_ALPHA_MODE = 0
    std::vector<std::pair<int32_t, float>> float_params;
    std::vector<std::pair<int32_t, std::array<float, 4>>> vec4_params;
    uint32_t vertex_count = 0, vertex_stride = 0, index_count = 0;
    std::vector<uint8_t> vertices; // raw, vertex_stride bytes each
    std::vector<uint16_t> indices;

    const std::string* texture(int32_t param) const;
    int alpha_mode() const;
};

struct Rmv2Lod {
    float distance = 0;
    std::vector<Rmv2Mesh> meshes;
};

struct Rmv2Model {
    std::string skeleton;
    std::vector<Rmv2Lod> lods;
};

// Reads the header and all meshes of the requested LODs (-1 = all). Meshes whose material
// block cannot be parsed keep shader/counts but no geometry (vertex_stride == 0).
Rmv2Model parse_rmv2(const std::vector<uint8_t>& bytes, int only_lod = -1); // throws std::runtime_error

uint32_t rmv2_vertex_stride(uint16_t vertex_format); // 0 if unknown

} // namespace we
