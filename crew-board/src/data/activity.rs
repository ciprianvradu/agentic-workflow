//! Activity event log and statistics for hook-based communication.
//!
//! Provides a ring buffer of recent activity events and per-terminal/global
//! statistics derived from Claude Code hook events.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// A completed tool execution span (from PreToolUse to PostToolUse).
#[derive(Debug, Clone)]
pub struct ToolSpan {
    pub terminal_id: String,
    pub tool_name: String,
    pub start: Instant,
    pub end: Instant,
    pub success: Option<bool>,
}

/// Maximum number of events stored in the ring buffer.
const MAX_EVENTS: usize = 500;

/// A single activity event from a hook.
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    pub timestamp: Instant,
    pub terminal_id: String,
    pub event_type: String,
    pub tool_name: Option<String>,
    pub tool_input_summary: Option<String>,
    pub success: Option<bool>,
}

/// Per-terminal hook statistics.
#[derive(Debug, Clone, Default)]
pub struct TerminalHookStats {
    pub total_tools: u32,
    pub errors: u32,
    pub files_touched: Vec<String>,
    pub active_tool: Option<String>,
    pub start_time: Option<Instant>,
}

/// Aggregate statistics across all terminals.
#[derive(Debug, Clone, Default)]
pub struct GlobalStats {
    pub total_tool_calls: u32,
    pub total_errors: u32,
    pub active_terminals: u32,
}

/// Ring-buffer activity log with filtering and statistics.
pub struct ActivityLog {
    events: VecDeque<ActivityEvent>,
    per_terminal: HashMap<String, TerminalHookStats>,
    open_spans: HashMap<String, (Instant, String)>,
    pub completed_spans: Vec<ToolSpan>,
}

