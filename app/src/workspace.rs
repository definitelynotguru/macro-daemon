use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use macro_daemon::config::ProfileFile;

pub struct ProfileDoc {
    pub file_name: String,
    pub file: ProfileFile,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub enum Status {
    Idle,
    Info(String),
    Error(String),
}

pub struct Workspace {
    pub keymap: macro_daemon::keymap::Keymap,
    pub dir: PathBuf,
    pub profiles: Vec<ProfileDoc>,
    pub open_profile: Option<String>,
    pub selected_macro: Option<String>,
    pub selected_event: Option<usize>,
    pub status: Status,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            keymap: macro_daemon::keymap::Keymap::load_embedded().unwrap_or_default(),
            dir: PathBuf::from("profiles"),
            profiles: Vec::new(),
            open_profile: None,
            selected_macro: None,
            selected_event: None,
            status: Status::Idle,
        }
    }

    pub fn load_dir(&mut self, dir: &Path) {
        self.dir = dir.to_path_buf();
        let mut docs = Vec::new();
        let mut errors = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            self.profiles = Vec::new();
            self.open_profile = None;
            self.clear_selection();
            self.status = Status::Error(format!("cannot read {}", dir.display()));
            return;
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    errors.push(err.to_string());
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            match Self::read_profile(&path) {
                Ok(mut file) => {
                    if file.profile.name.is_empty() {
                        file.profile.name = file_name.trim_end_matches(".json").to_string();
                    }
                    docs.push(ProfileDoc {
                        file_name,
                        file,
                        dirty: false,
                    });
                }
                Err(err) => errors.push(format!("{file_name}: {err:#}")),
            }
        }
        docs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        self.profiles = docs;
        if let Some(open) = &self.open_profile {
            if !self.profiles.iter().any(|p| &p.file_name == open) {
                self.open_profile = None;
            }
        }
        if self.open_profile.is_none() {
            self.open_profile = self.profiles.first().map(|p| p.file_name.clone());
        }
        self.clear_selection();
        if errors.is_empty() {
            self.status = Status::Idle;
        } else {
            self.status = Status::Error(errors.join("; "));
        }
    }

    fn read_profile(path: &Path) -> Result<ProfileFile> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file: ProfileFile =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        file.validate()
            .with_context(|| format!("validating {}", path.display()))?;
        Ok(file)
    }

    fn clear_selection(&mut self) {
        self.selected_macro = None;
        self.selected_event = None;
    }

    pub fn dirty(&self) -> bool {
        self.profiles.iter().any(|p| p.dirty)
    }

    pub fn open(&self) -> Option<&ProfileDoc> {
        let open = self.open_profile.as_ref()?;
        self.profiles.iter().find(|p| &p.file_name == open)
    }

    pub fn open_mut(&mut self) -> Option<&mut ProfileDoc> {
        let open = self.open_profile.clone()?;
        self.profiles.iter_mut().find(|p| p.file_name == open)
    }

    pub fn selected(&self) -> Option<&macro_daemon::config::Macro> {
        let id = self.selected_macro.as_ref()?;
        let doc = self.open()?;
        doc.file.macro_by_id(id)
    }

    pub fn selected_binding(&self) -> Option<&macro_daemon::config::Binding> {
        let doc = self.open()?;
        let id = self.selected_macro.as_ref()?;
        doc.file.bindings.iter().find(|b| {
            b.macro_id.as_deref() == Some(id.as_str())
                && b.binding_type == macro_daemon::config::binding_types::CUSTOM_ACTION
        })
    }

    pub fn select_profile(&mut self, file_name: &str) {
        if !self.profiles.iter().any(|p| p.file_name == file_name) {
            return;
        }
        if self.open_profile.as_deref() == Some(file_name) {
            return;
        }
        self.open_profile = Some(file_name.to_string());
        self.clear_selection();
        self.status = Status::Idle;
    }

    pub fn select_macro(&mut self, id: &str) {
        let exists = self
            .open()
            .is_some_and(|doc| doc.file.macro_by_id(id).is_some());
        if !exists {
            return;
        }
        if self.selected_macro.as_deref() == Some(id) {
            return;
        }
        self.selected_macro = Some(id.to_string());
        self.selected_event = None;
        self.status = Status::Idle;
    }

    pub fn select_event(&mut self, index: usize) {
        let Some(count) = self.selected().map(|m| m.events.len()) else {
            return;
        };
        if index >= count {
            return;
        }
        self.selected_event = Some(index);
    }

    pub fn deselect_event(&mut self) {
        self.selected_event = None;
    }

    pub fn new_macro(&mut self, id: String, binding: macro_daemon::config::Binding) {
        let Some(doc) = self.open_mut() else {
            return;
        };
        doc.dirty = true;
        doc.file.macros.push(macro_daemon::config::Macro {
            id: id.clone(),
            name: "new-macro".to_string(),
            events: Vec::new(),
            recording_options: None,
            cloud_last_updated_at: String::new(),
            is_deleted: 0,
            is_new: 0,
        });
        doc.file.bindings.push(binding);
        self.selected_macro = Some(id);
        self.selected_event = None;
        self.status = Status::Idle;
    }

    pub fn duplicate_macro(&mut self, id: &str, new_id: String) {
        let Some(doc) = self.open_mut() else {
            return;
        };
        let Some(source) = doc
            .file
            .macros
            .iter()
            .find(|m| m.id == id && m.is_deleted == 0)
        else {
            return;
        };
        let mut copy = source.clone();
        copy.id = new_id.clone();
        copy.name = format!("{} copy", source.name);
        let binding = doc
            .file
            .bindings
            .iter()
            .find(|b| b.macro_id.as_deref() == Some(id))
            .cloned();
        doc.file.macros.push(copy);
        if let Some(mut binding) = binding {
            binding.id = Some(format!("b-{new_id}"));
            binding.macro_id = Some(new_id.clone());
            doc.file.bindings.push(binding);
        }
        doc.dirty = true;
        self.selected_macro = Some(new_id);
        self.selected_event = None;
        self.status = Status::Idle;
    }

    pub fn delete_macro(&mut self, id: &str) {
        if let Some(doc) = self.open_mut() {
            doc.file.macros.retain(|m| m.id != id);
            doc.file
                .bindings
                .retain(|b| b.macro_id.as_deref() != Some(id));
            doc.dirty = true;
        }
        if self.selected_macro.as_deref() == Some(id) {
            self.clear_selection();
        }
        self.status = Status::Idle;
    }

    pub fn set_trigger(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut macro_daemon::config::TriggerData),
    ) {
        self.mutate_binding(id, |b| apply(&mut b.trigger_data));
    }

    pub fn set_playback(
        &mut self,
        id: &str,
        apply: impl FnOnce(&mut macro_daemon::config::PlaybackOptions),
    ) {
        self.mutate_binding(id, |b| apply(&mut b.playback_options_data));
    }

    fn mutate_binding(&mut self, id: &str, apply: impl FnOnce(&mut macro_daemon::config::Binding)) {
        let Some(doc) = self.open_mut() else {
            return;
        };
        let Some(binding) = doc
            .file
            .bindings
            .iter_mut()
            .find(|b| b.macro_id.as_deref() == Some(id))
        else {
            return;
        };
        apply(binding);
        doc.dirty = true;
    }

    pub fn set_num_repeat(&mut self, id: &str, value: u8) {
        self.set_playback(id, |p| p.num_repeat = value);
    }

    pub fn set_repeat_delay(&mut self, id: &str, value: u16) {
        self.set_playback(id, |p| p.repeat_delay = value);
    }

    pub fn set_direction(&mut self, id: &str, direction: i8) {
        self.set_playback(id, |p| p.direction = direction);
    }

    pub fn add_app_rule(&mut self, path: String) {
        if path.trim().is_empty() {
            return;
        }
        let Some(doc) = self.open_mut() else {
            return;
        };
        if doc.file.profile.apps.iter().any(|a| a == &path) {
            return;
        }
        doc.file.profile.apps.push(path);
        doc.dirty = true;
    }

    pub fn remove_app_rule(&mut self, index: usize) {
        let Some(doc) = self.open_mut() else {
            return;
        };
        if index >= doc.file.profile.apps.len() {
            return;
        }
        doc.file.profile.apps.remove(index);
        doc.dirty = true;
    }

    pub fn update_event(
        &mut self,
        macro_id: &str,
        index: usize,
        apply: impl FnOnce(&mut macro_daemon::config::MacroEvent),
    ) {
        let Some(doc) = self.open_mut() else {
            return;
        };
        let Some(m) = doc
            .file
            .macros
            .iter_mut()
            .find(|m| m.id == macro_id && m.is_deleted == 0)
        else {
            return;
        };
        if index >= m.events.len() {
            return;
        }
        apply(&mut m.events[index]);
        doc.dirty = true;
    }

    pub fn save_open_profile(&mut self) -> Result<()> {
        let Some(file_name) = self.open_profile.clone() else {
            anyhow::bail!("no profile open");
        };
        let text = {
            let Some(doc) = self.open_mut() else {
                anyhow::bail!("no profile open");
            };
            doc.file.validate()?;
            serde_json::to_string_pretty(&doc.file)?
        };
        let path = self.dir.join(&file_name);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        if let Some(doc) = self.open_mut() {
            doc.dirty = false;
        }
        self.status = Status::Info(format!("Saved {file_name}"));
        Ok(())
    }

    pub fn reload(&mut self) {
        self.load_dir(&self.dir.clone());
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
