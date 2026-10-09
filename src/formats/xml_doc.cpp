#include "xml_doc.h"

#include <algorithm>
#include <cctype>
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
    d.origin_ = bytes;
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

std::vector<uint8_t> XmlDoc::save() const { return encode_text(baked_text(), encoding_); }

std::string XmlDoc::baked_text() const {
    if (edits_.empty() && appended_values_.empty()) return text_;

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
    return out;
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

// ---- structural edits ----------------------------------------------------------------------------------------

namespace we {

namespace {

// The lines an element occupies. aligned: it starts a line (after indentation) and ends a line, so the whole
// lines [ls, le) can be cut or copied; at_eof: it starts a line and ends the file; otherwise only the element
// itself is touched.
struct Block { size_t ls, le; bool aligned; bool at_eof; };

Block block_of(const std::string& t, const XmlElement& el) {
    Block b{el.start_off, el.end_off, false, false};
    size_t ls = el.start_off;
    while (ls > 0 && (t[ls - 1] == ' ' || t[ls - 1] == '\t')) --ls;
    bool lead = ls == 0 || t[ls - 1] == '\n';
    size_t p = el.end_off;
    while (p < t.size() && (t[p] == ' ' || t[p] == '\t')) ++p;
    if (p < t.size() && t[p] == '\r') ++p;
    if (lead && p < t.size() && t[p] == '\n') { b.ls = ls; b.le = p + 1; b.aligned = true; }
    else if (lead && p >= t.size()) { b.ls = ls; b.at_eof = true; }
    return b;
}

std::string eol_of(const std::string& t) { return t.find("\r\n") != std::string::npos ? "\r\n" : "\n"; }

std::string line_indent(const std::string& t, size_t off) { // leading whitespace of the line holding off
    size_t ls = off;
    while (ls > 0 && t[ls - 1] != '\n') --ls;
    size_t e = ls;
    while (e < off && (t[e] == ' ' || t[e] == '\t')) ++e;
    return t.substr(ls, e - ls);
}

bool valid_name(std::string_view n) {
    if (n.empty() || std::isdigit(uint8_t(n[0])) || n[0] == '-' || n[0] == '.') return false;
    for (char c : n)
        if (!(std::isalnum(uint8_t(c)) || c == '_' || c == '-' || c == '.' || c == ':')) return false;
    return true;
}

} // namespace

XmlDoc XmlDoc::baked() const {
    XmlDoc b = parse(save());
    b.origin_ = origin_;
    b.structural_ = structural_;
    return b;
}

XmlDoc XmlDoc::rebuilt(std::string utf8) const {
    XmlDoc d = parse(encode_text(utf8, encoding_));
    d.origin_ = origin_;
    d.structural_ = structural_ + 1;
    return d;
}

int XmlDoc::subtree_size(int e) const {
    int n = 1;
    for (int c : elems_[e].children) n += subtree_size(c);
    return n;
}

XmlDoc XmlDoc::removed_element(int e) const {
    XmlDoc b = baked();
    const XmlElement& el = b.elems_[e];
    if (el.parent < 0 && b.roots_.size() == 1) throw std::runtime_error("cannot remove the only root element");
    Block k = block_of(b.text_, el);
    std::string t = b.text_;
    if (k.aligned) t.erase(k.ls, k.le - k.ls);
    else if (k.at_eof && k.ls > 0) { // also the line break that led to it
        size_t q = k.ls;
        if (t[q - 1] == '\n') --q;
        if (q > 0 && t[q - 1] == '\r') --q;
        t.erase(q);
    }
    else t.erase(el.start_off, el.end_off - el.start_off);
    return b.rebuilt(std::move(t));
}

XmlDoc XmlDoc::duplicated_element(int e, int* new_index) const {
    XmlDoc b = baked();
    const XmlElement& el = b.elems_[e];
    Block k = block_of(b.text_, el);
    std::string t = b.text_;
    if (k.aligned) t.insert(k.le, b.text_, k.ls, k.le - k.ls);
    else if (k.at_eof) t.insert(el.end_off, eol_of(b.text_) + b.text_.substr(k.ls, el.end_off - k.ls));
    else t.insert(el.end_off, b.text_, el.start_off, el.end_off - el.start_off);
    if (new_index) *new_index = e + b.subtree_size(e);
    return b.rebuilt(std::move(t));
}

XmlDoc XmlDoc::moved_element(int e, int dir) const {
    XmlDoc b = baked();
    const XmlElement& el = b.elems_[e];
    const std::vector<int>& sib = el.parent < 0 ? b.roots_ : b.elems_[el.parent].children;
    auto it = std::find(sib.begin(), sib.end(), e);
    if (dir < 0 ? it == sib.begin() : it + 1 == sib.end()) throw std::runtime_error("no sibling to swap with");
    int a = dir < 0 ? *(it - 1) : e, c = dir < 0 ? e : *(it + 1); // a comes first in the file
    Block ba = block_of(b.text_, b.elems_[a]), bc = block_of(b.text_, b.elems_[c]);
    if (!ba.aligned || !bc.aligned || ba.le != bc.ls) throw std::runtime_error("elements are not on separate adjacent lines");
    std::string t = b.text_.substr(0, ba.ls) + b.text_.substr(bc.ls, bc.le - bc.ls) + b.text_.substr(ba.ls, ba.le - ba.ls) +
                    b.text_.substr(bc.le);
    return b.rebuilt(std::move(t));
}

XmlDoc XmlDoc::with_child(int parent, std::string_view name, int* new_index) const {
    if (!valid_name(name)) throw std::runtime_error("invalid element name");
    XmlDoc b = baked();
    const XmlElement& p = b.elems_[parent];
    const std::string& t0 = b.text_;
    std::string eol = eol_of(t0), child = "<" + std::string(name) + "/>";
    std::string pin = line_indent(t0, p.start_off);
    std::string unit = pin.find('\t') != std::string::npos ? "\t" : "    ";
    std::string t = t0;
    if (!p.children.empty()) {
        const XmlElement& last = b.elems_[p.children.back()];
        Block k = block_of(t0, last);
        if (k.aligned) t.insert(k.le, line_indent(t0, last.start_off) + child + eol);
        else t.insert(last.end_off, child);
    } else if (p.self_closing) {
        t.replace(p.attrs_end, p.end_off - p.attrs_end, ">" + eol + pin + unit + child + eol + pin + "</" + p.name + ">");
    } else {
        size_t et = t0.rfind("</" + p.name, p.end_off - 1);
        for (size_t i = p.start_end; i < et; ++i)
            if (!std::isspace(uint8_t(t0[i]))) throw std::runtime_error("element has text content");
        t.replace(p.start_end, et - p.start_end, eol + pin + unit + child + eol + pin);
    }
    if (new_index) *new_index = parent + b.subtree_size(parent);
    return b.rebuilt(std::move(t));
}

XmlDoc XmlDoc::with_attr_added(int elem, std::string_view name, std::string_view value) const {
    if (!valid_name(name)) throw std::runtime_error("invalid attribute name");
    XmlDoc d = *this;
    d.set(elem, name, value);
    return d.baked().rebuilt(d.baked().text_);
}

XmlDoc XmlDoc::without_attr(int elem, size_t attr) const {
    XmlDoc b = baked();
    const XmlAttr& a = b.elems_[elem].attrs.at(attr);
    const std::string& t0 = b.text_;
    size_t end = a.value_off + a.value_len + 1; // closing quote
    size_t p = a.value_off - 1;                 // opening quote
    while (p > 0 && std::isspace(uint8_t(t0[p - 1]))) --p;
    if (p > 0 && t0[p - 1] == '=') --p;
    while (p > 0 && std::isspace(uint8_t(t0[p - 1]))) --p;
    p -= std::min(p, a.name.size());
    if (p > 0 && (t0[p - 1] == ' ' || t0[p - 1] == '\t')) --p; // the separator before the name
    std::string t = t0;
    t.erase(p, end - p);
    return b.rebuilt(std::move(t));
}

} // namespace we
