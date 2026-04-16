pub mod activity_view;
pub mod beads_view;
pub mod cleanup_popup;
pub mod config_view;
pub mod cost_view;
pub mod create_popup;
pub mod detail_pane;
pub mod help_popup;
pub mod keybindings;
pub mod launch_popup;
pub mod permission_popup;
pub mod search_popup;
pub mod splash_popup;
pub mod stats_popup;
pub mod status_bar;
pub mod styles;
pub mod task_list;
pub mod terminal_view;

use crate::app::{ActiveView, App};
use crate::terminal::TerminalStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App) {
    // Clear all mouse hit-test rects at frame start
    *app.pane_rects.borrow_mut() = None;
    *app.content_rect.borrow_mut() = None;
    *app.list_inner_rect.borrow_mut() = None;
    app.doc_list_item_offsets.borrow_mut().clear();
    app.view_tab_rects.borrow_mut().clear();
    app.terminal_list_line_map.borrow_mut().clear();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),  // Main content
            Constraint::Length(2), // Status bar
        ])
        .split(frame.area());

    // Main content: dual-pane or single-pane depending on view
    match app.active_view {
        ActiveView::Tasks => draw_dual_pane(frame, app, chunks[0]),
        ActiveView::BeadsIssues => beads_view::draw(frame, app, chunks[0]),
        ActiveView::Config => config_view::draw(frame, app, chunks[0]),
        ActiveView::CostSummary => cost_view::draw(frame, app, chunks[0]),
        ActiveView::Terminals => terminal_view::draw(frame, app, chunks[0]),
        ActiveView::ActivityFeed => activity_view::draw(frame, app, chunks[0]),
    };

    // Status bar
    status_bar::draw(frame, app, chunks[1]);

    // Popup overlays (drawn on top)
    if app.launch_popup.is_some() {
        launch_popup::draw(frame, app);
    }
    if app.create_popup.is_some() {
        create_popup::draw(frame, app);
    }
    if app.cleanup_popup.is_some() {
        cleanup_popup::draw(frame, app);
    }
    if app.permission_popup.is_some() {
        permission_popup::draw(frame, app, chunks[0]);
    }
    if app.search_popup.is_some() {
        search_popup::draw(frame, app);
    }
    if app.stats_popup.is_some() {
        stats_popup::draw(frame, app);
    }
    if app.show_help {
        help_popup::draw(frame, app);
    }
    if app.show_splash {
        splash_popup::draw(frame, app);
    }
    if app.quit_confirm {
        draw_quit_confirm(frame, app);
    }
}

fn draw_quit_confirm(frame: &mut Frame, app: &App) {
    let (running, attention) = app
        .terminal_manager
        .as_ref()
        .map(|mgr| {
            let r = mgr
                .terminals
                .iter()
                .filter(|t| matches!(t.status, TerminalStatus::Running))
                .count();
            let a = mgr
                .terminals
                .iter()
                .filter(|t| matches!(t.status, TerminalStatus::NeedsAttention(_)))
                .count();
            (r, a)
        })
        .unwrap_or((0, 0));

    let mut lines = vec![Line::from(Span::styled(
        " Quit crew-board? ",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::raw(""));

    if running + attention > 0 {
        if running > 0 {
            lines.push(Line::from(format!("  {} crew(s) still running", running)));
        }
        if attention > 0 {
            lines.push(Line::from(format!(
                "  {} terminal(s) need attention",
                attention
            )));
        }
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[Y]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("es  "),
        Span::styled("[N]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("o / Esc"),
    ]));

    let height = lines.len() as u16 + 2;
    let width = 36;
    let area = frame.area();
    let popup = Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width.min(area.width),
        height.min(area.height),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_dual_pane(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let left_pct = app.pane_width_tasks as u16;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(left_pct), Constraint::Percentage(100 - left_pct)])
        .split(area);

    // Store pane rects for mouse click-to-focus
    *app.pane_rects.borrow_mut() = Some((chunks[0], chunks[1]));

    task_list::draw(frame, app, chunks[0]);
    detail_pane::draw(frame, app, chunks[1]);
}
