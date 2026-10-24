//! The generator (docs/plans/siege_ai.md, "Design"). In order:
//!
//! 1. **Perimeter.** Walls, no-go regions and the sea are a barrier; the outside is flood-filled from the map's edge. What is not reached
//!    around the main capture point is the settlement (a town without a closed ring: the barrier closed over its gaps, then with the
//!    buildings too). Its free inside and the wall pieces along it, grown outward, give the fort perimeter.
//! 2. **Free space** inside the perimeter: no no-go, building, wall or sea; slivers and tiny holes (wells, statues) removed. Its
//!    clearance (distance to the nearest obstacle) and its skeleton (thinned to lines) give the streets.
//! 3. **Nodes** on the skeleton: an area at each capture point, on the city side of each gate, at each plaza (a wide spot) and at each
//!    dead end; an entry where a street leaves through the perimeter; an intersection at each junction no area covers.
//! 4. **Regions:** the free space split between the nodes (nearest along the ground, within each node's reach). The skeleton outside every
//!    region is the streets: a piece touching two nodes is a street between them; one touching more gets an intersection; one longer than
//!    the longest street gets areas along it. Regions that touch are joined by an area connection.
//! 5. **Shapes:** each node's rectangle grown in its own free space (rectangles never overlap), its Boundary its region grown a little
//!    (onto the building faces); each street's line from inside one rectangle to inside the other, its two sides on the corridor's faces
//!    with a short tick into each node.
//! 6. **Walls:** the middle of the fort pieces' platforms, cut where it enters no-go.
//! 7. **Deployment and approaches** from the distance to the perimeter.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::raster::{Grid, Mask, N4, N8, area2, dist, inside, simplify, simplify_loop};
use crate::{Deployment, Edge, EdgeKind, Input, Layout, Node, NodeKind, P, ZoneKind};

/// The raster's cell (scene units).
pub const CELL: f64 = 2.0;

/// The fort perimeter's distance outside the walls (CA: 10-14).
pub const PERIMETER_OUT: f64 = 12.0;

/// A street longer than this gets areas along it (a guide: the AI takes long to walk long streets).
pub const MAX_STREET: f64 = 300.0;

/// A stretch of the perimeter at most this far from a wall edge is closed by the wall (CA's perimeter: ~14 out of the wall line, ours
/// ~25); further, by a separator.
const SEPARATOR_WALL: f64 = 35.0;

/// A gate's wall line goes this far past its ends, to join the walkways either side.
const GATE_OVERLAP: f64 = 8.0;

/// Separators only where the walls run along at least this share of the perimeter (a walled town).
const SEPARATOR_MIN_WALLED: f64 = 0.3;

/// A town whose perimeter comes this close to the sea is a port.
const PORT_REACH: f64 = 40.0;

/// A port's quay entries: on streets at most this far from the water, at most this many, this far apart.
const QUAY_REACH: f64 = 30.0;
const MAX_QUAY_ENTRIES: usize = 3;
const QUAY_APART: f64 = 100.0;

/// Two nodes whose regions touch are joined by an area connection when its line is at most this long, else by a street.
pub const MAX_CONNECTION: f64 = 40.0;

/// A plaza: a spot of the free space at least this far from every obstacle (its width twice that).
pub const PLAZA_CLEARANCE: f64 = 20.0;

/// The defender's zone: the perimeter this far in; its guerrilla-exclusion polygon this far out.
pub const DEFENDER_IN: f64 = 7.0;
pub const DEFENDER_GUERRILLA_OUT: f64 = 150.0;

/// The attacker's zone starts this far outside the perimeter (the user: about 190; CA: ~200), its guerrilla one this far.
pub const ATTACKER_OUT: f64 = 190.0;
pub const ATTACKER_GUERRILLA_OUT: f64 = 70.0;

/// The defender's zone takes in the sea up to this far from the perimeter (CA's: 200-500 from the coast).
pub const DEFENDER_SEA: f64 = 400.0;

/// In a port the attacker's land stops this short of the coast, its sea this far offshore (CA's: ~20, ~150).
pub const ATTACKER_SHORE: f64 = 20.0;
pub const ATTACKER_OFFSHORE: f64 = 150.0;

/// The attacker's rectangle overhangs the map by this (CA draws past the edge; the export clips it).
pub const OVERHANG: f64 = 100.0;

/// Capture points further than this from the main one don't decide where the settlement is.
const MAX_CAPTURE_REACH: f64 = 1000.0;

/// Enclosed pockets at most this far apart (a wall's thickness, a rock) are parts of one settlement.
const POCKET_GAP: f64 = 30.0;

/// A no-go region at most this big next to the settlement is one of its blocks: the perimeter goes around it.
const MAX_BLOCK_AREA: f64 = 60_000.0;

/// A gap in the settlement's outline at least this wide is a way in (an entry node where a street leaves through it).
pub const MIN_ENTRY_GAP: f64 = 20.0;

/// The settlement's barrier attempts: whether the buildings are part of it, and the radius its gaps are closed with.
const ENCLOSURES: [(bool, f64); 11] = [
    (false, 0.0), (false, 6.0), (false, 12.0), (false, 24.0), (true, 12.0), (true, 24.0), (true, 40.0), (true, 60.0), (true, 90.0),
    (true, 130.0), (true, 180.0),
];

