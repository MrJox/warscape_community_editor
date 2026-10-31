//! Custom terrain meshes (a custom-mesh tile's `CustomTerrain` mesh: the cliffs), with `rigidterrain.hlsl`'s custom geometry entries. The
//! game's tile loader takes `custom_mesh.rigid_model_v2` as such a tile's only geometry; its terrain's patches are drawn too (the map
//! context makes them a floor at the mesh's foot: something under the sea, not traced in the game). The mesh is drawn in two places:
//!
//! 1. the terrain surface pass (`geometry_custom_30`): `vs30_custom_geom` with the surface's pixel shaders (z / w and the view-space depth),
//!    so the tile's blend box projects its material onto the mesh as onto a patch;
//! 2. after the blend boxes (`custom_geometry`): `vs30_custom_geom_mat` + `ps30_custom_geom_mat`, the mesh's own albedo added to the
//!    accumulation targets with the weight `10 * N0.w` (the CS2's vertex colour alpha: 0 on the plateau, 1 on the cliff faces).
//!
//! The vertex shaders are the surface's `vs30_geom` with the position first made (x, 2 y, -z, 1), lifted by the LF the same way; so the
//! meshes are drawn inside `draw_terrain_patches`' LF groups. The vertex: P0 (position), N0 (the normal and weight bytes, read as
//! B8G8R8A8: the game's `vaf_byte_4_swizzle`), N1 and N2 (BOB's tangent frame, which no pixel shader reads: flat here), T0 (uv).
//! ASSUMPTION: the game's instance rows scale y by half of x and z (see `CUSTOM_TERRAIN_Y`).
//! Not drawn: their shadows (the game's custom geometry has no shadow entry of its own; not traced).

use std::collections::HashMap;

use wce_scene::render_scene::{CUSTOM_TERRAIN_Y, CustomTerrainMesh, Entity, EntityId, Payload};

use super::*;
use super::terrain::box_in_frustum;
use crate::shader_lib::{bind_sampler, bind_srv};

pub(crate) const CUSTOM_GEOM_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_custom_geom";
pub(crate) const CUSTOM_MAT_VS: &str = "dx11_sm_3_0_rigidterrain.hlsl_vs_vs30_custom_geom_mat";
pub(crate) const CUSTOM_MAT_PS: &str = "dx11_sm_3_0_rigidterrain.hlsl_ps_ps30_custom_geom_mat";

/// Bytes of a vertex in the buffer.
const STRIDE: u32 = 32;

/// N1 and N2: a flat unit vector (+z) with weight 0.
const FLAT: [u8; 4] = [255, 127, 127, 0];

