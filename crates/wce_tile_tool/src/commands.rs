//! The tile tool's commands: everything the panels and the mouse do (the old Tile Studio script flags), plus test commands (assert_*).
//!
//! | Studio flag | command |
//! |---|---|
//! | `--tile <folder>` | `tile <folder>` |
//! | `--paint k x z n` | `paint k x z n` |
//! | `--select t\|n` | `tile_select t\|n` |
//! | `--move dx dz` | `tile_move dx dz` |
//! | `--duplicate`, `--delete`, `--rotate d`, `--add key` | `duplicate`, `delete_item`, `rotate d`, `add_building key` |
//! | `--add-polygon s`, `--polygon-point op i x z` | `add_polygon s`, `polygon_point op i x z` |
//! | `--new-layer n`, `--move-to-layer n` | `new_layer n`, `move_to_layer n` |
//! | `--save-to dir`, `--undo n` / `--redo n` | `save_to dir`, `undo` / `redo` (the editor's, one step each) |
//!
//! The app maps the old flag names to these commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wce_editor::editor::Editor;
use wce_editor_core::command_registry::{CommandRegistry, CommandResult};
use wce_editor_core::document::{Document, SaveTarget};
use wce_formats::tile_index::TileIndex;
use wce_foundation::math::Float3;
use wce_tile::world_transform::world_transform;
use wce_tile_editor::terrain_bake::TerrainView;
use wce_tile_editor::tile_document::{BrushKind, Placement, TileDocument};

use wce_tile::property_enums::WALL_BASE;
use crate::tool::{HINTS, MODE_NAMES, TileMode, TileTool, document, matches_words, primary, select_elem, selected, with_tile};

/// An inspector that writes the rows it is given, with their values (for `tile_inspect`).
#[derive(Default)]
struct DumpInspector {
    target: wce_editor_core::inspector::InspectTarget,
    lines: Vec<String>,

    /// One edit to apply to the row with this name (`tile_edit`): the value as text (a number, true / false, text, a choice's number or
    /// name, `r,g,b,a`, `Name=true` for a row of check boxes).
    edit: Option<(String, String)>,
    applied: bool,

    /// Text rows applied and not yet ended.
    ended: bool,
}

impl DumpInspector {
    fn wants(&mut self, field: &str, label: &str) -> Option<String> {
        let (f, v) = self.edit.as_ref()?;
        if f != field && f != label {
            return None;
        }

        let v = v.clone();
        self.applied = true;
        Some(v)
    }
}

impl wce_editor_core::inspector::Inspector for DumpInspector {
    fn target(&self) -> wce_editor_core::inspector::InspectTarget {
        self.target
    }

    fn set_target(&mut self, target: wce_editor_core::inspector::InspectTarget) {
        self.target = target;
    }

    fn section(&mut self, title: &str) {
        self.lines.push(format!("== {title}"));
    }

    fn mark_fields(&mut self, marks: Vec<(String, String)>) {
        for (f, note) in marks {
            self.lines.push(format!("  ! {f}: {note}"));
        }
    }

    fn text(&mut self, _: &str, label: &str, value: &str) {
        self.lines.push(format!("  {label}: {value}  (read-only)"));
    }

