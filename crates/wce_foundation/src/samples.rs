//! A grid of 16-bit samples (a map's LF heights) shared by everything that reads them: the map's model (its edits), the placed map (the
//! fill, the objects standing on the LF) and the scene (the renderer's height windows). Row-major, the file's own order (row 0 the north
//! edge for an LF).
//!
//! Its samples live in memory, or in a copy-on-write view of the file they were read from (`map_file`): the samples nobody changed stay the
//! file's (the system pages them in as they are read and drops them under pressure; they are not the process's private memory) and a
//! page written to becomes the process's own copy, the file untouched. The campaign map's LF is 375 MB, which the model, the placed map
//! and the scene each held a copy of (docs/plans/terrain_editor.md, Phase 8).
//!
//! Write a file with a view of it only after [`SampleGrid::detach`] (the samples copied into memory, the view closed). Windows refuses to
//! truncate or replace it otherwise. Unix doesn't: a change to the file shows through the pages nobody wrote to, and reading a page a
//! truncation cut off kills the process (SIGBUS). Replacing it by renaming another file over it is safe there, the view keeps the old file.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// The samples' storage.
enum Storage {
    Owned(Vec<u16>),
    Mapped(MappedView),
}

impl Storage {
    fn samples(&self) -> &[u16] {
        match self {
            Self::Owned(v) => v,
            Self::Mapped(m) => m.samples(),
        }
    }

    fn samples_mut(&mut self) -> &mut [u16] {
        match self {
            Self::Owned(v) => v,
            Self::Mapped(m) => m.samples_mut(),
        }
    }
}

pub struct SampleGrid {
    w: u32,
    h: u32,
    storage: RwLock<Storage>,
    revision: AtomicU64,

    /// The samples changed since the last `take_dirty` (x0, y0, x1, y1; end exclusive).
    dirty: Mutex<Option<[u32; 4]>>,
}

impl std::fmt::Debug for SampleGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SampleGrid({} x {}, {})", self.w, self.h, if self.is_mapped() { "mapped" } else { "in memory" })
    }
}

/// The samples to read: row-major, `width` a row.
pub struct GridRead<'a>(RwLockReadGuard<'a, Storage>);

impl std::ops::Deref for GridRead<'_> {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        self.0.samples()
    }
}

/// The samples to change (mark what changed with [`SampleGrid::touch`]).
pub struct GridWrite<'a>(RwLockWriteGuard<'a, Storage>);

impl std::ops::Deref for GridWrite<'_> {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        self.0.samples()
    }
}

impl std::ops::DerefMut for GridWrite<'_> {
    fn deref_mut(&mut self) -> &mut [u16] {
        self.0.samples_mut()
    }
}

impl SampleGrid {
    /// `w` x `h` samples in memory (`values` row by row; padded with 0 or cut to `w * h`).
    pub fn new(w: u32, h: u32, mut values: Vec<u16>) -> Self {
        values.resize(w as usize * h as usize, 0);
        Self::with(w, h, Storage::Owned(values))
    }

    fn with(w: u32, h: u32, storage: Storage) -> Self {
        Self { w, h, storage: RwLock::new(storage), revision: AtomicU64::new(0), dirty: Mutex::new(None) }
    }

