//! Quick diagnostic: prints raw crossterm key events.
//! Run with: cargo run --example key_debug
//! Press Ctrl+C to quit.

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyboardEnhancementFlags,
            PushKeyboardEnhancementFlags, PopKeyboardEnhancementFlags},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use std::io;

fn main() {
    enable_raw_mode().unwrap();

    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();
    println!("TERM_PROGRAM={term_program:?}  TERM={term:?}\r");

    let supports = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    println!("supports_keyboard_enhancement() = {supports}\r");

    let kitty_ok = execute!(
        io::stdout(),
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        )
    )
    .is_ok();
    println!("PushKeyboardEnhancementFlags = {kitty_ok}\r");
    println!("\r");
    println!("Press keys to see events. Ctrl+C to quit.\r");
    println!("Try: Shift alone, F1, Shift+F1, Ctrl+F1\r");
    println!("---\r");

    loop {
        if event::poll(std::time::Duration::from_millis(100)).unwrap() {
            let ev = event::read().unwrap();
            match ev {
                Event::Key(key) => {
                    println!(
                        "kind={:?}  code={:?}  modifiers={:?}  state={:?}\r",
                        key.kind, key.code, key.modifiers, key.state
                    );
                    if key.kind == KeyEventKind::Press
                        && key.code == KeyCode::Char('c')
                        && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                    {
                        break;
                    }
                }
                _ => {
                    println!("event={ev:?}\r");
                }
            }
        }
    }

    if kitty_ok {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode().unwrap();
    println!();
}
