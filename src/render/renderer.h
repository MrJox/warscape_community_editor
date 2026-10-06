#pragma once
// Battle renderer built from the game's own shaders. Pass order and shader choices are documented
// in docs/re/renderer.md; anything not yet confirmed against the game is marked there.
#include "render/environment.h"
#include "render/math.h"
#include "render/shader_lib.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <memory>
#include <string>
#include <vector>

namespace we {

class Vfs;
class TestScene;

struct Camera {
    float3 eye{0, 30, -80}, target{0, 5, 0};
    float fov_y = 0.8f, znear = 0.5f, zfar = 6000.0f;
};

class Renderer {
public:
    enum class View { Final, HdrRaw, Albedo, Normal, Specular, Depth, SkyCube };

    Renderer(ID3D11Device* dev, ID3D11DeviceContext* ctx, Vfs& vfs);
    ~Renderer();

    void render(const TestScene* scene, const Camera& cam, const EnvironmentParams& env, int width, int height, float time_s);
    ID3D11ShaderResourceView* output() const { return out_srv_.Get(); }
    View view = View::Final;

    ConstantBuffers& cbuffers() { return cbs_; }
    const std::vector<std::string>& errors() const { return errors_; }

private:
    struct Target {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> tex;
        Microsoft::WRL::ComPtr<ID3D11RenderTargetView> rtv;
        Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> srv;
    };
    void resize(int w, int h);
    Target make_target(int w, int h, DXGI_FORMAT fmt);
    void make_states();
    void set_camera(const float4x4& view, const float4x4& proj, const float3& eye, int w, int h, float time_s);
    void pass_sky_cube(const EnvironmentParams& env, float time_s);
    void pass_gbuffer(const TestScene* scene);
    void pass_sky_forward();
    void pass_directional_light();
    void pass_display();
    void fullscreen(const GameShader& ps);
    const GameShader* shader(const std::string& name);
    void error(const std::string& e);

    ID3D11Device* dev_;
    ID3D11DeviceContext* ctx_;
    Vfs& vfs_;
    ConstantBuffers cbs_;
    std::unique_ptr<ShaderLibrary> lib_;
    std::vector<std::string> errors_;

    int w_ = 0, h_ = 0;
    Target gb_[5];   // gbuffer channels 1..4 + per-instance id
    Target hdr_, out_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> out_srv_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> depth_tex_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilView> dsv_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> depth_srv_;

    static constexpr int kSkyCube = 256;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> sky_cube_;
    Microsoft::WRL::ComPtr<ID3D11RenderTargetView> sky_face_rtv_[6];
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> sky_cube_srv_;

    Microsoft::WRL::ComPtr<ID3D11Buffer> cube_vb_, cube_ib_, instance_vb_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> instance_ids_tex_;
    Microsoft::WRL::ComPtr<ID3D11ShaderResourceView> instance_ids_srv_, white_srv_, black_srv_, shadow_srv_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> shadow_tex_;
    Microsoft::WRL::ComPtr<ID3D11VertexShader> fs_vs_;
    Microsoft::WRL::ComPtr<ID3D11PixelShader> display_ps_;
    Microsoft::WRL::ComPtr<ID3D11Buffer> display_cb_;

    Microsoft::WRL::ComPtr<ID3D11SamplerState> s_wrap_aniso_, s_clamp_linear_, s_clamp_point_, s_shadow_cmp_;
    Microsoft::WRL::ComPtr<ID3D11DepthStencilState> ds_gbuffer_, ds_sky_, ds_light_, ds_off_;
    Microsoft::WRL::ComPtr<ID3D11RasterizerState> rs_back_, rs_none_;
    Microsoft::WRL::ComPtr<ID3D11BlendState> bs_opaque_;
};

} // namespace we
