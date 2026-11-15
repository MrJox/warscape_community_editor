//! Opening, saving and exporting tiles, the Tiles browser, new tiles, sharing, and the map context.

use super::*;

/// A command's `--variations a,b` (a combined project's variations to export, comma or space separated up to the next `--`), and the
/// other arguments.
fn variations_arg(a: &[String]) -> (Vec<String>, Option<Vec<String>>) {
    let Some(at) = a.iter().position(|x| x == "--variations") else { return (a.to_vec(), None) };
    let end = a[at + 1..].iter().position(|x| x.starts_with("--")).map_or(a.len(), |k| at + 1 + k);
    let list = a[at + 1..end].iter().flat_map(|x| x.split(',')).map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect();
    let rest = a[..at].iter().chain(&a[end..]).cloned().collect();
    (rest, Some(list))
}

pub(super) fn register(c: &mut CommandRegistry<Editor>) {
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
        "open_exported",
        "open_exported <exported tile folder | terrain/tiles/... in the packs> [workspace folder] [--fresh]",
        "open an exported tile: decompile it into a project at its own folder in the workspace (the projects), its tile.wce marked \
         wce_extracted_tile, and open that; a project there already (extracted before, or the kit's tile saved) opens instead, unless \
         --fresh: extracted over it (the Tiles panel and the menu ask first)",
        |e, a| {
            let fresh = a.iter().any(|x| x == "--fresh");
            let a = a.iter().filter(|x| *x != "--fresh").collect::<Vec<_>>();
            let Some(target) = a.first() else {
                return fail("usage: open_exported <exported tile folder | terrain/tiles/...> [workspace folder] [--fresh]");
            };
            let workspace = a.get(1).map(PathBuf::from);
            with_tile(e, |t, e| t.open_exported(e, target, workspace.as_deref(), fresh).map_or_else(fail, ok))
        },
    );

    c.add(
        "tile_browse",
        "tile_browse <kit number | packs> [filter words]",
        "show a source in the Tiles panel (a kit, or the game packs' exported tiles) with a filter",
        |e, a| {
            let Some(source) = a.first() else { return fail("usage: tile_browse <kit number | packs> [filter words]") };
            let filter = a[1..].join(" ");
            with_tile(e, |t, e| t.browse(e, source, &filter).map_or_else(fail, ok))
        },
    );

    c.add(
        "tile_launch_game", "tile_launch_game [--variation <v>] [--dry-run] [--ignore-movie-packs] [--async]",
        "Launch in game (docs/plans/launch_in_game.md): export the open tile into data\\wce_launch.pack with a battle on it and start the game \
         straight into that battle, with the editor's mod packs (used_mods.txt untouched; the pack is removed when the game exits). --dry-run: \
         the pack and the battle's XML into <output>/launch, the command line in the log, no game. --async: as the menu, on a thread with the \
         progress window (the command returns at once). --variation: a combined project's variation (else the first shown)",
        |e, a| {
            // A combined project's variation: `--variation <v>` (else the first shown).
            let variation = a.iter().position(|x| x == "--variation").and_then(|i| a.get(i + 1)).cloned();
            let dry = a.iter().any(|x| x == "--dry-run");
            let ignore = a.iter().any(|x| x == "--ignore-movie-packs");
            let menu = a.iter().any(|x| x == "--async");
            with_tile(e, |t, e| {
                t.launch.variation = variation;
                t.launch_in_game(e, dry, ignore, menu).map_or_else(fail, ok)
            })
        },
    );

    c.add(
        "save_exported", "save_exported [folder] [--variations a,b]",
        "File > Export > As exported tile: save the open tile, then write it as an exported tile (stages A, B and C) and the tile database under the folder (default: its last one, else the projects folder); a combined project's variations ticked in the menu, or those of --variations",
        |e, a| {
            let (a, only) = variations_arg(a);
            let out = a.first().map_or_else(|| e.export_dir(crate::tool::EXPORTED_TILE_KEY), PathBuf::from);
            with_tile(e, |t, e| t.save_exported(e, &out, only.as_deref()).map_or_else(fail, ok))
        },
    );

    c.add(
        "save_to_pack", "save_to_pack <mod pack> [--variations a,b]",
        "save the open tile, then write it as an exported tile with the tile database's files into the mod pack (created if missing; the pack's files of the tile's folder replaced); a combined project's variations ticked in the menu, or those of --variations",
        |e, a| {
            let (a, only) = variations_arg(a);
            let Some(pack) = a.first() else { return fail("usage: save_to_pack <mod pack> [--variations a,b]") };
            let pack = PathBuf::from(pack);
            with_tile(e, |t, e| t.save_to_pack(e, &pack, only.as_deref()).map_or_else(fail, ok))
        },
    );

    c.add(
        "map_context", "map_context <map | campaign> [placement number | cell <x> <y> <rotation degrees>] [near cells] [mid cells] [far cells] [ground cells]",
        "show the open tile among its neighbours in a battle map, streamed around the camera (waits for the chunks around it); 'campaign' is the game's campaign battle map",
        |e, a| {
            let Some(name) = a.first() else { return fail("usage: map_context <map | campaign> [placement | x y rotation] [near] [mid] [far] [ground]") };
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            with_tile(e, |t, e| {
                use crate::context_panel::{GAME_MAP, load_map, location_names};
                t.prepare_neighbours(e);
                let Some(raw) = document(e).and_then(|d| t.context.raw_of(d)) else { return fail("the tile is not in a kit") };
                let files = e.files();
                let ui = &mut t.context;
                ui.game_dir = e.game().campaign_battle_map;
                ui.bake_ground = e.game().key != wce_vfs::GameProfile::ROME_2.key;
                ui.refresh_maps(&raw, files.as_deref());
                // `campaign` is the game's own campaign; another map of the packs by its folder (`cha_attila_map`).
                let want = if name == "campaign" { GAME_MAP.to_owned() } else { name.clone() };
                let want = if ui.maps.contains(&want) { want } else { format!("{want}{}", crate::context_panel::GAME_FILES) };
                let Some(k) = ui.maps.iter().position(|m| *m == want) else { return fail(format!("no map {name}; maps: {}", ui.maps.join(", "))) };
                ui.map = k as i32;
                if ui.loaded.as_ref().is_none_or(|l| l.0 != want) {
                    // A map of the packs names the packs' tiles (a mod's among them): read with their database.
                    // A kit map: from the first root that has it, with the tile databases layered as the panel reads them (a project's
                    // alone misses the map's tiles).
                    let packs = ui.packs_tree.clone().filter(|_| crate::context_panel::game_map_folder(&want, ui.game_dir).is_some()).and_then(|t| t.raw().ok());
                    let db = match packs {
                        Some(p) => wce_tile::battle_map::tile_db::TileDatabase::load(&p.join("terrain/tiles/battle/_tile_database")).map(std::sync::Arc::new),
                        None => crate::context_panel::database(ui, &raw),
                    };
                    let db = match db {
                        Ok(db) => db,
                        Err(err) => return fail(err),
                    };

                    let map_raw = ui.map_raw(&raw, &want);
                    match load_map(&want, ui.game_dir, &map_raw, &db, files.as_deref()) {
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
                        return fail("usage: map_context <map> cell <x> <y> <rotation degrees> [near] [mid] [far] [ground]");
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

                for (k, ring) in [&mut ui.near, &mut ui.mid, &mut ui.far, &mut ui.ground].into_iter().enumerate() {
                    if let Some(&r) = nums.get(used + k) {
                        *ring = r.max(1);
                    }
                }

                ui.shown = true;
                ui.adopt_climate(e, id);
                let d = e.docs.find_as::<TileDocument>(id).expect("a tile");
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

    c.add(
        "map_context_async",
        "map_context_async <map | campaign>",
        "test: what the Map context section does when shown with a map (reads and builds on the worker thread)",
        |e, a| {
            let Some(name) = a.first() else { return fail("usage: map_context_async <map | campaign>") };
            with_tile(e, |t, e| {
                t.prepare_neighbours(e);
                let Some(raw) = document(e).and_then(|d| t.context.raw_of(d)) else { return fail("the tile is not in a kit") };
                let files = e.files();
                let ui = &mut t.context;
                ui.game_dir = e.game().campaign_battle_map;
                ui.bake_ground = e.game().key != wce_vfs::GameProfile::ROME_2.key;
                ui.refresh_maps(&raw, files.as_deref());
                let want = if name == "campaign" { crate::context_panel::GAME_MAP.to_owned() } else { name.clone() };
                let want = if ui.maps.contains(&want) { want } else { format!("{want}{}", crate::context_panel::GAME_FILES) };
                let Some(k) = ui.maps.iter().position(|m| *m == want) else {
                    return fail(format!("no map {name}; maps: {}", ui.maps.join(", ")));
                };
                ui.map = k as i32;
                ui.shown = true;
                ui.loaded = None;
                ui.start_map(&raw, files);
                ok("started")
            })
        },
    );

    c.add(
        "map_context_settle",
        "map_context_settle",
        "wait until the map context's chunks around the camera are on the scene; says what it has",
        |e, _| {
            with_tile(e, |t, e| {
                if t.context.view.streamer.is_none() {
                    return fail("no map context");
                }

                t.context.settle(e);
                ok(t.context.status.clone())
            })
        },
    );

    c.add(
        "map_context_view",
        "map_context_view <distance> [minimap on|off]",
        "the camera's view distance while the map context shows (scene units; the game's is 13000), and the minimap",
        |e, a| {
            let Some(d) = a.first().and_then(|v| v.parse::<f32>().ok()) else {
                return fail("usage: map_context_view <distance> [minimap on|off]");
            };
            with_tile(e, |t, e| {
                t.context.view_distance = d.max(crate::context_panel::GAME_FAR_PLANE);
                if let Some(m) = a.get(2) {
                    t.context.minimap = m == "on";
                }

                t.context.stream(e);
                ok(format!("view distance {:.0}", t.context.view_distance))
            })
        },
    );

    c.add(
        "map_context_snow",
        "map_context_snow on|off [amount]",
        "the season's snow where the map has it (the Map context section's switch and amount)",
        |e, a| {
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
        },
    );

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

    c.add(
        "tiles",
        "tiles [filter words]",
        "list the tile database entries of the kits (the tile browser's rows), optionally filtered; says how many",
        |e, a| {
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
        },
    );

    c.add(
        "assert_tiles",
        "assert_tiles <n> [filter words]",
        "test: the tile browser lists n entries for the filter (over all kits)",
        |e, a| {
            let Some(n) = a.first() else { return fail("usage: assert_tiles <n> [filter words]") };
            with_tile(e, |t, e| {
                let words = a[1..].iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
                let shown = t
                    .kit_roots(e)
                    .iter()
                    .map(|root| TileIndex::shared(root).entries().iter().filter(|en| matches_words(en, &words)).count())
                    .sum::<usize>();
                if shown as i64 == atoi(n) { ok("") } else { fail(format!("{shown} tiles, expected {n}")) }
            })
        },
    );

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

    c.add("tile_wait_jobs", "tile_wait_jobs", "wait for what the walls and roads are making off the editor's thread (for scripts; the walls' pieces go on the scene at the next frame)", |e, _| {
        let start = std::time::Instant::now();
        let wait = || {
            while wce_tile_editor::tile_document::jobs_running() && start.elapsed() < std::time::Duration::from_secs(120) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };

        wait();
        if let Some(d) = document(e) {
            d.tile_mut().finish_road_jobs();
        }

        wait();
        ok(format!("{:.2} s", start.elapsed().as_secs_f64()))
    });

    c.add(
        "tile_export_text",
        "tile_export_text <file>",
        "write the scene as an export reads it (the roads' pieces, bridge spans and crossing hints in their layer) to a file",
        |e, a| {
            let Some(path) = a.first().cloned() else { return fail("usage: tile_export_text <file>") };
            let Some(d) = document(e) else { return no_tile() };
            match d
                .text_for_export(false)
                .and_then(|t| wce_foundation::files::write_replacing(&path, t).map_err(|err| format!("{path}: {err}")))
            {
                Ok(()) => ok(format!("written to {path}")),
                Err(why) => fail(why),
            }
        },
    );

    c.add(
        "save_for_kit",
        "save_for_kit [raw_data folder] [--variations a,b]",
        "write the open tile as TEd reads it into the Assembly Kit (or this raw_data folder); a combined project's variations ticked in File > Export, or those of --variations, each in its folder",
        |e, a| {
            let (a, only) = variations_arg(a);
            let kit = a.first().map(std::path::PathBuf::from);
            with_tile(e, |t, e| t.save_for_kit(e, kit, only.as_deref()).map_or_else(fail, ok))
        },
    );

    c.add("new_tile", "new_tile <name> [x cells] [y cells] [height m] [density 32|64|128]", "a new battle tile, flat at the height, as a project (Tile > New tile...; defaults 8 8 10, density 128, or 32 above 16 cells), opened", |e, a| {
        use wce_tile::new_tile;
        let Some(name) = a.first() else { return fail("usage: new_tile <name> [x cells] [y cells] [height m] [density]") };
        let name = match new_tile::check_name(name) {
            Ok(n) => n,
            Err(err) => return fail(&err),
        };

        let number = |i: usize, default: f64| a.get(i).map_or(Some(default), |v| v.parse::<f64>().ok());
        let (Some(x), Some(y), Some(height)) = (number(1, f64::from(new_tile::DEFAULT_CELLS)), number(2, f64::from(new_tile::DEFAULT_CELLS)), number(3, new_tile::DEFAULT_HEIGHT)) else {
            return fail("the size and height are numbers");
        };

        let (cells_x, cells_y) = (x as u32, y as u32);
        let density = match a.get(4) {
            Some(d) => match d.parse::<u32>() {
                Ok(d) => d,
                Err(_) => return fail("the density is 32, 64 or 128"),
            },
            None => new_tile::suggested_density(cells_x, cells_y, new_tile::DEFAULT_DENSITY, false),
        };

        let t = new_tile::NewTile { name, cells_x, cells_y, height, density, seed: new_tile::clock_seed() };
        with_tile(e, |tool, e| tool.create_tile(e, &t).map_or_else(fail, ok))
    });

    c.add("extract", "extract <exported tile folder | terrain/tiles/... in the packs>", "extract a tile as the Tiles panel's packs and File > Open exported tile... do: asked first when a project is there (open it, overwrite, cancel)", |e, a| {
        let Some(target) = a.first().cloned() else { return fail("usage: extract <exported tile folder | terrain/tiles/...>") };
        with_tile(e, |t, e| ok(t.extract_or_ask(e, &target)))
    });

    c.add("migrate_extracted", "migrate_extracted", "move the projects of tiles extracted by older versions (<tile>_extracted) to their own names, marked as extracted tiles (Tile > Migrate old extracted projects)", |e, _| {
        with_tile(e, |t, e| t.migrate_extracted(e).map_or_else(fail, ok))
    });

    c.add(
        "save_tile_as",
        "save_tile_as <tile set> <name> <variation>",
        "TEd's Save Tile As (File > Save tile as...): the open tile as another battle tile in the projects folder",
        |e, a| {
            let (Some(set), Some(name), Some(v)) = (a.first().cloned(), a.get(1).cloned(), a.get(2).cloned()) else {
                return fail("usage: save_tile_as <tile set> <name> <variation>");
            };
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            with_tile(e, |t, e| t.save_tile_as(e, id, &set, &name, &v).map_or_else(fail, ok))
        },
    );

    c.add("resize_tile", "resize_tile <x cells> <z cells> [centre | <x cell> <z cell>] [flat]", "TEd's Resize Tile (Tile > Resize tile...): the old tile centred (default) or at a cell, the new area carried out from the edges (or flat); saved and opened again", |e, a| {
        let (Some(x), Some(z)) = (a.first().and_then(|v| v.parse::<i32>().ok()), a.get(1).and_then(|v| v.parse::<i32>().ok())) else { return fail("usage: resize_tile <x cells> <z cells> [centre | <x cell> <z cell>] [flat]") };
        let Some(id) = crate::tool::document_id(e) else { return no_tile() };
        let flat = a.iter().any(|v| v == "flat");
        let at = (a.len() >= 4).then(|| Some((a[2].parse::<i32>().ok()?, a[3].parse::<i32>().ok()?))).flatten();
        let Some(d) = document(e) else { return no_tile() };
        let Some(old) = d.project().entry.as_ref().map(|en| (en.width(), en.height())) else { return fail("no tile database entry") };
        let ui = crate::resize_tile::ResizeUi {
            old, size: [x, z], centre: [at.is_none(), at.is_none()], offset: at.map_or([0, 0], |(a, b)| [a, b]), extend: !flat, ..Default::default()
        };
        let r = ui.resize();
        with_tile(e, |t, e| t.resize_tile(e, id, &r).map_or_else(fail, ok))
    });

    c.add(
        "tile_set_new",
        "tile_set_new <name> [r g b]",
        "a new tile set in the projects' tile database (Tile > Tile database... > New tile set); the colour picked when not given",
        |e, a| {
            let Some(name) = a.first().cloned() else { return fail("usage: tile_set_new <name> [r g b]") };
            let colour =
                (a.len() >= 4).then(|| Some([a[1].parse::<u8>().ok()?, a[2].parse::<u8>().ok()?, a[3].parse::<u8>().ok()?])).flatten();
            with_tile(e, |t, e| t.new_tile_set(e, &name, colour).map_or_else(fail, ok))
        },
    );

    c.add(
        "tile_database",
        "tile_database [<set> <n tiles>]",
        "open the Tile database window; with a set, test that it lists that many tiles in it",
        |e, a| {
            with_tile(e, |t, e| {
                t.open_tile_database(e);
                let ui = t.tile_db.as_ref().expect("opened");
                match (a.first(), a.get(1).and_then(|v| v.parse::<usize>().ok())) {
                    (Some(set), Some(n)) => {
                        let got = ui.tiles.get(set).map_or(0, Vec::len);
                        if got == n && ui.sets.iter().any(|s| &s.0.name == set) {
                            ok("")
                        } else {
                            fail(format!("set {set}: {got} tile(s), listed {}", ui.sets.iter().any(|s| &s.0.name == set)))
                        }
                    }

                    _ => ok(format!("{} set(s)", ui.sets.len())),
                }
            })
        },
    );

    c.add("new_tile_dialog", "new_tile_dialog", "open the New tile dialog (File > New tile...)", |e, _| {
        with_tile(e, |t, _| {
            t.open_new_tile_dialog();
            ok("the New tile dialog opens")
        })
    });

    c.add(
        "import_from_kit",
        "import_from_kit",
        "open the Assembly Kit's copy of the open project (its next save replaces the project; an extracted tile is extracted again)",
        |e, _| with_tile(e, |t, e| t.import_from_kit(e).map_or_else(fail, ok)),
    );

    c.add(
        "save_to",
        "save_to <folder>",
        "write the tile (tile.wce, the height map and its other files) into another folder (the originals stay)",
        |e, a| {
            let Some(d) = document(e) else { return no_tile() };
            let Some(dir) = a.first() else { return fail("usage: save_to <folder>") };
            match d.save(&SaveTarget { path: PathBuf::from(dir) }) {
                Ok(()) => ok(d.tile().last_message.clone()),
                Err(msg) => fail(msg),
            }
        },
    );

    c.add(
        "tile_diagnostics",
        "tile_diagnostics <file.txt>",
        "write the tile's counts and model fallbacks (what Tile Studio wrote next to a shot)",
        |e, a| {
            let Some(id) = crate::tool::document_id(e) else { return no_tile() };
            let Some(path) = a.first() else { return fail("usage: tile_diagnostics <file.txt>") };
            let text = e.docs.find_as::<TileDocument>(id).expect("a tile").tile().diagnostics(Some(&e.scene));
            match wce_foundation::files::write_replacing(path, text) {
                Ok(()) => ok(""),
                Err(_) => fail(format!("cannot write {path}")),
            }
        },
    );

    c.add(
        "share_project",
        "share_project <zip>",
        "write the open tile's project (its variations, entry and the projects' prefabs it uses) into a zip to share",
        |e, a| {
            let Some(out) = a.first().map(std::path::PathBuf::from) else { return fail("usage: share_project <zip>") };
            with_tile(e, |t, e| {
                t.share_project(e, Some(&out)).map_or_else(
                    |why| {
                        t.share_failed(e, "Share project", &why);
                        fail(why)
                    },
                    ok,
                )
            })
        },
    );

    c.add(
        "import_shared",
        "import_shared <zip> [overwrite]",
        "import a shared project into the projects folder and open its tile; without overwrite, one that replaces files asks first",
        |e, a| {
            let Some(zip) = a.first().map(std::path::PathBuf::from) else { return fail("usage: import_shared <zip> [overwrite]") };
            let overwrite = a.get(1).is_some_and(|w| w == "overwrite");
            with_tile(e, |t, e| {
                t.import_shared_from(e, &zip, overwrite).map_or_else(
                    |why| {
                        t.share_failed(e, "Import shared project", &why);
                        fail(why)
                    },
                    ok,
                )
            })
        },
    );

    c.add(
        "thumbnail",
        "thumbnail <building | type | prefab> <key> [copy to.png]",
        "ask for the Create panel thumbnail of an entry; ok with its file once it is made (wait frames and call again), copied when asked",
        |e, a| {
            let (Some(what), Some(key)) = (a.first().cloned(), a.get(1).cloned()) else {
                return fail("usage: thumbnail <building | type | prefab> <key> [copy to.png]");
            };
            let copy = a.get(2).cloned();
            with_tile(e, |t, e| match crate::create_panel::thumbnail_for_script(t, e, &what, &key) {
                Ok(Some(file)) => match copy.map(|to| std::fs::copy(&file, to)) {
                    Some(Err(err)) => fail(err.to_string()),
                    _ => ok(file.display().to_string()),
                },

                Ok(None) => fail("not made yet"),
                Err(err) => fail(err),
            })
        },
    );

    c.add("tile_ref", "tile_ref human|unit|ship <x m> <z m> [unit] [faction] [soldier] [slot] [rowing] | status", "places a reference: a soldier (ref_human), a whole unit of the game's unit size (ref_unit) or a naval unit's ship with its men aboard (ref_ship, on the water; `rowing`: its oars row); saved in tile.wce, never exported. status: what is loaded and drawn", |e, a| {
        let usage = "usage: tile_ref human|unit|ship <x m> <z m> [unit] [faction] [soldier] [slot] [rowing] | status";
        if a.first().is_some_and(|c| c == "status") {
            return with_tile(e, |t, _| ok(t.ref_units.summary()));
        }

        let kind = match a.first().map(String::as_str) {
            Some("human") => wce_tile_editor::tile_document::REF_HUMAN,
            Some("unit") => wce_tile_editor::tile_document::REF_UNIT,
            Some("ship") => wce_tile_editor::tile_document::REF_SHIP,
            _ => return fail(usage),
        };

        let (Some(x), Some(z)) = (a.get(1).and_then(|v| number(v)), a.get(2).and_then(|v| number(v))) else { return fail(usage) };
        let rowing = a.iter().skip(3).any(|v| v == "rowing");
        let mut attrs: std::collections::BTreeMap<String, String> = ["unit", "faction", "seed", "slot"].iter().zip(a.iter().skip(3).filter(|v| *v != "rowing")).map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        if rowing {
            attrs.insert("rowing".to_owned(), "1".to_owned());
        }

        with_tile(e, |t, e| {
            let Some(d) = document(e) else { return no_tile() };
            let Some(into) = t.new_items_layer(d) else { return fail("this tile has no scene graph") };
            let at = Float3::new(x, d.tile().terrain_height(x, z), z);
            d.create_with(kind, into, &wce_tile_editor::tile_document::Create::Point(at), "", &attrs).map_or_else(fail, |_| ok(format!("added {kind}")))
        })
    });

    c.add(
        "tile_favourite",
        "tile_favourite <entry key>",
        "star a tile of the Tiles panel's source (its location: terrain/tiles/...), or take its star away",
        |e, a| {
            let Some(key) = a.first().map(|k| wce_formats::tile_entry::normalize_location(k)) else {
                return fail("usage: tile_favourite <entry key>");
            };
            with_tile(e, |t, e| {
                let source = t.browse_source();
                ok(if t.toggle_favourite(e, &source, &key) { format!("{key}: favourite") } else { format!("{key}: not a favourite") })
            })
        },
    );
}
