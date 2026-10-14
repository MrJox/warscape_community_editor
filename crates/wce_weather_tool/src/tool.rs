//! The tool: documents, lighting the scene, the sample map, the Files / Properties / Weather scene panels and the menus.

use std::any::Any;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use wce_editor::editor::Editor;
use wce_editor::gui;
use wce_editor::tool::Tool;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::Document;
use wce_editor_core::selection::Selection;
use wce_formats::c_num::printf_g_prec;
use wce_formats::env_value::{ValueKind, format_value, parse_value};
use wce_formats::xml_doc::{XmlDoc, decode_entities, encode_entities};
use wce_foundation::log;
use wce_foundation::math::Float3;
use wce_render::renderer::View;
use wce_scene::environment::{EnvironmentParams, read_environment, read_graphics_options};
use wce_scene::render_scene::{DocumentId, SourceRef};
use wce_vfs::normalize;
use wce_weather::sample_map_document::SampleMapDocument;
use wce_weather::xml_file_document::{Op, Source, XmlFileDocument};

use crate::picker::WeatherPicker;

/// The environment the sandbox lit the sample map with before an environment document is open: what the gates use.
const DEFAULT_ENVIRONMENT: &str = "weather/charlemagne/med_sp_land_day_dry.environment";

struct Category {
    label: &'static str,
    prefix: &'static str,
    ext: &'static str,
}

const CATEGORIES: [Category; 4] = [
    Category { label: "Battle environments", prefix: "", ext: ".environment" },
    Category { label: "Lighting (campaign, porthole)", prefix: "", ext: ".lighting" },
    Category { label: "Weather definitions", prefix: "weather/", ext: ".xml" },
    Category { label: "VFX", prefix: "vfx/", ext: ".xml" },
];

const VIEWS: [(View, &str); 11] = [
    (View::Final, "Final"), (View::HdrRaw, "HDR raw"), (View::Albedo, "Albedo"), (View::Normal, "Normal"), (View::Specular, "Specular"), (View::Depth, "Depth"),
    (View::SkyCube, "Sky cube"), (View::Bloom, "Bloom"), (View::Quarter, "Quarter"), (View::ToneMapped, "Tone mapped"), (View::Ao, "AO"),
];

/// Renderer view `n` (0 final ... 10 AO), clamped.
pub fn view_of(n: i32) -> View {
    VIEWS[n.clamp(0, 10) as usize].0
}

const MODIFIED: [f32; 4] = [1.0, 0.75, 0.3, 1.0];
const MOD: [f32; 4] = [0.55, 0.85, 1.0, 1.0];
const DIM: [f32; 4] = [0.6, 0.6, 0.6, 1.0];

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn element_label(doc: &XmlDoc, e: usize) -> String {
    let mut label = doc.elements()[e].name.clone();
    if let Some(n) = doc.get(e, "name").or_else(|| doc.get(e, "inst_name")) {
        label += &format!("  '{n}'");
    }

    label
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    needle.is_empty() || hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// A structural edit asked for by a context menu, applied after the tree is drawn (it changes the element indices).
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    op: Op,
    elem: usize,
    attr: usize,
    name: String,
    value: String,
}

/// The in-pack files of open documents the particle system reads (vfx/ and weather definitions), so their edits show live.
type Overlay = Arc<Mutex<HashMap<String, Vec<u8>>>>;

pub struct WeatherTool {
    tile_was_open: bool,

    // Lighting the scene.
    default_env: EnvironmentParams,
    default_env_loaded: bool,
    pub(crate) env_doc: DocumentId,
    pub(crate) blend_doc: DocumentId,
    pub(crate) blend_t: f32,

    /// The game's preferences.script.txt.
    output_gamma: f32,
    output_brightness: f32,

    /// What the viewport rendered last frame (menus read it).
    pub(crate) last_params: EnvironmentParams,
    overlay: Overlay,
    overlay_rev: Option<u64>,

    // File browser.
    category: usize,
    filter: String,
    listing: Vec<String>,
    listing_dirty: bool,

    // Properties tree.
    attr_filter: String,
    new_name: String,
    new_value: String,
    gesture: u64,
    select_after: Option<usize>,
    pending: Option<Pending>,

    // Sample map.
    pub(crate) scene_seed: i32,
    pub(crate) scene_count: i32,

    pub(crate) picker: WeatherPicker,
}

