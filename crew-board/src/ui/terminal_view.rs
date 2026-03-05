//! View 5: Terminals — embedded terminal multiplexer view.
//!
//! Left panel: list of terminals with status icons.
//! Right panel: rendered output of the focused terminal (or tiled layout).

use crate::app::{App, TerminalInputMode, TerminalLayout};
use crate::terminal::{self, widget, EmbeddedTerminal, TerminalStatus};
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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

    // Calculate crew list width based on layout mode
    let list_width = match app.terminal_layout {
        TerminalLayout::Focused => 20u16,
        TerminalLayout::Tiled2 => 18,
        TerminalLayout::Tiled4 => 15,
        TerminalLayout::Stacked => 20,
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(list_width),
            Constraint::Min(40),
        ])
        .split(area);

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

    let block = Block::default()
        .title(" Crew List ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    for (i, term) in mgr.terminals.iter().enumerate() {
        let icon = match &term.status {
            TerminalStatus::Running => Span::styled(
                "\u{25cf} ", // ●
                Style::default().fg(Color::Green),
            ),
            TerminalStatus::NeedsAttention(reason) => {
                let (icon, color) = match reason {
                    terminal::AttentionReason::PermissionPrompt => ("\u{25c6} ", Color::Cyan), // ◆ prompt
                    terminal::AttentionReason::Idle { .. } => ("\u{25cb} ", Color::DarkGray),  // ○ idle
                    terminal::AttentionReason::Error => ("\u{2716} ", Color::Red),             // ✖ error
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
        let short_id = base_id.strip_prefix("TASK_").map_or_else(
            || base_id.to_string(),
            |num| format!("#{}", num),
        );
        let display_name = short_id.as_str();

        // Truncate to fit
        let avail = max_label.saturating_sub(elapsed_str.len() + 1);
        let label = if display_name.len() > avail {
            format!("{}...", &display_name[..avail.saturating_sub(3)])
        } else {
            display_name.to_string()
        };

        let style = if i == mgr.focused {
            styles::selected_style()
        } else if let Some(idx) = term.color_scheme_index {
            Style::default().fg(styles::get_scheme(idx).tab)
        } else {
            Style::default()
        };

        let time_style = Style::default().fg(Color::DarkGray);

        lines.push(Line::from(vec![
            icon,
            Span::styled(label, style),
            Span::styled(format!(" {}", elapsed_str), time_style),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Focused layout: one large terminal panel.
fn draw_focused_layout(frame: &mut Frame, app: &App, area: Rect) {
    let mgr = match &app.terminal_manager {
        Some(m) => m,
        None => return,
    };

    if let Some(term) = mgr.focused_terminal() {
        let is_focused = app.terminal_input_mode == TerminalInputMode::TerminalFocused
            || app.terminal_input_mode == TerminalInputMode::PrefixPending;
        let is_scrollback = app.terminal_input_mode == TerminalInputMode::ScrollBack;

        draw_single_terminal(frame, app, area, term, is_focused || is_scrollback, mgr.focused);
    }
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
fn terminal_indices_for_layout(mgr: &terminal::TerminalManager, max: usize) -> Vec<usize> {
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

    // Build title with status + elapsed + scroll indicator
    let elapsed = term.spawned_at.elapsed().as_secs();
    let elapsed_str = format_elapsed(elapsed);

    let title = match &term.status {
        TerminalStatus::Running => {
            let focus_marker = if is_terminal_focused {
                " \u{25c4} " // ◄
            } else {
                " "
            };
            let scroll_marker = if is_scrollback {
                format!(" [SCROLL +{}]", term.scroll_offset)
            } else {
                String::new()
            };
            format!(
                " {} ({}) {} {}{}",
                term.id, term.label, elapsed_str, focus_marker.trim(), scroll_marker
            )
        }
        TerminalStatus::NeedsAttention(reason) => match reason {
            terminal::AttentionReason::Idle { seconds } => {
                format!(" {} ({}) [idle {}s] ", term.id, term.label, seconds)
            }
            terminal::AttentionReason::PermissionPrompt => {
                format!(" {} ({}) [\u{25c6} PROMPT] ", term.id, term.label)
            }
            terminal::AttentionReason::Error => {
                format!(" {} ({}) [\u{2716} ERROR] ", term.id, term.label)
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

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Store inner rect for mouse hit-testing
    app.terminal_panel_rects
        .borrow_mut()
        .push((_term_index, inner));

    // Resize PTY + parser to match actual render area (handles spawn, resize, layout changes)
    widget::resize_if_needed(&term.parser, &term.master, inner.height, inner.width);

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
        &term.parser,
        show_cursor,
        term.scroll_offset,
        sel_range.as_ref(),
    );

    // Draw scroll indicator when in scroll-back mode
    if is_scrollback && inner.height > 0 {
        let scrollback_total = widget::scrollback_available(&term.parser);
        if scrollback_total > 0 {
            let indicator = format!(
                " [{}/{}] ",
                term.scroll_offset,
                scrollback_total
            );
            let indicator_span = Span::styled(
                indicator,
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Magenta)
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
