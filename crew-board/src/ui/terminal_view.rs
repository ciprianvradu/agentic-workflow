//! View 5: Terminals — embedded terminal multiplexer view.
//!
//! Left panel: list of terminals with status icons.
//! Right panel: rendered output of the focused terminal (or tiled layout).

use crate::app::{App, TerminalInputMode, TerminalLayout};
use crate::terminal::{self, widget, EmbeddedTerminal, TerminalManager, TerminalStatus};
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    // Clear panel rects at the start of each draw
    app.terminal_panel_rects.borrow_mut().clear();

    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => {
            let block = Block::default()
                .title(" Terminals ")
                .borders(Borders::ALL)
                .border_style(styles::unfocused_border_style());
            let msg = Paragraph::new("Terminal manager not initialized.")
                .block(block)
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(msg, area);
            return;
        }
    };

    if mgr.terminals.is_empty() {
        let block = Block::default()
            .title(" Terminals ")
            .borders(Borders::ALL)
            .border_style(styles::unfocused_border_style());
        let msg = Paragraph::new("No embedded terminals. Use F2 to launch a task terminal.")
            .block(block)
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, area);
        return;
    }

    // Calculate crew list width based on layout mode and configurable base width
    let base_width = app.pane_width_terminals as u16;
    let list_width = match app.terminal_layout {
        TerminalLayout::Focused => base_width,
        TerminalLayout::Tiled2 => base_width.saturating_sub(2),
        TerminalLayout::Tiled4 => base_width.saturating_sub(5),
        TerminalLayout::Stacked => base_width,
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(list_width),
            Constraint::Min(40),
        ])
        .split(area);

    // Store pane rects for mouse click-to-focus
    *app.pane_rects.borrow_mut() = Some((chunks[0], chunks[1]));

    draw_terminal_list(frame, app, chunks[0]);

    match app.terminal_layout {
        TerminalLayout::Focused => draw_focused_layout(frame, app, chunks[1]),
        TerminalLayout::Tiled2 => draw_tiled2_layout(frame, app, chunks[1]),
        TerminalLayout::Tiled4 => draw_tiled4_layout(frame, app, chunks[1]),
        TerminalLayout::Stacked => draw_stacked_layout(frame, app, chunks[1]),
    }
}

