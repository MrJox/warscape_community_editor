#include "editor/editor.h"

#include "editor/panels.h"
#include "editor/steam_detect.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "scene/environment.h"

#include <imgui_internal.h>

#include <algorithm>
#include <cstdlib>
#include <fstream>

namespace fs = std::filesystem;

namespace wce {

namespace {

bool parse_float(const std::string& s, float& out) {
    char* end = nullptr;
    out = std::strtof(s.c_str(), &end);
    return end && *end == '\0' && !s.empty();
}

bool parse_u64(const std::string& s, uint64_t& out) {
    char* end = nullptr;
    out = std::strtoull(s.c_str(), &end, 10);
    return end && *end == '\0' && !s.empty();
}

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

} // namespace

// ---- Tool

void Tool::default_layout(ImGuiID dockspace) {
    ImGuiID center = dockspace, left = 0, right = 0, bottom = 0;
    left = ImGui::DockBuilderSplitNode(center, ImGuiDir_Left, 0.20f, nullptr, &center);
    right = ImGui::DockBuilderSplitNode(center, ImGuiDir_Right, 0.25f, nullptr, &center);
    bottom = ImGui::DockBuilderSplitNode(center, ImGuiDir_Down, 0.25f, nullptr, &center);
    ImGui::DockBuilderDockWindow("Viewport", center);
    ImGui::DockBuilderDockWindow("Outliner", left);
    ImGui::DockBuilderDockWindow("Inspector", right);
    ImGui::DockBuilderDockWindow("Assets", bottom);
    ImGui::DockBuilderDockWindow("Log", bottom);
    ImGui::DockBuilderDockWindow("History", bottom);
    ImGui::DockBuilderDockWindow(title(), right);   // a tool's main panel, when it has one named like the tool, tabs with the inspector
}

void PlaceholderTool::draw_panels(Editor& editor) {
    (void)editor;
    ImGui::SetNextWindowSize(ImVec2(420, 160), ImGuiCond_FirstUseEver);
    if (ImGui::Begin(title_.c_str())) {
        ImGui::TextWrapped("%s", plan_.c_str());
        ImGui::Separator();
        ImGui::TextDisabled("Registered so that the tool switcher, the layout files and document routing are exercised. No features yet.");
    }
    ImGui::End();
}

// ---- Editor

Editor::Editor(ID3D11Device* dev, ID3D11DeviceContext* ctx) : dev_(dev), ctx_(ctx), settings_(Settings::load()) { register_builtin_commands(); }
Editor::~Editor() = default;

void Editor::add_tool(std::unique_ptr<Tool> tool) {
    tool->register_commands(*this, commands_);
    tools_.push_back(std::move(tool));
}

void Editor::status(const std::string& text, bool error) {
    status_ = text;
    status_error_ = error;
    if (error) Log::error(text);
    else Log::info(text);
}

bool Editor::start() {
    try {
        Vfs::Config cfg = Vfs::default_config();
        auto v = std::make_unique<Vfs>(cfg);
        v->mount();
        vfs_ = std::move(v);
        renderer_ = std::make_unique<Renderer>(dev_, ctx_, *vfs_);
        renderer_->apply_graphics_options(read_graphics_options());
        first_run_ = false;
        std::string msg = "Mounted " + std::to_string(vfs_->packs().size()) + " packs, " + std::to_string(vfs_->files().size()) + " files.";
        for (auto& m : vfs_->missing_mods()) msg += " Missing mod: " + m + ".";
        if (!vfs_->backend_note().empty()) msg += " " + vfs_->backend_note() + ".";
        status(msg);
        return true;
    } catch (const std::exception& e) {
        vfs_.reset();
        renderer_.reset();
        first_run_ = true;
        Log::error(std::string("Start failed: ") + e.what());
        first_run_message_ = e.what();
        if (first_run_game_.empty()) {
            if (auto g = settings_.get("game_dir", "WCE_GAME")) first_run_game_ = *g;
            else first_run_game_ = narrow(detect_attila_folder().wstring());
        }
        if (first_run_rpfm_.empty()) {
            if (auto r = settings_.get("rpfm_dir", "WCE_RPFM_DIR")) first_run_rpfm_ = *r;
            else first_run_rpfm_ = narrow(detect_rpfm_folder(fs::path(widen(first_run_game_))).wstring());
        }
        return false;
    }
}

// ---- layouts

fs::path Editor::layout_file(const std::string& tool_id) const { return Settings::default_dir() / "layouts" / widen(tool_id + ".ini"); }

std::filesystem::path Editor::output_dir() const {
    if (auto v = settings_.get("output_dir", "WCE_OUTPUT_DIR")) return fs::path(widen(*v));
    const char* profile = std::getenv("USERPROFILE");
    return fs::path(widen(profile ? profile : "C:\\")) / "Documents" / "AttilaWeatherEditor" / "out";
}

void Editor::set_output_dir(const std::filesystem::path& dir, bool persist) {
    settings_.set("output_dir", narrow(dir.wstring()));
    if (persist) settings_.save();
}

void Editor::save_layout() {
    if (tools_.empty()) return;
    const fs::path p = layout_file(tools_[active_tool_]->id());
    std::error_code ec;
    fs::create_directories(p.parent_path(), ec);
    ImGui::SaveIniSettingsToDisk(narrow(p.wstring()).c_str());
}

void Editor::reset_layout() {
    if (tools_.empty()) return;
    std::error_code ec;
    fs::remove(layout_file(tools_[active_tool_]->id()), ec);
    layout_pending_default_ = true;
}

bool Editor::set_tool(const std::string& id) {
    for (size_t i = 0; i < tools_.size(); ++i)
        if (id == tools_[i]->id()) {
            pending_tool_ = int(i);
            return true;
        }
    return false;
}

void Editor::apply_tool_switch() {
    if (pending_tool_ < 0 || pending_tool_ >= int(tools_.size()) || pending_tool_ == active_tool_) {
        pending_tool_ = -1;
        return;
    }
    save_layout();
    active_tool_ = pending_tool_;
    pending_tool_ = -1;
    layout_loaded_ = false;
}

void Editor::release_test_keys() {
    ImGuiIO& io = ImGui::GetIO();
    for (HeldKey& k : test_keys_)
        if (--k.frames_left == 0) io.AddKeyEvent(k.key, false);
    test_keys_.erase(std::remove_if(test_keys_.begin(), test_keys_.end(), [](const HeldKey& k) { return k.frames_left <= 0; }), test_keys_.end());
}

void Editor::inject_input() {
    if (!clock_.fixed_step()) return;
    ImGuiIO& io = ImGui::GetIO();
    if (!mouse_active_) {
        io.AddMousePosEvent(-FLT_MAX, -FLT_MAX);   // the real cursor is ignored
        for (int b = 0; b < 3; ++b) io.AddMouseButtonEvent(b, false);
        return;
    }
    if (!mouse_queue_.empty()) {
        mouse_cur_ = mouse_queue_.front();
        mouse_queue_.pop_front();
    }
    const ImVec2 o = viewport_.image_origin();
    io.AddMousePosEvent(o.x + mouse_cur_.x, o.y + mouse_cur_.y);
    for (int b = 0; b < 3; ++b) io.AddMouseButtonEvent(b, mouse_cur_.down[b]);
    if (mouse_cur_.wheel != 0) {
        io.AddMouseWheelEvent(0, mouse_cur_.wheel);
        mouse_cur_.wheel = 0;
    }
}

void Editor::pre_frame() {
    release_test_keys();
    if (tools_.empty()) return;
    apply_tool_switch();
    if (!layout_loaded_) {
        const fs::path p = layout_file(tools_[active_tool_]->id());
        std::error_code ec;
        if (fs::exists(p, ec)) {
            ImGui::LoadIniSettingsFromDisk(narrow(p.wstring()).c_str());
            layout_pending_default_ = false;
        } else {
            ImGui::LoadIniSettingsFromMemory("", 0);
            layout_pending_default_ = true;
        }
        layout_loaded_ = true;
        if (renderer_) tools_[active_tool_]->activated(*this);
    }
}

// ---- documents

bool Editor::open_path(const std::string& path, bool from_packs, std::string* message) {
    const size_t dot = path.rfind('.');
    const std::string ext = dot == std::string::npos ? std::string() : lower(path.substr(dot));
    auto claims = [&](const Tool& t) {
        for (const std::string& e : t.opens())
            if (e == ext) return true;
        return false;
    };
    Tool* tool = active_tool() && claims(*active_tool()) ? active_tool() : nullptr;
    if (!tool)
        for (auto& t : tools_)
            if (claims(*t)) { tool = t.get(); break; }
    if (!tool) {
        if (message) *message = "no tool opens '" + ext + "' files";
        return false;
    }
    if (from_packs) {   // already open: just show it
        const std::string key = Vfs::normalize(path);
        for (DocumentId id : docs_.order())
            if (const Document* d = docs_.find(id); d && d->source_key() == key) {
                docs_.set_active(id);
                if (message) *message = "already open: " + path;
                return true;
            }
    }
    std::unique_ptr<Document> doc;
    try {
        doc = tool->open(*this, path, from_packs);
    } catch (const std::exception& e) {
        if (message) *message = std::string("cannot open ") + path + ": " + e.what();
        return false;
    }
    if (!doc) {
        if (message) *message = std::string(tool->title()) + " could not open " + path;
        return false;
    }
    const std::string tid = tool->id();
    docs_.add(std::move(doc), tid);
    if (tid != active_tool()->id()) set_tool(tid);
    if (message) *message = "opened " + path;
    return true;
}

// ---- frame

void Editor::frame() {
    ++frame_;
    if (script_.done() == false && !script_.failed()) {
        if (!script_.step(commands_)) request_quit(1);
    } else if (script_.failed()) {
        request_quit(1);
    }
    docs_.update();
    if (tools_.empty()) return;
    if (renderer_) tools_[active_tool_]->update(*this);

    ImGuiIO& io = ImGui::GetIO();
    const ImGuiViewport* vp = ImGui::GetMainViewport();
    ImGui::SetNextWindowPos(vp->WorkPos);
    ImGui::SetNextWindowSize(vp->WorkSize);
    ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.0f);
    ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.0f);
    ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0, 0));
    ImGui::Begin("##host", nullptr,
                 ImGuiWindowFlags_MenuBar | ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoCollapse | ImGuiWindowFlags_NoResize | ImGuiWindowFlags_NoMove |
                     ImGuiWindowFlags_NoBringToFrontOnFocus | ImGuiWindowFlags_NoNavFocus | ImGuiWindowFlags_NoDocking | ImGuiWindowFlags_NoSavedSettings);
    ImGui::PopStyleVar(3);
    draw_menu_bar();
    draw_toolbar();
    dockspace_ = ImGui::GetID("wce_dockspace");
    const ImVec2 avail = ImGui::GetContentRegionAvail();
    ImGui::DockSpace(dockspace_, avail, ImGuiDockNodeFlags_None);
    if (layout_pending_default_) {
        ImGui::DockBuilderRemoveNode(dockspace_);
        ImGui::DockBuilderAddNode(dockspace_, ImGuiDockNodeFlags_DockSpace);
        ImGui::DockBuilderSetNodeSize(dockspace_, avail);
        tools_[active_tool_]->default_layout(dockspace_);
        ImGui::DockBuilderFinish(dockspace_);
        layout_pending_default_ = false;
    }
    ImGui::End();

    if (first_run_) {
        draw_first_run();
        return;
    }

    // shortcuts (not while typing into a field)
    if (!io.WantTextInput) {
        if (io.KeyCtrl)
            for (int i = 0; i < 9 && i < int(tools_.size()); ++i)
                if (ImGui::IsKeyPressed(ImGuiKey(ImGuiKey_1 + i), false)) set_tool(tools_[i]->id());
        if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_Z, true)) commands_.run(io.KeyShift ? "redo" : "undo");
        if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_Y, true)) commands_.run("redo");
        if (io.KeyCtrl && ImGui::IsKeyPressed(ImGuiKey_S, false)) commands_.run(io.KeyShift ? "save_all" : "save");
        if (!io.KeyCtrl && viewport_.hovered) {
            if (ImGui::IsKeyPressed(ImGuiKey_W, false) && !viewport_.is_fly()) viewport_.gizmo = GizmoMode::Translate;
            if (ImGui::IsKeyPressed(ImGuiKey_E, false) && !viewport_.is_fly()) viewport_.gizmo = GizmoMode::Rotate;
            if (ImGui::IsKeyPressed(ImGuiKey_R, false) && !viewport_.is_fly()) viewport_.gizmo = GizmoMode::Scale;
            if (ImGui::IsKeyPressed(ImGuiKey_Q, false) && !viewport_.is_fly()) viewport_.gizmo = GizmoMode::Off;
            if (ImGui::IsKeyPressed(ImGuiKey_F, false)) viewport_.focus_selection(*this);
        }
    }

    draw_panels();
}

