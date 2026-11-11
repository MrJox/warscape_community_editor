//! Skinned models: the units, through the game's weighted shaders (docs/plans/units.md).
//!
//! `weightedcommon.hlsl` reads its bones from a float texture, `t_animation_map`: a bone is 3 texels, the rows of its 3x4 skinning matrix, at
//! `instance offset + 3 * bone`. The shader wraps that index at `g_animation_texture_width` and then adds 1 and 2 without wrapping, so a bone
//! must not cross a line's end: [`PALETTE_WIDTH`] is a multiple of 3 and every palette starts on one. The engine keeps a ring of such textures
//! (`WEIGHTED_MESH_ANIMATION_TEXTURE_CACHE`); here one dynamic texture, written once a frame, the instances sharing a palette sharing it.
//!
//! The instances are the entities' own small list, not the draw set's items: they change every frame and there are few. Nothing is culled yet,
//! neither by the camera nor by the shadow cascades: the clipper does it.
//!
//! The parts that aren't skinned (shields, weapons, crests: static meshes on a bone) are drawn here too, through the rigid shaders with instances
//! of their own: each part has its faction colours and decal, which the draw set's batches don't carry.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, Float4x4, length};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{DEFAULT_INSTANCE_PARAMS, EntityId, MODEL_SKINNED_SUFFIX, Payload, RenderScene};

use super::{DIRT_GBUFFER_ALPHA_PS, DIRT_GBUFFER_PS, RIGID_GBUFFER_ALPHA_PS, RIGID_GBUFFER_PS, RIGID_VS, Renderer, SHADOW_PS as RIGID_SHADOW_PS, SHADOW_VS as RIGID_SHADOW_VS, SKIN_STENCIL, dynamic_buffer, element, ps_samplers, rigid_layout};
use crate::model_cache::{GpuMesh, GpuModel, MeshKind, RigidInstance};
use crate::shader_lib::{GameShader, bind_srv};

/// The palette texture's width in texels: 512 bones per line, more than any skeleton has.
pub(crate) const PALETTE_WIDTH: usize = 1536;

/// The lines of a new palette texture, doubled when more are needed.
const PALETTE_LINES: usize = 64;

/// The weighted vertex shaders of the two vertex formats.
const VS_2_BONES: &str = "dx11_sm_3_0_weightedcommon.hlsl_vs_vs30_main";
const VS_4_BONES: &str = "dx11_sm_3_0_weightedcommon.hlsl_vs_vs30_main_4_bones";
const SHADOW_VS_2_BONES: &str = "dx11_sm_3_0_weightedcommon.hlsl_vs_vs30_shadow";
const SHADOW_VS_4_BONES: &str = "dx11_sm_3_0_weightedcommon.hlsl_vs_vs30_shadow_4_bones";
const SHADOW_PS: &str = "dx11_sm_3_0_weightedcommon.hlsl_ps_ps30_shadow";

/// The vertex stride of a two-bone mesh (`VertexFormat::Weighted`); a four-bone one (`Cinematic`) has 32.
const STRIDE_2_BONES: u32 = 28;

/// One instance: TEXCOORD0..7 of the weighted vertex shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SkinnedGpuInstance {
    /// TEXCOORD0..2: the rows of the 3x4 world matrix.
    world: [[f32; 4]; 3],

    /// TEXCOORD3: .x the palette's first texel, .y scales the fourth faction colour.
    palette: [f32; 4],

    /// TEXCOORD4 to 5: passed to the pixel shader; 0 leaves the colour and gloss alone (`.zw` of TEXCOORD4, not traced: dirt and blood?).
    params: [[f32; 4]; 2],

    /// TEXCOORD6: the tint, rgb and in `.w` the colour mask channel it applies to (1 to 3; 0: none).
    tint: [f32; 4],

    /// TEXCOORD7.
    extra: [f32; 4],
}

/// The instances of one mesh, in a row of the instance buffer.
#[derive(Clone)]
struct Run {
    model: Arc<GpuModel>,

    /// 0: the model's LOD 0, n: its coarser LOD n - 1.
    lod: usize,
    mesh: usize,
    faction: [[f32; 3]; 4],
    casts_shadow: bool,

