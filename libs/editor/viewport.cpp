#include "editor/editor.h"

#include "foundation/log.h"
#include "scene/environment.h"

#include <ImGuizmo.h>

#include <algorithm>
#include <cmath>

namespace wce {

namespace {

// ImGuizmo wants row-vector matrices (v * M, row-major in memory); ours are column-vector (M * v), so it gets the transpose.
void to_gizmo(const float4x4& m, float out[16]) {
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c) out[r * 4 + c] = m.m[c][r];
}
float4x4 from_gizmo(const float in[16]) {
    float4x4 m;
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c) m.m[c][r] = in[r * 4 + c];
    return m;
}

} // namespace

Viewport::Viewport() : orbit_(std::make_unique<OrbitCamera>()), fly_(std::make_unique<FlyCamera>()) { controller_ = orbit_.get(); }

void Viewport::use_orbit() {
    if (!fly_active_) return;
    orbit_->set_camera(fly_->camera());
    controller_ = orbit_.get();
    fly_active_ = false;
}

void Viewport::use_fly() {
    if (fly_active_) return;
    fly_->set_camera(orbit_->camera());
    controller_ = fly_.get();
    fly_active_ = true;
}

void Viewport::render(Editor& editor, int w, int h) {
    Renderer* r = editor.renderer();
    if (!r) return;
    RenderScene& scene = editor.scene();
    if (!scene.environment.active) {   // nothing lights the scene yet: the default environment with the player's output settings
        EnvironmentParams e;
        const GraphicsOptions go = read_graphics_options();
        e.output_gamma = go.gamma;
        e.output_brightness = go.brightness;
        scene.environment.active = e;
    }
    r->show_grid = show_grid;
    r->selection = editor.selection().entities(scene);
    Clock& clock = editor.clock();
    clock.tick();
    r->deterministic = clock.fixed_step();
    if (!target_.resize(editor.device(), w, h)) {
        Log::error("cannot create the viewport surface");
        return;
    }
    r->render(scene, ViewParams{controller_->camera(), float(clock.time())}, target_);
    ++frames_rendered;
}

EntityId Viewport::pick(Editor& editor, int x, int y) {
    Renderer* r = editor.renderer();
    if (!r || !target_.valid()) return 0;
    return r->pick(editor.scene(), ViewParams{controller_->camera(), float(editor.clock().time())}, target_, x, y);
}

bool Viewport::capture(Editor& editor, int w, int h, int frames, const std::wstring& path) {
    Renderer* r = editor.renderer();
    if (!r || w < 16 || h < 16) return false;
    RenderScene& scene = editor.scene();
    if (!scene.environment.active) scene.environment.active = EnvironmentParams{};
    RenderTarget out;
    if (!out.create(editor.device(), w, h)) return false;
    Clock clock = Clock::fixed(1.0 / 60.0);
    r->deterministic = true;
    r->show_grid = show_grid;
    r->selection = editor.selection().entities(scene);
    try {
        for (int i = 0; i < frames; ++i) {
            clock.tick();
            r->render(scene, ViewParams{controller_->camera(), float(clock.time())}, out);
        }
    } catch (const std::exception& e) {
        Log::error(std::string("capture: ") + e.what());
        return false;
    }
    return out.save_png(editor.device(), editor.context(), path);
}