/// The layout of a settlement.
pub fn generate(input: &Input) -> Layout {
    let mut out = Layout::default();
    let (lo, hi) = input.bounds;
    if hi[0] - lo[0] < 10.0 || hi[1] - lo[1] < 10.0 {
        out.warnings.push("the map has no size".to_owned());
        return out;
    }

    let g = Grid::new(lo, hi, CELL);
    let mut nogo = g.mask();
    for p in &input.nogo {
        g.fill_polygon(&mut nogo, p);
    }

    let mut buildings = g.mask();
    for p in &input.obstacles {
        g.fill_polygon(&mut buildings, p);
    }

    // Each fort piece's cells: its footprint, its platforms, its outlines (thin parts, a gate's leaves, cover no cell centre) and its
    // walkway line.
    let wall_cells: Vec<Vec<usize>> = input.walls.iter().map(|w| {
        let mut m = vec![];
        for r in w.footprint.iter().chain(&w.platforms) {
            m.extend(g.polygon_cells(r));
            let mut ring = r.clone();
            ring.push(r[0]);
            m.extend(line_cells(&g, &ring, 1.2));
        }

        m.extend(line_cells(&g, &w.walkway, 3.0));
        m
    }).collect();
    let mut walls = g.mask();
    for c in wall_cells.iter().flatten() {
        walls[*c] = true;
    }

    // An encampment's palisade: its runs are walls; its openings are shut for the settlement's outline (their entries open them again).
    let runs = crate::palisade::runs(&input.palisade);
    let openings = crate::palisade::openings(&runs);
    for r in &runs {
        for c in line_cells(&g, &r.line(), 1.5) {
            walls[c] = true;
        }
    }

    let mut shut = g.mask();
    for o in &openings {
        for c in line_cells(&g, &[o.a, o.b], 2.5) {
            shut[c] = true;
        }
    }

    // The sea: the water joined to the map's edge (not ponds and fountains), unless the settlement is in it (a town on low ground
    // inland).
    let mut sea: Mask = match &input.sea {
        Some(f) => {
            let raw: Mask = (0..g.len()).map(|k| f.at(g.centre(k))).collect();
            g.flood(&raw, (0..g.len()).filter(|&k| g.on_border(k)))
        }
        None => g.mask(),
    };
    let wet = |p: P| g.at(p).is_some_and(|k| sea[k]);
    if wet(settlement_seed(input)) || input.captures.iter().any(|c| wet(c.at)) {
        out.warnings.push("the settlement stands on ground under 0 m joined to the map's edge: taken as land, not sea".to_owned());
        sea = g.mask();
    }


    // ---- 1. The settlement and its perimeter: the first barrier that closes around the seed and every capture point near it (CA's
    // perimeters hold all of them), else the first that closes around the seed.
    let seed_point = settlement_seed(input);
    let barrier0: Mask = (0..g.len()).map(|k| walls[k] || nogo[k] || sea[k] || shut[k]).collect();
    let capture_cells: Vec<usize> = input.captures.iter().filter(|c| dist(c.at, seed_point) <= MAX_CAPTURE_REACH).filter_map(|c| g.at(c.at)).collect();
    let (mut found, mut fallback) = (None, None);
    for (with_buildings, r) in ENCLOSURES {
        let raw: Mask = (0..g.len()).map(|k| barrier0[k] || (with_buildings && buildings[k])).collect();
        let barrier = g.close(&raw, r);
        let pass: Mask = barrier.iter().map(|&b| !b).collect();
        let outside = g.flood(&pass, (0..g.len()).filter(|&k| g.on_border(k)));
        let Some(seed) = nearest(&g, seed_point, |k| !raw[k] && !outside[k], 200.0) else { continue };

        // Every capture point next to the inside (one in a block, a building or a tower on the wall: by the open ground next to it).
        let inside = pockets(&g, &raw, &outside, seed, &[]);
        let all = capture_cells.iter().all(|&c| {
            let p = g.centre(c);
            nearest(&g, p, |k| inside[k], 40.0).is_some() || nearest(&g, p, |k| !raw[k], 40.0).is_none()
        });
        let candidate = (with_buildings, r, raw, barrier, outside, seed);
        if all {
            found = Some(candidate);
            break;
        }

        fallback = fallback.or(Some(candidate));
    }

    if found.is_none() && fallback.is_some() {
        out.warnings.push("some capture points stay outside the settlement's outline".to_owned());
    }

    let Some((with_buildings, closed_r, raw, _, outside, seed)) = found.or(fallback) else {
        out.warnings.push("no settlement found: the walls, no-go regions and buildings around the main capture point never close, even over gaps of 360 units".to_owned());
        return out;
    };

    if closed_r >= MIN_ENTRY_GAP / 2.0 {
        out.warnings.push(format!(
            "the walls and no-go regions do not close around the settlement: its outline was closed over gaps up to {:.0} units{} (streets leaving there get entry nodes)",
            2.0 * closed_r, if with_buildings { " with the buildings" } else { "" },
        ));
    }

    // The free inside around the seed (the closed barrier only decides what is outside: the streets it filled are in), and what bounds
    // it: the fort pieces along it, the small no-go regions (blocks of houses) and buildings at its edge.
    let capture_at: Vec<usize> = input.captures.iter().filter_map(|c| g.at(c.at)).collect();
    let interior = pockets(&g, &raw, &outside, seed, &capture_at);
    let near_interior = g.dilate(&interior, 4.0);
    let mut core = interior.clone();
    let mut bounding: Vec<Vec<usize>> = wall_cells.clone();
    for p in input.nogo.iter().filter(|p| area2(p).abs() / 2.0 <= MAX_BLOCK_AREA) {
        bounding.push(g.polygon_cells(p));
    }

    if with_buildings {
        bounding.extend(input.obstacles.iter().map(|p| g.polygon_cells(p)));
    }

    for cells in &bounding {
        if cells.iter().any(|&c| near_interior[c]) {
            for &c in cells {
                core[c] = true;
            }
        }
    }

    let region = {
        let grown = g.dilate(&core, PERIMETER_OUT);
        let filled = g.fill_holes(&grown);
        // The part around the seed.
        let (label, _) = g.components(&filled);
        let id = label[seed];
        filled.iter().enumerate().map(|(k, &s)| s && label[k] == id).collect::<Mask>()
    };

    out.perimeter = outer_loop(&g, &region, 2.0);

    // A port: the town reaches the sea (a coastal town further from it is laid out as an inland one, as CA's).
    let dout = g.distance(&region);
    let port = (0..g.len()).any(|k| sea[k] && dout[k] <= PORT_REACH);
    if out.perimeter.len() < 3 {
        out.warnings.push("the settlement's outline is empty".to_owned());
        return out;
    }

    // ---- 2. Free space: the inside, without what blocks.
    let blocked: Mask = (0..g.len()).map(|k| nogo[k] || buildings[k] || walls[k] || sea[k]).collect();
    let mut free: Mask = (0..g.len()).map(|k| interior[k] && !blocked[k]).collect();
    free = g.dilate(&g.erode(&free, 3.0), 3.0).into_iter().zip(&free).map(|(a, &b)| a && b).collect();
    fill_small_holes(&g, &mut free, 300.0);

    // Each palisade opening and the ground outside it (its entry's) are free, and a way out (the lines into it are kept).
    let mut entry_ground: Vec<(P, Vec<usize>)> = vec![];
    for o in &openings {
        let along = [(o.b[0] - o.a[0]) / o.width.max(1e-9), (o.b[1] - o.a[1]) / o.width.max(1e-9)];
        let (across, depth) = (o.width.clamp(24.0, 32.0) / 2.0, 10.0);
        let c = [o.mid[0] + o.out[0] * (depth + 1.0), o.mid[1] + o.out[1] * (depth + 1.0)];
        let corner = |a: f64, b: f64| [c[0] + along[0] * a + o.out[0] * b, c[1] + along[1] * a + o.out[1] * b];
        let mut cells = g.polygon_cells(&[corner(-across, -depth), corner(across, -depth), corner(across, depth), corner(-across, depth)]);
        cells.extend(line_cells(&g, &[o.a, o.b], 3.0));
        cells.extend(line_cells(&g, &[[o.mid[0] - o.out[0] * 8.0, o.mid[1] - o.out[1] * 8.0], o.mid], 4.0));
        cells.retain(|&k| !blocked[k]);
        for &k in &cells {
            free[k] = true;
        }

        let ground = g.polygon_cells(&[corner(-across, -depth), corner(across, -depth), corner(across, depth), corner(-across, depth)]);
        entry_ground.push((c, ground));
    }

    keep_large_parts(&g, &mut free, 1500.0, seed);
    let clearance = g.distance(&free.iter().map(|&s| !s).collect());

    // Where a street leaves the settlement: free cells next to the outside, in a gap the outline closed (one wide enough to walk through).
    let mut exits = g.mask();
    if closed_r >= MIN_ENTRY_GAP / 2.0 {
        for k in 0..g.len() {
            if free[k] && N8.iter().any(|&d| g.step(k, d).is_some_and(|n| outside[n] && !blocked[n])) {
                exits[k] = true;
            }
        }
    }

    for (_, outer) in &entry_ground {
        for &k in outer {
            if free[k] {
                exits[k] = true;
            }
        }
    }

    let near_exit = g.dilate(&exits, 8.0);
    let mut skel = g.thin(&free);
    prune(&g, &mut skel, &near_exit, 6);

    // ---- 3. Node seeds.
    let mut seeds: Vec<Seed> = vec![];
    let skel_cells: Vec<usize> = (0..g.len()).filter(|&k| skel[k]).collect();
    if skel_cells.is_empty() {
        out.warnings.push("no free space inside the perimeter".to_owned());
        return out;
    }

    let snap = |p: P, within: f64| -> Option<usize> { nearest(&g, p, |k| skel[k], within) };
    let gates: Vec<&crate::WallPiece> = input.walls.iter().filter(|w| w.gate).collect();

    // A port's quays: entries on the waterfront inside the town (CA's walled ports have them ~16 units from the water), a few, apart.
    if port {
        let to_sea = g.distance(&sea);
        let mut quay: Vec<usize> = skel_cells.iter().copied().filter(|&k| to_sea[k] <= QUAY_REACH).collect();
        quay.sort_by(|&a, &b| to_sea[a].total_cmp(&to_sea[b]));
        let mut placed = 0;
        for k in quay {
            if placed < MAX_QUAY_ENTRIES && push_seed(&mut seeds, &g, k, NodeKind::Entry, QUAY_APART) {
                placed += 1;
            }
        }
    }

    // An entry outside each palisade opening.
    for (c, _) in &entry_ground {
        match snap(*c, 16.0) {
            Some(k) => {
                push_seed(&mut seeds, &g, k, NodeKind::Entry, 30.0);
            }
            None => {
                out.warnings.push(format!("no way through the palisade opening at ({:.0}, {:.0})", c[0], c[1]));
            }
        }
    }

    // Captures (the gate ones are the gates' areas).
    let mut captures = input.captures.clone();
    captures.sort_by_key(|c| !c.major);
    for c in &captures {
        let at_gate = gates.iter().any(|w| crate::raster::seg_dist(c.at, w.ends[0], w.ends[1]) < 30.0);
        let in_free = nearest(&g, c.at, |k| free[k], if c.major { 80.0 } else { 15.0 }).is_some();
        if at_gate || !in_free {
            continue;
        }

        if let Some(k) = snap(c.at, if c.major { 100.0 } else { 60.0 }) {
            push_seed(&mut seeds, &g, k, NodeKind::Area, 30.0);
        }
    }

    // Gates: on the city side.
    for w in &gates {
        let mid = [(w.ends[0][0] + w.ends[1][0]) / 2.0, (w.ends[0][1] + w.ends[1][1]) / 2.0];
        let (dx, dz) = (w.ends[1][0] - w.ends[0][0], w.ends[1][1] - w.ends[0][1]);
        let l = dx.hypot(dz).max(1e-6);
        let n = [-dz / l, dx / l];
        // The side towards the inside (from an inner gate, either).
        let inside_at = |s: f64, d: f64| g.at([mid[0] + s * n[0] * d, mid[1] + s * n[1] * d]).is_some_and(|k| interior[k]);
        let inward = |s: f64| (10..=40).step_by(5).any(|d| inside_at(s, f64::from(d)));
        let side = [1.0, -1.0].into_iter().find(|&s| inward(s) && !inward(-s)).or_else(|| [1.0, -1.0].into_iter().find(|&s| inward(s)));
        let Some(s) = side else {
            out.warnings.push(format!("a gate at ({:.0}, {:.0}) has no city side inside the perimeter", mid[0], mid[1]));
            continue;
        };

        let target = [mid[0] + s * n[0] * 24.0, mid[1] + s * n[1] * 24.0];
        match snap(target, 40.0) {
            Some(k) => {
                push_seed(&mut seeds, &g, k, NodeKind::Area, 25.0);
            }
            None => out.warnings.push(format!("no street behind the gate at ({:.0}, {:.0})", mid[0], mid[1])),
        }
    }

    // Plazas, widest first: wide spots, wider than the streets around them.
    let mut wide: Vec<usize> = skel_cells.iter().copied().filter(|&k| clearance[k] >= PLAZA_CLEARANCE && clearance[k] >= 1.5 * street_width_near(&g, &skel, &clearance, k)).collect();
    wide.sort_by(|&a, &b| clearance[b].total_cmp(&clearance[a]));
    for k in wide {
        let r = (2.0 * clearance[k]).max(70.0);
        push_seed(&mut seeds, &g, k, NodeKind::Area, r);
    }

    // Dead ends and exits.
    for &k in &skel_cells {
        if neighbours(&g, &skel, k) != 1 {
            continue;
        }

        if near_exit[k] {
            // Back from the edge along the skeleton.
            let k = walk_back(&g, &skel, k, 6);
            push_seed(&mut seeds, &g, k, NodeKind::Entry, 40.0);
        } else {
            push_seed(&mut seeds, &g, k, NodeKind::Area, 40.0);
        }
    }

    // Junctions no area covers.
    for &k in &skel_cells {
        if crossings(&g, &skel, k) >= 3 {
            push_seed(&mut seeds, &g, k, NodeKind::Intersection, 26.0);
        }
    }

    // ---- 4. Regions, streets, area connections (more nodes where a street joins three nodes or is too long).
    let mut split = None;
    let mut comps = vec![];
    for round in 0..6 {
        let sources: Vec<(usize, f64)> = seeds.iter().map(|s| (s.cell, reach(s.kind, clearance[s.cell]))).collect();
        let (owner, _) = g.geodesic_split(&free, &sources);
        comps = street_pieces(&g, &skel, &owner);
        let mut added = false;
        for c in &comps {
            if c.touches.len() > 2 && round < 5 {
                let k = c.cells.iter().copied().find(|&k| crossings(&g, &skel, k) >= 3).unwrap_or(c.cells[c.cells.len() / 2]);
                added |= push_seed(&mut seeds, &g, k, NodeKind::Intersection, 0.0);
            } else if c.touches.len() == 2 && c.cells.len() as f64 * CELL > MAX_STREET && round < 5 {
                let order = order_piece(&g, &skel, &c.cells, &owner, c.touches[0]);
                let n = (order.len() as f64 * CELL / MAX_STREET).floor() as usize;
                for i in 1..=n {
                    added |= push_seed(&mut seeds, &g, order[i * order.len() / (n + 1)], NodeKind::Area, 0.0);
                }
            }
        }

        split = Some(owner);
        if !added {
            break;
        }
    }

    let owner = split.unwrap_or_default();

    // ---- 5. Shapes.
    let mut cells_of: Vec<Vec<usize>> = vec![vec![]; seeds.len()];
    for (k, &o) in owner.iter().enumerate() {
        if o != usize::MAX {
            cells_of[o].push(k);
        }
    }

    let mut boxes: Vec<Rect> = seeds.iter().enumerate().map(|(i, s)| {
        let mine: Vec<P> = cells_of[i].iter().copied().filter(|&k| skel[k]).map(|k| g.centre(k)).collect();
        grow_rect(&g, &free, &owner, &clearance, i, s, &mine)
    }).collect();
    separate(&mut boxes);
    for (i, s) in seeds.iter().enumerate() {
        let mut cells = cells_of[i].clone();
        let r = &boxes[i];
        cells.extend(g.polygon_cells(&r.corners()));
        let boundary = local_outline(&g, &cells, 3.0);
        out.nodes.push(Node { kind: s.kind, centre: r.c, bbox: r.corners(), boundary });
    }

    let mut joined: BTreeSet<(usize, usize)> = BTreeSet::new();
    for c in comps.iter().filter(|c| c.touches.len() == 2) {
        let (a, b) = (c.touches[0], c.touches[1]);
        if !joined.insert((a.min(b), a.max(b))) {
            continue;
        }

        let set: BTreeSet<usize> = c.cells.iter().copied().collect();
        let ok = |k: usize| skel[k] && (set.contains(&k) || owner[k] == a || owner[k] == b);
        if let Some(e) = bfs_path(&g, seeds[a].cell, ok, |k| k == seeds[b].cell).and_then(|path| street(&g, &free, &owner, &boxes, &out.nodes, a, b, &path)) {
            out.edges.push(e);
        }
    }

    // Touching regions (a shared border of 8 units at least) not joined by a street: an area connection when their rectangles are
    // close (CA's: ~20 units), else a street through the two regions.
    let mut touching: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for k in 0..g.len() {
        let a = owner[k];
        if a == usize::MAX {
            continue;
        }

        for d in [(1, 0), (0, 1)] {
            if let Some(n) = g.step(k, d) {
                let b = owner[n];
                if b != usize::MAX && b != a {
                    *touching.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
        }
    }

    for (&(a, b), &n) in &touching {
        if (n as f64) * CELL < 8.0 || joined.contains(&(a, b)) {
            continue;
        }

        let (ra, rb) = (&boxes[a], &boxes[b]);
        let (qa, qb) = (ra.towards(rb.c), rb.towards(ra.c));
        if dist(qa, qb) <= MAX_CONNECTION {
            joined.insert((a, b));
            out.edges.push(Edge { kind: EdgeKind::AreaConnection, line: vec![qa, qb], sides: vec![], nodes: Some((a, b)) });
            continue;
        }

        let ok = |k: usize| free[k] && (owner[k] == a || owner[k] == b);
        if let Some(e) = bfs_path(&g, seeds[a].cell, ok, |k| k == seeds[b].cell).and_then(|path| street(&g, &free, &owner, &boxes, &out.nodes, a, b, &path)) {
            joined.insert((a, b));
            out.edges.push(e);
        }
    }

    // Parts of the graph joined: the nearest two nodes of two parts by a street through the free space and the gates (inner walls split
    // a town into parts joined by their gates), while that helps.
    let mut gate_cells = g.mask();
    for (w, cells) in input.walls.iter().zip(&wall_cells) {
        if w.gate {
            for &c in cells {
                gate_cells[c] = true;
            }
        }
    }

    let passable: Mask = (0..g.len()).map(|k| free[k] || gate_cells[k]).collect();

    for _ in 0..out.nodes.len() {
        let part = graph_labels(out.nodes.len(), &joined);
        let mut best: Option<(f64, usize, usize)> = None;
        for a in 0..out.nodes.len() {
            for b in a + 1..out.nodes.len() {
                let d = dist(out.nodes[a].centre, out.nodes[b].centre);
                if part[a] != part[b] && best.is_none_or(|x| d < x.0) {
                    best = Some((d, a, b));
                }
            }
        }

        let Some((_, a, b)) = best else { break };
        let path = bfs_path(&g, seeds[a].cell, |k| passable[k], |k| k == seeds[b].cell);
        match path.and_then(|path| street(&g, &passable, &owner, &boxes, &out.nodes, a, b, &path)) {
            Some(e) => {
                joined.insert((a.min(b), a.max(b)));
                out.edges.push(e);
            }
            None => break,
        }
    }

    let parts = graph_parts(out.nodes.len(), &joined);
    if parts > 1 {
        out.warnings.push(format!("the street graph is in {parts} parts: some nodes cannot be reached from the others along the free ground"));
    }

    // ---- 6. Wall edges.
    for run in wall_runs(input, &g, &nogo, &region) {
        out.edges.push(Edge { kind: EdgeKind::Wall, line: run, sides: vec![], nodes: None });
    }

    out.separators = separators(&out.perimeter, &out.edges.iter().filter(|e| e.kind == EdgeKind::Wall).map(|e| e.line.clone()).collect::<Vec<_>>());

    // The ring closed: walls, separators and no-go regions keep the outside from the town (as in CA's tiles but one).
    let wall_lines: Vec<&Vec<P>> = out.edges.iter().filter(|e| e.kind == EdgeKind::Wall).map(|e| &e.line).collect();
    if !out.separators.is_empty() {
        let mut ring = nogo.clone();
        for l in wall_lines.into_iter().chain(&out.separators) {
            for c in line_cells(&g, l, 2.0) {
                ring[c] = true;
            }
        }

        let pass: Mask = ring.iter().map(|&r| !r).collect();
        let reached = g.flood(&pass, (0..g.len()).filter(|&k| g.on_border(k)));
        if reached[seed] {
            // Where: the stretches of the perimeter the outside comes through.
            let mut lp = out.perimeter.clone();
            lp.push(lp[0]);
            let mut at: Vec<P> = vec![];
            for p in resample(&lp, 5.0) {
                if g.at(p).is_some_and(|k| reached[k]) && at.iter().all(|&q| dist(p, q) > 60.0) {
                    at.push(p);
                }
            }

            let list = at.iter().take(4).map(|p| format!("({:.0}, {:.0})", p[0], p[1])).collect::<Vec<_>>().join(", ");
            out.warnings.push(format!("the walls, separators and no-go regions leave the ring around the town open (the perimeter near {list})"));
        }
    }

    for r in runs.iter().filter(|r| r.pieces.len() >= 2) {
        out.edges.push(Edge { kind: EdgeKind::Palisade, line: simplify(&r.line(), 0.5), sides: vec![], nodes: None });
    }

    if input.walls.iter().any(|w| !w.platforms.is_empty()) && !out.edges.iter().any(|e| e.kind == EdgeKind::Wall) {
        out.warnings.push("the settlement has fort pieces but none with a walkway along the perimeter: no wall edges".to_owned());
    }

    // ---- 7. Deployment zones and approach lines.
    let town = g.centre(seed);
    let zones = deployment(&mut out, &g, &region, &dout, &sea, port, input.bounds, seed);
    approaches(&mut out, &g, &zones.attacker, &nogo, input.bounds, town);
    if port && let Some(heights) = &input.sea {
        let land: Mask = sea.iter().map(|&s| !s).collect();
        let (to_land, to_sea) = (g.distance(&land), g.distance(&sea));
        let lobe: Mask = (0..g.len()).map(|k| sea[k] && zones.defender[k]).collect();
        out.deployment_lines = crate::ports::deployment_lines(&g, &lobe, &to_land, town);
        (out.beaches, out.beach_approaches) = crate::ports::beaches(&g, &sea, &nogo, &dout, heights, &to_sea, town);
        if out.deployment_lines.is_empty() {
            out.warnings.push("a port without room for the defender's deployment lines at sea".to_owned());
        }
    }

    out
}

/// The AI separators that close the ring where the walls don't (CA's: walls, separators and no-go regions close it in 47 of 48 walled
/// tiles): each stretch of the perimeter further than `SEPARATOR_WALL` from every wall edge, from the wall line at one end to the wall
/// line at the other. None without walls, or when the walls run along the whole perimeter.
fn separators(perimeter: &[P], walls: &[Vec<P>]) -> Vec<Vec<P>> {
    if walls.is_empty() || perimeter.len() < 3 {
        return vec![];
    }

    let mut ring = perimeter.to_vec();
    ring.push(perimeter[0]);
    let mut samples = resample(&ring, 5.0);
    samples.pop();
    // By a wall: near a wall line, beside it (not off a run's end: the ring goes on past the end, a separator closes it).
    let beside_wall = |p: P| {
        let mut best = (f64::MAX, false);
        for w in walls {
            let last = w.len() - 1;
            for (i, s) in w.windows(2).enumerate() {
                let (a, b) = (s[0], s[1]);
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let l2 = dx * dx + dz * dz;
                let t = if l2 > 0.0 { ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2 } else { 0.0 };
                let off_end = (i == 0 && t < 0.0) || (i + 1 == last && t > 1.0);
                let d = crate::raster::seg_dist(p, a, b);
                if d < best.0 {
                    best = (d, off_end);
                }
            }
        }

        best.0 <= SEPARATOR_WALL && !best.1
    };
    let mut near: Vec<bool> = samples.iter().map(|&p| beside_wall(p)).collect();
    let n = samples.len();
    // Short stretches away from the walls are the walls' own bends (closed by them); then short wall bits between separators go.
    flip_short(&mut near, false, 6);
    flip_short(&mut near, true, 4);
    // A town whose walls are not its outline (a minor town's citadel) has none, as CA's.
    if (near.iter().filter(|&&x| x).count() as f64) < SEPARATOR_MIN_WALLED * n as f64 {
        return vec![];
    }

    let Some(start) = near.iter().position(|&n| n) else { return vec![] };

    // The nearest point on the wall lines.
    let onto_wall = |p: P| {
        let mut best = (f64::MAX, p);
        for s in walls.iter().flat_map(|w| w.windows(2)) {
            let (a, b) = (s[0], s[1]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let q = [a[0] + t * dx, a[1] + t * dz];
            if dist(p, q) < best.0 {
                best = (dist(p, q), q);
            }
        }

        best.1
    };

    let mut out = vec![];
    let mut i = 0;
    while i < n {
        let k = (start + i) % n;
        if near[k] {
            i += 1;
            continue;
        }

        // A stretch away from the walls: from the last sample by a wall to the next one.
        let mut line = vec![onto_wall(samples[(k + n - 1) % n])];
        while i < n && !near[(start + i) % n] {
            line.push(samples[(start + i) % n]);
            i += 1;
        }

        line.push(onto_wall(samples[(start + i) % n]));
        if line.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>() >= 10.0 {
            out.push(simplify(&line, 3.0));
        }
    }

    out
}

/// Runs of `value` shorter than `max` in a ring of flags flipped.
fn flip_short(flags: &mut [bool], value: bool, max: usize) {
    let n = flags.len();
    let Some(start) = flags.iter().position(|&f| f != value) else { return };
    let mut i = 0;
    while i < n {
        let k = (start + i) % n;
        if flags[k] != value {
            i += 1;
            continue;
        }

        let run: Vec<usize> = (i..n).map(|j| (start + j) % n).take_while(|&j| flags[j] == value).collect();
        i += run.len();
        if run.len() < max {
            for j in run {
                flags[j] = !value;
            }
        }
    }
}

/// Points along the line at most `step` apart (its own points kept).
fn resample(line: &[P], step: f64) -> Vec<P> {
    let mut out = vec![];
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = (dist(a, b) / step).ceil().max(1.0) as usize;
        for i in 0..n {
            let t = i as f64 / n as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }

    if let Some(&l) = line.last() {
        out.push(l);
    }

    out
}

/// The cells within `r` of a polyline.
fn line_cells(g: &Grid, line: &[P], r: f64) -> Vec<usize> {
    let mut out = vec![];
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (k0, k1) = (g.clamp([a[0].min(b[0]) - r, a[1].min(b[1]) - r]), g.clamp([a[0].max(b[0]) + r, a[1].max(b[1]) + r]));
        for j in k0 / g.w..=k1 / g.w {
            for i in k0 % g.w..=k1 % g.w {
                let k = j * g.w + i;
                if crate::raster::seg_dist(g.centre(k), a, b) <= r {
                    out.push(k);
                }
            }
        }
    }

    out
}

/// The settlement's free inside: the open pocket around the seed, and the pockets inner walls split from it: those holding one of
/// `with` (capture points), and those across a wall or a rock (`POCKET_GAP`) from what is in, while there are more.
fn pockets(g: &Grid, raw: &Mask, outside: &Mask, seed: usize, with: &[usize]) -> Mask {
    let not_out: Mask = (0..g.len()).map(|k| !outside[k] && !raw[k]).collect();
    let (pocket, sizes) = g.components(&not_out);
    let mut taken = vec![false; sizes.len()];
    if pocket[seed] == usize::MAX {
        return g.mask();
    }

    taken[pocket[seed]] = true;
    for &k in with {
        if pocket[k] != usize::MAX {
            taken[pocket[k]] = true;
        }
    }

    loop {
        let now: Mask = pocket.iter().map(|&p| p != usize::MAX && taken[p]).collect();
        let near = g.dilate(&now, POCKET_GAP);
        let mut more = false;
        for k in 0..g.len() {
            let p = pocket[k];
            if p != usize::MAX && !taken[p] && near[k] && (sizes[p] as f64) * g.cell * g.cell >= 2000.0 {
                taken[p] = true;
                more = true;
            }
        }

        if !more {
            return pocket.iter().map(|&p| p != usize::MAX && taken[p]).collect();
        }
    }
}

/// Where the settlement is: the main capture point, else any, else the middle of the walls, else the map's.
fn settlement_seed(input: &Input) -> P {
    if let Some(c) = input.captures.iter().find(|c| c.major).or(input.captures.first()) {
        return c.at;
    }

    let pts: Vec<P> = input.walls.iter().flat_map(|w| w.ends).collect();
    if !pts.is_empty() {
        let n = pts.len() as f64;
        return [pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[1]).sum::<f64>() / n];
    }

    let (lo, hi) = input.bounds;
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
}

/// How wide the streets around cell k are: the lower quarter of the skeleton's clearance within 60 units.
fn street_width_near(g: &Grid, skel: &Mask, clearance: &[f64], k: usize) -> f64 {
    let r = (60.0 / g.cell) as isize;
    let mut v = vec![];
    for dj in (-r..=r).step_by(2) {
        for di in (-r..=r).step_by(2) {
            if let Some(n) = g.step(k, (di, dj))
                && skel[n]
            {
                v.push(clearance[n]);
            }
        }
    }

    if v.is_empty() {
        return 0.0;
    }

    v.sort_by(f64::total_cmp);
    v[v.len() / 4]
}

/// The cell nearest `p` (within `within`) for which `ok` holds.
fn nearest(g: &Grid, p: P, ok: impl Fn(usize) -> bool, within: f64) -> Option<usize> {
    let c = g.clamp(p);
    let r = (within / g.cell).ceil() as isize;
    let mut best: Option<(f64, usize)> = None;
    for dj in -r..=r {
        for di in -r..=r {
            let Some(k) = g.step(c, (di, dj)) else { continue };
            if !ok(k) {
                continue;
            }

            let d = dist(g.centre(k), p);
            if d <= within && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, k));
            }
        }
    }

    best.map(|(_, k)| k)
}

