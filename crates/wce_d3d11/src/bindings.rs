#[inline]
pub unsafe fn CreateDXGIFactory1<T>() -> windows_core::Result<T>
where
    T: windows_core::Interface,
{
    windows_core::link!("dxgi.dll" "system" fn CreateDXGIFactory1(riid : *const windows_core::GUID, ppfactory : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
    let mut result__ = core::ptr::null_mut();
    unsafe {
        CreateDXGIFactory1(&T::IID, &mut result__)
            .and_then(|| windows_core::imp::Type::from_abi(result__))
    }
}
#[inline]
pub unsafe fn D3D11CreateDevice<P0>(
    padapter: P0,
    drivertype: D3D_DRIVER_TYPE,
    software: HMODULE,
    flags: u32,
    pfeaturelevels: Option<&[D3D_FEATURE_LEVEL]>,
    sdkversion: u32,
    ppdevice: Option<*mut Option<ID3D11Device>>,
    pfeaturelevel: Option<*mut D3D_FEATURE_LEVEL>,
    ppimmediatecontext: Option<*mut Option<ID3D11DeviceContext>>,
) -> windows_core::HRESULT
where
    P0: windows_core::Param<IDXGIAdapter>,
{
    windows_core::link!("d3d11.dll" "system" fn D3D11CreateDevice(padapter : *mut core::ffi::c_void, drivertype : D3D_DRIVER_TYPE, software : HMODULE, flags : u32, pfeaturelevels : *const D3D_FEATURE_LEVEL, featurelevels : u32, sdkversion : u32, ppdevice : *mut *mut core::ffi::c_void, pfeaturelevel : *mut D3D_FEATURE_LEVEL, ppimmediatecontext : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
    unsafe {
        D3D11CreateDevice(
            padapter.param().abi(),
            drivertype,
            software,
            flags,
            pfeaturelevels.map_or(core::ptr::null(), |slice| slice.as_ptr()),
            pfeaturelevels.map_or(0, |slice| slice.len().try_into().unwrap()),
            sdkversion,
            ppdevice.unwrap_or(core::mem::zeroed()) as _,
            pfeaturelevel.unwrap_or(core::mem::zeroed()) as _,
            ppimmediatecontext.unwrap_or(core::mem::zeroed()) as _,
        )
    }
}
#[inline]
pub unsafe fn D3D11CreateDeviceAndSwapChain<P0>(
    padapter: P0,
    drivertype: D3D_DRIVER_TYPE,
    software: HMODULE,
    flags: u32,
    pfeaturelevels: Option<&[D3D_FEATURE_LEVEL]>,
    sdkversion: u32,
    pswapchaindesc: Option<*const DXGI_SWAP_CHAIN_DESC>,
    ppswapchain: Option<*mut Option<IDXGISwapChain>>,
    ppdevice: Option<*mut Option<ID3D11Device>>,
    pfeaturelevel: Option<*mut D3D_FEATURE_LEVEL>,
    ppimmediatecontext: Option<*mut Option<ID3D11DeviceContext>>,
) -> windows_core::HRESULT
where
    P0: windows_core::Param<IDXGIAdapter>,
{
    windows_core::link!("d3d11.dll" "system" fn D3D11CreateDeviceAndSwapChain(padapter : *mut core::ffi::c_void, drivertype : D3D_DRIVER_TYPE, software : HMODULE, flags : u32, pfeaturelevels : *const D3D_FEATURE_LEVEL, featurelevels : u32, sdkversion : u32, pswapchaindesc : *const DXGI_SWAP_CHAIN_DESC, ppswapchain : *mut *mut core::ffi::c_void, ppdevice : *mut *mut core::ffi::c_void, pfeaturelevel : *mut D3D_FEATURE_LEVEL, ppimmediatecontext : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            padapter.param().abi(),
            drivertype,
            software,
            flags,
            pfeaturelevels.map_or(core::ptr::null(), |slice| slice.as_ptr()),
            pfeaturelevels.map_or(0, |slice| slice.len().try_into().unwrap()),
            sdkversion,
            pswapchaindesc.unwrap_or(core::mem::zeroed()) as _,
            ppswapchain.unwrap_or(core::mem::zeroed()) as _,
            ppdevice.unwrap_or(core::mem::zeroed()) as _,
            pfeaturelevel.unwrap_or(core::mem::zeroed()) as _,
            ppimmediatecontext.unwrap_or(core::mem::zeroed()) as _,
        )
    }
}
#[inline]
pub unsafe fn D3DCompile<P2, P4, P5, P6>(
    psrcdata: *const core::ffi::c_void,
    srcdatasize: usize,
    psourcename: P2,
    pdefines: Option<*const D3D_SHADER_MACRO>,
    pinclude: P4,
    pentrypoint: P5,
    ptarget: P6,
    flags1: u32,
    flags2: u32,
    ppcode: *mut Option<ID3D10Blob>,
    pperrormsgs: *mut Option<ID3D10Blob>,
) -> windows_core::HRESULT
where
    P2: windows_core::Param<windows_core::PCSTR>,
    P4: windows_core::Param<ID3DInclude>,
    P5: windows_core::Param<windows_core::PCSTR>,
    P6: windows_core::Param<windows_core::PCSTR>,
{
    windows_core::link!("d3dcompiler_47.dll" "system" fn D3DCompile(psrcdata : *const core::ffi::c_void, srcdatasize : usize, psourcename : windows_core::PCSTR, pdefines : *const D3D_SHADER_MACRO, pinclude : *mut core::ffi::c_void, pentrypoint : windows_core::PCSTR, ptarget : windows_core::PCSTR, flags1 : u32, flags2 : u32, ppcode : *mut *mut core::ffi::c_void, pperrormsgs : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
    unsafe {
        D3DCompile(
            psrcdata,
            srcdatasize,
            psourcename.param().abi(),
            pdefines.unwrap_or(core::mem::zeroed()) as _,
            pinclude.param().abi(),
            pentrypoint.param().abi(),
            ptarget.param().abi(),
            flags1,
            flags2,
            core::mem::transmute(ppcode),
            core::mem::transmute(pperrormsgs),
        )
    }
}
#[inline]
pub unsafe fn D3DReflect(
    psrcdata: *const core::ffi::c_void,
    srcdatasize: usize,
    pinterface: *const windows_core::GUID,
    ppreflector: *mut *mut core::ffi::c_void,
) -> windows_core::HRESULT {
    windows_core::link!("d3dcompiler_47.dll" "system" fn D3DReflect(psrcdata : *const core::ffi::c_void, srcdatasize : usize, pinterface : *const windows_core::GUID, ppreflector : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
    unsafe { D3DReflect(psrcdata, srcdatasize, pinterface, ppreflector as _) }
}
pub const D3D10_1_SRV_DIMENSION_BUFFER: D3D_SRV_DIMENSION = 1;
pub const D3D10_1_SRV_DIMENSION_TEXTURE1D: D3D_SRV_DIMENSION = 2;
pub const D3D10_1_SRV_DIMENSION_TEXTURE1DARRAY: D3D_SRV_DIMENSION = 3;
pub const D3D10_1_SRV_DIMENSION_TEXTURE2D: D3D_SRV_DIMENSION = 4;
pub const D3D10_1_SRV_DIMENSION_TEXTURE2DARRAY: D3D_SRV_DIMENSION = 5;
pub const D3D10_1_SRV_DIMENSION_TEXTURE2DMS: D3D_SRV_DIMENSION = 6;
pub const D3D10_1_SRV_DIMENSION_TEXTURE2DMSARRAY: D3D_SRV_DIMENSION = 7;
pub const D3D10_1_SRV_DIMENSION_TEXTURE3D: D3D_SRV_DIMENSION = 8;
pub const D3D10_1_SRV_DIMENSION_TEXTURECUBE: D3D_SRV_DIMENSION = 9;
pub const D3D10_1_SRV_DIMENSION_TEXTURECUBEARRAY: D3D_SRV_DIMENSION = 10;
pub const D3D10_1_SRV_DIMENSION_UNKNOWN: D3D_SRV_DIMENSION = 0;
pub const D3D10_CT_CBUFFER: D3D_CBUFFER_TYPE = 0;
pub const D3D10_CT_TBUFFER: D3D_CBUFFER_TYPE = 1;
pub const D3D10_PRIMITIVE_TOPOLOGY_LINELIST: D3D_PRIMITIVE_TOPOLOGY = 2;
pub const D3D10_PRIMITIVE_TOPOLOGY_LINELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 10;
pub const D3D10_PRIMITIVE_TOPOLOGY_LINESTRIP: D3D_PRIMITIVE_TOPOLOGY = 3;
pub const D3D10_PRIMITIVE_TOPOLOGY_LINESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 11;
pub const D3D10_PRIMITIVE_TOPOLOGY_POINTLIST: D3D_PRIMITIVE_TOPOLOGY = 1;
pub const D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST: D3D_PRIMITIVE_TOPOLOGY = 4;
pub const D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 12;
pub const D3D10_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP: D3D_PRIMITIVE_TOPOLOGY = 5;
pub const D3D10_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 13;
pub const D3D10_PRIMITIVE_TOPOLOGY_UNDEFINED: D3D_PRIMITIVE_TOPOLOGY = 0;
pub const D3D10_SIT_CBUFFER: D3D_SHADER_INPUT_TYPE = 0;
pub const D3D10_SIT_SAMPLER: D3D_SHADER_INPUT_TYPE = 3;
pub const D3D10_SIT_TBUFFER: D3D_SHADER_INPUT_TYPE = 1;
pub const D3D10_SIT_TEXTURE: D3D_SHADER_INPUT_TYPE = 2;
pub const D3D10_SRV_DIMENSION_BUFFER: D3D_SRV_DIMENSION = 1;
pub const D3D10_SRV_DIMENSION_TEXTURE1D: D3D_SRV_DIMENSION = 2;
pub const D3D10_SRV_DIMENSION_TEXTURE1DARRAY: D3D_SRV_DIMENSION = 3;
pub const D3D10_SRV_DIMENSION_TEXTURE2D: D3D_SRV_DIMENSION = 4;
pub const D3D10_SRV_DIMENSION_TEXTURE2DARRAY: D3D_SRV_DIMENSION = 5;
pub const D3D10_SRV_DIMENSION_TEXTURE2DMS: D3D_SRV_DIMENSION = 6;
pub const D3D10_SRV_DIMENSION_TEXTURE2DMSARRAY: D3D_SRV_DIMENSION = 7;
pub const D3D10_SRV_DIMENSION_TEXTURE3D: D3D_SRV_DIMENSION = 8;
pub const D3D10_SRV_DIMENSION_TEXTURECUBE: D3D_SRV_DIMENSION = 9;
pub const D3D10_SRV_DIMENSION_UNKNOWN: D3D_SRV_DIMENSION = 0;
pub const D3D10_SVC_MATRIX_COLUMNS: D3D_SHADER_VARIABLE_CLASS = 3;
pub const D3D10_SVC_MATRIX_ROWS: D3D_SHADER_VARIABLE_CLASS = 2;
pub const D3D10_SVC_OBJECT: D3D_SHADER_VARIABLE_CLASS = 4;
pub const D3D10_SVC_SCALAR: D3D_SHADER_VARIABLE_CLASS = 0;
pub const D3D10_SVC_STRUCT: D3D_SHADER_VARIABLE_CLASS = 5;
pub const D3D10_SVC_VECTOR: D3D_SHADER_VARIABLE_CLASS = 1;
pub const D3D11_APPEND_ALIGNED_ELEMENT: u32 = 4294967295;
pub const D3D11_ASYNC_GETDATA_DONOTFLUSH: D3D11_ASYNC_GETDATA_FLAG = 1;
pub type D3D11_ASYNC_GETDATA_FLAG = i32;
pub const D3D11_BIND_CONSTANT_BUFFER: D3D11_BIND_FLAG = 4;
pub const D3D11_BIND_DECODER: D3D11_BIND_FLAG = 512;
pub const D3D11_BIND_DEPTH_STENCIL: D3D11_BIND_FLAG = 64;
pub type D3D11_BIND_FLAG = i32;
pub const D3D11_BIND_INDEX_BUFFER: D3D11_BIND_FLAG = 2;
pub const D3D11_BIND_RENDER_TARGET: D3D11_BIND_FLAG = 32;
pub const D3D11_BIND_SHADER_RESOURCE: D3D11_BIND_FLAG = 8;
pub const D3D11_BIND_STREAM_OUTPUT: D3D11_BIND_FLAG = 16;
pub const D3D11_BIND_UNORDERED_ACCESS: D3D11_BIND_FLAG = 128;
pub const D3D11_BIND_VERTEX_BUFFER: D3D11_BIND_FLAG = 1;
pub const D3D11_BIND_VIDEO_ENCODER: D3D11_BIND_FLAG = 1024;
pub type D3D11_BLEND = i32;
pub const D3D11_BLEND_BLEND_FACTOR: D3D11_BLEND = 14;
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct D3D11_BLEND_DESC {
    pub AlphaToCoverageEnable: windows_core::BOOL,
    pub IndependentBlendEnable: windows_core::BOOL,
    pub RenderTarget: [D3D11_RENDER_TARGET_BLEND_DESC; 8],
}
impl Default for D3D11_BLEND_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub const D3D11_BLEND_DEST_ALPHA: D3D11_BLEND = 7;
pub const D3D11_BLEND_DEST_COLOR: D3D11_BLEND = 9;
pub const D3D11_BLEND_INV_BLEND_FACTOR: D3D11_BLEND = 15;
pub const D3D11_BLEND_INV_DEST_ALPHA: D3D11_BLEND = 8;
pub const D3D11_BLEND_INV_DEST_COLOR: D3D11_BLEND = 10;
pub const D3D11_BLEND_INV_SRC1_ALPHA: D3D11_BLEND = 19;
pub const D3D11_BLEND_INV_SRC1_COLOR: D3D11_BLEND = 17;
pub const D3D11_BLEND_INV_SRC_ALPHA: D3D11_BLEND = 6;
pub const D3D11_BLEND_INV_SRC_COLOR: D3D11_BLEND = 4;
pub const D3D11_BLEND_ONE: D3D11_BLEND = 2;
pub type D3D11_BLEND_OP = i32;
pub const D3D11_BLEND_OP_ADD: D3D11_BLEND_OP = 1;
pub const D3D11_BLEND_OP_MAX: D3D11_BLEND_OP = 5;
pub const D3D11_BLEND_OP_MIN: D3D11_BLEND_OP = 4;
pub const D3D11_BLEND_OP_REV_SUBTRACT: D3D11_BLEND_OP = 3;
pub const D3D11_BLEND_OP_SUBTRACT: D3D11_BLEND_OP = 2;
pub const D3D11_BLEND_SRC1_ALPHA: D3D11_BLEND = 18;
pub const D3D11_BLEND_SRC1_COLOR: D3D11_BLEND = 16;
pub const D3D11_BLEND_SRC_ALPHA: D3D11_BLEND = 5;
pub const D3D11_BLEND_SRC_ALPHA_SAT: D3D11_BLEND = 11;
pub const D3D11_BLEND_SRC_COLOR: D3D11_BLEND = 3;
pub const D3D11_BLEND_ZERO: D3D11_BLEND = 1;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_BOX {
    pub left: u32,
    pub top: u32,
    pub front: u32,
    pub right: u32,
    pub bottom: u32,
    pub back: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_BUFFEREX_SRV {
    pub FirstElement: u32,
    pub NumElements: u32,
    pub Flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_BUFFER_DESC {
    pub ByteWidth: u32,
    pub Usage: D3D11_USAGE,
    pub BindFlags: u32,
    pub CPUAccessFlags: u32,
    pub MiscFlags: u32,
    pub StructureByteStride: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_BUFFER_RTV {
    pub Anonymous: D3D11_BUFFER_RTV_0,
    pub Anonymous2: D3D11_BUFFER_RTV_1,
}
impl Default for D3D11_BUFFER_RTV {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_BUFFER_RTV_0 {
    pub FirstElement: u32,
    pub ElementOffset: u32,
}
impl Default for D3D11_BUFFER_RTV_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_BUFFER_RTV_1 {
    pub NumElements: u32,
    pub ElementWidth: u32,
}
impl Default for D3D11_BUFFER_RTV_1 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_BUFFER_SRV {
    pub Anonymous: D3D11_BUFFER_SRV_0,
    pub Anonymous2: D3D11_BUFFER_SRV_1,
}
impl Default for D3D11_BUFFER_SRV {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_BUFFER_SRV_0 {
    pub FirstElement: u32,
    pub ElementOffset: u32,
}
impl Default for D3D11_BUFFER_SRV_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_BUFFER_SRV_1 {
    pub NumElements: u32,
    pub ElementWidth: u32,
}
impl Default for D3D11_BUFFER_SRV_1 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_BUFFER_UAV {
    pub FirstElement: u32,
    pub NumElements: u32,
    pub Flags: u32,
}
pub const D3D11_CLEAR_DEPTH: D3D11_CLEAR_FLAG = 1;
pub type D3D11_CLEAR_FLAG = i32;
pub const D3D11_CLEAR_STENCIL: D3D11_CLEAR_FLAG = 2;
pub type D3D11_COLOR_WRITE_ENABLE = i32;
pub const D3D11_COLOR_WRITE_ENABLE_ALL: D3D11_COLOR_WRITE_ENABLE = 15;
pub const D3D11_COLOR_WRITE_ENABLE_ALPHA: D3D11_COLOR_WRITE_ENABLE = 8;
pub const D3D11_COLOR_WRITE_ENABLE_BLUE: D3D11_COLOR_WRITE_ENABLE = 4;
pub const D3D11_COLOR_WRITE_ENABLE_GREEN: D3D11_COLOR_WRITE_ENABLE = 2;
pub const D3D11_COLOR_WRITE_ENABLE_RED: D3D11_COLOR_WRITE_ENABLE = 1;
pub const D3D11_COMPARISON_ALWAYS: D3D11_COMPARISON_FUNC = 8;
pub const D3D11_COMPARISON_EQUAL: D3D11_COMPARISON_FUNC = 3;
pub type D3D11_COMPARISON_FUNC = i32;
pub const D3D11_COMPARISON_GREATER: D3D11_COMPARISON_FUNC = 5;
pub const D3D11_COMPARISON_GREATER_EQUAL: D3D11_COMPARISON_FUNC = 7;
pub const D3D11_COMPARISON_LESS: D3D11_COMPARISON_FUNC = 2;
pub const D3D11_COMPARISON_LESS_EQUAL: D3D11_COMPARISON_FUNC = 4;
pub const D3D11_COMPARISON_NEVER: D3D11_COMPARISON_FUNC = 1;
pub const D3D11_COMPARISON_NOT_EQUAL: D3D11_COMPARISON_FUNC = 6;
pub type D3D11_COUNTER = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_COUNTER_DESC {
    pub Counter: D3D11_COUNTER,
    pub MiscFlags: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_COUNTER_INFO {
    pub LastDeviceDependentCounter: D3D11_COUNTER,
    pub NumSimultaneousCounters: u32,
    pub NumDetectableParallelUnits: u8,
}
pub type D3D11_COUNTER_TYPE = i32;
pub type D3D11_CPU_ACCESS_FLAG = i32;
pub const D3D11_CPU_ACCESS_READ: D3D11_CPU_ACCESS_FLAG = 131072;
pub const D3D11_CPU_ACCESS_WRITE: D3D11_CPU_ACCESS_FLAG = 65536;
pub const D3D11_CREATE_DEVICE_BGRA_SUPPORT: D3D11_CREATE_DEVICE_FLAG = 32;
pub const D3D11_CREATE_DEVICE_DEBUG: D3D11_CREATE_DEVICE_FLAG = 2;
pub const D3D11_CREATE_DEVICE_DEBUGGABLE: D3D11_CREATE_DEVICE_FLAG = 64;
pub const D3D11_CREATE_DEVICE_DISABLE_GPU_TIMEOUT: D3D11_CREATE_DEVICE_FLAG = 256;
pub type D3D11_CREATE_DEVICE_FLAG = i32;
pub const D3D11_CREATE_DEVICE_PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY:
    D3D11_CREATE_DEVICE_FLAG = 128;
pub const D3D11_CREATE_DEVICE_PREVENT_INTERNAL_THREADING_OPTIMIZATIONS: D3D11_CREATE_DEVICE_FLAG =
    8;
pub const D3D11_CREATE_DEVICE_SINGLETHREADED: D3D11_CREATE_DEVICE_FLAG = 1;
pub const D3D11_CREATE_DEVICE_SWITCH_TO_REF: D3D11_CREATE_DEVICE_FLAG = 4;
pub const D3D11_CREATE_DEVICE_VIDEO_SUPPORT: D3D11_CREATE_DEVICE_FLAG = 2048;
pub const D3D11_CT_CBUFFER: D3D_CBUFFER_TYPE = 0;
pub const D3D11_CT_INTERFACE_POINTERS: D3D_CBUFFER_TYPE = 2;
pub const D3D11_CT_RESOURCE_BIND_INFO: D3D_CBUFFER_TYPE = 3;
pub const D3D11_CT_TBUFFER: D3D_CBUFFER_TYPE = 1;
pub const D3D11_CULL_BACK: D3D11_CULL_MODE = 3;
pub const D3D11_CULL_FRONT: D3D11_CULL_MODE = 2;
pub type D3D11_CULL_MODE = i32;
pub const D3D11_CULL_NONE: D3D11_CULL_MODE = 1;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_DEPTH_STENCILOP_DESC {
    pub StencilFailOp: D3D11_STENCIL_OP,
    pub StencilDepthFailOp: D3D11_STENCIL_OP,
    pub StencilPassOp: D3D11_STENCIL_OP,
    pub StencilFunc: D3D11_COMPARISON_FUNC,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_DEPTH_STENCIL_DESC {
    pub DepthEnable: windows_core::BOOL,
    pub DepthWriteMask: D3D11_DEPTH_WRITE_MASK,
    pub DepthFunc: D3D11_COMPARISON_FUNC,
    pub StencilEnable: windows_core::BOOL,
    pub StencilReadMask: u8,
    pub StencilWriteMask: u8,
    pub FrontFace: D3D11_DEPTH_STENCILOP_DESC,
    pub BackFace: D3D11_DEPTH_STENCILOP_DESC,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_DEPTH_STENCIL_VIEW_DESC {
    pub Format: DXGI_FORMAT,
    pub ViewDimension: D3D11_DSV_DIMENSION,
    pub Flags: u32,
    pub Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0,
}
impl Default for D3D11_DEPTH_STENCIL_VIEW_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_DEPTH_STENCIL_VIEW_DESC_0 {
    pub Texture1D: D3D11_TEX1D_DSV,
    pub Texture1DArray: D3D11_TEX1D_ARRAY_DSV,
    pub Texture2D: D3D11_TEX2D_DSV,
    pub Texture2DArray: D3D11_TEX2D_ARRAY_DSV,
    pub Texture2DMS: D3D11_TEX2DMS_DSV,
    pub Texture2DMSArray: D3D11_TEX2DMS_ARRAY_DSV,
}
impl Default for D3D11_DEPTH_STENCIL_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub type D3D11_DEPTH_WRITE_MASK = i32;
pub const D3D11_DEPTH_WRITE_MASK_ALL: D3D11_DEPTH_WRITE_MASK = 1;
pub const D3D11_DEPTH_WRITE_MASK_ZERO: D3D11_DEPTH_WRITE_MASK = 0;
pub type D3D11_DEVICE_CONTEXT_TYPE = i32;
pub type D3D11_DSV_DIMENSION = i32;
pub const D3D11_DSV_DIMENSION_TEXTURE1D: D3D11_DSV_DIMENSION = 1;
pub const D3D11_DSV_DIMENSION_TEXTURE1DARRAY: D3D11_DSV_DIMENSION = 2;
pub const D3D11_DSV_DIMENSION_TEXTURE2D: D3D11_DSV_DIMENSION = 3;
pub const D3D11_DSV_DIMENSION_TEXTURE2DARRAY: D3D11_DSV_DIMENSION = 4;
pub const D3D11_DSV_DIMENSION_TEXTURE2DMS: D3D11_DSV_DIMENSION = 5;
pub const D3D11_DSV_DIMENSION_TEXTURE2DMSARRAY: D3D11_DSV_DIMENSION = 6;
pub const D3D11_DSV_DIMENSION_UNKNOWN: D3D11_DSV_DIMENSION = 0;
pub type D3D11_FEATURE = i32;
pub type D3D11_FILL_MODE = i32;
pub const D3D11_FILL_SOLID: D3D11_FILL_MODE = 3;
pub const D3D11_FILL_WIREFRAME: D3D11_FILL_MODE = 2;
pub type D3D11_FILTER = i32;
pub const D3D11_FILTER_ANISOTROPIC: D3D11_FILTER = 85;
pub const D3D11_FILTER_COMPARISON_ANISOTROPIC: D3D11_FILTER = 213;
pub const D3D11_FILTER_COMPARISON_MIN_LINEAR_MAG_MIP_POINT: D3D11_FILTER = 144;
pub const D3D11_FILTER_COMPARISON_MIN_LINEAR_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 145;
pub const D3D11_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 148;
pub const D3D11_FILTER_COMPARISON_MIN_MAG_MIP_LINEAR: D3D11_FILTER = 149;
pub const D3D11_FILTER_COMPARISON_MIN_MAG_MIP_POINT: D3D11_FILTER = 128;
pub const D3D11_FILTER_COMPARISON_MIN_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 129;
pub const D3D11_FILTER_COMPARISON_MIN_POINT_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 132;
pub const D3D11_FILTER_COMPARISON_MIN_POINT_MAG_MIP_LINEAR: D3D11_FILTER = 133;
pub const D3D11_FILTER_MAXIMUM_ANISOTROPIC: D3D11_FILTER = 469;
pub const D3D11_FILTER_MAXIMUM_MIN_LINEAR_MAG_MIP_POINT: D3D11_FILTER = 400;
pub const D3D11_FILTER_MAXIMUM_MIN_LINEAR_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 401;
pub const D3D11_FILTER_MAXIMUM_MIN_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 404;
pub const D3D11_FILTER_MAXIMUM_MIN_MAG_MIP_LINEAR: D3D11_FILTER = 405;
pub const D3D11_FILTER_MAXIMUM_MIN_MAG_MIP_POINT: D3D11_FILTER = 384;
pub const D3D11_FILTER_MAXIMUM_MIN_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 385;
pub const D3D11_FILTER_MAXIMUM_MIN_POINT_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 388;
pub const D3D11_FILTER_MAXIMUM_MIN_POINT_MAG_MIP_LINEAR: D3D11_FILTER = 389;
pub const D3D11_FILTER_MINIMUM_ANISOTROPIC: D3D11_FILTER = 341;
pub const D3D11_FILTER_MINIMUM_MIN_LINEAR_MAG_MIP_POINT: D3D11_FILTER = 272;
pub const D3D11_FILTER_MINIMUM_MIN_LINEAR_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 273;
pub const D3D11_FILTER_MINIMUM_MIN_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 276;
pub const D3D11_FILTER_MINIMUM_MIN_MAG_MIP_LINEAR: D3D11_FILTER = 277;
pub const D3D11_FILTER_MINIMUM_MIN_MAG_MIP_POINT: D3D11_FILTER = 256;
pub const D3D11_FILTER_MINIMUM_MIN_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 257;
pub const D3D11_FILTER_MINIMUM_MIN_POINT_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 260;
pub const D3D11_FILTER_MINIMUM_MIN_POINT_MAG_MIP_LINEAR: D3D11_FILTER = 261;
pub const D3D11_FILTER_MIN_LINEAR_MAG_MIP_POINT: D3D11_FILTER = 16;
pub const D3D11_FILTER_MIN_LINEAR_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 17;
pub const D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 20;
pub const D3D11_FILTER_MIN_MAG_MIP_LINEAR: D3D11_FILTER = 21;
pub const D3D11_FILTER_MIN_MAG_MIP_POINT: D3D11_FILTER = 0;
pub const D3D11_FILTER_MIN_MAG_POINT_MIP_LINEAR: D3D11_FILTER = 1;
pub const D3D11_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT: D3D11_FILTER = 4;
pub const D3D11_FILTER_MIN_POINT_MAG_MIP_LINEAR: D3D11_FILTER = 5;
pub const D3D11_FLOAT32_MAX: f32 = 340282350000000000000000000000000000000.0;
pub type D3D11_INPUT_CLASSIFICATION = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_INPUT_ELEMENT_DESC {
    pub SemanticName: windows_core::PCSTR,
    pub SemanticIndex: u32,
    pub Format: DXGI_FORMAT,
    pub InputSlot: u32,
    pub AlignedByteOffset: u32,
    pub InputSlotClass: D3D11_INPUT_CLASSIFICATION,
    pub InstanceDataStepRate: u32,
}
pub const D3D11_INPUT_PER_INSTANCE_DATA: D3D11_INPUT_CLASSIFICATION = 1;
pub const D3D11_INPUT_PER_VERTEX_DATA: D3D11_INPUT_CLASSIFICATION = 0;
pub type D3D11_MAP = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_MAPPED_SUBRESOURCE {
    pub pData: *mut core::ffi::c_void,
    pub RowPitch: u32,
    pub DepthPitch: u32,
}
pub type D3D11_MAP_FLAG = i32;
pub const D3D11_MAP_FLAG_DO_NOT_WAIT: D3D11_MAP_FLAG = 1048576;
pub const D3D11_MAP_READ: D3D11_MAP = 1;
pub const D3D11_MAP_READ_WRITE: D3D11_MAP = 3;
pub const D3D11_MAP_WRITE: D3D11_MAP = 2;
pub const D3D11_MAP_WRITE_DISCARD: D3D11_MAP = 4;
pub const D3D11_MAP_WRITE_NO_OVERWRITE: D3D11_MAP = 5;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct D3D11_PRIMITIVE_TOPOLOGY(pub D3D_PRIMITIVE_TOPOLOGY);
pub const D3D11_PRIMITIVE_TOPOLOGY_10_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 42;
pub const D3D11_PRIMITIVE_TOPOLOGY_11_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 43;
pub const D3D11_PRIMITIVE_TOPOLOGY_12_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 44;
pub const D3D11_PRIMITIVE_TOPOLOGY_13_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 45;
pub const D3D11_PRIMITIVE_TOPOLOGY_14_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 46;
pub const D3D11_PRIMITIVE_TOPOLOGY_15_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 47;
pub const D3D11_PRIMITIVE_TOPOLOGY_16_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 48;
pub const D3D11_PRIMITIVE_TOPOLOGY_17_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 49;
pub const D3D11_PRIMITIVE_TOPOLOGY_18_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 50;
pub const D3D11_PRIMITIVE_TOPOLOGY_19_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 51;
pub const D3D11_PRIMITIVE_TOPOLOGY_1_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 33;
pub const D3D11_PRIMITIVE_TOPOLOGY_20_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 52;
pub const D3D11_PRIMITIVE_TOPOLOGY_21_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 53;
pub const D3D11_PRIMITIVE_TOPOLOGY_22_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 54;
pub const D3D11_PRIMITIVE_TOPOLOGY_23_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 55;
pub const D3D11_PRIMITIVE_TOPOLOGY_24_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 56;
pub const D3D11_PRIMITIVE_TOPOLOGY_25_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 57;
pub const D3D11_PRIMITIVE_TOPOLOGY_26_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 58;
pub const D3D11_PRIMITIVE_TOPOLOGY_27_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 59;
pub const D3D11_PRIMITIVE_TOPOLOGY_28_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 60;
pub const D3D11_PRIMITIVE_TOPOLOGY_29_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 61;
pub const D3D11_PRIMITIVE_TOPOLOGY_2_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 34;
pub const D3D11_PRIMITIVE_TOPOLOGY_30_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 62;
pub const D3D11_PRIMITIVE_TOPOLOGY_31_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 63;
pub const D3D11_PRIMITIVE_TOPOLOGY_32_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 64;
pub const D3D11_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 35;
pub const D3D11_PRIMITIVE_TOPOLOGY_4_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 36;
pub const D3D11_PRIMITIVE_TOPOLOGY_5_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 37;
pub const D3D11_PRIMITIVE_TOPOLOGY_6_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 38;
pub const D3D11_PRIMITIVE_TOPOLOGY_7_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 39;
pub const D3D11_PRIMITIVE_TOPOLOGY_8_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 40;
pub const D3D11_PRIMITIVE_TOPOLOGY_9_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 41;
pub const D3D11_PRIMITIVE_TOPOLOGY_LINELIST: D3D_PRIMITIVE_TOPOLOGY = 2;
pub const D3D11_PRIMITIVE_TOPOLOGY_LINELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 10;
pub const D3D11_PRIMITIVE_TOPOLOGY_LINESTRIP: D3D_PRIMITIVE_TOPOLOGY = 3;
pub const D3D11_PRIMITIVE_TOPOLOGY_LINESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 11;
pub const D3D11_PRIMITIVE_TOPOLOGY_POINTLIST: D3D_PRIMITIVE_TOPOLOGY = 1;
pub const D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST: D3D_PRIMITIVE_TOPOLOGY = 4;
pub const D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 12;
pub const D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP: D3D_PRIMITIVE_TOPOLOGY = 5;
pub const D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 13;
pub const D3D11_PRIMITIVE_TOPOLOGY_UNDEFINED: D3D_PRIMITIVE_TOPOLOGY = 0;
pub type D3D11_QUERY = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_QUERY_DATA_TIMESTAMP_DISJOINT {
    pub Frequency: u64,
    pub Disjoint: windows_core::BOOL,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_QUERY_DESC {
    pub Query: D3D11_QUERY,
    pub MiscFlags: u32,
}
pub const D3D11_QUERY_EVENT: D3D11_QUERY = 0;
pub const D3D11_QUERY_OCCLUSION: D3D11_QUERY = 1;
pub const D3D11_QUERY_OCCLUSION_PREDICATE: D3D11_QUERY = 5;
pub const D3D11_QUERY_PIPELINE_STATISTICS: D3D11_QUERY = 4;
pub const D3D11_QUERY_SO_OVERFLOW_PREDICATE: D3D11_QUERY = 7;
pub const D3D11_QUERY_SO_OVERFLOW_PREDICATE_STREAM0: D3D11_QUERY = 9;
pub const D3D11_QUERY_SO_OVERFLOW_PREDICATE_STREAM1: D3D11_QUERY = 11;
pub const D3D11_QUERY_SO_OVERFLOW_PREDICATE_STREAM2: D3D11_QUERY = 13;
pub const D3D11_QUERY_SO_OVERFLOW_PREDICATE_STREAM3: D3D11_QUERY = 15;
pub const D3D11_QUERY_SO_STATISTICS: D3D11_QUERY = 6;
pub const D3D11_QUERY_SO_STATISTICS_STREAM0: D3D11_QUERY = 8;
pub const D3D11_QUERY_SO_STATISTICS_STREAM1: D3D11_QUERY = 10;
pub const D3D11_QUERY_SO_STATISTICS_STREAM2: D3D11_QUERY = 12;
pub const D3D11_QUERY_SO_STATISTICS_STREAM3: D3D11_QUERY = 14;
pub const D3D11_QUERY_TIMESTAMP: D3D11_QUERY = 2;
pub const D3D11_QUERY_TIMESTAMP_DISJOINT: D3D11_QUERY = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct D3D11_RASTERIZER_DESC {
    pub FillMode: D3D11_FILL_MODE,
    pub CullMode: D3D11_CULL_MODE,
    pub FrontCounterClockwise: windows_core::BOOL,
    pub DepthBias: i32,
    pub DepthBiasClamp: f32,
    pub SlopeScaledDepthBias: f32,
    pub DepthClipEnable: windows_core::BOOL,
    pub ScissorEnable: windows_core::BOOL,
    pub MultisampleEnable: windows_core::BOOL,
    pub AntialiasedLineEnable: windows_core::BOOL,
}
pub type D3D11_RECT = RECT;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_RENDER_TARGET_BLEND_DESC {
    pub BlendEnable: windows_core::BOOL,
    pub SrcBlend: D3D11_BLEND,
    pub DestBlend: D3D11_BLEND,
    pub BlendOp: D3D11_BLEND_OP,
    pub SrcBlendAlpha: D3D11_BLEND,
    pub DestBlendAlpha: D3D11_BLEND,
    pub BlendOpAlpha: D3D11_BLEND_OP,
    pub RenderTargetWriteMask: u8,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_RENDER_TARGET_VIEW_DESC {
    pub Format: DXGI_FORMAT,
    pub ViewDimension: D3D11_RTV_DIMENSION,
    pub Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0,
}
impl Default for D3D11_RENDER_TARGET_VIEW_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_RENDER_TARGET_VIEW_DESC_0 {
    pub Buffer: D3D11_BUFFER_RTV,
    pub Texture1D: D3D11_TEX1D_RTV,
    pub Texture1DArray: D3D11_TEX1D_ARRAY_RTV,
    pub Texture2D: D3D11_TEX2D_RTV,
    pub Texture2DArray: D3D11_TEX2D_ARRAY_RTV,
    pub Texture2DMS: D3D11_TEX2DMS_RTV,
    pub Texture2DMSArray: D3D11_TEX2DMS_ARRAY_RTV,
    pub Texture3D: D3D11_TEX3D_RTV,
}
impl Default for D3D11_RENDER_TARGET_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub type D3D11_RESOURCE_DIMENSION = i32;
pub const D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS: D3D11_RESOURCE_MISC_FLAG = 32;
pub const D3D11_RESOURCE_MISC_BUFFER_STRUCTURED: D3D11_RESOURCE_MISC_FLAG = 64;
pub const D3D11_RESOURCE_MISC_DRAWINDIRECT_ARGS: D3D11_RESOURCE_MISC_FLAG = 16;
pub type D3D11_RESOURCE_MISC_FLAG = i32;
pub const D3D11_RESOURCE_MISC_GDI_COMPATIBLE: D3D11_RESOURCE_MISC_FLAG = 512;
pub const D3D11_RESOURCE_MISC_GENERATE_MIPS: D3D11_RESOURCE_MISC_FLAG = 1;
pub const D3D11_RESOURCE_MISC_GUARDED: D3D11_RESOURCE_MISC_FLAG = 32768;
pub const D3D11_RESOURCE_MISC_HW_PROTECTED: D3D11_RESOURCE_MISC_FLAG = 524288;
pub const D3D11_RESOURCE_MISC_NO_SHADER_ACCESS: D3D11_RESOURCE_MISC_FLAG = 4194304;
pub const D3D11_RESOURCE_MISC_RESOURCE_CLAMP: D3D11_RESOURCE_MISC_FLAG = 128;
pub const D3D11_RESOURCE_MISC_RESTRICTED_CONTENT: D3D11_RESOURCE_MISC_FLAG = 4096;
pub const D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE: D3D11_RESOURCE_MISC_FLAG = 8192;
pub const D3D11_RESOURCE_MISC_RESTRICT_SHARED_RESOURCE_DRIVER: D3D11_RESOURCE_MISC_FLAG = 16384;
pub const D3D11_RESOURCE_MISC_SHARED: D3D11_RESOURCE_MISC_FLAG = 2;
pub const D3D11_RESOURCE_MISC_SHARED_DISPLAYABLE: D3D11_RESOURCE_MISC_FLAG = 1048576;
pub const D3D11_RESOURCE_MISC_SHARED_EXCLUSIVE_WRITER: D3D11_RESOURCE_MISC_FLAG = 2097152;
pub const D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX: D3D11_RESOURCE_MISC_FLAG = 256;
pub const D3D11_RESOURCE_MISC_SHARED_NTHANDLE: D3D11_RESOURCE_MISC_FLAG = 2048;
pub const D3D11_RESOURCE_MISC_TEXTURECUBE: D3D11_RESOURCE_MISC_FLAG = 4;
pub const D3D11_RESOURCE_MISC_TILED: D3D11_RESOURCE_MISC_FLAG = 262144;
pub const D3D11_RESOURCE_MISC_TILE_POOL: D3D11_RESOURCE_MISC_FLAG = 131072;
pub type D3D11_RTV_DIMENSION = i32;
pub const D3D11_RTV_DIMENSION_BUFFER: D3D11_RTV_DIMENSION = 1;
pub const D3D11_RTV_DIMENSION_TEXTURE1D: D3D11_RTV_DIMENSION = 2;
pub const D3D11_RTV_DIMENSION_TEXTURE1DARRAY: D3D11_RTV_DIMENSION = 3;
pub const D3D11_RTV_DIMENSION_TEXTURE2D: D3D11_RTV_DIMENSION = 4;
pub const D3D11_RTV_DIMENSION_TEXTURE2DARRAY: D3D11_RTV_DIMENSION = 5;
pub const D3D11_RTV_DIMENSION_TEXTURE2DMS: D3D11_RTV_DIMENSION = 6;
pub const D3D11_RTV_DIMENSION_TEXTURE2DMSARRAY: D3D11_RTV_DIMENSION = 7;
pub const D3D11_RTV_DIMENSION_TEXTURE3D: D3D11_RTV_DIMENSION = 8;
pub const D3D11_RTV_DIMENSION_UNKNOWN: D3D11_RTV_DIMENSION = 0;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct D3D11_SAMPLER_DESC {
    pub Filter: D3D11_FILTER,
    pub AddressU: D3D11_TEXTURE_ADDRESS_MODE,
    pub AddressV: D3D11_TEXTURE_ADDRESS_MODE,
    pub AddressW: D3D11_TEXTURE_ADDRESS_MODE,
    pub MipLODBias: f32,
    pub MaxAnisotropy: u32,
    pub ComparisonFunc: D3D11_COMPARISON_FUNC,
    pub BorderColor: [f32; 4],
    pub MinLOD: f32,
    pub MaxLOD: f32,
}
impl Default for D3D11_SAMPLER_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub const D3D11_SDK_VERSION: i32 = 7;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SHADER_BUFFER_DESC {
    pub Name: windows_core::PCSTR,
    pub Type: D3D_CBUFFER_TYPE,
    pub Variables: u32,
    pub Size: u32,
    pub uFlags: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SHADER_DESC {
    pub Version: u32,
    pub Creator: windows_core::PCSTR,
    pub Flags: u32,
    pub ConstantBuffers: u32,
    pub BoundResources: u32,
    pub InputParameters: u32,
    pub OutputParameters: u32,
    pub InstructionCount: u32,
    pub TempRegisterCount: u32,
    pub TempArrayCount: u32,
    pub DefCount: u32,
    pub DclCount: u32,
    pub TextureNormalInstructions: u32,
    pub TextureLoadInstructions: u32,
    pub TextureCompInstructions: u32,
    pub TextureBiasInstructions: u32,
    pub TextureGradientInstructions: u32,
    pub FloatInstructionCount: u32,
    pub IntInstructionCount: u32,
    pub UintInstructionCount: u32,
    pub StaticFlowControlCount: u32,
    pub DynamicFlowControlCount: u32,
    pub MacroInstructionCount: u32,
    pub ArrayInstructionCount: u32,
    pub CutInstructionCount: u32,
    pub EmitInstructionCount: u32,
    pub GSOutputTopology: D3D_PRIMITIVE_TOPOLOGY,
    pub GSMaxOutputVertexCount: u32,
    pub InputPrimitive: D3D_PRIMITIVE,
    pub PatchConstantParameters: u32,
    pub cGSInstanceCount: u32,
    pub cControlPoints: u32,
    pub HSOutputPrimitive: D3D_TESSELLATOR_OUTPUT_PRIMITIVE,
    pub HSPartitioning: D3D_TESSELLATOR_PARTITIONING,
    pub TessellatorDomain: D3D_TESSELLATOR_DOMAIN,
    pub cBarrierInstructions: u32,
    pub cInterlockedInstructions: u32,
    pub cTextureStoreInstructions: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SHADER_INPUT_BIND_DESC {
    pub Name: windows_core::PCSTR,
    pub Type: D3D_SHADER_INPUT_TYPE,
    pub BindPoint: u32,
    pub BindCount: u32,
    pub uFlags: u32,
    pub ReturnType: D3D_RESOURCE_RETURN_TYPE,
    pub Dimension: D3D_SRV_DIMENSION,
    pub NumSamples: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_SHADER_RESOURCE_VIEW_DESC {
    pub Format: DXGI_FORMAT,
    pub ViewDimension: D3D11_SRV_DIMENSION,
    pub Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0,
}
impl Default for D3D11_SHADER_RESOURCE_VIEW_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
    pub Buffer: D3D11_BUFFER_SRV,
    pub Texture1D: D3D11_TEX1D_SRV,
    pub Texture1DArray: D3D11_TEX1D_ARRAY_SRV,
    pub Texture2D: D3D11_TEX2D_SRV,
    pub Texture2DArray: D3D11_TEX2D_ARRAY_SRV,
    pub Texture2DMS: D3D11_TEX2DMS_SRV,
    pub Texture2DMSArray: D3D11_TEX2DMS_ARRAY_SRV,
    pub Texture3D: D3D11_TEX3D_SRV,
    pub TextureCube: D3D11_TEXCUBE_SRV,
    pub TextureCubeArray: D3D11_TEXCUBE_ARRAY_SRV,
    pub BufferEx: D3D11_BUFFEREX_SRV,
}
impl Default for D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SHADER_TYPE_DESC {
    pub Class: D3D_SHADER_VARIABLE_CLASS,
    pub Type: D3D_SHADER_VARIABLE_TYPE,
    pub Rows: u32,
    pub Columns: u32,
    pub Elements: u32,
    pub Members: u32,
    pub Offset: u32,
    pub Name: windows_core::PCSTR,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SHADER_VARIABLE_DESC {
    pub Name: windows_core::PCSTR,
    pub StartOffset: u32,
    pub Size: u32,
    pub uFlags: u32,
    pub DefaultValue: *mut core::ffi::c_void,
    pub StartTexture: u32,
    pub TextureSize: u32,
    pub StartSampler: u32,
    pub SamplerSize: u32,
}
pub type D3D11_SHADER_VERSION_TYPE = i32;
pub const D3D11_SHVER_COMPUTE_SHADER: D3D11_SHADER_VERSION_TYPE = 5;
pub const D3D11_SHVER_DOMAIN_SHADER: D3D11_SHADER_VERSION_TYPE = 4;
pub const D3D11_SHVER_GEOMETRY_SHADER: D3D11_SHADER_VERSION_TYPE = 2;
pub const D3D11_SHVER_HULL_SHADER: D3D11_SHADER_VERSION_TYPE = 3;
pub const D3D11_SHVER_PIXEL_SHADER: D3D11_SHADER_VERSION_TYPE = 0;
pub const D3D11_SHVER_RESERVED0: D3D11_SHADER_VERSION_TYPE = 65520;
pub const D3D11_SHVER_VERTEX_SHADER: D3D11_SHADER_VERSION_TYPE = 1;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SIGNATURE_PARAMETER_DESC {
    pub SemanticName: windows_core::PCSTR,
    pub SemanticIndex: u32,
    pub Register: u32,
    pub SystemValueType: D3D_NAME,
    pub ComponentType: D3D_REGISTER_COMPONENT_TYPE,
    pub Mask: u8,
    pub ReadWriteMask: u8,
    pub Stream: u32,
    pub MinPrecision: D3D_MIN_PRECISION,
}
pub const D3D11_SIT_BYTEADDRESS: D3D_SHADER_INPUT_TYPE = 7;
pub const D3D11_SIT_STRUCTURED: D3D_SHADER_INPUT_TYPE = 5;
pub const D3D11_SIT_UAV_APPEND_STRUCTURED: D3D_SHADER_INPUT_TYPE = 9;
pub const D3D11_SIT_UAV_CONSUME_STRUCTURED: D3D_SHADER_INPUT_TYPE = 10;
pub const D3D11_SIT_UAV_RWBYTEADDRESS: D3D_SHADER_INPUT_TYPE = 8;
pub const D3D11_SIT_UAV_RWSTRUCTURED: D3D_SHADER_INPUT_TYPE = 6;
pub const D3D11_SIT_UAV_RWSTRUCTURED_WITH_COUNTER: D3D_SHADER_INPUT_TYPE = 11;
pub const D3D11_SIT_UAV_RWTYPED: D3D_SHADER_INPUT_TYPE = 4;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SO_DECLARATION_ENTRY {
    pub Stream: u32,
    pub SemanticName: windows_core::PCSTR,
    pub SemanticIndex: u32,
    pub StartComponent: u8,
    pub ComponentCount: u8,
    pub OutputSlot: u8,
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct D3D11_SRV_DIMENSION(pub D3D_SRV_DIMENSION);
pub const D3D11_SRV_DIMENSION_BUFFER: D3D_SRV_DIMENSION = 1;
pub const D3D11_SRV_DIMENSION_BUFFEREX: D3D_SRV_DIMENSION = 11;
pub const D3D11_SRV_DIMENSION_TEXTURE1D: D3D_SRV_DIMENSION = 2;
pub const D3D11_SRV_DIMENSION_TEXTURE1DARRAY: D3D_SRV_DIMENSION = 3;
pub const D3D11_SRV_DIMENSION_TEXTURE2D: D3D_SRV_DIMENSION = 4;
pub const D3D11_SRV_DIMENSION_TEXTURE2DARRAY: D3D_SRV_DIMENSION = 5;
pub const D3D11_SRV_DIMENSION_TEXTURE2DMS: D3D_SRV_DIMENSION = 6;
pub const D3D11_SRV_DIMENSION_TEXTURE2DMSARRAY: D3D_SRV_DIMENSION = 7;
pub const D3D11_SRV_DIMENSION_TEXTURE3D: D3D_SRV_DIMENSION = 8;
pub const D3D11_SRV_DIMENSION_TEXTURECUBE: D3D_SRV_DIMENSION = 9;
pub const D3D11_SRV_DIMENSION_TEXTURECUBEARRAY: D3D_SRV_DIMENSION = 10;
pub const D3D11_SRV_DIMENSION_UNKNOWN: D3D_SRV_DIMENSION = 0;
pub type D3D11_STENCIL_OP = i32;
pub const D3D11_STENCIL_OP_DECR: D3D11_STENCIL_OP = 8;
pub const D3D11_STENCIL_OP_DECR_SAT: D3D11_STENCIL_OP = 5;
pub const D3D11_STENCIL_OP_INCR: D3D11_STENCIL_OP = 7;
pub const D3D11_STENCIL_OP_INCR_SAT: D3D11_STENCIL_OP = 4;
pub const D3D11_STENCIL_OP_INVERT: D3D11_STENCIL_OP = 6;
pub const D3D11_STENCIL_OP_KEEP: D3D11_STENCIL_OP = 1;
pub const D3D11_STENCIL_OP_REPLACE: D3D11_STENCIL_OP = 3;
pub const D3D11_STENCIL_OP_ZERO: D3D11_STENCIL_OP = 2;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_SUBRESOURCE_DATA {
    pub pSysMem: *const core::ffi::c_void,
    pub SysMemPitch: u32,
    pub SysMemSlicePitch: u32,
}
pub const D3D11_SVC_INTERFACE_CLASS: D3D_SHADER_VARIABLE_CLASS = 6;
pub const D3D11_SVC_INTERFACE_POINTER: D3D_SHADER_VARIABLE_CLASS = 7;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_ARRAY_DSV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_ARRAY_RTV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_ARRAY_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_ARRAY_UAV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_DSV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_RTV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX1D_UAV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_ARRAY_DSV {
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_ARRAY_RTV {
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_ARRAY_SRV {
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_DSV {
    pub UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_RTV {
    pub UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2DMS_SRV {
    pub UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_ARRAY_DSV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_ARRAY_RTV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_ARRAY_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_ARRAY_UAV {
    pub MipSlice: u32,
    pub FirstArraySlice: u32,
    pub ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_DSV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_RTV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX2D_UAV {
    pub MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX3D_RTV {
    pub MipSlice: u32,
    pub FirstWSlice: u32,
    pub WSize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX3D_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEX3D_UAV {
    pub MipSlice: u32,
    pub FirstWSlice: u32,
    pub WSize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEXCUBE_ARRAY_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
    pub First2DArrayFace: u32,
    pub NumCubes: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEXCUBE_SRV {
    pub MostDetailedMip: u32,
    pub MipLevels: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEXTURE1D_DESC {
    pub Width: u32,
    pub MipLevels: u32,
    pub ArraySize: u32,
    pub Format: DXGI_FORMAT,
    pub Usage: D3D11_USAGE,
    pub BindFlags: u32,
    pub CPUAccessFlags: u32,
    pub MiscFlags: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEXTURE2D_DESC {
    pub Width: u32,
    pub Height: u32,
    pub MipLevels: u32,
    pub ArraySize: u32,
    pub Format: DXGI_FORMAT,
    pub SampleDesc: DXGI_SAMPLE_DESC,
    pub Usage: D3D11_USAGE,
    pub BindFlags: u32,
    pub CPUAccessFlags: u32,
    pub MiscFlags: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D11_TEXTURE3D_DESC {
    pub Width: u32,
    pub Height: u32,
    pub Depth: u32,
    pub MipLevels: u32,
    pub Format: DXGI_FORMAT,
    pub Usage: D3D11_USAGE,
    pub BindFlags: u32,
    pub CPUAccessFlags: u32,
    pub MiscFlags: u32,
}
pub const D3D11_TEXTURE_ADDRESS_BORDER: D3D11_TEXTURE_ADDRESS_MODE = 4;
pub const D3D11_TEXTURE_ADDRESS_CLAMP: D3D11_TEXTURE_ADDRESS_MODE = 3;
pub const D3D11_TEXTURE_ADDRESS_MIRROR: D3D11_TEXTURE_ADDRESS_MODE = 2;
pub const D3D11_TEXTURE_ADDRESS_MIRROR_ONCE: D3D11_TEXTURE_ADDRESS_MODE = 5;
pub type D3D11_TEXTURE_ADDRESS_MODE = i32;
pub const D3D11_TEXTURE_ADDRESS_WRAP: D3D11_TEXTURE_ADDRESS_MODE = 1;
pub type D3D11_UAV_DIMENSION = i32;
pub const D3D11_UAV_DIMENSION_BUFFER: D3D11_UAV_DIMENSION = 1;
pub const D3D11_UAV_DIMENSION_TEXTURE1D: D3D11_UAV_DIMENSION = 2;
pub const D3D11_UAV_DIMENSION_TEXTURE1DARRAY: D3D11_UAV_DIMENSION = 3;
pub const D3D11_UAV_DIMENSION_TEXTURE2D: D3D11_UAV_DIMENSION = 4;
pub const D3D11_UAV_DIMENSION_TEXTURE2DARRAY: D3D11_UAV_DIMENSION = 5;
pub const D3D11_UAV_DIMENSION_TEXTURE3D: D3D11_UAV_DIMENSION = 8;
pub const D3D11_UAV_DIMENSION_UNKNOWN: D3D11_UAV_DIMENSION = 0;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct D3D11_UNORDERED_ACCESS_VIEW_DESC {
    pub Format: DXGI_FORMAT,
    pub ViewDimension: D3D11_UAV_DIMENSION,
    pub Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0,
}
impl Default for D3D11_UNORDERED_ACCESS_VIEW_DESC {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
    pub Buffer: D3D11_BUFFER_UAV,
    pub Texture1D: D3D11_TEX1D_UAV,
    pub Texture1DArray: D3D11_TEX1D_ARRAY_UAV,
    pub Texture2D: D3D11_TEX2D_UAV,
    pub Texture2DArray: D3D11_TEX2D_ARRAY_UAV,
    pub Texture3D: D3D11_TEX3D_UAV,
}
impl Default for D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub type D3D11_USAGE = i32;
pub const D3D11_USAGE_DEFAULT: D3D11_USAGE = 0;
pub const D3D11_USAGE_DYNAMIC: D3D11_USAGE = 2;
pub const D3D11_USAGE_IMMUTABLE: D3D11_USAGE = 1;
pub const D3D11_USAGE_STAGING: D3D11_USAGE = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct D3D11_VIEWPORT {
    pub TopLeftX: f32,
    pub TopLeftY: f32,
    pub Width: f32,
    pub Height: f32,
    pub MinDepth: f32,
    pub MaxDepth: f32,
}
pub type D3D_CBUFFER_TYPE = i32;
pub const D3D_CT_CBUFFER: D3D_CBUFFER_TYPE = 0;
pub const D3D_CT_INTERFACE_POINTERS: D3D_CBUFFER_TYPE = 2;
pub const D3D_CT_RESOURCE_BIND_INFO: D3D_CBUFFER_TYPE = 3;
pub const D3D_CT_TBUFFER: D3D_CBUFFER_TYPE = 1;
pub type D3D_DRIVER_TYPE = i32;
pub const D3D_DRIVER_TYPE_HARDWARE: D3D_DRIVER_TYPE = 1;
pub const D3D_DRIVER_TYPE_NULL: D3D_DRIVER_TYPE = 3;
pub const D3D_DRIVER_TYPE_REFERENCE: D3D_DRIVER_TYPE = 2;
pub const D3D_DRIVER_TYPE_SOFTWARE: D3D_DRIVER_TYPE = 4;
pub const D3D_DRIVER_TYPE_UNKNOWN: D3D_DRIVER_TYPE = 0;
pub const D3D_DRIVER_TYPE_WARP: D3D_DRIVER_TYPE = 5;
pub type D3D_FEATURE_LEVEL = i32;
pub const D3D_FEATURE_LEVEL_10_0: D3D_FEATURE_LEVEL = 40960;
pub const D3D_FEATURE_LEVEL_10_1: D3D_FEATURE_LEVEL = 41216;
pub const D3D_FEATURE_LEVEL_11_0: D3D_FEATURE_LEVEL = 45056;
pub const D3D_FEATURE_LEVEL_11_1: D3D_FEATURE_LEVEL = 45312;
pub const D3D_FEATURE_LEVEL_12_0: D3D_FEATURE_LEVEL = 49152;
pub const D3D_FEATURE_LEVEL_12_1: D3D_FEATURE_LEVEL = 49408;
pub const D3D_FEATURE_LEVEL_12_2: D3D_FEATURE_LEVEL = 49664;
pub const D3D_FEATURE_LEVEL_1_0_CORE: D3D_FEATURE_LEVEL = 4096;
pub const D3D_FEATURE_LEVEL_1_0_GENERIC: D3D_FEATURE_LEVEL = 256;
pub const D3D_FEATURE_LEVEL_9_1: D3D_FEATURE_LEVEL = 37120;
pub const D3D_FEATURE_LEVEL_9_2: D3D_FEATURE_LEVEL = 37376;
pub const D3D_FEATURE_LEVEL_9_3: D3D_FEATURE_LEVEL = 37632;
pub type D3D_MIN_PRECISION = i32;
pub type D3D_NAME = i32;
pub type D3D_PRIMITIVE = i32;
pub type D3D_PRIMITIVE_TOPOLOGY = i32;
pub const D3D_PRIMITIVE_TOPOLOGY_10_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 42;
pub const D3D_PRIMITIVE_TOPOLOGY_11_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 43;
pub const D3D_PRIMITIVE_TOPOLOGY_12_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 44;
pub const D3D_PRIMITIVE_TOPOLOGY_13_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 45;
pub const D3D_PRIMITIVE_TOPOLOGY_14_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 46;
pub const D3D_PRIMITIVE_TOPOLOGY_15_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 47;
pub const D3D_PRIMITIVE_TOPOLOGY_16_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 48;
pub const D3D_PRIMITIVE_TOPOLOGY_17_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 49;
pub const D3D_PRIMITIVE_TOPOLOGY_18_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 50;
pub const D3D_PRIMITIVE_TOPOLOGY_19_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 51;
pub const D3D_PRIMITIVE_TOPOLOGY_1_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 33;
pub const D3D_PRIMITIVE_TOPOLOGY_20_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 52;
pub const D3D_PRIMITIVE_TOPOLOGY_21_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 53;
pub const D3D_PRIMITIVE_TOPOLOGY_22_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 54;
pub const D3D_PRIMITIVE_TOPOLOGY_23_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 55;
pub const D3D_PRIMITIVE_TOPOLOGY_24_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 56;
pub const D3D_PRIMITIVE_TOPOLOGY_25_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 57;
pub const D3D_PRIMITIVE_TOPOLOGY_26_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 58;
pub const D3D_PRIMITIVE_TOPOLOGY_27_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 59;
pub const D3D_PRIMITIVE_TOPOLOGY_28_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 60;
pub const D3D_PRIMITIVE_TOPOLOGY_29_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 61;
pub const D3D_PRIMITIVE_TOPOLOGY_2_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 34;
pub const D3D_PRIMITIVE_TOPOLOGY_30_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 62;
pub const D3D_PRIMITIVE_TOPOLOGY_31_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 63;
pub const D3D_PRIMITIVE_TOPOLOGY_32_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 64;
pub const D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 35;
pub const D3D_PRIMITIVE_TOPOLOGY_4_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 36;
pub const D3D_PRIMITIVE_TOPOLOGY_5_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 37;
pub const D3D_PRIMITIVE_TOPOLOGY_6_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 38;
pub const D3D_PRIMITIVE_TOPOLOGY_7_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 39;
pub const D3D_PRIMITIVE_TOPOLOGY_8_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 40;
pub const D3D_PRIMITIVE_TOPOLOGY_9_CONTROL_POINT_PATCHLIST: D3D_PRIMITIVE_TOPOLOGY = 41;
pub const D3D_PRIMITIVE_TOPOLOGY_LINELIST: D3D_PRIMITIVE_TOPOLOGY = 2;
pub const D3D_PRIMITIVE_TOPOLOGY_LINELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 10;
pub const D3D_PRIMITIVE_TOPOLOGY_LINESTRIP: D3D_PRIMITIVE_TOPOLOGY = 3;
pub const D3D_PRIMITIVE_TOPOLOGY_LINESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 11;
pub const D3D_PRIMITIVE_TOPOLOGY_POINTLIST: D3D_PRIMITIVE_TOPOLOGY = 1;
pub const D3D_PRIMITIVE_TOPOLOGY_TRIANGLEFAN: D3D_PRIMITIVE_TOPOLOGY = 6;
pub const D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST: D3D_PRIMITIVE_TOPOLOGY = 4;
pub const D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST_ADJ: D3D_PRIMITIVE_TOPOLOGY = 12;
pub const D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP: D3D_PRIMITIVE_TOPOLOGY = 5;
pub const D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP_ADJ: D3D_PRIMITIVE_TOPOLOGY = 13;
pub const D3D_PRIMITIVE_TOPOLOGY_UNDEFINED: D3D_PRIMITIVE_TOPOLOGY = 0;
pub type D3D_REGISTER_COMPONENT_TYPE = i32;
pub type D3D_RESOURCE_RETURN_TYPE = i32;
pub type D3D_SHADER_INPUT_TYPE = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct D3D_SHADER_MACRO {
    pub Name: windows_core::PCSTR,
    pub Definition: windows_core::PCSTR,
}
pub type D3D_SHADER_VARIABLE_CLASS = i32;
pub type D3D_SHADER_VARIABLE_TYPE = i32;
pub const D3D_SIT_BYTEADDRESS: D3D_SHADER_INPUT_TYPE = 7;
pub const D3D_SIT_CBUFFER: D3D_SHADER_INPUT_TYPE = 0;
pub const D3D_SIT_RTACCELERATIONSTRUCTURE: D3D_SHADER_INPUT_TYPE = 12;
pub const D3D_SIT_SAMPLER: D3D_SHADER_INPUT_TYPE = 3;
pub const D3D_SIT_STRUCTURED: D3D_SHADER_INPUT_TYPE = 5;
pub const D3D_SIT_TBUFFER: D3D_SHADER_INPUT_TYPE = 1;
pub const D3D_SIT_TEXTURE: D3D_SHADER_INPUT_TYPE = 2;
pub const D3D_SIT_UAV_APPEND_STRUCTURED: D3D_SHADER_INPUT_TYPE = 9;
pub const D3D_SIT_UAV_CONSUME_STRUCTURED: D3D_SHADER_INPUT_TYPE = 10;
pub const D3D_SIT_UAV_FEEDBACKTEXTURE: D3D_SHADER_INPUT_TYPE = 13;
pub const D3D_SIT_UAV_RWBYTEADDRESS: D3D_SHADER_INPUT_TYPE = 8;
pub const D3D_SIT_UAV_RWSTRUCTURED: D3D_SHADER_INPUT_TYPE = 6;
pub const D3D_SIT_UAV_RWSTRUCTURED_WITH_COUNTER: D3D_SHADER_INPUT_TYPE = 11;
pub const D3D_SIT_UAV_RWTYPED: D3D_SHADER_INPUT_TYPE = 4;
pub type D3D_SRV_DIMENSION = i32;
pub const D3D_SRV_DIMENSION_BUFFER: D3D_SRV_DIMENSION = 1;
pub const D3D_SRV_DIMENSION_BUFFEREX: D3D_SRV_DIMENSION = 11;
pub const D3D_SRV_DIMENSION_TEXTURE1D: D3D_SRV_DIMENSION = 2;
pub const D3D_SRV_DIMENSION_TEXTURE1DARRAY: D3D_SRV_DIMENSION = 3;
pub const D3D_SRV_DIMENSION_TEXTURE2D: D3D_SRV_DIMENSION = 4;
pub const D3D_SRV_DIMENSION_TEXTURE2DARRAY: D3D_SRV_DIMENSION = 5;
pub const D3D_SRV_DIMENSION_TEXTURE2DMS: D3D_SRV_DIMENSION = 6;
pub const D3D_SRV_DIMENSION_TEXTURE2DMSARRAY: D3D_SRV_DIMENSION = 7;
pub const D3D_SRV_DIMENSION_TEXTURE3D: D3D_SRV_DIMENSION = 8;
pub const D3D_SRV_DIMENSION_TEXTURECUBE: D3D_SRV_DIMENSION = 9;
pub const D3D_SRV_DIMENSION_TEXTURECUBEARRAY: D3D_SRV_DIMENSION = 10;
pub const D3D_SRV_DIMENSION_UNKNOWN: D3D_SRV_DIMENSION = 0;
pub const D3D_SVC_FORCE_DWORD: D3D_SHADER_VARIABLE_CLASS = 2147483647;
pub const D3D_SVC_INTERFACE_CLASS: D3D_SHADER_VARIABLE_CLASS = 6;
pub const D3D_SVC_INTERFACE_POINTER: D3D_SHADER_VARIABLE_CLASS = 7;
pub const D3D_SVC_MATRIX_COLUMNS: D3D_SHADER_VARIABLE_CLASS = 3;
pub const D3D_SVC_MATRIX_ROWS: D3D_SHADER_VARIABLE_CLASS = 2;
pub const D3D_SVC_OBJECT: D3D_SHADER_VARIABLE_CLASS = 4;
pub const D3D_SVC_SCALAR: D3D_SHADER_VARIABLE_CLASS = 0;
pub const D3D_SVC_STRUCT: D3D_SHADER_VARIABLE_CLASS = 5;
pub const D3D_SVC_VECTOR: D3D_SHADER_VARIABLE_CLASS = 1;
pub type D3D_TESSELLATOR_DOMAIN = i32;
pub type D3D_TESSELLATOR_OUTPUT_PRIMITIVE = i32;
pub type D3D_TESSELLATOR_PARTITIONING = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DXGI_ADAPTER_DESC1 {
    pub Description: [u16; 128],
    pub VendorId: u32,
    pub DeviceId: u32,
    pub SubSysId: u32,
    pub Revision: u32,
    pub DedicatedVideoMemory: usize,
    pub DedicatedSystemMemory: usize,
    pub SharedSystemMemory: usize,
    pub AdapterLuid: LUID,
    pub Flags: u32,
}
impl Default for DXGI_ADAPTER_DESC1 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub type DXGI_ADAPTER_FLAG = i32;
pub const DXGI_ADAPTER_FLAG_FORCE_DWORD: DXGI_ADAPTER_FLAG = -1;
pub const DXGI_ADAPTER_FLAG_NONE: DXGI_ADAPTER_FLAG = 0;
pub const DXGI_ADAPTER_FLAG_REMOTE: DXGI_ADAPTER_FLAG = 1;
pub const DXGI_ADAPTER_FLAG_SOFTWARE: DXGI_ADAPTER_FLAG = 2;
pub type DXGI_FORMAT = i32;
pub const DXGI_FORMAT_420_OPAQUE: DXGI_FORMAT = 106;
pub const DXGI_FORMAT_A4B4G4R4_UNORM: DXGI_FORMAT = 191;
pub const DXGI_FORMAT_A8P8: DXGI_FORMAT = 114;
pub const DXGI_FORMAT_A8_UNORM: DXGI_FORMAT = 65;
pub const DXGI_FORMAT_AI44: DXGI_FORMAT = 111;
pub const DXGI_FORMAT_AYUV: DXGI_FORMAT = 100;
pub const DXGI_FORMAT_B4G4R4A4_UNORM: DXGI_FORMAT = 115;
pub const DXGI_FORMAT_B5G5R5A1_UNORM: DXGI_FORMAT = 86;
pub const DXGI_FORMAT_B5G6R5_UNORM: DXGI_FORMAT = 85;
pub const DXGI_FORMAT_B8G8R8A8_TYPELESS: DXGI_FORMAT = 90;
pub const DXGI_FORMAT_B8G8R8A8_UNORM: DXGI_FORMAT = 87;
pub const DXGI_FORMAT_B8G8R8A8_UNORM_SRGB: DXGI_FORMAT = 91;
pub const DXGI_FORMAT_B8G8R8X8_TYPELESS: DXGI_FORMAT = 92;
pub const DXGI_FORMAT_B8G8R8X8_UNORM: DXGI_FORMAT = 88;
pub const DXGI_FORMAT_B8G8R8X8_UNORM_SRGB: DXGI_FORMAT = 93;
pub const DXGI_FORMAT_BC1_TYPELESS: DXGI_FORMAT = 70;
pub const DXGI_FORMAT_BC1_UNORM: DXGI_FORMAT = 71;
pub const DXGI_FORMAT_BC1_UNORM_SRGB: DXGI_FORMAT = 72;
pub const DXGI_FORMAT_BC2_TYPELESS: DXGI_FORMAT = 73;
pub const DXGI_FORMAT_BC2_UNORM: DXGI_FORMAT = 74;
pub const DXGI_FORMAT_BC2_UNORM_SRGB: DXGI_FORMAT = 75;
pub const DXGI_FORMAT_BC3_TYPELESS: DXGI_FORMAT = 76;
pub const DXGI_FORMAT_BC3_UNORM: DXGI_FORMAT = 77;
pub const DXGI_FORMAT_BC3_UNORM_SRGB: DXGI_FORMAT = 78;
pub const DXGI_FORMAT_BC4_SNORM: DXGI_FORMAT = 81;
pub const DXGI_FORMAT_BC4_TYPELESS: DXGI_FORMAT = 79;
pub const DXGI_FORMAT_BC4_UNORM: DXGI_FORMAT = 80;
pub const DXGI_FORMAT_BC5_SNORM: DXGI_FORMAT = 84;
pub const DXGI_FORMAT_BC5_TYPELESS: DXGI_FORMAT = 82;
pub const DXGI_FORMAT_BC5_UNORM: DXGI_FORMAT = 83;
pub const DXGI_FORMAT_BC6H_SF16: DXGI_FORMAT = 96;
pub const DXGI_FORMAT_BC6H_TYPELESS: DXGI_FORMAT = 94;
pub const DXGI_FORMAT_BC6H_UF16: DXGI_FORMAT = 95;
pub const DXGI_FORMAT_BC7_TYPELESS: DXGI_FORMAT = 97;
pub const DXGI_FORMAT_BC7_UNORM: DXGI_FORMAT = 98;
pub const DXGI_FORMAT_BC7_UNORM_SRGB: DXGI_FORMAT = 99;
pub const DXGI_FORMAT_D16_UNORM: DXGI_FORMAT = 55;
pub const DXGI_FORMAT_D24_UNORM_S8_UINT: DXGI_FORMAT = 45;
pub const DXGI_FORMAT_D32_FLOAT: DXGI_FORMAT = 40;
pub const DXGI_FORMAT_D32_FLOAT_S8X24_UINT: DXGI_FORMAT = 20;
pub const DXGI_FORMAT_FORCE_UINT: DXGI_FORMAT = -1;
pub const DXGI_FORMAT_G8R8_G8B8_UNORM: DXGI_FORMAT = 69;
pub const DXGI_FORMAT_IA44: DXGI_FORMAT = 112;
pub const DXGI_FORMAT_NV11: DXGI_FORMAT = 110;
pub const DXGI_FORMAT_NV12: DXGI_FORMAT = 103;
pub const DXGI_FORMAT_P010: DXGI_FORMAT = 104;
pub const DXGI_FORMAT_P016: DXGI_FORMAT = 105;
pub const DXGI_FORMAT_P208: DXGI_FORMAT = 130;
pub const DXGI_FORMAT_P8: DXGI_FORMAT = 113;
pub const DXGI_FORMAT_R10G10B10A2_TYPELESS: DXGI_FORMAT = 23;
pub const DXGI_FORMAT_R10G10B10A2_UINT: DXGI_FORMAT = 25;
pub const DXGI_FORMAT_R10G10B10A2_UNORM: DXGI_FORMAT = 24;
pub const DXGI_FORMAT_R10G10B10_XR_BIAS_A2_UNORM: DXGI_FORMAT = 89;
pub const DXGI_FORMAT_R11G11B10_FLOAT: DXGI_FORMAT = 26;
pub const DXGI_FORMAT_R16G16B16A16_FLOAT: DXGI_FORMAT = 10;
pub const DXGI_FORMAT_R16G16B16A16_SINT: DXGI_FORMAT = 14;
pub const DXGI_FORMAT_R16G16B16A16_SNORM: DXGI_FORMAT = 13;
pub const DXGI_FORMAT_R16G16B16A16_TYPELESS: DXGI_FORMAT = 9;
pub const DXGI_FORMAT_R16G16B16A16_UINT: DXGI_FORMAT = 12;
pub const DXGI_FORMAT_R16G16B16A16_UNORM: DXGI_FORMAT = 11;
pub const DXGI_FORMAT_R16G16_FLOAT: DXGI_FORMAT = 34;
pub const DXGI_FORMAT_R16G16_SINT: DXGI_FORMAT = 38;
pub const DXGI_FORMAT_R16G16_SNORM: DXGI_FORMAT = 37;
pub const DXGI_FORMAT_R16G16_TYPELESS: DXGI_FORMAT = 33;
pub const DXGI_FORMAT_R16G16_UINT: DXGI_FORMAT = 36;
pub const DXGI_FORMAT_R16G16_UNORM: DXGI_FORMAT = 35;
pub const DXGI_FORMAT_R16_FLOAT: DXGI_FORMAT = 54;
pub const DXGI_FORMAT_R16_SINT: DXGI_FORMAT = 59;
pub const DXGI_FORMAT_R16_SNORM: DXGI_FORMAT = 58;
pub const DXGI_FORMAT_R16_TYPELESS: DXGI_FORMAT = 53;
pub const DXGI_FORMAT_R16_UINT: DXGI_FORMAT = 57;
pub const DXGI_FORMAT_R16_UNORM: DXGI_FORMAT = 56;
pub const DXGI_FORMAT_R1_UNORM: DXGI_FORMAT = 66;
pub const DXGI_FORMAT_R24G8_TYPELESS: DXGI_FORMAT = 44;
pub const DXGI_FORMAT_R24_UNORM_X8_TYPELESS: DXGI_FORMAT = 46;
pub const DXGI_FORMAT_R32G32B32A32_FLOAT: DXGI_FORMAT = 2;
pub const DXGI_FORMAT_R32G32B32A32_SINT: DXGI_FORMAT = 4;
pub const DXGI_FORMAT_R32G32B32A32_TYPELESS: DXGI_FORMAT = 1;
pub const DXGI_FORMAT_R32G32B32A32_UINT: DXGI_FORMAT = 3;
pub const DXGI_FORMAT_R32G32B32_FLOAT: DXGI_FORMAT = 6;
pub const DXGI_FORMAT_R32G32B32_SINT: DXGI_FORMAT = 8;
pub const DXGI_FORMAT_R32G32B32_TYPELESS: DXGI_FORMAT = 5;
pub const DXGI_FORMAT_R32G32B32_UINT: DXGI_FORMAT = 7;
pub const DXGI_FORMAT_R32G32_FLOAT: DXGI_FORMAT = 16;
pub const DXGI_FORMAT_R32G32_SINT: DXGI_FORMAT = 18;
pub const DXGI_FORMAT_R32G32_TYPELESS: DXGI_FORMAT = 15;
pub const DXGI_FORMAT_R32G32_UINT: DXGI_FORMAT = 17;
pub const DXGI_FORMAT_R32G8X24_TYPELESS: DXGI_FORMAT = 19;
pub const DXGI_FORMAT_R32_FLOAT: DXGI_FORMAT = 41;
pub const DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS: DXGI_FORMAT = 21;
pub const DXGI_FORMAT_R32_SINT: DXGI_FORMAT = 43;
pub const DXGI_FORMAT_R32_TYPELESS: DXGI_FORMAT = 39;
pub const DXGI_FORMAT_R32_UINT: DXGI_FORMAT = 42;
pub const DXGI_FORMAT_R8G8B8A8_SINT: DXGI_FORMAT = 32;
pub const DXGI_FORMAT_R8G8B8A8_SNORM: DXGI_FORMAT = 31;
pub const DXGI_FORMAT_R8G8B8A8_TYPELESS: DXGI_FORMAT = 27;
pub const DXGI_FORMAT_R8G8B8A8_UINT: DXGI_FORMAT = 30;
pub const DXGI_FORMAT_R8G8B8A8_UNORM: DXGI_FORMAT = 28;
pub const DXGI_FORMAT_R8G8B8A8_UNORM_SRGB: DXGI_FORMAT = 29;
pub const DXGI_FORMAT_R8G8_B8G8_UNORM: DXGI_FORMAT = 68;
pub const DXGI_FORMAT_R8G8_SINT: DXGI_FORMAT = 52;
pub const DXGI_FORMAT_R8G8_SNORM: DXGI_FORMAT = 51;
pub const DXGI_FORMAT_R8G8_TYPELESS: DXGI_FORMAT = 48;
pub const DXGI_FORMAT_R8G8_UINT: DXGI_FORMAT = 50;
pub const DXGI_FORMAT_R8G8_UNORM: DXGI_FORMAT = 49;
pub const DXGI_FORMAT_R8_SINT: DXGI_FORMAT = 64;
pub const DXGI_FORMAT_R8_SNORM: DXGI_FORMAT = 63;
pub const DXGI_FORMAT_R8_TYPELESS: DXGI_FORMAT = 60;
pub const DXGI_FORMAT_R8_UINT: DXGI_FORMAT = 62;
pub const DXGI_FORMAT_R8_UNORM: DXGI_FORMAT = 61;
pub const DXGI_FORMAT_R9G9B9E5_SHAREDEXP: DXGI_FORMAT = 67;
pub const DXGI_FORMAT_SAMPLER_FEEDBACK_MIN_MIP_OPAQUE: DXGI_FORMAT = 189;
pub const DXGI_FORMAT_SAMPLER_FEEDBACK_MIP_REGION_USED_OPAQUE: DXGI_FORMAT = 190;
pub const DXGI_FORMAT_UNKNOWN: DXGI_FORMAT = 0;
pub const DXGI_FORMAT_V208: DXGI_FORMAT = 131;
pub const DXGI_FORMAT_V408: DXGI_FORMAT = 132;
pub const DXGI_FORMAT_X24_TYPELESS_G8_UINT: DXGI_FORMAT = 47;
pub const DXGI_FORMAT_X32_TYPELESS_G8X24_UINT: DXGI_FORMAT = 22;
pub const DXGI_FORMAT_Y210: DXGI_FORMAT = 108;
pub const DXGI_FORMAT_Y216: DXGI_FORMAT = 109;
pub const DXGI_FORMAT_Y410: DXGI_FORMAT = 101;
pub const DXGI_FORMAT_Y416: DXGI_FORMAT = 102;
pub const DXGI_FORMAT_YUY2: DXGI_FORMAT = 107;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_FRAME_STATISTICS {
    pub PresentCount: u32,
    pub PresentRefreshCount: u32,
    pub SyncRefreshCount: u32,
    pub SyncQPCTime: i64,
    pub SyncGPUTime: i64,
}
pub type DXGI_GPU_PREFERENCE = i32;
pub const DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE: DXGI_GPU_PREFERENCE = 2;
pub const DXGI_GPU_PREFERENCE_MINIMUM_POWER: DXGI_GPU_PREFERENCE = 1;
pub const DXGI_GPU_PREFERENCE_UNSPECIFIED: DXGI_GPU_PREFERENCE = 0;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_MODE_DESC {
    pub Width: u32,
    pub Height: u32,
    pub RefreshRate: DXGI_RATIONAL,
    pub Format: DXGI_FORMAT,
    pub ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER,
    pub Scaling: DXGI_MODE_SCALING,
}
pub type DXGI_MODE_SCALING = i32;
pub type DXGI_MODE_SCANLINE_ORDER = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_RATIONAL {
    pub Numerator: u32,
    pub Denominator: u32,
}
pub type DXGI_RESIDENCY = i32;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_SAMPLE_DESC {
    pub Count: u32,
    pub Quality: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_SHARED_RESOURCE {
    pub Handle: HANDLE,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_SURFACE_DESC {
    pub Width: u32,
    pub Height: u32,
    pub Format: DXGI_FORMAT,
    pub SampleDesc: DXGI_SAMPLE_DESC,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DXGI_SWAP_CHAIN_DESC {
    pub BufferDesc: DXGI_MODE_DESC,
    pub SampleDesc: DXGI_SAMPLE_DESC,
    pub BufferUsage: DXGI_USAGE,
    pub BufferCount: u32,
    pub OutputWindow: HWND,
    pub Windowed: windows_core::BOOL,
    pub SwapEffect: DXGI_SWAP_EFFECT,
    pub Flags: u32,
}
pub type DXGI_SWAP_EFFECT = i32;
pub const DXGI_SWAP_EFFECT_DISCARD: DXGI_SWAP_EFFECT = 0;
pub const DXGI_SWAP_EFFECT_FLIP_DISCARD: DXGI_SWAP_EFFECT = 4;
pub const DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL: DXGI_SWAP_EFFECT = 3;
pub const DXGI_SWAP_EFFECT_SEQUENTIAL: DXGI_SWAP_EFFECT = 1;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DXGI_USAGE(pub u32);
pub const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 32;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HANDLE(pub *mut core::ffi::c_void);
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HINSTANCE(pub *mut core::ffi::c_void);
pub type HMODULE = HINSTANCE;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HWND(pub *mut core::ffi::c_void);
windows_core::imp::define_interface!(
    ID3D10Blob,
    ID3D10Blob_Vtbl,
    0x8ba5fb08_5195_40e2_ac58_0d989c3a0102
);
windows_core::imp::interface_hierarchy!(ID3D10Blob, windows_core::IUnknown);
impl ID3D10Blob {
    pub unsafe fn GetBufferPointer(&self) -> *mut core::ffi::c_void {
        unsafe {
            (windows_core::Interface::vtable(self).GetBufferPointer)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetBufferSize(&self) -> usize {
        unsafe {
            (windows_core::Interface::vtable(self).GetBufferSize)(windows_core::Interface::as_raw(
                self,
            ))
        }
    }
}
#[repr(C)]
pub struct ID3D10Blob_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub GetBufferPointer:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> *mut core::ffi::c_void,
    pub GetBufferSize: unsafe extern "system" fn(*mut core::ffi::c_void) -> usize,
}
pub trait ID3D10Blob_Impl: windows_core::IUnknownImpl {
    fn GetBufferPointer(&self) -> *mut core::ffi::c_void;
    fn GetBufferSize(&self) -> usize;
}
impl ID3D10Blob_Vtbl {
    pub const fn new<Identity: ID3D10Blob_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn GetBufferPointer<
            Identity: ID3D10Blob_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> *mut core::ffi::c_void {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D10Blob_Impl::GetBufferPointer(this)
            }
        }
        unsafe extern "system" fn GetBufferSize<Identity: ID3D10Blob_Impl, const OFFSET: isize>(
            this: *mut core::ffi::c_void,
        ) -> usize {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D10Blob_Impl::GetBufferSize(this)
            }
        }
        Self {
            base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
            GetBufferPointer: GetBufferPointer::<Identity, OFFSET>,
            GetBufferSize: GetBufferSize::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<ID3D10Blob as windows_core::Interface>::IID
    }
}
impl windows_core::RuntimeName for ID3D10Blob {}
windows_core::imp::define_interface!(
    ID3D11Asynchronous,
    ID3D11Asynchronous_Vtbl,
    0x4b35d0cd_1e15_4258_9c98_1b1333f6dd3b
);
impl core::ops::Deref for ID3D11Asynchronous {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Asynchronous,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11Asynchronous_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    GetDataSize: usize,
}
impl windows_core::RuntimeName for ID3D11Asynchronous {}
windows_core::imp::define_interface!(
    ID3D11BlendState,
    ID3D11BlendState_Vtbl,
    0x75b68faa_347d_4159_8f45_a0640f01cd9a
);
impl core::ops::Deref for ID3D11BlendState {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11BlendState,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
impl ID3D11BlendState {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_BLEND_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11BlendState_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_BLEND_DESC),
}
impl windows_core::RuntimeName for ID3D11BlendState {}
windows_core::imp::define_interface!(
    ID3D11Buffer,
    ID3D11Buffer_Vtbl,
    0x48570b85_d1ee_4fcd_a250_eb350722b037
);
impl core::ops::Deref for ID3D11Buffer {
    type Target = ID3D11Resource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Buffer,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Resource
);
impl ID3D11Buffer {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_BUFFER_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11Buffer_Vtbl {
    pub base__: ID3D11Resource_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_BUFFER_DESC),
}
impl windows_core::RuntimeName for ID3D11Buffer {}
windows_core::imp::define_interface!(
    ID3D11ClassInstance,
    ID3D11ClassInstance_Vtbl,
    0xa6cd7faa_b0b7_4a2f_9436_8662a65797cb
);
impl core::ops::Deref for ID3D11ClassInstance {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11ClassInstance,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11ClassInstance_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    GetClassLinkage: usize,
    GetDesc: usize,
    GetInstanceName: usize,
    GetTypeName: usize,
}
impl windows_core::RuntimeName for ID3D11ClassInstance {}
windows_core::imp::define_interface!(
    ID3D11ClassLinkage,
    ID3D11ClassLinkage_Vtbl,
    0xddf57cba_9543_46e4_a12b_f207a0fe7fed
);
impl core::ops::Deref for ID3D11ClassLinkage {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11ClassLinkage,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11ClassLinkage_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    GetClassInstance: usize,
    CreateClassInstance: usize,
}
impl windows_core::RuntimeName for ID3D11ClassLinkage {}
windows_core::imp::define_interface!(
    ID3D11CommandList,
    ID3D11CommandList_Vtbl,
    0xa24bc4d1_769e_43f7_8013_98ff566c18e2
);
impl core::ops::Deref for ID3D11CommandList {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11CommandList,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11CommandList_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    GetContextFlags: usize,
}
impl windows_core::RuntimeName for ID3D11CommandList {}
windows_core::imp::define_interface!(
    ID3D11ComputeShader,
    ID3D11ComputeShader_Vtbl,
    0x4f5b196e_c2bd_495e_bd01_1fded38e4969
);
impl core::ops::Deref for ID3D11ComputeShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11ComputeShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11ComputeShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11ComputeShader {}
windows_core::imp::define_interface!(
    ID3D11Counter,
    ID3D11Counter_Vtbl,
    0x6e8c49fb_a371_4770_b440_29086022b741
);
impl core::ops::Deref for ID3D11Counter {
    type Target = ID3D11Asynchronous;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Counter,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Asynchronous
);
#[repr(C)]
pub struct ID3D11Counter_Vtbl {
    pub base__: ID3D11Asynchronous_Vtbl,
    GetDesc: usize,
}
impl windows_core::RuntimeName for ID3D11Counter {}
windows_core::imp::define_interface!(
    ID3D11DepthStencilState,
    ID3D11DepthStencilState_Vtbl,
    0x03823efb_8d8f_4e1c_9aa2_f64bb2cbfdf1
);
impl core::ops::Deref for ID3D11DepthStencilState {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11DepthStencilState,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
impl ID3D11DepthStencilState {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_DEPTH_STENCIL_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11DepthStencilState_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_DEPTH_STENCIL_DESC),
}
impl windows_core::RuntimeName for ID3D11DepthStencilState {}
windows_core::imp::define_interface!(
    ID3D11DepthStencilView,
    ID3D11DepthStencilView_Vtbl,
    0x9fdac92a_1876_48c3_afad_25b94f84a9b6
);
impl core::ops::Deref for ID3D11DepthStencilView {
    type Target = ID3D11View;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11DepthStencilView,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11View
);
impl ID3D11DepthStencilView {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_DEPTH_STENCIL_VIEW_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11DepthStencilView_Vtbl {
    pub base__: ID3D11View_Vtbl,
    pub GetDesc:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_DEPTH_STENCIL_VIEW_DESC),
}
impl windows_core::RuntimeName for ID3D11DepthStencilView {}
windows_core::imp::define_interface!(
    ID3D11Device,
    ID3D11Device_Vtbl,
    0xdb6f6ddb_ac77_4e88_8253_819df9bbf140
);
windows_core::imp::interface_hierarchy!(ID3D11Device, windows_core::IUnknown);
impl ID3D11Device {
    pub unsafe fn CreateBuffer(
        &self,
        pdesc: *const D3D11_BUFFER_DESC,
        pinitialdata: Option<*const D3D11_SUBRESOURCE_DATA>,
        ppbuffer: Option<*mut Option<ID3D11Buffer>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateBuffer)(
                windows_core::Interface::as_raw(self),
                pdesc,
                pinitialdata.unwrap_or(core::mem::zeroed()) as _,
                ppbuffer.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateTexture1D(
        &self,
        pdesc: *const D3D11_TEXTURE1D_DESC,
        pinitialdata: Option<*const D3D11_SUBRESOURCE_DATA>,
        pptexture1d: Option<*mut Option<ID3D11Texture1D>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateTexture1D)(
                windows_core::Interface::as_raw(self),
                pdesc,
                pinitialdata.unwrap_or(core::mem::zeroed()) as _,
                pptexture1d.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateTexture2D(
        &self,
        pdesc: *const D3D11_TEXTURE2D_DESC,
        pinitialdata: Option<*const D3D11_SUBRESOURCE_DATA>,
        pptexture2d: Option<*mut Option<ID3D11Texture2D>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateTexture2D)(
                windows_core::Interface::as_raw(self),
                pdesc,
                pinitialdata.unwrap_or(core::mem::zeroed()) as _,
                pptexture2d.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateTexture3D(
        &self,
        pdesc: *const D3D11_TEXTURE3D_DESC,
        pinitialdata: Option<*const D3D11_SUBRESOURCE_DATA>,
        pptexture3d: Option<*mut Option<ID3D11Texture3D>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateTexture3D)(
                windows_core::Interface::as_raw(self),
                pdesc,
                pinitialdata.unwrap_or(core::mem::zeroed()) as _,
                pptexture3d.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateShaderResourceView<P0>(
        &self,
        presource: P0,
        pdesc: Option<*const D3D11_SHADER_RESOURCE_VIEW_DESC>,
        ppsrview: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateShaderResourceView)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                pdesc.unwrap_or(core::mem::zeroed()) as _,
                ppsrview.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateUnorderedAccessView<P0>(
        &self,
        presource: P0,
        pdesc: Option<*const D3D11_UNORDERED_ACCESS_VIEW_DESC>,
        ppuaview: Option<*mut Option<ID3D11UnorderedAccessView>>,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateUnorderedAccessView)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                pdesc.unwrap_or(core::mem::zeroed()) as _,
                ppuaview.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateRenderTargetView<P0>(
        &self,
        presource: P0,
        pdesc: Option<*const D3D11_RENDER_TARGET_VIEW_DESC>,
        pprtview: Option<*mut Option<ID3D11RenderTargetView>>,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateRenderTargetView)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                pdesc.unwrap_or(core::mem::zeroed()) as _,
                pprtview.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateDepthStencilView<P0>(
        &self,
        presource: P0,
        pdesc: Option<*const D3D11_DEPTH_STENCIL_VIEW_DESC>,
        ppdepthstencilview: Option<*mut Option<ID3D11DepthStencilView>>,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateDepthStencilView)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                pdesc.unwrap_or(core::mem::zeroed()) as _,
                ppdepthstencilview.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateInputLayout(
        &self,
        pinputelementdescs: &[D3D11_INPUT_ELEMENT_DESC],
        pshaderbytecodewithinputsignature: &[u8],
        ppinputlayout: Option<*mut Option<ID3D11InputLayout>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateInputLayout)(
                windows_core::Interface::as_raw(self),
                pinputelementdescs.as_ptr(),
                pinputelementdescs.len().try_into().unwrap(),
                core::mem::transmute(pshaderbytecodewithinputsignature.as_ptr()),
                pshaderbytecodewithinputsignature.len().try_into().unwrap(),
                ppinputlayout.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateVertexShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        ppvertexshader: Option<*mut Option<ID3D11VertexShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateVertexShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                ppvertexshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateGeometryShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        ppgeometryshader: Option<*mut Option<ID3D11GeometryShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateGeometryShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                ppgeometryshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateGeometryShaderWithStreamOutput<P7>(
        &self,
        pshaderbytecode: &[u8],
        psodeclaration: Option<&[D3D11_SO_DECLARATION_ENTRY]>,
        pbufferstrides: Option<&[u32]>,
        rasterizedstream: u32,
        pclasslinkage: P7,
        ppgeometryshader: Option<*mut Option<ID3D11GeometryShader>>,
    ) -> windows_core::HRESULT
    where
        P7: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateGeometryShaderWithStreamOutput)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                psodeclaration.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                psodeclaration.map_or(0, |slice| slice.len().try_into().unwrap()),
                pbufferstrides.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                pbufferstrides.map_or(0, |slice| slice.len().try_into().unwrap()),
                rasterizedstream,
                pclasslinkage.param().abi(),
                ppgeometryshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreatePixelShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        pppixelshader: Option<*mut Option<ID3D11PixelShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreatePixelShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                pppixelshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateHullShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        pphullshader: Option<*mut Option<ID3D11HullShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateHullShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                pphullshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateDomainShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        ppdomainshader: Option<*mut Option<ID3D11DomainShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateDomainShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                ppdomainshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateComputeShader<P2>(
        &self,
        pshaderbytecode: &[u8],
        pclasslinkage: P2,
        ppcomputeshader: Option<*mut Option<ID3D11ComputeShader>>,
    ) -> windows_core::HRESULT
    where
        P2: windows_core::Param<ID3D11ClassLinkage>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CreateComputeShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pshaderbytecode.as_ptr()),
                pshaderbytecode.len().try_into().unwrap(),
                pclasslinkage.param().abi(),
                ppcomputeshader.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateClassLinkage(&self) -> windows_core::Result<ID3D11ClassLinkage> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateClassLinkage)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub unsafe fn CreateBlendState(
        &self,
        pblendstatedesc: *const D3D11_BLEND_DESC,
        ppblendstate: Option<*mut Option<ID3D11BlendState>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateBlendState)(
                windows_core::Interface::as_raw(self),
                pblendstatedesc,
                ppblendstate.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateDepthStencilState(
        &self,
        pdepthstencildesc: *const D3D11_DEPTH_STENCIL_DESC,
        ppdepthstencilstate: Option<*mut Option<ID3D11DepthStencilState>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateDepthStencilState)(
                windows_core::Interface::as_raw(self),
                pdepthstencildesc,
                ppdepthstencilstate.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateRasterizerState(
        &self,
        prasterizerdesc: *const D3D11_RASTERIZER_DESC,
        pprasterizerstate: Option<*mut Option<ID3D11RasterizerState>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateRasterizerState)(
                windows_core::Interface::as_raw(self),
                prasterizerdesc,
                pprasterizerstate.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateSamplerState(
        &self,
        psamplerdesc: *const D3D11_SAMPLER_DESC,
        ppsamplerstate: Option<*mut Option<ID3D11SamplerState>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateSamplerState)(
                windows_core::Interface::as_raw(self),
                psamplerdesc,
                ppsamplerstate.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateQuery(
        &self,
        pquerydesc: *const D3D11_QUERY_DESC,
        ppquery: Option<*mut Option<ID3D11Query>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateQuery)(
                windows_core::Interface::as_raw(self),
                pquerydesc,
                ppquery.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreatePredicate(
        &self,
        ppredicatedesc: *const D3D11_QUERY_DESC,
        pppredicate: Option<*mut Option<ID3D11Predicate>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreatePredicate)(
                windows_core::Interface::as_raw(self),
                ppredicatedesc,
                pppredicate.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateCounter(
        &self,
        pcounterdesc: *const D3D11_COUNTER_DESC,
        ppcounter: Option<*mut Option<ID3D11Counter>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateCounter)(
                windows_core::Interface::as_raw(self),
                pcounterdesc,
                ppcounter.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CreateDeferredContext(
        &self,
        contextflags: u32,
        ppdeferredcontext: Option<*mut Option<ID3D11DeviceContext>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateDeferredContext)(
                windows_core::Interface::as_raw(self),
                contextflags,
                ppdeferredcontext.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn OpenSharedResource<T>(
        &self,
        hresource: HANDLE,
        result__: *mut Option<T>,
    ) -> windows_core::Result<()>
    where
        T: windows_core::Interface,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OpenSharedResource)(
                windows_core::Interface::as_raw(self),
                hresource,
                &T::IID,
                result__ as *mut _ as *mut _,
            )
            .ok()
        }
    }
    pub unsafe fn CheckFormatSupport(&self, format: DXGI_FORMAT) -> windows_core::Result<u32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CheckFormatSupport)(
                windows_core::Interface::as_raw(self),
                format,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub unsafe fn CheckMultisampleQualityLevels(
        &self,
        format: DXGI_FORMAT,
        samplecount: u32,
    ) -> windows_core::Result<u32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CheckMultisampleQualityLevels)(
                windows_core::Interface::as_raw(self),
                format,
                samplecount,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub unsafe fn CheckCounterInfo(&self) -> D3D11_COUNTER_INFO {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CheckCounterInfo)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            result__
        }
    }
    pub unsafe fn CheckCounter(
        &self,
        pdesc: *const D3D11_COUNTER_DESC,
        ptype: *mut D3D11_COUNTER_TYPE,
        pactivecounters: *mut u32,
        szname: Option<windows_core::PSTR>,
        pnamelength: Option<*mut u32>,
        szunits: Option<windows_core::PSTR>,
        punitslength: Option<*mut u32>,
        szdescription: Option<windows_core::PSTR>,
        pdescriptionlength: Option<*mut u32>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CheckCounter)(
                windows_core::Interface::as_raw(self),
                pdesc,
                ptype as _,
                pactivecounters as _,
                szname.unwrap_or(core::mem::zeroed()) as _,
                pnamelength.unwrap_or(core::mem::zeroed()) as _,
                szunits.unwrap_or(core::mem::zeroed()) as _,
                punitslength.unwrap_or(core::mem::zeroed()) as _,
                szdescription.unwrap_or(core::mem::zeroed()) as _,
                pdescriptionlength.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn CheckFeatureSupport(
        &self,
        feature: D3D11_FEATURE,
        pfeaturesupportdata: *mut core::ffi::c_void,
        featuresupportdatasize: u32,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CheckFeatureSupport)(
                windows_core::Interface::as_raw(self),
                feature,
                pfeaturesupportdata as _,
                featuresupportdatasize,
            )
        }
    }
    pub unsafe fn GetPrivateData(
        &self,
        guid: *const windows_core::GUID,
        pdatasize: *mut u32,
        pdata: Option<*mut core::ffi::c_void>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetPrivateData)(
                windows_core::Interface::as_raw(self),
                guid,
                pdatasize as _,
                pdata.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn SetPrivateData(
        &self,
        guid: *const windows_core::GUID,
        datasize: u32,
        pdata: Option<*const core::ffi::c_void>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).SetPrivateData)(
                windows_core::Interface::as_raw(self),
                guid,
                datasize,
                pdata.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn SetPrivateDataInterface<P1>(
        &self,
        guid: *const windows_core::GUID,
        pdata: P1,
    ) -> windows_core::HRESULT
    where
        P1: windows_core::Param<windows_core::IUnknown>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetPrivateDataInterface)(
                windows_core::Interface::as_raw(self),
                guid,
                pdata.param().abi(),
            )
        }
    }
    pub unsafe fn GetFeatureLevel(&self) -> D3D_FEATURE_LEVEL {
        unsafe {
            (windows_core::Interface::vtable(self).GetFeatureLevel)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetCreationFlags(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetCreationFlags)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetDeviceRemovedReason(&self) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDeviceRemovedReason)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetImmediateContext(&self) -> windows_core::Result<ID3D11DeviceContext> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetImmediateContext)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            windows_core::imp::Type::from_abi(result__)
        }
    }
    pub unsafe fn SetExceptionMode(&self, raiseflags: u32) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).SetExceptionMode)(
                windows_core::Interface::as_raw(self),
                raiseflags,
            )
        }
    }
    pub unsafe fn GetExceptionMode(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetExceptionMode)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11Device_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub CreateBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_BUFFER_DESC,
        *const D3D11_SUBRESOURCE_DATA,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateTexture1D: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_TEXTURE1D_DESC,
        *const D3D11_SUBRESOURCE_DATA,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateTexture2D: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_TEXTURE2D_DESC,
        *const D3D11_SUBRESOURCE_DATA,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateTexture3D: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_TEXTURE3D_DESC,
        *const D3D11_SUBRESOURCE_DATA,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateShaderResourceView: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const D3D11_SHADER_RESOURCE_VIEW_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateUnorderedAccessView: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const D3D11_UNORDERED_ACCESS_VIEW_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateRenderTargetView: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const D3D11_RENDER_TARGET_VIEW_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateDepthStencilView: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const D3D11_DEPTH_STENCIL_VIEW_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateInputLayout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_INPUT_ELEMENT_DESC,
        u32,
        *const core::ffi::c_void,
        usize,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateVertexShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateGeometryShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateGeometryShaderWithStreamOutput: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *const D3D11_SO_DECLARATION_ENTRY,
        u32,
        *const u32,
        u32,
        u32,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    )
        -> windows_core::HRESULT,
    pub CreatePixelShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateHullShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateDomainShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateComputeShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const core::ffi::c_void,
        usize,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateClassLinkage: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateBlendState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_BLEND_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateDepthStencilState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_DEPTH_STENCIL_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateRasterizerState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_RASTERIZER_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateSamplerState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_SAMPLER_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateQuery: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_QUERY_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreatePredicate: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_QUERY_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateCounter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_COUNTER_DESC,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateDeferredContext: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub OpenSharedResource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HANDLE,
        *const windows_core::GUID,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CheckFormatSupport: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DXGI_FORMAT,
        *mut u32,
    ) -> windows_core::HRESULT,
    pub CheckMultisampleQualityLevels: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DXGI_FORMAT,
        u32,
        *mut u32,
    ) -> windows_core::HRESULT,
    pub CheckCounterInfo:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_COUNTER_INFO),
    pub CheckCounter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const D3D11_COUNTER_DESC,
        *mut D3D11_COUNTER_TYPE,
        *mut u32,
        windows_core::PSTR,
        *mut u32,
        windows_core::PSTR,
        *mut u32,
        windows_core::PSTR,
        *mut u32,
    ) -> windows_core::HRESULT,
    pub CheckFeatureSupport: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        D3D11_FEATURE,
        *mut core::ffi::c_void,
        u32,
    ) -> windows_core::HRESULT,
    pub GetPrivateData: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const windows_core::GUID,
        *mut u32,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetPrivateData: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const windows_core::GUID,
        u32,
        *const core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetPrivateDataInterface: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const windows_core::GUID,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetFeatureLevel: unsafe extern "system" fn(*mut core::ffi::c_void) -> D3D_FEATURE_LEVEL,
    pub GetCreationFlags: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetDeviceRemovedReason:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub GetImmediateContext:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void),
    pub SetExceptionMode:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> windows_core::HRESULT,
    pub GetExceptionMode: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
}
pub trait ID3D11Device_Impl: windows_core::IUnknownImpl {
    fn CreateBuffer(
        &self,
        pdesc: *const D3D11_BUFFER_DESC,
        pinitialdata: *const D3D11_SUBRESOURCE_DATA,
        ppbuffer: windows_core::OutRef<ID3D11Buffer>,
    ) -> windows_core::Result<()>;
    fn CreateTexture1D(
        &self,
        pdesc: *const D3D11_TEXTURE1D_DESC,
        pinitialdata: *const D3D11_SUBRESOURCE_DATA,
        pptexture1d: windows_core::OutRef<ID3D11Texture1D>,
    ) -> windows_core::Result<()>;
    fn CreateTexture2D(
        &self,
        pdesc: *const D3D11_TEXTURE2D_DESC,
        pinitialdata: *const D3D11_SUBRESOURCE_DATA,
        pptexture2d: windows_core::OutRef<ID3D11Texture2D>,
    ) -> windows_core::Result<()>;
    fn CreateTexture3D(
        &self,
        pdesc: *const D3D11_TEXTURE3D_DESC,
        pinitialdata: *const D3D11_SUBRESOURCE_DATA,
        pptexture3d: windows_core::OutRef<ID3D11Texture3D>,
    ) -> windows_core::Result<()>;
    fn CreateShaderResourceView(
        &self,
        presource: windows_core::Ref<ID3D11Resource>,
        pdesc: *const D3D11_SHADER_RESOURCE_VIEW_DESC,
        ppsrview: windows_core::OutRef<ID3D11ShaderResourceView>,
    ) -> windows_core::Result<()>;
    fn CreateUnorderedAccessView(
        &self,
        presource: windows_core::Ref<ID3D11Resource>,
        pdesc: *const D3D11_UNORDERED_ACCESS_VIEW_DESC,
        ppuaview: windows_core::OutRef<ID3D11UnorderedAccessView>,
    ) -> windows_core::Result<()>;
    fn CreateRenderTargetView(
        &self,
        presource: windows_core::Ref<ID3D11Resource>,
        pdesc: *const D3D11_RENDER_TARGET_VIEW_DESC,
        pprtview: windows_core::OutRef<ID3D11RenderTargetView>,
    ) -> windows_core::Result<()>;
    fn CreateDepthStencilView(
        &self,
        presource: windows_core::Ref<ID3D11Resource>,
        pdesc: *const D3D11_DEPTH_STENCIL_VIEW_DESC,
        ppdepthstencilview: windows_core::OutRef<ID3D11DepthStencilView>,
    ) -> windows_core::Result<()>;
    fn CreateInputLayout(
        &self,
        pinputelementdescs: *const D3D11_INPUT_ELEMENT_DESC,
        numelements: u32,
        pshaderbytecodewithinputsignature: *const core::ffi::c_void,
        bytecodelength: usize,
        ppinputlayout: windows_core::OutRef<ID3D11InputLayout>,
    ) -> windows_core::Result<()>;
    fn CreateVertexShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        ppvertexshader: windows_core::OutRef<ID3D11VertexShader>,
    ) -> windows_core::Result<()>;
    fn CreateGeometryShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        ppgeometryshader: windows_core::OutRef<ID3D11GeometryShader>,
    ) -> windows_core::Result<()>;
    fn CreateGeometryShaderWithStreamOutput(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        psodeclaration: *const D3D11_SO_DECLARATION_ENTRY,
        numentries: u32,
        pbufferstrides: *const u32,
        numstrides: u32,
        rasterizedstream: u32,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        ppgeometryshader: windows_core::OutRef<ID3D11GeometryShader>,
    ) -> windows_core::Result<()>;
    fn CreatePixelShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        pppixelshader: windows_core::OutRef<ID3D11PixelShader>,
    ) -> windows_core::Result<()>;
    fn CreateHullShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        pphullshader: windows_core::OutRef<ID3D11HullShader>,
    ) -> windows_core::Result<()>;
    fn CreateDomainShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        ppdomainshader: windows_core::OutRef<ID3D11DomainShader>,
    ) -> windows_core::Result<()>;
    fn CreateComputeShader(
        &self,
        pshaderbytecode: *const core::ffi::c_void,
        bytecodelength: usize,
        pclasslinkage: windows_core::Ref<ID3D11ClassLinkage>,
        ppcomputeshader: windows_core::OutRef<ID3D11ComputeShader>,
    ) -> windows_core::Result<()>;
    fn CreateClassLinkage(&self) -> windows_core::Result<ID3D11ClassLinkage>;
    fn CreateBlendState(
        &self,
        pblendstatedesc: *const D3D11_BLEND_DESC,
        ppblendstate: windows_core::OutRef<ID3D11BlendState>,
    ) -> windows_core::Result<()>;
    fn CreateDepthStencilState(
        &self,
        pdepthstencildesc: *const D3D11_DEPTH_STENCIL_DESC,
        ppdepthstencilstate: windows_core::OutRef<ID3D11DepthStencilState>,
    ) -> windows_core::Result<()>;
    fn CreateRasterizerState(
        &self,
        prasterizerdesc: *const D3D11_RASTERIZER_DESC,
        pprasterizerstate: windows_core::OutRef<ID3D11RasterizerState>,
    ) -> windows_core::Result<()>;
    fn CreateSamplerState(
        &self,
        psamplerdesc: *const D3D11_SAMPLER_DESC,
        ppsamplerstate: windows_core::OutRef<ID3D11SamplerState>,
    ) -> windows_core::Result<()>;
    fn CreateQuery(
        &self,
        pquerydesc: *const D3D11_QUERY_DESC,
        ppquery: windows_core::OutRef<ID3D11Query>,
    ) -> windows_core::Result<()>;
    fn CreatePredicate(
        &self,
        ppredicatedesc: *const D3D11_QUERY_DESC,
        pppredicate: windows_core::OutRef<ID3D11Predicate>,
    ) -> windows_core::Result<()>;
    fn CreateCounter(
        &self,
        pcounterdesc: *const D3D11_COUNTER_DESC,
        ppcounter: windows_core::OutRef<ID3D11Counter>,
    ) -> windows_core::Result<()>;
    fn CreateDeferredContext(
        &self,
        contextflags: u32,
        ppdeferredcontext: windows_core::OutRef<ID3D11DeviceContext>,
    ) -> windows_core::Result<()>;
    fn OpenSharedResource(
        &self,
        hresource: HANDLE,
        returnedinterface: *const windows_core::GUID,
        ppresource: *mut *mut core::ffi::c_void,
    ) -> windows_core::Result<()>;
    fn CheckFormatSupport(&self, format: DXGI_FORMAT) -> windows_core::Result<u32>;
    fn CheckMultisampleQualityLevels(
        &self,
        format: DXGI_FORMAT,
        samplecount: u32,
    ) -> windows_core::Result<u32>;
    fn CheckCounterInfo(&self, pcounterinfo: *mut D3D11_COUNTER_INFO);
    fn CheckCounter(
        &self,
        pdesc: *const D3D11_COUNTER_DESC,
        ptype: *mut D3D11_COUNTER_TYPE,
        pactivecounters: *mut u32,
        szname: windows_core::PSTR,
        pnamelength: *mut u32,
        szunits: windows_core::PSTR,
        punitslength: *mut u32,
        szdescription: windows_core::PSTR,
        pdescriptionlength: *mut u32,
    ) -> windows_core::Result<()>;
    fn CheckFeatureSupport(
        &self,
        feature: D3D11_FEATURE,
        pfeaturesupportdata: *mut core::ffi::c_void,
        featuresupportdatasize: u32,
    ) -> windows_core::Result<()>;
    fn GetPrivateData(
        &self,
        guid: *const windows_core::GUID,
        pdatasize: *mut u32,
        pdata: *mut core::ffi::c_void,
    ) -> windows_core::Result<()>;
    fn SetPrivateData(
        &self,
        guid: *const windows_core::GUID,
        datasize: u32,
        pdata: *const core::ffi::c_void,
    ) -> windows_core::Result<()>;
    fn SetPrivateDataInterface(
        &self,
        guid: *const windows_core::GUID,
        pdata: windows_core::Ref<windows_core::IUnknown>,
    ) -> windows_core::Result<()>;
    fn GetFeatureLevel(&self) -> D3D_FEATURE_LEVEL;
    fn GetCreationFlags(&self) -> u32;
    fn GetDeviceRemovedReason(&self) -> windows_core::Result<()>;
    fn GetImmediateContext(&self, ppimmediatecontext: windows_core::OutRef<ID3D11DeviceContext>);
    fn SetExceptionMode(&self, raiseflags: u32) -> windows_core::Result<()>;
    fn GetExceptionMode(&self) -> u32;
}
impl ID3D11Device_Vtbl {
    pub const fn new<Identity: ID3D11Device_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn CreateBuffer<Identity: ID3D11Device_Impl, const OFFSET: isize>(
            this: *mut core::ffi::c_void,
            pdesc: *const D3D11_BUFFER_DESC,
            pinitialdata: *const D3D11_SUBRESOURCE_DATA,
            ppbuffer: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateBuffer(
                    this,
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&pinitialdata),
                    core::mem::transmute_copy(&ppbuffer),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateTexture1D<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pdesc: *const D3D11_TEXTURE1D_DESC,
            pinitialdata: *const D3D11_SUBRESOURCE_DATA,
            pptexture1d: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateTexture1D(
                    this,
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&pinitialdata),
                    core::mem::transmute_copy(&pptexture1d),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateTexture2D<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pdesc: *const D3D11_TEXTURE2D_DESC,
            pinitialdata: *const D3D11_SUBRESOURCE_DATA,
            pptexture2d: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateTexture2D(
                    this,
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&pinitialdata),
                    core::mem::transmute_copy(&pptexture2d),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateTexture3D<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pdesc: *const D3D11_TEXTURE3D_DESC,
            pinitialdata: *const D3D11_SUBRESOURCE_DATA,
            pptexture3d: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateTexture3D(
                    this,
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&pinitialdata),
                    core::mem::transmute_copy(&pptexture3d),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateShaderResourceView<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            presource: *mut core::ffi::c_void,
            pdesc: *const D3D11_SHADER_RESOURCE_VIEW_DESC,
            ppsrview: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateShaderResourceView(
                    this,
                    core::mem::transmute_copy(&presource),
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&ppsrview),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateUnorderedAccessView<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            presource: *mut core::ffi::c_void,
            pdesc: *const D3D11_UNORDERED_ACCESS_VIEW_DESC,
            ppuaview: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateUnorderedAccessView(
                    this,
                    core::mem::transmute_copy(&presource),
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&ppuaview),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateRenderTargetView<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            presource: *mut core::ffi::c_void,
            pdesc: *const D3D11_RENDER_TARGET_VIEW_DESC,
            pprtview: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateRenderTargetView(
                    this,
                    core::mem::transmute_copy(&presource),
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&pprtview),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateDepthStencilView<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            presource: *mut core::ffi::c_void,
            pdesc: *const D3D11_DEPTH_STENCIL_VIEW_DESC,
            ppdepthstencilview: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateDepthStencilView(
                    this,
                    core::mem::transmute_copy(&presource),
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&ppdepthstencilview),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateInputLayout<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pinputelementdescs: *const D3D11_INPUT_ELEMENT_DESC,
            numelements: u32,
            pshaderbytecodewithinputsignature: *const core::ffi::c_void,
            bytecodelength: usize,
            ppinputlayout: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateInputLayout(
                    this,
                    core::mem::transmute_copy(&pinputelementdescs),
                    core::mem::transmute_copy(&numelements),
                    core::mem::transmute_copy(&pshaderbytecodewithinputsignature),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&ppinputlayout),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateVertexShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            ppvertexshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateVertexShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&ppvertexshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateGeometryShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            ppgeometryshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateGeometryShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&ppgeometryshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateGeometryShaderWithStreamOutput<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            psodeclaration: *const D3D11_SO_DECLARATION_ENTRY,
            numentries: u32,
            pbufferstrides: *const u32,
            numstrides: u32,
            rasterizedstream: u32,
            pclasslinkage: *mut core::ffi::c_void,
            ppgeometryshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateGeometryShaderWithStreamOutput(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&psodeclaration),
                    core::mem::transmute_copy(&numentries),
                    core::mem::transmute_copy(&pbufferstrides),
                    core::mem::transmute_copy(&numstrides),
                    core::mem::transmute_copy(&rasterizedstream),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&ppgeometryshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreatePixelShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            pppixelshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreatePixelShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&pppixelshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateHullShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            pphullshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateHullShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&pphullshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateDomainShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            ppdomainshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateDomainShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&ppdomainshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateComputeShader<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pshaderbytecode: *const core::ffi::c_void,
            bytecodelength: usize,
            pclasslinkage: *mut core::ffi::c_void,
            ppcomputeshader: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateComputeShader(
                    this,
                    core::mem::transmute_copy(&pshaderbytecode),
                    core::mem::transmute_copy(&bytecodelength),
                    core::mem::transmute_copy(&pclasslinkage),
                    core::mem::transmute_copy(&ppcomputeshader),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateClassLinkage<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pplinkage: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match ID3D11Device_Impl::CreateClassLinkage(this) {
                    Ok(ok__) => {
                        pplinkage.write(core::mem::transmute(ok__));
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn CreateBlendState<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pblendstatedesc: *const D3D11_BLEND_DESC,
            ppblendstate: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateBlendState(
                    this,
                    core::mem::transmute_copy(&pblendstatedesc),
                    core::mem::transmute_copy(&ppblendstate),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateDepthStencilState<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pdepthstencildesc: *const D3D11_DEPTH_STENCIL_DESC,
            ppdepthstencilstate: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateDepthStencilState(
                    this,
                    core::mem::transmute_copy(&pdepthstencildesc),
                    core::mem::transmute_copy(&ppdepthstencilstate),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateRasterizerState<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            prasterizerdesc: *const D3D11_RASTERIZER_DESC,
            pprasterizerstate: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateRasterizerState(
                    this,
                    core::mem::transmute_copy(&prasterizerdesc),
                    core::mem::transmute_copy(&pprasterizerstate),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateSamplerState<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            psamplerdesc: *const D3D11_SAMPLER_DESC,
            ppsamplerstate: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateSamplerState(
                    this,
                    core::mem::transmute_copy(&psamplerdesc),
                    core::mem::transmute_copy(&ppsamplerstate),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateQuery<Identity: ID3D11Device_Impl, const OFFSET: isize>(
            this: *mut core::ffi::c_void,
            pquerydesc: *const D3D11_QUERY_DESC,
            ppquery: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateQuery(
                    this,
                    core::mem::transmute_copy(&pquerydesc),
                    core::mem::transmute_copy(&ppquery),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreatePredicate<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            ppredicatedesc: *const D3D11_QUERY_DESC,
            pppredicate: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreatePredicate(
                    this,
                    core::mem::transmute_copy(&ppredicatedesc),
                    core::mem::transmute_copy(&pppredicate),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateCounter<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pcounterdesc: *const D3D11_COUNTER_DESC,
            ppcounter: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateCounter(
                    this,
                    core::mem::transmute_copy(&pcounterdesc),
                    core::mem::transmute_copy(&ppcounter),
                )
                .into()
            }
        }
        unsafe extern "system" fn CreateDeferredContext<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            contextflags: u32,
            ppdeferredcontext: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CreateDeferredContext(
                    this,
                    core::mem::transmute_copy(&contextflags),
                    core::mem::transmute_copy(&ppdeferredcontext),
                )
                .into()
            }
        }
        unsafe extern "system" fn OpenSharedResource<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            hresource: HANDLE,
            returnedinterface: *const windows_core::GUID,
            ppresource: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::OpenSharedResource(
                    this,
                    core::mem::transmute_copy(&hresource),
                    core::mem::transmute_copy(&returnedinterface),
                    core::mem::transmute_copy(&ppresource),
                )
                .into()
            }
        }
        unsafe extern "system" fn CheckFormatSupport<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            format: DXGI_FORMAT,
            pformatsupport: *mut u32,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match ID3D11Device_Impl::CheckFormatSupport(
                    this,
                    core::mem::transmute_copy(&format),
                ) {
                    Ok(ok__) => {
                        pformatsupport.write(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn CheckMultisampleQualityLevels<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            format: DXGI_FORMAT,
            samplecount: u32,
            pnumqualitylevels: *mut u32,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match ID3D11Device_Impl::CheckMultisampleQualityLevels(
                    this,
                    core::mem::transmute_copy(&format),
                    core::mem::transmute_copy(&samplecount),
                ) {
                    Ok(ok__) => {
                        pnumqualitylevels.write(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn CheckCounterInfo<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pcounterinfo: *mut D3D11_COUNTER_INFO,
        ) {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CheckCounterInfo(this, core::mem::transmute_copy(&pcounterinfo));
            }
        }
        unsafe extern "system" fn CheckCounter<Identity: ID3D11Device_Impl, const OFFSET: isize>(
            this: *mut core::ffi::c_void,
            pdesc: *const D3D11_COUNTER_DESC,
            ptype: *mut D3D11_COUNTER_TYPE,
            pactivecounters: *mut u32,
            szname: windows_core::PSTR,
            pnamelength: *mut u32,
            szunits: windows_core::PSTR,
            punitslength: *mut u32,
            szdescription: windows_core::PSTR,
            pdescriptionlength: *mut u32,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CheckCounter(
                    this,
                    core::mem::transmute_copy(&pdesc),
                    core::mem::transmute_copy(&ptype),
                    core::mem::transmute_copy(&pactivecounters),
                    core::mem::transmute_copy(&szname),
                    core::mem::transmute_copy(&pnamelength),
                    core::mem::transmute_copy(&szunits),
                    core::mem::transmute_copy(&punitslength),
                    core::mem::transmute_copy(&szdescription),
                    core::mem::transmute_copy(&pdescriptionlength),
                )
                .into()
            }
        }
        unsafe extern "system" fn CheckFeatureSupport<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            feature: D3D11_FEATURE,
            pfeaturesupportdata: *mut core::ffi::c_void,
            featuresupportdatasize: u32,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::CheckFeatureSupport(
                    this,
                    core::mem::transmute_copy(&feature),
                    core::mem::transmute_copy(&pfeaturesupportdata),
                    core::mem::transmute_copy(&featuresupportdatasize),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetPrivateData<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            guid: *const windows_core::GUID,
            pdatasize: *mut u32,
            pdata: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetPrivateData(
                    this,
                    core::mem::transmute_copy(&guid),
                    core::mem::transmute_copy(&pdatasize),
                    core::mem::transmute_copy(&pdata),
                )
                .into()
            }
        }
        unsafe extern "system" fn SetPrivateData<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            guid: *const windows_core::GUID,
            datasize: u32,
            pdata: *const core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::SetPrivateData(
                    this,
                    core::mem::transmute_copy(&guid),
                    core::mem::transmute_copy(&datasize),
                    core::mem::transmute_copy(&pdata),
                )
                .into()
            }
        }
        unsafe extern "system" fn SetPrivateDataInterface<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            guid: *const windows_core::GUID,
            pdata: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::SetPrivateDataInterface(
                    this,
                    core::mem::transmute_copy(&guid),
                    core::mem::transmute_copy(&pdata),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetFeatureLevel<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> D3D_FEATURE_LEVEL {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetFeatureLevel(this)
            }
        }
        unsafe extern "system" fn GetCreationFlags<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetCreationFlags(this)
            }
        }
        unsafe extern "system" fn GetDeviceRemovedReason<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetDeviceRemovedReason(this).into()
            }
        }
        unsafe extern "system" fn GetImmediateContext<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            ppimmediatecontext: *mut *mut core::ffi::c_void,
        ) {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetImmediateContext(
                    this,
                    core::mem::transmute_copy(&ppimmediatecontext),
                );
            }
        }
        unsafe extern "system" fn SetExceptionMode<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            raiseflags: u32,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::SetExceptionMode(this, core::mem::transmute_copy(&raiseflags))
                    .into()
            }
        }
        unsafe extern "system" fn GetExceptionMode<
            Identity: ID3D11Device_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11Device_Impl::GetExceptionMode(this)
            }
        }
        Self {
            base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
            CreateBuffer: CreateBuffer::<Identity, OFFSET>,
            CreateTexture1D: CreateTexture1D::<Identity, OFFSET>,
            CreateTexture2D: CreateTexture2D::<Identity, OFFSET>,
            CreateTexture3D: CreateTexture3D::<Identity, OFFSET>,
            CreateShaderResourceView: CreateShaderResourceView::<Identity, OFFSET>,
            CreateUnorderedAccessView: CreateUnorderedAccessView::<Identity, OFFSET>,
            CreateRenderTargetView: CreateRenderTargetView::<Identity, OFFSET>,
            CreateDepthStencilView: CreateDepthStencilView::<Identity, OFFSET>,
            CreateInputLayout: CreateInputLayout::<Identity, OFFSET>,
            CreateVertexShader: CreateVertexShader::<Identity, OFFSET>,
            CreateGeometryShader: CreateGeometryShader::<Identity, OFFSET>,
            CreateGeometryShaderWithStreamOutput: CreateGeometryShaderWithStreamOutput::<
                Identity,
                OFFSET,
            >,
            CreatePixelShader: CreatePixelShader::<Identity, OFFSET>,
            CreateHullShader: CreateHullShader::<Identity, OFFSET>,
            CreateDomainShader: CreateDomainShader::<Identity, OFFSET>,
            CreateComputeShader: CreateComputeShader::<Identity, OFFSET>,
            CreateClassLinkage: CreateClassLinkage::<Identity, OFFSET>,
            CreateBlendState: CreateBlendState::<Identity, OFFSET>,
            CreateDepthStencilState: CreateDepthStencilState::<Identity, OFFSET>,
            CreateRasterizerState: CreateRasterizerState::<Identity, OFFSET>,
            CreateSamplerState: CreateSamplerState::<Identity, OFFSET>,
            CreateQuery: CreateQuery::<Identity, OFFSET>,
            CreatePredicate: CreatePredicate::<Identity, OFFSET>,
            CreateCounter: CreateCounter::<Identity, OFFSET>,
            CreateDeferredContext: CreateDeferredContext::<Identity, OFFSET>,
            OpenSharedResource: OpenSharedResource::<Identity, OFFSET>,
            CheckFormatSupport: CheckFormatSupport::<Identity, OFFSET>,
            CheckMultisampleQualityLevels: CheckMultisampleQualityLevels::<Identity, OFFSET>,
            CheckCounterInfo: CheckCounterInfo::<Identity, OFFSET>,
            CheckCounter: CheckCounter::<Identity, OFFSET>,
            CheckFeatureSupport: CheckFeatureSupport::<Identity, OFFSET>,
            GetPrivateData: GetPrivateData::<Identity, OFFSET>,
            SetPrivateData: SetPrivateData::<Identity, OFFSET>,
            SetPrivateDataInterface: SetPrivateDataInterface::<Identity, OFFSET>,
            GetFeatureLevel: GetFeatureLevel::<Identity, OFFSET>,
            GetCreationFlags: GetCreationFlags::<Identity, OFFSET>,
            GetDeviceRemovedReason: GetDeviceRemovedReason::<Identity, OFFSET>,
            GetImmediateContext: GetImmediateContext::<Identity, OFFSET>,
            SetExceptionMode: SetExceptionMode::<Identity, OFFSET>,
            GetExceptionMode: GetExceptionMode::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<ID3D11Device as windows_core::Interface>::IID
    }
}
impl windows_core::RuntimeName for ID3D11Device {}
windows_core::imp::define_interface!(
    ID3D11DeviceChild,
    ID3D11DeviceChild_Vtbl,
    0x1841e5c8_16b0_489b_bcc8_44cfb0d5deae
);
windows_core::imp::interface_hierarchy!(ID3D11DeviceChild, windows_core::IUnknown);
#[repr(C)]
pub struct ID3D11DeviceChild_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    GetDevice: usize,
    GetPrivateData: usize,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
}
impl windows_core::RuntimeName for ID3D11DeviceChild {}
windows_core::imp::define_interface!(
    ID3D11DeviceContext,
    ID3D11DeviceContext_Vtbl,
    0xc0bfa96c_e089_44fb_8eaf_26f8796190da
);
impl core::ops::Deref for ID3D11DeviceContext {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11DeviceContext,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
impl ID3D11DeviceContext {
    pub unsafe fn VSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn PSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn PSSetShader<P0>(
        &self,
        ppixelshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11PixelShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).PSSetShader)(
                windows_core::Interface::as_raw(self),
                ppixelshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn PSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn VSSetShader<P0>(
        &self,
        pvertexshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11VertexShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).VSSetShader)(
                windows_core::Interface::as_raw(self),
                pvertexshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn DrawIndexed(
        &self,
        indexcount: u32,
        startindexlocation: u32,
        basevertexlocation: i32,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DrawIndexed)(
                windows_core::Interface::as_raw(self),
                indexcount,
                startindexlocation,
                basevertexlocation,
            );
        }
    }
    pub unsafe fn Draw(&self, vertexcount: u32, startvertexlocation: u32) {
        unsafe {
            (windows_core::Interface::vtable(self).Draw)(
                windows_core::Interface::as_raw(self),
                vertexcount,
                startvertexlocation,
            );
        }
    }
    pub unsafe fn Map<P0>(
        &self,
        presource: P0,
        subresource: u32,
        maptype: D3D11_MAP,
        mapflags: u32,
        pmappedresource: Option<*mut D3D11_MAPPED_SUBRESOURCE>,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Map)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                subresource,
                maptype,
                mapflags,
                pmappedresource.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn Unmap<P0>(&self, presource: P0, subresource: u32)
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Unmap)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                subresource,
            );
        }
    }
    pub unsafe fn PSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn IASetInputLayout<P0>(&self, pinputlayout: P0)
    where
        P0: windows_core::Param<ID3D11InputLayout>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).IASetInputLayout)(
                windows_core::Interface::as_raw(self),
                pinputlayout.param().abi(),
            );
        }
    }
    pub unsafe fn IASetVertexBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppvertexbuffers: Option<*const Option<ID3D11Buffer>>,
        pstrides: Option<*const u32>,
        poffsets: Option<*const u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).IASetVertexBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppvertexbuffers.unwrap_or(core::mem::zeroed()) as _,
                pstrides.unwrap_or(core::mem::zeroed()) as _,
                poffsets.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn IASetIndexBuffer<P0>(&self, pindexbuffer: P0, format: DXGI_FORMAT, offset: u32)
    where
        P0: windows_core::Param<ID3D11Buffer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).IASetIndexBuffer)(
                windows_core::Interface::as_raw(self),
                pindexbuffer.param().abi(),
                format,
                offset,
            );
        }
    }
    pub unsafe fn DrawIndexedInstanced(
        &self,
        indexcountperinstance: u32,
        instancecount: u32,
        startindexlocation: u32,
        basevertexlocation: i32,
        startinstancelocation: u32,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DrawIndexedInstanced)(
                windows_core::Interface::as_raw(self),
                indexcountperinstance,
                instancecount,
                startindexlocation,
                basevertexlocation,
                startinstancelocation,
            );
        }
    }
    pub unsafe fn DrawInstanced(
        &self,
        vertexcountperinstance: u32,
        instancecount: u32,
        startvertexlocation: u32,
        startinstancelocation: u32,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DrawInstanced)(
                windows_core::Interface::as_raw(self),
                vertexcountperinstance,
                instancecount,
                startvertexlocation,
                startinstancelocation,
            );
        }
    }
    pub unsafe fn GSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn GSSetShader<P0>(
        &self,
        pshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11GeometryShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GSSetShader)(
                windows_core::Interface::as_raw(self),
                pshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn IASetPrimitiveTopology(&self, topology: D3D11_PRIMITIVE_TOPOLOGY) {
        unsafe {
            (windows_core::Interface::vtable(self).IASetPrimitiveTopology)(
                windows_core::Interface::as_raw(self),
                topology,
            );
        }
    }
    pub unsafe fn VSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn VSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn Begin<P0>(&self, pasync: P0)
    where
        P0: windows_core::Param<ID3D11Asynchronous>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Begin)(
                windows_core::Interface::as_raw(self),
                pasync.param().abi(),
            );
        }
    }
    pub unsafe fn End<P0>(&self, pasync: P0)
    where
        P0: windows_core::Param<ID3D11Asynchronous>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).End)(
                windows_core::Interface::as_raw(self),
                pasync.param().abi(),
            );
        }
    }
    pub unsafe fn GetData<P0>(
        &self,
        pasync: P0,
        pdata: Option<*mut core::ffi::c_void>,
        datasize: u32,
        getdataflags: u32,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<ID3D11Asynchronous>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetData)(
                windows_core::Interface::as_raw(self),
                pasync.param().abi(),
                pdata.unwrap_or(core::mem::zeroed()) as _,
                datasize,
                getdataflags,
            )
        }
    }
    pub unsafe fn SetPredication<P0>(&self, ppredicate: P0, predicatevalue: bool)
    where
        P0: windows_core::Param<ID3D11Predicate>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetPredication)(
                windows_core::Interface::as_raw(self),
                ppredicate.param().abi(),
                predicatevalue.into(),
            );
        }
    }
    pub unsafe fn GSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn GSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn OMSetRenderTargets<P2>(
        &self,
        pprendertargetviews: Option<&[Option<ID3D11RenderTargetView>]>,
        pdepthstencilview: P2,
    ) where
        P2: windows_core::Param<ID3D11DepthStencilView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OMSetRenderTargets)(
                windows_core::Interface::as_raw(self),
                pprendertargetviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    pprendertargetviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                pdepthstencilview.param().abi(),
            );
        }
    }
    pub unsafe fn OMSetRenderTargetsAndUnorderedAccessViews<P2>(
        &self,
        pprendertargetviews: Option<&[Option<ID3D11RenderTargetView>]>,
        pdepthstencilview: P2,
        uavstartslot: u32,
        numuavs: u32,
        ppunorderedaccessviews: Option<*const Option<ID3D11UnorderedAccessView>>,
        puavinitialcounts: Option<*const u32>,
    ) where
        P2: windows_core::Param<ID3D11DepthStencilView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OMSetRenderTargetsAndUnorderedAccessViews)(
                windows_core::Interface::as_raw(self),
                pprendertargetviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    pprendertargetviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                pdepthstencilview.param().abi(),
                uavstartslot,
                numuavs,
                ppunorderedaccessviews.unwrap_or(core::mem::zeroed()) as _,
                puavinitialcounts.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn OMSetBlendState<P0>(
        &self,
        pblendstate: P0,
        blendfactor: Option<&[f32; 4]>,
        samplemask: u32,
    ) where
        P0: windows_core::Param<ID3D11BlendState>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OMSetBlendState)(
                windows_core::Interface::as_raw(self),
                pblendstate.param().abi(),
                blendfactor.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                samplemask,
            );
        }
    }
    pub unsafe fn OMSetDepthStencilState<P0>(&self, pdepthstencilstate: P0, stencilref: u32)
    where
        P0: windows_core::Param<ID3D11DepthStencilState>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).OMSetDepthStencilState)(
                windows_core::Interface::as_raw(self),
                pdepthstencilstate.param().abi(),
                stencilref,
            );
        }
    }
    pub unsafe fn SOSetTargets(
        &self,
        numbuffers: u32,
        ppsotargets: Option<*const Option<ID3D11Buffer>>,
        poffsets: Option<*const u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).SOSetTargets)(
                windows_core::Interface::as_raw(self),
                numbuffers,
                ppsotargets.unwrap_or(core::mem::zeroed()) as _,
                poffsets.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn DrawAuto(&self) {
        unsafe {
            (windows_core::Interface::vtable(self).DrawAuto)(windows_core::Interface::as_raw(self));
        }
    }
    pub unsafe fn DrawIndexedInstancedIndirect<P0>(
        &self,
        pbufferforargs: P0,
        alignedbyteoffsetforargs: u32,
    ) where
        P0: windows_core::Param<ID3D11Buffer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).DrawIndexedInstancedIndirect)(
                windows_core::Interface::as_raw(self),
                pbufferforargs.param().abi(),
                alignedbyteoffsetforargs,
            );
        }
    }
    pub unsafe fn DrawInstancedIndirect<P0>(
        &self,
        pbufferforargs: P0,
        alignedbyteoffsetforargs: u32,
    ) where
        P0: windows_core::Param<ID3D11Buffer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).DrawInstancedIndirect)(
                windows_core::Interface::as_raw(self),
                pbufferforargs.param().abi(),
                alignedbyteoffsetforargs,
            );
        }
    }
    pub unsafe fn Dispatch(
        &self,
        threadgroupcountx: u32,
        threadgroupcounty: u32,
        threadgroupcountz: u32,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).Dispatch)(
                windows_core::Interface::as_raw(self),
                threadgroupcountx,
                threadgroupcounty,
                threadgroupcountz,
            );
        }
    }
    pub unsafe fn DispatchIndirect<P0>(&self, pbufferforargs: P0, alignedbyteoffsetforargs: u32)
    where
        P0: windows_core::Param<ID3D11Buffer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).DispatchIndirect)(
                windows_core::Interface::as_raw(self),
                pbufferforargs.param().abi(),
                alignedbyteoffsetforargs,
            );
        }
    }
    pub unsafe fn RSSetState<P0>(&self, prasterizerstate: P0)
    where
        P0: windows_core::Param<ID3D11RasterizerState>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).RSSetState)(
                windows_core::Interface::as_raw(self),
                prasterizerstate.param().abi(),
            );
        }
    }
    pub unsafe fn RSSetViewports(&self, pviewports: Option<&[D3D11_VIEWPORT]>) {
        unsafe {
            (windows_core::Interface::vtable(self).RSSetViewports)(
                windows_core::Interface::as_raw(self),
                pviewports.map_or(0, |slice| slice.len().try_into().unwrap()),
                pviewports.map_or(core::ptr::null(), |slice| slice.as_ptr()),
            );
        }
    }
    pub unsafe fn RSSetScissorRects(&self, prects: Option<&[D3D11_RECT]>) {
        unsafe {
            (windows_core::Interface::vtable(self).RSSetScissorRects)(
                windows_core::Interface::as_raw(self),
                prects.map_or(0, |slice| slice.len().try_into().unwrap()),
                prects.map_or(core::ptr::null(), |slice| slice.as_ptr()),
            );
        }
    }
    pub unsafe fn CopySubresourceRegion<P0, P5>(
        &self,
        pdstresource: P0,
        dstsubresource: u32,
        dstx: u32,
        dsty: u32,
        dstz: u32,
        psrcresource: P5,
        srcsubresource: u32,
        psrcbox: Option<*const D3D11_BOX>,
    ) where
        P0: windows_core::Param<ID3D11Resource>,
        P5: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CopySubresourceRegion)(
                windows_core::Interface::as_raw(self),
                pdstresource.param().abi(),
                dstsubresource,
                dstx,
                dsty,
                dstz,
                psrcresource.param().abi(),
                srcsubresource,
                psrcbox.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CopyResource<P0, P1>(&self, pdstresource: P0, psrcresource: P1)
    where
        P0: windows_core::Param<ID3D11Resource>,
        P1: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CopyResource)(
                windows_core::Interface::as_raw(self),
                pdstresource.param().abi(),
                psrcresource.param().abi(),
            );
        }
    }
    pub unsafe fn UpdateSubresource<P0>(
        &self,
        pdstresource: P0,
        dstsubresource: u32,
        pdstbox: Option<*const D3D11_BOX>,
        psrcdata: *const core::ffi::c_void,
        srcrowpitch: u32,
        srcdepthpitch: u32,
    ) where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).UpdateSubresource)(
                windows_core::Interface::as_raw(self),
                pdstresource.param().abi(),
                dstsubresource,
                pdstbox.unwrap_or(core::mem::zeroed()) as _,
                psrcdata,
                srcrowpitch,
                srcdepthpitch,
            );
        }
    }
    pub unsafe fn CopyStructureCount<P0, P2>(
        &self,
        pdstbuffer: P0,
        dstalignedbyteoffset: u32,
        psrcview: P2,
    ) where
        P0: windows_core::Param<ID3D11Buffer>,
        P2: windows_core::Param<ID3D11UnorderedAccessView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CopyStructureCount)(
                windows_core::Interface::as_raw(self),
                pdstbuffer.param().abi(),
                dstalignedbyteoffset,
                psrcview.param().abi(),
            );
        }
    }
    pub unsafe fn ClearRenderTargetView<P0>(&self, prendertargetview: P0, colorrgba: &[f32; 4])
    where
        P0: windows_core::Param<ID3D11RenderTargetView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ClearRenderTargetView)(
                windows_core::Interface::as_raw(self),
                prendertargetview.param().abi(),
                colorrgba.as_ptr(),
            );
        }
    }
    pub unsafe fn ClearUnorderedAccessViewUint<P0>(
        &self,
        punorderedaccessview: P0,
        values: &[u32; 4],
    ) where
        P0: windows_core::Param<ID3D11UnorderedAccessView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ClearUnorderedAccessViewUint)(
                windows_core::Interface::as_raw(self),
                punorderedaccessview.param().abi(),
                values.as_ptr(),
            );
        }
    }
    pub unsafe fn ClearUnorderedAccessViewFloat<P0>(
        &self,
        punorderedaccessview: P0,
        values: &[f32; 4],
    ) where
        P0: windows_core::Param<ID3D11UnorderedAccessView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ClearUnorderedAccessViewFloat)(
                windows_core::Interface::as_raw(self),
                punorderedaccessview.param().abi(),
                values.as_ptr(),
            );
        }
    }
    pub unsafe fn ClearDepthStencilView<P0>(
        &self,
        pdepthstencilview: P0,
        clearflags: u32,
        depth: f32,
        stencil: u8,
    ) where
        P0: windows_core::Param<ID3D11DepthStencilView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ClearDepthStencilView)(
                windows_core::Interface::as_raw(self),
                pdepthstencilview.param().abi(),
                clearflags,
                depth,
                stencil,
            );
        }
    }
    pub unsafe fn GenerateMips<P0>(&self, pshaderresourceview: P0)
    where
        P0: windows_core::Param<ID3D11ShaderResourceView>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GenerateMips)(
                windows_core::Interface::as_raw(self),
                pshaderresourceview.param().abi(),
            );
        }
    }
    pub unsafe fn SetResourceMinLOD<P0>(&self, presource: P0, minlod: f32)
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetResourceMinLOD)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
                minlod,
            );
        }
    }
    pub unsafe fn GetResourceMinLOD<P0>(&self, presource: P0) -> f32
    where
        P0: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetResourceMinLOD)(
                windows_core::Interface::as_raw(self),
                presource.param().abi(),
            )
        }
    }
    pub unsafe fn ResolveSubresource<P0, P2>(
        &self,
        pdstresource: P0,
        dstsubresource: u32,
        psrcresource: P2,
        srcsubresource: u32,
        format: DXGI_FORMAT,
    ) where
        P0: windows_core::Param<ID3D11Resource>,
        P2: windows_core::Param<ID3D11Resource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ResolveSubresource)(
                windows_core::Interface::as_raw(self),
                pdstresource.param().abi(),
                dstsubresource,
                psrcresource.param().abi(),
                srcsubresource,
                format,
            );
        }
    }
    pub unsafe fn ExecuteCommandList<P0>(&self, pcommandlist: P0, restorecontextstate: bool)
    where
        P0: windows_core::Param<ID3D11CommandList>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ExecuteCommandList)(
                windows_core::Interface::as_raw(self),
                pcommandlist.param().abi(),
                restorecontextstate.into(),
            );
        }
    }
    pub unsafe fn HSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn HSSetShader<P0>(
        &self,
        phullshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11HullShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).HSSetShader)(
                windows_core::Interface::as_raw(self),
                phullshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn HSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn HSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn DSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn DSSetShader<P0>(
        &self,
        pdomainshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11DomainShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).DSSetShader)(
                windows_core::Interface::as_raw(self),
                pdomainshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn DSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn DSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn CSSetShaderResources(
        &self,
        startslot: u32,
        ppshaderresourceviews: Option<&[Option<ID3D11ShaderResourceView>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSSetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppshaderresourceviews.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppshaderresourceviews.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn CSSetUnorderedAccessViews(
        &self,
        startslot: u32,
        numuavs: u32,
        ppunorderedaccessviews: Option<*const Option<ID3D11UnorderedAccessView>>,
        puavinitialcounts: Option<*const u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSSetUnorderedAccessViews)(
                windows_core::Interface::as_raw(self),
                startslot,
                numuavs,
                ppunorderedaccessviews.unwrap_or(core::mem::zeroed()) as _,
                puavinitialcounts.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSSetShader<P0>(
        &self,
        pcomputeshader: P0,
        ppclassinstances: Option<&[Option<ID3D11ClassInstance>]>,
    ) where
        P0: windows_core::Param<ID3D11ComputeShader>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CSSetShader)(
                windows_core::Interface::as_raw(self),
                pcomputeshader.param().abi(),
                core::mem::transmute(
                    ppclassinstances.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
                ppclassinstances.map_or(0, |slice| slice.len().try_into().unwrap()),
            );
        }
    }
    pub unsafe fn CSSetSamplers(
        &self,
        startslot: u32,
        ppsamplers: Option<&[Option<ID3D11SamplerState>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSSetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppsamplers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(ppsamplers.map_or(core::ptr::null(), |slice| slice.as_ptr())),
            );
        }
    }
    pub unsafe fn CSSetConstantBuffers(
        &self,
        startslot: u32,
        ppconstantbuffers: Option<&[Option<ID3D11Buffer>]>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSSetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                ppconstantbuffers.map_or(0, |slice| slice.len().try_into().unwrap()),
                core::mem::transmute(
                    ppconstantbuffers.map_or(core::ptr::null(), |slice| slice.as_ptr()),
                ),
            );
        }
    }
    pub unsafe fn VSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn PSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn PSGetShader(
        &self,
        pppixelshader: *mut Option<ID3D11PixelShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pppixelshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn PSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn VSGetShader(
        &self,
        ppvertexshader: *mut Option<ID3D11VertexShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppvertexshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn PSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).PSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn IAGetInputLayout(&self) -> windows_core::Result<ID3D11InputLayout> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IAGetInputLayout)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            windows_core::imp::Type::from_abi(result__)
        }
    }
    pub unsafe fn IAGetVertexBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppvertexbuffers: Option<*mut Option<ID3D11Buffer>>,
        pstrides: Option<*mut u32>,
        poffsets: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).IAGetVertexBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppvertexbuffers.unwrap_or(core::mem::zeroed()) as _,
                pstrides.unwrap_or(core::mem::zeroed()) as _,
                poffsets.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn IAGetIndexBuffer(
        &self,
        pindexbuffer: *mut Option<ID3D11Buffer>,
        format: Option<*mut DXGI_FORMAT>,
        offset: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).IAGetIndexBuffer)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pindexbuffer),
                format.unwrap_or(core::mem::zeroed()) as _,
                offset.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn GSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn GSGetShader(
        &self,
        ppgeometryshader: *mut Option<ID3D11GeometryShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppgeometryshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn IAGetPrimitiveTopology(&self) -> D3D11_PRIMITIVE_TOPOLOGY {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IAGetPrimitiveTopology)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            result__
        }
    }
    pub unsafe fn VSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn VSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).VSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn GetPredication(
        &self,
        pppredicate: *mut Option<ID3D11Predicate>,
        ppredicatevalue: Option<*mut windows_core::BOOL>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GetPredication)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pppredicate),
                ppredicatevalue.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn GSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn GSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).GSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn OMGetRenderTargets(
        &self,
        numviews: u32,
        pprendertargetviews: Option<*mut Option<ID3D11RenderTargetView>>,
        ppdepthstencilview: *mut Option<ID3D11DepthStencilView>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).OMGetRenderTargets)(
                windows_core::Interface::as_raw(self),
                numviews,
                pprendertargetviews.unwrap_or(core::mem::zeroed()) as _,
                core::mem::transmute(ppdepthstencilview),
            );
        }
    }
    pub unsafe fn OMGetRenderTargetsAndUnorderedAccessViews(
        &self,
        numrtvs: u32,
        pprendertargetviews: Option<*mut Option<ID3D11RenderTargetView>>,
        ppdepthstencilview: *mut Option<ID3D11DepthStencilView>,
        uavstartslot: u32,
        numuavs: u32,
        ppunorderedaccessviews: Option<*mut Option<ID3D11UnorderedAccessView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).OMGetRenderTargetsAndUnorderedAccessViews)(
                windows_core::Interface::as_raw(self),
                numrtvs,
                pprendertargetviews.unwrap_or(core::mem::zeroed()) as _,
                core::mem::transmute(ppdepthstencilview),
                uavstartslot,
                numuavs,
                ppunorderedaccessviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn OMGetBlendState(
        &self,
        ppblendstate: *mut Option<ID3D11BlendState>,
        blendfactor: Option<*mut f32>,
        psamplemask: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).OMGetBlendState)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppblendstate),
                blendfactor.unwrap_or(core::mem::zeroed()) as _,
                psamplemask.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn OMGetDepthStencilState(
        &self,
        ppdepthstencilstate: *mut Option<ID3D11DepthStencilState>,
        pstencilref: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).OMGetDepthStencilState)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppdepthstencilstate),
                pstencilref.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn SOGetTargets(
        &self,
        numbuffers: u32,
        ppsotargets: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).SOGetTargets)(
                windows_core::Interface::as_raw(self),
                numbuffers,
                ppsotargets.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn RSGetState(&self) -> windows_core::Result<ID3D11RasterizerState> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RSGetState)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            windows_core::imp::Type::from_abi(result__)
        }
    }
    pub unsafe fn RSGetViewports(
        &self,
        pnumviewports: *mut u32,
        pviewports: Option<*mut D3D11_VIEWPORT>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).RSGetViewports)(
                windows_core::Interface::as_raw(self),
                pnumviewports as _,
                pviewports.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn RSGetScissorRects(&self, pnumrects: *mut u32, prects: Option<*mut D3D11_RECT>) {
        unsafe {
            (windows_core::Interface::vtable(self).RSGetScissorRects)(
                windows_core::Interface::as_raw(self),
                pnumrects as _,
                prects.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn HSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn HSGetShader(
        &self,
        pphullshader: *mut Option<ID3D11HullShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(pphullshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn HSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn HSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).HSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn DSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn DSGetShader(
        &self,
        ppdomainshader: *mut Option<ID3D11DomainShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppdomainshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn DSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn DSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).DSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSGetShaderResources(
        &self,
        startslot: u32,
        numviews: u32,
        ppshaderresourceviews: Option<*mut Option<ID3D11ShaderResourceView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSGetShaderResources)(
                windows_core::Interface::as_raw(self),
                startslot,
                numviews,
                ppshaderresourceviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSGetUnorderedAccessViews(
        &self,
        startslot: u32,
        numuavs: u32,
        ppunorderedaccessviews: Option<*mut Option<ID3D11UnorderedAccessView>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSGetUnorderedAccessViews)(
                windows_core::Interface::as_raw(self),
                startslot,
                numuavs,
                ppunorderedaccessviews.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSGetShader(
        &self,
        ppcomputeshader: *mut Option<ID3D11ComputeShader>,
        ppclassinstances: Option<*mut Option<ID3D11ClassInstance>>,
        pnumclassinstances: Option<*mut u32>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSGetShader)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppcomputeshader),
                ppclassinstances.unwrap_or(core::mem::zeroed()) as _,
                pnumclassinstances.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSGetSamplers(
        &self,
        startslot: u32,
        numsamplers: u32,
        ppsamplers: Option<*mut Option<ID3D11SamplerState>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSGetSamplers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numsamplers,
                ppsamplers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn CSGetConstantBuffers(
        &self,
        startslot: u32,
        numbuffers: u32,
        ppconstantbuffers: Option<*mut Option<ID3D11Buffer>>,
    ) {
        unsafe {
            (windows_core::Interface::vtable(self).CSGetConstantBuffers)(
                windows_core::Interface::as_raw(self),
                startslot,
                numbuffers,
                ppconstantbuffers.unwrap_or(core::mem::zeroed()) as _,
            );
        }
    }
    pub unsafe fn ClearState(&self) {
        unsafe {
            (windows_core::Interface::vtable(self).ClearState)(windows_core::Interface::as_raw(
                self,
            ));
        }
    }
    pub unsafe fn Flush(&self) {
        unsafe {
            (windows_core::Interface::vtable(self).Flush)(windows_core::Interface::as_raw(self));
        }
    }
    pub unsafe fn GetType(&self) -> D3D11_DEVICE_CONTEXT_TYPE {
        unsafe {
            (windows_core::Interface::vtable(self).GetType)(windows_core::Interface::as_raw(self))
        }
    }
    pub unsafe fn GetContextFlags(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetContextFlags)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn FinishCommandList(
        &self,
        restoredeferredcontextstate: bool,
        ppcommandlist: Option<*mut Option<ID3D11CommandList>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).FinishCommandList)(
                windows_core::Interface::as_raw(self),
                restoredeferredcontextstate.into(),
                ppcommandlist.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11DeviceContext_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub VSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub PSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub PSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub PSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub VSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub DrawIndexed: unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, i32),
    pub Draw: unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32),
    pub Map: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        D3D11_MAP,
        u32,
        *mut D3D11_MAPPED_SUBRESOURCE,
    ) -> windows_core::HRESULT,
    pub Unmap: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32),
    pub PSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub IASetInputLayout: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void),
    pub IASetVertexBuffers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        u32,
        *const *mut core::ffi::c_void,
        *const u32,
        *const u32,
    ),
    pub IASetIndexBuffer:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, DXGI_FORMAT, u32),
    pub DrawIndexedInstanced:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, u32, i32, u32),
    pub DrawInstanced: unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, u32, u32),
    pub GSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub GSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub IASetPrimitiveTopology:
        unsafe extern "system" fn(*mut core::ffi::c_void, D3D11_PRIMITIVE_TOPOLOGY),
    pub VSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub VSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub Begin: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void),
    pub End: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void),
    pub GetData: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        u32,
    ) -> windows_core::HRESULT,
    pub SetPredication: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        windows_core::BOOL,
    ),
    pub GSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub GSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub OMSetRenderTargets: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *const *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ),
    pub OMSetRenderTargetsAndUnorderedAccessViews: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *const *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        u32,
        *const *mut core::ffi::c_void,
        *const u32,
    ),
    pub OMSetBlendState:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, *const f32, u32),
    pub OMSetDepthStencilState:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32),
    pub SOSetTargets: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *const *mut core::ffi::c_void,
        *const u32,
    ),
    pub DrawAuto: unsafe extern "system" fn(*mut core::ffi::c_void),
    pub DrawIndexedInstancedIndirect:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32),
    pub DrawInstancedIndirect:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32),
    pub Dispatch: unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, u32),
    pub DispatchIndirect:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32),
    pub RSSetState: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void),
    pub RSSetViewports:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, *const D3D11_VIEWPORT),
    pub RSSetScissorRects:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, *const D3D11_RECT),
    pub CopySubresourceRegion: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        u32,
        u32,
        u32,
        *mut core::ffi::c_void,
        u32,
        *const D3D11_BOX,
    ),
    pub CopyResource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ),
    pub UpdateSubresource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        *const D3D11_BOX,
        *const core::ffi::c_void,
        u32,
        u32,
    ),
    pub CopyStructureCount: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        *mut core::ffi::c_void,
    ),
    pub ClearRenderTargetView:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, *const f32),
    pub ClearUnorderedAccessViewUint:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, *const u32),
    pub ClearUnorderedAccessViewFloat:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, *const f32),
    pub ClearDepthStencilView:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, u32, f32, u8),
    pub GenerateMips: unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void),
    pub SetResourceMinLOD:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void, f32),
    pub GetResourceMinLOD:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut core::ffi::c_void) -> f32,
    pub ResolveSubresource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        u32,
        *mut core::ffi::c_void,
        u32,
        DXGI_FORMAT,
    ),
    pub ExecuteCommandList: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        windows_core::BOOL,
    ),
    pub HSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub HSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub HSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub HSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub DSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub DSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub DSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub DSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub CSSetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub CSSetUnorderedAccessViews: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        u32,
        *const *mut core::ffi::c_void,
        *const u32,
    ),
    pub CSSetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        u32,
    ),
    pub CSSetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub CSSetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *const *mut core::ffi::c_void),
    pub VSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub PSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub PSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub PSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub VSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub PSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub IAGetInputLayout:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void),
    pub IAGetVertexBuffers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        u32,
        *mut *mut core::ffi::c_void,
        *mut u32,
        *mut u32,
    ),
    pub IAGetIndexBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut DXGI_FORMAT,
        *mut u32,
    ),
    pub GSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub GSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub IAGetPrimitiveTopology:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_PRIMITIVE_TOPOLOGY),
    pub VSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub VSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub GetPredication: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut windows_core::BOOL,
    ),
    pub GSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub GSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub OMGetRenderTargets: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ),
    pub OMGetRenderTargetsAndUnorderedAccessViews: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        u32,
        u32,
        *mut *mut core::ffi::c_void,
    ),
    pub OMGetBlendState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut f32,
        *mut u32,
    ),
    pub OMGetDepthStencilState:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void, *mut u32),
    pub SOGetTargets:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, *mut *mut core::ffi::c_void),
    pub RSGetState: unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void),
    pub RSGetViewports:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32, *mut D3D11_VIEWPORT),
    pub RSGetScissorRects:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32, *mut D3D11_RECT),
    pub HSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub HSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub HSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub HSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub DSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub DSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub DSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub DSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub CSGetShaderResources:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub CSGetUnorderedAccessViews:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub CSGetShader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut u32,
    ),
    pub CSGetSamplers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub CSGetConstantBuffers:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32, *mut *mut core::ffi::c_void),
    pub ClearState: unsafe extern "system" fn(*mut core::ffi::c_void),
    pub Flush: unsafe extern "system" fn(*mut core::ffi::c_void),
    pub GetType: unsafe extern "system" fn(*mut core::ffi::c_void) -> D3D11_DEVICE_CONTEXT_TYPE,
    pub GetContextFlags: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub FinishCommandList: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::BOOL,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
