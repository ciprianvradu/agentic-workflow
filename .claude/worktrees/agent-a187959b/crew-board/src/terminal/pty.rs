//! PTY spawning logic for embedded terminals.
//!
//! Extracts the portable-pty setup from `examples/terminal_spike.rs` into a
//! reusable function that spawns a child process in a pseudoterminal and
//! returns shared handles for the parser, writer, and master PTY.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Shared vt100 parser handle.
pub type SharedParser = Arc<Mutex<vt100::Parser>>;
/// Shared PTY writer handle.
pub type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
/// Shared master PTY handle.
pub type SharedMaster = Arc<Mutex<Box<dyn MasterPty + Send>>>;

/// Notification that the child process has exited.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExitEvent {
    /// Exit code from the process (portable-pty returns u32, we store as i32 for
    /// compatibility with TerminalStatus::Exited(i32)).
    pub code: i32,
    /// When the exit was detected.
    pub timestamp: Instant,
}

/// Shared channel for exit notification (None = still running).
pub type SharedExitSignal = Arc<Mutex<Option<ExitEvent>>>;

/// Why a terminal needs attention.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AttentionKind {
    PermissionPrompt { line: String },
    Idle { seconds: u64 },
    Error { line: String },
}

/// Notification that a terminal needs user attention.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AttentionEvent {
    pub kind: AttentionKind,
    pub timestamp: Instant,
}

/// Shared channel for attention notifications (None = no attention needed).
pub type SharedAttentionSignal = Arc<Mutex<Option<AttentionEvent>>>;

/// All handles returned by `spawn_pty`.
pub struct PtyHandles {
    pub parser: SharedParser,
    pub writer: SharedWriter,
    pub master: SharedMaster,
    pub exit_signal: SharedExitSignal,
    pub attention_signal: SharedAttentionSignal,
    pub last_output: Arc<Mutex<Instant>>,
}

/// Scan the vt100 screen for patterns that suggest the terminal needs attention.
/// Returns Some(AttentionKind) if attention is needed, None otherwise.
fn scan_for_attention(parser: &Arc<Mutex<vt100::Parser>>) -> Option<AttentionKind> {
    let p = parser.lock().unwrap();
    let screen = p.screen();

    let (rows, cols) = screen.size();
    let rows = rows as usize;
    let cols = cols as usize;

    // Read all screen lines, then find the last non-empty line and scan
    // backwards from there. This handles tall terminals where the prompt
    // content sits above many empty rows.
    let mut all_lines: Vec<String> = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = String::with_capacity(cols);
        for col in 0..cols {
            if let Some(cell) = screen.cell(row as u16, col as u16) {
                if cell.has_contents() {
                    line.push_str(&cell.contents());
                } else {
                    line.push(' ');
                }
            } else {
                line.push(' ');
            }
        }
        all_lines.push(line.trim_end().to_string());
    }

    // Find the last non-empty line and take up to 10 lines ending there
    let last_nonempty = all_lines.iter().rposition(|l| !l.is_empty()).unwrap_or(0);
    let scan_start = last_nonempty.saturating_sub(9);
    let recent_lines: Vec<String> = all_lines[scan_start..=last_nonempty].to_vec();

    let combined = recent_lines.join("\n");
    let combined_lower = combined.to_lowercase();

    // Prompt detection:
    // Permission prompts, Claude Code questions, checkpoint menus
    if (combined_lower.contains("allow") && combined_lower.contains("deny"))
        || combined_lower.contains("(y/n)")
        || combined_lower.contains("(yes/no)")
        || combined_lower.contains("do you want to proceed")
        || combined_lower.contains("press enter to continue")
        || combined_lower.contains("enter to select")
        || combined_lower.contains("how would you like to proceed")
        || combined_lower.contains("tab to amend")
    {
        // Find the last non-empty line as context
        let context_line = recent_lines
            .iter()
            .rev()
            .find(|l| !l.is_empty())
            .cloned()
            .unwrap_or_default();
        return Some(AttentionKind::PermissionPrompt { line: context_line });
    }

    // Error detection (only on last 3 lines, more conservative)
    let error_lines: Vec<&String> = recent_lines.iter().rev().take(3).collect();
    for line in &error_lines {
        let lower = line.to_lowercase();
        if lower.contains("error:") || lower.contains("fatal:") || lower.contains("panic at") {
            return Some(AttentionKind::Error {
                line: line.to_string(),
            });
        }
    }

    None
}

