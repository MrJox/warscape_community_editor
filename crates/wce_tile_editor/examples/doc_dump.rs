//! Differential dump of the tile document, in the format of the C++ harness: tiles opened through a kit, published into a render scene through
//! the document manager, random edit sessions with the scene, the saved text, the history and the view state hashed after every step, picks,
//! ground looks, and saves.
//!
//!   cargo run --release -p wce_tile_editor --example doc_dump -- <fixtures folder> <game files folder> <temp folder>

#![allow(clippy::manual_is_multiple_of, reason = "the random draws mirror the C++ harness's expressions")]

use std::path::PathBuf;
use std::sync::Arc;

use wce_editor_core::document::{Document, OutlinerNode, SaveTarget};
use wce_editor_core::document_manager::DocumentManager;
use wce_foundation::math::{Double3, Float3, Float4x4, normalize, rotation_y, translation};
use wce_scene::render_scene::{Overlay, Payload, RenderScene, TerrainImage};
use wce_tile::tile_project::TileProject;
use wce_tile_editor::terrain_bake::{TerrainLook, TerrainView};
use wce_tile_editor::tile_document::{BrushKind, BrushSettings, PieceState, PieceStatus, Placement, TileDocument};
use wce_vfs::files::{Files, FolderFiles};

const START: u64 = 14695981039346656037;

fn fnv(d: &[u8], h: u64) -> u64 {
    d.iter().fold(h, |h, &c| (h ^ u64::from(c)).wrapping_mul(1099511628211))
}

fn fs_(s: &str, h: u64) -> u64 {
    fnv(s.as_bytes(), h)
}

fn f32s(f: &[f32], h: u64) -> u64 {
    f.iter().fold(h, |h, v| fnv(&v.to_le_bytes(), h))
}

fn f64s(f: &[f64], h: u64) -> u64 {
    f.iter().fold(h, |h, v| fnv(&v.to_le_bytes(), h))
}

fn u8_(v: u64, h: u64) -> u64 {
    fnv(&v.to_le_bytes(), h)
}

fn img(i: &TerrainImage, h: u64) -> u64 {
    let h = u8_(u64::from(i.h), u8_(u64::from(i.w), h));
    match &i.rgba {
        Some(px) => fnv(px, h),
        None => u8_(0, h),
    }
}

fn overlay(o: &Overlay, mut h: u64) -> u64 {
    for v in [&o.lines, &o.triangles] {
        h = u8_(v.len() as u64, h);
        for x in v {
            h = f32s(&[x.p.x, x.p.y, x.p.z], h);
            h = u8_(u64::from(x.rgba), h);
        }
    }

    u8_(u64::from(o.depth_test) * 2 + u64::from(o.pickable), h)
}

