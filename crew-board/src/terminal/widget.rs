//! Terminal rendering widget and keyboard encoding.
//!
//! Maps vt100 screen cells to ratatui spans with correct color and attribute
//! mapping. The rendering logic is extracted from `examples/terminal_spike.rs`.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::sync::{Arc, Mutex};

/// Map a vt100 color to a ratatui Color.
fn vt100_color_to_ratatui(color: vt100::Color) -> Option<Color> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(idx) => Some(Color::Indexed(idx)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

/// Resize the PTY and vt100 parser to match the actual render area.
/// Called every draw cycle to keep parser dimensions in sync with the widget.
pub fn resize_if_needed(
    parser: &Arc<Mutex<vt100::Parser>>,
    master: &Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    rows: u16,
    cols: u16,
) {
    if rows == 0 || cols == 0 {
        return;
    }
    let needs_resize = {
        let p = parser.lock().unwrap();
        p.screen().size() != (rows, cols)
    };
    if needs_resize {
        let _ = master.lock().unwrap().resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        parser.lock().unwrap().set_size(rows, cols);
    }
}

/// Query the actual number of scrollback lines available in the buffer.
/// Uses set_scrollback(MAX) + scrollback() to get the clamped value.
pub fn scrollback_available(parser: &Arc<Mutex<vt100::Parser>>) -> usize {
    let mut p = parser.lock().unwrap();
    let prev = p.screen().scrollback();
    p.set_scrollback(usize::MAX);
    let max = p.screen().scrollback();
    p.set_scrollback(prev);
    max
}

/// Range of cells selected by mouse (panel-relative coordinates).
pub struct SelectionRange {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

impl SelectionRange {
    /// Check if a cell at (row, col) falls within this selection.
    fn contains(&self, row: u16, col: u16) -> bool {
        // Normalize direction
        let (sr, sc, er, ec) = if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_row, self.start_col, self.end_row, self.end_col)
        } else {
            (self.end_row, self.end_col, self.start_row, self.start_col)
        };

        if row < sr || row > er {
            return false;
        }
        if sr == er {
            // Single line selection
            return col >= sc && col <= ec;
        }
        if row == sr {
            return col >= sc;
        }
        if row == er {
            return col <= ec;
        }
        // Middle rows are fully selected
        true
    }
}

/// Render the vt100 screen state into the given frame area.
///
/// When `show_cursor` is true, the cursor position is highlighted with a
/// reversed style to make it visible.
///
/// `scroll_offset` > 0 enables scrollback mode: the view shifts up by that
/// many lines into the scrollback buffer.
///
/// `selection` highlights cells in the given range with a reversed style.
pub fn draw_terminal(
    frame: &mut Frame,
    area: Rect,
    parser: &Arc<Mutex<vt100::Parser>>,
    show_cursor: bool,
    scroll_offset: usize,
    selection: Option<&SelectionRange>,
) {
    let mut p = parser.lock().unwrap();

    // Temporarily set scrollback offset for rendering
    let prev_scrollback = p.screen().scrollback();
    if scroll_offset != prev_scrollback {
        p.set_scrollback(scroll_offset);
    }

    let screen = p.screen();
    let rows = area.height as usize;
    let cols = area.width as usize;

    let cursor_pos = if show_cursor && scroll_offset == 0 {
        let pos = screen.cursor_position();
        Some((pos.0 as usize, pos.1 as usize))
    } else {
        None
    };

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
                    (
                        ch,
                        c.fgcolor(),
                        c.bgcolor(),
                        c.bold(),
                        c.italic(),
                        c.underline(),
                        c.inverse(),
                    )
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

            // Highlight cursor position
            if let Some((cr, cc)) = cursor_pos {
                if row == cr && col == cc {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            }

            // Highlight selected cells
            if let Some(sel) = selection {
                if sel.contains(row as u16, col as u16) {
                    style = Style::default()
                        .fg(Color::White)
                        .bg(Color::Indexed(24)); // deep blue selection
                }
            }

            spans.push(Span::styled(ch, style));
        }

        lines.push(Line::from(spans));
    }

    // Restore scrollback offset
    if scroll_offset != prev_scrollback {
        p.set_scrollback(prev_scrollback);
    }

    drop(p); // release lock before rendering

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Compute the xterm modifier parameter: 1 + (shift?1:0) + (alt?2:0) + (ctrl?4:0).
/// Returns 0 when no modifiers are set (meaning: omit the parameter).
fn xterm_mod_param(modifiers: KeyModifiers) -> u8 {
    let mut m: u8 = 0;
    if modifiers.contains(KeyModifiers::SHIFT) {
        m += 1;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        m += 2;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        m += 4;
    }
    if m > 0 { m + 1 } else { 0 }
}

