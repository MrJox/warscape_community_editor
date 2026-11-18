//! A campaign map's shroud plane (`shroud_plane.hlsl`; docs/plans/campaign_environment.md, Phase 4): a grid draped over the map's low-res
//! height map 1.1 world units above the ground, following the camera, drawn over the lit scene after the fog (RenderDoc, camp_attila
//! 2026-11-16: events 18699 and 18700, the same draw twice, alpha blended). Its pixel shader:
//! - alpha = the shroud's density (1 - `s_shroud`.a)^6.4, softened by the distance to the scene behind (min(d, 1)^1.35), plus a fade over
//!   the outer 2 % of `g_campaign_world_bounds`; below 1/255 the pixel is discarded;
//! - colour = the battle fog's formula (`fog_VS_PS`, the sky cube) over four layers of `s_shroud_cloud` scrolled by `time_in_sec`, lit by
//!   the sun from below the horizon.
//!
//! Fully revealed (`s_shroud` white, the editor's default: no faction's fog of war) the plane shows only at the map's edges.
//! The vertex shader (vs30_shroud_main): a vertex at round(position.xz + `g_quantised_camera_position`), its height the low-res map's
//! bilinear sample (uv = (x, world height - z) / `g_world_dimensions`) * `g_height_map_lf_scale` + 1 - `g_height_map_vertical_offset` +
//! 0.1. The game's grid: 256 x 256 vertices over +-500 (390 150 indices), the camera quantised by 1000 / 255 (floor).
//! DEVIATION, for review: the game's draw writes depth (LessEqual); here the depth is read only (the shader reads the scene's depth, which
//! the editor has no copy of while the plane draws).

use wce_foundation::math::Float3;
use wce_foundation::wce_prof;
use wce_scene::render_scene::{LfMap, RenderScene};

use super::*;
use crate::shader_lib::{bind_sampler, bind_srv};

const SHROUD_VS: &str = "dx11_sm_3_0_shroud_plane.hlsl_vs_vs30_shroud_main";
const SHROUD_PS: &str = "dx11_sm_3_0_shroud_plane.hlsl_ps_ps30_shroud_main";

/// The grid's vertices a side and half its width (world units).
const GRID: usize = 256;
const GRID_HALF: f32 = 500.0;

/// The shroud's cloud texture (TDD overrides it; `terrain/textures/common/`).
const SHROUD_CLOUD: &str = "terrain/textures/common/shroud_cloud.dds";

/// The low-res height map's samples a side at most (the game's is one a cell: 2032 x 1441 on Attila's map).
const LOW_RES_MAX: i32 = 2048;

/// The plane's GPU resources: the grid, the low-res height map (for an LF's id), the depth state.
#[derive(Default)]
pub(crate) struct ShroudGpu {
    vb: Option<ID3D11Buffer>,
    ib: Option<ID3D11Buffer>,
    index_count: u32,

    /// (LF id, its revision, the texture, samples a side, LF samples a texel).
    low_res: Option<(u64, u64, Option<ID3D11ShaderResourceView>, [i32; 2], i32)>,
    ds: Option<ID3D11DepthStencilState>,

    /// The fog of war on the GPU (its `ShroudMap::id`, the texture), and whether this frame draws one.
    fog: Option<(u64, Option<ID3D11ShaderResourceView>)>,
    pub(crate) fog_on: bool,
}

/// The fog of war texture's padding in uv (64 texels of 1485 x 1240: the game's `g_uv_offset`).
const FOG_UV_OFFSET: [f32; 2] = [64.0 / 1485.0, 64.0 / 1240.0];

impl Renderer {
    /// A frame's fog of war (`CampaignShroud::fog`): its texture uploaded when it changes and the shaders' shroud constants (`shroud_PS`:
    /// the map's bounds in the view's frame from its LF, the texture's padding). Every reader of `s_shroud` binds [`Self::shroud_srv`]
    /// and enables it by [`Self::shroud_enabled`].
    pub(crate) fn update_fog(&mut self, scene: &RenderScene) {
        let shroud = scene.campaign_shroud.as_ref().filter(|_| scene.campaign);
        let fog = shroud.and_then(|s| s.fog.clone());
        match &fog {
            Some(f) if self.shroud.fog.as_ref().is_none_or(|(id, _)| *id != f.id) => {
                let srv = immutable_srv(&self.dev, f.width, f.height, DXGI_FORMAT_A8_UNORM, &f.pixels);
                wce_foundation::log::info(format!(
                    "fog of war texture {} uploaded: {} ({} x {}, {} bytes)",
                    f.id,
                    srv.is_some(),
                    f.width,
                    f.height,
                    f.pixels.len()
                ));
                self.shroud.fog = Some((f.id, srv));
            }

            None => self.shroud.fog = None,
            _ => {}
        }

        self.shroud.fog_on = self.shroud.fog.as_ref().is_some_and(|(_, s)| s.is_some());
        let on = self.shroud_enabled();
        self.cbs.set_f("shroud_PS", "g_shroud_enabled", &[on]);
        if let Some(s) = shroud {
            let u = self.camera.units;
            let world = [s.lf.nx as f32 * s.lf.step * u, s.lf.nz as f32 * s.lf.step * u];
            let cbs = &mut self.cbs;
            cbs.set_f("shroud_PS", "g_campaign_world_bounds", &[0.0, 0.0, world[0], world[1]]);
            cbs.set_f("shroud_PS", "g_campaign_playable_area_bounds", &[0.0, 0.0, world[0], world[1]]);
            cbs.set_f("shroud_PS", "g_uv_offset", &FOG_UV_OFFSET);
        }
    }

