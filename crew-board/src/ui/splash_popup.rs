/// Welcome splash screen shown on startup.
///
/// Gives instant awareness of active tasks across all repos.
/// Tasks are selectable — Enter resumes, N creates new, Esc dismisses.
use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

const MAX_ACTIVE_IN_SPLASH: usize = 5;
const MAX_HEALTH_WARNINGS: usize = 3;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = centered_rect(62, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" crew-board v{} ", env!("CARGO_PKG_VERSION")))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Use pre-built splash_active_tasks for consistent ordering with key handling
    let active_count = app.splash_active_tasks.len();

    // Total stats
    let total_repos = app.repos.len();
    let total_tasks: usize = app.repos.iter().map(|r| r.tasks.len()).sum();
    let total_issues: usize = app.repos.iter().map(|r| r.issues.len()).sum();
    let open_issues: usize = app.repos.iter().map(|r| r.open_issue_count()).sum();

    let key_style = Style::default().fg(Color::Cyan);
    let dim = Style::default().fg(Color::DarkGray);
    let active_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();

    // Health warnings at top (per accessibility recommendation)
    let health_warnings = collect_health_warnings(app);
    if !health_warnings.is_empty() {
        let shown = health_warnings.len().min(MAX_HEALTH_WARNINGS);
        for warning in &health_warnings[..shown] {
            lines.push(Line::from(vec![
                Span::styled("WARNING: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(warning.clone(), Style::default().fg(Color::Yellow)),
            ]));
        }
        if health_warnings.len() > MAX_HEALTH_WARNINGS {
            lines.push(Line::from(Span::styled(
                format!("  ... and {} more issues — see task details", health_warnings.len() - MAX_HEALTH_WARNINGS),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::raw(""));
    }

    // Active work section
    if active_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No active tasks",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  Press ", dim),
            Span::styled("N", key_style),
            Span::styled(" or ", dim),
            Span::styled("F4", key_style),
            Span::styled(" to create a new worktree", dim),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} active task{}", active_count, if active_count == 1 { "" } else { "s" }),
                active_style,
            ),
            Span::styled(
                if active_count > MAX_ACTIVE_IN_SPLASH {
                    format!(" (showing top {})", MAX_ACTIVE_IN_SPLASH)
                } else {
                    String::new()
                },
                dim,
            ),
        ]));
        lines.push(Line::raw(""));

        let shown_count = active_count.min(MAX_ACTIVE_IN_SPLASH);
        for (idx, &(ri, ti)) in app.splash_active_tasks.iter().enumerate().take(shown_count) {
            let repo = &app.repos[ri];
            let task = &repo.tasks[ti];
            let repo_name = &repo.name;
            let phase = task.state.status_label();
            let desc = truncate_str(&task.state.description, 40);
            let task_id = &task.state.task_id;
            let is_selected = idx == app.splash_task_cursor;

            // Progress indicator for implementer phase
            let progress_str = if task.state.implementation_progress.total_steps > 0 {
                format!(
                    " {}/{}",
                    task.state.implementation_progress.current_step,
                    task.state.implementation_progress.total_steps
                )
            } else {
                String::new()
            };

            let prefix = if is_selected { "  ▸ " } else { "    " };
            let prefix_style = if is_selected {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let task_id_style = if is_selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD).bg(Color::Indexed(24))
            } else {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            };
            let phase_style = if is_selected {
                Style::default().fg(Color::Cyan).bg(Color::Indexed(24))
            } else {
                Style::default().fg(Color::Cyan)
            };
            let desc_style = if is_selected {
                Style::default().fg(Color::White).bg(Color::Indexed(24))
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(task_id.to_string(), task_id_style),
                Span::styled(format!("  [{}{}]", phase, progress_str), phase_style),
                Span::styled(format!("  {}", desc), desc_style),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(repo_name.to_string(), dim),
            ]));
        }

        if active_count > MAX_ACTIVE_IN_SPLASH {
            lines.push(Line::from(Span::styled(
                format!(
                    "  ... and {} more — press Esc for full list",
                    active_count - MAX_ACTIVE_IN_SPLASH
                ),
                dim,
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("─".repeat(inner.width.saturating_sub(2) as usize), dim)));
    lines.push(Line::raw(""));

    // Actionable hints
    if active_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("  Enter", key_style),
            Span::styled("  Resume selected task", dim),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  N", key_style),
        Span::styled("      New worktree", dim),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Esc", key_style),
        Span::styled("    Full dashboard", dim),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  F10", key_style),
        Span::styled("    Quit", dim),
    ]));

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("─".repeat(inner.width.saturating_sub(2) as usize), dim)));
    lines.push(Line::raw(""));

    // Summary stats line
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(
                "{} repos · {} tasks · {} issues ({} open)",
                total_repos, total_tasks, total_issues, open_issues
            ),
            dim,
        ),
    ]));

    lines.push(Line::raw(""));

    // Navigation hint
    if active_count > 0 {
        lines.push(Line::from(Span::styled(
            "  ↑↓ select task · Enter resume · N new · Esc dashboard",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  N new worktree · Esc dashboard",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Scrollable content
    let total_lines = lines.len() as u16;
    let visible_lines = inner.height;
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let scroll_offset = app.splash_scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));
    frame.render_widget(paragraph, inner);

    // Scrollbar if content overflows
    if total_lines > visible_lines {
        let mut scrollbar_state =
            ScrollbarState::new(max_scroll as usize).position(scroll_offset as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            inner,
            &mut scrollbar_state,
        );
    }
}

/// Collect health warnings from cached health status (no file I/O during rendering).
fn collect_health_warnings(app: &App) -> Vec<String> {
    let mut warnings = Vec::new();
    for repo in &app.repos {
        for task in &repo.tasks {
            match &task.cached_health {
                crate::data::task::TaskHealth::Healthy => {}
                crate::data::task::TaskHealth::MissingOutputs(missing) => {
                    warnings.push(format!(
                        "{}: missing output for phase(s) {}",
                        task.state.task_id,
                        missing.join(", "),
                    ));
                }
                crate::data::task::TaskHealth::StalePhase(phase) => {
                    warnings.push(format!(
                        "{}: phase {} started but no output found (30+ min)",
                        task.state.task_id, phase
                    ));
                }
            }
        }
    }
    warnings
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
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
