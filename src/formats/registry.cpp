#include "formats/registry.h"

namespace te {

void register_xml_format();

std::vector<Format>& formats() {
    static std::vector<Format> f;
    return f;
}

void register_all_formats() {
    static bool done = false;
    if (done) return;
    done = true;
    register_xml_format();
}

const Format* find_format(const std::string& path, const std::vector<uint8_t>& head) {
    for (const Format& f : formats())
        if (f.claims(path, head)) return &f;
    return nullptr;
}

std::string describe_diff(const std::vector<uint8_t>& a, const std::vector<uint8_t>& b) {
    size_t i = 0;
    while (i < a.size() && i < b.size() && a[i] == b[i]) ++i;
    return "first difference at byte " + std::to_string(i) + ", sizes " + std::to_string(a.size()) + "/" +
           std::to_string(b.size());
}

} // namespace te
