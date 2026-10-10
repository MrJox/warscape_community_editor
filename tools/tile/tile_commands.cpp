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
#include "formats/tile_index.h"
#include "tile/tile_tool.h"

#include <functional>

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

// A browser row matches when every word (from args[first]) is in its location, file name, set or variation names, in any case.
bool matches_words(const TileIndexEntry& e, const std::vector<std::string>& args, size_t first) {
    std::string hay = e.key + " " + e.name + " " + e.tile_set;
    for (const std::string& v : e.variations) hay += " " + v;
    for (char& c : hay) c = char(std::tolower(static_cast<unsigned char>(c)));
    for (size_t i = first; i < args.size(); ++i) {
        std::string w = args[i];
        for (char& c : w) c = char(std::tolower(static_cast<unsigned char>(c)));
        if (hay.find(w) == std::string::npos) return false;
    }
    return true;
}

// An element named on a command line: "#123" (an element index), a layer path ("Walls/Standard_Walls"), "selected" (the first selected item).
int element_arg(TileDocument& d, TileTool& tool, Editor& ed, const std::string& a) {
    if (a.empty()) return -1;
    if (a == "selected") return tool.primary(ed);
    if (a[0] == '#') return std::atoi(a.c_str() + 1);
    return d.find_layer(a);
}

} // namespace

