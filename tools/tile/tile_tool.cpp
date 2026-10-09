#include "tile/tile_tool.h"
#include "tool_factories.h"

#include "editor/editor.h"
#include "editor/steam_detect.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "vfs/vfs.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <fstream>
#include <iterator>

#include <imgui_internal.h>

namespace fs = std::filesystem;

namespace wce {

namespace {

const char* kPolygonTypes[] = {"AI_HINT_POLYGON_INSTANCE", "NOGO_REGION_POLYGON_INSTANCE", "GO_REGION_POLYGON_INSTANCE", "VegetationExclusionPolygonInstance", "SIEGE_AI_POLYGON_INSTANCE"};
const char* kHints[] = {"AIH_FORT_APPROACH", "AIH_DEFENSIVE_HILL", "AIH_FORT_PERIMETER", "AIH_AMBUSH_CAVALRY_LINE", "AIH_AMBUSH_MELEE_LINE", "AIH_AMBUSH_MISSILE_LINE",
                       "AIH_AMBUSH_ASSAULT_AREA", "AIH_AMBUSH_DEFENDER_INITIAL_PATH", "AIH_AMBUSH_FOREST", "AIH_AMBUSH_FIREBALL_LINE", "AIH_BEACH_LANDING",
                       "AIH_DEFAULT_DEPLOYMENT_LINE", "AIH_RIVER_EDGE", "AIH_BRIDGE_CROSSING", "AIH_COVER_LINE", "AIH_BEACH_LANDING_FORT_APPROACH"};

fs::path tile_dir_of(const std::string& path) {
    fs::path p(widen(path));
    std::error_code ec;
    if (fs::is_directory(p, ec)) return p;
    return p.parent_path();
}

} // namespace

const char* TileTool::polygon_type(int i) { return kPolygonTypes[i]; }
const char* TileTool::hint(int i) { return kHints[i]; }

TileDocument* TileTool::document(Editor& editor) {
    if (auto* d = dynamic_cast<TileDocument*>(editor.documents().active())) return d;
    for (DocumentId id : editor.documents().order())
        if (auto* d = dynamic_cast<TileDocument*>(editor.documents().find(id))) return d;
    return nullptr;
}

std::vector<int> TileTool::selected(Editor& editor) {
    std::vector<int> out;
    TileDocument* d = document(editor);
    if (!d) return out;
    for (const SourceRef& r : editor.selection().items())
        if (r.doc == d->id() && r.item != TileDocument::kTerrain) out.push_back(TileDocument::elem_of(r.item));
    return out;
}

int TileTool::primary(Editor& editor) {
    TileDocument* d = document(editor);
    if (!d) return -1;
    for (int e : selected(editor))
        if (d->is_instance(e)) return e;
    return -1;
}

void TileTool::select_elem(Editor& editor, int elem, bool add) {
    TileDocument* d = document(editor);
    if (!d) return;
    if (elem < 0) { if (!add) editor.selection().clear(); return; }
    if (add) editor.selection().add(d->ref_of(elem));
    else editor.selection().set(d->ref_of(elem));
    editor.documents().set_active(d->id());
}

// ---- opening

std::unique_ptr<Document> TileTool::open(Editor& editor, const std::string& path, bool from_packs) {
    if (from_packs) throw std::runtime_error("tiles are opened from the assembly kit folders on disk");
    const fs::path dir = tile_dir_of(path);
    std::unique_ptr<TileProject> project = TileProject::open(dir);   // throws
    return std::make_unique<TileDocument>(std::move(project), editor.vfs());
}

bool TileTool::open_folder(Editor& editor, const std::string& dir, std::string* message) {
    std::string msg;
    const fs::path tile = fs::path(widen(dir));
    std::error_code ec;
    const bool ok = editor.open_path(narrow((fs::is_directory(tile, ec) ? tile / "tile.ted" : tile).wstring()), false, &msg);
    if (message) *message = msg;
    return ok;
}

void TileTool::load_environment_map(const std::string& tile_dir) {
    env_.reset();
    std::string g = tile_dir;
    for (char& c : g)
        if (c == '\\') c = '/';
    const size_t at = g.find("/raw_data/");
    if (at == std::string::npos) { env_status_ = "no raw_data folder in the tile path"; return; }
    const fs::path p = fs::path(widen(g.substr(0, at + 10))) / "warscape_asset_variation_db" / "environment_map.xml";
    try {
        std::ifstream in(p, std::ios::binary);
        if (!in) throw std::runtime_error("cannot read " + narrow(p.wstring()));
        std::vector<uint8_t> b((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        env_ = std::make_unique<VariationDb>(VariationDb::parse_environment_map(b));
        env_status_ = std::to_string(env_->entries[0].variations.size()) + " environment files";
    } catch (const std::exception& e) {
        env_status_ = std::string("environment_map.xml: ") + e.what();
    }
}

void TileTool::default_layout(ImGuiID dockspace) {
    ImGuiID center = dockspace;
    const ImGuiID left = ImGui::DockBuilderSplitNode(center, ImGuiDir_Left, 0.22f, nullptr, &center);
    const ImGuiID right = ImGui::DockBuilderSplitNode(center, ImGuiDir_Right, 0.30f, nullptr, &center);
    const ImGuiID bottom = ImGui::DockBuilderSplitNode(center, ImGuiDir_Down, 0.18f, nullptr, &center);
    ImGui::DockBuilderDockWindow("Viewport", center);
    ImGui::DockBuilderDockWindow("Layers", left);
    ImGui::DockBuilderDockWindow("Outliner", left);
    ImGui::DockBuilderDockWindow("Tile", right);
    ImGui::DockBuilderDockWindow("Inspector", right);
    ImGui::DockBuilderDockWindow("Assets", bottom);
    ImGui::DockBuilderDockWindow("Log", bottom);
    ImGui::DockBuilderDockWindow("History", bottom);
}

void TileTool::activated(Editor& editor) {
    editor.scene().environment = EnvironmentSlot{};   // the default environment until a weather document or the picker lights it (Phase 7)
    if (editor.viewport().is_fly()) editor.viewport().use_orbit();
    if (auto* orbit = dynamic_cast<OrbitCamera*>(&editor.viewport().controller())) orbit->min_eye_height = -1e6f;
}

// ---- per frame

float TileTool::handle_size(Editor& editor) const {
    const Camera c = editor.viewport().camera();
    return std::max(0.8f, length(c.eye - c.target) * 0.012f);
}

Overlay TileTool::tool_overlay(Editor& editor, TileDocument& d) {
    Overlay o;
    o.depth_test = false;
    o.pickable = false;
    const uint32_t gold = overlay_colour(1.0f, 0.85f, 0.2f);
    if (mode_ != TileMode::Select && terrain_hit_ && editor.viewport().hovered) {
        const int n = 64;
        float3 prev{};
        for (int i = 0; i <= n; ++i) {
            const float a = float(i) * 6.2831853f / float(n);
            float3 p{cursor_.x + std::cos(a) * brush_.radius_m, 0, cursor_.z + std::sin(a) * brush_.radius_m};
            p.y = d.terrain_height(p.x, p.z) + 0.4f;
            if (i) o.line(prev, p, gold);
            prev = p;
        }
    }
    if (drawing_) {
        const float r = handle_size(editor);
        const uint32_t hot = overlay_colour(1.0f, 0.9f, 0.1f);
        for (size_t i = 0; i < draw_pts_.size(); ++i) {
            const float3& p = draw_pts_[i];
            o.line({p.x - r, p.y + 0.4f, p.z}, {p.x + r, p.y + 0.4f, p.z}, hot);
            o.line({p.x, p.y + 0.4f, p.z - r}, {p.x, p.y + 0.4f, p.z + r}, hot);
            o.line({p.x, p.y, p.z}, {p.x, p.y + r * 1.5f, p.z}, hot);
            if (i) o.line({draw_pts_[i - 1].x, draw_pts_[i - 1].y + 0.4f, draw_pts_[i - 1].z}, {p.x, p.y + 0.4f, p.z}, hot);
        }
    }
    if (d.display().show_border) {
        const float ex = float(d.extent_x()), ez = float(d.extent_z());
        const uint32_t white = overlay_colour(0.9f, 0.9f, 0.9f);
        const int seg = 32;
        auto edge = [&](float x0, float z0, float x1, float z1) {
            float3 prev{};
            for (int i = 0; i <= seg; ++i) {
                const float t = float(i) / float(seg);
                float3 p{x0 + (x1 - x0) * t, 0, z0 + (z1 - z0) * t};
                p.y = d.terrain_height(p.x, p.z) + 0.3f;
                if (i) o.line(prev, p, white);
                prev = p;
            }
        };
        edge(0, 0, ex, 0);
        edge(ex, 0, ex, ez);
        edge(ex, ez, 0, ez);
        edge(0, ez, 0, 0);
    }
    return o;
}

void TileTool::update(Editor& editor) {
    TileDocument* d = document(editor);
    if (!d) return;
    const LayerId layer = editor.documents().layer_of(d->id());
    // A new tile: frame it, and read the environment map of its kit.
    if (camera_set_for_ != d->id()) {
        camera_set_for_ = d->id();
        const double ex = d->extent_x(), ez = d->extent_z();
        if (auto* orbit = dynamic_cast<OrbitCamera*>(&editor.viewport().controller()))
            orbit->set_orbit(0.8f, 0.6f, float(std::max(ex, ez) * 1.2), {float(ex * 0.5), 0.f, float(ez * 0.5)});
        load_environment_map(narrow(d->project().dir.wstring()));
        // the weather tool's sample map would show its ground plane through the tile (the weather tool shows it again when it becomes active)
        for (DocumentId id : editor.documents().order())
            if (const Document* o = editor.documents().find(id); o && o->kind() == "sample_map") editor.documents().set_visible(id, false);
        poly_point_ = -1;
        drawing_ = false;
        grabbing_ = false;
        stroke_active_ = false;
        mode_ = TileMode::Select;
    }
    // Structural edits move element indices: the command says what is selected afterwards.
    if (auto hint = d->take_selection_hint()) {
        editor.selection().clear();
        for (uint64_t item : *hint) editor.selection().add({d->id(), item});
        poly_point_ = -1;
    }
    const Camera cam = editor.viewport().camera();
    d->update_view(editor.scene(), layer, cam.eye);
    d->set_highlight(editor.scene(), selected(editor), poly_point_, handle_size(editor), tool_overlay(editor, *d));
}

// ---- mouse

bool TileTool::cursor_ray(Editor& editor, int x, int y, float3& o, float3& d) const { return editor.viewport().ray(x, y, o, d); }

void TileTool::on_hover(Editor& editor, TileDocument& d, const ViewportEvent& ev) {
    float3 o, dir, hit;
    terrain_hit_ = false;
    if (!cursor_ray(editor, ev.x, ev.y, o, dir)) return;
    if (d.pick_terrain(o, dir, hit)) {
        terrain_hit_ = true;
        cursor_ = hit;
    }
    if (grabbing_ && terrain_hit_ && grab_elem_ >= 0) {
        float gx = cursor_.x, gz = cursor_.z;
        if (snap_grid_ && grid_m_ > 0.01f) {
            gx = std::round(gx / grid_m_) * grid_m_;
            gz = std::round(gz / grid_m_) * grid_m_;
        }
        const float gy = d.terrain_height(gx, gz) + float(grab_y_offset_);
        d.set_world_metres(grab_elem_, {gx, gy, gz}, grab_gesture_);
    }
}

bool TileTool::viewport_input(Editor& editor, const ViewportEvent& ev) {
    TileDocument* d = document(editor);
    if (!d) return false;
    using K = ViewportEvent::Kind;
    float3 o, dir;
    const bool have_ray = cursor_ray(editor, ev.x, ev.y, o, dir);
    switch (ev.kind) {
    case K::Hover:
        on_hover(editor, *d, ev);
        return false;
    case K::Press: {
        on_hover(editor, *d, ev);
        if (grabbing_) { end_grab(editor, true); return true; }
        if (drawing_) {
            if (terrain_hit_) draw_pts_.push_back(cursor_);
            return true;
        }
        if (mode_ != TileMode::Select) {
            if (terrain_hit_ && d->project().has_height) {
                stroke_active_ = true;
                d->brush_step(BrushKind(int(mode_) - 1), cursor_.x, cursor_.z, brush_, editor.clock().dt());
            }
            return true;
        }
        // select mode: a polygon point handle starts a drag, shift + click near an edge inserts a point
        const int sel = primary(editor);
        if (have_ray && sel >= 0 && d->is_polygon(sel)) {
            const auto pts = d->polygon_world_points(sel);
            int best = -1;
            float best_t = 1e30f;
            for (size_t i = 0; i < pts.size(); ++i) {
                const float3 c = pts[i];
                const float t = dot(c - o, dir);
                if (t <= 0) continue;
                const float perp = length(c - (o + dir * t));
                if (perp < std::max(1.2f, t * 0.015f) && t < best_t) { best_t = t; best = int(i); }
            }
            if (best >= 0) {
                poly_point_ = best;
                point_drag_ = true;
                point_gesture_ = d->next_gesture();
                return true;
            }
            if (ev.shift && terrain_hit_) {
                const bool closed = d->tscene().doc().get(sel, "polygon_closed").value_or("1") != "0";
                size_t at = 0;
                float bd = 1e30f;
                for (size_t i = 0; i < pts.size(); ++i) {
                    if (i + 1 == pts.size() && !closed) break;
                    const float3 &a = pts[i], &b = pts[(i + 1) % pts.size()];
                    const float abx = b.x - a.x, abz = b.z - a.z, l2 = abx * abx + abz * abz;
                    const float t = l2 > 1e-6f ? std::clamp(((cursor_.x - a.x) * abx + (cursor_.z - a.z) * abz) / l2, 0.f, 1.f) : 0.f;
                    const float dx = a.x + abx * t - cursor_.x, dz = a.z + abz * t - cursor_.z, dd = dx * dx + dz * dz;
                    if (dd < bd) { bd = dd; at = i; }
                }
                std::string err;
                if (d->polygon_point(sel, "insert", int(at), cursor_, &err)) {
                    poly_point_ = int(at) + 1;
                    status = "point inserted";
                }
                return true;
            }
        }
        return false;
    }
    case K::Drag: {
        if (stroke_active_) {
            on_hover(editor, *d, ev);
            if (terrain_hit_) d->brush_step(BrushKind(int(mode_) - 1), cursor_.x, cursor_.z, brush_, editor.clock().dt());
        } else if (point_drag_) {
            on_hover(editor, *d, ev);
            const int sel = primary(editor);
            if (terrain_hit_ && sel >= 0 && poly_point_ >= 0) {
                const auto pts = d->tscene().polygon_points(sel);
                if (size_t(poly_point_) < pts.size()) {
                    const Vec3 before = d->tscene().point_position(pts[size_t(poly_point_)]).value_or(Vec3{0, 0, 0});
                    const Vec3 after = d->to_local(sel, cursor_);
                    if (before != after) d->polygon_point(sel, "move", poly_point_, cursor_, nullptr, point_gesture_);
                }
            }
        }
        return stroke_active_ || point_drag_;
    }
    case K::Release:
        if (stroke_active_) {
            stroke_active_ = false;
            d->end_stroke();
        }
        if (point_drag_) {
            point_drag_ = false;
            d->history().break_merge();
        }
        return true;
    case K::Click: {
        // select: the model piece under the cursor (exact), else the nearest instance sphere along the ray
        int hit = -1;
        if (ev.hit_source.doc == d->id() && ev.hit_source.item != TileDocument::kTerrain) hit = TileDocument::elem_of(ev.hit_source.item);
        if (hit < 0 && have_ray) hit = d->pick_instance(o, dir);
        if (hit < 0) {
            if (!ev.ctrl) editor.selection().clear();
            poly_point_ = -1;
            return true;
        }
        if (ev.ctrl) editor.selection().toggle(d->ref_of(hit));
        else if (ev.shift) editor.selection().add(d->ref_of(hit));
        else editor.selection().set(d->ref_of(hit));
        editor.documents().set_active(d->id());
        poly_point_ = -1;
        return true;
    }
    }
    return false;
}

// ---- grab, polygons

bool TileTool::begin_grab(Editor& editor) {
    TileDocument* d = document(editor);
    const int e = primary(editor);
    if (!d || e < 0 || grabbing_) return false;
    const float3 p = d->world_position(e);
    grab_elem_ = e;
    grab_y_offset_ = p.y - d->terrain_height(p.x, p.z);
    grab_history_ = d->history().position();
    grab_gesture_ = d->next_gesture();
    grabbing_ = true;
    status = "grab: move the mouse over the ground, click to confirm, Esc / right click to cancel";
    return true;
}

void TileTool::end_grab(Editor& editor, bool commit) {
    TileDocument* d = document(editor);
    if (!grabbing_) return;
    grabbing_ = false;
    if (!d) return;
    d->history().break_merge();
    if (!commit) {
        d->history().jump_to(grab_history_);
        d->history().drop_redo();
    }
    grab_elem_ = -1;
}

void TileTool::finish_polygon(Editor& editor) {
    TileDocument* d = document(editor);
    if (!drawing_ || !d) return;
    drawing_ = false;
    std::string err;
    const int sel = primary(editor);
    const int layer = sel >= 0 ? d->layer_of(sel) : d->tscene().scene_graph();
    const int e = d->add_polygon(layer, kPolygonTypes[new_poly_type_], kHints[new_hint_], draw_pts_, &err);
    draw_pts_.clear();
    poly_point_ = -1;
    status = e >= 0 ? std::string("added ") + kPolygonTypes[new_poly_type_] + " in " + d->tscene().layer_path(e) : err;
}

void TileTool::handle_keys(Editor& editor) {
    ImGuiIO& io = ImGui::GetIO();
    TileDocument* d = document(editor);
    if (!d || io.WantTextInput) return;
    const int sel = primary(editor);
    if (ImGui::IsKeyPressed(ImGuiKey_Escape, false)) {
        if (grabbing_) end_grab(editor, false);
        else if (drawing_) { drawing_ = false; draw_pts_.clear(); }
        else if (mode_ != TileMode::Select) mode_ = TileMode::Select;
    }
    if (grabbing_ && ImGui::IsMouseClicked(ImGuiMouseButton_Right)) end_grab(editor, false);
    if (io.KeyCtrl) {
        if (ImGui::IsKeyPressed(ImGuiKey_D, false) && sel >= 0 && !grabbing_) {
            std::string err;
            const int copy = d->duplicate(sel, &err);
            status = copy >= 0 ? "duplicated" : err;
        }
        if (ImGui::IsKeyPressed(ImGuiKey_R, false) && sel >= 0 && !grabbing_) d->rotate(sel, io.KeyShift ? -angle_step_ : angle_step_);
        return;
    }
    if (ImGui::IsKeyPressed(ImGuiKey_G, false) && sel >= 0) begin_grab(editor);
    if (ImGui::IsKeyPressed(ImGuiKey_Delete, false) && !grabbing_) {
        if (sel >= 0 && poly_point_ >= 0 && d->is_polygon(sel)) {
            std::string err;
            if (d->polygon_point(sel, "remove", poly_point_, {}, &err)) status = "point removed";
            else status = err;
            poly_point_ = -1;
        } else if (!selected(editor).empty()) {
            for (int e : selected(editor)) { d->remove(e); break; }   // one structural edit at a time: indices move
            status = "deleted";
        }
    }
    if (drawing_ && (ImGui::IsKeyPressed(ImGuiKey_Enter, false) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter, false))) finish_polygon(editor);
}

// ---- panels

void TileTool::draw_panels(Editor& editor) {
    handle_keys(editor);
    draw_tile_panel(editor);
    draw_layers_panel(editor);
}

void TileTool::draw_menus(Editor& editor) {
    TileDocument* d = document(editor);
    if (ImGui::BeginMenu("Tile")) {
        if (ImGui::MenuItem("Open tile...")) {
            const fs::path p = file_dialog(false, {}, L"Tile (tile.ted)\0tile.ted;*.ted\0All files\0*.*\0");
            std::string msg;
            if (!p.empty()) editor.status(editor.open_path(narrow(p.wstring()), false, &msg) ? msg : "open failed: " + msg, false);
        }
        if (d) {
            TileDisplay& v = d->display();
            if (ImGui::MenuItem("Polygon outlines", nullptr, &v.show_polygons)) d->display_changed();
            if (ImGui::MenuItem("Distance culling of markers", nullptr, &v.lod)) d->display_changed();
            ImGui::MenuItem("Tile border", nullptr, &v.show_border);
            ImGui::Separator();
            ImGui::TextUnformatted("Draw distances (m)");
            ImGui::SetNextItemWidth(160);
            if (ImGui::DragFloat("vegetation", &v.veg_distance, 5.f, 20.f, 20000.f)) d->display_changed();
            ImGui::SetNextItemWidth(160);
            if (ImGui::DragFloat("decals", &v.decal_distance, 5.f, 20.f, 20000.f)) d->display_changed();
            ImGui::SetNextItemWidth(160);
            if (ImGui::DragFloat("others", &v.other_distance, 5.f, 20.f, 40000.f)) d->display_changed();
        }
        ImGui::EndMenu();
    }
}

void TileTool::draw_tile_panel(Editor& editor) {
    ImGui::SetNextWindowSize(ImVec2(460, 640), ImGuiCond_FirstUseEver);
    if (!ImGui::Begin("Tile")) { ImGui::End(); return; }
    TileDocument* d = document(editor);
    if (!d) {
        ImGui::TextWrapped("No tile open. File > Open file... (a tile.ted), Tile > Open tile..., or --tile <folder>.");
        ImGui::End();
        return;
    }
    TileProject& p = d->project();
    ImGui::Text("%s", p.dir.filename().string().c_str());
    ImGui::TextDisabled("%s", p.dir.parent_path().filename().string().c_str());
    ImGui::Separator();
    const char* names[] = {"Select", "Raise", "Lower", "Smooth", "Flatten", "Level"};
    for (int i = 0; i < 6; ++i) {
        if (i) ImGui::SameLine();
        if (ImGui::RadioButton(names[i], mode_ == TileMode(i))) set_mode(TileMode(i));
    }
    ImGui::SliderFloat("radius (m)", &brush_.radius_m, 1.f, 120.f);
    ImGui::SliderFloat("strength", &brush_.strength, 0.5f, 30.f);
    ImGui::SliderFloat("falloff", &brush_.falloff, 0.f, 1.f);
    ImGui::DragFloat("flatten height (m)", &brush_.flatten_height, 0.1f);
    if (terrain_hit_) ImGui::Text("cursor: %.1f, %.1f  height %.2f m", cursor_.x, cursor_.z, d->terrain_height(cursor_.x, cursor_.z));
    else ImGui::TextDisabled("cursor: -");
    if (p.has_height) {
        const HeightField& h = *p.height;
        ImGui::Text("height map %ux%u, %.2f..%.2f m (min %.3f scale %.3f)%s", h.width(), h.height(), h.min_height(), h.min_height() + h.scale(), p.info.min_height, p.info.scale,
                    p.info.found ? "" : "  [tile database entry NOT found]");
    } else ImGui::TextDisabled("no height_map_0.png");
    ImGui::Text("tile %u x %u cells, %.0f x %.0f m", p.info.cells_x, p.info.cells_y, d->extent_x(), d->extent_z());
    ImGui::Text("tile.ted v%d, %zu instances%s", d->tscene().project_version(), d->instances().size(), d->modified() ? "  (modified)" : "");
    ImGui::Text("undo %zu / redo %zu", d->history().position(), d->history().size() - d->history().position());
    if (!d->model_fallbacks().empty()) ImGui::TextColored(ImVec4(1, 0.7f, 0.3f, 1), "%zu building keys have no model (boxes), see the log", d->model_fallbacks().size());
    ImGui::Separator();
    ImGui::Checkbox("snap to grid", &snap_grid_);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90);
    ImGui::DragFloat("grid (m)", &grid_m_, 0.5f, 0.5f, 64.f);
    ImGui::SetNextItemWidth(90);
    ImGui::DragFloat("rotate step (deg)", &angle_step_, 1.f, 1.f, 180.f);
    ImGui::InputText("building key", building_key_, sizeof building_key_);
    ImGui::SameLine();
    if (ImGui::Button("Add")) {
        std::string err;
        const int sel = primary(editor);
        const int e = d->add_building(building_key_, sel, terrain_hit_ ? &cursor_ : nullptr, &err);
        status = e >= 0 ? std::string("added ") + building_key_ : err;
    }
    const int sel = primary(editor);
    ImGui::BeginDisabled(sel < 0);
    if (ImGui::Button("Grab on ground (G)")) begin_grab(editor);
    ImGui::SameLine();
    if (ImGui::Button("Drop to terrain") && sel >= 0) {
        float3 w = d->world_position(sel);
        w.y = d->terrain_height(w.x, w.z);
        d->set_world_metres(sel, w, 0);
    }
    if (ImGui::Button("Duplicate (Ctrl+D)")) { std::string err; if (d->duplicate(sel, &err) < 0) status = err; }
    ImGui::SameLine();
    if (ImGui::Button("Delete (Del)")) d->remove(sel);
    ImGui::SameLine();
    if (ImGui::Button("Rotate (Ctrl+R)")) d->rotate(sel, angle_step_);
    ImGui::EndDisabled();
    ImGui::Separator();
    draw_polygon_panel(editor, *d);
    ImGui::Separator();
    draw_weather_panel();
    if (!status.empty()) {
        ImGui::Separator();
        ImGui::TextWrapped("%s", status.c_str());
    }
    ImGui::End();
}

void TileTool::draw_polygon_panel(Editor& editor, TileDocument& d) {
    if (!ImGui::CollapsingHeader("Polygons / AI hints", ImGuiTreeNodeFlags_DefaultOpen)) return;
    ImGui::SetNextItemWidth(260);
    if (ImGui::BeginCombo("type", kPolygonTypes[new_poly_type_])) {
        for (int i = 0; i < kPolygonTypeCount; ++i)
            if (ImGui::Selectable(kPolygonTypes[i], i == new_poly_type_)) new_poly_type_ = i;
        ImGui::EndCombo();
    }
    if (new_poly_type_ == 0) {
        ImGui::SetNextItemWidth(260);
        if (ImGui::BeginCombo("hint", kHints[new_hint_])) {
            for (int i = 0; i < kHintCount; ++i)
                if (ImGui::Selectable(kHints[i], i == new_hint_)) new_hint_ = i;
            ImGui::EndCombo();
        }
    }
    if (!drawing_) {
        if (ImGui::Button("Draw polygon")) {
            start_drawing();
            status = "click the terrain to add points; Enter finishes, Esc cancels";
        }
    } else {
        ImGui::Text("%zu points", draw_pts_.size());
        ImGui::SameLine();
        if (ImGui::Button("Finish")) finish_polygon(editor);
        ImGui::SameLine();
        if (ImGui::Button("Cancel")) { drawing_ = false; draw_pts_.clear(); }
    }
    const int sel = primary(editor);
    if (sel >= 0 && d.is_polygon(sel)) {
        ImGui::Text("selected: %zu points, area %.1f", d.tscene().polygon_points(sel).size(), d.tscene().polygon_area(sel));
        if (poly_point_ >= 0) ImGui::Text("point %d: drag to move, Delete removes, shift+click edge inserts", poly_point_);
        else ImGui::TextDisabled("click a point handle to edit; shift+click near an edge inserts a point");
    }
}

void TileTool::draw_weather_panel() {
    if (!ImGui::CollapsingHeader("Weather / climate (which environment the game picks)")) return;
    if (!env_) { ImGui::TextDisabled("%s", env_status_.c_str()); return; }
    auto combo = [&](const char* label, const char* category) {
        std::string& value = wq_[category];
        const auto* meta = env_->meta(category);
        const std::vector<std::string> vals = meta ? meta->values : std::vector<std::string>{};
        if (ImGui::BeginCombo(label, value.empty() ? "(any)" : value.c_str())) {
            if (ImGui::Selectable("(any)", value.empty())) value.clear();
            for (const auto& v : vals)
                if (ImGui::Selectable(v.c_str(), v == value)) value = v;
            ImGui::EndCombo();
        }
    };
    combo("map", "map");
    combo("climate", "climate");
    combo("season", "season");
    combo("climate change", "climate_change_index");
    combo("precipitation", "precipitation");
    combo("time of day", "time_of_day");
    combo("battle type", "battle_types");
    combo("snow attrition", "snow_attrition");
    VariationDb::Query query;
    for (const auto& [category, value] : wq_)
        if (!value.empty()) query[category] = value;
    const auto res = env_->candidates_listed_map("environment", query);
    const auto& m = res.candidates;
    ImGui::Text("%zu matching files%s, each picked with probability 1/%zu", m.size(), res.used_default_map ? " (after the default_map retry)" : "", m.empty() ? size_t(1) : m.size());
    ImGui::BeginChild("envlist", ImVec2(0, 140), true);
    for (const auto& c : m) ImGui::TextUnformatted(c.variation->filename.c_str());
    ImGui::EndChild();
}

void TileTool::draw_layers_panel(Editor& editor) {
    ImGui::SetNextWindowSize(ImVec2(380, 600), ImGuiCond_FirstUseEver);
    if (!ImGui::Begin("Layers")) { ImGui::End(); return; }
    TileDocument* d = document(editor);
    if (!d) { ImGui::TextDisabled("No tile open."); ImGui::End(); return; }
    if (ImGui::CollapsingHeader("Instance types", ImGuiTreeNodeFlags_DefaultOpen)) {
        TileDisplay& v = d->display();
        for (const auto& [t, n] : d->type_counts()) {
            bool vis = v.hidden_types.count(t) == 0;
            char l[200];
            std::snprintf(l, sizeof l, "%s (%d)##t", t.c_str(), n);
            if (ImGui::Checkbox(l, &vis)) {
                if (vis) v.hidden_types.erase(t); else v.hidden_types.insert(t);
                d->display_changed();
            }
        }
    }
    if (ImGui::CollapsingHeader("Layers", ImGuiTreeNodeFlags_DefaultOpen)) {
        const int g = d->tscene().scene_graph();
        if (g >= 0) draw_layer_node(editor, *d, g, 0);
        else ImGui::TextDisabled("This tile has no SCENE_GRAPH.");
    }
    ImGui::End();
}

void TileTool::draw_layer_node(Editor& editor, TileDocument& d, int elem, int depth) {
    TedScene& sc = d.tscene();
    const auto kids = sc.children(elem);
    const bool layer = sc.is_layer(elem) || !kids.empty();
    std::string label = sc.name(elem);
    if (label.empty()) label = sc.type(elem);
    char buf[300];
    std::snprintf(buf, sizeof buf, "%s  [%s%s]###n%d", label.c_str(), sc.type(elem).c_str(), kids.empty() ? "" : (", " + std::to_string(kids.size())).c_str(), elem);
    const bool is_sel = editor.selection().contains(d.ref_of(elem));
    ImGuiTreeNodeFlags fl = ImGuiTreeNodeFlags_SpanAvailWidth | (layer ? 0 : ImGuiTreeNodeFlags_Leaf) | (is_sel ? ImGuiTreeNodeFlags_Selected : 0);
    if (depth < 1) fl |= ImGuiTreeNodeFlags_DefaultOpen;
    const bool open = ImGui::TreeNodeEx(buf, fl);
    if (ImGui::IsItemClicked() && !ImGui::IsItemToggledOpen()) select_elem(editor, elem);
    if (ImGui::BeginPopupContextItem()) {
        const bool is_layer = sc.is_layer(elem);
        const int sel = primary(editor);
        std::string err;
        if (is_layer && ImGui::MenuItem("New layer inside")) {
            const int l = d.add_layer(elem, "New layer", &err);
            status = l >= 0 ? "layer added" : err;
        }
        if (is_layer && sel >= 0 && sel != elem && ImGui::MenuItem("Move selected here")) {
            status = d.move_to_layer(sel, elem, &err) ? "moved to layer " + sc.name(elem) : err;
        }
        if (is_layer && sc.type(elem) != "SCENE_GRAPH") {
            if (ImGui::BeginMenu("Rename")) {
                if (ImGui::IsWindowAppearing()) std::snprintf(rename_buf_, sizeof rename_buf_, "%s", sc.name(elem).c_str());
                if (ImGui::InputText("##rename", rename_buf_, sizeof rename_buf_, ImGuiInputTextFlags_EnterReturnsTrue)) {
                    d.rename(elem, rename_buf_);
                    ImGui::CloseCurrentPopup();
                }
                ImGui::EndMenu();
            }
            if (ImGui::MenuItem("Delete layer and contents")) { d.remove_layer(elem); status = "layer deleted"; }
        }
        ImGui::EndPopup();
    }
    if (open) {
        size_t shown = 0;
        for (int k : kids) {
            if (++shown > 300) { ImGui::TextDisabled("... %zu more", kids.size() - 300); break; }
            draw_layer_node(editor, d, k, depth + 1);
        }
        ImGui::TreePop();
    }
}

std::unique_ptr<Tool> make_tile_tool() { return std::make_unique<TileTool>(); }

} // namespace wce
