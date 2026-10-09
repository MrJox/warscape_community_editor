// Compares the editor's environment -> cbuffer mapping with live values read from a game memory dump.
//
//   env_vs_dump <in-pack .environment path> <dump_cbs.py output.txt> [gamma brightness]
//   env_vs_dump --all <dump_cbs.py output.txt> [gamma brightness]
//
// Prints, for every variable the editor writes that also appears in the dump: both values and OK / DIFF.
// The dump file is the text written by tools/re/dump_cbs.py ("cbuffer.var  v0 v1 v2 ...").
// --all ranks every .environment file in the packs by how many of those variables match: the dump does not
// say which weather file the battle used, but its sun / sky / fog constants identify it.
#include "vfs/vfs.h"
#include "formats/xml_doc.h"
#include "render/environment.h"
#include "render/shader_lib.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>

using namespace wce;

namespace {

struct DumpVar {
    std::string key, cb, var;
    std::vector<float> values;
};

std::vector<DumpVar> read_dump(const char* path) {
    std::vector<DumpVar> out;
    std::ifstream in(path);
    std::string line;
    while (std::getline(in, line)) {
        std::istringstream is(line);
        DumpVar d;
        if (!(is >> d.key)) continue;
        auto dot = d.key.find('.');
        if (dot == std::string::npos) continue;
        d.cb = d.key.substr(0, dot);
        d.var = d.key.substr(dot + 1);
        float x;
        while (is >> x) d.values.push_back(x);
        out.push_back(std::move(d));
    }
    return out;
}

// Returns {match, differ}; prints every compared variable when verbose.
std::pair<int, int> compare(ConstantBuffers& cbs, const std::vector<DumpVar>& dump, bool verbose) {
    int ok = 0, diff = 0;
    for (const auto& d : dump) {
        auto* b = cbs.find(d.cb);
        if (!b || !b->vars.count(d.var)) continue;
        if (d.cb == "lighting_VS_PS" && d.var == "g_apply_environment_specular") continue; // engine flag, not from the file
        const auto& v = b->vars.at(d.var);
        size_t n = std::min<size_t>(v.size / 4, d.values.size());
        bool same = true;
        std::string mine_s, game_s;
        for (size_t i = 0; i < n; ++i) {
            float m;
            std::memcpy(&m, b->cpu.data() + v.offset + 4 * i, 4);
            float g = d.values[i];
            if (std::fabs(m - g) > 1e-3f * std::max(1.0f, std::fabs(g))) same = false;
            char buf[64];
            std::snprintf(buf, sizeof buf, "%.6g ", m);
            mine_s += buf;
            std::snprintf(buf, sizeof buf, "%.6g ", g);
            game_s += buf;
        }
        (same ? ok : diff)++;
        if (verbose)
            std::printf("%-4s %-50s editor: %s\n     %-50s game:   %s\n", same ? "OK" : "DIFF", d.key.c_str(), mine_s.c_str(), "",
                        game_s.c_str());
    }
    return {ok, diff};
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::printf("usage: env_vs_dump <environment path | --all> <dump_cbs output> [gamma brightness]\n");
        return 2;
    }
    Microsoft::WRL::ComPtr<ID3D11Device> dev;
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, &fl, 1, D3D11_SDK_VERSION, &dev, nullptr, nullptr))) return 1;
    Vfs vfs(Vfs::default_config());
    vfs.mount();
    ConstantBuffers cbs;
    ShaderLibrary lib(dev.Get(), vfs, cbs);
    for (const char* s : {"dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen",
                          "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef",
                          "dx11_sm_3_0_battlefog.hlsl_ps_ps30_main", "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high",
                          "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_determine_black_and_white_points",
                          "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_hdr_luma_to_quarter_size_luma_stats"})
        lib.get(s);
    const std::vector<DumpVar> dump = read_dump(argv[2]);
    auto load = [&](const std::string& path) {
        XmlDoc doc = XmlDoc::parse(vfs.read(path));
        EnvironmentParams e = read_environment(doc);
        if (argc > 4) {
            e.output_gamma = float(std::atof(argv[3]));
            e.output_brightness = float(std::atof(argv[4]));
        }
        apply_environment(e, cbs);
    };

    if (std::strcmp(argv[1], "--all") == 0) {
        std::vector<std::string> files = vfs.list("", ".environment");
        vfs.prefetch(files);
        std::vector<std::pair<int, std::string>> ranked;
        for (const auto& f : files) {
            try {
                load(f);
            } catch (const std::exception&) {
                continue;
            }
            ranked.push_back({compare(cbs, dump, false).first, f});
        }
        std::sort(ranked.begin(), ranked.end(), [](auto& a, auto& b) { return a.first > b.first; });
        for (size_t i = 0; i < ranked.size() && i < 15; ++i) std::printf("%3d  %s\n", ranked[i].first, ranked[i].second.c_str());
        return 0;
    }

    load(argv[1]);
    auto [ok, diff] = compare(cbs, dump, true);
    std::printf("\n%d match, %d differ\n", ok, diff);
    return diff ? 1 : 0;
}
