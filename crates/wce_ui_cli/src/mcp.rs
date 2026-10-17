//! MCP over stdio: newline-delimited JSON-RPC 2.0, enough of the protocol for tools.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::{Session, tools};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn respond(out: &mut impl Write, id: &Value, result: Value) {
    let _ = writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    let _ = out.flush();
}

fn respond_err(out: &mut impl Write, id: &Value, code: i64, msg: &str) {
    let _ = writeln!(out, "{}", json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }));
    let _ = out.flush();
}

pub fn serve(session: &mut Session, input: impl BufRead, mut out: impl Write) {
    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            respond_err(&mut out, &Value::Null, -32700, "parse error");
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else { continue }; // notifications get no answer
        match method {
            "initialize" => {
                let ver = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("2024-11-05");
                respond(
                    &mut out,
                    &id,
                    json!({
                        "protocolVersion": ver,
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "wce-ui", "version": env!("CARGO_PKG_VERSION") },
                        "instructions": "Edit Total War UI layouts. ui_files lists layouts in the game packs, ui_open opens one (converted to XML), ui_tree/ui_entry/ui_xml inspect it, ui_set_* / ui_add / ui_delete / ui_reparent edit it, ui_render shows a PNG, ui_save writes it into a mod pack."
                    }),
                );
            }
            "ping" => respond(&mut out, &id, json!({})),
            "tools/list" => respond(&mut out, &id, json!({ "tools": tools::list() })),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match session.call(name, &args) {
                    Ok(r) => {
                        let mut content = vec![json!({ "type": "text", "text": r.text })];
                        if let Some(png) = r.png {
                            content.push(json!({ "type": "image", "data": base64(&png), "mimeType": "image/png" }));
                        }
                        respond(&mut out, &id, json!({ "content": content }));
                    }
                    Err(e) => respond(&mut out, &id, json!({ "content": [{ "type": "text", "text": e }], "isError": true })),
                }
            }
            _ => respond_err(&mut out, &id, -32601, "method not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_known() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
