// The tile tool's commands: everything the panels and the mouse do (the old Tile Studio script flags), plus test commands (assert_*).
//   Studio flag        command
//   --tile <folder>    tile <folder>
//   --paint k x z n    paint k x z n
//   --select t|n       tile_select t|n
//   --move dx dz       tile_move dx dz
//   --duplicate        duplicate            --delete    delete_item        --rotate d   rotate d      --add key   add_building key
//   --add-polygon s    add_polygon s        --polygon-point op i x z   polygon_point op i x z
//   --new-layer n      new_layer n          --move-to-layer n          move_to_layer n
//   --save-to dir      save_to dir          --undo n / --redo n        undo / redo (the editor's, one step each)
// app/main.cpp maps the old flag names to these commands.
#include "editor/editor.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "tile/tile_tool.h"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <fstream>

namespace fs = std::filesystem;

namespace wce {

struct TileCommands {};

namespace {

bool number(const std::string& s, float& out) {
    char* end = nullptr;
    out = std::strtof(s.c_str(), &end);
    return end && end != s.c_str() && *end == 0;
}

std::vector<std::string> split(const std::string& s, char sep) {
    std::vector<std::string> out;
    size_t at = 0;
    while (at <= s.size()) {
        size_t e = s.find(sep, at);
        if (e == std::string::npos) e = s.size();
        out.push_back(s.substr(at, e - at));
        at = e + 1;
    }
    return out;
}

} // namespace

void TileTool::register_commands(Editor& editor, CommandRegistry& commands) {
    Editor* ed = &editor;
    auto ok = [](std::string m = {}) { return CommandResult::success(std::move(m)); };
    auto fail = [](std::string m) { return CommandResult::fail(std::move(m)); };
    // Every command works on the active tile and says so when there is none.
    auto need = [this, ed](TileDocument*& d) { d = document(*ed); return d != nullptr; };

    commands.add("tile", "tile <folder>", "open a tile folder (the one with tile.ted) and switch to the tile tool", [this, ed, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: tile <folder>");
        std::string msg;
        return open_folder(*ed, a[0], &msg) ? ok(msg) : fail(msg);
    });

    commands.add("tile_mode", "tile_mode select|raise|lower|smooth|flatten|level", "what the left mouse button does in the viewport", [this, fail, ok](const CommandRegistry::Args& a) {
        static const char* names[] = {"select", "raise", "lower", "smooth", "flatten", "level"};
        if (a.empty()) return fail("usage: tile_mode select|raise|lower|smooth|flatten|level");
        for (int i = 0; i < 6; ++i)
            if (a[0] == names[i]) { set_mode(TileMode(i)); return ok(); }
        return fail("unknown mode '" + a[0] + "'");
    });

    commands.add("brush", "brush <radius m> [strength] [falloff] [flatten height m]", "set the terrain brush", [this, fail, ok](const CommandRegistry::Args& a) {
        float v;
        if (a.empty() || !number(a[0], v)) return fail("usage: brush <radius m> [strength] [falloff] [flatten height m]");
        brush_.radius_m = std::clamp(v, 1.0f, 500.0f);
        if (a.size() > 1 && number(a[1], v)) brush_.strength = v;
        if (a.size() > 2 && number(a[2], v)) brush_.falloff = std::clamp(v, 0.0f, 1.0f);
        if (a.size() > 3 && number(a[3], v)) brush_.flatten_height = v;
        return ok();
    });

    commands.add("paint", "paint raise|lower|smooth|flatten|level <x m> <z m> <steps>", "a brush stroke at a point of the tile (one undo entry)", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float x, z, n;
        static const char* names[] = {"raise", "lower", "smooth", "flatten", "level"};
        if (a.size() < 4 || !number(a[1], x) || !number(a[2], z) || !number(a[3], n)) return fail("usage: paint raise|lower|smooth|flatten|level <x m> <z m> <steps>");
        int kind = -1;
        for (int i = 0; i < 5; ++i)
            if (a[0] == names[i]) kind = i;
        if (kind < 0) return fail("unknown brush '" + a[0] + "'");
        if (!d->project().has_height) return fail("no height map");
        for (int i = 0; i < int(n); ++i) d->brush_step(BrushKind(kind), x, z, brush_, 1.0f / 60.0f);
        d->end_stroke();
        return ok();
    });