void Viewport::focus_selection(Editor& editor) {
    Renderer* r = editor.renderer();
    if (!r || editor.selection().empty()) return;
    float3 lo{1e30f, 1e30f, 1e30f}, hi{-1e30f, -1e30f, -1e30f};
    auto grow = [&](const float3& p) {
        lo = {std::min(lo.x, p.x), std::min(lo.y, p.y), std::min(lo.z, p.z)};
        hi = {std::max(hi.x, p.x), std::max(hi.y, p.y), std::max(hi.z, p.z)};
    };
    for (EntityId id : editor.selection().entities(editor.scene())) {
        const Entity* e = editor.scene().find(id);
        if (!e) continue;
        if (const MeshInstance* mi = e->get<MeshInstance>()) {
            if (const GpuModel* m = r->models().model(mi->model))
                for (int k = 0; k < 8; ++k)
                    grow(e->world.transform_point({(k & 1) ? m->bmax.x : m->bmin.x, (k & 2) ? m->bmax.y : m->bmin.y, (k & 4) ? m->bmax.z : m->bmin.z}));
        } else if (const Overlay* o = e->get<Overlay>()) {
            for (const auto* vs : {&o->lines, &o->triangles})
                for (const OverlayVertex& v : *vs) grow(e->world.transform_point(v.p));
        }
    }
    if (lo.x > hi.x) return;
    controller_->focus((lo + hi) * 0.5f, std::max(2.0f, length(hi - lo) * 0.5f));
}

