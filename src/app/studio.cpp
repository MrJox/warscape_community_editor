#include "app/studio.h"

#include "imgui.h"

#include <DirectXMath.h>
#include <d3dcompiler.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <unordered_map>

using namespace DirectX;
namespace fs = std::filesystem;

namespace ts {
namespace {

const char* kShaders = R"HLSL(
cbuffer Frame : register(b0) {
    float4x4 vp;
    float4 sun;
    float4 height_range;   // x = min, y = range
    float4 brush;          // x, z = centre (m), y = radius (m), w = active
    float4 select_box;     // unused
};
struct TIn { float3 p : POSITION; float3 n : NORMAL; };
struct TOut { float4 pos : SV_Position; float3 n : NORMAL; float3 w : TEXCOORD0; };
TOut VSTerrain(TIn i) { TOut o; o.pos = mul(float4(i.p, 1), vp); o.n = i.n; o.w = i.p; return o; }
float4 PSTerrain(TOut i) : SV_Target {
    float3 n = normalize(i.n);
    float l = saturate(dot(n, normalize(sun.xyz))) * 0.75 + 0.25;
    float t = saturate((i.w.y - height_range.x) / max(height_range.y, 0.001));
    float3 grass = float3(0.23, 0.36, 0.17), rock = float3(0.52, 0.48, 0.42), snow = float3(0.9, 0.92, 0.95);
    float3 c = lerp(grass, rock, smoothstep(0.35, 0.7, t));
    c = lerp(c, snow, smoothstep(0.85, 1.0, t));
    c = lerp(c, rock, smoothstep(0.55, 0.85, 1.0 - n.y));
    c *= l;
    if (brush.w > 0.5) {
        float d = distance(i.w.xz, brush.xz);
        float ring = smoothstep(brush.y * 0.04 + 0.2, 0.0, abs(d - brush.y));
        c = lerp(c, float3(1, 0.85, 0.2), saturate(ring) * 0.9 + (d < brush.y ? 0.08 : 0.0));
    }
    return float4(c, 1);
}
struct BIn { float3 p : POSITION; float3 n : NORMAL; float3 ipos : IPOS; float isize : ISIZE; float4 icol : ICOL; };
struct BOut { float4 pos : SV_Position; float4 col : COLOR; };
BOut VSBox(BIn i) {
    BOut o;
    float3 w = i.ipos + i.p * i.isize;
    o.pos = mul(float4(w, 1), vp);
    float l = saturate(dot(normalize(i.n), normalize(sun.xyz))) * 0.5 + 0.5;
    o.col = float4(i.icol.rgb * l, i.icol.a);
    return o;
}
float4 PSColor(BOut i) : SV_Target { return i.col; }
struct LIn { float3 p : POSITION; float4 c : COLOR; };
BOut VSLine(LIn i) { BOut o; o.pos = mul(float4(i.p, 1), vp); o.col = i.c; return o; }
)HLSL";

struct TerrainVertex { float p[3]; float n[3]; };
struct BoxVertex { float p[3]; float n[3]; };
struct InstanceData { float pos[3]; float size; float col[4]; };
struct LineVertex { float p[3]; float c[4]; };
struct FrameCB { XMFLOAT4X4 vp; XMFLOAT4 sun; XMFLOAT4 height_range; XMFLOAT4 brush; XMFLOAT4 select_box; };

uint32_t pack_color(float r, float g, float b) {
    return uint32_t(r * 255) | uint32_t(g * 255) << 8 | uint32_t(b * 255) << 16 | 0xff000000u;
}

void unpack(uint32_t c, float out[4]) {
    out[0] = (c & 255) / 255.f; out[1] = ((c >> 8) & 255) / 255.f; out[2] = ((c >> 16) & 255) / 255.f; out[3] = 1.f;
}

struct TypeStyle { uint32_t color; float size; bool draw; };

TypeStyle style_of(const std::string& t) {
    if (t == "BUILDING_INSTANCE") return {pack_color(0.85f, 0.45f, 0.2f), 5.f, true};
    if (t == "VEGETATION_INSTANCE") return {pack_color(0.2f, 0.7f, 0.25f), 1.5f, true};
    if (t == "ProjectedDecalInstance") return {pack_color(0.55f, 0.45f, 0.35f), 1.5f, true};
    if (t == "VFXInstance") return {pack_color(0.75f, 0.35f, 0.9f), 2.f, true};
    if (t == "POINT_LIGHT_INSTANCE") return {pack_color(1.f, 0.95f, 0.4f), 1.5f, true};
    if (t == "SIMPLE_MESH_INSTANCE") return {pack_color(0.6f, 0.6f, 0.75f), 4.f, true};
    if (t == "CaptureLocationInstance") return {pack_color(0.2f, 0.6f, 1.f), 3.f, true};
    if (t == "SiegeAINodeInstance" || t == "SiegeAIEdgeInstance") return {pack_color(0.95f, 0.2f, 0.2f), 2.f, true};
    if (t.find("POLYGON") != std::string::npos || t.find("Polygon") != std::string::npos || t.find("Zone") != std::string::npos || t == "AISeparatorInstance")
        return {pack_color(0.3f, 0.9f, 0.9f), 1.5f, true};
    return {pack_color(0.8f, 0.8f, 0.8f), 2.f, true};
}

float degrees(double r) { return float(r * 180.0 / 3.14159265358979323846); }
double radians(float d) { return double(d) * 3.14159265358979323846 / 180.0; }

} // namespace

struct Action {
    enum Kind { Height, Move, Struct } kind = Height;
    int elem = 0;
    te::Vec3 old_pos{}, new_pos{}, old_euler{}, new_euler{};
    te::TedScene::Snapshot before, after;     // Struct: whole-document snapshots
    int sel_before = -1, sel_after = -1;
};

struct Studio::Impl {
    ID3D11Device* dev;
    ID3D11DeviceContext* ctx;
    std::unique_ptr<te::TileProject> project;
    std::string status = "Open a tile folder (File > Open) or pass --tile <folder>.";
    std::string path_input;
    char path_buf[1024] = {0};

    // D3D objects
    ComPtr<ID3D11VertexShader> vs_terrain, vs_box, vs_line;
    ComPtr<ID3D11PixelShader> ps_terrain, ps_color;
    ComPtr<ID3D11InputLayout> il_terrain, il_box, il_line;
    ComPtr<ID3D11Buffer> cb, terrain_vb, terrain_ib, box_vb, box_ib, instance_vb, line_vb;
    ComPtr<ID3D11DepthStencilState> depth_on;
    ComPtr<ID3D11RasterizerState> raster, raster_wire;
    ComPtr<ID3D11Texture2D> depth_tex;
    ComPtr<ID3D11DepthStencilView> dsv;
    int surf_w = 0, surf_h = 0;
    UINT terrain_index_count = 0, instance_capacity = 0, line_capacity = 0;
    UINT instance_count = 0, line_count = 0;

