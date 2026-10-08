cmake_minimum_required(VERSION 3.24)
project(attila_tile_editor LANGUAGES CXX)

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

add_library(tile_formats STATIC src/formats/registry.cpp src/formats/xml_format.cpp src/formats/fastbin_tile.cpp)
target_include_directories(tile_formats PUBLIC src)
target_link_libraries(tile_formats PUBLIC attila_core)

add_executable(roundtrip tools/roundtrip.cpp)
target_link_libraries(roundtrip PRIVATE tile_formats)
