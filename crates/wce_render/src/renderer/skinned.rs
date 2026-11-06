//! Skinned models: the units, through the game's weighted shaders (docs/plans/units.md).
//!
//! `weightedcommon.hlsl` reads its bones from a float texture, `t_animation_map`: a bone is 3 texels, the rows of its 3x4 skinning matrix, at
//! `instance offset + 3 * bone`. The shader wraps that index at `g_animation_texture_width` and then adds 1 and 2 without wrapping, so a bone
//! must not cross a line's end: [`PALETTE_WIDTH`] is a multiple of 3 and every palette starts on one. The engine keeps a ring of such textures
//! (`WEIGHTED_MESH_ANIMATION_TEXTURE_CACHE`); here one dynamic texture, written once a frame, the instances sharing a palette sharing it.
//!
//! The instances are the entities' own small list, not the draw set's items: they change every frame and there are few. Nothing is culled yet,
//! neither by the camera nor by the shadow cascades: the clipper does it.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use wce_d3d11::*;
use wce_foundation::math::{Float3, length};
use wce_foundation::wce_prof;
use wce_scene::render_scene::{EntityId, MODEL_SKINNED_SUFFIX, Payload, RenderScene};

use super::{Renderer, dynamic_buffer, element, ps_samplers};
use crate::model_cache::{GpuMesh, GpuModel, MeshKind};
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
    runs: Vec<Run>,
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

/// The G-buffer pixel shader of a mesh. **ASSUMPTION** until compared with the game: `_noclip` for the materials that aren't alpha-tested.
fn gbuffer_ps(m: &GpuMesh) -> &'static str {
    match (m.kind == MeshKind::WeightedSkin, m.alpha_mode == 1) {
        (false, true) => "dx11_sm_3_0_weighted.hlsl_ps_ps30_gbuffer",
        (false, false) => "dx11_sm_3_0_weighted.hlsl_ps_ps30_gbuffer_noclip",
        (true, true) => "dx11_sm_3_0_weighted.hlsl_ps_ps30_gbuffer_skin",
        (true, false) => "dx11_sm_3_0_weighted.hlsl_ps_ps30_gbuffer_skin_noclip",
    }
}

/// The lines `texels` take in the palette texture.
fn lines_of(texels: usize) -> usize {
    texels.div_ceil(PALETTE_WIDTH)
}

impl Renderer {

