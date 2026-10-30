//! Resources the passes share and never change: the editor's helper shaders, fixed geometry and constant textures.

use wce_d3d11::*;

use super::{SUN_MESH_SIZE, compile, f32_bytes, immutable_buffer, immutable_srv, solid_srv};
use crate::RenderError;

/// Editor-side helpers (not game shaders): the full-screen triangle and the debug display pass.
const HELPER_HLSL: &str = r#"
struct VSOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VSOut fs_vs(uint id : SV_VertexID) {
    VSOut o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.pos = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
// Same, with a second texcoord packed into the same register (ssao.hlsl pixel_resolve expects TEXCOORD1.zw).
struct VSOut2 { float4 pos : SV_Position; float2 uv : TEXCOORD0; float2 uv1 : TEXCOORD1; };
VSOut2 fs_vs2(uint id : SV_VertexID) {
    VSOut2 o;
    o.uv = float2((id << 1) & 2, id & 2);
    o.uv1 = o.uv;
    o.pos = float4(o.uv * float2(2, -2) + float2(-1, 1), 0, 1);
    return o;
}
cbuffer display : register(b0) { int mode; float exposure; float2 pad; };
Texture2D src : register(t0);
TextureCube cube : register(t1);
SamplerState smp : register(s0);
float4 display_ps(VSOut i) : SV_Target {
    float4 c = src.SampleLevel(smp, i.uv, 0);
    if (mode == 0) { // TEMPORARY until the game's tone mapper chain is wired: simple exposure + gamma
        float3 x = c.rgb * exposure;
        x = x / (1 + x);
        return float4(pow(saturate(x), 1 / 2.2), 1);
    }
    if (mode == 1) return float4(pow(saturate(c.rgb), 1 / 2.2), 1);
    if (mode == 2) return float4(c.rgb * c.rgb, 1);          // albedo is stored as sqrt
    if (mode == 3) return float4(c.rgb, 1);
    if (mode == 4) return float4(frac(c.rrr * 50), 1);
    float2 a = i.uv * float2(6.2831853, 3.14159265);
    float3 d = float3(sin(a.y) * sin(a.x), cos(a.y), sin(a.y) * cos(a.x));
    return float4(pow(saturate(cube.SampleLevel(smp, d, 0).rgb), 1 / 2.2), 1);
}
"#;

#[derive(Clone, Debug, Default)]
pub(crate) struct SharedResources {
    /// The full-screen triangle; fs_vs2 also passes the uv as TEXCOORD1.
    pub fs_vs: Option<ID3D11VertexShader>,
    pub fs_vs2: Option<ID3D11VertexShader>,

    /// The debug display pass and its constants.
    pub display_ps: Option<ID3D11PixelShader>,
    pub display_cb: Option<ID3D11Buffer>,
    pub cube_vb: Option<ID3D11Buffer>,
    pub cube_ib: Option<ID3D11Buffer>,
    pub sun_vb: Option<ID3D11Buffer>,
    pub flare_vb: Option<ID3D11Buffer>,
    pub cloud_quad_vb: Option<ID3D11Buffer>,
    pub instance_ids_srv: Option<ID3D11ShaderResourceView>,
    pub white_srv: Option<ID3D11ShaderResourceView>,
    pub black_srv: Option<ID3D11ShaderResourceView>,
    pub overlay_none_srv: Option<ID3D11ShaderResourceView>,
    pub jitter_srv: Option<ID3D11ShaderResourceView>,

    /// The environment specular pass's `s_terrain` stand-in: the battle map's ground colour (the game binds the whole map's far diffuse,
    /// 8192 x 8192, which the editor does not build), the average of Rome's campaign map: (0.49, 0.43, 0.30), squared by the shader.
    pub ground_colour_srv: Option<ID3D11ShaderResourceView>,
}

impl SharedResources {

    /// # Errors
    ///
    /// Fails if the helper shaders don't compile.
    pub fn new(dev: &ID3D11Device) -> Result<Self, RenderError> {
        let vs = compile(HELPER_HLSL, "helpers", "fs_vs", "vs_5_0")?;
        let vs2 = compile(HELPER_HLSL, "helpers", "fs_vs2", "vs_5_0")?;
        let ps = compile(HELPER_HLSL, "helpers", "display_ps", "ps_5_0")?;
        let cb = D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER as u32, CPUAccessFlags: 0, MiscFlags: 0, StructureByteStride: 0 };
        let mut r = Self {
            fs_vs: dev.create_vertex_shader(&vs).ok(),
            fs_vs2: dev.create_vertex_shader(&vs2).ok(),
            display_ps: dev.create_pixel_shader(&ps).ok(),
            display_cb: dev.create_buffer(&cb, None).ok(),
            ..Default::default()
        };

