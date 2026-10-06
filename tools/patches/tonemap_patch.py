"""One-off source patch: tone mapping chain in the renderer (kept for history; applied once)."""
import os
import sys

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")


def edit(rel, old, new):
    p = os.path.join(ROOT, rel)
    s = open(p, "rb").read().decode("utf-8")
    nl = "\r\n" if "\r\n" in s else "\n"
    o, n = old.replace("\n", nl), new.replace("\n", nl)
    if o not in s:
        sys.exit(f"not found in {rel}: {old[:60]!r}")
    open(p, "wb").write(s.replace(o, n, 1).encode("utf-8"))


R = "src/render/renderer.cpp"

edit(R, 'const char* kDirLightPS = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef";\n',
'''const char* kDirLightPS = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_directionallight.hlsl_ps_ps30_slm_highdef";
#define TM "dx11_sm_3_0_hdr_tone_mapper.hlsl_ps_ps_"
const char* kTmDownLuma = TM "downsample_image_store_luma";
const char* kTmLumaStats = TM "hdr_luma_to_quarter_size_luma_stats";
const char* kTmStatsDown = TM "downsample_quarter_size_luma_stats";
const char* kTmPoints = TM "determine_black_and_white_points";
const char* kTmHighPass = TM "high_pass_luminance_cutoff";
const char* kTmBlurX = TM "blur_x";
const char* kTmBlurY = TM "blur_y";
const char* kTmToneMap = TM "constant_tone_mapper_with_bloom"; // linear out; hdr_to_screen applies gamma
#undef TM
const char* kToScreenPS = "dx11_sm_3_0_hdr_to_screen.hlsl_ps_ps30_main_high";
''')

edit(R, '''    out_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    out_srv_ = out_.srv;
''', '''    out_ = make_target(w, h, DXGI_FORMAT_R8G8B8A8_UNORM);
    out_srv_ = out_.srv;

    auto sized = [&](int tw, int th, DXGI_FORMAT f) {
        SizedTarget s;
        s.w = std::max(1, tw);
        s.h = std::max(1, th);
        s.t = make_target(s.w, s.h, f);
        return s;
    };
    int qw = (w + 3) / 4, qh = (h + 3) / 4;
    quarter_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT); // rgb + luma in .w
    bloom_a_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
    bloom_b_ = sized(qw, qh, DXGI_FORMAT_R16G16B16A16_FLOAT);
    ldr_ = sized(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);     // tone-mapped, linear
    stats_.clear();
    int sw = qw, sh = qh;
    do {
        sw = (sw + 3) / 4;
        sh = (sh + 3) / 4;
        stats_.push_back(sized(sw, sh, DXGI_FORMAT_R32G32B32A32_FLOAT)); // min, max, avg, max
    } while (sw > 1 || sh > 1);
''')

edit(R, "    // Faction colours: white = no tint on the test map.\n", '''    // Black/white points: two 1x1 targets (previous/current) plus CPU readback for the constant tone mapper.
    for (int i = 0; i < 2; ++i) {
        points_[i].w = points_[i].h = 1;
        points_[i].t = make_target(1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT);
        D3D11_TEXTURE2D_DESC sd{1, 1, 1, 1, DXGI_FORMAT_R32G32B32A32_FLOAT, {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ};
        dev_->CreateTexture2D(&sd, nullptr, &points_staging_[i]);
    }
    {
        D3D11_TEXTURE2D_DESC sd{256, 1, 1, 1, DXGI_FORMAT_R8G8B8A8_UNORM, {1, 0}, D3D11_USAGE_DEFAULT, D3D11_BIND_SHADER_RESOURCE};
        dev_->CreateTexture2D(&sd, nullptr, &scurve_tex_);
        dev_->CreateShaderResourceView(scurve_tex_.Get(), nullptr, &scurve_srv_);
    }

    // Faction colours: white = no tint on the test map.
''')

