cmake_minimum_required(VERSION 3.24)
project(attila_tile_editor LANGUAGES C CXX)

# Migration scaffolding: Tile Studio (viewer / editor). Compile options and the language standard come from the root CMakeLists.
add_executable(tile_studio WIN32 src/app/main.cpp src/app/studio.cpp)
target_include_directories(tile_studio PRIVATE src)
target_link_libraries(tile_studio PRIVATE wce_tile wce_render imgui d3d11 dxgi d3dcompiler dxguid comdlg32)
