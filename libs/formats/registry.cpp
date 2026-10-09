#include "formats/registry.h"

namespace te {

void register_xml_format();
void register_fastbin_tile_formats();
void register_bmd_bin_format();
void register_markers_format();
void register_image_container_formats();
void register_chmf_format();
void register_png_format();
void register_opaque_formats();

std::vector<Format>& formats() {
    static std::vector<Format> f;
    return f;
}

void register_all_formats() {
    static bool done = false;
    if (done) return;
    done = true;
    register_xml_format();
    register_fastbin_tile_formats();
    register_bmd_bin_format();
    register_markers_format();
    register_image_container_formats();
    register_chmf_format();
    register_png_format();
    register_opaque_formats();
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
