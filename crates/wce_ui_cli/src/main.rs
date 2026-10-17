//! wce-ui: Total War UI layouts on the command line, and as an MCP server for Claude Code.
//!
//!   wce-ui mcp                         serve the tools over stdio (MCP)
//!   wce-ui <tool> [--key value]...     run one tool; layout tools take --source to open the layout first
//!   wce-ui script <file>               run lines of `<tool> --key value ...` in one session (# comments)
//!   wce-ui tools                       list the tools and their arguments
//!
//! Global option: --game <folder> (default: wce.ini / WCE_GAME / ATTILA_DIR).
//! One-shot edits: `wce-ui set_entry --source x.xml --entry 3 --x 10 --save` opens, edits and saves back (a disk file) in one go.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde_json::{Map, Value, json};
use wce_ui_cli::{Session, mcp, tools};

fn parse_value(s: &str) -> Value {
    if let Ok(i) = s.parse::<i64>() {
        return json!(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        if s.contains('.') {
            return json!(f);
        }
    }
    match s {
        "true" => json!(true),
        "false" => json!(false),
        _ => json!(s),
    }
}

/// `--key value` pairs (a flag with no value is true) and the bare words in order.
fn parse_args(args: &[String]) -> (Map<String, Value>, Vec<String>) {
    let mut m = Map::new();
    let mut bare = vec![];
    let mut i = 0;
    while i < args.len() {
        if let Some(k) = args[i].strip_prefix("--") {
            if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                m.insert(k.replace('-', "_"), parse_value(&args[i + 1]));
                i += 2;
            } else {
                m.insert(k.replace('-', "_"), json!(true));
                i += 1;
            }
        } else {
            bare.push(args[i].clone());
            i += 1;
        }
    }
    (m, bare)
}

/// Splits a script line into words, honouring double quotes.
fn words(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                q = !q;
                any = true;
            }
            c if c.is_whitespace() && !q => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn run_line(s: &mut Session, argv: &[String]) -> Result<(), String> {
    let Some((tool, rest)) = argv.split_first() else { return Ok(()) };
    let (mut a, bare) = parse_args(rest);
    // `wce-ui open ui/x` and `wce-ui tree ui/x`: a bare word is the source
    if let Some(b) = bare.first() {
        a.entry("source").or_insert(json!(b));
    }
    let tool = tool.strip_prefix("ui_").unwrap_or(tool).replace('-', "_");
    let mut tool = tool.as_str();
    let mut save_after = false;
    match tool {
        "to_xml" => tool = "xml",
        "from_xml" => {
            tool = "validate";
            save_after = true;
        }
        _ => {}
    }
    let needs_doc = !matches!(tool, "files" | "open" | "close" | "list_open");
    let save = a.remove("save").is_some_and(|v| v != json!(false)) || save_after;
    let mut opened = false;
    if needs_doc {
        if let Some(src) = a.remove("source") {
            s.call("open", &json!({ "source": src, "id": "cli" }))?;
            a.insert("id".into(), json!("cli"));
            opened = true;
        }
    }
    let r = s.call(tool, &Value::Object(a.clone()))?;
    if tool == "xml" {
        if let Some(out) = a.get("out").and_then(|v| v.as_str()) {
            std::fs::write(out, &r.text).map_err(|e| format!("{out}: {e}"))?;
            println!("wrote {} bytes to {out}", r.text.len());
        } else {
            println!("{}", r.text);
        }
    } else {
        println!("{}", r.text);
    }
    if opened && save {
        let mut sa = Map::new();
        sa.insert("id".into(), json!("cli"));
        for k in ["pack", "path", "out", "format"] {
            if let Some(v) = a.get(k) {
                sa.insert(k.into(), v.clone());
            }
        }
        let r = s.call("save", &Value::Object(sa))?;
        println!("{}", r.text);
    }
    Ok(())
}

fn main() {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let mut game: Option<PathBuf> = None;
    if let Some(p) = argv.iter().position(|a| a == "--game") {
        if p + 1 < argv.len() {
            game = Some(PathBuf::from(argv.remove(p + 1)));
        }
        argv.remove(p);
    }
    if argv.is_empty() || argv[0] == "help" || argv[0] == "--help" {
        println!("{}", include_str!("../usage.txt"));
        return;
    }
    let mut session = Session::new(game);
    match argv[0].as_str() {
        "mcp" => {
            let stdin = std::io::stdin();
            mcp::serve(&mut session, stdin.lock(), std::io::stdout().lock());
        }
        "tools" => {
            for t in tools::list() {
                println!("{}: {}", t["name"].as_str().unwrap_or(""), t["description"].as_str().unwrap_or(""));
                if let Some(p) = t["inputSchema"]["properties"].as_object() {
                    for (k, v) in p {
                        println!("    --{k}  {}", v["description"].as_str().unwrap_or(""));
                    }
                }
            }
        }
        "script" => {
            let Some(path) = argv.get(1) else {
                eprintln!("script needs a file");
                std::process::exit(2);
            };
            let f = std::fs::File::open(path).unwrap_or_else(|e| {
                eprintln!("{path}: {e}");
                std::process::exit(2)
            });
            let mut failed = false;
            for (n, line) in BufReader::new(f).lines().map_while(Result::ok).enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                println!("> {line}");
                if let Err(e) = run_line(&mut session, &words(line)) {
                    eprintln!("line {}: {e}", n + 1);
                    failed = true;
                    break;
                }
            }
            if failed {
                std::process::exit(1);
            }
        }
        _ => {
            if let Err(e) = run_line(&mut session, &argv) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}