void Editor::draw_panels() {
    if (panels.viewport) viewport_.draw(*this);
    if (panels.outliner) Panels::outliner(*this);
    if (panels.inspector) Panels::inspector(*this);
    if (panels.assets) Panels::assets(*this);
    if (panels.log) Panels::log(*this);
    if (panels.history) Panels::history(*this);
    if (panels.commands) Panels::commands(*this);
    if (Tool* t = active_tool()) t->draw_panels(*this);
}

void Editor::draw_toolbar() {
    ImGui::SetCursorPosX(ImGui::GetCursorPosX() + 6);
    ImGui::AlignTextToFramePadding();
    for (size_t i = 0; i < tools_.size(); ++i) {
        const bool active = int(i) == active_tool_;
        if (active) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
        std::string label = std::string(tools_[i]->title());
        if (i < 9) label += "  Ctrl+" + std::to_string(i + 1);
        if (ImGui::Button((label + "##tool" + std::to_string(i)).c_str())) set_tool(tools_[i]->id());
        if (active) ImGui::PopStyleColor();
        ImGui::SameLine();
    }
    ImGui::TextDisabled("|");
    ImGui::SameLine();
    const char* gizmo_names[] = {"None", "Move", "Rotate", "Scale"};
    for (int g = 0; g < 4; ++g) {
        const bool on = int(viewport_.gizmo) == g;
        if (on) ImGui::PushStyleColor(ImGuiCol_Button, ImGui::GetStyleColorVec4(ImGuiCol_ButtonActive));
        if (ImGui::SmallButton(gizmo_names[g])) viewport_.gizmo = GizmoMode(g);
        if (on) ImGui::PopStyleColor();
        ImGui::SameLine();
    }
    ImGui::TextDisabled("|");
    ImGui::SameLine();
    ImGui::PushStyleColor(ImGuiCol_Text, status_error_ ? ImVec4(1.0f, 0.45f, 0.4f, 1) : ImGui::GetStyleColorVec4(ImGuiCol_TextDisabled));
    ImGui::TextUnformatted(status_.c_str());
    ImGui::PopStyleColor();
    ImGui::Separator();
}