impl windows_core::RuntimeName for ID3D11DeviceContext {}
windows_core::imp::define_interface!(
    ID3D11DomainShader,
    ID3D11DomainShader_Vtbl,
    0xf582c508_0f36_490c_9977_31eece268cfa
);
impl core::ops::Deref for ID3D11DomainShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11DomainShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11DomainShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11DomainShader {}
windows_core::imp::define_interface!(
    ID3D11GeometryShader,
    ID3D11GeometryShader_Vtbl,
    0x38325b96_effb_4022_ba02_2e795b70275c
);
impl core::ops::Deref for ID3D11GeometryShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11GeometryShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11GeometryShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11GeometryShader {}
windows_core::imp::define_interface!(
    ID3D11HullShader,
    ID3D11HullShader_Vtbl,
    0x8e5c6061_628a_4c8e_8264_bbe45cb3d5dd
);
impl core::ops::Deref for ID3D11HullShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11HullShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11HullShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11HullShader {}
windows_core::imp::define_interface!(
    ID3D11InputLayout,
    ID3D11InputLayout_Vtbl,
    0xe4819ddc_4cf0_4025_bd26_5de82a3e07b7
);
impl core::ops::Deref for ID3D11InputLayout {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11InputLayout,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11InputLayout_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11InputLayout {}
windows_core::imp::define_interface!(
    ID3D11PixelShader,
    ID3D11PixelShader_Vtbl,
    0xea82e40d_51dc_4f33_93d4_db7c9125ae8c
);
impl core::ops::Deref for ID3D11PixelShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11PixelShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11PixelShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11PixelShader {}
windows_core::imp::define_interface!(
    ID3D11Predicate,
    ID3D11Predicate_Vtbl,
    0x9eb576dd_9f77_4d86_81aa_8bab5fe490e2
);
impl core::ops::Deref for ID3D11Predicate {
    type Target = ID3D11Query;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Predicate,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Asynchronous,
    ID3D11Query
);
#[repr(C)]
pub struct ID3D11Predicate_Vtbl {
    pub base__: ID3D11Query_Vtbl,
}
impl windows_core::RuntimeName for ID3D11Predicate {}
windows_core::imp::define_interface!(
    ID3D11Query,
    ID3D11Query_Vtbl,
    0xd6c00747_87b7_425e_b84d_44d108560afd
);
impl core::ops::Deref for ID3D11Query {
    type Target = ID3D11Asynchronous;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Query,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Asynchronous
);
impl ID3D11Query {
    pub unsafe fn GetDesc(&self) -> D3D11_QUERY_DESC {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            result__
        }
    }
}
#[repr(C)]
pub struct ID3D11Query_Vtbl {
    pub base__: ID3D11Asynchronous_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_QUERY_DESC),
}
impl windows_core::RuntimeName for ID3D11Query {}
windows_core::imp::define_interface!(
    ID3D11RasterizerState,
    ID3D11RasterizerState_Vtbl,
    0x9bb4ab81_ab1a_4d8f_b506_fc04200b6ee7
);
impl core::ops::Deref for ID3D11RasterizerState {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11RasterizerState,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
impl ID3D11RasterizerState {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_RASTERIZER_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11RasterizerState_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_RASTERIZER_DESC),
}
impl windows_core::RuntimeName for ID3D11RasterizerState {}
windows_core::imp::define_interface!(
    ID3D11RenderTargetView,
    ID3D11RenderTargetView_Vtbl,
    0xdfdba067_0b8d_4865_875b_d7b4516cc164
);
impl core::ops::Deref for ID3D11RenderTargetView {
    type Target = ID3D11View;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11RenderTargetView,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11View
);
impl ID3D11RenderTargetView {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_RENDER_TARGET_VIEW_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11RenderTargetView_Vtbl {
    pub base__: ID3D11View_Vtbl,
    pub GetDesc:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_RENDER_TARGET_VIEW_DESC),
}
impl windows_core::RuntimeName for ID3D11RenderTargetView {}
windows_core::imp::define_interface!(
    ID3D11Resource,
    ID3D11Resource_Vtbl,
    0xdc8e63f3_d12b_4952_b47b_5e45026a862d
);
impl core::ops::Deref for ID3D11Resource {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(ID3D11Resource, windows_core::IUnknown, ID3D11DeviceChild);
impl ID3D11Resource {
    pub unsafe fn GetType(&self) -> D3D11_RESOURCE_DIMENSION {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetType)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            );
            result__
        }
    }
    pub unsafe fn SetEvictionPriority(&self, evictionpriority: u32) {
        unsafe {
            (windows_core::Interface::vtable(self).SetEvictionPriority)(
                windows_core::Interface::as_raw(self),
                evictionpriority,
            );
        }
    }
    pub unsafe fn GetEvictionPriority(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetEvictionPriority)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11Resource_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub GetType: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_RESOURCE_DIMENSION),
    pub SetEvictionPriority: unsafe extern "system" fn(*mut core::ffi::c_void, u32),
    pub GetEvictionPriority: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
}
impl windows_core::RuntimeName for ID3D11Resource {}
windows_core::imp::define_interface!(
    ID3D11SamplerState,
    ID3D11SamplerState_Vtbl,
    0xda6fea51_564c_4487_9810_f0d0f9b4e3a5
);
impl core::ops::Deref for ID3D11SamplerState {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11SamplerState,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
impl ID3D11SamplerState {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SAMPLER_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11SamplerState_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_SAMPLER_DESC),
}
impl windows_core::RuntimeName for ID3D11SamplerState {}
windows_core::imp::define_interface!(
    ID3D11ShaderReflection,
    ID3D11ShaderReflection_Vtbl,
    0x8d536ca1_0cca_4956_a837_786963755584
);
windows_core::imp::interface_hierarchy!(ID3D11ShaderReflection, windows_core::IUnknown);
impl ID3D11ShaderReflection {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SHADER_DESC) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetConstantBufferByIndex(
        &self,
        index: u32,
    ) -> Option<ID3D11ShaderReflectionConstantBuffer> {
        unsafe {
            (windows_core::Interface::vtable(self).GetConstantBufferByIndex)(
                windows_core::Interface::as_raw(self),
                index,
            )
        }
    }
    pub unsafe fn GetConstantBufferByName<P0>(
        &self,
        name: P0,
    ) -> Option<ID3D11ShaderReflectionConstantBuffer>
    where
        P0: windows_core::Param<windows_core::PCSTR>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetConstantBufferByName)(
                windows_core::Interface::as_raw(self),
                name.param().abi(),
            )
        }
    }
    pub unsafe fn GetResourceBindingDesc(
        &self,
        resourceindex: u32,
        pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetResourceBindingDesc)(
                windows_core::Interface::as_raw(self),
                resourceindex,
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetInputParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetInputParameterDesc)(
                windows_core::Interface::as_raw(self),
                parameterindex,
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetOutputParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetOutputParameterDesc)(
                windows_core::Interface::as_raw(self),
                parameterindex,
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetPatchConstantParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetPatchConstantParameterDesc)(
                windows_core::Interface::as_raw(self),
                parameterindex,
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetVariableByName<P0>(&self, name: P0) -> Option<ID3D11ShaderReflectionVariable>
    where
        P0: windows_core::Param<windows_core::PCSTR>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetVariableByName)(
                windows_core::Interface::as_raw(self),
                name.param().abi(),
            )
        }
    }
    pub unsafe fn GetResourceBindingDescByName<P0>(
        &self,
        name: P0,
        pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::HRESULT
    where
        P0: windows_core::Param<windows_core::PCSTR>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetResourceBindingDescByName)(
                windows_core::Interface::as_raw(self),
                name.param().abi(),
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetMovInstructionCount(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetMovInstructionCount)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetMovcInstructionCount(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetMovcInstructionCount)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetConversionInstructionCount(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetConversionInstructionCount)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetBitwiseInstructionCount(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetBitwiseInstructionCount)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetGSInputPrimitive(&self) -> D3D_PRIMITIVE {
        unsafe {
            (windows_core::Interface::vtable(self).GetGSInputPrimitive)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn IsSampleFrequencyShader(&self) -> windows_core::BOOL {
        unsafe {
            (windows_core::Interface::vtable(self).IsSampleFrequencyShader)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetNumInterfaceSlots(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetNumInterfaceSlots)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetMinFeatureLevel(&self) -> windows_core::Result<D3D_FEATURE_LEVEL> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetMinFeatureLevel)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub unsafe fn GetThreadGroupSize(
        &self,
        psizex: Option<*mut u32>,
        psizey: Option<*mut u32>,
        psizez: Option<*mut u32>,
    ) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetThreadGroupSize)(
                windows_core::Interface::as_raw(self),
                psizex.unwrap_or(core::mem::zeroed()) as _,
                psizey.unwrap_or(core::mem::zeroed()) as _,
                psizez.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn GetRequiresFlags(&self) -> u64 {
        unsafe {
            (windows_core::Interface::vtable(self).GetRequiresFlags)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11ShaderReflection_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub GetDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut D3D11_SHADER_DESC,
    ) -> windows_core::HRESULT,
    pub GetConstantBufferByIndex:
        unsafe extern "system" fn(
            *mut core::ffi::c_void,
            u32,
        ) -> Option<ID3D11ShaderReflectionConstantBuffer>,
    pub GetConstantBufferByName:
        unsafe extern "system" fn(
            *mut core::ffi::c_void,
            windows_core::PCSTR,
        ) -> Option<ID3D11ShaderReflectionConstantBuffer>,
    pub GetResourceBindingDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::HRESULT,
    pub GetInputParameterDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT,
    pub GetOutputParameterDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT,
    pub GetPatchConstantParameterDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::HRESULT,
    pub GetVariableByName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionVariable>,
    pub GetResourceBindingDescByName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::PCSTR,
        *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::HRESULT,
    pub GetMovInstructionCount: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetMovcInstructionCount: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetConversionInstructionCount: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetBitwiseInstructionCount: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetGSInputPrimitive: unsafe extern "system" fn(*mut core::ffi::c_void) -> D3D_PRIMITIVE,
    pub IsSampleFrequencyShader:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::BOOL,
    pub GetNumInterfaceSlots: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetMinFeatureLevel: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut D3D_FEATURE_LEVEL,
    ) -> windows_core::HRESULT,
    pub GetThreadGroupSize:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32, *mut u32, *mut u32) -> u32,
    pub GetRequiresFlags: unsafe extern "system" fn(*mut core::ffi::c_void) -> u64,
}
pub trait ID3D11ShaderReflection_Impl: windows_core::IUnknownImpl {
    fn GetDesc(&self, pdesc: *mut D3D11_SHADER_DESC) -> windows_core::Result<()>;
    fn GetConstantBufferByIndex(&self, index: u32) -> Option<ID3D11ShaderReflectionConstantBuffer>;
    fn GetConstantBufferByName(
        &self,
        name: &windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionConstantBuffer>;
    fn GetResourceBindingDesc(
        &self,
        resourceindex: u32,
        pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::Result<()>;
    fn GetInputParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::Result<()>;
    fn GetOutputParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::Result<()>;
    fn GetPatchConstantParameterDesc(
        &self,
        parameterindex: u32,
        pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
    ) -> windows_core::Result<()>;
    fn GetVariableByName(
        &self,
        name: &windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionVariable>;
    fn GetResourceBindingDescByName(
        &self,
        name: &windows_core::PCSTR,
        pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
    ) -> windows_core::Result<()>;
    fn GetMovInstructionCount(&self) -> u32;
    fn GetMovcInstructionCount(&self) -> u32;
    fn GetConversionInstructionCount(&self) -> u32;
    fn GetBitwiseInstructionCount(&self) -> u32;
    fn GetGSInputPrimitive(&self) -> D3D_PRIMITIVE;
    fn IsSampleFrequencyShader(&self) -> windows_core::BOOL;
    fn GetNumInterfaceSlots(&self) -> u32;
    fn GetMinFeatureLevel(&self) -> windows_core::Result<D3D_FEATURE_LEVEL>;
    fn GetThreadGroupSize(&self, psizex: *mut u32, psizey: *mut u32, psizez: *mut u32) -> u32;
    fn GetRequiresFlags(&self) -> u64;
}
impl ID3D11ShaderReflection_Vtbl {
    pub const fn new<Identity: ID3D11ShaderReflection_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn GetDesc<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            pdesc: *mut D3D11_SHADER_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetDesc(this, core::mem::transmute_copy(&pdesc)).into()
            }
        }
        unsafe extern "system" fn GetConstantBufferByIndex<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            index: u32,
        ) -> Option<ID3D11ShaderReflectionConstantBuffer> {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetConstantBufferByIndex(
                    this,
                    core::mem::transmute_copy(&index),
                )
            }
        }
        unsafe extern "system" fn GetConstantBufferByName<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            name: windows_core::PCSTR,
        ) -> Option<ID3D11ShaderReflectionConstantBuffer> {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetConstantBufferByName(
                    this,
                    core::mem::transmute(&name),
                )
            }
        }
        unsafe extern "system" fn GetResourceBindingDesc<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            resourceindex: u32,
            pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetResourceBindingDesc(
                    this,
                    core::mem::transmute_copy(&resourceindex),
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetInputParameterDesc<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            parameterindex: u32,
            pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetInputParameterDesc(
                    this,
                    core::mem::transmute_copy(&parameterindex),
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetOutputParameterDesc<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            parameterindex: u32,
            pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetOutputParameterDesc(
                    this,
                    core::mem::transmute_copy(&parameterindex),
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetPatchConstantParameterDesc<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            parameterindex: u32,
            pdesc: *mut D3D11_SIGNATURE_PARAMETER_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetPatchConstantParameterDesc(
                    this,
                    core::mem::transmute_copy(&parameterindex),
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetVariableByName<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            name: windows_core::PCSTR,
        ) -> Option<ID3D11ShaderReflectionVariable> {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetVariableByName(this, core::mem::transmute(&name))
            }
        }
        unsafe extern "system" fn GetResourceBindingDescByName<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            name: windows_core::PCSTR,
            pdesc: *mut D3D11_SHADER_INPUT_BIND_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetResourceBindingDescByName(
                    this,
                    core::mem::transmute(&name),
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetMovInstructionCount<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetMovInstructionCount(this)
            }
        }
        unsafe extern "system" fn GetMovcInstructionCount<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetMovcInstructionCount(this)
            }
        }
        unsafe extern "system" fn GetConversionInstructionCount<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetConversionInstructionCount(this)
            }
        }
        unsafe extern "system" fn GetBitwiseInstructionCount<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetBitwiseInstructionCount(this)
            }
        }
        unsafe extern "system" fn GetGSInputPrimitive<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> D3D_PRIMITIVE {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetGSInputPrimitive(this)
            }
        }
        unsafe extern "system" fn IsSampleFrequencyShader<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> windows_core::BOOL {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::IsSampleFrequencyShader(this)
            }
        }
        unsafe extern "system" fn GetNumInterfaceSlots<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetNumInterfaceSlots(this)
            }
        }
        unsafe extern "system" fn GetMinFeatureLevel<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            plevel: *mut D3D_FEATURE_LEVEL,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match ID3D11ShaderReflection_Impl::GetMinFeatureLevel(this) {
                    Ok(ok__) => {
                        plevel.write(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn GetThreadGroupSize<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            psizex: *mut u32,
            psizey: *mut u32,
            psizez: *mut u32,
        ) -> u32 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetThreadGroupSize(
                    this,
                    core::mem::transmute_copy(&psizex),
                    core::mem::transmute_copy(&psizey),
                    core::mem::transmute_copy(&psizez),
                )
            }
        }
        unsafe extern "system" fn GetRequiresFlags<
            Identity: ID3D11ShaderReflection_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
        ) -> u64 {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ID3D11ShaderReflection_Impl::GetRequiresFlags(this)
            }
        }
        Self {
            base__: windows_core::IUnknown_Vtbl::new::<Identity, OFFSET>(),
            GetDesc: GetDesc::<Identity, OFFSET>,
            GetConstantBufferByIndex: GetConstantBufferByIndex::<Identity, OFFSET>,
            GetConstantBufferByName: GetConstantBufferByName::<Identity, OFFSET>,
            GetResourceBindingDesc: GetResourceBindingDesc::<Identity, OFFSET>,
            GetInputParameterDesc: GetInputParameterDesc::<Identity, OFFSET>,
            GetOutputParameterDesc: GetOutputParameterDesc::<Identity, OFFSET>,
            GetPatchConstantParameterDesc: GetPatchConstantParameterDesc::<Identity, OFFSET>,
            GetVariableByName: GetVariableByName::<Identity, OFFSET>,
            GetResourceBindingDescByName: GetResourceBindingDescByName::<Identity, OFFSET>,
            GetMovInstructionCount: GetMovInstructionCount::<Identity, OFFSET>,
            GetMovcInstructionCount: GetMovcInstructionCount::<Identity, OFFSET>,
            GetConversionInstructionCount: GetConversionInstructionCount::<Identity, OFFSET>,
            GetBitwiseInstructionCount: GetBitwiseInstructionCount::<Identity, OFFSET>,
            GetGSInputPrimitive: GetGSInputPrimitive::<Identity, OFFSET>,
            IsSampleFrequencyShader: IsSampleFrequencyShader::<Identity, OFFSET>,
            GetNumInterfaceSlots: GetNumInterfaceSlots::<Identity, OFFSET>,
            GetMinFeatureLevel: GetMinFeatureLevel::<Identity, OFFSET>,
            GetThreadGroupSize: GetThreadGroupSize::<Identity, OFFSET>,
            GetRequiresFlags: GetRequiresFlags::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<ID3D11ShaderReflection as windows_core::Interface>::IID
    }
}
impl windows_core::RuntimeName for ID3D11ShaderReflection {}
windows_core::imp::define_interface!(
    ID3D11ShaderReflectionConstantBuffer,
    ID3D11ShaderReflectionConstantBuffer_Vtbl
);
impl ID3D11ShaderReflectionConstantBuffer {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SHADER_BUFFER_DESC) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetVariableByIndex(&self, index: u32) -> Option<ID3D11ShaderReflectionVariable> {
        unsafe {
            (windows_core::Interface::vtable(self).GetVariableByIndex)(
                windows_core::Interface::as_raw(self),
                index,
            )
        }
    }
    pub unsafe fn GetVariableByName<P0>(&self, name: P0) -> Option<ID3D11ShaderReflectionVariable>
    where
        P0: windows_core::Param<windows_core::PCSTR>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetVariableByName)(
                windows_core::Interface::as_raw(self),
                name.param().abi(),
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11ShaderReflectionConstantBuffer_Vtbl {
    pub GetDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut D3D11_SHADER_BUFFER_DESC,
    ) -> windows_core::HRESULT,
    pub GetVariableByIndex: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
    )
        -> Option<ID3D11ShaderReflectionVariable>,
    pub GetVariableByName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionVariable>,
}
pub trait ID3D11ShaderReflectionConstantBuffer_Impl {
    fn GetDesc(&self, pdesc: *mut D3D11_SHADER_BUFFER_DESC) -> windows_core::Result<()>;
    fn GetVariableByIndex(&self, index: u32) -> Option<ID3D11ShaderReflectionVariable>;
    fn GetVariableByName(
        &self,
        name: &windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionVariable>;
}
impl ID3D11ShaderReflectionConstantBuffer_Vtbl {
    pub const fn new<Identity: ID3D11ShaderReflectionConstantBuffer_Impl>() -> Self {
        unsafe extern "system" fn GetDesc<Identity: ID3D11ShaderReflectionConstantBuffer_Impl>(
            this: *mut core::ffi::c_void,
            pdesc: *mut D3D11_SHADER_BUFFER_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionConstantBuffer_Impl::GetDesc(
                    this,
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetVariableByIndex<
            Identity: ID3D11ShaderReflectionConstantBuffer_Impl,
        >(
            this: *mut core::ffi::c_void,
            index: u32,
        ) -> Option<ID3D11ShaderReflectionVariable> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionConstantBuffer_Impl::GetVariableByIndex(
                    this,
                    core::mem::transmute_copy(&index),
                )
            }
        }
        unsafe extern "system" fn GetVariableByName<
            Identity: ID3D11ShaderReflectionConstantBuffer_Impl,
        >(
            this: *mut core::ffi::c_void,
            name: windows_core::PCSTR,
        ) -> Option<ID3D11ShaderReflectionVariable> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionConstantBuffer_Impl::GetVariableByName(
                    this,
                    core::mem::transmute(&name),
                )
            }
        }
        Self {
            GetDesc: GetDesc::<Identity>,
            GetVariableByIndex: GetVariableByIndex::<Identity>,
            GetVariableByName: GetVariableByName::<Identity>,
        }
    }
}
struct ID3D11ShaderReflectionConstantBuffer_ImplVtbl<T: ID3D11ShaderReflectionConstantBuffer_Impl>(
    core::marker::PhantomData<T>,
);
impl<T: ID3D11ShaderReflectionConstantBuffer_Impl>
    ID3D11ShaderReflectionConstantBuffer_ImplVtbl<T>
{
    const VTABLE: ID3D11ShaderReflectionConstantBuffer_Vtbl =
        ID3D11ShaderReflectionConstantBuffer_Vtbl::new::<T>();
}
impl ID3D11ShaderReflectionConstantBuffer {
    pub fn new<'a, T: ID3D11ShaderReflectionConstantBuffer_Impl>(
        this: &'a T,
    ) -> windows_core::ScopedInterface<'a, Self> {
        let this = windows_core::ScopedHeap {
            vtable: &ID3D11ShaderReflectionConstantBuffer_ImplVtbl::<T>::VTABLE as *const _
                as *const _,
            this: this as *const _ as *const _,
        };
        let this = core::mem::ManuallyDrop::new(windows_core::imp::box_new(this));
        unsafe { windows_core::ScopedInterface::new(core::mem::transmute(&this.vtable)) }
    }
}
windows_core::imp::define_interface!(ID3D11ShaderReflectionType, ID3D11ShaderReflectionType_Vtbl);
impl ID3D11ShaderReflectionType {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SHADER_TYPE_DESC) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetMemberTypeByIndex(&self, index: u32) -> Option<Self> {
        unsafe {
            (windows_core::Interface::vtable(self).GetMemberTypeByIndex)(
                windows_core::Interface::as_raw(self),
                index,
            )
        }
    }
    pub unsafe fn GetMemberTypeByName<P0>(&self, name: P0) -> Option<Self>
    where
        P0: windows_core::Param<windows_core::PCSTR>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).GetMemberTypeByName)(
                windows_core::Interface::as_raw(self),
                name.param().abi(),
            )
        }
    }
    pub unsafe fn GetMemberTypeName(&self, index: u32) -> windows_core::PCSTR {
        unsafe {
            (windows_core::Interface::vtable(self).GetMemberTypeName)(
                windows_core::Interface::as_raw(self),
                index,
            )
        }
    }
    pub unsafe fn IsEqual<P0>(&self, ptype: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<Self>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).IsEqual)(
                windows_core::Interface::as_raw(self),
                ptype.param().abi(),
            )
        }
    }
    pub unsafe fn GetSubType(&self) -> Option<Self> {
        unsafe {
            (windows_core::Interface::vtable(self).GetSubType)(windows_core::Interface::as_raw(
                self,
            ))
        }
    }
    pub unsafe fn GetBaseClass(&self) -> Option<Self> {
        unsafe {
            (windows_core::Interface::vtable(self).GetBaseClass)(windows_core::Interface::as_raw(
                self,
            ))
        }
    }
    pub unsafe fn GetNumInterfaces(&self) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetNumInterfaces)(
                windows_core::Interface::as_raw(self),
            )
        }
    }
    pub unsafe fn GetInterfaceByIndex(&self, uindex: u32) -> Option<Self> {
        unsafe {
            (windows_core::Interface::vtable(self).GetInterfaceByIndex)(
                windows_core::Interface::as_raw(self),
                uindex,
            )
        }
    }
    pub unsafe fn IsOfType<P0>(&self, ptype: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<Self>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).IsOfType)(
                windows_core::Interface::as_raw(self),
                ptype.param().abi(),
            )
        }
    }
    pub unsafe fn ImplementsInterface<P0>(&self, pbase: P0) -> windows_core::HRESULT
    where
        P0: windows_core::Param<Self>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ImplementsInterface)(
                windows_core::Interface::as_raw(self),
                pbase.param().abi(),
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11ShaderReflectionType_Vtbl {
    pub GetDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut D3D11_SHADER_TYPE_DESC,
    ) -> windows_core::HRESULT,
    pub GetMemberTypeByIndex: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
    ) -> Option<ID3D11ShaderReflectionType>,
    pub GetMemberTypeByName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::PCSTR,
    ) -> Option<ID3D11ShaderReflectionType>,
    pub GetMemberTypeName:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> windows_core::PCSTR,
    pub IsEqual: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetSubType:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> Option<ID3D11ShaderReflectionType>,
    pub GetBaseClass:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> Option<ID3D11ShaderReflectionType>,
    pub GetNumInterfaces: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    pub GetInterfaceByIndex: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
    ) -> Option<ID3D11ShaderReflectionType>,
    pub IsOfType: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ImplementsInterface: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
