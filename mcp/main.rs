//! Stdio MCP. Newline-delimited JSON-RPC, plus a Content-Length frame for older clients.
//! stdout is protocol only.
use std::io::{self, BufRead, Write};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

mod tools;

fn main() {
    if let Err(err) = serve() {
        eprintln!("macro-mcp: {err:#}");
        std::process::exit(1);
    }
}

fn serve() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    loop {
        let Some(message) = read_message(&mut reader)? else {
            return Ok(());
        };
        dispatch(&message);
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("content-length:") {
            return read_framed(reader, trimmed).map(Some);
        }
        match serde_json::from_str(trimmed) {
            Ok(message) => return Ok(Some(message)),
            Err(err) => eprintln!("macro-mcp: ignoring invalid JSON: {err}"),
        }
    }
}

fn read_framed(reader: &mut impl BufRead, first: &str) -> Result<Value> {
    let mut length = content_length(first)?;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header)?;
        if n == 0 {
            bail!("truncated MCP frame");
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().context("Content-Length")?;
        }
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("parsing framed JSON-RPC body")
}

fn content_length(header: &str) -> Result<usize> {
    let value = header
        .split_once(':')
        .map(|(_, value)| value.trim())
        .context("Content-Length header")?;
    value.parse().context("Content-Length")
}

fn dispatch(message: &Value) {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let Some(id) = message.get("id").cloned() else {
        return;
    };
    let result = match method {
        "initialize" => Ok(initialize(message)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list()),
        "tools/call" => tool_call(message),
        other => Err(json!({
            "code": -32601,
            "message": format!("unknown method {other}"),
        })),
    };
    let response = match result {
        Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    };
    write_message(&response);
}

fn initialize(message: &Value) -> Value {
    let requested = message
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");
    let version = if requested.is_empty() {
        "2024-11-05"
    } else {
        requested
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "macro-daemon-mcp",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "resolve_key",
                "description": "Resolve a key name through the daemon keymap. Accepts E, Numpad 4, KEY_KP4, and KP4. Unknown names are an error, not a guess.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }
            },
            {
                "name": "bind_hold",
                "description": "Write or update one profile so a trigger press plays a macro that holds another key for hold_ms milliseconds. The file is the daemon's ProfileFile JSON.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "profiles_dir": { "type": "string" },
                        "profile": { "type": "string" },
                        "trigger": { "type": "string" },
                        "hold_key": { "type": "string" },
                        "hold_ms": { "type": "integer", "minimum": 0, "maximum": 60000 }
                    },
                    "required": ["profiles_dir", "profile", "trigger", "hold_key", "hold_ms"],
                    "additionalProperties": false
                }
            },
            {
                "name": "validate",
                "description": "Load every profile in a directory the way macro-daemon --validate does. Fails when ProfileSet::load_dir fails.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "profiles_dir": { "type": "string" }
                    },
                    "required": ["profiles_dir"],
                    "additionalProperties": false
                }
            },
            {
                "name": "hold_proof",
                "description": "Run a hold through the daemon engine and return min, median, and max. Engine-level only. This does not inject through /dev/uinput.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "hold_ms": { "type": "integer", "minimum": 0, "maximum": 60000 },
                        "iters": { "type": "integer", "minimum": 1, "maximum": 30 }
                    },
                    "required": ["hold_ms", "iters"],
                    "additionalProperties": false
                }
            }
        ]
    })
}

fn tool_call(message: &Value) -> Result<Value, Value> {
    let name = message
        .pointer("/params/name")
        .and_then(Value::as_str)
        .ok_or_else(|| rpc_error(-32602, "tools/call requires params.name"))?;
    let arguments = message
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match dispatch_tool(name, &arguments) {
        Ok(value) => Ok(tool_result(&value, false)),
        Err(err) => Ok(tool_result(&json!(err.to_string()), true)),
    }
}

fn dispatch_tool(name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "resolve_key" => call_resolve(arguments),
        "bind_hold" => call_bind(arguments),
        "validate" => call_validate(arguments),
        "hold_proof" => call_proof(arguments),
        other => bail!("unknown tool {other}"),
    }
}

fn call_resolve(arguments: &Value) -> Result<Value> {
    let name = arguments
        .get("name")
        .and_then(Value::as_str)
        .context("resolve_key requires name")?;
    let key = tools::resolve_key(name)?;
    Ok(serde_json::to_value(key)?)
}

fn call_bind(arguments: &Value) -> Result<Value> {
    let hold_ms = required_u32(arguments, "hold_ms")?;
    tools::bind_hold(tools::BindHold {
        profiles_dir: required_string(arguments, "profiles_dir")?.into(),
        profile: required_string(arguments, "profile")?,
        trigger: required_string(arguments, "trigger")?,
        hold_key: required_string(arguments, "hold_key")?,
        hold_ms,
    })
}

fn call_validate(arguments: &Value) -> Result<Value> {
    let dir = required_string(arguments, "profiles_dir")?;
    tools::validate_profiles(std::path::Path::new(&dir))
}

fn call_proof(arguments: &Value) -> Result<Value> {
    let hold_ms = required_u32(arguments, "hold_ms")?;
    let iters = required_u32(arguments, "iters")? as usize;
    tools::hold_proof(hold_ms, iters)
}

fn required_string(arguments: &Value, key: &str) -> Result<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("{key} must be a string"))
}

fn required_u32(arguments: &Value, key: &str) -> Result<u32> {
    let value = arguments
        .get(key)
        .with_context(|| format!("missing {key}"))?;
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .with_context(|| format!("{key} must be an integer"))
}

fn tool_result(value: &Value, is_error: bool) -> Value {
    let text = if is_error {
        value.as_str().unwrap_or("error").to_string()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

fn rpc_error(code: i32, message: impl Into<String>) -> Value {
    json!({"code": code, "message": message.into()})
}

fn write_message(message: &Value) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{message}");
    let _ = out.flush();
}
