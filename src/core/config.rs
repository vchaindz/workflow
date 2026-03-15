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
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub server: ServerConfig,
    /// Path to SSH private key for secrets encryption/decryption
    #[serde(default, skip_serializing)]
    pub secrets_ssh_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    pub pre_run: Option<String>,
    pub post_run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default = "default_true")]
    pub auto_commit: bool,
    #[serde(default = "default_true")]
    pub auto_push: bool,
    #[serde(default = "default_true")]
    pub auto_pull_on_start: bool,
    #[serde(default)]
    pub sync_interval_minutes: Option<u32>,
    #[serde(default = "default_branch")]
    pub branch: String,
}

fn default_true() -> bool {
    true
}

fn default_branch() -> String {
    "main".to_string()
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            remote_url: None,
            auto_commit: true,
            auto_push: true,
            auto_pull_on_start: true,
            sync_interval_minutes: None,
            branch: "main".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_runs: usize,
}

fn default_port() -> u16 {
    8080
}
fn default_bind() -> String {
    "127.0.0.1".to_string()
}
fn default_max_concurrent() -> usize {
    4
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            bind: "127.0.0.1".to_string(),
            api_key: None,
            max_concurrent_runs: 4,
        }
    }
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
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
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
            sync: SyncConfig::default(),
            server: ServerConfig::default(),
            secrets_ssh_key: None,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = default_workflows_dir().join("config.toml");
        let mut config = if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            toml::from_str(&contents).map_err(DzError::from)?
        } else {
            Config::default()
        };

        // Overlay config.local.toml if it exists (machine-specific overrides)
        let local_path = config.workflows_dir.join("config.local.toml");
        if local_path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&local_path) {
                if let Ok(local) = toml::from_str::<Config>(&contents) {
                    // Overlay non-default fields
                    if local.editor != default_editor() {
                        config.editor = local.editor;
                    }
                    if local.workflows_dir != default_workflows_dir() {
                        config.workflows_dir = local.workflows_dir;
                    }
                    if !local.theme.is_empty() {
                        config.theme = local.theme;
                    }
                    if local.default_timeout.is_some() {
                        config.default_timeout = local.default_timeout;
                    }
                }
            }
        }

        Ok(config)
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

    /// Persist sync config to config.toml, preserving other fields.
    pub fn save_sync_config(&self) -> Result<()> {
        self.save_bookmarks()
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