pub trait ID3D11ShaderReflectionType_Impl {
    fn GetDesc(&self, pdesc: *mut D3D11_SHADER_TYPE_DESC) -> windows_core::Result<()>;
    fn GetMemberTypeByIndex(&self, index: u32) -> Option<ID3D11ShaderReflectionType>;
    fn GetMemberTypeByName(&self, name: &windows_core::PCSTR)
    -> Option<ID3D11ShaderReflectionType>;
    fn GetMemberTypeName(&self, index: u32) -> windows_core::PCSTR;
    fn IsEqual(
        &self,
        ptype: windows_core::Ref<ID3D11ShaderReflectionType>,
    ) -> windows_core::Result<()>;
    fn GetSubType(&self) -> Option<ID3D11ShaderReflectionType>;
    fn GetBaseClass(&self) -> Option<ID3D11ShaderReflectionType>;
    fn GetNumInterfaces(&self) -> u32;
    fn GetInterfaceByIndex(&self, uindex: u32) -> Option<ID3D11ShaderReflectionType>;
    fn IsOfType(
        &self,
        ptype: windows_core::Ref<ID3D11ShaderReflectionType>,
    ) -> windows_core::Result<()>;
    fn ImplementsInterface(
        &self,
        pbase: windows_core::Ref<ID3D11ShaderReflectionType>,
    ) -> windows_core::Result<()>;
}
impl ID3D11ShaderReflectionType_Vtbl {
    pub const fn new<Identity: ID3D11ShaderReflectionType_Impl>() -> Self {
        unsafe extern "system" fn GetDesc<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            pdesc: *mut D3D11_SHADER_TYPE_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetDesc(this, core::mem::transmute_copy(&pdesc))
                    .into()
            }
        }
        unsafe extern "system" fn GetMemberTypeByIndex<
            Identity: ID3D11ShaderReflectionType_Impl,
        >(
            this: *mut core::ffi::c_void,
            index: u32,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetMemberTypeByIndex(
                    this,
                    core::mem::transmute_copy(&index),
                )
            }
        }
        unsafe extern "system" fn GetMemberTypeByName<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            name: windows_core::PCSTR,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetMemberTypeByName(
                    this,
                    core::mem::transmute(&name),
                )
            }
        }
        unsafe extern "system" fn GetMemberTypeName<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            index: u32,
        ) -> windows_core::PCSTR {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetMemberTypeName(
                    this,
                    core::mem::transmute_copy(&index),
                )
            }
        }
        unsafe extern "system" fn IsEqual<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            ptype: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::IsEqual(this, core::mem::transmute_copy(&ptype))
                    .into()
            }
        }
        unsafe extern "system" fn GetSubType<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetSubType(this)
            }
        }
        unsafe extern "system" fn GetBaseClass<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetBaseClass(this)
            }
        }
        unsafe extern "system" fn GetNumInterfaces<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
        ) -> u32 {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetNumInterfaces(this)
            }
        }
        unsafe extern "system" fn GetInterfaceByIndex<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            uindex: u32,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::GetInterfaceByIndex(
                    this,
                    core::mem::transmute_copy(&uindex),
                )
            }
        }
        unsafe extern "system" fn IsOfType<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            ptype: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::IsOfType(this, core::mem::transmute_copy(&ptype))
                    .into()
            }
        }
        unsafe extern "system" fn ImplementsInterface<Identity: ID3D11ShaderReflectionType_Impl>(
            this: *mut core::ffi::c_void,
            pbase: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionType_Impl::ImplementsInterface(
                    this,
                    core::mem::transmute_copy(&pbase),
                )
                .into()
            }
        }
        Self {
            GetDesc: GetDesc::<Identity>,
            GetMemberTypeByIndex: GetMemberTypeByIndex::<Identity>,
            GetMemberTypeByName: GetMemberTypeByName::<Identity>,
            GetMemberTypeName: GetMemberTypeName::<Identity>,
            IsEqual: IsEqual::<Identity>,
            GetSubType: GetSubType::<Identity>,
            GetBaseClass: GetBaseClass::<Identity>,
            GetNumInterfaces: GetNumInterfaces::<Identity>,
            GetInterfaceByIndex: GetInterfaceByIndex::<Identity>,
            IsOfType: IsOfType::<Identity>,
            ImplementsInterface: ImplementsInterface::<Identity>,
        }
    }
}
struct ID3D11ShaderReflectionType_ImplVtbl<T: ID3D11ShaderReflectionType_Impl>(
    core::marker::PhantomData<T>,
);
impl<T: ID3D11ShaderReflectionType_Impl> ID3D11ShaderReflectionType_ImplVtbl<T> {
    const VTABLE: ID3D11ShaderReflectionType_Vtbl = ID3D11ShaderReflectionType_Vtbl::new::<T>();
}
impl ID3D11ShaderReflectionType {
    pub fn new<'a, T: ID3D11ShaderReflectionType_Impl>(
        this: &'a T,
    ) -> windows_core::ScopedInterface<'a, Self> {
        let this = windows_core::ScopedHeap {
            vtable: &ID3D11ShaderReflectionType_ImplVtbl::<T>::VTABLE as *const _ as *const _,
            this: this as *const _ as *const _,
        };
        let this = core::mem::ManuallyDrop::new(windows_core::imp::box_new(this));
        unsafe { windows_core::ScopedInterface::new(core::mem::transmute(&this.vtable)) }
    }
}
windows_core::imp::define_interface!(
    ID3D11ShaderReflectionVariable,
    ID3D11ShaderReflectionVariable_Vtbl
);
impl ID3D11ShaderReflectionVariable {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SHADER_VARIABLE_DESC) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
    pub unsafe fn GetType(&self) -> Option<ID3D11ShaderReflectionType> {
        unsafe {
            (windows_core::Interface::vtable(self).GetType)(windows_core::Interface::as_raw(self))
        }
    }
    pub unsafe fn GetBuffer(&self) -> Option<ID3D11ShaderReflectionConstantBuffer> {
        unsafe {
            (windows_core::Interface::vtable(self).GetBuffer)(windows_core::Interface::as_raw(self))
        }
    }
    pub unsafe fn GetInterfaceSlot(&self, uarrayindex: u32) -> u32 {
        unsafe {
            (windows_core::Interface::vtable(self).GetInterfaceSlot)(
                windows_core::Interface::as_raw(self),
                uarrayindex,
            )
        }
    }
}
#[repr(C)]
pub struct ID3D11ShaderReflectionVariable_Vtbl {
    pub GetDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut D3D11_SHADER_VARIABLE_DESC,
    ) -> windows_core::HRESULT,
    pub GetType:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> Option<ID3D11ShaderReflectionType>,
    pub GetBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
    ) -> Option<ID3D11ShaderReflectionConstantBuffer>,
    pub GetInterfaceSlot: unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> u32,
}
pub trait ID3D11ShaderReflectionVariable_Impl {
    fn GetDesc(&self, pdesc: *mut D3D11_SHADER_VARIABLE_DESC) -> windows_core::Result<()>;
    fn GetType(&self) -> Option<ID3D11ShaderReflectionType>;
    fn GetBuffer(&self) -> Option<ID3D11ShaderReflectionConstantBuffer>;
    fn GetInterfaceSlot(&self, uarrayindex: u32) -> u32;
}
impl ID3D11ShaderReflectionVariable_Vtbl {
    pub const fn new<Identity: ID3D11ShaderReflectionVariable_Impl>() -> Self {
        unsafe extern "system" fn GetDesc<Identity: ID3D11ShaderReflectionVariable_Impl>(
            this: *mut core::ffi::c_void,
            pdesc: *mut D3D11_SHADER_VARIABLE_DESC,
        ) -> windows_core::HRESULT {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionVariable_Impl::GetDesc(
                    this,
                    core::mem::transmute_copy(&pdesc),
                )
                .into()
            }
        }
        unsafe extern "system" fn GetType<Identity: ID3D11ShaderReflectionVariable_Impl>(
            this: *mut core::ffi::c_void,
        ) -> Option<ID3D11ShaderReflectionType> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionVariable_Impl::GetType(this)
            }
        }
        unsafe extern "system" fn GetBuffer<Identity: ID3D11ShaderReflectionVariable_Impl>(
            this: *mut core::ffi::c_void,
        ) -> Option<ID3D11ShaderReflectionConstantBuffer> {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionVariable_Impl::GetBuffer(this)
            }
        }
        unsafe extern "system" fn GetInterfaceSlot<
            Identity: ID3D11ShaderReflectionVariable_Impl,
        >(
            this: *mut core::ffi::c_void,
            uarrayindex: u32,
        ) -> u32 {
            unsafe {
                let this = (this as *mut *mut core::ffi::c_void) as *const windows_core::ScopedHeap;
                let this = &*((*this).this as *const Identity);
                ID3D11ShaderReflectionVariable_Impl::GetInterfaceSlot(
                    this,
                    core::mem::transmute_copy(&uarrayindex),
                )
            }
        }
        Self {
            GetDesc: GetDesc::<Identity>,
            GetType: GetType::<Identity>,
            GetBuffer: GetBuffer::<Identity>,
            GetInterfaceSlot: GetInterfaceSlot::<Identity>,
        }
    }
}
struct ID3D11ShaderReflectionVariable_ImplVtbl<T: ID3D11ShaderReflectionVariable_Impl>(
    core::marker::PhantomData<T>,
);
impl<T: ID3D11ShaderReflectionVariable_Impl> ID3D11ShaderReflectionVariable_ImplVtbl<T> {
    const VTABLE: ID3D11ShaderReflectionVariable_Vtbl =
        ID3D11ShaderReflectionVariable_Vtbl::new::<T>();
}
impl ID3D11ShaderReflectionVariable {
    pub fn new<'a, T: ID3D11ShaderReflectionVariable_Impl>(
        this: &'a T,
    ) -> windows_core::ScopedInterface<'a, Self> {
        let this = windows_core::ScopedHeap {
            vtable: &ID3D11ShaderReflectionVariable_ImplVtbl::<T>::VTABLE as *const _ as *const _,
            this: this as *const _ as *const _,
        };
        let this = core::mem::ManuallyDrop::new(windows_core::imp::box_new(this));
        unsafe { windows_core::ScopedInterface::new(core::mem::transmute(&this.vtable)) }
    }
}
windows_core::imp::define_interface!(
    ID3D11ShaderResourceView,
    ID3D11ShaderResourceView_Vtbl,
    0xb0e06fe0_8192_4e1a_b1ca_36d7414710b2
);
impl core::ops::Deref for ID3D11ShaderResourceView {
    type Target = ID3D11View;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11ShaderResourceView,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11View
);
impl ID3D11ShaderResourceView {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_SHADER_RESOURCE_VIEW_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11ShaderResourceView_Vtbl {
    pub base__: ID3D11View_Vtbl,
    pub GetDesc:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_SHADER_RESOURCE_VIEW_DESC),
}
impl windows_core::RuntimeName for ID3D11ShaderResourceView {}
windows_core::imp::define_interface!(
    ID3D11Texture1D,
    ID3D11Texture1D_Vtbl,
    0xf8fb5c27_c6b3_4f75_a4c8_439af2ef564c
);
impl core::ops::Deref for ID3D11Texture1D {
    type Target = ID3D11Resource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Texture1D,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Resource
);
#[repr(C)]
pub struct ID3D11Texture1D_Vtbl {
    pub base__: ID3D11Resource_Vtbl,
    GetDesc: usize,
}
impl windows_core::RuntimeName for ID3D11Texture1D {}
windows_core::imp::define_interface!(
    ID3D11Texture2D,
    ID3D11Texture2D_Vtbl,
    0x6f15aaf2_d208_4e89_9ab4_489535d34f9c
);
impl core::ops::Deref for ID3D11Texture2D {
    type Target = ID3D11Resource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Texture2D,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Resource
);
impl ID3D11Texture2D {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_TEXTURE2D_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11Texture2D_Vtbl {
    pub base__: ID3D11Resource_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_TEXTURE2D_DESC),
}
impl windows_core::RuntimeName for ID3D11Texture2D {}
windows_core::imp::define_interface!(
    ID3D11Texture3D,
    ID3D11Texture3D_Vtbl,
    0x037e866e_f56d_4357_a8af_9dabbe6e250e
);
impl core::ops::Deref for ID3D11Texture3D {
    type Target = ID3D11Resource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11Texture3D,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11Resource
);
impl ID3D11Texture3D {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_TEXTURE3D_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11Texture3D_Vtbl {
    pub base__: ID3D11Resource_Vtbl,
    pub GetDesc: unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_TEXTURE3D_DESC),
}
impl windows_core::RuntimeName for ID3D11Texture3D {}
windows_core::imp::define_interface!(
    ID3D11UnorderedAccessView,
    ID3D11UnorderedAccessView_Vtbl,
    0x28acf509_7f5c_48f6_8611_f316010a6380
);
impl core::ops::Deref for ID3D11UnorderedAccessView {
    type Target = ID3D11View;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11UnorderedAccessView,
    windows_core::IUnknown,
    ID3D11DeviceChild,
    ID3D11View
);
impl ID3D11UnorderedAccessView {
    pub unsafe fn GetDesc(&self, pdesc: *mut D3D11_UNORDERED_ACCESS_VIEW_DESC) {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            );
        }
    }
}
#[repr(C)]
pub struct ID3D11UnorderedAccessView_Vtbl {
    pub base__: ID3D11View_Vtbl,
    pub GetDesc:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut D3D11_UNORDERED_ACCESS_VIEW_DESC),
}
impl windows_core::RuntimeName for ID3D11UnorderedAccessView {}
windows_core::imp::define_interface!(
    ID3D11VertexShader,
    ID3D11VertexShader_Vtbl,
    0x3b301d64_d678_4289_8897_22f8928b72f3
);
impl core::ops::Deref for ID3D11VertexShader {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11VertexShader,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11VertexShader_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
}
impl windows_core::RuntimeName for ID3D11VertexShader {}
windows_core::imp::define_interface!(
    ID3D11View,
    ID3D11View_Vtbl,
    0x839d1216_bb2e_412b_b7f4_a9dbebe08ed1
);
impl core::ops::Deref for ID3D11View {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(ID3D11View, windows_core::IUnknown, ID3D11DeviceChild);
#[repr(C)]
pub struct ID3D11View_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    GetResource: usize,
}
impl windows_core::RuntimeName for ID3D11View {}
windows_core::imp::define_interface!(ID3DInclude, ID3DInclude_Vtbl);
#[repr(C)]
pub struct ID3DInclude_Vtbl {
    Open: usize,
    Close: usize,
}
windows_core::imp::define_interface!(
    IDXGIAdapter,
    IDXGIAdapter_Vtbl,
    0x2411e7e1_12ac_4ccf_bd14_9798e8534dc0
);
impl core::ops::Deref for IDXGIAdapter {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIAdapter, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIAdapter_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    EnumOutputs: usize,
    GetDesc: usize,
    CheckInterfaceSupport: usize,
}
impl windows_core::RuntimeName for IDXGIAdapter {}
windows_core::imp::define_interface!(
    IDXGIAdapter1,
    IDXGIAdapter1_Vtbl,
    0x29038f61_3839_4626_91fd_086879011a05
);
impl core::ops::Deref for IDXGIAdapter1 {
    type Target = IDXGIAdapter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIAdapter1,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIAdapter
);
impl IDXGIAdapter1 {
    pub unsafe fn GetDesc1(&self, pdesc: *mut DXGI_ADAPTER_DESC1) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc1)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
}
#[repr(C)]
pub struct IDXGIAdapter1_Vtbl {
    pub base__: IDXGIAdapter_Vtbl,
    pub GetDesc1: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut DXGI_ADAPTER_DESC1,
    ) -> windows_core::HRESULT,
}
impl windows_core::RuntimeName for IDXGIAdapter1 {}
windows_core::imp::define_interface!(
    IDXGIDevice,
    IDXGIDevice_Vtbl,
    0x54ec77fa_1377_44e6_8c32_88fd5f44c84c
);
impl core::ops::Deref for IDXGIDevice {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIDevice, windows_core::IUnknown, IDXGIObject);
impl IDXGIDevice {
    pub unsafe fn GetAdapter(&self) -> windows_core::Result<IDXGIAdapter> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetAdapter)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub unsafe fn CreateSurface(
        &self,
        pdesc: *const DXGI_SURFACE_DESC,
        numsurfaces: u32,
        usage: DXGI_USAGE,
        psharedresource: Option<*const DXGI_SHARED_RESOURCE>,
        ppsurface: *mut Option<IDXGISurface>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).CreateSurface)(
                windows_core::Interface::as_raw(self),
                pdesc,
                numsurfaces,
                usage,
                psharedresource.unwrap_or(core::mem::zeroed()) as _,
                core::mem::transmute(ppsurface),
            )
        }
    }
    pub unsafe fn QueryResourceResidency(
        &self,
        ppresources: *const Option<windows_core::IUnknown>,
        presidencystatus: *mut DXGI_RESIDENCY,
        numresources: u32,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).QueryResourceResidency)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute(ppresources),
                presidencystatus as _,
                numresources,
            )
        }
    }
    pub unsafe fn SetGPUThreadPriority(&self, priority: i32) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).SetGPUThreadPriority)(
                windows_core::Interface::as_raw(self),
                priority,
            )
        }
    }
    pub unsafe fn GetGPUThreadPriority(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetGPUThreadPriority)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IDXGIDevice_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    pub GetAdapter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateSurface: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const DXGI_SURFACE_DESC,
        u32,
        DXGI_USAGE,
        *const DXGI_SHARED_RESOURCE,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub QueryResourceResidency: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const *mut core::ffi::c_void,
        *mut DXGI_RESIDENCY,
        u32,
    ) -> windows_core::HRESULT,
    pub SetGPUThreadPriority:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    pub GetGPUThreadPriority:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
}
impl windows_core::RuntimeName for IDXGIDevice {}
windows_core::imp::define_interface!(
    IDXGIDeviceSubObject,
    IDXGIDeviceSubObject_Vtbl,
    0x3d3e0379_f9de_4d58_bb6c_18d62992f1a6
);
impl core::ops::Deref for IDXGIDeviceSubObject {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIDeviceSubObject, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIDeviceSubObject_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    GetDevice: usize,
}
impl windows_core::RuntimeName for IDXGIDeviceSubObject {}
windows_core::imp::define_interface!(
    IDXGIFactory,
    IDXGIFactory_Vtbl,
    0x7b7166ec_21c7_44ae_b21a_c9ae321ae369
);
impl core::ops::Deref for IDXGIFactory {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIFactory, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIFactory_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    EnumAdapters: usize,
    MakeWindowAssociation: usize,
    GetWindowAssociation: usize,
    CreateSwapChain: usize,
    CreateSoftwareAdapter: usize,
}
impl windows_core::RuntimeName for IDXGIFactory {}
windows_core::imp::define_interface!(
    IDXGIFactory1,
    IDXGIFactory1_Vtbl,
    0x770aae78_f26f_4dba_a829_253c83d1b387
);
impl core::ops::Deref for IDXGIFactory1 {
    type Target = IDXGIFactory;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory1,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory
);
impl IDXGIFactory1 {
    pub unsafe fn EnumAdapters1(&self, adapter: u32) -> windows_core::Result<IDXGIAdapter1> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).EnumAdapters1)(
                windows_core::Interface::as_raw(self),
                adapter,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub unsafe fn IsCurrent(&self) -> windows_core::BOOL {
        unsafe {
            (windows_core::Interface::vtable(self).IsCurrent)(windows_core::Interface::as_raw(self))
        }
    }
}
#[repr(C)]
pub struct IDXGIFactory1_Vtbl {
    pub base__: IDXGIFactory_Vtbl,
    pub EnumAdapters1: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsCurrent: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::BOOL,
}
impl windows_core::RuntimeName for IDXGIFactory1 {}
windows_core::imp::define_interface!(
    IDXGIFactory2,
    IDXGIFactory2_Vtbl,
    0x50c83a1c_e072_4c48_87b0_3630fa36a6d0
);
impl core::ops::Deref for IDXGIFactory2 {
    type Target = IDXGIFactory1;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory2,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory,
    IDXGIFactory1
);
#[repr(C)]
pub struct IDXGIFactory2_Vtbl {
    pub base__: IDXGIFactory1_Vtbl,
    IsWindowedStereoEnabled: usize,
    CreateSwapChainForHwnd: usize,
    CreateSwapChainForCoreWindow: usize,
    GetSharedResourceAdapterLuid: usize,
    RegisterStereoStatusWindow: usize,
    RegisterStereoStatusEvent: usize,
    UnregisterStereoStatus: usize,
    RegisterOcclusionStatusWindow: usize,
    RegisterOcclusionStatusEvent: usize,
    UnregisterOcclusionStatus: usize,
    CreateSwapChainForComposition: usize,
}
impl windows_core::RuntimeName for IDXGIFactory2 {}
windows_core::imp::define_interface!(
    IDXGIFactory3,
    IDXGIFactory3_Vtbl,
    0x25483823_cd46_4c7d_86ca_47aa95b837bd
);
impl core::ops::Deref for IDXGIFactory3 {
    type Target = IDXGIFactory2;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory3,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory,
    IDXGIFactory1,
    IDXGIFactory2
);
#[repr(C)]
pub struct IDXGIFactory3_Vtbl {
    pub base__: IDXGIFactory2_Vtbl,
    GetCreationFlags: usize,
}
impl windows_core::RuntimeName for IDXGIFactory3 {}
windows_core::imp::define_interface!(
    IDXGIFactory4,
    IDXGIFactory4_Vtbl,
    0x1bc6ea02_ef36_464f_bf0c_21ca39e5168a
);
impl core::ops::Deref for IDXGIFactory4 {
    type Target = IDXGIFactory3;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory4,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory,
    IDXGIFactory1,
    IDXGIFactory2,
    IDXGIFactory3
);
#[repr(C)]
pub struct IDXGIFactory4_Vtbl {
    pub base__: IDXGIFactory3_Vtbl,
    EnumAdapterByLuid: usize,
    EnumWarpAdapter: usize,
}
impl windows_core::RuntimeName for IDXGIFactory4 {}
windows_core::imp::define_interface!(
    IDXGIFactory5,
    IDXGIFactory5_Vtbl,
    0x7632e1f5_ee65_4dca_87fd_84cd75f8838d
);
impl core::ops::Deref for IDXGIFactory5 {
    type Target = IDXGIFactory4;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory5,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory,
    IDXGIFactory1,
    IDXGIFactory2,
    IDXGIFactory3,
    IDXGIFactory4
);
#[repr(C)]
pub struct IDXGIFactory5_Vtbl {
    pub base__: IDXGIFactory4_Vtbl,
    CheckFeatureSupport: usize,
}
impl windows_core::RuntimeName for IDXGIFactory5 {}
windows_core::imp::define_interface!(
    IDXGIFactory6,
    IDXGIFactory6_Vtbl,
    0xc1b6694f_ff09_44a9_b03c_77900a0a1d17
);
impl core::ops::Deref for IDXGIFactory6 {
    type Target = IDXGIFactory5;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGIFactory6,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIFactory,
    IDXGIFactory1,
    IDXGIFactory2,
    IDXGIFactory3,
    IDXGIFactory4,
    IDXGIFactory5
);
impl IDXGIFactory6 {
    pub unsafe fn EnumAdapterByGpuPreference<T>(
        &self,
        adapter: u32,
        gpupreference: DXGI_GPU_PREFERENCE,
    ) -> windows_core::Result<T>
    where
        T: windows_core::Interface,
    {
        let mut result__ = core::ptr::null_mut();
        unsafe {
            (windows_core::Interface::vtable(self).EnumAdapterByGpuPreference)(
                windows_core::Interface::as_raw(self),
                adapter,
                gpupreference,
                &T::IID,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDXGIFactory6_Vtbl {
    pub base__: IDXGIFactory5_Vtbl,
    pub EnumAdapterByGpuPreference: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        DXGI_GPU_PREFERENCE,
        *const windows_core::GUID,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
impl windows_core::RuntimeName for IDXGIFactory6 {}
windows_core::imp::define_interface!(
    IDXGIObject,
    IDXGIObject_Vtbl,
    0xaec22fb8_76f3_4639_9be0_28eb43a67a2e
);
windows_core::imp::interface_hierarchy!(IDXGIObject, windows_core::IUnknown);
#[repr(C)]
pub struct IDXGIObject_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
    GetPrivateData: usize,
    GetParent: usize,
}
impl windows_core::RuntimeName for IDXGIObject {}
windows_core::imp::define_interface!(
    IDXGIOutput,
    IDXGIOutput_Vtbl,
    0xae02eedb_c735_4690_8d52_5a8dc20213aa
);
impl core::ops::Deref for IDXGIOutput {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIOutput, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIOutput_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    GetDesc: usize,
    GetDisplayModeList: usize,
    FindClosestMatchingMode: usize,
    WaitForVBlank: usize,
    TakeOwnership: usize,
    ReleaseOwnership: usize,
    GetGammaControlCapabilities: usize,
    SetGammaControl: usize,
    GetGammaControl: usize,
    SetDisplaySurface: usize,
    GetDisplaySurfaceData: usize,
    GetFrameStatistics: usize,
}
impl windows_core::RuntimeName for IDXGIOutput {}
windows_core::imp::define_interface!(
    IDXGISurface,
    IDXGISurface_Vtbl,
    0xcafcb56c_6ac3_4889_bf47_9e23bbd260ec
);
impl core::ops::Deref for IDXGISurface {
    type Target = IDXGIDeviceSubObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGISurface,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIDeviceSubObject
);
#[repr(C)]
pub struct IDXGISurface_Vtbl {
    pub base__: IDXGIDeviceSubObject_Vtbl,
    GetDesc: usize,
    Map: usize,
    Unmap: usize,
}
impl windows_core::RuntimeName for IDXGISurface {}
windows_core::imp::define_interface!(
    IDXGISwapChain,
    IDXGISwapChain_Vtbl,
    0x310d36a0_d2e7_4c0a_aa04_6a9d23b8886a
);
impl core::ops::Deref for IDXGISwapChain {
    type Target = IDXGIDeviceSubObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    IDXGISwapChain,
    windows_core::IUnknown,
    IDXGIObject,
    IDXGIDeviceSubObject
);
impl IDXGISwapChain {
    pub unsafe fn Present(&self, syncinterval: u32, flags: u32) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).Present)(
                windows_core::Interface::as_raw(self),
                syncinterval,
                flags,
            )
        }
    }
    pub unsafe fn GetBuffer<T>(&self, buffer: u32) -> windows_core::Result<T>
    where
        T: windows_core::Interface,
    {
        let mut result__ = core::ptr::null_mut();
        unsafe {
            (windows_core::Interface::vtable(self).GetBuffer)(
                windows_core::Interface::as_raw(self),
                buffer,
                &T::IID,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub unsafe fn SetFullscreenState<P1>(
        &self,
        fullscreen: bool,
        ptarget: P1,
    ) -> windows_core::HRESULT
    where
        P1: windows_core::Param<IDXGIOutput>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetFullscreenState)(
                windows_core::Interface::as_raw(self),
                fullscreen.into(),
                ptarget.param().abi(),
            )
        }
    }
    pub unsafe fn GetFullscreenState(
        &self,
        pfullscreen: Option<*mut windows_core::BOOL>,
        pptarget: Option<*mut Option<IDXGIOutput>>,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetFullscreenState)(
                windows_core::Interface::as_raw(self),
                pfullscreen.unwrap_or(core::mem::zeroed()) as _,
                pptarget.unwrap_or(core::mem::zeroed()) as _,
            )
        }
    }
    pub unsafe fn GetDesc(&self, pdesc: *mut DXGI_SWAP_CHAIN_DESC) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetDesc)(
                windows_core::Interface::as_raw(self),
                pdesc as _,
            )
        }
    }
    pub unsafe fn ResizeBuffers(
        &self,
        buffercount: u32,
        width: u32,
        height: u32,
        newformat: DXGI_FORMAT,
        swapchainflags: u32,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).ResizeBuffers)(
                windows_core::Interface::as_raw(self),
                buffercount,
                width,
                height,
                newformat,
                swapchainflags,
            )
        }
    }
    pub unsafe fn ResizeTarget(
        &self,
        pnewtargetparameters: *const DXGI_MODE_DESC,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).ResizeTarget)(
                windows_core::Interface::as_raw(self),
                pnewtargetparameters,
            )
        }
    }
    pub unsafe fn GetContainingOutput(&self) -> windows_core::Result<IDXGIOutput> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetContainingOutput)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub unsafe fn GetFrameStatistics(
        &self,
        pstats: *mut DXGI_FRAME_STATISTICS,
    ) -> windows_core::HRESULT {
        unsafe {
            (windows_core::Interface::vtable(self).GetFrameStatistics)(
                windows_core::Interface::as_raw(self),
                pstats as _,
            )
        }
    }
    pub unsafe fn GetLastPresentCount(&self) -> windows_core::Result<u32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetLastPresentCount)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IDXGISwapChain_Vtbl {
    pub base__: IDXGIDeviceSubObject_Vtbl,
    pub Present:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, u32) -> windows_core::HRESULT,
    pub GetBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *const windows_core::GUID,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetFullscreenState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::BOOL,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetFullscreenState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_core::BOOL,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetDesc: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut DXGI_SWAP_CHAIN_DESC,
    ) -> windows_core::HRESULT,
    pub ResizeBuffers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        u32,
        u32,
        DXGI_FORMAT,
        u32,
    ) -> windows_core::HRESULT,
    pub ResizeTarget: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const DXGI_MODE_DESC,
    ) -> windows_core::HRESULT,
    pub GetContainingOutput: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetFrameStatistics: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut DXGI_FRAME_STATISTICS,
    ) -> windows_core::HRESULT,
    pub GetLastPresentCount:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
}
impl windows_core::RuntimeName for IDXGISwapChain {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LUID {
    pub LowPart: u32,
    pub HighPart: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