    commands.add("tile_select", "tile_select <type | element index> [add]", "select the first instance of a type, or an element by index", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: tile_select <type | element index> [add]");
        const bool add = a.size() > 1 && a[1] == "add";
        char* end = nullptr;
        const long idx = std::strtol(a[0].c_str(), &end, 10);
        int found = -1;
        if (end && *end == 0 && !a[0].empty()) found = int(idx);
        else
            for (int e : d->instances())
                if (d->tscene().type(e) == a[0]) { found = e; break; }
        if (found < 0 || found >= int(d->tscene().doc().elements().size())) return fail("nothing found for '" + a[0] + "'");
        select_elem(*ed, found, add);
        return ok("element " + std::to_string(found));
    });

    commands.add("tile_move", "tile_move <dx m> <dz m>", "move the selected instance over the ground (keeps its height above the terrain)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float dx, dz;
        const int e = primary(*ed);
        if (a.size() < 2 || !number(a[0], dx) || !number(a[1], dz)) return fail("usage: tile_move <dx m> <dz m>");
        if (e < 0) return fail("nothing selected");
        float3 p = d->world_position(e);
        const float above = p.y - d->terrain_height(p.x, p.z);
        p.x += dx;
        p.z += dz;
        p.y = d->terrain_height(p.x, p.z) + above;
        return d->set_world_metres(e, p, 0) ? ok() : fail("cannot move this element");
    });

    commands.add("grab_to", "grab_to <x m> <z m>", "the G grab: put the selected instance on the ground at a point (grid snap applies)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float x, z;
        if (a.size() < 2 || !number(a[0], x) || !number(a[1], z)) return fail("usage: grab_to <x m> <z m>");
        if (!begin_grab(*ed)) return fail("nothing selected");
        if (snap_grid_ && grid_m_ > 0.01f) {
            x = std::round(x / grid_m_) * grid_m_;
            z = std::round(z / grid_m_) * grid_m_;
        }
        d->set_world_metres(grab_elem_, {x, d->terrain_height(x, z) + float(grab_y_offset_), z}, grab_gesture_);
        end_grab(*ed, true);
        return ok();
    });

    commands.add("duplicate", "duplicate", "duplicate the selected instance (4 m to the side) and select the copy", [this, ed, need, fail, ok](const CommandRegistry::Args&) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        std::string err;
        return d->duplicate(e, &err) >= 0 ? ok("duplicated") : fail(err);
    });

    commands.add("delete_item", "delete_item", "delete the selected instance", [this, ed, need, fail, ok](const CommandRegistry::Args&) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        const int e = primary(*ed);
        return e >= 0 && d->remove(e) ? ok("deleted") : fail("nothing selected");
    });

    commands.add("rotate", "rotate <degrees>", "rotate the selected instance about the world Y axis", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float deg;
        if (a.empty() || !number(a[0], deg)) return fail("usage: rotate <degrees>");
        const int e = primary(*ed);
        return e >= 0 && d->rotate(e, deg) ? ok() : fail("nothing selected");
    });

    commands.add("add_building", "add_building <key> [x m z m]", "add a building (a copy of the selected or the first one, with this key), on the ground at x z when given", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: add_building <key> [x m z m]");
        float x, z;
        std::string err;
        int e;
        if (a.size() >= 3 && number(a[1], x) && number(a[2], z)) {
            const float3 at{x, d->terrain_height(x, z), z};
            e = d->add_building(a[0], primary(*ed), &at, &err);
        } else e = d->add_building(a[0], primary(*ed), nullptr, &err);
        return e >= 0 ? ok("added " + a[0]) : fail(err);
    });

    commands.add("add_polygon", "add_polygon \"TYPE|HINT|x,z;x,z;...\"", "add a polygon (AI hint, zone, ...) from points in metres; selects it", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: add_polygon \"TYPE|HINT|x,z;x,z;...\"");
        const auto parts = split(a[0], '|');
        if (parts.size() < 3) return fail("expected TYPE|HINT|x,z;x,z");
        std::vector<float3> pts;
        for (const std::string& item : split(parts[2], ';')) {
            const auto xz = split(item, ',');
            float x, z;
            if (xz.size() == 2 && number(xz[0], x) && number(xz[1], z)) pts.push_back({x, d->terrain_height(x, z), z});
        }
        std::string err;
        const int layer = primary(*ed) >= 0 ? d->layer_of(primary(*ed)) : d->tscene().scene_graph();
        const int e = d->add_polygon(layer, parts[0], parts[1], pts, &err);
        return e >= 0 ? ok("added " + parts[0]) : fail(err);
    });

    commands.add("polygon_point", "polygon_point move|insert|remove <index> <x m> <z m>", "edit a point of the selected polygon (insert: after the index)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float x = 0, z = 0, idx;
        if (a.size() < 2 || !number(a[1], idx)) return fail("usage: polygon_point move|insert|remove <index> <x m> <z m>");
        if (a.size() >= 4) { number(a[2], x); number(a[3], z); }
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        std::string err;
        return d->polygon_point(e, a[0], int(idx), {x, d->terrain_height(x, z), z}, &err) ? ok() : fail(err);
    });

    commands.add("new_layer", "new_layer <name>", "a new layer under the scene graph, selected", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: new_layer <name>");
        std::string err;
        return d->add_layer(d->tscene().scene_graph(), a[0], &err) >= 0 ? ok() : fail(err);
    });

    commands.add("move_to_layer", "move_to_layer <layer name>", "move the selected instance into the layer with this name", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: move_to_layer <layer name>");
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        for (size_t i = 0; i < d->tscene().doc().elements().size(); ++i)
            if (d->tscene().is_layer(int(i)) && d->tscene().name(int(i)) == a[0]) {
                std::string err;
                return d->move_to_layer(e, int(i), &err) ? ok() : fail(err);
            }
        return fail("no layer named '" + a[0] + "'");
    });

    commands.add("save_to", "save_to <folder>", "write tile.ted and height_map_0.png into another folder (the originals stay)", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: save_to <folder>");
        std::string msg;
        return d->save({fs::path(widen(a[0]))}, &msg) ? ok(msg) : fail(msg);
    });

    commands.add("tile_diagnostics", "tile_diagnostics <file.txt>", "write the tile's counts and model fallbacks (what Tile Studio wrote next to a shot)", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (a.empty()) return fail("usage: tile_diagnostics <file.txt>");
        std::ofstream f(fs::path(widen(a[0])), std::ios::binary);
        if (!f) return fail("cannot write " + a[0]);
        f << d->diagnostics();
        return ok();
    });

    // ---- scripted mouse on instances (the same events a real click or drag sends)
    commands.add("tile_click", "tile_click <element> [ctrl|shift]", "click the pixel where an instance is (selects it through the viewport, like the mouse)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_click <element> [ctrl|shift]");
        const int e = std::atoi(a[0].c_str());
        if (!d->is_instance(e)) return fail("not an instance");
        float3 p = d->world_position(e);
        if (d->tscene().type(e) == "BUILDING_INSTANCE") p.y += d->marker_radius(e) * 0.5f;
        float x, y;
        if (!ed->viewport().project(p, x, y)) return fail("the instance is not in view");
        char x_s[32], y_s[32];
        std::snprintf(x_s, sizeof x_s, "%.1f", x);
        std::snprintf(y_s, sizeof y_s, "%.1f", y);
        ed->commands().run("mouse", {"click", x_s, y_s});
        return ok();
    });
    commands.add("tile_click_ground", "tile_click_ground <x m> <z m>", "click the pixel where a ground point is", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z;
        if (!need(d) || a.size() < 2 || !number(a[0], x) || !number(a[1], z)) return fail("usage: tile_click_ground <x m> <z m>");
        float px, py;
        if (!ed->viewport().project({x, d->terrain_height(x, z), z}, px, py)) return fail("the point is not in view");
        char x_s[32], y_s[32];
        std::snprintf(x_s, sizeof x_s, "%.1f", px);
        std::snprintf(y_s, sizeof y_s, "%.1f", py);
        ed->commands().run("mouse", {"click", x_s, y_s});
        return ok();
    });
    commands.add("tile_drag_point", "tile_drag_point <index> <x m> <z m>", "drag a point handle of the selected polygon to a ground point with the mouse", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z, idx;
        if (!need(d) || a.size() < 3 || !number(a[0], idx) || !number(a[1], x) || !number(a[2], z)) return fail("usage: tile_drag_point <index> <x m> <z m>");
        const int e = primary(*ed);
        if (e < 0 || !d->is_polygon(e)) return fail("no polygon selected");
        const auto pts = d->polygon_world_points(e);
        if (idx < 0 || size_t(idx) >= pts.size()) return fail("no such point");
        float x0, y0, x1, y1;
        if (!ed->viewport().project(pts[size_t(idx)], x0, y0) || !ed->viewport().project({x, d->terrain_height(x, z), z}, x1, y1)) return fail("not in view");
        char b[5][32];
        std::snprintf(b[0], 32, "%.1f", x0);
        std::snprintf(b[1], 32, "%.1f", y0);
        std::snprintf(b[2], 32, "%.1f", x1);
        std::snprintf(b[3], 32, "%.1f", y1);
        std::snprintf(b[4], 32, "%d", 6);
        ed->commands().run("mouse", {"drag", b[0], b[1], b[2], b[3], b[4]});
        return ok();
    });
    commands.add("draw_polygon", "draw_polygon <type> [hint]", "start drawing a polygon: click the ground for points, Enter finishes (key Enter), Esc cancels", [this, fail, ok](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: draw_polygon <type> [hint]");
        new_poly_type_ = -1;
        for (int i = 0; i < TileTool::kPolygonTypeCount; ++i)
            if (a[0] == polygon_type(i)) new_poly_type_ = i;
        if (new_poly_type_ < 0) { new_poly_type_ = 0; return fail("unknown polygon type '" + a[0] + "'"); }
        if (a.size() > 1)
            for (int i = 0; i < TileTool::kHintCount; ++i)
                if (a[1] == hint(i)) new_hint_ = i;
        start_drawing();
        return ok();
    });
    commands.add("assert_at_cursor", "assert_at_cursor [tolerance]", "test: the selected instance is on the ground point the mouse last hovered", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float tol = 0.5f;
        if (!need(d)) return fail("no tile open");
        if (!a.empty()) number(a[0], tol);
        const int e = primary(*ed);
        if (e < 0 || !terrain_hit_) return fail("nothing selected or the cursor never touched the ground");
        const float3 p = d->world_position(e);
        char b[128];
        std::snprintf(b, sizeof b, "at %.2f %.2f, cursor %.2f %.2f", p.x, p.z, cursor_.x, cursor_.z);
        return std::fabs(p.x - cursor_.x) <= tol && std::fabs(p.z - cursor_.z) <= tol ? ok() : fail(b);
    });
    commands.add("assert_mode", "assert_mode select|raise|...", "test: the tile tool's left-button mode", [this, fail, ok](const CommandRegistry::Args& a) {
        static const char* names[] = {"select", "raise", "lower", "smooth", "flatten", "level"};
        return !a.empty() && a[0] == names[int(mode_)] ? ok() : fail(std::string("mode is ") + names[int(mode_)]);
    });
    commands.add("assert_grabbing", "assert_grabbing 0|1", "test: whether a G grab is in progress", [this, fail, ok](const CommandRegistry::Args& a) {
        return !a.empty() && (a[0] == "1") == grabbing_ ? ok() : fail(grabbing_ ? "grabbing" : "not grabbing");
    });

    // ---- tests
    commands.add("assert_instances", "assert_instances <n>", "test: the tile has n instances", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: assert_instances <n> (with a tile open)");
        return d->instances().size() == size_t(std::atoi(a[0].c_str())) ? ok() : fail("instances: " + std::to_string(d->instances().size()) + ", expected " + a[0]);
    });
    commands.add("assert_type_count", "assert_type_count <type> <n>", "test: the tile has n instances of a type", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_type_count <type> <n> (with a tile open)");
        const auto it = d->type_counts().find(a[0]);
        const int n = it == d->type_counts().end() ? 0 : it->second;
        return n == std::atoi(a[1].c_str()) ? ok() : fail(a[0] + ": " + std::to_string(n) + ", expected " + a[1]);
    });
    commands.add("assert_world", "assert_world <x m> <y m> <z m> [tolerance]", "test: the selected instance is at this world position", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, y, z, tol = 0.01f;
        if (!need(d) || a.size() < 3 || !number(a[0], x) || !number(a[1], y) || !number(a[2], z)) return fail("usage: assert_world <x m> <y m> <z m> [tolerance]");
        if (a.size() > 3) number(a[3], tol);
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        const float3 p = d->world_position(e);
        char b[160];
        std::snprintf(b, sizeof b, "at %.3f %.3f %.3f, expected %.3f %.3f %.3f", p.x, p.y, p.z, x, y, z);
        return std::fabs(p.x - x) <= tol && std::fabs(p.y - y) <= tol && std::fabs(p.z - z) <= tol ? ok() : fail(b);
    });
    commands.add("assert_world_xz", "assert_world_xz <x m> <z m> [tolerance]", "test: the selected instance is at this ground position (any height)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z, tol = 0.01f;
        if (!need(d) || a.size() < 2 || !number(a[0], x) || !number(a[1], z)) return fail("usage: assert_world_xz <x m> <z m> [tolerance]");
        if (a.size() > 2) number(a[2], tol);
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        const float3 p = d->world_position(e);
        char b[128];
        std::snprintf(b, sizeof b, "at %.3f %.3f, expected %.3f %.3f", p.x, p.z, x, z);
        return std::fabs(p.x - x) <= tol && std::fabs(p.z - z) <= tol ? ok() : fail(b);
    });
    commands.add("mark_height", "mark_height <name> <x m> <z m>", "remember the terrain height at a point under a name (for assert_height_delta)", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z;
        if (!need(d) || a.size() < 3 || !number(a[1], x) || !number(a[2], z)) return fail("usage: mark_height <name> <x m> <z m>");
        marks_[a[0]] = d->terrain_height(x, z);
        return ok();
    });
    commands.add("assert_height_delta", "assert_height_delta <name> <x m> <z m> <min> <max>", "test: the height at a point changed by an amount in [min, max] since mark_height", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z, lo, hi;
        if (!need(d) || a.size() < 5 || !number(a[1], x) || !number(a[2], z) || !number(a[3], lo) || !number(a[4], hi)) return fail("usage: assert_height_delta <name> <x m> <z m> <min> <max>");
        const auto it = marks_.find(a[0]);
        if (it == marks_.end()) return fail("no mark '" + a[0] + "'");
        const float delta = d->terrain_height(x, z) - it->second;
        char b[96];
        std::snprintf(b, sizeof b, "height changed by %.4f, expected %.4f .. %.4f", delta, lo, hi);
        return delta >= lo && delta <= hi ? ok() : fail(b);
    });
    commands.add("assert_height", "assert_height <x m> <z m> <height m> [tolerance]", "test: the terrain height at a point", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z, h, tol = 0.01f;
        if (!need(d) || a.size() < 3 || !number(a[0], x) || !number(a[1], z) || !number(a[2], h)) return fail("usage: assert_height <x m> <z m> <height m> [tolerance]");
        if (a.size() > 3) number(a[3], tol);
        const float got = d->terrain_height(x, z);
        char b[96];
        std::snprintf(b, sizeof b, "height %.4f, expected %.4f", got, h);
        return std::fabs(got - h) <= tol ? ok() : fail(b);
    });
    commands.add("assert_selected_type", "assert_selected_type <type>", "test: the primary selected element has this type", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: assert_selected_type <type>");
        const int e = primary(*ed);
        if (e < 0) return fail("nothing selected");
        return d->tscene().type(e) == a[0] ? ok() : fail("selected " + d->tscene().type(e));
    });
    commands.add("assert_file_exists", "assert_file_exists <path>", "test: a file exists", [fail, ok](const CommandRegistry::Args& a) {
        std::error_code ec;
        return !a.empty() && fs::exists(fs::path(widen(a[0])), ec) ? ok() : fail("missing " + (a.empty() ? std::string() : a[0]));
    });
}

} // namespace wce
