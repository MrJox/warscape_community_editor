# wce.exe: the unified editor.
add_executable(wce WIN32 main.cpp)
target_link_libraries(wce PRIVATE wce_tools wce_editor imgui ${WCE_APP_LIBS})
if(MSVC)
    target_compile_options(wce PRIVATE /Zi)
    target_link_options(wce PRIVATE /DEBUG)
endif()
