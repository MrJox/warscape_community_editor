//! The projected decals of a tile.ted as the tool places them: element index, key, world position and world axes (x, y, z) times the
//! scale (TEd units), and the group path; for finding a decal on the map and checking how its box is turned.
//!
//!   cargo run --release -p wce_tile --example decal_list -- <tile.ted> [text in the key or a group's name]

use wce_foundation::math::Double3;
use wce_tile::ted_scene::TedScene;
use wce_tile::world_transform::world_transform;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(path) = args.first() else {
        eprintln!("usage: decal_list <tile.ted> [filter]");
        std::process::exit(2);
    };

    let filter = args.get(1).map(|f| f.to_lowercase());
    let scene = TedScene::parse(std::fs::read(path).expect("the tile.ted")).expect("a tile.ted");
    for e in scene.find_all("ProjectedDecalInstance") {
        let doc = scene.doc();
        let key = doc.get(e, "key").unwrap_or_default();
        let mut names = vec![];
        let mut p = doc.elements()[e].parent;
        while let Some(x) = p {
            if let Some(n) = doc.get(x, "name") {
                let o = scene.euler(x).map_or(String::new(), |o| format!("({:.2} {:.2} {:.2})", o.x, o.y, o.z));
                names.push(format!("{n}#{x}{o}"));
            }
            p = doc.elements()[x].parent;
        }

        names.reverse();
        let path = names.join(" / ");
        if let Some(f) = &filter
            && !key.to_lowercase().contains(f.as_str())
            && !path.to_lowercase().contains(f.as_str())
        {
            continue;
        }

        let w = world_transform(&scene, e);
        let s = scene.scale(e).unwrap_or(Double3::new(1.0, 1.0, 1.0));
        let axis = |c: usize, k: f64| {
            let r = &w.rotation.m;
            format!("({:.2} {:.2} {:.2})", r[0][c] * k, r[1][c] * k, r[2][c] * k)
        };

        println!(
            "#{e} {} at ({:.1} {:.1} {:.1}) x {} y {} z {} | {path}",
            key.rsplit('\\').next().unwrap_or_default(),
            w.position.x, w.position.y, w.position.z,
            axis(0, s.x), axis(1, s.y), axis(2, s.z),
        );
    }
}
