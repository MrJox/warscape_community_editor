#include "weather/xml_file_document.h"

#include "formats/env_value.h"
#include "foundation/process.h"
#include "foundation/text_encoding.h"

#include <algorithm>
#include <cmath>
#include <functional>

namespace fs = std::filesystem;

namespace wce {

class XmlAttrCommand : public Command {
public:
    XmlAttrCommand(XmlFileDocument& d, int elem, size_t attr, std::string before, std::string after, uint64_t gesture)
        : doc_(d), elem_(elem), attr_(attr), before_(std::move(before)), after_(std::move(after)), gesture_(gesture) {}
    std::string name() const override { return "Edit " + doc_.xml_.path_of(elem_) + " @" + doc_.xml_.elements()[elem_].attrs[attr_].name; }
    void apply() override { doc_.xml_.set_attr_raw(elem_, attr_, after_); }
    void revert() override { doc_.xml_.set_attr_raw(elem_, attr_, before_); }
    uint64_t merge_key() const override { return gesture_ ? gesture_ ^ (uint64_t(elem_) << 20) ^ (uint64_t(attr_) << 48) : 0; }
    bool merge_from(const Command& next) override {
        const auto* n = dynamic_cast<const XmlAttrCommand*>(&next);
        if (!n || n->elem_ != elem_ || n->attr_ != attr_) return false;
        after_ = n->after_;
        return true;
    }
    std::vector<uint64_t> dirty_items() const override { return {uint64_t(elem_)}; }

private:
    XmlFileDocument& doc_;
    int elem_;
    size_t attr_;
    std::string before_, after_;
    uint64_t gesture_;
};

// A structural edit swaps the whole XmlDoc (element indices change), so it keeps snapshots like the legacy editor did.
class XmlSnapshotCommand : public Command {
public:
    XmlSnapshotCommand(XmlFileDocument& d, std::string name, std::shared_ptr<XmlDoc> before, std::shared_ptr<XmlDoc> after)
        : doc_(d), name_(std::move(name)), before_(std::move(before)), after_(std::move(after)) {}
    std::string name() const override { return name_; }
    void apply() override { doc_.xml_ = *after_; }
    void revert() override { doc_.xml_ = *before_; }

private:
    XmlFileDocument& doc_;
    std::string name_;
    std::shared_ptr<XmlDoc> before_, after_;
};

namespace {

std::string lower(std::string s) {
    for (char& c : s) c = char(std::tolower(static_cast<unsigned char>(c)));
    return s;
}

std::string element_label(const XmlDoc& doc, int e) {
    const XmlElement& el = doc.elements()[e];
    std::string label = el.name;
    if (auto n = doc.get(e, "name")) label += "  '" + *n + "'";
    else if (auto n2 = doc.get(e, "inst_name")) label += "  '" + *n2 + "'";
    return label;
}

} // namespace

XmlFileDocument::XmlFileDocument(Source source, const std::vector<uint8_t>& bytes, fs::path output_dir)
    : source_(std::move(source)), xml_(XmlDoc::parse(bytes)), output_dir_(std::move(output_dir)) {}

std::string XmlFileDocument::title() const {
    return source_.vfs_path.empty() ? narrow(source_.disk.filename().wstring()) : source_.vfs_path.substr(source_.vfs_path.rfind('/') + 1);
}

std::string XmlFileDocument::kind() const {
    const std::string t = lower(title());
    if (t.size() > 12 && t.compare(t.size() - 12, 12, ".environment") == 0) return "environment";
    if (t.size() > 9 && t.compare(t.size() - 9, 9, ".lighting") == 0) return "lighting";
    if (source_.vfs_path.rfind("vfx/", 0) == 0) return "vfx";
    if (source_.vfs_path.rfind("weather/", 0) == 0) return "weather_definitions";
    return "xml";
}

bool XmlFileDocument::is_environment() const {
    const std::string k = kind();
    return k == "environment" || k == "lighting";
}

fs::path XmlFileDocument::default_save_path() const {
    if (source_.vfs_path.empty()) return source_.disk;
    return output_dir_ / widen(source_.vfs_path);
}

bool XmlFileDocument::save(const SaveTarget& target, std::string* message) {
    const fs::path to = target.path.empty() ? default_save_path() : target.path;
    try {
        std::error_code ec;
        fs::create_directories(to.parent_path(), ec);
        if (!write_file(narrow(to.wstring()), xml_.save())) throw std::runtime_error("write failed");
        // The saved file becomes the new baseline.
        xml_ = XmlDoc::parse(read_file(narrow(to.wstring())));
        history_.clear();
        if (source_.vfs_path.empty()) source_.disk = to;
        if (message) *message = "saved " + narrow(to.wstring());
        return true;
    } catch (const std::exception& e) {
        if (message) *message = std::string("save failed: ") + e.what();
        return false;
    }
}

void XmlFileDocument::revert_all() {
    if (xml_.structural_edits() > 0) xml_ = XmlDoc::parse(xml_.origin_bytes());
    else xml_.revert_all();
    history_.clear();
}

void XmlFileDocument::set_attr(int elem, size_t attr, const std::string& raw, uint64_t gesture) {
    if (elem < 0 || elem >= int(xml_.elements().size()) || attr >= xml_.elements()[elem].attrs.size()) return;
    const std::string before(xml_.attr_raw(elem, attr));
    if (before == raw) return;
    history_.push(std::make_unique<XmlAttrCommand>(*this, elem, attr, before, raw, gesture));
}

void XmlFileDocument::set_attr_text(int elem, size_t attr, const std::string& text, uint64_t gesture) {
    if (elem < 0 || elem >= int(xml_.elements().size()) || attr >= xml_.elements()[elem].attrs.size()) return;
    const XmlAttr& at = xml_.elements()[elem].attrs[attr];
    EnvValue v = parse_value(XmlDoc::decode_entities(std::string(xml_.attr_raw(elem, attr))));
    // Same number style as the original text; plain strings are taken as typed.
    EnvValue n = parse_value(text);
    if (n.kind == v.kind) n.style = v.style;
    const std::string out = n.kind == ValueKind::String ? text : format_value(n);
    set_attr(elem, attr, XmlDoc::encode_entities(out, at.quote), gesture);
}

bool XmlFileDocument::structural(Op op, int elem, size_t attr, const std::string& name, const std::string& value, int* select_after, std::string* error) {
    try {
        if (elem < 0 || elem >= int(xml_.elements().size())) throw std::runtime_error("no such element");
        int sel = elem;
        XmlDoc next;
        std::string label;
        switch (op) {
        case Op::Remove: next = xml_.removed_element(elem); sel = xml_.elements()[elem].parent; label = "Delete " + xml_.path_of(elem); break;
        case Op::Duplicate: next = xml_.duplicated_element(elem, &sel); label = "Duplicate " + xml_.path_of(elem); break;
        case Op::MoveUp: next = xml_.moved_element(elem, -1); sel = -1; label = "Move up " + xml_.path_of(elem); break;
        case Op::MoveDown: next = xml_.moved_element(elem, +1); sel = -1; label = "Move down " + xml_.path_of(elem); break;
        case Op::AddChild: next = xml_.with_child(elem, name, &sel); label = "Add <" + name + "> to " + xml_.path_of(elem); break;
        case Op::AddAttr: next = xml_.with_attr_added(elem, name, value); label = "Add @" + name + " to " + xml_.path_of(elem); break;
        case Op::RemoveAttr: label = "Remove @" + xml_.elements()[elem].attrs.at(attr).name + " of " + xml_.path_of(elem); next = xml_.without_attr(elem, attr); break;
        }
        history_.push(std::make_unique<XmlSnapshotCommand>(*this, label, std::make_shared<XmlDoc>(xml_), std::make_shared<XmlDoc>(next)));
        if (select_after) *select_after = sel;
        return true;
    } catch (const std::exception& e) {
        if (error) *error = e.what();
        return false;
    }
}

OutlinerNode XmlFileDocument::outline() const {
    std::function<OutlinerNode(int)> build = [&](int e) {
        OutlinerNode n{element_label(xml_, e), {id(), uint64_t(e)}, true, {}};
        for (int c : xml_.elements()[e].children) n.children.push_back(build(c));
        return n;
    };
    OutlinerNode root{title(), {id(), uint64_t(0xffffffffu)}, false, {}};
    for (int r : xml_.roots()) root.children.push_back(build(r));
    return root;
}

std::string XmlFileDocument::describe(const SourceRef& item) const {
    if (item.item >= xml_.elements().size()) return {};
    return xml_.path_of(int(item.item));
}

void XmlFileDocument::inspect(const SourceRef& item, Inspector& ins) {
    if (item.item >= xml_.elements().size()) return;
    const int e = int(item.item);
    const XmlElement& el = xml_.elements()[e];
    ins.section("Attributes of <" + el.name + ">");
    if (el.attrs.empty()) ins.text("none", "(no attributes)", "");
    const uint64_t gesture_base = (uint64_t(id()) << 40) ^ (uint64_t(e) << 8) ^ 0x7;
    for (size_t a = 0; a < el.attrs.size(); ++a) {
        const std::string raw(xml_.attr_raw(e, a));
        EnvValue v = parse_value(XmlDoc::decode_entities(raw));
        const std::string& label = el.attrs[a].name;
        const std::string field = label;
        bool changed = false;
        switch (v.kind) {
        case ValueKind::Bool: changed = ins.edit_bool(field, label, v.b); break;
        case ValueKind::Int: { int64_t i = v.i; changed = ins.edit_int(field, label, i); v.i = i; break; }
        case ValueKind::Float: changed = ins.edit_float(field, label, v.f[0], std::max(std::fabs(v.f[0]) * 0.005f, 0.001f)); break;
        case ValueKind::Rgb: changed = ins.edit_color3(field, label, v.f.data()); break;
        case ValueKind::Argb: changed = ins.edit_color4(field, label, v.f.data()); break;
        case ValueKind::Variance: changed = ins.edit_float2(field, label, v.f.data()); break;
        case ValueKind::Euler: changed = ins.edit_float3(field, label, v.f.data()); break;
        case ValueKind::String: changed = ins.edit_string(field, label, v.s); break;
        }
        if (changed) set_attr(e, a, XmlDoc::encode_entities(format_value(v), el.attrs[a].quote), gesture_base ^ (uint64_t(a) << 52));
        if (ins.gesture_ended()) history_.break_merge();
    }
}

} // namespace wce
