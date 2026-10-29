//! A destroyed building made from its intact one: per piece, every level's model (all LODs), every collapse's model, animation and
//! effects, the logic file, under the new key. Deterministic: the same building, parameters and seed give the same bytes.
//!
//! Per piece: each LOD's triangles are split by the cells and capped (one cell set for every LOD, so they agree); the fragments of a
//! damage level are simulated falling among what still stands and the earlier levels' debris; where they land is baked into the level
//! (and every later one), with the rubble heap. The collapse into level L (`_destruct{L}_anim`, from level L - 1) has the standing part and
//! the earlier debris as bone 0 and a bone per fragment, as the game's.

use std::collections::BTreeMap;

use rayon::prelude::*;

use rpfm_lib::files::rigidmodel::{MeshBlock, RigidModel};
use rpfm_lib::files::rigidmodel::vertices::Vertex;
use rpfm_lib::files::Encodeable;
use wce_formats::anim::AnimFile;
use wce_formats::anim_meta::{MetaBody, MetaEntry, MetaFile};

use crate::assemble::{Gather, Slot, inner, lod_like, moved, polygon_tris, surface_tri};
use crate::caps::{Cap, Caps, caps};
use crate::cells::Cells;
use crate::fracture::{Split, Tri, is_cut, lod_triangles, outside, pos, split};
use crate::geom::{Aabb, Rng, V3, v3};
use crate::materials::{DAMAGE_SPAN, RUBBLE_SPAN, classify, culture, damage_sets, has_set, override_of, set_block};
use crate::params::{Interior, MaterialClass, Params, is_fort, solid_by_kind};
use crate::rubble::Heap;
use crate::sim::{Body, Fixed, Impact, simulate};
use crate::solid::Solid;
use crate::source::{Source, Templates, sound_entry};
use crate::tech::{LevelLogic, PieceLogic, intact_collisions, make_logic, weld};

/// What a collapse came out as.
#[derive(Clone, Debug, Default)]
pub struct Collapse {
    pub piece: u32,

    /// The damage level it falls into (1: the first).
    pub level: usize,
    pub fragments: usize,
    pub duration: f32,

    /// Its `.meta` entries (dust, sounds), as generated or edited.
    pub effects: Vec<MetaEntry>,
}

/// The new building's folder and what was made.
#[derive(Clone, Debug, Default)]
pub struct Generated {
    pub key: String,
    pub files: BTreeMap<String, Vec<u8>>,

    /// Levels in all (the intact one and the damage levels).
    pub levels: usize,
    pub collapses: Vec<Collapse>,
    pub notes: Vec<String>,

    /// The new logic file (also in `files`), for the rows.
    pub logic: Option<Vec<u8>>,

    /// The damage centre and box of each damage level, the building's frame.
    pub damage: Vec<Aabb>,
}

/// The sampling step of the broken faces per LOD (m).
const CAP_STEP: [f32; 5] = [0.5, 0.8, 1.5, 2.5, 4.0];

/// Fragments smaller than this (their box's diagonal, m) are left out of a LOD.
const LOD_MIN_FRAGMENT: [f32; 5] = [0.0, 0.0, 1.0, 2.0, 3.5];

/// One fragment of a piece: its cell, its bone's origin, and its size.
#[derive(Clone, Debug)]
struct Fragment {
    cell: usize,
    level: usize,
    origin: V3,
    size: f32,
    volume: f32,
    class: MaterialClass,
    hull: Vec<V3>,
}

/// A piece's LOD split, capped, with what its caps and blocks are made of.
struct PieceLod {
    split: Split,
    caps: Caps,
    solid: Solid,
    block_of_tri: Vec<usize>,
    cut: Vec<bool>,
}

struct Context<'a> {
    params: &'a Params,
    source: &'a Source,
    templates: &'a Templates,
    exists: &'a dyn Fn(&str) -> bool,
    cells: Cells,
    culture: String,
    shell: Option<f32>,
    walkable: bool,

    /// Around the damage: where a shell shows its inner side.
    near_damage: Aabb,

    /// How many LODs are made (all of them, or the first for a preview).
    lods: usize,
}

/// How much is made.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// The levels' LODs: all of them, or only the first ones (a preview; not for the game).
    pub lods: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { lods: usize::MAX }
    }
}

