//! The sentence is the client. It must not send HID usages. The server resolves names.
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Instant;

use macro_daemon::clock::RealWait;
use macro_daemon::config::{PlaybackOptions, PlaybackType, ProfileFile};
use macro_daemon::engine::Runner;
use macro_daemon::inject::{Injector, MockEvent, MockInjector, WheelAxis};
use macro_daemon::keymap::Keymap;
use serde_json::{json, Value};

struct Rpc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl Rpc {
    fn spawn() -> Self {
        let bin = env!("CARGO_BIN_EXE_macro-mcp");
        let mut child = Command::new(bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("macro-mcp starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({"jsonrpc": "2.0", "method": method}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let line = self.read_line();
        let message: Value = serde_json::from_str(&line).expect("response is JSON");
        assert_eq!(message["id"], id, "response id: {line}");
        assert!(message.get("error").is_none(), "rpc error: {line}");
        message["result"].clone()
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write");
        self.stdin.flush().expect("flush");
    }

    fn read_line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read");
        assert!(!line.is_empty(), "server closed stdout");
        line
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .expect("tool text")
        .to_string()
}

fn tool_json(result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{result}");
    serde_json::from_str(&tool_text(result)).expect("tool JSON")
}

#[test]
fn sentence_authors_a_numpad4_hold_of_e_through_stdio() {
    let mut rpc = Rpc::spawn();
    let init = rpc.request(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sentence", "version": "0"}
        }),
    );
    assert_eq!(init["serverInfo"]["name"], "macro-daemon-mcp");
    rpc.notify("notifications/initialized");

    let listed = rpc.request("tools/list", json!({}));
    let names: Vec<&str> = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["resolve_key", "bind_hold", "validate", "hold_proof"]
    );

    let unknown = rpc.request(
        "tools/call",
        json!({"name": "resolve_key", "arguments": {"name": "NotAKey"}}),
    );
    assert_eq!(unknown["isError"], true, "{unknown}");
    assert!(
        tool_text(&unknown).contains("unknown key"),
        "{}",
        tool_text(&unknown)
    );

    let trigger = tool_json(&rpc.request(
        "tools/call",
        json!({"name": "resolve_key", "arguments": {"name": "Numpad 4"}}),
    ));
    let hold = tool_json(&rpc.request(
        "tools/call",
        json!({"name": "resolve_key", "arguments": {"name": "E"}}),
    ));
    assert_eq!(trigger["evdev_name"], "KEY_KP4");
    assert_eq!(trigger["hid_usage"], 92, "server check: Numpad 4 is HID 92");
    assert_eq!(trigger["page"], 1);
    assert_eq!(trigger["evdev"], 75, "server check: Numpad 4 is evdev 75");
    assert_eq!(hold["evdev_name"], "KEY_E");
    assert_eq!(hold["hid_usage"], 8, "server check: E is HID 8");
    assert_eq!(hold["page"], 1);
    assert_eq!(hold["evdev"], 18, "server check: E is evdev 18");

    let dir = tempfile::tempdir().unwrap();
    let written = tool_json(&rpc.request(
        "tools/call",
        json!({
            "name": "bind_hold",
            "arguments": {
                "profiles_dir": dir.path().display().to_string(),
                "profile": "sentence",
                "trigger": "Numpad 4",
                "hold_key": "E",
                "hold_ms": 400
            }
        }),
    ));
    assert_eq!(written["trigger"]["hid_usage"], trigger["hid_usage"]);
    assert_eq!(written["hold_key"]["hid_usage"], hold["hid_usage"]);

    let validated = tool_json(&rpc.request(
        "tools/call",
        json!({
            "name": "validate",
            "arguments": {"profiles_dir": dir.path().display().to_string()}
        }),
    ));
    assert_eq!(validated["ok"], true, "{validated}");

    let path = written["path"].as_str().unwrap();
    let file = load_profile(Path::new(path));
    file.validate().expect("ProfileFile::validate");
    assert_eq!(file.bindings.len(), 1);
    assert_eq!(file.bindings[0].trigger_data.hid_code, 92);
    assert_eq!(file.bindings[0].trigger_data.direction, 1);
    assert_eq!(
        file.bindings[0]
            .playback_options_data
            .playback_type
            .to_client(),
        0
    );
    let events = &file.macros[0].events;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].code, hold["hid_usage"].as_u64().unwrap() as u16);
    assert_eq!(events[0].extra_data, 1);
    assert!(events[1].event_type.is_delay());
    assert_eq!(events[1].extra_data, 400);
    assert_eq!(events[2].extra_data, 0);

    let keymap = Keymap::load_embedded().unwrap();
    let evdev = keymap
        .lookup(events[0].page, events[0].code)
        .expect("written hold key is in the keymap")
        .evdev;
    assert_eq!(evdev, hold["evdev"].as_i64().unwrap() as i32);

    let samples = engine_hold(events, evdev, 5);
    let (min, median, max) = summarize(&samples);
    println!(
        "engine-level written macro hold ms: min {min:.3} median {median:.3} max {max:.3} samples {samples:?}"
    );
    assert!(
        (median - 400.0).abs() <= 10.0,
        "written macro median {median} is not within 10 ms of 400"
    );

    let proof = tool_json(&rpc.request(
        "tools/call",
        json!({"name": "hold_proof", "arguments": {"hold_ms": 400, "iters": 5}}),
    ));
    println!("engine-level hold_proof: {proof}");
    assert_eq!(proof["level"], "engine");
    assert_eq!(proof["iters"], 5);
    let proof_median = proof["median_ms"].as_f64().unwrap();
    assert!(
        (proof_median - 400.0).abs() <= 10.0,
        "hold_proof median {proof_median} is not within 10 ms of 400"
    );
}

