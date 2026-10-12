// The game's terrain colour pipeline (docs/re/terrain.md "The tool's terrain pass"): the compiled shaders of rigidterrain.hlsl, fed the tile's maps.
//
//   1. surface    vs30_geom_new + ps30_geom   the terrain grid, displaced by the height texture: depth z/w into `terrain_zw_` (+ hardware depth, stencil 2)
//                 vs30_geom_new + ps_depth    the same surface's view-space depth into `terrain_viewz_`
//   2. blend      vs30_blend + ps30_blend     one box per terrain tile; for every pixel it covers, the tile's blend / index / alpha / normal maps and the climate's
//                                             diffuse / normal texture arrays make premultiplied colour, normal and light parameters, accumulated into three targets
//   3. resolve    vs30_terrain_to_gbuffer + ps30_terrain_to_gbuffer_ss   fullscreen: the accumulated terrain targets (+ mid-distance detail, far maps, snow) become the
//                                             G-buffer channels 1 .. 4 of the terrain pixels (stencil 2 -> 1, so the lights treat them as geometry)
//
// Objects are drawn before (pass_gbuffer); the terrain only fills the pixels it wins the depth test for. The legacy terrain patches (rigid G-buffer, flat colour)
// remain for picking, shadows and the environment cube, and draw the ground of a scene whose terrain has no material.
#include "foundation/log.h"
#include "foundation/profile.h"
#include "render/renderer.h"
#include "render/shader_lib.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>

using Microsoft::WRL::ComPtr;

namespace wce {

namespace {

const char* kGeomVS = "dx11_sm_4_0_rigidterrain.hlsl_vs_vs30_geom_new";
const char* kGeomPS = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_geom";
const char* kDepthPS = "dx11_sm_4_0_rigidterrain.hlsl_ps_ps_depth";
const char* kBlendVS = "dx11_sm_4_0_rigidterrain.hlsl_vs_vs30_blend";
const char* kBlendPS = "dx11_sm_4_0_rigidterrain.hlsl_ps_ps30_blend";
const char* kResolveVS = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_terrain_to_gbuffer";
const char* kResolvePS = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_terrain_to_gbuffer_ss";

constexpr int kPatchCells = 64;

ComPtr<ID3D11Buffer> make_buffer(ID3D11Device* dev, const void* data, size_t bytes, UINT bind, D3D11_USAGE usage = D3D11_USAGE_IMMUTABLE) {
    ComPtr<ID3D11Buffer> b;
    D3D11_BUFFER_DESC bd{UINT(bytes), usage, bind, usage == D3D11_USAGE_DYNAMIC ? D3D11_CPU_ACCESS_WRITE : 0u};
    D3D11_SUBRESOURCE_DATA sd{data};
    dev->CreateBuffer(&bd, data ? &sd : nullptr, &b);
    return b;
}

// The frustum test of an axis-aligned box against the view-projection matrix (D3D clip volume: -w <= x, y <= w, 0 <= z <= w).
bool box_in_frustum(const float4x4& vp, const float bmin[3], const float bmax[3]) {
    for (int plane = 0; plane < 6; ++plane) {
        float p[4];
        for (int c = 0; c < 4; ++c) {
            const float w = vp.m[3][c];
            switch (plane) {
            case 0: p[c] = w + vp.m[0][c]; break;
            case 1: p[c] = w - vp.m[0][c]; break;
            case 2: p[c] = w + vp.m[1][c]; break;
            case 3: p[c] = w - vp.m[1][c]; break;
            case 4: p[c] = vp.m[2][c]; break;
            default: p[c] = w - vp.m[2][c]; break;
            }
        }
        const float x = p[0] >= 0 ? bmax[0] : bmin[0], y = p[1] >= 0 ? bmax[1] : bmin[1], z = p[2] >= 0 ? bmax[2] : bmin[2];
        if (p[0] * x + p[1] * y + p[2] * z + p[3] < 0) return false;
    }
    return true;
}

// The entity transforms the tile tool uses: a translation, and a scale of z (the height map's rows are not always as far apart as its columns).
bool is_simple_world(const float4x4& w) {
    for (int r = 0; r < 3; ++r)
        for (int c = 0; c < 3; ++c) {
            if (r == 2 && c == 2) continue;
            if (std::fabs(w.m[r][c] - (r == c ? 1.0f : 0.0f)) > 1e-5f) return false;
        }
    return w.m[2][2] > 1e-4f;
}

} // namespace

void Renderer::make_terrain_states() {
    if (ds_terrain_depth_) return;
    D3D11_DEPTH_STENCIL_DESC d{};
    d.DepthEnable = TRUE;
    d.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
    d.DepthFunc = D3D11_COMPARISON_LESS;
    d.StencilEnable = TRUE;
    d.StencilReadMask = d.StencilWriteMask = 0xff;
    d.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_REPLACE, D3D11_COMPARISON_ALWAYS};
    d.BackFace = d.FrontFace;
    dev_->CreateDepthStencilState(&d, &ds_terrain_depth_);          // the surface wins where it is nearest: stencil 2 (ref)
    D3D11_DEPTH_STENCIL_DESC e{};
    e.DepthEnable = TRUE;
    e.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    e.DepthFunc = D3D11_COMPARISON_EQUAL;
    dev_->CreateDepthStencilState(&e, &ds_terrain_equal_);          // the same surface again (view-space depth): exactly the pixels it won
    D3D11_DEPTH_STENCIL_DESC s{};
    s.DepthEnable = FALSE;
    s.StencilEnable = TRUE;
    s.StencilReadMask = s.StencilWriteMask = 0xff;
    s.FrontFace = {D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_STENCIL_OP_KEEP, D3D11_COMPARISON_EQUAL};
    s.BackFace = s.FrontFace;
    dev_->CreateDepthStencilState(&s, &ds_terrain_stencil_);        // only the terrain pixels (stencil 2)
    s.FrontFace.StencilPassOp = D3D11_STENCIL_OP_DECR;
    s.BackFace = s.FrontFace;
    dev_->CreateDepthStencilState(&s, &ds_terrain_resolve_);        // ... and they become geometry (stencil 1) for the lights
    D3D11_BLEND_DESC b{};
    b.RenderTarget[0].BlendEnable = TRUE;
    b.RenderTarget[0].SrcBlend = b.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    b.RenderTarget[0].DestBlend = b.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    b.RenderTarget[0].BlendOp = b.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    b.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev_->CreateBlendState(&b, &bs_terrain_add_);                   // tiles accumulate (premultiplied by their alpha)
    D3D11_RASTERIZER_DESC r{};
    r.FillMode = D3D11_FILL_SOLID;
    r.CullMode = D3D11_CULL_FRONT;
    r.DepthClipEnable = TRUE;
    dev_->CreateRasterizerState(&r, &rs_front_cull_);               // the blend boxes: their back faces cover the pixels once, also from inside the box