/// Encode a crossterm key event into the byte sequence expected by the PTY.
///
/// Supports modifier-aware encoding for special keys:
/// - Arrows: `\x1b[1;{mod}A` (e.g. Ctrl+Right = `\x1b[1;5C` for word-forward)
/// - Home/End: `\x1b[1;{mod}H` / `\x1b[1;{mod}F`
/// - Tilde keys (PgUp, Delete, Insert, F5+): `\x1b[N;{mod}~`
/// - F1-F4 with modifiers: `\x1b[1;{mod}P` (CSI form instead of SS3)
pub fn key_to_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    let m = xterm_mod_param(modifiers);

    // Ctrl+key — character control codes (Ctrl+A=0x01 .. Ctrl+Z=0x1a)
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = code {
            // Some terminals (kitty protocol) report Ctrl+Enter as Char('\r') or Char('\n')
            // instead of KeyCode::Enter. Route these to the CSI u encoding below.
            if c == '\r' || c == '\n' {
                return format!("\x1b[13;{}u", m).into_bytes();
            }
            let ctrl = (c.to_ascii_lowercase() as u8)
                .wrapping_sub(b'a')
                .wrapping_add(1);
            if ctrl <= 26 {
                return vec![ctrl];
            }
            return vec![];
        }
        if code == KeyCode::Char('[') {
            return vec![0x1b];
        }
    }

    // Alt+key — send ESC prefix before the character
    if modifiers.contains(KeyModifiers::ALT) && !modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = code {
            let mut buf = vec![0x1b]; // ESC prefix
            let mut ch_buf = [0u8; 4];
            buf.extend_from_slice(c.encode_utf8(&mut ch_buf).as_bytes());
            return buf;
        }
    }

    match code {
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }
        // Enter/Backspace/Tab: use CSI u encoding when modifiers are held
        // so apps like Claude Code can distinguish Ctrl+Enter (newline) from Enter (submit)
        KeyCode::Enter => {
            if m > 0 {
                format!("\x1b[13;{}u", m).into_bytes()
            } else {
                b"\r".to_vec()
            }
        }
        KeyCode::Backspace => {
            if m > 0 {
                format!("\x1b[127;{}u", m).into_bytes()
            } else {
                b"\x7f".to_vec()
            }
        }
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),

        // Arrow keys: \x1b[A or \x1b[1;{mod}A
        KeyCode::Up => csi_final(b'A', m),
        KeyCode::Down => csi_final(b'B', m),
        KeyCode::Right => csi_final(b'C', m),
        KeyCode::Left => csi_final(b'D', m),

        // Home/End: \x1b[H or \x1b[1;{mod}H
        KeyCode::Home => csi_final(b'H', m),
        KeyCode::End => csi_final(b'F', m),

        // Tilde keys: \x1b[N~ or \x1b[N;{mod}~
        KeyCode::Insert => csi_tilde(2, m),
        KeyCode::Delete => csi_tilde(3, m),
        KeyCode::PageUp => csi_tilde(5, m),
        KeyCode::PageDown => csi_tilde(6, m),

        // F-keys: F1-F4 use SS3 (no mod) or CSI 1;mod (with mod)
        KeyCode::F(1) => fkey_ss3(b'P', m),
        KeyCode::F(2) => fkey_ss3(b'Q', m),
        KeyCode::F(3) => fkey_ss3(b'R', m),
        KeyCode::F(4) => fkey_ss3(b'S', m),
        // F5+ use tilde encoding
        KeyCode::F(5) => csi_tilde(15, m),
        KeyCode::F(6) => csi_tilde(17, m),
        KeyCode::F(7) => csi_tilde(18, m),
        KeyCode::F(8) => csi_tilde(19, m),
        KeyCode::F(9) => csi_tilde(20, m),
        KeyCode::F(10) => csi_tilde(21, m),
        KeyCode::F(11) => csi_tilde(23, m),
        KeyCode::F(12) => csi_tilde(24, m),
        KeyCode::F(_) => vec![], // F13+ not standard
        // Fallback: anything else — don't silently drop
        _ => vec![],
    }
}

/// CSI sequence with a final byte: `\x1b[A` or `\x1b[1;{mod}A`.
fn csi_final(final_byte: u8, m: u8) -> Vec<u8> {
    if m == 0 {
        vec![0x1b, b'[', final_byte]
    } else {
        format!("\x1b[1;{}{}", m, final_byte as char)
            .into_bytes()
    }
}

/// CSI tilde sequence: `\x1b[N~` or `\x1b[N;{mod}~`.
fn csi_tilde(n: u8, m: u8) -> Vec<u8> {
    if m == 0 {
        format!("\x1b[{}~", n).into_bytes()
    } else {
        format!("\x1b[{};{}~", n, m).into_bytes()
    }
}

/// F1-F4: SS3 without modifier (`\x1bOP`), CSI with modifier (`\x1b[1;{mod}P`).
fn fkey_ss3(letter: u8, m: u8) -> Vec<u8> {
    if m == 0 {
        vec![0x1b, b'O', letter]
    } else {
        format!("\x1b[1;{}{}", m, letter as char).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn test_plain_enter() {
        assert_eq!(key_to_bytes(KeyCode::Enter, KeyModifiers::NONE), b"\r");
    }

    #[test]
    fn test_ctrl_enter_keycode() {
        // When crossterm reports Ctrl+Enter as KeyCode::Enter
        let bytes = key_to_bytes(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(bytes, b"\x1b[13;5u");
    }

    #[test]
    fn test_ctrl_enter_as_char_cr() {
        // When crossterm (kitty protocol) reports Ctrl+Enter as Char('\r')
        let bytes = key_to_bytes(KeyCode::Char('\r'), KeyModifiers::CONTROL);
        assert_eq!(bytes, b"\x1b[13;5u");
    }

    #[test]
    fn test_ctrl_enter_as_char_lf() {
        // When crossterm (kitty protocol) reports Ctrl+Enter as Char('\n')
        let bytes = key_to_bytes(KeyCode::Char('\n'), KeyModifiers::CONTROL);
        assert_eq!(bytes, b"\x1b[13;5u");
    }

    #[test]
    fn test_ctrl_a() {
        assert_eq!(key_to_bytes(KeyCode::Char('a'), KeyModifiers::CONTROL), vec![1]);
    }

    #[test]
    fn test_ctrl_c() {
        assert_eq!(key_to_bytes(KeyCode::Char('c'), KeyModifiers::CONTROL), vec![3]);
    }

    #[test]
    fn test_ctrl_right_arrow() {
        let bytes = key_to_bytes(KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(bytes, b"\x1b[1;5C");
    }
}
