cmake_minimum_required(VERSION 3.24)
project(attila_weather_editor LANGUAGES CXX)

# Migration scaffolding: the editor application (Phase 5 moves it to tools/weather and app/). Compile options and the language
# standard come from the root CMakeLists.
add_executable(weather_editor WIN32 src/editor/main.cpp src/editor/app.cpp )
target_include_directories(weather_editor PRIVATE src)
target_link_libraries(weather_editor PRIVATE wce_weather wce_weather_ui wce_render imgui comdlg32 dbghelp)
if(MSVC)
    target_compile_options(weather_editor PRIVATE /Zi)
    target_link_options(weather_editor PRIVATE /DEBUG)
endif()

add_executable(vfx_check tools/vfx_check.cpp)
target_link_libraries(vfx_check PRIVATE wce_assets)

add_executable(env_vs_dump tools/env_vs_dump.cpp)
target_link_libraries(env_vs_dump PRIVATE wce_render)
