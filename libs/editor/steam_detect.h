#pragma once
// First-run helpers: find an installed Total War: Attila through Steam, and an RPFM folder next to the usual places. Windows only.
#include <filesystem>
#include <string>
#include <vector>

namespace wce {

// Steam library folders (from the registry's SteamPath and steamapps/libraryfolders.vdf).
std::vector<std::filesystem::path> steam_library_folders();
// <library>/steamapps/common/Total War Attila with data/data.pack, or empty.
std::filesystem::path detect_attila_folder();
// A folder that holds rpfm_cli.exe, looked for in the game folder's parent, the user's Downloads and Program Files, or empty.
std::filesystem::path detect_rpfm_folder(const std::filesystem::path& game_dir);
// Folder picker (IFileDialog); empty when cancelled.
std::filesystem::path pick_folder(const std::wstring& title);

} // namespace wce
