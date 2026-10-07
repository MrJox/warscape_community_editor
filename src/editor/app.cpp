#include "editor/app.h"

#include "core/process.h"
#include "formats/env_value.h"

#include "imgui.h"
#include "render/environment.h"

#include <windows.h>
#include <commdlg.h>

#include <algorithm>
#include <cmath>
#include <fstream>

namespace fs = std::filesystem;

namespace we {

namespace {

struct Category {
    const char* label;
    const char* prefix;
    const char* ext;
};
const Category kCategories[] = {
    {"Battle environments", "", ".environment"},
    {"Lighting (campaign, porthole)", "", ".lighting"},
    {"Weather definitions", "weather/", ".xml"},
    {"VFX", "vfx/", ".xml"},
};

const ImVec4 kModified(1.0f, 0.75f, 0.3f, 1.0f);
const ImVec4 kMod(0.55f, 0.85f, 1.0f, 1.0f);
const ImVec4 kDim(0.6f, 0.6f, 0.6f, 1.0f);

std::optional<fs::path> file_dialog(bool save, const fs::path& initial) {
    wchar_t buf[MAX_PATH * 4] = {};
    std::wstring init = initial.filename().wstring();
    wcsncpy(buf, init.c_str(), std::size(buf) - 1);
    std::wstring dir = initial.parent_path().wstring();
    OPENFILENAMEW ofn{sizeof ofn};
    ofn.lpstrFilter = L"Weather files (*.environment;*.lighting;*.xml)\0*.environment;*.lighting;*.xml\0All files\0*.*\0";
    ofn.lpstrFile = buf;
    ofn.nMaxFile = DWORD(std::size(buf));
    ofn.lpstrInitialDir = dir.empty() ? nullptr : dir.c_str();
    ofn.Flags = OFN_NOCHANGEDIR | (save ? OFN_OVERWRITEPROMPT : OFN_FILEMUSTEXIST);
    BOOL ok = save ? GetSaveFileNameW(&ofn) : GetOpenFileNameW(&ofn);
    if (!ok) return std::nullopt;
    return fs::path(buf);
}

std::string element_label(const XmlDoc& doc, int e) {
    const XmlElement& el = doc.elements()[e];
    std::string label = el.name;
    if (auto n = doc.get(e, "name")) label += "  '" + *n + "'";
    else if (auto n2 = doc.get(e, "inst_name")) label += "  '" + *n2 + "'";
    return label;
}

bool contains_ci(const std::string& hay, const char* needle) {
    if (!*needle) return true;
    std::string n = needle;
    auto it = std::search(hay.begin(), hay.end(), n.begin(), n.end(),
                          [](char a, char b) { return std::tolower(uint8_t(a)) == std::tolower(uint8_t(b)); });
    return it != hay.end();
}

} // namespace

std::string Document::title() const {
    std::string name = vfs_path.empty() ? source_disk.filename().string() : vfs_path.substr(vfs_path.rfind('/') + 1);
    return (doc.modified() ? "* " : "") + name;
}

App::App(ID3D11Device* dev, ID3D11DeviceContext* ctx) : dev_(dev), ctx_(ctx) {
    Vfs::Config cfg = Vfs::default_config();
    settings_path_ = cfg.cache_dir / "settings.ini";
    output_dir_ = fs::path(widen(std::getenv("USERPROFILE") ? std::getenv("USERPROFILE") : "C:\\")) / "Documents" / "AttilaWeatherEditor" / "out";
    load_settings();
    mount();
    init_renderer();
}

void App::open_path(const std::string& path) {
    if (vfs_ && vfs_->owner(path)) open_vfs(Vfs::normalize(path));
    else open_disk(fs::path(widen(path)));
}

std::string App::diagnostics() const {
    std::string d = "status: " + status_ + "\n";
    d += "scene objects: " + std::to_string(scene_ ? scene_->objects().size() : 0) + "\n";
    if (renderer_) d += "cloud sprites: " + std::to_string(renderer_->cloud_sprites()) + "\n";
    if (scene_)
        for (auto& e : scene_->errors()) d += "scene error: " + e + "\n";
    if (textures_)
        for (auto& e : textures_->errors()) d += "texture error: " + e + "\n";
    if (renderer_) {
        d += "points readback: " + std::to_string(renderer_->debug_maps) + " maps, raw " + std::to_string(renderer_->debug_points[0]) + " " +
             std::to_string(renderer_->debug_points[1]) + " " + std::to_string(renderer_->debug_points[2]) + " " + std::to_string(renderer_->debug_points[3]) + "\n";
        d += "luma black/white points: " + std::to_string(renderer_->luma_black()) + " / " + std::to_string(renderer_->luma_white()) + "\n";
        for (auto& e : renderer_->errors()) d += "render error: " + e + "\n";
        for (auto& m : renderer_->cbuffers().missing()) d += "missing cb var: " + m + "\n";
    }
    return d;
}

void App::init_renderer() {
    if (!vfs_) return;
    try {
        textures_ = std::make_unique<TextureCache>(dev_, *vfs_);
        scene_ = std::make_unique<TestScene>(dev_, *vfs_, *textures_);
        renderer_ = std::make_unique<Renderer>(dev_, ctx_, *vfs_);
        read_output_settings(output_gamma_, output_brightness_);
        int n = scene_->generate(uint32_t(scene_seed_), scene_count_);
        float3 ext = scene_->bounds_max() - scene_->bounds_min();
        cam_dist_ = std::max(60.0f, std::max(ext.x, ext.z) * 0.9f);
        start_time_ = ImGui::GetTime();
        set_status(status_ + " Test map: " + std::to_string(n) + " buildings.", status_error_);
    } catch (const std::exception& e) {
        set_status(std::string("Renderer init failed: ") + e.what(), true);
    }
}

void App::draw_viewport() {
    if (!renderer_) {
        ImGui::TextColored(kDim, "Renderer unavailable (see status line).");
        return;
    }
    // Toolbar
    const char* views[] = {"Final", "HDR raw", "Albedo", "Normal", "Specular", "Depth", "Sky cube", "Bloom", "Quarter", "Tone mapped"};
    int v = int(renderer_->view);
    ImGui::SetNextItemWidth(170);
    if (ImGui::Combo("##view", &v, views, IM_ARRAYSIZE(views))) renderer_->view = Renderer::View(v);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90);
    ImGui::InputInt("seed", &scene_seed_);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90);
    ImGui::InputInt("count", &scene_count_);
    scene_count_ = std::clamp(scene_count_, 1, 100);
    ImGui::SameLine();
    if (ImGui::Button("Re-roll")) {
        int n = scene_->generate(uint32_t(scene_seed_), scene_count_);
        set_status("Test map: " + std::to_string(n) + " buildings.");
    }
    ImGui::SameLine();
    const Document* env = env_doc_ >= 0 && env_doc_ < int(docs_.size()) ? docs_[env_doc_].get() : nullptr;
    ImGui::TextColored(kDim, "environment: %s", env ? env->title().c_str() : "(none - open a .environment)");

    ImVec2 size = ImGui::GetContentRegionAvail();
    int w = int(size.x), h = int(size.y);
    if (w < 16 || h < 16) return;

    // Cross-fade: second environment + slider.
    ImGui::SetNextItemWidth(200);
    const Document* other = blend_doc_ >= 0 && blend_doc_ < int(docs_.size()) && blend_doc_ != env_doc_ ? docs_[blend_doc_].get() : nullptr;
    if (ImGui::BeginCombo("##blend", other ? other->title().c_str() : "cross-fade: (none)")) {
        if (ImGui::Selectable("(none)", !other)) blend_doc_ = -1;
        for (int i = 0; i < int(docs_.size()); ++i) {
            std::string t = docs_[i]->title();
            if (i == env_doc_ || (t.find(".environment") == std::string::npos && t.find(".lighting") == std::string::npos)) continue;
            if (ImGui::Selectable((t + "##b" + std::to_string(i)).c_str(), i == blend_doc_)) blend_doc_ = i;
        }
        ImGui::EndCombo();
    }
    if (other) {
        ImGui::SameLine();
        ImGui::SetNextItemWidth(180);
        ImGui::SliderFloat("##t", &blend_t_, 0.0f, 1.0f, "blend %.2f");
    }

    EnvironmentParams params = env ? read_environment(env->doc) : EnvironmentParams{};
    if (env && other && blend_t_ > 0) params = blend_environments(params, read_environment(other->doc), blend_t_);
    params.output_gamma = output_gamma_;
    params.output_brightness = output_brightness_;
    // Camera: orbit around the target.
    float cp = std::cos(cam_pitch_), sp = std::sin(cam_pitch_);
    camera_.target = cam_target_;
    camera_.eye = cam_target_ + float3{std::sin(cam_yaw_) * cp, sp, -std::cos(cam_yaw_) * cp} * cam_dist_;
    camera_.eye.y = std::max(camera_.eye.y, 2.0f); // stay above the ground plane when looking up
    try {
        renderer_->render(scene_.get(), camera_, params, w, h, float(ImGui::GetTime() - start_time_));
    } catch (const std::exception& e) {
        set_status(std::string("Render error: ") + e.what(), true);
    }
    if (!renderer_->output()) return;

    ImVec2 pos = ImGui::GetCursorScreenPos();
    ImGui::Image(ImTextureID(renderer_->output()), size);
    ImGui::SetCursorScreenPos(pos);
    ImGui::InvisibleButton("##vp", size, ImGuiButtonFlags_MouseButtonLeft | ImGuiButtonFlags_MouseButtonRight | ImGuiButtonFlags_MouseButtonMiddle);
    ImGuiIO& io = ImGui::GetIO();
    if (ImGui::IsItemActive()) {
        if (ImGui::IsMouseDown(ImGuiMouseButton_Left) || ImGui::IsMouseDown(ImGuiMouseButton_Right)) {
            cam_yaw_ += io.MouseDelta.x * 0.006f;
            cam_pitch_ = std::clamp(cam_pitch_ + io.MouseDelta.y * 0.006f, -1.2f, 1.5f);
        }
        if (ImGui::IsMouseDown(ImGuiMouseButton_Middle)) {
            float3 right{std::cos(cam_yaw_), 0, std::sin(cam_yaw_)};
            float3 fwd{-std::sin(cam_yaw_), 0, std::cos(cam_yaw_)};
            float k = cam_dist_ * 0.0015f;
            cam_target_ = cam_target_ - right * (io.MouseDelta.x * k) + fwd * (io.MouseDelta.y * k);
        }
    }
    if (ImGui::IsItemHovered() && io.MouseWheel != 0) cam_dist_ = std::clamp(cam_dist_ * std::pow(0.88f, io.MouseWheel), 5.0f, 3000.0f);
    if (!renderer_->errors().empty() && ImGui::IsItemHovered()) {
        std::string all;
        for (auto& e : renderer_->errors()) all += e + "\n";
        ImGui::SetTooltip("%s", all.c_str());
    }
}

