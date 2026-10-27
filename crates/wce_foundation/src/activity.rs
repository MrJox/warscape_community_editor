//! What long work is doing, for progress windows: [`start`] returns a guard that marks a labelled piece of work as going on until it is
//! dropped, then keeps it among the finished ones with its time. Any thread may start one (work spread over a thread pool shows each of
//! its pieces); a window reads [`snapshot`] each frame. One registry for the process: whoever shows it [`clear`]s it before its work.
//! A mutex per start and end, so the pieces are coarse (a compile stage, a climate's trees), not inner loops.

use std::sync::{LazyLock, Mutex};
use std::time::Instant;

/// The finished pieces kept (the oldest go first).
const KEEP: usize = 256;

#[derive(Default)]
struct State {
    next: u64,
    active: Vec<(u64, String, Instant)>,
    done: Vec<(String, f32)>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A piece of work going on, until dropped.
#[must_use = "the activity ends when the guard is dropped"]
pub struct Activity(u64);

/// Marks a piece of work as going on.
pub fn start(label: impl Into<String>) -> Activity {
    let mut s = state();
    s.next += 1;
    let id = s.next;
    s.active.push((id, label.into(), Instant::now()));
    Activity(id)
}

impl Drop for Activity {
    fn drop(&mut self) {
        let mut s = state();
        if let Some(i) = s.active.iter().position(|(id, ..)| *id == self.0) {
            let (_, label, at) = s.active.remove(i);
            s.done.push((label, at.elapsed().as_secs_f32()));
            if s.done.len() > KEEP {
                let extra = s.done.len() - KEEP;
                s.done.drain(..extra);
            }
        }
    }
}

/// The work going on (label, seconds so far; in start order) and the finished pieces (label, seconds; in end order).
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub active: Vec<(String, f32)>,
    pub done: Vec<(String, f32)>,
}

pub fn snapshot() -> Snapshot {
    let s = state();
    Snapshot { active: s.active.iter().map(|(_, l, at)| (l.clone(), at.elapsed().as_secs_f32())).collect(), done: s.done.clone() }
}

/// Forgets the finished pieces (the ones going on stay).
pub fn clear() {
    state().done.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_go_from_active_to_done() {
        let a = start("test.a");
        let b = start("test.b");
        assert!(snapshot().active.iter().any(|(l, _)| l == "test.a"));
        drop(a);
        let s = snapshot();
        assert!(s.done.iter().any(|(l, _)| l == "test.a") && !s.active.iter().any(|(l, _)| l == "test.a"));
        assert!(s.active.iter().any(|(l, _)| l == "test.b"));
        drop(b);
    }
}
