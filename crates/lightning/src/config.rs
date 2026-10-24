use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: String,
    pub theme: ThemeMode,
    /// Glass slider, 0 (opaque) ..= 100 (most see-through); 50 = tuned default.
    pub glass: u8,
    pub search_apps: bool,
    pub search_files: bool,
    pub search_settings: bool,
    pub search_power: bool,
    pub launch_at_startup: bool,
    /// Keep the overlay up when another window takes focus.
    pub always_on_top: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "Alt+Space".into(),
            theme: ThemeMode::Auto,
            glass: 50,
            search_apps: true,
            search_files: true,
            search_settings: true,
            search_power: true,
            launch_at_startup: false,
            always_on_top: false,
        }
    }
}

/// `%APPDATA%\Lightning` for config, `%LOCALAPPDATA%\Lightning` for indexes.
pub fn config_dir() -> PathBuf {
    env_dir("APPDATA").join("Lightning")
}

pub fn data_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Lightning")
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(config_dir().join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(dir.join("config.json"), s);
        }
    }

    pub fn sources(&self) -> lightning_core::Sources {
        lightning_core::Sources {
            apps: self.search_apps,
            files: self.search_files,
            settings: self.search_settings,
            power: self.search_power,
        }
    }
}
