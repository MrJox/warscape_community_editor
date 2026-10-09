// editor_check: the editor core without a window or GPU: command stack (merge, undo, redo, saved state), document manager and scene
// layers, selection, command registry and script runner, camera controllers, property paths.
#include "editor/camera_controller.h"
#include "editor/command.h"
#include "editor/command_registry.h"
#include "editor/document_manager.h"
#include "editor/selection.h"

#include <cmath>
#include <cstdio>

using namespace wce;

static int failures = 0;
#define CHECK(cond, msg) do { if (!(cond)) { ++failures; std::printf("FAIL: %s\n", msg); } } while (0)

namespace {

// An inspector that records what the document asks for and plays back scripted edits.
class RecordingInspector : public Inspector {
public:
    std::vector<std::string> calls;                  // "<kind> <path>"
    bool edit_next = false;                          // the next edit_float reports a change to `value_to_set`
    float value_to_set = 0;
    bool ended = false;
    void section(const std::string& t) override { calls.push_back("section " + t); }
    void text(const std::string& f, const std::string&, const std::string&) override { calls.push_back("text " + path(f).to_string()); }
    bool edit_float(const std::string& f, const std::string&, float& v, float, float, float) override {
        calls.push_back("float " + path(f).to_string());
        if (!edit_next) return false;
        v = value_to_set;
        return true;
    }
    bool edit_float3(const std::string& f, const std::string&, float*, float) override { calls.push_back("float3 " + path(f).to_string()); return false; }
    bool edit_float2(const std::string& f, const std::string&, float*, float) override { calls.push_back("float2 " + path(f).to_string()); return false; }
    bool edit_int(const std::string& f, const std::string&, int64_t&, float) override { calls.push_back("int " + path(f).to_string()); return false; }
    bool edit_color3(const std::string& f, const std::string&, float*) override { calls.push_back("color3 " + path(f).to_string()); return false; }
    bool edit_color4(const std::string& f, const std::string&, float*) override { calls.push_back("color4 " + path(f).to_string()); return false; }
    bool edit_bool(const std::string& f, const std::string&, bool&) override { calls.push_back("bool " + path(f).to_string()); return false; }
    bool edit_string(const std::string& f, const std::string&, std::string&) override { calls.push_back("string " + path(f).to_string()); return false; }
    bool edit_choice(const std::string& f, const std::string&, int&, const std::vector<std::string>&) override { calls.push_back("choice " + path(f).to_string()); return false; }
    bool gesture_ended() override { return ended; }
};

// A document of N numbers; items are the indices; each edit is a command.
class NumberDoc : public Document {
public:
    explicit NumberDoc(int n) : values(size_t(n), 0.0f) {}
    std::string title() const override { return "numbers"; }
    std::string kind() const override { return "numbers"; }
    bool can_save() const override { return true; }
    bool save(const SaveTarget&, std::string*) override { ++saves; return true; }
    void publish(RenderScene& scene, LayerId layer) override {
        ++publishes;
        scene.clear_layer(layer);
        for (size_t i = 0; i < values.size(); ++i) scene.add(layer, translation({values[i], 0, 0}), Overlay{}, SourceRef{id(), i});
    }
    void inspect(const SourceRef& r, Inspector& ins) override {
        ins.section("Number");
        float v = values[r.item];
        if (ins.edit_float("value", "value", v)) set(r.item, v, (uint64_t(id()) << 32) | r.item);
        if (ins.gesture_ended()) history_.break_merge();
    }
    bool item_transform(const SourceRef& r, float4x4& w) const override {
        if (r.item >= values.size()) return false;
        w = translation({values[r.item], 0, 0});
        return true;
    }
    void transform_items(const std::vector<SourceRef>& items, const float4x4& delta, uint64_t gesture) override {
        for (const SourceRef& r : items) set(r.item, values[r.item] + delta.m[0][3], gesture);
    }
    void set(size_t i, float v, uint64_t key);
    std::vector<float> values;
    int saves = 0, publishes = 0;
};

class SetCommand : public Command {
public:
    SetCommand(NumberDoc& d, size_t i, float before, float after, uint64_t key) : d_(d), i_(i), before_(before), after_(after), key_(key) {}
    std::string name() const override { return "set " + std::to_string(i_); }
    void apply() override { d_.values[i_] = after_; }
    void revert() override { d_.values[i_] = before_; }
    uint64_t merge_key() const override { return key_; }
    bool merge_from(const Command& next) override {
        const auto* n = dynamic_cast<const SetCommand*>(&next);
        if (!n || n->i_ != i_) return false;
        after_ = n->after_;
        return true;
    }
    std::vector<uint64_t> dirty_items() const override { return {i_}; }

private:
    NumberDoc& d_;
    size_t i_;
    float before_, after_;
    uint64_t key_;
};

void NumberDoc::set(size_t i, float v, uint64_t key) { history_.push(std::make_unique<SetCommand>(*this, i, values[i], v, key)); }

} // namespace

