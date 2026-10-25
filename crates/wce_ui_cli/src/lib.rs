//! The tools behind `wce-ui`: one session holds the open layouts and the mounted game files; the command line and the
//! MCP server both call `Session::call`.

pub mod mcp;
pub mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{Value, json};
use wce_ui_model::images::{self, Rgba8Image};
use wce_ui_model::layout::Rect;
use wce_ui_model::related::{self, RelKind, Relation, RelationIndex};
use wce_ui_model::model::EntryKind;
use wce_ui_model::fonts::FontLibrary;
use wce_ui_model::skins;
use wce_ui_model::DrawOptions;
use wce_ui_model::render::{RenderOptions, render};
use wce_ui_model::{Entry, UiModel};
use wce_vfs::{Config, GameProfile, Vfs};

/// What a tool returns: text (JSON or plain) and optionally a PNG.
pub struct Reply {
    pub text: String,
    pub png: Option<Vec<u8>>,
}

impl Reply {
    fn text(s: impl Into<String>) -> Self {
        Self { text: s.into(), png: None }
    }

    fn json(v: Value) -> Self {
        Self { text: serde_json::to_string_pretty(&v).unwrap_or_default(), png: None }
    }
}

#[derive(Clone, Debug)]
enum Origin {
    Disk(PathBuf),
    Pack(String),
}

struct Doc {
    model: UiModel,
    origin: Origin,
    dirty: bool,
}

pub struct Session {
    game_dir: Option<PathBuf>,
    game: &'static GameProfile,
    vfs: Option<Rc<Vfs>>,
    fonts: FontLibrary,
    docs: Vec<(String, Doc)>,
    next_id: u32,
    images: HashMap<String, Option<Rc<Rgba8Image>>>,
    relations: Option<Rc<RelationIndex>>,
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

fn arg_i(a: &Value, k: &str) -> Option<i64> {
    a.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

fn arg_f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

fn arg_b(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| matches!(s, "true" | "yes" | "1"))))
}

fn is_ui_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("ui/") && (!name.contains('.') || name.ends_with(".cml") || name.ends_with(".fc") || name.ends_with(".twui") || name.ends_with(".twui.images"))
}

impl Session {
    pub fn new(game_dir: Option<PathBuf>) -> Self {
        let settings = wce_foundation::settings::Settings::load_default();
        let game_dir = game_dir.or_else(|| settings.game_dir());
        Self { game_dir, game: settings.game_profile(), vfs: None, fonts: FontLibrary::default(), docs: vec![], next_id: 1, images: HashMap::new(), relations: None }
    }

    fn vfs(&mut self) -> Result<&Vfs, String> {
        if self.vfs.is_none() {
            let dir = self.game_dir.clone().ok_or("the game folder is not set: pass --game <folder>, set WCE_GAME, or run the editor once")?;
            let mut cfg = Config::new(dir, self.game.clone());
            if let Ok(o) = std::env::var("WCE_OVERLAY") {
                cfg.overlay_folders = o.split(';').map(str::trim).filter(|s| !s.is_empty()).map(std::path::PathBuf::from).collect();
            }
            // the game's release packs, and the mods named in WCE_MOD_PACKS (; separated file names); not the launcher's list
            cfg.use_mods = false;
            cfg.mod_packs = Some(std::env::var("WCE_MOD_PACKS").map(|m| m.split(';').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect()).unwrap_or_default());
            let v = Vfs::mount(&cfg).map_err(|e| e.to_string())?;
            self.vfs = Some(Rc::new(v));
        }
        Ok(self.vfs.as_deref().unwrap())
    }

    fn doc(&self, id: &str) -> Result<&Doc, String> {
        self.docs.iter().find(|(i, _)| i == id).map(|(_, d)| d).ok_or_else(|| format!("no open layout '{id}' (use open first)"))
    }

    fn doc_mut(&mut self, id: &str) -> Result<&mut Doc, String> {
        self.docs.iter_mut().find(|(i, _)| i == id).map(|(_, d)| d).ok_or_else(|| format!("no open layout '{id}' (use open first)"))
    }

    /// The layout the call names: `id`, or the only open one.
    fn id_of(&self, a: &Value) -> Result<String, String> {
        if let Some(i) = arg_str(a, "id") {
            return Ok(i.to_string());
        }
        match self.docs.as_slice() {
            [(i, _)] => Ok(i.clone()),
            [] => Err("no layout is open (use open first)".into()),
            _ => Err("several layouts are open: pass id".into()),
        }
    }

