use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const SETTINGS_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub volume: f32,
    pub muted: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { volume: 0.75, muted: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub version: u32,
    pub apps: HashMap<String, AppSettings>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { version: SETTINGS_VERSION, apps: HashMap::new() }
    }
}

pub struct SettingsState {
    pub inner: Mutex<Settings>,
    pub path: PathBuf,
}

pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Settings>(&s) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("settings.json parse error, using defaults: {}", e);
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {}", e))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("rename failed: {}", e))?;
    Ok(())
}
