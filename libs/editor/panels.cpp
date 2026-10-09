#include "editor/panels.h"

#include "editor/editor.h"
#include "foundation/log.h"
#include "foundation/text_encoding.h"

#include <imgui.h>

#include <algorithm>
#include <cstring>

namespace wce {

// ---- ImGuiInspector

// The value column starts after the longest label that fits: 40% of the panel, at least 110 px.
static float label_column() { return std::max(110.0f, ImGui::GetContentRegionAvail().x * 0.4f); }

bool ImGuiInspector::item_deactivated_after_edit() { return ImGui::IsItemDeactivatedAfterEdit(); }

void ImGuiInspector::section(const std::string& title) {
    ImGui::SeparatorText(title.c_str());
}

void ImGuiInspector::text(const std::string& field, const std::string& label, const std::string& value) {
    (void)field;
    ImGui::TextDisabled("%s", label.c_str());
    ImGui::SameLine(label_column());
    ImGui::TextUnformatted(value.c_str());
}

bool ImGuiInspector::edit_float(const std::string& field, const std::string& label, float& v, float speed, float lo, float hi) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::DragFloat(id(field).c_str(), &v, speed, lo, hi, "%.3f");
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_float3(const std::string& field, const std::string& label, float v[3], float speed) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::DragFloat3(id(field).c_str(), v, speed, 0, 0, "%.3f");
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_float2(const std::string& field, const std::string& label, float v[2], float speed) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::DragFloat2(id(field).c_str(), v, speed, 0, 0, "%.6g");
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_int(const std::string& field, const std::string& label, int64_t& v, float speed) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::DragScalar(id(field).c_str(), ImGuiDataType_S64, &v, speed);
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_color3(const std::string& field, const std::string& label, float v[3]) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::ColorEdit3(id(field).c_str(), v, ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR);
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_color4(const std::string& field, const std::string& label, float v[4]) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    const bool changed = ImGui::ColorEdit4(id(field).c_str(), v, ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR | ImGuiColorEditFlags_AlphaBar);
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_bool(const std::string& field, const std::string& label, bool& v) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    const bool changed = ImGui::Checkbox(id(field).c_str(), &v);
    ended_ = changed;
    return changed;
}

bool ImGuiInspector::edit_string(const std::string& field, const std::string& label, std::string& v) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    char buf[512];
    std::snprintf(buf, sizeof buf, "%s", v.c_str());
    const bool changed = ImGui::InputText(id(field).c_str(), buf, sizeof buf);
    if (changed) v = buf;
    finish_widget();
    return changed;
}

bool ImGuiInspector::edit_choice(const std::string& field, const std::string& label, int& index, const std::vector<std::string>& choices) {
    ImGui::TextUnformatted(label.c_str());
    ImGui::SameLine(label_column());
    ImGui::SetNextItemWidth(-1);
    bool changed = false;
    const char* shown = index >= 0 && index < int(choices.size()) ? choices[size_t(index)].c_str() : "";
    if (ImGui::BeginCombo(id(field).c_str(), shown)) {
        for (int i = 0; i < int(choices.size()); ++i)
            if (ImGui::Selectable(choices[size_t(i)].c_str(), i == index)) {
                index = i;
                changed = true;
            }
        ImGui::EndCombo();
    }
    ended_ = changed;
    return changed;
}

// ---- outliner

namespace {

void draw_node(Editor& editor, const OutlinerNode& n) {
    Selection& sel = editor.selection();
    if (n.children.empty()) {
        const bool selected = n.selectable && sel.contains(n.ref);
        if (ImGui::Selectable((n.label + "##" + std::to_string(n.ref.doc) + "_" + std::to_string(n.ref.item)).c_str(), selected) && n.selectable) {
            if (ImGui::GetIO().KeyCtrl) sel.toggle(n.ref);
            else sel.set(n.ref);
            editor.documents().set_active(n.ref.doc);
        }
        return;
    }
    const bool selected = n.selectable && sel.contains(n.ref);
    ImGuiTreeNodeFlags flags = ImGuiTreeNodeFlags_OpenOnArrow | ImGuiTreeNodeFlags_SpanAvailWidth | ImGuiTreeNodeFlags_DefaultOpen;
    if (selected) flags |= ImGuiTreeNodeFlags_Selected;
    const bool open = ImGui::TreeNodeEx((n.label + "##" + std::to_string(n.ref.doc) + "_" + std::to_string(n.ref.item)).c_str(), flags);
    if (ImGui::IsItemClicked() && !ImGui::IsItemToggledOpen() && n.selectable) {
        sel.set(n.ref);
        editor.documents().set_active(n.ref.doc);
    }
    if (open) {
        for (const OutlinerNode& c : n.children) draw_node(editor, c);
        ImGui::TreePop();
    }
}

} // namespace