void App::mount() {
    try {
        auto v = std::make_unique<Vfs>(Vfs::default_config());
        v->mount();
        vfs_ = std::move(v);
        std::string msg = "Mounted " + std::to_string(vfs_->packs().size()) + " packs, " + std::to_string(vfs_->files().size()) + " files.";
        for (auto& m : vfs_->missing_mods()) msg += " Missing mod: " + m + ".";
        set_status(msg, !vfs_->missing_mods().empty());
    } catch (const std::exception& e) {
        set_status(std::string("Mount failed: ") + e.what(), true);
    }
    listing_dirty_ = true;
}

void App::set_status(std::string s, bool error) {
    status_ = std::move(s);
    status_error_ = error;
}

void App::load_settings() {
    std::ifstream f(settings_path_);
    std::string l;
    while (std::getline(f, l))
        if (l.rfind("output_dir=", 0) == 0) output_dir_ = fs::path(widen(l.substr(11)));
}

void App::store_settings() const {
    fs::create_directories(settings_path_.parent_path());
    std::ofstream f(settings_path_);
    f << "output_dir=" << narrow(output_dir_.wstring()) << "\n";
}

void App::open_vfs(const std::string& path) {
    for (size_t i = 0; i < docs_.size(); ++i)
        if (docs_[i]->vfs_path == path) { focus_request_ = int(i); return; }
    try {
        auto disk = vfs_->fetch(path);
        if (!disk) throw std::runtime_error("not found in packs");
        auto d = std::make_unique<Document>();
        d->vfs_path = path;
        d->source_disk = *disk;
        d->source_label = vfs_->packs()[*vfs_->owner(path)].name;
        d->doc = XmlDoc::parse(read_file(disk->string()));
        docs_.push_back(std::move(d));
        focus_request_ = int(docs_.size()) - 1;
        set_status("Opened " + path);
    } catch (const std::exception& e) {
        set_status("Cannot open " + path + ": " + e.what(), true);
    }
}