/// The largest outer outline of the mask, simplified.
fn outer_loop(g: &Grid, m: &Mask, tol: f64) -> Vec<P> {
    let filled = g.fill_holes(m);
    g.contours(&filled).into_iter().max_by(|a, b| area2(a).abs().total_cmp(&area2(b).abs())).map(|l| simplify_loop(&l, tol)).unwrap_or_default()
}

/// Unset parts enclosed by set cells and smaller than `max_area` are set.
fn fill_small_holes(g: &Grid, m: &mut Mask, max_area: f64) {
    let filled = g.fill_holes(m);
    let holes: Mask = (0..g.len()).map(|k| filled[k] && !m[k]).collect();
    let (label, sizes) = g.components(&holes);
    for k in 0..g.len() {
        if label[k] != usize::MAX && sizes[label[k]] as f64 * g.cell * g.cell < max_area {
            m[k] = true;
        }
    }
}

/// Set parts smaller than `min_area` are unset, but for the one holding `keep`.
fn keep_large_parts(g: &Grid, m: &mut Mask, min_area: f64, keep: usize) {
    let (label, sizes) = g.components(m);
    for k in 0..g.len() {
        if label[k] != usize::MAX && label[k] != label[keep] && (sizes[label[k]] as f64) * g.cell * g.cell < min_area {
            m[k] = false;
        }
    }
}

