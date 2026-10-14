//! Every menu action is a named command with string arguments, so the same entry serves the menu, the command line (`--cmd "name args"`),
//! script files (`--script file`) and tests. Tools register their own commands next to the editor's.
//!
//! Script files: one command per line, `#` starts a comment, arguments split on spaces (double quotes group). A line starting with `-` may fail
//! without stopping the script. A command can hold the script for some rendered frames (`frames N`) or ask to run again after the next frame
//! (`wait_loaded`) through its result.
//!
//! Commands run on a host (the editor) that owns the registry: a command gets the host to act on, and [`CommandRegistry::run`] takes the host,
//! not the registry, so that commands can reach everything the host owns.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use wce_foundation::log;

pub type Args = Vec<String>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandResult {
    pub ok: bool,
    pub message: String,

    /// The script runner waits this many rendered frames before the next line.
    pub wait_frames: i32,

    /// The script runner runs this line again after the next frame instead of moving on.
    pub repeat: bool,
}

impl CommandResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self { ok: true, message: message.into(), ..Default::default() }
    }

    pub fn fail(message: impl Into<String>) -> Self {
        Self { ok: false, message: message.into(), ..Default::default() }
    }

    /// Holds a running script for `frames` rendered frames.
    pub fn wait(mut self, frames: i32) -> Self {
        self.wait_frames = frames;
        self
    }

    /// Runs the script line again after the next frame.
    pub fn again(mut self) -> Self {
        self.repeat = true;
        self
    }
}

/// What owns the commands.
pub trait CommandHost: Sized + 'static {
    fn commands(&self) -> &CommandRegistry<Self>;

    /// Lines a command wants run next in the running script (the `script` command includes a file this way): the script runner takes them
    /// after each line it runs and puts them right after it.
    fn take_script_lines(&mut self) -> Vec<String> {
        vec![]
    }
}

pub type CommandFn<H> = Rc<dyn Fn(&mut H, &[String]) -> CommandResult>;

pub struct Entry<H> {
    pub name: String,
    pub usage: String,
    pub help: String,
    pub run: CommandFn<H>,
}

pub struct CommandRegistry<H> {
    entries: BTreeMap<String, Entry<H>>,
}

impl<H> Default for CommandRegistry<H> {
    fn default() -> Self {
        Self { entries: BTreeMap::new() }
    }
}

impl<H: CommandHost> CommandRegistry<H> {

    /// Replaces an earlier registration of the same name.
    pub fn add(&mut self, name: &str, usage: &str, help: &str, run: impl Fn(&mut H, &[String]) -> CommandResult + 'static) {
        self.entries.insert(name.to_owned(), Entry { name: name.to_owned(), usage: usage.to_owned(), help: help.to_owned(), run: Rc::new(run) });
    }

    pub fn find(&self, name: &str) -> Option<&Entry<H>> {
        self.entries.get(name)
    }

    /// Sorted by name.
    pub fn entries(&self) -> impl Iterator<Item = &Entry<H>> {
        self.entries.values()
    }

    /// Tokenizes and runs one command line on the host.
    pub fn run(host: &mut H, line: &str) -> CommandResult {
        let mut t = Self::tokenize(line);
        if t.is_empty() {
            return CommandResult::success("");
        }

        let name = t.remove(0);
        Self::run_args(host, &name, &t)
    }

    /// Runs a command on the host. A panic in the command becomes a failed result.
    pub fn run_args(host: &mut H, name: &str, args: &[String]) -> CommandResult {
        let Some(run) = host.commands().find(name).map(|e| e.run.clone()) else { return CommandResult::fail(format!("unknown command '{name}'")) };
        match catch_unwind(AssertUnwindSafe(|| run(host, args))) {
            Ok(result) => result,
            Err(panic) => {
                let what = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
                CommandResult::fail(format!("{name}: {what}"))
            }
        }
    }