int main() {
    // ---- command stack
    {
        NumberDoc d(3);
        CommandStack& h = d.history();
        CHECK(!h.modified() && !h.can_undo() && !h.can_redo(), "a new stack is clean");
        d.set(0, 1, 0);
        d.set(1, 2, 0);
        CHECK(h.size() == 2 && h.position() == 2 && h.modified(), "two edits, modified");
        CHECK(h.undo() && d.values[1] == 0 && h.position() == 1, "undo");
        CHECK(h.redo() && d.values[1] == 2, "redo");
        h.undo();
        d.set(2, 5, 0);
        CHECK(!h.can_redo() && h.size() == 2, "a new edit drops the redo tail");
        h.mark_saved();
        CHECK(!h.modified(), "saved");
        h.undo();
        CHECK(h.modified(), "undo after saving is a change");
        h.redo();
        CHECK(!h.modified(), "back at the saved position");
        h.undo();
        d.set(1, 9, 0);   // the saved state was in the dropped tail
        CHECK(h.modified(), "after dropping the saved state the document stays modified");

        // merged gesture: one entry for many pushes, until break_merge
        NumberDoc g(1);
        for (int i = 1; i <= 10; ++i) g.set(0, float(i), 77);
        CHECK(g.history().size() == 1 && g.values[0] == 10, "a drag gesture is one undo entry");
        g.history().undo();
        CHECK(g.values[0] == 0, "undoing the merged gesture restores the value before the drag");
        g.history().redo();
        g.history().break_merge();
        g.set(0, 11, 77);
        CHECK(g.history().size() == 2, "break_merge ends the gesture");
        g.history().mark_saved();
        g.set(0, 12, 77);
        CHECK(g.history().size() == 3, "an edit right after saving never merges into the saved entry");
        g.history().jump_to(0);
        CHECK(g.values[0] == 0 && g.history().position() == 0, "jump_to");
        g.history().jump_to(3);
        CHECK(g.values[0] == 12, "jump_to forward");
        CHECK(g.history().last_dirty().items == std::vector<uint64_t>{0} && !g.history().last_dirty().all, "the stack reports the touched items");
        {
            const CommandStack::Dirty d = g.history().take_dirty();   // everything since the stack was made: several steps of undo and redo in one frame add up
            CHECK(!d.all && !d.items.empty() && d.items.front() == 0, "take_dirty collects the items of every step since the last call");
            const CommandStack::Dirty again = g.history().take_dirty();
            CHECK(!again.all && again.items.empty(), "take_dirty starts over after it was taken");
        }
    }

    // ---- documents and the scene
    {
        RenderScene scene;
        DocumentManager docs(scene);
        auto a = std::make_unique<NumberDoc>(3);
        NumberDoc* pa = a.get();
        const DocumentId ia = docs.add(std::move(a), "t");
        auto b = std::make_unique<NumberDoc>(2);
        const DocumentId ib = docs.add(std::move(b), "t");
        CHECK(docs.order().size() == 2 && docs.active_id() == ib, "two documents, the last one active");
        CHECK(scene.entities().size() == 5 && scene.layers().size() == 2, "each document has a layer with its entities");
        CHECK(scene.entities_of({ia, 1}).size() == 1, "entities_of finds the entity of an item");
        const int before = pa->publishes;
        docs.update();
        CHECK(pa->publishes == before, "an unchanged document is not re-published");
        pa->set(1, 4, 0);
        docs.update();
        CHECK(pa->publishes == before + 1, "an edited document is re-published");
        CHECK(scene.find(scene.entities_of({ia, 1})[0])->world.m[0][3] == 4, "the scene shows the edit");
        CHECK(docs.any_modified(), "modified");
        std::vector<std::string> msgs;
        CHECK(docs.save_all(&msgs) && pa->saves == 1 && !docs.any_modified(), "save_all saves what changed");
        docs.set_visible(ia, false);
        CHECK(!scene.drawable(*scene.find(scene.entities_of({ia, 0})[0])), "a hidden document's entities are not drawable");
        CHECK(docs.close(ia) && scene.entities().size() == 2 && docs.active_id() == ib, "closing removes the layer and its entities");
    }

    // ---- selection
    {
        RenderScene scene;
        DocumentManager docs(scene);
        auto d = std::make_unique<NumberDoc>(4);
        const DocumentId id = docs.add(std::move(d), "t");
        Selection sel;
        sel.set({id, 1});
        sel.add({id, 2});
        sel.add({id, 2});
        CHECK(sel.items().size() == 2 && sel.contains({id, 1}), "set and add");
        sel.toggle({id, 1});
        CHECK(sel.items().size() == 1 && !sel.contains({id, 1}), "toggle removes");
        CHECK(sel.entities(scene).size() == 1, "selected items map to scene entities");
        const uint64_t rev = sel.revision();
        sel.set({id, 2});
        CHECK(sel.revision() == rev, "setting the same single item changes nothing");
        sel.remove_document(id);
        CHECK(sel.empty(), "closing a document drops its items");
    }

    // ---- command registry and scripts
    {
        CommandRegistry reg;
        int calls = 0;
        std::string last;
        reg.add("count", "count", "", [&](const CommandRegistry::Args&) { ++calls; return CommandResult::success(); });
        reg.add("say", "say words", "", [&](const CommandRegistry::Args& a) { last = a.empty() ? "" : a[0]; return CommandResult::success(); });
        reg.add("bad", "bad", "", [](const CommandRegistry::Args&) { return CommandResult::fail("nope"); });
        reg.add("pause", "pause N", "", [&](const CommandRegistry::Args& a) { reg.wait_frames = std::atoi(a[0].c_str()); return CommandResult::success(); });
        reg.add("boom", "boom", "", [](const CommandRegistry::Args&) -> CommandResult { throw std::runtime_error("kaput"); });
        CHECK(CommandRegistry::tokenize("a \"b c\" d # comment").size() == 3, "tokenize groups quotes and drops comments");
        CHECK(CommandRegistry::tokenize("say \"\"").size() == 2, "an empty quoted argument is an argument");
        CHECK(reg.run("say \"hello world\"").ok && last == "hello world", "run with a quoted argument");
        CHECK(!reg.run("nothing").ok, "unknown command fails");
        CHECK(!reg.run("boom").ok && reg.run("boom").message.find("kaput") != std::string::npos, "an exception becomes a failed result");

        ScriptRunner s;
        s.load_text("# a script\ncount\n\n-bad\ncount\npause 3\ncount\n");
        int frames = 0;
        while (!s.done() && frames < 20) { s.step(reg); ++frames; }
        CHECK(!s.failed() && calls == 3, "an optional failing line does not stop the script");
        CHECK(frames == 4, "pause 3 holds the script for three frames");
        ScriptRunner t;
        t.load_text("count\nbad\ncount\n");
        calls = 0;
        t.step(reg);
        CHECK(t.failed() && calls == 1 && t.error().find("bad") != std::string::npos, "a failing line stops the script with its line number");
    }

    // ---- cameras
    {
        OrbitCamera o;
        o.set_orbit(0.6f, 0.35f, 120.0f, {0, 5, 0});
        Camera c = o.camera();
        OrbitCamera o2;
        o2.set_camera(c);
        CHECK(std::fabs(o2.yaw - 0.6f) < 1e-4f && std::fabs(o2.pitch - 0.35f) < 1e-4f && std::fabs(o2.dist - 120.0f) < 1e-2f, "orbit round trip through a Camera");
        CameraInput in;
        in.wheel = 1;
        o.update(in);
        CHECK(o.dist < 120.0f, "the wheel zooms in");
        in = CameraInput{};
        in.rotate = true;
        in.dx = 100;
        const float yaw = o.yaw;
        o.update(in);
        CHECK(o.yaw > yaw, "dragging turns the camera");
        FlyCamera f;
        f.set_camera(c);
        CHECK(length(f.camera().eye - c.eye) < 1e-3f, "fly continues from the orbit view");
        const float3 e0 = f.eye;
        in = CameraInput{};
        in.forward = true;
        in.dt = 1.0f;
        f.update(in);
        CHECK(length(f.eye - e0) > 1.0f, "fly moves forward");
        CHECK(std::fabs(o.fov_y - 1.13446f) < 1e-5f && o.znear == 1.0f && o.zfar == 13000.0f, "the lens is the game's battle camera");
    }

    // ---- inspector framework: paths, edits become merged commands, the gesture end closes the merge
    {
        NumberDoc d(2);
        RecordingInspector ins;
        ins.begin(d.id(), 1);
        d.inspect({d.id(), 1}, ins);
        CHECK(ins.calls.size() == 2 && ins.calls[0] == "section Number" && ins.calls[1] == "float " + std::to_string(d.id()) + "/1/value", "the document describes its item through the inspector");
        ins.edit_next = true;
        for (float v : {1.0f, 2.0f, 3.0f}) {
            ins.value_to_set = v;
            d.inspect({d.id(), 1}, ins);
        }
        CHECK(d.values[1] == 3 && d.history().size() == 1, "a drag in the inspector is one undo entry");
        ins.ended = true;
        ins.edit_next = false;
        d.inspect({d.id(), 1}, ins);
        ins.ended = false;
        ins.edit_next = true;
        ins.value_to_set = 4;
        d.inspect({d.id(), 1}, ins);
        CHECK(d.history().size() == 2, "the end of a gesture closes the merge");
        d.history().undo();
        CHECK(d.values[1] == 3, "undo goes back to the value before the second gesture");
    }

    // ---- property paths
    {
        PropertyPath a{1, 2, "offset"}, b{1, 2, "offset"}, c{1, 3, "offset"};
        CHECK(a == b && !(a == c) && a.to_string() == "1/2/offset", "property paths compare and print");
    }

    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
