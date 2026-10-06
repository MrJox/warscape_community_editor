#include "xml_doc.h"

#include <stdexcept>

namespace we {

namespace {

bool is_space(char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; }
bool is_name_end(char c) { return is_space(c) || c == '/' || c == '>' || c == '=' || c == '\0'; }

[[noreturn]] void fail(size_t off, const std::string& what) {
    throw std::runtime_error("xml: " + what + " at offset " + std::to_string(off));
}

} // namespace

XmlDoc XmlDoc::parse(const std::vector<uint8_t>& bytes) {
    XmlDoc d;
    DecodedText dt = decode_text(bytes);
    d.text_ = std::move(dt.utf8);
    d.encoding_ = dt.encoding;
    const std::string& t = d.text_;
    const size_t n = t.size();
    auto at = [&](size_t i) { return i < n ? t[i] : '\0'; };
    auto skip_to = [&](size_t i, std::string_view end) {
        size_t p = t.find(end, i);
        if (p == std::string::npos) fail(i, "unterminated '" + std::string(end) + "'");
        return p + end.size();
    };

    std::vector<int> stack;
    size_t i = 0;
    while (i < n) {
        size_t lt = t.find('<', i);
        if (lt == std::string::npos) break;
        i = lt;
        if (t.compare(i, 4, "<!--") == 0) { i = skip_to(i + 4, "-->"); continue; }
        if (t.compare(i, 9, "<![CDATA[") == 0) { i = skip_to(i + 9, "]]>"); continue; }
        if (at(i + 1) == '?') { i = skip_to(i + 2, "?>"); continue; }
        if (at(i + 1) == '!') { i = skip_to(i + 2, ">"); continue; }

        if (at(i + 1) == '/') { // end tag
            size_t s = i + 2, e = s;
            while (!is_name_end(at(e))) ++e;
            std::string name = t.substr(s, e - s);
            size_t close = t.find('>', e);
            if (close == std::string::npos) fail(i, "unterminated end tag");
            // Tolerate unbalanced files: pop until the matching name.
            int match = -1;
            for (int k = int(stack.size()) - 1; k >= 0; --k)
                if (d.elems_[stack[k]].name == name) { match = k; break; }
            if (match < 0) fail(i, "end tag </" + name + "> without start");
            while (int(stack.size()) > match) {
                d.elems_[stack.back()].end_off = close + 1;
                stack.pop_back();
            }
            i = close + 1;
            continue;
        }

        // start tag
        XmlElement el;
        el.start_off = i;
        size_t s = i + 1, e = s;
        while (!is_name_end(at(e))) ++e;
        if (e == s) fail(i, "empty element name");
        el.name = t.substr(s, e - s);
        size_t p = e;
        for (;;) {
            while (is_space(at(p))) ++p;
            char c = at(p);
            if (c == '\0') fail(i, "unterminated start tag");
            if (c == '/' && at(p + 1) == '>') { el.attrs_end = p; el.self_closing = true; p += 2; break; }
            if (c == '>') { el.attrs_end = p; p += 1; break; }
            size_t ns = p;
            while (!is_name_end(at(p))) ++p;
            if (p == ns) fail(p, "bad attribute");
            XmlAttr a;
            a.name = t.substr(ns, p - ns);
            while (is_space(at(p))) ++p;
            if (at(p) != '=') fail(p, "expected '=' after " + a.name);
            ++p;
            while (is_space(at(p))) ++p;
            char q = at(p);
            if (q != '\'' && q != '"') fail(p, "expected quote for " + a.name);
            size_t ve = t.find(q, p + 1);
            if (ve == std::string::npos) fail(p, "unterminated value");
            a.quote = q;
            a.value_off = p + 1;
            a.value_len = ve - (p + 1);
            el.attrs.push_back(std::move(a));
            p = ve + 1;
        }
        el.start_end = p;
        el.end_off = p;
        el.parent = stack.empty() ? -1 : stack.back();
        int idx = int(d.elems_.size());
        bool self_closing = el.self_closing;
        d.elems_.push_back(std::move(el));
        if (d.elems_[idx].parent < 0) d.roots_.push_back(idx);
        else d.elems_[d.elems_[idx].parent].children.push_back(idx);
        if (!self_closing) stack.push_back(idx);
        i = p;
    }
    if (!stack.empty()) fail(n, "unclosed <" + d.elems_[stack.back()].name + ">");
    return d;
}

std::vector<uint8_t> XmlDoc::save() const {
    if (edits_.empty() && appended_values_.empty()) return encode_text(text_, encoding_);

    // Merge value edits and attribute insertions into one ordered list of span replacements.
    std::map<size_t, Edit> ops = edits_;
    for (auto& [key, raw] : appended_values_) {
        const XmlElement& el = elems_[key.first];
        const XmlAttr& a = el.attrs[key.second];
        Edit& op = ops[el.attrs_end]; // zero-length insertion before '/>' or '>'
        op.len = 0;
        op.text += " " + a.name + "=" + a.quote + raw + a.quote;
    }
    std::string out;
    out.reserve(text_.size() + 256);
    size_t pos = 0;
    for (auto& [off, op] : ops) {
        out.append(text_, pos, off - pos);
        out += op.text;
        pos = off + op.len;
    }
    out.append(text_, pos, std::string::npos);
    return encode_text(out, encoding_);
}

std::string_view XmlDoc::attr_raw(int elem, size_t attr) const {
    const XmlAttr& a = elems_[elem].attrs[attr];
    if (a.appended) return appended_values_.at({elem, attr});
    auto it = edits_.find(a.value_off);
    if (it != edits_.end()) return it->second.text;
    return std::string_view(text_).substr(a.value_off, a.value_len);
}

std::string_view XmlDoc::original_raw(int elem, size_t attr) const {
    const XmlAttr& a = elems_[elem].attrs[attr];
    if (a.appended) return {};
    return std::string_view(text_).substr(a.value_off, a.value_len);
}

const XmlAttr* XmlDoc::find_attr(int elem, std::string_view name, size_t* index) const {
    const auto& attrs = elems_[elem].attrs;
    for (size_t k = 0; k < attrs.size(); ++k)
        if (attrs[k].name == name) {
            if (index) *index = k;
            return &attrs[k];
        }
    return nullptr;
}

std::optional<std::string> XmlDoc::get(int elem, std::string_view name) const {
    size_t k;
    if (!find_attr(elem, name, &k)) return std::nullopt;
    return decode_entities(attr_raw(elem, k));
}

void XmlDoc::set_attr_raw(int elem, size_t attr, std::string raw) {
    const XmlAttr& a = elems_[elem].attrs[attr];
    if (a.appended) { appended_values_[{elem, attr}] = std::move(raw); return; }
    if (std::string_view(text_).substr(a.value_off, a.value_len) == raw) edits_.erase(a.value_off);
    else edits_[a.value_off] = Edit{a.value_len, std::move(raw)};
}

void XmlDoc::set(int elem, std::string_view name, std::string_view value) {
    size_t k;
    if (const XmlAttr* a = find_attr(elem, name, &k)) {
        set_attr_raw(elem, k, encode_entities(value, a->quote));
        return;
    }
    XmlElement& el = elems_[elem];
    XmlAttr a;
    a.name = std::string(name);
    a.quote = el.attrs.empty() ? '\'' : el.attrs.back().quote;
    a.appended = true;
    a.value_off = el.attrs_end;
    appended_values_[{elem, el.attrs.size()}] = encode_entities(value, a.quote);
    el.attrs.push_back(std::move(a));
}

void XmlDoc::revert_all() {
    edits_.clear();
    appended_values_.clear();
    for (auto& el : elems_)
        while (!el.attrs.empty() && el.attrs.back().appended) el.attrs.pop_back();
}

std::string XmlDoc::path_of(int elem) const {
    std::string p;
    for (int e = elem; e >= 0; e = elems_[e].parent) p = elems_[e].name + (p.empty() ? "" : "/" + p);
    return p;
}

std::string XmlDoc::decode_entities(std::string_view raw) {
    std::string out;
    out.reserve(raw.size());
    for (size_t i = 0; i < raw.size(); ++i) {
        if (raw[i] != '&') { out += raw[i]; continue; }
        size_t semi = raw.find(';', i);
        if (semi == std::string_view::npos) { out += raw[i]; continue; }
        std::string_view ent = raw.substr(i + 1, semi - i - 1);
        if (ent == "amp") out += '&';
        else if (ent == "lt") out += '<';
        else if (ent == "gt") out += '>';
        else if (ent == "quot") out += '"';
        else if (ent == "apos") out += '\'';
        else { out.append(raw.substr(i, semi - i + 1)); }
        i = semi;
    }
    return out;
}

std::string XmlDoc::encode_entities(std::string_view text, char quote) {
    std::string out;
    for (char c : text) {
        if (c == '&') out += "&amp;";
        else if (c == '<') out += "&lt;";
        else if (c == quote) out += quote == '"' ? "&quot;" : "&apos;";
        else out += c;
    }
    return out;
}

} // namespace we
