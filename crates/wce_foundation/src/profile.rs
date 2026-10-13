//! Scope timers for finding hot paths: `wce_prof!("name")` times the rest of the enclosing block into a named slot (total time, calls,
//! worst call). A slot is registered once per call site and updated with relaxed atomics, so scopes can sit in worker threads.
//! [`report`] prints the slots sorted by total time. A few ns per scope, always compiled in.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

/// The times of one name.
#[derive(Debug, Default)]
pub struct Slot {
    pub name: &'static str,
    pub ns: AtomicU64,
    pub calls: AtomicU64,
    pub worst: AtomicU64,
}

static SLOTS: LazyLock<Mutex<Vec<&'static Slot>>> = LazyLock::new(Default::default);

/// The slot of a name, registered on first use. Slots live for the whole process.
pub fn slot(name: &'static str) -> &'static Slot {
    let mut slots = SLOTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(slot) = slots.iter().find(|slot| slot.name == name) {
        return slot;
    }

    let slot: &'static Slot = Box::leak(Box::new(Slot { name, ..Default::default() }));
    slots.push(slot);
    slot
}

/// Times its own life into a slot.
pub struct Scope {
    slot: &'static Slot,
    start: Instant,
}

impl Scope {
    pub fn new(slot: &'static Slot) -> Self {
        Self { slot, start: Instant::now() }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let ns = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.slot.ns.fetch_add(ns, Ordering::Relaxed);
        self.slot.calls.fetch_add(1, Ordering::Relaxed);
        self.slot.worst.fetch_max(ns, Ordering::Relaxed);
    }
}

/// Times the rest of the enclosing block into the slot of a name.
#[macro_export]
macro_rules! wce_prof {
    ($name:expr) => {
        let _wce_prof_scope = {
            static SLOT: ::std::sync::OnceLock<&'static $crate::profile::Slot> = ::std::sync::OnceLock::new();
            $crate::profile::Scope::new(SLOT.get_or_init(|| $crate::profile::slot($name)))
        };
    };
}

/// Zeroes every slot.
pub fn reset() {
    for slot in SLOTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter() {
        slot.ns.store(0, Ordering::Relaxed);
        slot.calls.store(0, Ordering::Relaxed);
        slot.worst.store(0, Ordering::Relaxed);
    }
}

/// One line per slot with calls and at least `min_ms` in total, the slowest first.
pub fn report(reset_after: bool, min_ms: f64) -> String {
    let mut slots = SLOTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    slots.sort_by_key(|slot| std::cmp::Reverse(slot.ns.load(Ordering::Relaxed)));
    let mut out = String::new();
    for slot in slots {
        let ms = slot.ns.load(Ordering::Relaxed) as f64 * 1e-6;
        let calls = slot.calls.load(Ordering::Relaxed);
        if ms < min_ms || calls == 0 {
            continue;
        }

        let worst = slot.worst.load(Ordering::Relaxed) as f64 * 1e-6;
        out.push_str(&format!("  {:<34} {ms:>10.2} ms {calls:>9} calls {:>9.4} ms/call  worst {worst:>8.2} ms\n", slot.name, ms / calls as f64));
    }

    if reset_after {
        reset();
    }

    out
}
