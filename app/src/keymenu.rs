use macro_daemon::keymap::PAGE_KEYBOARD;

/// Key choices for the trigger picker, as `(hid_usage, label)` pairs. Usage
/// ids match `assets/keymap.json` on page 1 (keyboard), so picking a pill
/// binds the key its label names.
pub const KEY_CHOICES: [(u16, &str); 8] = [
    (0, "pick key"),
    (62, "F5"),
    (74, "Home"),
    (30, "1"),
    (32, "3"),
    (224, "L Ctrl"),
    (226, "L Alt"),
    (225, "L Shift"),
];

pub const KEY_PAGE: u8 = PAGE_KEYBOARD;