    /// An entry by index (`3`), by name path (`root/panel/button`) or by name (`button`, must be unique).
    fn resolve(&self, id: &str, v: &Value) -> Result<(usize, Entry), String> {
        let d = self.doc(id)?;
        let ents = d.model.entries();
        if let Some(i) = v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())) {
            return ents.get(i as usize).cloned().map(|e| (i as usize, e)).ok_or_else(|| format!("no entry {i} (the layout has {})", ents.len()));
        }
        let s = v.as_str().ok_or("entry must be an index or a name")?;
        let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
        let matches: Vec<usize> = if parts.len() > 1 {
            (0..ents.len())
                .filter(|&i| {
                    let mut cur = Some(i);
                    for p in parts.iter().rev() {
                        match cur {
                            Some(c) if ents[c].name == *p => cur = ents[c].parent,
                            _ => return false,
                        }
                    }
                    true
                })
                .collect()
        } else {
            (0..ents.len()).filter(|&i| ents[i].name == s).collect()
        };
        match matches.as_slice() {
            [i] => Ok((*i, ents[*i].clone())),
            [] => Err(format!("no entry named '{s}'")),
            m => Err(format!("'{s}' names {} entries ({}); use an index or a longer path", m.len(), m.iter().take(8).map(|i| i.to_string()).collect::<Vec<_>>().join(", "))),
        }
    }

    fn entry_arg(&self, id: &str, a: &Value, key: &str) -> Result<(usize, Entry), String> {
        let v = a.get(key).ok_or_else(|| format!("'{key}' is required"))?;
        self.resolve(id, v)
    }

    /// One line per component. `screen` is where the game puts it (docked components are placed by their dock point, not by the stored offset).
    fn row(i: usize, e: &Entry, r: &Rect) -> String {
        let kind = if e.kind == EntryKind::Template { " [template]" } else { "" };
        let (sx, sy) = (r.x.round() as i32, r.y.round() as i32);
        let moved = if (sx, sy) != (e.ax, e.ay) { format!(" (stored {},{})", e.ax, e.ay) } else { String::new() };
        format!("{}[{}] {}{} ({}) @{},{} {}x{}  screen {},{}{}", "  ".repeat(e.depth), i, e.name, kind, e.class, e.x, e.y, r.w.round() as i32, r.h.round() as i32, sx, sy, moved)
    }

    /// The other layouts that go with the layout `id` (it must come from the packs).
    fn relations_of(&mut self, id: &str) -> Result<Vec<Relation>, String> {
        let path = match &self.doc(id)?.origin {
            Origin::Pack(p) => p.clone(),
            Origin::Disk(_) => return Err("related layouts are found by the layout's pack path: open it from the packs".into()),
        };

        if self.relations.is_none() {
            self.vfs()?;
            let idx = RelationIndex::build(self.vfs.as_deref().unwrap());
            self.relations = Some(Rc::new(idx));
        }

        Ok(self.relations.as_ref().unwrap().relations(&path))
    }

    /// The reference layers for a render: (below, above).
    fn related_layers(&mut self, id: &str, o: &DrawOptions, all: bool, alpha: f32) -> Result<(Vec<wce_ui_model::DrawItem>, Vec<wce_ui_model::DrawItem>), String> {
        let rels: Vec<Relation> = self.relations_of(id)?.into_iter().filter(|r| all || !r.generic).collect();
        let main = self.doc(id)?.model.clone();
        let (mut below, mut above) = (vec![], vec![]);
        for r in rels {
            let Ok(bytes) = self.vfs()?.read(&r.path) else { continue };
            let Ok(other) = UiModel::from_bytes(&bytes) else { continue };
            if let Some(items) = related::layer(&main, o, &r, &other, alpha) {
                match r.kind {
                    RelKind::Child => above.extend(items),
                    RelKind::Parent => below.extend(items),
                }
            }
        }

        Ok((below, above))
    }

    fn find_image(&mut self, path: &str, skin: Option<&str>) -> Option<Rc<Rgba8Image>> {
        let key = format!("{}|{}", skin.unwrap_or(""), path);
        if let Some(c) = self.images.get(&key) {
            return c.clone();
        }
        let mut found = None;
        if self.vfs().is_ok() {
            let vfs = self.vfs.as_deref().unwrap();
            for cand in skins::candidates(path, skin) {
                if let Ok(b) = vfs.read(&cand) {
                    if let Ok(img) = images::decode(&cand, &b) {
                        found = Some(Rc::new(img));
                        break;
                    }
                }
            }
        }
        self.images.insert(key, found.clone());
        found
    }

    /// A skin folder name (`default` is no skin).
    fn resolve_skin(&mut self, name: &str) -> Result<Option<String>, String> {
        if name.is_empty() || name == "default" {
            return Ok(None);
        }

        let list = skins::list(self.vfs()?, None);
        if list.iter().any(|s| s.name == name) {
            return Ok(Some(name.to_string()));
        }

        Err(format!("no skin folder 'ui/skins/{name}' (ui_skins lists them)"))
    }

    fn summary(&self, id: &str) -> Value {
        let d = self.doc(id).unwrap();
        json!({ "id": id, "version": d.model.version, "entries": d.model.entries().len(), "modified": d.dirty, "source": match &d.origin { Origin::Disk(p) => p.display().to_string(), Origin::Pack(p) => format!("pack:{p}") } })
    }

    /// Runs a tool by name (`ui_open`, `ui_tree`, ...; the `ui_` prefix is optional).
    pub fn call(&mut self, tool: &str, a: &Value) -> Result<Reply, String> {
        let t = tool.strip_prefix("ui_").unwrap_or(tool);

        // Any layout tool accepts `source` (a pack path or a file) in place of `id`: the layout is opened on first use.
        if !matches!(t, "files" | "open" | "close" | "list_open") && a.get("id").is_none() {
            if let Some(src) = arg_str(a, "source") {
                let known = self.docs.iter().find(|(_, d)| match &d.origin {
                    Origin::Pack(p) => *p == wce_vfs::normalize(src),
                    Origin::Disk(p) => p == Path::new(src),
                });
                let id = match known {
                    Some((i, _)) => i.clone(),
                    None => {
                        let r = self.call("open", &json!({ "source": src }))?;
                        let v: Value = serde_json::from_str(&r.text).map_err(|e| e.to_string())?;
                        v["id"].as_str().unwrap_or("").to_string()
                    }
                };
                let mut b = a.clone();
                b["id"] = json!(id);
                return self.call(t, &b);
            }
        }

        match t {
            "files" => {
                let filter = arg_str(a, "filter").unwrap_or("").to_lowercase();
                let limit = arg_i(a, "limit").unwrap_or(500) as usize;
                let vfs = self.vfs()?;
                let all: Vec<&str> = vfs.files().keys().map(|s| s.as_str()).filter(|p| is_ui_path(p) && p.to_lowercase().contains(&filter)).collect();
                let shown: Vec<&str> = all.iter().take(limit).copied().collect();
                Ok(Reply::text(format!("{} files{}\n{}", all.len(), if all.len() > shown.len() { format!(" (first {})", shown.len()) } else { String::new() }, shown.join("\n"))))
            }
            "open" => {
                let src = arg_str(a, "source").ok_or("'source' is required: a path in the game packs (ui/campaign ui/x) or a file on disk")?;
                let (bytes, origin) = if Path::new(src).is_file() {
                    (std::fs::read(src).map_err(|e| e.to_string())?, Origin::Disk(PathBuf::from(src)))
                } else {
                    let norm = wce_vfs::normalize(src);
                    let b = self.vfs()?.read(&norm).map_err(|e| format!("{src}: {e}"))?;
                    (b, Origin::Pack(norm))
                };
                let model = if wce_ui_format::looks_like_ui(&bytes) {
                    UiModel::from_bytes(&bytes)?
                } else {
                    UiModel::from_xml(std::str::from_utf8(&bytes).map_err(|_| "not a UI file and not UTF-8 XML")?).map_err(|e| format!("{src}: {e}"))?
                };
                let id = arg_str(a, "id").map(str::to_string).unwrap_or_else(|| {
                    let i = format!("ui{}", self.next_id);
                    self.next_id += 1;
                    i
                });
                self.docs.retain(|(i, _)| *i != id);
                self.docs.push((id.clone(), Doc { model, origin, dirty: false }));
                Ok(Reply::json(self.summary(&id)))
            }
            "close" => {
                let id = self.id_of(a)?;
                self.docs.retain(|(i, _)| *i != id);
                Ok(Reply::text(format!("closed {id}")))
            }
            "list_open" => Ok(Reply::json(Value::Array(self.docs.iter().map(|(i, _)| self.summary(i)).collect()))),
            "xml" => {
                let id = self.id_of(a)?;
                let d = self.doc(&id)?;
                if let Some(e) = a.get("entry") {
                    let (_, en) = self.resolve(&id, e)?;
                    let el = d.model.dom.at(&en.path).ok_or("entry vanished")?;
                    if arg_b(a, "named").unwrap_or(false) {
                        return Ok(Reply::text(d.model.entry_to_named_xml(&en.path).ok_or("entry vanished")?));
                    }
                    let dom = wce_ui_model::Dom { root: el.clone() };
                    Ok(Reply::text(dom.to_xml()))
                } else if arg_b(a, "named").unwrap_or(false) {
                    Ok(Reply::text(d.model.to_named_xml()))
                } else {
                    Ok(Reply::text(d.model.to_xml()))
                }
            }
            "set_xml" => {
                let id = self.id_of(a)?;
                let xml = arg_str(a, "xml").ok_or("'xml' is required")?;
                if let Some(e) = a.get("entry") {
                    let (_, en) = self.resolve(&id, e)?;
                    let d = self.doc_mut(&id)?;
                    let frag_root = d.model.fragment_from_xml(xml)?;
                    if frag_root.name != "uientry" && frag_root.name != "template" {
                        return Err("the replacement must be a <component> (<uientry>) or <template> element".into());
                    }
                    let mut trial = d.model.clone();
                    *trial.dom_mut().at_mut(&en.path).ok_or("entry vanished")? = frag_root;
                    trial.to_bytes().map_err(|e| format!("the replacement does not convert: {e}"))?;
                    d.model = trial;
                } else {
                    let m = UiModel::from_xml(xml)?;
                    let mut check = m.clone();
                    check.to_bytes().map_err(|e| format!("the XML does not convert: {e}"))?;
                    self.doc_mut(&id)?.model = m;
                }
                self.doc_mut(&id)?.dirty = true;
                Ok(Reply::json(self.summary(&id)))
            }
            "tree" => {
                let id = self.id_of(a)?;
                let d = self.doc(&id)?;
                let max = arg_i(a, "max_depth").map(|v| v as usize).unwrap_or(usize::MAX);
                let ents = d.model.entries();
                if ents.is_empty() {
                    return Ok(Reply::text("(this file has no components: it is not a layout)"));
                }
                let rects = d.model.geometry(&ents, &self.draw_options(&id, a)?);
                let lines: Vec<String> = ents.iter().enumerate().filter(|(_, e)| e.depth <= max).map(|(i, e)| Self::row(i, e, &rects[i])).collect();
                Ok(Reply::text(lines.join("\n")))
            }
            "entry" => {
                let id = self.id_of(a)?;
                let (i, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc(&id)?;
                let ents = d.model.entries();
                let kids: Vec<String> = ents.iter().enumerate().filter(|(_, e)| e.parent == Some(i)).map(|(k, e)| format!("[{k}] {}", e.name)).collect();
                let fields: Vec<Value> = d.model.fields(&en.path).into_iter().map(|f| json!({"field": f.idx, "type": f.tag, "label": f.label, "value": f.value})).collect();
                Ok(Reply::json(json!({
                    "index": i, "name": en.name, "class": en.class, "id": en.id,
                    "offset": [en.x, en.y], "size": [en.w, en.h], "screen": [en.ax, en.ay],
                    "parent": en.parent, "children": kids, "properties": props_json(&d.model, &en.path), "fields": fields,
                })))
            }
            "state" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc(&id)?;
                let sp = d.model.default_state_path(&en.path).ok_or("entry has no state")?;
                let fields: Vec<Value> = d.model.fields(&sp).into_iter().map(|f| json!({"field": f.idx, "type": f.tag, "label": f.label, "value": f.value})).collect();
                Ok(Reply::json(json!({ "state_path": sp, "properties": props_json(&d.model, &sp), "fields": fields })))
            }
            "set_field" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let field = arg_i(a, "field").ok_or("'field' (the number from entry/state) is required")? as usize;
                let value = a.get("value").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).ok_or("'value' is required")?;
                let in_state = arg_b(a, "state").unwrap_or(false);
                let d = self.doc_mut(&id)?;
                let path = if in_state { d.model.default_state_path(&en.path).ok_or("entry has no state")? } else { en.path.clone() };
                d.model.set_field(&path, field, &value)?;
                d.dirty = true;
                Ok(Reply::text("ok"))
            }
            "set_property" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let name = arg_str(a, "name").ok_or("'name' (a property name from ui_entry or ui_state) is required")?.to_string();
                let value = a.get("value").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).ok_or("'value' is required")?;
                let d = self.doc_mut(&id)?;
                let path = match arg_str(a, "state") {
                    None | Some("") => {
                        if arg_str(a, "scope") == Some("state") { d.model.default_state_path(&en.path).ok_or("entry has no state")? } else { en.path.clone() }
                    }
                    Some(st) => d.model.state_paths(&en.path).into_iter().find(|(_, n)| n == st).map(|(p, _)| p).ok_or_else(|| format!("the component has no state called {st:?}"))?,
                };

                let props = d.model.properties(&path);
                let p = props
                    .items
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("no property {name:?}; the names are: {}", props.items.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")))?;
                let change = p.edit_from_text(&value)?;
                d.model.edit_prop(&path, p.idx, change)?;
                d.dirty = true;
                Ok(Reply::text("ok"))
            }
            "set_entry" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc_mut(&id)?;
                if a.get("x").is_some() || a.get("y").is_some() {
                    d.model.set_offset(&en.path, arg_i(a, "x").unwrap_or(en.x as i64) as i32, arg_i(a, "y").unwrap_or(en.y as i64) as i32)?;
                }
                if a.get("w").is_some() || a.get("h").is_some() {
                    d.model.set_size(&en.path, arg_i(a, "w").unwrap_or(en.w as i64) as i32, arg_i(a, "h").unwrap_or(en.h as i64) as i32)?;
                }
                if let Some(n) = arg_str(a, "name") {
                    d.model.set_name(&en.path, n)?;
                }
                if let Some(t) = arg_str(a, "text") {
                    d.model.set_text(&en.path, t)?;
                }
                if let Some(c) = arg_str(a, "color") {
                    let h = c.trim_start_matches('#');
                    if h.len() != 6 && h.len() != 8 {
                        return Err("color is RRGGBB or RRGGBBAA".into());
                    }
                    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| "bad color".to_string());
                    d.model.set_state_color(&en.path, [p(0)?, p(2)?, p(4)?, if h.len() == 8 { p(6)? } else { 255 }])?;
                }
                d.dirty = true;
                Ok(Reply::text("ok"))
            }
            "add" => {
                let id = self.id_of(a)?;
                let (_, parent) = self.entry_arg(&id, a, "parent")?;
                let like = match a.get("like") {
                    Some(v) => Some(self.resolve(&id, v)?.1.path),
                    None => None,
                };
                let name = arg_str(a, "name").unwrap_or("new_entry").to_string();
                let d = self.doc_mut(&id)?;
                let p = d.model.add_child(&parent.path, like.as_deref(), &name)?;
                d.dirty = true;
                self.created(&id, &p)
            }
            "duplicate" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc_mut(&id)?;
                let p = d.model.duplicate(&en.path)?;
                d.dirty = true;
                self.created(&id, &p)
            }
            "delete" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc_mut(&id)?;
                d.model.delete(&en.path)?;
                d.dirty = true;
                Ok(Reply::text("deleted"))
            }
            "move" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let d = self.doc_mut(&id)?;
                let p = d.model.move_sibling(&en.path, arg_i(a, "delta").ok_or("'delta' is required (negative: earlier, drawn below)")? as i32)?;
                d.dirty = true;
                self.created(&id, &p)
            }
            "reparent" => {
                let id = self.id_of(a)?;
                let (_, en) = self.entry_arg(&id, a, "entry")?;
                let (_, par) = self.entry_arg(&id, a, "parent")?;
                let keep = arg_b(a, "keep_screen_position").unwrap_or(true);
                let index = arg_i(a, "index").map(|v| v as usize);
                let d = self.doc_mut(&id)?;
                let p = d.model.reparent(&en.path, &par.path, index, keep)?;
                d.dirty = true;
                self.created(&id, &p)
            }
            "render" => {
                let id = self.id_of(a)?;
                let scale = arg_f(a, "scale").unwrap_or(1.0) as f32;
                let model = self.doc(&id)?.model.clone();
                let mut o = RenderOptions::fit(&model, scale);
                if let Some(x) = arg_f(a, "x") {
                    o.origin.0 = x as f32;
                }
                if let Some(y) = arg_f(a, "y") {
                    o.origin.1 = y as f32;
                }
                if let Some(w) = arg_i(a, "width") {
                    o.width = w.clamp(1, 8192) as u32;
                }
                if let Some(h) = arg_i(a, "height") {
                    o.height = h.clamp(1, 8192) as u32;
                }
                o.outlines = arg_b(a, "outlines").unwrap_or(false);
                let skin = match arg_str(a, "skin") {
                    Some(n) => self.resolve_skin(n)?,
                    None => None,
                };
                o.draw = self.draw_options(&id, a)?;
                if let Some((sw, sh)) = o.draw.screen {
                    if arg_i(a, "width").is_none() {
                        o.width = (sw as f32 * scale).ceil().clamp(1.0, 8192.0) as u32;
                    }

                    if arg_i(a, "height").is_none() {
                        o.height = (sh as f32 * scale).ceil().clamp(1.0, 8192.0) as u32;
                    }
                }
                if let Some(s) = a.get("select") {
                    o.selected = Some(self.resolve(&id, s)?.0);
                    // frame the selection unless the caller placed the view
                    if a.get("x").is_none() && a.get("y").is_none() && arg_b(a, "frame").unwrap_or(false) {
                        let ents = model.entries();
                        let r = model.geometry(&ents, &o.draw)[o.selected.unwrap()];
                        o.origin = (r.x - 8.0, r.y - 8.0);
                        o.width = ((r.w + 16.0) * scale).ceil().clamp(16.0, 8192.0) as u32;
                        o.height = ((r.h + 16.0) * scale).ceil().clamp(16.0, 8192.0) as u32;
                    }
                }
                if let Some(name) = arg_str(a, "animation") {
                    let which = a.get("animation_entry").ok_or("'animation_entry' is required with animation")?;
                    let (ei, _) = self.resolve(&id, which)?;
                    let ents = model.entries();
                    if !model.animations(&ents[ei].path).iter().any(|x| x.name == name) {
                        return Err(format!("{} has no animation '{}' (see ui_animations)", ents[ei].name, name));
                    }

                    let t = arg_i(a, "time").map(|v| v.max(0) as u32).unwrap_or(u32::MAX / 2);
                    o.draw.pose = Some(Rc::new(model.pose_at(&ents, ei, name, t)));
                }

                if arg_b(a, "related").unwrap_or(false) {
                    let alpha = arg_f(a, "related_alpha").unwrap_or(0.5).clamp(0.0, 1.0) as f32;
                    let (below, above) = self.related_layers(&id, &o.draw, arg_b(a, "related_all").unwrap_or(false), alpha)?;
                    o.below = below;
                    o.above = above;
                }

                let vfs = self.vfs.clone();
                let mut fonts = std::mem::take(&mut self.fonts);
                let canvas = {
                    let mut resolve = |font: &str, size: i32, cat: &str| vfs.as_deref().and_then(|v| fonts.resolve(v, font, size, cat)).map(|r| (r.cuf, r.color));
                    render(&model, &o, &mut |p| self.find_image(p, skin.as_deref()), &mut resolve)
                };
                self.fonts = fonts;
                let png = canvas.to_png()?;
                let missing = self.images.values().filter(|v| v.is_none()).count();
                let text = format!("{}x{} px, scale {}{}", canvas.w, canvas.h, scale, if missing > 0 { format!("; {missing} picture(s) not found (drawn as magenta outlines)") } else { String::new() });
                if let Some(path) = arg_str(a, "out") {
                    std::fs::write(path, &png).map_err(|e| e.to_string())?;
                    return Ok(Reply { text: format!("{text}; written to {path}"), png: Some(png) });
                }
                Ok(Reply { text, png: Some(png) })
            }
            "skins" => {
                let lines: Vec<String> = skins::list(self.vfs()?, None).iter().map(|s| format!("{} ({} pictures)", s.name, s.files)).collect();
                Ok(Reply::text(lines.join("\n")))
            }
            "related" => {
                let id = self.id_of(a)?;
                let rels = self.relations_of(&id)?;
                if rels.is_empty() {
                    return Ok(Reply::text("no layout in the packs is named after a component of this one, or has a component named after this one"));
                }

                let lines: Vec<String> = rels
                    .iter()
                    .map(|r| format!("{} {} (via component '{}'){}", if r.kind == RelKind::Child { "child " } else { "parent" }, r.path, r.via, if r.generic { " [generic name: probably not a real link]" } else { "" }))
                    .collect();
                Ok(Reply::text(lines.join("\n")))
            }
            "animations" => {
                let id = self.id_of(a)?;
                let d = self.doc(&id)?;
                let ents = d.model.entries();
                let list: Vec<(usize, wce_ui_model::anim::Anim)> = match a.get("entry") {
                    Some(e) => {
                        let (i, en) = self.resolve(&id, e)?;
                        d.model.animations(&en.path).into_iter().map(|x| (i, x)).collect()
                    }
                    None => d.model.all_animations(&ents),
                };

                if list.is_empty() {
                    return Ok(Reply::text("no animations"));
                }

                let mut lines = vec![];
                for (i, an) in &list {
                    lines.push(format!("[{}] {} animation '{}': {} ms, {} frame(s){}", i, ents[*i].name, an.name, an.duration(), an.frames.len(), if an.propagate { ", propagates to descendants" } else { "" }));
                    if a.get("entry").is_some() {
                        for (k, f) in an.frames.iter().enumerate() {
                            let mut what = vec![];
                            for (bit, name) in [(1u32, "colour"), (2, "position"), (4, "size"), (8, "shader"), (0x10, "rotation"), (0x20, "crossfade"), (0x40, "opacity"), (0x80, "text colour")] {
                                if f.mask & bit != 0 {
                                    what.push(name);
                                }
                            }

                            let trig: Vec<String> = f.triggers.iter().map(|t| format!("component {} state '{}' animation '{}' property '{}'", t.component, t.state, t.animation, t.property)).collect();
                            lines.push(format!(
                                "  frame {k}: {} ms, {:?}{}, moves [{}], offset {:?}{}, size {:?}, colour {:?}{}",
                                f.time,
                                f.curve,
                                if f.weight != 1.0 { format!(" weight {}", f.weight) } else { String::new() },
                                what.join(", "),
                                f.offset,
                                if f.absolute { " (absolute)" } else { " (relative)" },
                                f.size,
                                f.color,
                                if trig.is_empty() { String::new() } else { format!(", at the end: {}", trig.join("; ")) }
                            ));
                        }
                    }
                }

                Ok(Reply::text(lines.join("\n")))
            }
            "states" => {
                let id = self.id_of(a)?;
                let d = self.doc(&id)?;
                if let Some(e) = a.get("entry") {
                    let (_, en) = self.resolve(&id, e)?;
                    let names = d.model.entry_state_names(&en.path);
                    let def = d.model.default_state_index(&en.path);
                    let lines: Vec<String> = names.iter().enumerate().map(|(i, n)| format!("{i} {n}{}", if i == def { " (default)" } else { "" })).collect();
                    return Ok(Reply::text(lines.join("\n")));
                }
                let lines: Vec<String> = d.model.state_names().iter().map(|(n, c)| format!("{n}: {c} components")).collect();
                Ok(Reply::text(lines.join("\n")))
            }
            "validate" => {
                let id = self.id_of(a)?;
                let mut m = self.doc(&id)?.model.clone();
                let bytes = m.to_bytes()?;
                let back = UiModel::from_bytes(&bytes)?;
                let same = back.to_xml() == m.to_xml();
                Ok(Reply::text(if same { format!("ok: {} bytes, converts to binary and back to the same XML", bytes.len()) } else { "the binary parses to different XML".into() }))
            }
            "save" => {
                let id = self.id_of(a)?;
                let as_xml = arg_str(a, "format") == Some("xml");
                let (data, name_hint) = {
                    let d = self.doc_mut(&id)?;
                    if as_xml {
                        (d.model.to_xml().into_bytes(), "xml")
                    } else {
                        (d.model.to_bytes()?, "bin")
                    }
                };
                let origin = self.doc(&id)?.origin.clone();
                if let Some(pack) = arg_str(a, "pack") {
                    if as_xml {
                        return Err("a pack holds the binary layout: leave format at bin when saving into a pack".into());
                    }
                    let inside = match arg_str(a, "path") {
                        Some(p) => wce_vfs::normalize(p),
                        None => match &origin {
                            Origin::Pack(p) => p.clone(),
                            _ => return Err("'path' (where the file goes inside the pack) is required for a layout that did not come from the packs".into()),
                        },
                    };
                    wce_ui_model::pack::write_pack(self.game, Path::new(pack), &inside, &data)?;
                    self.doc_mut(&id)?.dirty = false;
                    return Ok(Reply::text(format!("saved {} bytes as {inside} in {pack}", data.len())));
                }
                let out = match arg_str(a, "out") {
                    Some(o) => PathBuf::from(o),
                    None => match &origin {
                        Origin::Disk(p) if !as_xml => p.clone(),
                        _ => return Err(format!("say where to save: 'out' (a file on disk) or 'pack' (a mod pack); format is {name_hint}")),
                    },
                };
                std::fs::write(&out, &data).map_err(|e| format!("{}: {e}", out.display()))?;
                self.doc_mut(&id)?.dirty = false;
                Ok(Reply::text(format!("saved {} bytes to {}", data.len(), out.display())))
            }
            _ => Err(format!("unknown tool '{tool}'")),
        }
    }

    /// The drawing options of a call: `screen`, `state`, `show_hidden`, `show_templates`, `show` / `hide` (lists of components) and `entry_state`.
    fn draw_options(&self, id: &str, a: &Value) -> Result<DrawOptions, String> {
        let mut o = DrawOptions { show_hidden: arg_b(a, "show_hidden").unwrap_or(false), show_templates: arg_b(a, "show_templates").unwrap_or(false), state: arg_str(a, "state").map(str::to_string), ..Default::default() };
        let as_list = |v: &Value| -> Vec<Value> {
            match v {
                Value::Array(x) => x.clone(),
                other => vec![other.clone()],
            }
        };
        if let Some(sz) = arg_str(a, "screen") {
            let (w, h) = sz.to_ascii_lowercase().split_once('x').and_then(|(w, h)| Some((w.trim().parse::<i32>().ok()?, h.trim().parse::<i32>().ok()?))).ok_or("'screen' is like 1920x1080")?;
            if !(64..=8192).contains(&w) || !(64..=8192).contains(&h) {
                return Err("'screen' must be between 64 and 8192 pixels on each side".into());
            }

            o.screen = Some((w, h));
        }

        for (key, on) in [("show", true), ("hide", false)] {
            if let Some(list) = a.get(key) {
                for it in as_list(list) {
                    o.visibility.insert(self.resolve(id, &it)?.1.path, on);
                }
            }
        }

        if let Some(e) = a.get("entry_state") {
            for it in as_list(e) {
                let t = it.as_str().ok_or("entry_state items are 'entry=state' strings")?;
                let (en, st) = t.split_once('=').ok_or("entry_state items are 'entry=state'")?;
                let (_, e) = self.resolve(id, &json!(en.trim()))?;
                let names = self.doc(id)?.model.entry_state_names(&e.path);
                let idx = names.iter().position(|n| n == st.trim()).ok_or_else(|| format!("{} has no state '{}'", e.name, st))?;
                o.entry_state.insert(e.path, idx);
            }
        }

        Ok(o)
    }

    fn created(&self, id: &str, path: &[usize]) -> Result<Reply, String> {
        let d = self.doc(id)?;
        let ents = d.model.entries();
        let i = ents.iter().position(|e| e.path == path).ok_or("the entry vanished")?;
        let rects = d.model.geometry(&ents, &DrawOptions::default());
        Ok(Reply::text(Self::row(i, &ents[i], &rects[i]).trim_start().to_string()))
    }
}