/// Spawn a child process inside a PTY and return shared handles.
///
/// A background reader thread is started that feeds PTY output into the
/// vt100 parser. The thread exits automatically when the child process
/// closes (EOF on the reader), and sets the exit signal with the process
/// exit code.
#[allow(dead_code)]
pub fn spawn_pty(
    command: &str,
    args: &[String],
    cwd: &Path,
    rows: u16,
    cols: u16,
) -> Result<PtyHandles> {
    spawn_pty_with_log(command, args, cwd, rows, cols, None, vec![])
}

/// Spawn a child process inside a PTY with optional output logging.
///
/// If `log_path` is Some, PTY output bytes are also written to the given file
/// (appended). This provides a persistent log of terminal output.
///
/// `env_vars` are additional environment variables set on the child process
/// (in addition to the default `TERM=xterm-256color`).
#[allow(dead_code)]
pub fn spawn_pty_with_log(
    command: &str,
    args: &[String],
    cwd: &Path,
    rows: u16,
    cols: u16,
    log_path: Option<PathBuf>,
    env_vars: Vec<(String, String)>,
) -> Result<PtyHandles> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open pty")?;

    let mut cmd = CommandBuilder::new(command);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    // Set additional env vars (e.g. hook communication vars)
    for (key, val) in &env_vars {
        cmd.env(key, val);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn child process")?;

    // Drop the slave side in the parent -- we don't need it after spawning
    drop(pair.slave);

    // Wrap the master for shared access
    let pty_master: SharedMaster = Arc::new(Mutex::new(pair.master));

    // Get a writer for sending input to the PTY
    let pty_writer: SharedWriter = {
        let master = pty_master.lock().unwrap();
        let writer = master.take_writer().context("get pty writer")?;
        Arc::new(Mutex::new(writer))
    };

    // Get a reader for receiving PTY output
    let pty_reader = {
        let master = pty_master.lock().unwrap();
        master.try_clone_reader().context("get pty reader")?
    };

    // vt100 parser -- shared between the reader thread and the draw loop
    let parser: SharedParser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 10_000)));

    // Shared signals
    let exit_signal: SharedExitSignal = Arc::new(Mutex::new(None));
    let attention_signal: SharedAttentionSignal = Arc::new(Mutex::new(None));
    let last_output: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));

    // Spawn a background thread to read PTY output and feed it into vt100.
    // On EOF, waits for child exit code and writes the exit signal.
    let parser_clone = Arc::clone(&parser);
    let exit_signal_clone = Arc::clone(&exit_signal);
    let attention_signal_clone = Arc::clone(&attention_signal);
    let last_output_clone = Arc::clone(&last_output);
    std::thread::spawn(move || {
        let mut reader = pty_reader;
        let mut buf = [0u8; 4096];
        let mut last_scan = Instant::now();
        let scan_interval = std::time::Duration::from_millis(500);

        // Open log file if configured
        let mut log_file = log_path.and_then(|p| {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .ok()
        });

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF -- child exited
                Ok(n) => {
                    let mut p = parser_clone.lock().unwrap();
                    p.process(&buf[..n]);
                    drop(p); // release parser lock before updating timestamp
                    *last_output_clone.lock().unwrap() = Instant::now();

                    // Tee output to log file
                    if let Some(ref mut f) = log_file {
                        let _ = f.write_all(&buf[..n]);
                    }

                    // Throttled attention scanning (every 500ms)
                    if last_scan.elapsed() >= scan_interval {
                        last_scan = Instant::now();
                        let attn = scan_for_attention(&parser_clone).map(|kind| AttentionEvent {
                            kind,
                            timestamp: Instant::now(),
                        });
                        *attention_signal_clone.lock().unwrap() = attn;
                    }
                }
                Err(_) => break,
            }
        }

        // Reader finished -- collect exit code from child.
        // portable_pty::ExitStatus::exit_code() returns u32; cast to i32.
        let code = match child.wait() {
            Ok(status) => status.exit_code() as i32,
            Err(_) => -1,
        };
        *exit_signal_clone.lock().unwrap() = Some(ExitEvent {
            code,
            timestamp: Instant::now(),
        });
    });

    Ok(PtyHandles {
        parser,
        writer: pty_writer,
        master: pty_master,
        exit_signal,
        attention_signal,
        last_output,
    })
}
