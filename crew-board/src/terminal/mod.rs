//! Terminal multiplexer supporting both embedded PTY and headless terminals.
//!
//! Manages multiple terminals that can be rendered inside the TUI.
//! Embedded terminals run a child process in a pseudoterminal, with output
//! parsed by vt100 and rendered via ratatui.
//! Headless terminals run a child process without a PTY, tracked via
//! `std::process::Child` for exit detection and lifecycle management.

pub mod pty;
pub mod widget;

use anyhow::Result;
use portable_pty::MasterPty;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Unique identifier for an embedded terminal (typically a task_id like "TASK_001").
pub type TerminalId = String;

/// State of an embedded terminal.
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalStatus {
    Running,
    NeedsAttention(AttentionReason),
    Exited(i32),
}

/// Reason a terminal needs attention.
#[derive(Debug, Clone, PartialEq)]
pub enum AttentionReason {
    PermissionPrompt { context: String },
    Idle { seconds: u64 },
    Error { context: String },
    /// Attention triggered by a Claude Code Notification hook event.
    HookNotification { message: String },
    /// Agent finished its turn and is waiting for user input.
    WaitingForInput,
}

/// Hook-based activity and status tracking for an embedded terminal.
///
/// Populated from Claude Code HTTP hook events (Phase 1: observational only).
#[derive(Debug, Clone)]
pub struct HookState {
    /// Name of the last event received (e.g. "PreToolUse").
    pub last_event: String,
    /// When the last event was received.
    pub last_event_at: Instant,
    /// Current tool activity label (e.g. "Edit src/main.rs").
    /// Set on PreToolUse, cleared on PostToolUse.
    pub activity_label: String,
    /// Cumulative tool usage counts (tool_name → count).
    /// Incremented on each PostToolUse event.
    pub tool_counts: HashMap<String, u32>,
    /// Whether a Claude Code session is currently active.
    pub session_active: bool,
    /// Total cost in USD accumulated across all sessions for this terminal.
    pub total_cost_usd: f64,
    /// Total input tokens accumulated across all sessions.
    pub total_input_tokens: u64,
    /// Total output tokens accumulated across all sessions.
    pub total_output_tokens: u64,
}

/// Stored launch parameters for relaunching a terminal.
#[derive(Debug, Clone)]
pub struct LaunchParams {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// PTY-specific state for embedded terminals.
/// Groups all fields that only exist when a PTY is allocated.
pub struct PtyState {
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// Shared exit signal from the reader thread.
    pub exit_signal: pty::SharedExitSignal,
    /// Shared attention signal from the reader thread.
    pub attention_signal: pty::SharedAttentionSignal,
    /// Timestamp of last PTY output (for idle detection).
    pub last_output: Arc<Mutex<Instant>>,
}

/// Headless-specific state for terminals running without a PTY.
/// The child process is tracked for exit detection and lifecycle management.
pub struct HeadlessState {
    pub child: Child,
}

/// Discriminates between PTY-backed (embedded) and headless terminal kinds.
pub enum TerminalKind {
    /// PTY-backed terminal with full interactive I/O.
    Embedded(PtyState),
    /// Headless terminal — no PTY, tracked via `std::process::Child`.
    Headless(HeadlessState),
}

/// A single terminal (embedded PTY or headless child process).
pub struct EmbeddedTerminal {
    pub id: TerminalId,
    pub label: String,
    /// Terminal kind: Embedded (PTY) or Headless (Child process).
    pub kind: TerminalKind,
    pub status: TerminalStatus,
    pub color_scheme_index: Option<usize>,
    /// Original launch parameters for relaunch.
    pub launch_params: LaunchParams,
    /// Scroll-back offset (0 = live view, >0 = scrolled back N lines).
    pub scroll_offset: usize,
    /// Timestamp when this terminal was spawned.
    pub spawned_at: Instant,
    /// Hook-based activity state (Some if hook communication is active).
    pub hook_state: Option<HookState>,
    /// The cwd that was written to .claude/settings.local.json (for cleanup).
    pub hook_settings_cwd: Option<PathBuf>,
    /// Additional hook config files to clean up on terminal dismiss (for non-Claude hosts).
    #[allow(dead_code)]
    pub hook_cleanup_paths: Vec<PathBuf>,
    /// Per-terminal auto-accept toggle. When true, permission prompts for this
    /// terminal are automatically approved (overrides global permission_profile).
    /// Toggled via 'a' key in Terminals view Normal mode. Default: false.
    pub auto_accept: bool,
}

impl EmbeddedTerminal {
    /// Returns `true` if this is an embedded (PTY-backed) terminal.
    pub fn is_embedded(&self) -> bool {
        matches!(self.kind, TerminalKind::Embedded(_))
    }

