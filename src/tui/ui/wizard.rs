use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::catalog;
use crate::core::history;
use crate::core::models::StepStatus;
use crate::core::wizard;

use super::app::{App, WizardMode, WizardStage};
use super::helpers::{simple_line_diff, DiffLine, format_failed_run_summary};

pub(super) fn draw_wizard(f: &mut Frame, app: &App) {
    let wiz = match app.wizard.as_ref() {
        Some(w) => w,
        None => return,
    };

    let area = f.area();
    // Larger modal for history stage, medium for AI stages, small for pick mode
    let (mw, mh) = if matches!(wiz.stage, WizardStage::PickMode) {
        (40.min(area.width.saturating_sub(4)), 10.min(area.height.saturating_sub(2)))
    } else if matches!(wiz.stage, WizardStage::ShellHistory | WizardStage::TemplateBrowse) {
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

    let title = if wiz.stage == WizardStage::PickMode {
        " New Workflow "
    } else {
        match wiz.mode {
            WizardMode::FromHistory => " New Task from History ",
            WizardMode::CloneTask => " Clone Task ",
            WizardMode::AiChat => " AI Task Generator ",
            WizardMode::AiUpdate => " AI Task Update ",
            WizardMode::FromTemplate => " Template Catalog ",
        }
    };
    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(block, popup);

    let inner = Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), popup.height.saturating_sub(2));

    // PickMode: render selection menu and return early
    if wiz.stage == WizardStage::PickMode {
        let cursor = wiz.pick_mode_cursor;
        let has_ai = app.cached_ai_tool.flatten().is_some();
        let mut options: Vec<(&str, &str)> = vec![("From shell history", "w")];
        if has_ai {
            options.push(("AI generate", "a"));
        }
        options.push(("From template", "t"));

        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            "Select creation method:",
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(""));
        for (i, (label, hint)) in options.iter().enumerate() {
            let selected = i == cursor;
            let bullet = if selected { "●" } else { "○" };
            let (style_bullet, style_label, style_hint) = if selected {
                (
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    Style::default().fg(Color::DarkGray),
                )
            } else {
                (
                    Style::default().fg(Color::DarkGray),
                    Style::default().fg(Color::Gray),
                    Style::default().fg(Color::DarkGray),
                )
            };
            let prefix = if selected { "> " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix, style_bullet),
                Span::styled(format!("{bullet} {label}"), style_label),
                Span::styled(format!("   ({hint})"), style_hint),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter: select  Esc: cancel",
            Style::default().fg(Color::DarkGray),
        )));
        let para = Paragraph::new(lines);
        f.render_widget(para, inner);
        return;
    }

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
        WizardStage::PickMode => unreachable!(), // handled above with early return
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
                // Show failure context if available
                if let Some(ref failed_log) = wiz.failed_run {
                    lines.extend(format_failed_run_summary(failed_log));
                }

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

            // Show failure context header for AI-fix
            if is_update {
                if let Some(ref failed_log) = wiz.failed_run {
                    let failed_names: Vec<String> = failed_log.steps.iter()
                        .filter(|s| s.status == StepStatus::Failed)
                        .map(|s| format!("{} (exit 1)", s.id))
                        .collect();
                    if !failed_names.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("Fixing: {}", failed_names.join(" | ")),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        )));
                        lines.push(Line::from(""));
                    }
                }
            }

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

            // Show diff view or plain YAML based on toggle
            if is_update && wiz.preview_diff_mode && !wiz.ai_source_yaml.is_empty() {
                let diff = simple_line_diff(&wiz.ai_source_yaml, &yaml);
                for dl in &diff {
                    match dl {
                        DiffLine::Same(s) => lines.push(Line::from(Span::styled(
                            format!("  {}", s), Style::default().fg(Color::DarkGray),
                        ))),
                        DiffLine::Added(s) => lines.push(Line::from(Span::styled(
                            format!("+ {}", s), Style::default().fg(Color::Green),
                        ))),
                        DiffLine::Removed(s) => lines.push(Line::from(Span::styled(
                            format!("- {}", s), Style::default().fg(Color::Red),
                        ))),
                    }
                }
            } else {
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
            }

            if matches!(wiz.mode, WizardMode::AiChat | WizardMode::AiUpdate) {
                let diff_hint = if is_update { ("D", "Toggle diff") } else { ("", "") };
                let mut footer: Vec<(&str, &str)> = vec![
                    ("Enter", "Save"), ("d", "Dry-run"), ("r", "Refine"), ("Up/Down", "Scroll"), ("Shift+Tab", "Back"), ("Esc", "Cancel"),
                ];
                if is_update {
                    footer.insert(2, diff_hint);
                }
                push_wizard_footer(&mut lines, inner.height, &footer);
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

pub(super) fn draw_confirm_delete(f: &mut Frame, app: &App) {
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

pub(super) fn draw_rename(f: &mut Frame, app: &App) {
    use crate::tui::app::RenameTarget;

    let state = match app.rename_state.as_ref() {
        Some(s) => s,
        None => return,
    };

    let is_category = state.target == RenameTarget::Category;
    let title = if is_category { " Rename Category " } else { " Rename Task " };

    let area = f.area();
    let w = 50.min(area.width.saturating_sub(4));
    let h = 10.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(block, popup);

    let inner = Rect::new(popup.x + 2, popup.y + 1, popup.width.saturating_sub(4), popup.height.saturating_sub(2));

    let (label, display_name) = if is_category {
        ("  Category  ", state.old_name.clone())
    } else {
        ("  Task  ", format!("{}/{}", state.category, state.old_name))
    };

    let mut name_spans = vec![
        Span::styled("  ", Style::default()),
        Span::styled(&state.new_name, Style::default().fg(Color::Yellow)),
        Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
    ];
    if !state.extension.is_empty() {
        name_spans.push(Span::styled(&state.extension, Style::default().fg(Color::DarkGray)));
    }

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(label, Style::default().fg(Color::DarkGray)),
            Span::styled(display_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "New name:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(name_spans),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Enter ", Style::default().fg(Color::Black).bg(Color::Cyan)),
            Span::styled(" Rename  ", Style::default().fg(Color::DarkGray)),
            Span::raw("   "),
            Span::styled(" Esc ", Style::default().fg(Color::Black).bg(Color::White)),
            Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

/// Push a bottom-aligned footer with key hints into the lines vec.
pub(super) fn push_wizard_footer(lines: &mut Vec<Line>, available_height: u16, hints: &[(&str, &str)]) {
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

pub(super) fn draw_variable_prompt(f: &mut Frame, app: &App) {
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
    let layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(desc_height),
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1),
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

    let items: Vec<ratatui::widgets::ListItem> = app
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
            ratatui::widgets::ListItem::new(Line::from(Span::styled(format!("{}{}", marker, choice), style)))
        })
        .collect();

    let list = ratatui::widgets::List::new(items);
    f.render_widget(list, layout[1]);

    // Hints
    let hint_spans = if app.var_prompt_error.is_some() && app.cached_ai_tool.flatten().is_some() {
        vec![
            Span::styled(" a", Style::default().fg(Color::Yellow)),
            Span::raw(" ai-fix · "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ]
    } else {
        vec![
            Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
            Span::raw(" select · "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" confirm · "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ]
    };
    let hint = Paragraph::new(Line::from(hint_spans))
        .alignment(Alignment::Center);
    f.render_widget(hint, layout[2]);
}
