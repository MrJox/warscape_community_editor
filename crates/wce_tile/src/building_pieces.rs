//! The files of a building's folder (docs/re/buildings.md): which pieces stand, at which level, and which files are something else.

/// The number NN of "destructNN" in a piece file name ("..._piece01_destruct02.rigid_model_v2" is 2); 0 for a file without one.
pub fn destruct_number(file: &str) -> u32 {
    let Some(at) = file.find("destruct") else { return 0 };
    file[at + "destruct".len()..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

/// Whether a file of a building's folder is a piece the tile draws standing: not a mesh of an animation (`..._destruct01_anim`, the collapse
/// between two levels, the gate swings, the siege engines' wheels), and of a piece with two states (`_gate_open` / `_gate_closed`,
/// `_ramp_...`, `_lever_...`) only the closed one. Drawing them all put a level's collapse mesh, offset, over the standing piece.
/// ASSUMPTION: a placed gate, ramp or lever stands closed (the engine's choice of state was not traced).
pub fn static_piece(file: &str) -> bool {
    let stem = file.strip_suffix(".rigid_model_v2").unwrap_or(file);
    !stem.ends_with("_anim") && !stem.ends_with("_open")
}

/// The file a numbered copy of a piece file copies: `..._destruct01_0.rigid_model_v2` -> `..._destruct01.rigid_model_v2`,
/// `..._destruct01_anim_0` -> `..._destruct01_anim`; None for a file without a `_<digits>` after its `destructNN`. Some mod packs (The
/// Dawnless Days: its Gondor, Moria and Khazad buildings) carry such a copy of every piece beside it, the same mesh; the building's logic
/// file names only the plain ones, so the game does not draw the copies. Drawing them put every piece twice and, through the copy of the
/// collapse (`_anim_0`, not an `_anim` by its ending), the falling rubble over the standing wall.
pub fn copied_piece(path: &str) -> Option<String> {
    let stem = path.strip_suffix(".rigid_model_v2")?;
    let file_at = stem.rfind('/').map_or(0, |s| s + 1);
    let tail_at = file_at + stem[file_at..].find("destruct")? + "destruct".len() + 2;
    let (base, number) = stem.rsplit_once('_')?;
    (base.len() >= tail_at && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{base}.rigid_model_v2"))
}

/// Whether a piece file is a numbered copy of another file of its folder (`copied_piece`, the copied file there).
pub fn piece_copy(path: &str, exists: impl Fn(&str) -> bool) -> bool {
    copied_piece(path).is_some_and(|original| exists(&original))
}

/// The part of a piece file after `destructNN` (`_left_wheel_looped_anim`, `_weighted_anim`, `_anim`; empty for a plain state).
fn piece_tail(file: &str) -> &str {
    let stem = file.strip_suffix(".rigid_model_v2").unwrap_or(file);
    stem.find("destruct").and_then(|at| stem.get(at + "destruct".len() + 2..)).unwrap_or("")
}

/// Whether a piece file is a mesh that only exists as an animation and is what the piece shows standing (anim_pose.rs in wce_render poses it
/// in its first frame): the siege engines' wheels (`_left_wheel_looped_anim`) and weighted bodies (`_weighted_anim`). Not the collapse
/// between two levels (`_destructNN_anim`, `_destruction_weighted_anim`) nor a gate's swing.
pub fn working_anim(file: &str) -> bool {
    let tail = piece_tail(file);
    tail.ends_with("_anim")
        && tail != "_anim"
        && !tail.starts_with("_destruction")
        && !tail.contains("gate")
        && !tail.contains("opening")
        && !tail.contains("closing")
}

/// The piece of a piece file (`piece02`).
pub fn piece_tag(file: &str) -> Option<&str> {
    let at = file.find("_piece")? + 1;
    let rest = &file[at..];
    Some(&rest[..rest.find('_').unwrap_or(rest.len())])
}

/// A working animation's piece file without its animation: the plain state file of the same piece and level.
pub fn plain_piece(path: &str) -> String {
    let file_at = path.rfind('/').map_or(0, |s| s + 1);
    let tail = piece_tail(&path[file_at..]);
    if tail.is_empty() {
        return path.to_owned();
    }

    let stem = path.strip_suffix(".rigid_model_v2").unwrap_or(path);
    format!("{}.rigid_model_v2", &stem[..stem.len() - tail.len()])
}
