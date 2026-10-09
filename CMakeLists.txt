cmake_minimum_required(VERSION 3.24)
project(warscape_community_editor LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(MSVC)
    add_compile_options(/W4 /permissive- /utf-8 /MP /wd4100)
    add_compile_definitions(NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS UNICODE _UNICODE)
endif()

# Every executable lands in build/<dir>/bin/<config>, so scripts/gates.sh finds the gates and the editors in one place.
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

enable_testing()

# Include roots: libs/ and tools/ (#include "foundation/...", "formats/...", "tile/...").
add_subdirectory(third_party)
add_subdirectory(libs)
add_subdirectory(tools)
add_subdirectory(cli)

# Migration scaffolding (PLAN.md, Phase 8 removes it): the two editors keep building from the code that moved into libs/ and tools/
# until wce.exe reaches parity.
add_subdirectory(legacy/weather_editor)
add_subdirectory(legacy/tile_editor)