/// Draw the left panel: list of terminals with status icons.
fn draw_terminal_list(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    let is_focused = app.terminal_input_mode == TerminalInputMode::Normal;

    let border_style = if is_focused {
        styles::focused_border_style()
    } else {
        styles::unfocused_border_style()
    };

    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };

    let block = Block::default()
        .title(" Crew List ")
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    let mut line_map: Vec<usize> = Vec::new(); // maps visual line -> terminal index

    for (i, term) in mgr.terminals.iter().enumerate() {
        let icon = match &term.status {
            TerminalStatus::Running => Span::styled(
                "\u{25cf} ", // ●
                Style::default().fg(Color::Green),
            ),
            TerminalStatus::NeedsAttention(reason) => {
                let (icon, color) = match reason {
                    terminal::AttentionReason::PermissionPrompt { .. } => ("\u{25c6} ", Color::Cyan), // ◆ prompt
                    terminal::AttentionReason::Idle { .. } => ("\u{25cb} ", Color::DarkGray),  // ○ idle
                    terminal::AttentionReason::Error { .. } => ("\u{2716} ", Color::Red),             // ✖ error
                    terminal::AttentionReason::HookNotification { .. } => ("\u{25c6} ", Color::Yellow), // ◆ notification
                    terminal::AttentionReason::WaitingForInput => ("\u{25b6} ", Color::Green), // ▶ waiting
                };
                Span::styled(
                    icon,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
            }
            TerminalStatus::Exited(code) => {
                let color = if *code == 0 {
                    Color::DarkGray
                } else {
                    Color::Red
                };
                Span::styled(
                    "\u{2717} ", // ✗
                    Style::default().fg(color),
                )
            }
        };

        // Show label (truncated) + elapsed time
        let max_label = inner.width.saturating_sub(4) as usize;
        let elapsed = term.spawned_at.elapsed().as_secs();
        let elapsed_str = format_elapsed(elapsed);

        // Show short task number as #NNN (strip "TASK_" prefix), fall back to id.
        // If id is a full path, extract the last component first.
        let base_id = if term.id.contains('/') || term.id.contains('\\') {
            std::path::Path::new(&term.id)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&term.id)
        } else {
            &term.id
        };
        let short_num = base_id.strip_prefix("TASK_").unwrap_or(base_id);
        // Extract repo initials from cwd (e.g. "agentic-workflow" → "AW")
        let repo_initials: String = term
            .launch_params
            .cwd
            .ancestors()
            .filter_map(|p| p.file_name())
            .find(|n| {
                let s = n.to_string_lossy();
                !s.starts_with("TASK_") && !s.ends_with("-worktrees")
            })
            .map(|n| {
                n.to_string_lossy()
                    .split(['-', '_', '.'])
                    .filter_map(|w| w.chars().next())
                    .map(|c| c.to_ascii_uppercase())
                    .collect()
            })
            .unwrap_or_else(|| "?".to_string());
        let display_name = format!("{}{}", repo_initials, short_num);

        // Truncate to fit
        let avail = max_label.saturating_sub(elapsed_str.len() + 1);
        let label = if display_name.len() > avail {
            format!("{}...", &display_name[..avail.saturating_sub(3)])
        } else {
            display_name
        };

        let style = if i == mgr.focused {
            styles::selected_style()
        } else if let Some(idx) = term.color_scheme_index {
            Style::default().fg(styles::get_scheme(idx).tab)
        } else {
            Style::default()
        };

        let time_style = Style::default().fg(Color::DarkGray);

        let auto_badge = if term.auto_accept {
            Span::styled("\u{26a1}", Style::default().fg(Color::Yellow)) // ⚡
        } else {
            Span::raw("")
        };
        let headless_badge = if term.is_headless() {
            Span::styled("[H]", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        };
        lines.push(Line::from(vec![
            icon,
            auto_badge,
            headless_badge,
            Span::styled(label, style),
            Span::styled(format!(" {}", elapsed_str), time_style),
        ]));
        line_map.push(i);

        // Second line: hook activity (if hook_state is Some with data)
        if let Some(hook_state) = &term.hook_state {
            let activity_style = Style::default().fg(Color::DarkGray);
            if !hook_state.activity_label.is_empty() {
                // Priority 1: active tool label
                let avail_width = inner.width.saturating_sub(2) as usize;
                let label_text = if hook_state.activity_label.len() > avail_width {
                    if avail_width > 2 {
                        format!("{}..", &hook_state.activity_label[..avail_width - 2])
                    } else {
                        hook_state.activity_label[..avail_width].to_string()
                    }
                } else {
                    hook_state.activity_label.clone()
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(label_text, activity_style),
                ]));
                line_map.push(i);
            } else if !hook_state.tool_counts.is_empty() {
                // Priority 2: abbreviated tool counts, sorted by count desc, top 3
                let mut counts: Vec<(&String, &u32)> = hook_state.tool_counts.iter().collect();
                counts.sort_by(|a, b| b.1.cmp(a.1));
                let summary: String = counts
                    .iter()
                    .take(3)
                    .map(|(name, count)| {
                        let first = name.chars().next().unwrap_or('?');
                        format!("{}:{}", first, count)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let avail_width = inner.width.saturating_sub(2) as usize;
                let summary = if summary.len() > avail_width {
                    summary[..avail_width].to_string()
                } else {
                    summary
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(summary, activity_style),
                ]));
                line_map.push(i);
            }
            // Priority 3: no hook data — omit second line (do nothing)
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);

    // Store rects and line map for mouse click-to-select
    *app.list_inner_rect.borrow_mut() = Some(inner);
    *app.terminal_list_line_map.borrow_mut() = line_map;
}

