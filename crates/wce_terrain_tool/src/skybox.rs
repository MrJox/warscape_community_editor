//! A campaign map's skybox as the game draws it (docs/plans/campaign_environment.md, "Sky"): `campaign_maps/<map>/display/skybox/`'s
//! `campaign_skybox.rigid_model_v2` (a cylinder 12.24 model units round, a 4096 x 512 cloud panorama) at the camera, unturned, scaled by
//! `campaign_sky_scale` 37.5 in the campaign world's units (fns 0x10afd250, 0x10b29f60; RenderDoc, camp_attila 2026-11-16: the world
//! matrix scale 37.5 at the eye plus the mesh's pivot, drawn in the G-buffer unlit, then fogged); `cubemap.dds` is the environment cube.
//! It casts no shadow. The game draws it into all four shadow cascades, but it lands outside every one of them: in capture A not a texel
//! of the shadow atlas changes at any of its four draws (events 313, 1089, 2059, 3429). Cast here, the editor's cascades (not the
//! game's fit) took it in: stripes over TDD's Rauros and a shadow the game never shows (the user, 2026-11-20).

use wce_editor::editor::Editor;
use wce_foundation::math::Float4x4;
use wce_scene::render_scene::{CampaignSky, DEFAULT_INSTANCE_PARAMS, DocumentId, EntityId, LayerId, MeshInstance, Payload, SourceRef};

/// `campaign_sky_scale` (tweakable, `CampaignTerrainDisplay.cpp`; its one reader at 0x10b29f9a).
const CAMPAIGN_SKY_SCALE: f32 = 37.5;

pub(crate) struct CampaignSkybox {
    layer: LayerId,
    entity: EntityId,

    /// Scene units a model unit: the sky scale over the scene's game units.
    scale: f32,
}

impl CampaignSkybox {
    /// The map's skybox on the scene (`map`: the campaign's folder name, `main_attila_map`), and its cube as the environment. None: the
    /// packs have no skybox model for it.
    pub fn show(e: &mut Editor, doc: DocumentId, map: &str) -> Option<Self> {
        let files = e.files()?;
        let dir = format!("campaign_maps/{map}/display/skybox");
        let model = format!("{dir}/campaign_skybox.rigid_model_v2");
        if !files.exists(&model) {
            return None;
        }

        let cube = format!("{dir}/cubemap.dds");
        e.scene.campaign_sky = files.exists(&cube).then_some(CampaignSky { environment: cube });
        let p = DEFAULT_INSTANCE_PARAMS;
        let mesh = MeshInstance {
            model,
            casts_shadow: false,
            lod: false,
            p4: [p[0], p[1], p[2], p[3]],
            p5: [p[4], p[5], p[6], p[7]],
            p6: [p[8], p[9], p[10], p[11]],
            p7: [p[12], p[13], p[14], p[15]],
            burning: false,
            faction_colours: None,
        };
        let layer = e.scene.add_layer(doc, "campaign sky");
        let entity = e.scene.add(layer, Float4x4::IDENTITY, Payload::MeshInstance(mesh), SourceRef { doc, item: u64::MAX });
        // Not a thing of the world: the camera neither bumps into it nor stands on it. It follows the eye, and a moved obstacle read
        // every model's box again at each step of the camera (100 ms a frame with a campaign map's forests ahead).
        if let Some(en) = e.scene.find_mut(entity) {
            en.locked = true;
            en.transient = true;
        }

        let me = Self { layer, entity, scale: CAMPAIGN_SKY_SCALE / e.scene.game_units.max(1e-9) };
        me.update(e);
        Some(me)
    }

    /// Keeps the skybox at the camera.
    pub fn update(&self, e: &mut Editor) {
        let eye = e.viewport.camera().eye;
        let s = self.scale;
        let mut world = Float4x4::IDENTITY;
        world.m[0] = [s, 0.0, 0.0, eye.x];
        world.m[1] = [0.0, s, 0.0, eye.y];
        world.m[2] = [0.0, 0.0, s, eye.z];
        if e.scene.find_mut(self.entity).is_some_and(|en| en.world != world) {
            e.scene.update_world_quiet(self.entity, world);
        }
    }

    /// Takes the skybox and its cube off the scene.
    pub fn clear(self, e: &mut Editor) {
        e.scene.remove_layer(self.layer);
        e.scene.campaign_sky = None;
    }
}
