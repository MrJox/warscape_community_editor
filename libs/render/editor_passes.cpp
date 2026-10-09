// Editor-side passes of the renderer: the overlay pass (scene overlays, grid, selection outlines) and entity picking. These are not game
// shaders: a minimal vertex-colour shader and an id shader, compiled from the strings below.
#include "render/renderer.h"

#include <d3dcompiler.h>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <stdexcept>

using Microsoft::WRL::ComPtr;

namespace wce {

namespace {

const char* kEditorHlsl = R"(
cbuffer Frame : register(b0) { row_major float4x4 vp; uint id; };

struct OvlIn  { float3 pos : POSITION; float4 col : COLOR; };
struct OvlOut { float4 pos : SV_Position; float4 col : COLOR; };
OvlOut ovl_vs(OvlIn i) { OvlOut o; o.pos = mul(vp, float4(i.pos, 1)); o.col = i.col; return o; }
float4 ovl_ps(OvlOut i) : SV_Target { return i.col; }

// Same vertex and instance layout as the game's rigid shaders: half4 position, rows of the 3x4 world matrix in POSITION1..3.
struct PickIn { float4 pos : POSITION0; float4 w0 : POSITION1; float4 w1 : POSITION2; float4 w2 : POSITION3; };
float4 pick_vs(PickIn i) : SV_Position {
    float4 p = float4(i.pos.xyz, 1);
    return mul(vp, float4(dot(i.w0, p), dot(i.w1, p), dot(i.w2, p), 1));
}
uint pick_ps(float4 p : SV_Position) : SV_Target { return id; }
)";

ComPtr<ID3DBlob> compile_editor(const char* entry, const char* target) {
    ComPtr<ID3DBlob> code, err;
    if (FAILED(D3DCompile(kEditorHlsl, std::strlen(kEditorHlsl), "editor_passes", nullptr, nullptr, entry, target, 0, 0, &code, &err)))
        throw std::runtime_error(std::string("editor shader: ") + (err ? static_cast<const char*>(err->GetBufferPointer()) : entry));
    return code;
}

struct FrameCb {
    float vp[16];
    uint32_t id;
    uint32_t pad[3];
};

uint32_t with_alpha(uint32_t rgba, float a) {
    const uint32_t alpha = uint32_t(std::clamp(a, 0.0f, 1.0f) * 255.0f + 0.5f);
    return (rgba & 0x00ffffffu) | (alpha << 24);
}

} // namespace