        // The sky dome's geometry: a cube around the camera (the sky shaders only use the direction).
        let s = 10.0f32;
        let verts = [[-s, -s, -s], [s, -s, -s], [s, s, -s], [-s, s, -s], [-s, -s, s], [s, -s, s], [s, s, s], [-s, s, s]];
        let idx: [u16; 36] = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 6, 2, 3, 7, 6, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        r.cube_vb = immutable_buffer(dev, &f32_bytes(verts.as_flattened()), D3D11_BIND_VERTEX_BUFFER);
        r.cube_ib = immutable_buffer(dev, &idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), D3D11_BIND_INDEX_BUFFER);

        // The per-instance uint lookup read by rigidcommon vs30_main (t0, offset rigid_config_PS[4].w). Zeros for now.
        r.instance_ids_srv = immutable_srv(dev, 1024, 1, DXGI_FORMAT_R32_UINT, &[0; 4096]);
        r.white_srv = solid_srv(dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0xFFFF_FFFFu32.to_le_bytes());
        r.black_srv = solid_srv(dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0u32.to_le_bytes());

        // No overlay: colour 0, alpha 1 (the lighting of terrain pixels, material id bit 0, is overlay.rgb + lit * overlay.a).
        r.overlay_none_srv = solid_srv(dev, DXGI_FORMAT_R8G8B8A8_UNORM, &0xFF00_0000u32.to_le_bytes());

        // The sun billboard: two triangles; POSITION.xy in view units (x sun_scale), TEXCOORD.xy.
        let d = SUN_MESH_SIZE;
        let sun = [[-d, -d, 0.0, 1.0], [-d, d, 0.0, 0.0], [d, d, 1.0, 0.0], [-d, -d, 0.0, 1.0], [d, d, 1.0, 0.0], [d, -d, 1.0, 1.0]];
        r.sun_vb = immutable_buffer(dev, &f32_bytes(sun.as_flattened()), D3D11_BIND_VERTEX_BUFFER);

        // The lens flare / vignette quad (the engine's full-screen quad): POSITION xy in [-1, 1], COLOR, TEXCOORD0 uv.
        let q: [[f32; 10]; 6] = [
            [-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0], [1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        ];

        r.flare_vb = immutable_buffer(dev, &f32_bytes(q.as_flattened()), D3D11_BIND_VERTEX_BUFFER);

        // The cloud sprite quad: POSITION xy in [-1, 1] (scaled by the sprite's angular size), TEXCOORD0 uv.
        let cq = [[-1.0f32, -1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0], [-1.0, -1.0, 0.0, 1.0], [1.0, 1.0, 1.0, 0.0], [1.0, -1.0, 1.0, 1.0]];
        r.cloud_quad_vb = immutable_buffer(dev, &f32_bytes(cq.as_flattened()), D3D11_BIND_VERTEX_BUFFER);

        // The HBAO jitter texture, 256^2 (g_JitterResolution in the dump). UNORM, expanded with *2-1 by the shader: x, y = direction rotation
        // (cos, sin of 2*pi*r / num_dir), z = step jitter. The engine's generator was not found; the texture only decorrelates the sample
        // directions and the blur passes remove the pattern, so the NVIDIA HBAO sample's content (which this shader mirrors) is used.
        let mut px = vec![0u8; 256 * 256 * 4];
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };

        for texel in px.as_chunks_mut::<4>().0 {
            let a = std::f32::consts::TAU * rnd() / 5.0;
            let j = rnd();
            for (c, v) in [a.cos(), a.sin(), j].into_iter().enumerate() {
                texel[c] = ((v * 0.5 + 0.5) * 255.0).round() as u8;
            }

            texel[3] = 255;
        }

        r.jitter_srv = immutable_srv(dev, 256, 256, DXGI_FORMAT_R8G8B8A8_UNORM, &px);
        r.ground_colour_srv = solid_srv(dev, DXGI_FORMAT_R8G8B8A8_UNORM, &[126, 110, 75, 255]);

        Ok(r)
    }
}