    /// Splits a line on spaces and tabs; double quotes group (an empty pair is an empty argument), `#` outside an argument starts a comment.
    pub fn tokenize(line: &str) -> Args {
        let mut out = vec![];
        let mut cur = Vec::new();
        let (mut in_quote, mut have) = (false, false);
        let push = |cur: &mut Vec<u8>, out: &mut Args| out.push(String::from_utf8_lossy(&std::mem::take(cur)).into_owned());
        for &c in line.as_bytes() {
            if in_quote {
                if c == b'"' {
                    in_quote = false;
                } else {
                    cur.push(c);
                }
            } else if c == b'"' {
                in_quote = true;
                have = true;
            } else if c == b' ' || c == b'\t' || c == b'\r' {
                if have {
                    push(&mut cur, &mut out);
                    have = false;
                }
            } else if c == b'#' && !have {
                break;
            } else {
                cur.push(c);
                have = true;
            }
        }

        if have {
            push(&mut cur, &mut out);
        }

        out
    }
}

/// Runs a script, some lines per rendered frame.
#[derive(Clone, Debug, Default)]
pub struct ScriptRunner {
    lines: Vec<String>,
    next: usize,
    wait: i32,
    failed: bool,
    error: String,
}

/// The lines of a script file.
///
/// # Errors
///
/// The file can't be read.
pub fn read_script(path: &str) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|_| format!("cannot open script {path}"))?;
    Ok(split_lines(&String::from_utf8_lossy(&bytes)))
}

fn split_lines(text: &str) -> Vec<String> {
    // getline: a final newline does not start another line.
    let mut lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }

    lines
}

impl ScriptRunner {
    pub fn load_text(&mut self, text: &str) {
        *self = Self { lines: split_lines(text), ..Default::default() };
    }

    /// # Errors
    ///
    /// The file can't be read.
    pub fn load_file(&mut self, path: &str) -> Result<(), String> {
        let lines = read_script(path)?;
        *self = Self { lines, ..Default::default() };
        Ok(())
    }

    /// Inserts the file's lines right after the current position (the `script` command), so the rest of the running script still follows.
    ///
    /// # Errors
    ///
    /// The file can't be read.
    pub fn include_file(&mut self, path: &str) -> Result<(), String> {
        self.include_lines(read_script(path)?);
        Ok(())
    }

    /// Inserts lines right after the current position.
    pub fn include_lines(&mut self, lines: Vec<String>) {
        self.lines.splice(self.next..self.next, lines);
    }

    pub fn done(&self) -> bool {
        self.next >= self.lines.len() && self.wait == 0
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    /// Runs lines until one asks to wait (or the script ends or fails). Call once per rendered frame; returns false once the script has failed.
    pub fn step<H: CommandHost>(&mut self, host: &mut H) -> bool {
        if self.failed {
            return false;
        }

        if self.wait > 0 {
            self.wait -= 1;
            return true;
        }

        while self.next < self.lines.len() {
            let mut line = self.lines[self.next].clone();
            self.next += 1;
            let Some(first) = line.find(|c| !matches!(c, ' ' | '\t' | '\r')) else { continue };
            if line[first..].starts_with('#') {
                continue;
            }

            let optional = line[first..].starts_with('-');
            if optional {
                line.replace_range(..=first, "");
            }

            let r = CommandRegistry::run(host, &line);
            let included = host.take_script_lines();
            if !included.is_empty() {
                self.include_lines(included);
            }

            if !r.message.is_empty() {
                log::info(format!("{line} -> {}", r.message));
            }

            if !r.ok && !optional {
                self.failed = true;
                self.error = format!("script line {}: {line}: {}", self.next, r.message);
                log::error(self.error.clone());
                return false;
            }

            if r.repeat {
                self.next -= 1;
                return true;
            }

            if r.wait_frames > 0 {
                // This frame counts as the first waited.
                self.wait = r.wait_frames - 1;
                return true;
            }
        }

        true
    }
}
