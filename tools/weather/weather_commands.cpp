// The weather tool's commands: everything the menus and panels do, plus the test commands (assert_*) the scripted checks use, and the old
// weather_editor command-line flags (--view, --blend, --pick, --picker, --vdb-script, --no-flare), which the app forwards as commands.
#include "editor/editor.h"
#include "foundation/log.h"
#include "foundation/process.h"
#include "weather/sample_map_document.h"
#include "weather/weather_tool.h"

#include <algorithm>
#include <cmath>

namespace fs = std::filesystem;

namespace wce {

namespace {

// An element given as an index, or as the name of its first occurrence.
int resolve_element(const XmlDoc& xml, const std::string& token) {
    if (!token.empty() && std::all_of(token.begin(), token.end(), [](char c) { return std::isdigit(static_cast<unsigned char>(c)); })) {
        const int e = std::atoi(token.c_str());
        return e < int(xml.elements().size()) ? e : -1;
    }
    for (int e = 0; e < int(xml.elements().size()); ++e)
        if (xml.elements()[e].name == token) return e;
    return -1;
}

int resolve_attr(const XmlDoc& xml, int elem, const std::string& name) {
    const auto& attrs = xml.elements()[elem].attrs;
    for (size_t a = 0; a < attrs.size(); ++a)
        if (attrs[a].name == name) return int(a);
    return -1;
}

} // namespace

// What the legacy weather editor wrote next to every shot (<shot>.png.txt): renderer, wind and particle state, for comparing runs and with the game's dumps.
std::string weather_diagnostics(Editor& editor, WeatherTool& tool) {
    std::string d = "status: " + editor.status_text() + "\n";
    d += "viewport sizes:" + editor.viewport().size_trace() + " x" + std::to_string(editor.viewport().frames_rendered) + "\n";
    d += "scene objects: " + std::to_string(editor.scene().entities().size()) + "\n";
    Renderer* r = editor.renderer();
    if (!r) return d;
    d += "draws: " + std::to_string(r->draw_count()) + ", meshes: " + std::to_string(r->mesh_count()) + "\n";
    d += "cloud sprites: " + std::to_string(r->cloud_sprites()) + "\n";
    d += "sun visibility (lens flare): " + std::to_string(r->sun_visibility()) + "\n";
    if (r->particles()) {
        const ParticleSystem& p = *r->particles();
        d += "weather vfx: '" + p.effect().definition.name + "' -> " + p.effect().definition.vfx1 + ", " + std::to_string(p.effect().emitters.size()) +
             " emitters, " + std::to_string(p.alive()) + " particles\n";
        for (size_t i = 0; i < p.effect().emitters.size(); ++i) {
            auto& e = p.effect().emitters[i];
            d += "  emitter: " + e.library + " / " + e.instance + " / " + e.name + ": " + std::to_string(p.alive(i)) + " particles\n";
        }
        const WindSim& w = r->wind();
        d += "wind: dir " + std::to_string(r->wind_direction_degrees(tool.last_params_)) + " deg, strength " + std::to_string(w.strength) + ", vfx (" +
             std::to_string(w.vfx_velocity.x) + " " + std::to_string(w.vfx_velocity.z) + "), g_wind (" + std::to_string(w.smoothed.x) + " " + std::to_string(w.smoothed.y) + ")\n";
        for (auto& n : p.effect().notes) d += "  vfx note: " + n + "\n";
        for (auto& e : p.errors()) d += "  vfx error: " + e + "\n";
    }
    for (auto& e : r->models().errors()) d += "scene error: " + e + "\n";
    for (auto& e : r->textures().errors()) d += "texture error: " + e + "\n";
    d += "points readback: " + std::to_string(r->debug_maps) + " maps, raw " + std::to_string(r->debug_points[0]) + " " + std::to_string(r->debug_points[1]) + " " +
         std::to_string(r->debug_points[2]) + " " + std::to_string(r->debug_points[3]) + "\n";
    d += "sky cube face averages (+x -x +y -y +z -z): " + r->engine_ambient_faces() + "\n";
    d += "luma black/white points: " + std::to_string(r->luma_black()) + " / " + std::to_string(r->luma_white()) + "\n";
    for (auto& e : r->errors()) d += "render error: " + e + "\n";
    for (auto& m : r->cbuffers().missing()) d += "missing cb var: " + m + "\n";
    return d;
}

void WeatherTool::register_commands(Editor& editor, CommandRegistry& commands) {
    auto ok = [](std::string m = {}) { return CommandResult::success(std::move(m)); };
    auto fail = [](std::string m) { return CommandResult::fail(std::move(m)); };

    commands.add("sample", "sample <seed> <count>", "open a new sample map document (the old one goes)", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        scene_seed_ = a.size() > 0 ? int(std::strtoul(a[0].c_str(), nullptr, 10)) : 1;
        scene_count_ = a.size() > 1 ? std::clamp(std::atoi(a[1].c_str()), 1, 100) : 16;
        if (!editor.vfs()) return fail("the game packs are not mounted");
        reroll(editor);
        return ok("sample map");
    });
    commands.add("reroll", "reroll", "generate the sample map again with the current seed and count", [this, &editor, ok](const CommandRegistry::Args&) {
        reroll(editor);
        return ok();
    });
    commands.add("assert_offset", "assert_offset <item> dx dy dz [yaw_degrees]", "test: a sample map building has this placement", [&editor, ok, fail](const CommandRegistry::Args& a) {
        SampleMapDocument* d = nullptr;
        for (DocumentId id : editor.documents().order())
            if (auto* s = dynamic_cast<SampleMapDocument*>(editor.documents().find(id))) d = s;
        if (!d || a.size() < 4) return fail("usage: assert_offset <item> dx dy dz [yaw_degrees] (needs a sample map)");
        const Placement p = d->placement(std::strtoull(a[0].c_str(), nullptr, 10));
        const float want[3] = {std::strtof(a[1].c_str(), nullptr), std::strtof(a[2].c_str(), nullptr), std::strtof(a[3].c_str(), nullptr)};
        const bool same = std::fabs(p.offset.x - want[0]) < 1e-3f && std::fabs(p.offset.y - want[1]) < 1e-3f && std::fabs(p.offset.z - want[2]) < 1e-3f &&
                          (a.size() < 5 || std::fabs(p.yaw * 57.2957795f - std::strtof(a[4].c_str(), nullptr)) < 1e-2f);
        return same ? ok()
                    : fail("placement is (" + std::to_string(p.offset.x) + ", " + std::to_string(p.offset.y) + ", " + std::to_string(p.offset.z) + ") yaw " +
                           std::to_string(p.yaw * 57.2957795f));
    });

