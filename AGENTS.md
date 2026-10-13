# Project guidance

Warscape Community Editor is an all-in-one Total War editor for weather, battle and campaign tiles and terrain tilemaps, units, cinematics, and related content. Use C++ with VS Code, CMake, and a cross-platform C++ compiler; the editor must run natively on Windows, macOS, and Linux. It is currently developed for Total War ATTILA but the architecture must be designed in a way that would allow to easily support other Total War games (starting from Rome 2 and ending with Pharaoh Dynasties).

Use RPFM to read game and mod data from packs. Use only the game's shaders. Keep visuals accurate and faithful to the game's rendering; deviations require explicit human permission.

For Total War development and modding in Claude Code, it is recommended to install the [Attila Modding Marketplace](https://github.com/MrJox/attila-modding-marketplace).

Configure and build with `cmake -S . -B build` and `cmake --build build --config Debug`. Run tests on demand with `ctest --test-dir build -C Debug --output-on-failure`; do not run tests after every change. Leave routine test runs to a human, or run them occasionally after major changes. Launch `build/bin/wce` (single-config builds), `build/bin/Debug/wce.exe` (Windows Visual Studio Debug build), or use VS Code's WCE launch configuration.

Use "grill me" technique when a feature implementation request is submitted.