    /// The fog of war texture (`s_shroud`), white (revealed) without one.
    pub(crate) fn shroud_srv(&self) -> Option<ID3D11ShaderResourceView> {
        self.shroud.fog.as_ref().and_then(|(_, s)| s.clone()).or_else(|| self.res.white_srv.clone())
    }

    /// `g_shroud_enabled` for the readers of [`Self::shroud_srv`]: 1 with a fog of war.
    pub(crate) fn shroud_enabled(&self) -> f32 {
        if self.shroud.fog_on { 1.0 } else { 0.0 }
    }

    /// The shroud plane of a campaign map (`RenderScene::campaign_shroud`), over the lit and fogged scene.
    pub(crate) fn pass_shroud_plane(&mut self, scene: &RenderScene) {
        let Some(shroud) = scene.campaign_shroud.as_ref() else { return };
        let lf = shroud.lf.clone();
        let (Some(vs), Some(ps)) = (self.shader(SHROUD_VS), self.shader(SHROUD_PS)) else { return };
        wce_prof!("render.shroud");
        self.make_shroud_grid();
        let Some((srv, dims, k)) = self.shroud_low_res(&lf) else { return };
        let Some(layout) = self.layout(&vs, "shroud_plane", &[element(c"POSITION", 0, DXGI_FORMAT_R32G32B32A32_FLOAT, 0, 0, false)]) else {
            return;
        };

        // The map's frame: its south-west corner (the LF's first sample) at 0, game units.
        let u = self.camera.units;
        let corner = Float3::new(lf.x0, self.camera.origin.y, lf.z0);
        let world = [dims[0] as f32 * k as f32 * lf.step * u, dims[1] as f32 * k as f32 * lf.step * u];
        let eye = (self.camera.eye - corner) * u;
        let q = 2.0 * GRID_HALF / (GRID - 1) as f32;
        self.shift_camera_constants(corner - self.camera.origin);

        let cbs = &mut self.cbs;
        cbs.set_f("shroud_plane_buffer", "g_quantised_camera_position", &[(eye.x / q).floor() * q, (eye.z / q).floor() * q]);
        cbs.set_f("vs_height_map_cb", "g_height_map_lf_scale", &[lf.scale * u]);
        cbs.set_f("vs_height_map_cb", "g_height_map_vertical_offset", &[lf.offset * u]);
        let n = [dims[0] as f32, dims[1] as f32];
        cbs.set_f("vs_height_map_cb", "g_height_map_dimension", &[n[0], n[1], 1.0 / n[0], 1.0 / n[1]]);
        cbs.set_f("vs_height_map_cb", "g_world_dimensions", &world);
        // Revealed: the shroud texture is white; the bounds are the map's, the uv offset the capture's (the shroud texture's padding).
        cbs.set_f("shroud_PS", "g_shroud_enabled", &[1.0]);
        cbs.set_f("shroud_PS", "g_campaign_world_bounds", &[0.0, 0.0, world[0], world[1]]);
        cbs.set_f("shroud_PS", "g_campaign_playable_area_bounds", &[0.0, 0.0, world[0], world[1]]);
        cbs.set_f("shroud_PS", "g_uv_offset", &FOG_UV_OFFSET);
        self.upload();

        let cloud = self.textures.get(SHROUD_CLOUD, true).and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let sky = self.sky_srv();
        let (ctx, st) = (&self.ctx, &self.states);
        ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv_read_depth().as_ref());
        ctx.set_depth_stencil_state(self.shroud.ds.as_ref(), 0);
        ctx.set_blend_state(st.bs_alpha.as_ref());
        ctx.set_rasterizer_state(st.rs_none.as_ref());
        ctx.set_input_layout(Some(&layout));
        ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        ctx.set_vertex_buffers(0, std::slice::from_ref(&self.shroud.vb), &[16], &[0]);
        ctx.set_index_buffer(self.shroud.ib.as_ref(), DXGI_FORMAT_R32_UINT, 0);
        self.bind(&vs);
        self.bind(&ps);
        let ctx = &self.ctx;
        bind_srv(ctx, &vs, "t_low_res_height_map", srv.as_ref());
        bind_srv(ctx, &ps, "s_shroud_cloud", cloud.as_ref().or(self.res.white_srv.as_ref()));
        let fog = self.shroud_srv();
        let ctx = &self.ctx;
        bind_srv(ctx, &ps, "s_shroud", fog.as_ref());
        bind_srv(ctx, &ps, "s_sky", sky.as_ref());
        bind_srv(ctx, &ps, "t_readable_depth_texture", self.gbuffer.depth_srv().as_ref());
        bind_sampler(ctx, &ps, "s_sky", self.states.s_clamp_linear.as_ref());
        bind_sampler(ctx, &ps, "s_shroud", self.states.s_clamp_linear.as_ref());
        bind_sampler(ctx, &ps, "s_shroud_cloud", self.states.s_wrap_linear.as_ref());