fn neighbours(g: &Grid, m: &Mask, k: usize) -> usize {
    N8.iter().filter(|&&d| g.step(k, d).is_some_and(|n| m[n])).count()
}

/// The number of separate runs of set cells around k: 1 at an end, 2 along a line, 3 or more at a junction.
fn crossings(g: &Grid, m: &Mask, k: usize) -> usize {
    let s: Vec<bool> = N8.iter().map(|&d| g.step(k, d).is_some_and(|n| m[n])).collect();
    let runs = (0..8).filter(|&i| !s[i] && s[(i + 1) % 8]).count();
    if runs == 0 && s.iter().any(|&x| x) { 1 } else { runs }
}

/// Short branches removed: an end not near an exit is taken off, `rounds` times, then lines left without a junction and shorter than
/// 20 units go.
fn prune(g: &Grid, skel: &mut Mask, keep: &Mask, rounds: usize) {
    for _ in 0..rounds {
        let ends: Vec<usize> = (0..g.len()).filter(|&k| skel[k] && !keep[k] && neighbours(g, skel, k) <= 1).collect();
        if ends.is_empty() {
            break;
        }

        for k in ends {
            skel[k] = false;
        }
    }

    let (label, sizes) = g.components(skel);
    for k in 0..g.len() {
        if label[k] != usize::MAX && (sizes[label[k]] as f64) * g.cell < 20.0 {
            skel[k] = false;
        }
    }
}

