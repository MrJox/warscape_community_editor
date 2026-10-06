// Shader gate + reflection dump for the game's compiled shaders.
//
//   fxc_dump <fxc_dir> <out_dir>
//
// For every dx11_*.fxc: parse the CA header, reflect the DXBC, create the shader on a hardware
// D3D11 device. Writes:
//   <out_dir>/index.tsv     one row per shader: name parts, stage, create result, cbuffers, resources
//   <out_dir>/cbuffers.txt  every distinct constant buffer layout with its variables and users
#include "core/text_encoding.h"
#include "formats/fxc.h"

#include <d3d11.h>
#include <d3d11shader.h>
#include <d3dcompiler.h>
#include <wrl/client.h>

#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>
#include <set>
#include <sstream>

namespace fs = std::filesystem;
using Microsoft::WRL::ComPtr;
using namespace we;

namespace {

const char* stage_name(UINT type) {
    switch (type) {
    case D3D11_SHVER_PIXEL_SHADER: return "ps";
    case D3D11_SHVER_VERTEX_SHADER: return "vs";
    case D3D11_SHVER_GEOMETRY_SHADER: return "gs";
    case D3D11_SHVER_HULL_SHADER: return "hs";
    case D3D11_SHVER_DOMAIN_SHADER: return "ds";
    case D3D11_SHVER_COMPUTE_SHADER: return "cs";
    }
    return "??";
}

HRESULT create(ID3D11Device* dev, UINT type, const void* code, size_t size) {
    switch (type) {
    case D3D11_SHVER_PIXEL_SHADER: { ComPtr<ID3D11PixelShader> s; return dev->CreatePixelShader(code, size, nullptr, &s); }
    case D3D11_SHVER_VERTEX_SHADER: { ComPtr<ID3D11VertexShader> s; return dev->CreateVertexShader(code, size, nullptr, &s); }
    case D3D11_SHVER_GEOMETRY_SHADER: { ComPtr<ID3D11GeometryShader> s; return dev->CreateGeometryShader(code, size, nullptr, &s); }
    case D3D11_SHVER_HULL_SHADER: { ComPtr<ID3D11HullShader> s; return dev->CreateHullShader(code, size, nullptr, &s); }
    case D3D11_SHVER_DOMAIN_SHADER: { ComPtr<ID3D11DomainShader> s; return dev->CreateDomainShader(code, size, nullptr, &s); }
    case D3D11_SHVER_COMPUTE_SHADER: { ComPtr<ID3D11ComputeShader> s; return dev->CreateComputeShader(code, size, nullptr, &s); }
    }
    return E_INVALIDARG;
}

std::string type_string(ID3D11ShaderReflectionType* t) {
    D3D11_SHADER_TYPE_DESC d{};
    t->GetDesc(&d);
    std::string s = d.Name ? d.Name : "?";
    if (d.Class == D3D_SVC_STRUCT) {
        s = "struct{";
        for (UINT m = 0; m < d.Members; ++m) {
            D3D11_SHADER_TYPE_DESC md{};
            t->GetMemberTypeByIndex(m)->GetDesc(&md);
            s += std::string(t->GetMemberTypeName(m)) + ":" + (md.Name ? md.Name : "?") + "@" + std::to_string(md.Offset) + ";";
        }
        s += "}";
    }
    if (d.Elements) s += "[" + std::to_string(d.Elements) + "]";
    return s;
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 3) {
        std::printf("usage: fxc_dump <fxc_dir> <out_dir>\n");
        return 2;
    }
    fs::path in = argv[1], out = argv[2];
    fs::create_directories(out);