edit(R, "void Renderer::pass_display() {\n", r'''void Renderer::fullscreen_to(ID3D11RenderTargetView* rtv, int w, int h, const GameShader& ps) {
    ctx_->OMSetRenderTargets(1, &rtv, nullptr);
    D3D11_VIEWPORT vp{0, 0, float(w), float(h), 0, 1};
    ctx_->RSSetViewports(1, &vp);
    cbs_.set_f("render_target_dims_buffer", "g_render_target_recip_size", {1.0f / w, 1.0f / h});
    cbs_.upload(dev_, ctx_);
    fullscreen(ps);
    ID3D11ShaderResourceView* nulls[8] = {};
    ctx_->PSSetShaderResources(0, 8, nulls);
}

void Renderer::pass_tonemap(const EnvironmentParams& env, float dt) {
    const GameShader* down = shader(kTmDownLuma);
    const GameShader* stats = shader(kTmLumaStats);
    const GameShader* stats_down = shader(kTmStatsDown);
    const GameShader* points = shader(kTmPoints);
    const GameShader* high = shader(kTmHighPass);
    const GameShader* bx = shader(kTmBlurX);
    const GameShader* by = shader(kTmBlurY);
    const GameShader* tm = shader(kTmToneMap);
    if (!down || !stats || !stats_down || !points || !high || !bx || !by || !tm) return;
    ctx_->OMSetDepthStencilState(ds_off_.Get(), 0);
    ctx_->RSSetState(rs_none_.Get());
    auto bind = [&](const GameShader& s, ID3D11SamplerState* st) {
        bind_shader(ctx_, s, cbs_);
        for (auto& [n, slot] : s.samplers) ctx_->PSSetSamplers(slot, 1, &st);
    };

    // 1. quarter-size copy with luma in .w
    bind(*down, s_clamp_linear_.Get());
    bind_srv(ctx_, *down, "g_hdr_rgb_texture_sampler", hdr_.srv.Get());
    fullscreen_to(quarter_.t.rtv.Get(), quarter_.w, quarter_.h, *down);

    // 2. luma statistics pyramid down to 1x1 (min, max, average), clamped by the absolute points
    ID3D11ShaderResourceView* src = quarter_.t.srv.Get();
    for (size_t i = 0; i < stats_.size(); ++i) {
        const GameShader& s = i == 0 ? *stats : *stats_down;
        bind(s, s_clamp_point_.Get());
        bind_srv(ctx_, s, i == 0 ? "g_luma_values_texture_sampler" : "g_luma_stats_texture_sampler", src);
        fullscreen_to(stats_[i].t.rtv.Get(), stats_[i].w, stats_[i].h, s);
        src = stats_[i].t.srv.Get();
    }

    // 3. black/white points with temporal adaptation (prev -> cur); the first frame snaps.
    int prev = points_cur_, cur = 1 - points_cur_;
    cbs_.set_f("auto_exposure_buffer", "g_auto_exposure_dt", {frame_ == 0 ? 1000.0f : dt});
    bind(*points, s_clamp_point_.Get());
    bind_srv(ctx_, *points, "g_prev_black_and_white_points_sampler", points_[prev].t.srv.Get());
    bind_srv(ctx_, *points, "g_luma_stats_texture_sampler", stats_.back().t.srv.Get());
    fullscreen_to(points_[cur].t.rtv.Get(), 1, 1, *points);
    points_cur_ = cur;
    // The constant tone mapper takes the points from a cbuffer: read back the previous frame's result.
    ctx_->CopyResource(points_staging_[frame_ & 1].Get(), points_[cur].t.tex.Get());
    if (frame_ > 0) {
        D3D11_MAPPED_SUBRESOURCE ms;
        auto* st = points_staging_[(frame_ + 1) & 1].Get();
        if (SUCCEEDED(ctx_->Map(st, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT, &ms))) {
            const float* p = static_cast<const float*>(ms.pData);
            if (p[1] > p[0]) {
                luma_black_ = p[0];
                luma_white_ = p[1];
            }
            ctx_->Unmap(st, 0);
        }
    }
    ++frame_;
    cbs_.set_f("constant_tone_mapping_buffer", "g_luma_black_point", {luma_black_});
    cbs_.set_f("constant_tone_mapping_buffer", "g_luma_white_point", {luma_white_});

    // 4. bloom: high pass on the quarter image, separable 11-tap blur
    bind(*high, s_clamp_point_.Get());
    bind_srv(ctx_, *high, "g_black_and_white_points_sampler", points_[cur].t.srv.Get());
    bind_srv(ctx_, *high, "g_hdr_rgb_texture_sampler", quarter_.t.srv.Get());
    fullscreen_to(bloom_a_.t.rtv.Get(), bloom_a_.w, bloom_a_.h, *high);
    bind(*bx, s_clamp_linear_.Get());
    bind_srv(ctx_, *bx, "g_hdr_rgb_texture_sampler", bloom_a_.t.srv.Get());
    fullscreen_to(bloom_b_.t.rtv.Get(), bloom_b_.w, bloom_b_.h, *bx);
    bind(*by, s_clamp_linear_.Get());
    bind_srv(ctx_, *by, "g_hdr_rgb_texture_sampler", bloom_b_.t.srv.Get());
    fullscreen_to(bloom_a_.t.rtv.Get(), bloom_a_.w, bloom_a_.h, *by);

    // 5. tone map with bloom and the S-curve
    uint8_t curve[256 * 4];
    build_scurve(env, curve);
    if (std::memcmp(curve, scurve_last_, sizeof curve) != 0) {
        ctx_->UpdateSubresource(scurve_tex_.Get(), 0, nullptr, curve, 256 * 4, 0);
        std::memcpy(scurve_last_, curve, sizeof curve);
    }
    bind(*tm, s_clamp_linear_.Get());
    bind_sampler(ctx_, *tm, "g_hdr_rgb_texture_sampler", s_clamp_point_.Get());
    bind_srv(ctx_, *tm, "g_hdr_rgb_texture_sampler", hdr_.srv.Get());
    bind_srv(ctx_, *tm, "g_hdr_rgb_bloom_texture_sampler", bloom_a_.t.srv.Get());
    bind_srv(ctx_, *tm, "g_scurve_texture_sampler", scurve_srv_.Get());
    fullscreen_to(ldr_.t.rtv.Get(), ldr_.w, ldr_.h, *tm);
}

void Renderer::pass_to_screen() {
    const GameShader* ps = shader(kToScreenPS);
    if (!ps) return;
    float zero2[2] = {0, 0};
    cbs_.set("fog_of_war_PS", "g_fog_of_war_blend", zero2, 8); // battle: no fog of war
    bind_shader(ctx_, *ps, cbs_);
    for (auto& [n, slot] : ps->samplers) ctx_->PSSetSamplers(slot, 1, s_clamp_point_.GetAddressOf());
    bind_srv(ctx_, *ps, "frame_sampler", ldr_.t.srv.Get());
    bind_srv(ctx_, *ps, "gbuffer_channel_4_sampler", gb_[3].srv.Get());
    bind_srv(ctx_, *ps, "s_fog_of_war_mask", black_srv_.Get());
    fullscreen_to(out_.rtv.Get(), w_, h_, *ps);
}

void Renderer::pass_display() {
''')