void App::open_disk(const fs::path& path) {
    try {
        auto d = std::make_unique<Document>();
        d->source_disk = path;
        d->source_label = "loose";
        d->doc = XmlDoc::parse(read_file(path.string()));
        docs_.push_back(std::move(d));
        focus_request_ = int(docs_.size()) - 1;
        set_status("Opened " + path.string());
    } catch (const std::exception& e) {
        set_status("Cannot open " + path.string() + ": " + e.what(), true);
    }
}

fs::path App::default_save_path(const Document& d) const {
    if (d.vfs_path.empty()) return d.source_disk;
    return output_dir_ / widen(d.vfs_path);
}

bool App::save(Document& d, const fs::path& to) {
    try {
        fs::create_directories(to.parent_path());
        if (!write_file(to.string(), d.doc.save())) throw std::runtime_error("write failed");
        // The saved file becomes the new baseline.
        d.doc = XmlDoc::parse(read_file(to.string()));
        d.undo.clear();
        d.redo.clear();
        if (d.vfs_path.empty()) d.source_disk = to;
        set_status("Saved " + to.string());
        return true;
    } catch (const std::exception& e) {
        set_status("Save failed: " + std::string(e.what()), true);
        return false;
    }
}

bool App::confirm_close() {
    int dirty = 0;
    for (auto& d : docs_) dirty += d->doc.modified();
    if (!dirty) return true;
    std::wstring msg = std::to_wstring(dirty) + L" file(s) have unsaved changes. Close anyway?";
    return MessageBoxW(nullptr, msg.c_str(), L"Attila Weather Editor", MB_YESNO | MB_ICONWARNING) == IDYES;
}

