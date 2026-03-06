use crate::app::{
    ActiveView, App, CleanupStep, CreateStep, DetailMode, FocusPane, LaunchStep,
    ModifierBarState, TerminalInputMode,
};
use crate::ui::styles;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // Line 1: View tabs + contextual hints + stats
    draw_info_line(frame, app, chunks[0]);

    // Line 2: F-key bar (context-adaptive)
    draw_fkey_bar(frame, app, chunks[1]);
}

fn draw_info_line(frame: &mut Frame, app: &App, area: Rect) {
    let elapsed = app.last_refresh.elapsed().as_secs();
    let hints = context_hints(app);

    let total_tasks: usize = app.repos.iter().map(|r| r.tasks.len()).sum();
    let active_tasks: usize = app.repos.iter().map(|r| r.active_task_count()).sum();
    let total_issues: usize = app.repos.iter().map(|r| r.issues.len()).sum();
    let open_issues: usize = app.repos.iter().map(|r| r.open_issue_count()).sum();

    // Attention + exited badge for terminal view indicator
    let attn_count = app
        .terminal_manager
        .as_ref()
        .map(|m| m.attention_count())
        .unwrap_or(0);
    let exited_count = app
        .terminal_manager
        .as_ref()
        .map(|m| m.exited_count())
        .unwrap_or(0);

    // Active view label with position (replaces tab bar — Shift+F1-F5 switches views)
    let (view_label, view_num) = match app.active_view {
        ActiveView::Tasks => ("Tasks", 1),
        ActiveView::BeadsIssues => ("Issues", 2),
        ActiveView::Config => ("Config", 3),
        ActiveView::CostSummary => ("Cost", 4),
        ActiveView::Terminals => ("Terms", 5),
    };
    // Terminal badge appended to view label when relevant
    let badge = if app.active_view == ActiveView::Terminals {
        if attn_count > 0 {
            format!(" \u{25c6}{}", attn_count) // ◆N
        } else if exited_count > 0 {
            format!(" \u{2717}{}", exited_count) // ✗N
        } else {
            String::new()
        }
    } else if attn_count > 0 {
        // Show attention badge even when not on Terminals view (cross-view flash)
        format!("  \u{25c6}{}", attn_count)
    } else {
        String::new()
    };

    // Determine if attention flash is active (alternating style)
    let flash_active = app.attention_flash_until.is_some_and(|until| {
        let elapsed_ms = (std::time::Instant::now()
            .duration_since(until.checked_sub(std::time::Duration::from_secs(2)).unwrap_or(until)))
            .as_millis();
        // Blink: 250ms on/off cycle
        (elapsed_ms / 250).is_multiple_of(2)
    });

    let line = Line::from(vec![
        Span::styled(
            format!(" {} [{}/5] ", view_label, view_num),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            badge,
            if flash_active && attn_count > 0 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            },
        ),
        Span::raw(" "),
        Span::styled(hints, styles::hint_style()),
        Span::styled(
            format!(
                "  {} repos {} tasks({} active) {} issues({} open) ({}s)",
                app.repos.len(),
                total_tasks,
                active_tasks,
                total_issues,
                open_issues,
                elapsed,
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn draw_fkey_bar(frame: &mut Frame, app: &App, area: Rect) {
    // When a popup is active, show popup-specific hints in the bar
    if let Some(hints) = popup_hints(app) {
        let line = Line::from(vec![Span::styled(
            hints,
            Style::default().fg(Color::Black).bg(Color::Cyan),
        )]);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
        return;
    }

    // Terminal mode-specific bars (override modifier layers)
    if app.active_view == ActiveView::Terminals {
        match app.terminal_input_mode {
            TerminalInputMode::TerminalFocused | TerminalInputMode::PrefixPending => {
                draw_terminal_focused_bar(frame, area);
                return;
            }
            TerminalInputMode::ScrollBack => {
                draw_scrollback_bar(frame, app, area);
                return;
            }
            TerminalInputMode::Normal => {}
        }
    }

    // Modifier layer bars
    match app.modifier_bar_state {
        ModifierBarState::Normal => draw_base_fkey_bar(frame, app, area),
        ModifierBarState::Shift => draw_shift_fkey_bar(frame, area),
        ModifierBarState::Ctrl => draw_ctrl_fkey_bar(frame, app, area),
        ModifierBarState::Alt => draw_reserved_layer(frame, area, "ALT"),
        ModifierBarState::ShiftCtrl => draw_reserved_layer(frame, area, "S+C"),
        ModifierBarState::AltShift => draw_reserved_layer(frame, area, "A+S"),
        ModifierBarState::CtrlAlt => draw_reserved_layer(frame, area, "C+A"),
    }
}

/// Base F-key bar (no modifier held).
fn draw_base_fkey_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(indicator_pad());
    spans.extend(fkey_cell(1, "Help"));
    spans.extend(fkey_cell(2, "Launch"));
    spans.extend(fkey_cell(3, "Search"));
    spans.extend(fkey_cell(4, "New"));
    spans.extend(fkey_cell(5, "Rfrsh"));
    spans.extend(fkey_cell(6, "Clean"));
    spans.extend(fkey_cell(7, "Attn"));
    spans.extend(fkey_cell(8, "Perms"));
    if app.active_view == ActiveView::Terminals
        && app.terminal_input_mode == TerminalInputMode::Normal
    {
        spans.extend(fkey_cell(9, "Focus"));
    } else {
        spans.extend(fkey_cell_empty(9));
    }
    spans.extend(fkey_cell(10, "Quit"));
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

/// Shift+F-key bar (view switching layer).
fn draw_shift_fkey_bar(frame: &mut Frame, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(layer_indicator_fixed("SHIFT"));
    spans.extend(fkey_cell(1, "Tasks"));
    spans.extend(fkey_cell(2, "Issues"));
    spans.extend(fkey_cell(3, "Config"));
    spans.extend(fkey_cell(4, "Cost"));
    spans.extend(fkey_cell(5, "Terms"));
    spans.extend(fkey_cell(6, "Docs"));
    spans.extend(fkey_cell(7, "Hist"));
    spans.extend(fkey_cell_empty(8));
    spans.extend(fkey_cell_empty(9));
    spans.extend(fkey_cell_empty(10));
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

/// Ctrl+F-key bar (admin/context layer).
fn draw_ctrl_fkey_bar(frame: &mut Frame, _app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(layer_indicator_fixed("CTRL"));
    for n in 1..=10 {
        if n == 5 {
            spans.extend(fkey_cell(n, "Live"));
        } else {
            spans.extend(fkey_cell_empty(n));
        }
    }
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

/// Bar shown when in TerminalFocused mode (input goes to PTY).
fn draw_terminal_focused_bar(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            " \u{2501}\u{2501} INPUT \u{2192} TERMINAL \u{2501}\u{2501} ",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " F12 ",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "exit ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            " S+F1",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Tasks ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            "F2",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Issues ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            "F3",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Cfg ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            "F4",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Cost ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            "F5",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Terms ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::styled(
            " S+PgUp/Dn",
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "crew ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
    ]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Bar shown during scroll-back mode.
fn draw_scrollback_bar(frame: &mut Frame, app: &App, area: Rect) {
    let offset = app
        .terminal_manager
        .as_ref()
        .and_then(|m| m.focused_terminal())
        .map(|t| t.scroll_offset)
        .unwrap_or(0);

    // If search input is active, show the search bar instead
    if let Some(ref input) = app.terminal_search_input {
        let line = Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(input.value().to_string(), Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(Color::DarkGray)),
            Span::styled("  Enter: search  Esc: cancel", Style::default().fg(Color::DarkGray)),
        ]);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
        return;
    }

    let search_info = if !app.terminal_search_query.is_empty() {
        let total = app.terminal_search_matches.len();
        if total > 0 {
            format!(
                "  \"{}\": {}/{} matches  n/N:next/prev",
                app.terminal_search_query,
                app.terminal_search_match_idx + 1,
                total
            )
        } else {
            format!("  \"{}\": no matches", app.terminal_search_query)
        }
    } else {
        String::new()
    };

    let mut spans = vec![
        Span::styled(
            " SCROLL\u{25b6} ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::default().bg(Color::Black)),
        prefix_key_span("\u{2191}\u{2193}/jk", "Line"),
        prefix_key_span("PgUp/Dn", "Page"),
        prefix_key_span("Home", "Top"),
        prefix_key_span("End", "Live+Exit"),
        prefix_key_span("/", "Search"),
        prefix_key_span("Esc", "Exit (keep pos)"),
        Span::styled(
            format!("  offset:{}", offset),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    if !search_info.is_empty() {
        spans.push(Span::styled(search_info, Style::default().fg(Color::Yellow)));
    }
    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Reserved modifier layer (no bindings yet — shows label + all empty slots).
fn draw_reserved_layer(frame: &mut Frame, area: Rect, label: &str) {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(layer_indicator_fixed(label));
    for n in 1..=10 {
        spans.extend(fkey_cell_empty(n));
    }
    let paragraph = Paragraph::new(Line::from(spans));
    frame.render_widget(paragraph, area);
}

// ── Helper functions ─────────────────────────────────────────────────

/// Fixed indicator width (matches " SHIFT▶" = 7 display columns).
const INDICATOR_WIDTH: usize = 7;

/// Fixed cell width: F{n}(2-3) + label_area + gap(1) = 9 per cell.
const CELL_WIDTH: usize = 9;

/// Fixed-width F-key cell (NC-style). Number: bold white. Label: black on cyan, padded.
fn fkey_cell(num: u8, label: &str) -> Vec<Span<'static>> {
    let fnum = format!("F{}", num);
    let label_area = CELL_WIDTH - fnum.len() - 1;
    let padded = format!("{:<w$}", label, w = label_area);
    vec![
        Span::styled(
            fnum,
            Style::default()
                .fg(Color::White)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(padded, Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::styled(" ", Style::default().bg(Color::Black)),
    ]
}

/// Fixed-width empty F-key cell (dimmed number, no label).
fn fkey_cell_empty(num: u8) -> Vec<Span<'static>> {
    let fnum = format!("F{}", num);
    let rest = CELL_WIDTH - fnum.len();
    vec![
        Span::styled(
            fnum,
            Style::default().fg(Color::DarkGray).bg(Color::Black),
        ),
        Span::styled(
            " ".repeat(rest),
            Style::default().bg(Color::Black),
        ),
    ]
}

/// Blank padding for the base layer (same width as modifier indicator).
fn indicator_pad() -> Span<'static> {
    Span::styled(
        " ".repeat(INDICATOR_WIDTH),
        Style::default().bg(Color::Black),
    )
}

/// Fixed-width layer indicator (e.g., " SHIFT▶" right-aligned to INDICATOR_WIDTH).
fn layer_indicator_fixed(label: &str) -> Span<'static> {
    let text = format!(" {}\u{25b6}", label);
    let padded = format!("{:>w$}", text, w = INDICATOR_WIDTH);
    Span::styled(
        padded,
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

/// Styled spans for a prefix command key+label.
fn prefix_key_span(key: &str, label: &str) -> Span<'static> {
    Span::styled(
        format!(" {}:{}", key, label),
        Style::default().fg(Color::Cyan),
    )
}

/// Context hints for the info line.
fn context_hints(app: &App) -> String {
    // Popups get their own hints in the F-key bar, so just show navigation hints
    if app.search_popup.is_some()
        || app.create_popup.is_some()
        || app.cleanup_popup.is_some()
        || app.launch_popup.is_some()
    {
        return String::new();
    }

    match app.active_view {
        ActiveView::Tasks => match app.focus_pane {
            FocusPane::Left => "\u{2191}\u{2193} nav  Enter expand  Tab\u{2192}pane  d docs  h hist".to_string(),
            FocusPane::Right => match &app.detail_mode {
                DetailMode::Overview => "PgUp/Dn scroll  d docs  h hist  Tab\u{2190}pane".to_string(),
                DetailMode::DocList { .. } => "\u{2191}\u{2193} select  Enter read  Esc back".to_string(),
                DetailMode::DocReader { .. } => "PgUp/Dn scroll  Esc back".to_string(),
                DetailMode::History => "PgUp/Dn scroll  Esc back".to_string(),
            },
        },
        ActiveView::BeadsIssues => "\u{2191}\u{2193} nav  Tab pane".to_string(),
        ActiveView::Config => "PgUp/Dn scroll".to_string(),
        ActiveView::CostSummary => "PgUp/Dn scroll".to_string(),
        ActiveView::Terminals => match app.terminal_input_mode {
            TerminalInputMode::Normal => {
                "\u{2191}\u{2193} nav  Enter/F12 focus  d dismiss  D all  [ scroll  F7 attn".to_string()
            }
            TerminalInputMode::TerminalFocused | TerminalInputMode::PrefixPending => {
                format!(
                    "F12 exit  Shift+F switch view  (input \u{2192} PTY)  {}",
                    app.terminal_layout.label()
                )
            }
            TerminalInputMode::ScrollBack => {
                format!(
                    "SCROLL: \u{2191}\u{2193}/PgUp/Dn  Home/End  q/Esc exit  offset:{}",
                    app.terminal_manager
                        .as_ref()
                        .and_then(|m| m.focused_terminal())
                        .map(|t| t.scroll_offset)
                        .unwrap_or(0)
                )
            }
        },
    }
}

/// If a popup is open, return hints to show in the F-key bar area.
fn popup_hints(app: &App) -> Option<String> {
    if let Some(popup) = &app.search_popup {
        let count = popup.results.len();
        return Some(if count > 0 {
            format!(" \u{2191}\u{2193} select  Enter go  Esc cancel  ({} results)", count)
        } else {
            " Type to search  Esc cancel".to_string()
        });
    }
    if let Some(popup) = &app.create_popup {
        return Some(match popup.step {
            CreateStep::InputDescription => " Enter next  Esc cancel".to_string(),
            CreateStep::SelectHost => " \u{2191}\u{2193} select  Enter confirm  Esc cancel".to_string(),
            CreateStep::ToggleSettings => {
                " \u{2191}\u{2193} nav  Space toggle  Enter confirm  Esc cancel".to_string()
            }
            CreateStep::Confirm => " Enter create  Esc cancel".to_string(),
            CreateStep::Executing => " Creating worktree...".to_string(),
            CreateStep::Done => " Enter confirm  Esc close".to_string(),
        });
    }
    if let Some(popup) = &app.cleanup_popup {
        return Some(match popup.step {
            CleanupStep::SelectWorktrees => {
                let n = popup.selected.len();
                format!(
                    " Space toggle  a select-all  Enter next ({} selected)  Esc cancel",
                    n
                )
            }
            CleanupStep::Settings => " Space toggle  Enter preview  Esc cancel".to_string(),
            CleanupStep::Preview => " Enter EXECUTE  j/k scroll  Esc cancel".to_string(),
            CleanupStep::Executing => " Cleaning worktrees...".to_string(),
            CleanupStep::Done => " Enter close  Esc close".to_string(),
        });
    }
    if app.permission_popup.is_some() {
        return Some(" \u{2191}\u{2193} select  a approve  d deny  A all  t type  v view  Esc close".to_string());
    }
    if let Some(popup) = &app.launch_popup {
        return Some(match popup.step {
            LaunchStep::SelectTerminal | LaunchStep::SelectHost => {
                " \u{2191}\u{2193} select  Enter confirm  Esc cancel".to_string()
            }
            LaunchStep::Done => " Enter close  Esc close".to_string(),
        });
    }
    None
}

