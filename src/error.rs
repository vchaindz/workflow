use std::fmt;

pub type Result<T> = std::result::Result<T, DzError>;

#[derive(Debug)]
pub enum DzError {
    Config(String),
    Discovery(String),
    Parse(String),
    CycleDetected(Vec<String>),
    Execution(String),
    TaskNotFound(String),
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
    Toml(toml::de::Error),
    Compare(String),
    Db(rusqlite::Error),
    Other(anyhow::Error),
}

impl fmt::Display for DzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DzError::Config(msg) => write!(f, "config error: {msg}"),
            DzError::Discovery(msg) => write!(f, "discovery error: {msg}"),
            DzError::Parse(msg) => write!(f, "parse error: {msg}"),
            DzError::CycleDetected(ids) => write!(f, "cycle detected in steps: {}", ids.join(" -> ")),
            DzError::Execution(msg) => write!(f, "execution error: {msg}"),
            DzError::TaskNotFound(name) => write!(f, "task not found: {name}"),
            DzError::Io(e) => write!(f, "I/O error: {e}"),
            DzError::Yaml(e) => write!(f, "YAML error: {e}"),
            DzError::Json(e) => write!(f, "JSON error: {e}"),
            DzError::Toml(e) => write!(f, "TOML error: {e}"),
            DzError::Compare(msg) => write!(f, "compare error: {msg}"),
            DzError::Db(e) => write!(f, "database error: {e}"),
            DzError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DzError {}

impl From<std::io::Error> for DzError {
    fn from(e: std::io::Error) -> Self {
        DzError::Io(e)
    }
}

impl From<serde_yaml::Error> for DzError {
    fn from(e: serde_yaml::Error) -> Self {
        DzError::Yaml(e)
    }
}

impl From<serde_json::Error> for DzError {
    fn from(e: serde_json::Error) -> Self {
        DzError::Json(e)
    }
}

impl From<toml::de::Error> for DzError {
    fn from(e: toml::de::Error) -> Self {
        DzError::Toml(e)
    }
}

impl From<rusqlite::Error> for DzError {
    fn from(e: rusqlite::Error) -> Self {
        DzError::Db(e)
    }
}

impl From<anyhow::Error> for DzError {
    fn from(e: anyhow::Error) -> Self {
        DzError::Other(e)
    }
}
