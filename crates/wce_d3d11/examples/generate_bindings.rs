//! Regenerates src/bindings.rs: `cargo run -p wce_d3d11 --example generate_bindings`. Add what the editor needs to `names`;
//! windows-bindgen adds what they depend on.

fn main() {
    let names = [
        "D3D11CreateDevice", "D3D11_SDK_VERSION", "D3DCompile", "D3DReflect", "D3DDisassemble",
        "ID3D11Device", "ID3D11DeviceContext", "ID3D11Texture2D", "ID3D11RenderTargetView", "ID3D11Buffer",
        "ID3D11VertexShader", "ID3D11PixelShader", "ID3D11ShaderReflection", "ID3D11ShaderReflectionConstantBuffer", "ID3D11ShaderReflectionVariable", "ID3D11ShaderReflectionType", "D3D11_PRIMITIVE_TOPOLOGY", "ID3D10Blob",
        "D3D11_TEXTURE2D_DESC", "D3D11_BUFFER_DESC", "D3D11_MAPPED_SUBRESOURCE", "D3D11_VIEWPORT", "D3D11_SUBRESOURCE_DATA",
        "D3D11_USAGE", "D3D11_BIND_FLAG", "D3D11_CPU_ACCESS_FLAG", "D3D11_MAP", "D3D11_CREATE_DEVICE_FLAG",
        "D3D11_SHADER_DESC", "D3D11_SHADER_BUFFER_DESC", "D3D11_SHADER_VARIABLE_DESC", "D3D11_SHADER_INPUT_BIND_DESC",
        "D3D_DRIVER_TYPE", "D3D_FEATURE_LEVEL", "D3D_PRIMITIVE_TOPOLOGY", "DXGI_FORMAT", "DXGI_SAMPLE_DESC",
        // The renderer, the editor and the app.
        "D3D11CreateDeviceAndSwapChain", "CreateDXGIFactory1", "IDXGIFactory1", "IDXGIFactory6", "IDXGIAdapter1", "IDXGIDevice", "IDXGISwapChain",
        "DXGI_SWAP_CHAIN_DESC", "DXGI_SWAP_EFFECT", "DXGI_USAGE_RENDER_TARGET_OUTPUT", "DXGI_ADAPTER_DESC1", "DXGI_ADAPTER_FLAG", "DXGI_GPU_PREFERENCE",
        "ID3D11ComputeShader", "ID3D11GeometryShader", "ID3D11HullShader", "ID3D11DomainShader", "ID3D11Query", "ID3D11Resource", "ID3D11Texture3D",
        "ID3D11ShaderResourceView", "ID3D11SamplerState", "ID3D11BlendState", "ID3D11DepthStencilState", "ID3D11DepthStencilView", "ID3D11RasterizerState",
        "ID3D11InputLayout", "ID3D11UnorderedAccessView",
        "D3D11_ASYNC_GETDATA_FLAG", "D3D11_BLEND_DESC", "D3D11_RENDER_TARGET_BLEND_DESC", "D3D11_BLEND", "D3D11_BLEND_OP", "D3D11_BOX", "D3D11_CLEAR_FLAG",
        "D3D11_COLOR_WRITE_ENABLE", "D3D11_COMPARISON_FUNC", "D3D11_CULL_MODE", "D3D11_FILL_MODE", "D3D11_DEPTH_STENCIL_DESC", "D3D11_DEPTH_STENCILOP_DESC",
        "D3D11_DEPTH_WRITE_MASK", "D3D11_DEPTH_STENCIL_VIEW_DESC", "D3D11_DSV_DIMENSION", "D3D11_FILTER", "D3D11_FLOAT32_MAX", "D3D11_INPUT_CLASSIFICATION",
        "D3D11_INPUT_ELEMENT_DESC", "D3D11_MAP_FLAG", "D3D11_QUERY", "D3D11_QUERY_DATA_TIMESTAMP_DISJOINT", "D3D11_QUERY_DESC", "D3D11_RASTERIZER_DESC",
        "D3D11_RENDER_TARGET_VIEW_DESC", "D3D11_RTV_DIMENSION", "D3D11_RESOURCE_MISC_FLAG", "D3D11_SAMPLER_DESC", "D3D11_SHADER_RESOURCE_VIEW_DESC",
        "D3D11_SHADER_TYPE_DESC", "D3D11_SHADER_VERSION_TYPE", "D3D11_SIGNATURE_PARAMETER_DESC", "D3D_SRV_DIMENSION", "D3D11_STENCIL_OP", "D3D11_TEXTURE3D_DESC",
        "D3D11_TEXTURE_ADDRESS_MODE", "D3D11_UNORDERED_ACCESS_VIEW_DESC", "D3D11_UAV_DIMENSION", "D3D_SHADER_INPUT_TYPE", "D3D_CBUFFER_TYPE",
        "D3D_SHADER_VARIABLE_CLASS", "D3D_SHADER_MACRO", "D3D11_APPEND_ALIGNED_ELEMENT",
        "ID3D11View", "ID3D11DeviceChild", "ID3D11Asynchronous", "IDXGIObject", "IDXGIDeviceSubObject", "IDXGIFactory", "IDXGIFactory2", "IDXGIFactory3", "IDXGIFactory4", "IDXGIFactory5",
    ];
    let mut args = vec!["--out", concat!(env!("CARGO_MANIFEST_DIR"), "/src/bindings.rs"), "--flat", "--filter"];
    args.extend(names);
    windows_bindgen::bindgen(args);
}