#[test]
fn framed_initialize_gets_a_framed_response() {
    let bin = env!("CARGO_BIN_EXE_macro-mcp");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("macro-mcp starts");
    let mut stdin = child.stdin.take().expect("stdin");
    let body = r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"framed","version":"0"}}}"#;
    write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write frame");
    stdin.flush().expect("flush");
    drop(stdin);
    let mut stdout = child.stdout.take().expect("stdout");
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut stdout, &mut bytes).expect("read");
    let text = String::from_utf8(bytes).expect("utf-8");
    let header_end = text
        .find("\r\n\r\n")
        .unwrap_or_else(|| panic!("framed response expected, got {text:?}"));
    assert!(
        text[..header_end].starts_with("Content-Length:"),
        "{text:?}"
    );
    let length: usize = text[..header_end]
        .trim_start_matches("Content-Length:")
        .trim()
        .parse()
        .expect("length");
    let payload = &text[header_end + 4..];
    assert_eq!(payload.len(), length, "{text:?}");
    let parsed: Value = serde_json::from_str(payload).expect("json body");
    assert_eq!(parsed["id"], 7);
    assert_eq!(parsed["result"]["serverInfo"]["name"], "macro-daemon-mcp");
    let _ = child.wait();
}

fn load_profile(path: &Path) -> ProfileFile {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn engine_hold(events: &[macro_daemon::config::MacroEvent], evdev: i32, iters: usize) -> Vec<f64> {
    let keymap = Keymap::load_embedded().unwrap();
    let opts = PlaybackOptions {
        playback_type: PlaybackType::PlayOnce,
        ..PlaybackOptions::default()
    };
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let mut mock = MockInjector::new();
        let mut clocked = Clocked {
            inner: &mut mock,
            stamps: Vec::new(),
        };
        let mut wait = RealWait;
        let mut runner = Runner::new(&mut clocked, &keymap, &mut wait);
        runner.execute(events, &opts, &mut |_| true).unwrap();
        samples.push(hold_ms(&clocked, evdev).expect("press and release"));
    }
    samples
}

struct Clocked<'a> {
    inner: &'a mut MockInjector,
    stamps: Vec<Instant>,
}

impl Injector for Clocked<'_> {
    fn key(&mut self, code: i32, pressed: bool) -> anyhow::Result<()> {
        self.stamps.push(Instant::now());
        self.inner.key(code, pressed)
    }
    fn button(&mut self, code: i32, pressed: bool) -> anyhow::Result<()> {
        self.stamps.push(Instant::now());
        self.inner.button(code, pressed)
    }
    fn wheel(&mut self, axis: WheelAxis, delta: i32) -> anyhow::Result<()> {
        self.stamps.push(Instant::now());
        self.inner.wheel(axis, delta)
    }
    fn sync(&mut self) -> anyhow::Result<()> {
        self.stamps.push(Instant::now());
        self.inner.sync()
    }
    fn describe(&self) -> &str {
        "sentence clock"
    }
}

fn hold_ms(clocked: &Clocked<'_>, evdev: i32) -> Option<f64> {
    let mut press = None;
    let mut release = None;
    for (index, event) in clocked.inner.events.iter().enumerate() {
        match event {
            MockEvent::Key {
                code,
                pressed: true,
            } if *code == evdev => {
                press = press.or(Some(index));
            }
            MockEvent::Key {
                code,
                pressed: false,
            } if *code == evdev => release = Some(index),
            _ => {}
        }
    }
    let (press, release) = press.zip(release)?;
    let start = clocked.stamps.get(press)?;
    let end = clocked.stamps.get(release)?;
    Some(end.duration_since(*start).as_secs_f64() * 1000.0)
}

fn summarize(samples: &[f64]) -> (f64, f64, f64) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    (sorted[0], median, sorted[n - 1])
}
