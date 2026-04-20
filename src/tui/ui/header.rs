use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::app::{App, AppMode};
use super::helpers::colorize_output_line;

pub(super) fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let version = env!("CARGO_PKG_VERSION");
    let stats = &app.header_stats;

    let bg_running = app.background_tasks.iter()
        .filter(|t| t.result.is_none() && t.error.is_none()).count();
    let bg_done = app.background_tasks.iter()
        .filter(|t| t.result.is_some() || t.error.is_some()).count();
    let fg_running: usize = if stats.currently_running { 1 } else { 0 };
    let running_count = fg_running + bg_running;

    let running_style = if running_count > 0 {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut spans = vec![
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
        Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
        {
            use crate::core::sync::SyncStatus;
            let (icon, color) = match app.sync_info.as_ref().map(|i| &i.status) {
                Some(SyncStatus::Clean) => ("●", Color::Green),
                Some(SyncStatus::Dirty(_)) => ("◐", Color::Yellow),
                Some(SyncStatus::Ahead(_)) => ("↑", Color::Cyan),
                Some(SyncStatus::Behind(_)) => ("↓", Color::Magenta),
                Some(SyncStatus::Diverged(_, _)) => ("⇅", Color::Red),
                Some(SyncStatus::NoRemote) => ("○", Color::DarkGray),
                Some(SyncStatus::Offline) => ("○", Color::DarkGray),
                Some(SyncStatus::NotInitialized) | None => ("○", Color::DarkGray),
            };
            Span::styled(format!("{icon} sync"), Style::default().fg(color))
        },
    ];

    if bg_running > 0 || bg_done > 0 {
        spans.push(Span::styled("  │  ", Style::default().fg(Color::DarkGray)));
        let mut bg_parts: Vec<Span> = vec![Span::styled("BG: ", Style::default().fg(Color::DarkGray))];
        if bg_running > 0 {
            bg_parts.push(Span::styled(
                format!("{}⏳", bg_running),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ));
        }
        if bg_done > 0 {
            if bg_running > 0 {
                bg_parts.push(Span::styled(" ", Style::default()));
            }
            bg_parts.push(Span::styled(
                format!("{}✓", bg_done),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }
        spans.extend(bg_parts);
    }

    let header = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Rgb(30, 30, 40)));
    f.render_widget(header, area);
}

pub(super) fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::{Block, Borders};

    let bg_running = app.background_tasks.iter()
        .filter(|t| t.result.is_none() && t.error.is_none()).count();
    let (title, border_color) = if app.is_executing {
        (" Running... ".to_string(), Color::Yellow)
    } else if bg_running > 0 {
        (format!(" Execution Log (BG: {bg_running} running) "), Color::Magenta)
    } else {
        (" Execution Log ".to_string(), Color::Green)
    };

    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner_height = area.height.saturating_sub(2) as usize; // borders take 2 lines
    let start = app.footer_log.len().saturating_sub(inner_height);
    let visible_lines: Vec<Line> = app.footer_log[start..]
        .iter()
        .map(|line| {
            let color = if line.contains("★") {
                Color::Magenta
            } else if line.contains("✓") || line.contains("Done") {
                Color::Green
            } else if line.contains("✗") || line.contains("Error") || line.contains("FAIL") {
                Color::Red
            } else if line.contains("▶") || line.contains("Starting") {
                Color::Yellow
            } else if line.contains("⊘") || line.contains("skipped") {
                Color::DarkGray
            } else {
                return Line::from(colorize_output_line(line.as_str()));
            };
            Line::from(Span::styled(line.as_str(), Style::default().fg(color)))
        })
        .collect();

    let para = Paragraph::new(visible_lines).block(block);
    f.render_widget(para, area);
}

pub(super) fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    use super::app::Focus;

    let content = match app.mode {
        AppMode::Search => {
            format!("Search: {}_ | ESC cancel", app.search_query)
        }
        AppMode::Running => "Running... b:background  (output in footer below)".to_string(),
        AppMode::StreamingOutput => "Streaming output — Esc/q to close".to_string(),
        AppMode::Comparing => "a:AI analysis | ESC:back | Up/Down:scroll".to_string(),
        AppMode::Wizard => " New Task Wizard ".to_string(),
        _ => {
            let sort_label = if app.sort_by_heat { "o:α-sort" } else { "o:heat-sort" };
            let filter_label = format!("F:{}", app.status_filter.next().label());
            let bg_done = app.background_tasks.iter()
                .filter(|t| t.result.is_some() || t.error.is_some()).count();

            let has_ai = app.cached_ai_tool.flatten().is_some();
            let has_failed_run = app.run_output.as_ref().map(|r| r.exit_code != 0).unwrap_or(false)
                && app.run_output_task_path.is_some();
            let has_run_output = app.run_output.is_some();

            let mut hints: Vec<&str> = Vec::new();

            // Contextual state hints first (most likely next action)
            if has_failed_run && has_ai {
                hints.push("a:ai-fix");
            }
            if has_run_output {
                hints.push("Esc:clear");
            }

            // Focus-specific hints
            match app.focus {
                Focus::Sidebar => {
                    hints.extend_from_slice(&[
                        "Enter:expand", "m:rename-cat",
                        "n:new",
                    ]);
                }
                Focus::TaskList => {
                    hints.extend_from_slice(&[
                        "r:run", "d:dry-run", "e:edit", "m:rename",
                    ]);
                    if has_ai && has_failed_run {
                        hints.push("a:ai-fix");
                    }
                    if has_ai {
                        hints.push("A:ai-update");
                    }
                    hints.extend_from_slice(&[
                        "c:compare", "W:clone", "L:logs",
                        "n:new",
                    ]);
                }
                Focus::Details => {
                    hints.extend_from_slice(&[
                        "↑↓:scroll", "PgUp/Dn:page",
                        "-:collapse", "+:expand", "Z:all", "{/}:jump",
                    ]);
                    if has_ai && has_failed_run {
                        hints.push("a:ai-fix");
                    }
                    if has_ai {
                        hints.push("A:ai-update");
                    }
                }
            }

            // Category/task management hints (not relevant in Details)
            if app.focus != Focus::Details {
                hints.extend_from_slice(&[
                    sort_label, &filter_label, "K:secrets", "g:sync",
                ]);
            }

            // Global hints always at end
            hints.extend_from_slice(&[
                "R:recent", "s:saved", "/:search", "h:help", "q:quit",
            ]);

            let mut line = hints.join("  ");
            if bg_done > 0 {
                line.push_str(&format!("  B:bg-result({bg_done})"));
            }
            line
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
