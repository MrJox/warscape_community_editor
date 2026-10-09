#include "weather/variation_editor.h"

#include "imgui.h"

#include <algorithm>

namespace wce {

namespace {

const ImVec4 kDim(0.6f, 0.6f, 0.6f, 1.0f);
const ImVec4 kBad(1.0f, 0.45f, 0.4f, 1.0f);

// "any" or "category: a, b; category: c" for the categories a variation restricts.
std::string summary(const VariationDb& db, uint32_t flags) {
    std::string s;
    for (auto& [cat, allowed] : db.decode(flags)) {
        const VariationDb::Meta* m = db.meta(cat);
        if (m && allowed.size() == m->values.size()) continue;
        if (!s.empty()) s += "; ";
        s += cat + ": ";
        if (allowed.empty()) s += "(nothing)";
        for (size_t i = 0; i < allowed.size(); ++i) s += (i ? ", " : "") + allowed[i];
    }
    return s.empty() ? "any" : s;
}

} // namespace

bool draw_variation_editor(VariationDb& db, VariationEditState& st, const IVfs* vfs) {
    bool changed = false;

    // ---- Categories and their values
    ImGui::Text("Values: %d of 32 flag bits in use", db.value_count());
    if (ImGui::TreeNode("Categories and values")) {
        for (int mi = 0; mi < int(db.metas.size()); ++mi) {
            const std::string category = db.metas[mi].name;
            ImGui::PushID(mi);
            if (ImGui::TreeNode(category.c_str(), "%s (%zu)", category.c_str(), db.metas[mi].values.size())) {
                int remove = -1;
                std::string rename_from, rename_to;
                for (int vi = 0; vi < int(db.metas[mi].values.size()); ++vi) {
                    ImGui::PushID(vi);
                    char buf[128];
                    std::snprintf(buf, sizeof buf, "%s", db.metas[mi].values[vi].c_str());
                    ImGui::SetNextItemWidth(260);
                    if (ImGui::InputText("##v", buf, sizeof buf, ImGuiInputTextFlags_EnterReturnsTrue) && buf != db.metas[mi].values[vi]) {
                        rename_from = db.metas[mi].values[vi];
                        rename_to = buf;
                    }
                    ImGui::SameLine();
                    if (ImGui::SmallButton("Remove")) remove = vi;
                    ImGui::PopID();
                }
                if (!rename_from.empty()) {
                    std::string why;
                    if (db.rename_value(category, rename_from, rename_to, &why)) { st.message = "Renamed " + rename_from + " to " + rename_to; changed = true; }
                    else st.message = why;
                }
                if (remove >= 0) {
                    std::string why, value = db.metas[mi].values[remove];
                    if (db.remove_value(category, value, &why)) { st.message = "Removed " + value; changed = true; }
                    else st.message = why;
                }
                std::string& typed = st.new_value[category];
                char nbuf[128];
                std::snprintf(nbuf, sizeof nbuf, "%s", typed.c_str());
                ImGui::SetNextItemWidth(260);
                const bool enter = ImGui::InputTextWithHint("##new", "new value", nbuf, sizeof nbuf, ImGuiInputTextFlags_EnterReturnsTrue);
                typed = nbuf;
                ImGui::SameLine();
                if ((ImGui::Button("Add value") || enter) && !typed.empty()) {
                    std::string why;
                    if (db.add_value(category, typed, &why)) { st.message = "Added " + typed; typed.clear(); changed = true; }
                    else st.message = why;
                }
                ImGui::TreePop();
            }
            ImGui::PopID();
        }
        ImGui::TextColored(kDim, "Removing or adding a value moves the flag bits of later values; every variation keeps allowing the same values.");
        ImGui::TreePop();
    }

    // ---- Variations
    if (db.entries.empty()) {
        ImGui::TextColored(kBad, "The database has no entries.");
    } else {
        st.entry = std::clamp(st.entry, 0, int(db.entries.size()) - 1);
        if (db.entries.size() > 1) {
            ImGui::SetNextItemWidth(200);
            if (ImGui::BeginCombo("Entry", db.entries[st.entry].key.c_str())) {
                for (int i = 0; i < int(db.entries.size()); ++i)
                    if (ImGui::Selectable(db.entries[i].key.c_str(), i == st.entry)) { st.entry = i; st.selected = -1; }
                ImGui::EndCombo();
            }
        }
        VariationDb::Entry& entry = db.entries[st.entry];
        ImGui::SetNextItemWidth(260);
        ImGui::InputTextWithHint("##filter", "filter variations", st.filter, sizeof st.filter);
        ImGui::SameLine();
        ImGui::Text("%zu variations", entry.variations.size());

        if (ImGui::BeginTable("vars", 3, ImGuiTableFlags_RowBg | ImGuiTableFlags_Borders | ImGuiTableFlags_ScrollY | ImGuiTableFlags_Resizable, ImVec2(0, 260))) {
            ImGui::TableSetupScrollFreeze(0, 1);
            ImGui::TableSetupColumn("File", ImGuiTableColumnFlags_WidthStretch, 3.0f);
            ImGui::TableSetupColumn("Allowed when", ImGuiTableColumnFlags_WidthStretch, 2.0f);
            ImGui::TableSetupColumn("##sel", ImGuiTableColumnFlags_WidthFixed, 56.0f);
            ImGui::TableHeadersRow();
            const std::string filter = st.filter;
            for (int i = 0; i < int(entry.variations.size()); ++i) {
                VariationDb::Variation& v = entry.variations[i];
                if (!filter.empty() && v.filename.find(filter) == std::string::npos) continue;
                ImGui::PushID(i);
                ImGui::TableNextRow();
                ImGui::TableSetColumnIndex(0);
                char buf[300];
                std::snprintf(buf, sizeof buf, "%s", v.filename.c_str());
                const bool missing = vfs && !vfs->owner(Vfs::normalize(v.filename));
                if (missing) ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.45f, 0.15f, 0.15f, 1));
                ImGui::SetNextItemWidth(-1);
                if (ImGui::InputText("##f", buf, sizeof buf) && v.filename != buf) { v.filename = buf; changed = true; }
                if (missing) {
                    ImGui::PopStyleColor();
                    if (ImGui::IsItemHovered()) ImGui::SetTooltip("This file is in no pack");
                }
                ImGui::TableSetColumnIndex(1);
                ImGui::TextUnformatted(summary(db, v.flags).c_str());
                ImGui::TableSetColumnIndex(2);
                if (ImGui::SmallButton(st.selected == i ? "editing" : "edit")) st.selected = i;
                ImGui::PopID();
            }
            ImGui::EndTable();
        }

