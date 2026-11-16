use std::path::Path;

use macro_daemon::config::{
    binding_types, Binding, EventType, Macro, MacroEvent, PlaybackOptions, PlaybackType,
    ProfileFile, ProfileMeta, TriggerData,
};
use macro_studio::schema::{
    direction_label, event_label, human_trigger, key_name, macro_total_ms, new_macro_id,
    PLAYBACK_TABLE,
};
use macro_studio::workspace::{Status, Workspace};

fn sample_file() -> ProfileFile {
    ProfileFile {
        version: 1,
        profile: ProfileMeta {
            name: "sample".to_string(),
            apps: vec!["/usr/bin/foo".to_string()],
        },
        macros: vec![
            Macro {
                id: "m-1".to_string(),
                name: "ctrl-c".to_string(),
                events: vec![
                    MacroEvent::key(224, true),
                    MacroEvent::key(6, true),
                    MacroEvent::delay(120),
                    MacroEvent::key(6, false),
                    MacroEvent::key(224, false),
                ],
                recording_options: None,
                cloud_last_updated_at: String::new(),
                is_deleted: 0,
                is_new: 0,
            },
            Macro {
                id: "m-2".to_string(),
                name: "wheel".to_string(),
                events: vec![MacroEvent::new(EventType::MouseWheel, 0, 0, 255)],
                recording_options: None,
                cloud_last_updated_at: String::new(),
                is_deleted: 0,
                is_new: 0,
            },
        ],
        bindings: vec![Binding {
            binding_type: binding_types::CUSTOM_ACTION.to_string(),
            id: Some("b-1".to_string()),
            config_id: None,
            macro_id: Some("m-1".to_string()),
            trigger_data: TriggerData::key(62),
            playback_options_data: PlaybackOptions {
                playback_type: PlaybackType::RepeatNTimes,
                num_repeat: 3,
                repeat_delay: 40,
                direction: -1,
            },
            os_macro_type: String::new(),
            transparent_macro_data: None,
            is_passthrough: false,
        }],
    }
}

fn write_profile(dir: &Path, file_name: &str, file: &ProfileFile) {
    std::fs::create_dir_all(dir).unwrap();
    let text = serde_json::to_string_pretty(file).unwrap();
    std::fs::write(dir.join(file_name), text).unwrap();
}

#[test]
fn round_trip_through_load_and_save() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());

    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    assert!(matches!(ws.status, Status::Idle));
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    ws.select_event(2);
    assert_eq!(ws.selected_event, Some(2));

    ws.save_open_profile().unwrap();
    assert!(!ws.dirty());

    let saved = std::fs::read_to_string(dir.join("sample.json")).unwrap();
    let reloaded: ProfileFile = serde_json::from_str(&saved).unwrap();
    reloaded.validate().unwrap();
    let a = serde_json::to_value(sample_file()).unwrap();
    let b = serde_json::to_value(&reloaded).unwrap();
    assert_eq!(a, b, "saved profile must be semantically identical");
}

#[test]
fn delete_selected_macro_clears_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    ws.select_event(1);

    ws.delete_macro("m-1");
    assert_eq!(ws.selected_macro, None);
    assert_eq!(ws.selected_event, None);
    assert!(ws.dirty());

    ws.select_macro("m-2");
    ws.select_macro("missing");
    assert_eq!(ws.selected_macro, Some("m-2".to_string()));
}

#[test]
fn duplicate_selects_the_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    ws.select_macro("m-1");

    ws.duplicate_macro("m-1", new_macro_id());
    let doc = ws.open().unwrap();
    assert_eq!(doc.file.macros.len(), 3);
    let copy = doc.file.macros.last().unwrap();
    assert_eq!(copy.name, "ctrl-c copy");
    assert_eq!(ws.selected_macro.as_deref(), Some(copy.id.as_str()));
    let binding = doc
        .file
        .bindings
        .iter()
        .find(|b| b.macro_id.as_deref() == Some(copy.id.as_str()))
        .expect("duplicate keeps a binding copy");
    assert_eq!(binding.trigger_data, TriggerData::key(62));
    assert_eq!(binding.playback_options_data.num_repeat, 3);
}

