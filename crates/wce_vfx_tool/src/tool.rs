//! The tool: the Files panel, the document, the effect playing on the sample map (or the open tile), playback, solo / mute and the
//! preview-only bytes the particle system reads meanwhile.

use std::any::Any;
use std::collections::BTreeSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use wce_editor::editor::Editor;
use wce_editor::shortcuts::{Action, Scope};
use wce_editor::tool::Tool;
use wce_editor::{gui, icons, theme};
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_formats::xml_doc::XmlDoc;
use wce_foundation::math::{Float3, Float4x4, translation};
use wce_scene::render_scene::{DocumentId, EntityId, Payload, SourceRef, VfxEmitter};
use wce_vfs::normalize;
use wce_weather::sample_map_document::SampleMapDocument;
use wce_weather::xml_file_document::{Source, XmlFileDocument};

use crate::model::VfxIndex;

/// The VFX tool's id: the owner of its documents.
pub const VFX_TOOL_ID: &str = "vfx";

const PLAY_PAUSE: &str = "vfx.play_pause";
const RESTART: &str = "vfx.restart";

/// The VFX tool's keys (Settings > Shortcuts).
const SHORTCUTS: &[Action] = &[
    Action {
        id: PLAY_PAUSE,
        scope: Scope::Tool(VFX_TOOL_ID),
        group: "VFX",
        label: "Play / pause the preview",
        defaults: &["Space"],
        held: false,
    },
    Action { id: RESTART, scope: Scope::Tool(VFX_TOOL_ID), group: "VFX", label: "Restart the preview", defaults: &["R"], held: false },
];

const MOD: [f32; 4] = [0.55, 0.85, 1.0, 1.0];

fn contains_ci(hay: &str, needle: &str) -> bool {
    needle.is_empty() || hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// What is muted or soloed: an emitter of the open file (by id) or a reference (by inst_name).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    Emitter(String),
    Reference(String),
}

impl Part {
    pub fn name(&self) -> &str {
        match self {
            Part::Emitter(n) | Part::Reference(n) => n,
        }
    }
}

/// Playback of the preview: the particle clock's rate, a loop that starts the effect again.
#[derive(Clone, Debug)]
pub struct Playback {
    pub playing: bool,
    pub speed: f32,
    pub looping: bool,
    pub loop_s: f32,

    /// Particle seconds since the effect (re)started.
    pub elapsed: f32,
}

impl Default for Playback {
    fn default() -> Self {
        Self { playing: true, speed: 1.0, looping: false, loop_s: 5.0, elapsed: 0.0 }
    }
}

/// What the viewport's move gizmo moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GizmoTarget {
    /// Where the effect plays (preview only).
    #[default]
    Spot,

    /// The focused emitter's SpawnPosition, or the focused reference's position (written to the file).
    Focused,
}

/// A text edit in progress (a curve dragged in the Inspector, a ring or box handle dragged in the viewport): played live through the
/// particle overlay, written to the document as one undo entry when the drag ends.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingText {
    pub elem: usize,
    pub text: String,
}

/// The effect playing: its document, its entity and the effect name it plays.
#[derive(Clone, Debug)]
struct Preview {
    doc: DocumentId,
    entity: EntityId,
    effect: String,
}

pub struct VfxTool {
    // Files.
    pub(crate) filter: String,
    listing: Vec<String>,
    pub(crate) listing_dirty: bool,

    /// The open document's index and the (document, version) it was made for.
    pub(crate) ix: VfxIndex,
    ix_key: (DocumentId, u64, usize),

    // Preview.
    pub(crate) play: Playback,
    pub(crate) spot: Option<Float3>,
    preview: Option<Preview>,
    pub(crate) muted: BTreeSet<Part>,
    pub(crate) solo: Option<Part>,
    override_key: Option<(String, u64)>,
    pub(crate) restart_asked: bool,

    /// The graphics quality variant the Inspector edits (0 the base).
    pub(crate) quality: i32,

    /// The emitter or reference whose handles the viewport shows.
    pub(crate) focus: Option<Part>,
    pub(crate) gizmo_target: GizmoTarget,
    pub(crate) show_handles: bool,