void Renderer::create_editor_resources() {
    if (ovl_vs_) return;
    auto vs = compile_editor("ovl_vs", "vs_5_0");
    dev_->CreateVertexShader(vs->GetBufferPointer(), vs->GetBufferSize(), nullptr, &ovl_vs_);
    const D3D11_INPUT_ELEMENT_DESC oel[] = {{"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0},
                                            {"COLOR", 0, DXGI_FORMAT_R8G8B8A8_UNORM, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0}};
    dev_->CreateInputLayout(oel, 2, vs->GetBufferPointer(), vs->GetBufferSize(), &ovl_il_);
    auto ps = compile_editor("ovl_ps", "ps_5_0");
    dev_->CreatePixelShader(ps->GetBufferPointer(), ps->GetBufferSize(), nullptr, &ovl_ps_);

    auto pvs = compile_editor("pick_vs", "vs_5_0");
    dev_->CreateVertexShader(pvs->GetBufferPointer(), pvs->GetBufferSize(), nullptr, &pick_vs_);
    std::vector<D3D11_INPUT_ELEMENT_DESC> pel = {{"POSITION", 0, DXGI_FORMAT_R16G16B16A16_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0}};
    for (UINT i = 1; i <= 3; ++i) pel.push_back({"POSITION", i, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, (i - 1) * 16, D3D11_INPUT_PER_INSTANCE_DATA, 1});
    HRESULT hr_pil = dev_->CreateInputLayout(pel.data(), UINT(pel.size()), pvs->GetBufferPointer(), pvs->GetBufferSize(), &pick_il_);
    if (FAILED(hr_pil)) errors_.push_back("pick input layout failed");
    auto pps = compile_editor("pick_ps", "ps_5_0");
    dev_->CreatePixelShader(pps->GetBufferPointer(), pps->GetBufferSize(), nullptr, &pick_ps_);

    D3D11_BUFFER_DESC cb{sizeof(FrameCb), D3D11_USAGE_DYNAMIC, D3D11_BIND_CONSTANT_BUFFER, D3D11_CPU_ACCESS_WRITE};
    dev_->CreateBuffer(&cb, nullptr, &ovl_cb_);
    dev_->CreateBuffer(&cb, nullptr, &pick_cb_);

    D3D11_BLEND_DESC bd{};
    bd.RenderTarget[0].BlendEnable = TRUE;
    bd.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    bd.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    bd.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    bd.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ZERO;
    bd.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    bd.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    bd.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev_->CreateBlendState(&bd, &ovl_blend_);

    D3D11_DEPTH_STENCIL_DESC dd{};
    dd.DepthEnable = TRUE;
    dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    dd.DepthFunc = D3D11_COMPARISON_LESS_EQUAL;
    dev_->CreateDepthStencilState(&dd, &ovl_ds_test_);
    dd.DepthEnable = FALSE;
    dev_->CreateDepthStencilState(&dd, &ovl_ds_off_);
    dd.DepthEnable = TRUE;
    dd.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
    dd.DepthFunc = D3D11_COMPARISON_LESS;
    dev_->CreateDepthStencilState(&dd, &pick_ds_);

    D3D11_RASTERIZER_DESC rd{};
    rd.FillMode = D3D11_FILL_SOLID;
    rd.CullMode = D3D11_CULL_NONE;
    rd.DepthClipEnable = TRUE;
    dev_->CreateRasterizerState(&rd, &ovl_rs_);
}

float4x4 Renderer::view_projection(const Camera& cam, int w, int h) {
    return perspective_lh(cam.fov_y, float(w) / float(h), cam.znear, cam.zfar) * look_at_lh(cam.eye, cam.target, {0, 1, 0});
}

bool Renderer::project(const Camera& cam, int w, int h, const float3& p, float& px, float& py) {
    const float4x4 m = view_projection(cam, w, h);
    const float cw = m.m[3][0] * p.x + m.m[3][1] * p.y + m.m[3][2] * p.z + m.m[3][3];
    if (cw <= 1e-6f) return false;
    const float cx = (m.m[0][0] * p.x + m.m[0][1] * p.y + m.m[0][2] * p.z + m.m[0][3]) / cw;
    const float cy = (m.m[1][0] * p.x + m.m[1][1] * p.y + m.m[1][2] * p.z + m.m[1][3]) / cw;
    px = (cx * 0.5f + 0.5f) * float(w);
    py = (0.5f - cy * 0.5f) * float(h);
    return true;
}

void Renderer::pass_overlay(const RenderScene& scene, RenderTarget& target) {
    create_editor_resources();
    // Four batches: lines / triangles, with and without the depth test.
    std::vector<OverlayVertex> lines_d, lines_n, tris_d, tris_n;
    for (const Entity& e : scene.entities()) {
        const Overlay* o = e.get<Overlay>();
        if (!o || !scene.drawable(e)) continue;
        auto& lines = o->depth_test ? lines_d : lines_n;
        auto& tris = o->depth_test ? tris_d : tris_n;
        for (const OverlayVertex& v : o->lines) lines.push_back({e.world.transform_point(v.p), v.rgba});
        for (const OverlayVertex& v : o->triangles) tris.push_back({e.world.transform_point(v.p), v.rgba});
    }
    if (show_grid) {
        const int n = int(grid_half_extent / grid_step);
        for (int i = -n; i <= n; ++i) {
            const float t = float(i) * grid_step;
            const uint32_t c = i == 0 ? overlay_colour(0.9f, 0.3f, 0.3f, 0.8f) : overlay_colour(0.7f, 0.7f, 0.7f, 0.25f);
            const uint32_t c2 = i == 0 ? overlay_colour(0.3f, 0.4f, 0.95f, 0.8f) : overlay_colour(0.7f, 0.7f, 0.7f, 0.25f);
            lines_d.push_back({{-grid_half_extent, 0, t}, c});
            lines_d.push_back({{grid_half_extent, 0, t}, c});
            lines_d.push_back({{t, 0, -grid_half_extent}, c2});
            lines_d.push_back({{t, 0, grid_half_extent}, c2});
        }
    }
    if (!selection.empty()) {
        Overlay sel;
        const uint32_t yellow = overlay_colour(1.0f, 0.85f, 0.2f, 1.0f);
        for (EntityId id : selection) {
            const Entity* e = scene.find(id);
            if (!e || !scene.drawable(*e)) continue;
            if (e->get<MeshInstance>() || e->get<Terrain>()) {
                for (const Draw& d : draws_)
                    if (d.entity == id) sel.box_wire(d.world, d.model->bmin, d.model->bmax, yellow);
            } else if (const Overlay* o = e->get<Overlay>()) {
                float3 lo{1e30f, 1e30f, 1e30f}, hi{-1e30f, -1e30f, -1e30f};
                for (const auto* vs : {&o->lines, &o->triangles})
                    for (const OverlayVertex& v : *vs) {
                        lo = {std::min(lo.x, v.p.x), std::min(lo.y, v.p.y), std::min(lo.z, v.p.z)};
                        hi = {std::max(hi.x, v.p.x), std::max(hi.y, v.p.y), std::max(hi.z, v.p.z)};
                    }
                if (lo.x <= hi.x) sel.box_wire(e->world, lo, hi, yellow);
            }
        }
        lines_n.insert(lines_n.end(), sel.lines.begin(), sel.lines.end());
    }
    const size_t total = lines_d.size() + lines_n.size() + tris_d.size() + tris_n.size();
    if (!total) return;

    if (ovl_vb_capacity_ < total) {
        ovl_vb_capacity_ = UINT(std::max<size_t>(total * 2, 4096));
        ovl_vb_.Reset();
        D3D11_BUFFER_DESC bd{UINT(ovl_vb_capacity_ * sizeof(OverlayVertex)), D3D11_USAGE_DYNAMIC, D3D11_BIND_VERTEX_BUFFER, D3D11_CPU_ACCESS_WRITE};
        dev_->CreateBuffer(&bd, nullptr, &ovl_vb_);
    }
    D3D11_MAPPED_SUBRESOURCE ms;
    if (FAILED(ctx_->Map(ovl_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) return;
    OverlayVertex* dst = static_cast<OverlayVertex*>(ms.pData);
    UINT first[4];
    const std::vector<OverlayVertex>* batches[4] = {&tris_d, &lines_d, &tris_n, &lines_n};
    UINT at = 0;
    for (int b = 0; b < 4; ++b) {
        first[b] = at;
        if (!batches[b]->empty()) std::memcpy(dst + at, batches[b]->data(), batches[b]->size() * sizeof(OverlayVertex));
        at += UINT(batches[b]->size());
    }
    ctx_->Unmap(ovl_vb_.Get(), 0);

    if (SUCCEEDED(ctx_->Map(ovl_cb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        FrameCb cb{};
        std::memcpy(cb.vp, view_proj_.data(), sizeof cb.vp);
        std::memcpy(ms.pData, &cb, sizeof cb);
        ctx_->Unmap(ovl_cb_.Get(), 0);
    }
    ID3D11RenderTargetView* rtv = target.rtv();
    ctx_->OMSetRenderTargets(1, &rtv, dsv_.Get());
    D3D11_VIEWPORT vp{0, 0, float(target.width()), float(target.height()), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->OMSetBlendState(ovl_blend_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(ovl_rs_.Get());
    ctx_->IASetInputLayout(ovl_il_.Get());
    UINT stride = sizeof(OverlayVertex), off = 0;
    ctx_->IASetVertexBuffers(0, 1, ovl_vb_.GetAddressOf(), &stride, &off);
    ctx_->VSSetShader(ovl_vs_.Get(), nullptr, 0);
    ctx_->VSSetConstantBuffers(0, 1, ovl_cb_.GetAddressOf());
    ctx_->PSSetShader(ovl_ps_.Get(), nullptr, 0);
    const D3D11_PRIMITIVE_TOPOLOGY topo[4] = {D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_PRIMITIVE_TOPOLOGY_LINELIST,
                                              D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D11_PRIMITIVE_TOPOLOGY_LINELIST};
    for (int b = 0; b < 4; ++b) {
        if (batches[b]->empty()) continue;
        ctx_->OMSetDepthStencilState(b < 2 ? ovl_ds_test_.Get() : ovl_ds_off_.Get(), 0);
        ctx_->IASetPrimitiveTopology(topo[b]);
        ctx_->Draw(UINT(batches[b]->size()), first[b]);
    }
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
}

EntityId Renderer::pick(const RenderScene& scene, const ViewParams& params, const RenderTarget& target, int x, int y) {
    const int w = target.width(), h = target.height();
    if (x < 0 || y < 0 || x >= w || y >= h) return 0;
    create_editor_resources();
    build_draws(scene);
    const float4x4 vp_m = view_projection(params.camera, w, h);

    // 1. Overlay entities in screen space: the nearest line within 6 px, or a triangle that contains the pixel.
    EntityId overlay_hit = 0;
    float best = 6.0f;
    const float fx = float(x) + 0.5f, fy = float(y) + 0.5f;
    for (const Entity& e : scene.entities()) {
        const Overlay* o = e.get<Overlay>();
        if (!o || !o->pickable || !scene.pickable(e)) continue;
        auto screen = [&](const OverlayVertex& v, float& sx, float& sy) { return project(params.camera, w, h, e.world.transform_point(v.p), sx, sy); };
        for (size_t i = 0; i + 1 < o->lines.size(); i += 2) {
            float ax, ay, bx, by;
            if (!screen(o->lines[i], ax, ay) || !screen(o->lines[i + 1], bx, by)) continue;
            const float dx = bx - ax, dy = by - ay, len2 = dx * dx + dy * dy;
            float t = len2 > 0 ? ((fx - ax) * dx + (fy - ay) * dy) / len2 : 0;
            t = std::clamp(t, 0.0f, 1.0f);
            const float d = std::hypot(fx - (ax + t * dx), fy - (ay + t * dy));
            if (d < best) { best = d; overlay_hit = e.id; }
        }
        for (size_t i = 0; i + 2 < o->triangles.size(); i += 3) {
            float px[3], py[3];
            if (!screen(o->triangles[i], px[0], py[0]) || !screen(o->triangles[i + 1], px[1], py[1]) || !screen(o->triangles[i + 2], px[2], py[2])) continue;
            auto edge = [&](int a, int b) { return (fx - px[a]) * (py[b] - py[a]) - (fy - py[a]) * (px[b] - px[a]); };
            const float e0 = edge(0, 1), e1 = edge(1, 2), e2 = edge(2, 0);
            if ((e0 >= 0 && e1 >= 0 && e2 >= 0) || (e0 <= 0 && e1 <= 0 && e2 <= 0)) { best = 0; overlay_hit = e.id; }
        }
    }
    if (overlay_hit) return overlay_hit;

    // 2. Meshes and terrain: an id pass at the target's size, read back at one pixel.
    if (pick_w_ != w || pick_h_ != h) {
        pick_id_tex_.Reset();
        pick_depth_tex_.Reset();
        pick_id_rtv_.Reset();
        pick_dsv_.Reset();
        D3D11_TEXTURE2D_DESC td{UINT(w), UINT(h), 1, 1, DXGI_FORMAT_R32_UINT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET};
        dev_->CreateTexture2D(&td, nullptr, &pick_id_tex_);
        dev_->CreateRenderTargetView(pick_id_tex_.Get(), nullptr, &pick_id_rtv_);
        D3D11_TEXTURE2D_DESC dt{UINT(w), UINT(h), 1, 1, DXGI_FORMAT_D32_FLOAT, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_DEPTH_STENCIL};
        dev_->CreateTexture2D(&dt, nullptr, &pick_depth_tex_);
        dev_->CreateDepthStencilView(pick_depth_tex_.Get(), nullptr, &pick_dsv_);
        D3D11_TEXTURE2D_DESC st{1, 1, 1, 1, DXGI_FORMAT_R32_UINT, {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ};
        pick_staging_.Reset();
        dev_->CreateTexture2D(&st, nullptr, &pick_staging_);
        pick_w_ = w;
        pick_h_ = h;
    }
    if (!pick_id_rtv_ || !pick_dsv_ || !pick_staging_) return 0;
    const UINT zero[4] = {0, 0, 0, 0};
    ctx_->ClearRenderTargetView(pick_id_rtv_.Get(), reinterpret_cast<const float*>(zero));   // unsigned integer target: the bits are the value
    ctx_->ClearDepthStencilView(pick_dsv_.Get(), D3D11_CLEAR_DEPTH, 1.0f, 0);
    ctx_->OMSetRenderTargets(1, pick_id_rtv_.GetAddressOf(), pick_dsv_.Get());
    D3D11_VIEWPORT vp{0, 0, float(w), float(h), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    ctx_->OMSetDepthStencilState(pick_ds_.Get(), 0);
    ctx_->OMSetBlendState(bs_opaque_.Get(), nullptr, 0xffffffff);
    ctx_->RSSetState(ovl_rs_.Get());
    ctx_->IASetInputLayout(pick_il_.Get());
    ctx_->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx_->VSSetShader(pick_vs_.Get(), nullptr, 0);
    ctx_->VSSetConstantBuffers(0, 1, pick_cb_.GetAddressOf());
    ctx_->PSSetConstantBuffers(0, 1, pick_cb_.GetAddressOf());   // the pixel shader writes `id`
    ctx_->PSSetShader(pick_ps_.Get(), nullptr, 0);
    for (const Draw& d : draws_) {
        const Entity* e = scene.find(d.entity);
        if (!e || !scene.pickable(*e)) continue;
        D3D11_MAPPED_SUBRESOURCE ms;
        if (SUCCEEDED(ctx_->Map(pick_cb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
            FrameCb cb{};
            std::memcpy(cb.vp, vp_m.data(), sizeof cb.vp);
            cb.id = d.entity;
            std::memcpy(ms.pData, &cb, sizeof cb);
            ctx_->Unmap(pick_cb_.Get(), 0);
        }
        for (const GpuMesh& m : d.model->meshes) {
            RigidInstance inst = rigid_instance(d, m);
            if (FAILED(ctx_->Map(instance_vb_.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) continue;
            std::memcpy(ms.pData, &inst, sizeof inst);
            ctx_->Unmap(instance_vb_.Get(), 0);
            ID3D11Buffer* vbs[2] = {m.vb.Get(), instance_vb_.Get()};
            UINT strides[2] = {32, sizeof(RigidInstance)}, offs[2] = {0, 0};
            ctx_->IASetVertexBuffers(0, 2, vbs, strides, offs);
            ctx_->IASetIndexBuffer(m.ib.Get(), DXGI_FORMAT_R16_UINT, 0);
            ctx_->DrawIndexedInstanced(m.index_count, 1, 0, 0, 0);
        }
    }
    ctx_->OMSetRenderTargets(0, nullptr, nullptr);
    const D3D11_BOX box{UINT(x), UINT(y), 0, UINT(x + 1), UINT(y + 1), 1};
    ctx_->CopySubresourceRegion(pick_staging_.Get(), 0, 0, 0, 0, pick_id_tex_.Get(), 0, &box);
    D3D11_MAPPED_SUBRESOURCE rd;
    if (FAILED(ctx_->Map(pick_staging_.Get(), 0, D3D11_MAP_READ, 0, &rd))) return 0;
    const EntityId id = *static_cast<const uint32_t*>(rd.pData);
    ctx_->Unmap(pick_staging_.Get(), 0);
    return id;
}

} // namespace wce