/// Generates the destroyed building.
///
/// # Errors
///
/// The building has nothing to break, or a file doesn't write.
pub fn generate(source: &Source, params: &Params, templates: &Templates, exists: &dyn Fn(&str) -> bool, options: Options) -> Result<Generated, String> {
    let n = params.levels.len();
    if n == 0 {
        return Err("no damage level".into());
    }

    let culture = params.culture.clone().unwrap_or_else(|| culture(&source.key, source.textures().iter().map(String::as_str)).to_owned());
    let solid_kind = match params.interior {
        Interior::Auto => solid_by_kind(&source.category, &source.key),
        Interior::Solid => true,
        Interior::Hollow => false,
    };

    let cx = Context {
        params,
        source,
        templates,
        exists,
        cells: planked(Cells::make(&source.bounds, params), source, params, &culture),
        culture,
        shell: (!solid_kind).then_some(params.thickness.max(0.05)),
        walkable: params.walkable.unwrap_or(is_fort(&source.category)),
        near_damage: {
            let mut b = Aabb::EMPTY;
            for v in params.levels.iter().flat_map(|l| &l.volumes) {
                b.merge(&v.bounds());
            }
            b.expanded(source.bounds.size().max() * 0.25)
        },
        lods: options.lods.max(1),
    };

    let key = params.key.clone();
    let mut out = Generated { key: key.clone(), levels: n + 1, ..Default::default() };
    out.notes.extend(source.notes.iter().cloned());
    out.damage = params.levels.iter().map(|l| {
        let mut b = Aabb::EMPTY;
        for v in &l.volumes {
            b.merge(&v.bounds());
        }
        b
    }).collect();

    let collisions = source.logic().map(|l| intact_collisions(l)).transpose()?.unwrap_or_default();
    let mut piece_heaps: Vec<(String, Vec<Option<Heap>>, Vec<Fixed>, Vec<Aabb>)> = vec![];
    for piece in &source.pieces {
        let made = make_piece(&cx, piece, &mut out)?;
        let name = piece.name();
        // The collision mesh of each damage level: the intact one cut and capped, and the heap.
        let mut fixed = vec![];
        if let Some(tris) = collisions.get(&name) {
            let solid = Solid::new(tris.clone(), None);
            let c = caps(&cx.cells, &solid, 0.5);
            for l in 1..=n {
                let mut t = outside(tris, &cx.cells, l);
                t.extend(c.standing_at(l).flat_map(|cap| (1..cap.poly.len() - 1).map(|i| [cap.poly[0], cap.poly[i], cap.poly[i + 1]])));
                if let Some(h) = &made.heaps[l] {
                    let coarse = resample(h, 2.0);
                    t.extend(coarse.mesh(0.3).into_iter().map(|q| q.map(|(p, _)| p)));
                }
                let (vertices, triangles) = weld(&t);
                fixed.push(Fixed { vertices, triangles });
            }
        }
        piece_heaps.push((name, made.heaps, fixed, made.bounds));
    }

    // The logic file.
    if let Some(logic) = source.logic() {
        let gone_fns = (1..=n).map(|l| {
            let cells = &cx.cells;
            move |p: &V3| cells.removed_by(p, l)
        }).collect::<Vec<_>>();
        let cut_cells = (1..=n).map(|l| {
            let mut v = cx.cells.shapes.iter().filter(|(i, _)| cx.cells.rank(**i) <= l).map(|(i, s)| (*i, s.bounds(), s.faces.iter().map(|f| f.plane).collect::<Vec<_>>())).collect::<Vec<_>>();
            v.sort_by_key(|x| x.0);
            v.into_iter().map(|(_, b, p)| (b, p)).collect::<Vec<_>>()
        }).collect::<Vec<_>>();
        let pieces = piece_heaps.iter().map(|(name, heaps, fixed, bounds)| PieceLogic {
            name: name.clone(),
            levels: (1..=n).map(|l| LevelLogic {
                gone: &gone_fns[l - 1],
                cells: cut_cells[l - 1].clone(),
                heap: heaps[l].as_ref(),
                walkable: cx.walkable,
                collision: fixed.get(l - 1).map(|f| (f.vertices.clone(), f.triangles.clone())).unwrap_or_default(),
                bounds: bounds[l],
                dock_height: source.bounds.max.y * 0.85,
            }).collect(),
        }).collect::<Vec<_>>();
        let (bytes, notes) = make_logic(logic, &pieces)?;
        out.notes.extend(notes);
        out.files.insert(format!("{key}_tech.cs2.parsed"), bytes.clone());
        out.logic = Some(bytes);
    } else {
        out.notes.push("no logic file: the building has no platforms, collision or firing lines".into());
    }

    // The other files of the folder that are not levels, renamed (the zone file).
    let zones = source.files.zones_name();
    if let Some(z) = source.files.files.get(&zones) {
        out.files.insert(format!("{key}_zone_tech.xml"), z.clone());
    }

    Ok(out)
}

