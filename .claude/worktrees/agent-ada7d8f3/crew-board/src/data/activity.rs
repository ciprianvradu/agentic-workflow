use std::collections::HashMap;
use std::time::Instant;

/// A single hook event recorded by the activity feed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ActivityEvent {
    pub timestamp: Instant,
    pub terminal_id: String,
    pub event_type: String,
    pub tool_name: Option<String>,
    pub tool_input_summary: Option<String>,
    pub success: Option<bool>,
}

/// A completed tool execution span (from PreToolUse to PostToolUse).
#[derive(Debug, Clone)]
pub struct ToolSpan {
    pub terminal_id: String,
    pub tool_name: String,
    pub start: Instant,
    pub end: Instant,
    pub success: Option<bool>,
}

/// Filter and display options for the activity feed.
#[derive(Debug, Clone)]
pub struct ActivityFilter {
    pub crew_filter: Option<String>,
    pub event_filter: Option<String>,
    pub tool_filter: Option<String>,
    pub auto_scroll: bool,
    pub timeline_mode: bool,
}

impl ActivityFilter {
    pub fn new() -> Self {
        ActivityFilter {
            crew_filter: None,
            event_filter: None,
            tool_filter: None,
            auto_scroll: true,
            timeline_mode: false,
        }
    }
}

impl Default for ActivityFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Accumulates activity events and tool spans from hook notifications.
#[derive(Debug)]
pub struct ActivityLog {
    events: Vec<ActivityEvent>,
    #[allow(dead_code)] // used in push()
    max_events: usize,
    #[allow(dead_code)] // used in push()
    open_spans: HashMap<String, (Instant, String)>,
    pub completed_spans: Vec<ToolSpan>,
}

impl ActivityLog {
    pub fn new() -> Self {
        ActivityLog {
            events: Vec::new(),
            max_events: 5000,
            open_spans: HashMap::new(),
            completed_spans: Vec::new(),
        }
    }

    /// Push a new event into the log.
    /// Automatically tracks PreToolUse/PostToolUse spans.
    #[allow(dead_code)] // will be called by hook server integration
    pub fn push(&mut self, event: ActivityEvent) {
        // Track tool spans
        match event.event_type.as_str() {
            "PreToolUse" => {
                if let Some(ref tool_name) = event.tool_name {
                    self.open_spans.insert(
                        event.terminal_id.clone(),
                        (event.timestamp, tool_name.clone()),
                    );
                }
            }
            "PostToolUse" => {
                if let Some((start, tool_name)) = self.open_spans.remove(&event.terminal_id) {
                    let span = ToolSpan {
                        terminal_id: event.terminal_id.clone(),
                        tool_name,
                        start,
                        end: event.timestamp,
                        success: event.success,
                    };
                    self.completed_spans.push(span);
                    // Cap completed_spans at 1000
                    if self.completed_spans.len() > 1000 {
                        self.completed_spans.remove(0);
                    }
                }
            }
            _ => {}
        }

        self.events.push(event);

        // Cap event log
        if self.events.len() > self.max_events {
            self.events.remove(0);
        }
    }

    /// Get all events (for table rendering).
    #[allow(dead_code)] // will be used by activity_view table rendering
    pub fn events(&self) -> &[ActivityEvent] {
        &self.events
    }

    /// Get filtered events based on the current filter settings.
    pub fn filtered_events(&self, filter: &ActivityFilter) -> Vec<&ActivityEvent> {
        self.events
            .iter()
            .filter(|e| {
                if let Some(ref crew) = filter.crew_filter {
                    if !e.terminal_id.contains(crew) {
                        return false;
                    }
                }
                if let Some(ref evt) = filter.event_filter {
                    if !e.event_type.contains(evt) {
                        return false;
                    }
                }
                if let Some(ref tool) = filter.tool_filter {
                    if let Some(ref tn) = e.tool_name {
                        if !tn.contains(tool) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Number of total events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[allow(dead_code)] // will be used by activity_view
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

    fn make_event(terminal_id: &str, event_type: &str, tool_name: Option<&str>) -> ActivityEvent {
        ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: terminal_id.to_string(),
            event_type: event_type.to_string(),
            tool_name: tool_name.map(|s| s.to_string()),
            tool_input_summary: None,
            success: None,
        }
    }

    #[test]
    fn test_activity_log_push() {
        let mut log = ActivityLog::new();
        assert!(log.is_empty());
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_activity_filter_default() {
        let filter = ActivityFilter::new();
        assert!(filter.crew_filter.is_none());
        assert!(filter.event_filter.is_none());
        assert!(filter.tool_filter.is_none());
        assert!(filter.auto_scroll);
        assert!(!filter.timeline_mode);
    }

    #[test]
    fn test_filtered_events() {
        let mut log = ActivityLog::new();
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        log.push(make_event("T2", "PreToolUse", Some("Bash")));

        let mut filter = ActivityFilter::new();
        let all = log.filtered_events(&filter);
        assert_eq!(all.len(), 2);

        filter.crew_filter = Some("T1".to_string());
        let filtered = log.filtered_events(&filter);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].terminal_id, "T1");
    }

    #[test]
    fn test_tool_span_tracking() {
        let mut log = ActivityLog::new();

        // PreToolUse starts a span
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PreToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: None,
        });
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

        // T1 starts
        log.push(make_event("T1", "PreToolUse", Some("Edit")));
        // T2 starts
        log.push(make_event("T2", "PreToolUse", Some("Bash")));
        // T1 finishes
        log.push(ActivityEvent {
            timestamp: Instant::now(),
            terminal_id: "T1".to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: None,
            success: Some(true),
        });
        assert_eq!(log.completed_spans.len(), 1);

        // T2 finishes
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
    fn test_span_cap_at_1000() {
        let mut log = ActivityLog::new();
        for i in 0..1010 {
            let tid = format!("T{}", i);
            log.push(ActivityEvent {
                timestamp: Instant::now(),
                terminal_id: tid.clone(),
                event_type: "PreToolUse".to_string(),
                tool_name: Some("Edit".to_string()),
                tool_input_summary: None,
                success: None,
            });
            log.push(ActivityEvent {
                timestamp: Instant::now(),
                terminal_id: tid,
                event_type: "PostToolUse".to_string(),
                tool_name: Some("Edit".to_string()),
                tool_input_summary: None,
                success: Some(true),
            });
        }
        assert_eq!(log.completed_spans.len(), 1000);
    }

    #[test]
    fn test_post_without_pre_is_ignored() {
        let mut log = ActivityLog::new();
        // PostToolUse without a preceding PreToolUse should not create a span
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
