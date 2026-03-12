use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum RawStep {
    CmdString(String),
    CmdMap {
        id: Option<String>,
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
        retry: Option<u32>,
        #[serde(default)]
        retry_delay: Option<u64>,
        #[serde(default)]
        interactive: Option<bool>,
        #[serde(default)]
        outputs: Vec<StepOutput>,
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
pub enum TaskHeat {
    Hot,  // ≥5 runs in 30d
    Warm, // 1–4 runs in 30d
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

impl Default for TaskHeat {
    fn default() -> Self {
        TaskHeat::Cold
    }
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
    pub retry: Option<u32>,
    #[serde(default)]
    pub retry_delay: Option<u64>,
    #[serde(default)]
    pub interactive: Option<bool>,
    #[serde(default)]
    pub outputs: Vec<StepOutput>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifyConfig {
    #[serde(default)]
    pub on_failure: Option<String>,
    #[serde(default)]
    pub on_success: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    StepStarted { step_id: String, cmd_preview: String },
    StepCompleted { step_id: String, status: StepStatus, duration_ms: u64 },
    StepSkipped { step_id: String },
    StepRetrying { step_id: String, attempt: u32, max: u32, delay_secs: u64 },
    DangerousCommand { step_id: String, warning: String },
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
