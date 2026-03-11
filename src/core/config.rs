use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::models::NotifyConfig;
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
    #[serde(default)]
    pub default_timeout: Option<u64>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub bookmarks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    pub pre_run: Option<String>,
    pub post_run: Option<String>,
}

fn default_workflows_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("workflow")
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
            default_timeout: None,
            secrets: Vec::new(),
            notify: NotifyConfig::default(),
            bookmarks: Vec::new(),
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

    pub fn db_path(&self) -> PathBuf {
        self.workflows_dir.join("history.db")
    }

    /// Toggle a bookmark. Returns true if added, false if removed.
    pub fn toggle_bookmark(&mut self, task_ref: &str) -> bool {
        if let Some(pos) = self.bookmarks.iter().position(|b| b == task_ref) {
            self.bookmarks.remove(pos);
            false
        } else {
            self.bookmarks.push(task_ref.to_string());
            true
        }
    }

    /// Persist bookmarks to config.toml, preserving other fields.
    pub fn save_bookmarks(&self) -> Result<()> {
        let config_path = self.workflows_dir.join("config.toml");

        // Re-serialize the full config to preserve all fields
        let contents = toml::to_string_pretty(self)
            .map_err(|e| DzError::Config(e.to_string()))?;
        std::fs::write(&config_path, contents)?;
        Ok(())
    }
}
