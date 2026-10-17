//! How well the game's docking rules agree with the stored offsets of every Attila layout, at the layout's own size:
//! a docked component should already sit where its dock point puts it, and a docked picture where its dock point puts it.
//! Also checks that sizing the root to a larger screen moves only docked things.
//!
//!   cargo run --release -p wce_ui_model --example layout_check -- --game <game folder> [--list]

use wce_ui_model::draw::DrawOptions;
use wce_ui_model::layout::{dock_of, place_images, state_size};
use wce_ui_model::{EntryKind, UiModel};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut game, mut list, mut only) = (None, false, String::new());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next(),
            "--list" => list = true,
            "--only" => only = args.next().unwrap_or_default(),
            _ => {}
        }
    }

    let game = game.expect("--game <folder>");
    let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
    let (mut comps, mut docked, mut off, mut pics, mut dpics, mut poff, mut moved_undocked, mut off_big) = (0, 0, 0, 0, 0, 0, 0, 0);
    for path in vfs.files().keys() {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !(path.starts_with("ui/") && !name.contains('.')) {
            continue;
        }

        if !only.is_empty() && !path.contains(&only) {
            continue;
        }

        let Ok(data) = vfs.read(path) else { continue };
        if !wce_ui_format::looks_like_ui(&data) {
            continue;
        }

        let Ok(m) = UiModel::from_bytes(&data) else { continue };
        if !m.is_layout() {
            continue;
        }

        let ents = m.entries();
        let o = DrawOptions::default();
        let rects = m.geometry(&ents, &o);
        let big = DrawOptions { screen: Some((3840, 2160)), ..Default::default() };
        let rects_big = m.geometry(&ents, &big);
        for (i, en) in ents.iter().enumerate() {
            if en.kind != EntryKind::Entry {
                continue;
            }

            comps += 1;
            let Some(el) = m.dom.at(&en.path) else { continue };
            let (dock, doff) = dock_of(&m, el);
            if dock > 0 {
                docked += 1;
                let r = rects[i];
                if (r.x - en.ax as f32).abs() > 0.5 || (r.y - en.ay as f32).abs() > 0.5 {
                    off += 1;
                    let big = (r.x - en.ax as f32).abs() > 1.5 || (r.y - en.ay as f32).abs() > 1.5;
                    if big {
                        off_big += 1;
                    }

                    if list && big && off_big <= 400 {
                        println!("component off: {path} {} dock {dock} off {:?} stored ({}, {}) docked ({}, {})", en.name, doff, en.ax, en.ay, r.x, r.y);
                    }
                }
            } else if en.depth > 0 && en.parent == Some(0) {
                // an undocked child of the root must not move with the screen
                let (a, b) = (rects[i], rects_big[i]);
                if (a.x - b.x).abs() > 0.5 || (a.y - b.y).abs() > 0.5 {
                    moved_undocked += 1;
                }
            }

            // pictures of the default state
            let Some(state) = m.default_state(el) else { continue };
            let Some(uses) = state.find("image_uses") else { continue };
            let list_uses: Vec<_> = uses.elems().filter(|u| u.name == "image_use").collect();
            let size = state_size(state);
            let placed = place_images(&list_uses, m.version, size, size, false);
            for (u, p) in list_uses.iter().zip(&placed) {
                pics += 1;
                let ints: Vec<i64> = u.elems().filter(|c| c.name == "i").filter_map(|c| c.text().trim().parse().ok()).collect();
                let us: Vec<i64> = u.elems().filter(|c| c.name == "u").filter_map(|c| c.text().trim().parse::<i64>().ok()).collect();
                let stored = (us.get(1).map(|v| *v as u32 as i32).unwrap_or(0) as f32, us.get(2).map(|v| *v as u32 as i32).unwrap_or(0) as f32);
                if ints.first().copied().unwrap_or(0) > 0 {
                    dpics += 1;
                    if (p.x - stored.0).abs() > 0.5 || (p.y - stored.1).abs() > 0.5 {
                        poff += 1;
                        if list && poff <= 25 {
                            println!("picture off: {path} {} dock {} stored {:?} docked ({}, {}) size {:?}", en.name, ints[0], stored, p.x, p.y, size);
                        }
                    }
                }
            }
        }
    }

    println!("{comps} components, {docked} docked, {off} docked components not where their dock puts them ({off_big} by more than a pixel)");
    println!("{pics} pictures, {dpics} docked, {poff} docked pictures not where their dock puts them");
    println!("{moved_undocked} undocked root children moved when the screen grew (must be 0)");

    if moved_undocked > 0 {
        std::process::exit(1);
    }
}
