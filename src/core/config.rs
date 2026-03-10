use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{DzError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_workflows_dir")]
    pub workflows_dir: PathBuf,
    #[serde(default = "default_log_retention")]
    pub log_retention_days: u32,
    #[serde(default = "default_editor")]
    pub editor: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    pub pre_run: Option<String>,
    pub post_run: Option<String>,
}

fn default_workflows_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("dzworkflows")
}

fn default_log_retention() -> u32 {
    30
}

fn default_editor() -> String {
    std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workflows_dir: default_workflows_dir(),
            log_retention_days: default_log_retention(),
            editor: default_editor(),
            theme: String::new(),
            hooks: HooksConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = default_workflows_dir().join("config.toml");
        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&contents).map_err(DzError::from)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            let config: Config = toml::from_str(&contents).map_err(DzError::from)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.workflows_dir.join("logs")
    }
}
