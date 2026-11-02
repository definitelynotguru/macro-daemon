//! Schema fidelity: the on-disk shapes must round-trip against MACRO_SPEC.md exactly.
use macro_daemon::config::{
    EventType, Macro, MacroEvent, PlaybackOptions, PlaybackType, ProfileFile, TransparentMacroData,
    TriggerData,
};

const SPEC_EXAMPLE: &str = r#"[
  {"type":2,"page":1,"code":224,"extraData":1,"timestamp":0},
  {"type":2,"page":1,"code":225,"extraData":1,"timestamp":0},
  {"type":2,"page":1,"code":25,"extraData":1,"timestamp":0},
  {"type":2,"page":1,"code":25,"extraData":0,"timestamp":0},
  {"type":2,"page":1,"code":225,"extraData":0,"timestamp":0},
  {"type":2,"page":1,"code":224,"extraData":0,"timestamp":0},
  {"type":4,"page":0,"code":0,"extraData":250,"timestamp":0},
  {"type":2,"page":1,"code":62,"extraData":1,"timestamp":0},
  {"type":2,"page":1,"code":62,"extraData":0,"timestamp":0}
]"#;

#[test]
fn spec_example_parses_into_event_struct() {
    let events: Vec<MacroEvent> = serde_json::from_str(SPEC_EXAMPLE).expect("spec example parses");
    assert_eq!(events.len(), 9);
    assert_eq!(events[0].event_type, EventType::KeyboardKey);
    assert_eq!(events[0].page, 1);
    assert_eq!(events[0].code, 224); // LeftCtrl
    assert_eq!(events[6].event_type, EventType::Delay);
    assert_eq!(events[6].extra_data, 250);
}

#[test]
fn field_names_are_preserved_verbatim() {
    let events: Vec<MacroEvent> = serde_json::from_str(SPEC_EXAMPLE).unwrap();
    let json = serde_json::to_value(events[0]).unwrap();
    let obj = json.as_object().unwrap();
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    // MACRO_SPEC.md §A.2: type, page, code, extraData, timestamp — nothing renamed.
    assert_eq!(keys, vec!["code", "extraData", "page", "timestamp", "type"]);
}

#[test]
fn event_array_roundtrips_byte_identically() {
    let events: Vec<MacroEvent> = serde_json::from_str(SPEC_EXAMPLE).unwrap();
    let again = serde_json::to_string(&events).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&again).unwrap();
    let original: Vec<serde_json::Value> = serde_json::from_str(SPEC_EXAMPLE).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn db_wrapper_matches_engine_framing() {
    // §A.2: device.unmarshalMacroEvents wraps the column as {"events": <array>}.
    let mac = Macro::from_db_events_string("id-1", "n", SPEC_EXAMPLE).unwrap();
    let wrapped = mac.db_events_string();
    assert!(wrapped.starts_with("{\"events\": ["));
    assert!(wrapped.ends_with("]}"));
    let back = Macro::from_db_events_string("id-1", "n", &wrapped).unwrap();
    assert_eq!(back.events, mac.events);
}