void TileTool::register_commands(Editor& editor, CommandRegistry& commands) {
    Editor* ed = &editor;
    auto ok = [](std::string m = {}) { return CommandResult::success(std::move(m)); };
    auto fail = [](std::string m) { return CommandResult::fail(std::move(m)); };
    // Every command works on the active tile and says so when there is none.
    auto need = [this, ed](TileDocument*& d) { d = document(*ed); return d != nullptr; };

    // The index of the kits' tile database entries keeps its rows in the settings folder between runs.
    TileIndex::set_cache_dir(editor.settings().file().parent_path() / "tile_index");

    commands.add("tile", "tile <entry xml | tile name | folder | tile.ted> [variation]",
                 "open a tile by its database entry (a TILES/*.xml, or a name like assembly_kit/osgiliath_test), by a variation folder or by its tile.ted, and switch to the tile tool",
                 [this, ed, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: tile <entry xml | tile name | folder | tile.ted> [variation]");
        std::string msg;
        return open_tile(*ed, a[0], a.size() > 1 ? a[1] : std::string(), &msg) ? ok(msg) : fail(msg);
    });

    commands.add("tiles", "tiles [filter words]", "list the tile database entries of the kits (the tile browser's rows), optionally filtered; says how many", [this, ed, ok, fail](const CommandRegistry::Args& a) {
        const std::vector<fs::path> roots = kit_roots(*ed);
        if (roots.empty()) return fail("no assembly kit found");
        std::string out;
        size_t total = 0, shown = 0;
        for (const fs::path& root : roots) {
            const auto index = TileIndex::shared(root);
            total += index->entries().size();
            for (const TileIndexEntry& e : index->entries()) {
                if (!matches_words(e, a, 0)) continue;
                if (++shown <= 40) {
                    std::string vars;
                    for (const std::string& v : e.variations) vars += (vars.empty() ? "" : ",") + v;
                    char b[400];
                    std::snprintf(b, sizeof b, "%s  %ux%u  [%s]  %s\n", e.key.c_str(), e.width, e.height, vars.c_str(), root.parent_path().filename().string().c_str());
                    out += b;
                }
            }
        }
        return ok(std::to_string(shown) + " of " + std::to_string(total) + " tiles" + (shown ? "\n" + out : std::string()));
    });
    commands.add("assert_tiles", "assert_tiles <n> [filter words]", "test: the tile browser lists n entries for the filter (over all kits)", [this, ed, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: assert_tiles <n> [filter words]");
        size_t shown = 0;
        for (const fs::path& root : kit_roots(*ed))
            for (const TileIndexEntry& e : TileIndex::shared(root)->entries())
                if (matches_words(e, a, 1)) ++shown;
        return shown == size_t(std::atoi(a[0].c_str())) ? ok() : fail(std::to_string(shown) + " tiles, expected " + a[0]);
    });

    commands.add("tile_entry", "tile_entry", "log the active tile's database entry (location, size, colour, ground groups, variations, links)", [need, fail, ok](const CommandRegistry::Args&) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        const TileProject& p = d->project();
        if (!p.entry) return ok("no tile database entry" + (p.warning.empty() ? std::string() : ": " + p.warning));
        const TileEntry& e = *p.entry;
        const TileTextureGroup g = e.texture_group();
        char b[600];
        std::snprintf(b, sizeof b, "%s: %s set %s, %u x %u cells, colour %.0f %.0f %.0f, ground %s/%s/%s/%s, %zu variations (open: %s), %zu links, %zu targets", e.file().filename().string().c_str(),
                      e.location_generic().c_str(), e.tile_set().c_str(), e.width(), e.height(), e.red(), e.green(), e.blue(), g.red.c_str(), g.green.c_str(), g.blue.c_str(), g.alpha.c_str(),
                      e.variations().size(), p.info.variation.c_str(), e.links().size(), e.link_targets().size());
        return ok(b);
    });
    commands.add("assert_entry", "assert_entry <field> <value>",
                 "test: a field of the active tile's database entry (set, location, name, kind, width, height, red, green, blue, ground_red/green/blue/alpha, variations, variation, min_height, scale, "
                 "normal_strength, overlap_border_size, raw_data_tri_density, links, targets, scalable, random_rotatable; `entry none` / `entry present`)",
                 [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_entry <field> <value> (with a tile open)");
        const TileProject& p = d->project();
        if (a[0] == "entry") return (a[1] == "none") == (p.entry == nullptr) ? ok() : fail(p.entry ? "the tile has an entry" : "the tile has no entry");
        if (!p.entry) return fail("the tile has no database entry");
        const TileEntry& e = *p.entry;
        const TileTextureGroup g = e.texture_group();
        const std::vector<TileVariation> vars = e.variations();
        const TileVariation* v = p.variation >= 0 && size_t(p.variation) < vars.size() ? &vars[size_t(p.variation)] : nullptr;
        std::string got;
        double num = 0;
        bool numeric = true;
        const std::string& f = a[0];
        if (f == "set") { got = e.tile_set(); numeric = false; }
        else if (f == "location") { got = e.location_generic(); numeric = false; }
        else if (f == "name") { got = e.file().stem().string(); numeric = false; }
        else if (f == "kind") { got = e.kind(); numeric = false; }
        else if (f == "width") num = e.width();
        else if (f == "height") num = e.height();
        else if (f == "red") num = e.red();
        else if (f == "green") num = e.green();
        else if (f == "blue") num = e.blue();
        else if (f == "ground_red") { got = g.red; numeric = false; }
        else if (f == "ground_green") { got = g.green; numeric = false; }
        else if (f == "ground_blue") { got = g.blue; numeric = false; }
        else if (f == "ground_alpha") { got = g.alpha; numeric = false; }
        else if (f == "variations") num = double(vars.size());
        else if (f == "variation") { got = p.info.variation; numeric = false; }
        else if (f == "links") num = double(e.links().size());
        else if (f == "targets") num = double(e.link_targets().size());
        else if (f == "scalable") { got = e.scalable() ? "true" : "false"; numeric = false; }
        else if (f == "random_rotatable") { got = e.random_rotatable() ? "true" : "false"; numeric = false; }
        else if (v && f == "min_height") num = v->min_height;
        else if (v && f == "scale") num = v->scale;
        else if (v && f == "normal_strength") num = v->normal_strength;
        else if (v && f == "overlap_border_size") num = v->overlap_border_size;
        else if (v && f == "raw_data_tri_density") num = v->raw_data_tri_density;
        else return fail("unknown field '" + f + "'" + (v ? "" : " (or the open folder is not one of the entry's variations)"));
        if (numeric) {
            char* end = nullptr;
            const double want = std::strtod(a[1].c_str(), &end);
            char b[160];
            std::snprintf(b, sizeof b, "%s is %.6g, expected %s", f.c_str(), num, a[1].c_str());
            return end && *end == 0 && std::fabs(num - want) <= 1e-4 ? ok() : fail(b);
        }
        return got == a[1] ? ok() : fail(f + " is '" + got + "', expected '" + a[1] + "'");
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
        if (!d->is_instance(e)) return fail("cannot move this element");
        d->set_world_metres(e, p, 0);
        return ok();
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

    // ---- layers (Phase 1): flags, structure, export type. A layer is named by its path from the scene graph ("Walls/Standard_Walls"), "#n" is an element index.
    commands.add("tile_flag", "tile_flag <layer path | #elem | selected> visible|active on|off", "the eye (OIF_VISIBLE) or the cursor arrow (OIF_ACTIVE, selectable) of a layer or instance", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 3) return fail("usage: tile_flag <layer path | #elem | selected> visible|active on|off");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0 || e >= int(d->tscene().doc().elements().size())) return fail("no such layer or element '" + a[0] + "'");
        const char* flag = a[1] == "visible" ? "OIF_VISIBLE" : a[1] == "active" ? "OIF_ACTIVE" : nullptr;
        if (!flag) return fail("the flag is visible or active");
        std::vector<int> targets = {e};
        if (a[0] == "selected") targets = selected(*ed);
        return d->set_flags(targets, flag, a[2] == "on") ? ok() : ok("already " + a[2]);
    });
    commands.add("tile_new_layer", "tile_new_layer <name> [parent layer path]", "a new layer in a layer (the scene graph by default), selected", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_new_layer <name> [parent layer path]");
        const int parent = a.size() > 1 ? d->find_layer(a[1]) : d->tscene().scene_graph();
        std::string err;
        return d->add_layer(parent, a[0], &err) >= 0 ? ok() : fail(err.empty() ? "no such parent layer" : err);
    });
    commands.add("tile_rename", "tile_rename <layer path | #elem | selected> <name>", "rename a layer or an instance", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: tile_rename <layer path | #elem | selected> <name>");
        const int e = element_arg(*d, *this, *ed, a[0]);
        return e >= 0 && d->rename(e, a[1]) ? ok() : fail("no such layer or element '" + a[0] + "'");
    });
    commands.add("tile_move_to", "tile_move_to <what: path | #elem | selected> <target layer path> [before <path | #elem>]", "move layers / instances into a layer (in front of a sibling): the selection when `selected`",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: tile_move_to <path | #elem | selected> <target layer path> [before <path | #elem>]");
        std::vector<int> what = a[0] == "selected" ? selected(*ed) : std::vector<int>{element_arg(*d, *this, *ed, a[0])};
        const int layer = element_arg(*d, *this, *ed, a[1]);
        int before = -1;
        if (a.size() > 3 && a[2] == "before") before = element_arg(*d, *this, *ed, a[3]);
        if (what.empty() || what[0] < 0 || layer < 0) return fail("nothing to move, or no such target layer");
        std::string err;
        return d->move_items(what, layer, before, &err) ? ok() : fail(err);
    });
    commands.add("tile_group", "tile_group <new layer name>", "put the selected items into a new layer next to the first one", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_group <new layer name>");
        std::string err;
        return d->group_items(selected(*ed), a[0], &err) >= 0 ? ok() : fail(err);
    });
    commands.add("tile_delete", "tile_delete <layer path | #elem | selected>", "delete layers / instances (with everything below)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_delete <layer path | #elem | selected>");
        std::vector<int> what = a[0] == "selected" ? selected(*ed) : std::vector<int>{element_arg(*d, *this, *ed, a[0])};
        return !what.empty() && what[0] >= 0 && d->remove_items(what) ? ok() : fail("nothing to delete");
    });
    commands.add("tile_duplicate_layer", "tile_duplicate_layer <layer path>", "duplicate a layer with everything in it (named '<name> copy')", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_duplicate_layer <layer path>");
        std::string err;
        return d->duplicate_layer(element_arg(*d, *this, *ed, a[0]), &err) >= 0 ? ok() : fail(err);
    });
    commands.add("tile_export_type", "tile_export_type <layer path> Normal|Escalation1|Escalation2|Escalation3", "the export type of a layer", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: tile_export_type <layer path> Normal|Escalation1|Escalation2|Escalation3");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0 || !d->tscene().is_layer(e)) return fail("no such layer");
        return d->set_attribute(e, "export_type", "LayerExportType_" + a[1]) ? ok() : fail("cannot set it");
    });
    commands.add("tile_select_layer", "tile_select_layer <layer path | #elem>", "select a layer or an element in the scene tree", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_select_layer <layer path | #elem>");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0) return fail("no such layer or element");
        select_elem(*ed, e);
        return ok();
    });
    commands.add("tile_select_in", "tile_select_in <layer path> <type> [frame|click]", "select the first instance of a type below a layer; `frame` points the camera at it, `click` clicks its pixel with the scripted mouse (frame it and wait some frames first)",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: tile_select_in <layer path> <type> [click]");
        const int layer = element_arg(*d, *this, *ed, a[0]);
        if (layer < 0) return fail("no such layer");
        int found = -1;
        for (int e : d->instances())
            if (d->tscene().type(e) == a[1] && d->tscene().is_descendant(e, layer)) { found = e; break; }
        if (found < 0) return fail("no " + a[1] + " in " + a[0]);
        if (a.size() > 2 && a[2] == "click") {
            const float3 p = d->world_position(found);
            float3 q = p;
            if (d->tscene().type(found) == "BUILDING_INSTANCE") q.y += d->marker_radius(found) * 0.5f;
            float x, y;
            if (!ed->viewport().project(q, x, y)) return fail("the instance is not in view");
            char x_s[32], y_s[32];
            std::snprintf(x_s, sizeof x_s, "%.1f", x);
            std::snprintf(y_s, sizeof y_s, "%.1f", y);
            ed->commands().run("mouse", {"click", x_s, y_s});
            return ok("clicked element " + std::to_string(found));
        }
        if (a.size() > 2 && a[2] == "frame") {   // the camera looks at it (wait a few frames before clicking)
            const float3 p = d->world_position(found);
            if (auto* orbit = dynamic_cast<OrbitCamera*>(&ed->viewport().controller())) orbit->set_orbit(0.8f, 0.7f, 60.0f, {p.x, p.y, p.z});
            return ok("framed element " + std::to_string(found));
        }
        select_elem(*ed, found);
        return ok("element " + std::to_string(found));
    });
    commands.add("tile_tree", "tile_tree [depth]", "log the scene tree from the root down to a depth (default 2) with each layer's flags and child count", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        const int depth = a.empty() ? 2 : std::atoi(a[0].c_str());
        std::string out;
        std::function<void(int, int)> walk = [&](int e, int level) {
            const auto kids = d->tscene().children(e);
            char b[300];
            std::snprintf(b, sizeof b, "%*s#%d %s '%s' %s%s%s %zu\n", level * 2, "", e, d->tscene().type(e).c_str(), d->tscene().name(e).c_str(), d->own_flag(e, "OIF_VISIBLE") ? "V" : "-", d->own_flag(e, "OIF_ACTIVE") ? "A" : "-",
                          (d->tscene().is_layer(e) ? " export=" + d->export_type(e) : std::string()).c_str(), kids.size());
            out += b;
            if (level < depth)
                for (int k : kids)
                    if (d->tscene().is_layer(k)) walk(k, level + 1);
        };
        if (d->tscene().scene_graph() >= 0) walk(d->tscene().scene_graph(), 0);
        return ok(out);
    });
    commands.add("assert_flag", "assert_flag <layer path | #elem> visible|active 0|1", "test: the element's own eye / cursor arrow", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 3) return fail("usage: assert_flag <layer path | #elem> visible|active 0|1");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0) return fail("no such layer or element");
        const bool v = d->own_flag(e, a[1] == "visible" ? "OIF_VISIBLE" : "OIF_ACTIVE");
        return v == (a[2] == "1") ? ok() : fail(a[0] + " " + a[1] + " is " + (v ? "1" : "0"));
    });
    commands.add("assert_shown", "assert_shown <layer path | #elem> 0|1", "test: the element is drawn (visible itself and in every layer above)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_shown <layer path | #elem> 0|1");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0) return fail("no such layer or element");
        return d->shown(e) == (a[1] == "1") ? ok() : fail(a[0] + (d->shown(e) ? " is shown" : " is hidden"));
    });
    commands.add("assert_selectable", "assert_selectable <layer path | #elem> 0|1", "test: the element can be picked (active itself and in every layer above)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_selectable <layer path | #elem> 0|1");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0) return fail("no such layer or element");
        return d->selectable(e) == (a[1] == "1") ? ok() : fail(a[0] + (d->selectable(e) ? " is selectable" : " is locked"));
    });
    commands.add("assert_layer", "assert_layer <layer path> <children n | exists 0|1 | export Type | name N>", "test: facts of a layer (children n | exists 0|1 | export Type | child <index> <name>)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_layer <layer path> <children n | exists 0|1 | export Type>");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (a[1] == "exists") return (e >= 0) == (a.size() > 2 && a[2] == "1") ? ok() : fail(e >= 0 ? "the layer exists" : "no such layer");
        if (e < 0) return fail("no such layer '" + a[0] + "'");
        if (a[1] == "children" && a.size() > 2) {
            const size_t n = d->tscene().children(e).size();
            return n == size_t(std::atoi(a[2].c_str())) ? ok() : fail("children: " + std::to_string(n));
        }
        if (a[1] == "export" && a.size() > 2) return d->export_type(e) == a[2] ? ok() : fail("export type " + d->export_type(e));
        if (a[1] == "child" && a.size() > 3) {   // the direct child at an index (counted from 0) has this name
            const auto kids = d->tscene().children(e);
            const size_t i = size_t(std::atoi(a[2].c_str()));
            const std::string got = i < kids.size() ? d->tscene().name(kids[i]) : "(none)";
            return got == a[3] ? ok() : fail("child " + a[2] + " is '" + got + "'");
        }
        return fail("usage: assert_layer <layer path> <children n | exists 0|1 | export Type>");
    });
    commands.add("assert_selection_names", "assert_selection_names <name>...", "test: the selected elements have exactly these names, in selection order", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        const std::vector<int> sel = selected(*ed);
        std::string got;
        for (int e : sel) got += (got.empty() ? "" : ",") + d->tscene().name(e);
        std::string want;
        for (const std::string& n : a) want += (want.empty() ? "" : ",") + n;
        return got == want ? ok() : fail("selected: " + got);
    });

    // ---- tests
    // ---- placement (Phase 2): landscape / freeform, snapping, the off-terrain report
    commands.add("tile_placement", "tile_placement <path | #elem | selected> landscape|freeform", "set the surface mode of the instances (layers: everything below); landscape snaps them to the terrain",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: tile_placement <path | #elem | selected> landscape|freeform");
        std::vector<int> what = a[0] == "selected" ? selected(*ed) : std::vector<int>{element_arg(*d, *this, *ed, a[0])};
        if (what.empty() || what[0] < 0) return fail("no such layer or element '" + a[0] + "'");
        const auto mode = a[1] == "landscape" ? TileDocument::Placement::Landscape : a[1] == "freeform" ? TileDocument::Placement::Freeform : TileDocument::Placement::None;
        if (mode == TileDocument::Placement::None) return fail("the mode is landscape or freeform");
        return ok(std::to_string(d->set_placement(what, mode)) + " changed");
    });
    commands.add("tile_snap", "tile_snap <path | #elem | selected | all> [freeform]", "put landscape instances (with `freeform`: every instance) on the terrain; `all` is every instance of the tile", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_snap <path | #elem | selected | all> [freeform]");
        std::vector<int> what;
        if (a[0] == "all") what = {d->tscene().scene_graph()};
        else what = a[0] == "selected" ? selected(*ed) : std::vector<int>{element_arg(*d, *this, *ed, a[0])};
        if (what.empty() || what[0] < 0) return fail("no such layer or element '" + a[0] + "'");
        return ok(std::to_string(d->snap_to_terrain(what, a.size() > 1 && a[1] == "freeform")) + " moved");
    });
    commands.add("tile_set_world", "tile_set_world <x m> <y m> <z m>", "type a world position for the selected instance (the Inspector's X / Y / Z fields)", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, y, z;
        if (!need(d) || a.size() < 3 || !number(a[0], x) || !number(a[1], y) || !number(a[2], z)) return fail("usage: tile_set_world <x m> <y m> <z m>");
        const int e = primary(*ed);
        if (e < 0 || !d->is_instance(e)) return fail("no instance selected");
        d->set_world_metres(e, {x, y, z}, 0);
        d->history().break_merge();
        return ok();
    });
    commands.add("tile_report_off_terrain", "tile_report_off_terrain [tolerance m] [layer path]", "log the landscape instances that are more than the tolerance (default 0.5) off the terrain, worst first",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        float tol = 0.5f;
        if (!a.empty() && !number(a[0], tol)) return fail("usage: tile_report_off_terrain [tolerance m] [layer path]");
        std::vector<int> among;
        if (a.size() > 1) {
            const int l = d->find_layer(a[1]);
            if (l < 0) return fail("no such layer '" + a[1] + "'");
            among = d->instances_below({l});
        }
        const auto list = d->off_terrain(tol, a.size() > 1 ? &among : nullptr);
        std::string s = std::to_string(list.size()) + " landscape instance(s) more than " + std::to_string(tol) + " m off the terrain";
        for (size_t i = 0; i < list.size() && i < 8; ++i) s += "\n  #" + std::to_string(list[i].elem) + " " + d->tscene().type(list[i].elem) + " '" + d->tscene().name(list[i].elem) + "' " + std::to_string(list[i].dy) + " m";
        return ok(s);
    });
    commands.add("assert_placement", "assert_placement <path | #elem | selected> landscape|freeform|none", "test: the element's surface mode", [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_placement <path | #elem | selected> landscape|freeform|none");
        const int e = element_arg(*d, *this, *ed, a[0]);
        if (e < 0) return fail("no such element '" + a[0] + "'");
        const auto p = d->placement(e);
        const std::string got = p == TileDocument::Placement::Landscape ? "landscape" : p == TileDocument::Placement::Freeform ? "freeform" : "none";
        return got == a[1] ? ok() : fail("placement is " + got + ", expected " + a[1]);
    });
    commands.add("assert_on_terrain", "assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]", "test: the instance (any placement mode) stands on the terrain (y = the terrain height) or not; for a layer, all instances below it",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.size() < 2) return fail("usage: assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]");
        float tol = 0.001f;
        if (a.size() > 2 && !number(a[2], tol)) return fail("bad tolerance");
        std::vector<int> what = a[0] == "selected" ? selected(*ed) : std::vector<int>{element_arg(*d, *this, *ed, a[0])};
        if (what.empty() || what[0] < 0) return fail("no such element '" + a[0] + "'");
        size_t off = 0;
        float worst = 0;
        for (int e : d->instances_below(what)) {   // any mode: the instance's y against the terrain at its x / z
            const float3 p = d->world_position(e);
            const float dy = p.y - d->landscape_y(e, p.x, p.z);
            if (std::fabs(dy) > tol) { ++off; if (std::fabs(dy) > std::fabs(worst)) worst = dy; }
        }
        const bool on = off == 0;
        if (on != (a[1] == "1")) return fail(std::string(on ? "all stand on the terrain" : std::to_string(off) + " off the terrain, worst " + std::to_string(worst) + " m") + ", expected " + (a[1] == "1" ? "on" : "off"));
        return ok();
    });
    commands.add("assert_off_terrain_count", "assert_off_terrain_count <tolerance m> <n>", "test: n landscape instances of the tile are more than the tolerance off the terrain", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float tol, n;
        if (!need(d) || a.size() < 2 || !number(a[0], tol) || !number(a[1], n)) return fail("usage: assert_off_terrain_count <tolerance m> <n>");
        const size_t got = d->off_terrain(tol).size();
        return got == size_t(n) ? ok() : fail(std::to_string(got) + " instances off the terrain, expected " + a[1]);
    });
    commands.add("assert_placement_counts", "assert_placement_counts <landscape n> <freeform n>", "test: how many instances are landscape / freeform", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float l, f;
        if (!need(d) || a.size() < 2 || !number(a[0], l) || !number(a[1], f)) return fail("usage: assert_placement_counts <landscape n> <freeform n>");
        const auto c = d->placement_counts();
        return c.landscape == size_t(l) && c.freeform == size_t(f) ? ok() : fail("landscape " + std::to_string(c.landscape) + ", freeform " + std::to_string(c.freeform));
    });

    // ---- ground look (Phase 3)
    commands.add("tile_terrain", "tile_terrain blended|flat [climate] [season su|au|wi]", "the ground colour of the open tile: the blend map mixed from the climate's textures, or the plain tiled texture",
                 [this, ed, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_terrain blended|flat [climate] [season su|au|wi]");
        TerrainLook look = d->terrain_look();
        if (a[0] == "blended") look.blended = true;
        else if (a[0] == "flat") look.blended = false;
        else return fail("the first word is blended or flat");
        if (a.size() > 1) look.climate = a[1];
        if (a.size() > 2) look.season = a[2];
        if (look.blended && !d->climates().find(look.climate) && !d->climates().climates.empty()) return fail("no climate '" + look.climate + "' in the kit's _settings.xml");
        d->set_terrain_look(look);
        new_look_ = look;
        return ok(d->terrain_note());
    });
    commands.add("tile_ground_view", "tile_ground_view colour|groups|ground|protection", "what the ground shows: the blended colour or a debug view of the tile's maps (blend groups, ground types, protection)", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: tile_ground_view colour|groups|ground|protection");
        TerrainLook look = d->terrain_look();
        look.blended = true;
        if (a[0] == "colour") look.view = TerrainView::Colour;
        else if (a[0] == "groups") look.view = TerrainView::BlendGroups;
        else if (a[0] == "ground") look.view = TerrainView::GroundTypes;
        else if (a[0] == "protection") look.view = TerrainView::Protection;
        else return fail("the view is colour, groups, ground or protection");
        d->set_terrain_look(look);
        new_look_ = look;
        return ok(d->terrain_note());
    });
    commands.add("tile_links", "tile_links [on|off]", "log the tile database entry's link targets and rules (the cells they name), or show / hide them on the ground", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d)) return fail("no tile open");
        if (!a.empty()) {
            if (a[0] != "on" && a[0] != "off") return fail("usage: tile_links [on|off]");
            d->display().show_links = a[0] == "on";
            d->display_changed();
            return ok();
        }
        std::string s;
        for (const auto& c : d->link_cells()) s += "\n  " + std::string(c.target ? "target " : (c.equals ? "= " : "!= ")) + c.set + " " + c.text;
        return ok(std::to_string(d->link_cells().size()) + " link item(s)" + s);
    });
    commands.add("assert_links", "assert_links <targets n> <rules n> [drawn 0|1]", "test: the entry has n link targets and n rules; whether they are drawn on the ground (after a frame)", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float t, r;
        if (!need(d) || a.size() < 2 || !number(a[0], t) || !number(a[1], r)) return fail("usage: assert_links <targets n> <rules n> [drawn 0|1]");
        size_t targets = 0, rules = 0;
        for (const auto& c : d->link_cells()) (c.target ? targets : rules)++;
        if (targets != size_t(t) || rules != size_t(r)) return fail(std::to_string(targets) + " targets, " + std::to_string(rules) + " rules");
        if (a.size() > 2 && (a[2] == "1") != d->links_drawn()) return fail(std::string("links are ") + (d->links_drawn() ? "" : "not ") + "drawn");
        return ok();
    });
    commands.add("assert_link_cell", "assert_link_cell <x> <y> <x0 m> <z0 m>", "test: the entry cell (x, y) lies at this world corner (the row counts from the +z edge)", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, y, ex, ez;
        if (!need(d) || a.size() < 4 || !number(a[0], x) || !number(a[1], y) || !number(a[2], ex) || !number(a[3], ez)) return fail("usage: assert_link_cell <x> <y> <x0 m> <z0 m>");
        float x0, z0, x1, z1;
        d->cell_rect(int(x), int(y), x0, z0, x1, z1);
        return std::fabs(x0 - ex) < 0.01f && std::fabs(z0 - ez) < 0.01f ? ok() : fail("cell corner is " + std::to_string(x0) + ", " + std::to_string(z0));
    });
    commands.add("tile_water", "tile_water on|off", "show or hide the water (river ribbons, sea / lake plane) of the open tile", [this, need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty() || (a[0] != "on" && a[0] != "off")) return fail("usage: tile_water on|off");
        d->display().show_water = a[0] == "on";
        d->display_changed();
        return ok();
    });
    commands.add("assert_water", "assert_water <pieces n>", "test: the tile has n water meshes (river splines + the sea plane) in the scene (after a frame)", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float n;
        if (!need(d) || a.empty() || !number(a[0], n)) return fail("usage: assert_water <pieces n>");
        return d->water_pieces() == size_t(n) ? ok(d->water_note()) : fail(std::to_string(d->water_pieces()) + " water meshes: " + d->water_note());
    });
    commands.add("assert_ground", "assert_ground blended|flat [layers n] [climate]", "test: the ground is the baked blend (with this many texture layers) or the plain texture", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        if (!need(d) || a.empty()) return fail("usage: assert_ground blended|flat [layers n] [climate]");
        const BakedColour* b = d->baked_colour();
        if (a[0] == "flat") return b == nullptr || !d->terrain_look().blended ? ok() : fail("the ground is blended: " + d->terrain_note());
        if (!b) return fail("the ground is not blended: " + d->terrain_note());
        float n;
        if (a.size() > 1 && number(a[1], n) && b->layers != int(n)) return fail(std::to_string(b->layers) + " layers, expected " + a[1]);
        if (a.size() > 2 && b->climate != a[2]) return fail("climate " + b->climate + ", expected " + a[2]);
        return ok(d->terrain_note());
    });
    commands.add("assert_ground_pixel", "assert_ground_pixel <x m> <z m> <r> <g> <b> [tolerance]", "test: the baked ground colour at a world point (0..255 per channel)", [need, fail, ok](const CommandRegistry::Args& a) {
        TileDocument* d;
        float x, z, r, g, bl, tol = 3;
        if (!need(d) || a.size() < 5 || !number(a[0], x) || !number(a[1], z) || !number(a[2], r) || !number(a[3], g) || !number(a[4], bl)) return fail("usage: assert_ground_pixel <x m> <z m> <r> <g> <b> [tolerance]");
        if (a.size() > 5) number(a[5], tol);
        const BakedColour* b = d->baked_colour();
        if (!b) return fail("the ground is not blended");
        const int ix = std::clamp(int((x - b->x0) / b->w_m * b->w), 0, int(b->w) - 1), iz = std::clamp(int((z - b->z0) / b->h_m * b->h), 0, int(b->h) - 1);
        const uint8_t* p = &b->rgba[(size_t(iz) * b->w + size_t(ix)) * 4];
        if (std::fabs(p[0] - r) > tol || std::fabs(p[1] - g) > tol || std::fabs(p[2] - bl) > tol) return fail("pixel is " + std::to_string(p[0]) + " " + std::to_string(p[1]) + " " + std::to_string(p[2]));
        return ok();
    });

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