    /// The decal's textures (`SkinnedPart`), for a mesh with a decal.
    decal: [Option<String>; 2],
    first: u32,
    count: u32,
}

impl Run {
    fn mesh(&self) -> &GpuMesh {
        match self.lod {
            0 => &self.model.meshes[self.mesh],
            l => &self.model.lods[l - 1].meshes[self.mesh],
        }
    }
}

/// The skinned entities and what they drew last.
#[derive(Default)]
pub(crate) struct SkinnedDraws {
    /// The skinned entities, and the scene's structure they were listed at.
    ids: Vec<EntityId>,
    scene_id: usize,
    revision: u64,

    palette: Option<ID3D11Texture2D>,
    palette_srv: Option<ID3D11ShaderResourceView>,
    palette_lines: usize,
    instances: Option<ID3D11Buffer>,
    capacity: usize,
    rigid_instances: Option<ID3D11Buffer>,
    rigid_capacity: usize,
    runs: Vec<Run>,
}

/// One placed mesh, before the sort.
enum Placed {
    Skinned(SkinnedGpuInstance),
    Rigid(RigidInstance),
}

/// A mesh's vertices are skinned (else it is a static mesh on a bone).
fn is_skinned(m: &GpuMesh) -> bool {
    matches!(m.kind, MeshKind::Weighted | MeshKind::WeightedSkin | MeshKind::WeightedCloth)
}

fn matrix_of(rows: &[f32; 12]) -> Float4x4 {
    let mut m = Float4x4::IDENTITY;
    for (r, row) in m.m.iter_mut().take(3).enumerate() {
        row.copy_from_slice(&rows[r * 4..r * 4 + 4]);
    }

    m
}

/// The input layout of a vertex format (from the shaders' code and the file's layout): the bytes of the bone indices and weights are read
/// BGRA, as normals are, so `v1.zyx` are the file's bytes 0, 1, 2.
fn layout_of(stride: u32) -> Vec<InputElement> {
    let (f16x4, f16x2, bytes, f32x4) = (DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT);
    let mut el = if stride == STRIDE_2_BONES {
        vec![
            element(c"POSITION", 0, f16x4, 0, 0, false),
            element(c"POSITION", 1, bytes, 0, 8, false),
            element(c"POSITION", 2, bytes, 0, 12, false),
            element(c"POSITION", 3, f16x2, 0, 16, false),
            element(c"POSITION", 4, bytes, 0, 20, false),
            element(c"POSITION", 5, bytes, 0, 24, false),
        ]
    } else {
        vec![
            element(c"POSITION", 0, f16x4, 0, 0, false),
            element(c"POSITION", 1, bytes, 0, 8, false),
            element(c"POSITION", 2, bytes, 0, 12, false),
            element(c"POSITION", 3, bytes, 0, 16, false),
            element(c"POSITION", 4, f16x2, 0, 20, false),
            element(c"POSITION", 5, bytes, 0, 24, false),
            element(c"POSITION", 6, bytes, 0, 28, false),
        ]
    };

    el.extend((0..8).map(|i| element(c"TEXCOORD", i, f32x4, 1, i * 16, true)));
    el
}

/// The G-buffer pixel shader of a mesh: its material's family and, for skin, the entry the skin lighting reads (`_skin`; `_skin_slm` writes
/// what the standard lighting reads, and the engine's choice between them isn't traced). **ASSUMPTION** until compared with the game: `_noclip`
/// for the materials that aren't alpha-tested.
fn gbuffer_ps(m: &GpuMesh) -> String {
    // A sail: `weightedcloth`, which has no dirt or decal variants (its dirt maps are its own textures; unbound, no dirt and no damage).
    if m.kind == MeshKind::WeightedCloth {
        let noclip = if m.alpha_mode == 1 { "" } else { "_noclip" };
        return format!("dx11_sm_3_0_weightedcloth.hlsl_ps_ps30_gbuffer{noclip}");
    }

    let family = match (m.dirt, m.decal) {
        (false, false) => "weighted",
        (true, false) => "weightedwithdirt",
        (false, true) => "weightedwithdecal",
        (true, true) => "weightedwithdecaldirt",
    };

    let skin = if m.kind == MeshKind::WeightedSkin { "_skin" } else { "" };
    let noclip = if m.alpha_mode == 1 { "" } else { "_noclip" };
    format!("dx11_sm_3_0_{family}.hlsl_ps_ps30_gbuffer{skin}{noclip}")
}

