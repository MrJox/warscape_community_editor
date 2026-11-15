//! How the tile is shown: the ground look, the minimap, links, technology, water and sea, weather, marks, grass and trees; the checks of what the scene holds.

use super::*;

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
    // ---- ground look
    c.add(
        "tile_terrain",
        "tile_terrain blended|flat [climate] [season sp|su|au|wi]",
        "the ground colour of the open tile: the blend map mixed from the climate's textures, or the plain tiled texture",
        |e, a| {
            let Some(id) = crate::tool::document_id(e).filter(|_| !a.is_empty()) else {
                return fail("usage: tile_terrain blended|flat [climate] [season sp|su|au|wi]");
            };
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
                return fail(format!("no climate '{}' in the tile database", look.climate));
            }

            d.tile_mut().set_terrain_look(&mut e.scene, look);
            ok(d.tile().terrain_note().to_owned())
        },
    );

    c.add(
        "tile_ground_view", "tile_ground_view colour|groups|ground|protection|water",
        "what the ground shows: the blended colour or a debug view of the tile's maps (blend groups, ground types, protection, water depth)", |e, a| {
            let Some(id) = crate::tool::document_id(e).filter(|_| !a.is_empty()) else { return fail("usage: tile_ground_view colour|groups|ground|protection|water") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let mut look = d.tile().terrain_look().clone();
            look.blended = true;
            look.view = match a[0].as_str() {
                "colour" => TerrainView::Colour,
                "groups" => TerrainView::BlendGroups,
                "ground" => TerrainView::GroundTypes,
                "protection" => TerrainView::Protection,
                "water" => TerrainView::WaterDepth,
                _ => return fail("the view is colour, groups, ground, protection or water"),
            };

            d.tile_mut().set_terrain_look(&mut e.scene, look);
            ok(d.tile().terrain_note().to_owned())
        },
    );

    c.add(
        "tile_auto_nogo", "tile_auto_nogo [on|off]",
        "show or hide the automatic no-go areas (BOB's terrain_outlines.xml: the steep ground of the final heights) on the ground; their count", |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return fail("no tile") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            match a.first().map(String::as_str) {
                None => {}
                Some(w @ ("on" | "off")) => {
                    d.tile_mut().display_mut().show_auto_nogo = w == "on";
                    d.tile_mut().display_changed();
                }

                Some(_) => return fail("usage: tile_auto_nogo [on|off]"),
            }

            let shown = d.tile().display().show_auto_nogo;
            ok(match d.tile().auto_nogos() {
                Some(n) => format!("automatic no-go areas {}: {} area(s), {} point(s)", if shown { "shown" } else { "hidden" }, n.len(), n.iter().map(Vec::len).sum::<usize>()),
                None => format!("automatic no-go areas {}: not worked out yet", if shown { "shown" } else { "hidden" }),
            })
        },
    );

    c.add(
        "minimap", "minimap [colour|groups|ground|protection|water|height|radar|none | siege|deployment|cells|nogo|camera on|off | zoom <k> [x z] | goto <x> <z>]",
        "the Minimap panel: its base picture, its layers, its zoom (and centre, metres); goto moves the camera as a click on it does", |e, a| {
            with_tile(e, |t, e| t.minimap.command(e, a).map_or_else(fail, ok))
        },
    );

    c.add("radar_save", "radar_save [force] [path.jpg|path.png]", "write the open tile's radar (docs/plans/radar.md: the minimap the game shows, drawn from the tile) as the game's JPEG (a PNG for a .png path), by default the tile folder's loading_screen_radar.jpg; force replaces a hand-made radar", |e, a| {
        with_tile(e, |t, e| {
            let id = crate::tool::document_id(e).ok_or("no tile");
            if let Ok(id) = id
                && let Err(why) = t.ensure_radar_water(e, id)
            {
                wce_foundation::log::warn(format!("radar: no rivers or cliffs: {why}"));
            }
            let force = a.iter().any(|w| w == "force");
            let path = a.iter().find(|w| *w != "force").map(String::as_str);
            t.minimap.save_radar(e, path, force).map_or_else(fail, ok)
        })
    });

    c.add(
        "tile_links",
        "tile_links [on|off]",
        "log the tile database entry's link targets and rules (the cells they name), or show / hide them on the ground",
        |e, a| {
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
                .map(|c| {
                    format!(
                        "\n  {}{} {}",
                        if c.target {
                            "target "
                        } else if c.equals {
                            "= "
                        } else {
                            "!= "
                        },
                        c.set,
                        c.text
                    )
                })
                .collect::<String>();
            ok(format!("{} link item(s){s}", cells.len()))
        },
    );

    c.add(
        "link_edit", "link_edit add target <set> <x> <y> | add rule <set> <x> <y> [not] | remove target|rule <i> | set rule <i> <field> <value>",
        "edit the tile's links as the Tile panel's Tile links does (one undo entry); fields: set x y test entry blend offline base_x base_y",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let Some(en) = d.project().entry.as_ref() else { return fail("no tile database entry") };
            let (mut targets, mut links) = (en.link_targets(), en.links());
            let int = |i: usize| a.get(i).and_then(|v| v.parse::<i32>().ok());
            let usage = "usage: link_edit add target <set> <x> <y> | add rule <set> <x> <y> [not] | remove target|rule <i> | set rule <i> <field> <value>";
            let name = match (a.first().map(String::as_str), a.get(1).map(String::as_str)) {
                (Some("add"), Some("target")) => {
                    let (Some(set), Some(x), Some(y)) = (a.get(2), int(3), int(4)) else { return fail(usage) };
                    targets.push(wce_formats::tile_entry::TileLinkTarget { elem: 0, serialise_version: 1, tile_set: set.clone(), x, y });
                    "Add a link target"
                }

                (Some("add"), Some("rule")) => {
                    let (Some(set), Some(x), Some(y)) = (a.get(2), int(3), int(4)) else { return fail(usage) };
                    let test = if a.get(5).is_some_and(|v| v == "not") { "TLT_NOT_EQUALS" } else { "TLT_EQUALS" };
                    links.push(wce_formats::tile_entry::TileLink {
                        elem: 0, serialise_version: 1, link_set: set.clone(), x, y, base_x: 1, base_y: 0, is_entry: false, blend_size: 64,
                        no_offline_blend: false, test: test.to_owned(), has_blend_quad: true,
                    });
                    "Add a link rule"
                }

                (Some("remove"), Some(kind)) => {
                    let Some(i) = int(2).map(|i| i as usize) else { return fail(usage) };
                    match kind {
                        "target" if i < targets.len() => drop(targets.remove(i)),
                        "rule" if i < links.len() => drop(links.remove(i)),
                        _ => return fail("no such link"),
                    }

                    "Remove a link"
                }

                (Some("set"), Some("rule")) => {
                    let (Some(i), Some(field), Some(v)) = (int(2).map(|i| i as usize), a.get(3), a.get(4)) else { return fail(usage) };
                    let Some(l) = links.get_mut(i) else { return fail("no such rule") };
                    let n = v.parse::<i32>().unwrap_or(0);
                    match field.as_str() {
                        "set" => l.link_set = v.clone(),
                        "x" => l.x = n,
                        "y" => l.y = n,
                        "test" => l.test = if v == "not" { "TLT_NOT_EQUALS" } else { "TLT_EQUALS" }.to_owned(),
                        "entry" => l.is_entry = v == "true",
                        "blend" => l.blend_size = n.max(0) as u32,
                        "offline" => l.no_offline_blend = v == "true",
                        "base_x" => l.base_x = n,
                        "base_y" => l.base_y = n,
                        _ => return fail(format!("no field {field}")),
                    }

                    "Change a link rule"
                }

                _ => return fail(usage),
            };

            match d.set_links(name, targets, links, 0) {
                Ok(true) => ok(""),
                Ok(false) => fail("nothing changed"),
                Err(err) => fail(err),
            }
        },
    );

    c.add(
        "link_candidates",
        "link_candidates [show <join> <k>]",
        "find the tiles that can follow the open tile at each join (Tile links); with show, show one in the map context",
        |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            if a.first().is_some_and(|x| x == "show") {
                let (Some(j), Some(k)) = (a.get(1).and_then(|v| v.parse().ok()), a.get(2).and_then(|v| v.parse().ok())) else {
                    return fail("usage: link_candidates show <join> <k>");
                };
                return with_tile(e, |t, e| t.show_link_candidate(e, id, j, k).map_or_else(fail, ok));
            }

            with_tile(e, |t, e| match t.find_link_candidates(e, id) {
                Ok(c) => ok(format!("per join: {c:?}")),
                Err(err) => fail(err),
            })
        },
    );

    c.add("tile_prop", "tile_prop <attribute> <value> [variation]", "set a tile property of the entry as the Tile panel's Tile properties does (red green blue mask random_rotatable ...; variation: normal_strength)", |e, a| {
        let (Some(attr), Some(value)) = (a.first().cloned(), a.get(1).cloned()) else { return fail("usage: tile_prop <attribute> <value> [variation]") };
        let value = if value == "-" { String::new() } else { value };
        let variation = a.get(2).is_some_and(|v| v == "variation");
        let Some(d) = document(e) else { return no_tile() };
        match d.set_entry_attr(variation, &attr, &value, 0) {
            Ok(true) => ok(""),
            Ok(false) => fail("it had that value"),
            Err(err) => fail(err),
        }
    });

    c.add(
        "integrity_errors",
        "integrity_errors [n]",
        "log TEd's scene integrity errors of the tile (the Problems section); with n, test that there are that many",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let p = d.tile().integrity_errors();
            for (el, m) in &p {
                wce_foundation::log::info(format!(
                    "integrity: #{el} {} '{}': {m}",
                    d.tile().tscene().type_of(*el),
                    d.tile().tscene().name(*el)
                ));
            }

            match a.first().and_then(|v| v.parse::<usize>().ok()) {
                Some(n) if n != p.len() => fail(format!("{} integrity error(s), expected {n}", p.len())),
                _ => ok(format!("{} integrity error(s)", p.len())),
            }
        },
    );

    c.add(
        "link_problems",
        "link_problems [n]",
        "log the tile's link problems (the validator); with n, test that there are that many",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let p = d.link_problems();
            for x in &p {
                wce_foundation::log::info(format!("link problem: {}", x.message));
            }

            match a.first().and_then(|v| v.parse::<usize>().ok()) {
                Some(n) if n != p.len() => fail(format!("{} link problem(s), expected {n}", p.len())),
                _ => ok(format!("{} link problem(s)", p.len())),
            }
        },
    );

    c.add(
        "assert_links",
        "assert_links <targets n> <rules n> [drawn 0|1]",
        "test: the entry has n link targets and n rules; whether they are drawn on the ground (after a frame)",
        |e, a| {
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((t, r))) = (document(e), parsed) else {
                return fail("usage: assert_links <targets n> <rules n> [drawn 0|1]");
            };
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
        },
    );

    c.add(
        "assert_link_cell",
        "assert_link_cell <x> <y> <x0 m> <z0 m>",
        "test: the entry cell (x, y) lies at this world corner (the row counts from the +z edge)",
        |e, a| {
            let parsed = (a.len() >= 4).then(|| Some((number(&a[0])?, number(&a[1])?, number(&a[2])?, number(&a[3])?))).flatten();
            let (Some(d), Some((x, y, ex, ez))) = (document(e), parsed) else {
                return fail("usage: assert_link_cell <x> <y> <x0 m> <z0 m>");
            };
            let (x0, z0, _, _) = d.tile().cell_rect(x as i32, y as i32);
            if (x0 - ex).abs() < 0.01 && (z0 - ez).abs() < 0.01 { ok("") } else { fail(format!("cell corner is {}, {}", f6(x0), f6(z0))) }
        },
    );

    c.add(
        "tile_variation", "tile_variation [<variation>]",
        "the variation of a combined project in view (the Variation chip): its scene and its terrain; no argument logs the project's variations",
        |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return fail("no tile") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let vars = d.tile().variations().to_vec();
            if vars.is_empty() {
                return fail("the tile has one variation: it is not a combined project");
            }

            if let Some(name) = a.first() {
                let Some(name) = vars.iter().find(|v| v.eq_ignore_ascii_case(name)) else {
                    return fail(format!("no variation '{name}' (the project has {})", vars.join(", ")));
                };

                if let Err(m) = d.tile_mut().view_variation(&mut e.scene, name) {
                    return fail(m);
                }
            }

            ok(format!("viewing {} of {}", d.tile().viewed_variation().unwrap_or_default(), vars.join(", ")))
        },
    );

    c.add(
        "tile_tech", "tile_tech [all | none | on|off <kind>... | scope all|selected | range <m> | xray|highest|detect on|off | fill <0..1>]",
        "the building technology overlays (the chip at the viewport's top right): kinds are collision gates outlines soft footprints platforms pipes formation docking arrows actionfx attached emitters bounds flag; no argument logs the settings", |e, a| {
            use wce_tile_editor::tile_document::{KINDS, TechKind, TechScope};
            let Some(id) = crate::tool::document_id(e) else { return fail("no tile") };
            let d = e.docs.find_as_mut::<TileDocument>(id).expect("a tile");
            let mut s = d.tile().display().tech.clone();
            let flag = |w: Option<&String>| match w.map(String::as_str) {
                Some("on") => Some(true),
                Some("off") => Some(false),
                _ => None,
            };

            match a.first().map(String::as_str) {
                None => {}
                Some("all") => s.set_all(true),
                Some("none") => s.set_all(false),
                Some(w @ ("on" | "off")) => {
                    for k in &a[1..] {
                        let Some(kind) = TechKind::from_key(k) else { return fail(format!("no kind {k}: {}", KINDS.map(TechKind::key).join(" "))) };
                        s.set(kind, w == "on");
                    }
                }

                Some("scope") => match a.get(1).map(String::as_str) {
                    Some("all") => s.scope = TechScope::All,
                    Some("selected") => s.scope = TechScope::Selected,
                    _ => return fail("usage: tile_tech scope all|selected"),
                },

                Some("range") => match a.get(1).and_then(|m| number(m)) {
                    Some(m) if m > 0.0 => s.distance = m,
                    _ => return fail("usage: tile_tech range <m>"),
                },

                Some("fill") => match a.get(1).and_then(|m| number(m)) {
                    Some(k) => s.fill = k.clamp(0.0, 1.0),
                    None => return fail("usage: tile_tech fill <0..1>"),
                },

                Some(w @ ("xray" | "highest" | "detect")) => {
                    let Some(on) = flag(a.get(1)) else { return fail(format!("usage: tile_tech {w} on|off")) };
                    match w {
                        "xray" => s.xray = on,
                        "highest" => s.highest = on,
                        _ => s.detect = on,
                    }
                }

                Some(_) => return fail("usage: tile_tech [all | none | on|off <kind>... | scope all|selected | range <m> | xray|highest|detect on|off | fill <0..1>]"),
            }

            d.tile_mut().display_mut().tech = s.clone();
            d.tile_mut().tech_changed();
            let on: Vec<&str> = KINDS.iter().filter(|k| s.on(**k)).map(|k| k.key()).collect();
            let stats = d.tile().tech_stats();
            ok(format!(
                "technology: {} | scope {} within {} m | xray {} | highest {} | detect {} | last drawn: {} building(s), {} line(s), {} collision(s)",
                if on.is_empty() { "none".to_owned() } else { on.join(" ") },
                if s.scope == TechScope::All { "all" } else { "selected" }, s.distance, s.xray, s.highest, s.detect, stats.buildings, stats.lines, stats.collisions,
            ))
        },
    );

    c.add(
        "assert_tech", "assert_tech <buildings> [min lines] [collisions n]",
        "test: the technology overlay drew this many buildings (after a frame), at least this many lines, and (with collisions) this many overlapping pairs", |e, a| {
            let (Some(d), Some(n)) = (document(e), a.first().and_then(|x| number(x))) else { return fail("usage: assert_tech <buildings> [min lines] [collisions n]") };
            let stats = d.tile().tech_stats();
            let lines = a.get(1).filter(|x| *x != "collisions").and_then(|x| number(x)).unwrap_or(0.0) as usize;
            let pairs = a.iter().position(|x| x == "collisions").and_then(|i| a.get(i + 1)).and_then(|x| number(x));
            if stats.buildings != n as usize {
                return fail(format!("{} building(s) drawn, expected {}", stats.buildings, n as usize));
            }

            if stats.lines < lines {
                return fail(format!("{} line(s) drawn, expected at least {lines}", stats.lines));
            }

            match pairs {
                Some(p) if stats.collisions != p as usize => fail(format!("{} collision(s), expected {}", stats.collisions, p as usize)),
                _ => ok(format!("{} building(s), {} line(s), {} collision(s)", stats.buildings, stats.lines, stats.collisions)),
            }
        },
    );

    c.add("tile_water", "tile_water on|off", "show or hide the river water of the open tile", |e, a| {
        let (Some(d), Some(w)) = (document(e), a.first().filter(|w| *w == "on" || *w == "off")) else {
            return fail("usage: tile_water on|off");
        };
        d.tile_mut().display_mut().show_water = w == "on";
        d.tile_mut().display_changed();
        ok("")
    });

    c.add("tile_marks", "tile_marks", "log the favourite and the recently opened tiles (source, key, variation)", |e, _| {
        with_tile(e, |t, e| {
            let (favourites, recent) = t.marks(e);
            let line = |r: &crate::settings::TileRef| {
                format!(
                    "  {}  {}{}\n",
                    r.source,
                    r.key,
                    if r.variation.is_empty() { String::new() } else { format!("  ({})", r.variation) }
                )
            };
            let text = format!(
                "favourites:\n{}recent:\n{}",
                favourites.iter().map(line).collect::<String>(),
                recent.iter().map(line).collect::<String>()
            );
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

    c.add(
        "tile_weather_edit",
        "tile_weather_edit",
        "edit the open tile's weather in the Weather tool (the tile follows the edits; Save writes it into the project)",
        |e, _| {
            with_tile(e, |t, e| {
                let Some(file) = e.weather_preview().map(str::to_owned) else { return fail("the tile has no weather file") };
                t.weather.edit(e, &file);
                ok("")
            })
        },
    );

    c.add(
        "tile_weather_search",
        "tile_weather_search [words]",
        "the words (and-ed) the files of the open tile's weather list must hold; none: every file",
        |e, a| {
            let words = a.join(" ");
            with_tile(e, |t, _| {
                t.weather.set_search(&words);
                ok("")
            })
        },
    );

    c.add(
        "tile_areas",
        "tile_areas outline <0..1> | fill <0..1> | type <type> <0..1> | reset",
        "the Areas chip: the polygons' outlines, fills and a type's fill factor, kept in the settings",
        |e, a| {
            const USAGE: &str = "usage: tile_areas outline <0..1> | fill <0..1> | type <type> <0..1> | reset";
            let value = |i: usize| a.get(i).and_then(|v| v.parse::<f32>().ok()).map(|v| v.clamp(0.0, 1.0));
            with_tile(e, |t, e| {
                let mut look = t.polygon_look(e).clone();
                match (a.first().map(String::as_str), value(1), value(2)) {
                    (Some("outline"), Some(v), _) => look.outline = v,
                    (Some("fill"), Some(v), _) => look.fill = v,
                    (Some("type"), _, Some(v)) => look.set_type_factor(&a[1], v),
                    (Some("reset"), _, _) => look = Default::default(),
                    _ => return fail(USAGE),
                }

                t.areas = Some(look);
                t.save_polygon_look(e);
                ok("")
            })
        },
    );

    c.add(
        "tile_type",
        "tile_type <type> on|off|only | tile_type all",
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

    c.add(
        "tile_sea",
        "tile_sea on|off",
        "show or hide the sea at 0 m (the tile's plane and the map context's sea; kept in the settings)",
        |e, a| {
            let Some(w) = a.first().filter(|w| *w == "on" || *w == "off") else { return fail("usage: tile_sea on|off") };
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            with_tile(e, |t, e| {
                t.set_show_sea(e, id, w == "on");
                ok("")
            })
        },
    );

    c.add(
        "tile_vfx_boxes",
        "tile_vfx_boxes on|off",
        "show or hide the VFX boxes (the VFXInstance markers; the effects still play; kept in the settings)",
        |e, a| {
            let Some(w) = a.first().filter(|w| *w == "on" || *w == "off") else { return fail("usage: tile_vfx_boxes on|off") };
            crate::tool::set_vfx_boxes(e, w == "on");
            ok("")
        },
    );

    c.add(
        "tile_sea_state",
        "tile_sea_state [wind]",
        "draw the sea with a battle wind's waves and whitecaps (a key of wind_levels_tables; kept in the settings); no wind: list them",
        |e, a| {
            let states = e.renderer().map(|r| r.sea_states()).unwrap_or_default();
            let Some(key) = a.first() else { return ok(format!("sea states: {}", states.join(", "))) };
            if !states.contains(key) {
                return fail(format!("no wind '{key}'; winds: {}", states.join(", ")));
            }

            crate::tool::set_sea_state(e, key);
            ok("")
        },
    );

    c.add("tree_list", "tree_list [on | off | auto | <category>]", "the procedural trees, generated as BOB would (again on every call): on / off; a category of the kit's (default, a climate key) instead of the ground's climate; auto: the ground's climate again; no argument: log the categories", |e, a| {
        let Some(d) = document(e) else { return fail("no tile open") };
        let t = d.tile_mut();
        let categories = t.tree_categories();
        match a.first().map(|s| s.to_lowercase()) {
            None => return ok(format!("tree categories: {} (drawn: {})", categories.join(", "), if t.display().tree_list.is_empty() { "none" } else { &t.display().tree_list })),
            Some(v) if v == "on" => t.display_mut().show_trees = true,
            Some(v) if v == "off" || v == "none" => t.display_mut().show_trees = false,
            Some(v) if v == "auto" => t.display_mut().tree_category.clear(),
            Some(v) => {
                if !categories.contains(&v) {
                    return fail(format!("no tree category {v}; the kit has: {}", categories.join(", ")));
                }

                t.display_mut().tree_category = v;
                t.display_mut().show_trees = true;
            }
        }

        t.reload_tree_list();
        ok("")
    });

    c.add(
        "grass",
        "grass on|off|regenerate",
        "draw the tile's grass near the camera, generated as BOB would: on / off; regenerate: again from the tile as it is",
        |e, a| {
            let Some(d) = document(e) else { return fail("no tile open") };
            let t = d.tile_mut();
            match a.first().map(String::as_str) {
                Some("on") => t.display_mut().show_grass = true,
                Some("off") => t.display_mut().show_grass = false,
                Some("regenerate") => t.reload_grass(),
                _ => return fail("usage: grass on|off|regenerate"),
            }

            ok("")
        },
    );

    c.add(
        "assert_water",
        "assert_water <pieces n>",
        "test: the tile has n water meshes (river splines + the sea plane) in the scene (after a frame)",
        |e, a| {
            let (Some(d), Some(n)) = (document(e), a.first().and_then(|x| number(x))) else {
                return fail("usage: assert_water <pieces n>");
            };
            let t = d.tile();
            if t.water_pieces() == n as usize {
                ok(t.water_note().to_owned())
            } else {
                fail(format!("{} water meshes: {}", t.water_pieces(), t.water_note()))
            }
        },
    );

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

    c.add(
        "assert_world_xz",
        "assert_world_xz <x m> <z m> [tolerance]",
        "test: the selected instance is at this ground position (any height)",
        |e, a| {
            let el = primary(e);
            let parsed = (a.len() >= 2).then(|| Some((number(&a[0])?, number(&a[1])?))).flatten();
            let (Some(d), Some((x, z))) = (document(e), parsed) else { return fail("usage: assert_world_xz <x m> <z m> [tolerance]") };
            let tol = a.get(2).and_then(|v| number(v)).unwrap_or(0.01);
            let Some(el) = el else { return fail("nothing selected") };
            let p = d.tile().world_position(el);
            if (p.x - x).abs() <= tol && (p.z - z).abs() <= tol {
                ok("")
            } else {
                fail(format!("at {:.3} {:.3}, expected {x:.3} {z:.3}", p.x, p.z))
            }
        },
    );

    c.add("assert_selected_type", "assert_selected_type <type>", "test: the primary selected element has this type", |e, a| {
        let el = primary(e);
        let (Some(d), Some(want)) = (document(e), a.first()) else { return fail("usage: assert_selected_type <type>") };
        let Some(el) = el else { return fail("nothing selected") };
        let ty = d.tile().tscene().type_of(el);
        if ty == want { ok("") } else { fail(format!("selected {ty}")) }
    });

    c.add("assert_file_exists", "assert_file_exists <path>", "test: a file exists", |_, a| match a.first() {
        Some(p) if Path::new(p).exists() => ok(""),
        other => fail(format!("missing {}", other.map_or("", String::as_str))),
    });

    // ---- reference image and screenshot
    c.add(
        "tile_reference",
        "tile_reference [<file.png>] [ground|screen|off] [opacity] [x,z,width,turn]",
        "the open tile's reference image: load a PNG (laid over the whole tile at first), show it on the ground, on the screen or not, at an opacity, and where it lies on the ground (metres, degrees clockwise)",
        |e, a| {
            with_doc(e, |t, e| {
                let mut words = a.iter().peekable();
                if let Some(file) = words.next_if(|w| w.to_lowercase().ends_with(".png"))
                    && let Err(m) = t.reference.load(e, std::path::Path::new(file))
                {
                    return fail(m);
                }

                let show = match words.next_if(|w| matches!(w.as_str(), "ground" | "screen" | "off")).map(String::as_str) {
                    Some("ground") => Some(crate::settings::ReferenceShow::Ground),
                    Some("screen") => Some(crate::settings::ReferenceShow::Screen),
                    Some(_) => Some(crate::settings::ReferenceShow::Off),
                    None => None,
                };
                let opacity = match words.next_if(|w| !w.contains(',')) {
                    Some(o) => match o.parse::<f32>() {
                        Ok(o) => Some(o),
                        Err(_) => return fail(format!("'{o}' is not an opacity")),
                    },
                    None => None,
                };
                let placement = match words.next() {
                    Some(p) => match parse_floats::<4>(p) {
                        Some(p) => Some(p),
                        None => return fail("the placement is x,z,width,turn"),
                    },
                    None => None,
                };
                t.reference.set(e, show, opacity, placement);
                let s = &t.reference.s;
                ok(format!(
                    "{} {:?} opacity {:.2} centre {:.1},{:.1} width {:.1} m turn {:.2}",
                    s.path, s.show, s.opacity, s.centre[0], s.centre[1], s.width, s.rotation
                ))
            })
        },
    );

    c.add(
        "tile_screenshot",
        "tile_screenshot [file.png]",
        "save the viewport's last image as a PNG: at the file, or named after the tile in the screenshot folder (Settings > Tile)",
        |e, a| {
            if crate::tool::document_id(e).is_none() {
                return no_tile();
            }

            crate::reference_image::save_screenshot(e, a.first().map(std::path::Path::new))
                .map_or_else(fail, |p| ok(p.display().to_string()))
        },
    );
}