#[test]
fn new_macro_gets_binding_and_default_playback() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");

    let id = new_macro_id();
    ws.new_macro(id.clone(), macro_studio::schema::default_binding(&id));
    assert!(ws.dirty());
    assert_eq!(ws.selected_macro.as_deref(), Some(id.as_str()));
    let doc = ws.open().unwrap();
    let m = doc.file.macro_by_id(&id).unwrap();
    assert!(m.events.is_empty());
    assert_eq!(m.name, "new-macro");
    let b = doc
        .file
        .bindings
        .iter()
        .find(|b| b.macro_id.as_deref() == Some(id.as_str()))
        .unwrap();
    assert_eq!(b.binding_type, binding_types::CUSTOM_ACTION);
    assert_eq!(b.playback_options_data, PlaybackOptions::default());
    doc.file.validate().unwrap();
}

#[test]
fn playback_field_enablement_mirrors_uses_predicates() {
    for entry in &PLAYBACK_TABLE {
        let pt = PlaybackType::from_client(entry.code).unwrap();
        let wants_repeat = pt == PlaybackType::RepeatNTimes;
        let wants_delay = matches!(
            pt,
            PlaybackType::RepeatNTimes
                | PlaybackType::RepeatWhilePressed
                | PlaybackType::ToggleRepeat
        );
        assert_eq!(pt.uses_num_repeat(), wants_repeat, "{}", entry.label);
        assert_eq!(pt.uses_repeat_delay(), wants_delay, "{}", entry.label);
    }
}

#[test]
fn event_labels_use_keymap_names() {
    let keymap = macro_daemon::keymap::Keymap::load_embedded().unwrap();
    let ctrl_down = MacroEvent::new(EventType::KeyboardKey, 1, 224, 1);
    assert_eq!(event_label(&ctrl_down, &keymap), "v Left Ctrl");
    let ctrl_up = MacroEvent::new(EventType::KeyboardKey, 1, 224, 0);
    assert_eq!(event_label(&ctrl_up, &keymap), "^ Left Ctrl");
    let f5_down = MacroEvent::new(EventType::KeyboardKey, 1, 62, 1);
    assert_eq!(event_label(&f5_down, &keymap), "v F5");
    let delay = MacroEvent::delay(120);
    assert_eq!(event_label(&delay, &keymap), "120ms");
    let unknown = MacroEvent::new(EventType::KeyboardKey, 1, 0xFE, 1);
    assert_eq!(event_label(&unknown, &keymap), "v usage(1,254)");
    assert_eq!(macro_total_ms(&[delay, ctrl_down]), 120);
}

#[test]
fn human_trigger_names_and_direction() {
    let keymap = macro_daemon::keymap::Keymap::load_embedded().unwrap();
    let press = TriggerData::key(62);
    assert_eq!(human_trigger(&press, &keymap), "F5 (press)");
    let mut release = press;
    release.direction = -1;
    assert_eq!(human_trigger(&release, &keymap), "F5 (release)");
    let unknown = TriggerData {
        hid_code: 234,
        page: 1,
        trigger_type: 2,
        direction: 1,
    };
    assert_eq!(human_trigger(&unknown, &keymap), "usage(1,234) (press)");
    assert_eq!(direction_label(1), "forward");
    assert_eq!(direction_label(-1), "reverse");
    assert_eq!(key_name(1, 224, &keymap), "Left Ctrl");
}

