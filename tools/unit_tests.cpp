// Small self-checking tests for the scene layer.
//   unit_tests <tile.ted>
#include "scene/ted_scene.h"
#include "scene/world_transform.h"

#include <cmath>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <random>

static int failures = 0;
#define CHECK(cond, msg) do { if (!(cond)) { ++failures; std::printf("FAIL: %s\n", msg); } } while (0)

static std::vector<uint8_t> slurp(const char* p) {
    std::ifstream f(p, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(f), {});
}

int main(int argc, char** argv) {
    // euler <-> matrix
    std::mt19937 rng(1);
    std::uniform_real_distribution<double> ang(-3.1, 3.1);
    double worst = 0;
    for (int i = 0; i < 20000; ++i) {
        te::Vec3 e{ang(rng), ang(rng) * 0.5, ang(rng)};
        te::Mat3 m = te::euler_matrix(e);
        te::Mat3 m2 = te::euler_matrix(te::euler_from_matrix(m));
        for (int k = 0; k < 9; ++k) worst = std::max(worst, std::fabs(m[k] - m2[k]));
    }
    CHECK(worst < 1e-9, "euler_from_matrix does not invert euler_matrix");
    std::printf("euler round trip worst error %.2e\n", worst);

    // number formatting
    CHECK(te::format_number(1.0) == "1", "format 1");
    CHECK(te::format_number(-0.0) == "-0", "format -0");
    CHECK(te::format_number(12.5) == "12.5", "format 12.5");
    CHECK(te::format_number(1235.059326) == "1235.059326", "format 1235.059326");
    CHECK(te::format_number(-28.2578125) == "-28.257813", "format tie rounds away from zero");

    if (argc > 1) {
        auto bytes = slurp(argv[1]);
        te::TedScene sc = te::TedScene::parse(bytes);
        auto buildings = sc.find_all("BUILDING_INSTANCE");
        CHECK(!buildings.empty(), "tile has buildings");
        int b = buildings[buildings.size() / 2];
        // duplicate, then restore the snapshot: bytes identical
        auto snap = sc.snapshot();
        int copy = sc.duplicate(b);
        CHECK(copy >= 0 && sc.type(copy) == "BUILDING_INSTANCE", "duplicate gives a building");
        CHECK(sc.save() != bytes, "duplicate changes the file");
        sc.restore(snap);
        CHECK(sc.save() == bytes, "restore after duplicate restores the bytes");
        // duplicate then remove the copy: bytes identical
        copy = sc.duplicate(b);
        sc.remove(copy);
        CHECK(sc.save() == bytes, "duplicate + remove restores the bytes");
        // yaw rotation: rotate by +0.3 then -0.3 returns to the same world rotation
        te::WorldTransform w0 = te::world_transform(sc, b);
        te::rotate_world_yaw(sc, b, 0.3);
        te::WorldTransform w1 = te::world_transform(sc, b);
        double diff = 0;
        for (int k = 0; k < 9; ++k) diff = std::max(diff, std::fabs(te::mat_mul(te::rotation_y(0.3), w0.rotation)[k] - w1.rotation[k]));
        CHECK(diff < 2e-5, "rotate_world_yaw applies Ry(delta) in world space");
        te::rotate_world_yaw(sc, b, -0.3);
        te::WorldTransform w2 = te::world_transform(sc, b);
        double back = 0;
        for (int k = 0; k < 9; ++k) back = std::max(back, std::fabs(w0.rotation[k] - w2.rotation[k]));
        CHECK(back < 2e-5, "rotating back restores the world rotation");
        // set_world_position keeps the world position
        te::set_world_position(sc, b, {w0.position[0] + 6, w0.position[1], w0.position[2] - 6});
        te::WorldTransform w3 = te::world_transform(sc, b);
        CHECK(std::fabs(w3.position[0] - (w0.position[0] + 6)) < 1e-3 && std::fabs(w3.position[2] - (w0.position[2] - 6)) < 1e-3, "set_world_position");
        sc.revert_all();
    }
    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
