#include "foundation/json.h"

#include <charconv>
#include <cmath>
#include <cstdio>
#include <stdexcept>

namespace wce {

namespace {

struct Parser {
    const std::string& s;
    size_t i = 0;

    [[noreturn]] void fail(const std::string& what) const { throw std::runtime_error("json: " + what + " at offset " + std::to_string(i)); }
    void ws() {
        while (i < s.size() && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r')) ++i;
    }
    bool eat(char c) {
        ws();
        if (i < s.size() && s[i] == c) { ++i; return true; }
        return false;
    }
    void expect(char c) {
        if (!eat(c)) fail(std::string("expected '") + c + "'");
    }
    bool literal(const char* w) {
        size_t n = std::char_traits<char>::length(w);
        if (s.compare(i, n, w) != 0) return false;
        i += n;
        return true;
    }

    unsigned hex4() {
        if (i + 4 > s.size()) fail("short \\u escape");
        unsigned v = 0;
        for (int k = 0; k < 4; ++k) {
            char c = s[i++];
            v <<= 4;
            if (c >= '0' && c <= '9') v |= unsigned(c - '0');
            else if (c >= 'a' && c <= 'f') v |= unsigned(c - 'a' + 10);
            else if (c >= 'A' && c <= 'F') v |= unsigned(c - 'A' + 10);
            else fail("bad \\u escape");
        }
        return v;
    }
    static void utf8(std::string& out, unsigned cp) {
        if (cp < 0x80) out += char(cp);
        else if (cp < 0x800) { out += char(0xC0 | (cp >> 6)); out += char(0x80 | (cp & 0x3F)); }
        else if (cp < 0x10000) { out += char(0xE0 | (cp >> 12)); out += char(0x80 | ((cp >> 6) & 0x3F)); out += char(0x80 | (cp & 0x3F)); }
        else { out += char(0xF0 | (cp >> 18)); out += char(0x80 | ((cp >> 12) & 0x3F)); out += char(0x80 | ((cp >> 6) & 0x3F)); out += char(0x80 | (cp & 0x3F)); }
    }

    std::string string() {
        expect('"');
        std::string out;
        for (;;) {
            if (i >= s.size()) fail("unterminated string");
            char c = s[i++];
            if (c == '"') return out;
            if (c != '\\') { out += c; continue; }
            if (i >= s.size()) fail("unterminated escape");
            switch (s[i++]) {
            case '"': out += '"'; break;
            case '\\': out += '\\'; break;
            case '/': out += '/'; break;
            case 'b': out += '\b'; break;
            case 'f': out += '\f'; break;
            case 'n': out += '\n'; break;
            case 'r': out += '\r'; break;
            case 't': out += '\t'; break;
            case 'u': {
                unsigned cp = hex4();
                if (cp >= 0xD800 && cp < 0xDC00 && s.compare(i, 2, "\\u") == 0) {
                    i += 2;
                    unsigned lo = hex4();
                    if (lo < 0xDC00 || lo > 0xDFFF) fail("bad surrogate pair");
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                }
                utf8(out, cp);
                break;
            }
            default: fail("bad escape");
            }
        }
    }

    Json value(int depth) {
        if (depth > 256) fail("nesting too deep");
        ws();
        if (i >= s.size()) fail("unexpected end");
        char c = s[i];
        if (c == '{') {
            ++i;
            Json::Object o;
            if (eat('}')) return Json(std::move(o));
            do {
                ws();
                std::string k = string();
                expect(':');
                o.emplace_back(std::move(k), value(depth + 1));
            } while (eat(','));
            expect('}');
            return Json(std::move(o));
        }
        if (c == '[') {
            ++i;
            Json::Array a;
            if (eat(']')) return Json(std::move(a));
            do a.push_back(value(depth + 1));
            while (eat(','));
            expect(']');
            return Json(std::move(a));
        }
        if (c == '"') return Json(string());
        if (literal("true")) return Json(true);
        if (literal("false")) return Json(false);
        if (literal("null")) return Json();
        double d = 0;
        auto r = std::from_chars(s.data() + i, s.data() + s.size(), d);
        if (r.ec != std::errc()) fail("bad value");
        i = size_t(r.ptr - s.data());
        return Json(d);
    }
};

void write_string(std::string& out, const std::string& s) {
    out += '"';
    for (unsigned char c : s) {
        switch (c) {
        case '"': out += "\\\""; break;
        case '\\': out += "\\\\"; break;
        case '\n': out += "\\n"; break;
        case '\r': out += "\\r"; break;
        case '\t': out += "\\t"; break;
        case '\b': out += "\\b"; break;
        case '\f': out += "\\f"; break;
        default:
            if (c < 0x20) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); out += b; }
            else out += char(c);
        }
    }
    out += '"';
}

void write(std::string& out, const Json& j) {
    if (j.is_null()) out += "null";
    else if (j.is_bool()) out += j.as_bool() ? "true" : "false";
    else if (j.is_number()) {
        double d = j.as_number();
        char b[40];
        if (std::isfinite(d) && d == std::floor(d) && std::fabs(d) < 9e15) std::snprintf(b, sizeof b, "%lld", static_cast<long long>(d));
        else std::snprintf(b, sizeof b, "%.17g", std::isfinite(d) ? d : 0.0);
        out += b;
    } else if (j.is_string()) write_string(out, j.as_string());
    else if (j.is_array()) {
        out += '[';
        bool first = true;
        for (const Json& e : j.as_array()) {
            if (!first) out += ',';
            first = false;
            write(out, e);
        }
        out += ']';
    } else {
        out += '{';
        bool first = true;
        for (const auto& [k, v] : j.as_object()) {
            if (!first) out += ',';
            first = false;
            write_string(out, k);
            out += ':';
            write(out, v);
        }
        out += '}';
    }
}

} // namespace

