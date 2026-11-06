//! Battle animation tables, as text: `animations/animation_tables/animation_tables.txt` (a unit's table: its skeletons, mount table and
//! fragments) and `animations/animation_fragments/*.txt` (a fragment: the clips of each slot). rpfm_lib reads the binary forms of later games.
//!
//! A slot can have several lines (the alternatives the game picks among). A fragment line is
//! `SLOT filename = "..." metadata = "..." metadata_sound = "..." flags` (the two metadata optional), the flags separated by commas (`blend_in_time = 0.20`,
//! `weapon_bone_2 = on`, `single_frame_variant = true`), or just a slot and flags (`RIDER_SHIELD_UP cancel,`).

/// A clip of a slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FragmentEntry {
    pub slot: String,

    /// Empty on a line with flags only.
    pub filename: String,
    pub metadata: String,
    pub metadata_sound: String,

    /// Name and value, in file order; the value is empty for a bare flag.
    pub flags: Vec<(String, String)>,
}

impl FragmentEntry {
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// A fragment file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fragment {
    pub skeleton: String,
    pub cinematic: bool,
    pub entries: Vec<FragmentEntry>,
}

/// An `animation_table`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationTable {
    pub name: String,
    pub skeleton: String,

    /// The fuller skeleton of its cinematics (`rome_man`).
    pub skeleton_cinematic: Option<String>,

    /// A rider's: the mount's table.
    pub mount_table: Option<String>,

    /// Fragment names, in order: `animations/animation_fragments/<name>.txt`.
    pub fragments: Vec<String>,
}

/// `key = "value"` at the start of `s`: the value and what follows.
fn quoted<'a>(s: &'a str, key: &str) -> Option<(&'a str, &'a str)> {
    let s = s.trim_start().strip_prefix(key)?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
    s.split_once('"')
}

fn flags(s: &str) -> Vec<(String, String)> {
    s.split(',').map(str::trim).filter(|f| !f.is_empty()).map(|f| match f.split_once('=') {
        Some((n, v)) => (n.trim().to_owned(), v.trim().to_owned()),
        None => (f.to_owned(), String::new()),
    }).collect()
}

fn fragment_line(slot: &str, rest: &str) -> Option<FragmentEntry> {
    if !rest.trim_start().starts_with("filename") {
        return Some(FragmentEntry { slot: slot.to_owned(), flags: flags(rest), ..FragmentEntry::default() });
    }

    let (filename, rest) = quoted(rest, "filename")?;
    let (metadata, rest) = quoted(rest, "metadata").unwrap_or(("", rest));
    let (metadata_sound, rest) = quoted(rest, "metadata_sound").unwrap_or(("", rest));
    Some(FragmentEntry { slot: slot.to_owned(), filename: filename.to_owned(), metadata: metadata.to_owned(), metadata_sound: metadata_sound.to_owned(), flags: flags(rest) })
}

/// Parses a fragment; `None` when a line isn't one.
pub fn parse_fragment(text: &str) -> Option<Fragment> {
    let mut out = Fragment::default();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (first, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match first {
            "version" => {}
            "skeleton_type" => rest.trim().clone_into(&mut out.skeleton),
            "cinematic" => out.cinematic = rest.trim() == "true",
            _ => out.entries.push(fragment_line(first, rest)?),
        }
    }

    Some(out)
}

/// Parses `animation_tables.txt`; `None` when a word isn't where one belongs.
pub fn parse_tables(text: &str) -> Option<Vec<AnimationTable>> {
    let mut words = text.split_whitespace();
    let mut out = vec![];
    while let Some(word) = words.next() {
        match word {
            "version" => {
                words.next()?;
            }

            "animation_table" => {
                let mut table = AnimationTable { name: words.next()?.to_owned(), ..AnimationTable::default() };
                if words.next()? != "{" {
                    return None;
                }

                loop {
                    match words.next()? {
                        "}" => break,
                        "skeleton_type" => words.next()?.clone_into(&mut table.skeleton),
                        "skeleton_type_cinematic" => table.skeleton_cinematic = Some(words.next()?.to_owned()),
                        "mount_table" => table.mount_table = Some(words.next()?.to_owned()),
                        "fragment" => table.fragments.push(words.next()?.to_owned()),
                        _ => return None,
                    }
                }

                out.push(table);
            }

            _ => return None,
        }
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{parse_fragment, parse_tables};

    #[test]
    fn reads_a_fragment() {
        let f = parse_fragment("version 1\r\n\r\nskeleton_type\t\tboar\r\ncinematic\t\tfalse\r\n\r\nSTAND\t\t\t\tfilename = \"a.anim\"\t\tmetadata = \"\"\t\tmetadata_sound = \"a.snd.meta\"\t\tblend_in_time = 0.20, weapon_bone_2 = on, \r\nRIDER_SHIELD_UP\t\t\t\tcancel, \r\n").expect("parses");
        assert_eq!((f.skeleton.as_str(), f.cinematic, f.entries.len()), ("boar", false, 2));
        let e = &f.entries[0];
        assert_eq!((e.slot.as_str(), e.filename.as_str(), e.metadata.as_str(), e.metadata_sound.as_str()), ("STAND", "a.anim", "", "a.snd.meta"));
        assert_eq!((e.flag("blend_in_time"), e.flag("weapon_bone_2"), e.flag("other")), (Some("0.20"), Some("on"), None));
        assert_eq!((f.entries[1].filename.as_str(), f.entries[1].flag("cancel")), ("", Some("")));
        let f = parse_fragment("STAND\t\tfilename = \"a.anim\"\t\tmetadata = \"a.meta\"    blend_in_time = 0.10,\n").expect("parses");
        assert_eq!((f.entries[0].metadata.as_str(), f.entries[0].metadata_sound.as_str(), f.entries[0].flag("blend_in_time")), ("a.meta", "", Some("0.10")));
    }

    #[test]
    fn reads_the_tables() {
        let t = parse_tables("version 1\n\nanimation_table rider\n{\n\tskeleton_type\t\trome_man_game\n\tskeleton_type_cinematic\t\trome_man\n\tmount_table\t\tmount\n\n\tfragment\t\ta\n\tfragment\t\tb\n}\n\nanimation_table mount\n{\n\tskeleton_type\t\thorse_r2\n}\n").expect("parses");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].skeleton_cinematic.as_deref(), t[0].mount_table.as_deref(), t[0].fragments.len()), (Some("rome_man"), Some("mount"), 2));
        assert_eq!((t[1].name.as_str(), t[1].skeleton.as_str(), t[1].mount_table.as_deref()), ("mount", "horse_r2", None));
        assert!(parse_tables("animation_table a\n{\n\tother x\n}\n").is_none());
    }
}