void Panels::outliner(Editor& editor) {
    if (!ImGui::Begin("Outliner", &editor.panels.outliner)) { ImGui::End(); return; }
    DocumentManager& docs = editor.documents();
    if (docs.order().empty()) ImGui::TextDisabled("No document open.");
    for (DocumentId id : docs.order()) {
        Document* d = docs.find(id);
        if (!d) continue;
        ImGui::PushID(int(id));
        bool visible = editor.scene().layer(docs.layer_of(id)) ? editor.scene().layer(docs.layer_of(id))->visible : true;
        if (ImGui::Checkbox("##vis", &visible)) docs.set_visible(id, visible);
        ImGui::SameLine();
        std::string title = d->title() + (d->modified() ? " *" : "");
        if (id == docs.active_id()) ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_CheckMark));
        const bool open = ImGui::CollapsingHeader((title + "###doc").c_str(), ImGuiTreeNodeFlags_DefaultOpen);
        if (id == docs.active_id()) ImGui::PopStyleColor();
        if (ImGui::IsItemClicked()) docs.set_active(id);
        if (open) draw_node(editor, d->outline());
        ImGui::PopID();
    }
    ImGui::End();
}

// ---- inspector

void Panels::inspector(Editor& editor) {
    if (!ImGui::Begin("Inspector", &editor.panels.inspector)) { ImGui::End(); return; }
    const Selection& sel = editor.selection();
    if (sel.empty()) {
        ImGui::TextDisabled("Nothing selected.");
    } else {
        const SourceRef ref = sel.items()[0];
        Document* d = editor.documents().find(ref.doc);
        if (!d) {
            ImGui::TextDisabled("The document is closed.");
        } else {
            if (sel.items().size() > 1) ImGui::TextDisabled("%zu items selected, showing the first", sel.items().size());
            const std::string what = d->describe(ref);
            ImGui::TextUnformatted(what.empty() ? "Item" : what.c_str());
            ImGui::Separator();
            ImGuiInspector ins;
            ins.begin(ref.doc, ref.item);
            d->inspect(ref, ins);
        }
    }
    ImGui::End();
}

// ---- assets

namespace {

struct AssetState {
    std::string folder;          // lower case, "" or ending with '/'
    char filter[128] = "";
    bool only_tool = true;
    std::string built_for;       // folder + filter + tool + vfs file count the listing was built for
    std::vector<std::string> folders;
    std::vector<std::string> files;
};

void rebuild_assets(Editor& editor, AssetState& st) {
    st.folders.clear();
    st.files.clear();
    Vfs* vfs = editor.vfs();
    if (!vfs) return;
    std::vector<std::string> exts;
    if (st.only_tool && editor.active_tool()) exts = editor.active_tool()->opens();
    const std::string filter = [&] {
        std::string f = st.filter;
        for (char& c : f) c = char(std::tolower(static_cast<unsigned char>(c)));
        return f;
    }();
    std::string last_folder;
    for (auto it = vfs->files().lower_bound(st.folder); it != vfs->files().end() && it->first.compare(0, st.folder.size(), st.folder) == 0; ++it) {
        const std::string& path = it->first;
        const std::string rest = path.substr(st.folder.size());
        const size_t slash = rest.find('/');
        if (slash != std::string::npos) {
            const std::string sub = rest.substr(0, slash);
            if (sub != last_folder) {
                last_folder = sub;
                st.folders.push_back(sub);
            }
            continue;
        }
        if (!exts.empty()) {
            bool match = false;
            for (const std::string& e : exts) match = match || (path.size() >= e.size() && path.compare(path.size() - e.size(), e.size(), e) == 0);
            if (!match) continue;
        }
        if (!filter.empty() && path.find(filter) == std::string::npos) continue;
        if (st.files.size() < 3000) st.files.push_back(rest);
    }
}

} // namespace

void Panels::assets(Editor& editor) {
    static AssetState st;
    if (!ImGui::Begin("Assets", &editor.panels.assets)) { ImGui::End(); return; }
    Vfs* vfs = editor.vfs();
    if (!vfs) {
        ImGui::TextDisabled("The game packs are not mounted.");
        ImGui::End();
        return;
    }
    const Tool* tool = editor.active_tool();
    const bool has_exts = tool && !tool->opens().empty();
    ImGui::SetNextItemWidth(220);
    ImGui::InputTextWithHint("##filter", "filter", st.filter, sizeof st.filter);
    ImGui::SameLine();
    if (has_exts) ImGui::Checkbox("only this tool's files", &st.only_tool);
    ImGui::SameLine();
    ImGui::TextDisabled("/%s", st.folder.c_str());
    const std::string key = st.folder + "|" + st.filter + "|" + (tool ? tool->id() : "") + (st.only_tool ? "1" : "0") + "|" + std::to_string(vfs->files().size());
    if (key != st.built_for) {
        rebuild_assets(editor, st);
        st.built_for = key;
    }
    if (ImGui::BeginChild("##list", ImVec2(0, 0), ImGuiChildFlags_Borders)) {
        if (!st.folder.empty() && ImGui::Selectable("[..]")) {
            std::string f = st.folder.substr(0, st.folder.size() - 1);
            const size_t cut = f.rfind('/');
            st.folder = cut == std::string::npos ? std::string() : f.substr(0, cut + 1);
        }
        for (const std::string& f : st.folders)
            if (ImGui::Selectable(("[" + f + "]").c_str())) st.folder += f + "/";
        for (const std::string& f : st.files) {
            const std::string full = st.folder + f;
            if (ImGui::Selectable(f.c_str(), false, ImGuiSelectableFlags_AllowDoubleClick) && ImGui::IsMouseDoubleClicked(0)) {
                std::string msg;
                if (!editor.open_path(full, true, &msg)) editor.status(msg, true);
                else editor.status(msg);
            }
            if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s\n(double-click to open)", full.c_str());
        }
    }
    ImGui::EndChild();
    ImGui::End();
}

