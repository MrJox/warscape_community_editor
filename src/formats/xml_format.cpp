// All tile XML: .xml and tile.ted. Goes through the lossless XmlDoc (byte spans, nothing re-serialised).
#include "formats/registry.h"
#include "formats/xml_doc.h"

#include <cctype>

namespace te {

static bool ends_with_ci(const std::string& s, const char* suffix) {
    size_t n = std::char_traits<char>::length(suffix);
    if (s.size() < n) return false;
    for (size_t i = 0; i < n; ++i)
        if (std::tolower(static_cast<unsigned char>(s[s.size() - n + i])) != suffix[i]) return false;
    return true;
}

void register_xml_format() {
    Format f;
    f.name = "xml";
    f.claims = [](const std::string& path, const std::vector<uint8_t>& head) {
        if (!(ends_with_ci(path, ".xml") || ends_with_ci(path, ".ted"))) return false;
        // must look like text XML: optional BOM / whitespace, then '<'
        size_t i = 0;
        if (head.size() >= 2 && head[0] == 0xFF && head[1] == 0xFE) return true; // UTF-16 LE BOM
        if (head.size() >= 3 && head[0] == 0xEF && head[1] == 0xBB && head[2] == 0xBF) i = 3;
        while (i < head.size() && (head[i] == ' ' || head[i] == '\t' || head[i] == '\r' || head[i] == '\n')) ++i;
        return i < head.size() && head[i] == '<';
    };
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            we::XmlDoc doc = we::XmlDoc::parse(bytes);
            r.output = doc.save();
            r.ok = r.output == bytes;
            if (!r.ok) r.error = describe_diff(bytes, r.output);
        } catch (const std::exception& e) {
            r.error = std::string("parse: ") + e.what();
        }
        return r;
    };
    formats().push_back(std::move(f));
}

} // namespace te