void App::frame() {
    const ImGuiViewport* vp = ImGui::GetMainViewport();
    ImGui::SetNextWindowPos(vp->WorkPos);
    ImGui::SetNextWindowSize(vp->WorkSize);
    ImGui::Begin("##root", nullptr, ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_MenuBar | ImGuiWindowFlags_NoBringToFrontOnFocus);
    draw_menu();

    float status_h = ImGui::GetFrameHeightWithSpacing();
    ImGui::BeginChild("browser", ImVec2(ImGui::GetContentRegionAvail().x * 0.24f, -status_h), ImGuiChildFlags_Borders | ImGuiChildFlags_ResizeX);
    draw_browser();
    ImGui::EndChild();
    ImGui::SameLine();
    ImGui::BeginChild("viewport", ImVec2(ImGui::GetContentRegionAvail().x * 0.6f, -status_h), ImGuiChildFlags_Borders | ImGuiChildFlags_ResizeX);
    draw_viewport();
    ImGui::EndChild();
    ImGui::SameLine();
    ImGui::BeginChild("docs", ImVec2(0, -status_h), ImGuiChildFlags_Borders);
    if (docs_.empty()) {
        ImGui::TextColored(kDim, "Open a file from the list on the left, or File > Open loose file.");
        ImGui::TextColored(kDim, "The 3D preview arrives in Phase 1; this build edits and saves the XML.");
    } else if (ImGui::BeginTabBar("tabs", ImGuiTabBarFlags_Reorderable | ImGuiTabBarFlags_AutoSelectNewTabs | ImGuiTabBarFlags_FittingPolicyScroll)) {
        for (int i = 0; i < int(docs_.size()); ++i) {
            Document& d = *docs_[i];
            bool open = true;
            ImGuiTabItemFlags fl = (focus_request_ == i ? ImGuiTabItemFlags_SetSelected : 0) | (d.doc.modified() ? ImGuiTabItemFlags_UnsavedDocument : 0);
            ImGui::PushID(i);
            std::string tab = (d.vfs_path.empty() ? d.source_disk.filename().string() : d.vfs_path.substr(d.vfs_path.rfind('/') + 1)) + "###tab";
            if (ImGui::BeginTabItem(tab.c_str(), &open, fl)) {
                active_ = i;
                std::string lower = d.title();
                if (lower.find(".environment") != std::string::npos || lower.find(".lighting") != std::string::npos) env_doc_ = i;
                draw_document(d);
                ImGui::EndTabItem();
            }
            ImGui::PopID();
            if (!open) {
                bool close = !d.doc.modified() ||
                             MessageBoxW(nullptr, L"Discard unsaved changes?", L"Attila Weather Editor", MB_YESNO | MB_ICONWARNING) == IDYES;
                if (close) {
                    if (blend_doc_ == i) blend_doc_ = -1;
                    else if (blend_doc_ > i) --blend_doc_;
                    if (env_doc_ == i) env_doc_ = -1;
                    else if (env_doc_ > i) --env_doc_;
                    docs_.erase(docs_.begin() + i);
                    --i;
                    active_ = -1;
                }
            }
        }
        focus_request_ = -1;
        ImGui::EndTabBar();
    }
    ImGui::EndChild();
    if (status_error_) ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "%s", status_.c_str());
    else ImGui::TextUnformatted(status_.c_str());
    ImGui::End();

    // Shortcuts
    ImGuiIO& io = ImGui::GetIO();
    if (active_ >= 0 && active_ < int(docs_.size()) && io.KeyCtrl && !ImGui::IsAnyItemActive()) {
        Document& d = *docs_[active_];
        if (ImGui::IsKeyPressed(ImGuiKey_Z) && !d.undo.empty()) {
            auto c = d.undo.back();
            d.undo.pop_back();
            d.doc.set_attr_raw(c.elem, c.attr, c.before);
            d.redo.push_back(c);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Y) && !d.redo.empty()) {
            auto c = d.redo.back();
            d.redo.pop_back();
            d.doc.set_attr_raw(c.elem, c.attr, c.after);
            d.undo.push_back(c);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_S)) save(d, default_save_path(d));
    }
}