    // the 65 x 65 vertex grid of a patch (x, 0, z in cells); the height comes from the texture
    std::vector<float> grid;
    grid.reserve(size_t(kPatchCells + 1) * (kPatchCells + 1) * 3);
    for (int z = 0; z <= kPatchCells; ++z)
        for (int x = 0; x <= kPatchCells; ++x) {
            grid.push_back(float(x));
            grid.push_back(0);
            grid.push_back(float(z));
        }
    terrain_grid_vb_ = make_buffer(dev_, grid.data(), grid.size() * 4, D3D11_BIND_VERTEX_BUFFER);
    // the unit box: corner i = (i & 1, i >> 1 & 1, i >> 2 & 1); triangles wound so that (b - a) x (c - a) points outwards, which is clockwise (front) seen from outside
    float box[8][3];
    for (int i = 0; i < 8; ++i) { box[i][0] = float(i & 1); box[i][1] = float((i >> 1) & 1); box[i][2] = float((i >> 2) & 1); }
    static const uint16_t quads[6][4] = {{0, 2, 3, 1}, {4, 5, 7, 6}, {0, 1, 5, 4}, {2, 6, 7, 3}, {0, 4, 6, 2}, {1, 3, 7, 5}};   // -z, +z, -y, +y, -x, +x
    std::vector<uint16_t> bi;
    for (const auto& q : quads) {
        auto emit = [&](int a, int bq, int c) {
            // orient by the outward direction of the quad's own centre
            float n[3], cen[3] = {0.5f, 0.5f, 0.5f}, mid[3] = {0, 0, 0};
            const float *pa = box[q[a]], *pb = box[q[bq]], *pc = box[q[c]];
            const float u[3] = {pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]}, v[3] = {pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]};
            n[0] = u[1] * v[2] - u[2] * v[1];
            n[1] = u[2] * v[0] - u[0] * v[2];
            n[2] = u[0] * v[1] - u[1] * v[0];
            for (int k = 0; k < 3; ++k) mid[k] = (pa[k] + pb[k] + pc[k]) / 3 - cen[k];
            const float dot = n[0] * mid[0] + n[1] * mid[1] + n[2] * mid[2];
            bi.push_back(q[a]);
            if (dot > 0) { bi.push_back(q[bq]); bi.push_back(q[c]); }
            else { bi.push_back(q[c]); bi.push_back(q[bq]); }
        };
        emit(0, 1, 2);
        emit(0, 2, 3);
    }
    box_vb_ = make_buffer(dev_, box, sizeof box, D3D11_BIND_VERTEX_BUFFER);
    box_ib_ = make_buffer(dev_, bi.data(), bi.size() * 2, D3D11_BIND_INDEX_BUFFER);
    const float tri[6][6] = {{-1, -1, 0, 1, 0, 1}, {-1, 1, 0, 1, 0, 0}, {1, 1, 0, 1, 1, 0}, {-1, -1, 0, 1, 0, 1}, {1, 1, 0, 1, 1, 0}, {1, -1, 0, 1, 1, 1}};   // x y z w u v
    resolve_vb_ = make_buffer(dev_, tri, sizeof tri, D3D11_BIND_VERTEX_BUFFER);
    terrain_inst_vb_ = make_buffer(dev_, nullptr, 64 * 4096, D3D11_BIND_VERTEX_BUFFER, D3D11_USAGE_DYNAMIC);
}

