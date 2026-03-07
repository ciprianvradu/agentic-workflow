//! Embedded terminal multiplexer.
//!
//! Manages multiple PTY terminals that can be rendered inside the TUI.
//! Each terminal runs a child process in a pseudoterminal, with output
//! parsed by vt100 and rendered via ratatui.

pub mod pty;
pub mod widget;

use anyhow::Result;
use portable_pty::MasterPty;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
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
}

/// Stored launch parameters for relaunching a terminal.
#[derive(Debug, Clone)]
pub struct LaunchParams {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// A single embedded terminal backed by a PTY.
pub struct EmbeddedTerminal {
    pub id: TerminalId,
    pub label: String,
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub status: TerminalStatus,
    pub color_scheme_index: Option<usize>,
    /// Original launch parameters for relaunch.
    pub launch_params: LaunchParams,
    /// Shared exit signal from the reader thread.
    pub exit_signal: pty::SharedExitSignal,
    /// Shared attention signal from the reader thread.
    pub attention_signal: pty::SharedAttentionSignal,
    /// Timestamp of last PTY output (for idle detection).
    pub last_output: Arc<Mutex<Instant>>,
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
            parser: handles.parser,
            writer: handles.writer,
            master: handles.master,
            status: TerminalStatus::Running,
            color_scheme_index,
            launch_params: LaunchParams {
                command: command.to_string(),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
            },
            exit_signal: handles.exit_signal,
            attention_signal: handles.attention_signal,
            last_output: handles.last_output,
            scroll_offset: 0,
            spawned_at: Instant::now(),
            hook_state: None,
            hook_settings_cwd: None,
            hook_cleanup_paths: Vec::new(),
        });

        // Focus the newly spawned terminal
        self.focused = self.terminals.len() - 1;

        Ok(())
    }

    /// Relaunch a terminal that has exited, reusing the same parameters.
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

        // Remove old terminal
        self.terminals.remove(idx);

        // Spawn fresh
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
    /// Also clears any attention signal (user is actively interacting).
    pub fn send_input(&self, bytes: &[u8]) -> Result<()> {
        if let Some(term) = self.focused_terminal() {
            let mut writer = term.writer.lock().unwrap();
            writer.write_all(bytes)?;
            writer.flush()?;
            // Clear attention -- user is actively interacting
            *term.attention_signal.lock().unwrap() = None;
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

            // Check exit signal
            let exit_event = term.exit_signal.lock().unwrap().clone();
            if let Some(event) = exit_event {
                term.status = TerminalStatus::Exited(event.code);
                changed = true;
                continue; // Exit takes precedence over attention
            }

            // Check for idle (no output for 120 seconds)
            let idle_threshold = std::time::Duration::from_secs(120);
            let last_out = *term.last_output.lock().unwrap();
            if last_out.elapsed() >= idle_threshold {
                let seconds = last_out.elapsed().as_secs();
                // Only set idle if not already flagged for a higher-priority reason
                let current_attn = term.attention_signal.lock().unwrap().clone();
                if current_attn.is_none() {
                    *term.attention_signal.lock().unwrap() = Some(pty::AttentionEvent {
                        kind: pty::AttentionKind::Idle { seconds },
                        timestamp: Instant::now(),
                    });
                }
            }

            // Check attention signal
            let attn_event = term.attention_signal.lock().unwrap().clone();
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
    /// Also clears any attention signal.
    pub fn send_input_to(&self, idx: usize, bytes: &[u8]) -> Result<()> {
        if let Some(term) = self.terminals.get(idx) {
            let mut writer = term.writer.lock().unwrap();
            writer.write_all(bytes)?;
            writer.flush()?;
            *term.attention_signal.lock().unwrap() = None;
        }
        Ok(())
    }

    /// Cleanup all terminals on app exit.
    /// Drops all writers (closes their stdin), which causes the child processes
    /// to eventually receive EOF and exit. The reader threads will detect this
    /// and clean up naturally.
    pub fn cleanup_all(&mut self) {
        // Drop writers to signal EOF to child processes
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
}
