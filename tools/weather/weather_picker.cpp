#include "weather/weather_picker.h"

#include "editor/editor.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"

#include <imgui.h>

#include <fstream>

namespace fs = std::filesystem;

namespace wce {

namespace {

const char* kVdbBin = "warscape_asset_variation_db/warscape_asset_variation_db.bin";
const char* kVdbXml = "warscape_asset_variation_db/warscape_asset_variation_db.xml";
const ImVec4 kDim(0.6f, 0.6f, 0.6f, 1.0f);

} // namespace

// Lists every .bin and .xml of the database in the packs and loads the one the game uses (the winning pack's .bin).
void WeatherPicker::load(Editor& editor) {
    loaded_ = true;
    db_.reset();
    error_.clear();
    message_.clear();
    sources_.clear();
    Vfs* vfs = editor.vfs();
    if (!vfs) { error_ = "Packs are not mounted."; return; }
    int use = -1;
    const auto winner = vfs->owner(kVdbBin);
    for (int pack : vfs->owners(kVdbBin)) {
        if (winner && pack == *winner) use = int(sources_.size());
        sources_.push_back({vfs->packs()[pack].name + " : .bin", pack, false});
    }
    for (int pack : vfs->owners(kVdbXml)) sources_.push_back({vfs->packs()[pack].name + " : .xml", pack, true});
    if (use < 0) { error_ = std::string(kVdbBin) + " is in no pack"; return; }
    load_source(editor, use);
}

void WeatherPicker::load_source(Editor& editor, int index) {
    db_.reset();
    error_.clear();
    message_.clear();
    if (index < 0 || index >= int(sources_.size())) return;
    Vfs* vfs = editor.vfs();
    source_index_ = index;
    const Source& src = sources_[index];
    try {
        auto disk = vfs->fetch_from(src.pack, src.xml ? kVdbXml : kVdbBin);
        if (!disk) throw std::runtime_error("cannot extract the file from " + vfs->packs()[src.pack].name);
        const std::vector<uint8_t> bytes = read_file(disk->string());
        db_ = std::make_unique<VariationDb>(src.xml ? VariationDb::parse_xml(bytes) : VariationDb::parse(bytes));
        source_ = src.label;
        // The other format of the same pack should hold the same data.
        auto other = vfs->fetch_from(src.pack, src.xml ? kVdbBin : kVdbXml);
        if (other) {
            const std::vector<uint8_t> ob = read_file(other->string());
            const VariationDb o = src.xml ? VariationDb::parse(ob) : VariationDb::parse_xml(ob);
            if (!(o == *db_)) message_ = std::string("The .bin and the .xml in ") + vfs->packs()[src.pack].name + " hold different data.";
        }
    } catch (const std::exception& e) {
        error_ = e.what();
        return;
    }
    orig_ = *db_;
    edit_ = VariationEditState();
    query_.clear();
    for (auto& m : db_->metas)
        if (!m.values.empty()) query_[m.name] = m.values.front();
    for (auto& [k, v] : overrides_) query_[k] = v;
}

void WeatherPicker::load_file(const std::string& path) {
    try {
        const std::vector<uint8_t> bytes = read_file(path);
        const bool xml = path.size() >= 4 && (path.substr(path.size() - 4) == ".xml" || path.substr(path.size() - 4) == ".XML");
        db_ = std::make_unique<VariationDb>(xml ? VariationDb::parse_xml(bytes) : VariationDb::parse(bytes));
        error_.clear();
        message_.clear();
        source_ = path;
        orig_ = *db_;
        edit_ = VariationEditState();
        query_.clear();
        for (auto& m : db_->metas)
            if (!m.values.empty()) query_[m.name] = m.values.front();
    } catch (const std::exception& e) {
        message_ = std::string("Cannot load ") + path + ": " + e.what();
    }
}

// Commands separated by ';' for automated tests: add-value:<category>=<value>, remove-value:<category>=<value>,
// rename-value:<category>=<from>><to>, add-variation:<file>, save:xml|bin|both. The result is shown in the picker window.
void WeatherPicker::run_script(const fs::path& output_dir) {
    std::string script = script_, log;
    script_.clear();
    size_t start = 0;
    while (start <= script.size()) {
        size_t semi = script.find(';', start);
        std::string cmd = script.substr(start, semi == std::string::npos ? std::string::npos : semi - start);
        start = semi == std::string::npos ? script.size() + 1 : semi + 1;
        if (cmd.empty()) continue;
        const size_t colon = cmd.find(':');
        const std::string op = cmd.substr(0, colon), arg = colon == std::string::npos ? "" : cmd.substr(colon + 1);
        const size_t eq = arg.find('=');
        std::string why;
        bool ok = true;
        if (op == "add-value" && eq != std::string::npos) ok = db_->add_value(arg.substr(0, eq), arg.substr(eq + 1), &why);
        else if (op == "remove-value" && eq != std::string::npos) ok = db_->remove_value(arg.substr(0, eq), arg.substr(eq + 1), &why);
        else if (op == "rename-value" && eq != std::string::npos && arg.find('>', eq) != std::string::npos) {
            const size_t gt = arg.find('>', eq);
            ok = db_->rename_value(arg.substr(0, eq), arg.substr(eq + 1, gt - eq - 1), arg.substr(gt + 1), &why);
        } else if (op == "add-variation" && !db_->entries.empty()) db_->entries[0].variations.push_back({arg, db_->flags_for({})});
        else if (op == "save") { save(output_dir, arg == "xml" || arg == "both", arg == "bin" || arg == "both"); log += message_ + " "; continue; }
        else { ok = false; why = "unknown command"; }
        log += op + (ok ? " ok " : " FAILED (" + why + ") ");
    }
    message_ = log;
}

// Writes the working copy as loose files under the output folder, at the in-pack path of the database.
void WeatherPicker::save(const fs::path& output_dir, bool xml, bool bin) {
    if (!db_) return;
    const auto problems = db_->validate();
    if (!problems.empty() && !save_anyway_) {
        message_ = "Not saved: the database has problems (see the Edit tab). Tick \"save anyway\" to write it regardless.";
        return;
    }
    try {
        fs::path dir = output_dir / "warscape_asset_variation_db";
        fs::create_directories(dir);
        std::string done;
        if (xml) {
            const std::string text = db_->to_xml();
            std::ofstream f(dir / "warscape_asset_variation_db.xml", std::ios::binary);
            f.write(text.data(), std::streamsize(text.size()));
            if (!f) throw std::runtime_error("cannot write the .xml");
            done += "warscape_asset_variation_db.xml ";
        }
        if (bin) {
            const std::vector<uint8_t> bytes = db_->to_bytes();
            std::ofstream f(dir / "warscape_asset_variation_db.bin", std::ios::binary);
            f.write(reinterpret_cast<const char*>(bytes.data()), std::streamsize(bytes.size()));
            if (!f) throw std::runtime_error("cannot write the .bin");
            done += "warscape_asset_variation_db.bin ";
        }
        message_ = "Saved " + done + "to " + narrow(dir.wstring());
        orig_ = *db_;
    } catch (const std::exception& e) {
        message_ = std::string("Cannot save: ") + e.what();
    }
}

bool WeatherPicker::ensure_loaded(Editor& editor, const fs::path& output_dir) {
    if (!loaded_) load(editor);
    if (db_ && !script_.empty()) run_script(output_dir);
    return db_ != nullptr;
}

// The candidate list the game builds for a battle (rules and DLL functions: formats/variation_db.h).
void WeatherPicker::draw(Editor& editor, const fs::path& output_dir, const std::string& active_environment) {
    if (!open) return;
    Vfs* vfs = editor.vfs();
    ImGui::SetNextWindowSize(ImVec2(760, 800), ImGuiCond_FirstUseEver);
    ImGui::SetNextWindowBgAlpha(1.0f);
    if (!ImGui::Begin("Weather picker", &open)) { ImGui::End(); return; }
    if (!loaded_) load(editor);
    if (!db_) {
        ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "Cannot read the variation database: %s", error_.c_str());
        if (ImGui::Button("Retry")) loaded_ = false;
        ImGui::End();
        return;
    }
    if (!script_.empty()) run_script(output_dir);
    const bool dirty = !(*db_ == orig_);
    ImGui::SetNextItemWidth(420);
    if (ImGui::BeginCombo("Source", source_.c_str())) {
        for (int i = 0; i < int(sources_.size()); ++i)
            if (ImGui::Selectable(sources_[i].label.c_str(), i == source_index_)) { load_source(editor, i); ImGui::EndCombo(); ImGui::End(); return; }
        ImGui::EndCombo();
    }
    ImGui::SameLine();
    if (ImGui::SmallButton("Reload")) { loaded_ = false; ImGui::End(); return; }
    ImGui::SetNextItemWidth(420);
    ImGui::InputTextWithHint("##loose", "or a loose .xml / .bin file", loose_, sizeof loose_);
    ImGui::SameLine();
    if (ImGui::SmallButton("Load file") && loose_[0]) load_file(loose_);
    if (dirty) ImGui::TextColored(ImVec4(0.9f, 0.8f, 0.4f, 1), "edited, not saved");
    else ImGui::TextColored(kDim, "unchanged");
    ImGui::SameLine();
    if (ImGui::Button("Save XML")) save(output_dir, true, false);
    ImGui::SameLine();
    if (ImGui::Button("Save FASTBIN")) save(output_dir, false, true);
    ImGui::SameLine();
    if (ImGui::Button("Save both")) save(output_dir, true, true);
    ImGui::SameLine();
    ImGui::Checkbox("save anyway", &save_anyway_);
    if (dirty) {
        ImGui::SameLine();
        if (ImGui::Button("Revert")) { *db_ = orig_; edit_ = VariationEditState(); }
    }
    ImGui::TextColored(kDim, "Saved to %s\\warscape_asset_variation_db\\", narrow(output_dir.wstring()).c_str());
    if (!message_.empty()) ImGui::TextColored(ImVec4(0.9f, 0.8f, 0.4f, 1), "%s", message_.c_str());
    if (!ImGui::BeginTabBar("vdbtabs")) { ImGui::End(); return; }
    if (ImGui::BeginTabItem("Edit")) {
        draw_variation_editor(*db_, edit_, vfs);
        ImGui::EndTabItem();
    }
    if (!ImGui::BeginTabItem("Pick", nullptr, select_pick_ ? ImGuiTabItemFlags_SetSelected : 0)) { ImGui::EndTabBar(); ImGui::End(); return; }
    select_pick_ = false;
    ImGui::Separator();
    ImGui::TextUnformatted("Battle conditions");
    for (auto& m : db_->metas) {
        std::string& cur = query_[m.name];
        ImGui::SetNextItemWidth(260);
        if (ImGui::BeginCombo(m.name.c_str(), cur.c_str())) {
            for (auto& v : m.values)
                if (ImGui::Selectable(v.c_str(), v == cur)) cur = v;
            ImGui::EndCombo();
        }
    }
    ImGui::TextColored(kDim, "query bits 0x%08X", db_->query_mask(query_));
    ImGui::Separator();