/// From an end, `n` cells back along the line.
fn walk_back(g: &Grid, skel: &Mask, mut k: usize, n: usize) -> usize {
    let mut prev = usize::MAX;
    for _ in 0..n {
        let next = N8.iter().filter_map(|&d| g.step(k, d)).find(|&c| skel[c] && c != prev);
        let Some(c) = next else { break };
        if crossings(g, skel, c) >= 3 {
            break;
        }

        prev = k;
        k = c;
    }

    k
}

#[derive(Clone, Copy, Debug)]
struct Seed {
    cell: usize,
    kind: NodeKind,
}

/// A node at cell k unless one is within `apart` (0: always). Returns whether it was added.
fn push_seed(seeds: &mut Vec<Seed>, g: &Grid, k: usize, kind: NodeKind, apart: f64) -> bool {
    let p = g.centre(k);
    if seeds.iter().any(|s| s.cell == k || (apart > 0.0 && dist(g.centre(s.cell), p) < apart)) {
        return false;
    }

    seeds.push(Seed { cell: k, kind });
    true
}

/// How far along the ground a node's region reaches.
fn reach(kind: NodeKind, clearance: f64) -> f64 {
    match kind {
        NodeKind::Area => (clearance * 1.3 + 20.0).clamp(30.0, 60.0),
        NodeKind::Intersection => (clearance + 10.0).clamp(18.0, 32.0),
        NodeKind::Entry => 24.0,
    }
}

/// A piece of the skeleton outside every region: its cells and the nodes whose regions it touches.
struct Piece {
    cells: Vec<usize>,
    touches: Vec<usize>,
}

fn street_pieces(g: &Grid, skel: &Mask, owner: &[usize]) -> Vec<Piece> {
    let open: Mask = (0..g.len()).map(|k| skel[k] && owner[k] == usize::MAX).collect();
    let (label, sizes) = g.components(&open);
    let mut pieces: Vec<Piece> = sizes.iter().map(|_| Piece { cells: vec![], touches: vec![] }).collect();
    for (k, &l) in label.iter().enumerate() {
        if l == usize::MAX {
            continue;
        }

        pieces[l].cells.push(k);
        for &d in &N8 {
            if let Some(n) = g.step(k, d)
                && skel[n] && owner[n] != usize::MAX && !pieces[l].touches.contains(&owner[n])
            {
                pieces[l].touches.push(owner[n]);
            }
        }
    }

    // Two nodes whose regions meet on the skeleton touch through a piece of no cells.
    pieces.retain(|p| !p.touches.is_empty());
    pieces
}

/// A piece's cells in order from the end touching node `from`.
fn order_piece(g: &Grid, skel: &Mask, cells: &[usize], owner: &[usize], from: usize) -> Vec<usize> {
    let set: BTreeSet<usize> = cells.iter().copied().collect();
    let start = cells.iter().copied().find(|&k| N8.iter().any(|&d| g.step(k, d).is_some_and(|n| skel[n] && owner[n] == from))).unwrap_or(cells[0]);
    let far = farthest(g, start, |k| set.contains(&k));
    bfs_path(g, start, |k| set.contains(&k), |k| k == far).unwrap_or_else(|| cells.to_vec())
}

/// The cell farthest (in steps) from `start` through the cells where `ok` holds.
fn farthest(g: &Grid, start: usize, ok: impl Fn(usize) -> bool) -> usize {
    let mut seen = HashMap::from([(start, 0usize)]);
    let mut q = VecDeque::from([start]);
    let mut last = start;
    while let Some(k) = q.pop_front() {
        last = k;
        for &d in &N8 {
            if let Some(n) = g.step(k, d)
                && ok(n) && !seen.contains_key(&n)
            {
                seen.insert(n, seen[&k] + 1);
                q.push_back(n);
            }
        }
    }

    last
}

/// The shortest 8-connected path from `start` to a cell where `goal` holds, through cells where `ok` holds.
fn bfs_path(g: &Grid, start: usize, ok: impl Fn(usize) -> bool, goal: impl Fn(usize) -> bool) -> Option<Vec<usize>> {
    let mut prev = HashMap::from([(start, usize::MAX)]);
    let mut q = VecDeque::from([start]);
    while let Some(k) = q.pop_front() {
        if goal(k) {
            let mut path = vec![k];
            let mut c = k;
            while prev[&c] != usize::MAX {
                c = prev[&c];
                path.push(c);
            }

            path.reverse();
            return Some(path);
        }

        // 4-neighbours first: straighter paths.
        for &d in N4.iter().chain(N8.iter().filter(|d| d.0 != 0 && d.1 != 0)) {
            if let Some(n) = g.step(k, d)
                && ok(n) && !prev.contains_key(&n)
            {
                prev.insert(n, k);
                q.push_back(n);
            }
        }
    }

    None
}

/// An oriented rectangle: centre, unit axis, half extents along the axis and across.
#[derive(Clone, Copy, Debug)]
struct Rect {
    c: P,
    u: P,
    hx: f64,
    hz: f64,
}

impl Rect {
    fn v(&self) -> P {
        [-self.u[1], self.u[0]]
    }

    fn point(&self, a: f64, b: f64) -> P {
        let v = self.v();
        [self.c[0] + self.u[0] * a + v[0] * b, self.c[1] + self.u[1] * a + v[1] * b]
    }

    /// The corners in order around it.
    fn corners(&self) -> [P; 4] {
        [self.point(-self.hx, -self.hz), self.point(self.hx, -self.hz), self.point(self.hx, self.hz), self.point(-self.hx, self.hz)]
    }

    fn local(&self, p: P) -> (f64, f64) {
        let d = [p[0] - self.c[0], p[1] - self.c[1]];
        let v = self.v();
        (d[0] * self.u[0] + d[1] * self.u[1], d[0] * v[0] + d[1] * v[1])
    }

    /// Inside, `margin` in from the sides.
    fn holds(&self, p: P, margin: f64) -> bool {
        let (a, b) = self.local(p);
        a.abs() <= self.hx - margin && b.abs() <= self.hz - margin
    }

