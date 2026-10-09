# One library per editor tool.
add_library(wce_tile STATIC tile/ted_scene.cpp tile/world_transform.cpp tile/height_field.cpp tile/tile_project.cpp)
target_include_directories(wce_tile PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_tile PUBLIC wce_formats)
