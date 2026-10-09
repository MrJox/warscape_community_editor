#pragma once
// Tile Studio: viewer / editor for one battle or campaign tile (tile.ted scene, height map).
//
// Render path (own, small): terrain mesh from the height field, instanced boxes for scene instances, line batches for
// polygons and selection. Input: RMB orbit, MMB pan, wheel zoom, LMB select / paint, G grab selected instance on the
// ground, Ctrl+Z / Ctrl+Y undo / redo, Ctrl+S save. World space: x, z in metres (scene units / unit scale), y in metres.
#include "project/tile_project.h"
#include "scene/world_transform.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <functional>
#include <memory>
#include <set>
#include <string>
#include <vector>

namespace ts {

using Microsoft::WRL::ComPtr;

struct Float3 { float x = 0, y = 0, z = 0; };

enum class Tool { Select, Raise, Lower, Smooth, Flatten, Level };

struct Instance {
    int elem = 0;
    Float3 pos;                // world metres
    float size = 2;
    uint32_t color = 0xffffffff;
    std::string type;
};

class Studio {
public:
    Studio(ID3D11Device* dev, ID3D11DeviceContext* ctx);
    ~Studio();

    bool open_tile(const std::string& dir, std::string* error = nullptr);
    void set_camera(float yaw, float pitch, float dist);
    void set_surface_size(int w, int h);                       // swap chain size (creates the depth buffer)
    void render_scene(ID3D11RenderTargetView* rtv);            // 3D view into the back buffer
    void frame();                                              // ImGui panels + input handling
    // scripting for tests and batch edits (same code paths as the mouse tools)
    bool script_paint(const std::string& tool, float x, float z, int steps, std::string* error = nullptr);  // x, z in metres
    bool script_select(const std::string& type_or_element);                                                  // first instance of a type, or an element index
    bool script_move(float dx, float dz);                                                                    // selected instance, metres on the ground plane
    bool script_duplicate();
    bool script_delete();
    bool script_rotate(float degrees);                                                                       // selected instance, about world Y
    bool script_add(const std::string& building_key);
    void script_undo(int n);
    void script_redo(int n);
    bool save_to(const std::string& dir, std::string* message);
    bool confirm_close();
    std::string diagnostics() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d;
};

} // namespace ts
