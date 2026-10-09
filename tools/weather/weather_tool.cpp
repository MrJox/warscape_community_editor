#include "weather/weather_tool.h"

#include "editor/editor.h"
#include "formats/env_value.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"
#include "scene/environment.h"
#include "tool_factories.h"
#include "weather/sample_map_document.h"

#include <imgui.h>
#include <imgui_internal.h>

#include <algorithm>
#include <cmath>
#include <cstring>

namespace fs = std::filesystem;

namespace wce {

namespace {

// The environment the sandbox lit the sample map with before an environment document is open: what the gates use.
constexpr const char* kDefaultEnvironment = "weather/charlemagne/med_sp_land_day_dry.environment";

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
    auto it = std::search(hay.begin(), hay.end(), n.begin(), n.end(), [](char a, char b) { return std::tolower(uint8_t(a)) == std::tolower(uint8_t(b)); });
    return it != hay.end();
}

} // namespace

XmlFileDocument* WeatherTool::find_xml(Editor& editor, DocumentId id) {
    return id ? dynamic_cast<XmlFileDocument*>(editor.documents().find(id)) : nullptr;
}

XmlFileDocument* WeatherTool::active_xml(Editor& editor) { return dynamic_cast<XmlFileDocument*>(editor.documents().active()); }

XmlFileDocument* WeatherTool::environment_document(Editor& editor) { return find_xml(editor, env_doc_); }

// ---- documents

std::unique_ptr<Document> WeatherTool::open(Editor& editor, const std::string& path, bool from_packs) {
    XmlFileDocument::Source src;
    std::vector<uint8_t> bytes;
    if (from_packs) {
        Vfs* vfs = editor.vfs();
        if (!vfs) throw std::runtime_error("the game packs are not mounted");
        const std::string p = Vfs::normalize(path);
        auto disk = vfs->fetch(p);
        if (!disk) throw std::runtime_error("not found in the packs");
        src.vfs_path = p;
        src.disk = *disk;
        src.label = vfs->packs()[*vfs->owner(p)].name;
        bytes = read_file(narrow(disk->wstring()));
    } else {
        src.disk = fs::path(widen(path));
        src.label = "loose";
        bytes = read_file(path);
    }
    return std::make_unique<XmlFileDocument>(std::move(src), bytes, editor.output_dir());
}

// ---- lighting the scene

void WeatherTool::activated(Editor& editor) {
    if (!editor.vfs()) return;
    const GraphicsOptions go = read_graphics_options();
    output_gamma_ = go.gamma;
    output_brightness_ = go.brightness;
    if (!default_env_loaded_) {
        try {
            default_env_ = read_environment(XmlDoc::parse(editor.vfs()->read(kDefaultEnvironment)));
            default_env_loaded_ = true;
        } catch (const std::exception& e) {
            Log::warn(std::string("default environment: ") + e.what());
        }
    }
    // Precipitation reads open documents first, so edits to vfx/*.xml and weather_definitions.xml show live.
    if (reader_for_ != editor.renderer() && editor.renderer() && editor.renderer()->particles()) {
        Editor* ed = &editor;
        editor.renderer()->particles()->set_reader([ed](const std::string& path) {
            const std::string p = Vfs::normalize(path);
            for (DocumentId id : ed->documents().order())
                if (auto* x = dynamic_cast<XmlFileDocument*>(ed->documents().find(id)); x && x->source().vfs_path == p) return x->xml().save();
            return ed->vfs()->read(p);
        });
        reader_for_ = editor.renderer();
    }
    ensure_sample_map(editor);
}

// The sample map is the stand-in scene of this tool: made when nothing else brings a ground (an open tile does), hidden and shown again by the document manager.
void WeatherTool::ensure_sample_map(Editor& editor) {
    if (!editor.vfs() || editor.documents().shows_scene()) return;
    for (DocumentId id : editor.documents().order())
        if (dynamic_cast<SampleMapDocument*>(editor.documents().find(id))) return;
    reroll(editor);
}