    /// The Inspector shows the focused part's section only (and the effect's).
    pub(crate) only_focused: bool,

    /// The Inspector lists the emitter and modifier properties whose name or label has this (any case).
    pub(crate) prop_filter: String,
    pub(crate) pending: Option<PendingText>,

    /// The handle being dragged in the viewport (gizmos.rs).
    pub(crate) drag: Option<crate::gizmos::HandleDrag>,
    pub(crate) hot_handle: Option<crate::gizmos::Handle>,

    /// The terrains' heights, for the spot.
    probe: wce_editor::coords::GroundProbe,

    tile_was_open: bool,
    camera_set: bool,
    open_after: Option<String>,
}

impl Default for VfxTool {
    fn default() -> Self {
        Self {
            filter: String::new(),
            listing: vec![],
            listing_dirty: true,
            ix: VfxIndex::default(),
            ix_key: (0, u64::MAX, 0),
            play: Playback::default(),
            spot: None,
            preview: None,
            muted: BTreeSet::new(),
            solo: None,
            override_key: None,
            restart_asked: false,
            quality: 0,
            focus: None,
            gizmo_target: GizmoTarget::Spot,
            show_handles: true,
            only_focused: false,
            prop_filter: String::new(),
            pending: None,
            drag: None,
            hot_handle: None,
            probe: Default::default(),
            tile_was_open: false,
            camera_set: false,
            open_after: None,
        }
    }
}

/// The VFX tool's document, when one is open.
pub fn document(e: &Editor) -> Option<&XmlFileDocument> {
    let id = e.docs.of_tool(VFX_TOOL_ID)?;
    e.docs.find_as::<XmlFileDocument>(id)
}

pub fn document_mut(e: &mut Editor) -> Option<&mut XmlFileDocument> {
    let id = e.docs.of_tool(VFX_TOOL_ID)?;
    e.docs.find_as_mut::<XmlFileDocument>(id)
}

/// The in-pack path the particle system reads a document as: its own, or `vfx/<file name>` for a file opened from disk.
pub fn preview_path(d: &XmlFileDocument) -> String {
    let src = d.source();
    if src.vfs_path.is_empty() { format!("vfx/{}", normalize(&d.title())) } else { src.vfs_path.clone() }
}

/// `vfx/a/b.xml` -> `a/b`: the effect name the particle system loads (`vfx/<name>.xml`).
fn effect_name(path: &str) -> String {
    let p = path.strip_prefix("vfx/").unwrap_or(path);
    p.strip_suffix(".xml").unwrap_or(p).to_owned()
}

/// Metres per scene unit.
pub fn units(e: &Editor) -> f32 {
    if e.scene.game_units > 0.0 { e.scene.game_units } else { 1.0 }
}

impl VfxTool {
    /// The index of the open document, made again when it changed.
    pub(crate) fn refresh_index(&mut self, e: &Editor) {
        let Some(d) = document(e) else {
            self.ix = VfxIndex::default();
            self.ix_key = (0, u64::MAX, 0);
            return;
        };

        let key = (d.id(), d.history().version(), d.xml().elements().len());
        if key != self.ix_key {
            self.ix = VfxIndex::new(d.xml());
            self.ix_key = key;
        }
    }

    // ---- the stand-in scene

    fn ensure_sample_map(&mut self, e: &mut Editor) {
        if e.vfs().is_none() || e.docs.shows_scene() {
            return;
        }

        if e.docs.order().iter().any(|&id| e.docs.find_as::<SampleMapDocument>(id).is_some()) {
            return;
        }

        let Some(files) = e.files() else { return };
        let doc = SampleMapDocument::new(&*files, 1, 16);
        if let Err(msg) = e.docs.add(&mut e.scene, Box::new(doc), VFX_TOOL_ID) {
            e.status(&msg, true);
        }
    }

    /// The camera on the effect, close (when no tile brings its own view).
    pub(crate) fn frame_camera(&mut self, e: &mut Editor) {
        let spot = self.spot.unwrap_or_default();
        let u = units(e);
        e.viewport.controller().set_orbit(0.6, 0.3, 14.0 / u, spot + Float3::new(0.0, 2.0 / u, 0.0));
    }

