//! Writes the sample cinematic scene (`wce_cinematic::sample`) as a project: `sample_scene <projects folder> [game]` makes
//! `<projects>/cinematics/wce_sample_battle/project.json`, refusing to replace one that is there.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(projects) = args.get(1) else {
        eprintln!("usage: sample_scene <projects folder> [game]");
        std::process::exit(2);
    };
    let p = wce_cinematic::sample::sample_project(args.get(2).map_or("attila", String::as_str));
    let dir = std::path::Path::new(projects).join("cinematics").join(&p.battle.key);
    let file = dir.join("project.json");
    if file.exists() {
        eprintln!("{} exists: not replaced", file.display());
        std::process::exit(1);
    }
    std::fs::create_dir_all(&dir).expect("the scene's folder");
    std::fs::write(&file, p.to_json()).expect("the project file");
    println!("{}", file.display());
}
