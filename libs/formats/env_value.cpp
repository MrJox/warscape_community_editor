#include "env_value.h"

#include <charconv>
#include <cstdio>

namespace we {

namespace {

bool parse_float(std::string_view s, float& out) {
    if (s.empty()) return false;
    auto r = std::from_chars(s.data(), s.data() + s.size(), out);
    return r.ec == std::errc() && r.ptr == s.data() + s.size();
}

std::vector<NumStyle> candidate_styles(std::string_view sample) {
    std::vector<NumStyle> c = {
        {NumStyle::Stripped, 6, false}, {NumStyle::Stripped, 6, true},
        {NumStyle::Shortest, 6, false}, {NumStyle::Shortest, 6, true}};
    // Fixed with the sample's own decimal count first.
    if (auto dot = sample.find('.'); dot != std::string_view::npos) {
        int n = int(sample.size() - dot - 1);
        if (n >= 1 && n <= 9) c.push_back({NumStyle::Fixed, n, false});
    }
    return c;
}

// Picks the first style that reproduces every component's text exactly.
NumStyle detect_style(const std::vector<std::string_view>& texts, const std::vector<float>& vals) {
    for (auto& texts_sample : texts) {
        for (const NumStyle& st : candidate_styles(texts_sample)) {
            bool all = true;
            for (size_t i = 0; i < vals.size() && all; ++i) all = format_number(vals[i], st) == texts[i];
            if (all) return st;
        }
    }
    return {};
}

} // namespace

std::string format_number(float v, const NumStyle& st) {
    char buf[64];
    std::string s;
    if (st.kind == NumStyle::Fixed) {
        std::snprintf(buf, sizeof buf, "%.*f", st.decimals, double(v));
        s = buf;
    } else {
        if (st.kind == NumStyle::Stripped) {
            std::snprintf(buf, sizeof buf, "%f", double(v));
            s = buf;
            if (s.find('.') != std::string::npos) {
                while (s.back() == '0') s.pop_back();
                if (s.back() == '.') s.pop_back();
            }
        } else {
            auto r = std::to_chars(buf, buf + sizeof buf, v);
            s.assign(buf, r.ptr);
        }
        if (s == "-0") s = "0";
        if (st.keep_one && s.find_first_of(".e") == std::string::npos) s += ".0";
    }
    return s;
}

EnvValue parse_value(std::string_view t) {
    EnvValue v;
    v.s = std::string(t);
    if (t == "true" || t == "false") {
        v.kind = ValueKind::Bool;
        v.b = t == "true";
        return v;
    }
    float x;
    if (long long n; t.find_first_of(".eE") == std::string_view::npos) {
        auto r = std::from_chars(t.data(), t.data() + t.size(), n);
        if (r.ec == std::errc() && r.ptr == t.data() + t.size()) {
            v.kind = ValueKind::Int;
            v.i = n;
            v.f = {float(n)};
            return v;
        }
    }
    if (parse_float(t, x)) {
        v.kind = t.find_first_of(".eE") == std::string_view::npos ? ValueKind::Int : ValueKind::Float;
        v.f = {x};
        v.style = detect_style({t}, v.f);
        return v;
    }
    auto open = t.find('(');
    if (open == std::string_view::npos || t.back() != ')') return v;
    std::string_view fn = t.substr(0, open), args = t.substr(open + 1, t.size() - open - 2);
    std::vector<float> comps;
    std::vector<std::string_view> texts;
    while (true) {
        auto comma = args.find(',');
        std::string_view a = args.substr(0, comma);
        if (!parse_float(a, x)) return v;
        comps.push_back(x);
        texts.push_back(a);
        if (comma == std::string_view::npos) break;
        args.remove_prefix(comma + 1);
    }
    struct { std::string_view name; ValueKind kind; size_t n; } table[] = {
        {"rgb", ValueKind::Rgb, 3}, {"argb", ValueKind::Argb, 4}, {"variance", ValueKind::Variance, 2}, {"euler", ValueKind::Euler, 3}};
    for (auto& e : table)
        if (fn == e.name && comps.size() == e.n) {
            v.kind = e.kind;
            v.f = comps;
            v.style = detect_style(texts, comps);
            return v;
        }
    return v;
}

std::string format_value(const EnvValue& v) {
    auto list = [&](const char* fn) {
        std::string s = std::string(fn) + "(";
        for (size_t i = 0; i < v.f.size(); ++i) s += (i ? "," : "") + format_number(v.f[i], v.style);
        return s + ")";
    };
    switch (v.kind) {
    case ValueKind::Bool: return v.b ? "true" : "false";
    case ValueKind::Int: return std::to_string(v.i);
    case ValueKind::Float: return format_number(v.f[0], v.style);
    case ValueKind::Rgb: return list("rgb");
    case ValueKind::Argb: return list("argb");
    case ValueKind::Variance: return list("variance");
    case ValueKind::Euler: return list("euler");
    case ValueKind::String: return v.s;
    }
    return v.s;
}

} // namespace we