    size_t total_values = 0;
    for (auto& m : db_->metas) total_values += m.values.size();
    VariationDb::Result r = db_->candidates("environment", query_);
    if (r.used_default_map)
        ImGui::TextColored(ImVec4(0.9f, 0.8f, 0.4f, 1), "Nothing matched map '%s', so the game asks again with map 'default_map'.", query_["map"].c_str());
    if (r.candidates.empty()) {
        ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "The database has no environment file for these conditions.");
    } else {
        ImGui::Text("%zu candidate(s); the game picks one uniformly at random (%.1f%% each).", r.candidates.size(), 100.0 / double(r.candidates.size()));
        if (ImGui::BeginTable("cands", 4, ImGuiTableFlags_RowBg | ImGuiTableFlags_Borders | ImGuiTableFlags_ScrollY, ImVec2(0, 220))) {
            ImGui::TableSetupColumn("File", ImGuiTableColumnFlags_WidthStretch);
            ImGui::TableSetupColumn("Allowed values", ImGuiTableColumnFlags_WidthFixed, 100);
            ImGui::TableSetupColumn("Pack", ImGuiTableColumnFlags_WidthFixed, 170);
            ImGui::TableSetupColumn("##open", ImGuiTableColumnFlags_WidthFixed, 50);
            ImGui::TableHeadersRow();
            for (size_t i = 0; i < r.candidates.size(); ++i) {
                const auto& c = r.candidates[i];
                const std::string path = Vfs::normalize(c.variation->filename);
                auto owner = vfs->owner(path);
                ImGui::PushID(int(i));
                ImGui::TableNextRow();
                ImGui::TableSetColumnIndex(0);
                ImGui::TextUnformatted(path.c_str());
                ImGui::TableSetColumnIndex(1);
                if (c.variation->flags == 0) ImGui::TextUnformatted("any");
                else ImGui::Text("%d of %zu", c.allowed_values, total_values);
                ImGui::TableSetColumnIndex(2);
                if (owner) ImGui::TextUnformatted(vfs->packs()[*owner].name.c_str());
                else ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "missing in all packs");
                ImGui::TableSetColumnIndex(3);
                if (owner && ImGui::SmallButton("Open")) {
                    std::string msg;
                    if (!editor.open_path(path, true, &msg)) editor.status(msg, true);
                }
                ImGui::PopID();
            }
            ImGui::EndTable();
        }
    }

    // Reverse lookup for the open document.
    ImGui::Separator();
    if (!active_environment.empty()) {
        ImGui::Text("Open file: %s", active_environment.c_str());
        const VariationDb::Entry* env = db_->entry("environment");
        int found = 0;
        if (env)
            for (auto& v : env->variations) {
                if (Vfs::normalize(v.filename) != active_environment) continue;
                ++found;
                ImGui::BulletText("selected when:");
                for (auto& [cat, allowed] : db_->decode(v.flags)) {
                    const VariationDb::Meta* m = db_->meta(cat);
                    std::string s;
                    if (allowed.empty()) s = "(nothing - never matches)";
                    else if (m && allowed.size() == m->values.size()) s = "any";
                    else
                        for (auto& a : allowed) s += (s.empty() ? "" : ", ") + a;
                    ImGui::Text("    %-22s %s", cat.c_str(), s.c_str());
                }
            }
        if (!found) ImGui::TextColored(kDim, "No variation refers to this file: weather selection never picks it.");
    } else {
        ImGui::TextColored(kDim, "Open an .environment file from the pack list to see the conditions under which the game picks it.");
    }
    ImGui::EndTabItem();
    ImGui::EndTabBar();
    ImGui::End();
}

} // namespace wce
