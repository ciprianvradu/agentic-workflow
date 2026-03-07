//! F8 popup: Permission Queue — shows all terminals waiting for permission approval.
//!
//! Shows two types of entries:
//! - PTY-based: terminals with NeedsAttention(PermissionPrompt/Error) from screen scanning
//! - Hook-based: structured permission requests from Claude Code hooks (PendingPermission)

use crate::app::App;
use crate::terminal::{AttentionReason, TerminalStatus};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// An entry in the permission queue popup.
#[derive(Debug, Clone)]
pub enum PermissionEntry {
    /// PTY-based: terminal index in terminal_manager.terminals.
    PtyBased { terminal_idx: usize },
    /// Hook-based: index into app.pending_permissions.
    HookBased { pending_idx: usize },
}

/// State for the permission queue popup.
pub struct PermissionPopup {
    /// All entries (PTY-based + hook-based).
    pub entries: Vec<PermissionEntry>,
    /// Cursor position in the entries list.
    pub cursor: usize,
    /// Quick-send input mode (only used for PTY-based entries).
    pub quick_send_input: Option<tui_input::Input>,
}

impl PermissionPopup {
    /// Build a new permission popup from the current terminal state and pending hook permissions.
    pub fn new(app: &App) -> Self {
        let mut entries: Vec<PermissionEntry> = Vec::new();

        // Add PTY-based entries
        if let Some(mgr) = &app.terminal_manager {
            for (i, t) in mgr.terminals.iter().enumerate() {
                if matches!(
                    t.status,
                    TerminalStatus::NeedsAttention(AttentionReason::PermissionPrompt { .. })
                        | TerminalStatus::NeedsAttention(AttentionReason::Error { .. })
                ) {
                    entries.push(PermissionEntry::PtyBased { terminal_idx: i });
                }
            }
        }

        // Add hook-based entries
        for (i, _) in app.pending_permissions.iter().enumerate() {
            entries.push(PermissionEntry::HookBased { pending_idx: i });
        }

        PermissionPopup {
            entries,
            cursor: 0,
            quick_send_input: None,
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, _area: Rect) {
    let popup = match &app.permission_popup {
        Some(p) => p,
        None => return,
    };

    let popup_area = centered_rect(72, 55, frame.area());
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

    // Split into list area + context area + input/hints area
    let has_quick_send = popup.quick_send_input.is_some();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // entry list
            Constraint::Length(4), // context/detail preview
            Constraint::Length(if has_quick_send { 3 } else { 2 }), // hints or input
        ])
        .split(inner);

