//! The sky and what lights the scene from it: the sky cube, the environment cube (with the engine ambient read back from it), the sun
//! disc with its occlusion queries, and the cloud planes.

use std::collections::BTreeMap;

use wce_scene::clouds::CloudField;

use std::f32::consts::PI;

use wce_foundation::math::{Float3, Float4x4, perspective_lh, translation};
use wce_scene::clouds::cloud_texture;
use wce_scene::environment::{EnvironmentParams, colour_to_linear};

use super::*;
use super::targets::GBuffer;
use crate::model_cache::half_to_float;
use crate::shader_lib::{bind_sampler, bind_srv};
use crate::texture::load_dds;

/// A square HDR cube map with a render target per face and mips (the shaders sample it at any roughness).
#[derive(Clone, Debug, Default)]
pub(crate) struct Cube {
    /// Its faces' size.
    pub size: u32,
    pub tex: Option<ID3D11Texture2D>,
    pub face_rtv: [Option<ID3D11RenderTargetView>; 6],
    pub srv: Option<ID3D11ShaderResourceView>,
}

impl Cube {
    pub(crate) fn new(dev: &ID3D11Device, size: u32) -> Self {
        let cd = D3D11_TEXTURE2D_DESC {
            Width: size, Height: size, MipLevels: 0, ArraySize: 6, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT, BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE) as u32, CPUAccessFlags: 0,
            MiscFlags: (D3D11_RESOURCE_MISC_TEXTURECUBE | D3D11_RESOURCE_MISC_GENERATE_MIPS) as u32,
        };

        let Ok(tex) = dev.create_texture_2d(&cd, None) else { return Self::default() };
        let face_rtv = std::array::from_fn(|f| {
            let mut rd = D3D11_RENDER_TARGET_VIEW_DESC { Format: cd.Format, ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DARRAY, ..Default::default() };
            rd.Anonymous.Texture2DArray.FirstArraySlice = f as u32;
            rd.Anonymous.Texture2DArray.ArraySize = 1;
            dev.create_render_target_view(&tex, Some(&rd)).ok()
        });

        Self { size, srv: dev.create_shader_resource_view(&tex, None).ok(), face_rtv, tex: Some(tex) }
    }
}

/// The sky cube's size at sky qualities 1 to 3 ("Sky Large CubeMap", fn 0x11905050); at 0 the game draws the sky through the 256 cube
/// of its environment map ("Sky Cube EnvMap RT"): `SKY_CUBE`.
pub(crate) const SKY_CUBE_LARGE: u32 = 1024;

/// The sky cube map (the output of ps30_skygen), and the sky textures (cloud maps, lens flare sprites) by path and colour space.
#[derive(Clone, Debug, Default)]
pub(crate) struct SkyCube {
    pub cube: Cube,
    pub textures: BTreeMap<String, Option<ID3D11ShaderResourceView>>,
}

impl SkyCube {
    pub fn new(dev: &ID3D11Device) -> Self {
        Self { cube: Cube::new(dev, SKY_CUBE_LARGE), textures: BTreeMap::new() }
    }
}

/// The environment cube ("EnvMapGen", fn 0x118e00e0): the whole scene rendered from 50 m above the origin into a cube; s_environment for
/// reflections and, through its face averages, the ambient cube of non-custom lighting.
#[derive(Clone, Debug, Default)]
pub(crate) struct EnvCube {
    pub cube: Cube,

    /// The G-buffer the cube's faces are drawn with (swapped with the main view's while they render).
    pub gbuffer: GBuffer,

    /// The cube's faces are being drawn.
    pub active: bool,

    /// The cube holds a rendered scene.
    pub valid: bool,

    /// The engine ambient (use_custom_ambient_cube_colours = false): the per-face averages of the cube (1x1 mip, read back through two
    /// alternating staging textures).
    pub ambient_staging: [Option<ID3D11Texture2D>; 2],
    pub ambient_faces: [[f32; 3]; 6],
    pub ambient_valid: bool,
    pub ambient_frame: u64,
}