void WeatherTool::reroll(Editor& editor) {
    if (!editor.vfs()) return;
    for (DocumentId id : std::vector<DocumentId>(editor.documents().order()))
        if (dynamic_cast<SampleMapDocument*>(editor.documents().find(id))) {
            editor.selection().remove_document(id);
            editor.documents().close(id);
        }
    auto doc = std::make_unique<SampleMapDocument>(*editor.vfs(), uint32_t(scene_seed_), scene_count_);
    const float3 ext = doc->info().bounds_max - doc->info().bounds_min;
    const int n = doc->info().buildings;
    const DocumentId previous = editor.documents().active_id();
    editor.documents().add(std::move(doc), "weather");
    if (previous) editor.documents().set_active(previous);   // the environment document being edited stays the active one
    if (!editor.documents().shows_scene()) {                  // with a tile open the camera stays on the tile
        editor.viewport().use_orbit();
        static_cast<OrbitCamera&>(editor.viewport().controller()).set_orbit(0.6f, 0.35f, std::max(60.0f, std::max(ext.x, ext.z) * 0.9f), {0, 5, 0});
    }
    editor.status("Test map: " + std::to_string(n) + " buildings.");
}

void WeatherTool::update(Editor& editor) {
    DocumentManager& docs = editor.documents();
    if (tile_was_open_ && !docs.shows_scene()) ensure_sample_map(editor);   // the last tile closed: the stand-in scene is back
    tile_was_open_ = docs.shows_scene();
    if (auto* x = active_xml(editor); x && x->is_environment()) env_doc_ = x->id();
    XmlFileDocument* env = find_xml(editor, env_doc_);
    if (!env) env_doc_ = 0;
    XmlFileDocument* other = blend_doc_ && blend_doc_ != env_doc_ ? find_xml(editor, blend_doc_) : nullptr;
    if (!other && blend_doc_ != 0 && !docs.find(blend_doc_)) blend_doc_ = 0;

    EnvironmentSlot& slot = editor.scene().environment;
    EnvironmentParams active = env ? read_environment(env->xml()) : default_env_;
    active.output_gamma = output_gamma_;
    active.output_brightness = output_brightness_;
    slot.active = active;
    slot.blend.reset();
    slot.blend_t = 0;
    slot.owner = env ? env->id() : 0;
    if (env && other && blend_t_ > 0) {
        EnvironmentParams b = read_environment(other->xml());
        b.output_gamma = output_gamma_;
        b.output_brightness = output_brightness_;
        slot.blend = b;
        slot.blend_t = blend_t_;
    }
    last_params_ = slot.resolved();

    // Edit state of the open documents the particle system reads (vfx/ and weather_definitions.xml).
    uint64_t rev = 0;
    for (DocumentId id : docs.order())
        if (auto* x = dynamic_cast<XmlFileDocument*>(docs.find(id)))
            if (x->source().vfs_path.rfind("vfx/", 0) == 0 || (x->source().vfs_path.rfind("weather/", 0) == 0 && x->kind() != "environment"))
                rev = rev * 1000003u + std::hash<std::string>{}(x->source().vfs_path) + x->history().version() * 7919u + (x->modified() ? 1u : 0u);
    if (editor.renderer()) editor.renderer()->vfx_revision = rev;
}

// ---- layout and panels

void WeatherTool::default_layout(ImGuiID dockspace) {
    ImGuiID center = dockspace;
    const ImGuiID left = ImGui::DockBuilderSplitNode(center, ImGuiDir_Left, 0.22f, nullptr, &center);
    const ImGuiID right = ImGui::DockBuilderSplitNode(center, ImGuiDir_Right, 0.34f, nullptr, &center);
    const ImGuiID bottom = ImGui::DockBuilderSplitNode(center, ImGuiDir_Down, 0.22f, nullptr, &center);
    ImGui::DockBuilderDockWindow("Viewport", center);
    ImGui::DockBuilderDockWindow("Files", left);
    ImGui::DockBuilderDockWindow("Outliner", left);
    ImGui::DockBuilderDockWindow("Properties", right);
    ImGui::DockBuilderDockWindow("Inspector", right);
    ImGui::DockBuilderDockWindow("Weather scene", bottom);
    ImGui::DockBuilderDockWindow("Assets", bottom);
    ImGui::DockBuilderDockWindow("Log", bottom);
    ImGui::DockBuilderDockWindow("History", bottom);
}

