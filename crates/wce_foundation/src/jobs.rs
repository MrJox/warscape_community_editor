//! Background jobs (file reading, decoding, GPU resource creation), run in priority order: the smallest priority first, the oldest
//! first among equals. A queued job's priority can be changed through the handle [`JobPool::post`] returns (the renderer re-ranks
//! pending model loads by distance every frame). [`io_pool`] has a handful of threads for jobs that wait on the disk.
//!
//! Data-parallel loops use rayon instead.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::thread::JoinHandle;

/// A queued job.
pub type Job = Box<dyn FnOnce() + Send + 'static>;

/// Names a posted job.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Handle(pub u64);

struct Task {
    id: u64,
    priority: f32,
    job: Job,
}

#[derive(Default)]
struct Queue {
    tasks: Vec<Task>,
    next_id: u64,
    stop: bool,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    work: Condvar,
    idle: Condvar,
    running: AtomicUsize,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

thread_local! {
    static PRIORITY: Cell<f32> = const { Cell::new(0.0) };
}

/// A pool of threads running posted jobs in priority order.
pub struct JobPool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl JobPool {

    /// A pool of `threads` threads; with none, `post` runs the job right away.
    pub fn new(threads: usize) -> Self {
        let shared = Arc::new(Shared { queue: Mutex::new(Queue { next_id: 1, ..Default::default() }), ..Default::default() });
        let workers = (0..threads).map(|_| {
            let shared = shared.clone();
            std::thread::spawn(move || worker(&shared))
        }).collect();

        Self { shared, workers }
    }

    /// Queues a job; smaller priorities run first.
    pub fn post(&self, job: impl FnOnce() + Send + 'static, priority: f32) -> Handle {
        if self.workers.is_empty() {
            job();
            return Handle(0);
        }

        let id = {
            let mut queue = self.shared.lock();
            let id = queue.next_id;
            queue.next_id += 1;
            queue.tasks.push(Task { id, priority, job: Box::new(job) });
            id
        };

        self.shared.work.notify_one();
        Handle(id)
    }

    /// Changes the priority of a job that hasn't started; nothing once it has.
    pub fn set_priority(&self, handle: Handle, priority: f32) {
        if let Some(task) = self.shared.lock().tasks.iter_mut().find(|task| task.id == handle.0) {
            task.priority = priority;
        }
    }

    /// Removes a job that hasn't started; false if it already did (or never existed).
    pub fn cancel(&self, handle: Handle) -> bool {
        let mut queue = self.shared.lock();
        match queue.tasks.iter().position(|task| task.id == handle.0) {
            Some(at) => {
                queue.tasks.remove(at);
                true
            }

            None => false,
        }
    }

    /// The priority of the job the calling pool thread is running (0 elsewhere).
    pub fn current_priority() -> f32 {
        PRIORITY.with(Cell::get)
    }

    pub fn queued(&self) -> usize {
        self.shared.lock().tasks.len()
    }

    pub fn running(&self) -> usize {
        self.shared.running.load(Ordering::Acquire)
    }

    /// Blocks until the queue is empty and no job runs.
    pub fn wait_idle(&self) {
        let mut queue = self.shared.lock();
        while !queue.tasks.is_empty() || self.shared.running.load(Ordering::Acquire) != 0 {
            queue = self.shared.idle.wait(queue).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Drop for JobPool {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.work.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker(shared: &Shared) {
    loop {
        let task = {
            let mut queue = shared.lock();
            while !queue.stop && queue.tasks.is_empty() {
                queue = shared.work.wait(queue).unwrap_or_else(|poisoned| poisoned.into_inner());
            }

            if queue.stop {
                return;
            }

            // The first of the smallest priorities: the oldest among equals.
            let best = queue.tasks.iter().enumerate()
                .fold(0, |best, (i, task)| if task.priority < queue.tasks[best].priority { i } else { best });
            shared.running.fetch_add(1, Ordering::AcqRel);
            queue.tasks.remove(best)
        };

        // A job that panics only loses its own work: the counters stay right, so waiting for the pool can't hang.
        PRIORITY.with(|p| p.set(task.priority));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task.job));
        PRIORITY.with(|p| p.set(0.0));
        shared.running.fetch_sub(1, Ordering::AcqRel);
        let _guard = shared.lock();
        shared.idle.notify_all();
    }
}

/// The pool for jobs that wait on the disk: half the cores, 2 to 6 threads.
pub fn io_pool() -> &'static JobPool {
    static POOL: LazyLock<JobPool> = LazyLock::new(|| {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        JobPool::new((cores / 2).clamp(2, 6))
    });

    &POOL
}

/// Lowers the calling thread's scheduling priority below normal (background work that must not take the cores from the render thread:
/// a map context's tile loading). Windows: THREAD_PRIORITY_BELOW_NORMAL; Linux: a nice value of 10 for the thread; elsewhere nothing.
pub fn lower_thread_priority() {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
        }

        const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
        // SAFETY: the pseudo handle of the calling thread, always valid.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
    }

    #[cfg(target_os = "linux")]
    {
        unsafe extern "C" {
            fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        }

        // SAFETY: PRIO_PROCESS with who 0 is the calling thread on Linux (threads are scheduled entities); no memory is passed.
        unsafe {
            setpriority(0, 0, 10);
        }
    }
}
