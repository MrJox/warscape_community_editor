//! A generated bridge checked as `wall_tech_check` checks a wall (docs/plans/procedural_roads.md, "Bridges"): each base of
//! `bridge::BASES` bent along a curved, sloped deck, then, in the world: the deck walkable from end to end (no stretch of its centre line
//! off every platform), no platforms over one another, the pieces' platforms meeting at their joints (corners shared within a
//! centimetre), every platform's corners on the deck's surface, each end piece with a platform edge the ground can link to (along its
//! collision outline, as the game links them), and the files reading back.
//!
//!   cargo run --release -p wce_walls --example bridge_check -- <game folder> [radius] [rise] [rpfm schema .ron] [exported .pack]

use std::io::Cursor;
use std::path::PathBuf;

use rpfm_lib::files::{Decodeable, cs2_parsed::Cs2Parsed};
use wce_vfs::{Config, GameProfile, Vfs};
use wce_walls::bridge::{BASES, BridgeWarp, Deck, make_piece, prepare};
use wce_walls::building::{BuildingFiles, is_piece_mesh};

/// The intact collision outlines of a logic file, as their corners (and whether each is a collision outline, with heights, or the
/// footprint ring beside them, without).
fn outlines(bytes: &[u8]) -> Vec<(Vec<[f64; 3]>, bool)> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).expect("a logic file");
    let v = serde_json::to_value(&parsed).expect("json");
    let mut out = vec![];
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let Some(d) = piece["destructs"].as_array().and_then(|d| d.first()) else { continue };
        for o in d["collision_outlines"].as_array().into_iter().flatten() {
            out.push((o["vertices"]["outline"].as_array().into_iter().flatten().map(|q| {
                let f = |k: &str| q[k].as_f64().unwrap_or(0.0);
                [f("x"), f("y"), f("z")]
            }).collect(), true));
        }

        // The footprint ring beside it, as points on the ground (y 0).
        for ring in d["orange_thingies"].as_array().into_iter().flatten() {
            out.push((ring.as_array().into_iter().flatten().map(|q| [q["vertex"]["x"].as_f64().unwrap_or(0.0), 0.0, q["vertex"]["y"].as_f64().unwrap_or(0.0)]).collect(), false));
        }
    }

    out
}

/// The intact platforms of a logic file, as their corners, and whether each is a ground one (`flag_2`).
fn platforms(bytes: &[u8]) -> Vec<(Vec<[f64; 3]>, bool)> {
    let parsed = Cs2Parsed::decode(&mut Cursor::new(bytes), &None).expect("a logic file");
    let v = serde_json::to_value(&parsed).expect("json");
    let mut out = vec![];
    for piece in v["pieces"].as_array().into_iter().flatten() {
        let Some(d) = piece["destructs"].as_array().and_then(|d| d.first()) else { continue };
        for p in d["platforms"].as_array().into_iter().flatten() {
            let pts = p["vertices"]["outline"].as_array().into_iter().flatten().map(|q| {
                let f = |k: &str| q[k].as_f64().unwrap_or(0.0);
                [f("x"), f("y"), f("z")]
            }).collect::<Vec<_>>();
            out.push((pts, p["flag_2"].as_bool() == Some(true)));
        }
    }

    out
}

