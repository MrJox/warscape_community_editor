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
