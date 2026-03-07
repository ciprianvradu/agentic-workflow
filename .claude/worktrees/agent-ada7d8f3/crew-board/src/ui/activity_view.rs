use crate::app::App;
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Filter bar
            Constraint::Min(5),   // Event table or timeline
        ])
        .split(area);

    draw_filter_bar(frame, app, chunks[0]);
    if app.activity_filter.timeline_mode {
        draw_timeline(frame, app, chunks[1]);
    } else {
        draw_event_table(frame, app, chunks[1]);
    }
}

fn draw_filter_bar(frame: &mut Frame, app: &App, area: Rect) {
    let filter = &app.activity_filter;
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled(
        " Activity ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    // Active filters
    if let Some(ref crew) = filter.crew_filter {
        spans.push(Span::styled(
            format!(" crew:{} ", crew),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(ref evt) = filter.event_filter {
        spans.push(Span::styled(
            format!(" event:{} ", evt),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(ref tool) = filter.tool_filter {
        spans.push(Span::styled(
            format!(" tool:{} ", tool),
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Auto-scroll indicator
    if filter.auto_scroll {
        spans.push(Span::styled(
            " [auto] ",
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Timeline mode indicator
    if filter.timeline_mode {
        spans.push(Span::styled(
            "[timeline] ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Event count
    let total = app.activity_log.len();
    let filtered = app.activity_log.filtered_events(filter).len();
    if total > 0 {
        spans.push(Span::styled(
            format!(" {}/{} events ", filtered, total),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // Key hints
    spans.push(Span::styled(
        "  t:crew e:event f:tool a:scroll g:timeline",
        Style::default().fg(Color::DarkGray),
    ));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn draw_event_table(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::focused_border_style())
        .title(" Events ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 20 {
        return;
    }

    let filtered = app.activity_log.filtered_events(&app.activity_filter);

    if filtered.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No events recorded yet. Waiting for hook events...",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(msg, inner);
        return;
    }

    let now = std::time::Instant::now();
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let header = Row::new(vec!["Age", "Crew", "Event", "Tool", "Status"])
        .style(header_style);

    let visible_rows = inner.height.saturating_sub(1) as usize; // -1 for header
    let total = filtered.len();
    let skip = if app.activity_filter.auto_scroll {
        total.saturating_sub(visible_rows)
    } else {
        app.activity_scroll.min(total.saturating_sub(visible_rows))
    };

    let rows: Vec<Row> = filtered
        .iter()
        .skip(skip)
        .take(visible_rows)
        .map(|e| {
            let age = format_age(now.duration_since(e.timestamp));
            let crew = e.terminal_id.strip_prefix("TASK_").unwrap_or(&e.terminal_id);
            let tool = e.tool_name.as_deref().unwrap_or("-");
            let status = match e.success {
                Some(true) => "ok",
                Some(false) => "FAIL",
                None => "-",
            };
            let style = match e.success {
                Some(false) => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            Row::new(vec![
                age,
                crew.to_string(),
                e.event_type.clone(),
                tool.to_string(),
                status.to_string(),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(6),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1);

    frame.render_widget(table, inner);
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
    // Pad to full width
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

/// Format a duration as a human-readable age string.
fn format_age(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}
