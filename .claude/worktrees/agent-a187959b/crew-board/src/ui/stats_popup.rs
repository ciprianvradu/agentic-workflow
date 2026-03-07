//! Stats popup -- per-terminal and global statistics overlay (Ctrl+F6).

use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

/// State for the stats popup.
#[derive(Debug)]
pub struct StatsPopup {
    pub scroll: usize,
}

impl StatsPopup {
    pub fn new() -> Self {
        Self { scroll: 0 }
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = centered_rect(65, 75, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Statistics -- crew-board ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bold = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);
    let dim = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);
    let red_style = Style::default().fg(Color::Red);
    let yellow_style = Style::default().fg(Color::Yellow);
    let red_bold_style = Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();

    // -- Global Stats Section --
    lines.push(Line::from(Span::styled("Global Statistics", bold)));
    lines.push(Line::from(""));

    let global = app.activity_log.global_stats();
    lines.push(stat_line(
        "Total tool calls",
        global.total_tool_calls,
        key_style,
        value_style,
    ));
    lines.push(stat_line(
        "Total errors",
        global.total_errors,
        key_style,
        if global.total_errors > 0 {
            red_style
        } else {
            value_style
        },
    ));
    lines.push(stat_line(
        "Active terminals",
        global.active_terminals,
        key_style,
        value_style,
    ));
    lines.push(stat_line(
        "Activity log events",
        app.activity_log.len(),
        key_style,
        value_style,
    ));
    lines.push(Line::from(""));

    // -- Security Stats Section --
    lines.push(Line::from(Span::styled("Security", bold)));
    lines.push(Line::from(""));

    let sec = &app.rules_engine.stats;
    lines.push(stat_line(
        "Denied",
        sec.denied,
        key_style,
        if sec.denied > 0 { red_style } else { value_style },
    ));
    lines.push(stat_line(
        "Warned",
        sec.warned,
        key_style,
        if sec.warned > 0 {
            yellow_style
        } else {
            value_style
        },
    ));
    lines.push(stat_line(
        "Auto-approved",
        sec.auto_approved,
        key_style,
        value_style,
    ));
    lines.push(stat_line(
        "Human-approved",
        sec.human_approved,
        key_style,
        value_style,
    ));
    lines.push(stat_line(
        "Credential exposures",
        sec.credential_exposures,
        key_style,
        if sec.credential_exposures > 0 {
            red_bold_style
        } else {
            value_style
        },
    ));
    lines.push(Line::from(""));

    // -- Orchestration Stats Section (if active) --
    if let Some(ref orch) = app.orchestration {
        lines.push(Line::from(Span::styled("Orchestration", bold)));
        lines.push(Line::from(""));

        let mode_str = match orch.mode {
            crate::orchestration::OrchestrationMode::Manual => "Manual",
            crate::orchestration::OrchestrationMode::SemiAuto => "Semi-Auto",
            crate::orchestration::OrchestrationMode::FullAuto => "Full-Auto",
        };
        lines.push(stat_line("Mode", mode_str, key_style, value_style));
        lines.push(stat_line("Pending", orch.pending_count(), key_style, value_style));
        lines.push(stat_line("Running", orch.running_count(), key_style, value_style));
        lines.push(stat_line("Completed", orch.completed_count(), key_style, value_style));
        lines.push(stat_line("Failed", orch.failed_count(), key_style,
            if orch.failed_count() > 0 { red_style } else { value_style }));
        lines.push(stat_line("Total cost", format!("${:.2}", orch.total_cost), key_style, value_style));
        lines.push(stat_line("Circuit breaker",
            if orch.circuit_breaker.tripped { "TRIPPED" } else { "OK" },
            key_style,
            if orch.circuit_breaker.tripped { red_bold_style } else { Style::default().fg(Color::Green) }));
        lines.push(Line::from(""));
    }

    // -- Per-Terminal Breakdown --
    lines.push(Line::from(Span::styled("Per-Terminal Breakdown", bold)));
    lines.push(Line::from(""));

    // Collect terminal IDs from terminal manager
    let mut terminal_ids: Vec<String> = Vec::new();
    if let Some(ref mgr) = app.terminal_manager {
        for term in &mgr.terminals {
            terminal_ids.push(term.id.clone());
        }
    }

    if terminal_ids.is_empty() {
        lines.push(Line::from(Span::styled("  (no terminals)", dim)));
    } else {
        for tid in &terminal_ids {
            let short_id = tid.strip_prefix("TASK_").unwrap_or(tid);
            lines.push(Line::from(Span::styled(
                format!("  {} ", short_id),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            if let Some(stats) = app.activity_log.stats_for_terminal(tid) {
                lines.push(Line::from(vec![
                    Span::styled("    Tools: ".to_string(), dim),
                    Span::styled(stats.total_tools.to_string(), value_style),
                    Span::styled("  Errors: ".to_string(), dim),
                    Span::styled(
                        stats.errors.to_string(),
                        if stats.errors > 0 {
                            red_style
                        } else {
                            value_style
                        },
                    ),
                    Span::styled("  Files: ".to_string(), dim),
                    Span::styled(stats.files_touched.len().to_string(), value_style),
                ]));
                if let Some(ref active) = stats.active_tool {
                    lines.push(Line::from(vec![
                        Span::styled("    Active: ".to_string(), dim),
                        Span::styled(active.clone(), Style::default().fg(Color::Green)),
                    ]));
                }
                // Show top files touched (up to 4)
                if !stats.files_touched.is_empty() {
                    let top_files: Vec<&str> = stats
                        .files_touched
                        .iter()
                        .rev()
                        .take(4)
                        .map(|s| s.as_str())
                        .collect();
                    lines.push(Line::from(vec![
                        Span::styled("    Recent: ".to_string(), dim),
                        Span::styled(top_files.join(", "), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            } else {
                lines.push(Line::from(Span::styled("    (no data)".to_string(), dim)));
            }
            lines.push(Line::from(""));
        }
    }

    // -- Footer --
    lines.push(Line::from(Span::styled(
        "  PgUp/PgDn scroll  Esc close",
        dim,
    )));

    let total_lines = lines.len();
    let visible = inner.height as usize;
    let scroll_offset = app.stats_popup.as_ref().map(|p| p.scroll).unwrap_or(0);
    let clamped_offset = scroll_offset.min(total_lines.saturating_sub(visible));

    let paragraph = Paragraph::new(lines).scroll((clamped_offset as u16, 0));
    frame.render_widget(paragraph, inner);

    // Scrollbar
    if total_lines > visible {
        let mut state =
            ScrollbarState::new(total_lines.saturating_sub(visible)).position(clamped_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut state,
        );
    }
}

/// Create a stat line with a label and value, using owned strings.
fn stat_line(
    label: &str,
    value: impl std::fmt::Display,
    label_style: Style,
    value_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<22}", label), label_style),
        Span::styled(format!("{}", value), value_style),
    ])
}

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