void WeatherTool::draw_panels(Editor& editor) {
    draw_files(editor);
    draw_properties(editor);
    draw_scene_bar(editor);
    XmlFileDocument* env = environment_document(editor);
    picker_.draw(editor, editor.output_dir(), env ? env->source().vfs_path : std::string());
}

void WeatherTool::draw_files(Editor& editor) {
    if (!ImGui::Begin("Files")) { ImGui::End(); return; }
    Vfs* vfs = editor.vfs();
    if (!vfs) {
        ImGui::TextWrapped("The game packs are not mounted. Check the status line.");
        ImGui::End();
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
        for (auto& p : vfs->list(kCategories[category_].prefix, kCategories[category_].ext))
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
            const int owner = *vfs->owner(p);
            const bool mod = vfs->packs()[owner].is_mod;
            if (mod) ImGui::PushStyleColor(ImGuiCol_Text, kMod);
            if (ImGui::Selectable(p.c_str(), false, ImGuiSelectableFlags_AllowDoubleClick) && ImGui::IsMouseDoubleClicked(0)) {
                std::string msg;
                if (!editor.open_path(p, true, &msg)) editor.status(msg, true);
                else editor.status(msg);
            }
            if (mod) ImGui::PopStyleColor();
            if (ImGui::IsItemHovered()) {
                std::string tip;
                for (int o : vfs->owners(p)) tip += (tip.empty() ? "" : "\n") + vfs->packs()[o].name + (tip.empty() ? "  (wins)" : "");
                ImGui::SetTooltip("%s", tip.c_str());
            }
        }
    ImGui::EndChild();
    ImGui::End();
}

void WeatherTool::draw_scene_bar(Editor& editor) {
    if (!ImGui::Begin("Weather scene")) { ImGui::End(); return; }
    Renderer* r = editor.renderer();
    if (!r) {
        ImGui::TextDisabled("Renderer unavailable (see the status line).");
        ImGui::End();
        return;
    }
    const char* views[] = {"Final", "HDR raw", "Albedo", "Normal", "Specular", "Depth", "Sky cube", "Bloom", "Quarter", "Tone mapped", "AO"};
    int v = int(r->view);
    ImGui::SetNextItemWidth(170);
    if (ImGui::Combo("view", &v, views, IM_ARRAYSIZE(views))) r->view = Renderer::View(v);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90);
    ImGui::InputInt("seed", &scene_seed_);
    ImGui::SameLine();
    ImGui::SetNextItemWidth(90);
    ImGui::InputInt("count", &scene_count_);
    scene_count_ = std::clamp(scene_count_, 1, 100);
    ImGui::SameLine();
    if (ImGui::Button("Re-roll")) reroll(editor);
    XmlFileDocument* env = environment_document(editor);
    ImGui::TextColored(kDim, "environment: %s", env ? env->title().c_str() : "(none open: the default dry day)");

    // Cross-fade: second environment + slider.
    ImGui::SetNextItemWidth(260);
    XmlFileDocument* other = blend_doc_ && blend_doc_ != env_doc_ ? find_xml(editor, blend_doc_) : nullptr;
    if (ImGui::BeginCombo("cross-fade with", other ? other->title().c_str() : "(none)")) {
        if (ImGui::Selectable("(none)", !other)) blend_doc_ = 0;
        for (DocumentId id : editor.documents().order()) {
            XmlFileDocument* x = find_xml(editor, id);
            if (!x || id == env_doc_ || !x->is_environment()) continue;
            if (ImGui::Selectable((x->title() + "##b" + std::to_string(id)).c_str(), id == blend_doc_)) blend_doc_ = id;
        }
        ImGui::EndCombo();
    }
    if (other) {
        ImGui::SameLine();
        ImGui::SetNextItemWidth(180);
        ImGui::SliderFloat("##t", &blend_t_, 0.0f, 1.0f, "blend %.2f");
    }
    if (!r->errors().empty()) {
        std::string all;
        for (auto& e : r->errors()) all += e + "\n";
        ImGui::TextColored(ImVec4(1, 0.45f, 0.4f, 1), "render errors (hover)");
        if (ImGui::IsItemHovered()) ImGui::SetTooltip("%s", all.c_str());
    }
    ImGui::End();
}

// ---- properties tree (the legacy editor's central view)

