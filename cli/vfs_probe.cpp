// Mounts the game packs through rpfm_cli and prints what the editor would see.
//   vfs_probe [path-to-fetch...]
#include "vfs/vfs.h"

#include <chrono>
#include <cstdio>

using namespace wce;

int main(int argc, char** argv) {
    try {
        auto t0 = std::chrono::steady_clock::now();
        Vfs vfs(Vfs::default_config());
        vfs.mount();
        double s = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
        std::printf("mounted %zu packs, %zu files in %.1f s\n", vfs.packs().size(), vfs.files().size(), s);
        for (auto& m : vfs.missing_mods()) std::printf("  MISSING mod %s (in used_mods.txt, not in data/)\n", m.c_str());
        for (auto& p : vfs.packs()) std::printf("  %s %s\n", p.is_mod ? "mod    " : "vanilla", p.name.c_str());
        std::printf(".environment %zu, .lighting %zu, vfx xml %zu, fxc %zu, building rmv2 %zu\n",
                    vfs.list("", ".environment").size(), vfs.list("", ".lighting").size(), vfs.list("vfx/", ".xml").size(),
                    vfs.list("fxc/", ".fxc").size(), vfs.list("rigidmodels/buildings/", ".rigid_model_v2").size());
        for (int i = 1; i < argc; ++i) {
            auto o = vfs.owner(argv[i]);
            auto d = vfs.fetch(argv[i]);
            std::printf("%s -> %s : %s\n", argv[i], o ? vfs.packs()[*o].name.c_str() : "-", d ? d->string().c_str() : "missing");
        }
    } catch (const std::exception& e) {
        std::printf("error: %s\n", e.what());
        return 1;
    }
    return 0;
}
