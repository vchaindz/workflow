use std::collections::HashSet;
use std::sync::mpsc;

use crate::core::config::Config;
use crate::core::models::{Category, ExecutionEvent, RunLog, StepStatus, Task, Workflow};

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
    ViewingLogs,
    Search,
    Help,
    Wizard,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WizardStage {
    Category,
    TaskName,
    Options,
    Preview,
}

#[derive(Debug, Clone)]
pub struct WizardState {
    pub stage: WizardStage,
    pub source_task_ref: String,
    pub source_workflow: Workflow,
    pub source_run: Option<RunLog>,
    pub category: String,
    pub task_name: String,
    pub category_cursor: Option<usize>,
    pub remove_failed: bool,
    pub remove_skipped: bool,
    pub parallelize: bool,
    pub preview_scroll: u16,
    pub active_toggle: usize,
    pub save_message: Option<String>,
}

pub struct App {
    pub categories: Vec<Category>,
    pub config: Config,
    pub focus: Focus,
    pub mode: AppMode,
    pub should_quit: bool,

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

    // Wizard state
    pub wizard: Option<WizardState>,
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
        Self {
            categories,
            config,
            focus: Focus::Sidebar,
            mode: AppMode::Normal,
            should_quit: false,
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
            wizard: None,
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
                        self.mode = AppMode::Normal;
                        self.focus = Focus::Details;
                        self.detail_scroll = 0;
                        self.is_executing = false;
                        self.event_rx = None;
                        return;
                    }
                    ExecutionEvent::WorkflowError { message } => {
                        self.footer_log.push(format!(
                            "[{}] ✗ Error: {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            message,
                        ));
                        self.mode = AppMode::Normal;
                        self.is_executing = false;
                        self.event_rx = None;
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
                        self.mode = AppMode::Normal;
                        self.is_executing = false;
                    }
                    self.event_rx = None;
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
                    },
                    Task {
                        name: "files".into(),
                        kind: TaskKind::ShellScript,
                        path: PathBuf::from("/tmp/wf/backup/files.sh"),
                        category: "backup".into(),
                        last_run: None,
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