        // Twice, as the game draws it.
        for _ in 0..2 {
            ctx.draw_indexed(self.shroud.index_count, 0, 0);
        }

        self.draw_stats.draw_calls += 2;
        self.shift_camera_constants(Float3::default());
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        null_ps_srvs(&self.ctx, 8);
        self.upload();
    }

    /// The grid (once) and the plane's depth state.
    fn make_shroud_grid(&mut self) {
        if self.shroud.vb.is_some() {
            return;
        }

        let q = 2.0 * GRID_HALF / (GRID - 1) as f32;
        let mut verts = Vec::with_capacity(GRID * GRID * 4);
        for j in 0..GRID {
            for i in 0..GRID {
                verts.extend([-GRID_HALF + i as f32 * q, 0.0, -GRID_HALF + j as f32 * q, 1.0]);
            }
        }

        let mut idx: Vec<u32> = Vec::with_capacity((GRID - 1) * (GRID - 1) * 6);
        for j in 0..GRID - 1 {
            for i in 0..GRID - 1 {
                let a = (j * GRID + i) as u32;
                let (b, c, d) = (a + 1, a + GRID as u32, a + GRID as u32 + 1);
                idx.extend([a, c, b, b, c, d]);
            }
        }

        self.shroud.vb = immutable_buffer(&self.dev, bytemuck::cast_slice(&verts), D3D11_BIND_VERTEX_BUFFER);
        self.shroud.ib = immutable_buffer(&self.dev, bytemuck::cast_slice(&idx), D3D11_BIND_INDEX_BUFFER);
        self.shroud.index_count = idx.len() as u32;
        self.shroud.ds = self
            .dev
            .create_depth_stencil_state(&D3D11_DEPTH_STENCIL_DESC {
                DepthEnable: true.into(),
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                DepthFunc: D3D11_COMPARISON_LESS_EQUAL,
                ..Default::default()
            })
            .ok();
    }

    /// The LF's low-res height map: every k-th sample (k the fewest that keep it within `LOW_RES_MAX`), row 0 the north edge; made again
    /// when the LF's samples change.
    fn shroud_low_res(&mut self, lf: &LfMap) -> Option<(Option<ID3D11ShaderResourceView>, [i32; 2], i32)> {
        let revision = lf.values.revision();
        if let Some((id, rev, srv, dims, k)) = &self.shroud.low_res
            && *id == lf.id
            && *rev == revision
        {
            return Some((srv.clone(), *dims, *k));
        }

        let k = (lf.nx.max(lf.nz) + LOW_RES_MAX - 1) / LOW_RES_MAX;
        let (w, h) = (lf.nx / k.max(1), lf.nz / k.max(1));
        if w < 2 || h < 2 {
            return None;
        }

        let values = lf.values.read();
        if values.len() != (lf.nx * lf.nz) as usize {
            return None;
        }

        let mut tex = vec![0u16; (w * h) as usize];
        for (r, row) in tex.chunks_mut(w as usize).enumerate() {
            let j = ((h - 1 - r as i32) * k).min(lf.nz - 1);
            for (c, v) in row.iter_mut().enumerate() {
                *v = values[(j * lf.nx + (c as i32 * k).min(lf.nx - 1)) as usize];
            }
        }

        drop(values);
        let td = D3D11_TEXTURE2D_DESC {
            Width: w as u32,
            Height: h as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R16_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let sd = [InitData::new(bytemuck::cast_slice(&tex), (w * 2) as u32)];
        let srv = self.dev.create_texture_2d(&td, Some(&sd)).ok().and_then(|t| self.dev.create_shader_resource_view(&t, None).ok());
        self.shroud.low_res = Some((lf.id, revision, srv.clone(), [w, h], k));
        Some((srv, [w, h], k))
    }
}
