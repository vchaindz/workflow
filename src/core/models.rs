use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// An env value can be a plain string or a dynamic command to execute.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    Static(String),
    Dynamic { cmd: String },
}

/// Deserialization-only types for flexible YAML step formats.
/// Supports: bare string, map without id, full map with id/needs.
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
    },
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawWorkflow {
    pub name: String,
    pub steps: Vec<RawStep>,
    #[serde(default)]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub workdir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskKind {
    ShellScript,
    YamlWorkflow,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub workdir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub cmd: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
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

#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    StepStarted { step_id: String, cmd_preview: String },
    StepCompleted { step_id: String, status: StepStatus, duration_ms: u64 },
    StepSkipped { step_id: String },
    WorkflowFinished { run_log: RunLog },
    WorkflowError { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub fail_count: u32,
    pub last_duration_ms: Option<u64>,
}
