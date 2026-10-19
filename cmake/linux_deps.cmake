# Linux: the D3D11 renderer runs on DXVK-native (D3D11 and DXGI on Vulkan, SDL3 windows) and the shader compiler/reflection come from
# vkd3d-utils. Both are fetched and built once, at configure time, into <build>/_deps/prefix; later configures reuse them.
# Needs: git, meson + ninja, glslang, a C/C++ compiler, autotools + bison + flex + widl (wine), perl, and the SDL3 and Vulkan
# development files.

set(WCE_DXVK_TAG "v3.1.1" CACHE STRING "DXVK-native release to build")
set(WCE_VKD3D_TAG "vkd3d-2.1" CACHE STRING "vkd3d release to build")
set(WCE_SPIRV_HEADERS_TAG "vulkan-sdk-1.4.321.0" CACHE STRING "SPIRV-Headers tag (vkd3d needs them)")

set(WCE_DEPS_DIR "${CMAKE_BINARY_DIR}/_deps")
set(WCE_DEPS_PREFIX "${WCE_DEPS_DIR}/prefix")

function(wce_dep_run what)
    execute_process(COMMAND ${ARGN} RESULT_VARIABLE rc OUTPUT_VARIABLE out ERROR_VARIABLE out)
    if(NOT rc EQUAL 0)
        message(FATAL_ERROR "${what} failed:\n${out}")
    endif()
endfunction()

function(wce_dep_clone name url tag)
    set(dir "${WCE_DEPS_DIR}/${name}")
    if(NOT EXISTS "${dir}/.wce_cloned")
        file(REMOVE_RECURSE "${dir}")
        message(STATUS "Fetching ${name} ${tag}")
        wce_dep_run("git clone ${name}" git -c advice.detachedHead=false clone --quiet --depth 1 --branch ${tag} --recurse-submodules --shallow-submodules ${url} "${dir}")
        file(WRITE "${dir}/.wce_cloned" "${tag}")
    endif()
endfunction()

find_program(WCE_MESON meson REQUIRED)
find_program(WCE_NINJA ninja REQUIRED)
find_program(WCE_MAKE make REQUIRED)
include(ProcessorCount)
ProcessorCount(WCE_JOBS)
if(WCE_JOBS EQUAL 0)
    set(WCE_JOBS 4)
endif()

