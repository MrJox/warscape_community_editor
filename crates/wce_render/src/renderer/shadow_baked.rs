//! Rome 2's baked shadows: the shadows past the cascades. A RenderDoc capture of a battle at Carthage (2026-11-01,
//! `rome2_carthage.rdc`) binds `g_tBakedShadowBuffer`, a 4096 x 4096 depth map, to every lit shader beside the cascades, with these
//! constants (`shadowmap_PS`; the light pass's code, e.g. directionallight `ps30_slm_highdef`):
//! - the map is the scene seen along the sun from above: a world point (x, y, z) (game units) maps to
//!   `uv = (Sx x + Shx y + Ox, Sz' z + Shz y + Oz')` with `Shx = Sx * dx / -dy`, `Shz = Sz' * dz / -dy` (d the light's direction: the point
//!   slides along the light down to height 0), and its depth is its height, `Sy y + Oy` with Sy = -1 / 439.3 (the highest point of a
//!   column wins); `g_vBakedShadowmapScale` = (Sx, Sz', Sy), `g_vBakedShadowmapOffset` = (Ox, Oz', Oy), `g_vBakedShadowmapShear` =
//!   (Shx, Shz, 1). Sx = 1 / 4272: 1.04 units a texel;
//! - a pixel `d` units from the camera takes it from `g_vBakedShadowmapParams.y` (238.7) on: `min(1, taps / 6 + saturate(d * .z + .w) +
//!   saturate(d * .x)^4)` with five comparison taps (LESS_EQUAL, linear), so it fades in from 238.7 to 286.4 (.z = -1 / 47.7, .w = 6) and
//!   out towards 1922 (.x); the cascades end at 341: shadows reach far past them. The editor had it off (its distance out of reach), so
//!   shadows ended where the cascades do.
//! - Its content: the buildings' and the trees' casters, not the terrain (most texels are empty in the capture).
//!
//! The game scrolls one map around the camera (its sampler wraps; the offsets are toroidal). The editor renders the map centred on the
//! camera (on a 128-unit grid) and again when the camera leaves that cell, the scene or the sun changes or models finish loading.

use wce_foundation::math::{Float3, Float4x4, translation};
use wce_foundation::wce_prof;
use wce_scene::camera::Camera;
use wce_scene::render_scene::RenderScene;

use super::*;
use crate::draw_set::{Frustum, View as DrawView, Visible};
use crate::shader_lib::{bind_sampler, bind_srv};

/// The map's size (the game's) and the units it covers (the capture's scale, 1 / 0.00023408).
const BAKED_SIZE: u32 = 4096;
const BAKED_SPAN: f32 = 4272.0;

/// Height to depth (the capture's): depth = BAKED_DEPTH_SCALE * y + BAKED_DEPTH_OFFSET, heights from -23 to 416 units over the sea.
const BAKED_DEPTH_SCALE: f32 = -0.002_276_255_3;
const BAKED_DEPTH_OFFSET: f32 = 0.946_718;

/// `g_vBakedShadowmapParams` (the capture's): the far fade, the start, the near fade's slope and intercept.
const BAKED_PARAMS: [f32; 4] = [0.000_520_183, 238.7, -0.020_946_795, 6.0];

/// The camera moves this far (game units) before the map is made again around it.
const BAKED_CELL: f32 = 128.0;

#[derive(Default)]
pub(crate) struct BakedShadow {
    pub dsv: Option<ID3D11DepthStencilView>,
    pub srv: Option<ID3D11ShaderResourceView>,

    /// What the map was made for (`None`: nothing yet).
    key: Option<(u64, u64, [i32; 2], [u32; 3], [u32; 3], u32, usize)>,

    /// The map's centre (absolute, scene units) and the light's direction it was made with.
    centre: Float3,
    dir: Float3,

    /// The map holds this frame's casters.
    pub valid: bool,
}

impl BakedShadow {
    fn make(&mut self, dev: &ID3D11Device) {
        let td = D3D11_TEXTURE2D_DESC {
            Width: BAKED_SIZE, Height: BAKED_SIZE, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32_TYPELESS, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0, MiscFlags: 0,
        };

        let Ok(tex) = dev.create_texture_2d(&td, None) else { return };
        let dd = D3D11_DEPTH_STENCIL_VIEW_DESC { Format: DXGI_FORMAT_D32_FLOAT, ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D, ..Default::default() };
        let mut sd = D3D11_SHADER_RESOURCE_VIEW_DESC { Format: DXGI_FORMAT_R32_FLOAT, ViewDimension: D3D11_SRV_DIMENSION(D3D11_SRV_DIMENSION_TEXTURE2D), ..Default::default() };
        sd.Anonymous.Texture2D.MipLevels = 1;
        self.dsv = dev.create_depth_stencil_view(&tex, Some(&dd)).ok();
        self.srv = dev.create_shader_resource_view(&tex, Some(&sd)).ok();
    }
}

