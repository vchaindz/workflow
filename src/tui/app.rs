use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::core::ai::{AiResult, AiResponse, AiTool};
use crate::core::compare::CompareResult;
use crate::core::config::Config;
use crate::core::history::HistoryEntry;
use crate::core::executor::{InteractiveRequest, StreamingRequest};
use crate::core::db::OverdueTask;
use crate::core::models::{Category, ExecutionEvent, RunLog, RuntimeVariable, StepStatus, Task, TaskHeat, Workflow};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Sidebar,
    TaskList,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppMode {
    Normal,
    Running,
    StreamingOutput,
    ViewingLogs,
    Search,
    Help,
    Comparing,
    Wizard,
    ConfirmDelete,
    RecentRuns,
    SavedTasks,
    OverdueReminder,
    VariablePrompt,
}

#[derive(Debug, Clone)]
pub struct DeleteState {
    pub task_name: String,
    pub task_path: std::path::PathBuf,
    pub category: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WizardStage {
    ShellHistory,
    AiPrompt,
    AiThinking,
    TemplateBrowse,
    TemplateVariables,
    Category,
    TaskName,
    Options,
    Preview,
    AiRefinePrompt,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WizardMode {
    FromHistory,
    CloneTask,
    AiChat,
    AiUpdate,
    FromTemplate,
}

#[derive(Debug)]
pub struct WizardState {
    pub mode: WizardMode,
    pub stage: WizardStage,

    // History stage fields
    pub history_entries: Vec<HistoryEntry>,
    pub history_filter: String,
    pub history_filtered: Vec<usize>,
    pub history_cursor: usize,
    pub history_selected: Vec<usize>,
    pub history_scroll_offset: usize,

    // Clone-task fields (CloneTask mode only)
    pub source_task_ref: Option<String>,
    pub source_workflow: Option<Workflow>,
    pub source_run: Option<RunLog>,
    pub remove_failed: bool,
    pub remove_skipped: bool,
    pub parallelize: bool,
    pub active_toggle: usize,

    // AI Chat fields
    pub ai_prompt: String,
    pub ai_tool: Option<AiTool>,
    pub ai_result_rx: Option<mpsc::Receiver<AiResult>>,
    pub ai_commands: Vec<String>,
    pub ai_error: Option<String>,
    pub ai_tick: u8,

    // Template fields
    pub template_entries: Vec<crate::core::catalog::TemplateEntry>,
    pub template_filter: String,
    pub template_filtered: Vec<usize>,
    pub template_cursor: usize,
    pub template_scroll_offset: usize,
    pub template_var_values: Vec<(String, String, Option<String>)>, // (name, current_value, default)
    pub template_var_cursor: usize,

    // AI Update fields
    pub ai_source_yaml: String,
    pub ai_source_path: Option<std::path::PathBuf>,
    pub ai_updated_yaml: Option<String>,

    // AI Refine fields
    pub ai_refine_prompt: String,

    // Shared fields
    pub category: String,
    pub task_name: String,
    pub category_cursor: Option<usize>,
    pub preview_scroll: u16,
    pub save_message: Option<String>,
}

pub struct HeaderStats {
    pub total_workflows: usize,
    pub currently_running: bool,
    pub total_runs: u64,
    pub failed_runs: u64,
}

pub struct App {
    pub categories: Vec<Category>,
    pub config: Config,
    pub focus: Focus,
    pub mode: AppMode,
    pub should_quit: bool,
    pub header_stats: HeaderStats,

    // Sidebar state
    pub selected_category: usize,

    // Task list state
    pub selected_task: usize,

    // Details state
    pub detail_scroll: u16,

    // Search
    pub search_query: String,
    pub filtered_indices: Option<Vec<(usize, usize)>>, // (cat_idx, task_idx)

    // Running state
    pub run_output: Option<RunLog>,
    pub running_message: Option<String>,

    // Log viewing
    pub viewing_logs: Vec<RunLog>,

    // Collapsed categories (by index)
    pub collapsed: HashSet<usize>,

    // Async execution state
    pub event_rx: Option<mpsc::Receiver<ExecutionEvent>>,
    pub footer_log: Vec<String>,
    pub is_executing: bool,
    pub executing_task_ref: Option<String>,
    pub step_states: Vec<StepState>,

    // Interactive step channel (executor → TUI suspend request)
    pub interactive_rx: Option<mpsc::Receiver<InteractiveRequest>>,

    // Streaming output modal state
    pub streaming_rx: Option<mpsc::Receiver<StreamingRequest>>,
    pub streaming_lines: Vec<String>,
    pub streaming_scroll: u16,
    pub streaming_auto_scroll: bool,
    pub streaming_step_id: Option<String>,
    pub streaming_cmd: Option<String>,
    pub streaming_kill_tx: Option<mpsc::Sender<()>>,

    // Compare state
    pub compare_result: Option<CompareResult>,

    // Wizard state
    pub wizard: Option<WizardState>,

    // Delete confirmation state
    pub delete_state: Option<DeleteState>,

    // Recent runs modal
    pub recent_runs: Vec<RunLog>,
    pub recent_runs_cursor: usize,

    // Saved tasks modal
    pub saved_tasks_cursor: usize,

    // Overdue reminder modal
    pub overdue_tasks: Vec<OverdueTask>,
    pub overdue_cursor: usize,

    // Heat-based sorting
    pub sort_by_heat: bool,

    // Step command cache for content-aware search (path -> lowercased content)
    pub step_cmd_cache: HashMap<PathBuf, String>,

    // Wizard dry-run: return to Wizard after execution finishes
    pub pending_wizard_return: bool,

    // Variable prompt modal state
    pub var_prompt_vars: Vec<RuntimeVariable>,
    pub var_prompt_index: usize,
    pub var_prompt_choices: Vec<String>,
    pub var_prompt_cursor: usize,
    pub var_prompt_scroll: usize,
    pub var_prompt_resolved: HashMap<String, String>,
    pub var_prompt_dry_run: bool,
    pub var_prompt_task: Option<Task>,
    pub var_prompt_workflow: Option<Workflow>,
    pub var_prompt_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StepState {
    pub id: String,
    pub cmd_preview: String,
    pub status: StepStatus,
    pub duration_ms: Option<u64>,
}

impl App {
    pub fn new(categories: Vec<Category>, config: Config) -> Self {
        let total_workflows = categories.iter().map(|c| c.tasks.len()).sum();
        Self {
            categories,
            config,
            focus: Focus::Sidebar,
            mode: AppMode::Normal,
            should_quit: false,
            header_stats: HeaderStats {
                total_workflows,
                currently_running: false,
                total_runs: 0,
                failed_runs: 0,
            },
            selected_category: 0,
            selected_task: 0,
            detail_scroll: 0,
            search_query: String::new(),
            filtered_indices: None,
            run_output: None,
            running_message: None,
            viewing_logs: Vec::new(),
            collapsed: HashSet::new(),
            event_rx: None,
            footer_log: Vec::new(),
            is_executing: false,
            executing_task_ref: None,
            step_states: Vec::new(),
            interactive_rx: None,
            streaming_rx: None,
            streaming_lines: Vec::new(),
            streaming_scroll: 0,
            streaming_auto_scroll: true,
            streaming_step_id: None,
            streaming_cmd: None,
            streaming_kill_tx: None,
            compare_result: None,
            wizard: None,
            delete_state: None,
            recent_runs: Vec::new(),
            recent_runs_cursor: 0,
            saved_tasks_cursor: 0,
            overdue_tasks: Vec::new(),
            overdue_cursor: 0,
            sort_by_heat: false,
            step_cmd_cache: HashMap::new(),
            pending_wizard_return: false,
            var_prompt_vars: Vec::new(),
            var_prompt_index: 0,
            var_prompt_choices: Vec::new(),
            var_prompt_cursor: 0,
            var_prompt_scroll: 0,
            var_prompt_resolved: HashMap::new(),
            var_prompt_dry_run: false,
            var_prompt_task: None,
            var_prompt_workflow: None,
            var_prompt_error: None,
        }
    }

    pub fn refresh_stats(&mut self) {
        self.header_stats.total_workflows = self.categories.iter().map(|c| c.tasks.len()).sum();
        self.header_stats.currently_running = self.is_executing;

        let db_path = self.config.workflows_dir.join("history.db");
        if let Ok(conn) = crate::core::db::open_db(&db_path) {
            if let Ok(stats) = crate::core::db::get_global_stats(&conn) {
                self.header_stats.total_runs = stats.total_runs;
                self.header_stats.failed_runs = stats.failed_runs;
            }
        }
    }

    pub fn check_overdue(&mut self) {
        let db_path = self.config.workflows_dir.join("history.db");
        if let Ok(conn) = crate::core::db::open_db(&db_path) {
            if let Ok(tasks) = crate::core::db::check_overdue_tasks(&conn, &self.categories) {
                if !tasks.is_empty() {
                    self.overdue_tasks = tasks;
                    self.overdue_cursor = 0;
                    self.mode = AppMode::OverdueReminder;
                }
            }
        }
    }

    pub fn load_heat_data(&mut self) {
        let db_path = self.config.workflows_dir.join("history.db");
        if let Ok(conn) = crate::core::db::open_db(&db_path) {
            if let Ok(heat_map) = crate::core::db::get_task_heat(&conn) {
                for cat in &mut self.categories {
                    for task in &mut cat.tasks {
                        let task_ref = format!("{}/{}", cat.name, task.name);
                        let count = heat_map.get(&task_ref).copied().unwrap_or(0);
                        task.heat = match count {
                            0 => TaskHeat::Cold,
                            1..=4 => TaskHeat::Warm,
                            _ => TaskHeat::Hot,
                        };
                    }
                }
            }
        }
    }

    pub fn toggle_sort(&mut self) {
        self.sort_by_heat = !self.sort_by_heat;
        self.apply_sort();
    }

    pub fn apply_sort(&mut self) {
        for cat in &mut self.categories {
            if self.sort_by_heat {
                cat.tasks.sort_by(|a, b| {
                    a.heat.cmp(&b.heat)
                        .then_with(|| a.name.cmp(&b.name))
                });
            } else {
                cat.tasks.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }
        self.selected_task = 0;
    }

    /// Build a cache of step commands/content per task path for search.
    pub fn build_step_cmd_cache(&mut self) {
        self.step_cmd_cache.clear();
        for cat in &self.categories {
            for task in &cat.tasks {
                let path = &task.path;
                if self.step_cmd_cache.contains_key(path) {
                    continue;
                }
                let content = match task.kind {
                    crate::core::models::TaskKind::YamlWorkflow => {
                        if let Ok(wf) = crate::core::parser::parse_workflow(path) {
                            wf.steps.iter().map(|s| s.cmd.as_str()).collect::<Vec<_>>().join("\n").to_lowercase()
                        } else if let Ok(raw) = std::fs::read_to_string(path) {
                            raw.to_lowercase()
                        } else {
                            String::new()
                        }
                    }
                    crate::core::models::TaskKind::ShellScript => {
                        std::fs::read_to_string(path)
                            .unwrap_or_default()
                            .to_lowercase()
                    }
                };
                self.step_cmd_cache.insert(path.clone(), content);
            }
        }
    }

    pub fn toggle_collapse(&mut self) {
        let idx = self.selected_category;
        if self.collapsed.contains(&idx) {
            self.collapsed.remove(&idx);
        } else {
            self.collapsed.insert(idx);
        }
    }

    pub fn is_collapsed(&self, idx: usize) -> bool {
        self.collapsed.contains(&idx)
    }

    pub fn current_tasks(&self) -> &[Task] {
        if self.filtered_indices.is_some() {
            // In search mode, the caller should use filtered_tasks() instead
            return &[];
        }
        self.categories
            .get(self.selected_category)
            .map(|c| c.tasks.as_slice())
            .unwrap_or(&[])
    }

    pub fn filtered_tasks(&self) -> Vec<&Task> {
        if let Some(ref indices) = self.filtered_indices {
            indices
                .iter()
                .filter_map(|&(ci, ti)| {
                    self.categories.get(ci).and_then(|c| c.tasks.get(ti))
                })
                .collect()
        } else {
            self.current_tasks().iter().collect()
        }
    }

    pub fn selected_task_ref(&self) -> Option<&Task> {
        let tasks = self.filtered_tasks();
        tasks.get(self.selected_task).copied()
    }

    pub fn category_count(&self) -> usize {
        self.categories.len()
    }

    pub fn task_count(&self) -> usize {
        self.filtered_tasks().len()
    }

    pub fn move_up(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                if self.selected_category > 0 {
                    self.selected_category -= 1;
                    self.selected_task = 0;
                }
            }
            Focus::TaskList => {
                if self.selected_task > 0 {
                    self.selected_task -= 1;
                    self.run_output = None;
                    self.detail_scroll = 0;
                }
            }
            Focus::Details => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                if self.selected_category + 1 < self.category_count() {
                    self.selected_category += 1;
                    self.selected_task = 0;
                }
            }
            Focus::TaskList => {
                if self.selected_task + 1 < self.task_count() {
                    self.selected_task += 1;
                    self.run_output = None;
                    self.detail_scroll = 0;
                }
            }
            Focus::Details => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
        }
    }

    pub fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::TaskList,
            Focus::TaskList => Focus::Details,
            Focus::Details => Focus::Sidebar,
        };
    }

    pub fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Details,
            Focus::TaskList => Focus::Sidebar,
            Focus::Details => Focus::TaskList,
        };
    }

    pub fn start_search(&mut self) {
        self.mode = AppMode::Search;
        self.search_query.clear();
        self.filtered_indices = None;
    }

    pub fn update_search(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_indices = None;
            return;
        }

        let query = self.search_query.to_lowercase();
        let mut indices = Vec::new();

        for (ci, cat) in self.categories.iter().enumerate() {
            for (ti, task) in cat.tasks.iter().enumerate() {
                if task.name.to_lowercase().contains(&query)
                    || cat.name.to_lowercase().contains(&query)
                {
                    indices.push((ci, ti));
                } else if let Some(content) = self.step_cmd_cache.get(&task.path) {
                    if content.contains(&query) {
                        indices.push((ci, ti));
                    }
                }
            }
        }

        self.filtered_indices = Some(indices);
        self.selected_task = 0;
    }

    pub fn cancel_search(&mut self) {
        self.mode = AppMode::Normal;
        self.search_query.clear();
        self.filtered_indices = None;
    }

    /// Navigate to a task by its "category/name" ref. Returns true if found.
    pub fn navigate_to_task(&mut self, task_ref: &str) -> bool {
        for (ci, cat) in self.categories.iter().enumerate() {
            for (ti, task) in cat.tasks.iter().enumerate() {
                let tr = format!("{}/{}", cat.name, task.name);
                if tr == task_ref {
                    self.filtered_indices = None;
                    self.search_query.clear();
                    self.selected_category = ci;
                    self.selected_task = ti;
                    self.focus = Focus::TaskList;
                    self.run_output = None;
                    self.detail_scroll = 0;
                    return true;
                }
            }
        }
        false
    }

    pub fn drain_execution_events(&mut self) {
        let rx = match self.event_rx.as_ref() {
            Some(rx) => rx,
            None => return,
        };

        loop {
            match rx.try_recv() {
                Ok(event) => match event {
                    ExecutionEvent::StepStarted { step_id, cmd_preview } => {
                        self.step_states.push(StepState {
                            id: step_id.clone(),
                            cmd_preview: cmd_preview.clone(),
                            status: StepStatus::Running,
                            duration_ms: None,
                        });
                        self.footer_log.push(format!(
                            "[{}] ▶ {} — {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            step_id,
                            cmd_preview,
                        ));
                    }
                    ExecutionEvent::StepCompleted { step_id, status, duration_ms } => {
                        if let Some(state) = self.step_states.iter_mut().find(|s| s.id == step_id) {
                            state.status = status.clone();
                            state.duration_ms = Some(duration_ms);
                        }
                        let icon = match status {
                            StepStatus::Success => "✓",
                            StepStatus::Failed => "✗",
                            StepStatus::Interactive => "⇄",
                            _ => "?",
                        };
                        self.footer_log.push(format!(
                            "[{}] {} {} ({}ms)",
                            chrono::Local::now().format("%H:%M:%S"),
                            icon,
                            step_id,
                            duration_ms,
                        ));
                    }
                    ExecutionEvent::StepTimedOut { step_id, timeout_secs, duration_ms } => {
                        if let Some(state) = self.step_states.iter_mut().find(|s| s.id == step_id) {
                            state.status = StepStatus::Timedout;
                            state.duration_ms = Some(duration_ms);
                        }
                        self.footer_log.push(format!(
                            "[{}] ⏱ {} timed out after {}s ({}ms)",
                            chrono::Local::now().format("%H:%M:%S"),
                            step_id,
                            timeout_secs,
                            duration_ms,
                        ));
                    }
                    ExecutionEvent::StepOutput { step_id: _, line } => {
                        self.streaming_lines.push(line);
                        // Auto-scroll if enabled
                        if self.streaming_auto_scroll {
                            let total = self.streaming_lines.len() as u16;
                            self.streaming_scroll = total.saturating_sub(1);
                        }
                    }
                    ExecutionEvent::DangerousCommand { step_id, warning } => {
                        self.footer_log.push(format!(
                            "[{}] ⚠ {} BLOCKED: {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            step_id,
                            warning,
                        ));
                    }
                    ExecutionEvent::StepRetrying { step_id, attempt, max, delay_secs } => {
                        self.footer_log.push(format!(
                            "[{}] ↻ {} retry {}/{} (wait {}s)",
                            chrono::Local::now().format("%H:%M:%S"),
                            step_id,
                            attempt,
                            max,
                            delay_secs,
                        ));
                    }
                    ExecutionEvent::StepSkipped { step_id } => {
                        self.step_states.push(StepState {
                            id: step_id.clone(),
                            cmd_preview: String::new(),
                            status: StepStatus::Skipped,
                            duration_ms: None,
                        });
                        self.footer_log.push(format!(
                            "[{}] ⊘ {} skipped",
                            chrono::Local::now().format("%H:%M:%S"),
                            step_id,
                        ));
                    }
                    ExecutionEvent::WorkflowFinished { run_log } => {
                        let icon = if run_log.exit_code == 0 { "✓" } else { "✗" };
                        self.footer_log.push(format!(
                            "[{}] {} Done (exit {})",
                            chrono::Local::now().format("%H:%M:%S"),
                            icon,
                            run_log.exit_code,
                        ));
                        self.run_output = Some(run_log);
                        if self.mode == AppMode::StreamingOutput {
                            // Stay in streaming mode so user can see final output
                            // They'll press Esc/q to close
                        } else if self.pending_wizard_return {
                            self.pending_wizard_return = false;
                            self.mode = AppMode::Wizard;
                        } else {
                            self.mode = AppMode::Normal;
                        }
                        self.focus = Focus::TaskList;
                        self.detail_scroll = 0;
                        self.is_executing = false;
                        self.event_rx = None;
                        self.interactive_rx = None;
                        self.streaming_rx = None;
                        return;
                    }
                    ExecutionEvent::WorkflowError { message } => {
                        self.footer_log.push(format!(
                            "[{}] ✗ Error: {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            message,
                        ));
                        if self.mode != AppMode::StreamingOutput {
                            if self.pending_wizard_return {
                                self.pending_wizard_return = false;
                                self.mode = AppMode::Wizard;
                            } else {
                                self.mode = AppMode::Normal;
                            }
                        }
                        self.is_executing = false;
                        self.event_rx = None;
                        self.interactive_rx = None;
                        self.streaming_rx = None;
                        return;
                    }
                },
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.is_executing {
                        self.footer_log.push(format!(
                            "[{}] ✗ Execution thread disconnected unexpectedly",
                            chrono::Local::now().format("%H:%M:%S"),
                        ));
                        if self.mode != AppMode::StreamingOutput {
                            if self.pending_wizard_return {
                                self.pending_wizard_return = false;
                                self.mode = AppMode::Wizard;
                            } else {
                                self.mode = AppMode::Normal;
                            }
                        }
                        self.is_executing = false;
                    }
                    self.event_rx = None;
                    self.interactive_rx = None;
                    self.streaming_rx = None;
                    return;
                }
            }
        }

        // Cap log size
        if self.footer_log.len() > 100 {
            let start = self.footer_log.len() - 50;
            self.footer_log = self.footer_log[start..].to_vec();
        }
    }

    pub fn check_streaming_requests(&mut self) {
        let rx = match self.streaming_rx.as_ref() {
            Some(rx) => rx,
            None => return,
        };

        if let Ok(req) = rx.try_recv() {
            self.streaming_lines.clear();
            self.streaming_scroll = 0;
            self.streaming_auto_scroll = true;
            self.streaming_step_id = Some(req.step_id);
            self.streaming_cmd = Some(req.cmd_preview);
            self.streaming_kill_tx = Some(req.kill_tx);
            self.mode = AppMode::StreamingOutput;
        }
    }

    pub fn close_streaming_modal(&mut self) {
        // Kill the child process if still running
        if let Some(kill_tx) = self.streaming_kill_tx.take() {
            let _ = kill_tx.send(());
        }
        self.streaming_step_id = None;
        self.streaming_cmd = None;
        self.streaming_lines.clear();
        self.streaming_scroll = 0;
        if !self.is_executing {
            if self.pending_wizard_return {
                self.pending_wizard_return = false;
                self.mode = AppMode::Wizard;
            } else {
                self.mode = AppMode::Normal;
            }
        } else {
            self.mode = AppMode::Running;
        }
    }

    pub fn drain_ai_events(&mut self) {
        let wiz = match self.wizard.as_mut() {
            Some(w) if w.stage == WizardStage::AiThinking => w,
            _ => return,
        };

        let rx = match wiz.ai_result_rx.as_ref() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(AiResult::Yaml(yaml)) => {
                // AI update mode: store the raw YAML and go straight to preview
                wiz.ai_updated_yaml = Some(yaml);
                wiz.ai_result_rx = None;
                wiz.ai_error = None;
                wiz.stage = WizardStage::Preview;
                wiz.preview_scroll = 0;
            }
            Ok(AiResult::Success(AiResponse { commands, task_name, category })) => {
                // Use AI-suggested name/category, fall back to heuristics
                wiz.category = category.unwrap_or_else(|| {
                    let refs: Vec<&str> = commands.iter().map(|s| s.as_str()).collect();
                    crate::core::history::suggest_category(&refs)
                });
                wiz.task_name = task_name.unwrap_or_else(|| {
                    commands.first()
                        .map(|c| crate::core::history::derive_task_name(c))
                        .unwrap_or_else(|| "task".to_string())
                });
                wiz.ai_commands = commands;
                wiz.ai_result_rx = None;
                wiz.ai_error = None;
                wiz.stage = WizardStage::Category;
            }
            Ok(AiResult::Error(msg)) => {
                wiz.ai_error = Some(msg);
                wiz.ai_result_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                wiz.ai_tick = wiz.ai_tick.wrapping_add(1);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                if wiz.ai_error.is_none() {
                    wiz.ai_error = Some("AI process disconnected unexpectedly".to_string());
                }
                wiz.ai_result_rx = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::core::models::{Category, Task, TaskKind};
    use std::path::PathBuf;

    fn make_test_app() -> App {
        let categories = vec![
            Category {
                name: "backup".into(),
                path: PathBuf::from("/tmp/wf/backup"),
                tasks: vec![
                    Task {
                        name: "db-full".into(),
                        kind: TaskKind::YamlWorkflow,
                        path: PathBuf::from("/tmp/wf/backup/db-full.yaml"),
                        category: "backup".into(),
                        last_run: None,
                        overdue: None,
                        heat: TaskHeat::Cold,
                    },
                    Task {
                        name: "files".into(),
                        kind: TaskKind::ShellScript,
                        path: PathBuf::from("/tmp/wf/backup/files.sh"),
                        category: "backup".into(),
                        last_run: None,
                        overdue: None,
                        heat: TaskHeat::Cold,
                    },
                ],
            },
            Category {
                name: "deploy".into(),
                path: PathBuf::from("/tmp/wf/deploy"),
                tasks: vec![Task {
                    name: "staging".into(),
                    kind: TaskKind::YamlWorkflow,
                    path: PathBuf::from("/tmp/wf/deploy/staging.yaml"),
                    category: "deploy".into(),
                    last_run: None,
                    overdue: None,
                    heat: TaskHeat::Cold,
                }],
            },
        ];
        let config = Config {
            workflows_dir: PathBuf::from("/tmp/wf"),
            ..Config::default()
        };
        App::new(categories, config)
    }

    #[test]
    fn test_focus_cycling() {
        let mut app = make_test_app();
        assert_eq!(app.focus, Focus::Sidebar);

        app.focus_next();
        assert_eq!(app.focus, Focus::TaskList);
        app.focus_next();
        assert_eq!(app.focus, Focus::Details);
        app.focus_next();
        assert_eq!(app.focus, Focus::Sidebar);

        // Reverse
        app.focus_prev();
        assert_eq!(app.focus, Focus::Details);
        app.focus_prev();
        assert_eq!(app.focus, Focus::TaskList);
        app.focus_prev();
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn test_navigation() {
        let mut app = make_test_app();

        // Sidebar: move down clamps at end
        assert_eq!(app.selected_category, 0);
        app.move_down();
        assert_eq!(app.selected_category, 1);
        app.move_down(); // already at last
        assert_eq!(app.selected_category, 1);

        // Moving category resets selected_task
        app.selected_task = 1;
        app.move_up();
        assert_eq!(app.selected_category, 0);
        assert_eq!(app.selected_task, 0);

        // move_up at 0 stays at 0
        app.move_up();
        assert_eq!(app.selected_category, 0);

        // TaskList navigation
        app.focus = Focus::TaskList;
        assert_eq!(app.selected_task, 0);
        app.move_down();
        assert_eq!(app.selected_task, 1);
        app.move_down(); // clamp: backup has 2 tasks (index 0,1)
        assert_eq!(app.selected_task, 1);
        app.move_up();
        assert_eq!(app.selected_task, 0);
        app.move_up(); // clamp at 0
        assert_eq!(app.selected_task, 0);
    }

    #[test]
    fn test_collapse() {
        let mut app = make_test_app();
        assert!(!app.is_collapsed(0));

        app.toggle_collapse();
        assert!(app.is_collapsed(0));

        app.toggle_collapse();
        assert!(!app.is_collapsed(0));
    }

    #[test]
    fn test_search() {
        let mut app = make_test_app();

        app.start_search();
        assert_eq!(app.mode, AppMode::Search);
        assert!(app.search_query.is_empty());

        // Type "stag" → matches "staging" in deploy
        app.search_query = "stag".into();
        app.update_search();
        let indices = app.filtered_indices.as_ref().unwrap();
        assert_eq!(indices.len(), 1);
        assert_eq!(indices[0], (1, 0)); // deploy cat, first task

        // Type "backup" → matches all tasks in backup category
        app.search_query = "backup".into();
        app.update_search();
        let indices = app.filtered_indices.as_ref().unwrap();
        assert_eq!(indices.len(), 2);

        // Empty query clears filter
        app.search_query.clear();
        app.update_search();
        assert!(app.filtered_indices.is_none());

        // Cancel restores Normal mode
        app.search_query = "test".into();
        app.cancel_search();
        assert_eq!(app.mode, AppMode::Normal);
        assert!(app.search_query.is_empty());
        assert!(app.filtered_indices.is_none());
    }

    #[test]
    fn test_selected_task_ref() {
        let mut app = make_test_app();

        // Default: first task of first category
        let task = app.selected_task_ref().unwrap();
        assert_eq!(task.name, "db-full");

        // Navigate to second task
        app.focus = Focus::TaskList;
        app.move_down();
        let task = app.selected_task_ref().unwrap();
        assert_eq!(task.name, "files");

        // Switch category → resets to first task
        app.focus = Focus::Sidebar;
        app.move_down();
        let task = app.selected_task_ref().unwrap();
        assert_eq!(task.name, "staging");
    }

    #[test]
    fn test_empty_categories() {
        let config = Config {
            workflows_dir: PathBuf::from("/tmp/wf"),
            ..Config::default()
        };
        let mut app = App::new(vec![], config);

        // No panics on empty data
        assert!(app.selected_task_ref().is_none());
        assert_eq!(app.category_count(), 0);
        assert_eq!(app.task_count(), 0);

        app.move_down();
        app.move_up();
        app.focus_next();
        app.toggle_collapse();
        app.start_search();
        app.search_query = "anything".into();
        app.update_search();
        assert_eq!(
            app.filtered_indices.as_ref().unwrap().len(),
            0
        );
    }
}
