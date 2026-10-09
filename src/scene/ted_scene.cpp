#include "scene/ted_scene.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <stdexcept>

namespace te {

std::string format_number(double v) {
    float f = static_cast<float>(v);
    double d = f;
    // exact decimal expansion of the float, then round half away from zero at the 6th decimal
    char buf[512];
    std::snprintf(buf, sizeof buf, "%.40f", std::fabs(d));
    std::string s = buf;
    size_t dot = s.find('.');
    std::string int_part = s.substr(0, dot), frac = s.substr(dot + 1);
    std::string keep = frac.substr(0, 6);
    bool up = frac[6] >= '5';
    std::string digits = int_part + keep;
    if (up) {
        int i = static_cast<int>(digits.size()) - 1;
        while (i >= 0) {
            if (digits[i] == '9') { digits[i] = '0'; --i; }
            else { ++digits[i]; break; }
        }
        if (i < 0) digits.insert(digits.begin(), '1');
    }
    std::string ip = digits.substr(0, digits.size() - 6), fp = digits.substr(digits.size() - 6);
    while (!fp.empty() && fp.back() == '0') fp.pop_back();
    std::string out = std::signbit(d) ? "-" : "";
    out += ip;
    if (!fp.empty()) out += "." + fp;
    return out;
}

std::string format_general(double v) {
    char buf[64];
    std::snprintf(buf, sizeof buf, "%g", static_cast<double>(static_cast<float>(v)));
    return buf;
}

std::optional<Vec3> parse_vec3(const std::string& text, const char* prefix) {
    std::string t = text;
    size_t plen = std::strlen(prefix);
    if (plen) {
        if (t.compare(0, plen, prefix) != 0 || t.size() < plen + 2 || t[plen] != '(' || t.back() != ')') return std::nullopt;
        t = t.substr(plen + 1, t.size() - plen - 2);
    }
    Vec3 v{};
    const char* p = t.c_str();
    for (int i = 0; i < 3; ++i) {
        char* end = nullptr;
        v[i] = std::strtod(p, &end);
        if (end == p) return std::nullopt;
        p = end;
        if (i < 2) { if (*p != ',') return std::nullopt; ++p; }
    }
    return v;
}

TedScene TedScene::parse(const std::vector<uint8_t>& bytes) {
    TedScene s;
    s.doc_ = we::XmlDoc::parse(bytes);
    if (s.doc_.roots().empty() || s.doc_.elements()[s.doc_.roots()[0]].name != "project") throw std::runtime_error("not a tile.ted project");
    return s;
}

int TedScene::project_version() const {
    return std::atoi(doc_.get(doc_.roots()[0], "version").value_or("0").c_str());
}

int TedScene::scene_graph() const {
    for (int c : doc_.elements()[doc_.roots()[0]].children)
        if (doc_.elements()[c].name == "scene")
            for (int g : doc_.elements()[c].children)
                if (doc_.elements()[g].name == "SCENE_GRAPH") return g;
    return -1;
}

std::vector<int> TedScene::children(int elem) const {
    std::vector<int> out;
    for (int c : doc_.elements()[elem].children)
        if (doc_.elements()[c].name == "children") for (int g : doc_.elements()[c].children) out.push_back(g);
    return out;
}

bool TedScene::is_layer(int elem) const {
    const std::string& n = doc_.elements()[elem].name;
    return n == "LayerInstance" || n == "UserInstanceGroup" || n == "INSTANCE_GROUP" || n == "SCENE_GRAPH";
}

std::optional<Vec3> TedScene::position(int elem) const {
    auto v = doc_.get(elem, "position");
    return v ? parse_vec3(*v, "vector") : std::nullopt;
}
std::optional<Vec3> TedScene::euler(int elem) const {
    auto v = doc_.get(elem, "orientation");
    return v ? parse_vec3(*v, "euler") : std::nullopt;
}
std::optional<Vec3> TedScene::scale(int elem) const {
    auto v = doc_.get(elem, "scale");
    if (!v) return std::nullopt;
    if (auto vec = parse_vec3(*v, "vector")) return vec;
    char* end = nullptr;
    double d = std::strtod(v->c_str(), &end);
    if (end == v->c_str()) return std::nullopt;
    return Vec3{d, d, d};
}

bool TedScene::visible(int elem) const { return doc_.get(elem, "flags").value_or("").find("OIF_VISIBLE") != std::string::npos; }
bool TedScene::active(int elem) const { return doc_.get(elem, "flags").value_or("").find("OIF_ACTIVE") != std::string::npos; }

void TedScene::set_vector(int elem, const char* attr, const char* prefix, const Vec3& v) {
    auto cur = doc_.get(elem, attr);
    if (cur) {
        if (auto old = parse_vec3(*cur, prefix)) {
            bool same = true;
            for (int i = 0; i < 3; ++i) same = same && static_cast<float>((*old)[i]) == static_cast<float>(v[i]);
            if (same) return;
        }
    }
    doc_.set(elem, attr, std::string(prefix) + "(" + format_number(v[0]) + "," + format_number(v[1]) + "," + format_number(v[2]) + ")");
}

void TedScene::translate(int elem, const Vec3& delta) {
    auto p = position(elem);
    if (!p) throw std::runtime_error("element has no position");
    set_position(elem, {(*p)[0] + delta[0], (*p)[1] + delta[1], (*p)[2] + delta[2]});
}

void TedScene::set_flag(int elem, const char* flag, bool on) {
    std::string f = doc_.get(elem, "flags").value_or("");
    bool has = f.find(flag) != std::string::npos;
    if (has == on) return;
    if (on) f += (f.empty() || f.back() == ' ' ? "" : "|") + std::string(flag);
    else {
        size_t p = f.find(flag);
        size_t len = std::strlen(flag);
        if (p > 0 && f[p - 1] == '|') { --p; ++len; } else if (p + len < f.size() && f[p + len] == '|') ++len;
        f.erase(p, len);
    }
    doc_.set(elem, "flags", f);
}

int TedScene::duplicate(int elem) {
    int copy = -1;
    doc_ = doc_.duplicated_element(elem, &copy);
    return copy;
}

void TedScene::remove(int elem) { doc_ = doc_.removed_element(elem); }


namespace {
// Offset of the first byte of the line that holds `pos` (start of its indentation).
size_t line_start(const std::string& t, size_t pos) {
    while (pos > 0 && t[pos - 1] != '\n') --pos;
    return pos;
}
size_t line_end(const std::string& t, size_t pos) {      // just after the newline that ends the line containing pos
    size_t e = t.find('\n', pos);
    return e == std::string::npos ? t.size() : e + 1;
}
std::string indent_of(const std::string& t, size_t pos) {
    size_t a = line_start(t, pos), b = a;
    while (b < t.size() && (t[b] == ' ' || t[b] == '\t')) ++b;
    return t.substr(a, b - a);
}
} // namespace

bool TedScene::is_descendant(int elem, int ancestor) const {
    for (int p = doc_.elements()[elem].parent; p >= 0; p = doc_.elements()[p].parent)
        if (p == ancestor) return true;
    return false;
}

int TedScene::add_layer(int parent_layer, const std::string& name) {
    we::XmlDoc d = we::XmlDoc::parse(doc_.save());
    std::string t = d.original_text();
    int wrapper = -1;
    for (int c : d.elements()[parent_layer].children)
        if (d.elements()[c].name == "children") wrapper = c;
    if (wrapper < 0) throw std::runtime_error("layer has no children element");
    const auto& w = d.elements()[wrapper];
    // insertion point: start of the line that holds </children>
    size_t close = t.rfind("</children>", w.end_off);
    size_t at = line_start(t, close);
    std::string ind = indent_of(t, close) + "    ";
    std::string nl = t.find("\r\n") != std::string::npos ? "\r\n" : "\n";
    std::string snippet = ind + "<LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"" +
                          we::XmlDoc::encode_entities(name, '"') + "\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">" + nl +
                          ind + "    <children>" + nl + ind + "    </children>" + nl + ind + "</LayerInstance>" + nl;
    t.insert(at, snippet);
    doc_ = we::XmlDoc::parse(std::vector<uint8_t>(t.begin(), t.end()));
    int w2 = -1;
    for (int c : doc_.elements()[parent_layer].children)
        if (doc_.elements()[c].name == "children") w2 = c;
    return doc_.elements()[w2].children.back();
}

int TedScene::move_to_layer(int elem, int layer) {
    if (elem == layer || is_descendant(layer, elem)) throw std::runtime_error("cannot move a layer into itself");
    we::XmlDoc d = we::XmlDoc::parse(doc_.save());
    std::string t = d.original_text();
    const auto& e = d.elements()[elem];
    int sub = d.subtree_size(elem);
    size_t cut_a = line_start(t, e.start_off), cut_b = line_end(t, e.end_off - 1);
    std::string piece = t.substr(cut_a, cut_b - cut_a);
    int wrapper = -1;
    for (int c : d.elements()[layer].children)
        if (d.elements()[c].name == "children") wrapper = c;
    if (wrapper < 0) throw std::runtime_error("target layer has no children element");
    const auto& w = d.elements()[wrapper];
    size_t close = t.rfind("</children>", w.end_off);
    size_t at = line_start(t, close);
    // re-indent the piece to the target depth
    std::string old_ind = indent_of(t, e.start_off), new_ind = indent_of(t, close) + "    ";
    std::string moved;
    {
        size_t pos = 0;
        while (pos < piece.size()) {
            size_t eol = piece.find('\n', pos);
            size_t stop = eol == std::string::npos ? piece.size() : eol + 1;
            std::string line = piece.substr(pos, stop - pos);
            if (line.compare(0, old_ind.size(), old_ind) == 0) line = new_ind + line.substr(old_ind.size());
            moved += line;
            pos = stop;
        }
    }
    // apply the later edit first so the earlier offsets stay valid
    if (cut_a < at) { t.insert(at, moved); t.erase(cut_a, cut_b - cut_a); }
    else { t.erase(cut_a, cut_b - cut_a); t.insert(at, moved); }
    doc_ = we::XmlDoc::parse(std::vector<uint8_t>(t.begin(), t.end()));
    // the moved element is the last child of the target layer's children; the layer's own index shifts down when the cut was before it
    int new_layer = layer - (elem < layer ? sub : 0);
    int w2 = -1;
    for (int c : doc_.elements()[new_layer].children)
        if (doc_.elements()[c].name == "children") w2 = c;
    return doc_.elements()[w2].children.back();
}

std::vector<int> TedScene::find_all(const std::string& type) const {
    std::vector<int> out;
    for (size_t i = 0; i < doc_.elements().size(); ++i)
        if (doc_.elements()[i].name == type) out.push_back(static_cast<int>(i));
    return out;
}

std::string TedScene::layer_path(int elem) const {
    std::vector<std::string> names;
    for (int p = doc_.elements()[elem].parent; p >= 0; p = doc_.elements()[p].parent)
        if (is_layer(p) && doc_.elements()[p].name != "SCENE_GRAPH") names.push_back(name(p));
    std::string out;
    for (size_t i = names.size(); i-- > 0;) out += (out.empty() ? "" : "/") + names[i];
    return out;
}

} // namespace te
