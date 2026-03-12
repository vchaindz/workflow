use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::db;
use crate::core::history;
use crate::core::models::{StepStatus, TaskHeat, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::core::wizard;

use crate::core::compare;

use crate::core::catalog;

use super::app::{App, AppMode, Focus, WizardMode, WizardStage};

pub fn draw(f: &mut Frame, app: &App) {
    let has_footer = !app.footer_log.is_empty();

    let chunks = if has_footer {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(7),
                Constraint::Length(1),
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(f.area())
    };

    draw_header(f, app, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(25),
            Constraint::Percentage(55),
        ])
        .split(chunks[1]);

    draw_sidebar(f, app, main_chunks[0]);
    draw_task_list(f, app, main_chunks[1]);
    draw_details(f, app, main_chunks[2]);

    if has_footer {
        draw_footer(f, app, chunks[2]);
        draw_status_bar(f, app, chunks[3]);
    } else {
        draw_status_bar(f, app, chunks[2]);
    }

    if app.mode == AppMode::Help {
        draw_help(f, app);
    }

    if app.mode == AppMode::Wizard {
        draw_wizard(f, app);
    }

    if app.mode == AppMode::ConfirmDelete {
        draw_confirm_delete(f, app);
    }

    if app.mode == AppMode::StreamingOutput {
        draw_streaming_modal(f, app);
    }

    if app.mode == AppMode::RecentRuns {
        draw_recent_runs(f, app);
    }

    if app.mode == AppMode::SavedTasks {
        draw_saved_tasks(f, app);
    }

    if app.mode == AppMode::OverdueReminder {
        draw_overdue_reminder(f, app);
    }

    if app.mode == AppMode::VariablePrompt {
        draw_variable_prompt(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let version = env!("CARGO_PKG_VERSION");
    let stats = &app.header_stats;

    let running_style = if stats.currently_running {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let running_count: usize = if stats.currently_running { 1 } else { 0 };

    let spans = vec![
        Span::styled(
            format!(" workflow v{}", version),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("Workflows: {}", stats.total_workflows),
            Style::default().fg(Color::White),
        ),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("Running: {}", running_count), running_style),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("Total runs: {}", stats.total_runs),
            Style::default().fg(Color::White),
        ),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("Failed: {}", stats.failed_runs),
            if stats.failed_runs > 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::White)
            },
        ),
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if let Some(Some(tool)) = app.cached_ai_tool {
                format!("AI: {}", tool.name())
            } else if app.cached_ai_tool == Some(None) {
                "AI: none".to_string()
            } else {
                "AI: ...".to_string()
            },
            Style::default().fg(Color::DarkGray),
        ),
    ];

    let header = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));
    f.render_widget(header, area);
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::Sidebar {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    if app.categories.is_empty() {
        let block = Block::default()
            .title("Categories")
            .borders(Borders::ALL)
            .border_style(style);

        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No workflows found",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(" w ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" From history", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" AI generate", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled(" t ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" Templates", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled(" e ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" Open dir", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press h for help",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let para = Paragraph::new(empty_lines).block(block);
        f.render_widget(para, area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();

    for (i, cat) in app.categories.iter().enumerate() {
        let marker = if i == app.selected_category {
            ">"
        } else {
            " "
        };
        let cat_style = if i == app.selected_category {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(
            ListItem::new(format!(
                "{marker} {} ({})",
                cat.name,
                cat.tasks.len()
            ))
            .style(cat_style),
        );
    }

    let block = Block::default()
        .title("Categories")
        .borders(Borders::ALL)
        .border_style(style);

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_task_list(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::TaskList {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let tasks = app.filtered_tasks();

    let mut items: Vec<ListItem> = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        let marker = if i == app.selected_task { ">" } else { " " };
        let kind = match task.kind {
            TaskKind::ShellScript => "sh",
            TaskKind::YamlWorkflow => "yaml",
        };
        let task_ref = format!("{}/{}", task.category, task.name);
        let star = if app.config.bookmarks.contains(&task_ref) { "★ " } else { "" };
        let (heat_icon, heat_color) = match task.heat {
            TaskHeat::Hot => ("▲", Color::Green),
            TaskHeat::Warm => ("·", Color::Reset),
            TaskHeat::Cold => ("▽", Color::Blue),
        };
        let name_style = if i == app.selected_task {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        // Last-run indicator
        let run_indicator = if let Some(ref summary) = task.last_run {
            let (icon, time) = if summary.fail_count > 0 && summary.last_failure > summary.last_success {
                let t = format_relative_short(summary.last_failure);
                ("\u{2717}", t) // ✗
            } else if summary.last_success.is_some() {
                let t = format_relative_short(summary.last_success);
                ("\u{2713}", t) // ✓
            } else {
                ("", String::new())
            };
            if icon.is_empty() {
                String::new()
            } else {
                format!(" {} {}", icon, time)
            }
        } else {
            String::new()
        };
        let run_color = if let Some(ref summary) = task.last_run {
            if summary.fail_count > 0 && summary.last_failure > summary.last_success {
                Color::Red
            } else {
                Color::Green
            }
        } else {
            Color::DarkGray
        };

        let mut spans = vec![
            Span::styled(format!("{marker} "), name_style),
            Span::styled(format!("{heat_icon} "), Style::default().fg(heat_color)),
            Span::styled(format!("{star}{} [{kind}]", task.name), name_style),
        ];
        if !run_indicator.is_empty() {
            spans.push(Span::styled(run_indicator, Style::default().fg(run_color)));
        }
        let line = Line::from(spans);
        items.push(ListItem::new(line));
    }

    let title = if app.filtered_indices.is_some() {
        format!("Tasks (search: {})", app.search_query)
    } else if app.status_filter != super::app::StatusFilter::All {
        format!("Tasks [{}]", app.status_filter.label())
    } else {
        "Tasks".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(style);

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::Details {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title("Details")
        .borders(Borders::ALL)
        .border_style(style);

    let content = if app.mode == AppMode::Comparing {
        if let Some(ref result) = app.compare_result {
            compare::format_compare(result, false)
        } else {
            "No comparison data".to_string()
        }
    } else if app.mode == AppMode::Running {
        format_live_progress(app)
    } else if app.mode == AppMode::ViewingLogs {
        format_logs(&app.viewing_logs)
    } else if let Some(ref run_log) = app.run_output {
        format_run_log(run_log)
    } else if let Some(task) = app.selected_task_ref() {
        let task_ref = format!("{}/{}", task.category, task.name);
        let last_run = db::open_db(&app.config.db_path())
            .ok()
            .and_then(|conn| db::get_task_history(&conn, &task_ref, 1).ok())
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });
        let mut preview = format_task_preview(task);
        if let Some(run_log) = last_run {
            preview.push_str("\n--- Last Run ---\n");
            preview.push_str(&format_run_log(&run_log));
        }
        preview
    } else {
        "Select a task to preview".to_string()
    };

    // Render content through tui-markdown for syntax highlighting
    let text = tui_markdown::from_str(&content);

    // Approximate line count (including wraps) to clamp scroll
    let inner_width = area.width.saturating_sub(2) as usize; // borders
    let content_lines: u16 = text
        .lines
        .iter()
        .map(|l| {
            if inner_width == 0 {
                1u16
            } else {
                1 + (l.width() / inner_width.max(1)) as u16
            }
        })
        .sum();
    let inner_height = area.height.saturating_sub(2); // borders
    let max_scroll = content_lines.saturating_sub(inner_height);
    let scroll = app.detail_scroll.min(max_scroll);

    let para = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    f.render_widget(para, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content = match app.mode {
        AppMode::Search => {
            format!("Search: {}_ | ESC cancel", app.search_query)
        }
        AppMode::Running => "Running... (output in footer below)".to_string(),
        AppMode::StreamingOutput => "Streaming output — Esc/q to close".to_string(),
        AppMode::Comparing => "c:compare | ESC:back | Up/Down:scroll".to_string(),
        AppMode::Wizard => " New Task Wizard ".to_string(),
        _ => {
            let sort_label = if app.sort_by_heat { "f:α-sort" } else { "f:heat-sort" };
            let filter_label = format!("F:{}", app.status_filter.next().label());
            format!("arrows:nav  r:run  d:dry-run  e:edit  c:compare  {sort_label}  {filter_label}  w:new  W:clone  t:template  a:ai  R:recent  s:saved  L:logs  /:search  h:help  q:quit")
        }
    };

    let bar = Paragraph::new(Line::from(vec![Span::styled(
        content,
        Style::default()
            .fg(Color::Black)
            .bg(Color::White),
    )]));
    f.render_widget(bar, area);
}

fn draw_wizard(f: &mut Frame, app: &App) {
    let wiz = match app.wizard.as_ref() {
        Some(w) => w,
        None => return,
    };

    let area = f.area();
    // Larger modal for history stage, medium for AI stages
    let (mw, mh) = if matches!(wiz.stage, WizardStage::ShellHistory | WizardStage::TemplateBrowse) {
        (80.min(area.width.saturating_sub(4)), 32.min(area.height.saturating_sub(2)))
    } else if matches!(wiz.stage, WizardStage::AiPrompt | WizardStage::AiThinking | WizardStage::TemplateVariables) {
        (70.min(area.width.saturating_sub(4)), 20.min(area.height.saturating_sub(2)))
    } else {
        (64.min(area.width.saturating_sub(6)), 26.min(area.height.saturating_sub(4)))
    };
    let x = (area.width.saturating_sub(mw)) / 2;
    let y = (area.height.saturating_sub(mh)) / 2;
    let popup = Rect::new(x, y, mw, mh);

    f.render_widget(Clear, popup);

    let title = match wiz.mode {
        WizardMode::FromHistory => " New Task from History ",
        WizardMode::CloneTask => " Clone Task ",
        WizardMode::AiChat => " AI Task Generator ",
        WizardMode::AiUpdate => " AI Task Update ",
        WizardMode::FromTemplate => " Template Catalog ",
    };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(block, popup);

    let inner = Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), popup.height.saturating_sub(2));

    let mut lines: Vec<Line> = Vec::new();

    // Dynamic progress breadcrumb
    let (stages, current_idx): (Vec<&str>, usize) = match wiz.mode {
        WizardMode::FromHistory => {
            let s = vec!["History", "Category", "Name", "Preview"];
            let idx = match wiz.stage {
                WizardStage::ShellHistory => 0,
                WizardStage::Category => 1,
                WizardStage::TaskName => 2,
                WizardStage::Preview => 3,
                _ => 0,
            };
            (s, idx)
        }
        WizardMode::CloneTask => {
            let s = vec!["Category", "Name", "Options", "Preview"];
            let idx = match wiz.stage {
                WizardStage::Category => 0,
                WizardStage::TaskName => 1,
                WizardStage::Options => 2,
                WizardStage::Preview => 3,
                _ => 0,
            };
            (s, idx)
        }
        WizardMode::AiChat => {
            let s = vec!["Prompt", "AI", "Category", "Name", "Preview"];
            let idx = match wiz.stage {
                WizardStage::AiPrompt => 0,
                WizardStage::AiThinking | WizardStage::AiRefinePrompt => 1,
                WizardStage::Category => 2,
                WizardStage::TaskName => 3,
                WizardStage::Preview => 4,
                _ => 0,
            };
            (s, idx)
        }
        WizardMode::AiUpdate => {
            let s = vec!["Prompt", "AI", "Preview"];
            let idx = match wiz.stage {
                WizardStage::AiPrompt => 0,
                WizardStage::AiThinking | WizardStage::AiRefinePrompt => 1,
                WizardStage::Preview => 2,
                _ => 0,
            };
            (s, idx)
        }
        WizardMode::FromTemplate => {
            let has_vars = !wiz.template_var_values.is_empty()
                || !wiz.template_entries.get(wiz.template_cursor)
                    .map(|e| e.variables.is_empty())
                    .unwrap_or(true);
            if has_vars {
                let s = vec!["Browse", "Variables", "Category", "Name", "Preview"];
                let idx = match wiz.stage {
                    WizardStage::TemplateBrowse => 0,
                    WizardStage::TemplateVariables => 1,
                    WizardStage::Category => 2,
                    WizardStage::TaskName => 3,
                    WizardStage::Preview => 4,
                    _ => 0,
                };
                (s, idx)
            } else {
                let s = vec!["Browse", "Category", "Name", "Preview"];
                let idx = match wiz.stage {
                    WizardStage::TemplateBrowse => 0,
                    WizardStage::Category => 1,
                    WizardStage::TaskName => 2,
                    WizardStage::Preview => 3,
                    _ => 0,
                };
                (s, idx)
            }
        }
    };

    let mut progress_spans: Vec<Span> = Vec::new();
    for (i, label) in stages.iter().enumerate() {
        if i > 0 {
            let sep_color = if i <= current_idx { Color::Cyan } else { Color::DarkGray };
            progress_spans.push(Span::styled(" > ", Style::default().fg(sep_color)));
        }
        let style = if i == current_idx {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if i < current_idx {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let prefix = if i < current_idx { "\u{2713} " } else { "" };
        progress_spans.push(Span::styled(format!("{prefix}{label}"), style));
    }
    lines.push(Line::from(progress_spans));

    let sep_w = inner.width as usize;
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(sep_w),
        Style::default().fg(Color::DarkGray),
    )));

    // Source info for clone mode
    if let Some(ref task_ref) = wiz.source_task_ref {
        lines.push(Line::from(vec![
            Span::styled("Source  ", Style::default().fg(Color::DarkGray)),
            Span::styled(task_ref.as_str(), Style::default().fg(Color::White)),
        ]));
        lines.push(Line::from(""));
    }

    // Handle save confirmation
    if let Some(ref msg) = wiz.save_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "\u{2713} Task created successfully",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )));

        let para = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(para, inner);
        return;
    }

    match wiz.stage {
        WizardStage::AiPrompt => {
            let tool_name = wiz.ai_tool.map(|t| t.name()).unwrap_or("AI");
            let is_update = wiz.mode == WizardMode::AiUpdate;

            lines.push(Line::from(Span::styled(
                if is_update { "Describe how to update this task" } else { "Describe the task you want to create" },
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("Using ", Style::default().fg(Color::DarkGray)),
                Span::styled(tool_name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(
                    if is_update { " to update workflow" } else { " to generate commands" },
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            if is_update {
                lines.push(Line::from(vec![
                    Span::styled("Task: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}/{}", wiz.category, wiz.task_name),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }

            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{}_", wiz.ai_prompt),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            lines.push(Line::from(Span::styled(
                "Examples:",
                Style::default().fg(Color::DarkGray),
            )));

            let examples: &[&str] = if is_update {
                &[
                    "  add error handling to each step",
                    "  parallelize independent steps",
                    "  add a cleanup step at the end",
                    "  increase timeouts to 120 seconds",
                ]
            } else {
                &[
                    "  backup postgres database to S3",
                    "  check if nginx is running and restart if down",
                    "  monitor disk usage and alert if above 90%",
                    "  rotate log files older than 7 days",
                ]
            };
            for example in examples {
                lines.push(Line::from(Span::styled(
                    *example,
                    Style::default().fg(Color::DarkGray),
                )));
            }

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Send"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::AiThinking => {
            if let Some(ref err) = wiz.ai_error {
                // Error state
                lines.push(Line::from(Span::styled(
                    "AI generation failed",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Prompt: {}", wiz.ai_prompt),
                    Style::default().fg(Color::DarkGray),
                )));

                push_wizard_footer(&mut lines, inner.height, &[
                    ("Esc", "Back to prompt"),
                ]);
            } else {
                // Spinner state
                let spinner_frames = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}"];
                let frame = spinner_frames[(wiz.ai_tick as usize) % spinner_frames.len()];
                let tool_name = wiz.ai_tool.map(|t| t.name()).unwrap_or("AI");

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", frame),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("Generating commands with {}...", tool_name),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("Prompt: {}", wiz.ai_prompt),
                    Style::default().fg(Color::DarkGray),
                )));

                push_wizard_footer(&mut lines, inner.height, &[
                    ("Esc", "Cancel"),
                ]);
            }
        }

        WizardStage::ShellHistory => {
            // Filter input
            lines.push(Line::from(vec![
                Span::styled(" Filter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(
                    format!("{}_", wiz.history_filter),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));

            // Count line
            lines.push(Line::from(Span::styled(
                format!(
                    "{} selected \u{00b7} {}/{} commands",
                    wiz.history_selected.len(),
                    wiz.history_filtered.len(),
                    wiz.history_entries.len(),
                ),
                Style::default().fg(Color::DarkGray),
            )));

            // Separator
            lines.push(Line::from(Span::styled(
                "\u{2500}".repeat(sep_w),
                Style::default().fg(Color::DarkGray),
            )));

            if wiz.history_entries.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "No shell history found",
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(Span::styled(
                    "Checked: $HISTFILE, ~/.zsh_history, ~/.bash_history",
                    Style::default().fg(Color::DarkGray),
                )));
            } else if wiz.history_filtered.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "No commands match filter",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                // Scrollable command list
                let visible_height = inner.height.saturating_sub(7) as usize; // header + footer
                let end = (wiz.history_scroll_offset + visible_height).min(wiz.history_filtered.len());
                let start = wiz.history_scroll_offset.min(end);
                let max_cmd_width = inner.width.saturating_sub(16) as usize; // checkbox + timestamp margin

                for (vi, &real_idx) in wiz.history_filtered[start..end].iter().enumerate() {
                    let list_pos = start + vi;
                    let is_cursor = list_pos == wiz.history_cursor;
                    let is_selected = wiz.history_selected.contains(&real_idx);

                    let entry = &wiz.history_entries[real_idx];

                    let check = if is_selected { "[x]" } else { "[ ]" };
                    let check_style = if is_selected {
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    let cmd_text = if entry.command.len() > max_cmd_width {
                        format!("{}...", &entry.command[..max_cmd_width.saturating_sub(3)])
                    } else {
                        entry.command.clone()
                    };

                    let cmd_style = if is_cursor {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let mut spans = vec![
                        Span::styled(format!("{} ", check), check_style),
                        Span::styled(cmd_text, cmd_style),
                    ];

                    if let Some(ts) = entry.timestamp {
                        let rel = history::format_relative_time(ts);
                        spans.push(Span::styled(
                            format!(" {}", rel),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }

                    lines.push(Line::from(spans));
                }
            }

            push_wizard_footer(&mut lines, inner.height, &[
                ("Space", "Select"), ("Enter", "Confirm"), ("Up/Down", "Navigate"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::TemplateBrowse => {
            // Filter input
            lines.push(Line::from(vec![
                Span::styled(" Filter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(
                    format!("{}_", wiz.template_filter),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));

            // Count line
            lines.push(Line::from(Span::styled(
                format!(
                    "{}/{} templates",
                    wiz.template_filtered.len(),
                    wiz.template_entries.len(),
                ),
                Style::default().fg(Color::DarkGray),
            )));

            // Separator
            lines.push(Line::from(Span::styled(
                "\u{2500}".repeat(sep_w),
                Style::default().fg(Color::DarkGray),
            )));

            if wiz.template_filtered.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "No templates match filter",
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                let visible_height = inner.height.saturating_sub(7) as usize;
                let end = (wiz.template_scroll_offset + visible_height).min(wiz.template_filtered.len());
                let start = wiz.template_scroll_offset.min(end);
                let max_name_width = inner.width.saturating_sub(6) as usize;

                for (vi, &real_idx) in wiz.template_filtered[start..end].iter().enumerate() {
                    let list_pos = start + vi;
                    let is_cursor = list_pos == wiz.template_cursor;
                    let entry = &wiz.template_entries[real_idx];

                    let ref_name = format!("{}/{}", entry.category, entry.slug);
                    let display = if let Some(ref desc) = entry.description {
                        let max_desc = max_name_width.saturating_sub(ref_name.len() + 3);
                        let short_desc: String = desc.chars().take(max_desc).collect();
                        format!("{} \u{2014} {}", ref_name, short_desc)
                    } else {
                        ref_name
                    };

                    let style = if is_cursor {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let pointer = if is_cursor { "\u{25b8} " } else { "  " };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", pointer, display),
                        style,
                    )));

                    // Show source tag on cursor line
                    if is_cursor {
                        let source_tag = format!("    [{}]", entry.source);
                        let vars_info = if entry.variables.is_empty() {
                            String::new()
                        } else {
                            format!("  {} variable(s)", entry.variables.len())
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", source_tag, vars_info),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            }

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Select"), ("Up/Down", "Navigate"), ("Type", "Filter"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::TemplateVariables => {
            let selected_name = wiz.template_entries.get(wiz.template_cursor)
                .map(|e| e.name.as_str())
                .unwrap_or("Template");
            lines.push(Line::from(Span::styled(
                format!("Configure variables for {}", selected_name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "Edit values, defaults are pre-filled",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            for (i, (name, value, default)) in wiz.template_var_values.iter().enumerate() {
                let is_active = i == wiz.template_var_cursor;

                // Variable description from the template entry
                let var_desc = wiz.template_entries.get(wiz.template_cursor)
                    .and_then(|e| e.variables.get(i))
                    .map(|v| v.description.as_str())
                    .unwrap_or("");

                let pointer = if is_active { "\u{25b8}" } else { " " };
                let label_style = if is_active {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", pointer), label_style),
                    Span::styled(name.as_str(), label_style),
                    Span::styled(
                        if var_desc.is_empty() { String::new() } else { format!(" ({})", var_desc) },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));

                let val_display = if is_active {
                    format!("{}_", value)
                } else {
                    value.clone()
                };

                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(
                        val_display,
                        if is_active {
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Green)
                        },
                    ),
                    if let Some(def) = default {
                        Span::styled(
                            format!("  default: {}", def),
                            Style::default().fg(Color::DarkGray),
                        )
                    } else {
                        Span::raw("")
                    },
                ]));
            }

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Confirm"), ("Up/Down", "Navigate"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::AiRefinePrompt => {
            lines.push(Line::from(Span::styled(
                "Refine your workflow",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "Describe changes (e.g., add error handling, use rsync instead)",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{}_", wiz.ai_refine_prompt),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ]));

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Refine"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::Category => {
            lines.push(Line::from(Span::styled(
                "Choose a category for the new task",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "Type a new name or use arrow keys to select",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled(" Category ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(
                    format!("{}_", wiz.category),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            if !app.categories.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Existing categories:",
                    Style::default().fg(Color::DarkGray),
                )));
                for (i, cat) in app.categories.iter().enumerate() {
                    let is_sel = wiz.category_cursor == Some(i);
                    let style = if is_sel {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let marker = if is_sel { "\u{25b8} " } else { "  " };
                    lines.push(Line::from(Span::styled(
                        format!("  {}{} ({} tasks)", marker, cat.name, cat.tasks.len()),
                        style,
                    )));
                }
            }

            let back_hints = if matches!(wiz.mode, WizardMode::FromHistory | WizardMode::AiChat | WizardMode::AiUpdate | WizardMode::FromTemplate) {
                vec![("Enter", "Confirm"), ("Up/Down", "Select"), ("Shift+Tab", "Back"), ("Esc", "Cancel")]
            } else {
                vec![("Enter", "Confirm"), ("Up/Down", "Select"), ("Esc", "Cancel")]
            };
            push_wizard_footer(&mut lines, inner.height, &back_hints);
        }

        WizardStage::TaskName => {
            lines.push(Line::from(Span::styled(
                "Name the new task",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "This will be the filename (without .yaml extension)",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled(" Category ", Style::default().fg(Color::Black).bg(Color::Green)),
                Span::raw(" "),
                Span::styled(&wiz.category, Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled(" Name ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(
                    format!("{}_", wiz.task_name),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled("  Output  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}/{}.yaml", wiz.category, wiz.task_name),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Confirm"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::Options => {
            lines.push(Line::from(Span::styled(
                "Configure optimizations",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "Toggle options with Space, navigate with arrow keys",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            lines.push(Line::from(vec![
                Span::styled(" Category ", Style::default().fg(Color::Black).bg(Color::Green)),
                Span::styled(format!(" {}  ", wiz.category), Style::default().fg(Color::Green)),
                Span::styled(" Name ", Style::default().fg(Color::Black).bg(Color::Green)),
                Span::styled(format!(" {}", wiz.task_name), Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(""));

            let has_run = wiz.source_run.is_some();
            let opts: &[(&str, &str, bool, bool)] = &[
                ("Remove failed steps", "Drop steps that failed in the last run", wiz.remove_failed, has_run),
                ("Remove skipped steps", "Drop steps that were skipped", wiz.remove_skipped, has_run),
                ("Parallelize steps", "Remove sequential deps between independent steps", wiz.parallelize, true),
            ];

            for (i, (label, desc, checked, enabled)) in opts.iter().enumerate() {
                let is_active = i == wiz.active_toggle;
                let check_icon = if *checked { "\u{25c9}" } else { "\u{25cb}" };

                let label_style = if !enabled {
                    Style::default().fg(Color::DarkGray)
                } else if is_active {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let check_style = if !enabled {
                    Style::default().fg(Color::DarkGray)
                } else if *checked {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let pointer = if is_active { "\u{25b8}" } else { " " };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", pointer), label_style),
                    Span::styled(format!("{} ", check_icon), check_style),
                    Span::styled(*label, label_style),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("      {}{}", desc, if !enabled { " (no run data)" } else { "" }),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            push_wizard_footer(&mut lines, inner.height, &[
                ("Enter", "Next"), ("Space", "Toggle"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
            ]);
        }

        WizardStage::Preview => {
            let is_update = wiz.mode == WizardMode::AiUpdate;
            lines.push(Line::from(Span::styled(
                if is_update { "Review updated workflow" } else { "Review and save" },
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled(
                    if is_update { "Updating  " } else { "Saving to  " },
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{}/{}.yaml", wiz.category, wiz.task_name),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(""));

            // YAML preview — generate based on mode
            let yaml = match wiz.mode {
                WizardMode::AiUpdate => {
                    wiz.ai_updated_yaml.clone().unwrap_or_default()
                }
                WizardMode::FromHistory => {
                    let commands: Vec<String> = wiz
                        .history_selected
                        .iter()
                        .filter_map(|&i| wiz.history_entries.get(i))
                        .map(|e| e.command.clone())
                        .collect();
                    let wf = wizard::workflow_from_commands(&wiz.task_name, &commands);
                    wizard::generate_yaml(&wf)
                }
                WizardMode::AiChat => {
                    if let Some(ref yaml) = wiz.ai_updated_yaml {
                        yaml.clone()
                    } else {
                        let wf = wizard::workflow_from_commands(&wiz.task_name, &wiz.ai_commands);
                        wizard::generate_yaml(&wf)
                    }
                }
                WizardMode::FromTemplate => {
                    let idx = wiz.template_cursor;
                    if let Some(entry) = wiz.template_entries.get(idx) {
                        let mut values = std::collections::HashMap::new();
                        for (name, val, _default) in &wiz.template_var_values {
                            values.insert(name.clone(), val.clone());
                        }
                        catalog::instantiate_template(entry, &values)
                    } else {
                        String::new()
                    }
                }
                WizardMode::CloneTask => {
                    let source_wf = wiz.source_workflow.as_ref().unwrap();
                    let optimized = wizard::optimize_workflow(
                        source_wf,
                        wiz.source_run.as_ref(),
                        wiz.remove_failed,
                        wiz.remove_skipped,
                        wiz.parallelize,
                    );
                    wizard::generate_yaml(&optimized)
                }
            };

            for line in yaml.lines() {
                let owned = line.to_string();
                let color = if line.starts_with("name:") || line.starts_with("env:") || line.starts_with("workdir:") || line.starts_with("steps:") {
                    Color::Cyan
                } else if line.trim_start().starts_with("- id:") {
                    Color::Yellow
                } else if line.trim_start().starts_with("cmd:") {
                    Color::Green
                } else if line.trim_start().starts_with("needs:") || line.trim_start().starts_with("parallel:") {
                    Color::Magenta
                } else {
                    Color::White
                };
                lines.push(Line::from(Span::styled(owned, Style::default().fg(color))));
            }

            if matches!(wiz.mode, WizardMode::AiChat | WizardMode::AiUpdate) {
                push_wizard_footer(&mut lines, inner.height, &[
                    ("Enter", "Save"), ("d", "Dry-run"), ("r", "Refine"), ("Up/Down", "Scroll"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
                ]);
            } else {
                push_wizard_footer(&mut lines, inner.height, &[
                    ("Enter", "Save"), ("d", "Dry-run"), ("Up/Down", "Scroll"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
                ]);
            }
        }
    }

    let scroll = match wiz.stage {
        WizardStage::Preview => wiz.preview_scroll,
        _ => 0,
    };

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(para, inner);
}

fn draw_confirm_delete(f: &mut Frame, app: &App) {
    let state = match app.delete_state.as_ref() {
        Some(s) => s,
        None => return,
    };

    let area = f.area();
    let w = 50.min(area.width.saturating_sub(4));
    let h = 10.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Delete Task ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    f.render_widget(block, popup);

    let inner = Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), popup.height.saturating_sub(2));

    let task_ref = format!("{}/{}", state.category, state.task_name);
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Delete this task? (moved to .trash/)",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Task  ", Style::default().fg(Color::DarkGray)),
            Span::styled(&task_ref, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  File  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                state.task_path.display().to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y/Enter ", Style::default().fg(Color::Black).bg(Color::Red)),
            Span::styled(" Delete  ", Style::default().fg(Color::DarkGray)),
            Span::raw("   "),
            Span::styled(" Any key ", Style::default().fg(Color::Black).bg(Color::White)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

/// Push a bottom-aligned footer with key hints into the lines vec.
fn push_wizard_footer(lines: &mut Vec<Line>, available_height: u16, hints: &[(&str, &str)]) {
    let target = (available_height as usize).saturating_sub(2);
    while lines.len() < target {
        lines.push(Line::from(""));
    }

    // Separator
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(40),
        Style::default().fg(Color::DarkGray),
    )));

    // Key hints
    let mut spans: Vec<Span> = Vec::new();
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", Style::default()));
        }
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(Color::Black).bg(Color::White),
        ));
        spans.push(Span::styled(format!(" {}", action), Style::default().fg(Color::DarkGray)));
    }
    lines.push(Line::from(spans));
}

fn format_live_progress(app: &App) -> String {
    let mut out = String::new();
    if let Some(ref task_ref) = app.executing_task_ref {
        out.push_str(&format!("Running: {}\n\n", task_ref));
    }

    if app.step_states.is_empty() {
        out.push_str("Preparing...\n");
        return out;
    }

    for (i, state) in app.step_states.iter().enumerate() {
        let icon = match state.status {
            StepStatus::Running => "▶",
            StepStatus::Success => "✓",
            StepStatus::Failed => "✗",
            StepStatus::Skipped => "⊘",
            StepStatus::Timedout => "⏱",
            StepStatus::Interactive => "⇄",
            StepStatus::Pending => "·",
        };

        let duration = match state.duration_ms {
            Some(ms) if ms >= 1000 => format!(" ({:.1}s)", ms as f64 / 1000.0),
            Some(ms) => format!(" ({}ms)", ms),
            None if state.status == StepStatus::Running => " ...".to_string(),
            None => String::new(),
        };

        out.push_str(&format!("  {} {}. {}{}\n", icon, i + 1, state.id, duration));

        if !state.cmd_preview.is_empty() && state.status == StepStatus::Running {
            out.push_str(&format!("       $ {}\n", state.cmd_preview));
        }
    }

    out
}

/// Format a timestamp as a short relative time like "2d", "5h", "3m".
fn format_relative_short(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let ts = match ts {
        Some(t) => t,
        None => return String::new(),
    };
    let elapsed = chrono::Utc::now() - ts;
    let mins = elapsed.num_minutes();
    if mins < 1 {
        "now".to_string()
    } else if mins < 60 {
        format!("{}m", mins)
    } else if mins < 1440 {
        format!("{}h", mins / 60)
    } else {
        format!("{}d", mins / 1440)
    }
}

fn format_task_preview(task: &crate::core::models::Task) -> String {
    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path),
        TaskKind::YamlWorkflow => parse_workflow(&task.path),
    };

    match workflow {
        Ok(wf) => {
            let mut out = format!("Workflow: {}\n", wf.name);
            if let Some(ref dir) = wf.workdir {
                out.push_str(&format!("Workdir:  {}\n", dir.display()));
            }
            if !wf.env.is_empty() {
                out.push_str(&format!("Env vars: {}\n", wf.env.len()));
            }
            out.push_str(&format!("Steps:    {}\n\n", wf.steps.len()));

            for (i, step) in wf.steps.iter().enumerate() {
                out.push_str(&format!("  {}. [{}]", i + 1, step.id));
                if !step.needs.is_empty() {
                    out.push_str(&format!(" (needs: {})", step.needs.join(", ")));
                }
                out.push('\n');
                // Show command, truncated per line
                let cmd = if step.cmd.len() > 120 {
                    format!("{}...", &step.cmd[..117])
                } else {
                    step.cmd.clone()
                };
                out.push_str(&format!("     $ {}\n", cmd));
            }
            out
        }
        Err(_) => {
            // Fallback to raw file content
            match std::fs::read_to_string(&task.path) {
                Ok(contents) => {
                    let lines: Vec<&str> = contents.lines().collect();
                    let max = 50.min(lines.len());
                    lines[..max].join("\n")
                }
                Err(e) => format!("Cannot read file: {e}"),
            }
        }
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let (title, border_color) = if app.is_executing {
        (" Running... ", Color::Yellow)
    } else {
        (" Execution Log ", Color::Green)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner_height = area.height.saturating_sub(2) as usize; // borders take 2 lines
    let start = app.footer_log.len().saturating_sub(inner_height);
    let visible_lines: Vec<Line> = app.footer_log[start..]
        .iter()
        .map(|line| {
            let color = if line.contains("✓") || line.contains("Done") {
                Color::Green
            } else if line.contains("✗") || line.contains("Error") || line.contains("FAIL") {
                Color::Red
            } else if line.contains("▶") || line.contains("Starting") {
                Color::Yellow
            } else if line.contains("⊘") || line.contains("skipped") {
                Color::DarkGray
            } else {
                Color::White
            };
            Line::from(Span::styled(line.as_str(), Style::default().fg(color)))
        })
        .collect();

    let para = Paragraph::new(visible_lines).block(block);
    f.render_widget(para, area);
}

fn format_run_log(log: &crate::core::models::RunLog) -> String {
    let mut out = format!(
        "Run: {}\nTask: {}\nStarted: {}\nExit: {}\n\n",
        log.id, log.task_ref, log.started, log.exit_code
    );

    for step in &log.steps {
        let icon = match step.status {
            StepStatus::Success => "[OK]",
            StepStatus::Failed => "[FAIL]",
            StepStatus::Skipped => "[SKIP]",
            StepStatus::Timedout => "[TIMEOUT]",
            StepStatus::Running => "[...]",
            StepStatus::Interactive => "[INTERACTIVE]",
            StepStatus::Pending => "[--]",
        };
        out.push_str(&format!(
            "{} {} ({}ms)\n",
            icon, step.id, step.duration_ms
        ));
        if !step.output.is_empty() {
            out.push_str(&format!("  {}\n", step.output.trim()));
        }
    }

    out
}

fn draw_help(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 50.min(area.width.saturating_sub(4));
    let h = 28.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    let help_text = if app.wizard.is_some() {
        // Wizard-mode help
        vec![
            Line::from(Span::styled(
                " Wizard Keys ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Enter       Confirm / Save"),
            Line::from("  Shift+Tab   Go back a step"),
            Line::from("  Esc         Cancel wizard"),
            Line::from("  Up/Down     Navigate / scroll"),
            Line::from("  Space       Toggle selection"),
            Line::from(""),
            Line::from(Span::styled(
                " Preview Stage ",
                Style::default().fg(Color::Yellow),
            )),
            Line::from("  d           Dry-run preview"),
            Line::from("  r           Refine with AI"),
            Line::from("  Enter       Save task"),
            Line::from(""),
            Line::from(Span::styled(
                "  Press any key to close",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    } else {
        // Normal-mode help
        vec![
            Line::from(Span::styled(
                " workflow ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Up/Down     Navigate items"),
            Line::from("  Left/Right  Switch panes"),
            Line::from("  Tab         Next pane"),
            Line::from("  Enter       Expand/enter category"),
            Line::from("  +           Expand category"),
            Line::from("  -           Collapse category"),
            Line::from("  r           Run selected task"),
            Line::from("  d           Dry-run selected task"),
            Line::from("  e           Edit task in $EDITOR"),
            Line::from("  w           New task from shell history"),
            Line::from("  W           Clone selected task"),
            Line::from("  a           AI task generator"),
            Line::from("  A           AI update selected task"),
            Line::from("  t           New task from template"),
            Line::from("  Del         Delete selected task"),
            Line::from("  c           Compare last 2 runs"),
            Line::from("  F           Filter: All/Failed/Overdue"),
            Line::from("  L           View run logs"),
            Line::from("  R           Recent runs (last 10)"),
            Line::from("  s           Saved/bookmarked tasks"),
            Line::from("  S           Toggle bookmark on task"),
            Line::from("  f           Toggle heat/alpha sort"),
            Line::from("  /           Search tasks"),
            Line::from("  q           Quit / close"),
            Line::from("  Esc         Cancel / dismiss"),
            Line::from(""),
            Line::from(Span::styled(
                "  Press any key to close",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let block = Block::default()
        .title(" Help ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup);
    let para = Paragraph::new(help_text).block(block);
    f.render_widget(para, popup);
}

fn draw_streaming_modal(f: &mut Frame, app: &App) {
    let area = f.area();
    // Use ~80% of the screen for the modal (10% margin on each side)
    let margin_x = (area.width / 10).max(2);
    let margin_y = (area.height / 10).max(1);
    let w = area.width.saturating_sub(margin_x * 2);
    let h = area.height.saturating_sub(margin_y * 2);
    let popup = Rect::new(margin_x, margin_y, w, h);

    let title = if let Some(ref cmd) = app.streaming_cmd {
        let max_len = (w as usize).saturating_sub(20);
        let label = if cmd.len() > max_len {
            format!("{}...", &cmd[..max_len])
        } else {
            cmd.clone()
        };
        format!(" {} ", label)
    } else {
        " Streaming Output ".to_string()
    };

    let status = if app.is_executing {
        Span::styled(" LIVE ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" DONE ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Left)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    // Inner area for content
    let inner = block.inner(popup);
    let content_height = inner.height.saturating_sub(1) as usize; // -1 for status line

    // Build output lines
    let lines: Vec<Line> = app.streaming_lines.iter().map(|l| {
        Line::from(Span::raw(l.as_str()))
    }).collect();

    // Calculate scroll offset
    let total = lines.len();
    let scroll_offset = if app.streaming_auto_scroll {
        total.saturating_sub(content_height)
    } else {
        (app.streaming_scroll as usize).min(total.saturating_sub(content_height))
    };

    // Split inner area: content + status bar
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let para = Paragraph::new(lines)
        .scroll((scroll_offset as u16, 0))
        .wrap(Wrap { trim: false });

    // Status bar
    let line_info = format!(
        " Lines: {} | Scroll: {}/{} ",
        total,
        scroll_offset + 1,
        total.saturating_sub(content_height).max(1),
    );
    let status_line = Line::from(vec![
        status,
        Span::raw("  "),
        Span::styled(line_info, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            "Esc/q: close  ↑↓: scroll  Home/End: top/bottom",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let status_bar = Paragraph::new(status_line);

    f.render_widget(Clear, popup);
    f.render_widget(block, popup);
    f.render_widget(para, inner_chunks[0]);
    f.render_widget(status_bar, inner_chunks[1]);
}

fn format_logs(logs: &[crate::core::models::RunLog]) -> String {
    if logs.is_empty() {
        return "No logs available".to_string();
    }

    let mut out = String::new();
    for log in logs {
        let status = if log.exit_code == 0 { "OK" } else { "FAIL" };
        out.push_str(&format!(
            "[{status}] {} @ {}\n",
            log.task_ref,
            log.started.format("%Y-%m-%d %H:%M:%S")
        ));
    }
    out
}

fn draw_recent_runs(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 65.min(area.width.saturating_sub(4));
    let h = 14.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Recent Runs (last 10) ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if app.recent_runs.is_empty() {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No recent runs found.",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(block);
        f.render_widget(para, popup);
        return;
    }

    let items: Vec<ListItem> = app
        .recent_runs
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let icon = if run.exit_code == 0 { "✓" } else { "✗" };
            let icon_color = if run.exit_code == 0 { Color::Green } else { Color::Red };
            let ts = run.started.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M");
            let duration = run
                .ended
                .map(|e| {
                    let ms = (e - run.started).num_milliseconds();
                    if ms >= 1000 {
                        format!("{:.1}s", ms as f64 / 1000.0)
                    } else {
                        format!("{}ms", ms)
                    }
                })
                .unwrap_or_else(|| "—".to_string());

            let style = if i == app.recent_runs_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(icon_color)),
                Span::styled(
                    format!("{:<30}", run.task_ref),
                    style,
                ),
                Span::styled(format!("{}", ts), Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(format!("{:>8}", duration), Style::default().fg(Color::Cyan)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

fn draw_overdue_reminder(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 65.min(area.width.saturating_sub(4));
    let h = (app.overdue_tasks.len() as u16 + 4).max(6).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" ⚠ Overdue Tasks ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let items: Vec<ListItem> = app
        .overdue_tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.overdue_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(" ! ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<35}", t.task_ref), style),
                Span::styled(
                    format!("{} day(s) overdue", t.overdue_days),
                    Style::default().fg(Color::Red),
                ),
            ]))
        })
        .collect();

    // Reserve last line inside block for hints
    let inner = block.inner(popup);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(block, popup);

    let list = List::new(items);
    f.render_widget(list, layout[0]);

    let hint = Paragraph::new(Line::from(vec![
        Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
        Span::raw(" navigate · "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" jump to task · "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" dismiss"),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(hint, layout[1]);
}

fn draw_variable_prompt(f: &mut Frame, app: &App) {
    let area = f.area();
    let total_vars = app.var_prompt_vars.len();
    let current_var = app.var_prompt_vars.get(app.var_prompt_index);
    let var_name = current_var.map(|v| v.name.as_str()).unwrap_or("?");
    let var_desc = current_var.and_then(|v| v.description.as_deref());

    let w = 60.min(area.width.saturating_sub(4));
    let h = 20.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let title = format!(" Select: {} ({}/{}) ", var_name, app.var_prompt_index + 1, total_vars);
    let border_color = if app.var_prompt_error.is_some() { Color::Red } else { Color::Cyan };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Layout: optional description, list, hints
    let desc_height = if var_desc.is_some() || app.var_prompt_error.is_some() { 1 } else { 0 };
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(desc_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // Description or error line
    if let Some(ref err) = app.var_prompt_error {
        let err_line = Paragraph::new(Line::from(Span::styled(
            format!(" ✗ {}", err),
            Style::default().fg(Color::Red),
        )));
        f.render_widget(err_line, layout[0]);
    } else if let Some(desc) = var_desc {
        let desc_line = Paragraph::new(Line::from(Span::styled(
            format!(" {}", desc),
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(desc_line, layout[0]);
    }

    // Choice list with scroll
    let visible_height = layout[1].height as usize;
    let scroll = if app.var_prompt_cursor >= app.var_prompt_scroll + visible_height {
        app.var_prompt_cursor.saturating_sub(visible_height - 1)
    } else {
        app.var_prompt_scroll
    };

    let items: Vec<ListItem> = app
        .var_prompt_choices
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(i, choice)| {
            let marker = if i == app.var_prompt_cursor { "▸ " } else { "  " };
            let style = if i == app.var_prompt_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(format!("{}{}", marker, choice), style)))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, layout[1]);

    // Hints
    let hint = Paragraph::new(Line::from(vec![
        Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
        Span::raw(" select · "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" confirm · "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" cancel"),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(hint, layout[2]);
}

fn draw_saved_tasks(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 55.min(area.width.saturating_sub(4));
    let bookmark_count = app.config.bookmarks.len();
    let h = (bookmark_count as u16 + 4).max(6).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Saved Tasks ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    if app.config.bookmarks.is_empty() {
        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No bookmarked tasks. Press S on a task to bookmark it.",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(block);
        f.render_widget(para, popup);
        return;
    }

    let items: Vec<ListItem> = app
        .config
        .bookmarks
        .iter()
        .enumerate()
        .map(|(i, task_ref)| {
            // Find the task kind
            let kind_str = app
                .categories
                .iter()
                .flat_map(|c| c.tasks.iter())
                .find(|t| format!("{}/{}", t.category, t.name) == *task_ref)
                .map(|t| match t.kind {
                    TaskKind::ShellScript => "sh",
                    TaskKind::YamlWorkflow => "yaml",
                })
                .unwrap_or("?");

            let style = if i == app.saved_tasks_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(" ★ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{:<35}", task_ref),
                    style,
                ),
                Span::styled(format!("[{}]", kind_str), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::core::models::{Category, StepStatus, Task, TaskHeat, TaskKind};
    use crate::tui::app::{App, AppMode, StepState};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::io::Write as IoWrite;
    use tempfile::TempDir;

    fn render_app(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let area = buf.area;
        let mut text = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = &buf[(x, y)];
                text.push_str(cell.symbol());
            }
            text.push('\n');
        }
        text
    }

    fn make_test_categories(tmp: &TempDir) -> Vec<Category> {
        // Create a YAML workflow file for render tests that need file parsing
        let backup_dir = tmp.path().join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();

        let yaml_path = backup_dir.join("db-full.yaml");
        let yaml_content = r#"name: db-full
workdir: /var/backups
env:
  DB_HOST: localhost
  DB_PORT: "5432"
steps:
  - id: dump
    cmd: pg_dump mydb > dump.sql
  - id: compress
    cmd: gzip dump.sql
    needs: [dump]
"#;
        std::fs::write(&yaml_path, yaml_content).unwrap();

        let sh_path = backup_dir.join("files.sh");
        let mut f = std::fs::File::create(&sh_path).unwrap();
        writeln!(f, "#!/bin/bash\ntar czf /tmp/backup.tar.gz /home").unwrap();

        vec![
            Category {
                name: "backup".into(),
                path: backup_dir.clone(),
                tasks: vec![
                    Task {
                        name: "db-full".into(),
                        kind: TaskKind::YamlWorkflow,
                        path: yaml_path,
                        category: "backup".into(),
                        last_run: None,
                        overdue: None,
                        heat: TaskHeat::Cold,
                    },
                    Task {
                        name: "files".into(),
                        kind: TaskKind::ShellScript,
                        path: sh_path,
                        category: "backup".into(),
                        last_run: None,
                        overdue: None,
                        heat: TaskHeat::Cold,
                    },
                ],
            },
            Category {
                name: "deploy".into(),
                path: tmp.path().join("deploy"),
                tasks: vec![],
            },
        ]
    }

    fn make_render_app(tmp: &TempDir) -> App {
        let categories = make_test_categories(tmp);
        let config = Config {
            workflows_dir: tmp.path().to_path_buf(),
            ..Config::default()
        };
        App::new(categories, config)
    }

    #[test]
    fn test_render_categories_in_sidebar() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("backup"), "sidebar should show 'backup' category");
        assert!(text.contains("deploy"), "sidebar should show 'deploy' category");
        // Task count in parentheses
        assert!(text.contains("(2)"), "sidebar should show task count for backup");
    }

    #[test]
    fn test_render_task_list() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("db-full"), "task list should show 'db-full'");
        assert!(text.contains("[yaml]"), "task list should show [yaml] kind label");
        assert!(text.contains("files"), "task list should show 'files'");
        assert!(text.contains("[sh]"), "task list should show [sh] kind label");
    }

    #[test]
    fn test_render_details_env_vars() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        // Default selection is first task (db-full.yaml) which has 2 env vars
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("Env vars: 2"), "details should show 'Env vars: 2'");
    }

    #[test]
    fn test_render_details_workdir() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("/var/backups"),
            "details should show workdir '/var/backups'"
        );
    }

    #[test]
    fn test_render_details_steps() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("dump"), "details should show step id 'dump'");
        assert!(text.contains("compress"), "details should show step id 'compress'");
        assert!(
            text.contains("pg_dump"),
            "details should show step command 'pg_dump'"
        );
        assert!(
            text.contains("gzip"),
            "details should show step command 'gzip'"
        );
    }

    #[test]
    fn test_render_search_mode() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Search;
        app.search_query = "db".into();

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Search: db"),
            "status bar should show 'Search: db' in search mode"
        );
    }

    #[test]
    fn test_render_help_overlay() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Help;

        let buf = render_app(&app, 120, 32);
        let text = buffer_text(&buf);

        assert!(
            text.contains("workflow"),
            "help overlay should show 'workflow'"
        );
        assert!(
            text.contains("Dry-run"),
            "help overlay should show key binding for dry-run"
        );
        assert!(
            text.contains("Search tasks"),
            "help overlay should show 'Search tasks'"
        );
    }

    #[test]
    fn test_render_running_dry_run() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Running;
        app.is_executing = true;
        app.executing_task_ref = Some("backup/db-full".into());
        app.step_states = vec![
            StepState {
                id: "dump".into(),
                cmd_preview: "[dry-run] pg_dump mydb".into(),
                status: StepStatus::Success,
                duration_ms: Some(0),
            },
            StepState {
                id: "compress".into(),
                cmd_preview: "[dry-run] gzip dump.sql".into(),
                status: StepStatus::Running,
                duration_ms: None,
            },
        ];
        app.footer_log = vec!["[12:00:00] Starting backup/db-full (dry-run)...".into()];

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("[dry-run]"),
            "running details should show [dry-run] prefix in commands"
        );
        assert!(
            text.contains("backup/db-full"),
            "running details should show task ref"
        );
    }

    #[test]
    fn test_render_footer_log() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.footer_log = vec![
            "[12:00:00] Starting backup/db-full...".into(),
            "[12:00:01] ✓ dump (150ms)".into(),
        ];

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Execution Log"),
            "footer should show 'Execution Log' title when not executing"
        );
        // Footer log content is rendered
        assert!(
            text.contains("Starting backup/db-full"),
            "footer should show log entries"
        );
    }
}