fn scene_hash(s: &RenderScene) -> u64 {
    let mut h = START;
    for e in s.entities() {
        h = u8_(u64::from(e.id), h);
        h = u8_(u64::from(e.layer), h);
        h = u8_(e.source.item, h);
        h = u8_(u64::from(e.visible) * 2 + u64::from(e.locked), h);
        for r in 0..4 {
            h = f32s(&e.world.m[r], h);
        }

        let index = match &e.payload {
            Payload::MeshInstance(_) => 0,
            Payload::Terrain(_) => 1,
            Payload::VfxEmitter(_) => 2,
            Payload::Overlay(_) => 3,
            Payload::CameraRig(_) => 4,
            Payload::Water(_) => 5,
            Payload::Decal(_) => 6,
            Payload::Grass(_) => 7,
            Payload::PointLight(_) => 8,
        };

        h = u8_(index, h);
        match &e.payload {
            Payload::MeshInstance(m) => {
                h = fs_(&m.model, h);
                h = u8_(u64::from(m.lod) * 2 + u64::from(m.casts_shadow), h);
            }

            Payload::Terrain(t) => {
                h = u8_(t.nx as i64 as u64, h);
                h = u8_(t.nz as i64 as u64, h);
                h = f32s(&[t.origin_x], h);
                h = f32s(&[t.origin_z], h);
                h = f32s(&[t.step], h);
                h = f32s(&t.heights, h);
                h = fs_(&t.diffuse, h);
                h = u8_(u64::from(t.lod), h);
                h = u8_(t.version, h);
                if let Some(skin) = &t.skin {
                    h = f64s(&[skin.x0, skin.z0, skin.w_m, skin.h_m], h);
                    h = u8_(u64::from(skin.w), h);
                    h = u8_(u64::from(skin.h), h);
                    h = fnv(&skin.rgba, h);
                    h = u8_(skin.id, h);
                }

                if let Some(m) = &t.material {
                    h = f64s(&[m.x0, m.z0, m.x1, m.z1, m.y0, m.y1], h);
                    h = img(&m.blend, h);
                    h = img(&m.index, h);
                    h = img(&m.normal, h);
                    h = u8_(m.normal_id, h);
                    h = u8_(m.id, h);
                    for k in 0..16 {
                        h = fs_(&m.diffuse[k], h);
                        h = fs_(&m.normal_tex[k], h);
                    }

                    h = f32s(&m.pixel_scale, h);
                    h = f32s(&m.outfield_scale, h);
                    h = f32s(&m.mid_strength, h);
                    h = f32s(&m.smoothness, h);
                    h = f32s(&m.reflectivity, h);
                    h = fs_(&m.detail_colour, h);
                    h = fs_(&m.detail_normal, h);
                }
            }

            Payload::Overlay(o) => h = overlay(o, h),
            Payload::Decal(d) => {
                h = fs_(&d.model, h);
                h = u8_(u64::from(d.tiled), h);
                h = f32s(&[d.opacity], h);
            }

            Payload::PointLight(l) => {
                h = f32s(&l.colour, h);
                h = f32s(&[l.colour_scale, l.colour_min, l.radius], h);
                h = u8_(u64::from(l.smooth) + 2 * u64::from(l.animation), h);
                h = f32s(&[l.speed[0], l.speed[1], l.phase], h);
            }

            Payload::Grass(g) => {
                h = fs_(&g.model, h);
                h = f32s(&g.instances.iter().flatten().copied().collect::<Vec<_>>(), h);
            }

            Payload::Water(w) => {
                let floats = w.verts.iter().flat_map(|v| [v.x, v.y, v.z, v.across, v.u, v.v, v.flow, v.fade]).collect::<Vec<_>>();
                h = f32s(&floats, h);
                h = fnv(&w.indices.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<_>>(), h);
                h = u8_(u64::from(w.river), h);
                h = f32s(&[w.foam], h);
                h = u8_(w.version, h);
            }

            _ => {}
        }
    }

    h
}

fn outline_hash(n: &OutlinerNode, h: u64) -> u64 {
    let mut h = u8_(u64::from(n.selectable), u8_(n.source.item, fs_(&n.label, h)));
    for c in &n.children {
        h = outline_hash(c, h);
    }

    h
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() % 100000) as f32 / 100000.0
    }
}

fn doc_of(docs: &mut DocumentManager, id: u32) -> &mut TileDocument {
    docs.find_as_mut::<TileDocument>(id).expect("the tile document")
}