    // ---- the preview

    /// The preview-only bytes of the document: the pending edit, then the mutes. None when the document's own bytes are what plays.
    fn preview_bytes(&self, d: &XmlFileDocument) -> Result<Option<Vec<u8>>, String> {
        let disk = d.source().vfs_path.is_empty();
        let hidden = self.hidden_parts();
        if self.pending.is_none() && hidden.is_empty() && !disk {
            return Ok(None);
        }

        let mut xml: XmlDoc = d.xml().clone();
        if let Some(p) = &self.pending {
            xml = xml.with_inner_text(p.elem, p.text.as_bytes()).map_err(|e| e.to_string())?;
        }

        // Removed from the last element up: the earlier ones keep their indices.
        let ix = VfxIndex::new(&xml);
        let mut cut = hidden
            .iter()
            .filter_map(|p| match p {
                Part::Emitter(id) => ix.emitter(id).map(|e| e.elem),
                Part::Reference(n) => ix.reference(n).map(|r| r.elem),
            })
            .collect::<Vec<_>>();
        cut.sort_unstable();
        for elem in cut.into_iter().rev() {
            xml = xml.removed_element(elem).map_err(|e| e.to_string())?;
        }

        Ok(Some(xml.save()))
    }

    /// The parts that do not play: the muted ones, or all but the soloed one.
    pub(crate) fn hidden_parts(&self) -> Vec<Part> {
        let all = self
            .ix
            .emitters
            .iter()
            .map(|e| Part::Emitter(e.id.clone()))
            .chain(self.ix.refs.iter().map(|r| Part::Reference(r.inst_name.clone())));
        match &self.solo {
            Some(s) => all.filter(|p| p != s).collect(),
            None => all.filter(|p| self.muted.contains(p)).collect(),
        }
    }

    pub(crate) fn plays(&self, p: &Part) -> bool {
        !self.hidden_parts().contains(p)
    }

    fn update_override(&mut self, e: &mut Editor) {
        let Some(d) = document(e) else {
            if let Some((path, _)) = self.override_key.take() {
                e.set_particle_override(&path, None);
            }

            return;
        };

        let path = preview_path(d);
        let mut h = DefaultHasher::new();
        (d.id(), d.history().version(), &self.muted, &self.solo, self.pending.as_ref().map(|p| (p.elem, &p.text))).hash(&mut h);
        let rev = h.finish();
        if self.override_key.as_ref().is_some_and(|(p, r)| *p == path && *r == rev) {
            return;
        }

        let bytes = match self.preview_bytes(d) {
            Ok(b) => b,
            Err(m) => {
                e.status(&format!("preview: {m}"), true);
                None
            }
        };

        if let Some((old, _)) = self.override_key.take().filter(|(p, _)| *p != path) {
            e.set_particle_override(&old, None);
        }

        e.set_particle_override(&path, bytes.map(|b| (rev, b)));
        self.override_key = Some((path, rev));
    }

    /// The effect's entity: made in the document's layer, moved to the spot, made again on a restart.
    fn update_entity(&mut self, e: &mut Editor) {
        let Some((doc, effect)) = document(e).map(|d| (d.id(), effect_name(&preview_path(d)))) else {
            self.preview = None;
            return;
        };

        if self.spot.is_none() {
            self.spot = Some(self.ground_point(e, e.viewport.camera().target));
        }

        let spot = self.spot.unwrap_or_default();

        let world = translation(spot);
        let layer = e.docs.layer_of(doc);
        let stale = self.preview.as_ref().is_none_or(|p| p.doc != doc || p.effect != effect || e.scene.find(p.entity).is_none());
        if self.restart_asked || stale {
            if let Some(p) = self.preview.take()
                && e.scene.find(p.entity).is_some()
            {
                e.scene.remove(p.entity);
            }

            let payload = Payload::VfxEmitter(VfxEmitter { effect: effect.clone(), scale: 1.0 / units(e), mesh: None });
            let id = e.scene.add(layer, world, payload, SourceRef { doc, item: u64::MAX });
            if let Some(en) = e.scene.find_mut(id) {
                en.locked = true;
            }

            self.preview = Some(Preview { doc, entity: id, effect });
            self.restart_asked = false;
            self.play.elapsed = 0.0;
        } else if let Some(p) = &self.preview
            && e.scene.find(p.entity).is_some_and(|en| en.world != world)
        {
            e.scene.update_world(p.entity, world);
        }

        e.scene.set_layer_visible(layer, true);
    }