/// The cells with the removed ones that are mostly wood (by the intact LOD 0's triangles in them) split in planks, their grain the
/// longest spread of those triangles.
fn planked(mut cells: Cells, source: &Source, params: &Params, culture: &str) -> Cells {
    let mut wood = vec![];
    let mut all = vec![];
    for p in &source.pieces {
        let Some(lod) = p.model.lods().first() else { continue };
        for (b, tris) in lod_triangles(lod).iter().enumerate() {
            let block = &lod.mesh_blocks()[b];
            let class = override_of(&params.materials, block.material()).and_then(|o| o.class).unwrap_or_else(|| classify(block.material(), culture));
            for t in tris {
                let c = (pos(&t[0]) + pos(&t[1]) + pos(&t[2])) / 3.0;
                all.push(c);
                if class == MaterialClass::Wood {
                    wood.push(c);
                }
            }
        }
    }

    if wood.is_empty() {
        return cells;
    }

    let budget = params.levels.iter().map(|l| l.max_fragments).sum::<usize>();
    let width = params.levels.iter().map(|l| l.fragment_size).fold(0.0, f32::max) * 0.35;
    let grain = |_: usize, shape: &crate::cells::Polyhedron| {
        let b = shape.bounds();
        let inside = |p: &&V3| b.contains(p) && shape.contains(p, 1e-4);
        let mine = wood.iter().filter(inside).copied().collect::<Vec<_>>();
        let total = all.iter().filter(inside).count();
        if mine.len() < 4 || mine.len() * 10 < total * 6 {
            return None;
        }

        // The grain: the longest axis of the wood's spread in the cell.
        let c = mine.iter().sum::<V3>() / mine.len() as f32;
        let cov = mine.iter().fold(nalgebra::Matrix3::<f32>::zeros(), |m, p| m + (p - c) * (p - c).transpose());
        let eig = cov.symmetric_eigen();
        let (k, _) = eig.eigenvalues.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
        Some(eig.eigenvectors.column(k).into_owned().normalize())
    };
    cells.split_planks(&grain, width.max(0.35), 4, budget);
    cells
}

/// What [`make_piece`] gives the logic file: per level (0 intact) its heap and the box of its geometry.
struct PieceMade {
    heaps: Vec<Option<Heap>>,
    bounds: Vec<Aabb>,
}