void WeatherTool::draw_properties(Editor& editor) {
    if (!ImGui::Begin("Properties")) { ImGui::End(); return; }
    if (XmlFileDocument* d = active_xml(editor)) draw_document(editor, *d);
    else ImGui::TextColored(kDim, "Open a file from the Files list, or File > Open file.");
    ImGui::End();
}

void WeatherTool::draw_document(Editor& editor, XmlFileDocument& d) {
    const auto& src = d.source();
    ImGui::TextColored(kDim, "%s", src.vfs_path.empty() ? narrow(src.disk.wstring()).c_str() : src.vfs_path.c_str());
    ImGui::SameLine();
    ImGui::TextColored(kDim, "  [%s]", src.label.c_str());
    if (ImGui::Button("Save")) editor.commands().run("save");
    ImGui::SameLine();
    ImGui::BeginDisabled(!d.modified());
    if (ImGui::Button("Revert all")) d.revert_all();
    ImGui::EndDisabled();
    ImGui::SameLine();
    ImGui::SetNextItemWidth(220);
    ImGui::InputTextWithHint("##af", "filter attributes", attr_filter_, sizeof attr_filter_);
    ImGui::SameLine();
    ImGui::TextColored(kDim, "-> %s", narrow(d.default_save_path().wstring()).c_str());

    if (ImGui::BeginTabBar("views")) {
        if (ImGui::BeginTabItem("Properties")) {
            ImGui::BeginChild("tree");
            for (int r : d.xml().roots()) draw_element(editor, d, r);
            ImGui::EndChild();
            apply_pending(editor, d);
            ImGui::EndTabItem();
        }
        if (ImGui::BeginTabItem("Changes")) {
            draw_changes(d);
            ImGui::EndTabItem();
        }
        ImGui::EndTabBar();
    }
}

void WeatherTool::draw_element(Editor& editor, XmlFileDocument& d, int e) {
    const XmlDoc& xml = d.xml();
    const XmlElement& el = xml.elements()[e];
    ImGui::PushID(e);
    bool any_mod = false;
    for (size_t a = 0; a < el.attrs.size() && !any_mod; ++a) any_mod = xml.attr_modified(e, a);
    if (any_mod) ImGui::PushStyleColor(ImGuiCol_Text, kModified);
    ImGuiTreeNodeFlags flags = el.parent < 0 ? ImGuiTreeNodeFlags_DefaultOpen : 0;
    if (editor.selection().contains({d.id(), uint64_t(e)})) flags |= ImGuiTreeNodeFlags_Selected;
    const bool open = ImGui::TreeNodeEx("##el", flags, "%s", element_label(xml, e).c_str());
    if (any_mod) ImGui::PopStyleColor();
    if (ImGui::IsItemClicked() && !ImGui::IsItemToggledOpen()) editor.selection().set({d.id(), uint64_t(e)});   // the Inspector shows it too
    if (e == select_after_) {
        ImGui::SetScrollHereY();
        select_after_ = -1;
    }
    if (ImGui::BeginPopupContextItem("elctx")) {
        using Op = XmlFileDocument::Op;
        ImGui::TextDisabled("%s", el.name.c_str());
        ImGui::Separator();
        if (ImGui::MenuItem("Duplicate")) pending_ = {true, Op::Duplicate, e};
        if (ImGui::MenuItem("Delete")) pending_ = {true, Op::Remove, e};
        if (ImGui::MenuItem("Move up")) pending_ = {true, Op::MoveUp, e};
        if (ImGui::MenuItem("Move down")) pending_ = {true, Op::MoveDown, e};
        if (ImGui::BeginMenu("Add child element")) {
            ImGui::SetNextItemWidth(220);
            const bool go = ImGui::InputTextWithHint("##cn", "element name, Enter to add", new_name_, sizeof new_name_, ImGuiInputTextFlags_EnterReturnsTrue);
            if (go && *new_name_) {
                pending_ = {true, Op::AddChild, e, 0, new_name_};
                *new_name_ = 0;
                ImGui::CloseCurrentPopup();
            }
            ImGui::EndMenu();
        }
        if (ImGui::BeginMenu("Add attribute")) {
            ImGui::SetNextItemWidth(220);
            ImGui::InputTextWithHint("##an", "attribute name", new_name_, sizeof new_name_);
            ImGui::SetNextItemWidth(220);
            const bool go = ImGui::InputTextWithHint("##av", "value, Enter to add", new_value_, sizeof new_value_, ImGuiInputTextFlags_EnterReturnsTrue);
            if (go && *new_name_) {
                pending_ = {true, Op::AddAttr, e, 0, new_name_, new_value_};
                *new_name_ = *new_value_ = 0;
                ImGui::CloseCurrentPopup();
            }
            ImGui::EndMenu();
        }
        if (ImGui::BeginMenu("Rename (name attribute)")) {
            if (ImGui::IsWindowAppearing()) {
                auto cur = xml.get(e, "name");
                std::snprintf(new_value_, sizeof new_value_, "%s", cur ? cur->c_str() : "");
            }
            ImGui::SetNextItemWidth(220);
            if (ImGui::InputText("##rn", new_value_, sizeof new_value_, ImGuiInputTextFlags_EnterReturnsTrue)) {
                size_t k;
                if (xml.find_attr(e, "name", &k)) d.set_attr(e, k, XmlDoc::encode_entities(new_value_, xml.elements()[e].attrs[k].quote));
                else pending_ = {true, Op::AddAttr, e, 0, "name", new_value_};
                ImGui::CloseCurrentPopup();
            }
            ImGui::EndMenu();
        }
        ImGui::EndPopup();
    }
    if (open) {
        bool has_attrs = false;
        for (size_t a = 0; a < el.attrs.size(); ++a)
            if (el.attrs[a].name != "name" && contains_ci(el.attrs[a].name, attr_filter_)) has_attrs = true;
        if (has_attrs && ImGui::BeginTable("attrs", 2, ImGuiTableFlags_SizingStretchProp | ImGuiTableFlags_RowBg)) {
            ImGui::TableSetupColumn("name", ImGuiTableColumnFlags_WidthFixed, 280);
            ImGui::TableSetupColumn("value", ImGuiTableColumnFlags_WidthStretch);
            for (size_t a = 0; a < el.attrs.size(); ++a)
                if (el.attrs[a].name != "name" && contains_ci(el.attrs[a].name, attr_filter_)) draw_attribute(editor, d, e, a);
            ImGui::EndTable();
        }
        for (int c : el.children) draw_element(editor, d, c);
        ImGui::TreePop();
    }
    ImGui::PopID();
}