    /// Returns `true` if this is a headless terminal.
    pub fn is_headless(&self) -> bool {
        matches!(self.kind, TerminalKind::Headless(_))
    }

    /// Returns the PTY parser if this is an embedded terminal.
    pub fn parser(&self) -> Option<&Arc<Mutex<vt100::Parser>>> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.parser),
            TerminalKind::Headless(_) => None,
        }
    }

    /// Returns the PTY writer if this is an embedded terminal.
    #[allow(dead_code)]
    pub fn writer(&self) -> Option<&Arc<Mutex<Box<dyn Write + Send>>>> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.writer),
            TerminalKind::Headless(_) => None,
        }
    }

    /// Returns the PTY master handle if this is an embedded terminal.
    pub fn master(&self) -> Option<&Arc<Mutex<Box<dyn MasterPty + Send>>>> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.master),
            TerminalKind::Headless(_) => None,
        }
    }

    /// Returns the exit signal if this is an embedded terminal.
    #[allow(dead_code)]
    pub fn exit_signal(&self) -> Option<&pty::SharedExitSignal> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.exit_signal),
            TerminalKind::Headless(_) => None,
        }
    }

    /// Returns the attention signal if this is an embedded terminal.
    #[allow(dead_code)]
    pub fn attention_signal(&self) -> Option<&pty::SharedAttentionSignal> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.attention_signal),
            TerminalKind::Headless(_) => None,
        }
    }

    /// Returns the last output timestamp if this is an embedded terminal.
    #[allow(dead_code)]
    pub fn last_output(&self) -> Option<&Arc<Mutex<Instant>>> {
        match &self.kind {
            TerminalKind::Embedded(pty) => Some(&pty.last_output),
            TerminalKind::Headless(_) => None,
        }
    }
}

/// Manages all embedded terminals.
pub struct TerminalManager {
    pub terminals: Vec<EmbeddedTerminal>,
    pub focused: usize,
}

impl TerminalManager {
    /// Create a new empty terminal manager.
    pub fn new() -> Self {
        TerminalManager {
            terminals: Vec::new(),
            focused: 0,
        }
    }

