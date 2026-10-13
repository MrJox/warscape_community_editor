#include "editor/steam_detect.h"

#include "foundation/process.h"

#ifdef _WIN32
#include <windows.h>
#include <commdlg.h>
#include <shobjidl.h>
#include <wrl/client.h>
#endif

#include <algorithm>
#include <cstdlib>
#include <fstream>
#include <regex>
#include <sstream>

namespace fs = std::filesystem;

namespace wce {

namespace {

#ifdef _WIN32
std::wstring registry_string(HKEY root, const wchar_t* key, const wchar_t* value) {
    wchar_t buf[1024];
    DWORD size = sizeof buf;
    if (RegGetValueW(root, key, value, RRF_RT_REG_SZ, nullptr, buf, &size) != ERROR_SUCCESS) return {};
    return buf;
}

// The Steam install folder, from the registry.
std::vector<fs::path> steam_roots() {
    std::wstring steam = registry_string(HKEY_CURRENT_USER, L"Software\\Valve\\Steam", L"SteamPath");
    if (steam.empty()) steam = registry_string(HKEY_LOCAL_MACHINE, L"SOFTWARE\\WOW6432Node\\Valve\\Steam", L"InstallPath");
    if (steam.empty()) return {};
    return {fs::path(steam)};
}
#else
fs::path home_dir() {
    const char* h = std::getenv("HOME");
    return h ? fs::path(h) : fs::path();
}

// The Steam install folders that exist: the native client, its ~/.steam link and the Flatpak one.
std::vector<fs::path> steam_roots() {
    std::vector<fs::path> out;
    const fs::path home = home_dir();
    if (home.empty()) return out;
    for (const fs::path& p : {home / ".local/share/Steam", home / ".steam/steam", home / ".var/app/com.valvesoftware.Steam/.local/share/Steam"}) {
        std::error_code ec;
        if (!fs::is_directory(p / "steamapps", ec)) continue;
        const fs::path canon = fs::canonical(p, ec);
        if (!ec && std::find(out.begin(), out.end(), canon) == out.end()) out.push_back(canon);
    }
    return out;
}

// The program `name` on $PATH, or empty.
fs::path find_in_path(const std::string& name) {
    const char* path = std::getenv("PATH");
    if (!path) return {};
    std::stringstream ss(path);
    std::string dir;
    while (std::getline(ss, dir, ':')) {
        std::error_code ec;
        const fs::path p = fs::path(dir.empty() ? "." : dir) / name;
        if (fs::is_regular_file(p, ec)) return p;
    }
    return {};
}

// A Win32 dialog filter ("Name\0*.a;*.b\0...\0") as (name, patterns separated by spaces) pairs.
std::vector<std::pair<std::string, std::string>> parse_filter(const wchar_t* filter) {
    std::vector<std::pair<std::string, std::string>> out;
    for (const wchar_t* at = filter; at && *at;) {
        const std::wstring name = at;
        at += name.size() + 1;
        if (!*at) break;
        std::wstring pats = at;
        at += pats.size() + 1;
        std::replace(pats.begin(), pats.end(), L';', L' ');
        out.emplace_back(narrow(name), narrow(pats));
    }
    return out;
}

// Runs a dialog program and returns the path it prints (empty when it was cancelled or is missing).
fs::path run_dialog(const fs::path& exe, const std::vector<std::wstring>& args) {
    if (exe.empty()) return {};
    ProcessResult r = run_process(exe, args);
    if (r.exit_code != 0) return {};
    std::string out = r.out;
    const size_t eol = out.find('\n');   // the first line: GTK may log warnings after it
    if (eol != std::string::npos) out.resize(eol);
    return out.empty() ? fs::path() : fs::path(out);
}
#endif

} // namespace

std::vector<fs::path> steam_library_folders() {
    std::vector<fs::path> out;
    for (const fs::path& root : steam_roots()) {
        if (std::find(out.begin(), out.end(), root) == out.end()) out.push_back(root);
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
            const fs::path lib(widen(unescaped));
            if (std::find(out.begin(), out.end(), lib) == out.end()) out.push_back(lib);
        }
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
#ifdef _WIN32
    wchar_t buf[MAX_PATH];
    if (GetEnvironmentVariableW(L"USERPROFILE", buf, MAX_PATH)) roots.push_back(fs::path(buf) / "Downloads");
    if (GetEnvironmentVariableW(L"ProgramFiles", buf, MAX_PATH)) roots.push_back(fs::path(buf));
#else
    for (const char* program : {"rpfm_cli", "rpfm_server"})   // distribution packages may ship only one of them
        if (const fs::path p = find_in_path(program); !p.empty()) return p.parent_path();
    if (const fs::path home = home_dir(); !home.empty()) roots.push_back(home / "Downloads");
    roots.push_back("/opt");
#endif
    const std::string cli = exe_name("rpfm_cli");
    for (const fs::path& r : roots) {
        std::error_code ec;
        if (fs::exists(r / cli, ec)) return r;
        for (fs::directory_iterator it(r, ec), end; !ec && it != end; it.increment(ec)) {
            const std::wstring name = it->path().filename().wstring();
            if (name.find(L"rpfm") != std::wstring::npos && fs::exists(it->path() / cli, ec)) return it->path();
        }
    }
    return {};
}

#ifdef _WIN32


fs::path file_dialog(bool save, const fs::path& initial, const wchar_t* filter) {
    wchar_t buf[MAX_PATH * 4] = {};
    const std::wstring init = initial.filename().wstring();
    wcsncpy(buf, init.c_str(), std::size(buf) - 1);
    const std::wstring dir = initial.parent_path().wstring();
    OPENFILENAMEW ofn{sizeof ofn};
    ofn.lpstrFilter = filter;
    ofn.lpstrFile = buf;
    ofn.nMaxFile = DWORD(std::size(buf));
    ofn.lpstrInitialDir = dir.empty() ? nullptr : dir.c_str();
    ofn.Flags = OFN_NOCHANGEDIR | (save ? OFN_OVERWRITEPROMPT : OFN_FILEMUSTEXIST);
    const BOOL ok = save ? GetSaveFileNameW(&ofn) : GetOpenFileNameW(&ofn);
    return ok ? fs::path(buf) : fs::path();
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

#else

// zenity (GTK) when it is installed, else kdialog (KDE).
fs::path file_dialog(bool save, const fs::path& initial, const wchar_t* filter) {
    const auto filters = parse_filter(filter);
    if (const fs::path zenity = find_in_path("zenity"); !zenity.empty()) {
        std::vector<std::wstring> args = {L"--file-selection"};
        if (save) {
            args.push_back(L"--save");
            args.push_back(L"--confirm-overwrite");
        }
        if (!initial.empty()) args.push_back(L"--filename=" + initial.wstring());
        for (const auto& [name, pats] : filters) args.push_back(widen("--file-filter=" + name + " | " + pats));
        return run_dialog(zenity, args);
    }
    std::string kfilter;
    for (const auto& [name, pats] : filters) kfilter += (kfilter.empty() ? "" : "\n") + pats + "|" + name;
    return run_dialog(find_in_path("kdialog"), {save ? L"--getsavefilename" : L"--getopenfilename", initial.empty() ? L"." : initial.wstring(), widen(kfilter)});
}

fs::path pick_folder(const std::wstring& title) {
    if (const fs::path zenity = find_in_path("zenity"); !zenity.empty()) return run_dialog(zenity, {L"--file-selection", L"--directory", L"--title=" + title});
    return run_dialog(find_in_path("kdialog"), {L"--getexistingdirectory", L".", L"--title", title});
}

#endif

} // namespace wce
