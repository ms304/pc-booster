use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub blacklisted_services: HashSet<String>,
}

impl Config {
    pub fn load() -> Self {
        let path = get_config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<Config>(&content) {
                    return config;
                }
            }
        }
        Config {
            blacklisted_services: HashSet::new(),
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = get_config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }
}

pub fn get_config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("pc-booster");
    path.push("config.json");
    path
}