    /// Spawn a new embedded terminal with the given command.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        id: TerminalId,
        label: String,
        command: &str,
        args: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        color_scheme_index: Option<usize>,
    ) -> Result<()> {
        self.spawn_with_log(id, label, command, args, cwd, rows, cols, color_scheme_index, None)
    }

    /// Spawn a new embedded terminal with optional output logging.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_log(
        &mut self,
        id: TerminalId,
        label: String,
        command: &str,
        args: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        color_scheme_index: Option<usize>,
        log_path: Option<std::path::PathBuf>,
    ) -> Result<()> {
        self.spawn_with_log_and_env(id, label, command, args, cwd, rows, cols, color_scheme_index, log_path, vec![])
    }

    /// Spawn a new embedded terminal with optional output logging and extra env vars.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_log_and_env(
        &mut self,
        id: TerminalId,
        label: String,
        command: &str,
        args: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        color_scheme_index: Option<usize>,
        log_path: Option<std::path::PathBuf>,
        env_vars: Vec<(String, String)>,
    ) -> Result<()> {
        let handles = pty::spawn_pty_with_log(command, args, cwd, rows, cols, log_path, env_vars)?;

        self.terminals.push(EmbeddedTerminal {
            id,
            label,
            kind: TerminalKind::Embedded(PtyState {
                parser: handles.parser,
                writer: handles.writer,
                master: handles.master,
                exit_signal: handles.exit_signal,
                attention_signal: handles.attention_signal,
                last_output: handles.last_output,
            }),
            status: TerminalStatus::Running,
            color_scheme_index,
            launch_params: LaunchParams {
                command: command.to_string(),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
            },
            scroll_offset: 0,
            spawned_at: Instant::now(),
            hook_state: None,
            hook_settings_cwd: None,
            hook_cleanup_paths: Vec::new(),
            auto_accept: false,
        });

        // Focus the newly spawned terminal
        self.focused = self.terminals.len() - 1;

        Ok(())
    }

    /// Spawn a headless terminal (no PTY) with the given command.
    ///
    /// The child process runs with stdin=null and stdout/stderr=null.
    /// Exit detection is via `child.try_wait()` in `poll_status()`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_headless(
        &mut self,
        id: TerminalId,
        label: String,
        command: &str,
        args: &[String],
        cwd: &Path,
        color_scheme_index: Option<usize>,
        env_vars: Vec<(String, String)>,
    ) -> Result<()> {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        for (key, val) in &env_vars {
            cmd.env(key, val);
        }

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!("Failed to spawn headless process '{}': {}", command, e)
        })?;

        self.terminals.push(EmbeddedTerminal {
            id,
            label,
            kind: TerminalKind::Headless(HeadlessState { child }),
            status: TerminalStatus::Running,
            color_scheme_index,
            launch_params: LaunchParams {
                command: command.to_string(),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
            },
            scroll_offset: 0,
            spawned_at: Instant::now(),
            hook_state: None,
            hook_settings_cwd: None,
            hook_cleanup_paths: Vec::new(),
            auto_accept: false,
        });

        // Focus the newly spawned terminal
        self.focused = self.terminals.len() - 1;

        Ok(())
    }

    /// Relaunch a terminal that has exited, reusing the same parameters.
    /// Preserves terminal kind: embedded terminals relaunch as embedded,
    /// headless terminals relaunch as headless.
    /// Returns Ok(true) if relaunched, Ok(false) if not exited.
    pub fn relaunch(&mut self, id: &str, rows: u16, cols: u16) -> Result<bool> {
        let idx = match self.terminals.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => return Ok(false),
        };

        if !matches!(self.terminals[idx].status, TerminalStatus::Exited(_)) {
            return Ok(false);
        }

        let term = &self.terminals[idx];
        let params = term.launch_params.clone();
        let label = term.label.clone();
        let terminal_id = term.id.clone();
        let color_idx = term.color_scheme_index;
        let was_headless = term.is_headless();

        // Remove old terminal
        self.terminals.remove(idx);

        // Spawn fresh (preserving kind)
        if was_headless {
            self.spawn_headless(
                terminal_id,
                label,
                &params.command,
                &params.args,
                &params.cwd,
                color_idx,
                vec![],
            )?;
        } else {
            self.spawn(
                terminal_id,
                label,
                &params.command,
                &params.args,
                &params.cwd,
                rows,
                cols,
                color_idx,
            )?;
        }

        Ok(true)
    }

    /// Remove a terminal by id.
    pub fn remove(&mut self, id: &str) {
        self.terminals.retain(|t| t.id != id);
        // Clamp focus index
        if !self.terminals.is_empty() {
            if self.focused >= self.terminals.len() {
                self.focused = self.terminals.len() - 1;
            }
        } else {
            self.focused = 0;
        }
    }

    /// Get the currently focused terminal.
    pub fn focused_terminal(&self) -> Option<&EmbeddedTerminal> {
        self.terminals.get(self.focused)
    }

    /// Get the currently focused terminal mutably.
    #[allow(dead_code)]
    pub fn focused_terminal_mut(&mut self) -> Option<&mut EmbeddedTerminal> {
        self.terminals.get_mut(self.focused)
    }

    /// Move focus to the next terminal.
    pub fn focus_next(&mut self) {
        if !self.terminals.is_empty() {
            self.focused = (self.focused + 1) % self.terminals.len();
        }
    }

    /// Move focus to the previous terminal.
    pub fn focus_prev(&mut self) {
        if !self.terminals.is_empty() {
            if self.focused == 0 {
                self.focused = self.terminals.len() - 1;
            } else {
                self.focused -= 1;
            }
        }
    }

    /// Move focus to the next non-exited terminal (wraps). Falls back to regular next.
    pub fn focus_next_running(&mut self) {
        if self.terminals.is_empty() {
            return;
        }
        let len = self.terminals.len();
        for offset in 1..=len {
            let idx = (self.focused + offset) % len;
            if !matches!(self.terminals[idx].status, TerminalStatus::Exited(_)) {
                self.focused = idx;
                return;
            }
        }
        // All exited — just move normally
        self.focus_next();
    }

    /// Move focus to the previous non-exited terminal (wraps). Falls back to regular prev.
    pub fn focus_prev_running(&mut self) {
        if self.terminals.is_empty() {
            return;
        }
        let len = self.terminals.len();
        for offset in 1..=len {
            let idx = (self.focused + len - offset) % len;
            if !matches!(self.terminals[idx].status, TerminalStatus::Exited(_)) {
                self.focused = idx;
                return;
            }
        }
        self.focus_prev();
    }

    /// Jump focus to the next terminal that needs attention.
    /// Returns true if focus was moved.
    pub fn focus_next_attention(&mut self) -> bool {
        if self.terminals.is_empty() {
            return false;
        }

        let len = self.terminals.len();
        for offset in 1..=len {
            let idx = (self.focused + offset) % len;
            if matches!(
                self.terminals[idx].status,
                TerminalStatus::NeedsAttention(_)
            ) {
                self.focused = idx;
                return true;
            }
        }
        false
    }

    /// Send input bytes to the currently focused terminal.
    /// For embedded terminals, writes to the PTY and clears attention signal.
    /// For headless terminals, this is a no-op (returns Ok).
    pub fn send_input(&self, bytes: &[u8]) -> Result<()> {
        if let Some(term) = self.focused_terminal() {
            match &term.kind {
                TerminalKind::Embedded(pty) => {
                    let mut writer = pty.writer.lock().unwrap();
                    writer.write_all(bytes)?;
                    writer.flush()?;
                    // Clear attention -- user is actively interacting
                    *pty.attention_signal.lock().unwrap() = None;
                }
                TerminalKind::Headless(_) => {
                    // No-op: headless terminals have no stdin
                }
            }
        }
        Ok(())
    }

    /// Poll all terminals for status changes (exit, attention).
    /// Call this once per event loop tick.
    /// Returns true if any terminal changed status.
    pub fn poll_status(&mut self) -> bool {
        let mut changed = false;

        for term in &mut self.terminals {
            // Skip terminals that already have a final status
            if matches!(term.status, TerminalStatus::Exited(_)) {
                continue;
            }

            match &mut term.kind {
                TerminalKind::Embedded(pty) => {
                    // Check exit signal
                    let exit_event = pty.exit_signal.lock().unwrap().clone();
                    if let Some(event) = exit_event {
                        term.status = TerminalStatus::Exited(event.code);
                        changed = true;
                        continue; // Exit takes precedence over attention
                    }

                    // Check for idle (no output for 120 seconds)
                    let idle_threshold = std::time::Duration::from_secs(120);
                    let last_out = *pty.last_output.lock().unwrap();
                    if last_out.elapsed() >= idle_threshold {
                        let seconds = last_out.elapsed().as_secs();
                        // Only set idle if not already flagged for a higher-priority reason
                        let current_attn = pty.attention_signal.lock().unwrap().clone();
                        if current_attn.is_none() {
                            *pty.attention_signal.lock().unwrap() = Some(pty::AttentionEvent {
                                kind: pty::AttentionKind::Idle { seconds },
                                timestamp: Instant::now(),
                            });
                        }
                    }

                    // Check attention signal
                    let attn_event = pty.attention_signal.lock().unwrap().clone();
                    if let Some(event) = attn_event {
                        let reason = match &event.kind {
                            pty::AttentionKind::PermissionPrompt { line } => {
                                AttentionReason::PermissionPrompt { context: line.clone() }
                            }
                            pty::AttentionKind::Idle { seconds } => {
                                AttentionReason::Idle { seconds: *seconds }
                            }
                            pty::AttentionKind::Error { line } => {
                                AttentionReason::Error { context: line.clone() }
                            }
                            pty::AttentionKind::WaitingForInput => {
                                AttentionReason::WaitingForInput
                            }
                        };
                        if term.status != TerminalStatus::NeedsAttention(reason.clone()) {
                            term.status = TerminalStatus::NeedsAttention(reason);
                            changed = true;
                        }
                    } else if matches!(term.status, TerminalStatus::NeedsAttention(_)) {
                        // Attention was cleared
                        term.status = TerminalStatus::Running;
                        changed = true;
                    }
                }
                TerminalKind::Headless(headless) => {
                    // Check if the headless child has exited via try_wait()
                    match headless.child.try_wait() {
                        Ok(Some(exit_status)) => {
                            let code = exit_status.code().unwrap_or(-1);
                            term.status = TerminalStatus::Exited(code);
                            changed = true;
                        }
                        Ok(None) => {
                            // Still running — nothing to do
                        }
                        Err(_) => {
                            // Error checking status — treat as exited with error
                            term.status = TerminalStatus::Exited(-1);
                            changed = true;
                        }
                    }
                }
            }
        }

        changed
    }

    /// Count how many terminals need attention.
    pub fn attention_count(&self) -> usize {
        self.terminals
            .iter()
            .filter(|t| matches!(t.status, TerminalStatus::NeedsAttention(_)))
            .count()
    }

    /// Count how many terminals have exited.
    pub fn exited_count(&self) -> usize {
        self.terminals
            .iter()
            .filter(|t| matches!(t.status, TerminalStatus::Exited(_)))
            .count()
    }

    /// Check if any terminal is still running.
    pub fn has_running(&self) -> bool {
        self.terminals
            .iter()
            .any(|t| matches!(t.status, TerminalStatus::Running))
    }

    /// Remove all exited terminals at once. Returns count removed.
    pub fn dismiss_all_exited(&mut self) -> usize {
        let before = self.terminals.len();
        self.terminals
            .retain(|t| !matches!(t.status, TerminalStatus::Exited(_)));
        let removed = before - self.terminals.len();
        // Clamp focus
        if !self.terminals.is_empty() {
            if self.focused >= self.terminals.len() {
                self.focused = self.terminals.len() - 1;
            }
        } else {
            self.focused = 0;
        }
        removed
    }

    /// Send input bytes to a specific terminal by index.
    /// For embedded terminals, writes to the PTY and clears attention signal.
    /// For headless terminals, this is a no-op (returns Ok).
    pub fn send_input_to(&self, idx: usize, bytes: &[u8]) -> Result<()> {
        if let Some(term) = self.terminals.get(idx) {
            match &term.kind {
                TerminalKind::Embedded(pty) => {
                    let mut writer = pty.writer.lock().unwrap();
                    writer.write_all(bytes)?;
                    writer.flush()?;
                    *pty.attention_signal.lock().unwrap() = None;
                }
                TerminalKind::Headless(_) => {
                    // No-op: headless terminals have no stdin
                }
            }
        }
        Ok(())
    }

    /// Cleanup all terminals on app exit.
    ///
    /// For embedded terminals (ConPTY on Windows): drop order matters — the writer
    /// must be dropped before the master PTY handle, otherwise `ClosePseudoConsole`
    /// can deadlock if the output pipe buffer isn't fully drained.
    /// For headless terminals: kill the child process if still running.
    pub fn cleanup_all(&mut self) {
        for term in &mut self.terminals {
            match &mut term.kind {
                TerminalKind::Embedded(pty) => {
                    // Drop writer first — signals EOF to child process stdin
                    if let Ok(mut w) = pty.writer.lock() {
                        *w = Box::new(std::io::sink());
                    }
                    // Drop master PTY handle — triggers ClosePseudoConsole on Windows.
                    if let Ok(m) = pty.master.lock() {
                        drop(m);
                    }
                }
                TerminalKind::Headless(headless) => {
                    // Kill the headless child process if still running
                    let _ = headless.child.kill();
                    let _ = headless.child.wait();
                }
            }
        }
        self.terminals.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attention_reason_permission_prompt_with_context() {
        let reason = AttentionReason::PermissionPrompt {
            context: "Allow running bash command?".to_string(),
        };
        match reason {
            AttentionReason::PermissionPrompt { context } => {
                assert_eq!(context, "Allow running bash command?");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_attention_reason_error_with_context() {
        let reason = AttentionReason::Error {
            context: "error: compilation failed".to_string(),
        };
        match reason {
            AttentionReason::Error { context } => {
                assert_eq!(context, "error: compilation failed");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_terminal_manager_new() {
        let mgr = TerminalManager::new();
        assert!(mgr.terminals.is_empty());
        assert_eq!(mgr.focused, 0);
        assert_eq!(mgr.attention_count(), 0);
        assert_eq!(mgr.exited_count(), 0);
        assert!(!mgr.has_running());
    }

    #[test]
    fn test_dismiss_all_exited_empty() {
        let mut mgr = TerminalManager::new();
        assert_eq!(mgr.dismiss_all_exited(), 0);
        assert!(mgr.terminals.is_empty());
    }

    #[test]
    fn test_terminal_status_eq() {
        assert_eq!(TerminalStatus::Running, TerminalStatus::Running);
        assert_eq!(TerminalStatus::Exited(0), TerminalStatus::Exited(0));
        assert_ne!(TerminalStatus::Exited(0), TerminalStatus::Exited(1));
        assert_ne!(TerminalStatus::Running, TerminalStatus::Exited(0));

        let reason1 = AttentionReason::PermissionPrompt {
            context: "test".to_string(),
        };
        let reason2 = AttentionReason::PermissionPrompt {
            context: "test".to_string(),
        };
        assert_eq!(
            TerminalStatus::NeedsAttention(reason1),
            TerminalStatus::NeedsAttention(reason2)
        );
    }

    #[test]
    fn test_focus_navigation_empty() {
        let mut mgr = TerminalManager::new();
        mgr.focus_next();
        assert_eq!(mgr.focused, 0);
        mgr.focus_prev();
        assert_eq!(mgr.focused, 0);
        assert!(!mgr.focus_next_attention());
    }

    #[test]
    fn test_attention_reason_idle() {
        let reason = AttentionReason::Idle { seconds: 120 };
        match reason {
            AttentionReason::Idle { seconds } => assert_eq!(seconds, 120),
            _ => panic!("Wrong variant"),
        }
    }

    // ── Headless terminal tests ──────────────────────────────────────

    #[test]
    fn test_headless_spawn_and_is_headless() {
        let mut mgr = TerminalManager::new();
        // Spawn a headless terminal running "true" (exits immediately with 0)
        mgr.spawn_headless(
            "TASK_H1".to_string(),
            "Headless Test".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .expect("headless spawn should succeed");

        assert_eq!(mgr.terminals.len(), 1);
        let term = &mgr.terminals[0];
        assert!(term.is_headless());
        assert!(!term.is_embedded());
        assert_eq!(term.id, "TASK_H1");
        assert_eq!(term.label, "Headless Test");
        assert_eq!(term.status, TerminalStatus::Running);
    }

    #[test]
    fn test_headless_accessors_return_none() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H2".to_string(),
            "Test".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        let term = &mgr.terminals[0];
        assert!(term.parser().is_none());
        assert!(term.writer().is_none());
        assert!(term.master().is_none());
        assert!(term.exit_signal().is_none());
        assert!(term.attention_signal().is_none());
        assert!(term.last_output().is_none());
    }

    #[test]
    fn test_headless_exit_detection_success() {
        let mut mgr = TerminalManager::new();
        // "true" exits with code 0
        mgr.spawn_headless(
            "TASK_H3".to_string(),
            "Test".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // Wait briefly for the process to exit
        std::thread::sleep(std::time::Duration::from_millis(100));

        let changed = mgr.poll_status();
        assert!(changed, "poll_status should detect exit");
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Exited(0));
    }

    #[test]
    fn test_headless_exit_detection_failure() {
        let mut mgr = TerminalManager::new();
        // "false" exits with code 1
        mgr.spawn_headless(
            "TASK_H4".to_string(),
            "Test".to_string(),
            "false",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(100));

        let changed = mgr.poll_status();
        assert!(changed);
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Exited(1));
    }

    #[test]
    fn test_headless_kill() {
        let mut mgr = TerminalManager::new();
        // "sleep 60" runs for a long time
        mgr.spawn_headless(
            "TASK_H5".to_string(),
            "Test".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // Should be running
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Running);

        // Kill it
        if let TerminalKind::Headless(ref mut hs) = mgr.terminals[0].kind {
            hs.child.kill().expect("kill should succeed");
            hs.child.wait().expect("wait should succeed");
        } else {
            panic!("Expected headless terminal");
        }

        // Poll should detect exit
        let changed = mgr.poll_status();
        assert!(changed);
        match mgr.terminals[0].status {
            TerminalStatus::Exited(_) => {} // signal-terminated processes may have various codes
            ref other => panic!("Expected Exited, got {:?}", other),
        }
    }

    #[test]
    fn test_headless_send_input_noop() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H6".to_string(),
            "Test".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // send_input should be a no-op for headless (no panic, returns Ok)
        let result = mgr.send_input(b"hello");
        assert!(result.is_ok());

        // send_input_to should also be a no-op
        let result = mgr.send_input_to(0, b"hello");
        assert!(result.is_ok());

        // Clean up
        mgr.cleanup_all();
    }

    #[test]
    fn test_headless_resize_noop() {
        // Headless terminals don't need resize since there's no PTY.
        // Verify that poll_status doesn't crash and terminal stays running.
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H7".to_string(),
            "Test".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // No resize method to call on TerminalManager, but resize is handled
        // in terminal_view.rs via resize_if_needed which checks parser/master.
        // Headless accessors return None so resize is skipped.
        // Just verify the terminal is unaffected:
        assert!(mgr.terminals[0].parser().is_none());
        assert!(mgr.terminals[0].master().is_none());
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Running);

        mgr.cleanup_all();
    }

    #[test]
    fn test_headless_dismiss_all_exited() {
        let mut mgr = TerminalManager::new();

        // Spawn a headless terminal that exits immediately
        mgr.spawn_headless(
            "TASK_H8".to_string(),
            "Exiting".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // Spawn a headless terminal that stays running
        mgr.spawn_headless(
            "TASK_H9".to_string(),
            "Running".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        assert_eq!(mgr.terminals.len(), 2);

        // Wait for first to exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        mgr.poll_status();

        assert_eq!(mgr.exited_count(), 1);

        // Dismiss all exited
        let removed = mgr.dismiss_all_exited();
        assert_eq!(removed, 1);
        assert_eq!(mgr.terminals.len(), 1);
        assert_eq!(mgr.terminals[0].id, "TASK_H9");

        // Clean up
        mgr.cleanup_all();
    }

    #[test]
    fn test_headless_cleanup_all_kills_running() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H10".to_string(),
            "Test".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        assert_eq!(mgr.terminals.len(), 1);
        assert!(mgr.has_running());

        // cleanup_all should kill and clear
        mgr.cleanup_all();
        assert!(mgr.terminals.is_empty());
    }

    #[test]
    fn test_headless_spawn_failure() {
        let mut mgr = TerminalManager::new();
        // Try to spawn a non-existent command
        let result = mgr.spawn_headless(
            "TASK_FAIL".to_string(),
            "Bad".to_string(),
            "this-command-does-not-exist-12345",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        );

        assert!(result.is_err());
        assert_eq!(mgr.terminals.len(), 0);
    }

    #[test]
    fn test_headless_relaunch_preserves_kind() {
        let mut mgr = TerminalManager::new();
        // Spawn a headless terminal that exits immediately
        mgr.spawn_headless(
            "TASK_H11".to_string(),
            "Relaunch".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // Wait for exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        mgr.poll_status();
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Exited(0));

        // Relaunch — should stay headless
        let relaunched = mgr.relaunch("TASK_H11", 24, 80).unwrap();
        assert!(relaunched);
        assert_eq!(mgr.terminals.len(), 1);
        assert!(mgr.terminals[0].is_headless());
    }

    #[test]
    fn test_concurrent_headless_spawns() {
        let mut mgr = TerminalManager::new();

        // Spawn 3 headless terminals rapidly
        for i in 0..3 {
            mgr.spawn_headless(
                format!("TASK_C{}", i),
                format!("Concurrent {}", i),
                "sleep",
                &["60".to_string()],
                Path::new("/tmp"),
                None,
                vec![],
            )
            .unwrap();
        }

        assert_eq!(mgr.terminals.len(), 3);

        // All should be running
        assert!(mgr.terminals.iter().all(|t| t.status == TerminalStatus::Running));
        assert!(mgr.terminals.iter().all(|t| t.is_headless()));

        // All IDs should be unique
        let ids: Vec<&str> = mgr.terminals.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);

        // Focus should be on last spawned
        assert_eq!(mgr.focused, 2);

        mgr.cleanup_all();
    }

    #[test]
    fn test_headless_poll_still_running() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H12".to_string(),
            "Test".to_string(),
            "sleep",
            &["60".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        // Poll — still running, no change
        let changed = mgr.poll_status();
        assert!(!changed);
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Running);

        mgr.cleanup_all();
    }

    #[test]
    fn test_headless_stdout_stderr_null() {
        // Spawn a command that produces output — should not block or crash
        // because stdout/stderr are set to Stdio::null()
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H13".to_string(),
            "Output Test".to_string(),
            "echo",
            &["lots of output".to_string()],
            Path::new("/tmp"),
            None,
            vec![],
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(100));
        mgr.poll_status();

        // Should have exited cleanly (echo exits 0)
        assert_eq!(mgr.terminals[0].status, TerminalStatus::Exited(0));
    }

    #[test]
    fn test_headless_with_env_vars() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_H14".to_string(),
            "Env Test".to_string(),
            "true",
            &[],
            Path::new("/tmp"),
            None,
            vec![
                ("CREW_BOARD_PORT".to_string(), "12345".to_string()),
                ("CREW_BOARD_TOKEN".to_string(), "abc123".to_string()),
            ],
        )
        .unwrap();

        assert_eq!(mgr.terminals.len(), 1);
        assert!(mgr.terminals[0].is_headless());
    }
}
