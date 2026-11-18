use crate::config::{Config, Result};
use crate::process::{spawn_command, stop};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

const MAX_LINE: u64 = 1024 * 1024;
const VERSION: &str = "2025-06-18";

fn send(output: &mut impl Write, message: Value) -> Result<()> {
    serde_json::to_writer(&mut *output, &message).map_err(|e| e.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|e| e.to_string())
}
fn response(input: &Receiver<Result<Value>>, id: u64, until: Instant) -> Result<Value> {
    loop {
        let budget = until
            .checked_duration_since(Instant::now())
            .ok_or("MCP call timed out")?;
        let msg = input
            .recv_timeout(budget)
            .map_err(|e| format!("MCP response: {e}"))??;
        if msg["jsonrpc"] != "2.0" {
            return Err("invalid MCP JSON-RPC envelope".into());
        }
        if msg["id"] != id {
            continue;
        }
        if !msg["error"].is_null() {
            return Err(format!("MCP server error: {}", msg["error"]));
        }
        if !msg["result"].is_object() {
            return Err("missing MCP result".into());
        }
        return Ok(msg["result"].clone());
    }
}
pub fn call(
    cfg: &Config,
    dir: &Path,
    tool: &str,
    descriptor: &Value,
    args: Value,
) -> Result<Value> {
    if !args.is_object() || args.to_string().len() > 128 * 1024 {
        return Err("tool arguments must be an object under 128 KiB".into());
    }
    if descriptor["type"] == "document" {
        return Ok(descriptor.clone());
    }
    if descriptor["type"] != "mcp" {
        return Err("tool type must be document or mcp".into());
    }
    let spec = cfg
        .approved_tools
        .get(tool)
        .ok_or("MCP tool is not approved in this machine configuration")?;
    let mut child = spawn_command(&spec.runtime, dir)?;
    let mut out = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let (messages, rx) = sync_channel(8);
    thread::spawn(move || {
        let mut stream = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            let message = match (&mut stream)
                .take(MAX_LINE + 1)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => break,
                Ok(n) if n as u64 > MAX_LINE => Err("MCP response exceeds 1 MiB".into()),
                Ok(_) => serde_json::from_slice(&line).map_err(|e| format!("MCP JSON: {e}")),
                Err(e) => Err(e.to_string()),
            };
            let invalid = message.is_err();
            if messages.send(message).is_err() || invalid {
                break;
            }
        }
    });
    thread::spawn(move || {
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });
    let until = Instant::now() + Duration::from_secs(spec.runtime.timeout_seconds.min(25));
    let outcome = (|| {
        send(
            &mut out,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"shipforge-worker","version":env!("CARGO_PKG_VERSION")}}}),
        )?;
        let init = response(&rx, 1, until)?;
        if init["protocolVersion"] != VERSION || !init["capabilities"]["tools"].is_object() {
            return Err("MCP server does not support the configured tools protocol".into());
        }
        send(
            &mut out,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )?;
        send(
            &mut out,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":spec.tool_name,"arguments":args}}),
        )?;
        response(&rx, 2, until)
    })();
    drop(out);
    stop(&mut child);
    outcome
}
