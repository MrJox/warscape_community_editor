//! A custom-mesh tile's mesh (the `cliff_custom` set) from the kit's `custom_mesh.cs2`, as BOB builds its `custom_mesh.rigid_model_v2`
//! (checked against the packs' model of `cliff_custom/2x2_lb_rb/aa`, `custom_mesh_check`): one level of detail per CS2 mesh in their order
//! (`_lod01` ...), the vertices as the CS2's (position as is, the vertex normal and colour alpha as the `N0` bytes, texture channel 0 as the
//! uv), the same triangles wound the other way (the shader turns z over; turned again into the scene's frame, they keep BOB's winding),
//! the corners shared by identical vertices. `N1` and `N2` (BOB's
//! tangent frame) are left flat: the vertex shaders pass them on and no pixel shader reads them.

use std::collections::HashMap;
use std::sync::Arc;

use wce_formats::cas2::Cas2File;
use wce_scene::render_scene::{CUSTOM_TERRAIN_Y, CustomTerrainLod, CustomTerrainMesh, CustomTerrainVertex};
use wce_vfs::files::Files;

/// The packs' model's level distances (BOB writes these for every cliff tile read; the CS2 has none).
const LOD_DISTANCES: [f32; 5] = [100.0, 200.0, 400.0, 500.0, 1000.0];

/// The cliffs' albedo, before its climate and `_diffuse.dds` (the packs' model's texture name; the CS2's material is 3ds Max's).
pub const CLIFF_TEXTURE: &str = "terrain/textures/battle/cliff/0_su";

/// A unit vector component as the game's byte: truncated, as BOB's (-0.0056 is 126, 0.0043 is 128).
fn unorm(v: f32) -> u8 {
    ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0) as u8
}

/// The albedo of a climate: `<name>_<climate>_diffuse.dds` when the packs have it, else `<name>_default_diffuse.dds` (the engine's
/// fallback as far as it was read: the only complete pair besides the climates').
pub fn albedo(name: &str, climate: &str, files: &dyn Files) -> String {
    let own = format!("{name}_{}_diffuse.dds", climate.to_lowercase());
    if !climate.is_empty() && files.exists(&own) { own } else { format!("{name}_default_diffuse.dds") }
}

/// The mesh of a CS2 of a tile `depth` long (z, scene units), with `albedo` and `id`.
///
/// # Errors
///
/// No mesh with triangles.
pub fn from_cs2(cs2: &Cas2File, albedo: String, id: u64, depth: f32) -> Result<CustomTerrainMesh, String> {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut lods = vec![];
    for (k, m) in cs2.meshes.iter().filter(|m| !m.faces.is_empty()).enumerate() {
        let mut at: HashMap<[u32; 6], u32> = HashMap::new();
        let (mut vertices, mut indices) = (vec![], Vec::with_capacity(m.faces.len() * 3));
        for f in &m.faces {
            for &c in &[f[0], f[2], f[1]] {
                let c = c as usize;
                let (p, n) = (m.positions[c], m.normals.get(c).copied().unwrap_or([0.0, 1.0, 0.0]));
                let weight = m.colours.get(c).map_or(0.0, |c| c[3]);
                let uv = m.uvs.get(c).and_then(|u| u.first()).map_or([0.0, 0.0], |u| [u[0], u[1]]);
                let v = CustomTerrainVertex { position: p, normal: [unorm(n[2]), unorm(n[1]), unorm(n[0]), (weight.clamp(0.0, 1.0) * 255.0).round() as u8], uv };
                // Shared by the vertices whose drawn bytes are the same. BOB keeps a few more apart (3501 against 3482 in the LOD 0 of
                // 2x2_lb_rb/aa), by its tangent bytes (N1), which no pixel shader reads.
                let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), u32::from_le_bytes(v.normal), uv[0].to_bits(), uv[1].to_bits()];
                let i = *at.entry(key).or_insert_with(|| {
                    vertices.push(v);
                    (vertices.len() - 1) as u32
                });

                indices.push(i);
                // Where it is drawn in the tile's frame.
                for (a, q) in [p[0], CUSTOM_TERRAIN_Y * p[1], depth + p[2]].into_iter().enumerate() {
                    (lo[a], hi[a]) = (lo[a].min(q), hi[a].max(q));
                }
            }
        }

        let distance = LOD_DISTANCES.get(k).copied().unwrap_or(f32::MAX);
        lods.push(CustomTerrainLod { distance, vertices: Arc::new(vertices), indices: Arc::new(indices) });
    }

    if lods.is_empty() {
        return Err("the CS2 has no mesh with triangles".to_owned());
    }

    Ok(CustomTerrainMesh { id, lods, albedo, depth, bounds: [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]] })
}

