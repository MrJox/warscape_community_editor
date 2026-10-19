# Linux only: builds DXVK-native and vkd3d-utils into build/_deps/prefix (cmake/linux_deps.cmake), which crates/wce_d3d11/build.rs links.
# The editor itself is the Cargo workspace (Cargo.toml). Windows needs nothing from here.
cmake_minimum_required(VERSION 3.24)
project(wce_linux_deps NONE)

if(NOT WIN32)
    include(cmake/linux_deps.cmake)
endif()