void Viewport::draw(Editor& editor) {
    hovered = false;
    ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0, 0));
    const bool open = ImGui::Begin("Viewport", &editor.panels.viewport, ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
    ImGui::PopStyleVar();
    if (!open) { ImGui::End(); return; }
    ImGuizmo::BeginFrame();
    const ImVec2 size = ImGui::GetContentRegionAvail();
    const int w = int(size.x), h = int(size.y);
    if (!editor.renderer()) {
        ImGui::TextDisabled("The renderer is not available (the game packs are not mounted).");
        ImGui::End();
        return;
    }
    if (w < 16 || h < 16) { ImGui::End(); return; }

    render(editor, w, h);
    if (!target_.valid()) { ImGui::End(); return; }

    const ImVec2 origin = ImGui::GetCursorScreenPos();
    ImGui::Image(ImTextureID(target_.srv()), size);
    ImGui::SetCursorScreenPos(origin);
    ImGui::InvisibleButton("##viewport", size, ImGuiButtonFlags_MouseButtonLeft | ImGuiButtonFlags_MouseButtonRight | ImGuiButtonFlags_MouseButtonMiddle);
    hovered = ImGui::IsItemHovered();
    const bool active = ImGui::IsItemActive();
    ImGuiIO& io = ImGui::GetIO();

    // ---- gizmo
    Document* gizmo_doc = nullptr;
    float4x4 world;
    SourceRef primary;
    if (gizmo != GizmoMode::Off && !fly_active_ && !editor.selection().empty()) {
        primary = editor.selection().items()[0];
        gizmo_doc = editor.documents().find(primary.doc);
        if (gizmo_doc && !gizmo_doc->item_transform(primary, world)) gizmo_doc = nullptr;
    }
    bool using_gizmo = false;
    if (gizmo_doc) {
        const Camera cam = controller_->camera();
        const float4x4 view_m = look_at_lh(cam.eye, cam.target, {0, 1, 0});
        const float4x4 proj = perspective_lh(cam.fov_y, float(w) / float(h), cam.znear, cam.zfar);
        float v[16], p[16], m[16];
        to_gizmo(view_m, v);
        to_gizmo(proj, p);
        to_gizmo(world, m);
        ImGuizmo::SetOrthographic(false);
        ImGuizmo::SetDrawlist();
        ImGuizmo::SetRect(origin.x, origin.y, size.x, size.y);
        const ImGuizmo::OPERATION op = gizmo == GizmoMode::Translate ? ImGuizmo::TRANSLATE : gizmo == GizmoMode::Rotate ? ImGuizmo::ROTATE : ImGuizmo::SCALE;
        const ImGuizmo::MODE mode = gizmo_local && gizmo != GizmoMode::Translate ? ImGuizmo::LOCAL : ImGuizmo::WORLD;
        float snap_values[3] = {snap_move, snap_move, snap_move};
        if (gizmo == GizmoMode::Rotate) snap_values[0] = snap_angle;
        if (gizmo == GizmoMode::Scale) snap_values[0] = snap_scale;
        ImGuizmo::Manipulate(v, p, op, mode, m, nullptr, snap ? snap_values : nullptr);
        using_gizmo = ImGuizmo::IsUsing();
        if (using_gizmo) {
            if (!gizmo_was_using_) ++gesture_;
            const float4x4 now = from_gizmo(m);
            const float4x4 delta = now * inverse(world);
            gizmo_doc->transform_items(editor.selection().items(), delta, gesture_);
        }
    }
    if (!using_gizmo && gizmo_was_using_) {
        if (Document* d = editor.documents().find(editor.selection().items().empty() ? 0 : editor.selection().items()[0].doc)) d->history().break_merge();
    }
    gizmo_was_using_ = using_gizmo;
    const bool over_gizmo = using_gizmo || (gizmo_doc && ImGuizmo::IsOver());

    // ---- camera
    CameraInput in;
    in.dt = io.DeltaTime;
    if (hovered || active) {
        in.dx = io.MouseDelta.x;
        in.dy = io.MouseDelta.y;
        in.wheel = hovered ? io.MouseWheel : 0;
        if (!over_gizmo) {
            in.rotate = fly_active_ ? ImGui::IsMouseDown(ImGuiMouseButton_Right) && active
                                    : (ImGui::IsMouseDown(ImGuiMouseButton_Left) || ImGui::IsMouseDown(ImGuiMouseButton_Right)) && active;
            in.pan = ImGui::IsMouseDown(ImGuiMouseButton_Middle) && active;
        }
        if (fly_active_ && ImGui::IsMouseDown(ImGuiMouseButton_Right) && !io.WantTextInput) {
            in.forward = ImGui::IsKeyDown(ImGuiKey_W);
            in.back = ImGui::IsKeyDown(ImGuiKey_S);
            in.left = ImGui::IsKeyDown(ImGuiKey_A);
            in.right = ImGui::IsKeyDown(ImGuiKey_D);
            in.up = ImGui::IsKeyDown(ImGuiKey_E);
            in.down = ImGui::IsKeyDown(ImGuiKey_Q);
            in.fast = io.KeyShift;
        }
    }
    controller_->update(in);

    // ---- click: a press and release without moving selects what is under the cursor
    if (hovered && ImGui::IsMouseClicked(ImGuiMouseButton_Left) && !over_gizmo) {
        press_ = {io.MousePos.x, io.MousePos.y};
        pressed_ = true;
    }
    if (pressed_ && ImGui::IsMouseReleased(ImGuiMouseButton_Left)) {
        pressed_ = false;
        const float moved = std::hypot(io.MousePos.x - press_.x, io.MousePos.y - press_.y);
        if (hovered && moved < 4.0f && !over_gizmo) {
            const int x = int(io.MousePos.x - origin.x), y = int(io.MousePos.y - origin.y);
            ViewportEvent ev;
            ev.kind = ViewportEvent::Kind::Click;
            ev.x = x;
            ev.y = y;
            ev.ctrl = io.KeyCtrl;
            ev.shift = io.KeyShift;
            ev.alt = io.KeyAlt;
            ev.hit = pick(editor, x, y);
            if (const Entity* e = editor.scene().find(ev.hit)) ev.hit_source = e->source;
            Tool* tool = editor.active_tool();
            if (!(tool && tool->viewport_input(editor, ev))) {
                if (ev.hit_source.doc == 0) {
                    if (!ev.ctrl) editor.selection().clear();
                } else if (ev.ctrl) {
                    editor.selection().toggle(ev.hit_source);
                } else if (ev.shift) {
                    editor.selection().add(ev.hit_source);
                } else {
                    editor.selection().set(ev.hit_source);
                }
                if (ev.hit_source.doc) editor.documents().set_active(ev.hit_source.doc);
            }
        }
    }
    ImGui::End();
}

} // namespace wce
