//! What the editor takes of rpfm_lib's rigid models (`rigid_model_v2`).

/// Whether the editor draws a rigid model of this version: 6, and 5 (some grass, `water_plane`, `boulder_a`, read by rpfm_lib into the
/// same structures and written back as they were). rpfm_lib also reads 1 and 3 (Attila's skybox `test_box`, the Greek agora buildings)
/// and Warhammer's 7 and 8; Attila's 1 and 3 have no mesh the renderer draws (docs/re/renderer.md, "Uk54 / Uk56 / Uk57"), 7 and 8 were
/// never checked. The model cache and the weather tool's sample map both ask this.
pub fn drawn_version(version: u32) -> bool {
    matches!(version, 5 | 6)
}
