#include "tile/ted_scene.h"

#include <algorithm>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <stdexcept>

namespace wce {

std::string format_ted_number(double v) {
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

TedScene TedScene::parse(const std::vector<uint8_t>& bytes) { return parse(std::vector<uint8_t>(bytes)); }

TedScene TedScene::parse(std::vector<uint8_t>&& bytes) {
    TedScene s;
    s.doc_ = wce::XmlDoc::parse(std::move(bytes));
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
    doc_.set(elem, attr, std::string(prefix) + "(" + format_ted_number(v[0]) + "," + format_ted_number(v[1]) + "," + format_ted_number(v[2]) + ")");
}

void TedScene::translate(int elem, const Vec3& delta) {
    auto p = position(elem);
    if (!p) throw std::runtime_error("element has no position");
    set_position(elem, {(*p)[0] + delta[0], (*p)[1] + delta[1], (*p)[2] + delta[2]});
}

// TEd writes the flags as " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE" (a leading space, always in that order; " " when none is set; checked by toggling the eye and the
// cursor of a layer in TEd and diffing the saved .ted, docs/formats/tile_ted.md), so that a flag toggled here changes the same bytes TEd changes.
void TedScene::set_flag(int elem, const char* flag, bool on) {
    const std::string f = doc_.get(elem, "flags").value_or("");
    std::vector<std::string> tokens;
    for (size_t at = 0; at <= f.size();) {
        size_t e = f.find('|', at);
        if (e == std::string::npos) e = f.size();
        size_t b = at, en = e;
        while (b < en && f[b] == ' ') ++b;
        while (en > b && f[en - 1] == ' ') --en;
        if (en > b) tokens.push_back(f.substr(b, en - b));
        at = e + 1;
    }
    const auto it = std::find(tokens.begin(), tokens.end(), flag);
    const bool has = it != tokens.end();
    if (has == on) return;
    if (on) tokens.push_back(flag);
    else tokens.erase(it);
    static const char* kOrder[] = {"OIF_VISIBLE", "OIF_ACTIVE", "OIF_DRAGGABLE"};
    std::stable_sort(tokens.begin(), tokens.end(), [](const std::string& a, const std::string& b) {
        auto rank = [](const std::string& x) { for (int i = 0; i < 3; ++i) if (x == kOrder[i]) return i; return 3; };
        return rank(a) < rank(b);
    });
    std::string out = " ";
    for (size_t i = 0; i < tokens.size(); ++i) out += (i ? "|" : "") + tokens[i];
    doc_.set(elem, "flags", out);
}

int TedScene::duplicate(int elem) { return doc_.duplicate_element_inplace(elem); }

void TedScene::remove(int elem) { doc_.remove_element_inplace(elem); }


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

// A tile.ted without SCENE_GRAPH (e.g. custom_monte_rosa/aa) makes scene_graph() return -1; layer operations on it must fail cleanly.
void TedScene::require_element(int elem, const char* what) const {
    if (elem < 0 || elem >= int(doc_.elements().size())) throw std::runtime_error(std::string("no such ") + what + " (the tile has no SCENE_GRAPH?)");
}

int TedScene::add_layer(int parent_layer, const std::string& name) {
    require_element(parent_layer, "layer");
    doc_.bake_pending();
    const std::string& t = doc_.original_text();
    int wrapper = -1;
    for (int c : doc_.elements()[size_t(parent_layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") wrapper = c;
    if (wrapper < 0) throw std::runtime_error("layer has no children element");
    const auto& w = doc_.elements()[size_t(wrapper)];
    // insertion point: start of the line that holds </children>
    size_t close = t.rfind("</children>", w.end_off);
    size_t at = line_start(t, close);
    std::string ind = indent_of(t, close) + "    ";
    std::string nl = t.find("\r\n") != std::string::npos ? "\r\n" : "\n";
    std::string snippet = ind + "<LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" lf_offset=\"0\" position=\"vector(0,0,0)\" name=\"" +
                          wce::XmlDoc::encode_entities(name, '"') + "\" surface_mode=\"MSM_FREEFORM\" orientation=\"euler(0,0,0)\">" + nl +
                          ind + "    <children>" + nl + ind + "    </children>" + nl + ind + "</LayerInstance>" + nl;
    doc_.splice(at, 0, snippet);
    int w2 = -1;
    for (int c : doc_.elements()[size_t(parent_layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") w2 = c;
    return doc_.elements()[size_t(w2)].children.back();
}

int TedScene::move_to_layer(int elem, int layer) { return move_element(elem, layer, -1); }

// Moves `elem` (with its subtree) into the children of `layer`, in front of the child `before` (-1: at the end). Returns the moved element's new index.
int TedScene::move_element(int elem, int layer, int before) {
    require_element(elem, "element");
    require_element(layer, "layer");
    if (elem == layer || is_descendant(layer, elem)) throw std::runtime_error("cannot move a layer into itself");
    if (before >= 0 && (before == elem || is_descendant(before, elem))) throw std::runtime_error("cannot move an element in front of itself");
    doc_.bake_pending();
    const std::string& t = doc_.original_text();
    const auto& e = doc_.elements()[size_t(elem)];
    int sub = doc_.subtree_size(elem);
    size_t cut_a = line_start(t, e.start_off), cut_b = line_end(t, e.end_off - 1);
    std::string piece = t.substr(cut_a, cut_b - cut_a);
    int wrapper = -1;
    for (int c : doc_.elements()[size_t(layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") wrapper = c;
    if (wrapper < 0) throw std::runtime_error("target layer has no children element");
    const auto& w = doc_.elements()[size_t(wrapper)];
    // where it goes: in front of `before` (the start of its line), or in front of </children>; k = its position among the target's children afterwards
    size_t at, ref_off;
    int k = 0;
    {
        const auto& kids = w.children;
        const int pos_elem = int(std::find(kids.begin(), kids.end(), elem) - kids.begin());
        if (before >= 0) {
            const int pos_before = int(std::find(kids.begin(), kids.end(), before) - kids.begin());
            if (pos_before >= int(kids.size())) throw std::runtime_error("the element to put it in front of is not in that layer");
            k = pos_before - (pos_elem < pos_before ? 1 : 0);
            at = line_start(t, doc_.elements()[size_t(before)].start_off);
            ref_off = doc_.elements()[size_t(before)].start_off;
        } else {
            k = int(kids.size()) - (pos_elem < int(kids.size()) ? 1 : 0);
            const size_t close = t.rfind("</children>", w.end_off);
            at = line_start(t, close);
            ref_off = close;
        }
    }
    if (at == cut_a || at == cut_b) return elem;   // already there
    const std::string old_ind = indent_of(t, e.start_off);
    const std::string new_ind = before >= 0 ? indent_of(t, ref_off) : indent_of(t, ref_off) + "    ";
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
    if (cut_a < at) { doc_.splice(at, 0, moved); doc_.splice(cut_a, cut_b - cut_a, ""); }
    else { doc_.splice(cut_a, cut_b - cut_a, ""); doc_.splice(at, 0, moved); }
    // the layer's own index shifts down when the cut was before it
    int new_layer = layer - (elem < layer ? sub : 0);
    int w2 = -1;
    for (int c : doc_.elements()[size_t(new_layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") w2 = c;
    return doc_.elements()[size_t(w2)].children[size_t(k)];
}

// Moves several elements (with their subtrees) into `layer`, in front of the child `before` (-1: at the end), keeping their document order. All the cuts and the one
// insertion are computed against the same text and applied from the highest offset down, so none of them disturbs the offsets of the others. Returns the new
// indices of the moved elements in that order (elements below one that is also moved go with it and are not listed).
std::vector<int> TedScene::move_elements(std::vector<int> elems, int layer, int before) {
    require_element(layer, "layer");
    std::sort(elems.begin(), elems.end());
    elems.erase(std::unique(elems.begin(), elems.end()), elems.end());
    for (int e : elems) {
        require_element(e, "element");
        if (e == layer || is_descendant(layer, e)) throw std::runtime_error("cannot move a layer into itself");
    }
    // an element whose ancestor is moved too goes with it
    std::vector<int> top;
    for (int e : elems) {
        bool nested = false;
        for (int p = doc_.elements()[size_t(e)].parent; p >= 0 && !nested; p = doc_.elements()[size_t(p)].parent) nested = std::binary_search(elems.begin(), elems.end(), p);
        if (!nested) top.push_back(e);
    }
    elems = top;
    if (elems.empty()) return {};
    for (int e : elems)
        if (before >= 0 && (before == e || is_descendant(before, e))) throw std::runtime_error("cannot move an element in front of itself");
    doc_.bake_pending();
    const std::string& t = doc_.original_text();
    int wrapper = -1;
    for (int c : doc_.elements()[size_t(layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") wrapper = c;
    if (wrapper < 0) throw std::runtime_error("target layer has no children element");
    const auto& w = doc_.elements()[size_t(wrapper)];
    size_t at, ref_off;
    if (before >= 0) {
        if (std::find(w.children.begin(), w.children.end(), before) == w.children.end()) throw std::runtime_error("the element to put it in front of is not in that layer");
        ref_off = doc_.elements()[size_t(before)].start_off;
        at = line_start(t, ref_off);
    } else {
        ref_off = t.rfind("</children>", w.end_off);
        at = line_start(t, ref_off);
    }
    const std::string new_ind = before >= 0 ? indent_of(t, ref_off) : indent_of(t, ref_off) + "    ";
    struct Edit { size_t pos, len; std::string text; };
    std::vector<Edit> edits;
    std::string inserted;
    int cut_in_target = 0, cut_before_target = 0;
    for (int e : elems) {
        const auto& el = doc_.elements()[size_t(e)];
        const size_t a = line_start(t, el.start_off), b = line_end(t, el.end_off - 1);
        std::string piece = t.substr(a, b - a);
        const std::string old_ind = indent_of(t, el.start_off);
        size_t pos = 0;
        while (pos < piece.size()) {
            size_t eol = piece.find('\n', pos);
            size_t stop = eol == std::string::npos ? piece.size() : eol + 1;
            std::string line = piece.substr(pos, stop - pos);
            if (line.compare(0, old_ind.size(), old_ind) == 0) line = new_ind + line.substr(old_ind.size());
            inserted += line;
            pos = stop;
        }
        if (at > a && at < b) throw std::runtime_error("the target position is inside a moved element");
        edits.push_back({a, b - a, {}});
        if (std::find(w.children.begin(), w.children.end(), e) != w.children.end()) ++cut_in_target;
        if (e < layer) cut_before_target += doc_.subtree_size(e);
    }
    // position of the moved block among the target's children afterwards
    int k = 0;
    for (size_t i = 0; i < w.children.size(); ++i) {
        const int c = w.children[i];
        if (before >= 0 && c == before) break;
        if (!std::binary_search(elems.begin(), elems.end(), c)) ++k;
    }
    (void)cut_in_target;
    edits.push_back({at, 0, inserted});
    std::stable_sort(edits.begin(), edits.end(), [](const Edit& x, const Edit& y) { return x.pos > y.pos; });
    for (const Edit& ed : edits) doc_.splice(ed.pos, ed.len, ed.text);
    const int new_layer = layer - cut_before_target;
    int w2 = -1;
    for (int c : doc_.elements()[size_t(new_layer)].children)
        if (doc_.elements()[size_t(c)].name == "children") w2 = c;
    std::vector<int> out;
    for (size_t i = 0; i < elems.size(); ++i) out.push_back(doc_.elements()[size_t(w2)].children[size_t(k) + i]);
    return out;
}

std::vector<int> TedScene::polygon_points(int elem) const {
    std::vector<int> out;
    for (int c : doc_.elements()[elem].children)
        if (doc_.elements()[c].name == "points")
            for (int p : doc_.elements()[c].children)
                if (doc_.elements()[p].name == "point") out.push_back(p);
    return out;
}

std::optional<Vec3> TedScene::point_position(int point_elem) const {
    auto s = doc_.get(point_elem, "position");
    return s ? parse_vec3(*s, "") : std::nullopt;
}

double TedScene::polygon_area(int elem) const {
    auto pts = polygon_points(elem);
    double a = 0;
    for (size_t i = 0; i < pts.size(); ++i) {
        auto p = point_position(pts[i]), q = point_position(pts[(i + 1) % pts.size()]);
        if (!p || !q) return 0;
        a += (*p)[0] * (*q)[2] - (*q)[0] * (*p)[2];
    }
    return std::fabs(a) / 2;
}

static void refresh_area(TedScene& sc, int elem) {
    if (sc.doc().get(elem, "area")) sc.set_attr(elem, "area", format_ted_number(sc.polygon_area(elem)));
}

void TedScene::set_polygon_point(int elem, size_t index, const Vec3& local) {
    auto pts = polygon_points(elem);
    if (index >= pts.size()) throw std::runtime_error("point index out of range");
    doc_.set(pts[index], "position", format_general(local[0]) + "," + format_general(local[1]) + "," + format_general(local[2]));
    refresh_area(*this, elem);
}

void TedScene::insert_polygon_point(int elem, size_t after_index, const Vec3& local) {
    auto pts = polygon_points(elem);
    if (pts.empty() || after_index >= pts.size()) throw std::runtime_error("point index out of range");
    const int copy = doc_.duplicate_element_inplace(pts[after_index]);
    doc_.set(copy, "position", format_general(local[0]) + "," + format_general(local[1]) + "," + format_general(local[2]));
    refresh_area(*this, elem);
}

bool TedScene::remove_polygon_point(int elem, size_t index) {
    auto pts = polygon_points(elem);
    if (pts.size() <= 2 || index >= pts.size()) return false;
    doc_.remove_element_inplace(pts[index]);
    refresh_area(*this, elem);
    return true;
}

int TedScene::add_polygon(int layer, const std::string& type, const std::string& name, const std::vector<Vec3>& world_points, const std::map<std::string, std::string>& extra) {
    require_element(layer, "layer");
    if (world_points.size() < 2) throw std::runtime_error("a polygon needs at least 2 points");
    // attribute defaults per type (most common values in the shipped tiles)
    std::map<std::string, std::string> a = {
        {"flags", " OIF_VISIBLE|OIF_ACTIVE|OIF_DRAGGABLE"}, {"lf_offset", "0"}, {"line_colour", "argb(1.0,1.0,1.0,1.0)"},
        {"polygon_closed", "1"}, {"recenter_polygon", "0"}, {"polygon_shape", "PS_POLYGON"}, {"selected_line_colour", "argb(1.0,0.0,0.0,1.0)"},
        {"surface_mode", "MSM_LANDSCAPE"}, {"orientation", "euler(0,0,0)"}};
    if (type == "AI_HINT_POLYGON_INSTANCE") { a["hint_type"] = "AIH_COVER_LINE"; a["generated"] = "false"; a["is_generated"] = "false"; a["max_subdivisions"] = "100"; a["subdivision_length"] = "5"; a["polygon_closed"] = "0"; }
    if (type == "NOGO_REGION_POLYGON_INSTANCE") a["line_colour"] = "argb(0.65,0.0,0.0,1.0)";
    if (type == "SIEGE_AI_POLYGON_INSTANCE") a["polygon_closed"] = "0";
    for (auto& kv : extra) a[kv.first] = kv.second;
    // centroid position, local points
    Vec3 c{0, 0, 0};
    for (auto& p : world_points) for (int i = 0; i < 3; ++i) c[i] += p[i] / double(world_points.size());
    a["position"] = "vector(" + format_ted_number(c[0]) + "," + format_ted_number(c[1]) + "," + format_ted_number(c[2]) + ")";
    a["name"] = name;
    std::vector<Vec3> local;
    for (auto& p : world_points) local.push_back({p[0] - c[0], p[1] - c[1], p[2] - c[2]});
    double area = 0;
    for (size_t i = 0; i < local.size(); ++i) { const Vec3& p = local[i]; const Vec3& q = local[(i + 1) % local.size()]; area += p[0] * q[2] - q[0] * p[2]; }
    a["area"] = format_ted_number(std::fabs(area) / 2);

    doc_.bake_pending();
    const wce::XmlDoc& d = doc_;
    const std::string& t = d.original_text();
    int wrapper = -1;
    for (int ch : d.elements()[layer].children)
        if (d.elements()[ch].name == "children") wrapper = ch;
    if (wrapper < 0) throw std::runtime_error("layer has no children element");
    size_t close = t.rfind("</children>", d.elements()[wrapper].end_off);
    size_t at = line_start(t, close);
    std::string ind = indent_of(t, close) + "    ";
    std::string nl = t.find("\r\n") != std::string::npos ? "\r\n" : "\n";
    std::string snippet = ind + "<" + type;
    // TEd's attribute order (as in shipped tiles), then anything else
    static const char* order[] = {"flags", "recenter_polygon", "lf_offset", "line_colour", "subdivision_length", "max_subdivisions", "hint_type", "is_generated", "polygon_closed",
                                  "position", "selected_line_colour", "name", "polygon_shape", "area", "generated", "surface_mode", "orientation"};
    auto emit = [&](const std::string& k, const std::string& v) { snippet += " " + k + "=\"" + wce::XmlDoc::encode_entities(v, '"') + "\""; };
    for (const char* k : order) { auto it = a.find(k); if (it != a.end()) { emit(it->first, it->second); a.erase(it); } }
    for (auto& kv : a) emit(kv.first, kv.second);
    snippet += ">" + nl + ind + "    <points>" + nl;
    for (auto& p : local) snippet += ind + "        <point position=\"" + format_general(p[0]) + "," + format_general(p[1]) + "," + format_general(p[2]) + "\"/>" + nl;
    snippet += ind + "    </points>" + nl + ind + "</" + type + ">" + nl;
    doc_.splice(at, 0, snippet);
    int w2 = -1;
    for (int ch : doc_.elements()[layer].children)
        if (doc_.elements()[ch].name == "children") w2 = ch;
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

} // namespace wce