    /// A point dropped onto the ground (the terrain's; the sample map's plane is at 0).
    fn ground_point(&mut self, e: &Editor, p: Float3) -> Float3 {
        let y = self.probe.ground_at(&e.scene, p.x, p.z).unwrap_or(0.0);
        Float3::new(p.x, y, p.z)
    }

    pub fn restart(&mut self) {
        self.restart_asked = true;
    }

    // ---- panels

    fn draw_files(&mut self, e: &mut Editor) {
        if !gui::begin("Files", None, 0) {
            gui::end();
            return;
        }

        let Some(vfs) = e.vfs().cloned() else {
            gui::text_wrapped("The game packs are not mounted. Check the status line.");
            gui::end();
            return;
        };

        gui::set_next_item_width(-1.0);
        if gui::input_text("##filter", Some("filter"), &mut self.filter, 256, 0) {
            self.listing_dirty = true;
        }

        if self.listing_dirty {
            self.listing = vfs.list("vfx/", ".xml").into_iter().filter(|p| contains_ci(p, &self.filter)).map(str::to_owned).collect();
            self.listing_dirty = false;
        }

        gui::text_colored(theme::dim(), &format!("{} effects", self.listing.len()));
        gui::begin_child("list", gui::v2(0.0, 0.0), 0, 0);
        let mut open = None;
        let listing = &self.listing;
        gui::clipped(listing.len(), |i| {
            let p = &listing[i];
            let is_mod = vfs.owner(p).is_some_and(|o| vfs.packs()[o].is_mod);
            if is_mod {
                gui::push_style_color(gui::COL_TEXT, gui::v4(MOD[0], MOD[1], MOD[2], MOD[3]));
            }

            let shown = p.strip_prefix("vfx/").unwrap_or(p);
            if gui::selectable(shown, false, gui::SELECTABLE_ALLOW_DOUBLE_CLICK) && gui::is_mouse_double_clicked(0) {
                open = Some(p.clone());
            }

            if is_mod {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                let tip = vfs
                    .owners(p)
                    .iter()
                    .enumerate()
                    .map(|(k, &o)| format!("{}{}", vfs.packs()[o].name, if k == 0 { "  (wins)" } else { "" }))
                    .collect::<Vec<_>>();
                gui::set_tooltip(&format!("Double-click to open.\n{}", tip.join("\n")));
            }
        });

        gui::end_child();
        gui::end();
        if let Some(p) = open {
            self.open_after = Some(p);
        }
    }

    fn draw_preview(&mut self, e: &mut Editor) {
        if !gui::begin("VFX preview", None, 0) {
            gui::end();
            return;
        }

        let Some(d) = document(e) else {
            gui::text_colored(theme::dim(), "Open an effect from the Files list (double-click), or File > Open file.");
            gui::end();
            return;
        };

        let title = d.title();
        let path = preview_path(d);

        // Playback.
        let play_label = if self.play.playing { format!("{} Pause", icons::SQUARE) } else { format!("{} Play", icons::PLAY) };
        if gui::button(&play_label) {
            self.play.playing = !self.play.playing;
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Stops or runs the particles' clock (every effect in the scene, the weather's too). Space.");
        }

        gui::same_line();
        if gui::button(&format!("{} Restart", icons::REFRESH_CW)) {
            self.restart();
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Starts the effect again from its first frame. R.");
        }

        gui::same_line();
        gui::set_next_item_width(140.0);
        gui::slider_float("speed", &mut self.play.speed, 0.05, 4.0, "%.2fx");
        gui::same_line();
        gui::checkbox("loop every", &mut self.play.looping);
        if gui::is_item_hovered() {
            gui::set_tooltip("Restarts the effect each time this many seconds of it have played: for effects that play once (bursts).");
        }

