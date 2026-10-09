// Which input order + sort algorithm reproduces BOB's tile order (area descending)? Experiment.
// Reads scratch/order_in2.txt (fn w h bob_rank location) from the working directory.
#include <algorithm>
#include <cctype>
#include <cstdio>
#include <fstream>
#include <string>
#include <vector>

struct R { std::string fn, loc; int w, h, rank; };
static std::string low(std::string s) { for (auto& c : s) c = (char)std::tolower((unsigned char)c); return s; }
static std::string dir_of(const std::string& s) { return s.substr(0, s.rfind(char(92))); }

int main() {
    std::ifstream in("order_in2.txt");
    std::vector<R> v;
    R r;
    while (in >> r.fn >> r.w >> r.h >> r.rank >> r.loc) v.push_back(r);
    auto bya = [](const R& a, const R& b) { return a.w * a.h > b.w * b.h; };
    struct Inp { const char* name; bool (*less)(const R&, const R&); };
    Inp inps[] = {
        {"fn ci", [](const R& a, const R& b) { return low(a.fn) < low(b.fn); }},
        {"fn cs", [](const R& a, const R& b) { return a.fn < b.fn; }},
        {"loc ci", [](const R& a, const R& b) { return low(a.loc) < low(b.loc); }},
        {"loc cs", [](const R& a, const R& b) { return a.loc < b.loc; }},
        {"dir then loc", [](const R& a, const R& b) { return dir_of(a.loc) != dir_of(b.loc) ? low(dir_of(a.loc)) < low(dir_of(b.loc)) : low(a.loc) < low(b.loc); }},
    };
    for (auto& ip : inps) for (int rev = 0; rev < 2; ++rev) for (int alg = 0; alg < 3; ++alg) {
        auto x = v;
        std::sort(x.begin(), x.end(), ip.less);
        if (rev) std::reverse(x.begin(), x.end());
        if (alg == 0) std::sort(x.begin(), x.end(), bya);
        else if (alg == 1) std::stable_sort(x.begin(), x.end(), bya);
        else { std::make_heap(x.begin(), x.end(), bya); std::sort_heap(x.begin(), x.end(), bya); }
        int same = 0;
        for (size_t k = 0; k < x.size(); ++k) same += x[k].rank == (int)k;
        std::printf("%5d of %zu: input %-13s reversed %d algorithm %s\n", same, x.size(), ip.name, rev, alg == 0 ? "sort" : alg == 1 ? "stable_sort" : "heap");
    }
}
