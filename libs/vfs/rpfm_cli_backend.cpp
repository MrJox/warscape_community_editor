#include "foundation/process.h"
#include "vfs/pack_backend.h"

#include <regex>
#include <sstream>
#include <stdexcept>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::vector<std::string> rpfm_lines(const std::string& out) {
    // rpfm_cli prints log lines ("hh:mm:ss [INFO] ...") before the payload.
    static const std::regex log(R"(^\d\d:\d\d:\d\d .*\[)");
    std::vector<std::string> lines;
    std::istringstream is(out);
    std::string l;
    while (std::getline(is, l)) {
        if (!l.empty() && l.back() == '\r') l.pop_back();
        if (l.empty() || std::regex_search(l, log)) continue;
        lines.push_back(l);
    }
    return lines;
}

class RpfmCliBackend : public PackBackend {
public:
    RpfmCliBackend(fs::path cli, std::string game) : cli_(std::move(cli)), game_(widen(std::move(game))) {}

    std::string name() const override { return "rpfm_cli"; }

    std::vector<std::string> list(const fs::path& pack) override {
        ProcessResult r = run_process(cli_, {L"--game", game_, L"pack", L"list", L"--pack-path", pack.wstring()});
        if (r.exit_code != 0) throw std::runtime_error("rpfm_cli list failed for " + narrow(pack.filename().wstring()) + ": " + r.out.substr(0, 400));
        return rpfm_lines(r.out);
    }

    void extract(const fs::path& pack, const std::vector<std::string>& paths, const fs::path& root) override {
        const size_t kMaxCmd = 30000;   // command line limit
        std::vector<std::wstring> args;
        size_t len = 0;
        auto flush = [&] {
            if (args.empty()) return;
            std::vector<std::wstring> full = {L"--game", game_, L"pack", L"extract", L"--pack-path", pack.wstring()};
            full.insert(full.end(), args.begin(), args.end());
            ProcessResult r = run_process(cli_, full);
            if (r.exit_code != 0) throw std::runtime_error("rpfm_cli extract failed for " + narrow(pack.filename().wstring()) + ": " + r.out.substr(0, 400));
            args.clear();
            len = 0;
        };
        for (const std::string& p : paths) {
            std::wstring a = widen(p) + L";" + root.wstring();
            if (len + a.size() + 8 > kMaxCmd) flush();
            args.push_back(L"-f");
            args.push_back(a);
            len += a.size() + 8;
        }
        flush();
    }

private:
    fs::path cli_;
    std::wstring game_;
};

} // namespace

std::unique_ptr<PackBackend> make_rpfm_cli_backend(const fs::path& rpfm_cli, const std::string& game_key) {
    if (rpfm_cli.empty() || !fs::exists(rpfm_cli)) throw std::runtime_error("rpfm_cli.exe not found" + (rpfm_cli.empty() ? std::string(" (rpfm_dir is not set)") : ": " + narrow(rpfm_cli.wstring())));
    return std::make_unique<RpfmCliBackend>(rpfm_cli, game_key);
}

} // namespace wce
