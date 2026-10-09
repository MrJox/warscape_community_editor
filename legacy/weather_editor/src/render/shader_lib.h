#pragma once
// Game shaders, loaded from the packs' fxc/ folder and bound by reflected names.
//
// The engine keeps one object per named cbuffer (camera_VS_PS, lighting_VS_PS, fog_VS_PS, ...) shared
// by every shader that declares it; ConstantBuffers mirrors that: values are set by variable name,
// uploaded when dirty, and bound to whatever slot each shader's reflection asks for.
#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Vfs;

enum class Stage { VS, PS, GS, CS, HS, DS };

struct GameShader {
    std::string name; // fxc file name without extension
    Stage stage = Stage::PS;
    std::vector<uint8_t> bytecode;
    Microsoft::WRL::ComPtr<ID3D11VertexShader> vs;
    Microsoft::WRL::ComPtr<ID3D11PixelShader> ps;
    Microsoft::WRL::ComPtr<ID3D11GeometryShader> gs;
    Microsoft::WRL::ComPtr<ID3D11ComputeShader> cs;
    std::map<std::string, UINT> cbuffers, textures, samplers, uavs; // name -> slot
};

class ConstantBuffers {
public:
    struct Var { uint32_t offset = 0, size = 0; };
    struct Buffer {
        std::string name;
        std::vector<uint8_t> cpu;
        std::map<std::string, Var> vars;
        Microsoft::WRL::ComPtr<ID3D11Buffer> gpu;
        bool dirty = true;
    };

    // Declares (or widens) a buffer from a shader's reflection.
    void declare(const std::string& name, uint32_t size, const std::map<std::string, Var>& vars);
    bool has(const std::string& cb, const std::string& var) const;
    // Writes bytes into a variable; size is clamped to the variable. Unknown names are ignored
    // (and reported once through missing()) so a pass can set everything it knows about.
    void set(const std::string& cb, const std::string& var, const void* data, size_t size);
    void set_f(const std::string& cb, const std::string& var, std::initializer_list<float> v);
    void set_i(const std::string& cb, const std::string& var, int v);
    void set_m(const std::string& cb, const std::string& var, const float m[16]); // row-major, uploaded transposed? no: as-is
    Buffer* find(const std::string& cb);
    void upload(ID3D11Device* dev, ID3D11DeviceContext* ctx);
    const std::vector<std::string>& missing() const { return missing_; }
    const std::map<std::string, Buffer>& all() const { return buffers_; }

private:
    std::map<std::string, Buffer> buffers_;
    std::vector<std::string> missing_;
};

class ShaderLibrary {
public:
    ShaderLibrary(ID3D11Device* dev, Vfs& vfs, ConstantBuffers& cbs) : dev_(dev), vfs_(vfs), cbs_(cbs) {}
    // name: fxc file name without folder/extension, e.g. "dx11_sm_3_0_skydome.hlsl_ps_ps30_skygen". Throws.
    const GameShader& get(const std::string& name);
    // Input layout for a vertex shader (cached per shader + layout id).
    ID3D11InputLayout* layout(const GameShader& vs, const std::string& id, const std::vector<D3D11_INPUT_ELEMENT_DESC>& elems);

private:
    ID3D11Device* dev_;
    Vfs& vfs_;
    ConstantBuffers& cbs_;
    std::map<std::string, std::unique_ptr<GameShader>> cache_;
    std::map<std::string, Microsoft::WRL::ComPtr<ID3D11InputLayout>> layouts_;
};

// Binds a shader and its constant buffers. Textures/samplers are bound by name by the caller.
void bind_shader(ID3D11DeviceContext* ctx, const GameShader& s, ConstantBuffers& cbs);
void bind_srv(ID3D11DeviceContext* ctx, const GameShader& s, const std::string& name, ID3D11ShaderResourceView* srv);
void bind_sampler(ID3D11DeviceContext* ctx, const GameShader& s, const std::string& name, ID3D11SamplerState* smp);

} // namespace wce