edit(R, '''    ID3D11ShaderResourceView* nulls[16] = {};
    ctx_->PSSetShaderResources(0, 16, nulls);
    pass_display();
''', '''    ID3D11ShaderResourceView* nulls[16] = {};
    ctx_->PSSetShaderResources(0, 16, nulls);
    float dt = last_time_ < 0 ? 0.016f : std::max(0.0f, time_s - last_time_);
    last_time_ = time_s;
    pass_tonemap(env, dt);
    if (view == View::Final) pass_to_screen();
    else pass_display();
''')

edit(R, "    case View::SkyCube: dc.mode = 5; break;\n", '''    case View::SkyCube: dc.mode = 5; break;
    case View::Bloom: dc.mode = 1; src = bloom_a_.t.srv.Get(); break;
''')

edit(R, "    for (const char* s : {kSkyVS, kSkyGenPS, kSkyPS, kRigidVS, kRigidGbufferPS, kRigidGbufferAlphaPS, kDirLightPS}) shader(s);\n",
'''    for (const char* s : {kSkyVS, kSkyGenPS, kSkyPS, kRigidVS, kRigidGbufferPS, kRigidGbufferAlphaPS, kDirLightPS, kTmDownLuma,
                          kTmLumaStats, kTmStatsDown, kTmPoints, kTmHighPass, kTmBlurX, kTmBlurY, kTmToneMap, kToScreenPS})
        shader(s);
''')

edit(R, "#include <cstring>\n", "#include <algorithm>\n#include <cstring>\n")
edit("src/render/renderer.h", "    uint64_t frame_ = 0;\n", "    uint64_t frame_ = 0;\n    float last_time_ = -1;\n")
edit("src/editor/app.cpp", '"Normal", "Specular", "Depth", "Sky cube"};', '"Normal", "Specular", "Depth", "Sky cube", "Bloom"};')
edit("src/editor/app.cpp", '"Final (temp. tone map)"', '"Final"')
print("patched")
