#include "foundation/profile.h"
#include "foundation/thread_pool.h"
#include "formats/xml_doc.h"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <stdexcept>

namespace wce {

namespace {

bool is_space(char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; }
bool is_name_end(char c) { return is_space(c) || c == '/' || c == '>' || c == '=' || c == '\0'; }

[[noreturn]] void fail(size_t off, const std::string& what) {
    throw std::runtime_error("xml: " + what + " at offset " + std::to_string(off));
}

} // namespace

XmlDoc XmlDoc::parse(const std::vector<uint8_t>& bytes) { return parse(std::vector<uint8_t>(bytes)); }

namespace {

// Reads the start tag at t[at] ('<'): name, attributes, where the attribute list ends and where the tag does. Returns the offset after the tag.
size_t parse_start_tag(const std::string& t, size_t n, size_t at_off, XmlElement& el, std::vector<XmlAttr>& scratch) {
    auto at = [&](size_t i) { return i < n ? t[i] : '\0'; };
    el.start_off = at_off;
    size_t s = at_off + 1, e = s;
    while (!is_name_end(at(e))) ++e;
    if (e == s) fail(at_off, "empty element name");
    el.name = t.substr(s, e - s);
    el.self_closing = false;
    size_t p = e;
    for (;;) {
        while (is_space(at(p))) ++p;
        char c = at(p);
        if (c == '\0') fail(at_off, "unterminated start tag");
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
        scratch.push_back(std::move(a));
        p = ve + 1;
    }
    el.attrs.assign(std::make_move_iterator(scratch.begin()), std::make_move_iterator(scratch.end()));
    scratch.clear();
    el.start_end = p;
    el.end_off = p;
    return p;
}

// What a chunk of a bigger document leaves open for its neighbours: end tags that close elements started before the chunk (`ext`, with the number of top-level
// elements read before them) and the elements still open at its end (`open`, outermost first).
struct ChunkTail {
    struct Close { size_t after_tops; std::string name; size_t end; };
    std::vector<Close> ext;
    std::vector<int> open;
};

// Reads the elements of t[begin, end) into `elems` (indices continue from elems.size(); top-level ones get parent -1 and are listed in `roots`). Returns the
// offset after the last construct read. With a `tail` the range may start and stop in the middle of the element tree.
size_t parse_range(const std::string& t, size_t begin, size_t n, std::vector<XmlElement>& elems, std::vector<int>& roots, ChunkTail* tail = nullptr) {
    auto at = [&](size_t i) { return i < n ? t[i] : '\0'; };
    auto skip_to = [&](size_t i, std::string_view end) {
        size_t p = t.find(end, i);
        if (p == std::string::npos) fail(i, "unterminated '" + std::string(end) + "'");
        return p + end.size();
    };
    std::vector<int> stack;
    std::vector<XmlAttr> scratch;   // the attributes of the tag being read: moved into an exactly sized vector (no growth steps per element)
    size_t i = begin;
    while (i < n) {
        size_t lt = t.find('<', i);
        if (lt == std::string::npos || lt >= n) break;
        i = lt;
        if (t.compare(i, 4, "<!--") == 0) { i = skip_to(i + 4, "-->"); continue; }
        if (t.compare(i, 9, "<![CDATA[") == 0) { i = skip_to(i + 9, "]]>"); continue; }
        if (at(i + 1) == '?') { i = skip_to(i + 2, "?>"); continue; }
        if (at(i + 1) == '!') { i = skip_to(i + 2, ">"); continue; }

        if (at(i + 1) == '/') { // end tag
            size_t s = i + 2, e = s;
            while (!is_name_end(at(e))) ++e;
            const std::string_view name(t.data() + s, e - s);
            size_t close = t.find('>', e);
            if (close == std::string::npos) fail(i, "unterminated end tag");
            // Tolerate unbalanced files: pop until the matching name.
            int match = -1;
            for (int k = int(stack.size()) - 1; k >= 0; --k)
                if (elems[size_t(stack[size_t(k)])].name == name) { match = k; break; }
            if (match < 0) {
                if (tail && stack.empty()) {   // closes an element of an earlier chunk: the merge resolves it
                    tail->ext.push_back({roots.size(), std::string(name), close + 1});
                    i = close + 1;
                    continue;
                }
                fail(i, "end tag </" + std::string(name) + "> without start");
            }
            while (int(stack.size()) > match) {
                elems[size_t(stack.back())].end_off = close + 1;
                stack.pop_back();
            }
            i = close + 1;
            continue;
        }

        XmlElement el;
        const size_t p = parse_start_tag(t, n, i, el, scratch);
        el.parent = stack.empty() ? -1 : stack.back();
        int idx = int(elems.size());
        bool self_closing = el.self_closing;
        elems.push_back(std::move(el));
        if (elems[size_t(idx)].parent < 0) roots.push_back(idx);
        else elems[size_t(elems[size_t(idx)].parent)].children.push_back(idx);
        if (!self_closing) stack.push_back(idx);
        i = p;
    }
    if (!stack.empty()) {
        if (!tail) fail(n, "unclosed <" + elems[size_t(stack.back())].name + ">");
        tail->open = stack;
    }
    return i;
}

// Reads the whole text in pieces side by side. The pieces start at '<' after a line break; the merge checks that no construct (comment, CDATA, attribute value)
// crosses a piece boundary and that every end tag that leaves its piece closes the innermost open element, which is true for every well formed file. Anything
// else returns false and the caller reads the text in one go (which also gives the proper error message).
bool parse_in_pieces(const std::string& t, size_t pieces, std::vector<XmlElement>& out, std::vector<int>& roots) {
    const size_t size = t.size();
    std::vector<size_t> cut{0};
    for (size_t k = 1; k < pieces; ++k) {
        size_t p = std::max(cut.back() + 1, size / pieces * k);
        for (;;) {
            p = t.find('\n', p);
            if (p == std::string::npos) break;
            size_t q = p + 1;
            while (q < size && (t[q] == ' ' || t[q] == '\t')) ++q;
            if (q < size && t[q] == '<') { p = q; break; }
            p = q;
        }
        if (p == std::string::npos) break;
        cut.push_back(p);
    }
    cut.push_back(size);
    const size_t count = cut.size() - 1;
    if (count < 2) return false;
    struct Piece { std::vector<XmlElement> elems; std::vector<int> tops; ChunkTail tail; size_t base = 0; };
    std::vector<Piece> piece(count);
    std::atomic<bool> bad{false};
    compute_pool().parallel_for(count, 1, [&](size_t b, size_t e, int) {
        for (size_t k = b; k < e; ++k) {
            Piece& pc = piece[k];
            try {
                pc.elems.reserve((cut[k + 1] - cut[k]) / 256 + 16);
                const size_t end = parse_range(t, cut[k], cut[k + 1], pc.elems, pc.tops, &pc.tail);
                if (end > cut[k + 1]) bad = true;   // a construct reaches over the boundary
            } catch (const std::exception&) {
                bad = true;
            }
        }
    });
    if (bad) return false;
    size_t total = 0;
    for (Piece& pc : piece) { pc.base = total; total += pc.elems.size(); }
    out.clear();
    out.resize(total);
    compute_pool().parallel_for(count, 1, [&](size_t b, size_t e, int) {
        for (size_t k = b; k < e; ++k) {
            Piece& pc = piece[k];
            const int base = int(pc.base);
            for (size_t i = 0; i < pc.elems.size(); ++i) {
                XmlElement& x = pc.elems[i];
                if (x.parent >= 0) x.parent += base;
                for (int& c : x.children) c += base;
                out[pc.base + i] = std::move(x);
            }
        }
    });
    // the elements that sit at a boundary: parents from the open elements of the pieces before, and the end tags that leave a piece
    std::vector<int> open;
    for (Piece& pc : piece) {
        size_t ti = 0;
        auto open_top = [&](int local) {
            const int g = int(pc.base) + local;
            const int parent = open.empty() ? -1 : open.back();
            out[size_t(g)].parent = parent;
            if (parent < 0) roots.push_back(g);
            else out[size_t(parent)].children.push_back(g);
        };
        for (const ChunkTail::Close& c : pc.tail.ext) {
            while (ti < c.after_tops) open_top(pc.tops[ti++]);
            if (open.empty() || out[size_t(open.back())].name != c.name) return false;
            out[size_t(open.back())].end_off = c.end;
            open.pop_back();
        }
        while (ti < pc.tops.size()) open_top(pc.tops[ti++]);
        for (int l : pc.tail.open) open.push_back(int(pc.base) + l);
    }
    return open.empty();
}

} // namespace

XmlDoc XmlDoc::parse(std::vector<uint8_t>&& bytes, size_t parallel_min) {
    XmlDoc d;
    d.origin_ = std::move(bytes);
    {
        WCE_PROF("xml.parse.decode");
        DecodedText dt = decode_text(d.origin_);
        d.text_ = std::move(dt.utf8);
        d.encoding_ = dt.encoding;
    }
    WCE_PROF("xml.parse.scan");
    if (d.text_.size() >= parallel_min) {
        const size_t pieces = std::clamp<size_t>(d.text_.size() / std::max<size_t>(parallel_min / 4, 1024), 2, 32);
        if (parse_in_pieces(d.text_, pieces, d.elems_, d.roots_)) return d;
        d.elems_.clear();
        d.roots_.clear();
    }
    d.elems_.reserve(d.text_.size() / 256 + 16);
    parse_range(d.text_, 0, d.text_.size(), d.elems_, d.roots_);
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

} // namespace wce

// ---- structural edits ----------------------------------------------------------------------------------------

namespace wce {

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

// ---- in-place edits

void XmlDoc::reparse_in_place() {
    XmlDoc fresh;
    fresh.elems_.reserve(text_.size() / 256 + 16);
    parse_range(text_, 0, text_.size(), fresh.elems_, fresh.roots_);
    elems_ = std::move(fresh.elems_);
    roots_ = std::move(fresh.roots_);
}

// A replacement of whole elements (removed ones lie completely inside [pos, pos + remove_len); the inserted text holds whole elements) below one parent element.
bool XmlDoc::patch_elements(size_t pos, size_t remove_len, const std::string& insert) {
    const size_t end_pos = pos + remove_len;
    const int n = int(elems_.size());
    const int a = int(std::partition_point(elems_.begin(), elems_.end(), [&](const XmlElement& e) { return e.start_off < pos; }) - elems_.begin());
    int b = a;
    while (b < n && elems_[size_t(b)].start_off < end_pos) {
        if (elems_[size_t(b)].end_off > end_pos) return false;
        ++b;
    }
    int parent = -1;
    for (int c = a - 1; c >= 0; c = elems_[size_t(c)].parent) {
        const XmlElement& e = elems_[size_t(c)];
        if (!e.self_closing && e.start_end <= pos && pos < e.end_off && end_pos <= e.end_off) { parent = c; break; }
    }
    if (parent < 0) return false;
    for (int i = a; i < b; ++i) {
        const int p = elems_[size_t(i)].parent;
        if (p != parent && !(p >= a && p < b)) return false;
    }
    std::vector<XmlElement> frag;
    std::vector<int> froots;
    if (!insert.empty()) parse_range(insert, 0, insert.size(), frag, froots);   // throws on malformed text
    const long long delta = (long long)insert.size() - (long long)remove_len;
    const int m = b - a, k = int(frag.size());
    auto map = [&](int i) { return i < a ? i : i - m + k; };   // new index of an old element outside the removed range
    // old elements: indices, offsets
    for (int i = 0; i < n; ++i) {
        if (i >= a && i < b) continue;
        XmlElement& e = elems_[size_t(i)];
        if (e.parent >= 0) e.parent = map(e.parent);
        if (i == parent) {
            std::vector<int> kids;
            kids.reserve(e.children.size() + size_t(k));
            bool placed = false;
            for (int c : e.children) {
                if (c >= a && c < b) continue;
                if (!placed && c >= b) {
                    for (int r : froots) kids.push_back(a + r);
                    placed = true;
                }
                kids.push_back(map(c));
            }
            if (!placed)
                for (int r : froots) kids.push_back(a + r);
            e.children = std::move(kids);
        } else {
            for (int& c : e.children) c = map(c);
        }
        if (i >= b) {
            e.start_off = size_t(ptrdiff_t(e.start_off) + ptrdiff_t(delta));
            e.attrs_end = size_t(ptrdiff_t(e.attrs_end) + ptrdiff_t(delta));
            e.start_end = size_t(ptrdiff_t(e.start_end) + ptrdiff_t(delta));
            e.end_off = size_t(ptrdiff_t(e.end_off) + ptrdiff_t(delta));
            for (XmlAttr& at : e.attrs) at.value_off = size_t(ptrdiff_t(at.value_off) + ptrdiff_t(delta));
        }
    }
    for (int c = parent; c >= 0; c = elems_[size_t(c)].parent) {   // the containers got longer or shorter
        XmlElement& e = elems_[size_t(c)];
        e.end_off = size_t(ptrdiff_t(e.end_off) + ptrdiff_t(delta));
    }
    for (XmlElement& e : frag) {
        e.start_off += pos;
        e.attrs_end += pos;
        e.start_end += pos;
        e.end_off += pos;
        for (XmlAttr& at : e.attrs) at.value_off += pos;
        e.parent = e.parent < 0 ? parent : a + e.parent;
        for (int& c : e.children) c += a;
    }
    elems_.erase(elems_.begin() + a, elems_.begin() + b);
    elems_.insert(elems_.begin() + a, std::make_move_iterator(frag.begin()), std::make_move_iterator(frag.end()));
    // the roots after the patch (a root element is never inside the replaced range here: its parent would be -1)
    for (int& r : roots_) r = map(r);
    return true;
}

// A replacement inside the start tag of one element (an attribute value, an added attribute): that tag is read again, everything after shifts.
bool XmlDoc::patch_start_tag(size_t pos, size_t remove_len, const std::string& insert) {
    const int a = int(std::partition_point(elems_.begin(), elems_.end(), [&](const XmlElement& e) { return e.start_off < pos; }) - elems_.begin());
    const int c = a - 1;
    if (c < 0) return false;
    XmlElement& el = elems_[size_t(c)];
    if (pos >= el.start_end || pos + remove_len > el.start_end) return false;   // (an insertion right after the tag is content, not part of the tag)
    const long long delta = (long long)insert.size() - (long long)remove_len;
    // the tag as it will read: text_ is still the old text here
    std::string tag = text_.substr(el.start_off, el.start_end - el.start_off);
    tag.replace(pos - el.start_off, remove_len, insert);
    XmlElement fresh;
    std::vector<XmlAttr> scratch;
    const size_t after = parse_start_tag(tag, tag.size(), 0, fresh, scratch);
    if (after != tag.size() || fresh.self_closing != el.self_closing || fresh.name != el.name) return false;
    el.attrs = std::move(fresh.attrs);
    for (XmlAttr& at : el.attrs) at.value_off += el.start_off;
    el.attrs_end = fresh.attrs_end + el.start_off;
    el.start_end = fresh.start_end + el.start_off;
    el.end_off = size_t(ptrdiff_t(el.end_off) + ptrdiff_t(delta));
    for (size_t i = size_t(a); i < elems_.size(); ++i) {   // everything after the tag, children included
        XmlElement& e = elems_[i];
        e.start_off = size_t(ptrdiff_t(e.start_off) + ptrdiff_t(delta));
        e.attrs_end = size_t(ptrdiff_t(e.attrs_end) + ptrdiff_t(delta));
        e.start_end = size_t(ptrdiff_t(e.start_end) + ptrdiff_t(delta));
        e.end_off = size_t(ptrdiff_t(e.end_off) + ptrdiff_t(delta));
        for (XmlAttr& at : e.attrs) at.value_off = size_t(ptrdiff_t(at.value_off) + ptrdiff_t(delta));
    }
    for (int p = el.parent; p >= 0; p = elems_[size_t(p)].parent) elems_[size_t(p)].end_off = size_t(ptrdiff_t(elems_[size_t(p)].end_off) + ptrdiff_t(delta));
    return true;
}

bool XmlDoc::replace_text(size_t pos, size_t remove_len, std::string_view insert, bool structural) {
    WCE_PROF("xml.replace_text");
    if (pos > text_.size() || remove_len > text_.size() - pos) throw std::runtime_error("xml: splice outside the text");
    const std::string ins(insert);
    bool patched = false;
    if (has_pending_edits()) throw std::runtime_error("xml: splice with pending attribute edits");
    try {
        WCE_PROF("xml.patch_table");
        patched = patch_start_tag(pos, remove_len, ins) || patch_elements(pos, remove_len, ins);
    } catch (const std::exception&) {
        patched = false;
    }
    text_.replace(pos, remove_len, ins);
    if (!patched) reparse_in_place();   // the change is neither whole elements nor inside one tag: read the document again
    structural_ += structural ? 1 : 0;
    return patched;
}

XmlSplice XmlDoc::splice(size_t pos, size_t remove_len, std::string_view insert) {
    XmlSplice s;
    s.pos = pos;
    s.removed = text_.substr(pos, remove_len);
    s.inserted = std::string(insert);
    replace_text(pos, remove_len, insert, true);
    journal_.push_back(s);
    return s;
}

void XmlDoc::apply_splice(const XmlSplice& s) {
    const size_t mark = journal_.size();
    bake_pending();
    replace_text(s.pos, s.removed.size(), s.inserted, true);
    journal_.resize(mark);
}

void XmlDoc::unapply_splice(const XmlSplice& s) {
    const size_t mark = journal_.size();
    bake_pending();
    replace_text(s.pos, s.inserted.size(), s.removed, false);
    structural_ = std::max(0, structural_ - 1);
    journal_.resize(mark);
}

// Pending value edits go into the text one by one, last first (so the offsets of the earlier ones stay valid); added attributes are inserted before the tag's end.
void XmlDoc::bake_pending() {
    if (!has_pending_edits()) return;
    std::vector<std::pair<size_t, Edit>> value_edits(edits_.begin(), edits_.end());
    std::vector<std::pair<std::pair<int, size_t>, std::string>> added(appended_values_.begin(), appended_values_.end());
    struct Op {
        size_t pos;
        size_t remove_len;
        std::string text;
        int elem = -1;           // for an added attribute: the element, whose attribute becomes a plain one
        size_t attr = 0;
    };
    std::vector<Op> ops;
    for (auto& [off, ed] : value_edits) ops.push_back({off, ed.len, ed.text});
    // several added attributes of one element go in as one insertion, in the order they were added
    std::map<int, std::string> insert_text;
    for (auto& [key, raw] : added) {
        const XmlAttr& at = elems_[size_t(key.first)].attrs[key.second];
        insert_text[key.first] += " " + at.name + "=" + at.quote + raw + at.quote;
    }
    for (auto& [elem, text] : insert_text) ops.push_back({elems_[size_t(elem)].attrs_end, 0, text, elem});
    std::sort(ops.begin(), ops.end(), [](const Op& x, const Op& y) { return x.pos > y.pos; });
    // the document is edited from here on: the pending maps are consumed (the added attributes are re-read from the new tag text)
    edits_.clear();
    appended_values_.clear();
    for (XmlElement& el : elems_)
        while (!el.attrs.empty() && el.attrs.back().appended) el.attrs.pop_back();   // gone from the table, back with their text
    for (const Op& op : ops) {
        XmlSplice s;
        s.pos = op.pos;
        s.removed = text_.substr(op.pos, op.remove_len);
        s.inserted = op.text;
        replace_text(op.pos, op.remove_len, op.text, false);
        journal_.push_back(std::move(s));
    }
}

void XmlDoc::remove_element_inplace(int e) {
    bake_pending();
    const XmlElement& el = elems_[size_t(e)];
    if (el.parent < 0 && roots_.size() == 1) throw std::runtime_error("cannot remove the only root element");
    Block k = block_of(text_, el);
    if (k.aligned) splice(k.ls, k.le - k.ls, "");
    else if (k.at_eof && k.ls > 0) {   // also the line break that led to it
        size_t q = k.ls;
        if (text_[q - 1] == '\n') --q;
        if (q > 0 && text_[q - 1] == '\r') --q;
        splice(q, text_.size() - q, "");
    } else {
        splice(el.start_off, el.end_off - el.start_off, "");
    }
}

int XmlDoc::duplicate_element_inplace(int e) {
    bake_pending();
    const XmlElement& el = elems_[size_t(e)];
    Block k = block_of(text_, el);
    const int new_index = e + subtree_size(e);
    if (k.aligned) splice(k.le, 0, std::string_view(text_).substr(k.ls, k.le - k.ls));
    else if (k.at_eof) splice(el.end_off, 0, eol_of(text_) + text_.substr(k.ls, el.end_off - k.ls));
    else splice(el.end_off, 0, std::string_view(text_).substr(el.start_off, el.end_off - el.start_off));
    return new_index;
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

} // namespace wce
