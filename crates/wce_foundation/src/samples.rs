//! A grid of 16-bit samples (a map's LF heights) shared by everything that reads them: the map's model (its edits), the placed map (the
//! fill, the objects standing on the LF) and the scene (the renderer's height windows). Row-major, the file's own order (row 0 the north
//! edge for an LF).
//!
//! Its samples live in memory, or in a copy-on-write view of the file they were read from (`map_file`): the samples nobody changed stay the
//! file's (the system pages them in as they are read and drops them under pressure; they are not the process's private memory) and a
//! page written to becomes the process's own copy, the file untouched. The campaign map's LF is 375 MB, which the model, the placed map
//! and the scene each held a copy of (docs/plans/terrain_editor.md, Phase 8).
//!
//! A file with a view of it can't be truncated or replaced (Windows): write it only after [`SampleGrid::detach`] (the samples copied into
//! memory, the view closed).

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
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, format!("{}: {} samples from byte {offset} past its end", path.display(), n)));
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

/// A copy-on-write view of a file's bytes from an offset (Windows); `None` where views are not made.
struct MappedView {
    #[cfg(windows)]
    base: *mut std::ffi::c_void,
    #[cfg(windows)]
    mapping: *mut std::ffi::c_void,
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

    #[cfg(not(windows))]
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
            let mapping = win::CreateFileMappingW(file.as_raw_handle().cast(), std::ptr::null_mut(), win::PAGE_WRITECOPY, (end >> 32) as u32, end as u32, std::ptr::null());
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

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use std::ffi::c_void;

    pub const PAGE_WRITECOPY: u32 = 0x08;
    pub const FILE_MAP_COPY: u32 = 0x01;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateFileMappingW(file: *mut c_void, attributes: *mut c_void, protect: u32, size_high: u32, size_low: u32, name: *const u16) -> *mut c_void;
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
}