fn make_piece(cx: &Context, piece: &crate::source::SourcePiece, out: &mut Generated) -> Result<PieceMade, String> {
    let params = cx.params;
    let n = params.levels.len();
    let key = &params.key;
    let src_key = &cx.source.key;
    let nn = piece.number;
    let lods = piece.model.lods();

    // Per LOD (in parallel): split, capped.
    let (cells, shell) = (&cx.cells, cx.shell);
    let pl = lods.par_iter().enumerate().take(cx.lods).map(|(l, lod)| {
        let tris = lod_triangles(lod);
        let cut = lod.mesh_blocks().iter().map(|b| is_cut(*b.mesh().material_type(), *b.material().vertex_format())).collect::<Vec<_>>();
        let mut flat = vec![];
        let mut block_of_tri = vec![];
        for (b, ts) in tris.iter().enumerate() {
            if cut[b] {
                for t in ts {
                    flat.push(t.clone().map(|v| pos(&v)));
                    block_of_tri.push(b);
                }
            }
        }

        let solid = Solid::new(flat, shell);
        let sp = split(&tris, &cut, cells);
        let cp = caps(cells, &solid, CAP_STEP[l.min(4)]);
        PieceLod { split: sp, caps: cp, solid, block_of_tri, cut }
    }).collect::<Vec<_>>();
    let lods = &lods[..pl.len()];

    let Some(lod0) = pl.first() else { return Err(format!("piece{nn:02}: no LOD")) };
    let classes0 = classes(cx, &lods[0].mesh_blocks()[..]);

    // The fragments (LOD 0), by level.
    let mut frags: Vec<Fragment> = vec![];
    let mut cells_here = lod0.split.cells.keys().chain(lod0.caps.fragments.iter().filter(|(_, v)| !v.is_empty()).map(|(k, _)| k)).copied().collect::<Vec<_>>();
    cells_here.sort_unstable();
    cells_here.dedup();
    for c in cells_here {
        let Some(level) = cx.cells.level[c].map(usize::from) else { continue };
        let mut pts = vec![];
        let mut votes = BTreeMap::<MaterialClass, usize>::new();
        if let Some(blocks) = lod0.split.cells.get(&c) {
            for (b, ts) in blocks.iter().enumerate() {
                for t in ts {
                    pts.extend(t.iter().map(pos));
                    *votes.entry(classes0.get(b).copied().unwrap_or_default()).or_default() += 1;
                }
            }
        }

        for cap in lod0.caps.fragments.get(&c).into_iter().flatten() {
            pts.extend(cap.poly.iter().copied());
        }

        if pts.len() < 4 {
            continue;
        }

        let b = Aabb::of(pts.iter().copied());
        let origin = b.centre();
        let step = (pts.len() / 256).max(1);
        let hull = pts.iter().step_by(step).map(|p| p - origin).collect();
        let class = votes.into_iter().max_by_key(|(_, k)| *k).map_or(MaterialClass::Stone, |(c, _)| c);
        frags.push(Fragment { cell: c, level, origin, size: b.size().norm(), volume: b.volume() * 0.35, class, hull });
    }

    // Each damage level's collapse simulated; the fragments' last poses.
    let mut finals: BTreeMap<usize, ([f32; 3], [f32; 4])> = BTreeMap::new();
    let mut heaps: Vec<Option<Heap>> = vec![None];
    let mut bounds = vec![Aabb::EMPTY; n + 1];
    let rng = Rng::new(params.seed ^ (u64::from(nn) << 32));
    for level in 1..=n {
        let lp = &params.levels[level - 1];
        let mine = frags.iter().filter(|f| f.level == level).collect::<Vec<_>>();
        let damage_centre = if lp.volumes.is_empty() { cx.source.bounds.centre() } else { lp.volumes.iter().map(|v| v3(v.centre)).sum::<V3>() / lp.volumes.len() as f32 };
        let reach = mine.iter().map(|f| (f.origin - damage_centre).norm()).fold(1e-3, f32::max);
        let dir = v3(lp.direction).try_normalize(1e-6).unwrap_or_else(V3::zeros);
        let mut r = rng.fork(level as u64);
        let bodies = mine.iter().map(|f| {
            let (density, friction, bounce) = f.class.physics();
            let out = (f.origin - damage_centre).try_normalize(1e-6).unwrap_or(dir);
            let v = dir * lp.force * r.range(0.6, 1.4) + (out * 0.6 + r.unit() * 0.4) * lp.force * lp.spread;
            Body {
                centre: f.origin,
                hull: f.hull.clone(),
                density,
                friction,
                restitution: bounce,
                release: lp.stagger * ((f.origin - damage_centre).norm() / reach) * r.range(0.75, 1.0) + r.range(0.0, 0.1),
                linvel: v,
                angvel: r.unit() * lp.spin * r.range(0.5, 2.0),
            }
        }).collect::<Vec<_>>();

        // What stands at this level, and the earlier debris, fixed.
        let standing = standing_gather(cx, &pl[0], lods, 0, level, &frags, &finals, false, heaps.get(level - 1).and_then(Option::as_ref));
        let fixed_tris = standing.slots.iter().flat_map(|(_, t)| t.iter().map(|t| t.clone().map(|v| pos(&v)))).collect::<Vec<_>>();
        let (vertices, triangles) = weld(&fixed_tris);
        let track = simulate(&bodies, &[Fixed { vertices, triangles }], params.fps, 2.0, params.max_duration);
        if let Some(last) = track.last() {
            for (f, pose) in mine.iter().zip(last) {
                finals.insert(f.cell, *pose);
            }
        }

        // The heap: every fragment so far, where it lies.
        let heap = (params.rubble > 0.0).then(|| {
            let mut region = Aabb::EMPTY;
            for f in frags.iter().filter(|f| f.level <= level) {
                if let Some((t, _)) = finals.get(&f.cell) {
                    region.add(&v3(*t));
                }
            }
            for v in params.levels.iter().take(level).flat_map(|l| &l.volumes) {
                let b = v.bounds();
                region.add(&V3::new(b.min.x, 0.0, b.min.z));
                region.add(&V3::new(b.max.x, 0.0, b.max.z));
            }
            let mut h = Heap::new(&region, 0.5);
            for f in frags.iter().filter(|f| f.level <= level) {
                if let Some((t, _)) = finals.get(&f.cell) {
                    h.deposit(t[0], t[2], f.volume * params.rubble * 0.6, 1.2 * f.volume.cbrt() + 1.0);
                }
            }
            h.relax(400);
            h
        }).filter(|h| h.max_height() > 0.05);

        // A level without fragments here changes at once (as most of the game's buildings do).
        if !mine.is_empty() {
            let made = collapse(cx, piece, &pl[0], lods, level, &frags, &finals, &track, heaps[level - 1].as_ref(), damage_centre, out)?;
            out.collapses.push(made);
        }
        heaps.push(heap);
    }

    // The levels' models.
    let intact_name = format!("{src_key}_piece{nn:02}_destruct01.rigid_model_v2");
    if let Some(bytes) = cx.source.files.files.get(&intact_name) {
        out.files.insert(format!("{key}_piece{nn:02}_destruct01.rigid_model_v2"), bytes.clone());
        bounds[0] = cx.source.bounds;
    }

    if let Some(meta) = cx.source.files.files.get(&format!("{intact_name}.meta")) {
        out.files.insert(format!("{key}_piece{nn:02}_destruct01.rigid_model_v2.meta"), meta.clone());
    }

    for state in &piece.states {
        if let Some(bytes) = cx.source.files.files.get(state) {
            let suffix = &state[intact_name.len() - ".rigid_model_v2".len()..];
            for level in 0..=n {
                let renamed = format!("{key}_piece{nn:02}_destruct{:02}{}", level + 1, suffix.trim_start_matches("_destruct01").trim_start_matches(&format!("{src_key}_piece{nn:02}")));
                out.files.insert(renamed, bytes.clone());
            }
        }
    }

    for level in 1..=n {
        let mut model = piece.model.clone();
        let mut new_lods = vec![];
        for (l, lod) in lods.iter().enumerate() {
            let g = standing_gather(cx, &pl[l], lods, l, level, &frags, &finals, true, heaps[level].as_ref());
            let b = g.bounds();
            if l == 0 {
                bounds[level] = b;
            }

            let pivot = if b.is_empty() { V3::zeros() } else { b.centre() };
            let name = format!("piece{nn:02}_destruct{:02}_lod{:02}", level + 1, l + 1);
            let blocks = g.blocks(&|s| material(cx, lod.mesh_blocks(), s), &name, -1, pivot);
            new_lods.push(lod_like(lod, blocks));
        }

        *model.lods_mut() = new_lods;
        model.set_skeleton_id(String::new());
        out.files.insert(format!("{key}_piece{nn:02}_destruct{:02}.rigid_model_v2", level + 1), encode(&mut model)?);
        out.files.insert(format!("{key}_piece{nn:02}_destruct{:02}.rigid_model_v2.meta", level + 1), standing_meta(cx, &intact_name, level, &frags).encode());
    }

    Ok(PieceMade { heaps, bounds })
}