// ---- log

void Panels::log(Editor& editor) {
    static bool show_info = true, show_warn = true, show_error = true;
    static uint64_t seen = 0;
    if (!ImGui::Begin("Log", &editor.panels.log)) { ImGui::End(); return; }
    ImGui::Checkbox("info", &show_info);
    ImGui::SameLine();
    ImGui::Checkbox("warnings", &show_warn);
    ImGui::SameLine();
    ImGui::Checkbox("errors", &show_error);
    ImGui::SameLine();
    if (ImGui::SmallButton("Clear")) Log::instance().clear();
    ImGui::Separator();
    if (ImGui::BeginChild("##log", ImVec2(0, 0), ImGuiChildFlags_None, ImGuiWindowFlags_HorizontalScrollbar)) {
        const bool at_bottom = ImGui::GetScrollY() >= ImGui::GetScrollMaxY() - 4;
        for (const LogEntry& e : Log::instance().entries()) {
            if ((e.level == LogLevel::Info && !show_info) || (e.level == LogLevel::Warning && !show_warn) || (e.level == LogLevel::Error && !show_error)) continue;
            const ImVec4 colour = e.level == LogLevel::Error ? ImVec4(1.0f, 0.45f, 0.4f, 1) : e.level == LogLevel::Warning ? ImVec4(1.0f, 0.8f, 0.4f, 1) : ImGui::GetStyleColorVec4(ImGuiCol_Text);
            ImGui::PushStyleColor(ImGuiCol_Text, colour);
            ImGui::TextUnformatted(e.text.c_str());
            ImGui::PopStyleColor();
        }
        const uint64_t now = Log::instance().sequence();
        if (now != seen && at_bottom) ImGui::SetScrollHereY(1.0f);
        seen = now;
    }
    ImGui::EndChild();
    ImGui::End();
}

// ---- history

void Panels::history(Editor& editor) {
    if (!ImGui::Begin("History", &editor.panels.history)) { ImGui::End(); return; }
    Document* d = editor.documents().active();
    if (!d) {
        ImGui::TextDisabled("No document.");
        ImGui::End();
        return;
    }
    CommandStack& h = d->history();
    ImGui::TextDisabled("%s", d->title().c_str());
    ImGui::BeginDisabled(!h.can_undo());
    if (ImGui::SmallButton("Undo")) h.undo();
    ImGui::EndDisabled();
    ImGui::SameLine();
    ImGui::BeginDisabled(!h.can_redo());
    if (ImGui::SmallButton("Redo")) h.redo();
    ImGui::EndDisabled();
    ImGui::Separator();
    if (ImGui::Selectable("(opened)", h.position() == 0)) h.jump_to(0);
    for (size_t i = 0; i < h.size(); ++i) {
        const bool applied = i < h.position();
        if (!applied) ImGui::PushStyleColor(ImGuiCol_Text, ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
        if (ImGui::Selectable((h.name(i) + "##h" + std::to_string(i)).c_str(), h.position() == i + 1)) h.jump_to(i + 1);
        if (!applied) ImGui::PopStyleColor();
    }
    ImGui::End();
}

// ---- commands (help + console)

void Panels::commands(Editor& editor) {
    if (!ImGui::Begin("Commands", &editor.panels.commands)) { ImGui::End(); return; }
    static char line[256] = "";
    ImGui::SetNextItemWidth(-1);
    if (ImGui::InputTextWithHint("##cmd", "command, e.g. select 1   (Enter runs it)", line, sizeof line, ImGuiInputTextFlags_EnterReturnsTrue)) {
        const CommandResult r = editor.commands().run(line);
        if (!r.message.empty() || !r.ok) Log::instance().add(r.ok ? LogLevel::Info : LogLevel::Error, std::string(line) + " -> " + r.message);
        line[0] = 0;
        ImGui::SetKeyboardFocusHere(-1);
    }
    ImGui::Separator();
    for (const auto* e : editor.commands().entries()) {
        ImGui::TextUnformatted(e->usage.c_str());
        ImGui::SameLine(300);
        ImGui::TextDisabled("%s", e->help.c_str());
    }
    ImGui::End();
}

} // namespace wce