// The patches of the heights, the height texture the game's vertex shader samples, and the patch table for culling. The shader reads the map with its own
// addressing (docs/re/terrain.md): uv = world xz / g_world_dimensions + g_lf_uv_offset, the second axis flipped (row r = size - k), no clamp below sample 1: so the
// texture is square, padded by one sample at the low side, and `origin - step` is the world position of its sample 0.
void Renderer::build_terrain_geo(EntityId id, const Terrain& t) {
    TerrainGeo& g = terrain_geo_[id];
    if (g.version == t.version && g.height_srv) return;
    g = TerrainGeo{};
    g.version = t.version;
    g.nx = t.nx;
    g.nz = t.nz;
    g.step = t.step;
    g.ox = t.origin_x;
    g.oz = t.origin_z;
    const int S = std::max(t.nx, t.nz) + 2;
    g.tex = S;
    std::vector<float> tex(size_t(S) * S, 0.0f);
    auto H = [&](int i, int j) { return t.height_at(std::clamp(i, 0, t.nx - 1), std::clamp(j, 0, t.nz - 1)); };
    for (int r = 0; r < S; ++r)
        for (int c = 0; c < S; ++c) tex[size_t(r) * S + size_t(c)] = H(c - 1, S - r - 1);
    D3D11_TEXTURE2D_DESC td{};
    td.Width = td.Height = UINT(S);
    td.MipLevels = td.ArraySize = 1;
    td.Format = DXGI_FORMAT_R32_FLOAT;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_IMMUTABLE;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA sd{tex.data(), UINT(S * 4), 0};
    ComPtr<ID3D11Texture2D> tx;
    if (FAILED(dev_->CreateTexture2D(&td, &sd, &tx)) || FAILED(dev_->CreateShaderResourceView(tx.Get(), nullptr, &g.height_srv))) {
        g.height_srv.Reset();
        return;
    }
    const int cx = t.nx - 1, cz = t.nz - 1;
    for (int pj = 0; pj * kPatchCells < cz; ++pj)
        for (int pi = 0; pi * kPatchCells < cx; ++pi) {
            TerrainGeo::Patch p;
            p.pi = pi;
            p.pj = pj;
            p.cx = std::min(kPatchCells, cx - pi * kPatchCells);
            p.cz = std::min(kPatchCells, cz - pj * kPatchCells);
            float lo = 1e30f, hi = -1e30f;
            for (int j = pj * kPatchCells; j <= pj * kPatchCells + p.cz; ++j)
                for (int i = pi * kPatchCells; i <= pi * kPatchCells + p.cx; ++i) {
                    lo = std::min(lo, t.height_at(i, j));
                    hi = std::max(hi, t.height_at(i, j));
                }
            p.bmin[0] = g.ox + float(pi * kPatchCells) * g.step;
            p.bmax[0] = p.bmin[0] + float(p.cx) * g.step;
            p.bmin[2] = g.oz + float(pj * kPatchCells) * g.step;
            p.bmax[2] = p.bmin[2] + float(p.cz) * g.step;
            p.bmin[1] = lo - 0.2f;   // the shader moves the surface by up to 9 cm
            p.bmax[1] = hi + 0.2f;
            g.patches.push_back(p);
        }
}

ID3D11Buffer* Renderer::terrain_grid_ib(int cx, int cz) {
    auto key = std::make_pair(cx, cz);
    auto it = terrain_grid_ib_.find(key);
    if (it != terrain_grid_ib_.end()) return it->second.Get();
    std::vector<uint16_t> idx;
    idx.reserve(size_t(cx) * cz * 6);
    const int row = kPatchCells + 1;
    for (int z = 0; z < cz; ++z)
        for (int x = 0; x < cx; ++x) {
            const uint16_t a = uint16_t(z * row + x), b = uint16_t(a + 1), c = uint16_t(a + row), d = uint16_t(c + 1);
            // x right, z up (looking down +y): clockwise = front (the surface faces +y)
            idx.insert(idx.end(), {a, c, b, b, c, d});
        }
    auto buf = make_buffer(dev_, idx.data(), idx.size() * 2, D3D11_BIND_INDEX_BUFFER);
    return (terrain_grid_ib_[key] = buf).Get();
}