/// The foot of a logic file's ramps (its platforms sloped over 15 degrees: their edges with both corners at the platform's lowest height),
/// and how many of those edges the ground can link to. As the game links a platform to the ground (docs/re/platforms_and_bridges.md,
/// "How units get on and off"), an edge of the ground's triangulation must run between the platform edge's two corners, within 0.0141
/// (`FUN_10354b40`); along a building, the triangulation's corners are its collision outlines' corners (`FUN_1065fc10`), unless other
/// outlines cross there. So an edge links when its two corners are two consecutive corners of a collision outline, in x and z: the kit's
/// `bridge_wood_2_end` has one foot edge, along its outline's.
fn foot_edges(bytes: &[u8]) -> (usize, usize) {
    let near = |a: &[f64; 3], b: &[f64; 3]| (a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2) <= 2.0e-4;
    let rings = outlines(bytes).into_iter().filter(|(_, collision)| *collision).map(|(r, _)| r).collect::<Vec<_>>();
    let (mut feet, mut along) = (0, 0);
    for (p, _) in platforms(bytes) {
        // A ramp: sloped more than 15 degrees (the kit's end ramps 26.6; a sloped deck far less).
        let (lo, hi) = p.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q[1]), hi.max(q[1])));
        if hi - lo < 0.05 || slope(&p) < 15f64.to_radians() {
            continue;
        }

        for i in 0..p.len() {
            let (a, b) = (&p[i], &p[(i + 1) % p.len()]);
            if (a[1] - lo).abs() > 0.01 || (b[1] - lo).abs() > 0.01 {
                continue;
            }

            feet += 1;
            if rings.iter().any(|r| (0..r.len()).any(|k| {
                let (c, d) = (&r[k], &r[(k + 1) % r.len()]);
                (near(a, c) && near(b, d)) || (near(a, d) && near(b, c))
            })) {
                along += 1;
            }
        }
    }

    (feet, along)
}

/// A platform's slope (its plane's angle to the horizontal): of the planes through its first corner and two others, the best spanned.
fn slope(p: &[[f64; 3]]) -> f64 {
    let mut best = ([0.0, 1.0, 0.0], 0.0);
    for i in 1..p.len() {
        for j in i + 1..p.len() {
            let (u, v) = ([p[i][0] - p[0][0], p[i][1] - p[0][1], p[i][2] - p[0][2]], [p[j][0] - p[0][0], p[j][1] - p[0][1], p[j][2] - p[0][2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > best.1 {
                best = (n, len);
            }
        }
    }

    if best.1 <= 1e-9 { 0.0 } else { (best.0[1].abs() / best.1).acos() }
}

/// Every ramp's foot (see [`foot_edges`]) along its piece's collision outline, edge for edge. Prints each piece with a ramp; false when
/// one's foot is not.
fn ramps_reach_the_ground<'a>(files: impl Iterator<Item = (&'a str, &'a [u8])>) -> bool {
    let mut ok = true;
    for (path, bytes) in files.filter(|(p, _)| p.ends_with(".cs2.parsed")) {
        let (feet, along) = foot_edges(bytes);
        if feet == 0 {
            continue;
        }

        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        // One foot edge, along the outline: the game links one edge per platform and ground region (docs/re/platforms_and_bridges.md), so
        // a foot split in two is crossed by one half only.
        let good = along == feet && feet == 1;
        let why = if along != feet { ": the ground cannot link to the rest, FAIL" } else if feet > 1 { ": split, the game links one of them only, FAIL" } else { "" };
        println!("  {name}: {along} of its ramps' {feet} foot edge(s) along its collision outline{why}");
        ok &= good;
    }

    ok
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut odd = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            odd = !odd;
        }
    }

    odd
}

fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-12)).clamp(0.0, 1.0);
    (p[0] - a[0] - d[0] * t).hypot(p[1] - a[1] - d[1] * t)
}

