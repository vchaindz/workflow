use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::path::PathBuf;

/// Reference to an MCP server: either an alias (string) or an inline definition (object).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerRef {
    Alias(String),
    Inline {
        command: String,
        #[serde(default)]
        env: Option<HashMap<String, String>>,
        #[serde(default)]
        secrets: Option<Vec<String>>,
    },
}

/// Configuration for an MCP step: which server, which tool, and optional arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpStepConfig {
    pub server: McpServerRef,
    pub tool: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

use crate::core::notify::{RateLimitConfig, RetryConfig, Severity};

/// Deserialize a field that can be either a single string or an array of strings.
/// A single string is deserialized as a one-element Vec.
/// Missing/null is deserialized as an empty Vec.
fn string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Single(String),
        Multiple(Vec<String>),
    }

    Ok(match Option::<StringOrVec>::deserialize(deserializer)? {
        Some(StringOrVec::Single(s)) => vec![s],
        Some(StringOrVec::Multiple(v)) => v,
        None => Vec::new(),
    })
}

/// Serialize Vec<String> as a single string when len==1, array when len>1, skip when empty.
fn vec_as_string_or_vec<S>(v: &Vec<String>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match v.len() {
        1 => serializer.serialize_str(&v[0]),
        _ => v.serialize(serializer),
    }
}

/// An env value can be a plain string or a dynamic command to execute.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    Static(String),
    Dynamic { cmd: String },
}

/// Deserialization-only types for flexible YAML step formats.
/// Supports: bare string, map without id, full map with id/needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepOutput {
    pub name: String,
    pub pattern: String,
}