/// The material class of each block of a LOD (an override, else from its textures).
fn classes(cx: &Context, blocks: &[MeshBlock]) -> Vec<MaterialClass> {
    blocks.iter().map(|b| override_of(&cx.params.materials, b.material()).and_then(|o| o.class).unwrap_or_else(|| classify(b.material(), &cx.culture))).collect()
}

/// The block a slot is made with: the source's own, or the culture's damage or rubble set.
fn material(cx: &Context, blocks: &[MeshBlock], slot: Slot) -> Option<MeshBlock> {
    match slot {
        Slot::Source(b) => blocks.get(b).cloned(),
        Slot::Damage(class) => {
            let class = MaterialClass::ALL[class as usize];
            let (tpl, _) = cx.templates.damage.as_ref().map(|(b, v)| (b.clone(), v.clone())).or_else(|| blocks.first().map(|b| (b.clone(), Vertex::default())))?;
            let set = damage_set(cx, blocks, class);
            Some(set_block(&tpl, &set, cx.exists))
        }

        Slot::Rubble => {
            let (tpl, _) = cx.templates.rubble.as_ref().map(|(b, v)| (b.clone(), v.clone())).or_else(|| blocks.first().map(|b| (b.clone(), Vertex::default())))?;
            let (_, rubble) = damage_sets(&cx.culture, MaterialClass::Stone);
            let set = if has_set(rubble, cx.exists) { rubble } else { "roman_wall_rubble" };
            Some(set_block(&tpl, set, cx.exists))
        }
    }
}

