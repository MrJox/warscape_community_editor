// Compares the editor's environment -> cbuffer mapping with live values read from a game memory dump.
//
//   env_vs_dump <in-pack .environment path> <dump_cbs.py output.txt> [gamma brightness]
//
// Prints, for every variable the editor writes that also appears in the dump: both values and OK / DIFF.
// The dump file is the text written by tools/re/dump_cbs.py ("cbuffer.var  v0 v1 v2 ...").
#include "core/vfs.h"
#include "formats/xml_doc.h"
#include "render/environment.h"
#include "render/shader_lib.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>

using namespace we;

int main(int argc, char** argv) {
    if (argc < 3) {
        std::printf("usage: env_vs_dump <environment path> <dump_cbs output> [gamma brightness]\n");
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
    XmlDoc doc = XmlDoc::parse(vfs.read(argv[1]));
    EnvironmentParams e = read_environment(doc);
    if (argc > 4) {
        e.output_gamma = float(std::atof(argv[3]));
        e.output_brightness = float(std::atof(argv[4]));
    }
    apply_environment(e, cbs);

    std::ifstream in(argv[2]);
    std::string line;
    int ok = 0, diff = 0;
    while (std::getline(in, line)) {
        std::istringstream is(line);
        std::string key;
        if (!(is >> key)) continue;
        auto dot = key.find('.');
        if (dot == std::string::npos) continue;
        std::string cb = key.substr(0, dot), var = key.substr(dot + 1);
        auto* b = cbs.find(cb);
        if (!b || !b->vars.count(var)) continue;
        const auto& v = b->vars.at(var);
        std::vector<float> game;
        float x;
        while (is >> x) game.push_back(x);
        size_t n = std::min<size_t>(v.size / 4, game.size());
        if (cb == "lighting_VS_PS" && var == "g_apply_environment_specular") continue; // engine flag, not from the file
        bool same = true;
        std::string mine_s, game_s;
        for (size_t i = 0; i < n; ++i) {
            float m;
            std::memcpy(&m, b->cpu.data() + v.offset + 4 * i, 4);
            float g = game[i];
            if (std::fabs(m - g) > 1e-3f * std::max(1.0f, std::fabs(g))) same = false;
            char buf[64];
            std::snprintf(buf, sizeof buf, "%.6g ", m);
            mine_s += buf;
            std::snprintf(buf, sizeof buf, "%.6g ", g);
            game_s += buf;
        }
        (same ? ok : diff)++;
        std::printf("%-4s %-50s editor: %s\n     %-50s game:   %s\n", same ? "OK" : "DIFF", key.c_str(), mine_s.c_str(), "", game_s.c_str());
    }
    std::printf("\n%d match, %d differ\n", ok, diff);
    return diff ? 1 : 0;
}
