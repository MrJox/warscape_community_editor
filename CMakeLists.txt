cmake_minimum_required(VERSION 3.24)
project(attila_weather_editor LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(MSVC)
    add_compile_options(/W4 /permissive- /utf-8 /MP /wd4100)
    add_compile_definitions(NOMINMAX WIN32_LEAN_AND_MEAN _CRT_SECURE_NO_WARNINGS UNICODE _UNICODE)
endif()

# Core: file formats and data access, no rendering.
add_library(we_core STATIC
    src/core/text_encoding.cpp
    src/formats/xml_doc.cpp
)
target_include_directories(we_core PUBLIC src)

add_executable(xml_roundtrip tools/xml_roundtrip.cpp)
target_link_libraries(xml_roundtrip PRIVATE we_core)

target_sources(we_core PRIVATE src/formats/fxc.cpp src/core/process.cpp src/core/vfs.cpp src/formats/env_value.cpp)

add_executable(fxc_dump tools/fxc_dump.cpp)
target_link_libraries(fxc_dump PRIVATE we_core d3d11 d3dcompiler dxguid)

add_executable(vfs_probe tools/vfs_probe.cpp)
target_link_libraries(vfs_probe PRIVATE we_core)

add_executable(format_check tools/format_check.cpp)
target_link_libraries(format_check PRIVATE we_core)
