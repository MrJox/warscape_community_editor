//! Rome 2's effects (VFX): CPU particles (`sim`, the game's emitter classes) drawn by the shaders the game generated per effect
//! (`vfx_<effect>_generated.hlsl`: one vertex shader with a branch per emitter, pixel shaders per view). docs/re/rome2_vfx.md.
//!
//! Per frame: every placed effect's instances update (fn 0x115639a0), then the DeferredAlpha view collects the particles of the
//! emitters drawn in it, sorts them back to front over all effects (VFX_SORT_PARTICLES, the game's std::sort) and draws them in
//! runs of one effect: the records are the instance data, a four-corner strip the quad.
//! TODO(rome2): the Distortion and SeaColourOverlay views, cast_shadows, and the rigid emitters (the effects' point lights).

pub mod sim;
pub mod sort;

use std::collections::BTreeMap;
use std::rc::Rc;

use wce_assets::vfx_def::{VfxFile, parse_vfx};
use wce_d3d11::*;
use wce_foundation::math::Float3;
use wce_formats::xml_doc::XmlDoc;

use crate::cbuffers::ConstantBuffers;
use crate::particles::{Frame, WorldEffect, blend_state, rotation_of};
use crate::shader_lib::{FileReader, GameShader, ShaderLibrary, bind_sampler, bind_shader, bind_srv};
use crate::texture::{Texture, load_dds};

use sim::{Desc, Emitter, Instance, Owner, RECORD, Rng};

/// The pixel shaders' prefix: the shadow variant the renderer's other passes use.
const PS_PREFIX: &str = "dx11_enable_shadows_shadows_hardware_gather_5_0_sm_5_0_";

/// The texture slots of VFX_SHADER_BASE (s_diffuse1..7; a pixel shader declares as many as its effect has textures).
const DIFFUSE_SLOTS: usize = 7;

/// One effect file as its instances draw it.
struct Lib {
    camera_following: bool,

    /// The billboard emitters: definition, shader branch (the XML position), texture slots, and the view (scene_view_type).
    emitters: Vec<(String, Desc, u32, u32, i32)>,

    /// The unique textures (FUN_11564d60's lists), `vfx/textures/...`.
    diffuse: Vec<String>,
    vs: String,
    ps: [String; 2],
}

impl Lib {
    fn new(name: &str, file: &VfxFile) -> Self {
        // The textures of the render stacks' chains, once each, in order.
        let mut diffuse: Vec<String> = Vec::new();
        let mut normal: Vec<String> = Vec::new();
        // An empty texture is an entry too (no texture): an emitter without a normal map has the first empty one's slot.
        let texture = |chain: &str, which: &str| file.chain(chain).and_then(|c| c.find("ps_texture")).and_then(|m| m.find(which)).map(|p| p.value.clone());

        for stack in &file.stacks {
            for chain in &stack.chains {
                if let Some(t) = texture(chain, "diffuse").filter(|t| !diffuse.contains(t)) {
                    diffuse.push(t);
                }

                if let Some(t) = texture(chain, "normal").filter(|t| !normal.contains(t)) {
                    normal.push(t);
                }
            }
        }

        let mut emitters = Vec::new();
        for (index, def) in file.emitters.iter().enumerate() {
            let Some(desc) = Desc::from_def(def) else { continue };
            if !desc.enable {
                continue;
            }

            // FUN_11550a80: the slots of the emitter's render stack (its chain's textures); -1 for none.
            let stack = file.stacks.iter().find(|s| s.emitters.contains(&def.id));
            let packed = stack.map_or(u32::MAX, |s| {
                let chain = s.chains.first().map(String::as_str).unwrap_or_default();
                let slot = |list: &[String], which: &str| texture(chain, which).and_then(|t| list.iter().position(|x| *x == t)).map_or(-1, |k| k as i32);
                slot(&diffuse, "diffuse").wrapping_add(slot(&normal, "normal").wrapping_mul(10)) as u32
            });

            let view = stack.map_or(0, |s| s.scene_view_type);
            emitters.push((def.id.clone(), desc, index as u32, packed, view));
        }

        let generated = format!("vfx_{name}_generated.hlsl");
        Self {
            camera_following: file.camera_following(),
            emitters,
            diffuse: diffuse.iter().map(|t| if t.is_empty() { String::new() } else { format!("vfx/textures/{t}") }).collect(),
            vs: format!("dx11_sm_3_0_{generated}_vs_vs30_main"),
            ps: [format!("{PS_PREFIX}{generated}_ps_ps30_main_high_quality"), format!("{PS_PREFIX}{generated}_ps_ps30_main")],
        }
    }
}