impl Default for WeatherTool {
    fn default() -> Self {
        Self {
            tile_was_open: false, default_env: EnvironmentParams::default(), default_env_loaded: false, env_doc: 0, blend_doc: 0, blend_t: 0.0, output_gamma: 2.2,
            output_brightness: 1.0, last_params: EnvironmentParams::default(), overlay: Overlay::default(), overlay_rev: None, category: 0, filter: String::new(),
            listing: vec![], listing_dirty: true, attr_filter: String::new(), new_name: String::new(), new_value: String::new(), gesture: 1, select_after: None,
            pending: None, scene_seed: 1, scene_count: 16, picker: WeatherPicker::default(),
        }
    }
}

pub(crate) fn find_xml(e: &mut Editor, id: DocumentId) -> Option<&mut XmlFileDocument> {
    if id == 0 { None } else { e.docs.find_as_mut::<XmlFileDocument>(id) }
}

pub(crate) fn active_xml(e: &mut Editor) -> Option<&mut XmlFileDocument> {
    let id = e.docs.active_id();
    find_xml(e, id)
}

fn is_environment_doc(e: &Editor, id: DocumentId) -> bool {
    e.docs.find_as::<XmlFileDocument>(id).is_some_and(|x| x.is_environment())
}

impl WeatherTool {
    /// The document that lights the scene (the last active .environment / .lighting).
    pub fn environment_document<'a>(&self, e: &'a mut Editor) -> Option<&'a mut XmlFileDocument> {
        find_xml(e, self.env_doc)
    }

    pub fn picker(&mut self) -> &mut WeatherPicker {
        &mut self.picker
    }

    /// The sample map is the stand-in scene of this tool: made when nothing else brings a ground (an open tile does), hidden and shown again
    /// by the document manager.
    fn ensure_sample_map(&mut self, e: &mut Editor) {
        if e.vfs().is_none() || e.docs.shows_scene() {
            return;
        }

        if e.docs.order().iter().any(|&id| e.docs.find_as::<SampleMapDocument>(id).is_some()) {
            return;
        }

        self.reroll(e);
    }

    pub(crate) fn reroll(&mut self, e: &mut Editor) {
        let Some(files) = e.files() else { return };
        for id in e.docs.order().to_vec() {
            if e.docs.find_as::<SampleMapDocument>(id).is_some() {
                e.selection.remove_document(id);
                e.docs.close(&mut e.scene, id);
            }
        }

        let doc = SampleMapDocument::new(&*files, self.scene_seed as u32, self.scene_count as usize);
        let ext = doc.info().bounds_max - doc.info().bounds_min;
        let n = doc.info().buildings;
        let previous = e.docs.active_id();
        e.docs.add(&mut e.scene, Box::new(doc), "weather");

        // The environment document being edited stays the active one.
        if previous != 0 {
            e.docs.set_active(previous);
        }

        // With a tile open the camera stays on the tile.
        if !e.docs.shows_scene() {
            e.viewport.use_orbit();
            e.viewport.orbit_mut().set_orbit(0.6, 0.35, 60.0f32.max(ext.x.max(ext.z) * 0.9), Float3::new(0.0, 5.0, 0.0));
        }

        e.status(&format!("Test map: {n} buildings."), false);
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
        if gui::begin_combo("##cat", CATEGORIES[self.category].label) {
            for (i, c) in CATEGORIES.iter().enumerate() {
                if gui::selectable(c.label, i == self.category, 0) {
                    self.category = i;
                    self.listing_dirty = true;
                }
            }

            gui::end_combo();
        }

        gui::set_next_item_width(-1.0);
        if gui::input_text("##filter", Some("filter"), &mut self.filter, 256, 0) {
            self.listing_dirty = true;
        }

        if self.listing_dirty {
            let c = &CATEGORIES[self.category];
            self.listing = vfs.list(c.prefix, c.ext).into_iter().filter(|p| contains_ci(p, &self.filter)).map(str::to_owned).collect();
            self.listing_dirty = false;
        }

        gui::text_colored(DIM, &format!("{} files", self.listing.len()));
        gui::begin_child("list", gui::v2(0.0, 0.0), 0, 0);
        let mut open = None;
        let listing = &self.listing;
        gui::clipped(listing.len(), |i| {
            let p = &listing[i];
            let is_mod = vfs.owner(p).is_some_and(|o| vfs.packs()[o].is_mod);
            if is_mod {
                gui::push_style_color(gui::COL_TEXT, gui::v4(MOD[0], MOD[1], MOD[2], MOD[3]));
            }

            if gui::selectable(p, false, gui::SELECTABLE_ALLOW_DOUBLE_CLICK) && gui::is_mouse_double_clicked(0) {
                open = Some(p.clone());
            }

            if is_mod {
                gui::pop_style_color(1);
            }

            if gui::is_item_hovered() {
                let tip = vfs.owners(p).iter().enumerate().map(|(k, &o)| format!("{}{}", vfs.packs()[o].name, if k == 0 { "  (wins)" } else { "" })).collect::<Vec<_>>();
                gui::set_tooltip(&tip.join("\n"));
            }
        });

        gui::end_child();
        gui::end();
        if let Some(p) = open {
            match e.open_path(&p, true, true) {
                Ok(msg) => e.status(&msg, false),
                Err(msg) => e.status(&msg, true),
            }
        }
    }

    fn draw_scene_bar(&mut self, e: &mut Editor) {
        if !gui::begin("Weather scene", None, 0) {
            gui::end();
            return;
        }

        let Some(r) = e.renderer() else {
            gui::text_disabled("Renderer unavailable (see the status line).");
            gui::end();
            return;
        };

        let mut v = VIEWS.iter().position(|(view, _)| *view == r.view).unwrap_or(0) as i32;
        gui::set_next_item_width(170.0);
        if gui::combo("view", &mut v, &VIEWS.map(|(_, name)| name)) {
            r.view = view_of(v);
        }

        let errors = r.errors().to_vec();
        gui::same_line();
        gui::set_next_item_width(90.0);
        gui::input_int("seed", &mut self.scene_seed);
        gui::same_line();
        gui::set_next_item_width(90.0);
        gui::input_int("count", &mut self.scene_count);
        self.scene_count = self.scene_count.clamp(1, 100);
        gui::same_line();
        if gui::button("Re-roll") {
            self.reroll(e);
        }

        let env_title = find_xml(e, self.env_doc).map(|x| x.title());
        gui::text_colored(DIM, &format!("environment: {}", env_title.as_deref().unwrap_or("(none open: the default dry day)")));

        // Cross-fade: second environment + slider.
        gui::set_next_item_width(260.0);
        let other = if self.blend_doc != 0 && self.blend_doc != self.env_doc { find_xml(e, self.blend_doc).map(|x| x.title()) } else { None };
        if gui::begin_combo("cross-fade with", other.as_deref().unwrap_or("(none)")) {
            if gui::selectable("(none)", other.is_none(), 0) {
                self.blend_doc = 0;
            }

            for id in e.docs.order().to_vec() {
                if id == self.env_doc || !is_environment_doc(e, id) {
                    continue;
                }

                let title = e.docs.find(id).map(|d| d.title()).unwrap_or_default();
                if gui::selectable(&format!("{title}##b{id}"), id == self.blend_doc, 0) {
                    self.blend_doc = id;
                }
            }

            gui::end_combo();
        }

        if other.is_some() {
            gui::same_line();
            gui::set_next_item_width(180.0);
            gui::slider_float("##t", &mut self.blend_t, 0.0, 1.0, "blend %.2f");
        }

        if !errors.is_empty() {
            gui::text_colored([1.0, 0.45, 0.4, 1.0], "render errors (hover)");
            if gui::is_item_hovered() {
                gui::set_tooltip(&errors.iter().map(|e| format!("{e}\n")).collect::<String>());
            }
        }

        gui::end();
    }

    // ---- properties tree (the legacy editor's central view)

    fn draw_properties(&mut self, e: &mut Editor) {
        if !gui::begin("Properties", None, 0) {
            gui::end();
            return;
        }

        let id = e.docs.active_id();
        if find_xml(e, id).is_some() {
            self.draw_document(e, id);
        } else {
            gui::text_colored(DIM, "Open a file from the Files list, or File > Open file.");
        }

        gui::end();
    }

    fn draw_document(&mut self, e: &mut Editor, id: DocumentId) {
        let mut save = false;
        {
            let Some(d) = e.docs.find_as_mut::<XmlFileDocument>(id) else { return };
            let src = d.source().clone();
            gui::text_colored(DIM, &if src.vfs_path.is_empty() { src.disk.display().to_string() } else { src.vfs_path.clone() });
            gui::same_line();
            gui::text_colored(DIM, &format!("  [{}]", src.label));
            if gui::button("Save") {
                save = true;
            }

            gui::same_line();
            gui::begin_disabled(!d.modified());
            if gui::button("Revert all") {
                d.revert_all();
            }

            gui::end_disabled();
            gui::same_line();
            gui::set_next_item_width(220.0);
            gui::input_text("##af", Some("filter attributes"), &mut self.attr_filter, 128, 0);
            gui::same_line();
            gui::text_colored(DIM, &format!("-> {}", d.default_save_path().display()));
        }

        if gui::begin_tab_bar("views") {
            if gui::begin_tab_item("Properties", 0) {
                gui::begin_child("tree", gui::v2(0.0, 0.0), 0, 0);
                if let Some(d) = e.docs.find_as_mut::<XmlFileDocument>(id) {
                    for r in d.xml().roots().to_vec() {
                        self.draw_element(&mut e.selection, d, r);
                    }
                }

                gui::end_child();
                self.apply_pending(e, id);
                gui::end_tab_item();
            }

            if gui::begin_tab_item("Changes", 0) {
                if let Some(d) = e.docs.find_as_mut::<XmlFileDocument>(id) {
                    draw_changes(d);
                }

                gui::end_tab_item();
            }

            gui::end_tab_bar();
        }

        if save {
            let _ = CommandRegistry::run(e, "save");
        }
    }

    fn draw_element(&mut self, sel: &mut Selection, d: &mut XmlFileDocument, e: usize) {
        let el = d.xml().elements()[e].clone();
        let doc = d.id();
        gui::push_id_int(e as i32);
        let any_mod = (0..el.attrs.len()).any(|a| d.xml().attr_modified(e, a));
        if any_mod {
            gui::push_style_color(gui::COL_TEXT, gui::v4(MODIFIED[0], MODIFIED[1], MODIFIED[2], MODIFIED[3]));
        }

        let mut flags = if el.parent.is_none() { gui::TREE_DEFAULT_OPEN } else { 0 };
        let me = SourceRef { doc, item: e as u64 };
        if sel.contains(me) {
            flags |= gui::TREE_SELECTED;
        }

        let open = gui::tree_node_ex_id("##el", flags, &element_label(d.xml(), e));
        if any_mod {
            gui::pop_style_color(1);
        }

        // The Inspector shows it too.
        if gui::is_item_clicked() && !gui::is_item_toggled_open() {
            sel.set(me);
        }

        if self.select_after == Some(e) {
            gui::set_scroll_here_y(0.5);
            self.select_after = None;
        }

        if gui::begin_popup_context_item("elctx") {
            let pending = |op, name: &str, value: &str| Some(Pending { op, elem: e, attr: 0, name: name.to_owned(), value: value.to_owned() });
            gui::text_disabled(&el.name);
            gui::separator();
            if gui::menu_item("Duplicate", None, false, true) {
                self.pending = pending(Op::Duplicate, "", "");
            }

            if gui::menu_item("Delete", None, false, true) {
                self.pending = pending(Op::Remove, "", "");
            }

            if gui::menu_item("Move up", None, false, true) {
                self.pending = pending(Op::MoveUp, "", "");
            }

            if gui::menu_item("Move down", None, false, true) {
                self.pending = pending(Op::MoveDown, "", "");
            }

            if gui::begin_menu("Add child element") {
                gui::set_next_item_width(220.0);
                let go = gui::input_text("##cn", Some("element name, Enter to add"), &mut self.new_name, 96, gui::INPUT_ENTER_RETURNS_TRUE);
                if go && !self.new_name.is_empty() {
                    self.pending = pending(Op::AddChild, &self.new_name, "");
                    self.new_name.clear();
                    gui::close_current_popup();
                }

                gui::end_menu();
            }

            if gui::begin_menu("Add attribute") {
                gui::set_next_item_width(220.0);
                gui::input_text("##an", Some("attribute name"), &mut self.new_name, 96, 0);
                gui::set_next_item_width(220.0);
                let go = gui::input_text("##av", Some("value, Enter to add"), &mut self.new_value, 256, gui::INPUT_ENTER_RETURNS_TRUE);
                if go && !self.new_name.is_empty() {
                    self.pending = pending(Op::AddAttr, &self.new_name, &self.new_value);
                    self.new_name.clear();
                    self.new_value.clear();
                    gui::close_current_popup();
                }

                gui::end_menu();
            }

            if gui::begin_menu("Rename (name attribute)") {
                if gui::is_window_appearing() {
                    self.new_value = d.xml().get(e, "name").unwrap_or_default();
                }

                gui::set_next_item_width(220.0);
                if gui::input_text("##rn", None, &mut self.new_value, 256, gui::INPUT_ENTER_RETURNS_TRUE) {
                    match d.xml().find_attr(e, "name") {
                        Some(k) => {
                            let quote = d.xml().elements()[e].attrs[k].quote;
                            d.set_attr(e, k, encode_entities(self.new_value.as_bytes(), quote), 0);
                        }

                        None => self.pending = pending(Op::AddAttr, "name", &self.new_value),
                    }

                    gui::close_current_popup();
                }

                gui::end_menu();
            }

            gui::end_popup();
        }

        if open {
            let filter = self.attr_filter.clone();
            let shown = |name: &str| name != "name" && contains_ci(name, &filter);
            let has_attrs = el.attrs.iter().any(|a| shown(&a.name));
            if has_attrs && gui::begin_table("attrs", 2, gui::TABLE_SIZING_STRETCH_PROP | gui::TABLE_ROW_BG, gui::v2(0.0, 0.0)) {
                gui::table_setup_column("name", gui::COLUMN_WIDTH_FIXED, 280.0);
                gui::table_setup_column("value", gui::COLUMN_WIDTH_STRETCH, 0.0);
                for (a, attr) in el.attrs.iter().enumerate() {
                    if shown(&attr.name) {
                        self.draw_attribute(d, e, a);
                    }
                }

                gui::end_table();
            }

            // A structural edit of a sibling may have changed the indices: draw the children the element has now.
            let children = d.xml().elements().get(e).map(|el| el.children.clone()).unwrap_or_default();
            for c in children {
                self.draw_element(sel, d, c);
            }

            gui::tree_pop();
        }

        gui::pop_id();
    }

    fn draw_attribute(&mut self, d: &mut XmlFileDocument, e: usize, a: usize) {
        let xml = d.xml();
        let at = xml.elements()[e].attrs[a].clone();
        let mut v = parse_value(&text(&decode_entities(xml.attr_raw(e, a))));
        let modified = xml.attr_modified(e, a);
        let original = xml.original_raw(e, a).to_vec();

        gui::table_next_row();
        gui::table_next_column();
        if modified {
            gui::push_style_color(gui::COL_TEXT, gui::v4(MODIFIED[0], MODIFIED[1], MODIFIED[2], MODIFIED[3]));
        }

        gui::align_text_to_frame_padding();
        gui::text(&at.name);
        if modified {
            gui::pop_style_color(1);
            if gui::is_item_hovered() {
                gui::set_tooltip(&format!("original: {}", text(&original)));
            }
        }

        gui::table_next_column();
        gui::push_id_int(a as i32);
        gui::set_next_item_width(-f32::MIN_POSITIVE);
        let changed = match v.kind {
            ValueKind::Bool => gui::checkbox("##v", &mut v.b),
            ValueKind::Int => gui::drag_i64("##v", &mut v.i, 0.2),
            ValueKind::Float => {
                let speed = (v.f[0].abs() * 0.005).max(0.001);
                gui::drag_float("##v", &mut v.f[0], speed, 0.0, 0.0, "%.6g")
            }

            ValueKind::Rgb => edit_array::<3>(&mut v.f, |f| gui::color_edit3("##v", f, gui::COLOR_FLOAT | gui::COLOR_HDR)),

            // "argb(...)" lists r, g, b, a (verified against the game's fog colour).
            ValueKind::Argb => edit_array::<4>(&mut v.f, |f| gui::color_edit4("##v", f, gui::COLOR_FLOAT | gui::COLOR_HDR | gui::COLOR_ALPHA_BAR)),
            ValueKind::Variance => edit_array::<2>(&mut v.f, |f| gui::drag_float2("##v", f, 0.01, "%.6g")),
            ValueKind::Euler => edit_array::<3>(&mut v.f, |f| gui::drag_float3("##v", f, 0.01, "%.6g")),
            ValueKind::String => {
                let mut buf = v.s.clone();
                let entered = gui::input_text("##v", None, &mut buf, 512, gui::INPUT_ENTER_RETURNS_TRUE);
                if entered {
                    v.s = buf;
                }

                entered
            }
        };

        // A drag or a typing run is one undo entry.
        if gui::is_item_activated() {
            self.gesture += 1;
        }

        if changed {
            let gesture = if v.kind == ValueKind::String { 0 } else { self.gesture };
            d.set_attr(e, a, encode_entities(format_value(&v).as_bytes(), at.quote), gesture);
        }

        if gui::is_item_deactivated() {
            d.history_mut().break_merge();
        }

        if gui::begin_popup_context_item("ctx") {
            if gui::menu_item("Revert to original", None, false, modified) {
                d.set_attr(e, a, original, 0);
            }

            if gui::menu_item("Remove attribute", None, false, true) {
                self.pending = Some(Pending { op: Op::RemoveAttr, elem: e, attr: a, name: String::new(), value: String::new() });
            }

            gui::end_popup();
        }

        gui::pop_id();
    }

    fn apply_pending(&mut self, e: &mut Editor, id: DocumentId) {
        let Some(p) = self.pending.take() else { return };
        let Some(d) = e.docs.find_as_mut::<XmlFileDocument>(id) else { return };
        match d.structural(p.op, p.elem, p.attr, &p.name, &p.value) {
            Err(err) => e.status(&format!("Edit failed: {err}"), true),
            Ok(sel) => {
                self.select_after = sel;

                // Element indices changed.
                e.selection.remove_document(id);
                if let Some(s) = sel {
                    e.selection.set(SourceRef { doc: id, item: s as u64 });
                }
            }
        }
    }
}