    // camera
    float yaw = 0.8f, pitch = 0.6f, dist = 400.f;
    XMFLOAT3 target{128, 0, 128};
    XMMATRIX view = XMMatrixIdentity(), proj = XMMatrixIdentity();

    // scene data for display
    std::vector<Instance> instances;
    std::vector<LineVertex> lines;               // polygon outlines (static until the scene changes)
    std::set<std::string> hidden_types;
    std::unordered_map<std::string, int> type_counts;
    bool scene_dirty = true, terrain_dirty = true;
    int selected = -1;                           // element index
    int hover_inst = -1;

    // tools
    Tool tool = Tool::Select;
    float brush_radius_m = 12.f, brush_strength = 6.f, brush_falloff = 0.7f, flatten_height = 0.f;
    bool brush_active = false, painting = false;
    Float3 brush_world;
    bool terrain_hit = false;
    float hit_height = 0;
    bool grabbing = false;
    te::Vec3 grab_old_pos{};
    double grab_y_offset = 0;
    bool show_vegetation = true, show_decals = true, show_wire = false, show_polys = true;
    bool dirty_flag_scene = false;

    // undo
    std::vector<Action> undo, redo;
    Action pending{};                            // inspector edit in progress
    bool pending_open = false;

    float last_dt = 0.016f;
    std::string diag;

    Studio* owner = nullptr;
    Impl(ID3D11Device* d, ID3D11DeviceContext* c) : dev(d), ctx(c) {}

    // ---------------------------------------------------------------------------------------------- setup
    bool compile(const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
        ComPtr<ID3DBlob> err;
        HRESULT hr = D3DCompile(kShaders, std::strlen(kShaders), "studio.hlsl", nullptr, nullptr, entry, target, D3DCOMPILE_OPTIMIZATION_LEVEL2, 0, &out, &err);
        if (FAILED(hr)) {
            diag += std::string("shader ") + entry + ": " + (err ? static_cast<const char*>(err->GetBufferPointer()) : "?") + "\n";
            return false;
        }
        return true;
    }