    /// Writes the frame's palettes and instances (before the first pass that draws them).
    pub(crate) fn prepare_skinned(&mut self, scene: &RenderScene) {
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
        let (units, o, eye) = (self.camera.units, self.camera.origin, self.camera.eye);
        let mut texels: Vec<[f32; 4]> = vec![];
        let mut offset_of = HashMap::<usize, usize>::new();
        let mut placed: Vec<(Run, SkinnedGpuInstance)> = vec![];
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

            let mut world = [[0.0f32; 4]; 3];
            for (r, row) in world.iter_mut().enumerate() {
                *row = std::array::from_fn(|c| e.world.m[r][c] * units);
                row[3] -= [o.x, o.y, o.z][r] * units;
            }

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
                for mesh in 0..count {
                    let run = Run { model: model.clone(), lod, mesh, faction: s.faction_colours, casts_shadow: s.casts_shadow, first: 0, count: 1 };
                    let instance = SkinnedGpuInstance { world, palette: [palette as f32, s.faction_scale, 0.0, 0.0], tint: part.tint, ..SkinnedGpuInstance::default() };
                    placed.push((run, instance));
                }
            }
        }

        // The same mesh and colours in a row: one instanced draw.
        let key = |r: &Run| (Arc::as_ptr(&r.model) as usize, r.lod, r.mesh, r.faction.map(|c| c.map(f32::to_bits)), r.casts_shadow);
        placed.sort_by_key(|(r, _)| key(r));
        let mut instances = Vec::with_capacity(placed.len());
        let mut runs: Vec<Run> = vec![];
        for (mut run, instance) in placed {
            instances.push(instance);
            match runs.last_mut().filter(|last| key(last) == key(&run)) {
                Some(last) => last.count += 1,
                None => {
                    run.first = instances.len() as u32 - 1;
                    runs.push(run);
                }
            }
        }

        if self.write_palette(&texels) && self.write_instances(&instances) {
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

    fn write_instances(&mut self, instances: &[SkinnedGpuInstance]) -> bool {
        if instances.len() > self.skinned.capacity {
            self.skinned.capacity = (instances.len() * 2).max(256);
            self.skinned.instances = dynamic_buffer(&self.dev, self.skinned.capacity * size_of::<SkinnedGpuInstance>(), D3D11_BIND_VERTEX_BUFFER);
        }

        let Some(buffer) = self.skinned.instances.clone() else {
            self.error("cannot create the skinned instance buffer".to_owned());
            self.skinned.capacity = 0;
            return false;
        };

        let Ok(mut mapped) = self.ctx.map(&buffer, 0, D3D11_MAP_WRITE_DISCARD, 0) else { return false };
        mapped.write(0, instances)
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

    /// Draws the skinned meshes into the bound G-buffer. The rigid instances stay bound afterwards.
    pub(crate) fn draw_skinned_gbuffer(&mut self) {
        if self.skinned.runs.is_empty() {
            return;
        }

        wce_prof!("render.skinned_gbuffer");
        let runs = std::mem::take(&mut self.skinned.runs);
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.skinned.instances), &[size_of::<SkinnedGpuInstance>() as u32], &[0]);
        self.ctx.set_rasterizer_state(self.states.rs_back.as_ref());
        for run in &runs {
            let m = run.mesh();
            let (Some((vs, layout)), Some(ps)) = (self.skinned_vs(m.stride, false), self.shader(gbuffer_ps(m))) else { continue };
            self.ctx.set_input_layout(Some(&layout));
            self.cbs.set_f("rigid_faction_colours", "faction_colours", &faction_constants(&run.faction));
            self.upload();
            self.bind(&vs);
            self.bind_palette(&vs);
            self.bind(&ps);
            ps_samplers(&self.ctx, &ps, &self.states.s_wrap_aniso);
            let slots = self.slots_of(&ps);
            self.bind_mesh_material(&ps, &slots, m);
            self.draw_run(run);
        }

        self.skinned.runs = runs;
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[size_of::<crate::model_cache::RigidInstance>() as u32], &[0]);
    }

    /// Draws the skinned casters into the bound shadow cascade. The caller binds its own shaders again; the rigid instances stay bound.
    pub(crate) fn draw_skinned_shadow(&mut self) {
        if self.skinned.runs.is_empty() {
            return;
        }

        let Some(ps) = self.shader(SHADOW_PS) else { return };
        let runs = std::mem::take(&mut self.skinned.runs);
        self.upload();
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.skinned.instances), &[size_of::<SkinnedGpuInstance>() as u32], &[0]);
        self.bind(&ps);
        ps_samplers(&self.ctx, &ps, &self.states.s_wrap_linear);
        for run in runs.iter().filter(|r| r.casts_shadow) {
            let m = run.mesh();
            let Some((vs, layout)) = self.skinned_vs(m.stride, true) else { continue };
            self.ctx.set_input_layout(Some(&layout));
            self.bind(&vs);
            self.bind_palette(&vs);
            let diffuse = m.tex[crate::model_cache::TEX_DIFFUSE].read().ok().and_then(|t| t.srv.clone());
            bind_srv(&self.ctx, &ps, "s_diffuse_map", diffuse.as_ref());
            self.draw_run(run);
            self.draw_stats.shadow_instances += u64::from(run.count);
            self.draw_stats.shadow_triangles += u64::from(m.index_count / 3 * run.count);
        }

        self.skinned.runs = runs;
        self.ctx.set_vertex_buffers(1, std::slice::from_ref(&self.draw_lists.ring.buffer), &[size_of::<crate::model_cache::RigidInstance>() as u32], &[0]);
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
    use super::{PALETTE_WIDTH, SkinnedGpuInstance, faction_constants, layout_of, lines_of};

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
    fn faction_colours_keep_their_rows() {
        let c = faction_constants(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0], [10.0, 11.0, 12.0]]);
        assert_eq!(c, [1.0, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0, 0.0, 7.0, 8.0, 9.0, 0.0, 10.0, 11.0, 12.0]);
    }
}