/// The G-buffer pixel shader of a static mesh on a bone.
fn rigid_ps(m: &GpuMesh) -> String {
    let alpha = m.alpha_mode == 1;
    match (m.kind, m.dirt, alpha) {
        (MeshKind::RigidDecal, true, false) => "dx11_sm_3_0_rigidwithdecaldirtmesh.hlsl_ps_ps30_gbuffer".to_owned(),
        (MeshKind::RigidDecal, true, true) => "dx11_sm_3_0_rigidwithdecaldirtmesh.hlsl_ps_ps30_gbuffer_alpha_test".to_owned(),
        (MeshKind::RigidDecal, false, false) => "dx11_sm_3_0_rigidwithdecalmesh.hlsl_ps_ps30_gbuffer".to_owned(),
        (MeshKind::RigidDecal, false, true) => "dx11_sm_3_0_rigidwithdecalmesh.hlsl_ps_ps30_gbuffer_alpha_test".to_owned(),
        (MeshKind::Dirtmap, _, false) => DIRT_GBUFFER_PS.to_owned(),
        (MeshKind::Dirtmap, _, true) => DIRT_GBUFFER_ALPHA_PS.to_owned(),
        (_, _, false) => RIGID_GBUFFER_PS.to_owned(),
        (_, _, true) => RIGID_GBUFFER_ALPHA_PS.to_owned(),
    }
}

/// The lines `texels` take in the palette texture.
fn lines_of(texels: usize) -> usize {
    texels.div_ceil(PALETTE_WIDTH)
}

impl Renderer {

