// Unit checks for libs/foundation: JSON reader and writer (what rpfm_server speaks) and settings precedence.
#include "foundation/json.h"
#include "foundation/settings.h"

#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>

using wce::Json;

static int failures = 0;
#define CHECK(cond, msg) do { if (!(cond)) { ++failures; std::printf("FAIL: %s\n", msg); } } while (0)

static bool throws(const char* text) {
    try { Json::parse(text); } catch (const std::runtime_error&) { return true; }
    return false;
}

int main() {
    Json j = Json::parse(R"({"a":[1,2.5,-3e2,true,false,null],"s":"x\"y\\z\n\u00e9\ud83d\ude00","o":{"k":"v"},"e":[],"eo":{}})");
    CHECK(j.find("a") && j.find("a")->as_array().size() == 6, "array of 6");
    CHECK(j.find("a")->at(1)->as_number() == 2.5 && j.find("a")->at(2)->as_number() == -300, "numbers");
    CHECK(j.find("a")->at(3)->as_bool() && !j.find("a")->at(4)->as_bool() && j.find("a")->at(5)->is_null(), "literals");
    CHECK(j.find("s")->as_string() == "x\"y\\z\n\xC3\xA9\xF0\x9F\x98\x80", "escapes, \\u and a surrogate pair become UTF-8");
    CHECK(j.find("o")->find("k")->as_string() == "v" && j.find("missing") == nullptr, "object lookup");
    CHECK(j.find("e")->as_array().empty() && j.find("eo")->as_object().empty(), "empty containers");

    // dump(parse(x)) is stable, and keeps member order
    const std::string once = j.dump();
    CHECK(Json::parse(once).dump() == once, "dump is stable");
    CHECK(once.find("\"a\"") < once.find("\"s\"") && once.find("\"s\"") < once.find("\"o\""), "member order kept");
    CHECK(Json(std::string("tab\there\x01")).dump() == "\"tab\\there\\u0001\"", "control characters are escaped");
    CHECK(Json(3).dump() == "3" && Json(0.5).dump() == "0.5" && Json(true).dump() == "true" && Json().dump() == "null", "scalar output");

    // building
    Json o = Json::object();
    o.set("pack_key", "a/b.pack").set("export_as_tsv", false);
    Json arr = Json::array();
    arr.push(1).push("two");
    o.set("list", std::move(arr));
    CHECK(o.dump() == R"({"pack_key":"a/b.pack","export_as_tsv":false,"list":[1,"two"]})", "built object");
    o.set("pack_key", "c");
    CHECK(o.find("pack_key")->as_string() == "c" && o.as_object().size() == 3, "set replaces");

    CHECK(throws("{"), "unterminated object");
    CHECK(throws("[1,]"), "trailing comma");
    CHECK(throws("{\"a\":1} x"), "trailing characters");
    CHECK(throws("\"abc"), "unterminated string");
    CHECK(throws(""), "empty text");
    CHECK(throws("\"\\q\""), "bad escape");

    // settings: the environment wins over the file
    namespace fs = std::filesystem;
    const fs::path dir = fs::temp_directory_path() / "wce_foundation_check";
    fs::create_directories(dir);
    { std::ofstream f(dir / "wce.ini"); f << "# comment\r\ngame_dir=C:\\games\r\nempty=\r\nrpfm_dir=D:\\rpfm\r\n"; }
    wce::Settings s = wce::Settings::load(dir);
    CHECK(s.get("game_dir").value_or("") == "C:\\games", "value from the file, CRLF tolerated");
    CHECK(!s.get("empty") && !s.get("missing") && !s.get("# comment"), "empty, missing and comment lines");
    _putenv_s("WCE_CHECK_GAME", "E:\\env");
    CHECK(s.get("game_dir", "WCE_CHECK_GAME").value_or("") == "E:\\env", "environment overrides the file");
    CHECK(s.get("game_dir", "WCE_CHECK_UNSET").value_or("") == "C:\\games", "unset environment variable falls back to the file");
    s.set("cache_dir", "F:\\c");
    CHECK(s.save(), "save");
    CHECK(wce::Settings::load(dir).get("cache_dir").value_or("") == "F:\\c", "saved value reloads");
    fs::remove_all(dir);

    std::printf("%s\n", failures ? "FAILED" : "all passed");
    return failures ? 1 : 0;
}