    /// A point inside towards `p`: half way from the centre to the side (CA's edges end so: ~0.5 of the half diagonal).
    fn towards(&self, p: P) -> P {
        let d = [p[0] - self.c[0], p[1] - self.c[1]];
        let l = d[0].hypot(d[1]).max(1e-9);
        let r = 0.5 * self.hx.min(self.hz);
        [self.c[0] + d[0] / l * r, self.c[1] + d[1] / l * r]
    }
}

/// A node's rectangle: grown in its own free space at the angle that gives the largest (its streets' axis, or the grid's: the
/// rectangle that fits best), intersections square.
fn grow_rect(g: &Grid, free: &Mask, owner: &[usize], clearance: &[f64], i: usize, s: &Seed, skel_mine: &[P]) -> Rect {
    let c = g.centre(s.cell);
    let pca = principal_axis(skel_mine).map_or(0.0, |u| u[1].atan2(u[0]));
    let angles = [pca, pca + std::f64::consts::FRAC_PI_4, 0.0, std::f64::consts::FRAC_PI_8, std::f64::consts::FRAC_PI_4, 3.0 * std::f64::consts::FRAC_PI_8];
    let (square, max) = match s.kind {
        NodeKind::Intersection => (true, (0.9 * clearance[s.cell]).clamp(5.0, 12.0)),
        NodeKind::Entry => (false, 16.0),
        NodeKind::Area => (false, 40.0),
    };

    let fits = |r: &Rect| {
        let (mut n, mut ok) = (0usize, 0usize);
        let (sa, sb) = ((r.hx / 2.0).ceil() as i32, (r.hz / 2.0).ceil() as i32);
        for a in -sa..=sa {
            for b in -sb..=sb {
                let p = r.point(f64::from(a) / f64::from(sa.max(1)) * r.hx, f64::from(b) / f64::from(sb.max(1)) * r.hz);
                n += 1;
                if g.at(p).is_some_and(|k| free[k] && (owner[k] == i || owner[k] == usize::MAX)) {
                    ok += 1;
                }
            }
        }

        ok as f64 >= 0.95 * n as f64
    };

    let mut best: Option<Rect> = None;
    for a in angles {
        let u = [a.cos(), a.sin()];
        let mut r = Rect { c, u, hx: 4.0, hz: 4.0 };
        let (mut grow_x, mut grow_z) = (true, true);
        while grow_x || grow_z {
            if square {
                let t = Rect { hx: r.hx + 1.0, hz: r.hz + 1.0, ..r };
                if t.hx <= max && fits(&t) { r = t } else { break }
                continue;
            }

            if grow_x {
                let t = Rect { hx: r.hx + 1.0, ..r };
                if t.hx <= max && fits(&t) { r = t } else { grow_x = false }
            }

            if grow_z {
                let t = Rect { hz: r.hz + 1.0, ..r };
                if t.hz <= max && fits(&t) { r = t } else { grow_z = false }
            }
        }

        if best.is_none_or(|b| r.hx * r.hz > b.hx * b.hz + 1e-9) {
            best = Some(r);
        }
    }

    best.unwrap_or(Rect { c, u: [1.0, 0.0], hx: 4.0, hz: 4.0 })
}

fn principal_axis(pts: &[P]) -> Option<P> {
    if pts.len() < 3 {
        return None;
    }

    let n = pts.len() as f64;
    let m = [pts.iter().map(|p| p[0]).sum::<f64>() / n, pts.iter().map(|p| p[1]).sum::<f64>() / n];
    let (mut sxx, mut sxz, mut szz) = (0.0, 0.0, 0.0);
    for p in pts {
        let (x, z) = (p[0] - m[0], p[1] - m[1]);
        sxx += x * x;
        sxz += x * z;
        szz += z * z;
    }

    let a = 0.5 * (2.0 * sxz).atan2(sxx - szz);
    Some([a.cos(), a.sin()])
}

/// Rectangles that overlap shrunk (the larger one) until none does: an edge's end in two rectangles goes to the last one the game reads.
fn separate(rects: &mut [Rect]) {
    for _ in 0..40 {
        let mut any = false;
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                if overlap(&rects[i], &rects[j]) {
                    any = true;
                    let k = if rects[i].hx * rects[i].hz >= rects[j].hx * rects[j].hz { i } else { j };
                    rects[k].hx = (rects[k].hx * 0.85).max(2.0);
                    rects[k].hz = (rects[k].hz * 0.85).max(2.0);
                }
            }
        }

        if !any {
            return;
        }
    }
}

/// Separating axes.
fn overlap(a: &Rect, b: &Rect) -> bool {
    let (ca, cb) = (a.corners(), b.corners());
    for axis in [a.u, a.v(), b.u, b.v()] {
        let proj = |cs: &[P; 4]| cs.iter().map(|p| p[0] * axis[0] + p[1] * axis[1]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (pa, pb) = (proj(&ca), proj(&cb));
        if pa.1 < pb.0 || pb.1 < pa.0 {
            return false;
        }
    }

    true
}

/// The outline of a set of cells grown by `grow`: its largest outer loop, simplified. Worked on a window around the cells.
fn local_outline(g: &Grid, cells: &[usize], grow: f64) -> Vec<P> {
    if cells.is_empty() {
        return vec![];
    }

    let (mut i0, mut j0, mut i1, mut j1) = (usize::MAX, usize::MAX, 0, 0);
    for &k in cells {
        let (i, j) = (k % g.w, k / g.w);
        (i0, j0, i1, j1) = (i0.min(i), j0.min(j), i1.max(i), j1.max(j));
    }

    let m = (grow / g.cell).ceil() as usize + 3;
    let (i0, j0) = (i0.saturating_sub(m), j0.saturating_sub(m));
    let (i1, j1) = ((i1 + m).min(g.w - 1), (j1 + m).min(g.h - 1));
    let lg = Grid { x0: g.x0 + i0 as f64 * g.cell, z0: g.z0 + j0 as f64 * g.cell, cell: g.cell, w: i1 - i0 + 1, h: j1 - j0 + 1 };
    let mut lm = lg.mask();
    for &k in cells {
        let (i, j) = (k % g.w, k / g.w);
        if (i0..=i1).contains(&i) && (j0..=j1).contains(&j) {
            lm[(j - j0) * lg.w + (i - i0)] = true;
        }
    }

    let grown = lg.dilate(&lm, grow);
    outer_loop(&lg, &grown, 1.5)
}

/// A street from node a to node b along `path` (cells from a's seed to b's).
#[allow(clippy::too_many_arguments)]
fn street(g: &Grid, free: &Mask, owner: &[usize], boxes: &[Rect], nodes: &[Node], a: usize, b: usize, path: &[usize]) -> Option<Edge> {
    let pts: Vec<P> = path.iter().map(|&k| g.centre(k)).collect();

    // The line: from deep enough in a's rectangle to deep enough in b's.
    let (ra, rb) = (&boxes[a], &boxes[b]);
    let deep = |r: &Rect| (0.3 * r.hx.min(r.hz)).min(4.0);
    let ka = pts.iter().rposition(|&p| ra.holds(p, deep(ra))).unwrap_or(0);
    let kb = pts.iter().position(|&p| rb.holds(p, deep(rb))).unwrap_or(pts.len() - 1);
    if kb <= ka {
        return None;
    }

    let mut line = simplify(&pts[ka..=kb], 1.0);
    if line.len() < 2 {
        line = vec![pts[ka], pts[kb]];
    }

    // The corridor: where the path is in neither region, a little into each.
    let ia = path.iter().rposition(|&k| owner[k] == a).unwrap_or(0);
    let ib = path.iter().position(|&k| owner[k] == b).unwrap_or(path.len() - 1);
    let (ia, ib) = (ia.saturating_sub(2), (ib + 2).min(path.len() - 1));
    let corridor = if ib > ia + 1 { &pts[ia..=ib] } else { &pts[..] };
    let mut sides = vec![vec![], vec![]];
    let step = 3usize;
    let mut i = 0;
    while i < corridor.len() {
        let (p0, p1) = (corridor[i.saturating_sub(3)], corridor[(i + 3).min(corridor.len() - 1)]);
        let t = [p1[0] - p0[0], p1[1] - p0[1]];
        let l = t[0].hypot(t[1]);
        if l > 1e-9 {
            let n = [-t[1] / l, t[0] / l];
            for (s, side) in [1.0, -1.0].into_iter().zip(sides.iter_mut()) {
                side.push(march(g, free, corridor[i], [n[0] * s, n[1] * s], 80.0));
            }
        }

        if i + 1 == corridor.len() {
            break;
        }

        i = (i + step).min(corridor.len() - 1);
    }

    let mut out_sides = vec![];
    for side in sides {
        if side.len() < 2 {
            continue;
        }

        let mut s = simplify(&smooth(&side), 2.0);
        s.insert(0, tick(s[0], &nodes[a]));
        s.push(tick(s[s.len() - 1], &nodes[b]));
        out_sides.push(s);
    }

    if out_sides.len() < 2 {
        return None;
    }

    Some(Edge { kind: EdgeKind::Street, line, sides: out_sides, nodes: Some((a, b)) })
}

/// From p along d through free cells: the last free point (at most `max` away).
fn march(g: &Grid, free: &Mask, p: P, d: P, max: f64) -> P {
    let mut last = p;
    let mut t = 0.0;
    while t <= max {
        let q = [p[0] + d[0] * t, p[1] + d[1] * t];
        if !g.at(q).is_some_and(|k| free[k]) {
            break;
        }

        last = q;
        t += 1.0;
    }

    last
}

/// A side's points averaged with their neighbours (the faces are jagged on the raster).
fn smooth(pts: &[P]) -> Vec<P> {
    (0..pts.len())
        .map(|i| {
            if i == 0 || i + 1 == pts.len() {
                return pts[i];
            }

            let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
            [(a[0] + 2.0 * b[0] + c[0]) / 4.0, (a[1] + 2.0 * b[1] + c[1]) / 4.0]
        })
        .collect()
}

/// A side's end turned into the node: 4 units towards its centre, further until inside its Boundary.
fn tick(end: P, node: &Node) -> P {
    let d = [node.centre[0] - end[0], node.centre[1] - end[1]];
    let l = d[0].hypot(d[1]).max(1e-9);
    let mut t = 4.0f64.min(l);
    loop {
        let q = [end[0] + d[0] / l * t, end[1] + d[1] / l * t];
        if node.boundary.len() < 3 || inside(q, &node.boundary) || t >= l {
            return q;
        }

        t = (t + 2.0).min(l);
    }
}

fn graph_parts(n: usize, joined: &BTreeSet<(usize, usize)>) -> usize {
    let l = graph_labels(n, joined);
    (0..n).filter(|&x| l[x] == x).count()
}

/// Each node's part of the graph (a node of it).
fn graph_labels(n: usize, joined: &BTreeSet<(usize, usize)>) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }

        x
    }

    for &(a, b) in joined {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    }

    (0..n).map(|x| find(&mut parent, x)).collect()
}

