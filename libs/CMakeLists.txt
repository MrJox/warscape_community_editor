# Dependency direction: foundation <- game <- vfs <- formats <- assets <- render <- scene <- editor <- tools/* <- app.
# formats and everything below it never include D3D or ImGui.
add_library(wce_foundation STATIC foundation/text_encoding.cpp foundation/process.cpp foundation/math.cpp)
target_include_directories(wce_foundation PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})

add_library(wce_vfs STATIC vfs/vfs.cpp)
target_link_libraries(wce_vfs PUBLIC wce_foundation)

add_library(wce_formats STATIC
    formats/xml_doc.cpp formats/env_value.cpp formats/fxc.cpp formats/rmv2.cpp formats/variation_db.cpp formats/variation_io.cpp
    formats/registry.cpp formats/xml_format.cpp formats/fastbin_tile.cpp formats/bmd_bin.cpp formats/markers.cpp
    formats/image_containers.cpp formats/chmf.cpp formats/png_file.cpp formats/tile_database.cpp formats/opaque_formats.cpp)
target_link_libraries(wce_formats PUBLIC wce_foundation zlib_vendor)

# D3D helpers that need no scene yet (the renderer joins in Phase 3).
add_library(wce_render STATIC render/image_io.cpp)
target_link_libraries(wce_render PUBLIC wce_formats d3d11)