void WeatherTool::draw_attribute(Editor& editor, XmlFileDocument& d, int e, size_t a) {
    (void)editor;
    const XmlDoc& xml = d.xml();
    const XmlAttr& at = xml.elements()[e].attrs[a];
    std::string raw(xml.attr_raw(e, a));
    EnvValue v = parse_value(XmlDoc::decode_entities(raw));
    const bool modified = xml.attr_modified(e, a);

    ImGui::TableNextRow();
    ImGui::TableNextColumn();
    if (modified) ImGui::PushStyleColor(ImGuiCol_Text, kModified);
    ImGui::AlignTextToFramePadding();
    ImGui::TextUnformatted(at.name.c_str());
    if (modified) ImGui::PopStyleColor();
    if (modified && ImGui::IsItemHovered()) ImGui::SetTooltip("original: %.*s", int(xml.original_raw(e, a).size()), xml.original_raw(e, a).data());
    ImGui::TableNextColumn();
    ImGui::PushID(int(a));
    ImGui::SetNextItemWidth(-FLT_MIN);

    bool changed = false;
    switch (v.kind) {
    case ValueKind::Bool: changed = ImGui::Checkbox("##v", &v.b); break;
    case ValueKind::Int: changed = ImGui::DragScalar("##v", ImGuiDataType_S64, &v.i, 0.2f); break;
    case ValueKind::Float: {
        const float speed = std::max(std::fabs(v.f[0]) * 0.005f, 0.001f);
        changed = ImGui::DragFloat("##v", &v.f[0], speed, 0, 0, "%.6g");
        break;
    }
    case ValueKind::Rgb: changed = ImGui::ColorEdit3("##v", v.f.data(), ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR); break;
    case ValueKind::Argb: // "argb(...)" lists r, g, b, a (verified against the game's fog colour)
        changed = ImGui::ColorEdit4("##v", v.f.data(), ImGuiColorEditFlags_Float | ImGuiColorEditFlags_HDR | ImGuiColorEditFlags_AlphaBar);
        break;
    case ValueKind::Variance: changed = ImGui::DragFloat2("##v", v.f.data(), 0.01f, 0, 0, "%.6g"); break;
    case ValueKind::Euler: changed = ImGui::DragFloat3("##v", v.f.data(), 0.01f, 0, 0, "%.6g"); break;
    case ValueKind::String: {
        char buf[512];
        std::snprintf(buf, sizeof buf, "%s", v.s.c_str());
        if (ImGui::InputText("##v", buf, sizeof buf, ImGuiInputTextFlags_EnterReturnsTrue)) {
            v.s = buf;
            changed = true;
        }
        break;
    }
    }
    if (ImGui::IsItemActivated()) ++gesture_;   // a drag or a typing run is one undo entry
    if (changed) d.set_attr(e, a, XmlDoc::encode_entities(format_value(v), at.quote), v.kind == ValueKind::String ? 0 : gesture_);
    if (ImGui::IsItemDeactivated()) d.history().break_merge();
    if (ImGui::BeginPopupContextItem("ctx")) {
        if (ImGui::MenuItem("Revert to original", nullptr, false, modified)) d.set_attr(e, a, std::string(xml.original_raw(e, a)));
        if (ImGui::MenuItem("Remove attribute")) pending_ = {true, XmlFileDocument::Op::RemoveAttr, e, a};
        ImGui::EndPopup();
    }
    ImGui::PopID();
}

