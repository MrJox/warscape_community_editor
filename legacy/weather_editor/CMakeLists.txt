cmake_minimum_required(VERSION 3.24)
project(attila_weather_editor LANGUAGES CXX)

# Migration scaffolding: what has not moved into libs/ yet (vfx and the renderer, Phase 3) plus the editor application.
# Compile options and the language standard come from the root CMakeLists.
add_library(we_legacy STATIC src/vfx/vfx_def.cpp src/vfx/vfx_constants.cpp src/vfx/weather_vfx.cpp)
target_include_directories(we_legacy PUBLIC src)
target_link_libraries(we_legacy PUBLIC wce_vfs wce_formats)

add_executable(weather_editor WIN32 src/editor/main.cpp src/editor/app.cpp src/editor/variation_editor.cpp
    src/render/texture.cpp src/render/shader_lib.cpp src/render/scene.cpp
    src/render/environment.cpp src/render/renderer.cpp src/render/clouds.cpp src/render/wind.cpp src/vfx/particles.cpp)
target_link_libraries(weather_editor PRIVATE we_legacy wce_render imgui d3d11 dxgi d3dcompiler dxguid comdlg32 dbghelp)
if(MSVC)
    target_compile_options(weather_editor PRIVATE /Zi)
    target_link_options(weather_editor PRIVATE /DEBUG)
endif()

add_executable(vfx_check tools/vfx_check.cpp)
target_link_libraries(vfx_check PRIVATE we_legacy)

add_executable(env_vs_dump tools/env_vs_dump.cpp src/render/environment.cpp src/render/shader_lib.cpp src/render/clouds.cpp)
target_link_libraries(env_vs_dump PRIVATE we_legacy d3d11 d3dcompiler dxguid)
