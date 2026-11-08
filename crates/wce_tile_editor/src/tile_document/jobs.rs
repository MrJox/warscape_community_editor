//! Work done off the editor's thread: what procedural walls and roads make (their pieces, their terrain) is computed on a thread of its own
//! from what it needs, taken when it is asked for (owned copies, shared blocks), and applied on the editor's thread when it is done: the
//! editor does not wait on it. A newer request for the same thing takes the place of an older one, whose result is let go.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// How many jobs are running, of every tile: what `tile_wait_jobs` waits for.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// A computation on its own thread.
pub(crate) struct Job<T> {
    rx: Receiver<T>,
}

/// What a job gave so far.
pub(crate) enum Poll<T> {
    Running,
    Done(T),

    /// It stopped without a result (it panicked): the log has why.
    Failed,
}

impl<T: Send + 'static> Job<T> {
    /// Starts `work` on a thread named `name`.
    pub(crate) fn spawn(name: &str, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = channel();
        RUNNING.fetch_add(1, Ordering::SeqCst);
        let spawned = std::thread::Builder::new().name(name.to_owned()).spawn(move || {
            // Counted down however it ends: a panic too.
            struct Done;
            impl Drop for Done {
                fn drop(&mut self) {
                    RUNNING.fetch_sub(1, Ordering::SeqCst);
                }
            }

            let _done = Done;
            let _ = tx.send(work());
        });

        if let Err(err) = spawned {
            RUNNING.fetch_sub(1, Ordering::SeqCst);
            wce_foundation::log::warn(format!("{name}: no thread: {err}"));
        }

        Self { rx }
    }

    /// Waits for it to end.
    pub(crate) fn wait(&self) -> Poll<T> {
        match self.rx.recv() {
            Ok(v) => Poll::Done(v),
            Err(_) => Poll::Failed,
        }
    }

    pub(crate) fn poll(&self) -> Poll<T> {
        match self.rx.try_recv() {
            Ok(v) => Poll::Done(v),
            Err(TryRecvError::Empty) => Poll::Running,
            Err(TryRecvError::Disconnected) => Poll::Failed,
        }
    }
}

/// Whether any job of any tile is running.
#[must_use]
pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst) > 0
}

/// The ground of a tile as a job reads it: its height map (a copy) and how it lies (`TileProject::height_at`).
#[derive(Clone)]
pub(crate) struct Ground {
    field: Option<std::sync::Arc<wce_tile::height_field::HeightField>>,
    border: f64,
    mpp: (f64, f64),
}

impl Ground {
    pub(crate) fn of(project: &wce_tile::tile_project::TileProject) -> Self {
        Self { field: project.height.clone().map(std::sync::Arc::new), border: project.border_m(), mpp: (project.mpp_x(), project.mpp_z()) }
    }

    /// As `TileProject::height_at`: at a place in metres, in the height map's units.
    pub(crate) fn height_at(&self, x_m: f64, z_m: f64) -> f64 {
        self.field.as_ref().map_or(0.0, |h| h.sample((x_m + self.border) / self.mpp.0, (z_m + self.border) / self.mpp.1))
    }
}
