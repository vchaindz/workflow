use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{App, Focus};

pub(super) fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
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
                Span::styled(" n ", Style::default().fg(Color::Black).bg(Color::White)),
                Span::styled(" New workflow", Style::default().fg(Color::DarkGray)),
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