/// The properties of the element at `path` as JSON: the game's name, group, kind of control, shown value and choices.
fn props_json(m: &wce_ui_model::UiModel, path: &[usize]) -> Vec<Value> {
    use wce_ui_model::props::Control;
    m.properties(path)
        .items
        .into_iter()
        .map(|p| {
            let kind = match &p.control {
                Control::Text => "text",
                Control::Int => "integer",
                Control::Float => "number",
                Control::Check => "yes/no",
                Control::Choice(_) => "choice",
                Control::Suggest(_) => "text",
                Control::Bit { .. } => "yes/no",
                Control::Colour => "colour",
                Control::Info => "read-only",
            };
            let choices: Vec<String> = match &p.control {
                Control::Choice(l) => l.iter().map(|(_, s)| s.clone()).collect(),
                Control::Suggest(l) => l.iter().map(|s| s.to_string()).collect(),
                _ => vec![],
            };
            let mut j = json!({"name": p.name, "group": p.group, "kind": kind, "value": p.shown(), "field": p.idx});
            if !choices.is_empty() {
                j["choices"] = json!(choices);
            }

            if !p.help.is_empty() {
                j["help"] = json!(p.help);
            }

            if p.advanced {
                j["advanced"] = json!(true);
            }

            j
        })
        .collect()
}
