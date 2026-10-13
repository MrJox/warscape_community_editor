# Dear ImGui (vendored, DX11 + Win32 backends on Windows, DX11 on DXVK-native + SDL3 on Linux)
add_library(imgui STATIC
    imgui/imgui.cpp imgui/imgui_draw.cpp imgui/imgui_tables.cpp imgui/imgui_widgets.cpp imgui/imgui_demo.cpp imgui/backends/imgui_impl_dx11.cpp)
target_include_directories(imgui PUBLIC imgui imgui/backends)
if(WIN32)
    target_sources(imgui PRIVATE imgui/backends/imgui_impl_win32.cpp)
else()
    target_sources(imgui PRIVATE imgui/backends/imgui_impl_sdl3.cpp)
    target_link_libraries(imgui PUBLIC wce_d3dcompiler SDL3::SDL3)
endif()
if(MSVC)
    target_compile_options(imgui PRIVATE /W0)
endif()

# zlib 1.3.1 (vendored, deflate and inflate only)
add_library(zlib_vendor STATIC
    zlib/adler32.c zlib/compress.c zlib/crc32.c zlib/deflate.c zlib/inffast.c zlib/inflate.c zlib/inftrees.c zlib/trees.c
    zlib/uncompr.c zlib/zutil.c)
target_include_directories(zlib_vendor PUBLIC zlib)
if(MSVC)
    target_compile_options(zlib_vendor PRIVATE /W0)
endif()

# ImGuizmo (MIT): translate / rotate / scale gizmos in the viewport.
add_library(imguizmo STATIC imguizmo/ImGuizmo.cpp)
target_include_directories(imguizmo PUBLIC imguizmo)
target_link_libraries(imguizmo PUBLIC imgui)
if(MSVC)
    target_compile_options(imguizmo PRIVATE /W0)
endif()