void Editor::draw_menu_bar() {
    if (!ImGui::BeginMenuBar()) return;
    Document* doc = docs_.active();
    if (ImGui::BeginMenu("File")) {
        if (ImGui::MenuItem("Open from the game packs...")) panels.assets = true;
        if (ImGui::MenuItem("Open file...")) {
            std::wstring filter = L"Files this editor opens";
            filter.push_back(L'\0');
            std::wstring exts;
            for (auto& t : tools_)
                for (const std::string& e : t->opens()) {
                    const std::wstring w = L"*" + widen(e);
                    if (exts.find(w) == std::wstring::npos) exts += (exts.empty() ? L"" : L";") + w;
                }
            filter += exts.empty() ? L"*.*" : exts;
            filter.push_back(L'\0');
            filter += L"All files";
            filter.push_back(L'\0');
            filter += L"*.*";
            filter.push_back(L'\0');
            const fs::path p = file_dialog(false, output_dir() / "x", filter.c_str());
            std::string msg;
            if (!p.empty()) status(open_path(narrow(p.wstring()), false, &msg) ? msg : "open failed: " + msg, false);
        }
        if (ImGui::MenuItem("Save", "Ctrl+S", false, doc && doc->can_save() && doc->modified())) commands_.run("save");
        if (ImGui::MenuItem("Save as...", nullptr, false, doc && doc->can_save())) {
            const fs::path p = file_dialog(true, output_dir() / widen(doc->title()), L"All files\0*.*\0");
            std::string msg;
            if (!p.empty()) {
                if (doc->save({p}, &msg)) doc->history().mark_saved();
                status(msg, false);
            }
        }
        if (ImGui::MenuItem("Save all", "Ctrl+Shift+S", false, docs_.any_modified())) commands_.run("save_all");
        if (ImGui::MenuItem("Close document", nullptr, false, doc != nullptr)) commands_.run("close");
        if (ImGui::MenuItem("Remount packs")) commands_.run("remount");
        ImGui::Separator();
        if (ImGui::MenuItem("Save layout")) save_layout();
        if (ImGui::MenuItem("Reset layout")) reset_layout();
        ImGui::Separator();
        if (ImGui::MenuItem("Quit")) request_quit();
        ImGui::EndMenu();
    }
    if (ImGui::BeginMenu("Edit")) {
        CommandStack* h = doc ? &doc->history() : nullptr;
        const std::string undo = h && h->can_undo() ? "Undo " + h->name(h->position() - 1) : "Undo";
        const std::string redo = h && h->can_redo() ? "Redo " + h->name(h->position()) : "Redo";
        if (ImGui::MenuItem(undo.c_str(), "Ctrl+Z", false, h && h->can_undo())) commands_.run("undo");
        if (ImGui::MenuItem(redo.c_str(), "Ctrl+Y", false, h && h->can_redo())) commands_.run("redo");
        ImGui::Separator();
        if (ImGui::MenuItem("Select none", nullptr, false, !selection_.empty())) selection_.clear();
        ImGui::EndMenu();
    }
    if (ImGui::BeginMenu("View")) {
        ImGui::MenuItem("Viewport", nullptr, &panels.viewport);
        ImGui::MenuItem("Outliner", nullptr, &panels.outliner);
        ImGui::MenuItem("Inspector", nullptr, &panels.inspector);
        ImGui::MenuItem("Assets", nullptr, &panels.assets);
        ImGui::MenuItem("Log", nullptr, &panels.log);
        ImGui::MenuItem("History", nullptr, &panels.history);
        ImGui::Separator();
        if (ImGui::MenuItem("Orbit camera", nullptr, !viewport_.is_fly())) viewport_.use_orbit();
        if (ImGui::MenuItem("Fly camera", nullptr, viewport_.is_fly())) viewport_.use_fly();
        if (ImGui::MenuItem("Focus selection", "F", false, !selection_.empty())) viewport_.focus_selection(*this);
        ImGui::MenuItem("Grid", nullptr, &viewport_.show_grid);
        ImGui::MenuItem("Snap", nullptr, &viewport_.snap);
        ImGui::EndMenu();
    }
    if (Tool* t = active_tool()) t->draw_menus(*this);
    if (ImGui::BeginMenu("Help")) {
        ImGui::MenuItem("Commands", nullptr, &panels.commands);
        ImGui::EndMenu();
    }
    ImGui::EndMenuBar();
}