/// The mesh's heights from above (its finest level) on a grid of `nx` x `nz` samples from (`ox`, `oz`) every `step` (scene units of the
/// tile's frame), as BOB's `hf_height_map.data` is (2 y: game units, the highest surface over each sample); samples the mesh does not
/// cover get its lowest height (the foot of the cliff, under the sea). The test's check of `CUSTOM_TERRAIN_Y`.
#[cfg(test)]
fn heights_from_above(mesh: &CustomTerrainMesh, nx: i32, nz: i32, ox: f64, oz: f64, step: f64) -> Vec<f32> {
    let low = mesh.bounds[1];
    let mut h = vec![f32::MIN; (nx.max(0) * nz.max(0)) as usize];
    let Some(l) = mesh.lods.first() else { return vec![low; h.len()] };
    let at = |k: u32| {
        let p = l.vertices[k as usize].position;
        (f64::from(p[0]), f64::from(CUSTOM_TERRAIN_Y * p[1]), f64::from(mesh.depth + p[2]))
    };

    for t in l.indices.chunks_exact(3) {
        let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
        let area = (b.0 - a.0) * (c.2 - a.2) - (c.0 - a.0) * (b.2 - a.2);
        if area.abs() < 1e-12 {
            continue;
        }

        let (x0, x1) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
        let (z0, z1) = (a.2.min(b.2).min(c.2), a.2.max(b.2).max(c.2));
        let (i0, i1) = (((x0 - ox) / step).ceil().max(0.0) as i32, (((x1 - ox) / step).floor() as i32).min(nx - 1));
        let (j0, j1) = (((z0 - oz) / step).ceil().max(0.0) as i32, (((z1 - oz) / step).floor() as i32).min(nz - 1));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (x, z) = (ox + f64::from(i) * step, oz + f64::from(j) * step);
                let wb = ((x - a.0) * (c.2 - a.2) - (c.0 - a.0) * (z - a.2)) / area;
                let wc = ((b.0 - a.0) * (z - a.2) - (x - a.0) * (b.2 - a.2)) / area;
                let wa = 1.0 - wb - wc;
                if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                    continue;
                }

                let y = (wa * a.1 + wb * b.1 + wc * c.1) as f32;
                let o = &mut h[(j * nx + i) as usize];
                *o = o.max(y);
            }
        }
    }

    for v in &mut h {
        if *v == f32::MIN {
            *v = low;
        }
    }

    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BOB's model of `cliff_custom/2x2_lb_rb/aa`: the same triangles, about its vertices (3501, 2480, 1839, 1663, 1569: it keeps a few
    /// more apart by their tangents). With `$WCE_GAME`, the game's folder.
    #[test]
    fn as_bobs_model() {
        let Some(game) = std::env::var_os("WCE_GAME") else { return };
        let path = std::path::Path::new(&game).join("assembly_kit/raw_data/terrain/tiles/battle/cliff_custom/2x2_lb_rb/aa/custom_mesh.cs2");
        let Ok(bytes) = std::fs::read(&path) else { return };
        let mesh = from_cs2(&Cas2File::parse(&bytes).unwrap(), String::new(), 1, 256.0).unwrap();
        let indices: Vec<usize> = mesh.lods.iter().map(|l| l.indices.len()).collect();
        assert_eq!(indices, [15633, 10518, 7050, 6222, 5811]);
        for (l, bob) in mesh.lods.iter().zip([3501, 2480, 1839, 1663, 1569]) {
            assert!(l.vertices.len() <= bob && l.vertices.len() * 100 >= bob * 99, "{} vertices against {bob}", l.vertices.len());
        }

        // From above, as BOB's hf_height_map.data (2 y, game units): the plateau's top 16.59 m.
        let h = heights_from_above(&mesh, 257, 257, 0.0, 0.0, 1.0);
        let top = h.iter().copied().fold(f32::MIN, f32::max);
        assert!((2.0 * top - 16.59).abs() < 0.2, "top {top}");
    }
}
