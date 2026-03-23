use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::app::{App, SecretsMode};
use super::helpers::colorize_output_line;

pub(super) fn draw_help(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 50.min(area.width.saturating_sub(4));
    let h = 36.min(area.height.saturating_sub(4));
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
            Line::from("  e           Edit task (in-app editor)"),
            Line::from("  m           Rename task/category"),
            Line::from("  n           New workflow (menu)"),
            Line::from("  W           Clone selected task"),
            Line::from("  a           AI fix failed run"),
            Line::from("  A           AI update selected task"),
            Line::from("  Del         Delete selected task"),
            Line::from("  T           Empty trash"),
            Line::from("  c           Compare last 2 runs"),
            Line::from("  F           Filter: All/Failed/Overdue"),
            Line::from("  L           View run logs"),
            Line::from("  R           Recent runs (last 10)"),
            Line::from("  s           Saved/bookmarked tasks"),
            Line::from("  S           Toggle bookmark on task"),
            Line::from("  K           Secrets manager"),
            Line::from("  g           Git sync"),
            Line::from("  f           Toggle heat/alpha sort"),
            Line::from("  M           Memory view (baselines, anomalies, trends)"),
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

pub(super) fn draw_streaming_modal(f: &mut Frame, app: &App) {
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

    // Build output lines with pattern-based coloring
    let lines: Vec<Line> = app.streaming_lines.iter().map(|l| {
        Line::from(colorize_output_line(l))
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

pub(super) fn draw_recent_runs(f: &mut Frame, app: &App) {
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

pub(super) fn draw_memory_view(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 75.min(area.width.saturating_sub(4));
    let h = 24.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let task_ref = app
        .selected_task_ref()
        .map(|t| format!("{}/{}", t.category, t.name))
        .unwrap_or_default();
    let block = Block::default()
        .title(format!(" Memory: {} ", task_ref))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let tm = app.task_memory_cache.get(&task_ref);
    let mut lines: Vec<Line> = Vec::new();

    if let Some(tm) = tm {
        // Health score
        let health = tm.health_score;
        let bar_len = (health as usize) / 5;
        let bar: String = "\u{2588}".repeat(bar_len);
        let health_color = if health >= 80 { Color::Green } else if health >= 50 { Color::Yellow } else { Color::Red };
        lines.push(Line::from(vec![
            Span::styled(" Health: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}/100 ", health), Style::default().fg(health_color).add_modifier(Modifier::BOLD)),
            Span::styled(bar, Style::default().fg(health_color)),
        ]));
        lines.push(Line::from(""));

        // Baselines
        if !tm.baselines.is_empty() {
            lines.push(Line::from(Span::styled(
                " Baselines",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  step            metric               mean     median    stddev       p95  samples",
                Style::default().fg(Color::DarkGray),
            )));
            for b in &tm.baselines {
                let step_end = if b.step_id.len() > 14 { b.step_id.char_indices().map(|(i,_)|i).take_while(|&i| i<=14).last().unwrap_or(0) } else { b.step_id.len() };
                let step = &b.step_id[..step_end];
                let metric_end = if b.metric_key.len() > 18 { b.metric_key.char_indices().map(|(i,_)|i).take_while(|&i| i<=18).last().unwrap_or(0) } else { b.metric_key.len() };
                let metric = &b.metric_key[..metric_end];
                lines.push(Line::from(Span::raw(format!(
                    "  {:<14}  {:<18} {:>8.1} {:>9.1} {:>9.1} {:>9.1} {:>8}",
                    step, metric, b.mean, b.median, b.stddev, b.p95, b.sample_count
                ))));
            }
            lines.push(Line::from(""));
        }

        // Duration trend
        if !tm.duration_trend.is_empty() {
            lines.push(Line::from(Span::styled(
                " Duration Trend (30d)",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            let vals: Vec<&crate::core::memory::TrendPoint> = tm.duration_trend.iter().rev().take(10).collect::<Vec<_>>().into_iter().rev().collect();
            let trend_str: String = vals
                .iter()
                .map(|t| format!("{:.0}ms", t.mean))
                .collect::<Vec<_>>()
                .join(" \u{2192} ");
            lines.push(Line::from(format!("  {}", trend_str)));
            lines.push(Line::from(""));
        }

        // Recent anomalies
        if !tm.recent_anomalies.is_empty() {
            lines.push(Line::from(Span::styled(
                " Recent Anomalies",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            for a in &tm.recent_anomalies {
                let ts = a.detected.format("%b %d %H:%M");
                let sev_color = match a.severity {
                    crate::core::memory::Severity::Critical => Color::Red,
                    crate::core::memory::Severity::Warning => Color::Yellow,
                    crate::core::memory::Severity::Info => Color::Blue,
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  [{}] ", ts), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("[{}] ", a.severity), Style::default().fg(sev_color)),
                    Span::raw(&a.description),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                " No anomalies detected.",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No memory data yet. Run the task a few times to build baselines.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Esc/q to close | \u{2191}\u{2193} to scroll",
        Style::default().fg(Color::DarkGray),
    )));

    let para = Paragraph::new(lines)
        .block(block)
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, popup);
}

pub(super) fn draw_overdue_reminder(f: &mut Frame, app: &App) {
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

pub(super) fn draw_saved_tasks(f: &mut Frame, app: &App) {
    use crate::core::models::TaskKind;

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

pub(super) fn draw_git_sync(f: &mut Frame, app: &App) {
    use crate::core::sync::SyncStatus;
    use crate::tui::app::SyncSetupStage;

    let area = f.area();
    let w = 55.min(area.width.saturating_sub(4));
    let h = 22.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    let is_repo = app.sync_info.as_ref()
        .map(|i| !matches!(i.status, SyncStatus::NotInitialized))
        .unwrap_or(false);
    let has_remote = app.sync_info.as_ref()
        .and_then(|i| i.remote_url.as_ref())
        .is_some();

    let mut lines: Vec<Line> = Vec::new();

    // Status line
    if let Some(ref info) = app.sync_info {
        let status_str = match &info.status {
            SyncStatus::NotInitialized => "Not initialized".to_string(),
            SyncStatus::Clean => "Clean".to_string(),
            SyncStatus::Dirty(n) => format!("{n} uncommitted change(s)"),
            SyncStatus::Ahead(n) => format!("{n} commit(s) ahead"),
            SyncStatus::Behind(n) => format!("{n} commit(s) behind"),
            SyncStatus::Diverged(a, b) => format!("Diverged ({a} ahead, {b} behind)"),
            SyncStatus::NoRemote => "No remote configured".to_string(),
            SyncStatus::Offline => "Offline".to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
            Span::styled(status_str, Style::default().fg(Color::White)),
        ]));

        if !info.branch.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  Branch: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&info.branch, Style::default().fg(Color::Cyan)),
            ]));
        }
        if let Some(ref url) = info.remote_url {
            let url_end = if url.len() > 40 { url.char_indices().map(|(i,_)|i).take_while(|&i| i<=40).last().unwrap_or(0) } else { url.len() };
            let display_url = &url[..url_end];
            lines.push(Line::from(vec![
                Span::styled("  Remote: ", Style::default().fg(Color::DarkGray)),
                Span::styled(display_url, Style::default().fg(Color::White)),
            ]));
        }
    }

    let auto_label = if app.config.sync.enabled { "on" } else { "off" };
    lines.push(Line::from(vec![
        Span::styled("  Auto-sync: ", Style::default().fg(Color::DarkGray)),
        Span::styled(auto_label, Style::default().fg(if app.config.sync.enabled { Color::Green } else { Color::Red })),
    ]));

    lines.push(Line::from(""));

    // Input mode for URL
    if app.sync_setup_stage == SyncSetupStage::BranchList {
        lines.push(Line::from(Span::styled(
            "  Select branch:",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));
        for (i, branch) in app.branch_list.iter().enumerate() {
            let is_selected = i == app.branch_list_cursor;
            let prefix = if is_selected { " > " } else { "   " };
            let suffix = if branch.is_remote_only { " (remote)" } else { "" };
            let style = if branch.is_current {
                Style::default().fg(Color::Green).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() })
            } else if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{}{suffix}", branch.name),
                style,
            )));
        }
        if app.branch_list.is_empty() {
            lines.push(Line::from(Span::styled(
                "   (no branches found)",
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Enter: switch  Esc: back",
            Style::default().fg(Color::DarkGray),
        )));
    } else if app.sync_setup_stage == SyncSetupStage::RepoUrl {
        lines.push(Line::from(Span::styled(
            "  Enter repository URL:",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(format!("  > {}▏", app.sync_setup_input)));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Enter: confirm  Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Menu items (context-dependent)
        let menu_items: Vec<&str> = if !is_repo {
            vec!["Init git repo", "Clone from URL"]
        } else if !has_remote {
            vec!["Add remote URL", "Create GitHub repo (gh)"]
        } else {
            vec!["Push now", "Pull now", "Refresh status", "Toggle auto-sync", "Switch branch"]
        };

        for (i, item) in menu_items.iter().enumerate() {
            let is_selected = i == app.sync_menu_cursor % menu_items.len();
            let prefix = if is_selected { " ▸ " } else { "   " };
            let style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(format!("{prefix}{item}"), style)));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Enter: select  Esc: close",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Message (success/error)
    if let Some((ref msg, is_error)) = app.sync_message {
        lines.push(Line::from(""));
        let color = if is_error { Color::Red } else { Color::Green };
        lines.push(Line::from(Span::styled(format!("  {msg}"), Style::default().fg(color))));
    }

    // First-run hint
    if app.sync_first_run_hint && !is_repo {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Tip: init a repo to sync workflows across machines",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let block = Block::default()
        .title(" Git Sync ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(Clear, popup);
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, popup);
}

pub(super) fn draw_edit_task(f: &mut Frame, app: &App) {
    let state = match app.edit_state.as_ref() {
        Some(s) => s,
        None => return,
    };

    let area = f.area();
    let margin_x: u16 = 2;
    let margin_y: u16 = 1;
    let w = area.width.saturating_sub(margin_x * 2);
    let h = area.height.saturating_sub(margin_y * 2);
    if w < 10 || h < 5 {
        return;
    }
    let popup = Rect::new(area.x + margin_x, area.y + margin_y, w, h);

    f.render_widget(Clear, popup);

    let border_color = if state.modified { Color::Yellow } else { Color::Cyan };
    let title = if state.modified {
        format!(" {} [modified] ", state.file_name)
    } else {
        format!(" {} ", state.file_name)
    };

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    f.render_widget(block, popup);

    // Inner area (inside border)
    let inner_x = popup.x + 1;
    let inner_y = popup.y + 1;
    let inner_w = popup.width.saturating_sub(2);
    let inner_h = popup.height.saturating_sub(3); // -2 border -1 status bar

    let gutter_width: u16 = 5; // "1234 " format
    let text_width = inner_w.saturating_sub(gutter_width) as usize;
    let text_height = inner_h as usize;

    // Draw lines with line numbers
    let mut lines_to_render: Vec<Line> = Vec::new();
    for vis_row in 0..text_height {
        let line_idx = state.scroll_row + vis_row;
        if line_idx >= state.lines.len() {
            // Empty line below content
            let gutter = Span::styled(
                format!("{:>4} ", "~"),
                Style::default().fg(Color::DarkGray),
            );
            lines_to_render.push(Line::from(vec![gutter]));
            continue;
        }

        let is_cursor_line = line_idx == state.cursor_row;
        let gutter_style = if is_cursor_line {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let gutter = Span::styled(
            format!("{:>4} ", line_idx + 1),
            gutter_style,
        );

        let line_content = &state.lines[line_idx];
        // Apply horizontal scroll
        let visible: String = if state.scroll_col < line_content.len() {
            line_content[state.scroll_col..].chars().take(text_width).collect()
        } else {
            String::new()
        };

        let text_style = if is_cursor_line {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        lines_to_render.push(Line::from(vec![
            gutter,
            Span::styled(visible, text_style),
        ]));
    }

    let text_area = Rect::new(inner_x, inner_y, inner_w, inner_h);
    let para = Paragraph::new(lines_to_render);
    f.render_widget(para, text_area);

    // Status bar
    let status_y = popup.y + popup.height.saturating_sub(2);
    let status_area = Rect::new(inner_x, status_y, inner_w, 1);

    let status_line = if state.confirm_discard {
        Line::from(vec![
            Span::styled(" UNSAVED CHANGES ", Style::default().fg(Color::Black).bg(Color::Yellow)),
            Span::styled(" Discard? ", Style::default().fg(Color::White)),
            Span::styled("y", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("/", Style::default().fg(Color::DarkGray)),
            Span::styled("n", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])
    } else {
        let mod_indicator = if state.modified {
            Span::styled(" MODIFIED ", Style::default().fg(Color::Black).bg(Color::Yellow))
        } else {
            Span::styled(" SAVED ", Style::default().fg(Color::Black).bg(Color::Green))
        };
        let pos = Span::styled(
            format!("  Ln {}, Col {}  ", state.cursor_row + 1, state.cursor_col + 1),
            Style::default().fg(Color::DarkGray),
        );
        Line::from(vec![
            mod_indicator, pos,
            Span::styled("Ctrl+S", Style::default().fg(Color::Cyan)),
            Span::styled(" Save  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::styled(" Close  ", Style::default().fg(Color::DarkGray)),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Cyan)),
            Span::styled(" Scroll", Style::default().fg(Color::DarkGray)),
        ])
    };

    f.render_widget(Paragraph::new(vec![status_line]), status_area);

    // Set cursor position
    let cursor_screen_row = (state.cursor_row - state.scroll_row) as u16;
    let cursor_screen_col = state.cursor_col.saturating_sub(state.scroll_col) as u16;
    let cursor_x = inner_x + gutter_width + cursor_screen_col;
    let cursor_y = inner_y + cursor_screen_row;
    if cursor_x < popup.x + popup.width - 1 && cursor_y < status_y {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

pub(super) fn draw_secrets(f: &mut Frame, app: &App) {
    let state = match app.secrets_state.as_ref() {
        Some(s) => s,
        None => return,
    };

    let area = f.area();
    let w = 60.min(area.width.saturating_sub(4));
    let h = 18.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    match state.mode {
        SecretsMode::NotInitialized => {
            let block = Block::default()
                .title(" Secrets (K) ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Secrets store not initialized",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];

            if let Some(ref err) = state.error {
                lines.push(Line::from(Span::styled(
                    format!("  {err}"),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
            }

            lines.push(Line::from("  Press Enter to initialize with auto-detected SSH key"));
            lines.push(Line::from(Span::styled(
                "  Or run `workflow secrets init` from CLI",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled(" Initialize  ", Style::default().fg(Color::DarkGray)),
                Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" Close", Style::default().fg(Color::DarkGray)),
            ]));

            let para = Paragraph::new(lines).block(block);
            f.render_widget(para, popup);
        }

        SecretsMode::ViewValue => {
            let block = Block::default()
                .title(" Secret Value ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let val = state.revealed_value.as_deref().unwrap_or("(not found)");
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Name:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&state.pending_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Value: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(val, Style::default().fg(Color::Green)),
                ]),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "  Press any key to dismiss",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let para = Paragraph::new(lines).block(block);
            f.render_widget(para, popup);
        }

        SecretsMode::ConfirmDelete => {
            let block = Block::default()
                .title(" Delete Secret ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));

            let name = state.names.get(state.cursor).map(|s| s.as_str()).unwrap_or("?");
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Delete this secret?",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Secret: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(name, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
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

            let para = Paragraph::new(lines).block(block);
            f.render_widget(para, popup);
        }

        SecretsMode::AddName => {
            let block = Block::default()
                .title(" Add Secret - Name ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let mut lines = vec![
                Line::from(""),
                Line::from("  Enter secret name:"),
                Line::from(""),
                Line::from(format!("  > {}_", state.input)),
            ];

            if let Some(ref err) = state.error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("  {err}"),
                    Style::default().fg(Color::Red),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled(" Confirm  ", Style::default().fg(Color::DarkGray)),
                Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
            ]));

            let para = Paragraph::new(lines).block(block);
            f.render_widget(para, popup);
        }

        SecretsMode::AddValue | SecretsMode::EditValue => {
            let title = if state.mode == SecretsMode::AddValue {
                " Add Secret - Value "
            } else {
                " Edit Secret Value "
            };
            let block = Block::default()
                .title(title)
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let masked: String = "*".repeat(state.input.len());
            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Secret: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&state.pending_name, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(""),
                Line::from("  Enter value (masked):"),
                Line::from(format!("  > {}|", masked)),
            ];

            if let Some(ref err) = state.error {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!("  {err}"),
                    Style::default().fg(Color::Red),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::styled(" Save  ", Style::default().fg(Color::DarkGray)),
                Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
            ]));

            let para = Paragraph::new(lines).block(block);
            f.render_widget(para, popup);
        }

        SecretsMode::List => {
            let block = Block::default()
                .title(" Secrets (K) ")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            if let Some(ref err) = state.error {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        format!("  {err}"),
                        Style::default().fg(Color::Red),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Press q to close",
                        Style::default().fg(Color::DarkGray),
                    )),
                ];
                let para = Paragraph::new(lines).block(block);
                f.render_widget(para, popup);
                return;
            }

            if state.names.is_empty() {
                let lines = vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  No secrets stored",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                        Span::styled(" add  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::White)),
                        Span::styled(" close", Style::default().fg(Color::DarkGray)),
                    ]),
                ];
                let para = Paragraph::new(lines).block(block);
                f.render_widget(para, popup);
                return;
            }

            // Split popup into list area and footer
            let inner = Rect::new(popup.x + 1, popup.y + 1, popup.width.saturating_sub(2), popup.height.saturating_sub(2));
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(inner);

            let items: Vec<ListItem> = state
                .names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let style = if i == state.cursor {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let marker = if i == state.cursor { ">" } else { " " };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {marker} "), Style::default().fg(Color::Cyan)),
                        Span::styled(name, style),
                    ]))
                })
                .collect();

            let list = List::new(items).block(block);
            f.render_widget(list, popup);

            // Footer hints
            let footer = Paragraph::new(Line::from(vec![
                Span::styled(" a", Style::default().fg(Color::Cyan)),
                Span::styled(":add ", Style::default().fg(Color::DarkGray)),
                Span::styled("v", Style::default().fg(Color::Cyan)),
                Span::styled(":view ", Style::default().fg(Color::DarkGray)),
                Span::styled("e", Style::default().fg(Color::Cyan)),
                Span::styled(":edit ", Style::default().fg(Color::DarkGray)),
                Span::styled("d", Style::default().fg(Color::Cyan)),
                Span::styled(":delete ", Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Cyan)),
                Span::styled(":close", Style::default().fg(Color::DarkGray)),
            ]));
            f.render_widget(footer, layout[1]);
        }
    }
}

pub(super) fn draw_getting_started(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = 54u16.min(area.width.saturating_sub(4));
    let h = 14u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Welcome to Workflow ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(block, popup);

    let options = ["Add workflow", "Dismiss"];
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "A file-based workflow orchestrator.",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(
            "Organize shell scripts and YAML workflows",
        )),
        Line::from(Span::raw(
            "into categories, run them from the TUI or",
        )),
        Line::from(Span::raw(
            "CLI, and track results.",
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Get started by creating your first workflow:",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
    ];

    for (i, label) in options.iter().enumerate() {
        let (bullet, style) = if i == app.getting_started_cursor {
            (
                Span::styled(" \u{25cf} ", Style::default().fg(Color::Cyan)),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )
        } else {
            (
                Span::styled(" \u{25cb} ", Style::default().fg(Color::DarkGray)),
                Style::default().fg(Color::White),
            )
        };
        lines.push(Line::from(vec![bullet, Span::styled(*label, style)]));
    }

    let content = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(content, layout[0]);

    let hint = Paragraph::new(Line::from(vec![
        Span::styled(" n", Style::default().fg(Color::Cyan)),
        Span::raw(" add workflow \u{b7} "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" dismiss"),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(hint, layout[1]);
}