    /// Writes the frame's palettes and instances (before the first pass that draws them). `eye` is the view's: `self.camera` may still be the environment
    /// cube's here, every fourth frame, and the LODs picked from it put all the soldiers at their distance from the battle's centre for that frame.
    pub(crate) fn prepare_skinned(&mut self, scene: &RenderScene, eye: Float3) {
        let scene_id = std::ptr::from_ref(scene) as usize;
        if self.skinned.scene_id != scene_id || self.skinned.revision != scene.structure_revision() {
            self.skinned.scene_id = scene_id;
            self.skinned.revision = scene.structure_revision();
            self.skinned.ids = scene.entities().iter().filter(|e| matches!(e.payload, Payload::SkinnedInstance(_))).map(|e| e.id).collect();
        }

        self.skinned.runs.clear();
        if self.skinned.ids.is_empty() {
            return;
        }

        wce_prof!("render.skinned_prepare");
        let (units, o) = (self.camera.units, self.camera.origin);
        let mut texels: Vec<[f32; 4]> = vec![];
        let mut offset_of = HashMap::<usize, usize>::new();
        let mut placed: Vec<(Run, Placed)> = vec![];
        for id in self.skinned.ids.clone() {
            let Some(e) = scene.find(id) else { continue };
            let Payload::SkinnedInstance(s) = &e.payload else { continue };
            if !scene.drawable(e) || s.palette.is_empty() {
                continue;
            }

            // A palette an instance shares with others is written once.
            let palette = *offset_of.entry(Arc::as_ptr(&s.palette) as usize).or_insert_with(|| {
                let at = texels.len();
                texels.extend(s.palette.iter().flat_map(|m| m.chunks_exact(4).map(|r| [r[0], r[1], r[2], r[3]])));
                at
            });

            // Scene units to game units, around the render origin.
            let rows = |m: &Float4x4| -> [[f32; 4]; 3] {
                std::array::from_fn(|r| {
                    let mut row: [f32; 4] = std::array::from_fn(|c| m.m[r][c] * units);
                    row[3] -= [o.x, o.y, o.z][r] * units;
                    row
                })
            };

            let world = rows(&e.world);
            // The file's LOD distances are in game units.
            let pos = Float3::new(e.world.m[0][3], e.world.m[1][3], e.world.m[2][3]);
            let distance = length(pos - eye) * units;
            for part in &s.parts {
                let Some(model) = self.models.try_get(&format!("{}{MODEL_SKINNED_SUFFIX}", part.model), distance) else { continue };
                let lod = (0..=model.lods.len()).find(|&l| match l {
                    0 => model.distance == 0.0 || distance <= model.distance,
                    l => distance <= model.lods[l - 1].distance,
                }).unwrap_or(model.lods.len());
                let count = if lod == 0 { model.meshes.len() } else { model.lods[lod - 1].meshes.len() };
                let f = s.faction_colours;
                let faction = [f[usize::from(part.masks[0])], f[usize::from(part.masks[1])], f[usize::from(part.masks[2])], f[3]];
                for mesh in 0..count {
                    let decal = [part.decal_diffuse.clone(), part.decal_normal.clone()];
                    let run = Run { model: model.clone(), lod, mesh, faction, casts_shadow: s.casts_shadow, decal, first: 0, count: 1 };
                    let m = run.mesh();
                    let instance = if is_skinned(m) {
                        if part.bone.is_some() {
                            continue;
                        }

                        // A sail's mesh: its own bones, after the model's earlier sails' (3 texels a bone).
                        let first = palette + 3 * m.palette_base as usize;
                        Placed::Skinned(SkinnedGpuInstance { world, palette: [first as f32, s.faction_scale, 0.0, 0.0], tint: part.tint, ..SkinnedGpuInstance::default() })
                    } else {
                        // A static mesh: on its part's bone, or its material's, at the pivot its vertices are stored around.
                        let bone = part.bone.or(u32::try_from(m.bone).ok()).and_then(|b| s.bones.get(b as usize));
                        let mut carrier = bone.map_or(e.world, |b| e.world * matrix_of(b)) * matrix_of(&part.offset);
                        if m.has_offset {
                            for r in 0..3 {
                                carrier.m[r][3] += carrier.m[r][0] * m.offset.x + carrier.m[r][1] * m.offset.y + carrier.m[r][2] * m.offset.z;
                            }
                        }

                        let p = |i: usize| std::array::from_fn(|k| DEFAULT_INSTANCE_PARAMS[i * 4 + k]);
                        Placed::Rigid(RigidInstance { world: rows(&carrier), p4: p(0), p5: p(1), p6: p(2), p7: p(3) })
                    };

                    placed.push((run, instance));
                }
            }
        }

        // The same mesh and colours in a row: one instanced draw.
        let key = |r: &Run| (Arc::as_ptr(&r.model) as usize, r.lod, r.mesh, r.faction.map(|c| c.map(f32::to_bits)), r.casts_shadow, r.decal.clone());
        placed.sort_by_key(|(r, _)| key(r));
        let (mut skinned, mut rigid) = (vec![], vec![]);
        let mut runs: Vec<Run> = vec![];
        for (mut run, instance) in placed {
            let first = match instance {
                Placed::Skinned(i) => {
                    skinned.push(i);
                    skinned.len()
                }

                Placed::Rigid(i) => {
                    rigid.push(i);
                    rigid.len()
                }
            } as u32 - 1;

            match runs.last_mut().filter(|last| key(last) == key(&run)) {
                Some(last) => last.count += 1,
                None => {
                    run.first = first;
                    runs.push(run);
                }
            }
        }

        if self.write_palette(&texels) && self.write_instances(&skinned, &rigid) {
            self.skinned.runs = runs;
        }
    }

    fn write_palette(&mut self, texels: &[[f32; 4]]) -> bool {
        let lines = lines_of(texels.len());
        if lines > self.skinned.palette_lines {
            let height = lines.next_power_of_two().max(PALETTE_LINES);
            let desc = D3D11_TEXTURE2D_DESC {
                Width: PALETTE_WIDTH as u32, Height: height as u32, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DYNAMIC, BindFlags: D3D11_BIND_SHADER_RESOURCE as u32,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE as u32, MiscFlags: 0,
            };

            let Ok(texture) = self.dev.create_texture_2d(&desc, None) else {
                self.error("cannot create the bone palette texture".to_owned());
                return false;
            };

            self.skinned.palette_srv = self.dev.create_shader_resource_view(&texture, None).ok();
            self.skinned.palette = Some(texture);
            self.skinned.palette_lines = height;
            self.cbs.set_f("variant_animation_size", "g_animation_texture_width", &[PALETTE_WIDTH as f32, height as f32]);
        }

        let Some(texture) = self.skinned.palette.clone() else { return false };
        let Ok(mut mapped) = self.ctx.map(&texture, 0, D3D11_MAP_WRITE_DISCARD, 0) else { return false };
        let pitch = mapped.row_pitch();
        texels.chunks(PALETTE_WIDTH).enumerate().all(|(y, line)| mapped.write(y * pitch, line))
    }

