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
    /// Agent finished and is showing an input prompt (e.g. `❯`).
    WaitingForInput,
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

    // Waiting-for-input detection: Claude Code shows ❯ (U+276F) as input prompt.
    // The last non-empty line being just the prompt char (with optional leading
    // whitespace or box-drawing decoration) means the agent is done and waiting.
    // NOTE: Only match standalone prompt characters — do NOT match lines that
    // merely end with ">" as that's extremely common in terminal output (HTML,
    // redirections, etc.) and causes rapid state flipping + excessive redraws.
    if let Some(last_line) = recent_lines.last().filter(|l| !l.is_empty()) {
        let trimmed = last_line.trim();
        if trimmed == "\u{276f}" || trimmed.ends_with("\u{276f}") {
            return Some(AttentionKind::WaitingForInput);
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
    // On Windows, strip the \\?\ extended-length path prefix from cwd.
    // This prefix (from canonicalize()) causes issues with Node.js/Bun/Git
    // file operations in the child process.
    let cwd = if cfg!(target_os = "windows") {
        let s = cwd.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            std::borrow::Cow::Owned(Path::new(stripped).to_path_buf())
        } else {
            std::borrow::Cow::Borrowed(cwd)
        }
    } else {
        std::borrow::Cow::Borrowed(cwd)
    };
    cmd.cwd(cwd.as_ref());
    // Pre-set terminal identification env vars so crossterm recognizes "WezTerm"
    // and skips feature probing (XTVERSION, DECRQM, DA2). Without these,
    // each unanswered probe times out (~5s), adding 10-15s to startup.
    cmd.env("TERM", "xterm-256color");
    cmd.env("TERM_PROGRAM", "WezTerm");
    cmd.env("TERM_PROGRAM_VERSION", "20240203");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("FORCE_COLOR", "3");
    if std::env::var("WT_SESSION").is_err() {
        cmd.env("WT_SESSION", "crew-board");
    }
    // Skip non-essential startup network calls
    cmd.env("DISABLE_AUTOUPDATER", "1");
    cmd.env("DO_NOT_TRACK", "1");
    cmd.env("CLAUDE_CODE_DISABLE_AUTOUPDATE", "1");
    // Set additional env vars (e.g. hook communication vars)
    for (key, val) in &env_vars {
        cmd.env(key, val);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .context("spawn child process")?;
    let child = Arc::new(Mutex::new(child));

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

    // On Windows, portable-pty 0.9 uses PSEUDOCONSOLE_INHERIT_CURSOR which causes
    // ConPTY to send ESC[6n (cursor position query) on the output pipe. ConPTY
    // deadlocks until it receives ESC[1;1R. The reader thread detects ESC[6n inline
    // and responds immediately. No proactive sending — that causes stdin garbage.

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
    // On Windows, ConPTY doesn't reliably send EOF on the output pipe when
    // the child exits. Spawn a separate thread that waits for the child process
    // to exit and sets the exit signal directly. This ensures poll_status()
    // detects the exit even if the reader thread is blocked on read().
    if cfg!(target_os = "windows") {
        let child_wait = Arc::clone(&child);
        let exit_signal_wait = Arc::clone(&exit_signal);
        std::thread::spawn(move || {
            let code = match child_wait.lock().unwrap().wait() {
                Ok(status) => status.exit_code() as i32,
                Err(_) => -1,
            };
            *exit_signal_wait.lock().unwrap() = Some(ExitEvent {
                code,
                timestamp: Instant::now(),
            });
        });
    }

    let writer_clone = Arc::clone(&pty_writer);
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
                    // Respond to terminal feature queries that the raw PTY can't answer.
                    // Without responses, Claude/Ink waits for each probe to timeout (~5-15s each).
                    // This affects both ConPTY (Windows) and raw PTYs (WSL/Linux) inside crew-board.
                    // Claude's Ink renderer sends:
                    //   1. ESC[>0q (XTVERSION) — asks terminal name
                    //   2. ESC[c (DA1 / Primary Device Attributes) — sentinel/flush
                    // It waits until DA1 responds before proceeding. Both must be answered.
                    {
                        let chunk = &buf[..n];

                        // ESC[6n — cursor position query
                        if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                            if let Ok(mut w) = writer_clone.lock() {
                                use std::io::Write;
                                let _ = w.write_all(b"\x1b[1;1R");
                                let _ = w.flush();
                            }
                        }

                        // ESC[>0q — XTVERSION query (Claude/Ink terminal detection)
                        // Claude sends XTVERSION then DA1 (ESC[c]) as sentinel.
                        // On ConPTY, it passes XTVERSION to us but swallows DA1 internally.
                        // On Linux/WSL PTY, neither gets answered by the PTY layer.
                        // Claude blocks in Promise.all until DA1 resolves.
                        // Send BOTH responses when we see XTVERSION — the DA1 response
                        // (ESC[?62;c) triggers the sentinel and unblocks startup.
                        if chunk.windows(5).any(|w| w == b"\x1b[>0q") {
                            if let Ok(mut w) = writer_clone.lock() {
                                use std::io::Write;
                                let _ = w.write_all(b"\x1bP>|crew-board\x1b\\");
                                let _ = w.write_all(b"\x1b[?62;c");
                                let _ = w.flush();
                            }
                        }

                        // ESC[c — DA1 (Primary Device Attributes) sent standalone
                        // On Linux/WSL, the PTY doesn't answer DA1 either. If Claude sends
                        // DA1 separately (not paired with XTVERSION), respond directly.
                        if chunk.windows(3).any(|w| w == b"\x1b[c") && !chunk.windows(5).any(|w| w == b"\x1b[>0q") {
                            if let Ok(mut w) = writer_clone.lock() {
                                use std::io::Write;
                                let _ = w.write_all(b"\x1b[?62;c");
                                let _ = w.flush();
                            }
                        }
                    }

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
        // On Windows, the wait thread may have already set the exit signal.
        // On Unix, we wait here. Either way, set the signal if not already set.
        if exit_signal_clone.lock().unwrap().is_none() {
            let code = match child.lock().unwrap().wait() {
                Ok(status) => status.exit_code() as i32,
                Err(_) => -1,
            };
            *exit_signal_clone.lock().unwrap() = Some(ExitEvent {
                code,
                timestamp: Instant::now(),
            });
        }
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