/// The damage set of a class's broken faces: an override of one of its materials, else the culture's (Roman's when the packs lack it).
fn damage_set(cx: &Context, blocks: &[MeshBlock], class: MaterialClass) -> String {
    for b in blocks {
        if let Some(o) = override_of(&cx.params.materials, b.material()) {
            if o.class.unwrap_or_else(|| classify(b.material(), &cx.culture)) == class {
                if let Some(d) = &o.damage {
                    return d.clone();
                }
            }
        }
    }

    let (set, _) = damage_sets(&cx.culture, class);
    if has_set(set, cx.exists) { set.to_owned() } else { "roman_damage_bricks_all".to_owned() }
}

fn template_vertex(cx: &Context, rubble: bool) -> Vertex {
    let t = if rubble { &cx.templates.rubble } else { &cx.templates.damage };
    t.as_ref().map(|(_, v)| v.clone()).unwrap_or_else(|| {
        let mut v = Vertex::default();
        v.set_normal(nalgebra::Vector4::new(0.0, 1.0, 0.0, -1.0));
        v.set_tangent(nalgebra::Vector4::new(1.0, 0.0, 0.0, -1.0));
        v.set_bitangent(nalgebra::Vector4::new(0.0, 0.0, 1.0, -1.0));
        v
    })
}

/// The class of a cap: the class of the block of the triangle nearest it.
fn cap_class(pl: &PieceLod, classes: &[MaterialClass], cap: &Cap) -> MaterialClass {
    let c = cap.poly.iter().sum::<V3>() / cap.poly.len().max(1) as f32;
    pl.solid.nearest(&c, 3.0).and_then(|t| pl.block_of_tri.get(t)).and_then(|&b| classes.get(b).copied()).unwrap_or_default()
}

fn cap_tris(cx: &Context, pl: &PieceLod, classes: &[MaterialClass], cap: &Cap) -> (Slot, Vec<Tri>) {
    let class = cap_class(pl, classes, cap);
    // Classes broken with the same texture set share a block: the slot is the first class of that set.
    let set = damage_sets(&cx.culture, class).0;
    let first = MaterialClass::ALL.iter().position(|c| damage_sets(&cx.culture, *c).0 == set).unwrap_or(0);
    (Slot::Damage(first as u8), polygon_tris(&template_vertex(cx, false), &cap.poly, &cap.normal, DAMAGE_SPAN))
}

/// What a level shows at a LOD: what stands (and its broken faces, and the inner side of shells), every earlier collapse's fragments
/// where they landed (`with_own`: also this level's), and the heap.
#[allow(clippy::too_many_arguments)]
fn standing_gather(cx: &Context, pl: &PieceLod, lods: &[rpfm_lib::files::rigidmodel::Lod], l: usize, level: usize, frags: &[Fragment], finals: &BTreeMap<usize, ([f32; 3], [f32; 4])>, with_own: bool, heap: Option<&Heap>) -> Gather {
    let mut g = Gather::default();
    let classes = classes(cx, lods[l].mesh_blocks());
    let shells = cx.shell.filter(|_| l == 0 && level > 0);
    for (b, tris) in pl.split.standing(&cx.cells, level).into_iter().enumerate() {
        if let Some(t) = shells.filter(|_| pl.cut.get(b).copied().unwrap_or(false)) {
            let near = tris.iter().filter(|x| cx.near_damage.contains(&((pos(&x[0]) + pos(&x[1]) + pos(&x[2])) / 3.0))).map(|x| inner(x, t)).collect::<Vec<_>>();
            g.add(Slot::Source(b), near);
        }
        g.add(Slot::Source(b), tris);
    }

    for cap in pl.caps.standing_at(level) {
        let (slot, tris) = cap_tris(cx, pl, &classes, cap);
        g.add(slot, tris);
    }

    let min = LOD_MIN_FRAGMENT[l.min(4)];
    for f in frags.iter().filter(|f| f.level < level || (with_own && f.level == level)) {
        if f.size < min {
            continue;
        }

        let Some((t, q)) = finals.get(&f.cell) else { continue };
        let to = v3(*t);
        add_fragment(cx, pl, &classes, f, &mut g, &|v: &Vertex| moved(v, &f.origin, *q, &to));
    }

    if let Some(h) = heap {
        let h = resample(h, [1.0, 1.0, 2.0, 3.0, 4.0][l.min(4)]);
        let tv = template_vertex(cx, true);
        g.add(Slot::Rubble, h.mesh(0.05).iter().map(|t| surface_tri(&tv, t, RUBBLE_SPAN)).collect::<Vec<_>>());
    }

    g
}