impl EnvCube {
    pub fn new(dev: &ID3D11Device) -> Self {
        let staging = D3D11_TEXTURE2D_DESC {
            Width: 1, Height: 1, MipLevels: 1, ArraySize: 6, Format: DXGI_FORMAT_R16G16B16A16_FLOAT, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING, BindFlags: 0, CPUAccessFlags: D3D11_CPU_ACCESS_READ as u32, MiscFlags: 0,
        };

        Self {
            cube: Cube::new(dev, SKY_CUBE),
            gbuffer: GBuffer::new(dev, SKY_CUBE, SKY_CUBE),
            ambient_staging: std::array::from_fn(|_| dev.create_texture_2d(&staging, None).ok()),
            ..Default::default()
        }
    }
}

/// The sun disc's texture, and its visibility for the lens flare (lense_flare_VS.sun_occlusion): occlusion queries of the disc, read back
/// a few frames later.
#[derive(Clone, Debug, Default)]
pub(crate) struct SunGpu {
    pub queries: [Option<ID3D11Query>; 4],
    pub query_frame: u64,
    pub visibility: f32,
    pub srv: Option<ID3D11ShaderResourceView>,
    pub texture_path: String,
}

impl SunGpu {
    pub fn new(dev: &ID3D11Device) -> Self {
        Self { queries: std::array::from_fn(|_| dev.create_query(D3D11_QUERY_OCCLUSION).ok()), ..Default::default() }
    }
}

/// The cloud sprites of a plane on the GPU.
#[derive(Default)]
pub(crate) struct CloudGpu {
    pub signature: String,
    pub field: CloudField,
    pub instances: Option<ID3D11Buffer>,
    pub capacity: u32,
    pub count: u32,
}

/// The cloud planes of the environment, simulated and drawn as sprites.
#[derive(Default)]
pub(crate) struct CloudLayer {
    pub planes: Vec<CloudGpu>,

    /// The simulation step of this frame (0 while the environment cube renders).
    pub dt: f32,
}

impl Renderer {

