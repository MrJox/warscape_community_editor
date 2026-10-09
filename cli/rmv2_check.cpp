// Gate for the rigid_model_v2 reader: parses every .rigid_model_v2 under the given folders.
//   rmv2_check <dir>...
#include "core/text_encoding.h"
#include "formats/rmv2.h"

#include <cstdio>
#include <filesystem>
#include <map>

namespace fs = std::filesystem;
using namespace we;

int main(int argc, char** argv) {
    size_t files = 0, failed = 0;
    std::map<int, std::pair<size_t, size_t>> by_shader;       // shader -> (with geometry, without)
    std::map<int, size_t> std_textures, std_alpha;            // RS_STANDARD_V5 texture params / alpha modes
    std::map<std::string, size_t> errors;
    for (int i = 1; i < argc; ++i)
        for (auto& de : fs::recursive_directory_iterator(argv[i])) {
            if (de.path().extension() != ".rigid_model_v2") continue;
            ++files;
            try {
                Rmv2Model m = parse_rmv2(read_file(de.path().string()));
                for (auto& lod : m.lods)
                    for (auto& me : lod.meshes) {
                        auto& c = by_shader[me.shader];
                        (me.vertex_stride ? c.first : c.second)++;
                        if (me.shader == RS_STANDARD_V5 && me.vertex_stride) {
                            for (auto& t : me.textures) std_textures[t.first]++;
                            std_alpha[me.alpha_mode()]++;
                        }
                    }
            } catch (const std::exception& e) {
                ++failed;
                errors[e.what()]++;
            }
        }
    std::printf("%zu files, %zu failed\n", files, failed);
    for (auto& [e, n] : errors) std::printf("  %5zu  %s\n", n, e.c_str());
    for (auto& [s, c] : by_shader) std::printf("shader %3d: %6zu meshes with geometry, %zu without\n", s, c.first, c.second);
    std::printf("RS_STANDARD_V5 texture params:");
    for (auto& [t, n] : std_textures) std::printf(" %d:%zu", t, n);
    std::printf("\nRS_STANDARD_V5 alpha modes:");
    for (auto& [a, n] : std_alpha) std::printf(" %d:%zu", a, n);
    std::printf("\n");
    return failed ? 1 : 0;
}
