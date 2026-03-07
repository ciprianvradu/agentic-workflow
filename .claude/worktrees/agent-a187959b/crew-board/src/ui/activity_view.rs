//! View 6: Activity Feed — real-time hook event stream.

use crate::app::App;
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

/// Activity feed filter state.
#[derive(Debug, Clone, Default)]
pub struct ActivityFilter {
    pub terminal: Option<String>,
    pub event_type: Option<String>,
    pub tool: Option<String>,
    pub auto_scroll: bool,
}

impl ActivityFilter {
    pub fn new() -> Self {
        Self {
            terminal: None,
            event_type: None,
            tool: None,
            auto_scroll: true,
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Filter bar
            Constraint::Min(5),   // Event table
        ])
        .split(area);

    draw_filter_bar(frame, app, chunks[0]);
    draw_event_table(frame, app, chunks[1]);
}

fn draw_filter_bar(frame: &mut Frame, app: &App, area: Rect) {
    let filter = &app.activity_filter;
    let global = app.activity_log.global_stats();

    let mut spans = vec![
        Span::styled(
            " Activity Feed ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    // Filter indicators
    if let Some(ref t) = filter.terminal {
        spans.push(Span::styled(
            format!("crew:{} ", t),
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(ref e) = filter.event_type {
        spans.push(Span::styled(
            format!("event:{} ", e),
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(ref f) = filter.tool {
        spans.push(Span::styled(
            format!("tool:{} ", f),
            Style::default().fg(Color::Yellow),
        ));
    }

    // Auto-scroll indicator
    let scroll_indicator = if filter.auto_scroll {
        "auto"
    } else {
        "manual"
    };
    spans.push(Span::styled(
        format!("[{}] ", scroll_indicator),
        Style::default().fg(if filter.auto_scroll {
            Color::Green
        } else {
            Color::DarkGray
        }),
    ));

    // Global stats
    spans.push(Span::styled(
        format!(
            "T{} E{} \u{25ce}{}active",
            global.total_tool_calls, global.total_errors, global.active_terminals
        ),
        Style::default().fg(Color::DarkGray),
    ));

    // Key hints
    spans.push(Span::styled(
        "  t:crew e:event f:tool a:scroll",
        Style::default().fg(Color::DarkGray),
    ));

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_event_table(frame: &mut Frame, app: &App, area: Rect) {
    let filter = &app.activity_filter;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::focused_border_style())
        .title(" Events ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 {
        return;
    }

    // Header line
    let header = Line::from(vec![
        Span::styled(
            format!("{:<8}", "Time"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<12}", "Crew"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<14}", "Event"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<10}", "Tool"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Detail".to_string(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    // Filter events
    let filtered = app.activity_log.filter(
        filter.terminal.as_deref(),
        filter.event_type.as_deref(),
        filter.tool.as_deref(),
    );

    let visible_rows = inner.height.saturating_sub(1) as usize; // -1 for header
    let total = filtered.len();

    // Auto-scroll: show last N events. Manual: use activity_scroll offset.
    let start = if filter.auto_scroll {
        total.saturating_sub(visible_rows)
    } else {
        app.activity_scroll
            .min(total.saturating_sub(visible_rows))
    };
    let end = (start + visible_rows).min(total);

    let mut lines = vec![header];

    for event in filtered.iter().skip(start).take(end - start) {
        let elapsed = event.timestamp.elapsed().as_secs();
        let time_str = format_elapsed_short(elapsed);

        let short_terminal = event
            .terminal_id
            .strip_prefix("TASK_")
            .unwrap_or(&event.terminal_id);

        let tool = event.tool_name.as_deref().unwrap_or("");
        let detail = event.tool_input_summary.as_deref().unwrap_or("");

        let (event_style, success_marker) = match event.event_type.as_str() {
            "PreToolUse" => (Style::default().fg(Color::Cyan), ""),
            "PostToolUse" => {
                let marker = match event.success {
                    Some(true) => "\u{2713}",  // checkmark
                    Some(false) => "\u{2717}", // X mark
                    None => "",
                };
                (Style::default().fg(Color::Green), marker)
            }
            "SessionStart" => (Style::default().fg(Color::Blue), "\u{25b6}"),
            "SessionEnd" => (Style::default().fg(Color::DarkGray), "\u{25a0}"),
            "Notification" => (
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "\u{25c6}",
            ),
            "PermissionRequest" => (Style::default().fg(Color::Red), "\u{26a0}"),
            "Stop" => (Style::default().fg(Color::Magenta), "\u{25a0}"),
            _ => (Style::default(), ""),
        };

        let detail_width = inner.width.saturating_sub(44) as usize;
        let detail_truncated = if detail.len() > detail_width {
            format!("{}..", &detail[..detail_width.saturating_sub(2)])
        } else {
            detail.to_string()
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<8}", time_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<12}", short_terminal),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{}{:<13}", success_marker, &event.event_type),
                event_style,
            ),
            Span::styled(format!("{:<10}", tool), Style::default().fg(Color::Cyan)),
            Span::styled(detail_truncated, Style::default().fg(Color::DarkGray)),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Scrollbar
    if total > visible_rows {
        let mut scrollbar_state =
            ScrollbarState::new(total.saturating_sub(visible_rows)).position(start);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut scrollbar_state,
        );
    }
}

fn format_elapsed_short(secs: u64) -> String {
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}
