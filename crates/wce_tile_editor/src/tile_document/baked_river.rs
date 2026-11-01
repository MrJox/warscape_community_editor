//! A decompiled tile's rivers when it has no river splines (decompiled before the decompiler fitted them, docs/formats/tile_decompile.md,
//! "Rivers"): the packs have none, only the water surface BOB baked into the terrain mesh's last block (`mesh.rigid_model_v2`, carried in the project's `decompiled_terrain`): eight f32
//! a vertex, u16 triangles, in the tile's metres with z the other way (z' = extent - z; 4x4_y_l1_r0_b3: the water 6.2 m
//! above a bed 11.9 m down, under the ground at 66 of 210 vertices, the bank edges, against 188 the other way). Attila's block is type 90 (x, y, z, flow speed, the width
//! across in metres, the distance along, alpha fade, the position across 0 .. 1; `wce_tile_io::attila::river`); Rome 2's type 48 (rpfm's
//! `RiverOutfield`), its last five floats in another order (river/4x4_y_l1_r0_b3: 0.1, then 0 / 0.25 .. along, 0 / 0.034 across, 1, 0.1).
//! The game's river shaders read the vertex as two float4s: the floats after the position go to them as they are (`WaterVertex`'s
//! across, u, v, flow, fade are the shader's P0.w and P1).

use std::path::Path;

use wce_scene::render_scene::{Water, WaterVertex};

/// The carried terrain mesh of a decompiled project (`wce_tile_io::attila::stage_b::CARRIED_DIR`).
const CARRIED_MESH: &str = "decompiled_terrain/mesh.rigid_model_v2";

/// The river block's types: Attila's, Rome 2's.
const RIVER_BLOCKS: [u32; 2] = [0x5a, 0x30];

/// The rivers of a decompiled tile in `dir` as one ribbon mesh in the scene's frame (x, z the tile's metres, z the other way; heights
/// over `unit`, as the terrain's), none when it has no river block.
pub(crate) fn baked_rivers(dir: &Path, extent_z: f64, unit: f64) -> Option<Water> {
    let bytes = std::fs::read(dir.join(CARRIED_MESH)).ok()?;
    let (verts, indices) = river_block(&bytes)?;
    let u = unit as f32;
    let verts = verts.iter().map(|v| WaterVertex {
        x: v[0], y: v[1] / u, z: extent_z as f32 - v[2], across: v[3], u: v[4], v: v[5], flow: v[6], fade: v[7],
    }).collect();

    Some(Water { river: true, foam: 0.0, verts, indices: indices.into_iter().map(u32::from).collect(), ..Default::default() })
}

/// The first LOD's river block of an RMV2 version 6 terrain mesh: its vertices and indices.
fn river_block(b: &[u8]) -> Option<(Vec<[f32; 8]>, Vec<u16>)> {
    let u32_at = |at: usize| b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..4) != Some(b"RMV2") || u32_at(8)? == 0 {
        return None;
    }

    // The LOD table after the 0x8c byte header: block count, vertex bytes, index bytes, the first block's offset, the distance.
    let (count, mut at) = (u32_at(0x8c)?, u32_at(0x98)? as usize);
    for _ in 0..count {
        let (kind, size, voff, vcount, ioff, icount) = (u32_at(at)?, u32_at(at + 4)? as usize, u32_at(at + 8)? as usize, u32_at(at + 12)? as usize, u32_at(at + 16)? as usize, u32_at(at + 20)? as usize);
        if RIVER_BLOCKS.contains(&kind) {
            let vb = b.get(at + voff..at + voff + vcount * 32)?;
            let verts = vb.chunks_exact(32).map(|c| std::array::from_fn(|k| f32::from_le_bytes([c[k * 4], c[k * 4 + 1], c[k * 4 + 2], c[k * 4 + 3]]))).collect();
            let ib = b.get(at + ioff..at + ioff + icount * 2)?;
            let indices = ib.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return Some((verts, indices));
        }

        if size == 0 {
            return None;
        }

        at += size;
    }

    None
}
