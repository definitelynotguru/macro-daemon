//! Per-application profile selection.
//!
//! MACRO_SPEC.md §D.4: the recovered engine selects a configuration per **executable path**
//! (`applications.path`), never by window title. We keep that rule and get the focused
//! application from whichever session is running:
//!
//! * Wayland/Wayland-compositor: `hyprctl activewindow -j`, then `swaymsg -t get_tree`
//! * X11: `xdotool getactivewindow getwindowpid`, then `xprop`
//!
//! Deliberately shell-based: the scope allows no extra crates, and every one of these probes is
//! a stable, documented CLI. A missing tool is never fatal — we log once and stay on the
//! current profile, because the trigger loop must never block on focus detection.
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

/// Which focus probe worked last time, so we stop paying for failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Hyprland,
    Sway,
    Xdotool,
    Xprop,
    None,
}

pub struct FocusWatcher {
    probe: Probe,
    warned: bool,
}

impl Default for FocusWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusWatcher {
    pub fn new() -> Self {
        Self {
            probe: Probe::None,
            warned: false,
        }
    }

    /// Current focused application executable, or `None` if it cannot be determined.
    pub fn active_app(&mut self) -> Option<PathBuf> {
        // Stick to a probe that already worked; otherwise try in order.
        if self.probe != Probe::None {
            if let Some(p) = self.try_probe(self.probe) {
                return Some(p);
            }
        }
        for probe in [Probe::Hyprland, Probe::Sway, Probe::Xdotool, Probe::Xprop] {
            if let Some(p) = self.try_probe(probe) {
                self.probe = probe;
                return Some(p);
            }
        }
        self.probe = Probe::None;
        if !self.warned {
            self.warned = true;
            tracing::warn!(
                "no focus probe available (hyprctl / swaymsg / xdotool / xprop); \
                 per-application rules disabled, staying on the current profile"
            );
        }
        None
    }

    fn try_probe(&self, probe: Probe) -> Option<PathBuf> {
        match probe {
            Probe::Hyprland => hyprland_active(),
            Probe::Sway => sway_active(),
            Probe::Xdotool => xdotool_active(),
            Probe::Xprop => xprop_active(),
            Probe::None => None,
        }
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn pid_to_exe(pid: u64) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

fn hyprland_active() -> Option<PathBuf> {
    let raw = run("hyprctl", &["activewindow", "-j"])?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let pid = v.get("pid").and_then(Value::as_u64).unwrap_or(0);
    if let Some(p) = pid_to_exe(pid) {
        return Some(p);
    }
    // Some clients report class only; the class is a process name, not a path, so it is the
    // last resort and still path-shaped for the matcher.
    v.get("class")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(PathBuf::from)
}

fn sway_active() -> Option<PathBuf> {
    let raw = run("swaymsg", &["-t", "get_tree", "-r"])?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let pid = find_focused_pid(&v)?;
    pid_to_exe(pid)
}

fn find_focused_pid(node: &Value) -> Option<u64> {
    if node.get("focused").and_then(Value::as_bool) == Some(true) {
        if let Some(pid) = node.get("pid").and_then(Value::as_u64) {
            if pid != 0 {
                return Some(pid);
            }
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(list) = node.get(key).and_then(Value::as_array) {
            for child in list {
                if let Some(pid) = find_focused_pid(child) {
                    return Some(pid);
                }
            }
        }
    }
    None
}

fn xdotool_active() -> Option<PathBuf> {
    let raw = run("xdotool", &["getactivewindow", "getwindowpid"])?;
    let pid: u64 = raw.trim().parse().ok()?;
    pid_to_exe(pid)
}

fn xprop_active() -> Option<PathBuf> {
    let root = run("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
    // _NET_ACTIVE_WINDOW(WINDOW): window id # 0x3e00007
    let wid = root.split('#').nth(1)?.trim().to_string();
    if wid.is_empty() || wid == "0x0" {
        return None;
    }
    let pid_prop = run("xprop", &["-id", &wid, "_NET_WM_PID"])?;
    let pid: u64 = pid_prop.split('=').nth(1)?.trim().parse().ok()?;
    pid_to_exe(pid)
}

/// Poll focus in a background thread, emitting only on change.
pub fn spawn_watcher(poll_ms: u64, tx: Sender<Option<PathBuf>>, stop: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("focus-watcher".into())
        .spawn(move || {
            let mut watcher = FocusWatcher::new();
            let mut last: Option<Option<PathBuf>> = None;
            while !stop.load(Ordering::Relaxed) {
                let current = watcher.active_app();
                if last.as_ref() != Some(&current) {
                    let _ = tx.send(current.clone());
                    last = Some(current);
                }
                thread::sleep(Duration::from_millis(poll_ms.max(50)));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sway_tree_walk_finds_focused_pid() {
        let tree = serde_json::json!({
            "focused": false,
            "nodes": [
                {"focused": false, "nodes": []},
                {"focused": true, "pid": 4242, "nodes": []}
            ]
        });
        assert_eq!(find_focused_pid(&tree), Some(4242));
    }

    #[test]
    fn sway_tree_without_focus_is_none() {
        let tree = serde_json::json!({"focused": false, "nodes": []});
        assert_eq!(find_focused_pid(&tree), None);
    }
}
