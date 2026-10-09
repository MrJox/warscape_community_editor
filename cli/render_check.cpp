// render_check: the renderer without a window. Builds RenderScenes, renders them offscreen with a fixed-step clock and checks what the
// editor depends on: determinism, entity picking (meshes, terrain, overlays, locked and hidden layers), the interim terrain pass, the
// overlay pass (colour, depth test, selection outline, grid) and PNG capture.
//   render_check [environment path in the packs]
#include "formats/png_file.h"
#include "formats/xml_doc.h"
#include "foundation/clock.h"
#include "foundation/text_encoding.h"
#include "render/renderer.h"
#include "scene/render_scene.h"
#include "vfs/vfs.h"
#include "weather/sample_map.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>

using namespace wce;
using Microsoft::WRL::ComPtr;

static int failures = 0;
#define CHECK(cond, msg) do { if (!(cond)) { ++failures; std::printf("FAIL: %s\n", msg); } } while (0)

namespace {

constexpr int kW = 640, kH = 360;

struct Gpu {
    ComPtr<ID3D11Device> dev;
    ComPtr<ID3D11DeviceContext> ctx;
};

bool make_device(Gpu& g) {
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0, got{};
    const char* dbg = std::getenv("WCE_D3D_DEBUG");   // needs the Windows "Graphics Tools" feature
    return SUCCEEDED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, dbg && *dbg == '1' ? D3D11_CREATE_DEVICE_DEBUG : 0, &fl, 1, D3D11_SDK_VERSION, &g.dev, &got, &g.ctx));
}

Camera orbit(float yaw, float pitch, float dist, float3 target) {
    Camera c;
    const float cp = std::cos(pitch), sp = std::sin(pitch);
    c.target = target;
    c.eye = target + float3{std::sin(yaw) * cp, sp, -std::cos(yaw) * cp} * dist;
    return c;
}

// Renders `frames` fixed-step frames; the last one stays in `target`.
void render_frames(Renderer& r, RenderScene& scene, const Camera& cam, RenderTarget& target, int frames) {
    Clock clock = Clock::fixed(1.0 / 60.0);
    r.deterministic = true;
    for (int i = 0; i < frames; ++i) {
        clock.tick();
        r.render(scene, ViewParams{cam, float(clock.time())}, target);
    }
}

struct Pixels {
    std::vector<uint8_t> rgba;
    int w = 0, h = 0;
    const uint8_t* at(int x, int y) const { return &rgba[(size_t(y) * size_t(w) + size_t(x)) * 4]; }
    // pixels within `tol` of the colour (r, g, b) in the box around (cx, cy)
    int count_near(int cx, int cy, int radius, int r, int g, int b, int tol = 3) const {
        int n = 0;
        for (int y = std::max(0, cy - radius); y <= std::min(h - 1, cy + radius); ++y)
            for (int x = std::max(0, cx - radius); x <= std::min(w - 1, cx + radius); ++x) {
                const uint8_t* p = at(x, y);
                if (std::abs(p[0] - r) <= tol && std::abs(p[1] - g) <= tol && std::abs(p[2] - b) <= tol) ++n;
            }
        return n;
    }
    int count_near_all(int r, int g, int b, int tol = 3) const { return count_near(w / 2, h / 2, std::max(w, h), r, g, b, tol); }
};

Pixels read(Gpu& g, const RenderTarget& t) {
    Pixels p;
    p.w = t.width();
    p.h = t.height();
    t.read_rgba8(g.dev.Get(), g.ctx.Get(), p.rgba);
    return p;
}

} // namespace