// The GPU side of a terrain material: the tile's maps (mipmapped), the climate's texture arrays, the light parameter texture, the detail maps.
void Renderer::build_terrain_material(const TerrainMaterial& m) {
    auto have = terrain_mat_.find(m.id);
    if (have != terrain_mat_.end()) {   // a height stroke only changes the normal map
        if (have->second.normal_id != m.normal_id) {
            Texture t;
            if (m.normal.rgba) t = make_rgba_texture(dev_, m.normal.w, m.normal.h, m.normal.rgba->data(), false);
            if (t.srv) { have->second.normal = std::move(t); have->second.normal_id = m.normal_id; }
        }
        return;
    }
    TerrainMatGpu g;
    g.normal_id = m.normal_id;
    auto img = [&](const TerrainImage& im, const char* what) {
        Texture t;
        if (im.rgba) t = make_rgba_texture(dev_, im.w, im.h, im.rgba->data(), false);
        if (!t.srv) error(std::string("terrain material: ") + what + " map did not upload");
        return t;
    };
    g.blend = img(m.blend, "blend");
    g.index = img(m.index, "index");
    g.normal = img(m.normal, "normal");
    g.alpha = make_solid_texture(dev_, 0xffffffffu);            // alpha 1 over the whole rectangle: the editor shows one tile; the feathered edge is for neighbours
    g.lf_normal = make_solid_texture(dev_, 0x7f007fffu);        // a flat normal (the far-distance normal map is switched off with g_far_tile_distance)
    g.black = make_solid_texture(dev_, 0x00000000u);
    // diffuse / normal texture arrays: slot = 4 * group + variation
    auto load_array = [&](const std::string* paths, bool srgb) {
        std::vector<std::vector<uint8_t>> bytes(16);
        std::vector<const std::vector<uint8_t>*> ptr(16, nullptr);
        for (int i = 0; i < 16; ++i) {
            if (paths[i].empty() || !vfs_.owner(paths[i])) continue;
            try {
                bytes[size_t(i)] = vfs_.read(paths[i]);
                ptr[size_t(i)] = &bytes[size_t(i)];
            } catch (const std::exception& e) {
                error("terrain material: " + paths[i] + ": " + e.what());
            }
        }
        int bad = 0;
        Texture t = load_dds_array(dev_, ptr, srgb, !srgb, &bad);
        if (bad) error("terrain material: " + std::to_string(bad) + " texture(s) of the climate differ in size / format from the others and are drawn neutral");
        return t;
    };
    g.diffuse = load_array(m.diffuse, true);
    g.normals = load_array(m.normal_tex, false);
    if (!g.diffuse.srv || !g.normals.srv) error("terrain material: the texture arrays did not build");
    // s_light_params: 16 texels (smoothness, reflectivity) per slot
    {
        float lp[16 * 2];
        for (int i = 0; i < 16; ++i) { lp[i * 2] = m.smoothness[i]; lp[i * 2 + 1] = m.reflectivity[i]; }
        D3D11_TEXTURE2D_DESC td{};
        td.Width = 16;
        td.Height = 1;
        td.MipLevels = td.ArraySize = 1;
        td.Format = DXGI_FORMAT_R32G32_FLOAT;
        td.SampleDesc.Count = 1;
        td.Usage = D3D11_USAGE_IMMUTABLE;
        td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        D3D11_SUBRESOURCE_DATA sd{lp, 16 * 8, 0};
        ComPtr<ID3D11Texture2D> tx;
        if (SUCCEEDED(dev_->CreateTexture2D(&td, &sd, &tx))) dev_->CreateShaderResourceView(tx.Get(), nullptr, &g.light_params);
    }
    if (terrain_mat_.size() > 4) terrain_mat_.clear();   // a few live at a time
    terrain_mat_[m.id] = std::move(g);
}

// Which terrains the pass takes over this frame: every visible terrain has a material (a mixed scene keeps the legacy ground), not the environment cube.
bool Renderer::prepare_terrain_materials(const RenderScene& scene) {
    terrain_mat_entities_.clear();
    if (env_pass_ || terrain_mat_failed_) return false;   // a shader that did not load: the legacy ground stays
    int total = 0;
    for (const Entity& e : scene.entities()) {
        const Terrain* t = e.get<Terrain>();
        if (!t || !scene.drawable(e)) continue;
        ++total;
        if (!t->material || t->nx < 2 || t->nz < 2 || t->heights.empty() || !is_simple_world(e.world)) return false;
        {   // the shader has one uv offset for both axes (see build_terrain_geo): the padded origin over the padded size must be the same in x and z
            const float kz = e.world.m[2][2], S = float(std::max(t->nx, t->nz) + 2);
            const float offx = (t->origin_x + e.world.m[0][3] - t->step) / (S * t->step), offz = (kz * t->origin_z + e.world.m[2][3] - kz * t->step) / (S * t->step * kz);
            if (std::fabs(offx - offz) > 1e-5f) return false;
        }
        terrain_mat_entities_.push_back(e.id);
    }
    return total > 0 && int(terrain_mat_entities_.size()) == total;
}