#[test]
fn app_rules_add_remove_and_save() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");

    ws.add_app_rule("/usr/bin/bar".to_string());
    assert!(ws.dirty());
    ws.add_app_rule("".to_string());
    let doc = ws.open().unwrap();
    assert_eq!(doc.file.profile.apps.len(), 2);
    ws.remove_app_rule(0);
    let doc = ws.open().unwrap();
    assert_eq!(doc.file.profile.apps, vec!["/usr/bin/bar".to_string()]);
    ws.save_open_profile().unwrap();
    let text = std::fs::read_to_string(dir.join("sample.json")).unwrap();
    let reloaded: ProfileFile = serde_json::from_str(&text).unwrap();
    assert_eq!(reloaded.profile.apps, vec!["/usr/bin/bar".to_string()]);
}

#[test]
fn save_refuses_invalid_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    let doc = ws.open_mut().unwrap();
    doc.file.bindings[0].macro_id = Some("m-gone".to_string());
    assert!(ws.save_open_profile().is_err());
    assert!(
        dir.join("sample.json").exists(),
        "file stays untouched on disk"
    );
}

#[test]
fn reload_discards_edits_and_keeps_open_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    ws.delete_macro("m-1");
    assert!(ws.dirty());

    ws.reload();
    assert!(!ws.dirty());
    assert_eq!(ws.open_profile.as_deref(), Some("sample.json"));
    let doc = ws.open().unwrap();
    assert_eq!(doc.file.macros.len(), 2, "edits discarded, disk state back");
    assert_eq!(ws.selected_macro, None);
}

#[test]
fn load_reports_error_per_file_without_failing_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "good.json", &sample_file());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.json"), "{not json").unwrap();

    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    assert_eq!(ws.profiles.len(), 1);
    assert!(matches!(ws.status, Status::Error(_)));
}

#[test]
fn saving_one_profile_keeps_other_profiles_dirty() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "a.json", &sample_file());
    let mut other = sample_file();
    other.profile.name = "other".to_string();
    write_profile(&dir, "b.json", &other);

    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("a.json");
    ws.select_macro("m-1");
    ws.delete_macro("m-1");
    assert!(ws.dirty(), "workspace is dirty while a is edited");

    ws.select_profile("b.json");
    assert!(ws.dirty(), "switching profiles must not clear a's edits");
    assert!(ws.save_open_profile().is_ok(), "b saves");
    assert!(
        ws.dirty(),
        "a is still unsaved after saving b; the flag must not be global"
    );

    ws.select_profile("a.json");
    assert!(ws.dirty(), "returning to a shows it as unsaved");
    assert!(ws.save_open_profile().is_ok());
    assert!(!ws.dirty(), "saving the last dirty profile clears the flag");
}

#[test]
fn key_choices_match_keymap_names() {
    let keymap = macro_daemon::keymap::Keymap::load_embedded().unwrap();
    for (code, label) in macro_studio::keymenu::KEY_CHOICES {
        if code == 0 {
            continue;
        }
        let key = keymap
            .lookup(macro_studio::keymenu::KEY_PAGE, code)
            .unwrap_or_else(|| panic!("usage {code} missing from keymap"));
        let name = key.name.trim_start_matches("KEY_").to_string();
        let normalized = name
            .replace("LEFTCTRL", "L CTRL")
            .replace("LEFTALT", "L ALT")
            .replace("LEFTSHIFT", "L SHIFT");
        assert!(
            label.eq_ignore_ascii_case(&normalized),
            "label {label} does not match keymap name {name} for usage {code}"
        );
    }
}

#[test]
fn no_op_mutations_do_not_mark_dirty() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    assert!(!ws.dirty());

    ws.set_playback("missing-macro", |p| p.num_repeat = 9);
    ws.set_trigger("missing-macro", |t| t.hid_code = 99);
    ws.update_event("m-1", 99, |e| e.extra_data = 1);
    ws.update_event("missing-macro", 0, |e| e.extra_data = 1);
    ws.delete_macro("missing-macro");
    assert!(
        !ws.dirty(),
        "mutations that changed nothing must not flag unsaved"
    );

    ws.delete_macro("m-1");
    assert!(ws.dirty(), "deleting a real macro flags unsaved");
}