/// The vertex (slot 0) and the instance's three world rows (slot 1).
pub(crate) fn custom_layout() -> [InputElement; 8] {
    [
        element(c"POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, false),
        element(c"NORMAL", 0, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 12, false),
        element(c"NORMAL", 1, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 16, false),
        element(c"NORMAL", 2, DXGI_FORMAT_B8G8R8A8_UNORM, 0, 20, false),
        element(c"TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 24, false),
        element(c"POSITION", 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 0, true),
        element(c"POSITION", 2, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 16, true),
        element(c"POSITION", 3, DXGI_FORMAT_R32G32B32A32_FLOAT, 1, 32, true),
    ]
}

/// A level of a mesh on the GPU.
#[derive(Clone, Debug, Default)]
pub(crate) struct CustomLodGpu {
    vb: Option<ID3D11Buffer>,
    ib: Option<ID3D11Buffer>,
    count: u32,
    distance: f32,
}

/// A mesh on the GPU, and the material update it was last used in.
#[derive(Clone, Debug, Default)]
pub(crate) struct CustomGpu {
    lods: Vec<CustomLodGpu>,
    albedo: String,
    bounds: [f32; 6],
    pub(crate) used: u64,
}

/// What the meshes are drawn for: the depth passes (the custom vertex shader, the bound pixel shader) or the material pass (both bound,
/// the albedo per mesh).
#[derive(Clone, Copy)]
pub(crate) enum CustomDraw<'a> {
    None,
    Depth(&'a GameShader),
    Material,
}

fn lod_buffers(dev: &ID3D11Device, l: &wce_scene::render_scene::CustomTerrainLod) -> CustomLodGpu {
    let mut bytes = Vec::with_capacity(l.vertices.len() * STRIDE as usize);
    for v in l.vertices.iter() {
        bytes.extend(f32_bytes(&v.position));
        bytes.extend(v.normal);
        bytes.extend(FLAT);
        bytes.extend(FLAT);
        bytes.extend(f32_bytes(&v.uv));
    }

    let ib: Vec<u8> = l.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    CustomLodGpu {
        vb: immutable_buffer(dev, &bytes, D3D11_BIND_VERTEX_BUFFER), ib: immutable_buffer(dev, &ib, D3D11_BIND_INDEX_BUFFER), count: l.indices.len() as u32,
        distance: l.distance,
    }
}

impl Renderer {

    /// A mesh's buffers, made the first time it is seen.
    pub(crate) fn build_custom(&mut self, m: &CustomTerrainMesh, epoch: u64) {
        let dev = self.dev.clone();
        let g = self.terrain.custom.entry(m.id).or_insert_with(|| CustomGpu {
            lods: m.lods.iter().map(|l| lod_buffers(&dev, l)).collect(), albedo: m.albedo.clone(), bounds: m.bounds, used: 0,
        });

        g.used = epoch;
    }

    /// The custom meshes of an LF group's terrains (`ids`, among `ents`) with the bound shaders and LF constants, the group's frame moved by
    /// `shift` (scene units); `cull` is the view-projection in scene units. Returns the triangles drawn.
    pub(crate) fn draw_custom_meshes(&mut self, ents: &HashMap<EntityId, &Entity>, ids: &[EntityId], how: CustomDraw, cull: &Float4x4, shift: [f32; 2]) -> u64 {
        let custom_vs = match how {
            CustomDraw::None => return 0,
            CustomDraw::Depth(vs) => Some(vs),
            CustomDraw::Material => None,
        };

        // The drawn levels: (mesh, level) -> instance rows.
        let (eye, units, origin) = (self.terrain.lod_eye, self.camera.units, self.camera.origin);
        let mut batches: Vec<((u64, usize), Vec<f32>)> = vec![];
        for id in ids {
            let Some(Payload::Terrain(t)) = ents.get(id).map(|e| &e.payload) else { continue };
            let (Some(m), Some(p)) = (&t.custom, self.terrain.placed.get(id)) else { continue };
            let Some(g) = self.terrain.custom.get(&m.id) else { continue };
            let w = &p.world.m;

            // The world box of the mesh, lifted by the LF under it.
            let b = g.bounds;
            let (mut bmin, mut bmax) = ([f32::MAX; 3], [f32::MIN; 3]);
            for k in 0..8 {
                let c = [b[if k & 1 == 0 { 0 } else { 3 }], b[if k & 2 == 0 { 1 } else { 4 }], b[if k & 4 == 0 { 2 } else { 5 }]];
                for r in 0..3 {
                    let v = w[r][0] * c[0] + w[r][1] * c[1] + w[r][2] * c[2] + w[r][3];
                    (bmin[r], bmax[r]) = (bmin[r].min(v), bmax[r].max(v));
                }
            }

            if let Some(lf) = &t.lf {
                let (lo, hi) = lf.range(bmin[0], bmin[2], bmax[0], bmax[2]);
                (bmin[1], bmax[1]) = (bmin[1] + lo, bmax[1] + hi);
            }

            if !box_in_frustum(cull, bmin, bmax) {
                continue;
            }

            // The level by the eye's distance to the box (scene units), as the rigid models' (the last beyond its distance).
            let d = (0..3).map(|r| { let e = [eye.x, eye.y, eye.z][r]; let q = e.clamp(bmin[r], bmax[r]) - e; q * q }).sum::<f32>().sqrt();
            let lod = g.lods.iter().position(|l| d <= l.distance).unwrap_or(g.lods.len().saturating_sub(1));

            // The rows take the shader's (x, 2 y, -z) to the world of the scene's (x, Y y, depth + z): y's column halved and times Y, z's
            // turned over and moved by the depth; game units, in the group's frame.
            let mut row = [0.0f32; 12];
            for r in 0..3 {
                row[r * 4..r * 4 + 4].copy_from_slice(&[w[r][0], w[r][1] * CUSTOM_TERRAIN_Y * 0.5, -w[r][2], w[r][3] + w[r][2] * m.depth].map(|v| v * units));
            }

            (row[3], row[7], row[11]) = (row[3] - shift[0] * units, row[7] - origin.y * units, row[11] - shift[1] * units);
            match batches.iter_mut().find(|(k, _)| *k == (m.id, lod)) {
                Some((_, rows)) => rows.extend(row),
                None => batches.push(((m.id, lod), row.to_vec())),
            }
        }

        if batches.is_empty() {
            return 0;
        }

        let vs_for_layout = match custom_vs {
            Some(vs) => vs.clone(),
            None => match self.shader(CUSTOM_MAT_VS) {
                Some(vs) => (*vs).clone(),
                None => return 0,
            },
        };

        let Some(il) = self.layout(&vs_for_layout, "terrain_custom", &custom_layout()) else { return 0 };
        let Some(inst_vb) = self.terrain.inst_vb.clone() else { return 0 };
        self.ctx.set_input_layout(Some(&il));
        if let Some(vs) = custom_vs {
            self.bind(vs);
        }

        let capacity = super::terrain::INSTANCE_BYTES / 48;
        let mut triangles = 0;
        for ((mesh, lod), rows) in batches {
            let Some(g) = self.terrain.custom.get(&mesh).cloned() else { continue };
            let Some(l) = g.lods.get(lod) else { continue };
            if matches!(how, CustomDraw::Material)
                && let Some(ps) = self.shader(CUSTOM_MAT_PS)
            {
                let albedo = self.textures.get(&g.albedo, true).and_then(|t| t.read().ok().and_then(|t| t.srv.clone()));
                bind_srv(&self.ctx, &ps, "s_albedo_map", albedo.as_ref());
            }

            self.ctx.set_vertex_buffers(0, &[l.vb.clone(), Some(inst_vb.clone())], &[STRIDE, 48], &[0, 0]);
            self.ctx.set_index_buffer(l.ib.as_ref(), DXGI_FORMAT_R32_UINT, 0);
            for chunk in rows.chunks(capacity * 12) {
                if !self.ctx.map(&inst_vb, 0, D3D11_MAP_WRITE_DISCARD, 0).is_ok_and(|mut ms| ms.write(0, chunk)) {
                    continue;
                }

                let n = (chunk.len() / 12) as u32;
                self.ctx.draw_indexed_instanced(l.count, n, 0, 0, 0);
                self.draw_stats.draw_calls += 1;
                triangles += u64::from(l.count / 3) * u64::from(n);
            }
        }

        triangles
    }

    /// The meshes' own material, after the blend boxes: added to the accumulation targets where their depth is the surface's.
    pub(crate) fn draw_custom_material(&mut self, ents: &[&Entity]) {
        if !ents.iter().any(|e| matches!(&e.payload, Payload::Terrain(t) if t.custom.is_some())) {
            return;
        }

        let (Some(vs), Some(ps)) = (self.shader(CUSTOM_MAT_VS), self.shader(CUSTOM_MAT_PS)) else { return };
        let acc = self.terrain.acc.iter().map(|a| a.rtv.clone()).collect::<Vec<_>>();
        self.ctx.set_render_targets(&acc, self.gbuffer.dsv.as_ref());
        self.ctx.set_depth_stencil_state(self.terrain.ds_equal.as_ref(), 0);
        self.ctx.set_blend_state(self.terrain.bs_add.as_ref());
        self.ctx.set_rasterizer_state(self.states.rs_none.as_ref());
        self.ctx.set_topology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        self.cbs.set_f("shroud_PS", "g_shroud_enabled", &[0.0]);
        self.upload();
        null_ps_srvs(&self.ctx, 16);
        self.bind(&ps);
        bind_srv(&self.ctx, &ps, "s_shroud", self.res.white_srv.as_ref());
        ps_samplers(&self.ctx, &ps, &self.states.s_wrap_aniso);
        bind_sampler(&self.ctx, &ps, "s_shroud", self.states.s_clamp_linear.as_ref());
        let (cull, clip) = (self.camera.main_view_proj, self.camera.cb_view_proj);
        let triangles = self.draw_terrain_patches_with(ents, &vs, &ps, &cull, &clip, CustomDraw::Material);
        self.draw_stats.triangles += triangles;
        null_ps_srvs(&self.ctx, 16);
    }
}
