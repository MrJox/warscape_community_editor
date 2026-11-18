//! A campaign map's borders and trade routes in the view (`wce_terrain_io::attila::overlay`, docs/formats/campaign_borders.md), drawn
//! in the overlay pass as the game draws them (`wce_render` `campaign_overlay.rs`): read on a worker for a faction (the borders' colours
//! are the diplomacy's towards it), handed to the scene in the view's frame.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use wce_editor::editor::Editor;
use wce_foundation::log;
use wce_scene::campaign_lighting::CAMPAIGN_Z_SCALE;
use wce_scene::render_scene::{CampaignLines, DocumentId, OverlayCurve, OverlayLine};
use wce_terrain_io::attila::borders::BorderCurve;
use wce_terrain_io::attila::diplomacy::FactionInfo;
use wce_terrain_io::attila::overlay::CampaignOverlay;

/// The game's constants (RenderDoc, camp_attila; docs/formats/campaign_borders.md): a border's half width and start / end fade lengths,
/// a trade route's half width and colour (240, 200, 20).
const BORDER_HALF_WIDTH: f32 = 0.225;
const BORDER_FADE: f32 = 0.75;
const ROUTE_HALF_WIDTH: f32 = 0.05;
const ROUTE_COLOUR: [f32; 4] = [240.0 / 255.0, 200.0 / 255.0, 20.0 / 255.0, 1.0];

/// What a worker read: the map's factions, the default, and the overlay for the faction asked for.
type Read = (DocumentId, Result<(Vec<FactionInfo>, Option<String>, CampaignOverlay), String>);

#[derive(Default)]
pub(crate) struct CampaignLinesView {
    /// The document shown and the faction its lines are for.
    pub shown: Option<(DocumentId, String)>,
    pub factions: Vec<FactionInfo>,
    pub faction: Option<String>,

    /// The faction's fog of war at the start, when it is shown (the Terrain panel's switch); none: revealed.
    pub fog: Option<Arc<wce_scene::render_scene::ShroudMap>>,
    rx: Option<Receiver<Read>>,
    next_id: u64,
}

impl CampaignLinesView {
    /// Reads a campaign map's lines on a worker (`map`: the campaign's folder name) for `faction` (None: the map's default).
    pub fn start(&mut self, e: &Editor, id: DocumentId, map: &str, faction: Option<String>) {
        let Some(files) = e.files() else { return };
        let Some(schema_path) = wce_foundation::settings::rpfm_schema_path(e.game()) else { return };
        let install = e.settings_ref().game_dir();
        let map = map.to_owned();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = (|| {
                let schema = rpfm_lib::schema::Schema::load(&schema_path, None).map_err(|e| e.to_string())?;
                let list = |p: &str| files.list(p, "");
                let read = |p: &str| files.read(p).ok().map(|b| b.to_vec());
                let startpos = crate::tool::startpos_reader(&files, install.as_deref());
                let (infos, default) = wce_terrain_io::attila::overlay::factions(&schema, &map, &list, &read, &startpos)?;
                let pick = faction.or_else(|| default.clone());
                let o = wce_terrain_io::attila::overlay::read(&schema, &map, pick.as_deref(), &list, &read, &startpos)?;
                Ok((infos, default, o))
            })();
            let _ = tx.send((id, r));
        });
    }

    /// Takes in what the worker read: the scene's lines for the document shown.
    pub fn poll(&mut self, e: &mut Editor, shown: DocumentId, map: &str) {
        let Some(rx) = &self.rx else { return };
        let Ok((id, r)) = rx.try_recv() else { return };
        self.rx = None;
        if id != shown {
            return;
        }

        match r {
            Ok((infos, _, o)) => {
                log::info(format!(
                    "terrain: the campaign's borders for {}: {} chains, {} trade route segments{}",
                    o.faction,
                    o.borders.len(),
                    o.trade_routes.iter().filter(|s| s.drawn).count(),
                    if o.notes.is_empty() { String::new() } else { format!(" ({})", o.notes.join("; ")) }
                ));
                self.factions = infos;
                self.faction = Some(o.faction.clone());
                self.shown = Some((id, o.faction.clone()));
                self.next_id += 1;
                e.scene.campaign_lines = Some(Arc::new(scene_lines(&o, map, self.next_id)));
            }

            Err(err) => log::warn(format!("terrain: no campaign borders: {err}")),
        }
    }

    pub fn clear(&mut self, e: &mut Editor) {
        e.scene.campaign_lines = None;
        self.shown = None;
        self.rx = None;
    }
}

/// The overlay in the view's frame: x as the game's (from the map's west edge), z the game's over `CAMPAIGN_Z_SCALE` (the editor's
/// cells are square; the game's hex world is 2 / sqrt 3 longer north to south). The trade routes drawn are the segments the game makes a
/// draw for (flag byte 0 is 0, fn 0x10aba960).
fn scene_lines(o: &CampaignOverlay, map: &str, id: u64) -> CampaignLines {
    let k = (1.0 / CAMPAIGN_Z_SCALE) as f32;
    let curves = |cs: &[BorderCurve]| -> Vec<OverlayCurve> {
        cs.iter().map(|c| OverlayCurve { cp: c.cp.map(|p| [p[0], p[1] * k]), ts: c.ts, uv: [c.uv_offset, c.uv_scale] }).collect()
    };
    let dir = format!("campaign_maps/{map}/display");
    let mut lines: Vec<OverlayLine> = o
        .trade_routes
        .iter()
        .filter(|s| s.drawn)
        .map(|s| OverlayLine {
            texture: format!("{dir}/trade/textures/dotted_line_trade.dds"),
            colour: ROUTE_COLOUR,
            half_width: ROUTE_HALF_WIDTH,
            length: 0.0,
            fades: [0.0, 0.0],
            curves: curves(&s.curves),
            close_end: true,
        })
        .collect();
    lines.extend(o.borders.iter().map(|b| OverlayLine {
        texture: format!("{dir}/borders/textures/{}.dds", if b.internal { "border_internal_diffuse" } else { "border_diffuse" }),
        colour: [b.rgb[0], b.rgb[1], b.rgb[2], 1.0],
        half_width: BORDER_HALF_WIDTH,
        length: b.length,
        fades: [BORDER_FADE, BORDER_FADE],
        curves: curves(&b.curves),
        close_end: true,
    }));
    CampaignLines { id, lines }
}
