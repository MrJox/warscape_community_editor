//! Making, removing, reordering and reparenting components, on every layout of the game: after each operation the layout still converts to a
//! binary layout and reads back, no component is lost or invented, a moved component keeps everything below it, its place on screen and its id,
//! and it ends up exactly where it was put.
//!
//!   cargo run --release -p wce_ui_model --example structure_check -- --game <game folder> [--list]

use wce_ui_model::model::Place;
use wce_ui_model::{Entry, EntryKind, UiModel};

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn subtree(ents: &[Entry], i: usize) -> Vec<usize> {
    let mut out = vec![i];
    let mut k = 0;
    while k < out.len() {
        let c = out[k];
        out.extend((0..ents.len()).filter(|&j| ents[j].parent == Some(c)));
        k += 1;
    }

    out
}

/// The names below `i` (and `i`), with their depth relative to `i`, in order.
fn shape(ents: &[Entry], i: usize) -> Vec<(String, usize)> {
    subtree(ents, i).into_iter().map(|k| (ents[k].name.clone(), ents[k].depth - ents[i].depth)).collect()
}

fn reads_back(m: &UiModel, count: usize) -> Result<(), String> {
    let bytes = m.clone().to_bytes()?;
    let back = UiModel::from_bytes(&bytes)?;
    if back.entries().len() != count {
        return Err(format!("{} components after converting and reading back, expected {count}", back.entries().len()));
    }

    Ok(())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut game, mut list) = (None, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next(),
            "--list" => list = true,
            _ => {}
        }
    }

    let game = game.expect("--game <folder>");
    let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
    let mut rng = Rng(7);
    let (mut files, mut moves, mut adds, mut dups, mut dels, mut bad) = (0, 0, 0, 0, 0, 0);
    let mut fail = |path: &str, what: String| {
        bad += 1;
        if bad <= 30 || list {
            println!("FAILED {path}: {what}");
        }
    };

    for path in vfs.files().keys() {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !(path.starts_with("ui/") && !name.contains('.')) {
            continue;
        }

        let Ok(data) = vfs.read(path) else { continue };
        if !wce_ui_format::looks_like_ui(&data) {
            continue;
        }

        let Ok(m0) = UiModel::from_bytes(&data) else { continue };
        if !m0.is_layout() {
            continue;
        }

        files += 1;
        let ents = m0.entries();
        let comps: Vec<usize> = (1..ents.len()).filter(|&i| ents[i].kind == EntryKind::Entry).collect();
        if comps.is_empty() {
            continue;
        }

        // ---- moves: several random ones per layout, each from the untouched layout
        for _ in 0..6 {
            let s = comps[rng.next(comps.len())];
            let candidates: Vec<usize> = (0..ents.len()).filter(|&t| ents[t].kind == EntryKind::Entry && !ents[t].path.starts_with(&ents[s].path) && t != s).collect();
            if candidates.is_empty() {
                continue;
            }

            let t = candidates[rng.next(candidates.len())];
            let place = match rng.next(3) {
                0 => Place::Before,
                1 => Place::After,
                _ => Place::Inside,
            };

            if t == 0 && place != Place::Inside {
                continue;
            }

            moves += 1;
            let mut m = m0.clone();
            let label = format!("move {} {:?} {}", ents[s].name, place, ents[t].name);
            let new = match m.move_to(&ents[s].path, &ents[t].path, place, true) {
                Ok(p) => p,
                Err(er) => {
                    fail(path, format!("{label}: {er}"));
                    continue;
                }
            };

            let after = m.entries();
            let Some(k) = after.iter().position(|e| e.path == new) else {
                fail(path, format!("{label}: the returned path is not a component"));
                continue;
            };

            if after.len() != ents.len() {
                fail(path, format!("{label}: {} components, there were {}", after.len(), ents.len()));
            }

            if after[k].id != ents[s].id || after[k].name != ents[s].name {
                fail(path, format!("{label}: the component moved is not the one asked for"));
            }

            if shape(&after, k) != shape(&ents, s) {
                fail(path, format!("{label}: what is inside the moved component changed"));
            }

            if (after[k].ax, after[k].ay) != (ents[s].ax, ents[s].ay) {
                fail(path, format!("{label}: it moved on screen from ({}, {}) to ({}, {})", ents[s].ax, ents[s].ay, after[k].ax, after[k].ay));
            }

            let parent_id = after[k].parent.map(|p| after[p].id);
            match place {
                Place::Inside => {
                    if parent_id != Some(ents[t].id) || after[k].parent.is_some_and(|p| after[p].name != ents[t].name) {
                        fail(path, format!("{label}: it is not inside the target"));
                    }

                    // last child
                    let sibs: Vec<usize> = (0..after.len()).filter(|&j| after[j].parent == after[k].parent).collect();
                    if sibs.last() != Some(&k) {
                        fail(path, format!("{label}: it is not the last child"));
                    }
                }
                Place::Before | Place::After => {
                    let want_parent = ents[t].parent.map(|p| ents[p].id);
                    if parent_id != want_parent || after[k].parent.map(|p| after[p].name.clone()) != ents[t].parent.map(|p| ents[p].name.clone()) {
                        fail(path, format!("{label}: it is not a sibling of the target"));
                    }

                    let sibs: Vec<usize> = (0..after.len()).filter(|&j| after[j].parent == after[k].parent).collect();
                    // the target again: ids repeat in a layout (a template used several times), so it is the same one of its kind in order
                    let nth = (0..ents.len()).filter(|&j| j != s && ents[j].id == ents[t].id && ents[j].name == ents[t].name).position(|j| j == t);
                    let tk = nth.and_then(|n| (0..after.len()).filter(|&j| j != k && after[j].id == ents[t].id && after[j].name == ents[t].name).nth(n));
                    let (pk, pt) = (sibs.iter().position(|&j| j == k), tk.and_then(|tk| sibs.iter().position(|&j| j == tk)));
                    let ok = match (pk, pt, place) {
                        (Some(a), Some(b), Place::Before) => a + 1 == b,
                        (Some(a), Some(b), Place::After) => a == b + 1,
                        _ => false,
                    };

                    if !ok {
                        fail(path, format!("{label}: it is not directly {} the target", if place == Place::Before { "before" } else { "after" }));
                    }
                }
            }

            if let Err(er) = reads_back(&m, ents.len()) {
                fail(path, format!("{label}: {er}"));
            }
        }

        // ---- add: into a component that has no children (the plainest case), and into one that has
        let leaf = comps.iter().copied().find(|&i| !ents.iter().any(|e| e.parent == Some(i)));
        let busy = comps.iter().copied().find(|&i| ents.iter().any(|e| e.parent == Some(i)));
        for (what, target) in [("a leaf", leaf), ("a parent", busy)] {
            let Some(t) = target else { continue };
            adds += 1;
            let mut m = m0.clone();
            match m.add_child(&ents[t].path, None, "wce_check_new") {
                Ok(p) => {
                    let after = m.entries();
                    match after.iter().position(|e| e.path == p) {
                        Some(k) if after[k].name == "wce_check_new" && after[k].parent.is_some_and(|q| after[q].id == ents[t].id) && after.len() == ents.len() + 1 => {
                            if after[k].id == 0 || after.iter().filter(|e| e.id == after[k].id).count() != 1 {
                                fail(path, format!("add into {what}: the new component's id is not unique"));
                            }
                        }
                        _ => fail(path, format!("add into {what}: the new component is not where it should be")),
                    }

                    if let Err(er) = reads_back(&m, ents.len() + 1) {
                        fail(path, format!("add into {what}: {er}"));
                    }
                }
                Err(er) => fail(path, format!("add into {what} ({}): {er}", ents[t].name)),
            }
        }

        // ---- duplicate and delete
        let s = comps[rng.next(comps.len())];
        let size = subtree(&ents, s).len();
        dups += 1;
        let mut m = m0.clone();
        match m.duplicate(&ents[s].path) {
            Ok(p) => {
                let after = m.entries();
                match after.iter().position(|e| e.path == p) {
                    Some(k) if after.len() == ents.len() + size && shape(&after, k) == shape(&ents, s) => {}
                    _ => fail(path, format!("duplicate {}: not a copy of it with everything inside", ents[s].name)),
                }

                if let Err(er) = reads_back(&m, ents.len() + size) {
                    fail(path, format!("duplicate {}: {er}", ents[s].name));
                }
            }
            Err(er) => fail(path, format!("duplicate {}: {er}", ents[s].name)),
        }

        dels += 1;
        let mut m = m0.clone();
        match m.delete(&ents[s].path) {
            Ok(()) => {
                let after = m.entries();
                let gone: std::collections::HashSet<usize> = subtree(&ents, s).into_iter().collect();
                let want: Vec<(String, usize)> = (0..ents.len()).filter(|i| !gone.contains(i)).map(|i| (ents[i].name.clone(), ents[i].depth)).collect();
                let have: Vec<(String, usize)> = after.iter().map(|e| (e.name.clone(), e.depth)).collect();
                if want != have {
                    fail(path, format!("delete {}: what is left is not the rest of the layout", ents[s].name));
                }

                if let Err(er) = reads_back(&m, ents.len() - size) {
                    fail(path, format!("delete {}: {er}", ents[s].name));
                }
            }
            Err(er) => fail(path, format!("delete {}: {er}", ents[s].name)),
        }
    }

    println!("{files} layouts: {moves} moves, {adds} adds, {dups} duplicates, {dels} deletes; {bad} problems");
    if bad > 0 || moves == 0 || adds == 0 {
        std::process::exit(1);
    }
}