void App::draw_menu() {
    if (!ImGui::BeginMenuBar()) return;
    if (ImGui::BeginMenu("File")) {
        if (ImGui::MenuItem("Open loose file...")) {
            if (auto p = file_dialog(false, output_dir_ / "x")) open_disk(*p);
        }
        Document* d = active_ >= 0 && active_ < int(docs_.size()) ? docs_[active_].get() : nullptr;
        if (ImGui::MenuItem("Save", "Ctrl+S", false, d != nullptr)) save(*d, default_save_path(*d));
        if (ImGui::MenuItem("Save as...", nullptr, false, d != nullptr)) {
            if (auto p = file_dialog(true, default_save_path(*d))) save(*d, *p);
        }
        ImGui::Separator();
        if (ImGui::MenuItem("Remount packs")) {
            mount();
            init_renderer();
        }
        ImGui::EndMenu();
    }
    if (ImGui::BeginMenu("Settings")) {
        static char buf[1024];
        std::string cur = narrow(output_dir_.wstring());
        strncpy(buf, cur.c_str(), sizeof buf - 1);
        ImGui::TextUnformatted("Output folder (files are saved under their in-pack path):");
        ImGui::SetNextItemWidth(600);
        if (ImGui::InputText("##out", buf, sizeof buf, ImGuiInputTextFlags_EnterReturnsTrue)) {
            output_dir_ = fs::path(widen(buf));
            store_settings();
        }
        ImGui::EndMenu();
    }
    ImGui::EndMenuBar();
}

void App::draw_browser() {
    if (!vfs_) {
        ImGui::TextWrapped("Packs are not mounted. Check the status line, then File > Remount packs.");
        return;
    }
    ImGui::SetNextItemWidth(-1);
    if (ImGui::BeginCombo("##cat", kCategories[category_].label)) {
        for (int i = 0; i < int(std::size(kCategories)); ++i)
            if (ImGui::Selectable(kCategories[i].label, i == category_)) {
                category_ = i;
                listing_dirty_ = true;
            }
        ImGui::EndCombo();
    }
    ImGui::SetNextItemWidth(-1);
    if (ImGui::InputTextWithHint("##filter", "filter", filter_, sizeof filter_)) listing_dirty_ = true;
    if (listing_dirty_) {
        listing_.clear();
        for (auto& p : vfs_->list(kCategories[category_].prefix, kCategories[category_].ext))
            if (contains_ci(p, filter_)) listing_.push_back(p);
        listing_dirty_ = false;
    }
    ImGui::TextColored(kDim, "%zu files", listing_.size());
    ImGui::BeginChild("list");
    ImGuiListClipper clip;
    clip.Begin(int(listing_.size()));
    while (clip.Step())
        for (int i = clip.DisplayStart; i < clip.DisplayEnd; ++i) {
            const std::string& p = listing_[i];
            int owner = *vfs_->owner(p);
            bool mod = vfs_->packs()[owner].is_mod;
            if (mod) ImGui::PushStyleColor(ImGuiCol_Text, kMod);
            if (ImGui::Selectable(p.c_str(), false, ImGuiSelectableFlags_AllowDoubleClick) && ImGui::IsMouseDoubleClicked(0)) open_vfs(p);
            if (mod) ImGui::PopStyleColor();
            if (ImGui::IsItemHovered()) {
                std::string tip;
                for (int o : vfs_->owners(p)) tip += (tip.empty() ? "" : "\n") + vfs_->packs()[o].name + (tip.empty() ? "  (wins)" : "");
                ImGui::SetTooltip("%s", tip.c_str());
            }
        }
    ImGui::EndChild();
}

