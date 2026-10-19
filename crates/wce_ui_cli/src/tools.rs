//! The tool list (names, descriptions and JSON schemas) the MCP server announces and `wce-ui help` prints.

use serde_json::{Value, json};

fn prop(kind: &str, desc: &str) -> Value {
    json!({ "type": kind, "description": desc })
}

fn tool(name: &str, desc: &str, props: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut p = serde_json::Map::new();
    for (k, v) in props {
        p.insert(k.to_string(), v);
    }
    json!({ "name": name, "description": desc, "inputSchema": { "type": "object", "properties": p, "required": required } })
}

pub fn list() -> Vec<Value> {
    let mut all = list_plain();
    for t in &mut all {
        if let Some(p) = t["inputSchema"]["properties"].as_object_mut() {
            if p.contains_key("id") && !p.contains_key("source") {
                p.insert("source".into(), prop("string", "Instead of id: a path in the game packs or a file on disk; the layout is opened on first use."));
            }
        }
    }

    all
}

fn list_plain() -> Vec<Value> {
    let id = ("id", prop("string", "The layout to work on. Optional when exactly one layout is open."));
    let entry = |k: &'static str| (k, prop("string", "A component: its number from ui_tree, a name path like root/panel/button, or a unique name."));
    vec![
        tool("ui_files", "List the UI layout files in the game's packs (vanilla and enabled mods), optionally filtered by a substring of the path.", vec![("filter", prop("string", "Substring of the path, case-insensitive.")), ("limit", prop("integer", "Most paths to list (default 500)."))], &[]),
        tool(
            "ui_open",
            "Open a layout for viewing and editing. The source is a path in the game packs (for example 'ui/campaign ui/settlement_panel') or a file on disk: a binary layout, or the XML of one. Binary layouts are converted to XML on open.",
            vec![("source", prop("string", "Pack path or disk path.")), ("id", prop("string", "Name for the open layout (default ui1, ui2, ...)."))],
            &["source"],
        ),
        tool("ui_close", "Close an open layout without saving.", vec![id.clone()], &[]),
        tool("ui_list_open", "List the open layouts.", vec![], &[]),
        tool("ui_tree", "The components of the layout, indented by depth: [number] name (class) @x,y WxH and the position on screen (docked components sit at their dock point plus dock offset, not at the stored offset; the stored one is shown when it differs). Numbers change after add, delete, duplicate, move and reparent; those return the new number.", vec![id.clone(), ("max_depth", prop("integer", "Deepest level to list (the root is 0).")), ("screen", prop("string", "Screen size like '1920x1080': positions as the game docks them there."))], &[]),
        tool("ui_entry", "Details of one component: rectangle, parent, children, its properties under the game's names (group, kind, shown value, choices; set them with ui_set_property) and every primitive field (number, type, the name the converter gives it, value).", vec![id.clone(), entry("entry")], &["entry"]),
        tool("ui_state", "The fields of a component's default state (text, font, colours, shaders), with field numbers for ui_set_field (state=true).", vec![id.clone(), entry("entry")], &["entry"]),
        tool("ui_xml", "The XML of the whole layout, or of one component and everything below it. With named=true the fields carry the names of ui_entry's properties (<offset_x>, <visible>yes</visible>, <component>) instead of the converter's tags and guessed comments; ui_set_xml and from-xml take either form.", vec![id.clone(), entry("entry"), ("named", prop("boolean", "Use the game's names."))], &[]),
        tool(
            "ui_set_xml",
            "Replace the XML of the whole layout, or of one component (a <uientry>/<component> or <template> element), in the converter's form or the named one (see ui_xml). Rejected, leaving the layout unchanged, if it does not parse or does not convert back to a binary layout. Array counts are corrected on save.",
            vec![id.clone(), entry("entry"), ("xml", prop("string", "The XML."))],
            &["xml"],
        ),
        tool(
            "ui_set_entry",
            "Change a component: x/y (offset in its parent), w/h (size of its default state), name, text (of the default state) and color (RRGGBB or RRGGBBAA of the text).",
            vec![id.clone(), entry("entry"), ("x", prop("integer", "")), ("y", prop("integer", "")), ("w", prop("integer", "")), ("h", prop("integer", "")), ("name", prop("string", "")), ("text", prop("string", "")), ("color", prop("string", ""))],
            &["entry"],
        ),
        tool(
            "ui_set_property",
            "Set a property of a component (or of one of its states) by the name ui_entry / ui_state list under 'properties', for example 'Visible', 'Dock point' (a choice by name: 'Bottom centre'), 'Horizontal alignment', 'Interactive', 'Text colour' (#RRGGBB). Check boxes take yes/no. Prefer this to ui_set_field: it knows what the flags and numbers mean.",
            vec![id.clone(), entry("entry"), ("name", prop("string", "Property name.")), ("value", prop("string", "New value.")), ("scope", prop("string", "'component' (default) or 'state' (the default state).")), ("state", prop("string", "Name of a state, instead of the default one."))],
            &["entry", "name", "value"],
        ),
        tool(
            "ui_set_field",
            "Set one primitive field by the number ui_entry (or ui_state with state=true) shows. Integers, floats, yes/no and strings are checked against the field's type.",
            vec![id.clone(), entry("entry"), ("field", prop("integer", "Field number.")), ("value", prop("string", "New value.")), ("state", prop("boolean", "The field belongs to the default state."))],
            &["entry", "field", "value"],
        ),
        tool(
            "ui_add",
            "Add a component under a parent. It is a copy of 'like' (default: the parent's first child) without children, with new ids and the given name. Returns its number.",
            vec![id.clone(), entry("parent"), entry("like"), ("name", prop("string", "Name of the new component."))],
            &["parent"],
        ),
        tool("ui_duplicate", "Copy a component (with everything below it and new ids) right after the original. Returns its number.", vec![id.clone(), entry("entry")], &["entry"]),
        tool("ui_delete", "Delete a component and everything below it.", vec![id.clone(), entry("entry")], &["entry"]),
        tool("ui_move", "Change the paint order among siblings: negative delta moves earlier (drawn below), positive later (on top).", vec![id.clone(), entry("entry"), ("delta", prop("integer", ""))], &["entry", "delta"]),
        tool(
            "ui_reparent",
            "Move a component into another component. By default its place on screen is kept (the offset is recalculated).",
            vec![id.clone(), entry("entry"), entry("parent"), ("index", prop("integer", "Position among the new parent's children (default: last).")), ("keep_screen_position", prop("boolean", ""))],
            &["entry", "parent"],
        ),
        tool(
            "ui_render",
            "Draw the layout to a PNG so you can see it: pictures come from the game's packs, text uses a small built-in font (upper case, no real fonts), behaviour is not shown. Outlines and a highlighted component help to find things.",
            vec![
                id.clone(),
                ("scale", prop("number", "Zoom (default 1).")),
                ("x", prop("number", "Layout x at the left edge.")),
                ("y", prop("number", "Layout y at the top edge.")),
                ("width", prop("integer", "Image width in pixels.")),
                ("height", prop("integer", "Image height in pixels.")),
                ("outlines", prop("boolean", "Outline every component.")),
                entry("select"),
                ("frame", prop("boolean", "With select: crop the view to the selected component.")),
                ("state", prop("string", "Draw this state wherever a component has it (see ui_states); others keep their default state. States belong to single components, there is no global one: prefer entry_state.")),
                ("screen", prop("string", "Size the root to a screen like '1920x1080', as the game does: docked components move with it, the others stay.")),
                ("skin", prop("string", "A skin folder under ui/skins (see ui_skins): pictures come from ui/skins/<skin>/ first.")),
                ("show_hidden", prop("boolean", "Also draw components the layout hides when it loads (tab pages, cards).")),
                ("show_templates", prop("boolean", "Also draw components the game fills in at runtime (names ending in template).")),
                ("show", prop("array", "Components to draw even if hidden.")),
                ("hide", prop("array", "Components to leave out.")),
                ("entry_state", prop("array", "Items like 'button=hover': draw that component in that state.")),
                ("animation", prop("string", "Draw the layout part way through this animation (see ui_animations) of the component in animation_entry.")),
                ("animation_entry", prop("string", "The component that owns the animation: number, name path or unique name.")),
                ("time", prop("integer", "Milliseconds after the animation starts (default: its end). Frames trigger other animations and states as they end.")),
                ("related", prop("boolean", "Also draw the layouts this one is used with, faded, as visual reference only (see ui_related): children over it, parents behind. Generic names (tooltip, background...) are left out unless related_all.")),
                ("related_all", prop("boolean", "With related: include the generic names too.")),
                ("related_alpha", prop("number", "Opacity of the reference layers, 0..1 (default 0.5).")),
                ("out", prop("string", "Also write the PNG to this file.")),
            ],
            &[],
        ),
        tool("ui_skins", "List the UI skins: the folders under ui/skins/. Pass one to ui_render.", vec![], &[]),
        tool(
            "ui_related",
            "Layouts this one is used with, found by name: a child is a layout whose file name is a component of this one (it is created under that component); a parent has a component named like this layout's file. The format does not store the link, so it is a guess; generic names are flagged. Pass related to ui_render to draw them as reference.",
            vec![id.clone()],
            &[],
        ),
        tool(
            "ui_animations",
            "The animations of a component (or of every component): name, length, whether they propagate to descendants and, for one component, the frames (time, easing, what moves, targets, triggers). Frames play one after another; each goes from where the last ended to its target. Edit them with ui_xml / ui_set_xml.",
            vec![id.clone(), entry("entry")],
            &[],
        ),
        tool("ui_states", "Names of the states in the layout and how many components have each; with entry, the states of that component (number, name, which is the default).", vec![id.clone(), entry("entry")], &[]),
        tool("ui_validate", "Check that the layout converts to a binary file and back to the same XML.", vec![id.clone()], &[]),
        tool(
            "ui_save",
            "Save the layout: into a mod pack ('pack', binary; 'path' says where inside the pack, default the place it was opened from), or to a file on disk ('out'; format bin or xml).",
            vec![id, ("pack", prop("string", "Path of a .pack file to write into (created if missing).")), ("path", prop("string", "Path inside the pack.")), ("out", prop("string", "Disk file.")), ("format", prop("string", "bin (default) or xml; a pack always gets bin."))],
            &[],
        ),
    ]
}
