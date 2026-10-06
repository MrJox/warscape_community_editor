#include "rmv2.h"

#include <cstring>
#include <stdexcept>

namespace we {

namespace {

struct In {
    const std::vector<uint8_t>& b;
    void need(size_t off, size_t n) const {
        if (off + n > b.size()) throw std::runtime_error("rmv2: truncated at " + std::to_string(off));
    }
    template <class T> T get(size_t off) const {
        need(off, sizeof(T));
        T v;
        std::memcpy(&v, &b[off], sizeof(T));
        return v;
    }
    std::string str(size_t off, size_t n) const {
        need(off, n);
        const char* p = reinterpret_cast<const char*>(&b[off]);
        return std::string(p, strnlen(p, n));
    }
};

constexpr size_t kMaterialCounts = 0x2C8;
constexpr size_t kMaterialArrays = 0x35C;

} // namespace

uint32_t rmv2_vertex_stride(uint16_t f) {
    switch (f) {
    case 0: return 32; case 1: return 24; case 2: return 28; case 3: return 28; case 4: return 32;
    case 5: return 32; case 6: return 56; case 7: return 12; case 8: return 12; case 9: return 28;
    case 10: return 32; case 11: return 44; case 12: return 16;
    }
    return 0;
}

const std::string* Rmv2Mesh::texture(int32_t param) const {
    for (auto& [p, path] : textures)
        if (p == param) return &path;
    return nullptr;
}

int Rmv2Mesh::alpha_mode() const {
    for (auto& [id, v] : int_params)
        if (id == 0) return v;
    return 0;
}

Rmv2Model parse_rmv2(const std::vector<uint8_t>& bytes, int only_lod) {
    In in{bytes};
    if (in.str(0, 4) != "RMV2") throw std::runtime_error("rmv2: bad signature");
    if (in.get<uint32_t>(4) != 6) throw std::runtime_error("rmv2: version " + std::to_string(in.get<uint32_t>(4)) + " (expected 6)");
    Rmv2Model m;
    uint32_t lod_count = in.get<uint32_t>(8);
    if (lod_count > 64) throw std::runtime_error("rmv2: bad LOD count");
    m.skeleton = in.str(12, 128);
    m.lods.resize(lod_count);
    for (uint32_t l = 0; l < lod_count; ++l) {
        size_t h = 140 + l * 20;
        uint32_t mesh_count = in.get<uint32_t>(h);
        size_t off = in.get<uint32_t>(h + 12);
        m.lods[l].distance = in.get<float>(h + 16);
        if (only_lod >= 0 && int(l) != only_lod) continue;
        for (uint32_t k = 0; k < mesh_count; ++k) {
            Rmv2Mesh me;
            me.shader = in.get<uint16_t>(off);
            me.render_flags = in.get<uint16_t>(off + 2);
            uint32_t section = in.get<uint32_t>(off + 4);
            uint32_t voff = in.get<uint32_t>(off + 8);
            me.vertex_count = in.get<uint32_t>(off + 12);
            uint32_t ioff = in.get<uint32_t>(off + 16);
            me.index_count = in.get<uint32_t>(off + 20);
            for (int i = 0; i < 6; ++i) me.aabb[i] = in.get<float>(off + 24 + 4 * i);
            me.lighting_constants = in.str(off + 48, 12);
            if (section == 0) throw std::runtime_error("rmv2: zero mesh section size");

            // MESH_HEADER_V5 material block: every shader whose block ends exactly at the vertex data.
            size_t mat = off + 80;
            bool v5 = voff >= 80 + kMaterialArrays;
            if (v5) {
                me.vertex_format = in.get<uint16_t>(mat);
                me.name = in.str(mat + 2, 32);
                me.texture_dir = in.str(mat + 0x22, 256);
                uint32_t n_attach = in.get<uint32_t>(mat + kMaterialCounts);
                uint32_t n_tex = in.get<uint32_t>(mat + kMaterialCounts + 4);
                uint32_t n_str = in.get<uint32_t>(mat + kMaterialCounts + 8);
                uint32_t n_float = in.get<uint32_t>(mat + kMaterialCounts + 12);
                uint32_t n_int = in.get<uint32_t>(mat + kMaterialCounts + 16);
                uint32_t n_vec4 = in.get<uint32_t>(mat + kMaterialCounts + 20);
                size_t p = mat + kMaterialArrays + size_t(n_attach) * 84;
                for (uint32_t i = 0; i < n_tex; ++i, p += 260) me.textures.emplace_back(in.get<int32_t>(p), in.str(p + 4, 256));
                for (uint32_t i = 0; i < n_str; ++i) {
                    p += 4;
                    uint32_t len = in.get<uint16_t>(p); // length-prefixed (u16) string
                    p += 2 + len;
                }
                for (uint32_t i = 0; i < n_float; ++i, p += 8) me.float_params.emplace_back(in.get<int32_t>(p), in.get<float>(p + 4));
                for (uint32_t i = 0; i < n_int; ++i, p += 8) me.int_params.emplace_back(in.get<int32_t>(p), in.get<int32_t>(p + 4));
                for (uint32_t i = 0; i < n_vec4; ++i, p += 20) {
                    std::array<float, 4> v;
                    for (int c = 0; c < 4; ++c) v[c] = in.get<float>(p + 4 + 4 * c);
                    me.vec4_params.emplace_back(in.get<int32_t>(p), v);
                }
                if (p != off + voff) v5 = false; // not a MESH_HEADER_V5 block after all
            }
            if (v5) me.vertex_stride = rmv2_vertex_stride(me.vertex_format);
            if (me.vertex_stride && me.vertex_count) {
                size_t vbytes = size_t(me.vertex_count) * me.vertex_stride;
                in.need(off + voff, vbytes);
                me.vertices.assign(bytes.begin() + off + voff, bytes.begin() + off + voff + vbytes);
                in.need(off + ioff, size_t(me.index_count) * 2);
                me.indices.resize(me.index_count);
                std::memcpy(me.indices.data(), &bytes[off + ioff], size_t(me.index_count) * 2);
            } else {
                me.vertex_stride = 0;
            }
            m.lods[l].meshes.push_back(std::move(me));
            off += section;
        }
    }
    return m;
}

} // namespace we