/// The walls' runs: the middle of the fort pieces' platforms (their union thinned to lines, short spurs off, traced into lines that go
/// straight on at forks), cut where they enter no-go; runs mostly outside the perimeter or shorter than 20 units dropped.
fn wall_runs(input: &Input, g: &Grid, nogo: &Mask, region: &Mask) -> Vec<Vec<P>> {
    // A gate's towers ring its passage with platforms: it goes straight from end to end instead.
    let mut walk = g.mask();
    for w in &input.walls {
        if w.gate {
            // Past its ends a little, into the walkways either side.
            let (a, b) = (w.ends[0], w.ends[1]);
            let l = dist(a, b);
            if l > 1e-6 {
                let u = [(b[0] - a[0]) / l * GATE_OVERLAP, (b[1] - a[1]) / l * GATE_OVERLAP];
                g.fill_line(&mut walk, &[[a[0] - u[0], a[1] - u[1]], [b[0] + u[0], b[1] + u[1]]], 3.0);
            }

            continue;
        }

        for r in &w.platforms {
            g.fill_polygon(&mut walk, r);
        }

        g.fill_line(&mut walk, &w.walkway, 3.0);
    }

    let walk = g.close(&walk, 2.0);
    let mut skel = g.thin(&walk);
    remove_spurs(g, &mut skel, 30.0);
    let mut runs = vec![];
    for line in trace(g, &skel) {
        let samples: Vec<P> = line.iter().map(|&k| g.centre(k)).collect();
        let mut run: Vec<P> = vec![];
        let mut flush = |run: &mut Vec<P>| {
            let len: f64 = run.windows(2).map(|w| dist(w[0], w[1])).sum();
            let inside_share = run.iter().filter(|&&p| g.at(p).is_some_and(|k| region[k])).count() as f64 / run.len().max(1) as f64;
            if len >= 20.0 && inside_share >= 0.5 {
                runs.push(simplify(&smooth(run), 1.0));
            }

            run.clear();
        };

        for p in samples {
            if g.at(p).is_some_and(|k| nogo[k]) {
                flush(&mut run);
            } else {
                run.push(p);
            }
        }

        flush(&mut run);
    }

    runs
}