/// Edits the first N components of a value through a fixed-size widget.
fn edit_array<const N: usize>(f: &mut [f32], edit: impl FnOnce(&mut [f32; N]) -> bool) -> bool {
    let mut a = [0.0f32; N];
    for (d, s) in a.iter_mut().zip(f.iter()) {
        *d = *s;
    }

    let changed = edit(&mut a);
    for (d, s) in f.iter_mut().zip(a.iter()) {
        *d = *s;
    }

    changed
}

fn draw_changes(d: &XmlFileDocument) {
    let xml = d.xml();
    if !xml.modified() {
        gui::text_colored(DIM, "No changes.");
        return;
    }

    if xml.structural_edits() > 0 {
        gui::text_colored(
            MODIFIED,
            &format!("{} structural edit(s): elements or attributes added, removed or moved. Attribute changes below are relative to the last of them.", xml.structural_edits()),
        );
    }

    if gui::begin_table("chg", 3, gui::TABLE_ROW_BG | gui::TABLE_RESIZABLE | gui::TABLE_SCROLL_Y, gui::v2(0.0, 0.0)) {
        gui::table_setup_column("attribute", 0, 0.0);
        gui::table_setup_column("original", 0, 0.0);
        gui::table_setup_column("new", 0, 0.0);
        gui::table_headers_row();
        for (e, el) in xml.elements().iter().enumerate() {
            for (a, attr) in el.attrs.iter().enumerate() {
                if xml.attr_modified(e, a) {
                    gui::table_next_row();
                    gui::table_next_column();
                    gui::text(&format!("{} @{}", xml.path_of(e), attr.name));
                    gui::table_next_column();
                    gui::text(&text(xml.original_raw(e, a)));
                    gui::table_next_column();
                    gui::text(&text(xml.attr_raw(e, a)));
                }
            }
        }

        gui::end_table();
    }
}