Json Json::parse(const std::string& text) {
    Parser p{text};
    Json v = p.value(0);
    p.ws();
    if (p.i != text.size()) p.fail("trailing characters");
    return v;
}

std::string Json::dump() const {
    std::string out;
    write(out, *this);
    return out;
}

bool Json::as_bool() const {
    if (!is_bool()) throw std::runtime_error("json: not a bool");
    return std::get<bool>(v_);
}
double Json::as_number() const {
    if (!is_number()) throw std::runtime_error("json: not a number");
    return std::get<double>(v_);
}
const std::string& Json::as_string() const {
    if (!is_string()) throw std::runtime_error("json: not a string");
    return std::get<std::string>(v_);
}
const Json::Array& Json::as_array() const {
    if (!is_array()) throw std::runtime_error("json: not an array");
    return std::get<Array>(v_);
}
const Json::Object& Json::as_object() const {
    if (!is_object()) throw std::runtime_error("json: not an object");
    return std::get<Object>(v_);
}

const Json* Json::find(const std::string& key) const {
    if (!is_object()) return nullptr;
    for (const auto& [k, v] : std::get<Object>(v_))
        if (k == key) return &v;
    return nullptr;
}
const Json* Json::at(size_t i) const {
    if (!is_array()) return nullptr;
    const Array& a = std::get<Array>(v_);
    return i < a.size() ? &a[i] : nullptr;
}
Json& Json::push(Json v) {
    if (!is_array()) throw std::runtime_error("json: push on a non-array");
    std::get<Array>(v_).push_back(std::move(v));
    return *this;
}
Json& Json::set(std::string key, Json v) {
    if (!is_object()) throw std::runtime_error("json: set on a non-object");
    for (auto& [k, e] : std::get<Object>(v_))
        if (k == key) { e = std::move(v); return *this; }
    std::get<Object>(v_).emplace_back(std::move(key), std::move(v));
    return *this;
}

} // namespace wce