void WeatherTool::apply_pending(Editor& editor, XmlFileDocument& d) {
    if (!pending_.active) return;
    const Pending op = pending_;
    pending_ = Pending{};
    std::string err;
    int sel = -1;
    if (!d.structural(op.op, op.elem, op.attr, op.name, op.value, &sel, &err)) {
        editor.status("Edit failed: " + err, true);
        return;
    }
    select_after_ = sel;
    editor.selection().remove_document(d.id());   // element indices changed
    if (sel >= 0) editor.selection().set({d.id(), uint64_t(sel)});
}

void WeatherTool::draw_changes(XmlFileDocument& d) {
    const XmlDoc& xml = d.xml();
    if (!xml.modified()) {
        ImGui::TextColored(kDim, "No changes.");
        return;
    }
    if (xml.structural_edits() > 0)
        ImGui::TextColored(kModified, "%d structural edit(s): elements or attributes added, removed or moved. Attribute changes below are relative to the last of them.",
                           xml.structural_edits());
    if (ImGui::BeginTable("chg", 3, ImGuiTableFlags_RowBg | ImGuiTableFlags_Resizable | ImGuiTableFlags_ScrollY)) {
        ImGui::TableSetupColumn("attribute");
        ImGui::TableSetupColumn("original");
        ImGui::TableSetupColumn("new");
        ImGui::TableHeadersRow();
        const auto& els = xml.elements();
        for (int e = 0; e < int(els.size()); ++e)
            for (size_t a = 0; a < els[e].attrs.size(); ++a)
                if (xml.attr_modified(e, a)) {
                    ImGui::TableNextRow();
                    ImGui::TableNextColumn();
                    ImGui::Text("%s @%s", xml.path_of(e).c_str(), els[e].attrs[a].name.c_str());
                    ImGui::TableNextColumn();
                    auto o = xml.original_raw(e, a);
                    ImGui::TextUnformatted(o.data(), o.data() + o.size());
                    ImGui::TableNextColumn();
                    auto n = xml.attr_raw(e, a);
                    ImGui::TextUnformatted(n.data(), n.data() + n.size());
                }
        ImGui::EndTable();
    }
}

// ---- menus

