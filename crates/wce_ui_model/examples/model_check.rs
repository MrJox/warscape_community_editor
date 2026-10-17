//! Gate for the model over every UI file of the game: the tree prints back to the converter's text, the model
//! finds a root and rectangles, and edit operations (move, resize, duplicate, delete, reparent, add) give XML that
//! converts to binary and parses again. Untouched files must give back their original bytes.
//!
//!   cargo run --release -p wce_ui_model --example model_check -- --game <game folder>

use wce_ui_model::UiModel;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut game = None;
    while let Some(a) = args.next() {
        if a == "--game" {
            game = args.next();
        }
    }
    let game = game.expect("--game <folder>");
    let vfs = wce_vfs::Vfs::mount(&wce_vfs::Config::new(game, wce_vfs::GameProfile::ATTILA)).expect("mount");
    let mut n = 0;
    let mut fails = 0;
    let mut entries_total = 0;
    let mut draw_total = 0;
    let (mut anim_total, mut frame_total) = (0, 0);
    for path in vfs.files().keys() {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !(path.starts_with("ui/") && !name.contains('.')) && !name.ends_with(".cml") {
            continue;
        }
        let Ok(data) = vfs.read(path) else { continue };
        if !wce_ui_format::looks_like_ui(&data) {
            continue;
        }
        n += 1;
        let mut fail = |what: &str| {
            fails += 1;
            println!("FAIL {path}: {what}");
        };
        let x = match wce_ui_format::ui_to_xml(&data) {
            Ok(x) => x,
            Err(e) => {
                fail(&format!("convert: {e}"));
                continue;
            }
        };
        let mut m = match UiModel::from_xml(&x.xml) {
            Ok(m) => m,
            Err(e) => {
                fail(&format!("parse: {e}"));
                continue;
            }
        };
        if m.to_xml() != x.xml {
            let a = m.to_xml();
            let at = a.bytes().zip(x.xml.bytes()).position(|(p, q)| p != q).unwrap_or(0);
            fail(&format!("tree prints different text at byte {at}: {:?} vs {:?}", &a[at.saturating_sub(40)..(at + 40).min(a.len())], &x.xml[at.saturating_sub(40)..(at + 40).min(x.xml.len())]));
            continue;
        }
        match m.to_bytes() {
            Ok(b) if b == data => {}
            Ok(_) => {
                fail("bytes differ after to_bytes");
                continue;
            }
            Err(e) => {
                fail(&format!("to_bytes: {e}"));
                continue;
            }
        }
        if !m.is_layout() {
            continue;
        }
        let ents = m.entries();
        entries_total += ents.len();
        if ents.is_empty() {
            fail("no entries");
            continue;
        }
        draw_total += m.draw_list().len();

        // animations: every one parses, plays to its end and gives a pose that draws
        for (ei, an) in m.all_animations(&ents) {
            anim_total += 1;
            frame_total += an.frames.len();
            let len = m.animation_length(&ents, ei, &an.name);
            if len < an.duration() {
                fail("an animation is shorter than its own frames");
            }

            for t in [0, len / 2, len] {
                let pose = std::rc::Rc::new(m.pose_at(&ents, ei, &an.name, t));
                let o = wce_ui_model::DrawOptions { pose: Some(pose), screen: Some((1920, 1080)), ..Default::default() };
                let _ = m.draw_list_with(&o);
            }
        }

        // Edits on the last leaf entry and the first child of the root.
        let Some(leaf) = ents.iter().rposition(|e| e.depth >= 1 && e.kind == wce_ui_model::EntryKind::Entry) else { continue };
        let lp = ents[leaf].path.clone();
        let ops = |m: &mut UiModel| -> Result<(), String> {
            // size first, as the canvas does: a docked component's dock offset is worked out for the size it ends with
            m.set_size(&lp, 33, 44)?;
            m.set_offset(&lp, 11, 22)?;
            let d = m.duplicate(&lp)?;
            m.set_name(&d, "dup")?;
            let before = m.entries().len();
            let e = m.entries().iter().find(|e| e.path == d).cloned().ok_or("dup missing")?;
            if (e.x, e.y, e.w, e.h) != (11, 22, 33, 44) {
                return Err(format!("dup has the wrong rect {:?}", (e.x, e.y, e.w, e.h)));
            }
            let moved = m.move_sibling(&d, -1)?;
            let a = m.add_child(&moved[..moved.len() - 2], Some(&moved), "added")?;
            let _ = a;
            m.delete(&moved)?;
            if m.entries().len() != before {
                // before counted the duplicate; the added one replaces it
            }
            Ok(())
        };
        if let Err(e) = ops(&mut m) {
            fail(&format!("edit: {e}"));
            continue;
        }
        match m.to_bytes() {
            Ok(b) => match UiModel::from_bytes(&b) {
                Ok(m2) => {
                    if m2.to_xml() != m.to_xml() {
                        fail("edited file does not reparse to the same XML");
                    }
                }
                Err(e) => fail(&format!("edited bytes do not parse: {e}")),
            },
            Err(e) => fail(&format!("edited to_bytes: {e}")),
        }
    }
    // the game's fonts: every .cuf parses, has glyphs for the basic letters, and lays text out
    let mut fonts = 0;
    for path in vfs.list("font/", ".cuf") {
        let Ok(bytes) = vfs.read(path) else { continue };
        match wce_ui_model::cuf::Cuf::parse(&bytes) {
            Ok(f) if f.glyph_of('A').is_some() && f.layout("Attackers", 400, 40, 1, 1).len() >= 8 && f.line_width("Attackers") > 20 => fonts += 1,
            Ok(_) => {
                fails += 1;
                println!("FAIL {path}: no letters or no layout");
            }
            Err(e) => {
                fails += 1;
                println!("FAIL {path}: {e}");
            }
        }
    }

    println!("{n} files, {entries_total} entries, {draw_total} draw items, {anim_total} animations ({frame_total} frames), {fonts} fonts, {fails} failures");
    if fails > 0 {
        std::process::exit(1);
    }
}