/// An instance of a library effect inside a placed one (the file itself, or one of its vfx_references).
struct Part {
    lib_id: usize,

    /// The reference's position (effect frame, unscaled).
    offset: [f32; 3],
    inst: Instance,
}

/// A placed effect.
struct Placed {
    key: u64,
    effect: String,
    parts: Vec<Part>,
}

/// The shaders of an effect file.
struct LibShaders {
    vs: Rc<GameShader>,
    ps: Rc<GameShader>,
    layout: ID3D11InputLayout,
}

pub struct Rome2Vfx {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    read: Box<FileReader<'static>>,
    libs: BTreeMap<String, Option<usize>>,
    lib_list: Vec<Rc<Lib>>,
    shaders: BTreeMap<usize, Option<LibShaders>>,
    textures: BTreeMap<String, Option<Texture>>,
    placed: Vec<Placed>,
    rng: Rng,
    clock: f32,
    prev_camera: [f32; 3],
    corners: Option<ID3D11Buffer>,
    records: Option<ID3D11Buffer>,
    records_capacity: usize,
    bs: Option<ID3D11BlendState>,
    ds_test: Option<ID3D11DepthStencilState>,
    ds_off: Option<ID3D11DepthStencilState>,
    rs: Option<ID3D11RasterizerState>,
    s_wrap: Option<ID3D11SamplerState>,
    s_point: Option<ID3D11SamplerState>,
    s_clamp: Option<ID3D11SamplerState>,
    errors: Vec<String>,
}

impl Rome2Vfx {
    pub fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, read: Box<FileReader<'static>>) -> Self {
        let corners: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let desc = D3D11_BUFFER_DESC { ByteWidth: size_of_val(&corners) as u32, Usage: D3D11_USAGE_IMMUTABLE, BindFlags: D3D11_BIND_VERTEX_BUFFER as u32, ..Default::default() };
        let corners = dev.create_buffer(&desc, Some(bytemuck::cast_slice(&corners))).ok();