    fn write_instances(&mut self, skinned: &[SkinnedGpuInstance], rigid: &[RigidInstance]) -> bool {
        if skinned.len() > self.skinned.capacity {
            self.skinned.capacity = (skinned.len() * 2).max(256);
            self.skinned.instances = dynamic_buffer(&self.dev, self.skinned.capacity * size_of::<SkinnedGpuInstance>(), D3D11_BIND_VERTEX_BUFFER);
        }

        if rigid.len() > self.skinned.rigid_capacity {
            self.skinned.rigid_capacity = (rigid.len() * 2).max(256);
            self.skinned.rigid_instances = dynamic_buffer(&self.dev, self.skinned.rigid_capacity * size_of::<RigidInstance>(), D3D11_BIND_VERTEX_BUFFER);
        }

        if !skinned.is_empty() && !self.skinned.instances.clone().is_some_and(|b| self.ctx.map(&b, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut m| m.write(0, skinned))) {
            return false;
        }

        rigid.is_empty() || self.skinned.rigid_instances.clone().is_some_and(|b| self.ctx.map(&b, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut m| m.write(0, rigid)))
    }

    /// Whether this frame's G-buffer has skin pixels: the skin lighting draws them.
    pub(crate) fn skin_drawn(&self) -> bool {
        self.skinned.runs.iter().any(|r| r.mesh().kind == MeshKind::WeightedSkin)
    }

    /// A mesh's decal: the part's textures, or none (its mask black, so that the decal adds nothing).
    fn bind_decal(&self, ps: &GameShader, run: &Run) {
        let srv = |path: &Option<String>, srgb: bool| path.as_ref().and_then(|p| self.textures.get(p, srgb)).and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
        let (diffuse, normal) = (srv(&run.decal[0], true), srv(&run.decal[1], false));
        if diffuse.is_none() {
            bind_srv(&self.ctx, ps, "s_decal_mask", self.res.black_srv.as_ref());
        }

        bind_srv(&self.ctx, ps, "s_decal_diffuse", diffuse.as_ref().or(self.res.black_srv.as_ref()));
        bind_srv(&self.ctx, ps, "s_decal_normal", normal.as_ref().or(self.res.black_srv.as_ref()));
    }

    /// The vertex shader's textures: the palette, and the rigid passes' empty selection ids.
    fn bind_palette(&self, vs: &GameShader) {
        for (name, slot) in &vs.textures {
            let srv = if name == "t_animation_map" { &self.skinned.palette_srv } else { &self.res.instance_ids_srv };
            self.ctx.vs_set_shader_resources(*slot, std::slice::from_ref(srv));
        }
    }

    fn skinned_vs(&mut self, stride: u32, shadow: bool) -> Option<(Rc<GameShader>, ID3D11InputLayout)> {
        let name = match (stride == STRIDE_2_BONES, shadow) {
            (true, false) => VS_2_BONES,
            (false, false) => VS_4_BONES,
            (true, true) => SHADOW_VS_2_BONES,
            (false, true) => SHADOW_VS_4_BONES,
        };

        let vs = self.shader(name)?;
        let layout = self.layout(&vs, name, &layout_of(stride))?;
        Some((vs, layout))
    }