void WeatherTool::draw_menus(Editor& editor) {
    Renderer* r = editor.renderer();
    if (ImGui::BeginMenu("Settings")) {
        static char buf[1024];
        const std::string cur = narrow(editor.output_dir().wstring());
        std::snprintf(buf, sizeof buf, "%s", cur.c_str());
        ImGui::TextUnformatted("Output folder (files are saved under their in-pack path):");
        ImGui::SetNextItemWidth(600);
        if (ImGui::InputText("##out", buf, sizeof buf, ImGuiInputTextFlags_EnterReturnsTrue)) editor.set_output_dir(fs::path(widen(buf)), true);
        if (r) {
            ImGui::Separator();
            ImGui::TextUnformatted("Game graphics options (read from preferences.script.txt):");
            ImGui::Checkbox("Vignette (gfx_vignette)", &r->vignette);
            ImGui::Checkbox("Depth of field (gfx_depth_of_field)", &r->depth_of_field);
            ImGui::Checkbox("SSAO (gfx_ssao)", &r->ssao);
            ImGui::Checkbox("Distortion (gfx_distortion)", &r->distortion);
            ImGui::Checkbox("Shadows", &r->shadows);
            ImGui::SetNextItemWidth(120);
            ImGui::SliderInt("Shadow quality (gfx_shadow_quality)", &r->shadow_quality, 0, 3);
            ImGui::Checkbox("Lens flare", &r->lens_flare);
            ImGui::DragFloat("Gamma (gfx_gamma_setting)", &output_gamma_, 0.01f, 1.0f, 3.0f);
            ImGui::DragFloat("Brightness (gfx_brightness_setting)", &output_brightness_, 0.01f, 0.5f, 2.0f);
        }
        ImGui::EndMenu();
    }
    if (ImGui::BeginMenu("Selection")) {
        ImGui::MenuItem("Weather picker (which environment file the game chooses)", nullptr, &picker_.open);
        ImGui::EndMenu();
    }
    if (r && r->particles() && ImGui::BeginMenu("Weather")) {
        ParticleSystem& p = *r->particles();
        const WeatherVfx& fx = p.effect();
        const WeatherDefinition& d = fx.definition;
        const WindSim& w = r->wind();
        ImGui::Checkbox("Precipitation", &p.enabled);
        ImGui::SameLine();
        if (ImGui::Button("Restart")) p.restart();
        ImGui::Separator();
        ImGui::Text("Weather definition: %s (%s, severity %d)", d.name.empty() ? "(none)" : d.name.c_str(), d.type.c_str(), d.severity);
        std::string effects = d.vfx1.empty() ? "(none)" : "vfx/" + d.vfx1 + ".xml";
        if (!d.vfx2.empty()) effects += ", vfx/" + d.vfx2 + ".xml";
        ImGui::Text("Effect: %s", effects.c_str());
        ImGui::Text("Wind direction %.1f deg%s", r->wind_direction_degrees(last_params_), last_params_.wind_dir_random ? " (wind_dir_random: random per battle)" : "");
        if (last_params_.wind_dir_random) {
            ImGui::SameLine();
            if (ImGui::Button("New battle direction")) r->wind_dir_seed = r->wind_dir_seed * 1103515245u + 12345u;
        }
        ImGui::Text("Wind %.3f (min %.3g, max %.3g), last gust %.3f for %.2f s, next roll in %.2f s", w.strength, d.wind_size_min, d.wind_size_max, w.gust_size,
                    w.gust_duration, std::max(0.0f, w.cooldown));
        ImGui::Text("VFX wind (%.2f, %.2f, %.2f), g_wind (%.3f, %.3f)", w.vfx_velocity.x, w.vfx_velocity.y, w.vfx_velocity.z, w.smoothed.x, w.smoothed.y);
        ImGui::Separator();
        ImGui::TextUnformatted("Battle (not stored in the weather files):");
        static const char* seasons[] = {"0", "1", "2", "3 (winter in the snow dump)"};
        ImGui::SetNextItemWidth(200);
        ImGui::Combo("Season (g_season_type)", &r->season, seasons, 4);
        ImGui::Checkbox("Snow on the map (g_snow_mask)", &r->snow_cover);
        ImGui::Separator();
        ImGui::Text("Particles: %u / %u", p.alive(), p.capacity());
        for (auto& e : fx.emitters) ImGui::BulletText("%s / %s / %s", e.library.c_str(), e.instance.c_str(), e.name.c_str());
        for (auto& n : fx.notes) ImGui::TextColored(ImVec4(0.9f, 0.8f, 0.4f, 1), "%s", n.c_str());
        for (auto& e : p.errors()) ImGui::TextColored(ImVec4(1, 0.4f, 0.4f, 1), "%s", e.c_str());
        ImGui::EndMenu();
    }
}

std::unique_ptr<Tool> make_weather_tool() { return std::make_unique<WeatherTool>(); }

} // namespace wce
