// Small self-checking tests for the scene layer.
//   unit_tests <tile.ted>
#include "tile/ted_scene.h"
#include "tile/world_transform.h"

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
        wce::Vec3 e{ang(rng), ang(rng) * 0.5, ang(rng)};
        wce::Mat3 m = wce::euler_matrix(e);
        wce::Mat3 m2 = wce::euler_matrix(wce::euler_from_matrix(m));
        for (int k = 0; k < 9; ++k) worst = std::max(worst, std::fabs(m[k] - m2[k]));
    }
    CHECK(worst < 1e-9, "euler_from_matrix does not invert euler_matrix");
    std::printf("euler round trip worst error %.2e\n", worst);

    // number formatting
    CHECK(wce::format_ted_number(1.0) == "1", "format 1");
    CHECK(wce::format_ted_number(-0.0) == "-0", "format -0");
    CHECK(wce::format_ted_number(12.5) == "12.5", "format 12.5");
    CHECK(wce::format_ted_number(1235.059326) == "1235.059326", "format 1235.059326");
    CHECK(wce::format_ted_number(-28.2578125) == "-28.257813", "format tie rounds away from zero");

    if (argc > 1) {
        auto bytes = slurp(argv[1]);
        wce::TedScene sc = wce::TedScene::parse(bytes);
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
        wce::WorldTransform w0 = wce::world_transform(sc, b);
        wce::rotate_world_yaw(sc, b, 0.3);
        wce::WorldTransform w1 = wce::world_transform(sc, b);
        double diff = 0;
        for (int k = 0; k < 9; ++k) diff = std::max(diff, std::fabs(wce::mat_mul(wce::rotation_y(0.3), w0.rotation)[k] - w1.rotation[k]));
        CHECK(diff < 2e-5, "rotate_world_yaw applies Ry(delta) in world space");
        wce::rotate_world_yaw(sc, b, -0.3);
        wce::WorldTransform w2 = wce::world_transform(sc, b);
        double back = 0;
        for (int k = 0; k < 9; ++k) back = std::max(back, std::fabs(w0.rotation[k] - w2.rotation[k]));
        CHECK(back < 2e-5, "rotating back restores the world rotation");
        // set_world_position keeps the world position
        wce::set_world_position(sc, b, {w0.position[0] + 6, w0.position[1], w0.position[2] - 6});
        wce::WorldTransform w3 = wce::world_transform(sc, b);
        CHECK(std::fabs(w3.position[0] - (w0.position[0] + 6)) < 1e-3 && std::fabs(w3.position[2] - (w0.position[2] - 6)) < 1e-3, "set_world_position");
        sc.revert_all();
        // layers: add, move an instance into it, restore
        {
            wce::TedScene s2 = wce::TedScene::parse(bytes);
            int root = s2.scene_graph();
            auto kids = s2.children(root);
            CHECK(!kids.empty(), "root has layers");
            int before_count = int(s2.find_all("BUILDING_INSTANCE").size());
            int layer = s2.add_layer(root, "Test layer");
            CHECK(layer >= 0 && s2.name(layer) == "Test layer" && s2.type(layer) == "LayerInstance", "add_layer creates the layer");
            CHECK(s2.children(layer).empty(), "new layer is empty");
            auto bs = s2.find_all("BUILDING_INSTANCE");
            int victim = bs[bs.size() / 3];
            std::string vkey = s2.doc().get(victim, "key").value_or("");
            std::string old_path = s2.layer_path(victim);
            int moved = s2.move_to_layer(victim, layer);
            CHECK(s2.type(moved) == "BUILDING_INSTANCE" && s2.doc().get(moved, "key").value_or("") == vkey, "moved instance keeps its key");
            CHECK(int(s2.find_all("BUILDING_INSTANCE").size()) == before_count, "move keeps the instance count");
            CHECK(s2.layer_path(moved).find("Test layer") != std::string::npos, "moved instance is in the new layer");
            CHECK(s2.layer_path(moved) != old_path, "layer path changed");
            // the saved text re-parses
            wce::TedScene s3 = wce::TedScene::parse(s2.save());
            CHECK(int(s3.find_all("BUILDING_INSTANCE").size()) == before_count, "saved text re-parses with the same count");
        }
    }
    if (argc > 1) {
        // polygons: add, edit points, area, undo via snapshot
        wce::TedScene sc = wce::TedScene::parse(slurp(argv[1]));
        auto orig = sc.save();
        int root = sc.scene_graph();
        int layer = sc.add_layer(root, "Poly layer");
        int poly = sc.add_polygon(layer, "AI_HINT_POLYGON_INSTANCE", "Test hint", {{0, 1, 0}, {20, 1, 0}, {20, 1, 10}, {0, 1, 10}}, {{"hint_type", "AIH_FORT_PERIMETER"}, {"polygon_closed", "1"}});
        CHECK(sc.polygon_points(poly).size() == 4, "polygon has 4 points");
        CHECK(std::fabs(sc.polygon_area(poly) - 200) < 1e-6, "rectangle area 200");
        CHECK(sc.doc().get(poly, "area").value_or("") == "200", "area attribute written");
        CHECK(sc.doc().get(poly, "hint_type").value_or("") == "AIH_FORT_PERIMETER", "hint type override");
        auto p0 = sc.point_position(sc.polygon_points(poly)[0]);
        CHECK(p0 && std::fabs((*p0)[0] + 10) < 1e-6, "points are local to the centroid");
        sc.set_polygon_point(poly, 0, {-20, 0, -5});
        CHECK(std::fabs(sc.polygon_area(poly) - 250) < 1e-6 || sc.polygon_area(poly) > 200, "area follows point edits");
        sc.insert_polygon_point(poly, 1, {0, 0, -8});
        CHECK(sc.polygon_points(poly).size() == 5, "insert adds a point");
        CHECK(sc.remove_polygon_point(poly, 1), "remove a point");
        CHECK(sc.polygon_points(poly).size() == 4, "remove drops a point");
        wce::TedScene re = wce::TedScene::parse(sc.save());
        CHECK(re.find_all("AI_HINT_POLYGON_INSTANCE").size() == sc.find_all("AI_HINT_POLYGON_INSTANCE").size(), "polygon survives save and parse");
        CHECK(sc.save() != orig, "polygon edits change the file");
    }
    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
