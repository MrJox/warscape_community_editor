//! Prints an `.anim` file: its skeleton, bones, frame rate, length and, per frame, each bone's translation and rotation (a quaternion).
//!
//!   cargo run --release -p wce_formats --example anim_dump -- <file.anim> [frames to print]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: anim_dump <file.anim> [frames]");
    let shown: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(4);
    let bytes = std::fs::read(&path).expect("read");
    let Some(a) = wce_formats::anim::Animation::decode(&bytes, usize::MAX) else {
        println!("{path}: not an animation this reads ({} bytes)", bytes.len());
        return;
    };

    println!("skeleton '{}', {} bones, {} fps, end {}, {} frames", a.skeleton, a.bones.len(), a.fps, a.end, a.frames.len());
    for (k, b) in a.bones.iter().enumerate() {
        println!("  bone {k} '{b}' parent {} tracked {:?}", a.parents.get(k).copied().unwrap_or(-1), a.tracked.get(k));
    }

    let step = (a.frames.len() / shown.max(1)).max(1);
    for (f, frame) in a.frames.iter().enumerate().step_by(step) {
        println!("frame {f}: {:?}", frame);
    }
}
