use std::path::PathBuf;

use macro_studio::editor::{select_opening, view};
use macro_studio::workspace::Workspace;
use rune::{Env, Runtime};

#[test]
fn opening_frame_selects_a_macro_and_shows_a_delay_chip() {
    let mut ws = Workspace::new();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../profiles");
    ws.load_dir(&dir);
    select_opening(&mut ws);

    let env = Env { system_dark: true, scale: 1.0 };
    let mut rt = Runtime::new(ws, view, (1080.0, 640.0), env);
    rt.frame(0.0);

    let elements = rt.elements();
    let delay = elements.iter().find(|el| {
        el.text
            .as_deref()
            .is_some_and(|text| text.ends_with(" ms") && text.chars().next().is_some_and(|c| c.is_ascii_digit()))
    });
    assert!(delay.is_some(), "opening frame must show a millisecond delay chip");

    let selected = elements.iter().find(|el| {
        el.key.as_deref().is_some_and(|key| key.starts_with("row/"))
            && el.fill.r > el.fill.g
            && el.fill.r > el.fill.b
    });
    assert!(selected.is_some(), "a macro row must be filled orange");
    assert!(
        elements.iter().any(|el| el.text.as_deref() == Some("MACROS")),
        "the rail must be titled MACROS"
    );
    assert!(
        elements.iter().any(|el| el.text.as_deref() == Some("SAVE")),
        "SAVE must be on the bottom bar"
    );
}