void App::draw_document(Document& d) {
    ImGui::TextColored(kDim, "%s", d.vfs_path.empty() ? d.source_disk.string().c_str() : d.vfs_path.c_str());
    ImGui::SameLine();
    ImGui::TextColored(kDim, "  [%s]", d.source_label.c_str());
    if (ImGui::Button("Save")) save(d, default_save_path(d));
    ImGui::SameLine();
    if (ImGui::Button("Save as...")) {
        if (auto p = file_dialog(true, default_save_path(d))) save(d, *p);
    }
    ImGui::SameLine();
    ImGui::BeginDisabled(!d.doc.modified());
    if (ImGui::Button("Revert all")) {
        d.doc.revert_all();
        d.undo.clear();
        d.redo.clear();
    }
    ImGui::EndDisabled();
    ImGui::SameLine();
    ImGui::SetNextItemWidth(220);
    ImGui::InputTextWithHint("##af", "filter attributes", attr_filter_, sizeof attr_filter_);
    ImGui::SameLine();
    ImGui::TextColored(kDim, "-> %s", narrow(default_save_path(d).wstring()).c_str());

    if (ImGui::BeginTabBar("views")) {
        if (ImGui::BeginTabItem("Properties")) {
            ImGui::BeginChild("tree");
            for (int r : d.doc.roots()) draw_element(d, r);
            ImGui::EndChild();
            ImGui::EndTabItem();
        }
        if (ImGui::BeginTabItem("Changes")) {
            draw_changes(d);
            ImGui::EndTabItem();
        }
        ImGui::EndTabBar();
    }
}

void App::draw_element(Document& d, int e) {
    const XmlElement& el = d.doc.elements()[e];
    ImGui::PushID(e);
    bool any_mod = false;
    for (size_t a = 0; a < el.attrs.size() && !any_mod; ++a) any_mod = d.doc.attr_modified(e, a);
    if (any_mod) ImGui::PushStyleColor(ImGuiCol_Text, kModified);
    bool open = ImGui::TreeNodeEx("##el", el.parent < 0 ? ImGuiTreeNodeFlags_DefaultOpen : 0, "%s", element_label(d.doc, e).c_str());
    if (any_mod) ImGui::PopStyleColor();
    if (open) {
        bool has_attrs = false;
        for (size_t a = 0; a < el.attrs.size(); ++a)
            if (el.attrs[a].name != "name" && contains_ci(el.attrs[a].name, attr_filter_)) has_attrs = true;
        if (has_attrs && ImGui::BeginTable("attrs", 2, ImGuiTableFlags_SizingStretchProp | ImGuiTableFlags_RowBg)) {
            ImGui::TableSetupColumn("name", ImGuiTableColumnFlags_WidthFixed, 280);
            ImGui::TableSetupColumn("value", ImGuiTableColumnFlags_WidthStretch);
            for (size_t a = 0; a < el.attrs.size(); ++a)
                if (el.attrs[a].name != "name" && contains_ci(el.attrs[a].name, attr_filter_)) draw_attribute(d, e, a);
            ImGui::EndTable();
        }
        for (int c : el.children) draw_element(d, c);
        ImGui::TreePop();
    }
    ImGui::PopID();
}

