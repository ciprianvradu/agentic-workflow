//! F8 popup: Permission Queue — shows all terminals waiting for permission approval.

use crate::app::App;
use crate::terminal::{AttentionReason, TerminalStatus};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// State for the permission queue popup.
pub struct PermissionPopup {
    /// Indices into terminal_manager.terminals that need permission.
    pub entries: Vec<usize>,
    /// Cursor position in the entries list.
    pub cursor: usize,
}

impl PermissionPopup {
    /// Build a new permission popup from the current terminal state.
    pub fn new(app: &App) -> Self {
        let entries = if let Some(mgr) = &app.terminal_manager {
            mgr.terminals
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    matches!(
                        t.status,
                        TerminalStatus::NeedsAttention(AttentionReason::PermissionPrompt)
                            | TerminalStatus::NeedsAttention(AttentionReason::Error)
                    )
                })
                .map(|(i, _)| i)
                .collect()
        } else {
            Vec::new()
        };
        PermissionPopup {
            entries,
            cursor: 0,
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, _area: Rect) {
    let popup = match &app.permission_popup {
        Some(p) => p,
        None => return,
    };

    let popup_area = centered_rect(60, 40, frame.area());
    frame.render_widget(Clear, popup_area);

    let title = format!(" Permission Queue ({}) ", popup.entries.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if popup.entries.is_empty() {
        let msg = Paragraph::new("No terminals need permission approval.")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, inner);
        return;
    }

    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    // Split into list area + hints area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(inner);

    // Draw entries
    let mut lines: Vec<Line> = Vec::new();
    for (i, &term_idx) in popup.entries.iter().enumerate() {
        if let Some(term) = mgr.terminals.get(term_idx) {
            let reason = match &term.status {
                TerminalStatus::NeedsAttention(AttentionReason::PermissionPrompt) => "[PERM]",
                TerminalStatus::NeedsAttention(AttentionReason::Error) => "[ERR] ",
                _ => "[???] ",
            };

            let selected = i == popup.cursor;
            let marker = if selected { "\u{25b8} " } else { "  " }; // ▸

            let style = if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), style),
                Span::styled(
                    format!("{} ", term.id),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("[{}] ", term.label),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(reason.to_string(), Style::default().fg(Color::Yellow)),
            ]));
        }
    }

    let list = Paragraph::new(lines);
    frame.render_widget(list, chunks[0]);

    // Hints
    let hints = Line::from(vec![
        Span::styled(
            " a",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("pprove  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "d",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("eny  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "v",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("iew terminal  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Esc",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", Style::default().fg(Color::DarkGray)),
    ]);
    let hints_paragraph = Paragraph::new(hints);
    frame.render_widget(hints_paragraph, chunks[1]);
}

/// Centered rectangle helper (percentage of parent area).
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
