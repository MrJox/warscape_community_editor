//! The Logic panel (docs/plans/cinematic.md, step 7): the battle logic graph on an imnodes canvas (its C API, on the editor's own
//! Dear ImGui context). Event nodes have a "then" output, action nodes a "do" input; a link from an event to an action makes the
//! action react to the event (`wce_cinematic::logic` compiles them to Lua on `lib_generated_battle`). Right-click adds a node,
//! Delete removes the selected ones, dragging a pin makes a link and dropping one off a pin removes it. Every change is an edit of
//! the document (undo / redo); a node's move becomes one when the mouse is released.

use std::collections::HashSet;
use std::ffi::c_int;

use dear_imnodes_sys as nd;
use wce_cinematic::logic::{NODE_TYPES, compile, node_type};
use wce_cinematic::scene::{LogicLink, LogicNode, Project};
use wce_editor::editor::Editor;
use wce_editor::gui::{self, v2};

use crate::scene_ui::{edit, scene_doc};

fn out_pin(node: u32) -> c_int {
    (node * 2) as c_int
}

fn in_pin(node: u32) -> c_int {
    (node * 2 + 1) as c_int
}

fn node_of_pin(pin: c_int) -> u32 {
    (pin / 2) as u32
}

pub struct LogicUi {
    ctx: *mut nd::ImNodesContext,
    /// Nodes whose position the canvas has (set from the project once, then read back).
    placed: HashSet<u32>,
    /// The project's version the canvas was laid out from: an undo moves nodes back.
    laid_out: Option<Vec<(u32, [f32; 2])>>,
    show_lua: bool,
    /// The canvas's top left on screen (grid space starts there, less the panning).
    origin: gui::Vec2,
}

impl Default for LogicUi {
    fn default() -> Self {
        Self { ctx: std::ptr::null_mut(), placed: HashSet::new(), laid_out: None, show_lua: false, origin: v2(0.0, 0.0) }
    }
}

impl Drop for LogicUi {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: made by imnodes_CreateContext, freed once.
            unsafe { nd::imnodes_DestroyContext(self.ctx) };
        }
    }
}

impl LogicUi {
    fn context(&mut self) {
        // SAFETY: the editor's ImGui context is current while panels draw; imnodes keeps a pointer to it.
        unsafe {
            nd::imnodes_SetImGuiContext(dear_imgui_sys::igGetCurrentContext() as *mut _);
            if self.ctx.is_null() {
                self.ctx = nd::imnodes_CreateContext();
                nd::imnodes_SetCurrentContext(self.ctx);
                nd::imnodes_StyleColorsDark(std::ptr::null_mut());
            }
            nd::imnodes_SetCurrentContext(self.ctx);
        }
    }

    pub fn panel(&mut self, e: &mut Editor) {
        gui::set_next_window_size(v2(700.0, 420.0), gui::COND_FIRST_USE_EVER);
        if !gui::begin("Logic", None, 0) {
            gui::end();
            return;
        }
        let Some(p) = scene_doc(e).map(|d| d.project().clone()) else {
            gui::text_disabled("No scene");
            gui::end();
            return;
        };
        gui::checkbox("Lua", &mut self.show_lua);
        if gui::is_item_hovered() {
            gui::set_tooltip("The Lua the graph compiles to (the start script's logic)");
        }
        gui::same_line();
        gui::text_disabled("(?)");
        if gui::is_item_hovered() {
            gui::set_tooltip(if p.logic.nodes.is_empty() {
                "No nodes: the enemy attacks when the intro ends. Right-click the canvas to add one"
            } else {
                "Right-click: add a node · drag pin to pin: link · Delete: remove the selected"
            });
        }
        let avail = gui::content_region_avail();
        let lua_h = if self.show_lua { (avail.y * 0.35).max(80.0) } else { 0.0 };
        if gui::begin_child("##canvas", v2(avail.x, avail.y - lua_h), 0, 0) {
            self.canvas(e, &p);
        }
        gui::end_child();
        if self.show_lua {
            let mut warnings = vec![];
            let mut lua = compile(&p, &mut warnings);
            for w in warnings {
                lua.push_str(&format!("-- warning: {w}\n"));
            }
            gui::input_text_multiline("##lua", &mut lua, v2(-1.0, lua_h - 8.0), dear_imgui_sys::ImGuiInputTextFlags_ReadOnly);
        }
        gui::end();
    }