    ComPtr<ID3D11Device> dev;
    D3D_FEATURE_LEVEL fl = D3D_FEATURE_LEVEL_11_0;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0, &fl, 1, D3D11_SDK_VERSION, &dev, nullptr, nullptr))) {
        std::printf("no D3D11 hardware device\n");
        return 1;
    }

    std::ofstream index(out / "index.tsv");
    index << "file\tprefix\tsource\tstage\tentry\tversion\tcreate\tcbuffers\tresources\tinputs\n";
    std::map<std::string, std::set<std::string>> cb_layouts; // layout text -> sources
    int total = 0, failed = 0;
    std::map<std::string, int> per_stage;

    for (auto& de : fs::directory_iterator(in)) {
        std::string fn = de.path().filename().string();
        if (fn.rfind("dx11", 0) != 0 || de.path().extension() != ".fxc") continue;
        ++total;
        FxcName nm;
        split_fxc_name(fn, nm);
        std::string row = fn + "\t" + nm.prefix + "\t" + nm.source + "\t";
        try {
            FxcFile f = parse_fxc(read_file(de.path().string()));
            ComPtr<ID3D11ShaderReflection> refl;
            if (FAILED(D3DReflect(f.dxbc(), f.dxbc_size(), IID_PPV_ARGS(&refl)))) throw std::runtime_error("D3DReflect failed");
            D3D11_SHADER_DESC sd{};
            refl->GetDesc(&sd);
            UINT type = D3D11_SHVER_GET_TYPE(sd.Version);
            HRESULT hr = create(dev.Get(), type, f.dxbc(), f.dxbc_size());
            if (FAILED(hr)) ++failed;
            per_stage[stage_name(type)]++;

            std::string cbs, res, inputs;
            for (UINT c = 0; c < sd.ConstantBuffers; ++c) {
                auto* cb = refl->GetConstantBufferByIndex(c);
                D3D11_SHADER_BUFFER_DESC bd{};
                cb->GetDesc(&bd);
                cbs += std::string(c ? "," : "") + bd.Name + ":" + std::to_string(bd.Size);
                std::ostringstream lay;
                lay << "cbuffer " << bd.Name << " (" << bd.Size << " bytes)\n";
                for (UINT v = 0; v < bd.Variables; ++v) {
                    auto* var = cb->GetVariableByIndex(v);
                    D3D11_SHADER_VARIABLE_DESC vd{};
                    var->GetDesc(&vd);
                    lay << "  +" << vd.StartOffset << "\t" << vd.Size << "\t" << type_string(var->GetType()) << "\t" << vd.Name << "\n";
                }
                cb_layouts[lay.str()].insert(nm.source);
            }
            for (UINT r = 0; r < sd.BoundResources; ++r) {
                D3D11_SHADER_INPUT_BIND_DESC bd{};
                refl->GetResourceBindingDesc(r, &bd);
                static const char* kinds[] = {"cb", "tb", "t", "s", "u", "st", "u", "u", "u", "u", "u", "u"};
                res += std::string(r ? "," : "") + bd.Name + "=" + (bd.Type < 12 ? kinds[bd.Type] : "?") + std::to_string(bd.BindPoint);
            }
            for (UINT p = 0; p < sd.InputParameters; ++p) {
                D3D11_SIGNATURE_PARAMETER_DESC pd{};
                refl->GetInputParameterDesc(p, &pd);
                inputs += std::string(p ? "," : "") + pd.SemanticName + std::to_string(pd.SemanticIndex);
            }
            char ver[16];
            std::snprintf(ver, sizeof ver, "%u.%u", D3D11_SHVER_GET_MAJOR(sd.Version), D3D11_SHVER_GET_MINOR(sd.Version));
            row += std::string(stage_name(type)) + "\t" + nm.entry + "\t" + ver + "\t" + (SUCCEEDED(hr) ? "ok" : "FAIL") + "\t" + cbs + "\t" + res + "\t" + inputs;
        } catch (const std::exception& e) {
            ++failed;
            row += std::string("?\t") + nm.entry + "\t\tERROR " + e.what() + "\t\t\t";
        }
        index << row << "\n";
    }

    std::ofstream cbo(out / "cbuffers.txt");
    for (auto& [lay, users] : cb_layouts) {
        cbo << lay << "  used by:";
        for (auto& u : users) cbo << " " << u;
        cbo << "\n\n";
    }
    std::printf("%d shaders, %d failed; ", total, failed);
    for (auto& [s, n] : per_stage) std::printf("%s=%d ", s.c_str(), n);
    std::printf("\n%zu distinct cbuffer layouts\n", cb_layouts.size());
    return failed ? 1 : 0;
}