        gui::same_line();
        gui::set_next_item_width(90.0);
        gui::drag_float("##loop", &mut self.play.loop_s, 0.05, 0.1, 600.0, "%.1f s");
        gui::same_line();
        gui::text_colored(theme::dim(), &format!("t = {:.1} s", self.play.elapsed));

        // Where it plays.
        gui::separator();
        let mut spot = self.spot.unwrap_or_default();
        let u = units(e);
        let mut m = [spot.x * u, spot.y * u, spot.z * u];
        gui::set_next_item_width(260.0);
        if gui::drag_float3("spot (m)", &mut m, 0.05, "%.2f") {
            spot = Float3::new(m[0] / u, m[1] / u, m[2] / u);
            self.spot = Some(spot);
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("Where the effect plays (preview only, not written to the file). The move gizmo moves it too.");
        }

        gui::same_line();
        if gui::button("Here") {
            self.spot = Some(self.ground_point(e, e.viewport.camera().target));
        }

        if gui::is_item_hovered() {
            gui::set_tooltip("To the ground where the camera looks.");
        }

        gui::same_line();
        if gui::button("Frame") {
            self.frame_camera(e);
        }

        gui::same_line();
        gui::checkbox("handles", &mut self.show_handles);
        if gui::is_item_hovered() {
            gui::set_tooltip(
                "The focused emitter's spawn ring, box, position and direction, or the focused reference's position and scale, drawn \
                 in the viewport: drag their dots to edit them.",
            );
        }

        gui::same_line();
        let mut target = i32::from(self.gizmo_target == GizmoTarget::Focused);
        gui::set_next_item_width(190.0);
        if gui::combo("gizmo", &mut target, &["moves the spot", "moves the focused part"]) {
            self.gizmo_target = if target == 1 { GizmoTarget::Focused } else { GizmoTarget::Spot };
        }

        if gui::is_item_hovered() {
            gui::set_tooltip(
                "What the move gizmo moves: where the effect plays (preview only), or the focused emitter's SpawnPosition / the focused \
                 reference's position (an edit of the file).",
            );
        }

        let lit = e.weather_preview().map_or_else(|| "the default day".to_owned(), str::to_owned);
        gui::text_colored(theme::dim(), &format!("{title}  ({path}), lit by {lit}"));

        // Solo / mute.
        gui::separator();
        gui::text("Parts");
        if gui::is_item_hovered() {
            gui::set_tooltip("Solo plays only that part, mute silences it: the preview only, never written to the file.");
        }

        if self.solo.is_some() || !self.muted.is_empty() {
            gui::same_line();
            if gui::small_button("all on") {
                self.solo = None;
                self.muted.clear();
            }
        }

        let parts = self
            .ix
            .emitters
            .iter()
            .map(|em| (Part::Emitter(em.id.clone()), format!("emitter {} ({})", em.id, em.kind)))
            .chain(
                self.ix.refs.iter().map(|r| (Part::Reference(r.inst_name.clone()), format!("reference {} -> {}", r.inst_name, r.library))),
            )
            .collect::<Vec<_>>();
        for (part, label) in parts {
            gui::push_id(&format!("{part:?}"));
            self.part_buttons(&part);
            gui::same_line();
            let playing = self.plays(&part);
            if self.focus.as_ref() == Some(&part) {
                gui::text_colored(theme::palette().highlight, &label);
            } else if playing {
                gui::text(&label);
            } else {
                gui::text_disabled(&label);
            }

            gui::pop_id();
        }

        // Particle state.
        gui::separator();
        if let Some(r) = e.renderer() {
            let p = r.particles();
            gui::text_colored(theme::dim(), &format!("particles {} / {}", p.alive(), p.capacity()));
            for err in p.errors().iter().take(8) {
                gui::text_colored(theme::error(), err);
            }
        }

