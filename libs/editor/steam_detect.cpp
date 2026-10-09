#include "editor/steam_detect.h"

#include "foundation/process.h"

#include <windows.h>
#include <shobjidl.h>
#include <wrl/client.h>

#include <algorithm>
#include <fstream>
#include <regex>
#include <sstream>

namespace fs = std::filesystem;

namespace wce {

namespace {

std::wstring registry_string(HKEY root, const wchar_t* key, const wchar_t* value) {
    wchar_t buf[1024];
    DWORD size = sizeof buf;
    if (RegGetValueW(root, key, value, RRF_RT_REG_SZ, nullptr, buf, &size) != ERROR_SUCCESS) return {};
    return buf;
}

} // namespace

std::vector<fs::path> steam_library_folders() {
    std::vector<fs::path> out;
    std::wstring steam = registry_string(HKEY_CURRENT_USER, L"Software\\Valve\\Steam", L"SteamPath");
    if (steam.empty()) steam = registry_string(HKEY_LOCAL_MACHINE, L"SOFTWARE\\WOW6432Node\\Valve\\Steam", L"InstallPath");
    if (steam.empty()) return out;
    const fs::path root = fs::path(steam);
    out.push_back(root);
    std::ifstream f(root / "steamapps" / "libraryfolders.vdf");
    std::stringstream ss;
    ss << f.rdbuf();
    const std::string text = ss.str();
    static const std::regex re(R"re("path"\s+"([^"]+)")re");
    for (auto it = std::sregex_iterator(text.begin(), text.end(), re); it != std::sregex_iterator(); ++it) {
        std::string p = (*it)[1];
        std::string unescaped;
        for (size_t i = 0; i < p.size(); ++i) {   // vdf doubles its backslashes
            unescaped += p[i];
            if (p[i] == '\\' && i + 1 < p.size() && p[i + 1] == '\\') ++i;
        }
        out.push_back(fs::path(widen(unescaped)));
    }
    return out;
}

fs::path detect_attila_folder() {
    for (const fs::path& lib : steam_library_folders()) {
        const fs::path g = lib / "steamapps" / "common" / "Total War Attila";
        std::error_code ec;
        if (fs::exists(g / "data" / "data.pack", ec)) return g;
    }
    return {};
}

fs::path detect_rpfm_folder(const fs::path& game_dir) {
    std::vector<fs::path> roots;
    if (!game_dir.empty()) roots.push_back(game_dir.parent_path());
    wchar_t buf[MAX_PATH];
    if (GetEnvironmentVariableW(L"USERPROFILE", buf, MAX_PATH)) roots.push_back(fs::path(buf) / "Downloads");
    if (GetEnvironmentVariableW(L"ProgramFiles", buf, MAX_PATH)) roots.push_back(fs::path(buf));
    for (const fs::path& r : roots) {
        std::error_code ec;
        if (fs::exists(r / "rpfm_cli.exe", ec)) return r;
        for (fs::directory_iterator it(r, ec), end; !ec && it != end; it.increment(ec)) {
            const std::wstring name = it->path().filename().wstring();
            if (name.find(L"rpfm") != std::wstring::npos && fs::exists(it->path() / "rpfm_cli.exe", ec)) return it->path();
        }
    }
    return {};
}

fs::path pick_folder(const std::wstring& title) {
    Microsoft::WRL::ComPtr<IFileDialog> dlg;
    if (FAILED(CoCreateInstance(CLSID_FileOpenDialog, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&dlg)))) return {};
    DWORD opts = 0;
    dlg->GetOptions(&opts);
    dlg->SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
    dlg->SetTitle(title.c_str());
    if (FAILED(dlg->Show(nullptr))) return {};
    Microsoft::WRL::ComPtr<IShellItem> item;
    if (FAILED(dlg->GetResult(&item))) return {};
    PWSTR p = nullptr;
    if (FAILED(item->GetDisplayName(SIGDN_FILESYSPATH, &p))) return {};
    fs::path out(p);
    CoTaskMemFree(p);
    return out;
}

} // namespace wce
