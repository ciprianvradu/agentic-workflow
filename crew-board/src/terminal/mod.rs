//! Embedded terminal multiplexer.
//!
//! Manages multiple PTY terminals that can be rendered inside the TUI.
//! Each terminal runs a child process in a pseudoterminal, with output
//! parsed by vt100 and rendered via ratatui.

pub mod pty;
pub mod widget;

use anyhow::Result;
use portable_pty::MasterPty;
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
    PermissionPrompt,
    Idle { seconds: u64 },
    Error,
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
        let handles = pty::spawn_pty(command, args, cwd, rows, cols)?;

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
                    pty::AttentionKind::PermissionPrompt { .. } => {
                        AttentionReason::PermissionPrompt
                    }
                    pty::AttentionKind::Idle { seconds } => {
                        AttentionReason::Idle { seconds: *seconds }
                    }
                    pty::AttentionKind::Error { .. } => AttentionReason::Error,
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

    /// Cleanup all terminals on app exit.
    /// Drops all writers (closes their stdin), which causes the child processes
    /// to eventually receive EOF and exit. The reader threads will detect this
    /// and clean up naturally.
    pub fn cleanup_all(&mut self) {
        // Drop writers to signal EOF to child processes
        self.terminals.clear();
    }
}
