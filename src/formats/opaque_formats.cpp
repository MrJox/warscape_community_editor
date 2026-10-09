// Files the editor carries through unchanged, registered so that the corpus gate accounts for every file:
//   * authoring leftovers nobody reads (World Machine projects, 3ds Max / Photoshop sources, JPEG previews, sessions);
//   * text files whose content is not interpreted yet (.agf asset lists, rules.bob, notes);
//   * binaries whose structure is checked by magic only (RMV2 meshes: weather_editor has the full reader; legacy tree lists).
// "opaque" means: read = keep the bytes, write = the same bytes. Nothing here is decoded.
#include "formats/registry.h"
#include "formats/tile_database.h"

#include <cctype>
#include <cstring>
#include <set>

namespace te {

static std::string lower_ext(const std::string& path) {
    size_t slash = path.find_last_of("/\\");
    std::string name = slash == std::string::npos ? path : path.substr(slash + 1);
    size_t dot = name.rfind('.');
    std::string ext = dot == std::string::npos ? "" : name.substr(dot);
    for (char& c : ext) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return ext;
}

static Format identity_format(const char* name, std::function<bool(const std::string&, const std::vector<uint8_t>&)> claims) {
    Format f;
    f.name = name;
    f.claims = std::move(claims);
    f.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        r.output = bytes;
        r.ok = true;
        return r;
    };
    return f;
}

void register_opaque_formats() {
    static const std::set<std::string> authoring = {".jpg", ".max", ".psd", ".session", ".tmd", ".tmp", ".editor", ".outlines", ".cs2"};
    static const std::set<std::string> text = {".agf", ".bob", ".txt"};
    formats().push_back(identity_format("opaque_authoring", [](const std::string& p, const std::vector<uint8_t>&) { return authoring.count(lower_ext(p)) > 0; }));
    formats().push_back(identity_format("opaque_text", [](const std::string& p, const std::vector<uint8_t>&) { return text.count(lower_ext(p)) > 0; }));

    // RMV2 meshes: magic check only (weather_editor/src/formats/rmv2.* parses them)
    Format rmv2 = identity_format("rmv2_magic", [](const std::string& p, const std::vector<uint8_t>& head) {
        return lower_ext(p) == ".rigid_model_v2" && head.size() >= 4 && std::memcmp(head.data(), "RMV2", 4) == 0;
    });
    formats().push_back(std::move(rmv2));

    // Legacy bmd.tree_list and extension-less working files (binary building_list): carried through
    formats().push_back(identity_format("opaque_binary", [](const std::string& p, const std::vector<uint8_t>&) {
        std::string ext = lower_ext(p);
        return ext == ".tree_list" || ext.empty();
    }));

    // tile_database.bin: structural walk of the whole file (the builder is checked by tools/tdb_build against BOB)
    Format tdb;
    tdb.name = "tile_database_bin";
    tdb.claims = [](const std::string& p, const std::vector<uint8_t>& head) {
        return p.size() >= 16 && p.compare(p.size() - 16, 16, "tile_database.bin") == 0 && head.size() >= 8 && std::memcmp(head.data(), "FASTBIN0", 8) == 0;
    };
    tdb.round_trip = [](const std::vector<uint8_t>& bytes) {
        RoundTripResult r;
        try {
            auto order = tile_database_order(bytes);
            r.ok = !order.empty();
            r.output = bytes;
            if (!r.ok) r.error = "no tiles found";
        } catch (const std::exception& e) {
            r.error = std::string("walk: ") + e.what();
        }
        return r;
    };
    formats().push_back(std::move(tdb));
}

} // namespace te