        gui::end();
    }

    /// The Solo, Mute and Focus buttons of a part.
    pub(crate) fn part_buttons(&mut self, part: &Part) {
        let soloed = self.solo.as_ref() == Some(part);
        let muted = self.muted.contains(part);
        let (s, m, f) = (soloed, muted, self.focus.as_ref() == Some(part));
        let toggle = |label: &str, on: bool, tip: &str| {
            if on {
                let h = theme::palette().highlight;
                gui::push_style_color(gui::COL_BUTTON, theme::v4([h[0], h[1], h[2], 0.55]));
            }

            let clicked = gui::small_button(label);
            if on {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                gui::set_tooltip(tip);
            }

            clicked
        };

        if toggle("S", s, "Solo: only this part plays (preview only).") {
            self.solo = if s { None } else { Some(part.clone()) };
        }

        gui::same_line();
        if toggle("M", m, "Mute: this part does not play (preview only).") {
            if m {
                self.muted.remove(part);
            } else {
                self.muted.insert(part.clone());
            }
        }

        gui::same_line();
        if toggle(icons::CROSSHAIR, f, "Focus: its handles in the viewport, and the gizmo's target when it moves the focused part.") {
            self.focus = if f { None } else { Some(part.clone()) };
        }
    }

    /// Opens `vfx/<library>.xml` (a reference's library).
    pub(crate) fn open_library(&mut self, library: &str) {
        self.open_after = Some(format!("vfx/{library}.xml"));
    }
}

impl Tool for VfxTool {
    fn id(&self) -> &str {
        VFX_TOOL_ID
    }

    fn title(&self) -> &str {
        "VFX"
    }

    fn icon(&self) -> &str {
        icons::SPARKLES
    }

    fn opens(&self) -> Vec<String> {
        vec![".xml".to_owned()]
    }

    fn opens_under(&self) -> Vec<String> {
        vec!["vfx/".to_owned()]
    }