/// Branches from an end to a fork shorter than `max` taken off the lines.
pub(crate) fn remove_spurs(g: &Grid, skel: &mut Mask, max: f64) {
    for _ in 0..3 {
        let mut changed = false;
        let ends: Vec<usize> = (0..g.len()).filter(|&k| skel[k] && neighbours(g, skel, k) == 1).collect();
        for e in ends {
            if !skel[e] {
                continue;
            }

            let mut path = vec![e];
            let mut prev = usize::MAX;
            let mut k = e;
            let at_fork = loop {
                let next: Vec<usize> = N8.iter().filter_map(|&d| g.step(k, d)).filter(|&c| skel[c] && c != prev && !path.contains(&c)).collect();
                if next.is_empty() {
                    break false;
                }

                if next.iter().any(|&c| crossings(g, skel, c) >= 3) || next.len() > 1 {
                    break true;
                }

                prev = k;
                k = next[0];
                path.push(k);
                if path.len() as f64 * g.cell > max {
                    break false;
                }
            };

            if at_fork && (path.len() as f64) * g.cell <= max {
                for c in path {
                    skel[c] = false;
                }

                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

/// Lines one cell wide traced into polylines of cells: from each end (then around what is left, the loops), going on at a fork the way
/// that turns least; pieces under 6 cells dropped.
pub(crate) fn trace(g: &Grid, skel: &Mask) -> Vec<Vec<usize>> {
    let mut seen = vec![false; g.len()];
    let mut out = vec![];
    let ends: Vec<usize> = (0..g.len()).filter(|&k| skel[k] && neighbours(g, skel, k) == 1).collect();
    let rest: Vec<usize> = (0..g.len()).filter(|&k| skel[k]).collect();
    for s in ends.into_iter().chain(rest) {
        if seen[s] {
            continue;
        }

        let mut line = vec![s];
        seen[s] = true;
        let mut k = s;
        let mut dir = (0.0f64, 0.0f64);
        loop {
            let options: Vec<usize> = N8.iter().filter_map(|&d| g.step(k, d)).filter(|&c| skel[c] && !seen[c]).collect();
            let pick = options.into_iter().max_by(|&a, &b| {
                let score = |c: usize| {
                    let (di, dj) = ((c % g.w) as f64 - (k % g.w) as f64, (c / g.w) as f64 - (k / g.w) as f64);
                    let l = di.hypot(dj);
                    // Straight on first; then the nearer (4-neighbours).
                    (di * dir.0 + dj * dir.1) / l - 0.01 * l
                };
                score(a).total_cmp(&score(b))
            });
            let Some(c) = pick else { break };
            let (di, dj) = ((c % g.w) as f64 - (k % g.w) as f64, (c / g.w) as f64 - (k / g.w) as f64);
            let l = di.hypot(dj);
            dir = (0.7 * dir.0 + 0.3 * di / l, 0.7 * dir.1 + 0.3 * dj / l);
            seen[c] = true;
            line.push(c);
            k = c;
        }

        if line.len() >= 6 {
            out.push(line);
        }
    }

    out
}

/// The deployment zones as masks: where each alliance may deploy.
struct Zones {
    defender: Mask,
    attacker: Mask,
}

/// Both alliances' zones (docs/plans/siege_ai.md, "Deployment zones").
#[allow(clippy::too_many_arguments)]
fn deployment(out: &mut Layout, g: &Grid, region: &Mask, dout: &[f64], sea: &Mask, port: bool, bounds: (P, P), seed: usize) -> Zones {
    let (lo, hi) = bounds;
    let rect = vec![[lo[0] - OVERHANG, lo[1] - OVERHANG], [hi[0] + OVERHANG, lo[1] - OVERHANG], [hi[0] + OVERHANG, hi[1] + OVERHANG], [lo[0] - OVERHANG, hi[1] + OVERHANG]];
    let within = |r: f64| -> Mask { dout.iter().map(|&d| d <= r).collect() };

    // The defender: the perimeter a little in (and the sea off the town, the harbour in it).
    let mut def = g.erode(region, DEFENDER_IN);
    if port {
        let near: Mask = (0..g.len()).map(|k| sea[k] && dout[k] <= DEFENDER_SEA).collect();
        let joined: Mask = (0..g.len()).map(|k| near[k] || def[k] || (sea[k] && dout[k] <= DEFENDER_IN + 2.0 * g.cell)).collect();
        let (label, _) = g.components(&joined);
        def = (0..g.len()).map(|k| def[k] || (near[k] && label[k] == label[seed])).collect();
    }

    let mut d = Deployment::default();
    d.polygons.push((ZoneKind::Additive, outer_loop(g, &def, 3.0)));
    d.polygons.push((ZoneKind::GuerrillaAdditive, outer_loop(g, &within(DEFENDER_GUERRILLA_OUT), 3.0)));
    out.defender = d;

    // The attacker: the whole map but the town and the ground around it; in a port, the land (short of the coast) and the sea away
    // from the defender's water and the shore, as separate polygons.
    let mut a = Deployment::default();
    let attacker: Mask = if port {
        let land: Mask = sea.iter().map(|&s| !s).collect();
        let (to_land, to_sea, to_def) = (g.distance(&land), g.distance(sea), g.distance(&def));
        let on_land: Mask = (0..g.len()).map(|k| !sea[k] && dout[k] > ATTACKER_OUT && to_sea[k] >= ATTACKER_SHORE).collect();
        let at_sea: Mask = (0..g.len()).map(|k| sea[k] && to_def[k] >= ATTACKER_OUT && to_land[k] >= ATTACKER_OFFSHORE).collect();
        let mut zone = g.mask();
        for part in [on_land, at_sea] {
            let (label, sizes) = g.components(&part);
            for (id, &size) in sizes.iter().enumerate() {
                if (size as f64) * g.cell * g.cell < 20_000.0 {
                    continue;
                }

                let piece: Mask = label.iter().map(|&l| l == id).collect();
                a.polygons.push((ZoneKind::Additive, outer_loop(g, &piece, 3.0)));
                // Its holes: the town, when the land goes all around it.
                let filled = g.fill_holes(&piece);
                let holes: Mask = (0..g.len()).map(|k| filled[k] && !piece[k]).collect();
                let (hl, hs) = g.components(&holes);
                for (h, &n) in hs.iter().enumerate() {
                    if (n as f64) * g.cell * g.cell >= 5_000.0 {
                        let hole: Mask = hl.iter().map(|&l| l == h).collect();
                        a.polygons.push((ZoneKind::Subtractive, outer_loop(g, &hole, 3.0)));
                    }
                }

                for (z, &p) in zone.iter_mut().zip(&piece) {
                    *z |= p;
                }
            }
        }

        zone
    } else {
        a.polygons.push((ZoneKind::Additive, rect.clone()));
        a.polygons.push((ZoneKind::Subtractive, outer_loop(g, &within(ATTACKER_OUT), 3.0)));
        dout.iter().map(|&d| d > ATTACKER_OUT).collect()
    };

    a.polygons.push((ZoneKind::GuerrillaAdditive, rect));
    a.polygons.push((ZoneKind::GuerrillaSubtractive, outer_loop(g, &within(ATTACKER_GUERRILLA_OUT), 3.0)));
    a.polygons.retain(|(_, p)| p.len() >= 3);
    out.attacker = a;
    Zones { defender: def, attacker }
}

/// Eight lines across the eight directions from the town, just inside the attacker's zone's inner edge, on the map, off no-go; turned
/// like CA's (the town on their left).
fn approaches(out: &mut Layout, g: &Grid, attacker: &Mask, nogo: &Mask, bounds: (P, P), town: P) {
    let (lo, hi) = bounds;
    let good = |p: P| {
        p[0] >= lo[0] + 20.0 && p[1] >= lo[1] + 20.0 && p[0] <= hi[0] - 20.0 && p[1] <= hi[1] - 20.0 && g.at(p).is_some_and(|k| attacker[k] && !nogo[k])
    };

    for i in 0..8 {
        let a = f64::from(i) * std::f64::consts::FRAC_PI_4;
        let u = [a.cos(), a.sin()];
        let t = [-u[1], u[0]];
        // The first point out along the ray in the zone, a little further in.
        let mut r = 0.0;
        let mut q = None;
        while r < 4000.0 {
            let p = [town[0] + u[0] * r, town[1] + u[1] * r];
            match g.at(p) {
                None => break,
                Some(k) if attacker[k] => {
                    q = Some(r + 10.0);
                    break;
                }
                _ => {}
            }

            r += 2.0;
        }

        let Some(start) = q else { continue };
        let mut placed = None;
        'out: for half in [60.0, 30.0] {
            let mut r = start;
            for _ in 0..15 {
                let p = [town[0] + u[0] * r, town[1] + u[1] * r];
                let (e0, e1) = ([p[0] - t[0] * half, p[1] - t[1] * half], [p[0] + t[0] * half, p[1] + t[1] * half]);
                if good(e0) && good(e1) && good(p) {
                    placed = Some([e0, e1]);
                    break 'out;
                }

                r += 10.0;
            }
        }

        if let Some(l) = placed {
            out.approaches.push(if crate::ports::winding(&l, town) > 0.0 { l } else { [l[1], l[0]] });
        }
    }

    if out.approaches.len() < 3 {
        out.warnings.push(format!("only {} approach lines fit on the map around the settlement", out.approaches.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Capture, WallPiece};

    /// A walled square town with a gate on the south side and a block of houses in the middle.
    fn town() -> Input {
        let mut walls = vec![];
        let (a, b) = (300.0, 700.0);
        // Pieces of 20 along each side, the walkway on the axis, 6 thick.
        let side = |p: P, q: P, gate_at: Option<usize>, walls: &mut Vec<WallPiece>| {
            let n = (dist(p, q) / 20.0).round() as usize;
            for i in 0..n {
                let s = [p[0] + (q[0] - p[0]) * i as f64 / n as f64, p[1] + (q[1] - p[1]) * i as f64 / n as f64];
                let e = [p[0] + (q[0] - p[0]) * (i + 1) as f64 / n as f64, p[1] + (q[1] - p[1]) * (i + 1) as f64 / n as f64];
                let d = [(e[0] - s[0]) / 20.0, (e[1] - s[1]) / 20.0];
                let nrm = [-d[1] * 3.0, d[0] * 3.0];
                let fp = vec![[s[0] - nrm[0], s[1] - nrm[1]], [e[0] - nrm[0], e[1] - nrm[1]], [e[0] + nrm[0], e[1] + nrm[1]], [s[0] + nrm[0], s[1] + nrm[1]]];
                walls.push(WallPiece { footprint: vec![fp.clone()], platforms: vec![fp], walkway: vec![], gate: gate_at == Some(i), ends: [s, e] });
            }
        };
        side([a, a], [b, a], Some(10), &mut walls);
        side([b, a], [b, b], None, &mut walls);
        side([b, b], [a, b], None, &mut walls);
        side([a, b], [a, a], None, &mut walls);
        let block = |x0: f64, z0: f64, x1: f64, z1: f64| vec![[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
        // A ring road 30 wide inside the walls, two streets 30 wide crossing in the middle.
        Input {
            bounds: ([0.0, 0.0], [1000.0, 1000.0]),
            nogo: vec![],
            obstacles: vec![block(333.0, 333.0, 485.0, 485.0), block(515.0, 333.0, 667.0, 485.0), block(333.0, 515.0, 485.0, 667.0), block(515.0, 515.0, 667.0, 667.0)],
            walls,
            captures: vec![Capture { at: [500.0, 500.0], major: true }],
            palisade: vec![],
            sea: None,
        }
    }

    #[test]
    fn walled_town() {
        let l = generate(&town());
        eprintln!("{}\n{:?}", l.summary(), l.warnings);
        assert!(l.perimeter.len() >= 4);
        let r = crate::check::check(&l);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(l.nodes.iter().any(|n| n.kind == NodeKind::Area));
        assert!(l.edges.iter().any(|e| e.kind == EdgeKind::Street));
        assert_eq!(l.edges.iter().filter(|e| e.kind == EdgeKind::Wall).count(), 1);
        assert!(l.approaches.len() >= 4);
    }

    /// A camp: a square palisade of side 200 (pieces every 10 along their z, 10 long towards -z, the camp on their +x), an opening of
    /// 30 in the middle of each side.
    fn camp() -> Input {
        let mut input = Input { bounds: ([0.0, 0.0], [1000.0, 1000.0]), captures: vec![Capture { at: [500.0, 500.0], major: true }], ..Input::default() };
        for side in 0..4 {
            let (c, s) = (std::f64::consts::FRAC_PI_2 * f64::from(side)).sin_cos();
            let (along, inward) = ([-c, s], [-s, -c]);
            for k in -9..=9 {
                if (-1..=1).contains(&k) {
                    continue;
                }

                let t = f64::from(k) * 10.0;
                let at = [500.0 + s * 100.0 + along[0] * t, 500.0 + c * 100.0 + along[1] * t];
                let p = |a: f64, z: f64| [at[0] + inward[0] * a + along[0] * z, at[1] + inward[1] * a + along[1] * z];
                input.walls.push(WallPiece { footprint: vec![vec![p(-0.5, 0.25), p(0.5, 0.25), p(0.5, -10.25), p(-0.5, -10.25)]], ends: [at, at], ..WallPiece::default() });
                input.palisade.push(crate::PalisadePiece { at });
            }
        }

        input
    }

    #[test]
    fn camp_has_an_entry_at_each_opening() {
        let l = generate(&camp());
        eprintln!("{}
{:?}", l.summary(), l.warnings);
        assert_eq!(l.nodes.iter().filter(|n| n.kind == NodeKind::Entry).count(), 4);
        assert_eq!(l.edges.iter().filter(|e| e.kind == EdgeKind::Palisade).count(), 4);
        // Outside the palisade.
        for n in l.nodes.iter().filter(|n| n.kind == NodeKind::Entry) {
            assert!((n.centre[0] - 500.0).abs().max((n.centre[1] - 500.0).abs()) > 100.0, "{:?}", n.centre);
        }
    }
}