/// The area two rings share on the ground (the second convex).
fn overlap_area(a: &[[f64; 2]], b: &[[f64; 2]]) -> f64 {
    let signed = |r: &[[f64; 2]]| (0..r.len()).map(|i| { let (p, q) = (r[i], r[(i + 1) % r.len()]); p[0] * q[1] - q[0] * p[1] }).sum::<f64>() / 2.0;
    let turn = signed(b).signum();
    let mut out = a.to_vec();
    for i in 0..b.len() {
        let (c, d) = (b[i], b[(i + 1) % b.len()]);
        let keep = |p: [f64; 2]| turn * ((d[0] - c[0]) * (p[1] - c[1]) - (d[1] - c[1]) * (p[0] - c[0])) >= 0.0;
        let input = std::mem::take(&mut out);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            if keep(p) {
                out.push(p);
            }

            if keep(p) != keep(q) {
                let (dp, dq) = (
                    (d[0] - c[0]) * (p[1] - c[1]) - (d[1] - c[1]) * (p[0] - c[0]),
                    (d[0] - c[0]) * (q[1] - c[1]) - (d[1] - c[1]) * (q[0] - c[0]),
                );
                let t = dp / (dp - dq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }

    if out.len() < 3 { 0.0 } else { signed(&out).abs() }
}

fn main() {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let vfs = Vfs::mount(&Config::new(PathBuf::from(&a[0]), GameProfile::ATTILA)).expect("the game's packs");
    let radius = a.get(1).and_then(|r| r.parse::<f64>().ok()).unwrap_or(60.0);
    let rise = a.get(2).and_then(|r| r.parse::<f64>().ok()).unwrap_or(6.0);
    let mut failed = false;

    // A quarter circle rising `rise` over it, scene units; the ground far under it.
    let path = (0..=90).map(|i| {
        let t = f64::from(i).to_radians();
        [radius * t.sin(), rise * f64::from(i) / 90.0, radius - radius * t.cos()]
    }).collect::<Vec<_>>();
    let deck = Deck::new(path);
    let ground = |_: f64, _: f64| -20.0;
    for b in BASES {
        let Ok(files) = BuildingFiles::read(&vfs, b.key) else {
            println!("{}: not in the packs", b.key);
            continue;
        };

        let base = prepare(&files).expect("the base prepared");
        let width = 12.0;
        let across = width / (base.z.1 - base.z.0).max(1e-6);
        println!("== {} ({}): x {:.2}..{:.2}, z {:.2}..{:.2}, bottom {:.2}, deck {:.2}{}", b.name, b.key, base.x.0, base.x.1, base.z.0, base.z.1, base.bottom, base.deck,
            if base.made_platform { ", its platform made" } else { "" });

        // Its end piece, when the base has one.
        let end = b.end.and_then(|k| BuildingFiles::read(&vfs, k).ok()).map(|f| prepare(&f).expect("the end piece prepared"));
        if let Some(kit) = b.end.and_then(|k| BuildingFiles::read(&vfs, k).ok()) {
            println!("  the kit's end piece, as a reference:");
            ramps_reach_the_ground([(kit.logic_name().as_str(), kit.files.get(&kit.logic_name()).expect("its logic file").as_slice())].into_iter());
        }
        let parts = wce_walls::bridge::parts(&deck, &base, end.as_ref());
        if let (Some(e), true) = (&end, std::env::var("BRIDGE_CHECK_DEBUG").is_ok()) {
            println!("    end piece: x {:.2?} z {:.2?} deck {:.2} bottom {:.2}; its platforms {:.2?}", e.x, e.z, e.deck, e.bottom, platforms(e.files.files.get(&e.files.logic_name()).unwrap()));
        }
        let deck_pieces = parts.iter().filter(|p| !p.end).count();
        let mut all: Vec<(usize, Vec<[f64; 3]>)> = vec![];
        let mut ends: Vec<((f64, f64), [f64; 3])> = vec![];
        let mut grounds: Vec<(usize, Vec<[f64; 3]>)> = vec![];
        let mut footprints: Vec<Vec<(Vec<[f64; 3]>, bool)>> = vec![];
        for (k, part) in parts.iter().enumerate() {
            let (s, origin) = (part.s, part.origin);
            let pb = if part.end { end.as_ref().unwrap_or(&base) } else { &base };
            let warp = BridgeWarp { deck: &part.deck, base: pb, s, across, origin, ground: &ground };
            let key = format!("wce_check_bridge_{}_{k:02}", b.name);
            let made = make_piece(pb, &key, &warp, part.ends(end.is_some())).expect("a piece made");
            for (name, bytes) in &made.files {
                let ok = if is_piece_mesh(name) { wce_walls::mesh::round_trips(bytes) } else if name.ends_with(".cs2.parsed") { wce_walls::logic::round_trips(bytes) } else { Ok(true) };
                if !ok.unwrap_or(false) {
                    println!("  {name}: does not read back");
                    failed = true;
                }
            }

            let logic = made.files.get(&made.logic_name()).expect("a logic file");
            if part.end {
                failed |= !ramps_reach_the_ground([(made.logic_name().as_str(), logic.as_slice())].into_iter());
            }

            for (p, ground) in platforms(logic) {
                let r = p.iter().map(|q| [q[0] + origin[0], q[1] + origin[1], q[2] + origin[2]]).collect::<Vec<_>>();
                if ground {
                    grounds.push((k, r));
                } else {
                    all.push((k, r));
                }
            }

            footprints.push(outlines(logic).into_iter().map(|(o, c)| (o.iter().map(|q| [q[0] + origin[0], q[1] + origin[1], q[2] + origin[2]]).collect(), c)).collect());
            if !part.end {
                ends.push((s, origin));
            }
        }

        println!("  {} piece(s) ({} of them end pieces), {} platform(s)", parts.len(), parts.len() - deck_pieces, all.len());
        if all.is_empty() {
            println!("  FAIL: no platform");
            failed = true;
            continue;
        }

        let xz = |r: &[[f64; 3]]| r.iter().map(|q| [q[0], q[2]]).collect::<Vec<_>>();

        // The deck walkable from end to end: every point of its centre line, a little in from its ends, on a platform near its surface.
        let mut gaps = 0;
        let len = deck.length();
        let mut s = 0.5;
        while s < len - 0.5 {
            let (c, _) = deck.at(s);
            let on = all.iter().any(|(_, r)| {
                let g = xz(r);
                let ys = r.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q[1]), hi.max(q[1])));
                c[1] >= ys.0 - 0.3 && c[1] <= ys.1 + 0.3 && (inside([c[0], c[2]], &g) || (0..g.len()).any(|i| seg_dist([c[0], c[2]], g[i], g[(i + 1) % g.len()]) < 0.05))
            });
            if !on {
                gaps += 1;
            }

            s += 1.0;
        }

        // No platforms over one another.
        let mut stacked = 0;
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                let shared = overlap_area(&xz(&all[i].1), &xz(&all[j].1));
                let mean = |r: &[[f64; 3]]| r.iter().map(|q| q[1]).sum::<f64>() / r.len() as f64;
                if shared >= 0.1 && (mean(&all[i].1) - mean(&all[j].1)).abs() < 2.0 {
                    stacked += 1;
                }
            }
        }

        // At each joint, every platform corner of one piece on it is a corner of the next's (within a centimetre).
        let mut loose = 0;
        for k in 0..ends.len().saturating_sub(1) {
            let joint = deck.at(ends[k].0.1).0;
            let near_joint = |q: &[f64; 3]| (q[0] - joint[0]).hypot(q[2] - joint[2]) < width * 0.75;
            let mine = all.iter().filter(|(p, _)| *p == k).flat_map(|(_, r)| r.iter().copied()).filter(near_joint).collect::<Vec<_>>();
            let next = all.iter().filter(|(p, _)| *p == k + 1).flat_map(|(_, r)| r.iter().copied()).filter(near_joint).collect::<Vec<_>>();
            for q in &mine {
                let on_joint = { let (c, t) = deck.at(ends[k].0.1); ((q[0] - c[0]) * t[0] + (q[2] - c[2]) * t[1]).abs() < 0.05 };
                if on_joint && !next.iter().any(|n| (n[0] - q[0]).abs() < 0.01 && (n[1] - q[1]).abs() < 0.01 && (n[2] - q[2]).abs() < 0.01) {
                    loose += 1;
                }
            }
        }

        // Every platform corner on the deck's surface (the walked-on height onto it).
        let mut off = 0;
        for (_, r) in &all {
            for q in r {
                let (s, _, _) = nearest(&deck, [q[0], q[2]]);
                // Past the deck's ends: its end ramps, down to the ground.
                if s <= 0.1 || s >= deck.length() - 0.1 {
                    continue;
                }

                if (deck.at(s).0[1] - q[1]).abs() > 0.1 {
                    off += 1;
                }
            }
        }

        // Past each end of the deck. With end pieces (the kit's split bridge, laid as by hand): each end piece's ramp shares an edge (two
        // corners or more) with a platform of the deck and drops from it to its foot, and there is no ground platform. Without: a plain
        // ramp then a ground platform (`flag_2`) running into the ground, the ramp's piece's footprint reaching as far.
        let mut bad_ends = 0;
        let same = |a: &[f64; 3], b: &[f64; 3]| (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01 && (a[2] - b[2]).abs() < 0.01;
        let shared = |a: &[[f64; 3]], b: &[[f64; 3]]| a.iter().filter(|q| b.iter().any(|n| same(n, q))).count();
        if end.is_some() {
            if !grounds.is_empty() {
                println!("  {} ground platform(s) with end pieces", grounds.len());
                bad_ends += 1;
            }

            for (k, part) in parts.iter().enumerate().filter(|(_, p)| p.end) {
                let ramps = all.iter().filter(|(p, _)| *p == k).map(|(_, r)| r).collect::<Vec<_>>();
                // Joined as the game links platforms (docs/re/platforms_and_bridges.md): an edge of the ramp with the same two corners as an
                // edge of the deck, run the other way, and only one link per pair: that edge must span the deck's whole end (its corners the
                // ramp's two corners on the deck's side, nothing between).
                let edges = |r: &[[f64; 3]]| (0..r.len()).map(|i| (r[i], r[(i + 1) % r.len()])).collect::<Vec<_>>();
                let joined = ramps.iter().any(|r| all.iter().filter(|(p, _)| !parts[*p].end).any(|(_, d)| {
                    let on_deck = r.iter().filter(|q| d.iter().any(|n| same(n, q))).count();
                    on_deck == 2 && edges(r).iter().any(|(a, b)| edges(d).iter().any(|(c, e)| same(a, e) && same(b, c)))
                }));
                let top = part.deck.at(part.s.1).0[1];
                let drop = ramps.iter().flat_map(|r| r.iter()).map(|q| top - q[1]).fold(f64::MIN, f64::max);
                if !joined || (drop - 1.0).abs() > 0.05 {
                    println!("  end piece {k}: joined to the deck {joined}, its ramp drops {drop:.2} (the kit's 1)");
                    bad_ends += 1;
                }
            }
        } else {
            for at in [0.0, deck.length()] {
                let (c, tt) = deck.at(at);
                let away = if at <= 0.0 { -1.0 } else { 1.0 };
                let past = |q: &[f64; 3]| ((q[0] - c[0]) * tt[0] + (q[2] - c[2]) * tt[1]) * away;
                let near = |r: &[[f64; 3]]| r.iter().any(|q| (q[0] - c[0]).hypot(q[2] - c[2]) < width + 8.0);
                let gs = grounds.iter().filter(|(_, r)| near(r)).collect::<Vec<_>>();
                let ok = gs.len() == 1 && all.iter().find(|(_, r)| shared(&gs[0].1, r) >= 2).is_some_and(|(piece, ramp)| {
                    let buried = gs[0].1.iter().filter(|q| (q[1] + 20.0 + wce_walls::bridge::GROUND_PLATFORM_BURY).abs() < 0.01).count() == 2;
                    let ramp_out = ramp.iter().map(&past).fold(f64::MIN, f64::max);
                    buried && footprints[*piece].iter().all(|(r, _)| (r.iter().map(&past).fold(f64::MIN, f64::max) - ramp_out).abs() < 0.05)
                });
                if !ok {
                    println!("  end at {at:.1}: no ramp and ground platform as made");
                    bad_ends += 1;
                }
            }
        }

        println!("  gaps along the deck: {gaps}; platforms stacked: {stacked}; joint corners unmatched: {loose}; corners off the deck: {off}; ends not as the kit's: {bad_ends}");
        if gaps + stacked + loose + off + bad_ends > 0 {
            failed = true;
        }
    }

    // With rpfm's schema: two bridges in a scene lowered as an export lowers them, in memory (no pack is written).
    if let Some(schema) = a.iter().find(|x| x.ends_with(".ron")) {
        let schema = rpfm_lib::schema::Schema::load(std::path::Path::new(schema), None).expect("the schema");
        let half = deck.path.iter().take(46).copied().collect::<Vec<_>>();
        let spans = [wce_walls::bridge::span_text("wood", 12.0, &deck.path), wce_walls::bridge::span_text("wood", 10.0, &half)];
        let mut layer = String::new();
        for s in &spans {
            for line in String::from_utf8_lossy(s).lines() {
                layer.push_str(&format!("                    {line}\n"));
            }
        }

        let text = format!(
            "<project version=\"17\">\n    <scene>\n        <SCENE_GRAPH flags=\" OIF_VISIBLE|OIF_ACTIVE\" name=\"scene\">\n            <children>\n                <LayerInstance flags=\" OIF_VISIBLE|OIF_ACTIVE\" export_type=\"LayerExportType_Normal\" position=\"vector(0,0,0)\" name=\"WCE procedural roads\" orientation=\"euler(0,0,0)\">\n                    <children>\n{layer}                    </children>\n                </LayerInstance>\n            </children>\n        </SCENE_GRAPH>\n    </scene>\n</project>\n"
        );
        let mut scene = wce_tile::ted_scene::TedScene::parse_any(text.into_bytes()).expect("the scene");
        let lowered = wce_walls::export::lower_walls(&mut scene, &ground, &vfs, &schema, "wce_check", None, &[]).expect("lowered");
        let left = scene.find_all(wce_walls::bridge::BRIDGE_SPAN).len();
        let instances = scene.find_all("BUILDING_INSTANCE").into_iter().map(|e| scene.doc().get(e, "key").unwrap_or_default()).collect::<Vec<_>>();
        let rows = |k: &str| lowered.tables.values().any(|t| {
            let id = t.column_position_by_name("key").unwrap_or(0);
            t.data().iter().any(|r| r[id].data_to_string().eq_ignore_ascii_case(k))
        });
        let missing_files = instances.iter().filter(|k| !lowered.files.iter().any(|(p, _)| p.ends_with(&format!("{k}_tech.cs2.parsed")))).count();
        let missing_rows = instances.iter().filter(|k| !rows(k)).count();
        println!("== lowered: {} span(s) left, {} piece(s): {:?}; without files {missing_files}, without rows {missing_rows}; notes {:?}", left, instances.len(), instances, lowered.notes);
        if left > 0 || instances.is_empty() || missing_files > 0 || missing_rows > 0 {
            failed = true;
        }

        failed |= !ramps_reach_the_ground(lowered.files.iter().map(|(p, b)| (p.as_str(), b.as_slice())));
    }

    // An exported pack (a tile's launch or export pack): its bridge pieces' ramps.
    if let Some(path) = a.iter().find(|x| x.ends_with(".pack")) {
        let games = rpfm_lib::games::supported_games::SupportedGames::default();
        let mut pack = rpfm_lib::files::pack::Pack::read_and_merge(&[path.into()], games.game("attila").expect("attila"), true, false, false).expect("the pack");
        let names = pack.files().keys().filter(|n| n.ends_with(".cs2.parsed")).cloned().collect::<Vec<_>>();
        let files = names.iter().filter_map(|n| pack.files_mut().get_mut(n).and_then(|f| f.encode(&None, false, false, true).ok().flatten()).map(|b| (n.clone(), b))).collect::<Vec<_>>();
        println!("== {path}");
        failed |= !ramps_reach_the_ground(files.iter().map(|(p, b)| (p.as_str(), b.as_slice())));
    }

    if failed {
        println!("FAILED");
        std::process::exit(1);
    }

    println!("ok");
}

/// The nearest place on the deck's centre line to a point (distance along it, distance to it, and the point).
fn nearest(deck: &Deck, p: [f64; 2]) -> (f64, f64, [f64; 2]) {
    let (mut best, mut at) = (f64::MAX, 0.0);
    let len = deck.length();
    let mut s = 0.0;
    while s <= len {
        let c = deck.at(s).0;
        let d = (c[0] - p[0]).hypot(c[2] - p[1]);
        if d < best {
            (best, at) = (d, s);
        }

        s += 0.05;
    }

    (at, best, p)
}
