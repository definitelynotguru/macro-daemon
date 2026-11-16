use std::path::PathBuf;

use rune::AppSpec;

use crate::editor::{select_opening, view};
use crate::workspace::Workspace;

pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--validate") {
        let dir = profile_dir_from(&args);
        std::process::exit(run_validate(&dir));
    }
    rune::set_app_id("macro-studio");
    rune::run(AppSpec {
        scenes: &[("delay", select_opening)],
        ..AppSpec::new("Macro Editor", (1080.0, 640.0), init_state, view)
    });
}

fn init_state() -> Workspace {
    let mut workspace = Workspace::new();
    workspace.load_dir(&profiles_dir());
    select_opening(&mut workspace);
    workspace
}

fn profiles_dir() -> PathBuf {
    profile_dir_from(rune::extra_args())
}

fn profile_dir_from(args: &[String]) -> PathBuf {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("profiles"))
}

pub fn run_validate(dir: &PathBuf) -> i32 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!("FAIL: cannot read dir {}", dir.display());
        return 1;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        eprintln!("FAIL: no profiles in {}", dir.display());
        return 1;
    }
    let mut failures = 0;
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
    if failures == 0 { 0 } else { 1 }
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