    fn edit_float(&mut self, field: &str, label: &str, v: &mut f32, _: f32, lo: f32, hi: f32) -> bool {
        self.lines.push(format!("  {label}: {v}  [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float3(&mut self, field: &str, label: &str, v: &mut [f32; 3], _: f32) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<3>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_float2(&mut self, field: &str, label: &str, v: &mut [f32; 2], _: f32) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<2>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_int(&mut self, field: &str, label: &str, v: &mut i64, _: f32) -> bool {
        self.lines.push(format!("  {label}: {v}"));
        match self.wants(field, label).and_then(|t| t.parse::<i64>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_color3(&mut self, field: &str, label: &str, v: &mut [f32; 3]) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<3>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_color4(&mut self, field: &str, label: &str, v: &mut [f32; 4]) -> bool {
        self.lines.push(format!("  {label}: {v:?}"));
        match self.wants(field, label).and_then(|t| parse_floats::<4>(&t)) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_bool(&mut self, field: &str, label: &str, v: &mut bool) -> bool {
        self.lines.push(format!("  {label}: {v}"));
        match self.wants(field, label).as_deref() {
            Some("true" | "1") => {
                *v = true;
                true
            }

            Some("false" | "0") => {
                *v = false;
                true
            }

            _ => false,
        }
    }

    fn edit_string(&mut self, field: &str, label: &str, v: &mut String) -> bool {
        self.lines.push(format!("  {label}: '{v}'"));
        match self.wants(field, label) {
            Some(x) => {
                *v = x;
                self.ended = true;
                true
            }

            None => false,
        }
    }

    fn edit_choice(&mut self, field: &str, label: &str, index: &mut i32, choices: &[String]) -> bool {
        let shown = usize::try_from(*index).ok().and_then(|i| choices.get(i)).map_or("?", String::as_str);
        self.lines.push(format!("  {label}: {shown}  ({} choices)", choices.len()));
        let Some(t) = self.wants(field, label) else { return false };
        match t.parse::<i32>().ok().or_else(|| choices.iter().position(|c| *c == t).map(|i| i as i32)) {
            Some(i) => {
                *index = i;
                true
            }

            None => false,
        }
    }

    fn gesture_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    fn edit_slider(&mut self, field: &str, label: &str, v: &mut f32, lo: f32, hi: f32) -> bool {
        self.lines.push(format!("  {label}: {v}  slider [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_int_slider(&mut self, field: &str, label: &str, v: &mut i64, lo: i64, hi: i64) -> bool {
        self.lines.push(format!("  {label}: {v}  slider [{lo} .. {hi}]"));
        match self.wants(field, label).and_then(|t| t.parse::<i64>().ok()) {
            Some(x) => {
                *v = x;
                true
            }

            None => false,
        }
    }

    fn edit_angle(&mut self, field: &str, label: &str, v: &mut f32, max: f32) -> bool {
        self.lines.push(format!("  {label}: {:.2} deg  [0 .. {:.0}]", v.to_degrees(), max.to_degrees()));

        // The value is in degrees, as typed in the box.
        match self.wants(field, label).and_then(|t| t.parse::<f32>().ok()) {
            Some(x) => {
                *v = x.to_radians();
                true
            }

            None => false,
        }
    }

    fn edit_checks(&mut self, field: &str, label: &str, names: &[&str], on: &mut [bool]) -> bool {
        let set = names.iter().zip(on.iter()).map(|(n, o)| format!("{n}={o}")).collect::<Vec<_>>().join(" ");
        self.lines.push(format!("  {label}: {set}"));
        let Some(t) = self.wants(field, label) else { return false };
        let Some((name, value)) = t.split_once('=') else { return false };
        match (names.iter().position(|n| *n == name), value) {
            (Some(i), "true") => on[i] = true,
            (Some(i), "false") => on[i] = false,
            _ => return false,
        }

        true
    }
}

/// `a,b,c` as numbers.
fn parse_floats<const N: usize>(text: &str) -> Option<[f32; N]> {
    let v = text.split(',').map(|x| x.trim().parse::<f32>().ok()).collect::<Option<Vec<_>>>()?;
    v.try_into().ok()
}

fn ok(m: impl Into<String>) -> CommandResult {
    CommandResult::success(m)
}

fn fail(m: impl Into<String>) -> CommandResult {
    CommandResult::fail(m)
}

/// The whole string as a float (strtof with nothing left over).
fn number(s: &str) -> Option<f32> {
    if s.is_empty() { None } else { s.parse().ok() }
}

/// C's atoi: the leading integer, 0 when there is none.
fn atoi(s: &str) -> i64 {
    wce_formats::c_num::strtol(s).unwrap_or(0)
}

/// `std::to_string` of a float: `%f`.
fn f6(v: f32) -> String {
    format!("{v:.6}")
}

fn no_tile() -> CommandResult {
    fail("no tile open")
}

/// Runs `f` with the tile tool and the active tile.
fn with_doc(e: &mut Editor, f: impl FnOnce(&mut TileTool, &mut Editor) -> CommandResult) -> CommandResult {
    if document(e).is_none() {
        return no_tile();
    }

    with_tile(e, f)
}

/// Runs the `mouse` command for a command that drives the scripted mouse: the caller returns `message` and the frames the mouse needs (the
/// C++ registry kept the nested command's wait).
fn mouse(e: &mut Editor, args: &[String], message: impl Into<String>) -> CommandResult {
    let r = CommandRegistry::run_args(e, "mouse", args);
    if r.ok { ok(message).wait(r.wait_frames) } else { r }
}

/// Clicks the viewport pixel of a world point with the scripted mouse; None when the point is not in view.
fn click_at(e: &mut Editor, p: Float3, message: impl Into<String>) -> Option<CommandResult> {
    let (x, y) = e.viewport.project(p)?;
    Some(mouse(e, &["click".to_owned(), format!("{x:.1}"), format!("{y:.1}")], message))
}

pub fn register(c: &mut CommandRegistry<Editor>) {
    c.add(
        "tile", "tile <entry xml | tile name | folder | tile.ted> [variation]",
        "open a tile by its database entry (a TILES/*.xml, or a name like assembly_kit/osgiliath_test), by a variation folder or by its tile.ted, and switch to the tile tool",
        |e, a| {
            let Some(target) = a.first() else { return fail("usage: tile <entry xml | tile name | folder | tile.ted> [variation]") };
            let variation = a.get(1).map_or("", String::as_str);
            with_tile(e, |t, e| t.open_tile(e, target, variation).map_or_else(fail, ok))
        },
    );

    c.add(
        "open_exported", "open_exported <exported tile folder | terrain/tiles/... in the packs> [workspace folder] [--fresh]",
        "open an exported tile: decompile it into a TEd project in the workspace (the Tile settings' workspace) and open that; a pack tile \
         decompiled before opens the workspace's copy unless --fresh",
        |e, a| {
            let fresh = a.iter().any(|x| x == "--fresh");
            let a = a.iter().filter(|x| *x != "--fresh").collect::<Vec<_>>();
            let Some(target) = a.first() else { return fail("usage: open_exported <exported tile folder | terrain/tiles/...> [workspace folder] [--fresh]") };
            let workspace = a.get(1).map(PathBuf::from);
            with_tile(e, |t, e| t.open_exported(e, target, workspace.as_deref(), fresh).map_or_else(fail, ok))
        },
    );

    c.add("tile_browse", "tile_browse <kit number | packs> [filter words]", "show a source in the Tiles panel (a kit, or the game packs' exported tiles) with a filter", |e, a| {
        let Some(source) = a.first() else { return fail("usage: tile_browse <kit number | packs> [filter words]") };
        let filter = a[1..].join(" ");
        with_tile(e, |t, e| t.browse(e, source, &filter).map_or_else(fail, ok))
    });

    c.add(
        "save_exported", "save_exported [output folder]",
        "save the open tile, then write it as an exported tile (stages A, B and C) and the tile database under the folder (default: the output folder)",
        |e, a| {
            let out = a.first().map_or_else(|| e.output_dir(), PathBuf::from);
            with_tile(e, |t, e| t.save_exported(e, &out).map_or_else(fail, ok))
        },
    );

    c.add(
        "map_context", "map_context <map | campaign> [placement number | cell <x> <y> <rotation degrees>] [near cells] [mid cells] [far cells]",
        "show the open tile among its neighbours in a battle map, streamed around the camera (waits for the chunks around it); 'campaign' is the game's campaign battle map",
        |e, a| {
            let Some(name) = a.first() else { return fail("usage: map_context <map | campaign> [placement | x y rotation] [near] [mid] [far]") };
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            with_tile(e, |t, e| {
                use crate::context_panel::{GAME_MAP, load_map, location_names};
                let Some(raw) = document(e).and_then(|d| d.project().raw_root.clone()) else { return fail("the tile is not in a kit") };
                let files = e.files();
                let ui = &mut t.context;
                ui.refresh_maps(&raw, files.as_deref());
                let want = if name == "campaign" { GAME_MAP.to_owned() } else { name.clone() };
                let Some(k) = ui.maps.iter().position(|m| *m == want) else { return fail(format!("no map {want}; maps: {}", ui.maps.join(", "))) };
                ui.map = k as i32;
                if ui.loaded.as_ref().is_none_or(|l| l.0 != want) {
                    let db = match wce_tile::battle_map::tile_db::TileDatabase::load(&raw.join("terrain/tiles/battle/_tile_database")) {
                        Ok(db) => db,
                        Err(err) => return fail(err),
                    };

                    match load_map(&want, &raw, &db, files.as_deref()) {
                        Ok((m, l)) => ui.loaded = Some((want.clone(), m, l)),
                        Err(err) => return fail(err),
                    }
                }

                let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
                ui.find_placements(d);
                let cell = a.get(1).is_some_and(|w| w == "cell");
                let rest = &a[if cell { 2 } else { 1 }..];
                let nums: Vec<i32> = rest.iter().map(|v| atoi(v) as i32).collect();
                let used = if cell || ui.placements.is_empty() {
                    if nums.len() < 3 && cell {
                        return fail("usage: map_context <map> cell <x> <y> <rotation degrees> [near] [mid] [far]");
                    }

                    ui.choice = ui.placements.len();
                    if nums.len() >= 3 {
                        ui.manual = [nums[0], nums[1], nums[2] / 90];
                        3
                    } else {
                        0
                    }
                } else if let Some(&n) = nums.first() {
                    ui.choice = (n.max(0) as usize).min(ui.placements.len() - 1);
                    1
                } else {
                    ui.choice = 0;
                    0
                };

                for (k, ring) in [&mut ui.near, &mut ui.mid, &mut ui.far].into_iter().enumerate() {
                    if let Some(&r) = nums.get(used + k) {
                        *ring = r.max(1);
                    }
                }

                ui.shown = true;
                let Some(rq) = ui.request(d) else { return fail("no request (no map or no kit)") };
                let s = wce_tile_editor::tile_document::context::Streamer::new(rq, files);
                ui.build += 1;
                ui.begin(e, id, Box::new(s));
                ui.settle(e);
                let ui = &t.context;
                let (_, map, loc) = ui.loaded.as_ref().expect("a map");
                let places: Vec<String> = ui.placements.iter().map(|&i| {
                    let p = &map.placements[i];
                    format!("({}, {}) {:#x} {}", p.x, p.y, p.rotation, location_names(loc.as_ref(), p.x, p.y))
                }).collect();
                ok(format!("{}; placements: [{}]", ui.status, places.join("; ")))
            })
        },
    );

    c.add("map_context_async", "map_context_async <map | campaign>", "test: what the Map context section does when shown with a map (reads and builds on the worker thread)", |e, a| {
        let Some(name) = a.first() else { return fail("usage: map_context_async <map | campaign>") };
        with_tile(e, |t, e| {
            let Some(raw) = document(e).and_then(|d| d.project().raw_root.clone()) else { return fail("the tile is not in a kit") };
            let files = e.files();
            let ui = &mut t.context;
            ui.refresh_maps(&raw, files.as_deref());
            let want = if name == "campaign" { crate::context_panel::GAME_MAP.to_owned() } else { name.clone() };
            let Some(k) = ui.maps.iter().position(|m| *m == want) else { return fail(format!("no map {want}")) };
            ui.map = k as i32;
            ui.shown = true;
            ui.loaded = None;
            ui.start_map(&raw, files);
            ok("started")
        })
    });

    c.add("map_context_settle", "map_context_settle", "wait until the map context's chunks around the camera are on the scene; says what it has", |e, _| {
        with_tile(e, |t, e| {
            if t.context.streamer.is_none() {
                return fail("no map context");
            }

            t.context.settle(e);
            ok(t.context.status.clone())
        })
    });

    c.add("map_context_snow", "map_context_snow on|off [amount]", "the season's snow where the map has it (the Map context section's switch and amount)", |e, a| {
        let on = match a.first().map(String::as_str) {
            Some("on") => true,
            Some("off") => false,
            _ => return fail("usage: map_context_snow on|off [amount]"),
        };

        let amount = a.get(1).and_then(|v| v.parse::<f32>().ok());
        with_tile(e, |t, e| {
            t.context.snow_on = on;
            if let Some(amount) = amount {
                t.context.snow_amount = amount.clamp(0.0, 1.0);
            }

            t.context.stream(e);
            ok(format!("snow {}", if e.scene.snow().is_some() { "where the map says" } else { "off (or none on the map this season)" }))
        })
    });

    c.add("map_context_cache", "map_context_cache clear", "empty the map context's cache of opened tiles", |_, a| {
        if a.first().is_none_or(|v| v != "clear") {
            return fail("usage: map_context_cache clear");
        }

        let dir = wce_tile_editor::tile_document::context::cache::cache_dir();
        ok(format!("{} cached tiles removed from {}", wce_tile_editor::tile_document::context::cache::clear(&dir), dir.display()))
    });

    c.add("map_context_off", "map_context_off", "take the map context off the scene", |e, _| {
        with_tile(e, |t, e| {
            t.context.shown = false;
            t.context.clear(e);
            ok("")
        })
    });

    c.add("tiles", "tiles [filter words]", "list the tile database entries of the kits (the tile browser's rows), optionally filtered; says how many", |e, a| {
        with_tile(e, |t, e| {
            let roots = t.kit_roots(e);
            if roots.is_empty() {
                return fail("no assembly kit found");
            }

            let words = a.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
            let (mut out, mut total, mut shown) = (String::new(), 0, 0);
            for root in &roots {
                let index = TileIndex::shared(root);
                total += index.entries().len();
                for en in index.entries().iter().filter(|en| matches_words(en, &words)) {
                    shown += 1;
                    if shown <= 40 {
                        let kit = root.parent().and_then(Path::file_name).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                        out += &format!("{}  {}x{}  [{}]  {kit}\n", en.key, en.width, en.height, en.variations.join(","));
                    }
                }
            }

            ok(format!("{shown} of {total} tiles{}", if shown != 0 { format!("\n{out}") } else { String::new() }))
        })
    });

    c.add("assert_tiles", "assert_tiles <n> [filter words]", "test: the tile browser lists n entries for the filter (over all kits)", |e, a| {
        let Some(n) = a.first() else { return fail("usage: assert_tiles <n> [filter words]") };
        with_tile(e, |t, e| {
            let words = a[1..].iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
            let shown = t.kit_roots(e).iter().map(|root| TileIndex::shared(root).entries().iter().filter(|en| matches_words(en, &words)).count()).sum::<usize>();
            if shown as i64 == atoi(n) { ok("") } else { fail(format!("{shown} tiles, expected {n}")) }
        })
    });

    c.add("tile_entry", "tile_entry", "log the active tile's database entry (location, size, colour, ground groups, variations, links)", |e, _| {
        let Some(d) = document(e) else { return no_tile() };
        let p = d.project();
        let Some(en) = &p.entry else {
            return ok(format!("no tile database entry{}", if p.warning.is_empty() { String::new() } else { format!(": {}", p.warning) }));
        };

        let g = en.texture_group();
        ok(format!(
            "{}: {} set {}, {} x {} cells, colour {:.0} {:.0} {:.0}, ground {}/{}/{}/{}, {} variations (open: {}), {} links, {} targets",
            en.file().file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), en.location_generic(), en.tile_set(), en.width(), en.height(),
            en.red(), en.green(), en.blue(), g.red, g.green, g.blue, g.alpha, en.variations().len(), p.info.variation, en.links().len(), en.link_targets().len(),
        ))
    });

    c.add(
        "assert_entry", "assert_entry <field> <value>",
        "test: a field of the active tile's database entry (set, location, name, kind, width, height, red, green, blue, ground_red/green/blue/alpha, variations, variation, min_height, scale, normal_strength, overlap_border_size, raw_data_tri_density, links, targets, scalable, random_rotatable; `entry none` / `entry present`)",
        |e, a| {
            let (Some(d), true) = (document(e), a.len() >= 2) else { return fail("usage: assert_entry <field> <value> (with a tile open)") };
            let p = d.project();
            if a[0] == "entry" {
                return if (a[1] == "none") == p.entry.is_none() { ok("") } else { fail(if p.entry.is_some() { "the tile has an entry" } else { "the tile has no entry" }) };
            }

            let Some(en) = &p.entry else { return fail("the tile has no database entry") };
            let g = en.texture_group();
            let vars = en.variations();
            let v = p.variation.and_then(|i| vars.get(i));
            let text = |s: String| (Some(s), 0.0);
            let flag = |b: bool| (Some(if b { "true" } else { "false" }.to_owned()), 0.0);
            let n = |x: f64| (None, x);
            let (got, num) = match a[0].as_str() {
                "set" => text(en.tile_set()),
                "location" => text(en.location_generic()),
                "name" => text(en.file().file_stem().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
                "kind" => text(en.kind()),
                "width" => n(f64::from(en.width())),
                "height" => n(f64::from(en.height())),
                "red" => n(en.red()),
                "green" => n(en.green()),
                "blue" => n(en.blue()),
                "ground_red" => text(g.red),
                "ground_green" => text(g.green),
                "ground_blue" => text(g.blue),
                "ground_alpha" => text(g.alpha),
                "variations" => n(vars.len() as f64),
                "variation" => text(p.info.variation.clone()),
                "links" => n(en.links().len() as f64),
                "targets" => n(en.link_targets().len() as f64),
                "scalable" => flag(en.scalable()),
                "random_rotatable" => flag(en.random_rotatable()),
                "min_height" if v.is_some() => n(v.expect("checked").min_height),
                "scale" if v.is_some() => n(v.expect("checked").scale),
                "normal_strength" if v.is_some() => n(v.expect("checked").normal_strength),
                "overlap_border_size" if v.is_some() => n(v.expect("checked").overlap_border_size),
                "raw_data_tri_density" if v.is_some() => n(f64::from(v.expect("checked").raw_data_tri_density)),
                other => {
                    return fail(format!(
                        "unknown field '{other}'{}", if v.is_some() { "" } else { " (or the open folder is not one of the entry's variations)" },
                    ));
                }
            };

            match got {
                None => {
                    let want = wce_formats::c_num::strtod(&a[1]).filter(|_| a[1].trim_start().len() == a[1].len());
                    match want {
                        Some(w) if (num - w).abs() <= 1e-4 && a[1].parse::<f64>().is_ok() => ok(""),
                        _ => fail(format!("{} is {}, expected {}", a[0], wce_formats::c_num::printf_g(num), a[1])),
                    }
                }

                Some(s) => {
                    if s == a[1] { ok("") } else { fail(format!("{} is '{s}', expected '{}'", a[0], a[1])) }
                }
            }
        },
    );

    c.add("scene_filter", "scene_filter [text]", "filter the Scene panel's tree by name or type (no text: clear it)", |e, a| {
        let text = a.join(" ");
        with_tile(e, |t, _| {
            t.tree_filter = text;
            ok("")
        })
    });

    c.add("tile_mode", "tile_mode select|raise|lower|smooth|flatten|level", "what the left mouse button does in the viewport", |e, a| {
        let Some(name) = a.first() else { return fail("usage: tile_mode select|raise|lower|smooth|flatten|level") };
        let Some(i) = MODE_NAMES.iter().position(|m| m == name) else { return fail(format!("unknown mode '{name}'")) };
        with_tile(e, |t, _| {
            t.set_mode(TileMode::from_index(i));
            ok("")
        })
    });

    c.add("brush", "brush <radius m> [strength] [falloff] [flatten height m]", "set the terrain brush", |e, a| {
        let Some(r) = a.first().and_then(|x| number(x)) else { return fail("usage: brush <radius m> [strength] [falloff] [flatten height m]") };
        with_tile(e, |t, _| {
            t.brush.radius_m = r.clamp(1.0, 500.0);
            if let Some(v) = a.get(1).and_then(|x| number(x)) {
                t.brush.strength = v;
            }

            if let Some(v) = a.get(2).and_then(|x| number(x)) {
                t.brush.falloff = v.clamp(0.0, 1.0);
            }

            if let Some(v) = a.get(3).and_then(|x| number(x)) {
                t.brush.flatten_height = v;
            }

            ok("")
        })
    });

    c.add("paint", "paint raise|lower|smooth|flatten|level <x m> <z m> <steps>", "a brush stroke at a point of the tile (one undo entry)", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 4).then(|| Some((number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
            let Some((x, z, n)) = parsed else { return fail("usage: paint raise|lower|smooth|flatten|level <x m> <z m> <steps>") };
            let kind = match a[0].as_str() {
                "raise" => BrushKind::Raise,
                "lower" => BrushKind::Lower,
                "smooth" => BrushKind::Smooth,
                "flatten" => BrushKind::Flatten,
                "level" => BrushKind::Level,
                other => return fail(format!("unknown brush '{other}'")),
            };

            let d = document(e).expect("a tile");
            if !d.project().has_height() {
                return fail("no height map");
            }

            for _ in 0..n as i32 {
                d.brush_step(kind, x, z, t.brush, 1.0 / 60.0);
            }

            d.end_stroke();
            ok("")
        })
    });

    c.add("tile_select", "tile_select <type | element index> [add]", "select the first instance of a type, or an element by index", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        let Some(what) = a.first() else { return fail("usage: tile_select <type | element index> [add]") };
        let add = a.get(1).is_some_and(|x| x == "add");
        let found = match what.parse::<i64>() {
            Ok(i) => usize::try_from(i).ok(),
            Err(_) => d.tile().instances().iter().copied().find(|&el| d.tile().tscene().type_of(el) == what),
        };

        let Some(found) = found.filter(|&f| f < d.tile().tscene().doc().elements().len()) else { return fail(format!("nothing found for '{what}'")) };
        select_elem(e, Some(found), add);
        ok(format!("element {found}"))
    });

    c.add(
        "tile_select_all", "tile_select_all <type> [key words]",
        "select every instance of a type (BUILDING_INSTANCE, ...) whose key holds the words (and-ed; none: all of them)",
        |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            let Some(d) = document(e) else { return no_tile() };
            let Some(ty) = a.first() else { return fail("usage: tile_select_all <type> [key words]") };
            let words = a[1..].iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
            let sc = d.tile().tscene();
            let key = |el: usize| sc.doc().find_attr(el, "key").map(|at| String::from_utf8_lossy(sc.doc().attr_raw(el, at)).to_lowercase()).unwrap_or_default();
            let found = d.tile().instances().iter().copied().filter(|&el| sc.type_of(el) == ty && words.iter().all(|w| key(el).contains(w))).collect::<Vec<_>>();
            e.selection.clear();
            for &el in &found {
                e.selection.add(wce_scene::render_scene::SourceRef { doc: id, item: wce_tile_editor::tile_document::item_of(el) });
            }

            e.docs.set_active(id);
            ok(format!("{} selected", found.len()))
        },
    );

    c.add("tile_move", "tile_move <dx m> <dz m>", "move the selected instance over the ground (keeps its height above the terrain)", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let Some((dx, dz)) = parsed else { return fail("usage: tile_move <dx m> <dz m>") };
        let Some(el) = el else { return fail("nothing selected") };
        let t = d.tile();
        let mut p = t.world_position(el);
        let above = p.y - t.terrain_height(p.x, p.z);
        p.x += dx;
        p.z += dz;
        p.y = t.terrain_height(p.x, p.z) + above;
        if !t.is_instance(el) {
            return fail("cannot move this element");
        }

        d.set_world_metres(el, p, 0);
        ok("")
    });

    c.add("grab_to", "grab_to <x m> <z m>", "the G grab: put the selected instance on the ground at a point (grid snap applies)", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let Some((mut x, mut z)) = parsed else { return fail("usage: grab_to <x m> <z m>") };
            if !t.begin_grab(e) {
                return fail("nothing selected");
            }

            if t.snap_grid && t.grid_m > 0.01 {
                x = (x / t.grid_m).round() * t.grid_m;
                z = (z / t.grid_m).round() * t.grid_m;
            }

            let d = document(e).expect("a tile");
            let y = d.tile().terrain_height(x, z) + t.grab_y_offset as f32;
            if let Some(g) = t.grab_elem {
                d.set_world_metres(g, Float3::new(x, y, z), t.grab_gesture);
            }

            t.end_grab(e, true);
            ok("")
        })
    });