    void create_resources() {
        ComPtr<ID3DBlob> b;
        compile("VSTerrain", "vs_5_0", b);
        dev->CreateVertexShader(b->GetBufferPointer(), b->GetBufferSize(), nullptr, &vs_terrain);
        D3D11_INPUT_ELEMENT_DESC te[] = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                         {"NORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0}};
        dev->CreateInputLayout(te, 2, b->GetBufferPointer(), b->GetBufferSize(), &il_terrain);
        compile("PSTerrain", "ps_5_0", b);
        dev->CreatePixelShader(b->GetBufferPointer(), b->GetBufferSize(), nullptr, &ps_terrain);
        compile("VSBox", "vs_5_0", b);
        dev->CreateVertexShader(b->GetBufferPointer(), b->GetBufferSize(), nullptr, &vs_box);
        D3D11_INPUT_ELEMENT_DESC be[] = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                         {"NORMAL", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                         {"IPOS", 0, DXGI_FORMAT_R32G32B32_FLOAT, 1, 0, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                         {"ISIZE", 0, DXGI_FORMAT_R32_FLOAT, 1, 12, D3D11_INPUT_PER_INSTANCE_DATA, 1},
                                         {"ICOL", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, D3D11_INPUT_PER_INSTANCE_DATA, 1}};
        dev->CreateInputLayout(be, 5, b->GetBufferPointer(), b->GetBufferSize(), &il_box);
        compile("PSColor", "ps_5_0", b);
        dev->CreatePixelShader(b->GetBufferPointer(), b->GetBufferSize(), nullptr, &ps_color);
        compile("VSLine", "vs_5_0", b);
        dev->CreateVertexShader(b->GetBufferPointer(), b->GetBufferSize(), nullptr, &vs_line);
        D3D11_INPUT_ELEMENT_DESC le[] = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                         {"COLOR", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0}};
        dev->CreateInputLayout(le, 2, b->GetBufferPointer(), b->GetBufferSize(), &il_line);

        D3D11_BUFFER_DESC cbd{sizeof(FrameCB), D3D11_USAGE_DYNAMIC, D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
        dev->CreateBuffer(&cbd, nullptr, &cb);

        // unit cube centred on the origin, 24 vertices with face normals
        std::vector<BoxVertex> v;
        std::vector<uint16_t> idx;
        const float n[6][3] = {{1, 0, 0}, {-1, 0, 0}, {0, 1, 0}, {0, -1, 0}, {0, 0, 1}, {0, 0, -1}};
        for (int f = 0; f < 6; ++f) {
            XMVECTOR nv = XMVectorSet(n[f][0], n[f][1], n[f][2], 0);
            XMVECTOR up = std::fabs(n[f][1]) > 0.5f ? XMVectorSet(1, 0, 0, 0) : XMVectorSet(0, 1, 0, 0);
            XMVECTOR r = XMVector3Normalize(XMVector3Cross(up, nv)), u = XMVector3Cross(nv, r);
            for (int k = 0; k < 4; ++k) {
                float sx = (k & 1) ? 0.5f : -0.5f, sy = (k & 2) ? 0.5f : -0.5f;
                XMVECTOR p = nv * 0.5f + r * sx + u * sy;
                BoxVertex bv{};
                XMStoreFloat3(reinterpret_cast<XMFLOAT3*>(bv.p), p);
                std::memcpy(bv.n, n[f], 12);
                v.push_back(bv);
            }
            uint16_t base = uint16_t(f * 4);
            for (uint16_t i : {0, 1, 2, 1, 3, 2}) idx.push_back(uint16_t(base + i));
        }
        D3D11_BUFFER_DESC vbd{UINT(v.size() * sizeof(BoxVertex)), D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER, 0, 0, 0};
        D3D11_SUBRESOURCE_DATA sd{v.data(), 0, 0};
        dev->CreateBuffer(&vbd, &sd, &box_vb);
        D3D11_BUFFER_DESC ibd{UINT(idx.size() * 2), D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER, 0, 0, 0};
        D3D11_SUBRESOURCE_DATA sdi{idx.data(), 0, 0};
        dev->CreateBuffer(&ibd, &sdi, &box_ib);

        D3D11_DEPTH_STENCIL_DESC dd{};
        dd.DepthEnable = TRUE;
        dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
        dd.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
        dev->CreateDepthStencilState(&dd, &depth_on);
        D3D11_RASTERIZER_DESC rd{};
        rd.FillMode = D3D11_FILL_SOLID;
        rd.CullMode = D3D11_CULL_NONE;
        rd.DepthClipEnable = TRUE;
        dev->CreateRasterizerState(&rd, &raster);
        rd.FillMode = D3D11_FILL_WIREFRAME;
        dev->CreateRasterizerState(&rd, &raster_wire);
    }

    void make_depth(int w, int h) {
        if (w <= 0 || h <= 0) return;
        surf_w = w; surf_h = h;
        D3D11_TEXTURE2D_DESC td{};
        td.Width = UINT(w); td.Height = UINT(h); td.MipLevels = 1; td.ArraySize = 1;
        td.Format = DXGI_FORMAT_D32_FLOAT; td.SampleDesc.Count = 1; td.BindFlags = D3D11_BIND_DEPTH_STENCIL;
        depth_tex.Reset(); dsv.Reset();
        dev->CreateTexture2D(&td, nullptr, &depth_tex);
        dev->CreateDepthStencilView(depth_tex.Get(), nullptr, &dsv);
    }

    // ---------------------------------------------------------------------------------------------- data
    double unit() const { return project ? project->unit_scale : 2.0; }
    double extent() const { return project ? project->extent_m() : 256.0; }

    float terrain_height(float x, float z) const {
        if (!project || !project->has_height) return 0.f;
        const auto& h = *project->height;
        double px = x / extent() * (h.width() - 1), py = z / extent() * (h.height() - 1);
        return float(h.sample(px, py));
    }

    void rebuild_terrain() {
        terrain_dirty = false;
        if (!project || !project->has_height) { terrain_index_count = 0; return; }
        const te::HeightField& hf = *project->height;
        int step = std::max(1, int(std::ceil(std::max(hf.width(), hf.height()) / 512.0)));
        int nx = (int(hf.width()) - 1) / step + 1, ny = (int(hf.height()) - 1) / step + 1;
        double mx = extent() / (hf.width() - 1), mz = extent() / (hf.height() - 1);
        std::vector<TerrainVertex> v(size_t(nx) * ny);
        double lo = 1e30, hi = -1e30;
        auto H = [&](int x, int y) { x = std::clamp(x, 0, int(hf.width()) - 1); y = std::clamp(y, 0, int(hf.height()) - 1); return hf.metres(uint32_t(x), uint32_t(y)); };
        for (int j = 0; j < ny; ++j)
            for (int i = 0; i < nx; ++i) {
                int x = std::min(i * step, int(hf.width()) - 1), y = std::min(j * step, int(hf.height()) - 1);
                double h0 = H(x, y);
                lo = std::min(lo, h0); hi = std::max(hi, h0);
                double dx = (H(x + step, y) - H(x - step, y)) / (2.0 * step * mx), dz = (H(x, y + step) - H(x, y - step)) / (2.0 * step * mz);
                XMVECTOR nrm = XMVector3Normalize(XMVectorSet(float(-dx), 1.f, float(-dz), 0));
                TerrainVertex& t = v[size_t(j) * nx + i];
                t.p[0] = float(x * mx); t.p[1] = float(h0); t.p[2] = float(y * mz);
                XMStoreFloat3(reinterpret_cast<XMFLOAT3*>(t.n), nrm);
            }
        std::vector<uint32_t> idx;
        idx.reserve(size_t(nx - 1) * (ny - 1) * 6);
        for (int j = 0; j + 1 < ny; ++j)
            for (int i = 0; i + 1 < nx; ++i) {
                uint32_t a = uint32_t(j * nx + i), b = a + 1, c = a + uint32_t(nx), d = c + 1;
                for (uint32_t k : {a, c, b, b, c, d}) idx.push_back(k);
            }
        terrain_index_count = UINT(idx.size());
        terrain_vb.Reset(); terrain_ib.Reset();
        D3D11_BUFFER_DESC vbd{UINT(v.size() * sizeof(TerrainVertex)), D3D11_USAGE_IMMUTABLE, D3D11_BIND_VERTEX_BUFFER, 0, 0, 0};
        D3D11_SUBRESOURCE_DATA sd{v.data(), 0, 0};
        dev->CreateBuffer(&vbd, &sd, &terrain_vb);
        D3D11_BUFFER_DESC ibd{UINT(idx.size() * 4), D3D11_USAGE_IMMUTABLE, D3D11_BIND_INDEX_BUFFER, 0, 0, 0};
        D3D11_SUBRESOURCE_DATA sdi{idx.data(), 0, 0};
        dev->CreateBuffer(&ibd, &sdi, &terrain_ib);
        hmin = float(lo); hrange = float(std::max(1.0, hi - lo));
        char dbuf[256];
        std::snprintf(dbuf, sizeof dbuf, "terrain %dx%d step %d spans x %.1f..%.1f z %.1f..%.1f extent %.1f height %.2f..%.2f\n", nx, ny, step, v.front().p[0], v.back().p[0], v.front().p[2], v.back().p[2], extent(), lo, hi);
        diag += dbuf;
    }
    float hmin = 0, hrange = 1;

    void rebuild_scene() {
        scene_dirty = false;
        instances.clear();
        lines.clear();
        type_counts.clear();
        if (!project) return;
        const te::TedScene& sc = project->scene;
        double u = unit();
        const auto& els = sc.doc().elements();
        for (size_t i = 0; i < els.size(); ++i) {
            int e = int(i);
            const std::string& t = els[i].name;
            if (t == "point" || t == "points" || t == "children" || t == "camera" || sc.is_layer(e)) continue;
            auto p = sc.position(e);
            if (!p) continue;
            ++type_counts[t];
            if (hidden_types.count(t)) continue;
            te::WorldTransform w = te::world_transform(sc, e);
            Instance in;
            in.elem = e;
            in.type = t;
            in.pos = {float(w.position[0] / u), float(w.position[1]), float(w.position[2] / u)};
            TypeStyle st = style_of(t);
            in.size = st.size;
            in.color = st.color;
            instances.push_back(in);
            // polygon outlines
            for (int c : els[i].children) {
                if (els[c].name != "points" || !show_polys) continue;
                std::vector<Float3> pts;
                for (int pt : els[c].children) {
                    auto s = sc.doc().get(pt, "position");
                    if (!s) continue;
                    auto v = te::parse_vec3(*s, "");
                    if (!v) continue;
                    te::Vec3 r = te::rotate_vec(w.rotation, *v);
                    pts.push_back({float((w.position[0] + r[0]) / u), float(w.position[1] + r[1]), float((w.position[2] + r[2]) / u)});
                }
                float col[4];
                unpack(st.color, col);
                bool closed = sc.doc().get(e, "polygon_closed").value_or("1") != "0";
                for (size_t k = 0; k + 1 < pts.size() + (closed ? 1 : 0); ++k) {
                    const Float3& a = pts[k]; const Float3& b = pts[(k + 1) % pts.size()];
                    lines.push_back({{a.x, a.y + 0.3f, a.z}, {col[0], col[1], col[2], 1}});
                    lines.push_back({{b.x, b.y + 0.3f, b.z}, {col[0], col[1], col[2], 1}});
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------- camera / picking
    XMVECTOR eye() const {
        return XMVectorSet(target.x + dist * std::cos(pitch) * std::sin(yaw), target.y + dist * std::sin(pitch), target.z + dist * std::cos(pitch) * std::cos(yaw), 1);
    }

    void update_matrices() {
        view = XMMatrixLookAtLH(eye(), XMLoadFloat3(&target), XMVectorSet(0, 1, 0, 0));
        float aspect = surf_h > 0 ? float(surf_w) / float(surf_h) : 1.f;
        proj = XMMatrixPerspectiveFovLH(XMConvertToRadians(55.f), aspect, std::max(0.3f, dist * 0.002f), 40000.f);
    }

    bool mouse_ray(XMVECTOR& o, XMVECTOR& d) const {
        ImVec2 m = ImGui::GetIO().MousePos;
        if (surf_w <= 0) return false;
        XMVECTOR a = XMVector3Unproject(XMVectorSet(m.x, m.y, 0, 1), 0, 0, float(surf_w), float(surf_h), 0, 1, proj, view, XMMatrixIdentity());
        XMVECTOR b = XMVector3Unproject(XMVectorSet(m.x, m.y, 1, 1), 0, 0, float(surf_w), float(surf_h), 0, 1, proj, view, XMMatrixIdentity());
        o = a;
        d = XMVector3Normalize(b - a);
        return true;
    }

    bool pick_terrain(XMVECTOR o, XMVECTOR d, Float3& hit) const {
        if (!project || !project->has_height) return false;
        float t = 0, step = std::max(0.5f, float(extent()) / float(project->height->width()));
        float tmax = dist * 6 + 2000;
        float prev_t = 0;
        bool prev_above = true;
        for (; t < tmax; t += step) {
            XMFLOAT3 p;
            XMStoreFloat3(&p, o + d * t);
            if (p.x < 0 || p.z < 0 || p.x > extent() || p.z > extent()) { prev_t = t; prev_above = true; continue; }
            bool above = p.y > terrain_height(p.x, p.z);
            if (!above && prev_above && t > 0) {
                float lo = prev_t, hi = t;
                for (int k = 0; k < 20; ++k) {
                    float mid = (lo + hi) * 0.5f;
                    XMFLOAT3 q;
                    XMStoreFloat3(&q, o + d * mid);
                    if (q.y > terrain_height(q.x, q.z)) lo = mid; else hi = mid;
                }
                XMStoreFloat3(reinterpret_cast<XMFLOAT3*>(&hit), o + d * hi);
                return true;
            }
            prev_above = above;
            prev_t = t;
        }
        return false;
    }

    int pick_instance(XMVECTOR o, XMVECTOR d) const {
        int best = -1;
        float best_t = 1e30f;
        for (const Instance& in : instances) {
            XMVECTOR c = XMVectorSet(in.pos.x, in.pos.y, in.pos.z, 0);
            float r = in.size * 0.75f;
            XMVECTOR oc = o - c;
            float b = XMVectorGetX(XMVector3Dot(oc, d)), cc = XMVectorGetX(XMVector3Dot(oc, oc)) - r * r;
            float disc = b * b - cc;
            if (disc < 0) continue;
            float t = -b - std::sqrt(disc);
            if (t > 0 && t < best_t) { best_t = t; best = in.elem; }
        }
        return best;
    }

    // ---------------------------------------------------------------------------------------------- drawing
    void upload_instances() {
        if (instances.empty()) { instance_count = 0; return; }
        UINT n = UINT(instances.size());
        if (n > instance_capacity) {
            instance_capacity = n + n / 4 + 64;
            instance_vb.Reset();
            D3D11_BUFFER_DESC bd{instance_capacity * sizeof(InstanceData), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
            dev->CreateBuffer(&bd, nullptr, &instance_vb);
        }
        D3D11_MAPPED_SUBRESOURCE m;
        if (FAILED(ctx->Map(instance_vb.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) return;
        auto* out = static_cast<InstanceData*>(m.pData);
        for (const Instance& in : instances) {
            InstanceData d{};
            d.pos[0] = in.pos.x; d.pos[1] = in.pos.y; d.pos[2] = in.pos.z;
            d.size = in.size * (in.elem == selected ? 1.4f : 1.f);
            unpack(in.elem == selected ? pack_color(1, 1, 0.2f) : in.color, d.col);
            *out++ = d;
        }
        ctx->Unmap(instance_vb.Get(), 0);
        instance_count = n;
    }

    void draw_lines(const std::vector<LineVertex>& v) {
        if (v.empty()) return;
        UINT n = UINT(v.size());
        if (n > line_capacity) {
            line_capacity = n + n / 4 + 64;
            line_vb.Reset();
            D3D11_BUFFER_DESC bd{line_capacity * sizeof(LineVertex), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE, 0, 0};
            dev->CreateBuffer(&bd, nullptr, &line_vb);
        }
        D3D11_MAPPED_SUBRESOURCE m;
        if (FAILED(ctx->Map(line_vb.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) return;
        std::memcpy(m.pData, v.data(), v.size() * sizeof(LineVertex));
        ctx->Unmap(line_vb.Get(), 0);
        UINT stride = sizeof(LineVertex), off = 0;
        ctx->IASetInputLayout(il_line.Get());
        ctx->IASetVertexBuffers(0, 1, line_vb.GetAddressOf(), &stride, &off);
        ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_LINELIST);
        ctx->VSSetShader(vs_line.Get(), nullptr, 0);
        ctx->PSSetShader(ps_color.Get(), nullptr, 0);
        ctx->Draw(n, 0);
    }

    void render(ID3D11RenderTargetView* rtv) {
        if (!vs_terrain) return;
        if (!dsv) return;
        if (terrain_dirty) rebuild_terrain();
        if (scene_dirty) rebuild_scene();
        update_matrices();
        ctx->ClearDepthStencilView(dsv.Get(), D3D11_CLEAR_DEPTH, 1.f, 0);
        ctx->OMSetRenderTargets(1, &rtv, dsv.Get());
        D3D11_VIEWPORT vp{0, 0, float(surf_w), float(surf_h), 0, 1};
        ctx->RSSetViewports(1, &vp);
        ctx->OMSetDepthStencilState(depth_on.Get(), 0);
        ctx->RSSetState(show_wire ? raster_wire.Get() : raster.Get());
        FrameCB f{};
        XMStoreFloat4x4(&f.vp, XMMatrixTranspose(view * proj));
        f.sun = {0.4f, 0.8f, 0.3f, 0};
        f.height_range = {hmin, hrange, 0, 0};
        f.brush = {brush_world.x, brush_radius_m, brush_world.z, (brush_active && terrain_hit && tool != Tool::Select) ? 1.f : 0.f};
        D3D11_MAPPED_SUBRESOURCE m;
        if (SUCCEEDED(ctx->Map(cb.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &m))) { std::memcpy(m.pData, &f, sizeof f); ctx->Unmap(cb.Get(), 0); }
        ctx->VSSetConstantBuffers(0, 1, cb.GetAddressOf());
        ctx->PSSetConstantBuffers(0, 1, cb.GetAddressOf());

        if (terrain_index_count) {
            UINT stride = sizeof(TerrainVertex), off = 0;
            ctx->IASetInputLayout(il_terrain.Get());
            ctx->IASetVertexBuffers(0, 1, terrain_vb.GetAddressOf(), &stride, &off);
            ctx->IASetIndexBuffer(terrain_ib.Get(), DXGI_FORMAT_R32_UINT, 0);
            ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx->VSSetShader(vs_terrain.Get(), nullptr, 0);
            ctx->PSSetShader(ps_terrain.Get(), nullptr, 0);
            ctx->DrawIndexed(terrain_index_count, 0, 0);
        }
        upload_instances();
        if (instance_count) {
            UINT strides[2] = {sizeof(BoxVertex), sizeof(InstanceData)}, offs[2] = {0, 0};
            ID3D11Buffer* vbs[2] = {box_vb.Get(), instance_vb.Get()};
            ctx->IASetInputLayout(il_box.Get());
            ctx->IASetVertexBuffers(0, 2, vbs, strides, offs);
            ctx->IASetIndexBuffer(box_ib.Get(), DXGI_FORMAT_R16_UINT, 0);
            ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx->VSSetShader(vs_box.Get(), nullptr, 0);
            ctx->PSSetShader(ps_color.Get(), nullptr, 0);
            ctx->DrawIndexedInstanced(36, instance_count, 0, 0, 0);
        }
        ctx->RSSetState(raster.Get());
        std::vector<LineVertex> l = lines;
        // selected instance: axes
        for (const Instance& in : instances)
            if (in.elem == selected) {
                const float a = 8.f;
                l.push_back({{in.pos.x, in.pos.y, in.pos.z}, {1, 0.2f, 0.2f, 1}}); l.push_back({{in.pos.x + a, in.pos.y, in.pos.z}, {1, 0.2f, 0.2f, 1}});
                l.push_back({{in.pos.x, in.pos.y, in.pos.z}, {0.2f, 1, 0.2f, 1}}); l.push_back({{in.pos.x, in.pos.y + a, in.pos.z}, {0.2f, 1, 0.2f, 1}});
                l.push_back({{in.pos.x, in.pos.y, in.pos.z}, {0.3f, 0.5f, 1, 1}}); l.push_back({{in.pos.x, in.pos.y, in.pos.z + a}, {0.3f, 0.5f, 1, 1}});
            }
        // tile border
        float e = float(extent());
        const float bc[4] = {0.9f, 0.9f, 0.9f, 1};
        auto corner = [&](float x, float z) { LineVertex v{{x, terrain_height(x, z), z}, {bc[0], bc[1], bc[2], bc[3]}}; return v; };
        LineVertex c0 = corner(0, 0), c1 = corner(e, 0), c2 = corner(e, e), c3 = corner(0, e);
        for (auto& pr : {std::pair<LineVertex, LineVertex>{c0, c1}, {c1, c2}, {c2, c3}, {c3, c0}}) { l.push_back(pr.first); l.push_back(pr.second); }
        draw_lines(l);
    }

    // ---------------------------------------------------------------------------------------------- editing
    void push_move(const Action& a) { undo.push_back(a); redo.clear(); }

    void do_undo() {
        if (undo.empty() || !project) return;
        Action a = undo.back(); undo.pop_back();
        if (a.kind == Action::Height) { if (project->height->undo()) { terrain_dirty = true; scene_dirty = true; } }
        else if (a.kind == Action::Struct) { project->scene.restore(a.before); selected = a.sel_before; scene_dirty = true; }
        else { project->scene.set_position(a.elem, a.old_pos); project->scene.set_euler(a.elem, a.old_euler); scene_dirty = true; }
        redo.push_back(a);
    }

    void do_redo() {
        if (redo.empty() || !project) return;
        Action a = redo.back(); redo.pop_back();
        if (a.kind == Action::Height) { if (project->height->redo()) { terrain_dirty = true; scene_dirty = true; } }
        else if (a.kind == Action::Struct) { project->scene.restore(a.after); selected = a.sel_after; scene_dirty = true; }
        else { project->scene.set_position(a.elem, a.new_pos); project->scene.set_euler(a.elem, a.new_euler); scene_dirty = true; }
        undo.push_back(a);
    }

    void paint(const Float3& at) {
        te::HeightField& hf = *project->height;
        double ppm = (hf.width() - 1) / extent();
        te::Brush b;
        b.cx = at.x * ppm; b.cy = at.z * ppm; b.radius = std::max(1.0, brush_radius_m * ppm);
        b.falloff = brush_falloff;
        double dt = std::min(0.05, double(last_dt));
        switch (tool) {
            case Tool::Raise: b.strength = 1; hf.raise(b, brush_strength * dt); break;
            case Tool::Lower: b.strength = 1; hf.raise(b, -brush_strength * dt); break;
            case Tool::Smooth: b.strength = std::min(1.0, brush_strength * dt * 0.5); hf.smooth(b); break;
            case Tool::Flatten: b.strength = std::min(1.0, brush_strength * dt * 0.5); hf.flatten(b, flatten_height); break;
            case Tool::Level: b.strength = std::min(1.0, brush_strength * dt * 0.5); hf.set_level_under(b); break;
            default: break;
        }
        project->height_dirty = true;
        terrain_dirty = true;
    }

    void handle_input() {
        ImGuiIO& io = ImGui::GetIO();
        last_dt = io.DeltaTime;
        bool over_ui = io.WantCaptureMouse;
        XMVECTOR o, d;
        terrain_hit = false;
        brush_active = false;
        if (!over_ui && mouse_ray(o, d) && project) {
            Float3 hit;
            if (pick_terrain(o, d, hit)) { terrain_hit = true; brush_world = hit; hit_height = hit.y; brush_active = true; }
            hover_inst = tool == Tool::Select ? pick_instance(o, d) : -1;
        } else hover_inst = -1;

        // camera
        if (!over_ui) {
            if (io.MouseDown[1]) { yaw -= io.MouseDelta.x * 0.005f; pitch = std::clamp(pitch + io.MouseDelta.y * 0.005f, -1.5f, 1.5f); }
            if (io.MouseDown[2]) {
                XMVECTOR e = eye(), fwd = XMVector3Normalize(XMLoadFloat3(&target) - e);
                XMVECTOR right = XMVector3Normalize(XMVector3Cross(XMVectorSet(0, 1, 0, 0), fwd)), up = XMVector3Cross(fwd, right);
                float k = dist * 0.0012f;
                XMVECTOR t = XMLoadFloat3(&target) - right * io.MouseDelta.x * k + up * io.MouseDelta.y * k;
                XMStoreFloat3(&target, t);
            }
            if (io.MouseWheel != 0) dist = std::clamp(dist * std::pow(0.88f, io.MouseWheel), 3.f, 20000.f);
        }
        if (!io.WantTextInput) {
            if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_Z, false)) do_undo();
            if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_Y, false)) do_redo();
            if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_S, false) && project) save();
            if (ImGui::IsKeyPressed(ImGuiKey_G, false) && selected >= 0 && !grabbing && project) begin_grab();
            if (ImGui::IsKeyPressed(ImGuiKey_Delete, false) && selected >= 0 && !grabbing) delete_selected();
            if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_D, false) && selected >= 0 && !grabbing) duplicate_selected();
            if (ImGui::IsKeyPressed(ImGuiKey_R, false) && selected >= 0 && !grabbing) rotate_selected(io.KeyShift ? -angle_step_deg : angle_step_deg);
            if (ImGui::IsKeyPressed(ImGuiKey_Escape, false) && grabbing) cancel_grab();
        }
        if (grabbing) {
            if (terrain_hit) {
                double u = unit();
                float gx = brush_world.x, gz = brush_world.z;
                if (snap_grid && grid_m > 0.01f) { gx = std::round(gx / grid_m) * grid_m; gz = std::round(gz / grid_m) * grid_m; }
                float gy = snap_grid ? terrain_height(gx, gz) : hit_height;
                te::set_world_position(project->scene, selected, {double(gx) * u, gy + grab_y_offset, double(gz) * u});
                scene_dirty = true;
            }
            if (io.MouseClicked[0] && !over_ui) end_grab();
            if (io.MouseClicked[1]) cancel_grab();
            return;
        }
        // left button
        if (!over_ui && project) {
            if (tool == Tool::Select) {
                if (io.MouseClicked[0]) selected = hover_inst;
            } else if (project->has_height) {
                if (io.MouseClicked[0] && terrain_hit) { project->height->begin_edit(); painting = true; }
                if (painting && io.MouseDown[0] && terrain_hit) paint(brush_world);
            }
        }
        if (painting && !io.MouseDown[0]) {
            painting = false;
            project->height->end_edit();
            Action a; a.kind = Action::Height;
            undo.push_back(a);
            redo.clear();
        }
    }

    // ---- instance operations
    bool snap_grid = false, snap_angle = true;
    float grid_m = 6.f, angle_step_deg = 15.f;
    char new_key[128] = {0};

    void commit_struct(const te::TedScene::Snapshot& before, int sel_before) {
        Action a;
        a.kind = Action::Struct;
        a.before = before;
        a.after = project->scene.snapshot();
        a.sel_before = sel_before;
        a.sel_after = selected;
        push_move(a);
        scene_dirty = true;
    }

    void duplicate_selected() {
        if (!project || selected < 0) return;
        auto before = project->scene.snapshot();
        int old = selected;
        int copy = project->scene.duplicate(selected);
        if (copy < 0) return;
        selected = copy;
        te::WorldTransform w = te::world_transform(project->scene, copy);
        double u = unit();
        te::set_world_position(project->scene, copy, {w.position[0] + 4.0 * u, w.position[1], w.position[2]});
        commit_struct(before, old);
        status = "duplicated";
    }

    void delete_selected() {
        if (!project || selected < 0) return;
        auto before = project->scene.snapshot();
        int old = selected;
        project->scene.remove(selected);
        selected = -1;
        commit_struct(before, old);
        status = "deleted";
    }

    void add_building(const char* key) {
        if (!project || !key[0]) return;
        int tmpl = -1;
        if (selected >= 0 && project->scene.type(selected) == "BUILDING_INSTANCE") tmpl = selected;
        else { auto all = project->scene.find_all("BUILDING_INSTANCE"); if (!all.empty()) tmpl = all.front(); }
        if (tmpl < 0) { status = "no BUILDING_INSTANCE to use as a template in this tile"; return; }
        auto before = project->scene.snapshot();
        int old = selected;
        int copy = project->scene.duplicate(tmpl);
        project->scene.set_attr(copy, "key", key);
        project->scene.set_attr(copy, "name", key);
        selected = copy;
        if (terrain_hit) {
            double u = unit();
            te::set_world_position(project->scene, copy, {double(brush_world.x) * u, double(hit_height), double(brush_world.z) * u});
        }
        commit_struct(before, old);
        status = std::string("added ") + key;
    }

    void rotate_selected(float deg) {
        if (!project || selected < 0) return;
        auto& sc = project->scene;
        Action a;
        a.kind = Action::Move; a.elem = selected;
        a.old_pos = sc.position(selected).value_or(te::Vec3{0, 0, 0});
        a.old_euler = sc.euler(selected).value_or(te::Vec3{0, 0, 0});
        te::rotate_world_yaw(sc, selected, radians(deg));
        a.new_pos = a.old_pos;
        a.new_euler = sc.euler(selected).value_or(a.old_euler);
        push_move(a);
        scene_dirty = true;
    }

    void begin_grab() {
        auto& sc = project->scene;
        auto p = sc.position(selected);
        auto w = te::world_transform(sc, selected);
        if (!p) return;
        grabbing = true;
        grab_old_pos = *p;
        double u = unit();
        grab_y_offset = w.position[1] - terrain_height(float(w.position[0] / u), float(w.position[2] / u));
        pending.kind = Action::Move; pending.elem = selected;
        pending.old_pos = *p; pending.old_euler = sc.euler(selected).value_or(te::Vec3{0, 0, 0});
    }
    void end_grab() {
        grabbing = false;
        auto& sc = project->scene;
        pending.new_pos = sc.position(selected).value_or(pending.old_pos);
        pending.new_euler = sc.euler(selected).value_or(pending.old_euler);
        push_move(pending);
    }
    void cancel_grab() {
        grabbing = false;
        project->scene.set_position(selected, grab_old_pos);
        scene_dirty = true;
    }

    bool save() {
        std::string msg;
        bool ok = project->save(project->dir, &msg);
        status = msg;
        if (ok) project->height_dirty = false;
        return ok;
    }

    // ---------------------------------------------------------------------------------------------- UI
    void tree_node(int elem, int depth) {
        auto& sc = project->scene;
        auto kids = sc.children(elem);
        bool layer = sc.is_layer(elem) || !kids.empty();
        std::string label = sc.name(elem);
        if (label.empty()) label = sc.type(elem);
        char buf[256];
        std::snprintf(buf, sizeof buf, "%s  [%s%s]###n%d", label.c_str(), sc.type(elem).c_str(), kids.empty() ? "" : (", " + std::to_string(kids.size())).c_str(), elem);
        ImGuiTreeNodeFlags fl = ImGuiTreeNodeFlags_SpanAvailWidth | (layer ? 0 : ImGuiTreeNodeFlags_Leaf) | (elem == selected ? ImGuiTreeNodeFlags_Selected : 0);
        if (depth < 1) fl |= ImGuiTreeNodeFlags_DefaultOpen;
        bool open = ImGui::TreeNodeEx(buf, fl);
        if (ImGui::IsItemClicked() && !ImGui::IsItemToggledOpen()) selected = elem;
        if (open) {
            size_t shown = 0;
            for (int k : kids) {
                if (++shown > 300) { ImGui::TextDisabled("... %zu more", kids.size() - 300); break; }
                tree_node(k, depth + 1);
            }
            ImGui::TreePop();
        }
    }

    void inspector() {
        auto& sc = project->scene;
        if (selected < 0) { ImGui::TextDisabled("Nothing selected."); return; }
        ImGui::Text("%s", sc.type(selected).c_str());
        ImGui::Text("name: %s", sc.name(selected).c_str());
        if (auto k = sc.doc().get(selected, "key")) ImGui::Text("key: %s", k->c_str());
        ImGui::Text("layer: %s", sc.layer_path(selected).c_str());
        auto p = sc.position(selected);
        auto e = sc.euler(selected);
        if (p) {
            float v[3] = {float((*p)[0]), float((*p)[1]), float((*p)[2])};
            if (ImGui::DragFloat3("position (scene)", v, 0.5f)) {
                if (!pending_open) { pending_open = true; pending = Action{}; pending.kind = Action::Move; pending.elem = selected; pending.old_pos = *p; pending.old_euler = e.value_or(te::Vec3{0, 0, 0}); }
                sc.set_position(selected, {v[0], v[1], v[2]});
                scene_dirty = true;
            }
            if (pending_open && ImGui::IsItemDeactivatedAfterEdit()) {
                pending.new_pos = sc.position(selected).value_or(pending.old_pos); pending.new_euler = sc.euler(selected).value_or(pending.old_euler);
                push_move(pending); pending_open = false;
            }
        }
        if (e) {
            float v[3] = {degrees((*e)[0]), degrees((*e)[1]), degrees((*e)[2])};
            if (ImGui::DragFloat3("euler (deg)", v, 0.5f)) {
                if (!pending_open) { pending_open = true; pending = Action{}; pending.kind = Action::Move; pending.elem = selected; pending.old_pos = p.value_or(te::Vec3{0, 0, 0}); pending.old_euler = *e; }
                sc.set_euler(selected, {radians(v[0]), radians(v[1]), radians(v[2])});
                scene_dirty = true;
            }
            if (pending_open && ImGui::IsItemDeactivatedAfterEdit()) {
                pending.new_pos = sc.position(selected).value_or(pending.old_pos); pending.new_euler = sc.euler(selected).value_or(pending.old_euler);
                push_move(pending); pending_open = false;
            }
        }
        if (ImGui::Button("Grab on ground (G)")) { if (!grabbing) begin_grab(); }
        ImGui::SameLine();
        if (ImGui::Button("Drop to terrain") && p) {
            auto w = te::world_transform(sc, selected);
            double u = unit();
            Action a; a.kind = Action::Move; a.elem = selected; a.old_pos = *p; a.old_euler = e.value_or(te::Vec3{0, 0, 0});
            te::set_world_position(sc, selected, {w.position[0], double(terrain_height(float(w.position[0] / u), float(w.position[2] / u))), w.position[2]});
            a.new_pos = sc.position(selected).value_or(*p); a.new_euler = a.old_euler;
            push_move(a);
            scene_dirty = true;
        }
        if (ImGui::Button("Duplicate (Ctrl+D)")) duplicate_selected();
        ImGui::SameLine();
        if (ImGui::Button("Delete (Del)")) delete_selected();
        ImGui::SameLine();
        if (ImGui::Button("Rotate +15 (R)")) rotate_selected(angle_step_deg);
        if (ImGui::TreeNode("attributes")) {
            const auto& el = sc.doc().elements()[selected];
            for (size_t i = 0; i < el.attrs.size(); ++i)
                ImGui::TextWrapped("%s = %s", el.attrs[i].name.c_str(), std::string(sc.doc().attr_raw(selected, i)).c_str());
            ImGui::TreePop();
        }
    }

    void panels() {
        ImGuiViewport* vp = ImGui::GetMainViewport();
        if (ImGui::BeginMainMenuBar()) {
            if (ImGui::BeginMenu("File")) {
                ImGui::SetNextItemWidth(520);
                ImGui::InputText("tile folder", path_buf, sizeof path_buf);
                if (ImGui::MenuItem("Open")) { std::string err; if (!owner->open_tile(path_buf, &err)) status = err; }
                if (ImGui::MenuItem("Save", "Ctrl+S", false, project != nullptr)) save();
                ImGui::EndMenu();
            }
            if (ImGui::BeginMenu("Edit")) {
                if (ImGui::MenuItem("Undo", "Ctrl+Z", false, !undo.empty())) do_undo();
                if (ImGui::MenuItem("Redo", "Ctrl+Y", false, !redo.empty())) do_redo();
                ImGui::EndMenu();
            }
            if (ImGui::BeginMenu("View")) {
                ImGui::MenuItem("Wireframe terrain", nullptr, &show_wire);
                if (ImGui::MenuItem("Polygon outlines", nullptr, &show_polys)) scene_dirty = true;
                ImGui::EndMenu();
            }
            ImGui::Text("   %s", status.c_str());
            ImGui::EndMainMenuBar();
        }
        float top = ImGui::GetFrameHeight();
        ImGui::SetNextWindowPos(ImVec2(0, top), ImGuiCond_Always);
        ImGui::SetNextWindowSize(ImVec2(380, vp->Size.y - top), ImGuiCond_Always);
        ImGui::SetNextWindowBgAlpha(0.88f);
        ImGui::Begin("Scene", nullptr, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoCollapse);
        if (project) {
            ImGui::Text("%s", project->dir.filename().string().c_str());
            ImGui::TextDisabled("%s", project->dir.parent_path().filename().string().c_str());
            if (ImGui::CollapsingHeader("Instance types", ImGuiTreeNodeFlags_DefaultOpen)) {
                for (auto& [t, n] : type_counts) {
                    bool vis = !hidden_types.count(t);
                    char l[200];
                    std::snprintf(l, sizeof l, "%s (%d)##t", t.c_str(), n);
                    if (ImGui::Checkbox(l, &vis)) { if (vis) hidden_types.erase(t); else hidden_types.insert(t); scene_dirty = true; }
                }
            }
            if (ImGui::CollapsingHeader("Layers", ImGuiTreeNodeFlags_DefaultOpen)) {
                int g = project->scene.scene_graph();
                if (g >= 0) tree_node(g, 0);
            }
        } else ImGui::TextWrapped("No tile open.");
        ImGui::End();

        ImGui::SetNextWindowPos(ImVec2(vp->Size.x - 460, top), ImGuiCond_Always);
        ImGui::SetNextWindowSize(ImVec2(460, vp->Size.y - top), ImGuiCond_Always);
        ImGui::SetNextWindowBgAlpha(0.88f);
        ImGui::Begin("Tools", nullptr, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoCollapse);
        if (project) {
            const char* names[] = {"Select", "Raise", "Lower", "Smooth", "Flatten", "Level"};
            for (int i = 0; i < 6; ++i) { if (i) ImGui::SameLine(); if (ImGui::RadioButton(names[i], tool == Tool(i))) tool = Tool(i); }
            ImGui::SliderFloat("radius (m)", &brush_radius_m, 1.f, 120.f);
            ImGui::SliderFloat("strength", &brush_strength, 0.5f, 30.f);
            ImGui::SliderFloat("falloff", &brush_falloff, 0.f, 1.f);
            ImGui::DragFloat("flatten height (m)", &flatten_height, 0.1f);
            if (terrain_hit) ImGui::Text("cursor: %.1f, %.1f  height %.2f m", brush_world.x, brush_world.z, hit_height);
            else ImGui::TextDisabled("cursor: -");
            if (project->has_height) {
                const auto& h = *project->height;
                ImGui::Text("height map %ux%u, %.2f..%.2f m (min %.3f scale %.3f)%s", h.width(), h.height(), h.min_height(), h.min_height() + h.scale(), project->info.min_height, project->info.scale, project->info.found ? "" : "  [tile database entry NOT found]");
            } else ImGui::TextDisabled("no height_map_0.png");
            ImGui::Text("tile %u x %u cells, %.0f m", project->info.cells_x, project->info.cells_y, extent());
            ImGui::Text("tile.ted v%d, %zu instances shown%s", project->scene.project_version(), instances.size(), project->scene_modified() ? "  (modified)" : "");
            ImGui::Text("undo %zu / redo %zu%s", undo.size(), redo.size(), project->height_dirty ? "  height modified" : "");
            ImGui::Separator();
            ImGui::Checkbox("snap to grid", &snap_grid);
            ImGui::SameLine();
            ImGui::SetNextItemWidth(90);
            ImGui::DragFloat("grid (m)", &grid_m, 0.5f, 0.5f, 64.f);
            ImGui::SetNextItemWidth(90);
            ImGui::DragFloat("rotate step (deg)", &angle_step_deg, 1.f, 1.f, 180.f);
            ImGui::InputText("building key", new_key, sizeof new_key);
            ImGui::SameLine();
            if (ImGui::Button("Add")) add_building(new_key);
            ImGui::Separator();
            inspector();
        }
        ImGui::End();
    }
};

Studio::Studio(ID3D11Device* dev, ID3D11DeviceContext* ctx) : d(std::make_unique<Impl>(dev, ctx)) {
    d->owner = this;
    d->create_resources();
}
Studio::~Studio() = default;

bool Studio::open_tile(const std::string& dir, std::string* error) {
    try {
        d->project = te::TileProject::open(fs::path(dir));
    } catch (const std::exception& e) {
        if (error) *error = e.what();
        d->diag += std::string("open failed: ") + e.what() + "\n";
        return false;
    }
    d->path_input = dir;
    std::snprintf(d->path_buf, sizeof d->path_buf, "%s", dir.c_str());
    d->selected = -1;
    d->undo.clear(); d->redo.clear();
    d->terrain_dirty = d->scene_dirty = true;
    double e = d->extent();
    d->target = {float(e * 0.5), 0.f, float(e * 0.5)};
    d->dist = float(e * 1.2);
    d->status = "Opened " + dir;
    d->diag += "opened " + dir + "\n";
    return true;
}

void Studio::set_camera(float yaw, float pitch, float dist) { d->yaw = yaw; d->pitch = pitch; d->dist = dist; }
void Studio::set_surface_size(int w, int h) { if (w != d->surf_w || h != d->surf_h) d->make_depth(w, h); }
void Studio::render_scene(ID3D11RenderTargetView* rtv) { d->render(rtv); }
void Studio::frame() {
    d->panels();
    d->handle_input();
}
bool Studio::script_paint(const std::string& tool, float x, float z, int steps, std::string* error) {
    if (!d->project || !d->project->has_height) { if (error) *error = "no height map"; return false; }
    Tool t = tool == "raise" ? Tool::Raise : tool == "lower" ? Tool::Lower : tool == "smooth" ? Tool::Smooth : tool == "flatten" ? Tool::Flatten : tool == "level" ? Tool::Level : Tool::Select;
    if (t == Tool::Select) { if (error) *error = "unknown tool " + tool; return false; }
    d->tool = t;
    d->last_dt = 1.f / 60.f;
    d->project->height->begin_edit();
    for (int i = 0; i < steps; ++i) d->paint({x, 0, z});
    d->project->height->end_edit();
    Action a; a.kind = Action::Height;
    d->undo.push_back(a);
    d->redo.clear();
    return true;
}

bool Studio::script_select(const std::string& type_or_element) {
    if (!d->project) return false;
    if (d->scene_dirty) d->rebuild_scene();
    char* end = nullptr;
    long idx = std::strtol(type_or_element.c_str(), &end, 10);
    if (end && *end == 0 && !type_or_element.empty()) { d->selected = int(idx); return true; }
    for (const Instance& in : d->instances)
        if (in.type == type_or_element) { d->selected = in.elem; return true; }
    return false;
}

bool Studio::script_move(float dx, float dz) {
    if (!d->project || d->selected < 0) return false;
    auto& sc = d->project->scene;
    auto p = sc.position(d->selected);
    if (!p) return false;
    Action a;
    a.kind = Action::Move; a.elem = d->selected; a.old_pos = *p; a.old_euler = sc.euler(d->selected).value_or(te::Vec3{0, 0, 0});
    te::WorldTransform w = te::world_transform(sc, d->selected);
    double u = d->unit();
    double nx = w.position[0] + double(dx) * u, nz = w.position[2] + double(dz) * u;
    te::set_world_position(sc, d->selected, {nx, double(d->terrain_height(float(nx / u), float(nz / u))) + (w.position[1] - d->terrain_height(float(w.position[0] / u), float(w.position[2] / u))), nz});
    a.new_pos = sc.position(d->selected).value_or(*p); a.new_euler = a.old_euler;
    d->push_move(a);
    d->scene_dirty = true;
    return true;
}

bool Studio::script_duplicate() { if (!d->project || d->selected < 0) return false; d->duplicate_selected(); return true; }
bool Studio::script_delete() { if (!d->project || d->selected < 0) return false; d->delete_selected(); return true; }
bool Studio::script_rotate(float degrees) { if (!d->project || d->selected < 0) return false; d->rotate_selected(degrees); return true; }
bool Studio::script_add(const std::string& key) { if (!d->project) return false; d->add_building(key.c_str()); return true; }
void Studio::script_undo(int n) { for (int i = 0; i < n; ++i) d->do_undo(); }
void Studio::script_redo(int n) { for (int i = 0; i < n; ++i) d->do_redo(); }

bool Studio::save_to(const std::string& dir, std::string* message) {
    if (!d->project) return false;
    bool ok = d->project->save(fs::path(dir), message);
    if (ok) d->project->height_dirty = false;
    return ok;
}

bool Studio::confirm_close() { return true; }
std::string Studio::diagnostics() const {
    char b[512];
    std::snprintf(b, sizeof b, "instances %zu, lines %zu, terrain indices %u\n", d->instances.size(), d->lines.size() / 2, d->terrain_index_count);
    return d->diag + b + d->status + "\n";
}

} // namespace ts
