//! The process-wide log: a ring of the last entries (the editor's Log panel shows them), plus optional echoes to stderr (command line
//! runs, scripts) and to a file. Thread safe.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Entries kept.
const KEEP: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    fn tag(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub sequence: u64,
    pub level: LogLevel,
    pub text: String,
}

#[derive(Default)]
struct State {
    entries: VecDeque<LogEntry>,
    sequence: u64,
    echo: bool,
    file: Option<File>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Adds an entry.
pub fn add(level: LogLevel, text: impl Into<String>) {
    let text = text.into();
    let mut state = state();
    let line = format!("[{}] {text}\n", level.tag());
    if state.echo {
        eprint!("{line}");
    }

    if let Some(file) = state.file.as_mut() {
        // The log has nowhere to report its own failures.
        let _ = file.write_all(line.as_bytes()).and_then(|()| file.flush());
    }

    state.sequence += 1;
    let sequence = state.sequence;
    state.entries.push_back(LogEntry { sequence, level, text });
    while state.entries.len() > KEEP {
        state.entries.pop_front();
    }
}

pub fn info(text: impl Into<String>) {
    add(LogLevel::Info, text);
}

pub fn warn(text: impl Into<String>) {
    add(LogLevel::Warning, text);
}

pub fn error(text: impl Into<String>) {
    add(LogLevel::Error, text);
}

/// The entries kept, oldest first.
pub fn entries() -> Vec<LogEntry> {
    state().entries.iter().cloned().collect()
}

/// Entries added so far (changes whenever the log does).
pub fn sequence() -> u64 {
    state().sequence
}

/// Entries of a level currently kept.
pub fn count(level: LogLevel) -> usize {
    state().entries.iter().filter(|entry| entry.level == level).count()
}

pub fn clear() {
    state().entries.clear();
}

pub fn echo_to_stderr(on: bool) {
    state().echo = on;
}

/// Also writes every entry, flushed, to a file (truncated now): what an interactive run logged stays readable after it ends.
///
/// # Errors
///
/// Fails if the file can't be created.
pub fn open_file(path: &Path) -> std::io::Result<()> {
    let file = File::create(path)?;
    state().file = Some(file);
    Ok(())
}