/// The map's constants for positions relative to `origin` (game units): scale, offset, shear (`shadowmap_PS`).
fn baked_constants(centre: Float3, dir: Float3, origin: Float3, units: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let s = 1.0 / BAKED_SPAN;
    let c = (centre - origin) * units;
    let dy = dir.y.min(-0.05);
    let shear = [s * dir.x / -dy, s * dir.z / -dy, 1.0];
    ([s, s, BAKED_DEPTH_SCALE], [0.5 - s * c.x, 0.5 - s * c.z, BAKED_DEPTH_OFFSET], shear)
}

impl Renderer {
    /// Binds the baked shadow map (white, all lit, without it) and its comparison sampler.
    pub(crate) fn bind_baked_shadow(&self, ps: &GameShader) {
        let srv = if self.baked.valid { self.baked.srv.as_ref() } else { self.res.white_srv.as_ref() };
        bind_srv(&self.ctx, ps, "g_tBakedShadowBuffer", srv);
        bind_sampler(&self.ctx, ps, "sBakedShadowBuffer", self.states.s_shadow_cmp.as_ref());
    }

    /// The baked shadows' constants for positions relative to the render origin moved by `shift` (scene units; the sea's frame).
    pub(crate) fn set_baked_constants(&mut self, shift: Float3) {
        if !self.baked.valid {
            self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapParams", &[0.0, 1.0e9, 0.0, 1.0]);
            return;
        }

        let (scale, offset, shear) = baked_constants(self.baked.centre, self.baked.dir, self.camera.origin + shift, self.camera.units);
        self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapScale", &scale);
        self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapOffset", &offset);
        self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapShear", &shear);
        self.cbs.set_f("shadowmap_PS", "g_vBakedShadowmapParams", &BAKED_PARAMS);
    }

