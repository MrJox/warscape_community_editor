# One library per editor tool.
add_library(wce_tile STATIC tile/ted_scene.cpp tile/world_transform.cpp tile/height_field.cpp tile/tile_project.cpp)
target_include_directories(wce_tile PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_tile PUBLIC wce_formats)

# Weather tool pieces that need no window: the sample map, the weather files as documents (XmlDoc + undo).
add_library(wce_weather STATIC weather/sample_map.cpp weather/sample_map_document.cpp weather/xml_file_document.cpp)
target_include_directories(wce_weather PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_weather PUBLIC wce_scene wce_vfs wce_formats wce_editor_core)

# The weather selection database editor widgets (also used by the legacy weather_editor until Phase 8).
add_library(wce_weather_ui STATIC weather/variation_editor.cpp)
target_include_directories(wce_weather_ui PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_weather_ui PUBLIC wce_formats wce_vfs imgui)

# The tools as editor plug-ins (Tool + Documents). Placeholders for the future tools register today so that the switcher, the layout
# files and document routing are exercised.
add_library(wce_tools STATIC
    weather/weather_tool.cpp weather/weather_commands.cpp weather/weather_picker.cpp
    tile/tile_tool.cpp unit/unit_tool.cpp vfx/vfx_tool.cpp terrain/terrain_tool.cpp cinematic/cinematic_tool.cpp)
target_include_directories(wce_tools PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(wce_tools PUBLIC wce_weather wce_weather_ui wce_editor)
