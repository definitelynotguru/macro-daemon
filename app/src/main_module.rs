use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{px, size, App, Bounds, Context, WindowBounds, WindowOptions};

use crate::studio::StudioView;
use crate::workspace::{Status, Workspace};

const WIN_W: f32 = 1280.0;
const WIN_H: f32 = 800.0;

pub fn main() {
    let mut dir = PathBuf::from("profiles");
    let mut validate = false;
    for arg in std::env::args().skip(1) {
        if arg == "--validate" {
            validate = true;
        } else {
            dir = PathBuf::from(arg);
        }
    }
    if validate {
        std::process::exit(run_validate(&dir));
    }
    run_gui(dir);
}

pub fn run_validate(dir: &PathBuf) -> i32 {
    let mut failures = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("FAIL: cannot read dir {}", dir.display());
        return 1;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        eprintln!("FAIL: no profiles in {}", dir.display());
        return 1;
    }
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match validate_one(&path) {
            Ok(()) => println!("OK   {name}"),
            Err(err) => {
                failures += 1;
                println!("FAIL {name}: {err:#}");
            }
        }
    }
    if failures == 0 {
        0
    } else {
        1
    }
}

pub fn validate_one(path: &PathBuf) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let file: macro_daemon::config::ProfileFile = serde_json::from_str(&text)?;
    file.validate()?;
    let round1 = serde_json::to_string(&file)?;
    let reparsed: macro_daemon::config::ProfileFile = serde_json::from_str(&round1)?;
    let round2 = serde_json::to_string(&reparsed)?;
    anyhow::ensure!(round1 == round2, "round-trip mismatch after re-serialize");
    Ok(())
}

pub fn run_gui(dir: PathBuf) {
    gpui::Application::new().run(move |cx: &mut App| {
        let mut ws = Workspace::new();
        ws.load_dir(&dir);
        let err = match &ws.status {
            Status::Error(text) => Some(text.clone()),
            _ => None,
        };
        if let Some(err) = err {
            eprintln!("macro-studio: {err}");
        }
        let workspace: gpui::Entity<Workspace> = cx.new(|_| ws);
        let bounds = Bounds::centered(None, size(px(WIN_W), px(WIN_H)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("macro-studio".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| {
                let workspace_entity = workspace.clone();
                let view = cx.new(move |cx: &mut Context<StudioView>| {
                    let ws_entity = workspace_entity.clone();
                    StudioView::new(ws_entity, cx)
                });
                view
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