#[test]
fn duplicate_macro_ids_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");

    ws.new_macro(
        "m-1".to_string(),
        macro_studio::schema::default_binding("m-1"),
    );
    assert_eq!(
        ws.open()
            .unwrap()
            .file
            .macros
            .iter()
            .filter(|m| m.id == "m-1")
            .count(),
        1,
        "second macro with the same id must be rejected"
    );
    assert_eq!(ws.selected_macro.as_deref(), None);

    ws.select_macro("m-1");
    ws.duplicate_macro("m-1", "m-2".to_string());
    ws.duplicate_macro("m-1", "m-2".to_string());
    assert_eq!(
        ws.open()
            .unwrap()
            .file
            .macros
            .iter()
            .filter(|m| m.id == "m-2")
            .count(),
        1,
        "duplicate_macro must reject a colliding new id"
    );
    assert!(ws.open().unwrap().file.validate().is_ok());
}

#[test]
fn headless_validate_accepts_good_and_rejects_corrupt_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "good.json", &sample_file());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("corrupt.json"), "{not json").unwrap();

    assert!(macro_studio::main_module::validate_one(&dir.join("good.json")).is_ok());
    assert!(macro_studio::main_module::validate_one(&dir.join("corrupt.json")).is_err());
    assert_eq!(macro_studio::main_module::run_validate(&dir), 1);

    std::fs::remove_file(dir.join("corrupt.json")).unwrap();
    assert_eq!(macro_studio::main_module::run_validate(&dir), 0);
}

#[test]
fn entries_sort_names_and_skip_deleted_macros() {
    let mut file = sample_file();
    file.macros[0].name = "beta".to_string();
    file.macros.push(Macro {
        id: "m-gone".to_string(),
        name: "ALPHA".to_string(),
        events: Vec::new(),
        recording_options: None,
        cloud_last_updated_at: String::new(),
        is_deleted: 1,
        is_new: 0,
    });
    file.macros.push(Macro {
        id: "m-alpha".to_string(),
        name: "ALPHA".to_string(),
        events: Vec::new(),
        recording_options: None,
        cloud_last_updated_at: String::new(),
        is_deleted: 0,
        is_new: 0,
    });
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "z.json", &file);
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    let names: Vec<_> = ws.entries().into_iter().map(|e| e.name).collect::<Vec<_>>();
    assert_eq!(names, ["ALPHA", "beta", "wheel"]);
    assert!(ws.entries().iter().all(|e| e.id != "m-gone"));
}

#[test]
fn clear_selected_events_persists_on_save() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.clear_selected_events();
    assert!(matches!(ws.status, Status::Error(_)));
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    ws.clear_selected_events();
    assert!(ws.selected().unwrap().events.is_empty());
    assert!(ws.dirty());
    assert!(ws.selected_event.is_none());
    assert!(matches!(ws.status, Status::Info(_)));
    ws.save_open_profile().unwrap();
    ws.reload();
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    assert!(ws.selected().unwrap().events.is_empty());
    ws.clear_selected_events();
    assert!(matches!(ws.status, Status::Info(_)));
}

#[test]
fn validate_rejects_an_empty_or_missing_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(macro_studio::main_module::run_validate(&empty), 1);
    assert_eq!(
        macro_studio::main_module::run_validate(&tmp.path().join("missing")),
        1
    );
}

#[test]
fn start_plays_the_selected_macro_through_mock_injector() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("profiles");
    write_profile(&dir, "sample.json", &sample_file());
    let mut ws = Workspace::new();
    ws.load_dir(&dir);
    ws.select_profile("sample.json");
    ws.select_macro("m-1");
    ws.play_selected();
    let Status::Info(text) = &ws.status else {
        panic!("expected an engine-level play report, got {:?}", ws.status);
    };
    assert!(text.contains("MockInjector"), "{text}");
    assert!(text.contains("injected"), "{text}");
    assert!(!text.contains("/dev/uinput"), "{text}");
}