/// Source for for_each iteration: either a static list or a template reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ForEachSource {
    StaticList(Vec<String>),
    TemplateRef(String),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum RawStep {
    CmdString(String),
    CmdMap {
        id: Option<String>,
        #[serde(default)]
        cmd: String,
        #[serde(default)]
        needs: Vec<String>,
        #[serde(default)]
        parallel: bool,
        #[serde(default)]
        timeout: Option<u64>,
        #[serde(default)]
        run_if: Option<String>,
        #[serde(default)]
        skip_if: Option<String>,
        #[serde(default)]
        retry: Option<u32>,
        #[serde(default)]
        retry_delay: Option<u64>,
        #[serde(default)]
        interactive: Option<bool>,
        #[serde(default)]
        outputs: Vec<StepOutput>,
        #[serde(default)]
        call: Option<String>,
        #[serde(default)]
        for_each: Option<Box<ForEachSource>>,
        #[serde(default)]
        for_each_cmd: Option<String>,
        #[serde(default)]
        for_each_parallel: bool,
        #[serde(default)]
        for_each_continue_on_error: bool,
        #[serde(default)]
        mcp: Option<McpStepConfig>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVariable {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub choices_cmd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflow {
    pub name: String,
    pub steps: Vec<RawStep>,
    #[serde(default)]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub workdir: Option<PathBuf>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub overdue: Option<u32>,
    #[serde(default)]
    pub variables: Vec<RuntimeVariable>,
    #[serde(default)]
    pub cleanup: Vec<RawStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskKind {
    ShellScript,
    YamlWorkflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Default)]
pub enum TaskHeat {
    Hot,  // ≥5 runs in 30d
    Warm, // 1–4 runs in 30d
    #[default]
    Cold, // 0 runs in 30d
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub path: PathBuf,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    pub kind: TaskKind,
    pub path: PathBuf,
    pub category: String,
    #[serde(skip)]
    pub last_run: Option<RunSummary>,
    #[serde(default)]
    pub overdue: Option<u32>,
    #[serde(skip)]
    pub heat: TaskHeat,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing)]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub workdir: Option<PathBuf>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub overdue: Option<u32>,
    #[serde(default)]
    pub variables: Vec<RuntimeVariable>,
    #[serde(default)]
    pub cleanup: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub cmd: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub parallel: bool,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub run_if: Option<String>,
    #[serde(default)]
    pub skip_if: Option<String>,
    #[serde(default)]
    pub retry: Option<u32>,
    #[serde(default)]
    pub retry_delay: Option<u64>,
    #[serde(default)]
    pub interactive: Option<bool>,
    #[serde(default)]
    pub outputs: Vec<StepOutput>,
    #[serde(default)]
    pub call: Option<String>,
    #[serde(default)]
    pub for_each: Option<ForEachSource>,
    #[serde(default)]
    pub for_each_cmd: Option<String>,
    #[serde(default)]
    pub for_each_parallel: bool,
    #[serde(default)]
    pub for_each_continue_on_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpStepConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
    Timedout,
    Interactive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub id: String,
    pub status: StepStatus,
    pub output: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLog {
    pub id: String,
    pub task_ref: String,
    pub started: DateTime<Utc>,
    pub ended: Option<DateTime<Utc>>,
    pub steps: Vec<StepResult>,
    pub exit_code: i32,
}

/// A notification channel with severity-based routing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotifyChannel {
    /// Target URL (e.g. "slack://hooks.slack.com/xxx")
    pub target: String,
    /// Severity levels that trigger this channel (e.g. ["failure", "warning"])
    pub on: Vec<Severity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifyConfig {
    #[serde(default, deserialize_with = "string_or_vec", serialize_with = "vec_as_string_or_vec", skip_serializing_if = "Vec::is_empty")]
    pub on_failure: Vec<String>,
    #[serde(default, deserialize_with = "string_or_vec", serialize_with = "vec_as_string_or_vec", skip_serializing_if = "Vec::is_empty")]
    pub on_success: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<NotifyChannel>,
    /// When true, workflow-level notify config fully replaces global config
    /// instead of merging with it (default: false = merge).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notify_override: bool,
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    /// Per-service rate limit overrides. Key is service name (e.g. "slack", "discord").
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub rate_limit: HashMap<String, RateLimitConfig>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    StepStarted { step_id: String, cmd_preview: String },
    StepCompleted { step_id: String, status: StepStatus, duration_ms: u64 },
    StepSkipped { step_id: String },
    StepRetrying { step_id: String, attempt: u32, max: u32, delay_secs: u64 },
    DangerousCommand { step_id: String, warning: String },
    LevelStarted { level: usize, step_count: usize },
    SubWorkflowStarted { parent_step_id: String, sub_task_ref: String },
    SubWorkflowFinished { parent_step_id: String, sub_task_ref: String, exit_code: i32 },
    ForEachStarted { step_id: String, item_count: usize },
    ForEachIterationCompleted { step_id: String, item: String, index: usize, status: StepStatus, duration_ms: u64 },
    Warning { step_id: String, message: String },
    StepOutput { step_id: String, line: String },
    WorkflowFinished { run_log: RunLog },
    StepTimedOut { step_id: String, timeout_secs: u64, duration_ms: u64 },
    WorkflowError { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub fail_count: u32,
    pub last_duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_config_toml_single_string() {
        let toml_str = r#"
on_failure = "slack://hooks.slack.com/xxx"
on_success = "ntfy://ntfy.sh/topic"
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.on_failure, vec!["slack://hooks.slack.com/xxx"]);
        assert_eq!(config.on_success, vec!["ntfy://ntfy.sh/topic"]);
    }

    #[test]
    fn test_notify_config_toml_array() {
        let toml_str = r#"
on_failure = ["slack://hooks.slack.com/xxx", "ntfy://ntfy.sh/alerts"]
on_success = ["webhook://example.com/hook"]
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.on_failure, vec![
            "slack://hooks.slack.com/xxx",
            "ntfy://ntfy.sh/alerts",
        ]);
        assert_eq!(config.on_success, vec!["webhook://example.com/hook"]);
    }

    #[test]
    fn test_notify_config_toml_missing_fields() {
        let toml_str = "";
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert!(config.on_failure.is_empty());
        assert!(config.on_success.is_empty());
    }

    #[test]
    fn test_notify_config_yaml_single_string() {
        let yaml_str = "on_failure: slack://hooks.slack.com/xxx\n";
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(config.on_failure, vec!["slack://hooks.slack.com/xxx"]);
        assert!(config.on_success.is_empty());
    }

    #[test]
    fn test_notify_config_yaml_array() {
        let yaml_str = r#"
on_failure:
  - "slack://a"
  - "ntfy://b"
on_success:
  - "webhook://c"
  - "discord://d"
"#;
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(config.on_failure, vec!["slack://a", "ntfy://b"]);
        assert_eq!(config.on_success, vec!["webhook://c", "discord://d"]);
    }

    #[test]
    fn test_notify_config_serialize_single() {
        let config = NotifyConfig {
            on_failure: vec!["slack://a".to_string()],
            ..Default::default()
        };
        // Single-element vec serializes as a plain string
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("on_failure = \"slack://a\""));
        // Empty vec is skipped
        assert!(!toml_str.contains("on_success"));
    }

    #[test]
    fn test_notify_config_serialize_multi() {
        let config = NotifyConfig {
            on_failure: vec!["slack://a".to_string(), "ntfy://b".to_string()],
            ..Default::default()
        };
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("on_failure"));
        assert!(toml_str.contains("slack://a"));
        assert!(toml_str.contains("ntfy://b"));
    }

    #[test]
    fn test_notify_config_yaml_channels() {
        let yaml_str = r#"
channels:
  - target: "slack://hooks.slack.com/xxx"
    on: [failure, warning]
  - target: "ntfy://ntfy.sh/topic"
    on: [success, info]
"#;
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(config.channels.len(), 2);
        assert_eq!(config.channels[0].target, "slack://hooks.slack.com/xxx");
        assert_eq!(config.channels[0].on, vec![Severity::Failure, Severity::Warning]);
        assert_eq!(config.channels[1].target, "ntfy://ntfy.sh/topic");
        assert_eq!(config.channels[1].on, vec![Severity::Success, Severity::Info]);
    }

    #[test]
    fn test_notify_config_toml_channels() {
        let toml_str = r#"
[[channels]]
target = "slack://hooks.slack.com/xxx"
on = ["failure", "warning"]

[[channels]]
target = "ntfy://ntfy.sh/topic"
on = ["success"]
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.channels.len(), 2);
        assert_eq!(config.channels[0].target, "slack://hooks.slack.com/xxx");
        assert_eq!(config.channels[0].on, vec![Severity::Failure, Severity::Warning]);
        assert_eq!(config.channels[1].target, "ntfy://ntfy.sh/topic");
        assert_eq!(config.channels[1].on, vec![Severity::Success]);
    }

    #[test]
    fn test_notify_config_channels_alongside_legacy() {
        let yaml_str = r#"
on_failure: "slack://legacy"
channels:
  - target: "ntfy://new"
    on: [failure, success]
"#;
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(config.on_failure, vec!["slack://legacy"]);
        assert_eq!(config.channels.len(), 1);
        assert_eq!(config.channels[0].target, "ntfy://new");
    }

    #[test]
    fn test_notify_config_empty_channels() {
        let yaml_str = "on_failure: slack://a\n";
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert!(config.channels.is_empty());
    }

    #[test]
    fn test_notify_config_notify_override_default_false() {
        let yaml_str = "on_failure: slack://a\n";
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert!(!config.notify_override);
    }

    #[test]
    fn test_notify_config_notify_override_true() {
        let yaml_str = "notify_override: true\non_failure: slack://a\n";
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert!(config.notify_override);
        assert_eq!(config.on_failure, vec!["slack://a"]);
    }

    #[test]
    fn test_notify_config_notify_override_toml() {
        let toml_str = r#"
notify_override = true
on_failure = "slack://a"
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert!(config.notify_override);
        assert_eq!(config.on_failure, vec!["slack://a"]);
    }

    #[test]
    fn test_notify_config_serialize_skips_false_override() {
        let config = NotifyConfig {
            on_failure: vec!["slack://a".to_string()],
            ..Default::default()
        };
        let toml_str = toml::to_string(&config).unwrap();
        assert!(!toml_str.contains("notify_override"));
    }

    #[test]
    fn test_notify_config_toml_retry() {
        let toml_str = r#"
on_failure = "slack://a"

[retry]
max_attempts = 5
initial_delay_ms = 500
backoff_factor = 1.5
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.on_failure, vec!["slack://a"]);
        let retry = config.retry.unwrap();
        assert_eq!(retry.max_attempts, 5);
        assert_eq!(retry.initial_delay_ms, 500);
        assert_eq!(retry.backoff_factor, 1.5);
    }

    #[test]
    fn test_notify_config_yaml_retry() {
        let yaml_str = r#"
on_failure: slack://a
retry:
  max_attempts: 4
  initial_delay_ms: 2000
  backoff_factor: 3.0
"#;
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        let retry = config.retry.unwrap();
        assert_eq!(retry.max_attempts, 4);
        assert_eq!(retry.initial_delay_ms, 2000);
        assert_eq!(retry.backoff_factor, 3.0);
    }

    #[test]
    fn test_notify_config_retry_defaults_when_absent() {
        let toml_str = "on_failure = \"slack://a\"\n";
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert!(config.retry.is_none());
    }

    #[test]
    fn test_notify_config_retry_partial_defaults() {
        let toml_str = r#"
[retry]
max_attempts = 5
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        let retry = config.retry.unwrap();
        assert_eq!(retry.max_attempts, 5);
        assert_eq!(retry.initial_delay_ms, 1000); // default
        assert_eq!(retry.backoff_factor, 2.0); // default
    }

    #[test]
    fn test_notify_config_rate_limit_default_empty() {
        let toml_str = "on_failure = \"slack://a\"\n";
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert!(config.rate_limit.is_empty());
    }

    #[test]
    fn test_notify_config_toml_rate_limit() {
        let toml_str = r#"
[rate_limit.slack]
max_per_window = 10
window_secs = 60

[rate_limit.discord]
max_per_window = 5
window_secs = 30
"#;
        let config: NotifyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rate_limit.len(), 2);
        assert_eq!(config.rate_limit["slack"].max_per_window, 10);
        assert_eq!(config.rate_limit["slack"].window_secs, 60);
        assert_eq!(config.rate_limit["discord"].max_per_window, 5);
        assert_eq!(config.rate_limit["discord"].window_secs, 30);
    }

    #[test]
    fn test_notify_config_yaml_rate_limit() {
        let yaml_str = r#"
rate_limit:
  telegram:
    max_per_window: 20
    window_secs: 1
"#;
        let config: NotifyConfig = serde_yaml::from_str(yaml_str).unwrap();
        assert_eq!(config.rate_limit.len(), 1);
        assert_eq!(config.rate_limit["telegram"].max_per_window, 20);
        assert_eq!(config.rate_limit["telegram"].window_secs, 1);
    }

    #[test]
    fn test_notify_config_serialize_skips_empty_rate_limit() {
        let config = NotifyConfig {
            on_failure: vec!["slack://a".to_string()],
            ..Default::default()
        };
        let toml_str = toml::to_string(&config).unwrap();
        assert!(!toml_str.contains("rate_limit"));
    }

    #[test]
    fn test_mcp_step_config_alias_server() {
        let json = r#"{
            "server": "github",
            "tool": "create_issue",
            "args": {"repo": "myorg/myapp", "title": "Bug report"}
        }"#;
        let config: McpStepConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config.server, McpServerRef::Alias(ref s) if s == "github"));
        assert_eq!(config.tool, "create_issue");
        let args = config.args.unwrap();
        assert_eq!(args["repo"], "myorg/myapp");
        assert_eq!(args["title"], "Bug report");
    }

    #[test]
    fn test_mcp_step_config_inline_server() {
        let json = r#"{
            "server": {
                "command": "npx @modelcontextprotocol/server-github",
                "env": {"GITHUB_TOKEN": "xxx"},
                "secrets": ["GITHUB_TOKEN"]
            },
            "tool": "list_repos",
            "args": null
        }"#;
        let config: McpStepConfig = serde_json::from_str(json).unwrap();
        match &config.server {
            McpServerRef::Inline { command, env, secrets } => {
                assert_eq!(command, "npx @modelcontextprotocol/server-github");
                let env = env.as_ref().unwrap();
                assert_eq!(env["GITHUB_TOKEN"], "xxx");
                let secrets = secrets.as_ref().unwrap();
                assert_eq!(secrets, &vec!["GITHUB_TOKEN".to_string()]);
            }
            McpServerRef::Alias(_) => panic!("Expected Inline variant"),
        }
        assert_eq!(config.tool, "list_repos");
        assert!(config.args.is_none());
    }
}
