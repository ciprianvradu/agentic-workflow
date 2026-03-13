use crate::app::App;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = centered_rect(70, 80, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help — crew-board ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bold = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);
    let dim = Style::default().fg(Color::DarkGray);
    let mod_style = Style::default().fg(Color::Green);

    let kitty_note = if app.kitty_protocol_enabled {
        "kitty protocol: ON (modifier layers update in real-time)"
    } else {
        "kitty protocol: OFF (modifier layers flash on Shift/Ctrl+F-key press)"
    };

    let lines = vec![
        Line::from(Span::styled("F-Key Bar (Base Layer)", bold)),
        Line::from(""),
        key_line("F1", "Show this help", key_style),
        key_line("F2", "Launch terminal with AI host", key_style),
        key_line("F3", "Search across tasks & documents", key_style),
        key_line("F4", "New worktree (Tasks/Issues/etc) / Cycle layout (Terminals)", key_style),
        key_line("F5", "Force refresh", key_style),
        key_line("F6", "Documents (Tasks) / Dismiss terminal (Terminals) / Cleanup (others)", key_style),
        key_line("F7", "History/Back (Tasks) / Jump to attention terminal (others)", key_style),
        key_line("F8", "Permission queue popup", key_style),
        key_line("F9", "Focus terminal (Terminals view)", key_style),
        key_line("F10", "Quit", key_style),
        Line::from(""),
        Line::from(Span::styled("Shift+F Layer (Views & Detail)", bold)),
        Line::from(""),
        key_line("Shift+F1", "Switch to Tasks view", mod_style),
        key_line("Shift+F2", "Switch to Issues view", mod_style),
        key_line("Shift+F3", "Switch to Config view", mod_style),
        key_line("Shift+F4", "Switch to Cost view", mod_style),
        key_line("Shift+F5", "Switch to Terminals view", mod_style),
        key_line("Shift+F6", "Switch to Activity Feed", mod_style),
        Line::from(""),
        Line::from(Span::styled("Ctrl+F Layer", bold)),
        Line::from(""),
        key_line("Ctrl+F4", "Dismiss All exited (Terminals) / Crew filter (Activity)", mod_style),
        key_line("Ctrl+F5", "Live view (Terminals) / Event filter (Activity)", mod_style),
        key_line("Ctrl+F6", "Statistics popup / Tool filter (Activity)", mod_style),
        key_line("Ctrl+F7", "Toggle auto-scroll (Activity)", mod_style),
        key_line("Ctrl+F8", "Enter scroll-back (Terminals) / Gantt (Activity)", mod_style),
        Line::from(""),
        Line::from(Span::styled("Navigation", bold)),
        Line::from(""),
        key_line("\u{2191}/\u{2193} or j/k", "Move up/down", key_style),
        key_line("Enter/Space", "Expand/collapse repo", key_style),
        key_line("Tab", "Switch pane focus", key_style),
        key_line("PgUp/PgDn", "Scroll detail pane", key_style),
        key_line("Home/End", "Jump to top/bottom of detail", key_style),
        key_line("`", "Cycle views", key_style),
        Line::from(""),
        Line::from(Span::styled("Detail Pane", bold)),
        Line::from(""),
        key_line("F6", "Browse task documents (Tasks view)", key_style),
        key_line("F7", "View task history (Tasks view)", key_style),
        key_line("Esc", "Back (close popup / exit detail view)", key_style),
        Line::from(""),
        Line::from(Span::styled("Terminal Modes (View 5)", bold)),
        Line::from(""),
        Line::from(Span::styled("  Normal Mode:", dim)),
        key_line("F12", "Focus terminal (all keys \u{2192} PTY)", key_style),
        key_line("Enter", "Relaunch exited terminal", key_style),
        key_line("F6 / Delete", "Dismiss exited terminal (Terminals view)", key_style),
        key_line("Ctrl+F4", "Dismiss ALL exited terminals", key_style),
        key_line("Right", "Cycle layout (focused/tiled/stacked)", key_style),
        key_line("Ctrl+F7", "Toggle auto-accept \u{26a1} for focused terminal", key_style),
        key_line("[", "Enter scroll-back mode", key_style),
        Line::from(""),
        Line::from(Span::styled("  Focused Mode (input \u{2192} PTY):", dim)),
        key_line("F5", "Previous terminal (skip exited)", key_style),
        key_line("F6", "Next terminal (skip exited)", key_style),
        key_line("F7", "Jump to next attention terminal", key_style),
        key_line("F12", "Exit focus (back to Normal)", key_style),
        key_line("Shift+F1-F6", "Switch view (global, even while focused)", mod_style),
        Line::from(""),
        Line::from(Span::styled("  Scroll-Back Mode:", dim)),
        key_line("\u{2191}\u{2193}/j/k", "Scroll line by line", key_style),
        key_line("PgUp/PgDn", "Scroll by page", key_style),
        key_line("Home", "Jump to top of scrollback", key_style),
        key_line("End/Esc/q", "Exit scroll-back (return to live)", key_style),
        key_line("/", "Search in terminal output", key_style),
        key_line("n / N", "Next / previous search match", key_style),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled("Activity Feed (View 6)", bold)),
        Line::from(""),
        key_line("Ctrl+F4-F8", "Crew/Event/Tool filter, Auto-scroll, Gantt (see Ctrl layer)", mod_style),
        Line::from(""),
        Line::from(Span::styled("  Permission Queue (F8):", dim)),
        key_line("a", "Approve selected (send y)", key_style),
        key_line("d", "Deny selected (send n)", key_style),
        key_line("A", "Approve ALL pending terminals", key_style),
        key_line("t", "Type custom response to send", key_style),
        key_line("v / Enter", "View: switch to terminal", key_style),
        key_line("Esc", "Close popup", key_style),
        Line::from(""),
        Line::from(Span::styled("  Mouse (Terminal Panels):", dim)),
        key_line("Click+drag", "Select text (constrained to panel)", key_style),
        key_line("Scroll wheel", "Scroll-back (3 lines per tick)", key_style),
        key_line("Release", "Auto-copy selection to clipboard (OSC 52)", key_style),
        Line::from(""),
        Line::from(Span::styled("Quit", bold)),
        Line::from(""),
        key_line("q", "Quit application", key_style),
        key_line("Ctrl+C", "Quit application", key_style),
        Line::from(""),
        Line::from(Span::styled(kitty_note, dim)),
        Line::from(""),
        Line::from(Span::styled("Esc close  \u{2191}\u{2193}/PgUp/PgDn scroll", dim)),
    ];

    let total_lines = lines.len() as u16;
    let visible_lines = inner.height;
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let scroll_offset = app.help_scroll.min(max_scroll);

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

fn key_line<'a>(key: &'a str, desc: &'a str, key_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<16}", key), key_style),
        Span::raw(desc),
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