impl ActivityLog {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(MAX_EVENTS),
            per_terminal: HashMap::new(),
            open_spans: HashMap::new(),
            completed_spans: Vec::new(),
        }
    }

    /// Push a new event, evicting the oldest if at capacity.
    pub fn push(&mut self, event: ActivityEvent) {
        // Update per-terminal stats
        let stats = self.per_terminal.entry(event.terminal_id.clone()).or_default();

        match event.event_type.as_str() {
            "SessionStart" => {
                stats.start_time = Some(event.timestamp);
                stats.active_tool = None;
            }
            "PreToolUse" => {
                stats.active_tool = event.tool_name.clone();
                // Start a tool span
                if let Some(ref tool_name) = event.tool_name {
                    self.open_spans.insert(
                        event.terminal_id.clone(),
                        (event.timestamp, tool_name.clone()),
                    );
                }
            }
            "PostToolUse" => {
                // Complete a tool span
                if let Some((start, tool_name)) = self.open_spans.remove(&event.terminal_id) {
                    let span = ToolSpan {
                        terminal_id: event.terminal_id.clone(),
                        tool_name,
                        start,
                        end: event.timestamp,
                        success: event.success,
                    };
                    self.completed_spans.push(span);
                    if self.completed_spans.len() > 1000 {
                        self.completed_spans.remove(0);
                    }
                }
                stats.total_tools += 1;
                if event.success == Some(false) {
                    stats.errors += 1;
                }
                // Track files touched
                if let Some(ref summary) = event.tool_input_summary {
                    if !summary.is_empty()
                        && !stats.files_touched.contains(summary)
                        && stats.files_touched.len() < 100
                    {
                        stats.files_touched.push(summary.clone());
                    }
                }
                stats.active_tool = None;
            }
            "SessionEnd" => {
                stats.active_tool = None;
            }
            _ => {}
        }

        // Add to ring buffer
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Get all events (oldest first).
    pub fn events(&self) -> &VecDeque<ActivityEvent> {
        &self.events
    }

    /// Filter events by optional terminal_id, event_type, and tool_name.
    pub fn filter(
        &self,
        terminal_id: Option<&str>,
        event_type: Option<&str>,
        tool_name: Option<&str>,
    ) -> Vec<&ActivityEvent> {
        self.events
            .iter()
            .filter(|e| {
                if let Some(tid) = terminal_id {
                    if e.terminal_id != tid {
                        return false;
                    }
                }
                if let Some(et) = event_type {
                    if e.event_type != et {
                        return false;
                    }
                }
                if let Some(tn) = tool_name {
                    match &e.tool_name {
                        Some(name) if name == tn => {}
                        _ => return false,
                    }
                }
                true
            })
            .collect()
    }

    /// Get stats for a specific terminal.
    pub fn stats_for_terminal(&self, terminal_id: &str) -> Option<&TerminalHookStats> {
        self.per_terminal.get(terminal_id)
    }

    /// Compute global aggregate stats.
    pub fn global_stats(&self) -> GlobalStats {
        let mut stats = GlobalStats::default();
        for ts in self.per_terminal.values() {
            stats.total_tool_calls += ts.total_tools;
            stats.total_errors += ts.errors;
            if ts.active_tool.is_some() {
                stats.active_terminals += 1;
            }
        }
        stats
    }

    /// Total number of events in the log.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(terminal_id: &str, event_type: &str, tool: Option<&str>) -> ActivityEvent {
        ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: terminal_id.to_string(),
            event_type: event_type.to_string(),
            tool_name: tool.map(|t| t.to_string()),
            tool_input_summary: None,
            success: None,
        }
    }

    #[test]
    fn test_push_and_len() {
        let mut log = ActivityLog::new();
        assert!(log.is_empty());
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut log = ActivityLog::new();
        for i in 0..600 {
            log.push(make_event(&format!("T{}", i % 10), "PostToolUse", Some("Bash")));
        }
        assert_eq!(log.len(), MAX_EVENTS);
    }

    #[test]
    fn test_filter_by_terminal() {
        let mut log = ActivityLog::new();
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(make_event("T2", "PreToolUse", Some("Read")));
        log.push(make_event("T1", "PostToolUse", Some("Edit")));

        let filtered = log.filter(Some("T1"), None, None);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_event_type() {
        let mut log = ActivityLog::new();
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(make_event("T1", "PostToolUse", Some("Edit")));

        let filtered = log.filter(None, Some("PostToolUse"), None);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_tool() {
        let mut log = ActivityLog::new();
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(make_event("T1", "PreToolUse", Some("Bash")));

        let filtered = log.filter(None, None, Some("Edit"));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_terminal_stats() {
        let mut log = ActivityLog::new();
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: Some(true),
        });
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input_summary: Some("ls".to_string()),
            success: Some(false),
        });

        let stats = log.stats_for_terminal("T1").unwrap();
        assert_eq!(stats.total_tools, 2);
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.files_touched.len(), 2);
    }

    #[test]
    fn test_global_stats() {
        let mut log = ActivityLog::new();
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T2".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input_summary: None,
            success: Some(true),
        });

        let global = log.global_stats();
        assert_eq!(global.total_tool_calls, 1);
        assert_eq!(global.active_terminals, 1); // T1 has active_tool
    }

    #[test]
    fn test_stats_combined_workflow() {
        let mut log = ActivityLog::new();
        // Simulate a full workflow: session start, tools, session end
        log.push(make_event("T1", "SessionStart", None));
        log.push(make_event("T1", "PreToolUse", Some("Read")));
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Read".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: Some(true),
        });
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/app.rs".to_string()),
            success: Some(true),
        });
        log.push(make_event("T1", "SessionEnd", None));

        // Verify stats
        let stats = log.stats_for_terminal("T1").unwrap();
        assert_eq!(stats.total_tools, 2);
        assert_eq!(stats.errors, 0);
        assert_eq!(stats.files_touched.len(), 2);

        // Verify filtering
        let pre_tools = log.filter(Some("T1"), Some("PreToolUse"), None);
        assert_eq!(pre_tools.len(), 2);

        let global = log.global_stats();
        assert_eq!(global.total_tool_calls, 2);
        assert_eq!(global.active_terminals, 0); // Session ended
    }

    #[test]
    fn test_tool_span_tracking() {
        let mut log = ActivityLog::new();

        // PreToolUse starts a span
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        assert!(log.completed_spans.is_empty());

        // PostToolUse completes the span
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: Some(true),
        });
        assert_eq!(log.completed_spans.len(), 1);
        assert_eq!(log.completed_spans[0].tool_name, "Edit");
        assert_eq!(log.completed_spans[0].success, Some(true));
    }

    #[test]
    fn test_multiple_spans_different_terminals() {
        let mut log = ActivityLog::new();

        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(make_event("T2", "PreToolUse", Some("Bash")));
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: None,
            success: Some(true),
        });
        assert_eq!(log.completed_spans.len(), 1);

        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T2".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input_summary: None,
            success: Some(false),
        });
        assert_eq!(log.completed_spans.len(), 2);
        assert_eq!(log.completed_spans[1].success, Some(false));
    }

    #[test]
    fn test_post_without_pre_no_span() {
        let mut log = ActivityLog::new();
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: None,
            success: Some(true),
        });
        assert!(log.completed_spans.is_empty());
    }
}