/// A fragment's triangles (its cell's part of every block, its caps, the inner side of a shell) through `place`.
fn add_fragment(cx: &Context, pl: &PieceLod, classes: &[MaterialClass], f: &Fragment, g: &mut Gather, place: &dyn Fn(&Vertex) -> Vertex) {
    if let Some(blocks) = pl.split.cells.get(&f.cell) {
        for (b, tris) in blocks.iter().enumerate() {
            g.add(Slot::Source(b), tris.iter().map(|t| t.clone().map(|v| place(&v))).collect::<Vec<_>>());
        }
    }

    for cap in pl.caps.fragments.get(&f.cell).into_iter().flatten() {
        let (slot, tris) = cap_tris(cx, pl, classes, cap);
        g.add(slot, tris.iter().map(|t| t.clone().map(|v| place(&v))).collect::<Vec<_>>());
    }
}

/// A heap sampled again every `step` (the far LODs').
fn resample(h: &Heap, step: f32) -> Heap {
    let region = Aabb { min: V3::new(h.x0 + 4.0, 0.0, h.z0 + 4.0), max: V3::new(h.x0 + (h.nx - 1) as f32 * h.step - 4.0, 0.0, h.z0 + (h.nz - 1) as f32 * h.step - 4.0) };
    let mut c = Heap::new(&region, step);
    for j in 0..c.nz {
        for i in 0..c.nx {
            c.h[j * c.nx + i] = h.height(c.x0 + i as f32 * step, c.z0 + j as f32 * step);
        }
    }
    c
}

/// The collapse into damage level `level`: its model (bone 0 what stays, a bone per fragment), its animation and its effects.
#[allow(clippy::too_many_arguments)]
fn collapse(cx: &Context, piece: &crate::source::SourcePiece, pl: &PieceLod, lods: &[rpfm_lib::files::rigidmodel::Lod], level: usize, frags: &[Fragment], finals: &BTreeMap<usize, ([f32; 3], [f32; 4])>, track: &crate::sim::Track, prev_heap: Option<&Heap>, centre: V3, out: &mut Generated) -> Result<Collapse, String> {
    let params = cx.params;
    let key = &params.key;
    let nn = piece.number;
    let stem = format!("{key}_piece{nn:02}_destruct{level:02}_anim");
    let name = format!("building_piece{nn:02}_destruct{level:02}_anim");
    let classes = classes(cx, lods[0].mesh_blocks());
    let mine = frags.iter().filter(|f| f.level == level).collect::<Vec<_>>();

    let mut blocks = vec![];
    let stay = standing_gather(cx, pl, lods, 0, level, frags, finals, false, prev_heap);
    let pivot = stay.bounds().centre();
    blocks.extend(stay.blocks(&|s| material(cx, lods[0].mesh_blocks(), s), &name, 0, if stay.is_empty() { V3::zeros() } else { pivot }));
    for (i, f) in mine.iter().enumerate() {
        let mut g = Gather::default();
        let origin = f.origin;
        add_fragment(cx, pl, &classes, f, &mut g, &|v: &Vertex| {
            let mut x = v.clone();
            let p = x.position_mut();
            (p.x, p.y, p.z) = (p.x - origin.x, p.y - origin.y, p.z - origin.z);
            x
        });
        blocks.extend(g.blocks(&|s| material(cx, lods[0].mesh_blocks(), s), &name, i as i32 + 1, V3::zeros()));
    }

    let mut model = piece.model.clone();
    model.set_skeleton_id("building".into());
    let lod0 = lod_like(&lods[0], blocks);
    *model.lods_mut() = vec![lod0];
    out.files.insert(format!("{stem}.rigid_model_v2"), encode(&mut model)?);

    // The animation: bone 0 still, a bone per fragment from the simulation.
    let names = vec![name.clone(); mine.len() + 1];
    let frames = track.frames.iter().map(|f| {
        let mut bones = vec![([0.0; 3], [0.0, 0.0, 0.0, 1.0])];
        bones.extend(f.iter().copied());
        bones
    }).collect::<Vec<_>>();
    let frames = if frames.is_empty() { vec![vec![([0.0; 3], [0.0, 0.0, 0.0, 1.0])]] } else { frames };
    let anim = AnimFile::rigid("building", params.fps, &names, &frames);
    let duration = anim.end;
    out.files.insert(format!("{stem}.anim"), anim.encode());

    // The effects.
    let effects = match params.effects.get(level - 1).cloned().flatten() {
        Some(e) => e,
        None => collapse_effects(cx, level, &mine, &track.impacts, centre),
    };
    out.files.insert(format!("{stem}.meta"), MetaFile { version: 2, entries: effects.clone() }.encode());
    Ok(Collapse { piece: nn, level, fragments: mine.len(), duration, effects })
}

