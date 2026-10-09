// The Scene panel of the Tile tool: the layer tree laid out like TEd's Scene tab (a name, then an eye and a cursor arrow at the right edge that are greyed when
// off), with selection, rename (F2 / double-click), delete, new layer, group, duplicate, export type, and drag and drop to reorder or move into a layer.
// The eye is OIF_VISIBLE and the cursor arrow OIF_ACTIVE (selectable): TileDocument::set_flags. The tree is flattened to the rows that are open and drawn through a list
// clipper, so a layer with 30,000 instances costs what its visible rows cost. Open layers are remembered by their name path, which survives the index shifts of structural
// edits (a node id made of the element index would not).
#include "editor/editor.h"
#include "foundation/log.h"
#include "tile/tile_tool.h"

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <functional>
#include <unordered_map>

#include <imgui_internal.h>

namespace wce {

namespace {

constexpr float kIconWidth = 22.0f;
constexpr size_t kConfirmDeleteAbove = 200;   // elements below a deleted layer: ask first

int parent_layer(const TedScene& sc, int e) {
    for (int p = sc.doc().elements()[size_t(e)].parent; p >= 0; p = sc.doc().elements()[size_t(p)].parent)
        if (sc.is_layer(p)) return p;
    return -1;
}

void draw_eye(ImDrawList* dl, ImVec2 c, bool on, bool inherited_off) {
    const ImU32 col = on ? (inherited_off ? IM_COL32(150, 150, 150, 255) : IM_COL32(235, 235, 235, 255)) : IM_COL32(95, 95, 95, 255);
    dl->AddEllipse(c, ImVec2(6.5f, 3.6f), col, 0.0f, 20, 1.3f);
    dl->AddCircleFilled(c, 2.2f, col);
    if (!on) dl->AddLine(ImVec2(c.x - 6, c.y + 4), ImVec2(c.x + 6, c.y - 4), col, 1.3f);
}

void draw_cursor(ImDrawList* dl, ImVec2 c, bool on, bool inherited_off) {
    const ImU32 col = on ? (inherited_off ? IM_COL32(150, 150, 150, 255) : IM_COL32(235, 235, 235, 255)) : IM_COL32(95, 95, 95, 255);
    const ImVec2 tip(c.x - 3.5f, c.y - 6);
    dl->AddTriangleFilled(tip, ImVec2(tip.x, tip.y + 11), ImVec2(tip.x + 7.5f, tip.y + 8), col);
    dl->AddLine(ImVec2(tip.x + 3, tip.y + 8), ImVec2(tip.x + 6, tip.y + 13), col, 1.8f);
}

void draw_layer_icon(ImDrawList* dl, ImVec2 c, ImU32 col) {
    for (int i = 0; i < 3; ++i) dl->AddRectFilled(ImVec2(c.x - 6, c.y - 5 + float(i) * 3.5f), ImVec2(c.x + 6, c.y - 3 + float(i) * 3.5f), col, 1.0f);
}

} // namespace

void TileTool::rebuild_tree(TileDocument& d) {
    tree_rows_.clear();
    TedScene& sc = d.tscene();
    const int g = sc.scene_graph();
    if (g < 0) return;
    std::function<void(int, int, const std::string&)> walk = [&](int e, int depth, const std::string& key) {
        const bool layer = sc.is_layer(e);
        std::vector<int> kids;
        if (layer) kids = sc.children(e);
        const bool open = depth == 0 || (layer && !kids.empty() && tree_open_.count(key) != 0);
        tree_rows_.push_back({e, depth, key, layer, open, kids.size()});
        if (!open) return;
        std::unordered_map<std::string, int> seen;
        for (int k : kids) {
            std::string nm = sc.name(k);
            if (nm.empty()) nm = sc.type(k);
            const int ordinal = seen[nm]++;
            walk(k, depth + 1, key + "/" + nm + "#" + std::to_string(ordinal));
        }
    };
    walk(g, 0, "root");
    tree_doc_ = d.id();
    tree_version_ = d.structure_version();
    tree_open_version_seen_ = tree_open_version_;
}

// Delete the selection (layers with everything in them), asking first when a layer holds a lot.
void TileTool::request_delete(Editor& editor) {
    TileDocument* d = document(editor);
    if (!d) return;
    const std::vector<int> sel = selected(editor);
    if (sel.empty()) return;
    size_t weight = 0;
    for (int e : sel) weight += size_t(d->tscene().doc().subtree_size(e));
    if (weight > kConfirmDeleteAbove && std::any_of(sel.begin(), sel.end(), [&](int e) { return d->tscene().is_layer(e); })) {
        delete_confirm_ = true;
        delete_weight_ = weight;
        return;
    }
    d->remove_items(sel);
    status = "deleted";
}

void TileTool::begin_rename(Editor& editor, int elem) {
    TileDocument* d = document(editor);
    if (!d) return;
    rename_elem_ = elem;
    rename_focus_ = true;
    std::snprintf(rename_buf_, sizeof rename_buf_, "%s", d->tscene().name(elem).c_str());
}

void TileTool::draw_layers_panel(Editor& editor) {
    ImGui::SetNextWindowSize(ImVec2(380, 600), ImGuiCond_FirstUseEver);
    if (!ImGui::Begin("Scene")) { ImGui::End(); return; }
    TileDocument* d = document(editor);
    if (!d) { ImGui::TextDisabled("No tile open."); ImGui::End(); return; }
    TedScene& sc = d->tscene();
    const int graph = sc.scene_graph();
    if (graph < 0) {
        ImGui::TextDisabled("This tile has no SCENE_GRAPH.");
        ImGui::End();
        return;
    }
    if (tree_doc_ != d->id() || tree_version_ != d->structure_version() || tree_open_version_seen_ != tree_open_version_) rebuild_tree(*d);

    // ---- toolbar
    {
        const std::vector<int> sel = selected(editor);
        int target = graph;   // where a new layer goes: inside the selected layer, else next to the selected item, else the scene graph
        if (!sel.empty()) target = sc.is_layer(sel[0]) ? sel[0] : std::max(parent_layer(sc, sel[0]), graph);
        if (ImGui::Button("New layer")) {
            std::string err;
            const int l = d->add_layer(target, "New layer", &err);
            if (l >= 0) { tree_open_.insert(tree_key_of(target)); ++tree_open_version_; begin_rename_next_ = true; }
            status = l >= 0 ? "layer added" : err;
        }
        ImGui::SameLine();
        ImGui::BeginDisabled(sel.empty());
        if (ImGui::Button("Group")) {
            std::string err;
            const int l = d->group_items(sel, "New layer", &err);
            status = l >= 0 ? "grouped into a new layer" : err;
            if (l >= 0) begin_rename_next_ = true;
        }
        if (ImGui::IsItemHovered()) ImGui::SetTooltip("Put the selected items into a new layer (Ctrl+G)");
        ImGui::SameLine();
        if (ImGui::Button("Delete")) request_delete(editor);
        ImGui::EndDisabled();
        ImGui::SameLine();
        const bool one_layer = sel.size() == 1 && sc.is_layer(sel[0]) && sc.type(sel[0]) != "SCENE_GRAPH";
        ImGui::BeginDisabled(!one_layer);
        if (ImGui::Button("Duplicate")) {
            std::string err;
            if (d->duplicate_layer(sel[0], &err) < 0) status = err;
        }
        ImGui::EndDisabled();
        ImGui::SameLine();
        ImGui::TextDisabled("%zu selected", sel.size());
    }

    // ---- the tree
    const float footer = ImGui::GetFrameHeightWithSpacing() * 1.2f;
    if (ImGui::BeginChild("scenetree", ImVec2(0, -footer), true)) {
        const float row_h = ImGui::GetTextLineHeightWithSpacing() + 2.0f;
        ImDrawList* dl = ImGui::GetWindowDrawList();
        const bool window_hovered = ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows | ImGuiHoveredFlags_RootAndChildWindows);
        std::vector<int> pending_drop;   // (elems, layer, before) once a drop was delivered this frame
        int drop_layer = -1, drop_before = -1;
        ImGuiListClipper clipper;
        clipper.Begin(int(tree_rows_.size()), row_h);
        while (clipper.Step())
            for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i) {
                const TreeRow row = tree_rows_[size_t(i)];
                const int e = row.elem;
                ImGui::PushID(i);
                const bool is_sel = editor.selection().contains(d->ref_of(e));
                const bool own_vis = d->own_flag(e, "OIF_VISIBLE"), own_act = d->own_flag(e, "OIF_ACTIVE");
                const bool vis = d->shown(e), act = d->selectable(e);
                const ImVec2 p0 = ImGui::GetCursorScreenPos();
                const float avail = ImGui::GetContentRegionAvail().x;
                // the row: selection, double-click to rename, context menu, drag source and drop target
                ImGui::SetNextItemAllowOverlap();
                const bool clicked = ImGui::Selectable("##row", is_sel, ImGuiSelectableFlags_AllowDoubleClick, ImVec2(avail - 2 * kIconWidth, row_h));
                const bool row_hovered = ImGui::IsItemHovered();
                const ImRect rect(p0, ImVec2(p0.x + avail - 2 * kIconWidth, p0.y + row_h));
                if (clicked && rename_elem_ != e) {
                    ImGuiIO& io = ImGui::GetIO();
                    if (io.KeyCtrl) editor.selection().toggle(d->ref_of(e));
                    else if (io.KeyShift && !editor.selection().empty() && anchor_ >= 0) {
                        int a = -1;
                        for (size_t k = 0; k < tree_rows_.size(); ++k)
                            if (tree_rows_[k].elem == anchor_) a = int(k);
                        if (a >= 0)
                            for (int k = std::min(a, i); k <= std::max(a, i); ++k) editor.selection().add(d->ref_of(tree_rows_[size_t(k)].elem));
                    } else {
                        editor.selection().set(d->ref_of(e));
                        anchor_ = e;
                    }
                    editor.documents().set_active(d->id());
                    if (ImGui::IsMouseDoubleClicked(0) && sc.type(e) != "SCENE_GRAPH") begin_rename(editor, e);
                }
                if (ImGui::IsItemClicked(ImGuiMouseButton_Right) && !is_sel) {
                    editor.selection().set(d->ref_of(e));
                    anchor_ = e;
                }
                // drag source: the row, or the whole selection when the row is part of it
                if (sc.type(e) != "SCENE_GRAPH" && ImGui::BeginDragDropSource(ImGuiDragDropFlags_SourceAllowNullID)) {
                    std::vector<int> elems = is_sel ? selected(editor) : std::vector<int>{e};
                    ImGui::SetDragDropPayload("TILE_ELEMS", elems.data(), elems.size() * sizeof(int));
                    ImGui::Text("%zu item%s", elems.size(), elems.size() == 1 ? "" : "s");
                    ImGui::EndDragDropSource();
                }
                if (ImGui::BeginDragDropTarget()) {
                    if (const ImGuiPayload* pl = ImGui::AcceptDragDropPayload("TILE_ELEMS", ImGuiDragDropFlags_AcceptBeforeDelivery | ImGuiDragDropFlags_AcceptNoDrawDefaultRect)) {
                        const float y = (ImGui::GetMousePos().y - rect.Min.y) / row_h;
                        enum { Before, Into, After } zone = row.layer ? (y < 0.25f ? Before : y > 0.75f ? After : Into) : (y < 0.5f ? Before : After);
                        if (sc.type(e) == "SCENE_GRAPH") zone = Into;
                        int layer = -1, before = -1;
                        if (zone == Into) layer = e;
                        else {
                            layer = parent_layer(sc, e);
                            if (zone == Before) before = e;
                            else {   // after: in front of the next sibling
                                const std::vector<int> sibs = layer >= 0 ? sc.children(layer) : std::vector<int>{};
                                const auto it = std::find(sibs.begin(), sibs.end(), e);
                                before = it != sibs.end() && it + 1 != sibs.end() ? *(it + 1) : -1;
                                // an open layer: "after" it means its first child position, which is "into" at the front
                                if (row.layer && row.open && row.kids) { layer = e; before = sc.children(e).front(); }
                            }
                        }
                        // the indicator
                        const ImU32 c = IM_COL32(255, 210, 60, 255);
                        if (zone == Into) dl->AddRect(rect.Min, rect.Max, c, 2.0f, 0, 1.5f);
                        else dl->AddLine(ImVec2(rect.Min.x, zone == Before ? rect.Min.y : rect.Max.y), ImVec2(rect.Max.x, zone == Before ? rect.Min.y : rect.Max.y), c, 2.0f);
                        if (pl->IsDelivery() && layer >= 0) {
                            const int* ids = static_cast<const int*>(pl->Data);
                            pending_drop.assign(ids, ids + pl->DataSize / int(sizeof(int)));
                            drop_layer = layer;
                            drop_before = before;
                        }
                    }
                    ImGui::EndDragDropTarget();
                }
                // context menu
                if (ImGui::BeginPopupContextItem("rowmenu")) {
                    const bool is_layer = row.layer;
                    const std::vector<int> sel = selected(editor);
                    std::string err;
                    if (is_layer && ImGui::MenuItem("New layer inside")) {
                        const int l = d->add_layer(e, "New layer", &err);
                        if (l >= 0) { tree_open_.insert(row.key); ++tree_open_version_; begin_rename_next_ = true; }
                        status = l >= 0 ? "layer added" : err;
                    }
                    if (!sel.empty() && ImGui::MenuItem("Group selection into a new layer", "Ctrl+G")) {
                        const int l = d->group_items(sel, "New layer", &err);
                        status = l >= 0 ? "grouped" : err;
                        if (l >= 0) begin_rename_next_ = true;
                    }
                    if (is_layer && !sel.empty() && std::find(sel.begin(), sel.end(), e) == sel.end() && ImGui::MenuItem("Move selection here")) {
                        status = d->move_items(sel, e, -1, &err) ? "moved into " + sc.name(e) : err;
                    }
                    ImGui::Separator();
                    if (sc.type(e) != "SCENE_GRAPH") {
                        if (ImGui::MenuItem("Rename", "F2")) begin_rename(editor, e);
                        if (is_layer && ImGui::MenuItem("Duplicate")) { if (d->duplicate_layer(e, &err) < 0) status = err; }
                        if (ImGui::MenuItem("Delete", "Del")) request_delete(editor);
                    }
                    ImGui::Separator();
                    if (ImGui::MenuItem(own_vis ? "Hide" : "Show")) d->set_flags(is_sel ? selected(editor) : std::vector<int>{e}, "OIF_VISIBLE", !own_vis);
                    if (ImGui::MenuItem(own_act ? "Lock (not selectable)" : "Unlock")) d->set_flags(is_sel ? selected(editor) : std::vector<int>{e}, "OIF_ACTIVE", !own_act);
                    if (is_layer && sc.type(e) != "SCENE_GRAPH" && sc.doc().get(e, "export_type")) {
                        if (ImGui::BeginMenu("Export type")) {
                            for (const char* kind : {"Normal", "Escalation1", "Escalation2", "Escalation3"})
                                if (ImGui::MenuItem(kind, nullptr, d->export_type(e) == kind)) d->set_attribute(e, "export_type", std::string("LayerExportType_") + kind);
                            ImGui::EndMenu();
                        }
                    }
                    ImGui::EndPopup();
                }
                // what is drawn on the row
                const float cy = p0.y + row_h * 0.5f;
                const float x0 = p0.x + 4.0f + float(row.depth) * 14.0f;
                if (row.layer && row.kids) {
                    const ImVec2 c(x0 + 5, cy);
                    const ImU32 ac = IM_COL32(190, 190, 190, 255);
                    if (row.open) dl->AddTriangleFilled(ImVec2(c.x - 4, c.y - 2.5f), ImVec2(c.x + 4, c.y - 2.5f), ImVec2(c.x, c.y + 3.5f), ac);
                    else dl->AddTriangleFilled(ImVec2(c.x - 2.5f, c.y - 4), ImVec2(c.x - 2.5f, c.y + 4), ImVec2(c.x + 3.5f, c.y), ac);
                    // a click on the arrow toggles; the row's own click does not select then
                    const ImRect arrow(ImVec2(x0 - 2, p0.y), ImVec2(x0 + 12, p0.y + row_h));
                    if (row_hovered && ImGui::IsMouseClicked(0) && arrow.Contains(ImGui::GetMousePos())) {
                        if (row.open) tree_open_.erase(row.key); else tree_open_.insert(row.key);
                        ++tree_open_version_;
                    }
                }
                const ImU32 text_col = !vis ? IM_COL32(110, 110, 110, 255) : !act ? IM_COL32(170, 170, 150, 255) : IM_COL32(235, 235, 235, 255);
                float tx = x0 + 16.0f;
                if (row.layer) {
                    draw_layer_icon(dl, ImVec2(tx + 6, cy), text_col);
                    tx += 18.0f;
                }
                if (rename_elem_ == e) {
                    ImGui::SetCursorScreenPos(ImVec2(tx, p0.y));
                    ImGui::SetNextItemWidth(std::max(60.0f, avail - 2 * kIconWidth - (tx - p0.x) - 4));
                    if (rename_focus_) { ImGui::SetKeyboardFocusHere(); rename_focus_ = false; rename_active_seen_ = false; }
                    const bool enter = ImGui::InputText("##rename", rename_buf_, sizeof rename_buf_, ImGuiInputTextFlags_EnterReturnsTrue | ImGuiInputTextFlags_AutoSelectAll);
                    if (ImGui::IsItemActive()) rename_active_seen_ = true;
                    if (enter) { d->rename(e, rename_buf_); rename_elem_ = -1; }
                    else if (ImGui::IsKeyPressed(ImGuiKey_Escape) || (rename_active_seen_ && !ImGui::IsItemActive())) rename_elem_ = -1;
                } else {
                    std::string label = sc.name(e);
                    if (label.empty()) label = sc.type(e);
                    if (row.layer && row.kids) label += "  (" + std::to_string(row.kids) + ")";
                    else if (!row.layer) label += "   " + sc.type(e);
                    if (row.layer && sc.type(e) != "SCENE_GRAPH") {
                        const std::string x = d->export_type(e);
                        if (x != "Normal") label += "  [" + x + "]";
                    }
                    dl->PushClipRect(ImVec2(tx, p0.y), ImVec2(p0.x + avail - 2 * kIconWidth - 2, p0.y + row_h), true);
                    dl->AddText(ImVec2(tx, p0.y + (row_h - ImGui::GetTextLineHeight()) * 0.5f), text_col, label.c_str());
                    dl->PopClipRect();
                }
                // the eye and the cursor arrow
                ImGui::SetCursorScreenPos(ImVec2(p0.x + avail - 2 * kIconWidth, p0.y));
                if (ImGui::InvisibleButton("##eye", ImVec2(kIconWidth, row_h))) d->set_flags(is_sel ? selected(editor) : std::vector<int>{e}, "OIF_VISIBLE", !own_vis);
                if (ImGui::IsItemHovered()) ImGui::SetTooltip(own_vis ? "Visible: click to hide" : "Hidden: click to show");
                draw_eye(dl, ImVec2(p0.x + avail - 1.5f * kIconWidth, cy), own_vis, own_vis && !vis);
                ImGui::SetCursorScreenPos(ImVec2(p0.x + avail - kIconWidth, p0.y));
                if (ImGui::InvisibleButton("##act", ImVec2(kIconWidth, row_h))) d->set_flags(is_sel ? selected(editor) : std::vector<int>{e}, "OIF_ACTIVE", !own_act);
                if (ImGui::IsItemHovered()) ImGui::SetTooltip(own_act ? "Selectable: click to lock" : "Locked: click to make selectable");
                draw_cursor(dl, ImVec2(p0.x + avail - 0.5f * kIconWidth, cy), own_act, own_act && !act);
                ImGui::SetCursorScreenPos(ImVec2(p0.x, p0.y + row_h));
                ImGui::PopID();
            }
        // keys while the pointer is over the tree
        if (window_hovered && !ImGui::GetIO().WantTextInput) {
            const std::vector<int> sel = selected(editor);
            if (ImGui::IsKeyPressed(ImGuiKey_F2, false) && sel.size() == 1 && sc.type(sel[0]) != "SCENE_GRAPH") begin_rename(editor, sel[0]);
        }
        if (!pending_drop.empty()) {
            std::string err;
            status = d->move_items(pending_drop, drop_layer, drop_before, &err) ? "moved" : err;
        }
    }
    ImGui::EndChild();
    // a layer created by a button or a menu: its name goes into edit mode as soon as the selection has caught up with the hint
    if (begin_rename_next_) {
        const std::vector<int> sel = selected(editor);
        if (sel.size() == 1 && sc.is_layer(sel[0]) && sc.type(sel[0]) != "SCENE_GRAPH") {
            begin_rename(editor, sel[0]);
            // open its ancestors so the row is visible
            for (int p = parent_layer(sc, sel[0]); p >= 0; p = parent_layer(sc, p)) tree_open_.insert(tree_key_of(p));
            ++tree_open_version_;
            begin_rename_next_ = false;
        } else if (++rename_wait_ > 5) {
            begin_rename_next_ = false;
        }
    } else {
        rename_wait_ = 0;
    }
    // ---- delete confirmation
    if (delete_confirm_) {
        ImGui::OpenPopup("Delete layers?");
        delete_confirm_ = false;
    }
    if (ImGui::BeginPopupModal("Delete layers?", nullptr, ImGuiWindowFlags_AlwaysAutoResize)) {
        ImGui::Text("The selection holds %zu elements. Delete it?", delete_weight_);
        if (ImGui::Button("Delete")) {
            d->remove_items(selected(editor));
            status = "deleted";
            ImGui::CloseCurrentPopup();
        }
        ImGui::SameLine();
        if (ImGui::Button("Cancel") || ImGui::IsKeyPressed(ImGuiKey_Escape)) ImGui::CloseCurrentPopup();
        ImGui::EndPopup();
    }
    // ---- what is on the tile, per instance type (the visibility of a type is a view setting of this tool, not a flag of the file)
    if (ImGui::CollapsingHeader("Instance types")) {
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
    ImGui::End();
}

// The open-state key of an element: the name path of the layers above it (see rebuild_tree).
std::string TileTool::tree_key_of(int elem) const {
    for (const TreeRow& r : tree_rows_)
        if (r.elem == elem) return r.key;
    return {};
}

} // namespace wce