        // VFX_SHADER_BASE (fn 0x114bc6f0): premultiplied colour and alpha, depth tested but not written, both faces.
        let premultiplied = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL as u8,
        };

        let mut dd = D3D11_DEPTH_STENCIL_DESC { DepthEnable: true.into(), DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO, DepthFunc: D3D11_COMPARISON_LESS_EQUAL, ..Default::default() };
        let ds_test = dev.create_depth_stencil_state(&dd).ok();
        dd.DepthEnable = false.into();
        let ds_off = dev.create_depth_stencil_state(&dd).ok();
        let rd = D3D11_RASTERIZER_DESC { FillMode: D3D11_FILL_SOLID, CullMode: D3D11_CULL_NONE, DepthClipEnable: true.into(), ..Default::default() };
        let sampler = |filter: D3D11_FILTER, address: D3D11_TEXTURE_ADDRESS_MODE| {
            let sd = D3D11_SAMPLER_DESC { Filter: filter, AddressU: address, AddressV: address, AddressW: address, MaxLOD: D3D11_FLOAT32_MAX, ..Default::default() };
            dev.create_sampler_state(&sd).ok()
        };

        Self {
            dev: dev.clone(),
            ctx: ctx.clone(),
            read,
            libs: BTreeMap::new(),
            lib_list: Vec::new(),
            shaders: BTreeMap::new(),
            textures: BTreeMap::new(),
            placed: Vec::new(),
            rng: Rng(0x2545_f491),
            clock: 0.0,
            prev_camera: [0.0; 3],
            corners,
            records: None,
            records_capacity: 0,
            bs: blend_state(dev, premultiplied),
            ds_test,
            ds_off,
            rs: dev.create_rasterizer_state(&rd).ok(),
            s_wrap: sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_WRAP),
            s_point: sampler(D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_TEXTURE_ADDRESS_CLAMP),
            s_clamp: sampler(D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_TEXTURE_ADDRESS_CLAMP),
            errors: Vec::new(),
        }
    }

    /// What failed (missing files, shaders), once each.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    fn error(&mut self, text: String) {
        if !self.errors.contains(&text) {
            wce_foundation::log::warn(format!("vfx: {text}"));
            self.errors.push(text);
        }
    }

    fn file(&mut self, name: &str) -> Option<VfxFile> {
        match (self.read)(&format!("vfx/{name}.xml")).and_then(|bytes| XmlDoc::parse(bytes).map_err(|error| error.to_string())) {
            Ok(doc) => Some(parse_vfx(&doc)),
            Err(error) => {
                self.error(format!("vfx/{name}.xml: {error}"));
                None
            }
        }
    }

    fn lib(&mut self, name: &str) -> Option<(usize, Rc<Lib>)> {
        let id = match self.libs.get(name) {
            Some(id) => *id,
            None => {
                let id = self.file(name).map(|file| {
                    self.lib_list.push(Rc::new(Lib::new(name, &file)));
                    self.lib_list.len() - 1
                });

                self.libs.insert(name.to_owned(), id);
                id
            }
        };

        id.map(|id| (id, self.lib_list[id].clone()))
    }

    /// The parts of a placed effect: its own emitters, or (when it has vfx_references) one instance per reference.
    fn parts(&mut self, fx: &WorldEffect) -> Vec<Part> {
        let Some(file) = self.file(&fx.effect) else { return Vec::new() };
        let (_, quat) = rotation_of(&fx.world);
        let pos = [fx.world.m[0][3], fx.world.m[1][3], fx.world.m[2][3]];
        let mut out = Vec::new();
        let mut make = |this: &mut Self, name: &str, scale: f32, offset: [f32; 3], reference: Option<(&wce_assets::vfx_def::VfxRef, f32)>| {
            let Some((lib_id, lib)) = this.lib(name) else { return };
            let emitters = lib.emitters.iter().filter_map(|(id, desc, index, packed, view)| {
                let r = reference.and_then(|(r, _)| r.find(id));
                if r.is_some_and(|r| !r.enable) {
                    return None;
                }

                // The reference's tint, as the bytes it is kept in (fn 0x11507640).
                let tint = r.map_or([1.0; 4], |r| r.tint_min.map(|v| f32::from((v * 255.0) as i32 as u8) / 255.0));
                let mut e = Emitter::new(desc.clone(), *index, *packed, tint);
                e.view = *view;
                Some(e)
            }).collect();

            let owner = Owner { scale, colour: [1.0; 4], reference: reference.map(|(r, _)| (r.start_time, r.infinite_life)) };
            let inst = Instance::new(owner, pos, quat, emitters, lib.camera_following);
            out.push(Part { lib_id, offset, inst });
        };

        if file.refs.is_empty() {
            make(self, &fx.effect, fx.scale, [0.0; 3], None);
        } else {
            for r in &file.refs {
                // The reference's scale (at least 0.01) times the placement's.
                let scale = r.scale_min.max(0.01) * fx.scale;
                make(self, &r.library, scale, r.pos, Some((r, scale)));
            }
        }

        out
    }

    /// The placed effects of the scene. An effect that stays (same key and name) keeps its particles; a moved one goes there.
    pub fn set_world_effects(&mut self, effects: &[WorldEffect]) {
        let mut old: BTreeMap<u64, Placed> = self.placed.drain(..).map(|p| (p.key, p)).collect();
        for fx in effects {
            let (_, quat) = rotation_of(&fx.world);
            let pos = [fx.world.m[0][3], fx.world.m[1][3], fx.world.m[2][3]];
            let placed = match old.remove(&fx.key).filter(|p| p.effect == fx.effect) {
                Some(mut p) => {
                    for part in &mut p.parts {
                        part.inst.quat = quat;
                        part.inst.pos = pos;
                    }

                    p
                }

                None => Placed { key: fx.key, effect: fx.effect.clone(), parts: self.parts(fx) },
            };

            self.placed.push(placed);
        }

        // The references sit at their offset, turned and scaled with the placement (fn 0x115639a0).
        for p in &mut self.placed {
            for part in &mut p.parts {
                let m = rotation_matrix(part.inst.quat);
                let s = part.inst.owner.scale;
                let o = part.offset.map(|v| v * s);
                for (k, row) in m.iter().enumerate() {
                    part.inst.pos[k] += row[0] * o[0] + row[1] * o[1] + row[2] * o[2];
                }
            }
        }
    }

    /// Particles alive, all effects.
    pub fn alive(&self) -> usize {
        self.placed.iter().flat_map(|p| &p.parts).flat_map(|part| &part.inst.emitters).map(|e| e.parts.len() - e.first).sum()
    }

    fn shaders(&mut self, lib_id: usize, lib: &Lib, shader_lib: &mut ShaderLibrary, read: &FileReader, cbs: &mut ConstantBuffers) -> bool {
        if let Some(s) = self.shaders.get(&lib_id) {
            return s.is_some();
        }

        let vs = shader_lib.get(read, cbs, &lib.vs);
        let ps = shader_lib.get(read, cbs, &lib.ps[0]).or_else(|_| shader_lib.get(read, cbs, &lib.ps[1]));
        let made = match (vs, ps) {
            (Ok(vs), Ok(ps)) => {
                let f4 = DXGI_FORMAT_R32G32B32A32_FLOAT;
                let elements = [
                    InputElement::new(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 0, false),
                    InputElement::new(c"POSITION", 0, f4, 1, 0, true),
                    InputElement::new(c"TEXCOORD", 1, f4, 1, 16, true),
                    InputElement::new(c"POSITION", 1, f4, 1, 32, true),
                    InputElement::new(c"POSITION", 2, f4, 1, 48, true),
                    InputElement::new(c"POSITION", 3, f4, 1, 64, true),
                    InputElement::new(c"POSITION", 4, DXGI_FORMAT_R32G32B32_FLOAT, 1, 80, true),
                    InputElement::new(c"POSITION", 5, f4, 1, 92, true),
                    InputElement::new(c"POSITION", 6, f4, 1, 108, true),
                ];

                match shader_lib.layout(&vs, "vfx_rome2_record", &elements) {
                    Ok(layout) => Some(LibShaders { vs, ps, layout }),
                    Err(error) => {
                        self.error(format!("{}: {error}", lib.vs));
                        None
                    }
                }
            }

            (Err(error), _) | (_, Err(error)) => {
                self.error(error.to_string());
                None
            }
        };

        let ok = made.is_some();
        self.shaders.insert(lib_id, made);
        ok
    }

    fn texture(&mut self, path: &str) -> Option<ID3D11ShaderResourceView> {
        if !self.textures.contains_key(path) {
            let t = (self.read)(path).map_err(|e| e.to_string()).and_then(|bytes| load_dds(&self.dev, &bytes, true).map_err(|e| e.to_string()));
            let t = match t {
                Ok(t) => Some(t),
                Err(error) => {
                    self.error(format!("{path}: {error}"));
                    None
                }
            };

            self.textures.insert(path.to_owned(), t);
        }

        self.textures[path].as_ref().and_then(|t| t.srv.clone())
    }

    /// The frame: the simulation, then the DeferredAlpha view.
    pub fn render(&mut self, f: &Frame, shader_lib: &mut ShaderLibrary, read: &FileReader, cbs: &mut ConstantBuffers, wind: Float3) {
        self.clock += f.dt.max(0.0);
        let t = self.clock;
        let camera = [f.eye.x, f.eye.y, f.eye.z];
        let prev = self.prev_camera;

        // The battle's wind (fn 0x115e8c20): the environment's x times VFX_WIND_SCALE_FACTOR (the frame's is scaled already).
        let wind = [wind.x, 0.0, 0.0];
        if f.dt > 0.0 {
            for p in &mut self.placed {
                for part in &mut p.parts {
                    part.inst.update(t, wind, camera, prev, &mut self.rng);
                }
            }
        }

        self.prev_camera = camera;

        // The view's particles: (record, effect file) in effect, emitter and record order; then sorted back to front.
        let mut items: Vec<(&[f32; RECORD], usize)> = Vec::new();
        let mut slots = Vec::new();
        for p in &self.placed {
            for part in &p.parts {
                for e in &part.inst.emitters {
                    if e.view != 0 || !e.shown() {
                        continue;
                    }

                    slots.clear();
                    e.drawn(t, &mut slots);
                    items.extend(slots.iter().map(|&k| (&e.parts[k].rec, part.lib_id)));
                }
            }
        }

        if items.is_empty() {
            return;
        }

        let mut keys: Vec<sort::Item> = items.iter().enumerate().map(|(k, (r, _))| {
            let (dx, dy, dz) = (camera[0] - r[12], camera[1] - r[13], camera[2] - r[14]);
            (dy * dy + dx * dx + dz * dz, k as u32)
        }).collect();

        sort::sort(&mut keys);
        let mut data: Vec<f32> = Vec::with_capacity(keys.len() * RECORD);
        for &(_, k) in &keys {
            data.extend_from_slice(items[k as usize].0);
        }

        // Runs of one effect file: (file, first record, count).
        let mut runs: Vec<(usize, u32, u32)> = Vec::new();
        for (n, &(_, k)) in keys.iter().enumerate() {
            let id = items[k as usize].1;
            match runs.last_mut() {
                Some(run) if run.0 == id => run.2 += 1,
                _ => runs.push((id, n as u32, 1)),
            }
        }

        drop(items);
        if !self.upload(&data) {
            return;
        }

        self.draw(f, &runs, shader_lib, read, cbs);
    }

    fn upload(&mut self, data: &[f32]) -> bool {
        let bytes = std::mem::size_of_val(data);
        if self.records.is_none() || self.records_capacity < bytes {
            let capacity = bytes.next_power_of_two().max(64 * 1024);
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: capacity as u32, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_VERTEX_BUFFER as u32, CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32,
                MiscFlags: 0, StructureByteStride: 0,
            };

            self.records = self.dev.create_buffer(&desc, None).ok();
            self.records_capacity = capacity;
        }

        let Some(vb) = self.records.clone() else { return false };
        match self.ctx.map(&vb, 0, D3D11_MAP_WRITE_DISCARD, 0) {
            Ok(mut ms) => ms.write(0, data),
            Err(_) => false,
        }
    }

    fn draw(&mut self, f: &Frame, runs: &[(usize, u32, u32)], shader_lib: &mut ShaderLibrary, read: &FileReader, cbs: &mut ConstantBuffers) {
        let Some(rtv) = f.target.clone() else { return };

        // cb_vfx: the camera's axes (billboards face it) and the whole viewport.
        let row = |r: usize| [f.view.m[r][0], f.view.m[r][1], f.view.m[r][2]];
        cbs.set_f("cb_vfx", "g_camera_aligned_x_axis", &row(0));
        cbs.set_f("cb_vfx", "g_camera_aligned_y_axis", &row(1));
        cbs.set_f("cb_vfx", "g_camera_aligned_z_axis", &row(2));
        cbs.set_f("cb_vfx", "g_viewport_offset_scale", &[0.0, 0.0, 1.0, 1.0]);
        cbs.set_f("cb_vfx", "g_screen_scale", &[1.0, 1.0]);
        let _ = cbs.upload(&self.dev, &self.ctx);

        self.ctx.set_render_targets(&[Some(rtv)], f.depth.as_ref());
        self.ctx.set_viewports(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: f.width as f32, Height: f.height as f32, MinDepth: 0.0, MaxDepth: 1.0 }]);
        self.ctx.set_blend_state(self.bs.as_ref());
        self.ctx.set_depth_stencil_state(if f.depth.is_some() { self.ds_test.as_ref() } else { self.ds_off.as_ref() }, 0);
        self.ctx.set_rasterizer_state(self.rs.as_ref());
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        self.ctx.set_vertex_buffers(0, &[self.corners.clone(), self.records.clone()], &[8, (RECORD * 4) as u32], &[0, 0]);
        for &(id, start, count) in runs {
            let lib = self.lib_list[id].clone();
            if !self.shaders(id, &lib, shader_lib, read, cbs) {
                continue;
            }

            let textures: Vec<_> = lib.diffuse.iter().take(DIFFUSE_SLOTS).map(|path| if path.is_empty() { None } else { self.texture(path) }).collect();
            let Some(Some(sh)) = self.shaders.get(&id) else { continue };
            self.ctx.set_input_layout(Some(&sh.layout));
            bind_shader(&self.ctx, &sh.vs, cbs);
            bind_shader(&self.ctx, &sh.ps, cbs);
            for slot in 0..DIFFUSE_SLOTS {
                let name = format!("s_diffuse{}", slot + 1);
                let srv = textures.get(slot).cloned().flatten();
                bind_srv(&self.ctx, &sh.ps, &name, srv.as_ref());
                bind_sampler(&self.ctx, &sh.ps, &name, self.s_wrap.as_ref());
            }

            bind_srv(&self.ctx, &sh.ps, "gbuffer_channel_4_sampler", f.depth_zw.as_ref());
            bind_sampler(&self.ctx, &sh.ps, "gbuffer_channel_4_sampler", self.s_point.as_ref());

            // ps_fog's colour from the sky.
            bind_srv(&self.ctx, &sh.ps, "s_sky", f.sky.as_ref());
            bind_sampler(&self.ctx, &sh.ps, "s_sky", self.s_clamp.as_ref());
            self.ctx.draw_instanced(4, count, 0, start);
        }

        self.ctx.set_vertex_buffers(0, &[None, None], &[0, 0], &[0, 0]);
        let none: [Option<ID3D11ShaderResourceView>; 9] = Default::default();
        self.ctx.ps_set_shader_resources(0, &none);

        // The next passes read the HDR target: it must not stay bound as one.
        self.ctx.set_render_targets(&[], None);
        self.ctx.set_blend_state(None);
        self.ctx.set_depth_stencil_state(None, 0);
    }
}

/// A unit quaternion (x, y, z, w) as rows.
fn rotation_matrix(q: [f32; 4]) -> [[f32; 3]; 3] {
    let (x2, y2, z2, w2) = (q[0] + q[0], q[1] + q[1], q[2] + q[2], q[3] + q[3]);
    [
        [1.0 - y2 * q[1] - z2 * q[2], x2 * q[1] - w2 * q[2], x2 * q[2] + w2 * q[1]],
        [x2 * q[1] + w2 * q[2], 1.0 - x2 * q[0] - z2 * q[2], y2 * q[2] - w2 * q[0]],
        [x2 * q[2] - w2 * q[1], y2 * q[2] + w2 * q[0], 1.0 - x2 * q[0] - y2 * q[1]],
    ]
}