int main(int argc, char** argv) {
    const std::string env_path = argc > 1 ? argv[1] : "weather/charlemagne/med_sp_land_day_dry.environment";
    try {
        Gpu gpu;
        if (!make_device(gpu)) { std::printf("no Direct3D 11 device\n"); return 2; }
        Vfs vfs(Vfs::default_config());
        vfs.mount();
        XmlDoc env_doc = XmlDoc::parse(vfs.read(env_path));
        EnvironmentParams env = read_environment(env_doc);

        // ---- scene A: the sample map (flat ground and houses)
        {
            RenderScene scene;
            const LayerId layer = scene.add_layer(1, "Sample map");
            SampleMapResult sm = publish_sample_map(vfs, scene, layer, 1, 1, 8);
            scene.environment.active = env;
            CHECK(sm.buildings > 0, "the sample map placed buildings");
            const Camera cam = orbit(0.6f, 0.35f, 120.0f, {0, 5, 0});

            Renderer r1(gpu.dev.Get(), gpu.ctx.Get(), vfs), r2(gpu.dev.Get(), gpu.ctx.Get(), vfs);
            RenderTarget t1(gpu.dev.Get(), kW, kH), t2(gpu.dev.Get(), kW, kH);
            render_frames(r1, scene, cam, t1, 4);
            render_frames(r2, scene, cam, t2, 4);
            Pixels a = read(gpu, t1), b = read(gpu, t2);
            CHECK(!a.rgba.empty() && a.rgba == b.rgba, "two renderers give the same pixels for the same fixed-step frames");
            CHECK(r1.draw_count() == scene.entities().size(), "every mesh and terrain entity has a draw");

            // picking: sky misses, ground hits the terrain entity, house centres hit their house
            EntityId terrain_id = 0;
            for (const Entity& e : scene.entities())
                if (e.get<Terrain>()) terrain_id = e.id;
            ViewParams vp{cam, 0};
            CHECK(r1.pick(scene, vp, t1, 10, 2) == 0, "picking the sky finds nothing");
            int ground_hits = 0;
            for (int x : {kW - 8, kW - 30, kW - 60})
                if (r1.pick(scene, vp, t1, x, kH - 8) == terrain_id) ++ground_hits;
            CHECK(ground_hits >= 2, "picking the ground at the bottom of the view finds the terrain entity");
            int house_hits = 0, houses_in_view = 0;
            for (int item = 1; item <= sm.buildings; ++item) {
                float3 lo{1e30f, 1e30f, 1e30f}, hi{-1e30f, -1e30f, -1e30f};
                for (const Entity& e : scene.entities()) {
                    const MeshInstance* mi = e.get<MeshInstance>();
                    if (!mi || e.source.item != uint64_t(item)) continue;
                    const GpuModel* m = r1.models().model(mi->model);
                    if (!m) continue;
                    for (int k = 0; k < 8; ++k) {
                        const float3 c{(k & 1) ? m->bmax.x : m->bmin.x, (k & 2) ? m->bmax.y : m->bmin.y, (k & 4) ? m->bmax.z : m->bmin.z};
                        const float3 w = e.world.transform_point(c);
                        lo = {std::min(lo.x, w.x), std::min(lo.y, w.y), std::min(lo.z, w.z)};
                        hi = {std::max(hi.x, w.x), std::max(hi.y, w.y), std::max(hi.z, w.z)};
                    }
                }
                if (lo.x > hi.x) continue;
                float px, py;
                if (!Renderer::project(cam, kW, kH, (lo + hi) * 0.5f, px, py) || px < 4 || py < 4 || px > kW - 4 || py > kH - 4) continue;
                ++houses_in_view;
                const EntityId id = r1.pick(scene, vp, t1, int(px), int(py));
                const Entity* e = scene.find(id);
                if (e && e->get<MeshInstance>() && e->source.item == uint64_t(item)) ++house_hits;
            }
            std::printf("houses in view %d, picked right %d\n", houses_in_view, house_hits);
            // the centre of a bounding box can be empty air (an arch) or hidden behind another house, so only a share has to match
            CHECK(houses_in_view > 0 && house_hits >= 2 && house_hits * 3 >= houses_in_view, "picking the centre of a house often finds a piece of that house");

            // a locked layer is not pickable, a hidden one is not drawn
            scene.layer(layer)->locked = true;
            CHECK(r1.pick(scene, vp, t1, kW - 8, kH - 8) == 0, "a locked layer is not picked");
            scene.layer(layer)->locked = false;
            scene.set_layer_visible(layer, false);
            render_frames(r1, scene, cam, t1, 1);
            CHECK(r1.draw_count() == 0, "a hidden layer has no draws");
            CHECK(r1.pick(scene, vp, t1, kW - 8, kH - 8) == 0, "a hidden layer is not picked");
            scene.set_layer_visible(layer, true);
        }

        // ---- scene B: a hill (3 x 3 terrain patches at most) with overlays on and inside it
        {
            RenderScene scene;
            const LayerId ground = scene.add_layer(1, "Hill");
            Terrain hill;
            hill.nx = hill.nz = 129;
            hill.step = 4;
            hill.origin_x = hill.origin_z = -256;
            hill.heights.resize(size_t(hill.nx) * hill.nz);
            for (int j = 0; j < hill.nz; ++j)
                for (int i = 0; i < hill.nx; ++i) {
                    const float x = hill.origin_x + float(i) * hill.step, z = hill.origin_z + float(j) * hill.step;
                    hill.heights[size_t(j) * hill.nx + i] = 40.0f * std::exp(-(x * x + z * z) / (2 * 80.0f * 80.0f));
                }
            const EntityId hill_id = scene.add(ground, float4x4{}, hill, SourceRef{1, 0});
            const LayerId marks = scene.add_layer(2, "Overlays");
            Overlay up;                                     // above the peak: always visible
            up.line({0, 41, 0}, {0, 110, 0}, overlay_colour(1, 0, 1));
            const EntityId up_id = scene.add(marks, float4x4{}, up, SourceRef{2, 1});
            Overlay buried;                                 // inside the hill: hidden by the depth test, visible without it
            buried.line({0, 2, 0}, {0, 30, 0}, overlay_colour(0, 1, 0));
            const EntityId buried_id = scene.add(marks, float4x4{}, buried, SourceRef{2, 2});
            scene.environment.active = env;
            const Camera cam = orbit(0.6f, 0.5f, 220.0f, {0, 25, 0});
            ViewParams vp{cam, 0};

            Renderer r(gpu.dev.Get(), gpu.ctx.Get(), vfs);
            RenderTarget t(gpu.dev.Get(), kW, kH);
            render_frames(r, scene, cam, t, 3);
            Pixels base = read(gpu, t);

            float px, py;
            CHECK(Renderer::project(cam, kW, kH, {0, 76, 0}, px, py), "the overlay is in front of the camera");
            const int mx = int(px), my = int(py);
            CHECK(base.count_near(mx, my, 2, 255, 0, 255, 2) > 0, "an overlay line is drawn in its own colour (no tone mapping)");
            float bx, by;
            Renderer::project(cam, kW, kH, {0, 16, 0}, bx, by);
            const int green_hidden = base.count_near(int(bx), int(by), 3, 0, 255, 0, 2);
            CHECK(green_hidden == 0, "an overlay line inside the hill is hidden by the depth test");

            // the terrain mesh: its peak is where the heights say (the id pass sees the surface, not the overlay)
            float tx, ty;
            Renderer::project(cam, kW, kH, {-60, hill.heights[size_t(64) * hill.nx + (64 - 15)], 0}, tx, ty);   // 60 m left of the peak on the surface
            CHECK(r.pick(scene, vp, t, int(tx), int(ty)) == hill_id, "picking the hill surface finds the terrain entity");
            CHECK(r.pick(scene, vp, t, mx, my) == up_id, "an overlay line is picked in screen space and wins over what is behind it");
            CHECK(r.pick(scene, vp, t, 3, 3) == 0, "picking empty sky finds nothing");

            // selection outline, grid, depth-test off
            r.selection = {hill_id};
            render_frames(r, scene, cam, t, 1);
            CHECK(read(gpu, t).count_near_all(255, 217, 51, 4) > 20, "a selected entity is outlined");
            r.selection.clear();
            r.show_grid = true;
            render_frames(r, scene, cam, t, 1);
            Pixels with_grid = read(gpu, t);
            CHECK(with_grid.rgba != base.rgba, "the grid changes the image");
            r.show_grid = false;
            Overlay* b = scene.find(buried_id)->get<Overlay>();
            b->depth_test = false;
            scene.touch();
            render_frames(r, scene, cam, t, 1);
            CHECK(read(gpu, t).count_near(int(bx), int(by), 3, 0, 255, 0, 2) > 0, "without the depth test the buried line shows");

            // capture: the PNG decodes to what the target holds
            const std::filesystem::path png = std::filesystem::temp_directory_path() / "wce_render_check.png";
            CHECK(t.save_png(gpu.dev.Get(), gpu.ctx.Get(), png.wstring()), "save_png");
            auto bytes = read_file(png.string());
            PngFile decoded = PngFile::parse(bytes);
            CHECK(decoded.width == uint32_t(kW) && decoded.height == uint32_t(kH), "the PNG has the target's size");
            Pixels now = read(gpu, t);
            bool same = decoded.color_type == 2 && decoded.pixels.size() == size_t(kW) * kH * 3;
            for (size_t i = 0; same && i < size_t(kW) * kH; ++i)
                same = decoded.pixels[i * 3] == now.rgba[i * 4] && decoded.pixels[i * 3 + 1] == now.rgba[i * 4 + 1] && decoded.pixels[i * 3 + 2] == now.rgba[i * 4 + 2];
            CHECK(same, "the PNG holds the target's pixels");
            std::filesystem::remove(png);
            for (auto& e : r.errors()) std::printf("renderer: %s\n", e.c_str());
        }
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