fn opt(v: Option<usize>) -> i64 {
    v.map_or(-1, |v| v as i64)
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let (fx, tmp) = (PathBuf::from(&args[0]), PathBuf::from(&args[2]));
    let files: Arc<dyn Files> = Arc::new(FolderFiles(PathBuf::from(&args[1])));
    let kit = fx.join("kit/raw_data/terrain/tiles/battle");
    let flags = ["OIF_VISIBLE", "OIF_ACTIVE"];
    let ptypes = ["AI_HINT_POLYGON_INSTANCE", "NOGO_REGION_POLYGON_INSTANCE"];
    let pops = ["move", "insert", "remove", "bogus"];
    let modes = [Placement::None, Placement::Landscape, Placement::Freeform];
    let kinds = [BrushKind::Raise, BrushKind::Lower, BrushKind::Smooth, BrushKind::Flatten, BrushKind::Level];
    let mut r = Rng(88172645463325252);
    for (tile_n, dir) in [kit.join("plains/town/aa"), kit.join("sea_coast/coast/bb"), kit.join("plains/loose/cc")].iter().enumerate() {
        let mut scene = RenderScene::default();
        let mut docs = DocumentManager::default();
        let doc = TileDocument::new(TileProject::open(dir).unwrap(), Some(files.clone()));
        let t = doc.tile();
        println!("tile {} title [{}] instances {} fallbacks {}", dir.strip_prefix(&fx).unwrap().to_string_lossy(), doc.title(), t.instances().len(), t.model_fallbacks().len());
        for (ty, n) in t.type_counts() {
            println!("  type {ty} {n}");
        }

        for k in t.model_fallbacks() {
            println!("  fallback {k}");
        }

        let id = docs.add(&mut scene, Box::new(doc), "tile");
        let d = doc_of(&mut docs, id);

        // What the model cache would say: every third piece fails, the rest load with a size from their path.
        let peek = |p: &str| {
            let h = fs_(p, START);
            if h % 3 == 0 {
                PieceState { status: PieceStatus::Failed, ..Default::default() }
            } else {
                let e = 2.0 + (h % 40) as f32;
                PieceState { status: PieceStatus::Ready, bmin: Float3::new(-e, 0.0, -e * 0.5), bmax: Float3::new(e, e * 0.7, e * 0.5) }
            }
        };

        let changed = d.tile_mut().poll_models(&peek);
        println!("  poll {} fallbacks {}", u8::from(changed), d.tile().model_fallbacks().len());
        docs.update(&mut scene);
        let d = doc_of(&mut docs, id);
        d.tile_mut().update_view(&mut scene, Float3::new(100.0, 50.0, 100.0));
        let t = d.tile();
        let g = t.ground_info();
        println!(
            "  start scene {:016x} outline {:016x} terrain [{}] water [{}] ground {} {} {} links {}", scene_hash(&scene), outline_hash(&d.outline(), START), t.terrain_note(), t.water_note(),
            u8::from(g.game_shaders), g.layers, g.climate, t.link_cells().len()
        );

        for c in t.link_cells() {
            println!("  link {} {} {} [{}]", c.x, c.y, c.set, c.text);
        }

        let mut selected: Vec<usize> = vec![];
        for k in 0..160u32 {
            let d = doc_of(&mut docs, id);
            let op = r.next() % 24;
            let ins = d.tile().instances().to_vec();
            let n_el = d.tile().tscene().doc().elements().len();
            let lays = (0..n_el).filter(|&e| d.tile().tscene().is_layer(e)).collect::<Vec<_>>();
            let ni = ins.len() as u32;
            let pick_ins = |r: &mut Rng| if ni != 0 { Some(ins[(r.next() % ni) as usize]) } else { None };
            let pick_lay = |r: &mut Rng| if lays.is_empty() { None } else { Some(lays[r.next() as usize % lays.len()]) };
            let ok = |v: bool| if v { "1".to_owned() } else { "x".to_owned() };
            let num = |v: Option<usize>| v.map_or("x".to_owned(), |v| v.to_string());
            let res = match op {
                0 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let (a, bb, c) = (r.f(-30.0, 30.0), r.f(-3.0, 3.0), r.f(-30.0, 30.0));
                    let g = u64::from(r.next() % 3);
                    ok(d.set_local(e, Some(Double3::new(f64::from(a), f64::from(bb), f64::from(c))), None, g))
                }

                1 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let a = r.f(-3.0, 3.0);
                    let g = u64::from(r.next() % 3);
                    ok(d.set_local(e, None, Some(Double3::new(f64::from(a), 0.0, 0.1)), g))
                }

                2 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let (x, y, z) = (r.f(-50.0, 300.0), r.f(-5.0, 40.0), r.f(-50.0, 300.0));
                    let g = u64::from(r.next() % 3);
                    ok(d.set_world_metres(e, Float3::new(x, y, z), g))
                }

                3 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let a = r.f(-180.0, 180.0);
                    ok(d.rotate(e, a))
                }

                4 if ni != 0 => {
                    let e = if r.next() % 4 == 0 { pick_lay(&mut r) } else { pick_ins(&mut r) };
                    num(e.and_then(|e| d.duplicate(e).ok()))
                }

                5 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    ok(d.remove(e))
                }

                6 => {
                    let use_t = r.next() % 2 != 0;
                    let tmpl = if use_t { pick_ins(&mut r) } else { None };
                    let (x, z) = (r.f(0.0, 250.0), r.f(0.0, 250.0));
                    let mode = modes[(r.next() % 3) as usize];
                    let at = r.next() % 2 != 0;
                    num(d.add_building("nordic_court_wall", tmpl, at.then_some(Float3::new(x, 5.0, z)), mode).ok())
                }

                7 => {
                    let l = if r.next() % 3 == 0 { None } else { pick_lay(&mut r) };
                    let t = ptypes[(r.next() % 2) as usize];
                    let n = r.next() % 5;
                    let mut pts = vec![];
                    for _ in 0..n {
                        let (x, z) = (r.f(0.0, 200.0), r.f(0.0, 200.0));
                        pts.push(Float3::new(x, 3.0, z));
                    }

                    let hint = if r.next() % 2 != 0 { "AIH_FORT_PERIMETER" } else { "" };
                    num(d.add_polygon(l, t, hint, &pts).ok())
                }

                8 => {
                    let polys = d.tile().tscene().find_all("AI_HINT_POLYGON_INSTANCE");
                    if polys.is_empty() {
                        String::new()
                    } else {
                        let p = polys[r.next() as usize % polys.len()];
                        let o = pops[(r.next() % 4) as usize];
                        let i = (r.next() % 7) as usize;
                        let (x, z) = (r.f(0.0, 200.0), r.f(0.0, 200.0));
                        let g = u64::from(r.next() % 3);
                        ok(d.polygon_point(p, o, i, Float3::new(x, 2.0, z), g).is_ok())
                    }
                }

                9 => {
                    let parent = if r.next() % 3 == 0 { d.tile().tscene().scene_graph() } else { pick_lay(&mut r) };
                    let name = if r.next() % 2 != 0 { "" } else { "Layer & <x>" };
                    num(d.add_layer(parent, name).ok())
                }

                10 if ni != 0 => {
                    let e = if r.next() % 4 == 0 { pick_lay(&mut r) } else { pick_ins(&mut r) };
                    let l = pick_lay(&mut r);
                    ok(matches!((e, l), (Some(e), Some(l)) if d.move_to_layer(e, l) == Ok(true)))
                }

                11 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let name = if r.next() % 2 != 0 { "renamed" } else { "" };
                    ok(d.rename(e, name))
                }

                12 if ni != 0 => {
                    let e = pick_ins(&mut r).unwrap();
                    let an = if r.next() % 2 != 0 { "opacity" } else { "new_attr" };
                    let av = if r.next() % 2 != 0 { "0.5" } else { "1" };
                    ok(d.set_attribute(e, an, av))
                }

                13 => {
                    let l = pick_lay(&mut r);
                    ok(l.is_some_and(|l| d.remove_layer(l)))
                }

                14 => {
                    let n = 1 + r.next() % 3;
                    let mut es = vec![];
                    for _ in 0..n {
                        es.push(if r.next() % 2 != 0 { pick_lay(&mut r) } else { pick_ins(&mut r) });
                    }

                    let es = es.into_iter().flatten().collect::<Vec<_>>();
                    let f = flags[(r.next() % 2) as usize];
                    let on = r.next() % 2 != 0;
                    ok(d.set_flags(&es, f, on))
                }

                15 if ni != 0 => {
                    let n = 1 + r.next() % 3;
                    let es = (0..n).map(|_| pick_ins(&mut r).unwrap()).collect::<Vec<_>>();
                    let l = pick_lay(&mut r);
                    let mut before = None;
                    if let Some(l) = l
                        && r.next() % 2 != 0
                    {
                        let kids = d.tile().tscene().children(l);
                        if !kids.is_empty() {
                            before = Some(kids[r.next() as usize % kids.len()]);
                        }
                    }

                    ok(l.is_some_and(|l| d.move_items(&es, l, before).is_ok()))
                }

                16 if ni != 0 => {
                    let n = 1 + r.next() % 3;
                    let es = (0..n).map(|_| pick_ins(&mut r).unwrap()).collect::<Vec<_>>();
                    num(d.group_items(&es, "Group").ok())
                }

                17 if ni != 0 => {
                    let n = 1 + r.next() % 3;
                    let mut es = vec![];
                    for _ in 0..n {
                        es.push(if r.next() % 5 == 0 { pick_lay(&mut r) } else { pick_ins(&mut r) });
                    }

                    ok(d.remove_items(&es.into_iter().flatten().collect::<Vec<_>>()))
                }

                18 => {
                    let l = pick_lay(&mut r);
                    num(l.and_then(|l| d.duplicate_layer(l).ok()))
                }

                19 => {
                    let n = 1 + r.next() % 3;
                    let mut es = vec![];
                    for _ in 0..n {
                        es.push(if r.next() % 4 == 0 { pick_lay(&mut r) } else { pick_ins(&mut r) });
                    }

                    let es = es.into_iter().flatten().collect::<Vec<_>>();
                    let which = r.next() % 3;
                    if which == 2 {
                        let free = r.next() % 2 != 0;
                        num(Some(d.snap_to_terrain(&es, free)))
                    } else {
                        num(Some(d.set_placement(&es, if which == 0 { Placement::Landscape } else { Placement::Freeform })))
                    }
                }

                20 => {
                    let kind = kinds[(r.next() % 5) as usize];
                    let s = BrushSettings { radius_m: r.f(3.0, 30.0), strength: r.f(1.0, 20.0), falloff: r.f(0.0, 1.0), flatten_height: r.f(-5.0, 20.0) };
                    let steps = 1 + r.next() % 4;
                    for _ in 0..steps {
                        let (x, z, dt) = (r.f(0.0, 250.0), r.f(0.0, 250.0), r.f(0.005, 0.08));
                        d.brush_step(kind, x, z, s, dt);
                    }

                    if r.next() % 3 != 0 {
                        d.end_stroke();
                    }

                    String::new()
                }

                21 => {
                    let n = 1 + r.next() % 4;
                    for _ in 0..n {
                        d.history_mut().undo();
                    }

                    String::new()
                }

                22 => {
                    let n = 1 + r.next() % 4;
                    for _ in 0..n {
                        d.history_mut().redo();
                    }

                    String::new()
                }

                23 if ni != 0 => {
                    let n = 1 + r.next() % 2;
                    let rs = (0..n).map(|_| d.ref_of(pick_ins(&mut r).unwrap())).collect::<Vec<_>>();
                    let (x, y, z, a) = (r.f(-10.0, 10.0), r.f(-3.0, 3.0), r.f(-10.0, 10.0), r.f(-1.0, 1.0));
                    let turn = r.next() % 2 != 0;
                    let g = u64::from(r.next() % 3);
                    let delta = translation(Float3::new(x, y, z)) * if turn { rotation_y(a) } else { Float4x4::IDENTITY };
                    d.transform_items(&rs, &delta, g);
                    "t".to_owned()
                }

                _ => String::new(),
            };

            docs.update(&mut scene);
            let d = doc_of(&mut docs, id);
            if let Some(hint) = d.take_selection_hint() {
                selected = hint.iter().filter(|&&it| it != 0).map(|&it| (it - 1) as usize).collect();
            }

            let (ex, ey, ez) = (r.f(-100.0, 400.0), r.f(20.0, 300.0), r.f(-100.0, 400.0));
            d.tile_mut().update_view(&mut scene, Float3::new(ex, ey, ez));
            let mut tool = Overlay::default();
            if k % 5 == 0 {
                tool.line(Float3::new(0.0, 0.0, 0.0), Float3::new(10.0, 10.0, 10.0), 0xff00_ff00);
            }

            let n_el = d.tile().tscene().doc().elements().len();
            let sel = selected.iter().copied().filter(|&e| e < n_el).collect::<Vec<_>>();
            let point = usize::try_from(k as i64 % 3 - 1).ok();
            d.tile_mut().set_highlight(&mut scene, &sel, point, 1.5, &tool);
            let bytes = d.tile().tscene().save();
            println!(
                "  {k} op {op} {res} ted {:016x} hist {}/{} mod {} struct {} scene {:016x} outline {:016x} water [{}]", fnv(&bytes, START), d.history().size(), d.history().position(),
                u8::from(d.modified()), d.tile().structure_version(), scene_hash(&scene), outline_hash(&d.outline(), START), d.tile().water_note()
            );

            if d.history().position() != 0 {
                println!("    name {}", d.history().name(d.history().position() - 1));
            }
        }

        let d = doc_of(&mut docs, id);
        d.end_stroke();
        let mut ph = START;
        for k in 0..300 {
            let (ox, oz, tx, tz) = (r.f(-50.0, 300.0), r.f(-50.0, 300.0), r.f(0.0, 260.0), r.f(0.0, 260.0));
            let o = Float3::new(ox, 150.0, oz);
            let dd = normalize(Float3::new(tx, 0.0, tz) - o);
            let e = d.tile().pick_instance(o, dd, k % 2 == 0);
            let hit = d.tile().pick_terrain(o, dd);
            ph = u8_(opt(e) as u64, ph);
            ph = u8_(u64::from(hit.is_some()), ph);
            if let Some(hit) = hit {
                ph = f32s(&[hit.x, hit.y, hit.z], ph);
            }
        }

        println!("  picks {ph:016x}");
        let t = d.tile();
        let pc = t.placement_counts();
        println!("  placement {} {} {} off {}", pc.landscape, pc.freeform, pc.other, t.off_terrain(0.01, None).len());
        for o in t.off_terrain(0.5, None) {
            println!("    off {} {:08x}", o.elem, o.dy.to_bits());
        }

        for v in 0..5 {
            let views = [TerrainView::Colour, TerrainView::BlendGroups, TerrainView::GroundTypes, TerrainView::Protection];
            let look = TerrainLook {
                blended: v != 4, view: views[v % 4], season: if v == 2 { "wi" } else { "su" }.to_owned(), climate: if v == 3 { "ROME_Med" } else { "ROME_Continental" }.to_owned(),
            };

            d.tile_mut().set_terrain_look(&mut scene, look);
            d.tile_mut().update_view(&mut scene, Float3::new(100.0, 50.0, 100.0));
            let baked = d.tile().baked_colour().map_or("-".to_owned(), |b| fnv(&b.rgba, START).to_string());
            println!("  look {v} [{}] scene {:016x} baked {baked}", d.tile().terrain_note(), scene_hash(&scene));
        }

        let t = d.tile_mut();
        t.display_mut().hidden_types.insert("BUILDING_INSTANCE".to_owned());
        t.display_mut().show_links = true;
        t.display_mut().show_polygons = false;
        t.display_changed();
        t.update_view(&mut scene, Float3::new(100.0, 50.0, 100.0));
        println!("  display scene {:016x} links {}", scene_hash(&scene), u8::from(t.links_drawn()));
        let out = tmp.join(format!("t{tile_n}"));
        let _ = std::fs::remove_dir_all(&out);
        match d.save(&SaveTarget { path: out.join("tile.ted") }) {
            Ok(()) => println!("  save 1 ok"),
            Err(msg) => println!("  save 0 {msg}"),
        }

        for f in ["tile.ted", "height_map_0.png"] {
            if let Ok(b) = std::fs::read(out.join(f)) {
                println!("  {f} {:016x}", fnv(&b, START));
            }
        }

        println!("  modified {}", u8::from(d.modified()));
        while d.history().can_undo() {
            d.history_mut().undo();
        }

        docs.update(&mut scene);
        let d = doc_of(&mut docs, id);
        d.tile_mut().update_view(&mut scene, Float3::new(100.0, 50.0, 100.0));
        println!("  undone ted {:016x} scene {:016x}", fnv(&d.tile().tscene().save(), START), scene_hash(&scene));
        while d.history().can_redo() {
            d.history_mut().redo();
        }

        docs.update(&mut scene);
        let d = doc_of(&mut docs, id);
        d.tile_mut().update_view(&mut scene, Float3::new(100.0, 50.0, 100.0));
        println!("  redone ted {:016x} scene {:016x}", fnv(&d.tile().tscene().save(), START), scene_hash(&scene));
    }
}
