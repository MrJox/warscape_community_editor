//! Writing the user's files so that a crash, a full disk or a power cut while saving never leaves one cut short: the bytes go to a temporary
//! file beside it, onto the disk, and then take its place in one step (a rename on the same volume). The file is the old one or the new one,
//! never half of either.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The temporary file `path` is written to first: hidden beside it (same folder, so the rename stays on its volume), named after it and the
/// process (two editors saving the same file do not write into one temporary file).
fn temporary(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.wce-saving", std::process::id()))
}

/// Writes `bytes` to `path`, replacing what it held only once they are all on the disk.
///
/// # Errors
///
/// The folder can't be written, the disk is full, or the file can't be replaced (another program holds it open): the file is then as it
/// was, and the temporary one is taken away.
pub fn write_replacing(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let path = path.as_ref();
    let tmp = temporary(path);
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes.as_ref())?;
        f.sync_all()
    })();

    let done = written.and_then(|()| std::fs::rename(&tmp, path));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }

    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_replaced_whole_and_nothing_is_left_beside_it() {
        let dir = std::env::temp_dir().join(format!("wce_write_replacing_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tile.ted");
        write_replacing(&file, b"first").unwrap();
        write_replacing(&file, b"second, longer").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"second, longer");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left");

        // A target that can't be replaced (a folder): an error, the folder as it was, no temporary file left.
        let folder = dir.join("held");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_replacing(&folder, b"x").is_err());
        assert!(folder.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