    // Draw entries
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in popup.entries.iter().enumerate() {
        let selected = i == popup.cursor;
        let marker = if selected { "\u{25b8} " } else { "  " }; // ▸

        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        match entry {
            PermissionEntry::PtyBased { terminal_idx } => {
                if let Some(term) = mgr.terminals.get(*terminal_idx) {
                    let reason_tag = match &term.status {
                        TerminalStatus::NeedsAttention(
                            AttentionReason::PermissionPrompt { .. },
                        ) => "[PERM]",
                        TerminalStatus::NeedsAttention(AttentionReason::Error { .. }) => {
                            "[ERR] "
                        }
                        _ => "[???] ",
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
                        Span::styled(reason_tag.to_string(), Style::default().fg(Color::Yellow)),
                        Span::styled(" PTY", Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            PermissionEntry::HookBased { pending_idx } => {
                if let Some(pending) = app.pending_permissions.get(*pending_idx) {
                    let event_tag = if pending.event_type == "PermissionRequest" {
                        "[HOOK]"
                    } else {
                        "[PRE] "
                    };

                    lines.push(Line::from(vec![
                        Span::styled(marker.to_string(), style),
                        Span::styled(
                            format!("{} ", pending.terminal_id),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            format!("{} ", pending.tool_name),
                            Style::default().fg(Color::Green),
                        ),
                        Span::styled(event_tag.to_string(), Style::default().fg(Color::Magenta)),
                    ]));
                }
            }
        }
    }

    let list = Paragraph::new(lines);
    frame.render_widget(list, chunks[0]);

    // Context/detail preview for selected entry
    let context_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let context_inner = context_block.inner(chunks[1]);
    frame.render_widget(context_block, chunks[1]);

    if let Some(entry) = popup.entries.get(popup.cursor) {
        match entry {
            PermissionEntry::PtyBased { terminal_idx } => {
                if let Some(term) = mgr.terminals.get(*terminal_idx) {
                    let context = match &term.status {
                        TerminalStatus::NeedsAttention(AttentionReason::PermissionPrompt {
                            context,
                        }) => context.clone(),
                        TerminalStatus::NeedsAttention(AttentionReason::Error { context }) => {
                            context.clone()
                        }
                        _ => String::new(),
                    };
                    if !context.is_empty() {
                        let truncated =
                            if context.len() > (chunks[1].width as usize).saturating_sub(2) {
                                format!(
                                    "{}...",
                                    &context[..context
                                        .len()
                                        .min((chunks[1].width as usize).saturating_sub(5))]
                                )
                            } else {
                                context
                            };
                        let para = Paragraph::new(Line::from(Span::styled(
                            truncated,
                            Style::default().fg(Color::White),
                        )))
                        .wrap(Wrap { trim: true });
                        frame.render_widget(para, context_inner);
                    }
                }
            }
            PermissionEntry::HookBased { pending_idx } => {
                if let Some(pending) = app.pending_permissions.get(*pending_idx) {
                    // Show structured tool info
                    let mut detail_lines: Vec<Line> = Vec::new();

                    // Tool + summary
                    let summary_line = if pending.tool_input_summary.is_empty() {
                        Line::from(vec![
                            Span::styled("Tool: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                pending.tool_name.clone(),
                                Style::default().fg(Color::Green),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled("Tool: ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                format!("{} ", pending.tool_name),
                                Style::default().fg(Color::Green),
                            ),
                            Span::styled(
                                pending.tool_input_summary.clone(),
                                Style::default().fg(Color::White),
                            ),
                        ])
                    };
                    detail_lines.push(summary_line);

                    // Show first key=value pair from tool_input if available
                    if let Some(ref input) = pending.tool_input {
                        if let Some(obj) = input.as_object() {
                            let width = context_inner.width as usize;
                            for (k, v) in obj.iter().take(1) {
                                let val_str = match v {
                                    serde_json::Value::String(s) => {
                                        let max = width.saturating_sub(k.len() + 4);
                                        if s.len() > max && max > 3 {
                                            format!("{}...", &s[..max.saturating_sub(3)])
                                        } else {
                                            s.clone()
                                        }
                                    }
                                    other => {
                                        let s = other.to_string();
                                        let max = width.saturating_sub(k.len() + 4);
                                        if s.len() > max && max > 3 {
                                            format!("{}...", &s[..max.saturating_sub(3)])
                                        } else {
                                            s
                                        }
                                    }
                                };
                                detail_lines.push(Line::from(vec![
                                    Span::styled(
                                        format!("{}: ", k),
                                        Style::default().fg(Color::DarkGray),
                                    ),
                                    Span::styled(val_str, Style::default().fg(Color::White)),
                                ]));
                            }
                        }
                    }

                    let para = Paragraph::new(detail_lines).wrap(Wrap { trim: false });
                    frame.render_widget(para, context_inner);
                }
            }
        }
    }

    // Quick-send input or hints
    if let Some(input) = &popup.quick_send_input {
        let input_line = Line::from(vec![
            Span::styled(
                " Send: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(input.value().to_string(), Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::DarkGray)),
        ]);
        let input_hints = Line::from(Span::styled(
            "  Enter: send  |  Esc: cancel",
            Style::default().fg(Color::DarkGray),
        ));
        let para = Paragraph::new(vec![input_line, input_hints]);
        frame.render_widget(para, chunks[2]);
    } else {
        let hints = Line::from(vec![
            Span::styled(
                " a",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ppr ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "d",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("eny ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "A",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ll ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "t",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("ype ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "v",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("iew ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" close", Style::default().fg(Color::DarkGray)),
        ]);
        let hints_paragraph = Paragraph::new(hints);
        frame.render_widget(hints_paragraph, chunks[2]);
    }
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