impl Tool for WeatherTool {
    fn id(&self) -> &str {
        "weather"
    }

    fn title(&self) -> &str {
        "Weather"
    }

    fn opens(&self) -> Vec<String> {
        vec![".environment".to_owned(), ".lighting".to_owned(), ".xml".to_owned()]
    }

    fn default_layout(&self, dockspace: gui::ImGuiID) {
        let (left, center) = gui::dock_builder_split(dockspace, gui::DIR_LEFT, 0.22);
        let (right, center) = gui::dock_builder_split(center, gui::DIR_RIGHT, 0.34);
        let (bottom, center) = gui::dock_builder_split(center, gui::DIR_DOWN, 0.22);
        gui::dock_builder_dock_window("Viewport", center);
        gui::dock_builder_dock_window("Files", left);
        gui::dock_builder_dock_window("Outliner", left);
        gui::dock_builder_dock_window("Properties", right);
        gui::dock_builder_dock_window("Inspector", right);
        gui::dock_builder_dock_window("Weather scene", bottom);
        gui::dock_builder_dock_window("Assets", bottom);
        gui::dock_builder_dock_window("Log", bottom);
        gui::dock_builder_dock_window("History", bottom);
    }

    fn register_commands(&self, commands: &mut CommandRegistry<Editor>) {
        crate::commands::register(commands);
    }