    pub(crate) fn pass_sky_cube(&mut self, _env: &EnvironmentParams, time_s: f32) {
        let vs = self.shader(SKY_VS);
        let ps = self.shader(SKY_GEN_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        let proj = perspective_lh(PI / 2.0, 1.0, 0.1, 100.0);
        let Some(layout) = self.layout(&vs, "pos3", &sky_layout()) else { return };

        // gfx_sky_quality (Attila, Thrones of Britannia): the large cube above 0. Rome 2's sizes are not read: the old 256.
        let size = if self.game == super::game::RenderGame::Rome2 || self.settings.sky_quality == 0 { SKY_CUBE } else { SKY_CUBE_LARGE };
        if self.sky.cube.size != size {
            self.sky.cube = Cube::new(&self.dev, size);
        }

        self.ctx.set_viewports(&[viewport(0.0, 0.0, size as f32, size as f32)]);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_off.as_ref(), 0);
        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.res.cube_vb), &[12], &[0]);
        self.ctx.set_index_buffer(self.res.cube_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        for f in 0..6 {
            // Directions only: the camera at the render origin.
            self.set_camera(&face_view(f), &proj, self.camera.origin, size, size, time_s);
            self.upload();
            self.ctx.set_render_targets(&[self.sky.cube.face_rtv[f].clone()], None);
            self.bind(&vs);
            self.bind(&ps);
            self.ctx.draw_indexed(36, 0, 0);
        }

        self.ctx.set_render_targets(&[], None);
        if let Some(srv) = &self.sky.cube.srv {
            self.ctx.generate_mips(srv);
        }
    }

    /// EnvMapGen (fn 0x118e00e0): six 90-degree views from 50 m above the ground at the world origin (the battle's centre: `fStack_31c = 50`
    /// plus the terrain height there; (0, 108.15, 0) at Rome's campaign placement), far plane 972800; then every face's lowest mip is read
    /// back (readback_ambient). Each face is the sky dome and both cloud planes, nothing else: no scene, no lights, no fog, and black below
    /// the horizon (the dome is a half sphere). Settled 2026-10-30 (docs/re/renderer.md): a RenderDoc capture of the game's build at minimum
    /// settings shows those draws alone, and a memory dump at max settings gives the face averages of that cube (sides within 5-15%, top
    /// within 10%), not of a cube with the scene in it.
    pub(crate) fn render_env_cube(&mut self, env: &EnvironmentParams, time_s: f32, scene: &wce_scene::render_scene::RenderScene) {
        let n = SKY_CUBE;
        std::mem::swap(&mut self.gbuffer, &mut self.env.gbuffer);
        let save_ao = std::mem::replace(&mut self.post.ao_result, self.res.white_srv.clone());
        let save_dt = std::mem::take(&mut self.cloud_layer.dt);
        self.env.active = true;
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[0.0]);
        let proj = perspective_lh(PI / 2.0, 1.0, 1.0, 972_800.0);

        // 50 game metres above the ground at the battle's centre (the sea when no terrain covers it).
        let [cx, cz] = scene.battle_centre;
        let ground = scene.entities().iter()
            .filter(|e| scene.drawable(e))
            .find_map(|e| match &e.payload {
                wce_scene::render_scene::Payload::Terrain(t) => t.ground_at(&e.world, cx, cz),
                _ => None,
            })
            .unwrap_or(scene.sea_level);
        let eye = Float3::new(cx, ground + 50.0 / self.camera.units, cz);
        for f in 0..6 {
            let view = face_view(f) * translation(-(eye - self.camera.origin));
            self.set_camera(&view, &proj, eye, n, n, time_s);
            self.upload();
            self.ctx.set_viewports(&[viewport(0.0, 0.0, n as f32, n as f32)]);
            self.ctx.set_blend_state(self.states.bs_opaque.as_ref());

            // No sun disc (the game draws none into the cube).
            for rtv in self.gbuffer.rtvs().iter().flatten() {
                self.ctx.clear_render_target_view(rtv, [0.0; 4]);
            }

            if let Some(dsv) = &self.gbuffer.dsv {
                self.ctx.clear_depth_stencil_view(dsv, D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL, 1.0, 0);
            }

            if let Some(rtv) = &self.gbuffer.hdr.rtv {
                self.ctx.clear_render_target_view(rtv, [0.0; 4]);
            }

            self.pass_sky_forward();
            self.pass_clouds(env);
            self.ctx.set_render_targets(&[], None);

            // Below the horizon black: the bottom face, and the lower half of the side faces (their up is +y, the horizon the middle row).
            let (Some(cube), Some(hdr)) = (self.env.cube.tex.clone(), self.gbuffer.hdr.tex.clone()) else { continue };
            let half = D3D11_BOX { left: 0, top: 0, front: 0, right: n, bottom: n / 2, back: 1 };
            match f {
                2 => self.ctx.copy_subresource_region(&cube, f as u32 * 9, [0; 3], &hdr, 0, None),
                3 => {
                    if let Some(rtv) = &self.gbuffer.hdr.rtv {
                        self.ctx.clear_render_target_view(rtv, [0.0; 4]);
                    }

                    self.ctx.copy_subresource_region(&cube, f as u32 * 9, [0; 3], &hdr, 0, None);
                }

                _ => {
                    self.ctx.copy_subresource_region(&cube, f as u32 * 9, [0; 3], &hdr, 0, Some(&half));
                    if let Some(rtv) = &self.gbuffer.hdr.rtv {
                        self.ctx.clear_render_target_view(rtv, [0.0; 4]);
                    }

                    self.ctx.copy_subresource_region(&cube, f as u32 * 9, [0, n / 2, 0], &hdr, 0, Some(&half));
                }
            }
        }

        if let Some(srv) = &self.env.cube.srv {
            self.ctx.generate_mips(srv);
        }

        self.env.valid = true;
        self.cbs.set_f("shadowmap_PS", "g_iSplitCount", &[if self.settings.shadows { 4.0 } else { 0.0 }]);
        self.env.active = false;
        self.cloud_layer.dt = save_dt;
        self.post.ao_result = save_ao;
        std::mem::swap(&mut self.gbuffer, &mut self.env.gbuffer);
        null_ps_srvs(&self.ctx, 16);
        self.readback_ambient();
    }

    /// The face averages of the environment cube: copies each face's 1x1 mip into this frame's staging texture and reads the previous one.
    fn readback_ambient(&mut self) {
        // 256 -> 1.
        let mips = 9;
        let (Some(dst), Some(src), Some(cube)) = (self.env.ambient_staging[(self.env.ambient_frame & 1) as usize].clone(), self.env.ambient_staging[((self.env.ambient_frame + 1) & 1) as usize].clone(), self.env.cube.tex.clone()) else { return };
        for f in 0..6 {
            self.ctx.copy_subresource_region(&dst, f, [0; 3], &cube, (mips - 1) + f * mips, None);
        }

        let first = self.env.ambient_frame == 0;
        self.env.ambient_frame += 1;
        if first {
            return;
        }

        for f in 0..6u32 {
            let Some(h) = map_read(&self.ctx, &src, f, self.settings.deterministic).and_then(|ms| ms.read::<[u16; 3]>()) else { return };
            for (face, half) in self.env.ambient_faces[f as usize].iter_mut().zip(h) {
                *face = half_to_float(half);
            }
        }

        self.env.ambient_valid = true;
    }

    /// Verified (EnvMapGen fn 0x118e00e0, ambient apply fn 0x1178a300, dry dump): with use_custom_ambient_cube_colours = false the cube is the
    /// per-face average (lowest mip) of the environment cube rendered from 50 m above the origin, the bottom face replaced by 0.35 * luma(top) *
    /// (0.71, 0.69, 0.66) (fn 0x118f0980, 0.35 = engine variable), times ambient_scale; lr = (+x, -x), tb = (+y, bottom), fb = (+z, -z).
    pub(crate) fn apply_engine_ambient(&mut self, env: &EnvironmentParams) {
        if env.use_custom_ambient || !self.env.ambient_valid {
            return;
        }

        let mut faces = self.env.ambient_faces;
        let top = faces[2];
        let luma = 0.2126 * top[0] + 0.7152 * top[1] + 0.0722 * top[2];
        let tint = [0.71f32, 0.69, 0.66];
        for c in 0..3 {
            faces[3][c] = 0.35 * tint[c] * luma;
        }

        let s = env.ambient_scale;
        for (var, a, b) in [("ambient_cube_lr", 0, 1), ("ambient_cube_tb", 2, 3), ("ambient_cube_fb", 4, 5)] {
            self.cbs.set_f("lighting_VS_PS", var, &[faces[a][0] * s, faces[a][1] * s, faces[a][2] * s, 0.0, faces[b][0] * s, faces[b][1] * s, faces[b][2] * s]);
        }
    }

    pub(crate) fn pass_sky_forward(&mut self) {
        let vs = self.shader(SKY_VS);
        let ps = self.shader(SKY_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        let Some(layout) = self.layout(&vs, "pos3", &sky_layout()) else { return };
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.res.cube_vb), &[12], &[0]);
        self.ctx.set_index_buffer(self.res.cube_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        self.bind(&vs);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "s_sky", self.sky.cube.srv.as_ref());
        bind_sampler(&self.ctx, &ps, "s_sky", self.states.s_clamp_linear.as_ref());
        self.ctx.draw_indexed(36, 0, 0);
    }

    /// The sun disc, and its occlusion (fn 0x1927460, getter fn 0x118f50c0): visible = the occlusion-query sample count of the sun draw, total =
    /// int(kx / w * W / 2) * int(ky / w * H / 2) for the view-space corner (k, k, z) of the sun quad, k = sun_scale * 2000 (1 when w <= 0);
    /// sun_occlusion = clamp(min(visible, total) / total, 0, 1).
    pub(crate) fn pass_sun(&mut self, env: &EnvironmentParams, view_m: &Float4x4, proj: &Float4x4) {
        let vs = self.shader(SUN_VS);
        let ps = self.shader(SUN_PS);
        if !self.env.active {
            let old = self.sun.queries[((self.sun.query_frame + 1) % 4) as usize].clone();
            let mut visible = 0u64;
            let query_ready = |r: &Self, q: &ID3D11Query, visible: &mut u64| {
                let flags = if r.settings.deterministic { 0 } else { D3D11_ASYNC_GETDATA_DONOTFLUSH as u32 };
                loop {
                    match r.ctx.get_data::<u64>(q, flags) {
                        Ok(Some(v)) => {
                            *visible = v;
                            return true;
                        }

                        Ok(None) if r.settings.deterministic => {}
                        _ => return false,
                    }
                }
            };

            if self.sun.query_frame >= 3
                && let Some(old) = old
                && query_ready(self, &old, &mut visible)
            {
                let c = env.to_sun() * SUN_DISTANCE;
                let z = view_m.m[2][0] * c.x + view_m.m[2][1] * c.y + view_m.m[2][2] * c.z;
                let k = env.sun_scale * SUN_MESH_SIZE;
                let w = proj.m[3][0] * k + proj.m[3][1] * k + proj.m[3][2] * z + proj.m[3][3];
                let mut total = 1u64;
                if w > 0.0 {
                    let cx = proj.m[0][0] * k + proj.m[0][1] * k + proj.m[0][2] * z + proj.m[0][3];
                    let cy = proj.m[1][0] * k + proj.m[1][1] * k + proj.m[1][2] * z + proj.m[1][3];
                    let px = (cx / w * self.gbuffer.w as f32 * 0.5) as i64;
                    let py = (cy / w * self.gbuffer.h as f32 * 0.5) as i64;
                    total = (px * py).max(1) as u64;
                } else {
                    visible = 0;
                }

                self.sun.visibility = ((visible.min(total) as f64 / total as f64) as f32).clamp(0.0, 1.0);
            }

            self.sun.query_frame += 1;
        }

        let (Some(vs), Some(ps)) = (vs, ps).into_iter_if(env.has_sky && env.sun_scale > 0.0) else {
            if !self.env.active {
                self.sun.visibility = 0.0;
            }

            return;
        };

        let tex = format!("rigidmodels/skybox/{}", env.sun_texture);
        if tex != self.sun.texture_path {
            self.sun.texture_path = tex.clone();
            self.sun.srv = None;
            match self.files.read(&tex).and_then(|bytes| load_dds(&self.dev, &bytes, true).map_err(|error| error.to_string())) {
                Ok(texture) => self.sun.srv = texture.srv,
                Err(error) => self.error(format!("sun texture: {error}")),
            }
        }

        if self.sun.srv.is_none() {
            return;
        }

        // g_world_view: the camera rotation, translation = rotation * (to_sun * distance) (camera-relative).
        let mut wv = *view_m;
        let s = env.to_sun() * SUN_DISTANCE;
        for r in 0..3 {
            wv.m[r][3] = view_m.m[r][0] * s.x + view_m.m[r][1] * s.y + view_m.m[r][2] * s.z;
        }

        self.cbs.set_m("sun_VS", "g_world_view", wv.data());
        self.cbs.set_m("sun_VS", "g_projection", proj.data());
        self.cbs.set_f("sun_VS", "g_sun_scale", &[env.sun_scale]);
        self.upload();
        let el = [element(c"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false), element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, false)];
        let Some(layout) = self.layout(&vs, "sun", &el) else { return };
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&self.res.sun_vb), &[16], &[0]);
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);

        // The SUN_SHADER blend (fn 0x116fcf80 area, blend object +0x23b): SRC_ALPHA / INV_SRC_ALPHA on colour and alpha.
        self.ctx.set_blend_state(self.states.bs_sun.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        self.bind(&vs);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "s_diffuse", self.sun.srv.as_ref());

        // SUN_SHADER s_diffuse.
        ps_samplers(&self.ctx, &ps, &self.states.s_wrap_linear);
        // The sun object's occlusion query wraps its own draw.
        if self.env.active {
            self.ctx.draw(6, 0);
        } else if let Some(q) = self.sun.queries[(self.sun.query_frame % 4) as usize].clone() {
            self.ctx.begin(&q);
            self.ctx.draw(6, 0);
            self.ctx.end(&q);
        } else {
            self.ctx.draw(6, 0);
        }

        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
    }

    /// A sky texture (cloud maps, the lens flare sprites), cached; `None` (reported once) when it can't be loaded.
    pub(crate) fn sky_texture(&mut self, path: &str, srgb: bool) -> Option<ID3D11ShaderResourceView> {
        let key = format!("{path}{}", if srgb { "|srgb" } else { "|unorm" });
        if let Some(srv) = self.sky.textures.get(&key) {
            return srv.clone();
        }

        // s_diffuse_cloud_maps_0/1: the sRGB-read flag is set (sampler +0x3c).
        let srv = match self.files.read(path).and_then(|bytes| load_dds(&self.dev, &bytes, srgb).map_err(|error| error.to_string())) {
            Ok(texture) => texture.srv,
            Err(error) => {
                self.error(format!("{path}: {error}"));
                None
            }
        };

        self.sky.textures.insert(key, srv.clone());
        srv
    }

    pub(crate) fn pass_clouds(&mut self, env: &EnvironmentParams) {
        let vs = self.shader(CLOUD_VS);
        let ps = self.shader(CLOUD_PS);
        let (Some(vs), Some(ps)) = (vs, ps) else { return };
        if !self.settings.clouds {
            return;
        }

        self.cloud_layer.planes.resize_with(env.cloud_planes.len(), Default::default);

        // The far plane first.
        let mut order = vec![];
        for (i, plane) in env.cloud_planes.iter().enumerate() {
            if plane.far_plane {
                order.insert(0, i);
            } else {
                order.push(i);
            }
        }

        let el = [
            element(c"POSITION", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false),
            element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 8, false),
            element(c"TEXCOORD", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
            element(c"TEXCOORD", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
            element(c"TEXCOORD", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
            element(c"TEXCOORD", 4, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 48, true),
            element(c"TEXCOORD", 5, DXGI_FORMAT_R32G32_FLOAT, 1, 64, true),
        ];

        let Some(layout) = self.layout(&vs, "clouds", &el) else { return };
        self.ctx.set_input_layout(Some(&layout));
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.ctx.set_render_targets(&[self.gbuffer.scene_hdr_rtv()], self.gbuffer.scene_dsv().as_ref());
        self.ctx.set_depth_stencil_state(self.states.ds_sky.as_ref(), 0);
        self.ctx.set_blend_state(self.states.bs_alpha.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());

        let to_sun = env.to_sun();
        for i in order {
            let p = &env.cloud_planes[i];

            // Regenerated when anything that affects spawning changes (the numbers as C's %f writes them, as the C++ editor compared them).
            let mut sig = format!("{}|{}|{:.6}|{:.6}|{:.6}|{}", self.settings.cloud_seed, p.grid_size, p.density, p.height_variation, p.rotation, i32::from(p.far_plane));
            for c in &p.clouds {
                sig += &format!("|{}{:.6}", i32::from(c.this_cloud_only), c.probability);
                for e in &c.elements {
                    sig += &format!(";{}{:.6}{:.6}{:.6}{:.6}{}{}{}", i32::from(e.enabled), e.scale, e.offset[0], e.offset[1], e.offset[2], e.texture_index, e.texture_row, e.texture_column);
                }
            }

            let mut dirty = false;
            let g = &mut self.cloud_layer.planes[i];
            if sig != g.signature {
                g.signature = sig;
                g.field.reset(p, self.settings.cloud_seed.wrapping_mul(7919).wrapping_add(i as u32));
                dirty = true;
            }

            if p.velocity * 0.1 > 0.001 && self.cloud_layer.dt > 0.0 {
                g.field.update(p, self.cloud_layer.dt);
                dirty = true;
            }

            if dirty {
                let inst = g.field.instances();
                g.count = inst.len() as u32;
                if g.count > g.capacity || g.instances.is_none() {
                    g.capacity = (g.count + 256).max(1024);
                    g.instances = dynamic_buffer(&self.dev, g.capacity as usize * 72, D3D11_BIND_VERTEX_BUFFER);
                }

                let bytes = inst.iter().flat_map(|c| c.t1.iter().chain(&c.t2).chain(&c.t3).chain(&c.t4).chain(&c.t5).flat_map(|f| f.to_le_bytes())).collect::<Vec<_>>();
                if g.count > 0
                    && let Some(buffer) = &g.instances
                    && let Ok(mut ms) = self.ctx.map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0)
                {
                    ms.write(0, &bytes);
                }
            }

            let (count, instances) = (self.cloud_layer.planes[i].count, self.cloud_layer.planes[i].instances.clone());
            if count == 0 || instances.is_none() {
                continue;
            }

            // cloud_plane_VS / cloud_plane_PS as the game fills them (memory dump): the colours raw except the shadow colour (pow 2.2).
            // Verified (fn 0x191e430): g_horizon_alpha_start / end = the file's value * 0.1 + 0.9.
            let alignment = self.wind.alignment();
            let cbs = &mut self.cbs;

            // fn 0x191e430 (unused by the shader).
            cbs.set_f("cloud_plane_VS", "g_wind_alignment", &[alignment]);
            cbs.set_f("cloud_plane_VS", "g_cloud_shadow_gradient", &[p.shadow_gradient]);
            cbs.set_f("cloud_plane_VS", "g_height_scale", &[p.height_scale]);
            cbs.set_f("cloud_plane_PS", "g_sun_direction", &[to_sun.x, to_sun.y, to_sun.z]);
            cbs.set_f("cloud_plane_PS", "g_sun_transmitted_colour", &p.transmitted);
            cbs.set_f("cloud_plane_PS", "g_sun_reflected_colour", &p.reflected);
            cbs.set_f("cloud_plane_PS", "g_transmit_falloff", &[p.transmit_falloff]);
            cbs.set_f("cloud_plane_PS", "g_flatten_factor", &[p.flatten]);
            let sh = colour_to_linear(Float3::new(p.shadow[0], p.shadow[1], p.shadow[2]));
            cbs.set_f("cloud_plane_PS", "g_cloud_shadow_colour", &[sh.x, sh.y, sh.z]);
            cbs.set_f("cloud_plane_PS", "g_cloud_shadow_falloff", &[p.shadow_falloff]);
            cbs.set_f("cloud_plane_PS", "g_horizon_alpha_start", &[p.horizon_alpha_start * 0.1 + 0.9]);
            cbs.set_f("cloud_plane_PS", "g_horizon_alpha_end", &[p.horizon_alpha_end * 0.1 + 0.9]);
            self.upload();
            self.bind(&vs);
            self.bind(&ps);
            let (map0, map1) = (self.sky_texture(&cloud_texture(p, 0), true), self.sky_texture(&cloud_texture(p, 1), true));
            bind_srv(&self.ctx, &ps, "s_diffuse_cloud_maps_0", map0.as_ref());
            bind_srv(&self.ctx, &ps, "s_diffuse_cloud_maps_1", map1.as_ref());

            // The cloud maps: linear wrap.
            ps_samplers(&self.ctx, &ps, &self.states.s_wrap_linear);
            self.ctx.set_vertex_buffers(0, &[self.res.cloud_quad_vb.clone(), instances], &[16, 72], &[0, 0]);
            self.ctx.set_index_buffer(self.res.cloud_quad_ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
            let n = super::shared::CLOUD_GRID as u32;
            self.ctx.draw_indexed_instanced(n * n * 6, count, 0, 0, 0);
        }

        self.ctx.set_blend_state(self.states.bs_opaque.as_ref());
    }
}
