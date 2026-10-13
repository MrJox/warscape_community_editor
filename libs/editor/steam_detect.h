#pragma once
// First-run helpers: find an installed Total War: Attila through Steam, and an RPFM folder next to the usual places; file and folder dialogs
// (Win32 on Windows, zenity or kdialog on Linux).
#include <filesystem>
#include <string>
#include <vector>

namespace wce {

// Steam library folders (the registry's SteamPath, or the usual Linux Steam folders, and steamapps/libraryfolders.vdf).
std::vector<std::filesystem::path> steam_library_folders();
// <library>/steamapps/common/Total War Attila with data/data.pack, or empty.
std::filesystem::path detect_attila_folder();
// A folder that holds rpfm_cli(.exe), looked for in the game folder's parent, the user's Downloads and Program Files (Linux: rpfm_cli or
// rpfm_server on $PATH, then Downloads and /opt), or empty.
std::filesystem::path detect_rpfm_folder(const std::filesystem::path& game_dir);
// Open or save file dialog (`filter` is the Win32 list of NUL-separated pairs, see OPENFILENAME); empty when cancelled.
std::filesystem::path file_dialog(bool save, const std::filesystem::path& initial, const wchar_t* filter);
// Folder picker (IFileDialog on Windows); empty when cancelled.
std::filesystem::path pick_folder(const std::wstring& title);

} // namespace wce