    fn project_folder(&self) -> &'static str {
        "loose/vfx"
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.2);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.36);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.26);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Files", left);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("VFX preview", bottom);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        crate::commands::register(commands);
    }

    fn lights_scene(&self) -> bool {
        false
    }

    fn shortcuts(&self) -> &'static [wce_editor::shortcuts::Action] {
        SHORTCUTS
    }

    fn activated(&mut self, e: &mut Editor) {
        self.ensure_sample_map(e);
        self.listing_dirty = true;
    }

    fn deactivated(&mut self, e: &mut Editor) {
        if let Some(r) = e.renderer() {
            r.settings.particle_time_scale = 1.0;
        }

        if let Some(p) = &self.preview {
            let layer = e.docs.layer_of(p.doc);
            e.scene.set_layer_visible(layer, false);
        }
    }

    fn game_changed(&mut self, _: &mut Editor) {
        *self = Self::default();
    }

    fn update(&mut self, e: &mut Editor) {
        if self.tile_was_open && !e.docs.shows_scene() {
            self.ensure_sample_map(e);
        }

        self.tile_was_open = e.docs.shows_scene();
        self.refresh_index(e);

        // A part that went (an edit renamed or removed it) is no longer muted, soloed or focused.
        let exists = |ix: &VfxIndex, p: &Part| match p {
            Part::Emitter(id) => ix.emitter(id).is_some(),
            Part::Reference(n) => ix.reference(n).is_some(),
        };
        self.muted.retain(|p| exists(&self.ix, p));
        if self.solo.as_ref().is_some_and(|p| !exists(&self.ix, p)) {
            self.solo = None;
        }

        if self.focus.as_ref().is_some_and(|p| !exists(&self.ix, p)) {
            self.focus = None;
        }

        if self.focus.is_none() {
            self.focus = self.ix.emitters.first().map(|em| Part::Emitter(em.id.clone()));
        }

        // Playback.
        if !gui::io().WantTextInput {
            if wce_editor::shortcuts::pressed(PLAY_PAUSE, false) {
                self.play.playing = !self.play.playing;
            }

            if wce_editor::shortcuts::pressed(RESTART, false) {
                self.restart();
            }
        }

        let dt = gui::io().DeltaTime.max(0.0);
        let rate = if self.play.playing { self.play.speed.max(0.0) } else { 0.0 };
        if let Some(r) = e.renderer() {
            r.settings.particle_time_scale = rate;
        }

        self.play.elapsed += dt.min(0.1) * rate;
        if self.play.looping && self.play.elapsed >= self.play.loop_s.max(0.1) {
            self.restart();
        }

        self.update_override(e);
        let had_preview = self.preview.is_some();
        self.update_entity(e);
        if !had_preview && self.preview.is_some() && !e.docs.shows_scene() && !self.camera_set {
            self.frame_camera(e);
            self.camera_set = true;
        }
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.draw_files(e);
        self.draw_preview(e);
        if let Some(path) = self.open_after.take() {
            match e.open_path_from(self, &path, true, true) {
                Ok(msg) => e.status(&msg, false),
                Err(msg) => e.status(&msg, true),
            }
        }
    }

    fn inspect(&mut self, e: &mut Editor, ins: &mut dyn wce_editor_core::inspector::Inspector) -> bool {
        if document(e).is_none() {
            return false;
        }

        self.refresh_index(e);
        crate::inspect::inspect(self, e, ins);
        true
    }

    fn inspector_subject(&mut self, e: &Editor) -> Option<u64> {
        e.docs.of_tool(VFX_TOOL_ID).map(|id| id as u64)
    }

    fn viewport_overlay(&mut self, e: &mut Editor, origin: gui::Vec2, size: gui::Vec2) {
        crate::gizmos::draw(self, e, origin, size);
    }

    fn viewport_input(&mut self, e: &mut Editor, event: &wce_editor::tool::ViewportEvent) -> bool {
        crate::gizmos::input(self, e, event)
    }

    fn claims_left_button(&self, _: &Editor) -> bool {
        self.drag.is_some() || self.hot_handle.is_some()
    }

    fn tool_gizmo(&self, e: &Editor, dragging: bool) -> Option<Float4x4> {
        document(e)?;
        crate::gizmos::gizmo_frame(self, e, dragging)
    }

    fn tool_gizmo_moved(&mut self, e: &mut Editor, frame: &Float4x4) {
        crate::gizmos::gizmo_moved(self, e, frame);
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        let (source, bytes) = if from_packs {
            // The project's loose copy (where Save writes it) wins over the packs: it is mounted before them.
            let vfs = e.vfs().ok_or("the game packs are not mounted")?;
            let p = normalize(path);
            let owner = vfs.owner(&p).ok_or("not found in the packs")?;
            let bytes = vfs.read(&p).map_err(|e| e.to_string())?;
            let pack = &vfs.packs()[owner];
            let label = if e.loose_copy(&p).is_some() { "project".to_owned() } else { pack.name.clone() };
            (Source { vfs_path: p, disk: pack.path.clone(), label }, bytes)
        } else {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            (Source { vfs_path: String::new(), disk: PathBuf::from(path), label: "loose".to_owned() }, bytes)
        };

        let doc = XmlFileDocument::new(source, bytes, e.loose_dir())?;
        if !doc.xml().roots().iter().any(|&r| matches!(doc.xml().elements()[r].name.as_str(), "vfx" | "vfx_references")) {
            return Err("not a vfx file (no <vfx> or <vfx_references>)".to_owned());
        }

        // A new effect: its own playback state.
        self.muted.clear();
        self.solo = None;
        self.focus = None;
        self.pending = None;
        self.play.elapsed = 0.0;
        self.restart_asked = true;
        Ok(Box::new(doc))
    }

    fn restore(
        &mut self,
        e: &mut Editor,
        copy: &std::path::Path,
        meta: &wce_editor_core::autosave::AutosaveMeta,
    ) -> Result<Option<Box<dyn Document>>, String> {
        let bytes = std::fs::read(copy.join(&meta.inner)).map_err(|err| format!("{}: {err}", copy.display()))?;
        let mut doc = self.open(e, &meta.open, meta.from_packs)?;
        doc.as_any_mut().downcast_mut::<XmlFileDocument>().ok_or("not a vfx file")?.restore_from(bytes)?;
        Ok(Some(doc))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Runs `f` on the VFX tool (a command reaching its tool).
pub(crate) fn with_vfx(e: &mut Editor, f: impl FnOnce(&mut VfxTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<VfxTool, _>(f).unwrap_or_else(|| CommandResult::fail("the VFX tool is not available"))
}