    /// Makes the baked map again when what it shows changed (Rome 2, shadows on), and sets its constants.
    pub(crate) fn pass_baked_shadows(&mut self, scene: &RenderScene, cam: &Camera, light_dir: Float3) {
        if self.game != game::RenderGame::Rome2 || !self.settings.shadows || self.env.active {
            self.baked.valid = false;
            self.set_baked_constants(Float3::default());
            return;
        }

        let u = self.camera.units;
        let cell = BAKED_CELL / u;
        let snapped = [(cam.eye.x / cell).round() as i32, (cam.eye.z / cell).round() as i32];
        let o = self.camera.origin;
        let key = (
            scene.revision(), scene.structure_revision(), snapped, [light_dir.x.to_bits(), light_dir.y.to_bits(), light_dir.z.to_bits()],
            [o.x.to_bits(), o.y.to_bits(), o.z.to_bits()], u.to_bits(), self.draw_lists.waiting,
        );

        if self.baked.valid && self.baked.key == Some(key) {
            self.set_baked_constants(Float3::default());
            return;
        }

        if self.baked.dsv.is_none() {
            self.baked.make(&self.dev);
        }

        let (Some(dsv), Some(vs), Some(ps)) = (self.baked.dsv.clone(), self.shader(SHADOW_VS), self.shader(SHADOW_PS)) else {
            self.baked.valid = false;
            self.set_baked_constants(Float3::default());
            return;
        };

        let Some(layout) = self.layout(&vs, "rigid_std", &rigid_layout()) else { return };
        wce_prof!("render.shadows_baked");
        self.baked.centre = Float3::new(snapped[0] as f32 * cell, 0.0, snapped[1] as f32 * cell);
        self.baked.dir = light_dir;
        self.baked.key = Some(key);

        // The clip transform of game units relative to the render origin: x = 2 u - 1, y = 1 - 2 v, z = the depth.
        let (scale, offset, shear) = baked_constants(self.baked.centre, light_dir, o, u);
        let mut g = Float4x4::IDENTITY;
        g.m[0] = [2.0 * scale[0], 2.0 * shear[0], 0.0, 2.0 * offset[0] - 1.0];
        g.m[1] = [0.0, -2.0 * shear[1], -2.0 * scale[1], 1.0 - 2.0 * offset[1]];
        g.m[2] = [0.0, scale[2], 0.0, offset[2]];
        g.m[3] = [0.0, 0.0, 0.0, 1.0];

        // In scene units (as the cascades' `light_rel`), and absolute for culling.
        let mut rel = g;
        for r in &mut rel.m {
            for v in r.iter_mut().take(3) {
                *v *= u;
            }
        }

        let abs = rel * translation(-o);
        self.ctx.set_render_targets(&[], Some(&dsv));
        self.ctx.clear_depth_stencil_view(&dsv, D3D11_CLEAR_DEPTH, 1.0, 0);
        self.ctx.set_viewports(&[viewport(0.0, 0.0, BAKED_SIZE as f32, BAKED_SIZE as f32)]);
        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_shadow_rome2.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.cbs.set_m("camera_VS_PS", "view_projection", game_clip(&rel, u).data());
        self.upload();
        self.bind(&vs);
        self.bind(&ps);
        for &slot in vs.textures.values() {
            self.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&self.res.instance_ids_srv));
        }

        let (terrain_draws, draws) = (std::mem::take(&mut self.draw_lists.terrain_draws), std::mem::take(&mut self.draw_lists.draws));
        let (mut vis_terrain, mut vis) = (std::mem::take(&mut self.draw_lists.vis_terrain), std::mem::take(&mut self.draw_lists.vis));
        let texel = BAKED_SPAN / BAKED_SIZE as f32 / u;
        let view = DrawView {
            frustum: Frustum::from_view_projection(&abs),
            eye: cam.eye,
            shadow: true,
            coarser: 3,
            lod_scale: self.settings.lod_scale,
            min_radius: texel * self.settings.shadow_min_texels,
            quality: self.lod_quality(),
            ..Default::default()
        };

        terrain_draws.cull(&view, &mut vis_terrain);
        draws.cull(&view, &mut vis);

        // Not the ground (the terrain's own patches): the capture's map holds the models alone.
        if self.terrain.active {
            vis_terrain = Visible::default();
        }

        let firsts = self.upload_instances([&terrain_draws, &draws], [&vis_terrain, &vis]);
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[(size_of::<RigidInstance>() as u32)], &[0]);
        let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |_, m| !m.kind.is_tree() && m.kind != MeshKind::Water);
        self.draw_stats.shadow_instances += instances;
        self.draw_stats.shadow_triangles += triangles;

        // The trees, alpha-tested on their diffuse map.
        let tree_shaders = TREE_SHADOW_PASSES.map(|(kind, tvs, tps)| (kind, self.shader(tvs).zip(self.shader(tps))));
        let tree_layout = self.shader(TREE_BRANCH_VS).and_then(|tvs| self.layout(&tvs, "tree", &tree_layout(self.game)));
        let billboard_layout = self.shader(TREE_BILLBOARD_VS).and_then(|bvs| self.layout(&bvs, "tree_billboard", &billboard_layout()));
        for (kind, pair) in &tree_shaders {
            let Some((tvs, tps)) = pair else { continue };
            let kind_layout = if *kind == MeshKind::TreeBillboard { &billboard_layout } else { &tree_layout };
            let Some(kind_layout) = kind_layout else { continue };
            self.ctx.set_input_layout(Some(kind_layout));
            self.bind(tvs);
            self.bind(tps);
            bind_srv(&self.ctx, tps, "s_dissolve_map", self.res.white_srv.as_ref());
            ps_samplers(&self.ctx, tps, &self.states.s_wrap_linear);
            let (instances, triangles) = self.draw_batches([&terrain_draws, &draws], [&vis_terrain, &vis], firsts, |r, m| {
                if m.kind != *kind {
                    return false;
                }

                let srv = r.vegetation_diffuse(m);
                bind_srv(&r.ctx, tps, "s_diffuse_map", srv.as_ref());
                bind_srv(&r.ctx, tps, "s_diffuse_map_tree", srv.as_ref());
                true
            });

            self.draw_stats.shadow_instances += instances;
            self.draw_stats.shadow_triangles += triangles;
        }

        self.draw_lists.terrain_draws = terrain_draws;
        self.draw_lists.draws = draws;
        self.draw_lists.vis_terrain = vis_terrain;
        self.draw_lists.vis = vis;
        self.ctx.set_render_targets(&[], None);
        self.baked.valid = true;
        self.set_baked_constants(Float3::default());
    }
}