    /// The instance buffer of a mesh's kind.
    fn bind_instances(&self, m: &GpuMesh) {
        let (buffer, stride) = if is_skinned(m) { (&self.skinned.instances, size_of::<SkinnedGpuInstance>()) } else { (&self.skinned.rigid_instances, size_of::<RigidInstance>()) };
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(buffer), &[stride as u32], &[0]);
    }

    /// The rigid passes' instances again.
    fn bind_rigid_ring(&self) {
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[size_of::<RigidInstance>() as u32], &[0]);
    }

    /// The shaders of one mesh in the G-buffer, bound.
    fn bind_mesh_gbuffer(&mut self, m: &GpuMesh) -> Option<Rc<GameShader>> {
        if is_skinned(m) {
            let (vs, layout) = self.skinned_vs(m.stride, false)?;
            let ps = self.shader(&gbuffer_ps(m))?;
            self.ctx.set_input_layout(Some(&layout));
            self.bind(&vs);
            self.bind_palette(&vs);
            self.bind(&ps);
            return Some(ps);
        }

        let vs = self.shader(RIGID_VS)?;
        let ps = self.shader(&rigid_ps(m))?;
        let layout = self.layout(&vs, "rigid_std", &rigid_layout())?;
        self.ctx.set_input_layout(Some(&layout));
        self.bind(&vs);
        for &slot in vs.textures.values() {
            self.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&self.res.instance_ids_srv));
        }

        self.bind(&ps);
        let snow = self.snow_normals_srv();
        bind_srv(&self.ctx, &ps, "s_fire_map", self.res.black_srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_snow_normals", snow.as_ref());
        bind_srv(&self.ctx, &ps, "s_diffuse_burn_map", self.res.black_srv.as_ref());
        bind_srv(&self.ctx, &ps, "s_dissolve_map", self.res.white_srv.as_ref());
        Some(ps)
    }

    /// Draws the units into the bound G-buffer. The rigid instances stay bound afterwards.
    pub(crate) fn draw_skinned_gbuffer(&mut self) {
        if self.skinned.runs.is_empty() {
            return;
        }

        wce_prof!("render.skinned_gbuffer");
        let runs = std::mem::take(&mut self.skinned.runs);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        for run in &runs {
            let m = run.mesh();
            self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), if m.kind == MeshKind::WeightedSkin { SKIN_STENCIL } else { 1 });
            self.cbs.set_f("rigid_faction_colours", "faction_colours", &faction_constants(&run.faction));
            if m.decal {
                self.cbs.set_f("rigid_config_PS", "g_decal_uv_rect_coords", &m.decal_rect);
            }

            self.upload();
            let Some(ps) = self.bind_mesh_gbuffer(m) else { continue };
            self.bind_instances(m);
            ps_samplers(&self.ctx, &ps, &self.states.s_wrap_aniso);
            let slots = self.slots_of(&ps);
            self.bind_mesh_material(&ps, &slots, m);
            if m.decal {
                self.bind_decal(&ps, run);
            }

            self.draw_run(run);
        }

        self.ctx.set_depth_stencil_state(self.states.ds_gbuffer.as_ref(), 1);
        self.skinned.runs = runs;
        self.bind_rigid_ring();
    }

    /// Draws the units' casters into the bound shadow cascade. The caller binds its own shaders again; the rigid instances stay bound.
    pub(crate) fn draw_skinned_shadow(&mut self) {
        if self.skinned.runs.is_empty() {
            return;
        }

        let (Some(ps), Some(rigid_vs), Some(rigid_ps)) = (self.shader(SHADOW_PS), self.shader(RIGID_SHADOW_VS), self.shader(RIGID_SHADOW_PS)) else { return };
        let Some(rigid_layout) = self.layout(&rigid_vs, "rigid_std", &rigid_layout()) else { return };
        let runs = std::mem::take(&mut self.skinned.runs);
        self.upload();
        for run in runs.iter().filter(|r| r.casts_shadow) {
            let m = run.mesh();
            if is_skinned(m) {
                let Some((vs, layout)) = self.skinned_vs(m.stride, true) else { continue };
                self.ctx.set_input_layout(Some(&layout));
                self.bind(&vs);
                self.bind_palette(&vs);
                self.bind(&ps);
                ps_samplers(&self.ctx, &ps, &self.states.s_wrap_linear);
                let diffuse = m.tex[crate::model_cache::TEX_DIFFUSE].read().ok().and_then(|t| t.srv.clone());
                bind_srv(&self.ctx, &ps, "s_diffuse_map", diffuse.as_ref());
            } else {
                self.ctx.set_input_layout(Some(&rigid_layout));
                self.bind(&rigid_vs);
                for &slot in rigid_vs.textures.values() {
                    self.ctx.vs_set_shader_resources(slot, std::slice::from_ref(&self.res.instance_ids_srv));
                }

                self.bind(&rigid_ps);
            }

            self.bind_instances(m);
            self.draw_run(run);
            self.draw_stats.shadow_instances += u64::from(run.count);
            self.draw_stats.shadow_triangles += u64::from(m.index_count / 3 * run.count);
        }

        self.skinned.runs = runs;
        self.bind_rigid_ring();
    }

    fn draw_run(&mut self, run: &Run) {
        let m = run.mesh();
        self.ctx.set_vertex_buffers(0, std::slice::from_ref(&m.vb), &[m.stride], &[0]);
        self.ctx.set_index_buffer(m.ib.as_ref(), DXGI_FORMAT_R16_UINT, 0);
        self.ctx.draw_indexed_instanced(m.index_count, run.count, 0, 0, run.first);
        self.draw_stats.draw_calls += 1;
    }
}