        // Add
        ImGui::SetNextItemWidth(520);
        ImGui::InputTextWithHint("##newfile", "weather\\folder\\file.environment", st.new_file, sizeof st.new_file);
        ImGui::SameLine();
        if (ImGui::Button("Add variation") && st.new_file[0]) {
            entry.variations.push_back({st.new_file, db.flags_for({})});
            st.selected = int(entry.variations.size()) - 1;
            st.new_file[0] = 0;
            changed = true;
        }

        // Conditions of the selected variation
        if (st.selected >= 0 && st.selected < int(entry.variations.size())) {
            VariationDb::Variation& v = entry.variations[st.selected];
            ImGui::Separator();
            ImGui::Text("Conditions of %s", v.filename.c_str());
            std::map<std::string, std::vector<std::string>> allowed;
            for (auto& [cat, values] : db.decode(v.flags)) allowed[cat] = values;
            bool edited = false;
            for (auto& m : db.metas) {
                auto& list = allowed[m.name];
                ImGui::PushID(m.name.c_str());
                ImGui::TextUnformatted(m.name.c_str());
                ImGui::SameLine(180);
                for (auto& value : m.values) {
                    bool on = std::find(list.begin(), list.end(), value) != list.end();
                    if (ImGui::Checkbox(value.c_str(), &on)) {
                        if (on) list.push_back(value);
                        else list.erase(std::remove(list.begin(), list.end(), value), list.end());
                        edited = true;
                    }
                    ImGui::SameLine();
                }
                ImGui::NewLine();
                ImGui::PopID();
            }
            if (edited) {
                v.flags = db.flags_for(allowed);
                changed = true;
            }
            ImGui::TextColored(kDim, "flags %u (0x%08X)", v.flags, v.flags);
            ImGui::SameLine();
            if (ImGui::SmallButton("Any (flags 0)")) { v.flags = 0; changed = true; }
            ImGui::SameLine();
            if (ImGui::SmallButton("Duplicate")) {
                entry.variations.insert(entry.variations.begin() + st.selected + 1, v);
                ++st.selected;
                changed = true;
            }
            ImGui::SameLine();
            if (ImGui::SmallButton("Delete")) {
                entry.variations.erase(entry.variations.begin() + st.selected);
                st.selected = -1;
                changed = true;
            }
        }
    }

    // ---- Problems
    const auto problems = db.validate();
    if (!problems.empty()) {
        ImGui::Separator();
        ImGui::TextColored(kBad, "%zu problem(s) for the game:", problems.size());
        for (auto& p : problems) ImGui::BulletText("%s", p.c_str());
    }
    if (!st.message.empty()) ImGui::TextUnformatted(st.message.c_str());
    return changed;
}

} // namespace wce
