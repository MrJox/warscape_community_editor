#include "foundation/text_encoding.h"

#include <fstream>
#include <stdexcept>

namespace wce {

static void append_utf8(std::string& out, uint32_t cp) {
    if (cp < 0x80) {
        out += char(cp);
    } else if (cp < 0x800) {
        out += char(0xC0 | (cp >> 6));
        out += char(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        out += char(0xE0 | (cp >> 12));
        out += char(0x80 | ((cp >> 6) & 0x3F));
        out += char(0x80 | (cp & 0x3F));
    } else {
        out += char(0xF0 | (cp >> 18));
        out += char(0x80 | ((cp >> 12) & 0x3F));
        out += char(0x80 | ((cp >> 6) & 0x3F));
        out += char(0x80 | (cp & 0x3F));
    }
}

DecodedText decode_text(const std::vector<uint8_t>& b) {
    DecodedText r;
    if (b.size() >= 2 && b[0] == 0xFF && b[1] == 0xFE) {
        r.encoding = TextEncoding::Utf16LeBom;
        if (b.size() % 2) throw std::runtime_error("odd byte count in UTF-16 file");
        r.utf8.reserve(b.size() / 2);
        for (size_t i = 2; i < b.size(); i += 2) {
            uint32_t u = b[i] | (b[i + 1] << 8);
            if (u >= 0xD800 && u < 0xDC00 && i + 3 < b.size()) {
                uint32_t lo = b[i + 2] | (b[i + 3] << 8);
                if (lo >= 0xDC00 && lo < 0xE000) {
                    append_utf8(r.utf8, 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                    continue;
                }
            }
            append_utf8(r.utf8, u); // lone surrogates pass through as WTF-8
        }
        return r;
    }
    size_t start = 0;
    if (b.size() >= 3 && b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF) {
        r.encoding = TextEncoding::Utf8Bom;
        start = 3;
    }
    r.utf8.assign(b.begin() + start, b.end());
    return r;
}

std::vector<uint8_t> encode_text(const std::string& s, TextEncoding enc) {
    std::vector<uint8_t> out;
    if (enc != TextEncoding::Utf16LeBom) {
        if (enc == TextEncoding::Utf8Bom) out = {0xEF, 0xBB, 0xBF};
        out.insert(out.end(), s.begin(), s.end());
        return out;
    }
    out.reserve(2 + s.size() * 2);
    out.push_back(0xFF);
    out.push_back(0xFE);
    auto put = [&](uint32_t u) { out.push_back(uint8_t(u)); out.push_back(uint8_t(u >> 8)); };
    for (size_t i = 0; i < s.size();) {
        uint8_t c = uint8_t(s[i]);
        uint32_t cp;
        int n;
        if (c < 0x80) { cp = c; n = 1; }
        else if ((c & 0xE0) == 0xC0) { cp = c & 0x1F; n = 2; }
        else if ((c & 0xF0) == 0xE0) { cp = c & 0x0F; n = 3; }
        else { cp = c & 0x07; n = 4; }
        for (int k = 1; k < n && i + k < s.size(); ++k) cp = (cp << 6) | (uint8_t(s[i + k]) & 0x3F);
        i += n;
        if (cp >= 0x10000) {
            cp -= 0x10000;
            put(0xD800 + (cp >> 10));
            put(0xDC00 + (cp & 0x3FF));
        } else {
            put(cp);
        }
    }
    return out;
}

std::vector<uint8_t> read_file(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) throw std::runtime_error("cannot open " + path);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

bool write_file(const std::string& path, const std::vector<uint8_t>& bytes) {
    std::ofstream f(path, std::ios::binary);
    if (!f) return false;
    f.write(reinterpret_cast<const char*>(bytes.data()), std::streamsize(bytes.size()));
    return bool(f);
}

} // namespace wce