    fn lights_scene(&self) -> bool {
        true
    }

    fn activated(&mut self, e: &mut Editor) {
        let Some(vfs) = e.vfs().cloned() else { return };
        let go = read_graphics_options();
        self.output_gamma = go.gamma;
        self.output_brightness = go.brightness;
        if !self.default_env_loaded {
            match vfs.read(DEFAULT_ENVIRONMENT).map_err(|e| e.to_string()).and_then(|b| XmlDoc::parse(b).map_err(|e| e.to_string())) {
                Ok(doc) => {
                    self.default_env = read_environment(&doc);
                    self.default_env_loaded = true;
                }

                Err(err) => log::warn(format!("default environment: {err}")),
            }
        }

        // Precipitation reads open documents first, so edits to vfx/*.xml and weather_definitions.xml show live (`update` keeps the overlay).
        if let Some(r) = e.renderer() {
            let overlay = self.overlay.clone();
            r.particles().set_reader(Box::new(move |path: &str| {
                let p = normalize(path);
                if let Some(bytes) = overlay.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&p) {
                    return Ok(bytes.clone());
                }

                vfs.read(&p).map_err(|e| e.to_string())
            }));
        }

        self.ensure_sample_map(e);
    }

    fn update(&mut self, e: &mut Editor) {
        // The last tile closed: the stand-in scene is back.
        if self.tile_was_open && !e.docs.shows_scene() {
            self.ensure_sample_map(e);
        }

        self.tile_was_open = e.docs.shows_scene();
        let active = e.docs.active_id();
        if is_environment_doc(e, active) {
            self.env_doc = active;
        }

        let env = find_xml(e, self.env_doc).map(|x| read_environment(x.xml()));
        if env.is_none() {
            self.env_doc = 0;
        }

        let other = if self.blend_doc != 0 && self.blend_doc != self.env_doc { find_xml(e, self.blend_doc).map(|x| read_environment(x.xml())) } else { None };
        if other.is_none() && self.blend_doc != 0 && e.docs.find(self.blend_doc).is_none() {
            self.blend_doc = 0;
        }

        let lit = env.is_some();
        let mut params = env.unwrap_or_else(|| self.default_env.clone());
        params.output_gamma = self.output_gamma;
        params.output_brightness = self.output_brightness;
        let slot = &mut e.scene.environment;
        slot.active = Some(params);
        slot.blend = None;
        slot.blend_t = 0.0;
        slot.owner = if lit { self.env_doc } else { 0 };
        if let (true, Some(mut b)) = (lit, other)
            && self.blend_t > 0.0
        {
            b.output_gamma = self.output_gamma;
            b.output_brightness = self.output_brightness;
            slot.blend = Some(b);
            slot.blend_t = self.blend_t;
        }

        self.last_params = slot.resolved();

        // Edit state of the open documents the particle system reads (vfx/ and weather_definitions.xml).
        let mut rev = 0u64;
        let mut live = vec![];
        for &id in e.docs.order() {
            let Some(x) = e.docs.find_as::<XmlFileDocument>(id) else { continue };
            let path = &x.source().vfs_path;
            if path.starts_with("vfx/") || (path.starts_with("weather/") && x.kind() != "environment") {
                let mut h = DefaultHasher::new();
                path.hash(&mut h);
                rev = rev.wrapping_mul(1_000_003).wrapping_add(h.finish()).wrapping_add(x.history().version().wrapping_mul(7919)).wrapping_add(u64::from(x.modified()));
                live.push(id);
            }
        }

        if self.overlay_rev != Some(rev) {
            let mut overlay = self.overlay.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            overlay.clear();
            for id in live {
                if let Some(x) = e.docs.find_as::<XmlFileDocument>(id) {
                    overlay.insert(x.source().vfs_path.clone(), x.xml().save());
                }
            }

            self.overlay_rev = Some(rev);
        }

        if let Some(r) = e.renderer() {
            r.vfx_revision = rev;
        }
    }

    fn draw_panels(&mut self, e: &mut Editor) {
        self.draw_files(e);
        self.draw_properties(e);
        self.draw_scene_bar(e);
        let env = find_xml(e, self.env_doc).map(|x| x.source().vfs_path.clone()).unwrap_or_default();
        let out = e.output_dir();
        self.picker.draw(e, &out, &env);
    }

    fn draw_menus(&mut self, e: &mut Editor) {
        if gui::begin_menu("Settings") {
            let mut buf = e.output_dir().display().to_string();
            gui::text("Output folder (files are saved under their in-pack path):");
            gui::set_next_item_width(600.0);
            if gui::input_text("##out", None, &mut buf, 1024, gui::INPUT_ENTER_RETURNS_TRUE) {
                e.set_output_dir(&PathBuf::from(&buf), true);
            }

            if let Some(r) = e.renderer() {
                gui::separator();
                gui::text("Game graphics options (read from preferences.script.txt):");
                gui::checkbox("Vignette (gfx_vignette)", &mut r.vignette);
                gui::checkbox("Depth of field (gfx_depth_of_field)", &mut r.depth_of_field);
                gui::checkbox("SSAO (gfx_ssao)", &mut r.ssao);
                gui::checkbox("Distortion (gfx_distortion)", &mut r.distortion);
                gui::checkbox("Shadows", &mut r.shadows);
                gui::set_next_item_width(120.0);
                gui::slider_int("Shadow quality (gfx_shadow_quality)", &mut r.shadow_quality, 0, 3);
                gui::checkbox("Lens flare", &mut r.lens_flare);
                gui::drag_float("Gamma (gfx_gamma_setting)", &mut self.output_gamma, 0.01, 1.0, 3.0, "%.3f");
                gui::drag_float("Brightness (gfx_brightness_setting)", &mut self.output_brightness, 0.01, 0.5, 2.0, "%.3f");
            }

            gui::end_menu();
        }

        if gui::begin_menu("Selection") {
            gui::menu_item_toggle("Weather picker (which environment file the game chooses)", &mut self.picker.open);
            gui::end_menu();
        }

        let Some(r) = e.renderer() else { return };
        if !gui::begin_menu("Weather") {
            return;
        }

        let p = r.particles();
        gui::checkbox("Precipitation", &mut p.enabled);
        gui::same_line();
        if gui::button("Restart") {
            p.restart();
        }

        let fx = p.effect();
        let d = fx.definition.clone();
        let emitters = fx.emitters.iter().map(|em| format!("{} / {} / {}", em.library, em.instance, em.name)).collect::<Vec<_>>();
        let notes = fx.notes.clone();
        let (alive, capacity, errors) = (p.alive(), p.capacity(), p.errors().to_vec());
        gui::separator();
        gui::text(&format!("Weather definition: {} ({}, severity {})", if d.name.is_empty() { "(none)" } else { &d.name }, d.kind, d.severity));
        let mut effects = if d.vfx1.is_empty() { "(none)".to_owned() } else { format!("vfx/{}.xml", d.vfx1) };
        if !d.vfx2.is_empty() {
            effects += &format!(", vfx/{}.xml", d.vfx2);
        }

        gui::text(&format!("Effect: {effects}"));
        let random = self.last_params.wind_dir_random;
        gui::text(&format!(
            "Wind direction {:.1} deg{}", r.wind_direction_degrees(&self.last_params), if random { " (wind_dir_random: random per battle)" } else { "" },
        ));
        if random {
            gui::same_line();
            if gui::button("New battle direction") {
                r.wind_dir_seed = r.wind_dir_seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            }
        }

        let w = r.wind();
        gui::text(&format!(
            "Wind {:.3} (min {}, max {}), last gust {:.3} for {:.2} s, next roll in {:.2} s", w.strength, printf_g_prec(f64::from(d.wind_size_min), 3),
            printf_g_prec(f64::from(d.wind_size_max), 3), w.gust_size, w.gust_duration, w.cooldown.max(0.0),
        ));
        gui::text(&format!(
            "VFX wind ({:.2}, {:.2}, {:.2}), g_wind ({:.3}, {:.3})", w.vfx_velocity.x, w.vfx_velocity.y, w.vfx_velocity.z, w.smoothed.x, w.smoothed.y,
        ));
        gui::separator();
        gui::text("Battle (not stored in the weather files):");
        gui::set_next_item_width(200.0);
        gui::combo("Season (g_season_type)", &mut r.season, &["0", "1", "2", "3 (winter in the snow dump)"]);
        gui::checkbox("Snow on the map (g_snow_mask)", &mut r.snow_cover);
        gui::separator();
        gui::text(&format!("Particles: {alive} / {capacity}"));
        for em in &emitters {
            gui::bullet_text(em);
        }

        for n in &notes {
            gui::text_colored([0.9, 0.8, 0.4, 1.0], n);
        }

        for err in &errors {
            gui::text_colored([1.0, 0.4, 0.4, 1.0], err);
        }

        gui::end_menu();
    }

    fn open(&mut self, e: &mut Editor, path: &str, from_packs: bool) -> Result<Box<dyn Document>, String> {
        let (source, bytes) = if from_packs {
            let vfs = e.vfs().ok_or("the game packs are not mounted")?;
            let p = normalize(path);
            let owner = vfs.owner(&p).ok_or("not found in the packs")?;
            let bytes = vfs.read(&p).map_err(|e| e.to_string())?;
            let pack = &vfs.packs()[owner];
            (Source { vfs_path: p, disk: pack.path.clone(), label: pack.name.clone() }, bytes)
        } else {
            let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            (Source { vfs_path: String::new(), disk: PathBuf::from(path), label: "loose".to_owned() }, bytes)
        };

        Ok(Box::new(XmlFileDocument::new(source, bytes, e.output_dir())?))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Runs `f` on the weather tool (a command reaching its tool).
pub(crate) fn with_weather(e: &mut Editor, f: impl FnOnce(&mut WeatherTool, &mut Editor) -> CommandResult) -> CommandResult {
    e.with_tool::<WeatherTool, _>(f).unwrap_or_else(|| CommandResult::fail("the weather tool is not available"))
}
