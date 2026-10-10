# Dependency direction: foundation <- game <- vfs <- formats <- assets <- render <- scene <- editor <- tools/* <- app.
# formats and everything below it never include D3D or ImGui.
add_library(wce_foundation STATIC foundation/text_encoding.cpp foundation/process.cpp foundation/math.cpp foundation/json.cpp foundation/settings.cpp foundation/log.cpp foundation/profile.cpp foundation/thread_pool.cpp)
target_include_directories(wce_foundation PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})

add_library(wce_game STATIC game/game_profile.cpp)
target_link_libraries(wce_game PUBLIC wce_foundation)

add_library(wce_vfs STATIC vfs/vfs.cpp vfs/mcp_client.cpp vfs/rpfm_server_backend.cpp vfs/rpfm_cli_backend.cpp)
target_link_libraries(wce_vfs PUBLIC wce_game wce_foundation winhttp)

add_library(wce_formats STATIC
    formats/xml_doc.cpp formats/env_value.cpp formats/fxc.cpp formats/rmv2.cpp formats/variation_db.cpp formats/variation_io.cpp
    formats/registry.cpp formats/xml_format.cpp formats/fastbin_tile.cpp formats/bmd_bin.cpp formats/markers.cpp
    formats/image_containers.cpp formats/chmf.cpp formats/png_file.cpp formats/tile_database.cpp formats/opaque_formats.cpp
    formats/tile_entry.cpp formats/tile_index.cpp formats/dds_decode.cpp formats/tile_climate.cpp)
target_link_libraries(wce_formats PUBLIC wce_foundation zlib_vendor)

# CPU asset loading on top of the VFS (weather vfx definitions; models and materials join later).
add_library(wce_assets STATIC assets/vfx_def.cpp assets/vfx_constants.cpp assets/weather_vfx.cpp)
target_link_libraries(wce_assets PUBLIC wce_vfs wce_formats)

# The shared render/editor scene and the data it carries (no GPU objects).
add_library(wce_scene STATIC scene/environment.cpp scene/clouds.cpp scene/render_scene.cpp)
target_link_libraries(wce_scene PUBLIC wce_formats)

# D3D11 renderer on the game's own shaders.
add_library(wce_render STATIC
    render/image_io.cpp render/render_target.cpp render/shader_lib.cpp render/texture.cpp render/wind.cpp render/model_cache.cpp render/particles.cpp render/editor_passes.cpp
    render/environment_apply.cpp render/renderer.cpp render/gpu_adapter.cpp render/gpu_timer.cpp render/draw_set.cpp render/water_pass.cpp)
target_link_libraries(wce_render PUBLIC wce_scene wce_assets wce_vfs wce_formats d3d11 dxgi d3dcompiler dxguid)

# The editor shell. editor_core has no ImGui or GPU code (commands, documents, selection, command registry, camera controllers) and is what
# editor_check tests; editor is the dockspace, panels, viewport and gizmos on top.
add_library(wce_editor_core STATIC editor/command.cpp editor/document_manager.cpp editor/command_registry.cpp editor/camera_controller.cpp)
target_link_libraries(wce_editor_core PUBLIC wce_scene wce_foundation)

add_library(wce_editor STATIC editor/editor.cpp editor/viewport.cpp editor/panels.cpp editor/steam_detect.cpp)
target_link_libraries(wce_editor PUBLIC wce_editor_core wce_render wce_vfs wce_game imgui imguizmo ole32 shell32 advapi32)