bool Editor::setup(const std::string& game_dir, const std::string& rpfm_dir, std::string* message) {
    settings_.set("game_dir", game_dir);
    if (!rpfm_dir.empty()) settings_.set("rpfm_dir", rpfm_dir);
    if (settings_.save()) Log::info("Saved " + narrow(settings_.file().wstring()));
    else Log::error("Cannot write " + narrow(settings_.file().wstring()));
    // Settings::load happens in Vfs::default_config(); the environment may still override game_dir.
    if (!start()) {
        if (message) *message = first_run_message_;
        return false;
    }
    if (!tools_.empty()) tools_[active_tool_]->activated(*this);
    if (message) *message = "mounted " + std::to_string(vfs_->packs().size()) + " packs";
    return true;
}

void Editor::draw_first_run() {
    ImGui::OpenPopup("First run");
    const ImGuiViewport* vp = ImGui::GetMainViewport();
    ImGui::SetNextWindowPos(vp->GetCenter(), ImGuiCond_Always, ImVec2(0.5f, 0.5f));
    ImGui::SetNextWindowSize(ImVec2(640, 0));
    if (!ImGui::BeginPopupModal("First run", nullptr, ImGuiWindowFlags_AlwaysAutoResize)) return;
    ImGui::TextWrapped("Warscape Community Editor needs Total War: Attila and RPFM. Nothing from the game is copied: files are read from your installation.");
    ImGui::Spacing();
    char buf[1024];
    auto field = [&](const char* label, std::string& value, const wchar_t* title) {
        std::snprintf(buf, sizeof buf, "%s", value.c_str());
        ImGui::SetNextItemWidth(470);
        if (ImGui::InputText(label, buf, sizeof buf)) value = buf;
        ImGui::SameLine();
        if (ImGui::Button((std::string("Browse##") + label).c_str())) {
            fs::path p = pick_folder(title);
            if (!p.empty()) value = narrow(p.wstring());
        }
    };
    field("Game folder", first_run_game_, L"Total War Attila folder");
    field("RPFM folder", first_run_rpfm_, L"Folder with rpfm_cli.exe");
    if (ImGui::Button("Detect")) {
        fs::path g = detect_attila_folder();
        if (!g.empty()) first_run_game_ = narrow(g.wstring());
        fs::path r = detect_rpfm_folder(g.empty() ? fs::path(widen(first_run_game_)) : g);
        if (!r.empty()) first_run_rpfm_ = narrow(r.wstring());
    }
    ImGui::SameLine();
    const bool game_ok = !first_run_game_.empty() && fs::exists(fs::path(widen(first_run_game_)) / "data");
    const bool rpfm_ok = !first_run_rpfm_.empty() && fs::exists(fs::path(widen(first_run_rpfm_)) / "rpfm_cli.exe");
    if (!game_ok) ImGui::TextColored(ImVec4(1, 0.5f, 0.4f, 1), "no data folder in the game folder");
    else if (!rpfm_ok) ImGui::TextColored(ImVec4(1, 0.8f, 0.4f, 1), "rpfm_cli.exe not found there (needed when rpfm_server cannot start)");
    if (!first_run_message_.empty()) {
        ImGui::PushTextWrapPos(600);
        ImGui::TextDisabled("%s", first_run_message_.c_str());
        ImGui::PopTextWrapPos();
    }
    ImGui::Spacing();
    ImGui::BeginDisabled(!game_ok);
    if (ImGui::Button("Save and continue")) {
        std::string msg;
        ImGui::CloseCurrentPopup();
        if (!setup(first_run_game_, first_run_rpfm_, &msg)) first_run_message_ = msg;
    }
    ImGui::EndDisabled();
    ImGui::EndPopup();
}

