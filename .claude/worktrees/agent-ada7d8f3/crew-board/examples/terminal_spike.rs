//! TUI-in-TUI rendering PoC spike for crew-board.
//!
//! Spawns a CLI program in a portable-pty pseudoterminal, feeds PTY output
//! through vt100::Parser to maintain a virtual screen buffer, and renders the
//! vt100 screen as a ratatui widget with correct color/attribute mapping.
//!
//! Usage:
//!   cargo run --example terminal_spike              # bash (default)
//!   cargo run --example terminal_spike -- vim       # test vim rendering
//!   cargo run --example terminal_spike -- htop      # test htop rendering
//!   cargo run --example terminal_spike -- claude    # test Claude Code rendering

use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

// ---------------------------------------------------------------------------
// Color mapping
// ---------------------------------------------------------------------------

/// Map a vt100 color to a ratatui Color.
fn vt100_color_to_ratatui(color: vt100::Color) -> Option<Color> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(idx) => Some(Color::Indexed(idx)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

// ---------------------------------------------------------------------------
// Keyboard input encoding
// ---------------------------------------------------------------------------

/// Encode a crossterm key event into the byte sequence expected by the PTY.
fn key_to_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    // Ctrl+key — both uppercase and lowercase
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = code {
            let ctrl = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a').wrapping_add(1);
            if ctrl <= 26 {
                return vec![ctrl];
            }
            return vec![];
        }
        // Ctrl+[ → ESC
        if code == KeyCode::Char('[') {
            return vec![0x1b];
        }
    }

    match code {
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        // F-keys — correct VT sequences (not n+10)
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(5) => b"\x1b[15~".to_vec(),
        KeyCode::F(6) => b"\x1b[17~".to_vec(),
        KeyCode::F(7) => b"\x1b[18~".to_vec(),
        KeyCode::F(8) => b"\x1b[19~".to_vec(),
        KeyCode::F(9) => b"\x1b[20~".to_vec(),
        KeyCode::F(10) => b"\x1b[21~".to_vec(),
        KeyCode::F(11) => b"\x1b[23~".to_vec(),
        KeyCode::F(12) => b"\x1b[24~".to_vec(),
        KeyCode::F(_) => vec![], // F13+ not commonly used
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Draw helpers
// ---------------------------------------------------------------------------

/// Render the vt100 screen state into the given frame area.
fn draw_terminal_widget(frame: &mut Frame, area: ratatui::layout::Rect, parser: &Arc<Mutex<vt100::Parser>>) {
    let p = parser.lock().unwrap();
    let screen = p.screen();

    let rows = area.height as usize;
    let cols = area.width as usize;

    let mut lines: Vec<Line> = Vec::with_capacity(rows);

    for row in 0..rows {
        let mut spans: Vec<Span> = Vec::with_capacity(cols);

        for col in 0..cols {
            let cell = screen.cell(row as u16, col as u16);
            let (ch, fg_color, bg_color, bold, italic, underline, inverse) = match cell {
                Some(c) => {
                    let ch = if c.has_contents() {
                        c.contents()
                    } else {
                        " ".to_string()
                    };
                    (ch, c.fgcolor(), c.bgcolor(), c.bold(), c.italic(), c.underline(), c.inverse())
                }
                None => (
                    " ".to_string(),
                    vt100::Color::Default,
                    vt100::Color::Default,
                    false,
                    false,
                    false,
                    false,
                ),
            };

            let mut style = Style::default();
            if let Some(fg) = vt100_color_to_ratatui(fg_color) {
                style = style.fg(fg);
            }
            if let Some(bg) = vt100_color_to_ratatui(bg_color) {
                style = style.bg(bg);
            }
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if inverse {
                style = style.add_modifier(Modifier::REVERSED);
            }

            spans.push(Span::styled(ch, style));
        }

        lines.push(Line::from(spans));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

// ---------------------------------------------------------------------------
// Main event loop
// ---------------------------------------------------------------------------

/// Shared writer type for the PTY stdin.
type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;

/// Shared master PTY handle for resize operations.
type PtyMaster = Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>;

fn run_spike(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    parser: Arc<Mutex<vt100::Parser>>,
    pty_writer: PtyWriter,
    pty_master: PtyMaster,
    program: &str,
) -> Result<()> {
    loop {
        // Draw frame
        terminal.draw(|frame| {
            let size = frame.area();

            // Layout: title bar (1) | terminal area (fill) | status bar (1)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(size);

            // Title bar
            let title = Paragraph::new(format!(" terminal_spike — {} ", program))
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            frame.render_widget(title, chunks[0]);

            // Terminal area with border
            let border_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" PTY ");
            let inner = border_block.inner(chunks[1]);
            frame.render_widget(border_block, chunks[1]);

            // Render vt100 screen into the inner area
            draw_terminal_widget(frame, inner, &parser);

            // Status bar — show cursor position from vt100 screen
            let (cur_row, cur_col) = {
                let p = parser.lock().unwrap();
                let pos = p.screen().cursor_position();
                (pos.0, pos.1)
            };
            let status = Paragraph::new(format!(
                " Ctrl-Q to quit | cursor ({}, {}) | inner {}x{}",
                cur_row, cur_col, inner.width, inner.height
            ))
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(status, chunks[2]);
        })?;

        // Poll for events with a short timeout so the screen refreshes from PTY output
        if !event::poll(Duration::from_millis(16))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                // Ctrl-Q exits the spike
                if key.code == KeyCode::Char('q')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    break;
                }

                let bytes = key_to_bytes(key.code, key.modifiers);
                if !bytes.is_empty() {
                    let mut writer = pty_writer.lock().unwrap();
                    let _ = writer.write_all(&bytes);
                    let _ = writer.flush();
                }
            }

            Event::Resize(cols, rows) => {
                // Account for border (2 cols, 2 rows) + title bar (1) + status bar (1)
                let pty_rows = rows.saturating_sub(4);
                let pty_cols = cols.saturating_sub(2);
                let new_size = PtySize {
                    rows: pty_rows,
                    cols: pty_cols,
                    pixel_width: 0,
                    pixel_height: 0,
                };
                let _ = pty_master.lock().unwrap().resize(new_size);
                // Resize the vt100 parser to match
                let mut p = parser.lock().unwrap();
                p.set_size(pty_rows, pty_cols);
            }

            Event::Mouse(_) => {}
            Event::FocusGained | Event::FocusLost => {}
            Event::Paste(_) => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    // CLI args: first positional arg is the program to spawn (default: bash)
    let args: Vec<String> = std::env::args().collect();
    let program = args.get(1).map(|s| s.as_str()).unwrap_or("bash");

    // Query the current terminal size so the PTY starts at the right dimensions.
    let (term_cols, term_rows) = crossterm::terminal::size().context("query terminal size")?;

    // Reserve rows/cols for our chrome: title(1) + border top/bottom(2) + status(1) = 4 rows,
    // border left/right = 2 cols.
    let pty_rows = term_rows.saturating_sub(4);
    let pty_cols = term_cols.saturating_sub(2);

    // Set up the PTY
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: pty_rows,
            cols: pty_cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open pty")?;

    // Set TERM so programs know what kind of terminal they're in
    let mut cmd = CommandBuilder::new(program);
    cmd.env("TERM", "xterm-256color");

    // Spawn the child process inside the PTY
    let _child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn child process")?;

    // Drop the slave side in the parent — we don't need it after spawning
    drop(pair.slave);

    // Wrap the master for shared access
    let pty_master: PtyMaster = Arc::new(Mutex::new(pair.master));

    // Get a writer for sending input to the PTY
    let pty_writer: PtyWriter = {
        let master = pty_master.lock().unwrap();
        let writer = master.take_writer().context("get pty writer")?;
        Arc::new(Mutex::new(writer))
    };

    // Get a reader for receiving PTY output
    let pty_reader = {
        let master = pty_master.lock().unwrap();
        master.try_clone_reader().context("get pty reader")?
    };

    // vt100 parser — shared between the reader thread and the draw loop
    let parser: Arc<Mutex<vt100::Parser>> = Arc::new(Mutex::new(vt100::Parser::new(
        pty_rows,
        pty_cols,
        0, // scrollback lines (0 = no scrollback buffer)
    )));

    // Spawn a background thread to read PTY output and feed it into vt100
    let parser_clone = Arc::clone(&parser);
    std::thread::spawn(move || {
        let mut reader = pty_reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — child exited
                Ok(n) => {
                    let mut p = parser_clone.lock().unwrap();
                    p.process(&buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    // Set up ratatui with crossterm
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    // Run the event loop
    let result = run_spike(&mut terminal, parser, pty_writer, pty_master, program);

    // Restore the terminal regardless of whether the loop errored
    disable_raw_mode().context("disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")?;

    result
}