/// `rigid_faction_colours.faction_colours`: float3[4], each at the start of a 16-byte row.
fn faction_constants(colours: &[[f32; 3]; 4]) -> [f32; 15] {
    std::array::from_fn(|i| if i % 4 == 3 { 0.0 } else { colours[i / 4][i % 4] })
}

#[cfg(test)]
mod tests {
    use super::{PALETTE_WIDTH, SkinnedGpuInstance, faction_constants, gbuffer_ps, layout_of, lines_of};
    use crate::model_cache::{GpuMesh, MeshKind};

    #[test]
    fn a_bone_never_crosses_a_line() {
        assert_eq!(PALETTE_WIDTH % 3, 0);
        assert_eq!((lines_of(0), lines_of(1), lines_of(PALETTE_WIDTH), lines_of(PALETTE_WIDTH + 1)), (0, 1, 1, 2));
    }

    #[test]
    fn instance_is_the_eight_texcoords() {
        assert_eq!(size_of::<SkinnedGpuInstance>(), 8 * 16);
        assert_eq!((layout_of(28).len(), layout_of(32).len()), (6 + 8, 7 + 8));
    }

    #[test]
    fn a_material_picks_its_family_and_entry() {
        let mesh = |kind, dirt, decal, alpha_mode| {
            let (vb, ib, offset, bmin, bmax) = (None, None, Default::default(), Default::default(), Default::default());
            let tex = std::array::from_fn(|_| Default::default());
            GpuMesh { vb, ib, index_count: 0, alpha_mode, stride: 28, kind, dirt, decal, bone: -1, decal_rect: [0.0; 4], uv2_tile: [0.0; 2], tint: [0.0; 12], offset, has_offset: false, tex, diffuse_path: String::new(), normal_path: String::new(), displacement: None, bmin, bmax, snow: false, palette_base: 0 }
        };

        assert_eq!(gbuffer_ps(&mesh(MeshKind::Weighted, false, false, -1)), "dx11_sm_3_0_weighted.hlsl_ps_ps30_gbuffer_noclip");
        assert_eq!(gbuffer_ps(&mesh(MeshKind::Weighted, true, true, 1)), "dx11_sm_3_0_weightedwithdecaldirt.hlsl_ps_ps30_gbuffer");
        assert_eq!(gbuffer_ps(&mesh(MeshKind::WeightedSkin, true, false, -1)), "dx11_sm_3_0_weightedwithdirt.hlsl_ps_ps30_gbuffer_skin_noclip");
        assert_eq!(gbuffer_ps(&mesh(MeshKind::WeightedSkin, false, true, 1)), "dx11_sm_3_0_weightedwithdecal.hlsl_ps_ps30_gbuffer_skin");
    }

    #[test]
    fn faction_colours_keep_their_rows() {
        let c = faction_constants(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0], [10.0, 11.0, 12.0]]);
        assert_eq!(c, [1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0, 0.0, 7.0, 8.0, 9.0, 0.0, 10.0, 11.0, 12.0]);
    }
}