    /// `w` x `h` little-endian samples of a file from byte `offset`, row by row, in a copy-on-write view of the file (read into memory
    /// where views are not made: another system, an odd offset).
    ///
    /// # Errors
    ///
    /// The file can't be opened or read, or is shorter than the samples.
    pub fn map_file(path: &std::path::Path, offset: u64, w: u32, h: u32) -> std::io::Result<Self> {
        let n = w as usize * h as usize;
        let len = std::fs::metadata(path)?.len();
        if offset + n as u64 * 2 > len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("{}: {} samples from byte {offset} past its end", path.display(), n),
            ));
        }

        if offset.is_multiple_of(2)
            && let Some(view) = MappedView::open(path, offset, n)
        {
            return Ok(Self::with(w, h, Storage::Mapped(view)));
        }

        use std::io::{Read, Seek};
        let mut f = std::fs::File::open(path)?;
        f.seek(std::io::SeekFrom::Start(offset))?;
        let mut bytes = vec![0u8; n * 2];
        f.read_exact(&mut bytes)?;
        Ok(Self::new(w, h, bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()))
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn read(&self) -> GridRead<'_> {
        GridRead(self.storage.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn write(&self) -> GridWrite<'_> {
        GridWrite(self.storage.write().unwrap_or_else(PoisonError::into_inner))
    }

    /// Marks samples as changed (x0, y0, x1, y1; end exclusive): the revision moves on, `take_dirty` returns them.
    pub fn touch(&self, rect: [u32; 4]) {
        let mut d = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        *d = Some(d.map_or(rect, |o| [o[0].min(rect[0]), o[1].min(rect[1]), o[2].max(rect[2]), o[3].max(rect[3])]));
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// Moves on with every [`Self::touch`] (0: never changed).
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// The samples touched since the last call.
    pub fn take_dirty(&self) -> Option<[u32; 4]> {
        self.dirty.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// Whether the samples are a view of their file.
    pub fn is_mapped(&self) -> bool {
        matches!(*self.storage.read().unwrap_or_else(PoisonError::into_inner), Storage::Mapped(_))
    }

    /// The samples copied into memory and the file's view closed (before the file is written).
    pub fn detach(&self) {
        let mut s = self.storage.write().unwrap_or_else(PoisonError::into_inner);
        if let Storage::Mapped(m) = &*s {
            let owned = m.samples().to_vec();
            *s = Storage::Owned(owned);
        }
    }

    /// A copy in memory (its own samples from now on).
    pub fn snapshot(&self) -> Self {
        Self::new(self.w, self.h, self.read().to_vec())
    }
}

/// A copy-on-write view of a file's bytes from an offset (Windows, Unix); `None` where views are not made.
struct MappedView {
    #[cfg(any(windows, unix))]
    base: *mut std::ffi::c_void,
    #[cfg(windows)]
    mapping: *mut std::ffi::c_void,
    #[cfg(unix)]
    mapped_bytes: usize,
    ptr: *mut u16,
    len: usize,
}

// SAFETY: the view is owned by its grid, read and written under the grid's lock.
unsafe impl Send for MappedView {}
// SAFETY: as above.
unsafe impl Sync for MappedView {}

impl MappedView {
    fn samples(&self) -> &[u16] {
        // SAFETY: `ptr` points at `len` samples of the view, alive until it is dropped.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn samples_mut(&mut self) -> &mut [u16] {
        // SAFETY: as above; the view is copy-on-write, so writing never reaches the file.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    #[cfg(not(any(windows, unix)))]
    fn open(_: &std::path::Path, _: u64, _: usize) -> Option<Self> {
        None
    }

    #[cfg(windows)]
    fn open(path: &std::path::Path, offset: u64, samples: usize) -> Option<Self> {
        use std::os::windows::io::AsRawHandle;

        // The file may be read and written by others while the view is open (a view of a file being replaced fails: `detach` first).
        let file = std::fs::OpenOptions::new().read(true).open(path).ok()?;
        let end = offset + samples as u64 * 2;
        // SAFETY: plain Win32 calls on a handle we own; a null result is checked.
        unsafe {
            let mapping = win::CreateFileMappingW(
                file.as_raw_handle().cast(),
                std::ptr::null_mut(),
                win::PAGE_WRITECOPY,
                (end >> 32) as u32,
                end as u32,
                std::ptr::null(),
            );
            if mapping.is_null() {
                return None;
            }

            // The view starts at the file's start (a view's offset must be a multiple of the allocation granularity).
            let base = win::MapViewOfFile(mapping, win::FILE_MAP_COPY, 0, 0, end as usize);
            if base.is_null() {
                win::CloseHandle(mapping);
                return None;
            }

            Some(Self { base, mapping, ptr: base.cast::<u8>().add(offset as usize).cast::<u16>(), len: samples })
        }
    }

    #[cfg(unix)]
    fn open(path: &std::path::Path, offset: u64, samples: usize) -> Option<Self> {
        use std::os::fd::AsRawFd;

        let file = std::fs::File::open(path).ok()?;
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(samples.checked_mul(2)?)?;
        // SAFETY: a new private mapping of a file we opened (it outlives the descriptor, closed on return); a failure is checked.
        unsafe {
            // The view starts at the file's start (a view's offset must be a multiple of the page size).
            let base =
                posix::mmap(std::ptr::null_mut(), end, posix::PROT_READ | posix::PROT_WRITE, posix::MAP_PRIVATE, file.as_raw_fd(), 0);
            if base.addr() == posix::MAP_FAILED {
                return None;
            }

            Some(Self { base, mapped_bytes: end, ptr: base.cast::<u8>().add(start).cast::<u16>(), len: samples })
        }
    }
}

#[cfg(windows)]
impl Drop for MappedView {
    fn drop(&mut self) {
        // SAFETY: the view and the mapping were made by `open` and are released once.
        unsafe {
            win::UnmapViewOfFile(self.base);
            win::CloseHandle(self.mapping);
        }
    }
}

#[cfg(unix)]
impl Drop for MappedView {
    fn drop(&mut self) {
        // SAFETY: the mapping was made by `open` and is released once.
        unsafe {
            posix::munmap(self.base, self.mapped_bytes);
        }
    }
}

/// The same values on Linux, macOS and the BSDs.
#[cfg(unix)]
mod posix {
    use std::ffi::c_void;

    pub const PROT_READ: i32 = 1;
    pub const PROT_WRITE: i32 = 2;
    pub const MAP_PRIVATE: i32 = 2;
    pub const MAP_FAILED: usize = usize::MAX;

    unsafe extern "C" {
        pub fn mmap(addr: *mut c_void, bytes: usize, prot: i32, flags: i32, fd: i32, offset: isize) -> *mut c_void;
        pub fn munmap(addr: *mut c_void, bytes: usize) -> i32;
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use std::ffi::c_void;

    pub const PAGE_WRITECOPY: u32 = 0x08;
    pub const FILE_MAP_COPY: u32 = 0x01;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateFileMappingW(
            file: *mut c_void,
            attributes: *mut c_void,
            protect: u32,
            size_high: u32,
            size_low: u32,
            name: *const u16,
        ) -> *mut c_void;
        pub fn MapViewOfFile(mapping: *mut c_void, access: u32, offset_high: u32, offset_low: u32, bytes: usize) -> *mut c_void;
        pub fn UnmapViewOfFile(base: *const c_void) -> i32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapped_file_reads_and_writes_copy_on_write() {
        let dir = std::env::temp_dir().join(format!("wce_samples_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.bin");
        let mut bytes = vec![7u8, 9];
        bytes.extend((0..12u16).flat_map(|v| (v * 100).to_le_bytes()));
        std::fs::write(&path, &bytes).unwrap();
        {
            let g = SampleGrid::map_file(&path, 2, 4, 3).unwrap();
            assert_eq!(g.is_mapped(), cfg!(any(windows, unix)), "a view of the file");
            assert_eq!(g.read()[5], 500);
            g.write()[5] = 1;
            g.touch([1, 1, 2, 2]);
            assert_eq!(g.read()[5], 1);
            assert_eq!(g.take_dirty(), Some([1, 1, 2, 2]));
            g.detach();
            assert!(!g.is_mapped());
            assert_eq!(g.read()[5], 1);
            assert_eq!(g.read()[6], 600);
        }

        // The file itself unchanged, and free to be replaced.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, b"x").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_shorter_than_the_samples_is_refused_and_an_odd_offset_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, [0u8, 1, 0, 2, 0]).unwrap();
        assert!(SampleGrid::map_file(&path, 0, 3, 1).is_err());

        let g = SampleGrid::map_file(&path, 1, 2, 1).unwrap();
        assert!(!g.is_mapped());
        assert_eq!(&*g.read(), &[0x0001, 0x0002]);
    }

    #[cfg(unix)]
    #[test]
    fn a_view_keeps_the_file_a_rename_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, [1u8, 0, 2, 0]).unwrap();
        let g = SampleGrid::map_file(&path, 0, 2, 1).unwrap();

        let other = dir.path().join("g.bin");
        std::fs::write(&other, [9u8]).unwrap();
        std::fs::rename(&other, &path).unwrap();
        assert_eq!(&*g.read(), &[1, 2]);
    }

    /// The pages of a large view, written across a page boundary and dropped: the edits stay out of the file.
    #[test]
    fn a_view_past_the_first_page_keeps_its_edits_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        let bytes: Vec<u8> = (0..20_000u16).flat_map(u16::to_le_bytes).collect();
        std::fs::write(&path, &bytes).unwrap();

        let g = SampleGrid::map_file(&path, 6, 1000, 19).unwrap();
        assert_eq!(g.read()[18_999], 19_002);
        g.write()[2045] = 1;
        g.write()[18_999] = 2;
        assert_eq!((g.read()[2045], g.read()[2046], g.read()[18_999]), (1, 2049, 2));
        drop(g);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