void App::draw_attribute(Document& d, int e, size_t a) {
    const XmlAttr& at = d.doc.elements()[e].attrs[a];
    std::string raw(d.doc.attr_raw(e, a));
    EnvValue v = parse_value(XmlDoc::decode_entities(raw));
    bool modified = d.doc.attr_modified(e, a);

    ImGui::TableNextRow();
    ImGui::TableNextColumn();
    if (modified) ImGui::PushStyleColor(ImGuiCol_Text, kModified);
    ImGui::AlignTextToFramePadding();
    ImGui::TextUnformatted(at.name.c_str());
    if (modified) ImGui::PopStyleColor();
    if (modified && ImGui::IsItemHovered()) ImGui::SetTooltip("original: %.*s", int(d.doc.original_raw(e, a).size()), d.doc.original_raw(e, a).data());
    ImGui::TableNextColumn();
    ImGui::PushID(int(a));
    ImGui::SetNextItemWidth(-FLT_MIN);

    bool changed = false;
    switch (v.kind) {
    case ValueKind::Bool: changed = ImGui::Checkbox("##v", &v.b); break;
    case ValueKind::Int: changed = ImGui::DragScalar("##v", ImGuiDataType_S64, &v.i, 0.2f); break;
    case ValueKind::Float: {
        float speed = std::max(std::fabs(v.f[0]) * 0.005f, 0.001f);
        changed = ImGui::DragFloat("##v", &v.f[0], speed, 0, 0, "%.6g");
        break;
    }
    case ValueKind::Rgb:
        changed = ImGui::ColorEdit3("##v", v.f.data(), ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR);
        break;
    case ValueKind::Argb: // "argb(...)" lists r, g, b, a (verified against the game's fog colour)
        changed = ImGui::ColorEdit4("##v", v.f.data(), ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR | ImGuiColorEditFlags_AlphaBar);
        break;
    case ValueKind::Variance: changed = ImGui::DragFloat2("##v", v.f.data(), 0.01f, 0, 0, "%.6g"); break;
    case ValueKind::Euler: changed = ImGui::DragFloat3("##v", v.f.data(), 0.01f, 0, 0, "%.6g"); break;
    case ValueKind::String: {
        char buf[512];
        strncpy(buf, v.s.c_str(), sizeof buf - 1);
        buf[sizeof buf - 1] = 0;
        if (ImGui::InputText("##v", buf, sizeof buf, ImGuiInputTextFlags_EnterReturnsTrue)) {
            v.s = buf;
            changed = true;
        }
        break;
    }
    }
    if (ImGui::IsItemActivated()) pending_before_ = raw;
    if (changed) {
        std::string out = format_value(v);
        d.doc.set_attr_raw(e, a, XmlDoc::encode_entities(out, at.quote));
    }
    if (ImGui::IsItemDeactivatedAfterEdit() || (changed && v.kind == ValueKind::String)) {
        std::string before = v.kind == ValueKind::String ? raw : pending_before_;
        std::string after(d.doc.attr_raw(e, a));
        if (before != after) {
            d.undo.push_back({e, a, before, after});
            d.redo.clear();
        }
    }
    if (modified && ImGui::BeginPopupContextItem("ctx")) {
        if (ImGui::MenuItem("Revert to original")) {
            std::string before(d.doc.attr_raw(e, a));
            d.doc.set_attr_raw(e, a, std::string(d.doc.original_raw(e, a)));
            d.undo.push_back({e, a, before, std::string(d.doc.original_raw(e, a))});
        }
        ImGui::EndPopup();
    }
    ImGui::PopID();
}

void App::draw_changes(Document& d) {
    if (!d.doc.modified()) {
        ImGui::TextColored(kDim, "No changes.");
        return;
    }
    if (ImGui::BeginTable("chg", 3, ImGuiTableFlags_RowBg | ImGuiTableFlags_Resizable | ImGuiTableFlags_ScrollY)) {
        ImGui::TableSetupColumn("attribute");
        ImGui::TableSetupColumn("original");
        ImGui::TableSetupColumn("new");
        ImGui::TableHeadersRow();
        const auto& els = d.doc.elements();
        for (int e = 0; e < int(els.size()); ++e)
            for (size_t a = 0; a < els[e].attrs.size(); ++a)
                if (d.doc.attr_modified(e, a)) {
                    ImGui::TableNextRow();
                    ImGui::TableNextColumn();
                    ImGui::Text("%s @%s", d.doc.path_of(e).c_str(), els[e].attrs[a].name.c_str());
                    ImGui::TableNextColumn();
                    auto o = d.doc.original_raw(e, a);
                    ImGui::TextUnformatted(o.data(), o.data() + o.size());
                    ImGui::TableNextColumn();
                    auto n = d.doc.attr_raw(e, a);
                    ImGui::TextUnformatted(n.data(), n.data() + n.size());
                }
        ImGui::EndTable();
    }
}

} // namespace we
