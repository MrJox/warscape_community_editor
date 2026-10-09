# One library per editor tool.
add_library(wce_tile STATIC tile/ted_scene.cpp tile/world_transform.cpp tile/height_field.cpp tile/tile_project.cpp)
target_include_directories(wce_tile PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_tile PUBLIC wce_formats)

# Weather tool pieces that do not need a window (the application joins in Phase 5).
add_library(wce_weather STATIC weather/sample_map.cpp)
target_include_directories(wce_weather PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_weather PUBLIC wce_scene wce_vfs wce_formats)