    // ---- the old command-line flags
    commands.add("view", "view <n>", "renderer view: 0 final, 1 HDR raw, 2 albedo, 3 normal, 4 specular, 5 depth, 6 sky cube, 7 bloom, 8 quarter, 9 tone mapped, 10 AO",
                 [&editor, ok, fail](const CommandRegistry::Args& a) {
                     if (a.empty() || !editor.renderer()) return fail("usage: view <n> (needs the renderer)");
                     editor.renderer()->view = Renderer::View(std::clamp(std::atoi(a[0].c_str()), 0, 10));
                     return ok();
                 });
    commands.add("blend", "blend <t>", "cross-fade the active environment with the first other open environment, t from 0 to 1", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: blend <t>");
        blend_t_ = std::clamp(std::strtof(a[0].c_str(), nullptr), 0.0f, 1.0f);
        // the partner is the first environment document that is not the one that lights the scene
        XmlFileDocument* env = active_xml(editor);
        const DocumentId lit = env && env->is_environment() ? env->id() : env_doc_;
        blend_doc_ = 0;
        for (DocumentId id : editor.documents().order())
            if (XmlFileDocument* x = find_xml(editor, id); x && x->is_environment() && id != lit) { blend_doc_ = id; break; }
        return blend_doc_ ? ok() : fail("no second environment is open");
    });
    commands.add("pick", "pick <category>=<value>", "weather picker condition (opens the picker; repeatable)", [this, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty() || a[0].find('=') == std::string::npos) return fail("usage: pick <category>=<value>");
        const size_t eq = a[0].find('=');
        picker_.set_pick(a[0].substr(0, eq), a[0].substr(eq + 1));
        picker_.open = true;
        return ok();
    });
    commands.add("picker", "picker pick|edit", "open the weather picker on the Pick (any other argument) or Edit tab", [this, ok](const CommandRegistry::Args& a) {
        picker_.open = true;
        if (!a.empty() && a[0] == "edit") picker_.show_edit_tab();
        return ok();
    });
    commands.add("vdb-script", "vdb-script \"add-value:climate=X;save:both\"", "edit and save the weather database (for tests; see docs/re/variation_db.md)", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: vdb-script \"<commands>\"");
        picker_.open = true;
        picker_.set_script(a[0]);
        if (!picker_.ensure_loaded(editor, editor.output_dir())) return fail("the database did not load");
        return ok(picker_.message());
    });
    commands.add("assert_candidates", "assert_candidates <n> [file]", "test: the picker finds n environment files for its conditions, optionally including <file>", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: assert_candidates <n> [file]");
        if (!picker_.ensure_loaded(editor, editor.output_dir())) return fail("the database did not load");
        const auto r = picker_.candidates();
        if (r.candidates.size() != std::strtoull(a[0].c_str(), nullptr, 10)) return fail(std::to_string(r.candidates.size()) + " candidates");
        if (a.size() > 1) {
            bool found = false;
            for (auto& c : r.candidates) found = found || Vfs::normalize(c.variation->filename) == Vfs::normalize(a[1]);
            if (!found) return fail(a[1] + " is not a candidate");
        }
        return ok();
    });
    commands.add("no-flare", "no-flare", "lens flare off (the old --no-flare flag)", [&editor, ok](const CommandRegistry::Args&) {
        if (editor.renderer()) editor.renderer()->lens_flare = false;
        return ok();
    });
    commands.add("output_dir", "output_dir <folder>", "where Save writes loose files (stored in wce.ini unless 'temp' is added)", [&editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: output_dir <folder> [temp]");
        editor.set_output_dir(fs::path(widen(a[0])), !(a.size() > 1 && a[1] == "temp"));
        return ok();
    });

    // ---- XML documents
    commands.add("edit_attr", "edit_attr <element index|name> <attribute> <value>", "set an attribute of the active weather file (one undo entry), the value as it is shown", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        XmlFileDocument* d = active_xml(editor);
        if (!d || a.size() < 3) return fail("usage: edit_attr <element> <attribute> <value> (needs an open weather file)");
        const int e = resolve_element(d->xml(), a[0]);
        if (e < 0) return fail("no element '" + a[0] + "'");
        const int attr = resolve_attr(d->xml(), e, a[1]);
        if (attr < 0) return fail("no attribute '" + a[1] + "' on " + a[0]);
        d->set_attr_text(e, size_t(attr), a[2]);
        return ok();
    });
    commands.add("xml_op", "xml_op duplicate|remove|up|down|add_child|add_attr|remove_attr <element> [name] [value]", "structural edit of the active weather file (one undo entry)", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        XmlFileDocument* d = active_xml(editor);
        if (!d || a.size() < 2) return fail("usage: xml_op <op> <element> [name] [value]");
        using Op = XmlFileDocument::Op;
        const int e = resolve_element(d->xml(), a[1]);
        if (e < 0) return fail("no element '" + a[1] + "'");
        const std::string name = a.size() > 2 ? a[2] : std::string(), value = a.size() > 3 ? a[3] : std::string();
        Op op;
        size_t attr = 0;
        if (a[0] == "duplicate") op = Op::Duplicate;
        else if (a[0] == "remove") op = Op::Remove;
        else if (a[0] == "up") op = Op::MoveUp;
        else if (a[0] == "down") op = Op::MoveDown;
        else if (a[0] == "add_child") op = Op::AddChild;
        else if (a[0] == "add_attr") op = Op::AddAttr;
        else if (a[0] == "remove_attr") {
            op = Op::RemoveAttr;
            const int k = resolve_attr(d->xml(), e, name);
            if (k < 0) return fail("no attribute '" + name + "'");
            attr = size_t(k);
        } else return fail("unknown operation '" + a[0] + "'");
        std::string err;
        int sel = -1;
        if (!d->structural(op, e, attr, name, value, &sel, &err)) return fail(err);
        editor.selection().remove_document(d->id());
        if (sel >= 0) editor.selection().set({d->id(), uint64_t(sel)});
        return ok();
    });
    commands.add("revert_xml", "revert_xml", "undo every edit of the active weather file (also structural ones)", [this, &editor, ok, fail](const CommandRegistry::Args&) {
        XmlFileDocument* d = active_xml(editor);
        if (!d) return fail("no weather file");
        d->revert_all();
        editor.selection().remove_document(d->id());
        return ok();
    });
    commands.add("assert_attr", "assert_attr <element index|name> <attribute> <value>", "test: an attribute of the active weather file shows this value", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        XmlFileDocument* d = active_xml(editor);
        if (!d || a.size() < 3) return fail("usage: assert_attr <element> <attribute> <value>");
        const int e = resolve_element(d->xml(), a[0]);
        if (e < 0) return fail("no element '" + a[0] + "'");
        const int attr = resolve_attr(d->xml(), e, a[1]);
        if (attr < 0) return fail("no attribute '" + a[1] + "'");
        const std::string now = XmlDoc::decode_entities(std::string(d->xml().attr_raw(e, size_t(attr))));
        return now == a[2] ? ok() : fail("value is '" + now + "'");
    });
    commands.add("assert_xml_elements", "assert_xml_elements N", "test: the active weather file has N elements", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        XmlFileDocument* d = active_xml(editor);
        if (!d || a.empty()) return fail("usage: assert_xml_elements N");
        return d->xml().elements().size() == std::strtoull(a[0].c_str(), nullptr, 10) ? ok() : fail(std::to_string(d->xml().elements().size()) + " elements");
    });
    commands.add("assert_env", "assert_env <field> <value>", "test: the environment the viewport shows has this value (sun_elevation, sun_azimuth, fog_distance_start, has_sky, blend_t, sun_scale)", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.size() < 2) return fail("usage: assert_env <field> <value>");
        const EnvironmentParams e = editor.scene().environment.resolved();
        float now;
        if (a[0] == "sun_elevation") now = e.sun_elevation;
        else if (a[0] == "sun_azimuth") now = e.sun_azimuth;
        else if (a[0] == "fog_distance_start") now = e.fog_distance_start;
        else if (a[0] == "has_sky") now = e.has_sky ? 1.0f : 0.0f;
        else if (a[0] == "sun_scale") now = e.sun_scale;
        else if (a[0] == "blend_t") now = editor.scene().environment.blend_t;
        else return fail("unknown field '" + a[0] + "'");
        const float want = std::strtof(a[1].c_str(), nullptr);
        return std::fabs(now - want) < 1e-3f * std::max(1.0f, std::fabs(want)) ? ok() : fail(a[0] + " is " + std::to_string(now));
    });
    commands.add("diagnostics", "diagnostics <file.txt>", "write the renderer, wind and particle state (what weather_editor wrote next to every shot)", [this, &editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: diagnostics <file.txt>");
        const std::string text = weather_diagnostics(editor, *this);
        return write_file(a[0], std::vector<uint8_t>(text.begin(), text.end())) ? ok() : fail("cannot write " + a[0]);
    });
    commands.add("assert_saved","assert_saved <relative path>", "test: a loose file exists under the output folder", [&editor, ok, fail](const CommandRegistry::Args& a) {
        if (a.empty()) return fail("usage: assert_saved <path under the output folder>");
        std::error_code ec;
        return fs::exists(editor.output_dir() / widen(a[0]), ec) ? ok() : fail("no such file");
    });
}

} // namespace wce
