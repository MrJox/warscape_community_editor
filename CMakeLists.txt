cmake_minimum_required(VERSION 3.24)
project(attila_tile_editor LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(MSVC)
    add_compile_options(/W4 /permissive- /utf-8 /MP /wd4100)
    add_compile_definitions(NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS UNICODE _UNICODE)
endif()

# attila_core: the shared lossless-format code. For now it compiles the weather_editor sources in place
# (weather_editor is under active edit by other sessions, so nothing is moved yet). When that tree is
# quiet, the files move to ../attila_core and weather_editor links the same target.
set(WE ${CMAKE_CURRENT_SOURCE_DIR}/../weather_editor/src)
add_library(attila_core STATIC
    ${WE}/core/text_encoding.cpp
    ${WE}/formats/xml_doc.cpp
    ${WE}/formats/variation_db.cpp
    ${WE}/formats/variation_io.cpp)
target_include_directories(attila_core PUBLIC ${WE})

add_library(zlib_vendor STATIC
    third_party/zlib/adler32.c third_party/zlib/compress.c third_party/zlib/crc32.c third_party/zlib/deflate.c
    third_party/zlib/inffast.c third_party/zlib/inflate.c third_party/zlib/inftrees.c third_party/zlib/trees.c
    third_party/zlib/uncompr.c third_party/zlib/zutil.c)
target_include_directories(zlib_vendor PUBLIC third_party/zlib)
if(MSVC)
    target_compile_options(zlib_vendor PRIVATE /W0)
endif()

add_library(tile_formats STATIC src/formats/registry.cpp src/formats/xml_format.cpp src/formats/fastbin_tile.cpp src/formats/bmd_bin.cpp src/formats/markers.cpp src/formats/image_containers.cpp src/formats/chmf.cpp src/formats/png_file.cpp src/formats/tile_database.cpp src/formats/opaque_formats.cpp)
target_include_directories(tile_formats PUBLIC src)
target_link_libraries(tile_formats PUBLIC attila_core zlib_vendor)

add_executable(roundtrip tools/roundtrip.cpp)
target_link_libraries(roundtrip PRIVATE tile_formats)

add_executable(order_test tools/order_test.cpp)

add_executable(tdb_build tools/tdb_build.cpp)
target_link_libraries(tdb_build PRIVATE tile_formats)

add_executable(png_debug tools/png_debug.cpp)
target_link_libraries(png_debug PRIVATE tile_formats)

add_library(tile_scene STATIC src/scene/ted_scene.cpp)
target_include_directories(tile_scene PUBLIC src)
target_link_libraries(tile_scene PUBLIC attila_core)
add_executable(scene_check tools/scene_check.cpp)
target_link_libraries(scene_check PRIVATE tile_scene)

add_library(tile_terrain STATIC src/terrain/height_field.cpp)
target_include_directories(tile_terrain PUBLIC src)
target_link_libraries(tile_terrain PUBLIC tile_formats)
add_executable(height_check tools/height_check.cpp)
target_link_libraries(height_check PRIVATE tile_terrain)

# ---- Tile Studio (viewer / editor). Dear ImGui and the PNG screenshot helper are compiled from weather_editor in place.
set(WEDIT ${CMAKE_CURRENT_SOURCE_DIR}/../weather_editor)
add_library(imgui_studio STATIC
    ${WEDIT}/third_party/imgui/imgui.cpp ${WEDIT}/third_party/imgui/imgui_draw.cpp ${WEDIT}/third_party/imgui/imgui_tables.cpp
    ${WEDIT}/third_party/imgui/imgui_widgets.cpp ${WEDIT}/third_party/imgui/imgui_demo.cpp
    ${WEDIT}/third_party/imgui/backends/imgui_impl_dx11.cpp ${WEDIT}/third_party/imgui/backends/imgui_impl_win32.cpp)
target_include_directories(imgui_studio PUBLIC ${WEDIT}/third_party/imgui ${WEDIT}/third_party/imgui/backends)
if(MSVC)
    target_compile_options(imgui_studio PRIVATE /W0)
endif()
add_library(tile_project STATIC src/project/tile_project.cpp src/scene/world_transform.cpp)
target_include_directories(tile_project PUBLIC src)
target_link_libraries(tile_project PUBLIC tile_formats tile_scene tile_terrain)
add_library(tile_weather STATIC src/weather/environment_map.cpp)
target_include_directories(tile_weather PUBLIC src)
target_link_libraries(tile_weather PUBLIC attila_core)
add_executable(tile_studio WIN32 src/app/main.cpp src/app/studio.cpp ${WE}/render/image_io.cpp)
target_include_directories(tile_studio PRIVATE ${WE})
target_link_libraries(tile_studio PRIVATE tile_project tile_weather imgui_studio d3d11 dxgi d3dcompiler dxguid windowscodecs ole32 comdlg32)

add_executable(unit_tests tools/unit_tests.cpp src/scene/world_transform.cpp)
target_link_libraries(unit_tests PRIVATE tile_scene)

add_executable(weather_check tools/weather_check.cpp)
target_link_libraries(weather_check PRIVATE tile_weather)
