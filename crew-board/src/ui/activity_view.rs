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
    pub timeline_mode: bool,
}

impl ActivityFilter {
    pub fn new() -> Self {
        Self {
            terminal: None,
            event_type: None,
            tool: None,
            auto_scroll: true,
            timeline_mode: false,
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Filter bar
            Constraint::Min(5),   // Event table or timeline
        ])
        .split(area);

    // Store content rect for mouse scroll (event table area)
    *app.content_rect.borrow_mut() = Some(chunks[1]);

    draw_filter_bar(frame, app, chunks[0]);
    if app.activity_filter.timeline_mode {
        draw_timeline(frame, app, chunks[1]);
    } else {
        draw_event_table(frame, app, chunks[1]);
    }
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

    // Timeline mode indicator
    if filter.timeline_mode {
        spans.push(Span::styled(
            "[timeline] ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }

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
        "  t:crew e:event f:tool a:scroll g:timeline",
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

fn draw_timeline(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::focused_border_style())
        .title(" Timeline ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    let spans = &app.activity_log.completed_spans;
    if spans.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No tool spans recorded yet. Waiting for hook events...",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(msg, inner);
        return;
    }

    // Find all unique terminal IDs (lanes)
    let mut terminal_ids: Vec<String> = Vec::new();
    for span in spans {
        if !terminal_ids.contains(&span.terminal_id) {
            terminal_ids.push(span.terminal_id.clone());
        }
    }

    // Time range: from earliest start to now
    let now = std::time::Instant::now();
    let earliest = spans.iter().map(|s| s.start).min().unwrap_or(now);
    let total_duration = now.duration_since(earliest).as_secs_f64().max(1.0);

    // Available width for the timeline bars (after lane label)
    let label_width = 12usize;
    let bar_width = (inner.width as usize).saturating_sub(label_width + 1);
    if bar_width < 5 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Header with time scale
    let time_header = format_time_scale(total_duration, bar_width);
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<w$}", "", w = label_width),
            Style::default(),
        ),
        Span::styled(time_header, Style::default().fg(Color::DarkGray)),
    ]));

    // One lane per terminal
    let max_lanes = inner.height.saturating_sub(1) as usize;
    for (lane_idx, tid) in terminal_ids.iter().take(max_lanes).enumerate() {
        let short = tid.strip_prefix("TASK_").unwrap_or(tid);
        let label = format!("{:<w$.w$}", short, w = label_width);

        // Build the bar: a char array for the time axis
        let mut bar: Vec<(char, Style)> =
            vec![(' ', Style::default().fg(Color::DarkGray)); bar_width];

        // Fill in spans for this terminal
        for span in spans.iter().filter(|s| s.terminal_id == *tid) {
            let start_offset = span.start.duration_since(earliest).as_secs_f64();
            let end_offset = span.end.duration_since(earliest).as_secs_f64();

            let start_col = ((start_offset / total_duration) * bar_width as f64) as usize;
            let end_col = ((end_offset / total_duration) * bar_width as f64) as usize;
            let end_col = end_col.max(start_col + 1).min(bar_width);

            let color = tool_color(&span.tool_name);
            let style = match span.success {
                Some(false) => Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default().fg(color),
            };

            let block_char = '\u{2588}'; // full block
            for item in bar.iter_mut().take(end_col.min(bar_width)).skip(start_col) {
                *item = (block_char, style);
            }
        }

        // Convert bar to spans
        let mut bar_spans: Vec<Span> = Vec::new();
        bar_spans.push(Span::styled(
            label,
            Style::default().fg(crew_lane_color(lane_idx)),
        ));

        // Group consecutive same-style chars for efficiency
        let mut i = 0;
        while i < bar.len() {
            let (ch, style) = bar[i];
            let mut j = i + 1;
            while j < bar.len() && bar[j].0 == ch && bar[j].1 == style {
                j += 1;
            }
            let s: String = std::iter::repeat_n(ch, j - i).collect();
            bar_spans.push(Span::styled(s, style));
            i = j;
        }

        lines.push(Line::from(bar_spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Format a time scale header for the timeline.
fn format_time_scale(total_secs: f64, width: usize) -> String {
    let mut scale = String::new();
    let marks = 5usize.min(width / 8);
    if marks == 0 {
        return " ".repeat(width);
    }
    let step = width / marks;
    for i in 0..marks {
        let pos = i * step;
        let time_at_pos = (pos as f64 / width as f64) * total_secs;
        let label = if time_at_pos < 60.0 {
            format!("{:.0}s", time_at_pos)
        } else if time_at_pos < 3600.0 {
            format!("{:.0}m", time_at_pos / 60.0)
        } else {
            format!("{:.0}h", time_at_pos / 3600.0)
        };
        let padded = format!("{:<w$}", label, w = step);
        scale.push_str(&padded);
    }
    while scale.len() < width {
        scale.push(' ');
    }
    scale.truncate(width);
    scale
}

/// Map tool names to colors for the timeline.
fn tool_color(tool: &str) -> Color {
    match tool {
        "Edit" | "Write" => Color::Green,
        "Read" | "Glob" | "Grep" => Color::Blue,
        "Bash" => Color::Yellow,
        "Agent" => Color::Magenta,
        "WebSearch" | "WebFetch" => Color::Cyan,
        _ => Color::White,
    }
}

/// Assign lane colors for terminal labels.
fn crew_lane_color(idx: usize) -> Color {
    const COLORS: [Color; 8] = [
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Blue,
        Color::Red,
        Color::White,
        Color::LightCyan,
    ];
    COLORS[idx % COLORS.len()]
}