// ---- commands

void Editor::register_builtin_commands() {
    auto ok = [](std::string m = {}) { return CommandResult::success(std::move(m)); };
    auto fail = [](std::string m) { return CommandResult::fail(std::move(m)); };

    commands_.add("help", "help [name]", "list the commands, or show one", [this, ok, fail](const CommandRegistry::Args& a) {
        if (!a.empty()) {
            const auto* e = commands_.find(a[0]);
            return e ? ok(e->usage + " - " + e->help) : fail("unknown command");
        }
        std::string all;
        for (const auto* e : commands_.entries()) all += e->usage + "\n";
        return ok(all);
    });
    commands_.add("echo", "echo text...", "write to the log", [ok](const CommandRegistry::Args& a) {
        std::string s;
        for (const auto& w : a) s += (s.empty() ? "" : " ") + w;
        Log::info(s);
        return ok();
    });
    commands_.add("frames", "frames N", "script: wait N rendered frames", [this, ok, fail](const CommandRegistry::Args& a) {
        uint64_t n;
        if (a.empty() || !parse_u64(a[0], n)) return fail("usage: frames N");
        commands_.wait_frames = int(std::min<uint64_t>(n, 100000));
        return ok();
    });
    commands_.add("quit", "quit [exit code]", "leave the editor", [this, ok](const CommandRegistry::Args& a) {
        uint64_t code = 0;
        if (!a.empty()) parse_u64(a[0], code);
        request_quit(int(code));
        return ok();
    });
    commands_.add("tool", "tool <id>", "switch the active tool (weather, tile, ...)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: tool <id>");
        return set_tool(a[0]) ? ok() : fail("no tool '" + a[0] + "'");
    });
    commands_.add("assert_tool", "assert_tool <id>", "test: the active tool is <id> (after a frame)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: assert_tool <id>");
        return active_tool() && a[0] == active_tool()->id() ? ok() : fail(std::string("active tool is ") + (active_tool() ? active_tool()->id() : "none"));
    });
    commands_.add("open", "open <path> [disk]", "open a file from the game packs (or from disk)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: open <path> [disk]");
        std::string msg;
        const bool packs = !(a.size() > 1 && a[1] == "disk");
        return open_path(a[0], packs, &msg) ? ok(msg) : fail(msg);
    });
    commands_.add("close", "close", "close the active document", [this, ok, fail](const CommandRegistry::Args&) {
        Document* d = docs_.active();
        if (!d) return fail("no document");
        selection_.remove_document(d->id());
        docs_.close(d->id());
        return ok();
    });
    commands_.add("save", "save", "save the active document", [this, ok, fail](const CommandRegistry::Args&) {
        Document* d = docs_.active();
        if (!d) return fail("no document");
        std::string msg;
        if (!d->save({}, &msg)) return fail(msg);
        d->history().mark_saved();
        return ok("saved " + d->title());
    });
    commands_.add("save_all", "save_all", "save every modified document", [this, ok, fail](const CommandRegistry::Args&) {
        std::vector<std::string> msgs;
        const bool all = docs_.save_all(&msgs);
        std::string s;
        for (auto& m : msgs) s += (s.empty() ? "" : "; ") + m;
        return all ? ok(s) : fail(s);
    });
    commands_.add("undo", "undo [n]", "undo the last edit (or n edits) of the active document", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        const int n = a.empty() ? 1 : std::max(1, std::atoi(a[0].c_str()));
        bool any = false;
        for (int i = 0; d && i < n; ++i) any = d->history().undo() || any;
        return any ? ok() : fail("nothing to undo");
    });
    commands_.add("redo", "redo [n]", "redo (n edits)", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        const int n = a.empty() ? 1 : std::max(1, std::atoi(a[0].c_str()));
        bool any = false;
        for (int i = 0; d && i < n; ++i) any = d->history().redo() || any;
        return any ? ok() : fail("nothing to redo");
    });
    commands_.add("select", "select none | select [add] <item>...", "select items of the active document", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        if (!d) return fail("no document");
        if (!a.empty() && a[0] == "none") { selection_.clear(); return ok(); }
        size_t i = 0;
        bool add = false;
        if (!a.empty() && a[0] == "add") { add = true; i = 1; }
        if (!add) selection_.clear();
        if (i >= a.size()) return fail("usage: select none | select [add] <item>...");
        for (; i < a.size(); ++i) {
            uint64_t item;
            if (!parse_u64(a[i], item)) return fail("bad item '" + a[i] + "'");
            selection_.add({d->id(), item});
        }
        return ok();
    });
    commands_.add("move", "move <item> dx dy dz", "move an item of the active document by a world-space offset (one undo entry)", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        uint64_t item;
        float v[3];
        if (!d || a.size() < 4 || !parse_u64(a[0], item) || !parse_float(a[1], v[0]) || !parse_float(a[2], v[1]) || !parse_float(a[3], v[2]))
            return fail("usage: move <item> dx dy dz");
        static uint64_t gesture = 1u << 40;
        d->transform_items({SourceRef{d->id(), item}}, translation({v[0], v[1], v[2]}), ++gesture);
        d->history().break_merge();
        return ok();
    });
    commands_.add("assert_selected", "assert_selected N", "test: N items are selected", [this, ok, fail](const CommandRegistry::Args& a) {
        uint64_t n;
        if (a.empty() || !parse_u64(a[0], n)) return fail("usage: assert_selected N");
        return selection_.items().size() == n ? ok() : fail("selected " + std::to_string(selection_.items().size()) + ", expected " + a[0]);
    });
    commands_.add("assert_modified", "assert_modified 0|1", "test: the active document is (not) modified", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        if (!d || a.empty()) return fail("usage: assert_modified 0|1 (needs a document)");
        return d->modified() == (a[0] == "1") ? ok() : fail(std::string("modified is ") + (d->modified() ? "1" : "0"));
    });
    commands_.add("assert_history", "assert_history size position", "test: the active document's undo stack has `size` entries, `position` applied", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        uint64_t s, p;
        if (!d || a.size() < 2 || !parse_u64(a[0], s) || !parse_u64(a[1], p)) return fail("usage: assert_history size position");
        return d->history().size() == s && d->history().position() == p ? ok()
               : fail("history is " + std::to_string(d->history().size()) + "/" + std::to_string(d->history().position()));
    });
    commands_.add("assert_documents", "assert_documents N", "test: N documents are open", [this, ok, fail](const CommandRegistry::Args& a) {
        uint64_t n;
        if (a.empty() || !parse_u64(a[0], n)) return fail("usage: assert_documents N");
        return docs_.order().size() == n ? ok() : fail("documents: " + std::to_string(docs_.order().size()));
    });
    commands_.add("cam", "cam yaw pitch dist [target_y]", "orbit camera, radians and metres (like weather_editor --cam)", [this, ok, fail](const CommandRegistry::Args& a) {
        float yaw, pitch, dist, ty = 5;
        if (a.size() < 3 || !parse_float(a[0], yaw) || !parse_float(a[1], pitch) || !parse_float(a[2], dist)) return fail("usage: cam yaw pitch dist [target_y]");
        if (a.size() > 3) parse_float(a[3], ty);
        viewport_.use_orbit();
        auto* o = static_cast<OrbitCamera*>(&viewport_.controller());
        o->set_orbit(yaw, pitch, dist, {o->target.x, ty, o->target.z});
        return ok();
    });
    commands_.add("camera", "camera orbit|fly", "camera controller", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: camera orbit|fly");
        if (a[0] == "fly") viewport_.use_fly();
        else viewport_.use_orbit();
        return ok();
    });
    commands_.add("focus", "focus", "frame the selection in the viewport", [this, ok](const CommandRegistry::Args&) {
        viewport_.focus_selection(*this);
        return ok();
    });
    commands_.add("gizmo", "gizmo off|translate|rotate|scale", "gizmo mode", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: gizmo off|translate|rotate|scale");
        viewport_.gizmo = a[0] == "translate" ? GizmoMode::Translate : a[0] == "rotate" ? GizmoMode::Rotate : a[0] == "scale" ? GizmoMode::Scale : GizmoMode::Off;
        return ok();
    });
    commands_.add("grid", "grid on|off", "viewport grid", [this, ok](const CommandRegistry::Args& a) {
        viewport_.show_grid = !a.empty() && a[0] == "on";
        return ok();
    });
    commands_.add("panel", "panel <name> on|off", "show or hide a panel (viewport outliner inspector assets log history commands)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.size() < 2) return fail("usage: panel <name> on|off");
        const bool on = a[1] == "on";
        bool* flags[] = {&panels.viewport, &panels.outliner, &panels.inspector, &panels.assets, &panels.log, &panels.history, &panels.commands};
        const char* names[] = {"viewport", "outliner", "inspector", "assets", "log", "history", "commands"};
        for (int i = 0; i < 7; ++i)
            if (a[0] == names[i]) { *flags[i] = on; return ok(); }
        return fail("no panel '" + a[0] + "'");
    });
    commands_.add("layout", "layout save|reset", "save or reset the active tool's dock layout", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: layout save|reset");
        if (a[0] == "save") save_layout();
        else if (a[0] == "reset") reset_layout();
        else return fail("usage: layout save|reset");
        return ok();
    });
    commands_.add("shot", "shot <file.png>", "save the whole window after this frame (the app loop does it)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: shot <file.png>");
        pending_shot = widen(a[0]);
        return ok();
    });
    commands_.add("capture", "capture WxH <file.png> [frames]", "render the scene offscreen (no window) and save a PNG", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.size() < 2) return fail("usage: capture WxH <file.png> [frames]");
        const int w = std::atoi(a[0].c_str());
        const size_t x = a[0].find('x');
        const int h = x == std::string::npos ? 0 : std::atoi(a[0].c_str() + x + 1);
        const int frames = a.size() > 2 ? std::max(1, std::atoi(a[2].c_str())) : 20;
        return viewport_.capture(*this, w, h, frames, widen(a[1])) ? ok() : fail("capture failed");
    });
    commands_.add("pick_pixel", "pick_pixel x y", "log the entity under a viewport pixel (of the last frame)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.size() < 2) return fail("usage: pick_pixel x y");
        const EntityId id = viewport_.pick(*this, std::atoi(a[0].c_str()), std::atoi(a[1].c_str()));
        const Entity* e = scene_.find(id);
        return ok(e ? "entity " + std::to_string(id) + " doc " + std::to_string(e->source.doc) + " item " + std::to_string(e->source.item) : "nothing");
    });
    commands_.add("click", "click x y [ctrl]", "select what is under a viewport pixel, as a click would", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.size() < 2) return fail("usage: click x y [ctrl]");
        const EntityId id = viewport_.pick(*this, std::atoi(a[0].c_str()), std::atoi(a[1].c_str()));
        const Entity* e = scene_.find(id);
        const bool ctrl = a.size() > 2 && a[2] == "ctrl";
        if (!e || e->source.doc == 0) {
            if (!ctrl) selection_.clear();
        } else if (ctrl) selection_.toggle(e->source);
        else selection_.set(e->source);
        return ok();
    });
    commands_.add("click_item", "click_item <item> [ctrl]", "click the viewport pixel where an item of the active document is (its pivot), as the mouse would", [this, ok, fail](const CommandRegistry::Args& a) {
        Document* d = docs_.active();
        uint64_t item;
        float4x4 world;
        if (!d || a.empty() || !parse_u64(a[0], item) || !d->item_transform({d->id(), item}, world)) return fail("usage: click_item <item> [ctrl] (the item needs a transform)");
        float px, py;
        if (!viewport_.target().valid() || !Renderer::project(viewport_.camera(), viewport_.width(), viewport_.height(), {world.m[0][3], world.m[1][3], world.m[2][3]}, px, py))
            return fail("the item is not in view");
        return commands_.run("click", {std::to_string(int(px)), std::to_string(int(py)), a.size() > 1 ? a[1] : ""});
    });
    commands_.add("assert_selected_item", "assert_selected_item <item>", "test: the first selected item of the active document is <item>", [this, ok, fail](const CommandRegistry::Args& a) {
        uint64_t item;
        if (a.empty() || !parse_u64(a[0], item)) return fail("usage: assert_selected_item <item>");
        if (selection_.empty()) return fail("nothing is selected");
        return selection_.items()[0].item == item ? ok() : fail("selected item is " + std::to_string(selection_.items()[0].item));
    });
    commands_.add("mouse", "mouse move x y | click x y [left|right|middle] | drag x0 y0 x1 y1 [steps] [button] | wheel n | off",
                  "drive the viewport with a scripted mouse (pixels of the viewport image), as the real one would; every step takes a rendered frame", [this, ok, fail](const CommandRegistry::Args& a) {
        // pixels, or "50%" of the viewport image's width (x) / height (y): the tests do not know the window size
        int axis = 0;
        auto num = [this, &axis](const std::string& t, float& v) {
            char* e = nullptr;
            v = std::strtof(t.c_str(), &e);
            if (e && e != t.c_str() && *e == '%' && e[1] == 0) {
                v = v * 0.01f * float(axis == 0 ? viewport_.width() : viewport_.height());
                return true;
            }
            return e && e != t.c_str() && *e == 0;
        };
        auto button = [](const CommandRegistry::Args& args, size_t at) { return at < args.size() && args[at] == "right" ? 1 : at < args.size() && args[at] == "middle" ? 2 : 0; };
        if (a.empty()) return fail("usage: mouse move|click|drag|wheel|off ...");
        MouseState m = mouse_cur_;
        float x0, y0, x1, y1;
        int frames = 0;
        if (a[0] == "off") { park_mouse(); return ok(); }
        if (a[0] == "wheel" && a.size() > 1 && (axis = 0, num(a[1], x0))) { m.wheel = x0; queue_mouse(m); commands_.wait_frames = 2; return ok(); }
        auto xy = [&](size_t at, float& x, float& y) { axis = 0; if (at + 1 >= a.size() || !num(a[at], x)) return false; axis = 1; return num(a[at + 1], y); };
        if (a[0] == "move" && a.size() > 2 && xy(1, x0, y0)) {
            m.x = x0; m.y = y0;
            queue_mouse(m);
            commands_.wait_frames = 2;
            return ok();
        }
        if (a[0] == "click" && a.size() > 2 && xy(1, x0, y0)) {
            const int b = button(a, 3);
            m.x = x0; m.y = y0;
            for (int i = 0; i < 4; ++i) {
                m.down[b] = i == 1 || i == 2;
                queue_mouse(m);
                ++frames;
            }
            commands_.wait_frames = frames + 2;
            return ok();
        }
        if (a[0] == "drag" && a.size() > 4 && xy(1, x0, y0) && xy(3, x1, y1)) {
            float n = 8;
            axis = 0;
            if (a.size() > 5) num(a[5], n);
            n = std::max(1.0f, n);
            const int b = button(a, 6);
            m.x = x0; m.y = y0;
            m.down[b] = false;
            queue_mouse(m);
            m.down[b] = true;
            queue_mouse(m);
            frames = 2;
            for (int i = 1; i <= int(n); ++i) {
                m.x = x0 + (x1 - x0) * float(i) / n;
                m.y = y0 + (y1 - y0) * float(i) / n;
                queue_mouse(m);
                ++frames;
            }
            queue_mouse(m);
            m.down[b] = false;
            queue_mouse(m);
            frames += 2;
            commands_.wait_frames = frames + 2;
            return ok();
        }
        return fail("usage: mouse move x y | click x y [button] | drag x0 y0 x1 y1 [steps] [button] | wheel n | off");
    });
    commands_.add("key", "key [Ctrl+][Shift+][Alt+]<A-Z|0-9|F1-F12|Delete|Escape|Enter|Space|Tab>", "press a key combination (held for two frames), as the keyboard would", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: key Ctrl+1");
        ImGuiIO& io = ImGui::GetIO();
        std::string rest = a[0];
        std::vector<ImGuiKey> mods;
        for (;;) {
            const std::string l = lower(rest);
            if (l.rfind("ctrl+", 0) == 0) { mods.push_back(ImGuiMod_Ctrl); rest = rest.substr(5); }
            else if (l.rfind("shift+", 0) == 0) { mods.push_back(ImGuiMod_Shift); rest = rest.substr(6); }
            else if (l.rfind("alt+", 0) == 0) { mods.push_back(ImGuiMod_Alt); rest = rest.substr(4); }
            else break;
        }
        ImGuiKey key = ImGuiKey_None;
        if (rest.size() == 1 && rest[0] >= '0' && rest[0] <= '9') key = ImGuiKey(ImGuiKey_0 + (rest[0] - '0'));
        else if (rest.size() == 1 && std::isalpha(static_cast<unsigned char>(rest[0]))) key = ImGuiKey(ImGuiKey_A + (std::toupper(static_cast<unsigned char>(rest[0])) - 'A'));
        else if (lower(rest) == "delete") key = ImGuiKey_Delete;
        else if (lower(rest) == "escape") key = ImGuiKey_Escape;
        else if (lower(rest) == "enter" || lower(rest) == "return") key = ImGuiKey_Enter;
        else if (lower(rest) == "space") key = ImGuiKey_Space;
        else if (lower(rest) == "tab") key = ImGuiKey_Tab;
        else if (rest.size() >= 2 && (rest[0] == 'F' || rest[0] == 'f')) {
            const int n = std::atoi(rest.c_str() + 1);
            if (n >= 1 && n <= 12) key = ImGuiKey(ImGuiKey_F1 + n - 1);
        }
        if (key == ImGuiKey_None) return fail("unknown key '" + rest + "'");
        for (ImGuiKey m : mods) { io.AddKeyEvent(m, true); test_keys_.push_back({m, 3}); }
        io.AddKeyEvent(key, true);
        test_keys_.push_back({key, 3});
        return ok();
    });
    commands_.add("show_panel", "show_panel <name>", "show a panel and bring it to the front of its tab group", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: show_panel <viewport|outliner|inspector|assets|log|history|commands>");
        const std::string n = lower(a[0]);
        bool* flags[] = {&panels.viewport, &panels.outliner, &panels.inspector, &panels.assets, &panels.log, &panels.history, &panels.commands};
        const char* names[] = {"viewport", "outliner", "inspector", "assets", "log", "history", "commands"};
        const char* titles[] = {"Viewport", "Outliner", "Inspector", "Assets", "Log", "History", "Commands"};
        for (int i = 0; i < 7; ++i)
            if (n == names[i]) {
                *flags[i] = true;
                ImGui::SetWindowFocus(titles[i]);
                return ok();
            }
        return fail("no panel '" + a[0] + "'");
    });
    commands_.add("setup", "setup <game folder> [rpfm folder]", "what the first-run dialog's button does: store the folders in wce.ini and mount", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: setup <game folder> [rpfm folder]");
        std::string msg;
        return setup(a[0], a.size() > 1 ? a[1] : std::string(), &msg) ? ok(msg) : fail(msg);
    });
    commands_.add("assert_mounted", "assert_mounted 0|1", "test: the game packs are (not) mounted", [this, ok, fail](const CommandRegistry::Args& a) {
        const bool want = a.empty() || a[0] != "0";
        return (vfs_ != nullptr) == want ? ok() : fail(vfs_ ? "mounted" : "not mounted");
    });
    commands_.add("remount", "remount", "read the packs again (after the team edited one) and rebuild the renderer", [this, ok, fail](const CommandRegistry::Args&) {
        const bool again = start();
        if (!again) return fail("mount failed");
        for (auto& t : tools_) t->activated(*this);
        return ok(status_);
    });
    commands_.add("script", "script <file>", "run a script file", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: script <file>");
        std::string err;
        if (!script_.include_file(a[0], &err)) return fail(err);
        return ok();
    });
    commands_.add("assert_log_errors", "assert_log_errors N", "test: the log holds N error entries", [ok, fail](const CommandRegistry::Args& a) {
        const size_t n = Log::instance().count(LogLevel::Error);
        return a.empty() || n == std::strtoull(a[0].c_str(), nullptr, 10) ? ok() : fail("log has " + std::to_string(n) + " errors");
    });
}

} // namespace wce