/// Focused layout: one large terminal panel with crew summary line.
fn draw_focused_layout(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    // Show a 1-line crew summary when there are multiple terminals
    let (summary_area, terminal_area) = if mgr.terminals.len() > 1 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(area);
        draw_crew_summary_line(frame, mgr, chunks[0]);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };
    let _ = summary_area;

    if let Some(term) = mgr.focused_terminal() {
        let is_focused = app.terminal_input_mode == TerminalInputMode::TerminalFocused
            || app.terminal_input_mode == TerminalInputMode::PrefixPending;
        let is_scrollback = app.terminal_input_mode == TerminalInputMode::ScrollBack;

        draw_single_terminal(frame, app, terminal_area, term, is_focused || is_scrollback, mgr.focused);
    }
}

/// Draw a one-line crew summary showing all terminals' status.
fn draw_crew_summary_line(frame: &mut Frame, mgr: &TerminalManager, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" crew: ", Style::default().fg(Color::DarkGray)));

    for (i, term) in mgr.terminals.iter().enumerate() {
        let is_focused = i == mgr.focused;
        let base_id = if term.id.contains('/') || term.id.contains('\\') {
            std::path::Path::new(&term.id)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&term.id)
        } else {
            &term.id
        };
        let short_id = base_id.strip_prefix("TASK_").map_or_else(
            || base_id.to_string(),
            |num| format!("#{}", num),
        );

        let (icon, icon_color) = match &term.status {
            TerminalStatus::Running => ("\u{25cf}", Color::Green),           // ●
            TerminalStatus::NeedsAttention(terminal::AttentionReason::WaitingForInput) => ("\u{25b6}", Color::Green), // ▶
            TerminalStatus::NeedsAttention(_) => ("\u{25c6}", Color::Yellow), // ◆
            TerminalStatus::Exited(code) => {
                if *code == 0 {
                    ("\u{2713}", Color::DarkGray) // ✓
                } else {
                    ("\u{2717}", Color::Red) // ✗
                }
            }
        };

        let label_style = if is_focused {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // Use lightning bolt icon when auto-accept is ON and terminal is running
        if term.auto_accept && matches!(term.status, TerminalStatus::Running) {
            spans.push(Span::styled("\u{26a1}", Style::default().fg(Color::Yellow))); // ⚡
        } else {
            spans.push(Span::styled(icon.to_string(), Style::default().fg(icon_color)));
        }
        // Show [H] indicator for headless terminals
        if term.is_headless() {
            spans.push(Span::styled("[H]", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(short_id, label_style));
        spans.push(Span::styled(" ", Style::default()));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Tiled-2 layout: two terminals side by side.
fn draw_tiled2_layout(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    let indices: Vec<usize> = terminal_indices_for_layout(mgr, 2);

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(split_constraints(indices.len(), Direction::Horizontal))
        .split(area);

    for (chunk_idx, &term_idx) in indices.iter().enumerate() {
        if chunk_idx < chunks.len() {
            let term = &mgr.terminals[term_idx];
            let is_active = term_idx == mgr.focused;
            draw_single_terminal(frame, app, chunks[chunk_idx], term, is_active, term_idx);
        }
    }
}

/// Tiled-4 layout: four terminals in a 2x2 grid.
fn draw_tiled4_layout(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    let indices: Vec<usize> = terminal_indices_for_layout(mgr, 4);

    // Split into top and bottom rows
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top_count = indices.len().div_ceil(2);
    let top_indices = &indices[..top_count.min(indices.len())];
    let bottom_indices = if indices.len() > top_count {
        &indices[top_count..]
    } else {
        &[]
    };

    // Top row
    if !top_indices.is_empty() {
        let top_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(split_constraints(top_indices.len(), Direction::Horizontal))
            .split(rows[0]);
        for (i, &idx) in top_indices.iter().enumerate() {
            if i < top_chunks.len() {
                let term = &mgr.terminals[idx];
                let is_active = idx == mgr.focused;
                draw_single_terminal(frame, app, top_chunks[i], term, is_active, idx);
            }
        }
    }

    // Bottom row
    if !bottom_indices.is_empty() {
        let bot_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(split_constraints(bottom_indices.len(), Direction::Horizontal))
            .split(rows[1]);
        for (i, &idx) in bottom_indices.iter().enumerate() {
            if i < bot_chunks.len() {
                let term = &mgr.terminals[idx];
                let is_active = idx == mgr.focused;
                draw_single_terminal(frame, app, bot_chunks[i], term, is_active, idx);
            }
        }
    }
}

/// Stacked layout: terminals stacked vertically.
fn draw_stacked_layout(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    let max_stacked = 5;
    let indices: Vec<usize> = terminal_indices_for_layout(mgr, max_stacked);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(split_constraints(indices.len(), Direction::Vertical))
        .split(area);

    for (chunk_idx, &term_idx) in indices.iter().enumerate() {
        if chunk_idx < chunks.len() {
            let term = &mgr.terminals[term_idx];
            let is_active = term_idx == mgr.focused;
            draw_single_terminal(frame, app, chunks[chunk_idx], term, is_active, term_idx);
        }
    }
}

/// Pick which terminal indices to show in tiled/stacked layouts.
/// Always includes the focused terminal. Fills remaining slots with neighbors.
pub(crate) fn terminal_indices_for_layout(mgr: &terminal::TerminalManager, max: usize) -> Vec<usize> {
    let total = mgr.terminals.len();
    if total == 0 {
        return vec![];
    }
    let count = total.min(max);

    // Start from focused, show count terminals wrapping around
    let mut indices = Vec::with_capacity(count);
    let start = if mgr.focused + count <= total {
        mgr.focused
    } else {
        total.saturating_sub(count)
    };
    for i in start..start + count {
        indices.push(i.min(total - 1));
    }
    indices.dedup();
    indices
}

/// Generate equal-split constraints for N items.
fn split_constraints(n: usize, _dir: Direction) -> Vec<Constraint> {
    if n == 0 {
        return vec![Constraint::Min(0)];
    }
    let pct = 100 / n as u16;
    (0..n)
        .map(|i| {
            if i == n - 1 {
                Constraint::Min(0)
            } else {
                Constraint::Percentage(pct)
            }
        })
        .collect()
}

/// Draw a single terminal panel with border, title, and PTY output.
fn draw_single_terminal(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    term: &EmbeddedTerminal,
    is_active: bool,
    _term_index: usize,
) {
    let is_terminal_focused = is_active
        && (app.terminal_input_mode == TerminalInputMode::TerminalFocused
            || app.terminal_input_mode == TerminalInputMode::PrefixPending);
    let is_scrollback = is_active && app.terminal_input_mode == TerminalInputMode::ScrollBack;

    // Build title with status + elapsed + scroll indicator + phase/progress
    let elapsed = term.spawned_at.elapsed().as_secs();
    let elapsed_str = format_elapsed(elapsed);

    // Look up phase and progress from task state
    let phase_info = lookup_task_phase(app, &term.id);

    let title = match &term.status {
        TerminalStatus::Running => {
            let focus_marker = if is_terminal_focused {
                " \u{25c4} " // ◄
            } else {
                " "
            };
            let scroll_marker = if is_scrollback {
                format!(" [SCROLL +{}]", term.scroll_offset)
            } else if term.scroll_offset > 0 {
                format!(" [+{}]", term.scroll_offset)
            } else {
                String::new()
            };
            format!(
                " {} ({}) {} {}{}{}",
                term.id, term.label, elapsed_str, focus_marker.trim(), phase_info, scroll_marker
            )
        }
        TerminalStatus::NeedsAttention(reason) => match reason {
            terminal::AttentionReason::Idle { seconds } => {
                format!(" {} ({}) [idle {}s] ", term.id, term.label, seconds)
            }
            terminal::AttentionReason::PermissionPrompt { .. } => {
                format!(" {} ({}) [\u{25c6} PROMPT] ", term.id, term.label)
            }
            terminal::AttentionReason::Error { .. } => {
                format!(" {} ({}) [\u{2716} ERROR] ", term.id, term.label)
            }
            terminal::AttentionReason::HookNotification { .. } => {
                format!(" {} ({}) [\u{25c6} NOTIFY] ", term.id, term.label)
            }
            terminal::AttentionReason::WaitingForInput => {
                format!(" {} ({}) [\u{25b6} INPUT] ", term.id, term.label)
            }
        },
        TerminalStatus::Exited(code) => {
            let status_str = if *code == 0 { "ok" } else { "FAILED" };
            format!(
                " {} ({}) [exited: {} - {}] ",
                term.id, term.label, code, status_str
            )
        }
    };

    let border_style = if is_scrollback {
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
    } else {
        match &term.status {
            TerminalStatus::Running => {
                if is_terminal_focused {
                    styles::focused_border_style()
                } else if is_active {
                    // Use crew color accent if available
                    if let Some(idx) = term.color_scheme_index {
                        Style::default()
                            .fg(styles::get_scheme(idx).tab)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    }
                } else {
                    // Inactive terminal - use subtle crew color
                    if let Some(idx) = term.color_scheme_index {
                        Style::default().fg(styles::get_scheme(idx).tab)
                    } else {
                        styles::unfocused_border_style()
                    }
                }
            }
            TerminalStatus::NeedsAttention(_) => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            TerminalStatus::Exited(code) => {
                if *code == 0 {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::Red)
                }
            }
        }
    };

    // Build overlay message for exited terminals
    let overlay = if matches!(term.status, TerminalStatus::Exited(_)) && is_active {
        Some("Process exited. Press Enter to relaunch, d to dismiss.")
    } else {
        None
    };

    let border_type = if is_terminal_focused || is_active {
        BorderType::Double
    } else {
        BorderType::Plain
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Store inner rect for mouse hit-testing
    app.terminal_panel_rects
        .borrow_mut()
        .push((_term_index, inner));

    // Kind-aware rendering: embedded terminals get PTY resize + output,
    // headless terminals get a placeholder status panel.
    if let (Some(parser), Some(master)) = (term.parser(), term.master()) {
        // Resize PTY + parser to match actual render area (handles spawn, resize, layout changes)
        widget::resize_if_needed(parser, master, inner.height, inner.width);

        // Build selection range if this terminal has an active/completed selection
        let sel_range = app.text_selection.as_ref().and_then(|sel| {
            if sel.terminal_idx == _term_index {
                Some(widget::SelectionRange {
                    start_row: sel.start_row,
                    start_col: sel.start_col,
                    end_row: sel.end_row,
                    end_col: sel.end_col,
                })
            } else {
                None
            }
        });

        // Render the vt100 screen into the inner area
        let show_cursor = is_terminal_focused && !is_scrollback;
        widget::draw_terminal(
            frame,
            inner,
            parser,
            show_cursor,
            term.scroll_offset,
            sel_range.as_ref(),
        );
    } else {
        // Headless terminal — render a status panel with hook activity info
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(""));

        let status_text = match &term.status {
            terminal::TerminalStatus::Running => "Headless terminal \u{2014} running (no interactive output)",
            terminal::TerminalStatus::Exited(code) => {
                if *code == 0 {
                    "Headless terminal \u{2014} exited successfully"
                } else {
                    "Headless terminal \u{2014} exited with error"
                }
            }
            terminal::TerminalStatus::NeedsAttention(_) => "Headless terminal \u{2014} needs attention",
        };
        lines.push(Line::from(Span::styled(
            status_text,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Show hook activity details if available
        if let Some(hook_state) = &term.hook_state {
            if !hook_state.activity_label.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  Activity: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&hook_state.activity_label, Style::default().fg(Color::White)),
                ]));
            }

            if !hook_state.tool_counts.is_empty() {
                let mut counts: Vec<(&String, &u32)> = hook_state.tool_counts.iter().collect();
                counts.sort_by(|a, b| b.1.cmp(a.1));
                let summary: String = counts
                    .iter()
                    .take(5)
                    .map(|(name, count)| format!("{}: {}", name, count))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(vec![
                    Span::styled("  Tools:    ", Style::default().fg(Color::DarkGray)),
                    Span::styled(summary, Style::default().fg(Color::White)),
                ]));
            }

            if hook_state.total_cost_usd > 0.0 {
                lines.push(Line::from(vec![
                    Span::styled("  Cost:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("${:.4}", hook_state.total_cost_usd),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }

            if hook_state.total_input_tokens > 0 || hook_state.total_output_tokens > 0 {
                lines.push(Line::from(vec![
                    Span::styled("  Tokens:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}in / {}out", hook_state.total_input_tokens, hook_state.total_output_tokens),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }

            if hook_state.tool_counts.is_empty() && hook_state.activity_label.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  Waiting for activity...",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "  Waiting for activity...",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let msg = Paragraph::new(lines);
        frame.render_widget(msg, inner);
    }

    // Draw scroll indicator when scrolled back (any mode, embedded only)
    if term.scroll_offset > 0 && inner.height > 0 && term.is_embedded() {
        let scrollback_total = term.parser()
            .map(widget::scrollback_available)
            .unwrap_or(0);
        if scrollback_total > 0 {
            let indicator = if is_scrollback {
                format!(
                    " [{}/{}] ",
                    term.scroll_offset,
                    scrollback_total
                )
            } else {
                format!(" [+{}] ", term.scroll_offset)
            };
            let bg_color = if is_scrollback { Color::Magenta } else { Color::DarkGray };
            let indicator_span = Span::styled(
                indicator,
                Style::default()
                    .fg(Color::White)
                    .bg(bg_color)
                    .add_modifier(Modifier::BOLD),
            );
            let indicator_line = Line::from(vec![indicator_span]);
            let indicator_area = Rect::new(
                inner.x,
                inner.y + inner.height - 1,
                inner.width.min(20),
                1,
            );
            frame.render_widget(Paragraph::new(indicator_line), indicator_area);
        }
    }

    // Draw overlay message if present
    if let Some(msg) = overlay {
        let msg_width = msg.len() as u16 + 4;
        let msg_height = 3u16;
        if inner.width >= msg_width && inner.height >= msg_height {
            let x = inner.x + (inner.width - msg_width) / 2;
            let y = inner.y + (inner.height - msg_height) / 2;
            let overlay_area = Rect::new(x, y, msg_width, msg_height);

            let overlay_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            let overlay_inner = overlay_block.inner(overlay_area);
            frame.render_widget(overlay_block, overlay_area);

            let overlay_text = Paragraph::new(msg)
                .style(Style::default().fg(Color::White).bg(Color::DarkGray));
            frame.render_widget(overlay_text, overlay_inner);
        }
    }
}

/// Look up the current phase and progress for a terminal's task.
/// Returns a string like " [implementer 60%]" or "" if not found.
fn lookup_task_phase(app: &App, terminal_id: &str) -> String {
    for repo in &app.repos {
        for task in &repo.tasks {
            if task.state.task_id == terminal_id {
                let phase = match &task.state.phase {
                    Some(p) if !p.is_empty() => p.as_str(),
                    _ => return String::new(),
                };
                let progress = &task.state.implementation_progress;
                let total = progress.total_steps;
                let current = progress.current_step;
                if total > 0 {
                    let pct = (current as f64 / total as f64 * 100.0) as u32;
                    return format!(" [{} {}%]", phase, pct);
                }
                return format!(" [{}]", phase);
            }
        }
    }
    String::new()
}

/// Format elapsed seconds into a human-readable string.
fn format_elapsed(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}
