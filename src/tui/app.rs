use crate::core::config::Config;
use crate::core::models::{Category, RunLog, Task};

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
        }
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
}
