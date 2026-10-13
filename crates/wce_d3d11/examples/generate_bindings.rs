//! Regenerates src/bindings.rs: `cargo run -p wce_d3d11 --example generate_bindings`. Add what the editor needs to `names`;
//! windows-bindgen adds what they depend on.

fn main() {
    let names = [
        "D3D11CreateDevice", "D3D11_SDK_VERSION", "D3DCompile", "D3DReflect",
        "ID3D11Device", "ID3D11DeviceContext", "ID3D11Texture2D", "ID3D11RenderTargetView", "ID3D11Buffer",
        "ID3D11VertexShader", "ID3D11PixelShader", "ID3D11ShaderReflection", "ID3D11ShaderReflectionConstantBuffer", "ID3D11ShaderReflectionVariable", "ID3D11ShaderReflectionType", "D3D11_PRIMITIVE_TOPOLOGY", "ID3D10Blob",
        "D3D11_TEXTURE2D_DESC", "D3D11_BUFFER_DESC", "D3D11_MAPPED_SUBRESOURCE", "D3D11_VIEWPORT", "D3D11_SUBRESOURCE_DATA",
        "D3D11_USAGE", "D3D11_BIND_FLAG", "D3D11_CPU_ACCESS_FLAG", "D3D11_MAP", "D3D11_CREATE_DEVICE_FLAG",
        "D3D11_SHADER_DESC", "D3D11_SHADER_BUFFER_DESC", "D3D11_SHADER_VARIABLE_DESC", "D3D11_SHADER_INPUT_BIND_DESC",
        "D3D_DRIVER_TYPE", "D3D_FEATURE_LEVEL", "D3D_PRIMITIVE_TOPOLOGY", "DXGI_FORMAT", "DXGI_SAMPLE_DESC",
    ];
    let mut args = vec!["--out", concat!(env!("CARGO_MANIFEST_DIR"), "/src/bindings.rs"), "--flat", "--filter"];
    args.extend(names);
    windows_bindgen::bindgen(args);
}