#[test]
fn header_direction_and_repeats_default_correctly() {
    // §A.4 DB default: {"type":0,"num_repeat":1,"repeat_delay":15}
    let opts: PlaybackOptions =
        serde_json::from_str(r#"{"type":0,"num_repeat":1,"repeat_delay":15}"#).unwrap();
    assert_eq!(opts.playback_type, PlaybackType::PlayOnce);
    assert_eq!(opts.num_repeat, 1);
    assert_eq!(opts.repeat_delay, 15);
    assert_eq!(opts.direction, 1);

    let bare: PlaybackOptions = serde_json::from_str(r#"{"type":2}"#).unwrap();
    assert_eq!(bare.playback_type, PlaybackType::RepeatWhilePressed);
    assert_eq!(bare.num_repeat, 1, "num_repeat defaults to 1");
    assert_eq!(bare.repeat_delay, 15, "repeat_delay defaults to 15 (§A.4)");
}

#[test]
fn dual_numbering_is_exhaustive_and_unambiguous() {
    // client 0..=4 / wire 1..=6 (§A.4). The mapping must be a total bijection on the
    // shared range, and unknown inputs must be rejected rather than coerced.
    let cases = [
        (0u8, 1u8, PlaybackType::PlayOnce),
        (1, 2, PlaybackType::RepeatNTimes),
        (2, 3, PlaybackType::RepeatWhilePressed),
        (3, 4, PlaybackType::ToggleRepeat),
        (4, 5, PlaybackType::ToggleHold),
    ];
    for (client, wire, ty) in cases {
        assert_eq!(PlaybackType::from_client(client).unwrap(), ty);
        assert_eq!(PlaybackType::from_wire(wire).unwrap(), ty);
        assert_eq!(ty.to_client(), client);
        assert_eq!(ty.to_wire(), wire);
    }
    assert_eq!(
        PlaybackType::from_wire(6).unwrap(),
        PlaybackType::HoldWhilePressed
    );

    assert!(
        PlaybackType::from_client(5).is_err(),
        "client 5 is not a valid discriminant"
    );
    assert!(PlaybackType::from_client(255).is_err());
    assert!(
        PlaybackType::from_wire(0).is_err(),
        "wire 0 is not a valid discriminant"
    );
    assert!(PlaybackType::from_wire(7).is_err());
}

#[test]
fn repeat_delay_is_only_honoured_where_the_spec_says() {
    // §B.4 table: repeat_delay is enabled for repeatNTimes / repeatWhilePressed / toggleRepeat.
    assert!(!PlaybackType::PlayOnce.uses_repeat_delay());
    assert!(PlaybackType::RepeatNTimes.uses_repeat_delay());
    assert!(PlaybackType::RepeatWhilePressed.uses_repeat_delay());
    assert!(PlaybackType::ToggleRepeat.uses_repeat_delay());
    assert!(!PlaybackType::ToggleHold.uses_repeat_delay());
    // num_repeat is only meaningful for repeatNTimes.
    assert!(PlaybackType::RepeatNTimes.uses_num_repeat());
    assert!(!PlaybackType::PlayOnce.uses_num_repeat());
}

#[test]
fn trigger_and_transparent_data_shapes() {
    let t: TriggerData =
        serde_json::from_str(r#"{"hidCode":71,"page":1,"type":2,"direction":1}"#).unwrap();
    assert_eq!(t.hid_code, 71);
    assert_eq!(t.page, 1);
    assert_eq!(t.trigger_type, 2);

    let d: TransparentMacroData =
        serde_json::from_str(r#"{"hidCode":4,"page":1,"type":2}"#).unwrap();
    assert_eq!(d.hid_code, 4);
}

#[test]
fn profile_files_load_and_validate() {
    let set = macro_daemon::ProfileSet::load_dir(std::path::Path::new("profiles")).unwrap();
    assert!(set.profiles.contains_key("default"));
    assert!(set.profiles.contains_key("editor"));

    let default = &set.profiles["default"];
    assert_eq!(default.macros.len(), 3);
    assert_eq!(default.bindings.len(), 3);
    default.validate().unwrap();
}

#[test]
fn full_profile_roundtrip_is_lossless() {
    let path = std::path::Path::new("profiles/default.json");
    let text = std::fs::read_to_string(path).unwrap();
    let prof: ProfileFile = serde_json::from_str(&text).unwrap();

    let reserialized = serde_json::to_string(&prof).unwrap();
    let reparsed: ProfileFile = serde_json::from_str(&reserialized).unwrap();

    assert_eq!(reparsed.macros.len(), prof.macros.len());
    for (a, b) in prof.macros.iter().zip(reparsed.macros.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.events, b.events);
    }
    assert_eq!(prof.bindings, reparsed.bindings);
}

#[test]
fn unknown_event_type_is_rejected_not_coerced() {
    let bad = r#"[{"type":3,"page":1,"code":4,"extraData":1,"timestamp":0}]"#;
    let err = serde_json::from_str::<Vec<MacroEvent>>(bad).unwrap_err();
    assert!(
        err.to_string().contains("unknown macro event type 3"),
        "unexpected error: {err}"
    );
}

#[test]
fn binding_accepts_int_passthrough_and_empty_transparent_column() {
    // §D.1: is_passthrough is INTEGER in the table, transparent_macro_data is TEXT DEFAULT ''.
    let b: macro_daemon::Binding = serde_json::from_str(
        r#"{
            "type":"customAction",
            "macro_id":"m",
            "trigger_data":{"hidCode":4,"page":1,"type":2,"direction":1},
            "is_passthrough":1,
            "transparent_macro_data":""
        }"#,
    )
    .unwrap();
    assert!(b.is_passthrough);
    assert!(b.transparent_macro_data.is_none());
}