void Renderer::pass_terrain_material(const RenderScene& scene) {
    if (terrain_mat_entities_.empty()) return;
    WCE_PROF("render.terrain");
    make_terrain_states();
    const GameShader *geom_vs = shader(kGeomVS), *geom_ps = shader(kGeomPS), *depth_ps = shader(kDepthPS), *blend_vs = shader(kBlendVS), *blend_ps = shader(kBlendPS),
                     *res_vs = shader(kResolveVS), *res_ps = shader(kResolvePS);
    if (!geom_vs || !geom_ps || !depth_ps || !blend_vs || !blend_ps || !res_vs || !res_ps) {
        terrain_mat_failed_ = true;
        return;
    }
    // targets
    if (!terrain_zw_.tex || terrain_rt_w_ != w_ || terrain_rt_h_ != h_) {
        terrain_zw_ = make_target(w_, h_, DXGI_FORMAT_R32_FLOAT);
        terrain_viewz_ = make_target(w_, h_, DXGI_FORMAT_R32_FLOAT);
        for (int i = 0; i < 3; ++i) terrain_acc_[i] = make_target(w_, h_, DXGI_FORMAT_R16G16B16A16_FLOAT);
        terrain_rt_w_ = w_;
        terrain_rt_h_ = h_;
    }
    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    const float clear_zw[4] = {1, 0, 0, 1}, zero[4] = {0, 0, 0, 0};
    ctx_->ClearRenderTargetView(terrain_zw_.rtv.Get(), clear_zw);
    ctx_->ClearRenderTargetView(terrain_viewz_.rtv.Get(), clear_zw);
    for (auto& a : terrain_acc_) ctx_->ClearRenderTargetView(a.rtv.Get(), zero);

    // ---- 1. the surface (per terrain entity: geometry, then the pixel shader's two outputs)
    std::vector<const Entity*> ents;
    for (EntityId id : terrain_mat_entities_) {
        const Entity* e = scene.find(id);
        const Terrain* t = e ? e->get<Terrain>() : nullptr;
        if (!t) continue;
        build_terrain_geo(id, *t);
        build_terrain_material(*t->material);
        TerrainGeo& tg = terrain_geo_[id];
        if (!tg.height_srv) continue;
        tg.kz = e->world.m[2][2];
        tg.tx = e->world.m[0][3];
        tg.ty = e->world.m[1][3];
        tg.tz = e->world.m[2][3];
        ents.push_back(e);
    }
    if (ents.empty()) return;
    const std::vector<D3D11_INPUT_ELEMENT_DESC> geom_el = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                                           {"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                                           {"POSITION", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                                           {"POSITION", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, D3D11_INPUT_PER_INSTANCE_DATA, 1}};
    ID3D11InputLayout* geom_il = nullptr;
    try {
        geom_il = lib_->layout(*geom_vs, "terrain_geom", geom_el);
    } catch (const std::exception& ex) {
        error(ex.what());
        return;
    }
    ctx_->RSSetState(rs_none_.Get());
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->IASetInputLayout(geom_il);
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    const UINT grid_stride = 12, inst_stride = 48, off0 = 0;
    ctx_->IASetVertexBuffers(0, 1, terrain_grid_vb_.GetAddressOf(), &grid_stride, &off0);
    // the visible patches per entity, grouped by size
    struct Batch { ID3D11Buffer* ib; int cx, cz; std::vector<float> rows; UINT count = 0; };
    auto draw_surface = [&](const GameShader& ps, ID3D11RenderTargetView* rtv, ID3D11DepthStencilState* ds, UINT stencil_ref) {
        ctx_->OMSetRenderTargets(1, &rtv, dsv_.Get());
        ctx_->OMSetDepthStencilState(ds, stencil_ref);
        for (const Entity* e : ents) {
            const Terrain& t = *e->get<Terrain>();
            const TerrainGeo& g = terrain_geo_[e->id];
            // vs_height_map_cb: the height texture (scale 1, no offset), its addressing, the world dimensions; river_CB has nothing to do with it
            const float world_x = float(g.tex) * g.step, world_z = world_x * g.kz;
            cbs_.set_f("vs_height_map_cb", "g_height_map_lf_scale", {1.0f});
            cbs_.set_f("vs_height_map_cb", "g_height_map_hf_scale", {1.0f});
            cbs_.set_f("vs_height_map_cb", "g_height_map_vertical_offset", {0.0f});
            cbs_.set_f("vs_height_map_cb", "g_height_map_dimension", {float(g.tex), float(g.tex), 1.0f / float(g.tex), 1.0f / float(g.tex)});
            cbs_.set_f("vs_height_map_cb", "g_world_dimensions", {world_x, world_z});
            cbs_.set_f("vs_height_map_cb", "g_lf_uv_offset", {-(g.ox + g.tx - g.step) / world_x});
            cbs_.upload(dev_, ctx_);
            bind_shader(ctx_, *geom_vs, cbs_);
            bind_shader(ctx_, ps, cbs_);
            bind_srv(ctx_, *geom_vs, "t_low_res_height_map", g.height_srv.Get());
            std::vector<Batch> batches;
            for (const TerrainGeo::Patch& p : g.patches) {
                const float wmin[3] = {p.bmin[0] + g.tx, p.bmin[1] + g.ty, g.kz * p.bmin[2] + g.tz}, wmax[3] = {p.bmax[0] + g.tx, p.bmax[1] + g.ty, g.kz * p.bmax[2] + g.tz};
                if (!box_in_frustum(view_proj_, wmin, wmax)) continue;
                Batch* b = nullptr;
                for (Batch& x : batches)
                    if (x.cx == p.cx && x.cz == p.cz) { b = &x; break; }
                if (!b) { batches.push_back({terrain_grid_ib(p.cx, p.cz), p.cx, p.cz, {}, 0}); b = &batches.back(); }
                const float tx = g.ox + g.tx + float(p.pi * kPatchCells) * g.step, tz = g.kz * (g.oz + float(p.pj * kPatchCells) * g.step) + g.tz;
                const float rows[12] = {g.step, 0, 0, tx, 0, 0, 0, g.ty, 0, 0, g.step * g.kz, tz};   // world x = gx * step + tx, the height texture's y (+ ty), world z = gz * step * kz + tz
                b->rows.insert(b->rows.end(), rows, rows + 12);
                ++b->count;
            }
            for (Batch& b : batches) {
                D3D11_MAPPED_SUBRESOURCE ms;
                const size_t bytes = b.rows.size() * 4;
                if (bytes > 64 * 4096) continue;
                if (FAILED(ctx_->Map(terrain_inst_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
                std::memcpy(ms.pData, b.rows.data(), bytes);
                ctx_->Unmap(terrain_inst_vb_.Get(), 0);
                ctx_->IASetVertexBuffers(1, 1, terrain_inst_vb_.GetAddressOf(), &inst_stride, &off0);
                ctx_->IASetIndexBuffer(b.ib, DXGI_FORMAT_R16_UINT, 0);
                ctx_->DrawIndexedInstanced(UINT(b.cx * b.cz * 6), b.count, 0, 0, 0);
                ++draw_stats_.draw_calls;
                draw_stats_.triangles += uint64_t(b.cx) * b.cz * 2 * b.count;
            }
            (void)t;
        }
    };
    draw_surface(*geom_ps, terrain_zw_.rtv.Get(), ds_terrain_depth_.Get(), 2);
    draw_surface(*depth_ps, terrain_viewz_.rtv.Get(), ds_terrain_equal_.Get(), 0);

    // ---- 2. the blend boxes
    {
        const std::vector<D3D11_INPUT_ELEMENT_DESC> el = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                                          {"TEXCOORD", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                                          {"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                                          {"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                                          {"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 48, D3D11_INPUT_PER_INSTANCE_DATA, 1}};
        ID3D11InputLayout* il = nullptr;
        try {
            il = lib_->layout(*blend_vs, "terrain_box", el);
        } catch (const std::exception& ex) {
            error(ex.what());
            return;
        }
        ID3D11RenderTargetView* rtvs[3] = {terrain_acc_[0].rtv.Get(), terrain_acc_[1].rtv.Get(), terrain_acc_[2].rtv.Get()};
        ctx_->OMSetRenderTargets(3, rtvs, dsv_.Get());
        ctx_->OMSetDepthStencilState(ds_terrain_stencil_.Get(), 2);
        ctx_->OMSetBlendState(bs_terrain_add_.Get(), nullptr, 0xffffffff);
        ctx_->RSSetState(rs_front_cull_.Get());
        ctx_->IASetInputLayout(il);
        for (const Entity* e : ents) {
            const Terrain& t = *e->get<Terrain>();
            const TerrainMaterial& m = *t.material;
            const TerrainMatGpu& g = terrain_mat_[m.id];
            // ps_height_map_cb: the values the live game shows (dump of a battle) where the tool has no other source; the far-distance blend and the map-wide normal
            // map are switched off (g_far_tile_distance / g_far_blend_range very large): the tile's own maps are the near field, and the editor has one tile
            cbs_.set_f("ps_height_map_cb", "g_mid_distance_details", {20.0f, 550.0f, 3.0f, 0.9f});
            cbs_.set_f("ps_height_map_cb", "g_near_distance_details", {200.0f, 4.0f, 0.6f, 0.0f});
            cbs_.set_f("ps_height_map_cb", "g_mid_distance_normal_strength", {2.0f});
            cbs_.set_f("ps_height_map_cb", "g_mid_distance_slope", {0.96f, 3.0f});
            cbs_.set_f("ps_height_map_cb", "g_channel_offsets", {0, 0, 0, 0});
            cbs_.set_f("ps_height_map_cb", "g_normal_scales", {1.0f, 1.0f, 1.0f});
            cbs_.set_f("ps_height_map_cb", "g_blend_gain", {1.0f});
            cbs_.set_f("ps_height_map_cb", "g_far_tile_distance", {1.0e9f});
            cbs_.set_f("ps_height_map_cb", "g_far_blend_range", {1.0e9f, 2.0e9f});
            cbs_.set_f("ps_height_map_cb", "g_far_world_write_uv_offset", {1, 1, 0, 0});
            cbs_.set_f("ps_height_map_cb", "g_far_world_read_uv_offset", {0.0f});
            cbs_.set_f("ps_height_map_cb", "g_blend_contrast", {100.0f});
            cbs_.set_f("ps_height_map_cb", "g_lq_infield_blend_range", {0.0f});
            cbs_.set_i("ps_height_map_cb", "g_is_campaign", 0);
            cbs_.set_f("vs_height_map_cb", "g_world_dimensions", {1.0e6f, 1.0e6f});   // the (disabled) map-wide normal map's uv
            // ps_texture_array_cb: one layer per texture group, its four variations are the slots 4 * layer + 0..3; g_array_section_sizes.z = 0 keeps the index at the tile's own
            // the shader reads `cb3[mapping + 1]` of the cbuffer, whose row 0 is g_light_params_texture_width: that is array element [mapping] (= the layer here):
            // (metres per repeat, outfield scale, mid-distance strength). Element [layer + 1] left layer 0 with a scale of 0 (uv = world / 0: one flat texel)
            std::vector<float> arr(400 * 4, 0.0f);
            for (int l = 0; l < 4; ++l) {
                arr[size_t(l) * 4 + 0] = m.pixel_scale[l];
                arr[size_t(l) * 4 + 1] = m.outfield_scale[l];
                arr[size_t(l) * 4 + 2] = m.mid_strength[l];
            }
            cbs_.set("ps_texture_array_cb", "g_blend_pixel_scale_array", arr.data(), arr.size() * 4);
            cbs_.set_f("ps_texture_array_cb", "g_light_params_texture_width", {16.0f});
            cbs_.set_f("ps_texture_array_cb", "g_array_section_sizes", {1.0f, 1.0f, 0.0f, 0.0f});
            cbs_.set_f("ps_texture_array_cb", "g_remapped_season", {0.0f});
            cbs_.set_f("ps_texture_array_cb", "g_climate_change_index", {0.0f});
            cbs_.set_f("ps_texture_array_cb", "g_layer_exempt_groups", {1.0e6f, 1.0e6f, 1.0e6f, 1.0e6f});
            cbs_.set_f("ps_texture_array_cb", "g_sea_texture_index", {0.0f, 0.0f});
            std::vector<float> map(2048 * 4, 0.0f);   // g_array_mapping[i] = i (a float per 16-byte element)
            for (int i = 0; i < 2048; ++i) map[size_t(i) * 4] = float(i);
            cbs_.set("ps_mapping_array", "g_array_mapping", map.data(), map.size() * 4);
            cbs_.upload(dev_, ctx_);
            bind_shader(ctx_, *blend_vs, cbs_);
            bind_shader(ctx_, *blend_ps, cbs_);
            ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
            bind_srv(ctx_, *blend_ps, "s_depth_map", terrain_zw_.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_alpha_map", g.alpha.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_blend_map", g.blend.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_index_map", g.index.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_normal_map", g.normal.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_lf_normal_map", g.lf_normal.srv.Get());
            bind_srv(ctx_, *blend_ps, "s_light_params", g.light_params.Get());
            bind_srv(ctx_, *blend_ps, "t_diffuse_array", g.diffuse.srv.Get());
            bind_srv(ctx_, *blend_ps, "t_normal_array", g.normals.srv.Get());
            for (auto& [n, slot] : blend_ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_linear_.GetAddressOf());
            bind_sampler(ctx_, *blend_ps, "s_depth_map", s_clamp_point_.Get());
            bind_sampler(ctx_, *blend_ps, "s_light_params", s_clamp_point_.Get());
            bind_sampler(ctx_, *blend_ps, "s_diffuse_array", s_wrap_aniso_.Get());
            bind_sampler(ctx_, *blend_ps, "s_normal_array", s_wrap_aniso_.Get());
            bind_sampler(ctx_, *blend_ps, "s_lf_normal_map", s_wrap_linear_.Get());
            // one instance: the layer mapping indices (T0), the box corners (T1 min, T2 max), rotation index 0 and the layer base 0 (T3)
            const float inst[16] = {0, 1, 2, 3, float(m.x0), float(m.y0), float(m.z0), 0, float(m.x1), float(m.y1), float(m.z1), 0, 0, 0, 0, 0};
            D3D11_MAPPED_SUBRESOURCE ms;
            if (FAILED(ctx_->Map(terrain_inst_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
            std::memcpy(ms.pData, inst, sizeof inst);
            ctx_->Unmap(terrain_inst_vb_.Get(), 0);
            ID3D11Buffer* vbs[2] = {box_vb_.Get(), terrain_inst_vb_.Get()};
            const UINT strides[2] = {12, 64}, offs[2] = {0, 0};
            ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
            ctx_->IASetIndexBuffer(box_ib_.Get(), DXGI_FORMAT_R16_UINT, 0);
            ctx_->DrawIndexedInstanced(36, 1, 0, 0, 0);
            ++draw_stats_.draw_calls;
        }
    }

    // ---- 3. the resolve: terrain pixels into the G-buffer
    {
        const TerrainMaterial& m = *ents.front()->get<Terrain>()->material;
        const TerrainMatGpu& g = terrain_mat_[m.id];
        const std::vector<D3D11_INPUT_ELEMENT_DESC> el = {{"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                                          {"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 16, D3D11_INPUT_PER_VERTEX_DATA, 0}};
        ID3D11InputLayout* il = nullptr;
        try {
            il = lib_->layout(*res_vs, "terrain_resolve", el);
        } catch (const std::exception& ex) {
            error(ex.what());
            return;
        }
        ID3D11RenderTargetView* rtvs[5] = {gb_[0].rtv.Get(), gb_[1].rtv.Get(), gb_[2].rtv.Get(), gb_[3].rtv.Get(), gb_[4].rtv.Get()};
        ctx_->OMSetRenderTargets(5, rtvs, dsv_.Get());
        ctx_->OMSetDepthStencilState(ds_terrain_resolve_.Get(), 2);
        ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
        ctx_->RSSetState(rs_none_.Get());
        ctx_->IASetInputLayout(il);
        cbs_.set_f("vs_height_map_cb", "g_world_dimensions", {1.0e6f, 1.0e6f});
        cbs_.set_f("vs_height_map_cb", "g_far_terrain_scale", {1.0f});
        cbs_.set_f("vs_height_map_cb", "g_layer1_texture_index", {0.0f});
        cbs_.set_f("vs_height_map_cb", "g_layer2_texture_index", {0.0f});
        cbs_.set_f("ps_height_map_cb", "g_far_blend_range", {1.0e9f, 2.0e9f});
        cbs_.set_f("shroud_PS", "g_shroud_enabled", {0.0f});
        cbs_.upload(dev_, ctx_);
        bind_shader(ctx_, *res_vs, cbs_);
        bind_shader(ctx_, *res_ps, cbs_);
        ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
        bind_srv(ctx_, *res_ps, "s_albedo_map", terrain_acc_[0].srv.Get());
        bind_srv(ctx_, *res_ps, "s_terrain_normal_map", terrain_acc_[1].srv.Get());
        bind_srv(ctx_, *res_ps, "s_terrain_gloss_map", terrain_acc_[2].srv.Get());
        bind_srv(ctx_, *res_ps, "s_depth_map", terrain_viewz_.srv.Get());
        bind_srv(ctx_, *res_ps, "s_far_diffuse_map", g.black.srv.Get());
        bind_srv(ctx_, *res_ps, "s_far_normal_map", g.lf_normal.srv.Get());
        bind_srv(ctx_, *res_ps, "s_mask_map", g.black.srv.Get());
        bind_srv(ctx_, *res_ps, "s_shroud", white_srv_.Get());
        bind_srv(ctx_, *res_ps, "s_snow_normals", g.lf_normal.srv.Get());
        bind_srv(ctx_, *res_ps, "t_diffuse_array", g.diffuse.srv.Get());
        const Texture* dc = m.detail_colour.empty() ? nullptr : textures_.get(m.detail_colour, false);
        const Texture* dn = m.detail_normal.empty() ? nullptr : textures_.get(m.detail_normal, false);
        bind_srv(ctx_, *res_ps, "s_mid_distance_detail_map_colour", dc ? dc->srv.Get() : white_srv_.Get());
        bind_srv(ctx_, *res_ps, "s_mid_distance_detail_map_normal", dn ? dn->srv.Get() : g.lf_normal.srv.Get());
        for (auto& [n, slot] : res_ps->samplers) ctx_->PSSetSamplers(slot, 1, s_wrap_linear_.GetAddressOf());
        for (const char* n : {"s_albedo_map", "s_terrain_normal_map", "s_terrain_gloss_map", "s_depth_map", "s_mask_map"}) bind_sampler(ctx_, *res_ps, n, s_clamp_point_.Get());
        bind_sampler(ctx_, *res_ps, "s_shroud", s_clamp_linear_.Get());
        bind_sampler(ctx_, *res_ps, "s_diffuse_array", s_wrap_aniso_.Get());
        const UINT stride = 24, off = 0;
        ctx_->IASetVertexBuffers(0, 1, resolve_vb_.GetAddressOf(), &stride, &off);
        ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx_->Draw(6, 0);
        ++draw_stats_.draw_calls;
    }
    ctx_->PSSetShaderResources(0, 16, std::array<ID3D11ShaderResourceView*, 16>{}.data());
    ctx_->RSSetState(rs_back_.Get());
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
}

} // namespace wce
