cmake_minimum_required(VERSION 3.24)
project(warscape_community_editor LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(MSVC)
    add_compile_options(/W4 /permissive- /utf-8 /MP /wd4100)
    add_compile_definitions(NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS UNICODE _UNICODE)
endif()

# Platform libraries. Windows: the system D3D11, WinHTTP and shell. Linux: DXVK-native + vkd3d-utils (cmake/linux_deps.cmake), libcurl, SDL3.
if(WIN32)
    set(WCE_D3D_LIBS d3d11 dxgi d3dcompiler dxguid)
    set(WCE_HTTP_LIBS winhttp)
    set(WCE_SHELL_LIBS ole32 shell32 advapi32)
    set(WCE_APP_LIBS dbghelp comdlg32)
else()
    include(cmake/linux_deps.cmake)
    find_package(SDL3 REQUIRED CONFIG)
    find_package(CURL REQUIRED)
    find_package(Threads REQUIRED)
    set(WCE_D3D_LIBS wce_d3dcompiler)
    set(WCE_HTTP_LIBS CURL::libcurl)
    set(WCE_SHELL_LIBS Threads::Threads)
    set(WCE_APP_LIBS SDL3::SDL3)
endif()

# Every executable lands in build/<dir>/bin/<config>, so scripts/gates.sh finds the gates and the editors in one place.
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

enable_testing()

# Include roots: libs/ and tools/ (#include "foundation/...", "formats/...", "tile/...").
add_subdirectory(third_party)
add_subdirectory(libs)
add_subdirectory(tools)
add_subdirectory(cli)
add_subdirectory(app)
