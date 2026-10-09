#include "render/shader_lib.h"

#include "vfs/vfs.h"
#include "formats/fxc.h"

#include <d3d11shader.h>
#include <d3dcompiler.h>

#include <algorithm>
#include <cstring>
#include <stdexcept>

using Microsoft::WRL::ComPtr;

namespace wce {

void ConstantBuffers::declare(const std::string& name, uint32_t size, const std::map<std::string, Var>& vars) {
    Buffer& b = buffers_[name];
    b.name = name;
    uint32_t aligned = (size + 15) & ~15u;
    if (b.cpu.size() < aligned) {
        b.cpu.resize(aligned, 0);
        b.gpu.Reset();
        b.dirty = true;
    }
    for (auto& [k, v] : vars) b.vars.emplace(k, v);
}

bool ConstantBuffers::has(const std::string& cb, const std::string& var) const {
    auto it = buffers_.find(cb);
    return it != buffers_.end() && it->second.vars.count(var);
}

void ConstantBuffers::set(const std::string& cb, const std::string& var, const void* data, size_t size) {
    auto it = buffers_.find(cb);
    const Var* v = nullptr;
    if (it != buffers_.end()) {
        auto vi = it->second.vars.find(var);
        if (vi != it->second.vars.end()) v = &vi->second;
    }
    if (!v) {
        std::string key = cb + "." + var;
        if (std::find(missing_.begin(), missing_.end(), key) == missing_.end()) missing_.push_back(key);
        return;
    }
    size_t n = std::min<size_t>(size, v->size);
    uint8_t* dst = it->second.cpu.data() + v->offset;
    if (std::memcmp(dst, data, n) != 0) {
        std::memcpy(dst, data, n);
        it->second.dirty = true;
    }
}

void ConstantBuffers::set_f(const std::string& cb, const std::string& var, std::initializer_list<float> v) {
    set(cb, var, v.begin(), v.size() * sizeof(float));
}

void ConstantBuffers::set_i(const std::string& cb, const std::string& var, int v) { set(cb, var, &v, sizeof v); }

void ConstantBuffers::set_m(const std::string& cb, const std::string& var, const float m[16]) { set(cb, var, m, 64); }

ConstantBuffers::Buffer* ConstantBuffers::find(const std::string& cb) {
    auto it = buffers_.find(cb);
    return it == buffers_.end() ? nullptr : &it->second;
}

void ConstantBuffers::upload(ID3D11Device* dev, ID3D11DeviceContext* ctx) {
    for (auto& [name, b] : buffers_) {
        if (!b.gpu) {
            D3D11_BUFFER_DESC bd{};
            bd.ByteWidth = UINT(b.cpu.size());
            bd.Usage = D3D11_USAGE_DEFAULT;
            bd.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
            dev->CreateBuffer(&bd, nullptr, &b.gpu);
            b.dirty = true;
        }
        if (b.dirty) {
            ctx->UpdateSubresource(b.gpu.Get(), 0, nullptr, b.cpu.data(), 0, 0);
            b.dirty = false;
        }
    }
}

const GameShader& ShaderLibrary::get(const std::string& name) {
    auto it = cache_.find(name);
    if (it != cache_.end()) return *it->second;

    FxcFile f = parse_fxc(vfs_.read("fxc/" + name + ".fxc"));
    auto s = std::make_unique<GameShader>();
    s->name = name;
    s->bytecode.assign(f.dxbc(), f.dxbc() + f.dxbc_size());
    ComPtr<ID3D11ShaderReflection> refl;
    if (FAILED(D3DReflect(s->bytecode.data(), s->bytecode.size(), IID_PPV_ARGS(&refl)))) throw std::runtime_error("reflect failed: " + name);
    D3D11_SHADER_DESC sd{};
    refl->GetDesc(&sd);
    HRESULT hr = E_FAIL;
    const void* code = s->bytecode.data();
    size_t size = s->bytecode.size();
    switch (D3D11_SHVER_GET_TYPE(sd.Version)) {
    case D3D11_SHVER_VERTEX_SHADER: s->stage = Stage::VS; hr = dev_->CreateVertexShader(code, size, nullptr, &s->vs); break;
    case D3D11_SHVER_PIXEL_SHADER: s->stage = Stage::PS; hr = dev_->CreatePixelShader(code, size, nullptr, &s->ps); break;
    case D3D11_SHVER_GEOMETRY_SHADER: s->stage = Stage::GS; hr = dev_->CreateGeometryShader(code, size, nullptr, &s->gs); break;
    case D3D11_SHVER_COMPUTE_SHADER: s->stage = Stage::CS; hr = dev_->CreateComputeShader(code, size, nullptr, &s->cs); break;
    default: throw std::runtime_error("unsupported shader stage: " + name);
    }
    if (FAILED(hr)) throw std::runtime_error("create failed: " + name);

    for (UINT r = 0; r < sd.BoundResources; ++r) {
        D3D11_SHADER_INPUT_BIND_DESC bd{};
        refl->GetResourceBindingDesc(r, &bd);
        switch (bd.Type) {
        case D3D_SIT_CBUFFER: s->cbuffers[bd.Name] = bd.BindPoint; break;
        case D3D_SIT_TEXTURE: case D3D_SIT_STRUCTURED: case D3D_SIT_BYTEADDRESS: s->textures[bd.Name] = bd.BindPoint; break;
        case D3D_SIT_SAMPLER: s->samplers[bd.Name] = bd.BindPoint; break;
        default: s->uavs[bd.Name] = bd.BindPoint; break;
        }
    }
    for (UINT c = 0; c < sd.ConstantBuffers; ++c) {
        auto* cb = refl->GetConstantBufferByIndex(c);
        D3D11_SHADER_BUFFER_DESC bd{};
        cb->GetDesc(&bd);
        if (bd.Type != D3D_CT_CBUFFER) continue;
        std::map<std::string, ConstantBuffers::Var> vars;
        for (UINT v = 0; v < bd.Variables; ++v) {
            D3D11_SHADER_VARIABLE_DESC vd{};
            cb->GetVariableByIndex(v)->GetDesc(&vd);
            vars[vd.Name] = {vd.StartOffset, vd.Size};
        }
        cbs_.declare(bd.Name, bd.Size, vars);
    }
    auto& ref = *s;
    cache_[name] = std::move(s);
    return ref;
}

ID3D11InputLayout* ShaderLibrary::layout(const GameShader& vs, const std::string& id, const std::vector<D3D11_INPUT_ELEMENT_DESC>& elems) {
    std::string key = vs.name + "|" + id;
    auto it = layouts_.find(key);
    if (it != layouts_.end()) return it->second.Get();
    ComPtr<ID3D11InputLayout> il;
    if (FAILED(dev_->CreateInputLayout(elems.data(), UINT(elems.size()), vs.bytecode.data(), vs.bytecode.size(), &il)))
        throw std::runtime_error("input layout failed: " + key);
    layouts_[key] = il;
    return il.Get();
}

void bind_shader(ID3D11DeviceContext* ctx, const GameShader& s, ConstantBuffers& cbs) {
    switch (s.stage) {
    case Stage::VS: ctx->VSSetShader(s.vs.Get(), nullptr, 0); break;
    case Stage::PS: ctx->PSSetShader(s.ps.Get(), nullptr, 0); break;
    case Stage::GS: ctx->GSSetShader(s.gs.Get(), nullptr, 0); break;
    case Stage::CS: ctx->CSSetShader(s.cs.Get(), nullptr, 0); break;
    default: break;
    }
    for (auto& [name, slot] : s.cbuffers) {
        auto* b = cbs.find(name);
        if (!b || !b->gpu) continue;
        ID3D11Buffer* buf = b->gpu.Get();
        switch (s.stage) {
        case Stage::VS: ctx->VSSetConstantBuffers(slot, 1, &buf); break;
        case Stage::PS: ctx->PSSetConstantBuffers(slot, 1, &buf); break;
        case Stage::GS: ctx->GSSetConstantBuffers(slot, 1, &buf); break;
        case Stage::CS: ctx->CSSetConstantBuffers(slot, 1, &buf); break;
        default: break;
        }
    }
}

void bind_srv(ID3D11DeviceContext* ctx, const GameShader& s, const std::string& name, ID3D11ShaderResourceView* srv) {
    auto it = s.textures.find(name);
    if (it == s.textures.end()) return;
    switch (s.stage) {
    case Stage::VS: ctx->VSSetShaderResources(it->second, 1, &srv); break;
    case Stage::PS: ctx->PSSetShaderResources(it->second, 1, &srv); break;
    case Stage::GS: ctx->GSSetShaderResources(it->second, 1, &srv); break;
    case Stage::CS: ctx->CSSetShaderResources(it->second, 1, &srv); break;
    default: break;
    }
}

void bind_sampler(ID3D11DeviceContext* ctx, const GameShader& s, const std::string& name, ID3D11SamplerState* smp) {
    auto it = s.samplers.find(name);
    if (it == s.samplers.end()) return;
    switch (s.stage) {
    case Stage::VS: ctx->VSSetSamplers(it->second, 1, &smp); break;
    case Stage::PS: ctx->PSSetSamplers(it->second, 1, &smp); break;
    case Stage::GS: ctx->GSSetSamplers(it->second, 1, &smp); break;
    case Stage::CS: ctx->CSSetSamplers(it->second, 1, &smp); break;
    default: break;
    }
}

} // namespace wce