    c.add("duplicate", "duplicate", "duplicate the selected instance (4 m to the side) and select the copy", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(el) = el else { return fail("nothing selected") };
        d.duplicate(el).map_or_else(fail, |_| ok("duplicated"))
    });

    c.add("delete_item", "delete_item", "delete the selected instance", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        if el.is_some_and(|el| d.remove(el)) { ok("deleted") } else { fail("nothing selected") }
    });

    c.add("rotate", "rotate <degrees>", "rotate the selected instance about the world Y axis", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(deg) = a.first().and_then(|x| number(x)) else { return fail("usage: rotate <degrees>") };
        if el.is_some_and(|el| d.rotate(el, deg)) { ok("") } else { fail("nothing selected") }
    });

    c.add(
        "add_building", "add_building <key> [x m z m]",
        "add a building (a copy of the selected or the first one, with this key) in the working layer, on the ground at x z when given; selects it",
        |e, a| {
            let el = primary(e);
            let Some(key) = a.first().cloned() else { return fail("usage: add_building <key> [x m z m]") };
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                let at = (a.len() >= 3).then(|| Some((number(&a[1])?, number(&a[2])?))).flatten().map(|(x, z)| Float3::new(x, d.tile().terrain_height(x, z), z));
                let into = t.new_items_layer(d);
                match d.add_building_in(&key, el, at, Placement::Landscape, into) {
                    Ok(added) => {
                        select_elem(e, Some(added), false);
                        ok(format!("added {key} as element {added}"))
                    }
                    Err(err) => fail(err),
                }
            })
        },
    );

    c.add("add_polygon", "add_polygon \"TYPE|HINT|x,z;x,z;...\"", "add a polygon (AI hint, zone, ...) from points in metres in the working layer; selects it", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(spec) = a.first() else { return fail("usage: add_polygon \"TYPE|HINT|x,z;x,z;...\"") };
        let parts = spec.split('|').collect::<Vec<_>>();
        if parts.len() < 3 {
            return fail("expected TYPE|HINT|x,z;x,z");
        }

        let pts = parts[2]
            .split(';')
            .filter_map(|item| {
                let xz = item.split(',').collect::<Vec<_>>();
                if xz.len() != 2 {
                    return None;
                }

                let (x, z) = (number(xz[0])?, number(xz[1])?);
                Some(Float3::new(x, d.tile().terrain_height(x, z), z))
            })
            .collect::<Vec<_>>();
        let _ = el;
        let (kind, hint) = (parts[0].to_owned(), parts[1].to_owned());
        with_tile(e, |t, e| {
            let Some(d) = document(e) else { return no_tile() };
            let layer = t.new_items_layer(d);
            if crate::tool::POLYGON_TYPES.contains(&kind.as_str()) {
                d.add_polygon(layer, &kind, &hint, &pts).map_or_else(fail, |_| ok(format!("added {kind}")))
            } else {
                let Some(layer) = layer else { return fail("this tile has no scene graph") };
                d.create(&kind, layer, &wce_tile_editor::tile_document::Create::Line(pts), "").map_or_else(fail, |_| ok(format!("added {kind}")))
            }
        })
    });

    c.add(
        "create", "create <type> <x m> <z m> [key]",
        "place a new instance of a type (a light, an effect, a decal, a capture location, ...; see the Create section) on the ground at x z, in the working layer",
        |e, a| {
            let (Some(kind), Some(x), Some(z)) = (a.first().cloned(), a.get(1).and_then(|v| number(v)), a.get(2).and_then(|v| number(v))) else {
                return fail("usage: create <type> <x m> <z m> [key]");
            };
            let key = a.get(3).cloned().unwrap_or_default();
            with_tile(e, |t, e| {
                let Some(d) = document(e) else { return no_tile() };
                let Some(into) = t.new_items_layer(d) else { return fail("this tile has no scene graph") };
                let at = Float3::new(x, d.tile().terrain_height(x, z), z);
                d.create(&kind, into, &wce_tile_editor::tile_document::Create::Point(at), &key).map_or_else(fail, |_| ok(format!("added {kind}")))
            })
        },
    );

    c.add(
        "polygon_point", "polygon_point move|insert|remove|in|out|prepend <index> <x m> <z m>",
        "edit a point of the selected polygon or spline (insert: after the index; a spline's in / out: its handles, prepend: a new first point)", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(idx) = a.get(1).and_then(|x| number(x)).filter(|_| a.len() >= 2) else {
            return fail("usage: polygon_point move|insert|remove|in|out|prepend <index> <x m> <z m>");
        };

        let (x, z) = if a.len() >= 4 { (number(&a[2]).unwrap_or(0.0), number(&a[3]).unwrap_or(0.0)) } else { (0.0, 0.0) };
        let Some(el) = el else { return fail("nothing selected") };
        let Ok(idx) = usize::try_from(idx as i64) else { return fail("no such point") };
        let p = Float3::new(x, d.tile().terrain_height(x, z), z);
        d.polygon_point(el, &a[0], idx, p, 0).map_or_else(fail, |()| ok(""))
    });

    c.add("spline_value", "spline_value <attribute> <value> [point index|all]", "a value of the selected river's or road's points (width, foam_amount, alpha_fade, flow_speed, terrain_offset)", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let (Some(attr), Some(value)) = (a.first(), a.get(1).filter(|v| number(v).is_some())) else {
            return fail("usage: spline_value <attribute> <value> [point index|all]");
        };

        let Some(el) = el.filter(|&s| d.is_spline(s)) else { return fail("no river or road selected") };
        let n = d.tile().tscene().polygon_points(el).len();
        let points = match a.get(2).map(String::as_str) {
            None | Some("all") => (0..n).collect::<Vec<_>>(),
            Some(i) => match i.parse::<usize>() {
                Ok(i) if i < n => vec![i],
                _ => return fail("no such point"),
            },
        };

        if d.set_spline_value(el, &points, attr, value, 0) { ok(format!("{attr} = {value} on {} points", points.len())) } else { fail("nothing changed") }
    });

    c.add("wall_sharp", "wall_sharp <point index> on|off", "mark a point of the selected wall (WALL_SPLINE) as a corner (a mitre) or a smooth one", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let (Some(i), Some(on)) = (a.first().and_then(|x| x.parse::<usize>().ok()), a.get(1).map(|x| x == "on")) else { return fail("usage: wall_sharp <point index> on|off") };
        let Some(el) = el.filter(|&el| d.tile().is_wall(el)) else { return fail("no wall selected") };
        if d.set_point_attribute(el, i, "sharp", wce_tile::properties::format_bool(on)) { ok("") } else { fail("no such point") }
    });

    c.add("wall_info", "wall_info", "log the selected wall's layout: base, length, corners, runs, step sections, attached ends, warnings", |e, _| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
        let runs = l.plan.runs.iter().map(|r| format!("{:.0}..{:.0} at {:.2} (drift {:.2})", r.s.0, r.s.1, r.level, r.drift)).collect::<Vec<_>>().join(", ");
        let steps = l.plan.steps.iter().map(|st| format!("{:.0}..{:.0} {:.2} -> {:.2}{}", st.s.0, st.s.1, st.from, st.to, if st.whole { " (bastion)" } else { "" })).collect::<Vec<_>>().join(", ");
        let corners = l.plan.corners.iter().map(|(s, i)| format!("point {i} at {s:.0}")).collect::<Vec<_>>().join(", ");
        let ps = wce_walls::pieces::pieces(&l.plan, l.profile);
        let (doors, skipped) = d.tile().wall_doors(&l, &ps);
        let (defences, defences_skipped) = wce_walls::pieces::defence_slots(&l.plan, l.profile);
        let defences = defences.iter().map(|(s, k)| format!("{k} at {s:.0}")).collect::<Vec<_>>().join(", ");
        ok(format!("{}; {} pieces, doors at pieces {doors:?}; towers and bastions [{defences}]; corners [{corners}]; runs [{runs}]; steps [{steps}]{}", crate::tool::wall_summary(&l), ps.len(),
            l.plan.warnings.iter().chain(&skipped).chain(&defences_skipped).map(|w| format!("; {w}")).collect::<String>()))
    });

    c.add("assert_wall", "assert_wall <corners> <steps> [start end: free|attached]", "test: the selected wall's layout has these many corners and step sections (and its ends are free or attached)", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
        let want = |i: usize| a.get(i).and_then(|x| x.parse::<usize>().ok());
        let (corners, steps) = (l.plan.corners.len(), l.plan.steps.len());
        if want(0).is_some_and(|n| n != corners) || want(1).is_some_and(|n| n != steps) {
            return fail(format!("{corners} corner(s), {steps} step section(s)"));
        }

        for (k, end) in l.ends.iter().enumerate() {
            if let Some(w) = a.get(2 + k).filter(|w| (*w == "attached") != end.is_some()) {
                return fail(format!("end {k} is {}, not {w}", if end.is_some() { "attached" } else { "free" }));
            }
        }

        ok("")
    });

    c.add("assert_wall_doors", "assert_wall_doors <n>", "test: the selected wall has n door sections", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(l) = el.and_then(|el| d.tile().wall_layout(el)) else { return fail("no wall selected") };
        let ps = wce_walls::pieces::pieces(&l.plan, l.profile);
        let (doors, _) = d.tile().wall_doors(&l, &ps);
        if a.first().and_then(|x| x.parse::<usize>().ok()) == Some(doors.len()) { ok("") } else { fail(format!("{} door section(s)", doors.len())) }
    });

    c.add("new_layer", "new_layer <name>", "a new layer under the scene graph, selected", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        let Some(name) = a.first() else { return fail("usage: new_layer <name>") };
        let graph = d.tile().tscene().scene_graph();
        d.add_layer(graph, name).map_or_else(fail, |_| ok(""))
    });

    c.add("move_to_layer", "move_to_layer <layer name>", "move the selected instance into the layer with this name", |e, a| {
        let el = primary(e);
        let Some(d) = document(e) else { return no_tile() };
        let Some(name) = a.first() else { return fail("usage: move_to_layer <layer name>") };
        let Some(el) = el else { return fail("nothing selected") };
        let sc = d.tile().tscene();
        let Some(layer) = (0..sc.doc().elements().len()).find(|&i| sc.is_layer(i) && sc.name(i) == *name) else {
            return fail(format!("no layer named '{name}'"));
        };

        d.move_to_layer(el, layer).map_or_else(fail, |_| ok(""))
    });

    c.add("tile_problems", "tile_problems", "log what the game would leave out of the open tile: the manual trees outside every vegetation exclusion polygon", |e, _| {
        let Some(d) = document(e) else { return no_tile() };
        let trees = d.trees_outside_exclusions();
        let shown = trees.iter().take(10).map(|&t| {
            let p = d.tile().world_position(t);
            format!("{t} ({:.1}, {:.1})", p.x, p.z)
        }).collect::<Vec<_>>();
        ok(format!("{} manual tree(s) outside every vegetation exclusion polygon{}{}", trees.len(), if shown.is_empty() { "" } else { ": " }, shown.join(", ")))
    });

    c.add("save_for_kit", "save_for_kit [raw_data folder]", "write the open tile as TEd reads it into the Assembly Kit (or this raw_data folder), without the extracted mark", |e, a| {
        let kit = a.first().map(std::path::PathBuf::from);
        with_tile(e, |t, e| t.save_for_kit(e, kit).map_or_else(fail, ok))
    });

    c.add("import_from_kit", "import_from_kit", "open the Assembly Kit's copy of the open project (its next save replaces the project; an extracted tile is extracted again)", |e, _| {
        with_tile(e, |t, e| t.import_from_kit(e).map_or_else(fail, ok))
    });

    c.add("save_to", "save_to <folder>", "write the tile (tile.wce, the height map and its other files) into another folder (the originals stay)", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        let Some(dir) = a.first() else { return fail("usage: save_to <folder>") };
        match d.save(&SaveTarget { path: PathBuf::from(dir) }) {
            Ok(()) => ok(d.tile().last_message.clone()),
            Err(msg) => fail(msg),
        }
    });

    c.add("tile_diagnostics", "tile_diagnostics <file.txt>", "write the tile's counts and model fallbacks (what Tile Studio wrote next to a shot)", |e, a| {
        let Some(id) = crate::tool::document_id(e) else { return no_tile() };
        let Some(path) = a.first() else { return fail("usage: tile_diagnostics <file.txt>") };
        let text = e.docs.find_as::<TileDocument>(id).expect("a tile").tile().diagnostics(Some(&e.scene));
        match std::fs::write(path, text) {
            Ok(()) => ok(""),
            Err(_) => fail(format!("cannot write {path}")),
        }
    });

    // ---- scripted mouse on instances (the same events a real click or drag sends)
    c.add("tile_click", "tile_click <element> [ctrl|shift]", "click the pixel where an instance is (selects it through the viewport, like the mouse)", |e, a| {
        let (Some(d), Some(x)) = (document(e), a.first()) else { return fail("usage: tile_click <element> [ctrl|shift]") };
        let Some(el) = usize::try_from(atoi(x)).ok().filter(|&el| d.tile().is_instance(el)) else { return fail("not an instance") };
        let mut p = d.tile().world_position(el);
        if d.tile().tscene().type_of(el) == "BUILDING_INSTANCE" {
            p.y += d.tile().marker_radius(el) * 0.5;
        }

        click_at(e, p, "").unwrap_or_else(|| fail("the instance is not in view"))
    });

    c.add("tile_click_ground", "tile_click_ground <x m> <z m>", "click the pixel where a ground point is", |e, a| {
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((x, z))) = (document(e), parsed) else { return fail("usage: tile_click_ground <x m> <z m>") };
        let p = Float3::new(x, d.tile().terrain_height(x, z), z);
        click_at(e, p, "").unwrap_or_else(|| fail("the point is not in view"))
    });

    c.add("tile_drag_point", "tile_drag_point <index> <x m> <z m>", "drag a point handle of the selected polygon to a ground point with the mouse", |e, a| {
        let el = primary(e);
        let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
        let (Some(d), Some((idx, x, z))) = (document(e), parsed) else { return fail("usage: tile_drag_point <index> <x m> <z m>") };
        let Some(el) = el.filter(|&el| d.tile().is_polygon(el)) else { return fail("no polygon selected") };
        let pts = d.tile().polygon_world_points(el);
        let Some(&from) = usize::try_from(idx as i64).ok().and_then(|i| pts.get(i)) else { return fail("no such point") };
        let to = Float3::new(x, d.tile().terrain_height(x, z), z);
        let (Some((x0, y0)), Some((x1, y1))) = (e.viewport.project(from), e.viewport.project(to)) else { return fail("not in view") };
        let args = ["drag".to_owned(), format!("{x0:.1}"), format!("{y0:.1}"), format!("{x1:.1}"), format!("{y1:.1}"), "6".to_owned()];
        mouse(e, &args, "")
    });

    c.add("draw_polygon", "draw_polygon <type> [hint | base wall]", "start drawing a polygon (or a WALL_SPLINE, with its base wall): click the ground for points, Enter finishes (key Enter), Esc cancels", |e, a| {
        let Some(kind) = a.first() else { return fail("usage: draw_polygon <type> [hint | base wall]") };
        with_tile(e, |t, e| {
            if let Err(err) = crate::create_panel::make_for_script(t, e, kind, "") {
                return fail(err);
            }

            if !t.drawing {
                t.stop_making();
                return fail(format!("{kind} is placed, not drawn"));
            }

            if let (Some(h), Some(d)) = (a.get(1).filter(|h| HINTS.contains(&h.as_str())), t.draft.as_mut()) {
                d.attrs.insert("hint_type".to_owned(), h.clone());
            }

            if let (Some(b), Some(d)) = (a.get(1).filter(|b| WALL_BASE.iter().any(|(k, _)| *k == b.as_str())), t.draft.as_mut()) {
                d.attrs.insert("wall_base".to_owned(), b.clone());
            }

            ok("")
        })
    });

    c.add("assert_at_cursor", "assert_at_cursor [tolerance]", "test: the selected instance is on the ground point the mouse last hovered", |e, a| {
        with_doc(e, |t, e| {
            let tol = a.first().and_then(|x| number(x)).unwrap_or(0.5);
            let el = primary(e);
            let (Some(el), true) = (el, t.terrain_hit) else { return fail("nothing selected or the cursor never touched the ground") };
            let p = document(e).expect("a tile").tile().world_position(el);
            let c = t.cursor;
            if (p.x - c.x).abs() <= tol && (p.z - c.z).abs() <= tol {
                ok("")
            } else {
                fail(format!("at {:.2} {:.2}, cursor {:.2} {:.2}", p.x, p.z, c.x, c.z))
            }
        })
    });

    c.add("assert_mode", "assert_mode select|raise|...", "test: the tile tool's left-button mode", |e, a| {
        with_tile(e, |t, _| {
            let now = MODE_NAMES[t.mode().index()];
            if a.first().is_some_and(|x| x == now) { ok("") } else { fail(format!("mode is {now}")) }
        })
    });

    c.add("assert_grabbing", "assert_grabbing 0|1", "test: whether a G grab is in progress", |e, a| {
        with_tile(e, |t, _| {
            if a.first().is_some_and(|x| (x == "1") == t.grabbing) { ok("") } else { fail(if t.grabbing { "grabbing" } else { "not grabbing" }) }
        })
    });

    // ---- layers: flags, structure, export type. A layer is named by its path from the scene graph ("Walls/Standard_Walls"), "#n" is an
    // element index.
    c.add(
        "tile_flag", "tile_flag <layer path | #elem | selected> visible|active on|off", "the eye (OIF_VISIBLE) or the cursor arrow (OIF_ACTIVE, selectable) of a layer or instance",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 3) else { return fail("usage: tile_flag <layer path | #elem | selected> visible|active on|off") };
            let el = element_arg(d, &a[0], primary_of(&sel, d));
            let Some(el) = el.filter(|&el| el < d.tile().tscene().doc().elements().len()) else { return fail(format!("no such layer or element '{}'", a[0])) };
            let flag = match a[1].as_str() {
                "visible" => "OIF_VISIBLE",
                "active" => "OIF_ACTIVE",
                _ => return fail("the flag is visible or active"),
            };

            let targets = if a[0] == "selected" { sel } else { vec![el] };
            if d.set_flags(&targets, flag, a[2] == "on") { ok("") } else { ok(format!("already {}", a[2])) }
        },
    );

    c.add("tile_new_layer", "tile_new_layer <name> [parent layer path]", "a new layer in a layer (the scene graph by default), selected", |e, a| {
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_new_layer <name> [parent layer path]") };
        let parent = match a.get(1) {
            Some(p) => d.tile().find_layer(p),
            None => d.tile().tscene().scene_graph(),
        };

        d.add_layer(parent, &a[0]).map_or_else(|err| fail(if err.is_empty() { "no such parent layer".to_owned() } else { err }), |_| ok(""))
    });

    c.add("tile_rename", "tile_rename <layer path | #elem | selected> <name>", "rename a layer or an instance", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: tile_rename <layer path | #elem | selected> <name>") };
        let el = element_arg(d, &a[0], primary_of(&sel, d));
        if el.is_some_and(|el| d.rename(el, &a[1])) { ok("") } else { fail(format!("no such layer or element '{}'", a[0])) }
    });

    c.add(
        "tile_move_to", "tile_move_to <what: path | #elem | selected> <target layer path> [before <path | #elem>]",
        "move layers / instances into a layer (in front of a sibling): the selection when `selected`", |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else {
                return fail("usage: tile_move_to <path | #elem | selected> <target layer path> [before <path | #elem>]");
            };

            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.iter().copied().map(Some).collect() } else { vec![element_arg(d, &a[0], p)] };
            let layer = element_arg(d, &a[1], p);
            let before = if a.len() > 3 && a[2] == "before" { element_arg(d, &a[3], p) } else { None };
            let (Some(Some(_)), Some(layer)) = (what.first(), layer) else { return fail("nothing to move, or no such target layer") };
            let what = what.into_iter().flatten().collect::<Vec<_>>();
            d.move_items(&what, layer, before).map_or_else(fail, |()| ok(""))
        },
    );

    c.add("tile_group", "tile_group <new layer name>", "put the selected items into a new layer next to the first one", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_group <new layer name>") };
        d.group_items(&sel, &a[0]).map_or_else(fail, |_| ok(""))
    });

    c.add("tile_working_layer", "tile_working_layer [layer path | #elem | none]", "TEd's working layer, where new items go (none: the root); no argument: log it", |e, a| {
        let sel = selected(e);
        let Some(id) = crate::tool::document_id(e) else { return no_tile() };
        with_tile(e, |t, e| {
            let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
            match a.first().map(String::as_str) {
                None => ok(t.working_layer_of(d).map_or_else(|| "the root".to_owned(), |l| crate::tool::TileTool::layer_label(d, l))),
                Some("none") => t.set_working_layer(d, None).map_or_else(fail, ok),
                Some(spec) => {
                    let Some(el) = element_arg(d, spec, primary_of(&sel, d)) else { return fail(format!("no layer '{spec}'")) };
                    t.set_working_layer(d, Some(el)).map_or_else(fail, ok)
                }
            }
        })
    });

    c.add("tile_instance_group", "tile_instance_group", "group the selection into an instance group in the working layer (Ctrl+G)", |e, _| {
        with_tile(e, |t, e| t.group_selection(e).map_or_else(fail, ok))
    });

    c.add("tile_ungroup", "tile_ungroup", "take the selected instance groups apart into the working layer (Ctrl+U)", |e, _| {
        with_tile(e, |t, e| t.ungroup_selection(e).map_or_else(fail, ok))
    });

    // ---- prefabs (prefab_panel.rs)

    c.add("prefab_list", "prefab_list [words]", "log the prefab library (key, folder), filtered by words", |e, a| {
        let words = a.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
        with_tile(e, |t, e| {
            let lib = t.prefab_library(e);
            let rows = lib.entries().filter(|p| words.iter().all(|w| format!("{}/{}", p.folder, p.key).to_lowercase().contains(w))).map(|p| format!("{}  {}", p.key, p.folder)).collect::<Vec<_>>();
            ok(format!("{} prefabs:\n{}", rows.len(), rows.join("\n")))
        })
    });

    c.add("prefab_rescan", "prefab_rescan", "scan the prefab folders again", |e, _| with_tile(e, |t, e| ok(format!("{} prefabs", t.rescan_prefabs(e)))));

    c.add("keys", "keys <type> [filter]", "the valid keys of a type in the open tile's files (the Create and Placement pickers' lists): the count and the first 20 matching", |e, a| {
        let Some(kind) = a.first().cloned() else { return fail("usage: keys <type> [filter]") };
        let filter = a.get(1).map(|f| f.to_lowercase()).unwrap_or_default();
        with_tile(e, |t, e| {
            let keys = t.catalogue(e, &kind);
            let hits = keys.iter().filter(|k| k.to_lowercase().contains(&filter)).collect::<Vec<_>>();
            ok(format!("{} keys, {} matching: {}", keys.len(), hits.len(), hits.iter().take(20).map(|k| k.as_str()).collect::<Vec<_>>().join(", ")))
        })
    });

    c.add("place_modes", "place_modes <freeform|landscape|entity> [none|surface]", "the placement chips: what new things stand on and how they are turned", |e, a| {
        use crate::settings::{RotationMode, SurfaceMode};
        let surface = match a.first().map(String::as_str) {
            Some("freeform") => SurfaceMode::Freeform,
            Some("landscape") => SurfaceMode::Landscape,
            Some("entity") => SurfaceMode::Entity,
            _ => return fail("usage: place_modes <freeform|landscape|entity> [none|surface]"),
        };

        let rotation = if a.get(1).map(String::as_str) == Some("surface") { RotationMode::Surface } else { RotationMode::None };
        with_tile(e, |t, e| {
            t.modes = Some((surface, rotation));
            e.update_tool_settings(crate::settings::SETTINGS_ID, |s: &mut crate::settings::TileSettings| {
                s.place_surface = surface;
                s.place_rotation = rotation;
            });
            ok("")
        })
    });

    c.add("veg_brush", "veg_brush on|off [radius] [spacing] [clearance] [fill]", "the vegetation brush: paint trees instead of placing one per click, and its settings", |e, a| {
        let on = match a.first().map(String::as_str) {
            Some("on") => true,
            Some("off") => false,
            _ => return fail("usage: veg_brush on|off [radius] [spacing] [clearance] [fill]"),
        };

        with_tile(e, |t, e| {
            let mut b = t.veg_brush(e);
            b.on = on;
            let num = |i: usize| a.get(i).and_then(|v| number(v));
            b.radius = num(1).unwrap_or(b.radius);
            b.spacing = num(2).unwrap_or(b.spacing);
            b.clearance = num(3).unwrap_or(b.clearance);
            b.fill = num(4).unwrap_or(b.fill);
            t.set_veg_brush(e, b);
            ok("")
        })
    });

    c.add("veg_set_new", "veg_set_new <name>", "a new vegetation set from the selected trees (each kind weighted by how many there are)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: veg_set_new <name>") };
        with_tile(e, |t, e| t.new_vegetation_set(e, &name).map_or_else(fail, ok))
    });

    c.add("veg_set_add", "veg_set_add <key>...", "add trees to the new vegetation set (not saved) and place or paint with it, as Ctrl + click on them in the Create panel does", |e, a| {
        if a.is_empty() {
            return fail("usage: veg_set_add <key>...");
        }

        let keys = a.iter().map(|k| wce_tile::vegetation_sets::SetEntry { key: k.clone(), weight: 1.0, scale: None }).collect::<Vec<_>>();
        with_tile(e, |t, e| {
            let trees = t.catalogue(e, "VEGETATION_INSTANCE");
            if let Some(k) = keys.iter().find(|k| !trees.contains(&k.key)) {
                return fail(format!("no tree '{}' in the game's files", k.key));
            }

            t.add_to_new_set(e, &keys);
            ok(t.status.clone())
        })
    });

    c.add("veg_set_save", "veg_set_save <name>", "save the new vegetation set under a name", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: veg_set_save <name>") };
        with_tile(e, |t, e| {
            t.scratch_name = name;
            t.save_new_set(e).map_or_else(fail, ok)
        })
    });

    c.add("veg_set", "veg_set <name>", "place or paint the trees of a vegetation set (as its Create panel entry)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: veg_set <name>") };
        with_tile(e, |t, e| crate::create_panel::make_set_for_script(t, e, &name).map_or_else(fail, |()| ok("")))
    });

    c.add("veg_paint", "veg_paint <x m> <z m> [<x m> <z m>]...", "one stroke of the vegetation brush through ground points (a dab at each): its trees go into the working layer", |e, a| {
        let pts = a.chunks(2).map(|p| (p.first().and_then(|v| number(v)), p.get(1).and_then(|v| number(v)))).collect::<Vec<_>>();
        if pts.is_empty() || a.len() % 2 != 0 || pts.iter().any(|p| p.0.is_none() || p.1.is_none()) {
            return fail("usage: veg_paint <x m> <z m> [<x m> <z m>]...");
        }

        with_tile(e, |t, e| {
            if !t.painting_trees(e) {
                return fail("not painting trees (start placing a tree with the brush on)");
            }

            for (i, (x, z)) in pts.iter().map(|p| (p.0.unwrap_or_default(), p.1.unwrap_or_default())).enumerate() {
                let Some(d) = document(e) else { return no_tile() };
                t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
                if i == 0 {
                    t.begin_stroke(e, false);
                } else {
                    t.dab(e);
                }
            }

            ok(t.end_stroke_veg(e))
        })
    });

    c.add("veg_erase", "veg_erase <x m> <z m>", "the vegetation brush's taking away (Ctrl + drag) once at a ground point: the manual trees in its circle go", |e, a| {
        let (Some(x), Some(z)) = (a.first().and_then(|v| number(v)), a.get(1).and_then(|v| number(v))) else { return fail("usage: veg_erase <x m> <z m>") };
        with_tile(e, |t, e| {
            let Some(d) = document(e) else { return no_tile() };
            t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
            t.begin_stroke(e, true);
            ok(t.end_stroke_veg(e))
        })
    });

    c.add("start_making", "start_making <type | building | prefab> [key]", "start placing or drawing something, as a click in the Create panel does (the mouse places it)", |e, a| {
        let Some(what) = a.first().cloned() else { return fail("usage: start_making <type | building | prefab> [key]") };
        let key = a.get(1).cloned().unwrap_or_default();
        with_tile(e, |t, e| crate::create_panel::make_for_script(t, e, &what, &key).map_or_else(fail, |()| ok("")))
    });

    c.add("quick_add", "quick_add", "open the Create panel's quick-add popup (Shift+A over the viewport)", |e, _| {
        with_tile(e, |t, _| {
            t.request_quick_add();
            ok("")
        })
    });

    c.add(
        "place_preview", "place_preview <type | building | prefab> <x m> <z m> [key] | place_preview off",
        "start placing (a Create type, a building or a prefab) and show its preview at x z as if the mouse were there", |e, a| {
        if a.first().map(String::as_str) == Some("off") {
            return with_tile(e, |t, _| {
                t.placing = None;
                t.preview_forced = false;
                ok("")
            });
        }

        let (Some(what), Some(x), Some(z)) = (a.first().cloned(), a.get(1).and_then(|v| number(v)), a.get(2).and_then(|v| number(v))) else {
            return fail("usage: place_preview <type | building | prefab> <x m> <z m> [key] | place_preview off");
        };

        let key = a.get(3).cloned().unwrap_or_default();
        with_tile(e, |t, e| {
            let Some(d) = crate::tool::document(e) else { return no_tile() };
            t.cursor = Float3::new(x, d.tile().terrain_height(x, z), z);
            t.terrain_hit = true;
            t.preview_forced = true;
            match crate::create_panel::make_for_script(t, e, &what, &key) {
                Ok(()) => ok(""),
                Err(err) => fail(err),
            }
        })
    });

    c.add("prefab_place", "prefab_place <key> [x m z m]", "place a prefab in the working layer, on the ground at x z (default: the view's centre)", |e, a| {
        let Some(key) = a.first().cloned() else { return fail("usage: prefab_place <key> [x m z m]") };
        let at = (a.len() >= 3).then(|| Some(Float3::new(number(&a[1])?, 0.0, number(&a[2])?))).flatten();
        with_tile(e, |t, e| {
            let at = at.unwrap_or_else(|| t.placement_point(e));
            t.place_prefab(e, &key, at).map_or_else(fail, ok)
        })
    });

    fn selected_group(e: &Editor) -> Option<usize> {
        let d = e.docs.find_as::<TileDocument>(crate::tool::document_id(e)?)?;
        selected(e).into_iter().find(|&el| d.tile().tscene().type_of(el) == "UserInstanceGroup")
    }

    c.add("prefab_edit", "prefab_edit", "edit the selected copy of a prefab", |e, _| {
        let Some(g) = selected_group(e) else { return fail("select a copy of a prefab") };
        with_tile(e, |t, e| t.edit_prefab(e, g).map_or_else(fail, ok))
    });

    c.add("prefab_apply", "prefab_apply", "write the edited prefab and update its copies in this tile", |e, _| with_tile(e, |t, e| t.apply_prefab_edit(e).map_or_else(fail, ok)));
    c.add("prefab_cancel", "prefab_cancel", "end the prefab edit without saving: the copy gets the file's contents back", |e, _| {
        with_tile(e, |t, e| t.cancel_prefab_edit(e).map_or_else(fail, ok))
    });

    c.add("prefab_recognise", "prefab_recognise", "make the instance groups that are exact copies of a prefab references to it (as opening a tile from the Assembly Kit does)", |e, _| {
        with_tile(e, |t, e| t.recognise_prefabs(e).map_or_else(fail, ok))
    });

    c.add("prefab_unpack", "prefab_unpack", "make the selected copy of a prefab a plain group", |e, _| {
        let Some(g) = selected_group(e) else { return fail("select a copy of a prefab") };
        with_tile(e, |t, e| t.unpack_prefab(e, g).map_or_else(fail, ok))
    });

    c.add("prefab_save_kit", "prefab_save_kit <key>", "write a prefab as TEd reads it into the Assembly Kit's art/prefabs (the prefabs placed in it baked)", |e, a| {
        let Some(key) = a.first().cloned() else { return fail("usage: prefab_save_kit <key>") };
        with_tile(e, |t, e| t.save_prefab_for_kit(e, &key).map_or_else(fail, ok))
    });

    c.add("prefab_save", "prefab_save <name> [folder]", "save the selected instance group as a new prefab (renamed after it)", |e, a| {
        let Some(name) = a.first().cloned() else { return fail("usage: prefab_save <name> [folder]") };
        let folder = a.get(1).cloned().unwrap_or_default();
        let Some(g) = selected_group(e) else { return fail("select an instance group") };
        with_tile(e, |t, e| t.save_as_prefab(e, g, &name, &folder).map_or_else(fail, ok))
    });

    c.add("tile_delete", "tile_delete <layer path | #elem | selected>", "delete layers / instances (with everything below)", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_delete <layer path | #elem | selected>") };
        let p = primary_of(&sel, d);
        let what = if a[0] == "selected" { sel.iter().copied().map(Some).collect() } else { vec![element_arg(d, &a[0], p)] };
        let ok_first = what.first().is_some_and(Option::is_some);
        let what = what.into_iter().flatten().collect::<Vec<_>>();
        if ok_first && d.remove_items(&what) { ok("") } else { fail("nothing to delete") }
    });

    c.add("tile_duplicate_layer", "tile_duplicate_layer <layer path>", "duplicate a layer with everything in it (named '<name> copy')", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_duplicate_layer <layer path>") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer") };
        d.duplicate_layer(el).map_or_else(fail, |_| ok(""))
    });

    c.add("tile_export_type", "tile_export_type <layer path> Normal|Off|Separate|Escalation1|Escalation2|Escalation3", "the export type of a layer", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: tile_export_type <layer path> Normal|Off|Separate|Escalation1|Escalation2|Escalation3") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)).filter(|&el| d.tile().tscene().is_layer(el)) else { return fail("no such layer") };
        if d.set_attribute(el, "export_type", &format!("LayerExportType_{}", a[1])) { ok("") } else { fail("cannot set it") }
    });

    c.add("tile_generate_siege_ai", "tile_generate_siege_ai", "Generate SiegeAI (docs/plans/siege_ai.md): the layout into its own layer, the old one not exported", |e, _| {
        let Some(d) = document(e) else { return fail("no tile open") };
        match d.generate_siege_ai() {
            Ok(l) => ok(&format!("{}{}", l.summary(), l.warnings.iter().map(|w| format!("
  {w}")).collect::<String>())),
            Err(err) => fail(&err),
        }
    });

    c.add("tile_select_layer", "tile_select_layer <layer path | #elem>", "select a layer or an element in the scene tree", |e, a| {
        let Some(what) = a.first() else { return fail("usage: tile_select_layer <layer path | #elem>") };
        let p = primary(e);
        let Some(d) = document(e) else { return fail("usage: tile_select_layer <layer path | #elem>") };
        let el = element_arg(d, what, p);
        let Some(el) = el else { return fail("no such layer or element") };
        select_elem(e, Some(el), false);
        ok("")
    });

    c.add(
        "tile_select_in", "tile_select_in <layer path> <type> [frame|click]",
        "select the first instance of a type below a layer; `frame` points the camera at it, `click` clicks its pixel with the scripted mouse (frame it and wait some frames first)",
        |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: tile_select_in <layer path> <type> [click]") };
            let Some(layer) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer") };
            let t = d.tile();
            let Some(found) = t.instances().iter().copied().find(|&el| t.tscene().type_of(el) == a[1] && t.tscene().is_descendant(el, layer)) else {
                return fail(format!("no {} in {}", a[1], a[0]));
            };

            let p = t.world_position(found);
            match a.get(2).map(String::as_str) {
                Some("click") => {
                    let mut q = p;
                    if t.tscene().type_of(found) == "BUILDING_INSTANCE" {
                        q.y += t.marker_radius(found) * 0.5;
                    }

                    click_at(e, q, format!("clicked element {found}")).unwrap_or_else(|| fail("the instance is not in view"))
                }

                // The camera looks at it (wait a few frames before clicking).
                Some("frame") => {
                    if !e.viewport.is_fly() {
                        e.viewport.orbit_mut().set_orbit(0.8, 0.7, 60.0, p);
                    }

                    ok(format!("framed element {found}"))
                }

                _ => {
                    select_elem(e, Some(found), false);
                    ok(format!("element {found}"))
                }
            }
        },
    );

    c.add("tile_tree", "tile_tree [depth]", "log the scene tree from the root down to a depth (default 2) with each layer's flags and child count", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        let depth = a.first().map_or(2, |x| atoi(x));
        let t = d.tile();
        let sc = t.tscene();
        let mut out = String::new();
        let mut stack = sc.scene_graph().map(|g| vec![(g, 0i64)]).unwrap_or_default();
        while let Some((el, level)) = stack.pop() {
            let kids = sc.children(el);
            out += &format!(
                "{:width$}#{el} {} '{}' {}{}{} {}\n", "", sc.type_of(el), sc.name(el), if t.own_flag(el, "OIF_VISIBLE") { "V" } else { "-" },
                if t.own_flag(el, "OIF_ACTIVE") { "A" } else { "-" }, if sc.is_layer(el) { format!(" export={}", t.export_type(el)) } else { String::new() },
                kids.len(), width = (level * 2) as usize,
            );
            if level < depth {
                stack.extend(kids.into_iter().filter(|&k| sc.is_layer(k)).rev().map(|k| (k, level + 1)));
            }
        }

        ok(out)
    });

    c.add("assert_flag", "assert_flag <layer path | #elem> visible|active 0|1", "test: the element's own eye / cursor arrow", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 3) else { return fail("usage: assert_flag <layer path | #elem> visible|active 0|1") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
        let v = d.tile().own_flag(el, if a[1] == "visible" { "OIF_VISIBLE" } else { "OIF_ACTIVE" });
        if v == (a[2] == "1") { ok("") } else { fail(format!("{} {} is {}", a[0], a[1], if v { "1" } else { "0" })) }
    });

    c.add("assert_shown", "assert_shown <layer path | #elem> 0|1", "test: the element is drawn (visible itself and in every layer above)", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_shown <layer path | #elem> 0|1") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
        let shown = d.tile().shown(el);
        if shown == (a[1] == "1") { ok("") } else { fail(format!("{}{}", a[0], if shown { " is shown" } else { " is hidden" })) }
    });

    c.add("assert_selectable", "assert_selectable <layer path | #elem> 0|1", "test: the element can be picked (active itself and in every layer above)", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_selectable <layer path | #elem> 0|1") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail("no such layer or element") };
        let s = d.tile().selectable(el);
        if s == (a[1] == "1") { ok("") } else { fail(format!("{}{}", a[0], if s { " is selectable" } else { " is locked" })) }
    });

    c.add(
        "assert_layer", "assert_layer <layer path> <children n | exists 0|1 | export Type | name N>",
        "test: facts of a layer (children n | exists 0|1 | export Type | child <index> <name>)", |e, a| {
            let sel = selected(e);
            let usage = "usage: assert_layer <layer path> <children n | exists 0|1 | export Type>";
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail(usage) };
            let el = element_arg(d, &a[0], primary_of(&sel, d));
            if a[1] == "exists" {
                return if el.is_some() == a.get(2).is_some_and(|x| x == "1") { ok("") } else { fail(if el.is_some() { "the layer exists" } else { "no such layer" }) };
            }

            let Some(el) = el else { return fail(format!("no such layer '{}'", a[0])) };
            let t = d.tile();
            match (a[1].as_str(), a.len()) {
                ("children", 3..) => {
                    let n = t.tscene().children(el).len();
                    if n as i64 == atoi(&a[2]) { ok("") } else { fail(format!("children: {n}")) }
                }

                ("export", 3..) => {
                    let x = t.export_type(el);
                    if x == a[2] { ok("") } else { fail(format!("export type {x}")) }
                }

                // The direct child at an index (counted from 0) has this name.
                ("child", 4..) => {
                    let kids = t.tscene().children(el);
                    let got = usize::try_from(atoi(&a[2])).ok().and_then(|i| kids.get(i)).map_or_else(|| "(none)".to_owned(), |&k| t.tscene().name(k));
                    if got == a[3] { ok("") } else { fail(format!("child {} is '{got}'", a[2])) }
                }

                _ => fail(usage),
            }
        },
    );

    c.add("assert_selection_names", "assert_selection_names <name>...", "test: the selected elements have exactly these names, in selection order", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e) else { return no_tile() };
        let got = sel.iter().map(|&el| d.tile().tscene().name(el)).collect::<Vec<_>>().join(",");
        let want = a.join(",");
        if got == want { ok("") } else { fail(format!("selected: {got}")) }
    });

    // ---- placement: landscape / freeform, snapping, the off-terrain report
    c.add(
        "tile_placement", "tile_placement <path | #elem | selected> landscape|freeform",
        "set the surface mode of the instances (layers: everything below); landscape snaps them to the terrain", |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: tile_placement <path | #elem | selected> landscape|freeform") };
            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.clone() } else { element_arg(d, &a[0], p).into_iter().collect() };
            if what.is_empty() {
                return fail(format!("no such layer or element '{}'", a[0]));
            }

            let mode = match a[1].as_str() {
                "landscape" => Placement::Landscape,
                "freeform" => Placement::Freeform,
                _ => return fail("the mode is landscape or freeform"),
            };

            ok(format!("{} changed", d.set_placement(&what, mode)))
        },
    );

    c.add(
        "tile_snap", "tile_snap <path | #elem | selected | all> [freeform]",
        "put landscape instances (with `freeform`: every instance) on the terrain; `all` is every instance of the tile", |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_snap <path | #elem | selected | all> [freeform]") };
            let p = primary_of(&sel, d);
            let what: Vec<usize> = match a[0].as_str() {
                "all" => d.tile().tscene().scene_graph().into_iter().collect(),
                "selected" => sel.clone(),
                other => element_arg(d, other, p).into_iter().collect(),
            };

            if what.is_empty() {
                return fail(format!("no such layer or element '{}'", a[0]));
            }

            ok(format!("{} moved", d.snap_to_terrain(&what, a.get(1).is_some_and(|x| x == "freeform"))))
        },
    );

    c.add("tile_set_world", "tile_set_world <x m> <y m> <z m>", "type a world position for the selected instance (the Inspector's X / Y / Z fields)", |e, a| {
        let el = primary(e);
        let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
        let (Some(d), Some((x, y, z))) = (document(e), parsed) else { return fail("usage: tile_set_world <x m> <y m> <z m>") };
        let Some(el) = el.filter(|&el| d.tile().is_instance(el)) else { return fail("no instance selected") };
        d.set_world_metres(el, Float3::new(x, y, z), 0);
        d.history_mut().break_merge();
        ok("")
    });

    c.add(
        "tile_report_off_terrain", "tile_report_off_terrain [tolerance m] [layer path]",
        "log the landscape instances that are more than the tolerance (default 0.5) off the terrain, worst first", |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let tol = match a.first() {
                Some(x) => match number(x) {
                    Some(v) => v,
                    None => return fail("usage: tile_report_off_terrain [tolerance m] [layer path]"),
                },

                None => 0.5,
            };

            let t = d.tile();
            let among = match a.get(1) {
                Some(path) => match t.find_layer(path) {
                    Some(l) => Some(t.instances_below(&[l])),
                    None => return fail(format!("no such layer '{path}'")),
                },

                None => None,
            };

            let list = t.off_terrain(tol, among.as_deref());
            let mut s = format!("{} landscape instance(s) more than {} m off the terrain", list.len(), f6(tol));

            // By type: how many, and the signed range of the offsets.
            let mut by_type: BTreeMap<String, (usize, f32, f32)> = BTreeMap::new();
            for o in &list {
                let acc = by_type.entry(t.tscene().type_of(o.elem).to_owned()).or_insert((0, 1e30, -1e30));
                acc.0 += 1;
                acc.1 = acc.1.min(o.dy);
                acc.2 = acc.2.max(o.dy);
            }

            for (ty, (n, lo, hi)) in &by_type {
                s += &format!("\n  {ty}: {n} ({} .. {} m)", f6(*lo), f6(*hi));
            }

            for o in list.iter().take(8) {
                let w = t.world_position(o.elem);
                s += &format!(
                    "\n  #{} {} '{}' {} m (world {} {} {}, terrain {})", o.elem, t.tscene().type_of(o.elem), t.tscene().name(o.elem), f6(o.dy), f6(w.x), f6(w.y), f6(w.z),
                    f6(t.terrain_height(w.x, w.z)),
                );
            }

            ok(s)
        },
    );

    c.add("assert_placement", "assert_placement <path | #elem | selected> landscape|freeform|none", "test: the element's surface mode", |e, a| {
        let sel = selected(e);
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_placement <path | #elem | selected> landscape|freeform|none") };
        let Some(el) = element_arg(d, &a[0], primary_of(&sel, d)) else { return fail(format!("no such element '{}'", a[0])) };
        let got = match d.tile().placement(el) {
            Placement::Landscape => "landscape",
            Placement::Freeform => "freeform",
            Placement::None => "none",
        };

        if got == a[1] { ok("") } else { fail(format!("placement is {got}, expected {}", a[1])) }
    });

    c.add(
        "assert_on_terrain", "assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]",
        "test: the instance (any placement mode) stands on the terrain (y = the terrain height) or not; for a layer, all instances below it", |e, a| {
            let sel = selected(e);
            let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_on_terrain <path | #elem | selected> 0|1 [tolerance m]") };
            let tol = match a.get(2) {
                Some(x) => match number(x) {
                    Some(v) => v,
                    None => return fail("bad tolerance"),
                },

                None => 0.001,
            };

            let p = primary_of(&sel, d);
            let what = if a[0] == "selected" { sel.clone() } else { element_arg(d, &a[0], p).into_iter().collect() };
            if what.is_empty() {
                return fail(format!("no such element '{}'", a[0]));
            }

            // Any mode: the instance's y against the terrain at its x / z.
            let t = d.tile();
            let (mut off, mut worst) = (0usize, 0f32);
            for el in t.instances_below(&what) {
                let p = t.world_position(el);
                let dy = p.y - t.landscape_y(el, p.x, p.z);
                if dy.abs() > tol {
                    off += 1;
                    if dy.abs() > worst.abs() {
                        worst = dy;
                    }
                }
            }

            let on = off == 0;
            if on != (a[1] == "1") {
                let now = if on { "all stand on the terrain".to_owned() } else { format!("{off} off the terrain, worst {} m", f6(worst)) };
                return fail(format!("{now}, expected {}", if a[1] == "1" { "on" } else { "off" }));
            }

            ok("")
        },
    );

    c.add("tile_inspect", "tile_inspect <element> [piece]", "log the Property Editor rows of an element (or of one of its building pieces) with their values", |e, a| {
        let (Some(d), Some(el)) = (document(e), a.first().and_then(|x| x.parse::<usize>().ok())) else { return fail("usage: tile_inspect <element> [piece]") };
        let item = match a.get(1).and_then(|x| x.parse::<usize>().ok()) {
            Some(k) => wce_tile_editor::tile_document::piece_item(el, k),
            None => wce_tile_editor::tile_document::item_of(el),
        };

        let mut rows = DumpInspector::default();
        let src = wce_scene::render_scene::SourceRef { doc: d.id(), item };
        d.inspect(src, &mut rows);
        for l in &rows.lines {
            wce_foundation::log::info(l.clone());
        }

        ok(format!("{} rows", rows.lines.len()))
    });

    c.add(
        "tile_edit", "tile_edit <element | selected> [piece <n>] <row> <value...>",
        "edit one row of the Property Editor of an element (or of a building piece) as the panel would; the value is a number, true / false, text, `a,b,c`, a choice's number or name, or `Name=true` for check boxes", |e, a| {
            let selected = primary(e);
            let el = a.first().and_then(|x| if x == "selected" { selected } else { x.parse::<usize>().ok() });
            let Some(el) = el else { return fail("usage: tile_edit <element | selected> [piece <n>] <row> <value...>") };
            let (item, rest) = match (a.get(1).map(String::as_str), a.get(2).and_then(|x| x.parse::<usize>().ok())) {
                (Some("piece"), Some(k)) => (wce_tile_editor::tile_document::piece_item(el, k), &a[3..]),
                _ => (wce_tile_editor::tile_document::item_of(el), &a[1..]),
            };

            let (Some(row), true) = (rest.first(), rest.len() >= 2) else { return fail("usage: tile_edit <element> [piece <n>] <row> <value...>") };
            let Some(d) = document(e) else { return fail("no tile is open") };
            let mut rows = DumpInspector { edit: Some((row.clone(), rest[1..].join(" "))), ..Default::default() };
            let src = wce_scene::render_scene::SourceRef { doc: d.id(), item };
            d.inspect(src, &mut rows);
            if rows.applied { ok(format!("{row} set")) } else { fail(format!("no row '{row}' to edit")) }
        },
    );

    c.add("tile_model_bounds", "tile_model_bounds <building key>", "log the bounds of a building key's model pieces (after a few frames, when the models have loaded)", |e, a| {
        let (Some(d), Some(key)) = (document(e), a.first()) else { return fail("usage: tile_model_bounds <building key>") };
        match d.tile().building_model(key) {
            Some(m) => ok(format!("{key}: ok {} resolved {} pieces {} bmin {:.3} {:.3} {:.3} bmax {:.3} {:.3} {:.3}", m.ok, m.resolved, m.pieces.len(), m.bmin.x, m.bmin.y, m.bmin.z, m.bmax.x, m.bmax.y, m.bmax.z)),
            None => fail(format!("the tile has no building key '{key}'")),
        }
    });

    c.add(
        "tile_dump_placement", "tile_dump_placement <file.tsv>",
        "write every instance with its surface mode, world position, the y of the file's transforms (before the landscape snap) and the terrain height under it (TSV), to study placement", |e, a| {
            let (Some(d), Some(path)) = (document(e), a.first()) else { return fail("usage: tile_dump_placement <file.tsv>") };
            let t = d.tile();
            let mut out = "elem\ttype\tname\tmode\twx\twy\twz\tfile_y\tterrain\n".to_owned();
            for &el in t.instances() {
                let w = t.world_position(el);
                let mode = match t.placement(el) {
                    Placement::Landscape => "landscape",
                    Placement::Freeform => "freeform",
                    Placement::None => "none",
                };

                // The C++ stream wrote floats with 6 significant digits.
                let g = |v: f32| wce_formats::c_num::printf_g(f64::from(v));
                out += &format!(
                    "{el}\t{}\t{}\t{mode}\t{}\t{}\t{}\t{}\t{}\n", t.tscene().type_of(el), t.tscene().name(el), g(w.x), g(w.y), g(w.z),
                    g((world_transform(t.tscene(), el).position.y / d.project().unit_scale) as f32), g(t.terrain_height(w.x, w.z)),
                );
            }

            match std::fs::write(path, out) {
                Ok(()) => ok(format!("{} instances written", t.instances().len())),
                Err(_) => fail(format!("cannot write {path}")),
            }
        },
    );

    c.add("assert_off_terrain_count", "assert_off_terrain_count <tolerance m> <n>", "test: n landscape instances of the tile are more than the tolerance off the terrain", |e, a| {
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((tol, n))) = (document(e), parsed) else { return fail("usage: assert_off_terrain_count <tolerance m> <n>") };
        let got = d.tile().off_terrain(tol, None).len();
        if got == n as usize { ok("") } else { fail(format!("{got} instances off the terrain, expected {}", a[1])) }
    });

    c.add("assert_placement_counts", "assert_placement_counts <landscape n> <freeform n>", "test: how many instances are landscape / freeform", |e, a| {
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((l, f))) = (document(e), parsed) else { return fail("usage: assert_placement_counts <landscape n> <freeform n>") };
        let c = d.tile().placement_counts();
        if c.landscape == l as usize && c.freeform == f as usize { ok("") } else { fail(format!("landscape {}, freeform {}", c.landscape, c.freeform)) }
    });

    // ---- ground look
    c.add(
        "tile_terrain", "tile_terrain blended|flat [climate] [season sp|su|au|wi]",
        "the ground colour of the open tile: the blend map mixed from the climate's textures, or the plain tiled texture", |e, a| {
            let Some(id) = crate::tool::document_id(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_terrain blended|flat [climate] [season sp|su|au|wi]") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let mut look = d.tile().terrain_look().clone();
            match a[0].as_str() {
                "blended" => look.blended = true,
                "flat" => look.blended = false,
                _ => return fail("the first word is blended or flat"),
            }

            if let Some(climate) = a.get(1) {
                look.climate = climate.clone();
            }

            if let Some(season) = a.get(2) {
                look.season = season.clone();
            }

            let climates = d.tile().climates();
            if look.blended && climates.find(&look.climate).is_none() && !climates.climates.is_empty() {
                return fail(format!("no climate '{}' in the kit's _settings.xml", look.climate));
            }

            d.tile_mut().set_terrain_look(&mut e.scene, look);
            ok(d.tile().terrain_note().to_owned())
        },
    );

    c.add(
        "tile_ground_view", "tile_ground_view colour|groups|ground|protection",
        "what the ground shows: the blended colour or a debug view of the tile's maps (blend groups, ground types, protection)", |e, a| {
            let Some(id) = crate::tool::document_id(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_ground_view colour|groups|ground|protection") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let mut look = d.tile().terrain_look().clone();
            look.blended = true;
            look.view = match a[0].as_str() {
                "colour" => TerrainView::Colour,
                "groups" => TerrainView::BlendGroups,
                "ground" => TerrainView::GroundTypes,
                "protection" => TerrainView::Protection,
                _ => return fail("the view is colour, groups, ground or protection"),
            };

            d.tile_mut().set_terrain_look(&mut e.scene, look);
            ok(d.tile().terrain_note().to_owned())
        },
    );

    c.add("tile_links", "tile_links [on|off]", "log the tile database entry's link targets and rules (the cells they name), or show / hide them on the ground", |e, a| {
        let Some(d) = document(e) else { return no_tile() };
        if let Some(w) = a.first() {
            if w != "on" && w != "off" {
                return fail("usage: tile_links [on|off]");
            }

            d.tile_mut().display_mut().show_links = w == "on";
            d.tile_mut().display_changed();
            return ok("");
        }

        let cells = d.tile().link_cells();
        let s = cells
            .iter()
            .map(|c| format!("\n  {}{} {}", if c.target { "target " } else if c.equals { "= " } else { "!= " }, c.set, c.text))
            .collect::<String>();
        ok(format!("{} link item(s){s}", cells.len()))
    });

    c.add("assert_links", "assert_links <targets n> <rules n> [drawn 0|1]", "test: the entry has n link targets and n rules; whether they are drawn on the ground (after a frame)", |e, a| {
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((t, r))) = (document(e), parsed) else { return fail("usage: assert_links <targets n> <rules n> [drawn 0|1]") };
        let cells = d.tile().link_cells();
        let targets = cells.iter().filter(|c| c.target).count();
        let rules = cells.len() - targets;
        if targets != t as usize || rules != r as usize {
            return fail(format!("{targets} targets, {rules} rules"));
        }

        let drawn = d.tile().links_drawn();
        if a.get(2).is_some_and(|x| (x == "1") != drawn) {
            return fail(format!("links are {}drawn", if drawn { "" } else { "not " }));
        }

        ok("")
    });

    c.add("assert_link_cell", "assert_link_cell <x> <y> <x0 m> <z0 m>", "test: the entry cell (x, y) lies at this world corner (the row counts from the +z edge)", |e, a| {
        let parsed = (a.len() >= 4).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
        let (Some(d), Some((x, y, ex, ez))) = (document(e), parsed) else { return fail("usage: assert_link_cell <x> <y> <x0 m> <z0 m>") };
        let (x0, z0, _, _) = d.tile().cell_rect(x as i32, y as i32);
        if (x0 - ex).abs() < 0.01 && (z0 - ez).abs() < 0.01 { ok("") } else { fail(format!("cell corner is {}, {}", f6(x0), f6(z0))) }
    });

    c.add("tile_water", "tile_water on|off", "show or hide the river water of the open tile", |e, a| {
        let (Some(d), Some(w)) = (document(e), a.first().filter(|w| *w == "on" || *w == "off")) else { return fail("usage: tile_water on|off") };
        d.tile_mut().display_mut().show_water = w == "on";
        d.tile_mut().display_changed();
        ok("")
    });

    c.add("tile_favourite", "tile_favourite <entry key>", "star a tile of the Tiles panel's source (its location: terrain/tiles/...), or take its star away", |e, a| {
        let Some(key) = a.first().map(|k| wce_formats::tile_entry::normalize_location(k)) else { return fail("usage: tile_favourite <entry key>") };
        with_tile(e, |t, e| {
            let source = t.browse_source();
            ok(if t.toggle_favourite(e, &source, &key) { format!("{key}: favourite") } else { format!("{key}: not a favourite") })
        })
    });

    c.add("tile_marks", "tile_marks", "log the favourite and the recently opened tiles (source, key, variation)", |e, _| {
        with_tile(e, |t, e| {
            let (favourites, recent) = t.marks(e);
            let line = |r: &crate::settings::TileRef| format!("  {}  {}{}\n", r.source, r.key, if r.variation.is_empty() { String::new() } else { format!("  ({})", r.variation) });
            let text = format!("favourites:\n{}recent:\n{}", favourites.iter().map(line).collect::<String>(), recent.iter().map(line).collect::<String>());
            wce_foundation::log::info(&text);
            ok(text)
        })
    });

    c.add(
        "tile_weather", "tile_weather [file | none]",
        "light the open tile with an environment file of the packs (none: the default light), kept for the tile in the settings; no file: log the files the game picks under the filters and the search",
        |e, a| {
            let file = a.first().cloned();
            with_tile(e, |t, e| match file.as_deref() {
                None => {
                    let (files, _) = t.weather.matching();
                    let lines = files.iter().map(|(f, n)| if *n > 1 { format!("{f}  (x{n})") } else { f.clone() }).collect::<Vec<_>>();
                    ok(format!("{} files:\n{}", files.len(), lines.join("\n")))
                }
                Some("none") => t.weather.set_file(e, "").map_or_else(fail, ok),
                Some(f) => t.weather.set_file(e, f).map_or_else(fail, ok),
            })
        },
    );

    c.add(
        "tile_weather_filter", "tile_weather_filter <category> [value]",
        "a filter of the open tile's weather list (an environment_map.xml category: map, climate, season, climate_change_index, precipitation, time_of_day, battle_types, snow_attrition); no value: any",
        |e, a| {
            let Some(category) = a.first().cloned() else { return fail("usage: tile_weather_filter <category> [value]") };
            let value = a.get(1).cloned().unwrap_or_default();
            with_tile(e, |t, e| t.weather.set_filter(e, &category, &value).map_or_else(fail, |()| ok("")))
        },
    );

    c.add("tile_weather_edit", "tile_weather_edit", "edit the open tile's weather in the Weather tool (a copy in the output folder, which the tile follows)", |e, _| {
        with_tile(e, |t, e| {
            let Some(file) = e.weather_preview().map(str::to_owned) else { return fail("the tile has no weather file") };
            t.weather.edit(e, &file);
            ok("")
        })
    });

    c.add("tile_weather_search", "tile_weather_search [words]", "the words (and-ed) the files of the open tile's weather list must hold; none: every file", |e, a| {
        let words = a.join(" ");
        with_tile(e, |t, _| {
            t.weather.set_search(&words);
            ok("")
        })
    });

    c.add(
        "tile_type", "tile_type <type> on|off|only | tile_type all",
        "the Scene panel's type toggles: list and show a type or not (a view setting), show only that type, or show every type",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let types = d.tile().type_counts().keys().cloned().collect::<Vec<_>>();
            let t = d.tile_mut();
            match (a.first().map(String::as_str), a.get(1).map(String::as_str)) {
                (Some("all"), None) => t.display_mut().hidden_types.clear(),
                (Some(ty), Some(how @ ("on" | "off" | "only"))) => {
                    if !types.iter().any(|x| x == ty) {
                        return fail(format!("no {ty} on this tile; types: {}", types.join(", ")));
                    }

                    match how {
                        "on" => {
                            t.display_mut().hidden_types.remove(ty);
                        }
                        "off" => {
                            t.display_mut().hidden_types.insert(ty.to_owned());
                        }
                        _ => t.display_mut().hidden_types = types.iter().filter(|x| *x != ty).cloned().collect(),
                    }
                }
                _ => return fail("usage: tile_type <type> on|off|only | tile_type all"),
            }

            t.display_changed();
            ok("")
        },
    );

    c.add("tile_sea", "tile_sea on|off", "show or hide the sea at 0 m (the tile's plane and the map context's sea; kept in the settings)", |e, a| {
        let Some(w) = a.first().filter(|w| *w == "on" || *w == "off") else { return fail("usage: tile_sea on|off") };
        let Some(id) = crate::tool::document_id(e) else { return no_tile() };
        with_tile(e, |t, e| {
            t.set_show_sea(e, id, w == "on");
            ok("")
        })
    });

    c.add("tile_vfx_boxes", "tile_vfx_boxes on|off", "show or hide the VFX boxes (the VFXInstance markers; the effects still play; kept in the settings)", |e, a| {
        let Some(w) = a.first().filter(|w| *w == "on" || *w == "off") else { return fail("usage: tile_vfx_boxes on|off") };
        crate::tool::set_vfx_boxes(e, w == "on");
        ok("")
    });

    c.add("tile_sea_state", "tile_sea_state [wind]", "draw the sea with a battle wind's waves and whitecaps (a key of wind_levels_tables; kept in the settings); no wind: list them", |e, a| {
        let states = e.renderer().map(|r| r.sea_states()).unwrap_or_default();
        let Some(key) = a.first() else { return ok(format!("sea states: {}", states.join(", "))) };
        if !states.contains(key) {
            return fail(format!("no wind '{key}'; winds: {}", states.join(", ")));
        }

        crate::tool::set_sea_state(e, key);
        ok("")
    });

    c.add("tree_list", "tree_list [name | gen:name | none]", "draw one of the tile's procedural tree lists (BOB's <name>.tree_list.bin: default, a climate key; gen:<name> generates it from the seeds, again on every call); no name: log the lists", |e, a| {
        let Some(d) = document(e) else { return fail("no tile open") };
        let lists = d.tile().tree_lists().to_vec();
        let Some(name) = a.first() else { return ok(format!("tree lists: {} (drawn: {})", lists.join(", "), d.tile().display().tree_list)) };
        let name = if *name == "none" { String::new() } else { name.to_lowercase() };
        if !name.is_empty() && !lists.contains(&name) {
            return fail(format!("no tree list {name}; the tile has: {}", lists.join(", ")));
        }

        d.tile_mut().display_mut().tree_list = name;
        d.tile_mut().reload_tree_list();
        ok("")
    });

    c.add("grass", "grass on|off|generated|packed", "draw the tile's grass near the camera: on / off; packed: BOB's default.grass_list.bin when the tile has one; generated: from the seed (again on every call)", |e, a| {
        let Some(d) = document(e) else { return fail("no tile open") };
        let t = d.tile_mut();
        match a.first().map(String::as_str) {
            Some("on") => t.display_mut().show_grass = true,
            Some("off") => t.display_mut().show_grass = false,
            Some("generated") => {
                t.display_mut().generate_grass = true;
                t.reload_grass();
            }
            Some("packed") => t.display_mut().generate_grass = false,
            _ => return fail("usage: grass on|off|generated|packed"),
        }

        ok("")
    });

    c.add("assert_water", "assert_water <pieces n>", "test: the tile has n water meshes (river splines + the sea plane) in the scene (after a frame)", |e, a| {
        let (Some(d), Some(n)) = (document(e), a.first().and_then(|x| number(x))) else { return fail("usage: assert_water <pieces n>") };
        let t = d.tile();
        if t.water_pieces() == n as usize {
            ok(t.water_note().to_owned())
        } else {
            fail(format!("{} water meshes: {}", t.water_pieces(), t.water_note()))
        }
    });

    c.add(
        "assert_ground", "assert_ground blended|flat [layers n] [climate]",
        "test: the ground is drawn by the game's terrain shaders (with this many textures in the climate's set) or is the plain texture / a debug view", |e, a| {
            let (Some(d), Some(kind)) = (document(e), a.first()) else { return fail("usage: assert_ground blended|flat [layers n] [climate]") };
            let t = d.tile();

            // A debug view's image.
            let baked = t.baked_colour().is_some();
            let g = t.ground_info();
            let note = t.terrain_note().to_owned();
            if kind == "flat" {
                return if !g.game_shaders && !baked { ok("") } else { fail(format!("the ground is blended: {note}")) };
            }

            if !g.game_shaders && !baked {
                return fail(format!("the ground is not blended: {note}"));
            }

            // A debug view of the maps: blended in the sense of the look.
            if !g.game_shaders {
                return ok(note);
            }

            if let Some(n) = a.get(1).and_then(|x| number(x))
                && g.layers != n as i32
            {
                return fail(format!("{} textures, expected {}", g.layers, a[1]));
            }

            if let Some(climate) = a.get(2)
                && g.climate != *climate
            {
                return fail(format!("climate {}, expected {climate}", g.climate));
            }

            ok(note)
        },
    );

    c.add("assert_instances", "assert_instances <n>", "test: the tile has n instances", |e, a| {
        let (Some(d), Some(n)) = (document(e), a.first()) else { return fail("usage: assert_instances <n> (with a tile open)") };
        let have = d.tile().instances().len();
        if have as i64 == atoi(n) { ok("") } else { fail(format!("instances: {have}, expected {n}")) }
    });

    c.add("assert_type_count", "assert_type_count <type> <n>", "test: the tile has n instances of a type", |e, a| {
        let Some(d) = document(e).filter(|_| a.len() >= 2) else { return fail("usage: assert_type_count <type> <n> (with a tile open)") };
        let n = d.tile().type_counts().get(&a[0]).copied().unwrap_or(0);
        if i64::from(n) == atoi(&a[1]) { ok("") } else { fail(format!("{}: {n}, expected {}", a[0], a[1])) }
    });

    c.add("assert_world", "assert_world <x m> <y m> <z m> [tolerance]", "test: the selected instance is at this world position", |e, a| {
        let el = primary(e);
        let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
        let (Some(d), Some((x, y, z))) = (document(e), parsed) else { return fail("usage: assert_world <x m> <y m> <z m> [tolerance]") };
        let tol = a.get(3).and_then(|v| number(v)).unwrap_or(0.01);
        let Some(el) = el else { return fail("nothing selected") };
        let p = d.tile().world_position(el);
        if (p.x - x).abs() <= tol && (p.y - y).abs() <= tol && (p.z - z).abs() <= tol {
            ok("")
        } else {
            fail(format!("at {:.3} {:.3} {:.3}, expected {x:.3} {y:.3} {z:.3}", p.x, p.y, p.z))
        }
    });

    c.add("assert_world_xz", "assert_world_xz <x m> <z m> [tolerance]", "test: the selected instance is at this ground position (any height)", |e, a| {
        let el = primary(e);
        let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
        let (Some(d), Some((x, z))) = (document(e), parsed) else { return fail("usage: assert_world_xz <x m> <z m> [tolerance]") };
        let tol = a.get(2).and_then(|v| number(v)).unwrap_or(0.01);
        let Some(el) = el else { return fail("nothing selected") };
        let p = d.tile().world_position(el);
        if (p.x - x).abs() <= tol && (p.z - z).abs() <= tol { ok("") } else { fail(format!("at {:.3} {:.3}, expected {x:.3} {z:.3}", p.x, p.z)) }
    });

    c.add("mark_height", "mark_height <name> <x m> <z m>", "remember the terrain height at a point under a name (for assert_height_delta)", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 3).then(|| Some((number(&a[1])?, number(&a[2])?))).flatten();
            let Some((x, z)) = parsed else { return fail("usage: mark_height <name> <x m> <z m>") };
            let h = document(e).expect("a tile").tile().terrain_height(x, z);
            t.marks.insert(a[0].clone(), h);
            ok("")
        })
    });

    c.add("assert_height_delta", "assert_height_delta <name> <x m> <z m> <min> <max>", "test: the height at a point changed by an amount in [min, max] since mark_height", |e, a| {
        with_doc(e, |t, e| {
            let parsed = (a.len() >= 5).then(|| Some((number(&a[1])?, number(&a[2])?, number(&a[3])?, number(&a[4])?))).flatten();
            let Some((x, z, lo, hi)) = parsed else { return fail("usage: assert_height_delta <name> <x m> <z m> <min> <max>") };
            let Some(&mark) = t.marks.get(&a[0]) else { return fail(format!("no mark '{}'", a[0])) };
            let delta = document(e).expect("a tile").tile().terrain_height(x, z) - mark;
            if delta >= lo && delta <= hi { ok("") } else { fail(format!("height changed by {delta:.4}, expected {lo:.4} .. {hi:.4}")) }
        })
    });

    c.add("assert_height", "assert_height <x m> <z m> <height m> [tolerance]", "test: the terrain height at a point", |e, a| {
        let parsed = (a.len() >= 3).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?))).flatten();
        let (Some(d), Some((x, z, h))) = (document(e), parsed) else { return fail("usage: assert_height <x m> <z m> <height m> [tolerance]") };
        let tol = a.get(3).and_then(|v| number(v)).unwrap_or(0.01);
        let got = d.tile().terrain_height(x, z);
        if (got - h).abs() <= tol { ok("") } else { fail(format!("height {got:.4}, expected {h:.4}")) }
    });

    c.add("assert_selected_type", "assert_selected_type <type>", "test: the primary selected element has this type", |e, a| {
        let el = primary(e);
        let (Some(d), Some(want)) = (document(e), a.first()) else { return fail("usage: assert_selected_type <type>") };
        let Some(el) = el else { return fail("nothing selected") };
        let ty = d.tile().tscene().type_of(el);
        if ty == want { ok("") } else { fail(format!("selected {ty}")) }
    });

    c.add("assert_file_exists", "assert_file_exists <path>", "test: a file exists", |_, a| {
        match a.first() {
            Some(p) if Path::new(p).exists() => ok(""),
            other => fail(format!("missing {}", other.map_or("", String::as_str))),
        }
    });
}

/// The first selected instance of `sel` (what "selected" names).
fn primary_of(sel: &[usize], d: &TileDocument) -> Option<usize> {
    sel.iter().copied().find(|&el| d.tile().is_instance(el))
}

/// An element named on a command line: "#123" (an element index), a layer path ("Walls/Standard_Walls"), "selected" (the first selected
/// item, read before the document was borrowed).
fn element_arg(d: &TileDocument, a: &str, primary: Option<usize>) -> Option<usize> {
    if a.is_empty() {
        return None;
    }

    if a == "selected" {
        return primary;
    }

    if let Some(n) = a.strip_prefix('#') {
        return usize::try_from(atoi(n)).ok();
    }

    d.tile().find_layer(a)
}
