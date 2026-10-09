#pragma once
// Typed view of environment attribute values.
//
// Syntax seen in all vanilla + TDD files (docs/re/xml_schema.tsv):
//   1 / 0.5 / 111.599998 / -7.2      float or int
//   true / false                     bool
//   rgb(1.0,0.788235,0.615686)       colour
//   argb(0.48,0.56,0.83,0.0039)      colour: despite the name the order is r, g, b, a (verified)
//   variance(0.3,40)                 two floats
//   euler(3.141593,1.000000,0.0)     legacy direction
//
// Number styles differ by file and tool generation ("%f" stripped: 111.599998, shortest: 244.8,
// fixed: 0.50 / 100.00 in vfx, 1.000000 in old files). parse_value detects the style that reproduces
// the original text, and format_value writes new numbers in that same style.
#include <string>
#include <string_view>
#include <vector>

namespace we {

enum class ValueKind { Float, Int, Bool, Rgb, Argb, Variance, Euler, String };

struct NumStyle {
    enum Kind { Stripped, Shortest, Fixed } kind = Stripped;
    int decimals = 6;       // Fixed only
    bool keep_one = false;  // Stripped/Shortest: whole numbers keep ".0"
};

struct EnvValue {
    ValueKind kind = ValueKind::String;
    std::vector<float> f; // Float/Int: 1; Rgb: r,g,b; Argb: r,g,b,a; Variance: 2; Euler: 3
    long long i = 0;      // Int
    bool b = false;
    std::string s;        // original text
    NumStyle style;
};

EnvValue parse_value(std::string_view text);
std::string format_value(const EnvValue& v);
std::string format_number(float v, const NumStyle& style);

} // namespace we