/// A collapse's effects: the dust over the damage at the start (light from intact, heavy after), the sounds that start it where the
/// damage is, and the impacts of the largest fragments where they hit (the game's sounds for stone, or for wood).
fn collapse_effects(cx: &Context, level: usize, mine: &[&Fragment], impacts: &[Impact], centre: V3) -> Vec<MetaEntry> {
    let mut b = Aabb::EMPTY;
    for v in &cx.params.levels[level - 1].volumes {
        b.merge(&v.bounds());
    }

    let mut b = Aabb { min: b.min.sup(&cx.source.bounds.min), max: b.max.inf(&cx.source.bounds.max) };
    if b.is_empty() {
        b = cx.source.bounds;
    }

    let dust = if level == 1 { "destruction_vfx_area_dust_light" } else { "destruction_vfx_area_dust_heavy" };
    let s = b.size();
    let mut out = vec![MetaFile::volumetric(dust, 0.0106, [b.centre().x, b.centre().y, b.centre().z], [s.x, s.y, s.z])];
    let wood = mine.iter().filter(|f| f.class == MaterialClass::Wood).count() * 2 > mine.len();
    let sounds = if wood { &cx.templates.wood } else { &cx.templates.stone };
    for id in &sounds.start {
        out.push(sound_entry(*id, [centre.x, centre.y, centre.z], 0.044));
    }

    if !sounds.impacts.is_empty() {
        let mut best = impacts.to_vec();
        best.sort_by(|a, b| b.speed.total_cmp(&a.speed));
        best.truncate(10);
        best.sort_by(|a, b| a.time.total_cmp(&b.time));
        for (k, i) in best.iter().enumerate() {
            out.push(sound_entry(sounds.impacts[k % sounds.impacts.len()], [i.at.x, i.at.y.max(1.0), i.at.z], i.time));
        }
    }

    out
}

/// A damage level's standing `.meta`: its burst over the damage (stone or wood), and the intact level's effects where it still stands.
fn standing_meta(cx: &Context, intact: &str, level: usize, frags: &[Fragment]) -> MetaFile {
    let mut entries = vec![];
    let lp = &cx.params.levels[level - 1];
    let mut b = Aabb::EMPTY;
    for v in &lp.volumes {
        b.merge(&v.bounds());
    }

    let mine = frags.iter().filter(|f| f.level == level).collect::<Vec<_>>();
    if !b.is_empty() && !mine.is_empty() {
        let wood = mine.iter().filter(|f| f.class == MaterialClass::Wood).count() * 2 > mine.len();
        let burst = if wood { "destruction_vfx_generic_rigid_wood" } else { "destruction_vfx_generic_rigid_stone" };
        let b = Aabb { min: b.min.sup(&cx.source.bounds.min), max: b.max.inf(&cx.source.bounds.max) };
        if !b.is_empty() {
            let s = b.size();
            entries.push(MetaFile::volumetric(burst, 0.0, [b.centre().x, b.centre().y, b.centre().z], [s.x, s.y, s.z]));
        }
    }

    if let Some(meta) = cx.source.files.files.get(&format!("{intact}.meta")).and_then(|m| MetaFile::parse(m)) {
        for e in meta.entries {
            let at = match &e.body {
                MetaBody::Effect { position, .. } => Some(v3(*position)),
                MetaBody::Volumetric { floats, .. } => Some(V3::new(floats[0] + floats[7], floats[1] + floats[8], floats[2] + floats[9])),
                MetaBody::Sound(_) => None,
            };

            if at.is_none_or(|p| !cx.cells.removed_by(&p, level)) {
                entries.push(e);
            }
        }
    }

    MetaFile { version: 2, entries }
}

fn encode(model: &mut RigidModel) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    model.encode(&mut out, &None).map_err(|e| format!("rmv2: {e}"))?;
    Ok(out)
}
