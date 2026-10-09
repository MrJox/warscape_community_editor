// Builds the particle emitters of a weather definition and compares their constants with a game memory dump.
//
//   vfx_check "<weather definition name>" [dump_cbs.py --array=512 output.txt]
//
// Without a dump file: lists the active emitters and their constant rows. With one: for every emitter finds the
// dumped row (index into the [512] arrays) that matches best and prints OK / DIFF per variable.
#include "vfs/vfs.h"
#include "assets/weather_vfx.h"

#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <map>
#include <sstream>

using namespace wce;

namespace {

struct Row { float f[4]; uint32_t u[4]; };
using DumpRows = std::map<int, std::map<std::string, Row>>; // index -> variable -> values

DumpRows read_rows(const char* path) {
    DumpRows rows;
    std::ifstream in(path);
    std::string line;
    while (std::getline(in, line)) {
        auto lb = line.find('['), rb = line.find(']');
        auto dot = line.find('.');
        if (lb == std::string::npos || rb == std::string::npos || dot == std::string::npos || dot > lb) continue;
        std::string var = line.substr(dot + 1, lb - dot - 1);
        int idx = std::atoi(line.c_str() + lb + 1);
        std::istringstream is(line.substr(rb + 1));
        Row r{};
        for (int i = 0; i < 4; ++i) {
            std::string tok;
            if (!(is >> tok)) break;
            if (tok[0] == '#') { r.u[i] = uint32_t(std::stoul(tok.substr(1))); std::memcpy(&r.f[i], &r.u[i], 4); }
            else { r.f[i] = float(std::atof(tok.c_str())); std::memcpy(&r.u[i], &r.f[i], 4); }
        }
        rows[idx][var] = r;
    }
    return rows;
}

struct Mine { const char* name; const float* f; const uint32_t* u; };

std::vector<Mine> mine(const EmitterConstants& k) {
    return {{"behaviour_mask", nullptr, k.behaviour_mask},
            {"vs_translation_params", k.vs_translation_params, nullptr},
            {"vs_translation_params1", k.vs_translation_params1, nullptr},
            {"ps_sim_game_logic_params", k.ps_sim_game_logic_params, nullptr},
            {"ps_sim_gravity_params", k.ps_sim_gravity_params, nullptr},
            {"ps_sim_deceleration_params", k.ps_sim_deceleration_params, nullptr},
            {"ps_sim_following_emitter_params", k.ps_sim_following_emitter_params, nullptr},
            {"ps_sim_turbulence_params", k.ps_sim_turbulence_params, nullptr},
            {"ps_lighting_legacy_params", k.ps_lighting_legacy_params, nullptr},
            {"ps_lighting_params", k.ps_lighting_params, nullptr},
            {"ps_soft_edge_params", k.ps_soft_edge_params, nullptr},
            {"vs_culling_distances", k.vs_culling_distances, nullptr},
            {"vs_rotation_params", k.vs_rotation_params, nullptr},
            {"vs_atlas_texture_params", k.vs_atlas_texture_params, nullptr},
            {"vs_cell_texture_params", nullptr, k.vs_cell_texture_params},
            {"vs_animated_texture_params", nullptr, k.vs_animated_texture_params},
            {"vs_animated_texture_params1", k.vs_animated_texture_params1, nullptr}};
}

bool same(const Mine& m, const Row& r, int i) {
    if (m.u) return m.u[i] == r.u[i];
    return std::fabs(m.f[i] - r.f[i]) <= 1e-4f * std::max(1.0f, std::fabs(r.f[i]));
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::printf("usage: vfx_check \"<weather definition>\" [dump rows.txt]\n");
        return 2;
    }
    Vfs vfs(Vfs::default_config());
    vfs.mount();
    WeatherVfx w = load_weather_vfx(vfs, argv[1]);
    std::printf("definition '%s' vfx1 '%s' type %s wind %.3g..%.3g: %zu emitters\n", w.definition.name.c_str(),
                w.definition.vfx1.c_str(), w.definition.type.c_str(), w.definition.wind_size_min, w.definition.wind_size_max,
                w.emitters.size());
    for (auto& n : w.notes) std::printf("  note: %s\n", n.c_str());
    DumpRows rows;
    if (argc > 2) rows = read_rows(argv[2]);
    int total_diff = 0;
    for (auto& e : w.emitters) {
        auto vars = mine(e.k);
        std::printf("\n%s / %s / %s  diffuse=%s\n", e.library.c_str(), e.instance.c_str(), e.name.c_str(), e.k.diffuse.c_str());
        if (rows.empty()) {
            for (auto& m : vars) {
                std::printf("  %-34s", m.name);
                for (int i = 0; i < 4; ++i) m.u ? std::printf(" #%u", m.u[i]) : std::printf(" %g", m.f[i]);
                std::printf("\n");
            }
            continue;
        }
        int best = -1, best_score = -1;
        for (auto& [idx, rv] : rows) {
            int score = 0;
            for (auto& m : vars) {
                auto it = rv.find(m.name);
                Row r = it == rv.end() ? Row{} : it->second;
                for (int i = 0; i < 4; ++i) score += same(m, r, i);
            }
            if (score > best_score) best_score = score, best = idx;
        }
        std::printf("  best dump row %d (%d of %zu components)\n", best, best_score, vars.size() * 4);
        for (auto& m : vars) {
            auto it = rows[best].find(m.name);
            Row r = it == rows[best].end() ? Row{} : it->second;
            bool ok = true;
            for (int i = 0; i < 4; ++i) ok &= same(m, r, i);
            if (ok) continue;
            ++total_diff;
            std::printf("  DIFF %-32s mine:", m.name);
            for (int i = 0; i < 4; ++i) m.u ? std::printf(" #%u", m.u[i]) : std::printf(" %g", m.f[i]);
            std::printf("   game:");
            for (int i = 0; i < 4; ++i) m.u ? std::printf(" #%u", r.u[i]) : std::printf(" %g", r.f[i]);
            std::printf("\n");
        }
    }
    return total_diff ? 1 : 0;
}