    fn canvas(&mut self, e: &mut Editor, p: &Project) {
        self.context();
        // An undo or redo that moved nodes: lay them out from the project again.
        let layout: Vec<(u32, [f32; 2])> = p.logic.nodes.iter().map(|n| (n.id, n.position)).collect();
        if self.laid_out.as_ref() != Some(&layout) {
            self.placed.clear();
        }

        let mut changes: Vec<(String, Box<dyn FnOnce(&mut Project)>)> = vec![];
        self.origin = gui::cursor_screen_pos();
        // SAFETY: imnodes calls between Begin/EndNodeEditor on the current contexts, ids stable for the frame.
        unsafe {
            nd::imnodes_BeginNodeEditor();
            for n in &p.logic.nodes {
                let id = n.id as c_int;
                if !self.placed.contains(&n.id) {
                    nd::imnodes_SetNodeGridSpacePos(id, nd::ImVec2_c { x: n.position[0], y: n.position[1] });
                    self.placed.insert(n.id);
                }
                let t = node_type(&n.kind);
                nd::imnodes_BeginNode(id);
                nd::imnodes_BeginNodeTitleBar();
                gui::text(t.map_or(n.kind.as_str(), |t| t.label));
                nd::imnodes_EndNodeTitleBar();
                let event = t.is_none_or(|t| t.event);
                for key in t.map_or(&[][..], |t| t.settings) {
                    let mut v = n.values.get(*key).cloned().unwrap_or_default();
                    gui::set_next_item_width(130.0);
                    if gui::input_text(&format!("{key}##n{}{key}", n.id), None, &mut v, 512, gui::INPUT_ENTER_RETURNS_TRUE) {
                        let (nid, key) = (n.id, (*key).to_owned());
                        changes.push((
                            "logic value".into(),
                            Box::new(move |p: &mut Project| {
                                if let Some(x) = p.logic.nodes.iter_mut().find(|x| x.id == nid) {
                                    x.values.insert(key, v);
                                }
                            }),
                        ));
                    }
                    if gui::is_item_hovered() {
                        gui::set_tooltip(setting_hint(key));
                    }
                }
                if event {
                    nd::imnodes_BeginOutputAttribute(out_pin(n.id), nd::ImNodesPinShape_TriangleFilled);
                    gui::text("then");
                    nd::imnodes_EndOutputAttribute();
                } else {
                    nd::imnodes_BeginInputAttribute(in_pin(n.id), nd::ImNodesPinShape_CircleFilled);
                    gui::text("do");
                    nd::imnodes_EndInputAttribute();
                }
                nd::imnodes_EndNode();
            }
            for (i, l) in p.logic.links.iter().enumerate() {
                nd::imnodes_Link(i as c_int, out_pin(l.from), in_pin(l.to));
            }
            nd::imnodes_MiniMap(0.15, nd::ImNodesMiniMapLocation_TopRight, None, std::ptr::null_mut());
            let hovered = nd::imnodes_IsEditorHovered();
            nd::imnodes_EndNodeEditor();

            // A new link: from an event's output to an action's input (either way round as dragged).
            let (mut sn, mut sa, mut en, mut ea, mut snap) = (0, 0, 0, 0, false);
            if nd::imnodes_IsLinkCreated_IntPtr(&mut sn, &mut sa, &mut en, &mut ea, &mut snap) {
                let (from, to) = if sa % 2 == 0 { (node_of_pin(sa), node_of_pin(ea)) } else { (node_of_pin(ea), node_of_pin(sa)) };
                let ok = p.logic.nodes.iter().any(|n| n.id == from && n.kind.starts_with("event."))
                    && p.logic.nodes.iter().any(|n| n.id == to && n.kind.starts_with("action."));
                if ok && !p.logic.links.iter().any(|l| l.from == from && l.to == to) {
                    changes.push((
                        "logic link".into(),
                        Box::new(move |p: &mut Project| {
                            p.logic.links.push(LogicLink { from, from_pin: "out".into(), to, to_pin: "in".into() })
                        }),
                    ));
                }
            }
            let mut gone: c_int = -1;
            if nd::imnodes_IsLinkDestroyed(&mut gone) && gone >= 0 {
                let i = gone as usize;
                changes.push((
                    "remove logic link".into(),
                    Box::new(move |p: &mut Project| {
                        if i < p.logic.links.len() {
                            p.logic.links.remove(i);
                        }
                    }),
                ));
            }

            // Delete: the selected nodes and their links.
            if hovered && gui::is_key_pressed(gui::KEY_DELETE, false) && nd::imnodes_NumSelectedNodes() > 0 {
                let mut ids = vec![0 as c_int; nd::imnodes_NumSelectedNodes() as usize];
                nd::imnodes_GetSelectedNodes(ids.as_mut_ptr());
                let ids: Vec<u32> = ids.into_iter().map(|i| i as u32).collect();
                nd::imnodes_ClearNodeSelection_Nil();
                changes.push((
                    "delete logic nodes".into(),
                    Box::new(move |p: &mut Project| {
                        p.logic.nodes.retain(|n| !ids.contains(&n.id));
                        p.logic.links.retain(|l| !ids.contains(&l.from) && !ids.contains(&l.to));
                    }),
                ));
            }

            // Moves: kept when the mouse is up again.
            if !gui::is_mouse_down(0) {
                let moved: Vec<(u32, [f32; 2])> = p
                    .logic
                    .nodes
                    .iter()
                    .filter_map(|n| {
                        let q = nd::imnodes_GetNodeGridSpacePos(n.id as c_int);
                        ((q.x - n.position[0]).abs() > 0.5 || (q.y - n.position[1]).abs() > 0.5).then_some((n.id, [q.x, q.y]))
                    })
                    .collect();
                if !moved.is_empty() {
                    changes.push((
                        "move logic nodes".into(),
                        Box::new(move |p: &mut Project| {
                            for (id, at) in moved {
                                if let Some(n) = p.logic.nodes.iter_mut().find(|n| n.id == id) {
                                    n.position = at;
                                }
                            }
                        }),
                    ));
                }
            }

            // Right-click on the canvas: add a node there.
            if hovered && gui::is_mouse_clicked(1) && !nd::imnodes_IsNodeHovered(std::ptr::null_mut()) {
                gui::open_popup("##addnode");
            }
        }
        if gui::begin_popup("##addnode") {
            let mouse = gui::mouse_pos();
            // SAFETY: the editor context is current; panning is in editor space.
            let pan = unsafe { nd::imnodes_EditorContextGetPanning() };
            let origin = self.origin;
            let at = [mouse.x - origin.x - pan.x, mouse.y - origin.y - pan.y];
            for (group, events) in [("Events", true), ("Actions", false)] {
                gui::separator_text(group);
                for t in NODE_TYPES.iter().filter(|t| t.event == events) {
                    if gui::selectable(t.label, false, 0) {
                        let kind = t.kind.to_owned();
                        changes.push((
                            "add logic node".into(),
                            Box::new(move |p: &mut Project| {
                                let id = p.logic.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                                p.logic.nodes.push(LogicNode { id, kind, values: Default::default(), position: at });
                            }),
                        ));
                    }
                }
            }
            gui::end_popup();
        }
        for (name, f) in changes {
            edit(e, &name, f);
        }
        if let Some(p) = scene_doc(e).map(|d| d.project().clone()) {
            self.laid_out = Some(p.logic.nodes.iter().map(|n| (n.id, n.position)).collect());
        }
    }
}

fn setting_hint(key: &str) -> &'static str {
    match key {
        "army" => "The army's number in Scene > Armies (0 is the first)",
        "clip" => "A clip's id (Timeline)",
        "seconds" => "Seconds",
        "distance" => "Metres",
        "threshold" => "0 to 1",
        "advice" => "An advice key (advice_levels / advice_threads)",
        "delay_ms" => "Milliseconds",
        "x" | "z" => "Battle position, metres",
        "radius" => "Metres",
        "event" => "A music event's name",
        "fade" => "Seconds",
        "name" => "A message name",
        "code" => "Lua run when the event comes",
        _ => "",
    }
}