# ---- DXVK-native: libdxvk_d3d11.so, libdxvk_dxgi.so and the Windows/D3D headers (include/dxvk)
# One local change, backported from upstream master (not in a release yet): D3D11SwapChain::ChangeProperties gave the presenter the old size on
# ResizeBuffers. Windows and X11 surfaces report their size, so it did not matter there; a Wayland surface does not, and a maximized window
# kept its first swapchain, stretched by the compositor.
set(WCE_DXVK_STAMP "${WCE_DEPS_PREFIX}/.dxvk-${WCE_DXVK_TAG}-resize")
if(NOT EXISTS "${WCE_DXVK_STAMP}")
    wce_dep_clone(dxvk https://github.com/doitsujin/dxvk.git ${WCE_DXVK_TAG})
    set(dxvk_swapchain "${WCE_DEPS_DIR}/dxvk/src/d3d11/d3d11_swapchain.cpp")
    file(READ "${dxvk_swapchain}" dxvk_src)
    set(dxvk_old "if (m_desc.Width != pDesc->Width || m_desc.Height != pDesc->Height)\n      m_presenter->setSurfaceExtent({ m_desc.Width, m_desc.Height });")
    set(dxvk_new "if (m_desc.Width != pDesc->Width || m_desc.Height != pDesc->Height)\n      m_presenter->setSurfaceExtent({ pDesc->Width, pDesc->Height });")
    string(FIND "${dxvk_src}" "${dxvk_old}" at)
    if(NOT at EQUAL -1)
        string(REPLACE "${dxvk_old}" "${dxvk_new}" dxvk_src "${dxvk_src}")
        file(WRITE "${dxvk_swapchain}" "${dxvk_src}")
    else()
        string(FIND "${dxvk_src}" "${dxvk_new}" at)
        if(at EQUAL -1)
            message(FATAL_ERROR "DXVK: the swapchain resize backport no longer applies to ${WCE_DXVK_TAG}")
        endif()
    endif()
    message(STATUS "Building DXVK-native (a few minutes, once)")
    file(REMOVE_RECURSE "${WCE_DEPS_DIR}/dxvk/build")
    wce_dep_run("DXVK meson setup" ${WCE_MESON} setup "${WCE_DEPS_DIR}/dxvk/build" "${WCE_DEPS_DIR}/dxvk" --buildtype=release --prefix=${WCE_DEPS_PREFIX} --libdir=lib
        -Denable_d3d8=false -Denable_d3d9=false -Denable_d3d10=false -Dnative_sdl3=enabled -Dnative_sdl2=disabled -Dnative_glfw=disabled)
    wce_dep_run("DXVK build" ${WCE_NINJA} -C "${WCE_DEPS_DIR}/dxvk/build" install)
    file(WRITE "${WCE_DXVK_STAMP}" "")
endif()

# ---- vkd3d: libvkd3d-utils.so (D3DCompile, D3DReflect, D3DDisassemble on vkd3d-shader)
# vkd3d declares WINAPI and the COM methods ms_abi on x86-64 (for Wine); DXVK-native's headers, which the editor compiles against, use the
# native ABI. The build is patched to the native ABI so that the exports and the blob/reflection vtables match those headers.
set(WCE_VKD3D_STAMP "${WCE_DEPS_PREFIX}/.vkd3d-${WCE_VKD3D_TAG}-sysv")
if(NOT EXISTS "${WCE_VKD3D_STAMP}")
    wce_dep_clone(spirv-headers https://github.com/KhronosGroup/SPIRV-Headers.git ${WCE_SPIRV_HEADERS_TAG})
    wce_dep_clone(vkd3d https://gitlab.winehq.org/wine/vkd3d.git ${WCE_VKD3D_TAG})
    message(STATUS "Building vkd3d (a few minutes, once)")
    # vkd3d's build scripts `use JSON`; the core JSON::PP does the same job when libjson-perl is not installed.
    file(WRITE "${WCE_DEPS_DIR}/perl/JSON.pm" "package JSON;\nuse JSON::PP ();\nuse Exporter 'import';\nour @EXPORT = qw(decode_json encode_json);\n"
        "sub decode_json { JSON::PP::decode_json(@_) }\nsub encode_json { JSON::PP::encode_json(@_) }\n1;\n")
    set(perl_env "PERL5LIB=${WCE_DEPS_DIR}/perl:$ENV{PERL5LIB}")
    set(src "${WCE_DEPS_DIR}/vkd3d")
    file(READ "${src}/include/vkd3d_windows.h" vkd3d_windows)
    set(ms_abi "#  define __stdcall __attribute__((ms_abi))")
    string(FIND "${vkd3d_windows}" "${ms_abi}" at)
    if(NOT at EQUAL -1)
        string(REPLACE "${ms_abi}" "#  define __stdcall /* native ABI, patched by warscape_community_editor */" vkd3d_windows "${vkd3d_windows}")
        file(WRITE "${src}/include/vkd3d_windows.h" "${vkd3d_windows}")
    elseif(NOT vkd3d_windows MATCHES "patched by warscape_community_editor")
        message(FATAL_ERROR "vkd3d ${WCE_VKD3D_TAG}: include/vkd3d_windows.h no longer has the ms_abi line cmake/linux_deps.cmake patches")
    endif()
    wce_dep_run("vkd3d autogen" ${CMAKE_COMMAND} -E chdir "${src}" ${CMAKE_COMMAND} -E env ${perl_env} ./autogen.sh)
    file(REMOVE_RECURSE "${src}/build")
    file(MAKE_DIRECTORY "${src}/build")
    wce_dep_run("vkd3d configure" ${CMAKE_COMMAND} -E chdir "${src}/build" ${CMAKE_COMMAND} -E env ${perl_env}
        ../configure --prefix=${WCE_DEPS_PREFIX} --libdir=${WCE_DEPS_PREFIX}/lib --disable-tests --disable-demos --disable-doxygen-doc
        --without-ncurses --without-xcb "CPPFLAGS=-I${WCE_DEPS_DIR}/spirv-headers/include")
    wce_dep_run("vkd3d build" ${CMAKE_COMMAND} -E chdir "${src}/build" ${CMAKE_COMMAND} -E env ${perl_env} ${WCE_MAKE} -j${WCE_JOBS})
    wce_dep_run("vkd3d install" ${CMAKE_COMMAND} -E chdir "${src}/build" ${WCE_MAKE} install)
    file(WRITE "${WCE_VKD3D_STAMP}" "")
endif()
