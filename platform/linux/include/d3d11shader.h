#pragma once
// DXVK-native's d3d11shader.h (from mingw-w64) lacks the shader version helpers of the Windows SDK header ...
#include_next <d3d11shader.h>

typedef enum D3D11_SHADER_VERSION_TYPE {
    D3D11_SHVER_PIXEL_SHADER = 0,
    D3D11_SHVER_VERTEX_SHADER = 1,
    D3D11_SHVER_GEOMETRY_SHADER = 2,
    D3D11_SHVER_HULL_SHADER = 3,
    D3D11_SHVER_DOMAIN_SHADER = 4,
    D3D11_SHVER_COMPUTE_SHADER = 5,
    D3D11_SHVER_RESERVED0 = 0xFFF0,
} D3D11_SHADER_VERSION_TYPE;

#define D3D11_SHVER_GET_TYPE(_Version) (((_Version) >> 16) & 0xffff)
#define D3D11_SHVER_GET_MAJOR(_Version) (((_Version) >> 4) & 0xf)
#define D3D11_SHVER_GET_MINOR(_Version) (((_Version) >> 0) & 0xf)

// ... and the __uuidof of the reflection interface (IID_PPV_ARGS); this is the current SDK's IID, the one vkd3d-utils' D3DReflect accepts.
__CRT_UUID_DECL(ID3D11ShaderReflection, 0x8d536ca1, 0x0cca, 0x4956, 0xa8, 0x37, 0x78, 0x69, 0x63, 0x75, 0x55, 0x84)
