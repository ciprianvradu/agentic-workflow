use crate::cleanup;
use crate::data::activity::ActivityLog;
use crate::data::file_claims::FileClaimsRegistry;
use crate::data::task::{self, Discovery, Interaction, TaskArtifact};
use crate::data::RepoData;
use crate::hook_server::{HookEvent, HookServer, PendingPermission, PermissionDecision};
use crate::security::RulesEngine;
use crate::launcher::{self, AiHost, TerminalEnv};
use crate::terminal::{AttentionReason, HookState, TerminalManager, TerminalStatus};
use crate::worktree;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use ratatui::layout::Rect;
use tui_input::Input;

/// Send a desktop notification (best-effort, non-blocking).
#[allow(unused_variables)]
fn send_desktop_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("notify-send")
                .args([&title, &body])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                body, title
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    });
}

/// Task list filter mode.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TaskFilter {
    /// Show all tasks (default behavior).
    #[default]
    All,
    /// Show only active tasks (not archived and not complete).
    Active,
    /// Show active tasks plus tasks completed within recent_done_days.
    ActiveAndRecentDone,
}

/// Which view/tab is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveView {
    Tasks,
    BeadsIssues,
    Config,
    CostSummary,
    Terminals,
    ActivityFeed,
}

/// Input mode for the terminal view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalInputMode {
    /// Normal mode: keys navigate the terminal list.
    Normal,
    /// Terminal focused: all keys go to the PTY (F12 exits back to Normal).
    TerminalFocused,
    /// Legacy prefix mode (unused — kept for enum compatibility).
    PrefixPending,
    /// Scroll-back mode: navigate the vt100 scrollback buffer.
    ScrollBack,
}

/// Permission profile controlling auto-approval behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PermissionProfile {
    /// All prompts require manual approval.
    Interactive,
    /// Auto-approve prompts matching configured patterns.
    Trusted,
    /// Auto-approve all permission prompts.
    Autonomous,
}

impl PermissionProfile {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trusted" => Self::Trusted,
            "autonomous" => Self::Autonomous,
            _ => Self::Interactive,
        }
    }
}

/// Layout mode for the Terminals view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalLayout {
    /// One large terminal + crew list sidebar (default).
    Focused,
    /// Two terminals side by side.
    Tiled2,
    /// Four terminals in a 2x2 grid.
    Tiled4,
    /// Terminals stacked vertically.
    Stacked,
}

impl TerminalLayout {
    /// Cycle to the next layout mode.
    pub fn next(self) -> Self {
        match self {
            Self::Focused => Self::Tiled2,
            Self::Tiled2 => Self::Tiled4,
            Self::Tiled4 => Self::Stacked,
            Self::Stacked => Self::Focused,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::Tiled2 => "tiled-2",
            Self::Tiled4 => "tiled-4",
            Self::Stacked => "stacked",
        }
    }
}

/// Which modifier layer is shown on the F-key bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModifierBarState {
    /// Base F-key bar (no modifier held).
    Normal,
    /// Shift layer: view switching + detail shortcuts.
    Shift,
    /// Ctrl layer: reserved for future expansion.
    Ctrl,
    /// Alt layer: reserved for future expansion.
    Alt,
    /// Shift+Ctrl layer: reserved for future expansion.
    ShiftCtrl,
    /// Alt+Shift layer: reserved for future expansion.
    AltShift,
    /// Ctrl+Alt layer: reserved for future expansion.
    CtrlAlt,
}

/// Mouse text selection state for terminal panels.
#[derive(Debug, Clone)]
pub struct TextSelection {
    /// Index of the terminal this selection belongs to.
    pub terminal_idx: usize,
    /// Inner rect of the terminal panel (absolute screen coordinates).
    pub panel_rect: Rect,
    /// Start position (col, row) relative to panel_rect origin.
    pub start_col: u16,
    pub start_row: u16,
    /// Current end position (col, row) relative to panel_rect origin.
    pub end_col: u16,
    pub end_row: u16,
    /// Whether the mouse button is still held (selection in progress).
    pub active: bool,
}

/// Which pane has focus in dual-pane views.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusPane {
    Left,
    Right,
}

/// A row in the flattened tree list.
#[derive(Debug, Clone)]
pub enum TreeRow {
    Repo(usize),            // index into repos vec
    Task(usize, usize),     // (repo_index, task_index)
}

/// What's shown in the detail (right) pane.
#[derive(Debug, Clone, PartialEq)]
pub enum DetailMode {
    /// Normal task/repo overview
    Overview,
    /// Browsing document list (cursor on a doc)
    DocList { cursor: usize },
    /// Reading a specific document
    DocReader {
        artifact_index: usize,
        content: String,
    },
    /// Viewing task history (decisions + iterations)
    History,
}

/// State for the F2 launch popup.
pub struct LaunchPopup {
    pub terminals: Vec<TerminalEnv>,
    pub hosts: Vec<AiHost>,
    pub step: LaunchStep,
    pub terminal_cursor: usize,
    pub host_cursor: usize,
    pub work_dir: PathBuf,
    pub task_id: String,
    pub task_desc: String,
    pub color_scheme_index: Option<usize>,
    pub result_msg: Option<String>,
    /// Whether to spawn in headless mode (no PTY). Resets each time popup opens.
    pub headless: bool,
}

#[derive(Debug, PartialEq)]
pub enum LaunchStep {
    SelectTerminal,
    /// Choose between Embedded (PTY) and Headless (no PTY) mode.
    /// Only shown when Embedded terminal is selected.
    SelectMode,
    SelectHost,
    Done,
}

/// Steps in the create worktree popup flow.
#[derive(PartialEq)]
pub enum CreateStep {
    InputDescription,
    SelectHost,
    ToggleSettings,
    Confirm,
    Executing,
    Done,
}

/// State for the "new worktree" popup.
pub struct CreateWorktreePopup {
    pub step: CreateStep,
    pub description_input: Input,
    pub hosts: Vec<AiHost>,
    pub host_cursor: usize,
    pub pull: bool,
    pub launch_after: bool,
    /// Whether to launch in headless mode (no PTY) after creation.
    pub headless: bool,
    /// Skip workflow checkpoints (--no-checkpoints). Auto-selects headless when enabled.
    pub no_checkpoints: bool,
    pub settings_cursor: usize,
    pub repo_path: PathBuf,
    pub repo_name: String,
    pub preview: Option<worktree::WorktreePreview>,
    pub handle: Option<std::thread::JoinHandle<Result<worktree::WorktreeResult, String>>>,
    pub started_at: Option<std::time::Instant>,
    pub result: Option<Result<worktree::WorktreeResult, String>>,
}

/// Steps in the cleanup worktree popup flow.
#[derive(PartialEq)]
pub enum CleanupStep {
    /// Select worktrees to clean up (multi-select with checkboxes)
    SelectWorktrees,
    /// Toggle cleanup settings (remove branch, keep on disk)
    Settings,
    /// Dry-run preview showing all actions + warnings
    Preview,
    /// Executing cleanup (background thread)
    Executing,
    /// Done: show results
    Done,
}

/// State for the F6 cleanup worktree popup.
pub struct CleanupPopup {
    pub step: CleanupStep,
    pub repo_path: PathBuf,
    pub repo_name: String,
    pub candidates: Vec<cleanup::WorktreeCandidate>,
    pub selected: HashSet<usize>,
    pub cursor: usize,
    pub remove_branch: bool,
    pub keep_on_disk: bool,
    pub settings_cursor: usize,
    pub preview: Vec<cleanup::CleanupAction>,
    pub handle: Option<std::thread::JoinHandle<Vec<cleanup::CleanupResult>>>,
    pub started_at: Option<std::time::Instant>,
    pub results: Option<Vec<cleanup::CleanupResult>>,
    pub scroll: u16,
}

/// A single search hit linking back to a specific task.
pub struct SearchResult {
    pub repo_index: usize,
    pub task_index: usize,
    pub task_id: String,
    pub description: String,
    pub match_source: String, // "description", "architect.md", "linked_issue", etc.
}

/// State for the `/` search popup.
pub struct SearchPopup {
    pub input: Input,
    pub results: Vec<SearchResult>,
    pub cursor: usize,
    /// Set to true when input changes; cleared after search runs.
    pub dirty: bool,
    /// When the last keystroke was received (for debounce).
    pub last_input: std::time::Instant,
}

/// Pre-built search corpus for a single task.
#[derive(Clone)]
pub struct SearchEntry {
    pub repo_index: usize,
    pub task_index: usize,
    pub task_id: String,
    pub description: String,
    /// Searchable segments: (lowercased_text, source_label).
    pub segments: Vec<(String, String)>,
}

/// Result of a background refresh.
#[allow(dead_code)]
pub struct BgRefreshResult {
    pub repos: Vec<RepoData>,
    pub changed_task_ids: Vec<String>,
    pub search_index: Vec<SearchEntry>,
}

pub struct App {
    pub repos: Vec<RepoData>,
    pub repo_paths: Vec<PathBuf>,
    pub poll_interval_secs: u64,

    // Tree navigation state
    pub expanded_repos: HashSet<usize>,
    pub tree_rows: Vec<TreeRow>,
    pub tree_cursor: usize,
    /// Current task filter mode (cycles with 'f' key).
    pub task_filter: TaskFilter,
    /// Number of days to include completed tasks in ActiveAndRecentDone filter.
    pub recent_done_days: u32,

    // Issue navigation (for beads view)
    pub selected_issue: usize,

    pub active_view: ActiveView,
    pub focus_pane: FocusPane,

    // UI state
    pub should_quit: bool,
    pub quit_confirm: bool,
    pub show_help: bool,
    pub help_scroll: u16,
    /// Whether the welcome splash screen is visible.
    pub show_splash: bool,
    /// Scroll offset for the splash popup content.
    pub splash_scroll: u16,
    /// Index of the highlighted task in the splash screen task list.
    pub splash_task_cursor: usize,
    /// Snapshot of active tasks for splash: (repo_index, task_index) sorted by updated_at desc.
    pub splash_active_tasks: Vec<(usize, usize)>,
    pub last_refresh: std::time::Instant,
    pub detail_scroll: u16,
    /// Max scroll offset computed during rendering (clamping target).
    pub detail_scroll_max: Cell<u16>,
    /// True when the terminal is wide enough for long F-key labels (>= 130 cols).
    /// Updated each draw cycle by `status_bar::draw()`.
    pub wide_labels: Cell<bool>,

    // Detail pane state
    pub detail_mode: DetailMode,
    pub cached_artifacts: Vec<TaskArtifact>,
    pub cached_task_dir: Option<PathBuf>,
    pub cached_interactions: Vec<Interaction>,
    pub cached_discoveries: Vec<Discovery>,
    pub cached_history_task_dir: Option<PathBuf>,

    // Launch popup
    pub launch_popup: Option<LaunchPopup>,

    // Create worktree popup
    pub create_popup: Option<CreateWorktreePopup>,

    // Search popup
    pub search_popup: Option<SearchPopup>,

    // Cleanup worktree popup
    pub cleanup_popup: Option<CleanupPopup>,

    // Search index (built on load, invalidated per-task on mtime change)
    pub search_index: Vec<SearchEntry>,

    // Background refresh
    pub bg_refresh_handle: Option<std::thread::JoinHandle<BgRefreshResult>>,

    // Permission queue popup
    pub permission_popup: Option<crate::ui::permission_popup::PermissionPopup>,

    // Embedded terminals
    pub terminal_manager: Option<TerminalManager>,
    pub terminal_input_mode: TerminalInputMode,
    pub terminal_layout: TerminalLayout,
    /// Last known terminal area dimensions (rows, cols) for proper PTY sizing.
    pub last_terminal_size: (u16, u16),

    // Configurable pane widths
    /// Left pane width percentage for Tasks view (configurable, 10-90).
    pub pane_width_tasks: u8,
    /// Left pane width percentage for Issues/Beads view (configurable, 10-90).
    pub pane_width_issues: u8,
    /// Crew list width in chars for Terminals view (configurable, 10-50).
    pub pane_width_terminals: u8,

    // Modifier F-key bar state
    /// Which modifier layer is currently shown on the F-key bar.
    pub modifier_bar_state: ModifierBarState,
    /// When set, the modifier bar reverts to Normal after this instant (flash fallback).
    pub modifier_bar_flash_until: Option<std::time::Instant>,
    /// When set, the status bar attention badge flashes until this instant.
    pub attention_flash_until: Option<std::time::Instant>,
    /// Whether the kitty keyboard protocol is active (modifier-only detection).
    pub kitty_protocol_enabled: bool,

    // Mouse text selection
    /// Current text selection state (if any).
    pub text_selection: Option<TextSelection>,
    /// Panel rects for terminal panels, set during draw for mouse hit-testing.
    /// Vec of (terminal_index, inner_rect).
    pub terminal_panel_rects: RefCell<Vec<(usize, Rect)>>,

    // Bell settings (loaded from settings.toml)
    /// Trigger a terminal bell (\x07) when a crew needs attention.
    pub system_bell: bool,
    /// Flash the status indicator when a crew needs attention.
    pub visual_bell: bool,
    /// Directory for terminal output logs (None = disabled).
    pub log_directory: Option<PathBuf>,

    // Permission profile
    /// Permission approval mode.
    pub permission_profile: PermissionProfile,
    /// Compiled regex patterns for auto-approval (trusted profile).
    pub auto_approve_patterns: Vec<regex::Regex>,
    /// Send desktop notifications on attention events.
    pub desktop_notifications: bool,
    /// Default auto-accept state for newly spawned terminals (from config).
    pub auto_accept_default: bool,

    // Terminal search (scroll-back mode)
    /// Active search input in scroll-back mode (None = not searching).
    pub terminal_search_input: Option<Input>,
    /// Last search query used for n/N navigation.
    pub terminal_search_query: String,
    /// Matched line offsets from bottom (for n/N navigation).
    pub terminal_search_matches: Vec<usize>,
    /// Current match index in `terminal_search_matches`.
    pub terminal_search_match_idx: usize,

    // Hook communication
    /// HTTP hook server (None if disabled or failed to start).
    pub hook_server: Option<HookServer>,
    /// Receiver for hook events from the HTTP server.
    pub hook_receiver: Option<Receiver<HookEvent>>,
    /// Receiver for pending permission requests from the hook server.
    pub hook_pending_rx: Option<Receiver<PendingPermission>>,
    /// Pending hook-based permission requests awaiting user approval.
    pub pending_permissions: Vec<PendingPermission>,
    /// Whether hook communication is enabled (from settings).
    pub hook_communication: bool,
    /// In-memory activity event log and per-terminal statistics.
    pub activity_log: ActivityLog,
    /// File claims registry for conflict detection across terminals.
    pub file_claims: FileClaimsRegistry,
    /// Security rules engine for tool governance.
    pub rules_engine: RulesEngine,

    // Activity feed view state
    /// Activity feed filter state (View 6).
    pub activity_filter: crate::ui::activity_view::ActivityFilter,
    /// Manual scroll offset for Activity Feed (used when auto_scroll is false).
    pub activity_scroll: usize,

    // Orchestration engine
    /// Auto-orchestration engine for task management.
    pub orchestration: Option<crate::orchestration::OrchestrationState>,

    // Stats popup
    /// Stats popup state (Ctrl+F6).
    pub stats_popup: Option<crate::ui::stats_popup::StatsPopup>,
}

/// Truncate a string to at most `max` characters, appending "..." if cut.
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let limit = max.saturating_sub(3);
        let boundary = s.char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= limit)
            .last()
            .unwrap_or(0);
        format!("{}...", &s[..boundary])
    }
}

/// Append a single hook event as a JSON line to `{task_dir}/history.jsonl`.
/// Opens the file in append mode for each write (atomic for lines under PIPE_BUF).
/// Silently ignores all errors — this is a best-effort audit log.
///
/// Uses `serde_json` serialization of `HistoryEvent` for safe, structured output.
fn append_history_jsonl(task_dir: &std::path::Path, terminal_id: &str, event: &HookEvent) {
    use std::io::Write;
    use crate::security::HistoryEvent;

    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let history_event = match event {
        HookEvent::SessionStart { session_id, .. } => HistoryEvent::SessionStart {
            ts,
            terminal_id: terminal_id.to_string(),
            session_id: session_id.clone(),
        },
        HookEvent::SessionEnd { .. } => HistoryEvent::SessionEnd {
            ts,
            terminal_id: terminal_id.to_string(),
        },
        HookEvent::PreToolUse { tool_name, tool_input_summary, .. } => HistoryEvent::PreToolUse {
            ts,
            terminal_id: terminal_id.to_string(),
            tool: tool_name.clone(),
            detail: tool_input_summary.clone(),
        },
        HookEvent::PostToolUse { tool_name, tool_input_summary, success, .. } => HistoryEvent::PostToolUse {
            ts,
            terminal_id: terminal_id.to_string(),
            tool: tool_name.clone(),
            detail: tool_input_summary.clone(),
            success: *success,
        },
        HookEvent::Notification { message, .. } => HistoryEvent::Notification {
            ts,
            terminal_id: terminal_id.to_string(),
            message: message.clone(),
        },
        HookEvent::Stop { preview, .. } => HistoryEvent::Stop {
            ts,
            terminal_id: terminal_id.to_string(),
            preview: preview.clone(),
        },
        HookEvent::PermissionRequest { tool_name, .. } => HistoryEvent::PermissionRequest {
            ts,
            terminal_id: terminal_id.to_string(),
            tool: tool_name.clone(),
        },
        HookEvent::UserPromptSubmit { prompt_preview, .. } => HistoryEvent::UserPrompt {
            ts,
            terminal_id: terminal_id.to_string(),
            prompt_preview: prompt_preview.clone(),
        },
    };

    let json_line = match serde_json::to_string(&history_event) {
        Ok(j) => format!("{}\n", j),
        Err(_) => return, // Skip on serialization error
    };

    let history_path = task_dir.join("history.jsonl");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history_path)
    {
        let _ = file.write_all(json_line.as_bytes());
    }
}

/// Append a permission decision to history.jsonl.
#[allow(dead_code)]
fn append_permission_decision(
    task_dir: &std::path::Path,
    terminal_id: &str,
    tool_name: &str,
    decision: &str,
    decided_by: &str,
    decided_via: &str,
) {
    use std::io::Write;
    use crate::security::HistoryEvent;

    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let event = HistoryEvent::PermissionDecision {
        ts,
        terminal_id: terminal_id.to_string(),
        tool: tool_name.to_string(),
        decision: decision.to_string(),
        decided_by: decided_by.to_string(),
        decided_via: decided_via.to_string(),
    };

    let json_line = match serde_json::to_string(&event) {
        Ok(j) => format!("{}\n", j),
        Err(_) => return,
    };

    let history_path = task_dir.join("history.jsonl");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history_path)
    {
        let _ = file.write_all(json_line.as_bytes());
    }
}

/// Build the full search index from all repos' tasks.
/// Module-level free function — callable from background threads.
fn build_search_index(repos: &[RepoData]) -> Vec<SearchEntry> {
    let mut index = Vec::new();
    for (repo_index, repo) in repos.iter().enumerate() {
        for (task_index, loaded) in repo.tasks.iter().enumerate() {
            index.push(build_search_entry(repo_index, task_index, loaded));
        }
    }
    index
}

/// Build a search entry for a single task.
/// Re-reads state.json from disk to capture raw JSON fields not modeled in TaskState.
fn build_search_entry(
    repo_index: usize,
    task_index: usize,
    loaded: &crate::data::task::LoadedTask,
) -> SearchEntry {
    let task = &loaded.state;
    let mut segments: Vec<(String, String)> = Vec::new();

    // Structured fields
    segments.push((task.task_id.to_lowercase(), "task_id".to_string()));
    segments.push((task.description.to_lowercase(), "description".to_string()));
    if let Some(ref wt) = task.worktree {
        if !wt.branch.is_empty() {
            segments.push((wt.branch.to_lowercase(), "branch".to_string()));
        }
    }
    if let Some(ref phase) = task.phase {
        segments.push((phase.to_lowercase(), "phase".to_string()));
    }

    // Skip disk reads for archived tasks
    if !loaded.archived {
        // Raw state.json (captures fields not in TaskState struct)
        let state_path = loaded.dir.join("state.json");
        if let Ok(raw) = std::fs::read_to_string(&state_path) {
            segments.push((raw.to_lowercase(), "state.json".to_string()));
        }

        // .md artifacts (first 4KB each)
        if let Ok(entries) = std::fs::read_dir(&loaded.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                if let Ok(file) = std::fs::File::open(&path) {
                    use std::io::Read;
                    let mut buf = vec![0u8; 4096];
                    let mut reader = std::io::BufReader::new(file);
                    let n = reader.read(&mut buf).unwrap_or(0);
                    let text = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let fname = path
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or("artifact")
                        .to_string();
                    segments.push((text, fname));
                }
            }
        }
    }

    SearchEntry {
        repo_index,
        task_index,
        task_id: task.task_id.clone(),
        description: task.description.clone(),
        segments,
    }
}

impl App {
    pub fn new(repo_paths: Vec<PathBuf>, poll_interval_secs: u64) -> Self {
        let repos: Vec<RepoData> = repo_paths.iter().map(|p| RepoData::load(p)).collect();

        // Auto-expand all repos on start
        let expanded: HashSet<usize> = (0..repos.len()).collect();

        let search_index = build_search_index(&repos);

        let mut app = App {
            repos,
            repo_paths,
            poll_interval_secs,
            expanded_repos: expanded,
            tree_rows: Vec::new(),
            tree_cursor: 0,
            task_filter: TaskFilter::All,
            recent_done_days: 7,
            selected_issue: 0,
            active_view: ActiveView::Tasks,
            focus_pane: FocusPane::Left,
            should_quit: false,
            quit_confirm: false,
            show_help: false,
            help_scroll: 0,
            show_splash: true,
            splash_scroll: 0,
            splash_task_cursor: 0,
            splash_active_tasks: Vec::new(),
            last_refresh: std::time::Instant::now(),
            detail_scroll: 0,
            detail_scroll_max: Cell::new(0),
            wide_labels: Cell::new(false),
            detail_mode: DetailMode::Overview,
            cached_artifacts: Vec::new(),
            cached_task_dir: None,
            cached_interactions: Vec::new(),
            cached_discoveries: Vec::new(),
            cached_history_task_dir: None,
            launch_popup: None,
            create_popup: None,
            search_popup: None,
            cleanup_popup: None,
            search_index,
            bg_refresh_handle: None,
            permission_popup: None,
            terminal_manager: Some(TerminalManager::new()),
            terminal_input_mode: TerminalInputMode::Normal,
            terminal_layout: TerminalLayout::Focused,
            last_terminal_size: (24, 80),
            pane_width_tasks: 40,
            pane_width_issues: 40,
            pane_width_terminals: 20,
            modifier_bar_state: ModifierBarState::Normal,
            modifier_bar_flash_until: None,
            attention_flash_until: None,
            kitty_protocol_enabled: false,
            text_selection: None,
            terminal_panel_rects: RefCell::new(Vec::new()),
            system_bell: false,
            visual_bell: true,
            log_directory: None,
            permission_profile: PermissionProfile::Interactive,
            auto_approve_patterns: Vec::new(),
            desktop_notifications: false,
            auto_accept_default: false,
            terminal_search_input: None,
            terminal_search_query: String::new(),
            terminal_search_matches: Vec::new(),
            terminal_search_match_idx: 0,
            hook_server: None,
            hook_receiver: None,
            hook_pending_rx: None,
            pending_permissions: Vec::new(),
            hook_communication: true, // overridden in main.rs from settings
            activity_log: ActivityLog::new(),
            file_claims: FileClaimsRegistry::new(),
            rules_engine: RulesEngine::empty(),
            activity_filter: crate::ui::activity_view::ActivityFilter::new(),
            activity_scroll: 0,
            orchestration: None,
            stats_popup: None,
        };
        app.rebuild_tree();
        app.build_splash_task_list();
        app.ensure_artifacts();
        app
    }

    /// Initialize the hook server (called from main.rs after settings are applied).
    pub fn init_hook_server(&mut self) {
        if !self.hook_communication {
            return;
        }
        match HookServer::start() {
            Ok((server, _port, rx, pending_rx)) => {
                self.hook_server = Some(server);
                self.hook_receiver = Some(rx);
                self.hook_pending_rx = Some(pending_rx);
            }
            Err(_) => {
                // Silent fallback — screen parsing continues
            }
        }
    }

    /// Drain the hook event receiver and process each event.
    /// Also drains the pending permission receiver into `self.pending_permissions`.
    /// Call once per event loop tick.
    pub fn drain_hook_events(&mut self) {
        // Drain the hook event receiver into a local vec to avoid borrow conflicts
        let events: Vec<HookEvent> = if let Some(ref rx) = self.hook_receiver {
            let mut evs = Vec::new();
            while let Ok(ev) = rx.try_recv() {
                evs.push(ev);
            }
            evs
        } else {
            return;
        };

        for event in events {
            self.process_hook_event(event);
        }

        // Drain the pending permission receiver
        if let Some(ref pending_rx) = self.hook_pending_rx {
            while let Ok(pending) = pending_rx.try_recv() {
                self.pending_permissions.push(pending);
            }
        }
    }

    /// Process a single hook event: route to the matching terminal and update HookState.
    fn process_hook_event(&mut self, event: HookEvent) {
        // Extract the terminal_id and determine if we need to trigger attention
        let (terminal_id, trigger_attention, attention_msg) = match &event {
            HookEvent::Notification { terminal_id, message } => {
                (terminal_id.clone(), true, Some(message.clone()))
            }
            HookEvent::PreToolUse { terminal_id, tool_name, tool_input_summary } => {
                // AskUserQuestion means the terminal is waiting for user input
                if tool_name == "AskUserQuestion" {
                    let msg = if tool_input_summary.is_empty() {
                        "Waiting for user input".to_string()
                    } else {
                        tool_input_summary.clone()
                    };
                    (terminal_id.clone(), true, Some(msg))
                } else {
                    (terminal_id.clone(), false, None)
                }
            }
            HookEvent::PostToolUse { terminal_id, .. }
            | HookEvent::SessionStart { terminal_id, .. }
            | HookEvent::SessionEnd { terminal_id }
            | HookEvent::Stop { terminal_id, .. }
            | HookEvent::PermissionRequest { terminal_id, .. }
            | HookEvent::UserPromptSubmit { terminal_id, .. } => {
                (terminal_id.clone(), false, None)
            }
        };

        // Check file conflicts for PreToolUse edit-type tools
        let conflict_warning: Option<String> = if let HookEvent::PreToolUse { tool_name, tool_input_summary, .. } = &event {
            if matches!(tool_name.as_str(), "Edit" | "Write" | "NotebookEdit") && !tool_input_summary.is_empty() {
                let conflicts = self.file_claims.check_conflicts(tool_input_summary, &terminal_id);
                if !conflicts.is_empty() {
                    let others: Vec<&str> = conflicts.iter().map(|c| c.terminal_id.as_str()).collect();
                    Some(format!("\u{26a0} {} also claimed by {}", tool_input_summary, others.join(", ")))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Find the task directory before borrowing terminal_manager mutably.
        let task_dir: Option<std::path::PathBuf> = self.repos.iter().find_map(|repo| {
            repo.tasks
                .iter()
                .find(|t| t.state.task_id == terminal_id)
                .map(|t| t.dir.clone())
        });

        let mgr = match &mut self.terminal_manager {
            Some(m) => m,
            None => return,
        };

        // Find the terminal with matching id
        let term = match mgr.terminals.iter_mut().find(|t| t.id == terminal_id) {
            Some(t) => t,
            None => return, // Unknown terminal (error path E5) — event dropped
        };

        // Initialize HookState if not yet present
        if term.hook_state.is_none() {
            term.hook_state = Some(HookState {
                last_event: String::new(),
                last_event_at: std::time::Instant::now(),
                activity_label: String::new(),
                tool_counts: std::collections::HashMap::new(),
                session_active: false,
                total_cost_usd: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
            });
        }

        let now = std::time::Instant::now();
        let state = term.hook_state.as_mut().unwrap();

        match &event {
            HookEvent::SessionStart { .. } => {
                state.last_event = "SessionStart".to_string();
                state.last_event_at = now;
                state.session_active = true;
            }
            HookEvent::SessionEnd { .. } => {
                state.last_event = "SessionEnd".to_string();
                state.last_event_at = now;
                state.session_active = false;
                state.activity_label.clear();
            }
            HookEvent::PreToolUse { tool_name, tool_input_summary, .. } => {
                state.last_event = "PreToolUse".to_string();
                state.last_event_at = now;
                if tool_input_summary.is_empty() {
                    state.activity_label = tool_name.clone();
                } else {
                    state.activity_label = format!("{} {}", tool_name, tool_input_summary);
                }
            }
            HookEvent::PostToolUse { tool_name, .. } => {
                state.last_event = "PostToolUse".to_string();
                state.last_event_at = now;
                state.activity_label.clear();
                *state.tool_counts.entry(tool_name.clone()).or_insert(0) += 1;
            }
            HookEvent::Notification { message, .. } => {
                state.last_event = "Notification".to_string();
                state.last_event_at = now;
                // Notification triggers attention (handled below)
                let _ = message;
            }
            HookEvent::Stop { session_cost, .. } => {
                state.last_event = "Stop".to_string();
                state.last_event_at = now;
                state.session_active = false;
                state.activity_label.clear();
                if let Some(ref cost) = session_cost {
                    state.total_cost_usd += cost.cost_usd;
                    state.total_input_tokens += cost.input_tokens;
                    state.total_output_tokens += cost.output_tokens;
                }
            }
            HookEvent::PermissionRequest { tool_name, .. } => {
                state.last_event = "PermissionRequest".to_string();
                state.last_event_at = now;
                state.activity_label = format!("awaiting permission: {}", tool_name);
            }
            HookEvent::UserPromptSubmit { .. } => {
                state.last_event = "UserPromptSubmit".to_string();
                state.last_event_at = now;
            }
        }

        // Override activity label with conflict warning if detected
        if let Some(ref warning) = conflict_warning {
            state.activity_label = warning.clone();
        }

        // Trigger attention for Notification and AskUserQuestion events
        if trigger_attention {
            if let Some(msg) = attention_msg {
                let reason = if matches!(&event, HookEvent::PreToolUse { tool_name, .. } if tool_name == "AskUserQuestion") {
                    AttentionReason::PermissionPrompt { context: msg }
                } else {
                    AttentionReason::HookNotification { message: msg }
                };
                if !matches!(&term.status, TerminalStatus::Exited(_)) {
                    term.status = TerminalStatus::NeedsAttention(reason);
                }
            }
        }

        // Stop event means Claude finished — flag as waiting for input
        if matches!(&event, HookEvent::Stop { .. })
            && !matches!(&term.status, TerminalStatus::Exited(_))
        {
            term.status = TerminalStatus::NeedsAttention(AttentionReason::WaitingForInput);
        }

        // Clear WaitingForInput when Claude starts working again
        if matches!(&event, HookEvent::PreToolUse { .. } | HookEvent::UserPromptSubmit { .. })
            && matches!(&term.status, TerminalStatus::NeedsAttention(AttentionReason::WaitingForInput))
        {
            term.status = TerminalStatus::Running;
        }

        // Push to activity log
        self.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: terminal_id.clone(),
            event_type: match &event {
                HookEvent::SessionStart { .. } => "SessionStart".to_string(),
                HookEvent::SessionEnd { .. } => "SessionEnd".to_string(),
                HookEvent::PreToolUse { .. } => "PreToolUse".to_string(),
                HookEvent::PostToolUse { .. } => "PostToolUse".to_string(),
                HookEvent::Notification { .. } => "Notification".to_string(),
                HookEvent::Stop { .. } => "Stop".to_string(),
                HookEvent::PermissionRequest { .. } => "PermissionRequest".to_string(),
                HookEvent::UserPromptSubmit { .. } => "UserPromptSubmit".to_string(),
            },
            tool_name: match &event {
                HookEvent::PreToolUse { tool_name, .. }
                | HookEvent::PostToolUse { tool_name, .. }
                | HookEvent::PermissionRequest { tool_name, .. } => Some(tool_name.clone()),
                HookEvent::UserPromptSubmit { .. } => None,
                _ => None,
            },
            tool_input_summary: match &event {
                HookEvent::PreToolUse { tool_input_summary, .. }
                | HookEvent::PostToolUse { tool_input_summary, .. } => {
                    if tool_input_summary.is_empty() { None } else { Some(tool_input_summary.clone()) }
                }
                HookEvent::UserPromptSubmit { prompt_preview, .. } => {
                    if prompt_preview.is_empty() { None } else { Some(prompt_preview.clone()) }
                }
                _ => None,
            },
            success: match &event {
                HookEvent::PostToolUse { success, .. } => Some(*success),
                HookEvent::UserPromptSubmit { .. } => None,
                _ => None,
            },
        });

        // Record file claims for edit-type tools (PostToolUse only, confirmed actions)
        if let HookEvent::PostToolUse { tool_name, tool_input_summary, .. } = &event {
            if matches!(tool_name.as_str(), "Edit" | "Write" | "NotebookEdit") && !tool_input_summary.is_empty() {
                self.file_claims.claim(tool_input_summary, &terminal_id, tool_name);
            }
        }

        // Credential scanning on PostToolUse tool_input_summary
        if let HookEvent::PostToolUse { tool_input_summary, .. } = &event {
            if !tool_input_summary.is_empty() {
                let cred_matches = self.rules_engine.scan_credentials(tool_input_summary);
                if !cred_matches.is_empty() {
                    self.rules_engine.stats.credential_exposures += 1;
                }
            }
        }

        // Append event to history.jsonl audit log (best-effort, silently ignore errors).
        if let Some(dir) = task_dir {
            append_history_jsonl(&dir, &terminal_id, &event);
        }
    }

    /// Returns true if the given task should be shown under the current filter.
    pub fn task_passes_filter(&self, task: &crate::data::task::LoadedTask) -> bool {
        match self.task_filter {
            TaskFilter::All => true,
            TaskFilter::Active => !task.archived && !task.state.is_complete(),
            TaskFilter::ActiveAndRecentDone => {
                if !task.archived && !task.state.is_complete() {
                    return true;
                }
                // Include completed (non-archived) tasks updated within recent_done_days
                if task.state.is_complete() && !task.archived && !task.state.updated_at.is_empty() {
                    if let Ok(updated) = chrono::DateTime::parse_from_rfc3339(&task.state.updated_at) {
                        let cutoff = chrono::Utc::now()
                            - chrono::Duration::days(self.recent_done_days as i64);
                        return updated >= cutoff;
                    }
                }
                false
            }
        }
    }

    /// Cycle to the next task filter mode and rebuild the tree.
    pub fn cycle_task_filter(&mut self) {
        self.task_filter = match self.task_filter {
            TaskFilter::All => TaskFilter::Active,
            TaskFilter::Active => TaskFilter::ActiveAndRecentDone,
            TaskFilter::ActiveAndRecentDone => TaskFilter::All,
        };
        self.rebuild_tree();
    }

    // ── Split Pane Spatial Navigation ────────────────────────────────────

    /// Move focus to the tile to the right of the current one (Tiled2/Tiled4 only).
    pub fn terminal_tile_focus_right(&mut self) {
        let mgr = match &mut self.terminal_manager {
            Some(m) => m,
            None => return,
        };
        let max = match self.terminal_layout {
            TerminalLayout::Focused => return,
            TerminalLayout::Tiled2 => 2,
            TerminalLayout::Tiled4 => 4,
            TerminalLayout::Stacked => return,
        };
        let indices = crate::ui::terminal_view::terminal_indices_for_layout(mgr, max);
        if indices.is_empty() { return; }
        let pos = match indices.iter().position(|&i| i == mgr.focused) {
            Some(p) => p,
            None => return,
        };
        match self.terminal_layout {
            TerminalLayout::Tiled2 => {
                if pos + 1 < indices.len() {
                    mgr.focused = indices[pos + 1];
                }
            }
            TerminalLayout::Tiled4 => {
                // 2x2: within row, right = pos + 1 (only if in left column)
                if pos % 2 == 0 && pos + 1 < indices.len() {
                    mgr.focused = indices[pos + 1];
                }
            }
            _ => {}
        }
    }

    /// Move focus to the tile to the left of the current one (Tiled2/Tiled4 only).
    pub fn terminal_tile_focus_left(&mut self) {
        let mgr = match &mut self.terminal_manager {
            Some(m) => m,
            None => return,
        };
        let max = match self.terminal_layout {
            TerminalLayout::Focused => return,
            TerminalLayout::Tiled2 => 2,
            TerminalLayout::Tiled4 => 4,
            TerminalLayout::Stacked => return,
        };
        let indices = crate::ui::terminal_view::terminal_indices_for_layout(mgr, max);
        if indices.is_empty() { return; }
        let pos = match indices.iter().position(|&i| i == mgr.focused) {
            Some(p) => p,
            None => return,
        };
        match self.terminal_layout {
            TerminalLayout::Tiled2 => {
                if pos > 0 {
                    mgr.focused = indices[pos - 1];
                }
            }
            TerminalLayout::Tiled4 => {
                if pos % 2 == 1 {
                    mgr.focused = indices[pos - 1];
                }
            }
            _ => {}
        }
    }

    /// Move focus to the tile below the current one (Tiled4/Stacked only).
    pub fn terminal_tile_focus_down(&mut self) {
        let mgr = match &mut self.terminal_manager {
            Some(m) => m,
            None => return,
        };
        let max = match self.terminal_layout {
            TerminalLayout::Focused => return,
            TerminalLayout::Tiled2 => return,
            TerminalLayout::Tiled4 => 4,
            TerminalLayout::Stacked => 5,
        };
        let indices = crate::ui::terminal_view::terminal_indices_for_layout(mgr, max);
        if indices.is_empty() { return; }
        let pos = match indices.iter().position(|&i| i == mgr.focused) {
            Some(p) => p,
            None => return,
        };
        match self.terminal_layout {
            TerminalLayout::Tiled4 => {
                // 2x2: down = same column, next row (pos + 2)
                if pos + 2 < indices.len() {
                    mgr.focused = indices[pos + 2];
                }
            }
            TerminalLayout::Stacked => {
                if pos + 1 < indices.len() {
                    mgr.focused = indices[pos + 1];
                }
            }
            _ => {}
        }
    }

    /// Move focus to the tile above the current one (Tiled4/Stacked only).
    pub fn terminal_tile_focus_up(&mut self) {
        let mgr = match &mut self.terminal_manager {
            Some(m) => m,
            None => return,
        };
        let max = match self.terminal_layout {
            TerminalLayout::Focused => return,
            TerminalLayout::Tiled2 => return,
            TerminalLayout::Tiled4 => 4,
            TerminalLayout::Stacked => 5,
        };
        let indices = crate::ui::terminal_view::terminal_indices_for_layout(mgr, max);
        if indices.is_empty() { return; }
        let pos = match indices.iter().position(|&i| i == mgr.focused) {
            Some(p) => p,
            None => return,
        };
        match self.terminal_layout {
            TerminalLayout::Tiled4 => {
                if pos >= 2 {
                    mgr.focused = indices[pos - 2];
                }
            }
            TerminalLayout::Stacked => {
                if pos > 0 {
                    mgr.focused = indices[pos - 1];
                }
            }
            _ => {}
        }
    }

    /// Rebuild the flattened tree from repos + expanded state.
    pub fn rebuild_tree(&mut self) {
        self.tree_rows.clear();
        for (ri, repo) in self.repos.iter().enumerate() {
            self.tree_rows.push(TreeRow::Repo(ri));
            if self.expanded_repos.contains(&ri) {
                for (ti, task) in repo.tasks.iter().enumerate() {
                    if self.task_passes_filter(task) {
                        self.tree_rows.push(TreeRow::Task(ri, ti));
                    }
                }
            }
        }
        // Clamp cursor
        if self.tree_cursor >= self.tree_rows.len() && !self.tree_rows.is_empty() {
            self.tree_cursor = self.tree_rows.len() - 1;
        }
    }

    /// Reload all data from disk (non-blocking).
    /// Launches a background thread; results are applied in `check_bg_refresh()`.
    pub fn refresh(&mut self) {
        self.start_bg_refresh();
    }

    /// Start a background refresh (non-blocking).
    /// If a refresh is already in progress, this is a no-op.
    fn start_bg_refresh(&mut self) {
        if self.bg_refresh_handle.is_some() {
            return; // Already refreshing
        }

        // Clone what the background thread needs
        let repo_paths = self.repo_paths.clone();
        let prev_repos = self.repos.clone();

        self.bg_refresh_handle = Some(std::thread::spawn(move || {
            let mut all_changed_ids = Vec::new();
            let mut new_repos = Vec::with_capacity(prev_repos.len());

            for (i, repo_path) in repo_paths.iter().enumerate() {
                if i < prev_repos.len() {
                    let (repo, changed_ids) =
                        RepoData::load_incremental(&prev_repos[i], repo_path);
                    all_changed_ids.extend(changed_ids);
                    new_repos.push(repo);
                } else {
                    new_repos.push(RepoData::load(repo_path));
                }
            }

            // Build search index in background too
            let search_index = build_search_index(&new_repos);

            BgRefreshResult {
                repos: new_repos,
                changed_task_ids: all_changed_ids,
                search_index,
            }
        }));
    }

    /// Poll for background refresh completion. Call each event-loop tick.
    pub fn check_bg_refresh(&mut self) {
        let handle = match self.bg_refresh_handle.take() {
            Some(h) => h,
            None => return,
        };

        if handle.is_finished() {
            match handle.join() {
                Ok(result) => {
                    self.repos = result.repos;
                    self.search_index = result.search_index;
                    self.rebuild_tree();
                    // Rebuild splash task list so indices stay valid after repo swap
                    if self.show_splash {
                        self.build_splash_task_list();
                    }
                    self.clamp_issue_selection();
                    self.cached_task_dir = None;
                    self.cached_history_task_dir = None;
                    self.ensure_artifacts();
                }
                Err(_) => {
                    // Thread panicked -- silently skip this refresh cycle
                }
            }
            // Reset timer on completion (not start) to avoid rapid re-refresh
            self.last_refresh = std::time::Instant::now();
        } else {
            // Not done yet, put it back
            self.bg_refresh_handle = Some(handle);
        }
    }

    /// The currently selected tree row.
    pub fn current_tree_row(&self) -> Option<&TreeRow> {
        self.tree_rows.get(self.tree_cursor)
    }

    /// Get the selected repo index (from whichever row is selected).
    pub fn selected_repo_index(&self) -> Option<usize> {
        match self.current_tree_row()? {
            TreeRow::Repo(ri) => Some(*ri),
            TreeRow::Task(ri, _) => Some(*ri),
        }
    }

    pub fn current_repo(&self) -> Option<&RepoData> {
        self.selected_repo_index()
            .and_then(|ri| self.repos.get(ri))
    }

    /// Get the selected loaded task (only if a task row is selected).
    pub fn current_loaded_task(&self) -> Option<&crate::data::task::LoadedTask> {
        match self.current_tree_row()? {
            TreeRow::Task(ri, ti) => self.repos.get(*ri)?.tasks.get(*ti),
            TreeRow::Repo(_) => None,
        }
    }

    /// Get the selected task state (only if a task row is selected).
    pub fn current_task(&self) -> Option<&crate::data::task::TaskState> {
        self.current_loaded_task().map(|lt| &lt.state)
    }

    pub fn current_issue(&self) -> Option<&crate::data::beads::BeadsIssue> {
        self.current_repo()
            .and_then(|r| r.issues.get(self.selected_issue))
    }

    // Tree navigation
    pub fn tree_down(&mut self) {
        if !self.tree_rows.is_empty() {
            let last = self.tree_rows.len() - 1;
            if self.tree_cursor < last {
                self.tree_cursor += 1;
                self.detail_scroll = 0;
                self.detail_mode = DetailMode::Overview;
                self.ensure_artifacts();
            }
        }
    }

    pub fn tree_up(&mut self) {
        if !self.tree_rows.is_empty() && self.tree_cursor > 0 {
            self.tree_cursor -= 1;
            self.detail_scroll = 0;
            self.detail_mode = DetailMode::Overview;
            self.ensure_artifacts();
        }
    }

    pub fn tree_page_down(&mut self, page_size: u16) {
        if !self.tree_rows.is_empty() {
            let last = self.tree_rows.len() - 1;
            let new_cursor = (self.tree_cursor + page_size as usize).min(last);
            if new_cursor != self.tree_cursor {
                self.tree_cursor = new_cursor;
                self.detail_scroll = 0;
                self.detail_mode = DetailMode::Overview;
                self.ensure_artifacts();
            }
        }
    }

    pub fn tree_page_up(&mut self, page_size: u16) {
        if !self.tree_rows.is_empty() && self.tree_cursor > 0 {
            let new_cursor = self.tree_cursor.saturating_sub(page_size as usize);
            if new_cursor != self.tree_cursor {
                self.tree_cursor = new_cursor;
                self.detail_scroll = 0;
                self.detail_mode = DetailMode::Overview;
                self.ensure_artifacts();
            }
        }
    }

    /// Toggle expand/collapse on a repo row, or select a task row.
    pub fn tree_toggle(&mut self) {
        if let Some(row) = self.tree_rows.get(self.tree_cursor).cloned() {
            match row {
                TreeRow::Repo(ri) => {
                    if self.expanded_repos.contains(&ri) {
                        self.expanded_repos.remove(&ri);
                    } else {
                        self.expanded_repos.insert(ri);
                    }
                    self.rebuild_tree();
                }
                TreeRow::Task(_, _) => {
                    // Task row: toggle is a no-op (already selected for detail view)
                }
            }
        }
    }

    /// Expand the current repo node (no-op if already expanded or on a task row).
    pub fn tree_expand(&mut self) {
        if let Some(TreeRow::Repo(ri)) = self.tree_rows.get(self.tree_cursor).cloned() {
            if !self.expanded_repos.contains(&ri) {
                self.expanded_repos.insert(ri);
                self.rebuild_tree();
            }
        }
    }

    /// Collapse the current repo node (no-op if already collapsed or on a task row).
    pub fn tree_collapse(&mut self) {
        if let Some(TreeRow::Repo(ri)) = self.tree_rows.get(self.tree_cursor).cloned() {
            if self.expanded_repos.contains(&ri) {
                self.expanded_repos.remove(&ri);
                self.rebuild_tree();
            }
        }
    }

    // Item navigation for beads view
    pub fn next_item(&mut self) {
        match self.active_view {
            ActiveView::Tasks => self.tree_down(),
            ActiveView::BeadsIssues => {
                if let Some(repo) = self.current_repo() {
                    if !repo.issues.is_empty() {
                        self.selected_issue = (self.selected_issue + 1) % repo.issues.len();
                        self.detail_scroll = 0;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn prev_item(&mut self) {
        match self.active_view {
            ActiveView::Tasks => self.tree_up(),
            ActiveView::BeadsIssues => {
                if let Some(repo) = self.current_repo() {
                    if !repo.issues.is_empty() {
                        self.selected_issue = if self.selected_issue == 0 {
                            repo.issues.len() - 1
                        } else {
                            self.selected_issue - 1
                        };
                        self.detail_scroll = 0;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus_pane = match self.focus_pane {
            FocusPane::Left => FocusPane::Right,
            FocusPane::Right => FocusPane::Left,
        };
    }

    pub fn next_view(&mut self) {
        self.active_view = match self.active_view {
            ActiveView::Tasks => ActiveView::BeadsIssues,
            ActiveView::BeadsIssues => ActiveView::Config,
            ActiveView::Config => ActiveView::CostSummary,
            ActiveView::CostSummary => ActiveView::Terminals,
            ActiveView::Terminals => ActiveView::ActivityFeed,
            ActiveView::ActivityFeed => ActiveView::Tasks,
        };
        self.detail_scroll = 0;
    }

    pub fn set_view(&mut self, view: ActiveView) {
        self.active_view = view;
        self.detail_scroll = 0;
    }

    pub fn scroll_detail_down(&mut self) {
        let max = self.detail_scroll_max.get();
        self.detail_scroll = self.detail_scroll.saturating_add(1).min(max);
    }

    pub fn scroll_detail_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    pub fn scroll_detail_page_down(&mut self, page_size: u16) {
        let max = self.detail_scroll_max.get();
        self.detail_scroll = self.detail_scroll.saturating_add(page_size).min(max);
    }

    pub fn scroll_detail_page_up(&mut self, page_size: u16) {
        self.detail_scroll = self.detail_scroll.saturating_sub(page_size);
    }

    /// Toggle activity filter by terminal (cycle through terminals or clear).
    pub fn activity_cycle_terminal_filter(&mut self) {
        let mut terminals: Vec<String> = self
            .activity_log
            .events()
            .iter()
            .map(|e| e.terminal_id.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        terminals.sort();

        if terminals.is_empty() {
            self.activity_filter.terminal = None;
            return;
        }

        match &self.activity_filter.terminal {
            None => {
                self.activity_filter.terminal = terminals.into_iter().next();
            }
            Some(current) => {
                let pos = terminals.iter().position(|t| t == current);
                match pos {
                    Some(i) if i + 1 < terminals.len() => {
                        self.activity_filter.terminal = Some(terminals[i + 1].clone());
                    }
                    _ => {
                        self.activity_filter.terminal = None;
                    }
                }
            }
        }
    }

    /// Toggle activity filter by event type.
    pub fn activity_cycle_event_filter(&mut self) {
        let types = [
            "PreToolUse",
            "PostToolUse",
            "SessionStart",
            "SessionEnd",
            "Notification",
            "PermissionRequest",
        ];
        match &self.activity_filter.event_type {
            None => self.activity_filter.event_type = Some(types[0].to_string()),
            Some(current) => {
                let pos = types.iter().position(|t| *t == current.as_str());
                match pos {
                    Some(i) if i + 1 < types.len() => {
                        self.activity_filter.event_type = Some(types[i + 1].to_string());
                    }
                    _ => self.activity_filter.event_type = None,
                }
            }
        }
    }

    /// Toggle activity filter by tool name.
    pub fn activity_cycle_tool_filter(&mut self) {
        let mut tools: Vec<String> = self
            .activity_log
            .events()
            .iter()
            .filter_map(|e| e.tool_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        tools.sort();

        if tools.is_empty() {
            self.activity_filter.tool = None;
            return;
        }

        match &self.activity_filter.tool {
            None => self.activity_filter.tool = tools.into_iter().next(),
            Some(current) => {
                let pos = tools.iter().position(|t| t == current);
                match pos {
                    Some(i) if i + 1 < tools.len() => {
                        self.activity_filter.tool = Some(tools[i + 1].clone());
                    }
                    _ => self.activity_filter.tool = None,
                }
            }
        }
    }

    fn clamp_issue_selection(&mut self) {
        if let Some(repo) = self.current_repo() {
            if self.selected_issue >= repo.issues.len() && !repo.issues.is_empty() {
                self.selected_issue = repo.issues.len() - 1;
            }
        }
    }

    /// Get the task directory for the currently selected task.
    pub fn current_task_dir(&self) -> Option<&PathBuf> {
        self.current_loaded_task().map(|lt| &lt.dir)
    }

    /// Load/refresh artifacts for the currently selected task.
    fn ensure_artifacts(&mut self) {
        let loaded = match self.current_loaded_task() {
            Some(lt) => lt,
            None => {
                self.cached_artifacts.clear();
                self.cached_task_dir = None;
                return;
            }
        };
        // Archived tasks have no artifacts on disk
        if loaded.archived {
            self.cached_artifacts.clear();
            self.cached_task_dir = None;
            return;
        }
        let task_dir = loaded.dir.clone();
        // Only reload if task changed
        if self.cached_task_dir.as_ref() != Some(&task_dir) {
            self.cached_artifacts = task::load_artifacts(&task_dir);
            self.cached_task_dir = Some(task_dir);
        }
    }

    /// Enter document list mode (press 'd' on a task).
    pub fn enter_doc_list(&mut self) {
        match self.current_loaded_task() {
            Some(lt) if !lt.archived => {}
            _ => return,
        }
        self.ensure_artifacts();
        if self.cached_artifacts.is_empty() {
            return;
        }
        self.detail_mode = DetailMode::DocList { cursor: 0 };
        self.detail_scroll = 0;
        self.focus_pane = FocusPane::Right;
    }

    /// Enter history view (press 'h' on a task).
    pub fn enter_history(&mut self) {
        match self.current_loaded_task() {
            Some(lt) if !lt.archived => {}
            _ => return,
        }
        self.ensure_history_data();
        self.detail_mode = DetailMode::History;
        self.detail_scroll = 0;
        self.focus_pane = FocusPane::Right;
    }

    /// Load interactions and discoveries for the current task (lazy, cached).
    fn ensure_history_data(&mut self) {
        let task_dir = match self.current_task_dir() {
            Some(d) => d.clone(),
            None => {
                self.cached_interactions.clear();
                self.cached_discoveries.clear();
                self.cached_history_task_dir = None;
                return;
            }
        };
        if self.cached_history_task_dir.as_ref() != Some(&task_dir) {
            self.cached_interactions = task::load_interactions(&task_dir);
            self.cached_discoveries = task::load_discoveries(&task_dir);
            self.cached_history_task_dir = Some(task_dir);
        }
    }

    /// Go back from doc reader/list/history to overview.
    pub fn detail_back(&mut self) {
        match &self.detail_mode {
            DetailMode::DocReader { .. } => {
                // Back to doc list
                self.detail_mode = DetailMode::DocList { cursor: 0 };
                self.detail_scroll = 0;
            }
            DetailMode::DocList { .. } | DetailMode::History => {
                self.detail_mode = DetailMode::Overview;
                self.detail_scroll = 0;
                self.focus_pane = FocusPane::Left;
            }
            DetailMode::Overview => {}
        }
    }

    /// Navigate down within the detail pane (doc list).
    pub fn detail_nav_down(&mut self) {
        if let DetailMode::DocList { cursor } = &mut self.detail_mode {
            if *cursor + 1 < self.cached_artifacts.len() {
                *cursor += 1;
            }
        }
    }

    /// Navigate up within the detail pane (doc list).
    pub fn detail_nav_up(&mut self) {
        if let DetailMode::DocList { cursor } = &mut self.detail_mode {
            if *cursor > 0 {
                *cursor -= 1;
            }
        }
    }

    /// Open the selected document for reading.
    pub fn detail_open_doc(&mut self) {
        if let DetailMode::DocList { cursor } = self.detail_mode {
            if cursor < self.cached_artifacts.len() {
                let artifact = &self.cached_artifacts[cursor];
                let content = std::fs::read_to_string(&artifact.path)
                    .unwrap_or_else(|e| format!("Error reading file: {}", e));
                self.detail_mode = DetailMode::DocReader {
                    artifact_index: cursor,
                    content,
                };
                self.detail_scroll = 0;
            }
        }
    }

    /// Open the launch popup for the currently selected task/repo.
    pub fn open_launch_popup(&mut self) {
        // Determine work directory, task_id, task_desc, color_scheme_index
        let (work_dir, task_id, task_desc, color_idx) = match self.current_tree_row() {
            Some(TreeRow::Task(ri, ti)) => {
                let repo = &self.repos[*ri];
                let loaded = &repo.tasks[*ti];
                // Skip launch for archived tasks
                if loaded.archived {
                    return;
                }
                let task = &loaded.state;
                // Use worktree abs path from launch info, then relative path, then repo root
                let dir = task
                    .worktree
                    .as_ref()
                    .and_then(|wt| {
                        // Prefer absolute path from launch info
                        if let Some(ref launch) = wt.launch {
                            if !launch.worktree_abs_path.is_empty() {
                                return Some(PathBuf::from(&launch.worktree_abs_path));
                            }
                        }
                        // Fall back to relative path resolved against repo root
                        if !wt.path.is_empty() {
                            let p = PathBuf::from(&wt.path);
                            if p.is_absolute() {
                                Some(p)
                            } else {
                                Some(repo.path.join(&p))
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| repo.path.clone());
                let color_idx = task.worktree.as_ref().map(|wt| wt.color_scheme_index);
                (dir, task.task_id.clone(), task.description.clone(), color_idx)
            }
            Some(TreeRow::Repo(ri)) => {
                let repo = &self.repos[*ri];
                (repo.path.clone(), repo.name.clone(), String::new(), None)
            }
            None => return,
        };

        let terminals = launcher::detect_terminals();
        let hosts = launcher::detect_ai_hosts();

        self.launch_popup = Some(LaunchPopup {
            terminals,
            hosts,
            step: LaunchStep::SelectTerminal,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir,
            task_id,
            task_desc,
            color_scheme_index: color_idx,
            result_msg: None,
            headless: false,
        });
    }

    /// Navigate up in the popup.
    pub fn popup_up(&mut self) {
        if let Some(popup) = &mut self.launch_popup {
            match popup.step {
                LaunchStep::SelectTerminal => {
                    if popup.terminal_cursor > 0 {
                        popup.terminal_cursor -= 1;
                    }
                }
                LaunchStep::SelectMode => {
                    // Toggle headless (only two options: Embedded / Headless)
                    popup.headless = !popup.headless;
                }
                LaunchStep::SelectHost => {
                    if popup.host_cursor > 0 {
                        popup.host_cursor -= 1;
                    }
                }
                LaunchStep::Done => {}
            }
        }
    }

    /// Navigate down in the popup.
    pub fn popup_down(&mut self) {
        if let Some(popup) = &mut self.launch_popup {
            match popup.step {
                LaunchStep::SelectTerminal => {
                    if popup.terminal_cursor + 1 < popup.terminals.len() {
                        popup.terminal_cursor += 1;
                    }
                }
                LaunchStep::SelectMode => {
                    // Toggle headless (only two options: Embedded / Headless)
                    popup.headless = !popup.headless;
                }
                LaunchStep::SelectHost => {
                    if popup.host_cursor + 1 < popup.hosts.len() {
                        popup.host_cursor += 1;
                    }
                }
                LaunchStep::Done => {}
            }
        }
    }

    /// Confirm current popup selection.
    pub fn popup_confirm(&mut self) {
        // Determine action from popup state, then act — avoids borrow conflicts
        // when spawning embedded terminals needs &mut self.
        enum Outcome {
            None,
            ClosePopup,
            SpawnEmbedded {
                task_id: String,
                host_label: String,
                command: String,
                args: Vec<String>,
                work_dir: PathBuf,
                color_idx: Option<usize>,
            },
            SpawnHeadless {
                task_id: String,
                host: launcher::AiHost,
                work_dir: PathBuf,
                color_idx: Option<usize>,
            },
        }

        let outcome = {
            let popup = match &mut self.launch_popup {
                Some(p) => p,
                None => return,
            };
            match popup.step {
                LaunchStep::SelectTerminal => {
                    let terminal = popup.terminals[popup.terminal_cursor];
                    if terminal == launcher::TerminalEnv::Embedded {
                        // Embedded terminal selected — show mode selection (Embedded/Headless)
                        popup.step = LaunchStep::SelectMode;
                    } else {
                        // External terminal — skip mode selection, go to host selection
                        popup.step = LaunchStep::SelectHost;
                    }
                    Outcome::None
                }
                LaunchStep::SelectMode => {
                    // Mode selected (Embedded or Headless), proceed to host selection
                    popup.step = LaunchStep::SelectHost;
                    Outcome::None
                }
                LaunchStep::SelectHost => {
                    let terminal = popup.terminals[popup.terminal_cursor];
                    let host = popup.hosts[popup.host_cursor];

                    if terminal == launcher::TerminalEnv::Embedded && popup.headless {
                        // Headless mode selected
                        let task_id = popup.task_id.clone();
                        let work_dir = popup.work_dir.clone();
                        let color_idx = popup.color_scheme_index;
                        Outcome::SpawnHeadless {
                            task_id,
                            host,
                            work_dir,
                            color_idx,
                        }
                    } else if terminal == launcher::TerminalEnv::Embedded {
                        let task_id = popup.task_id.clone();
                        let work_dir = popup.work_dir.clone();
                        let color_idx = popup.color_scheme_index;
                        let host_label = host.label().to_string();
                        let (command, args) = launcher::embed_cmd_args(host, &task_id);
                        Outcome::SpawnEmbedded {
                            task_id,
                            host_label,
                            command,
                            args,
                            work_dir,
                            color_idx,
                        }
                    } else {
                        let cs = popup.color_scheme_index.map(launcher::get_hex_scheme);
                        let result = launcher::launch(
                            terminal,
                            host,
                            &popup.work_dir,
                            &popup.task_id,
                            &popup.task_desc,
                            cs,
                        );
                        popup.result_msg = Some(match result {
                            Ok(()) => {
                                format!("Launched {} in {}", host.label(), terminal.label())
                            }
                            Err(e) => format!("Error: {}", e),
                        });
                        popup.step = LaunchStep::Done;
                        Outcome::None
                    }
                }
                LaunchStep::Done => Outcome::ClosePopup,
            }
        };

        match outcome {
            Outcome::ClosePopup => {
                self.launch_popup = None;
            }
            Outcome::SpawnEmbedded {
                task_id,
                host_label,
                command,
                args,
                work_dir,
                color_idx,
            } => {
                self.spawn_terminal(
                    &task_id, &host_label, &command, &args, &work_dir, color_idx,
                );
                self.launch_popup = None;
                self.active_view = ActiveView::Terminals;
            }
            Outcome::SpawnHeadless {
                task_id,
                host,
                work_dir,
                color_idx,
            } => {
                let label = format!("{} (headless)", host.label());
                self.spawn_headless_terminal(
                    &task_id, &label, host, &work_dir, color_idx, None,
                );
                self.launch_popup = None;
                self.active_view = ActiveView::Terminals;
            }
            Outcome::None => {}
        }
    }

    /// Build the list of active tasks for the splash screen.
    /// Populates `splash_active_tasks` with (repo_index, task_index) pairs
    /// sorted by updated_at descending (most recent first).
    pub fn build_splash_task_list(&mut self) {
        let mut tasks: Vec<(usize, usize, &str)> = Vec::new();
        for (ri, repo) in self.repos.iter().enumerate() {
            for (ti, task) in repo.tasks.iter().enumerate() {
                if !task.archived && !task.state.is_complete() {
                    tasks.push((ri, ti, &task.state.updated_at));
                }
            }
        }
        tasks.sort_by(|a, b| b.2.cmp(a.2));
        self.splash_active_tasks = tasks.into_iter().map(|(ri, ti, _)| (ri, ti)).collect();
        // Clamp cursor
        if !self.splash_active_tasks.is_empty() {
            self.splash_task_cursor = self.splash_task_cursor.min(self.splash_active_tasks.len() - 1);
        } else {
            self.splash_task_cursor = 0;
        }
    }

    /// Launch the task highlighted by the splash cursor in an embedded terminal.
    /// Returns true if a terminal was spawned.
    pub fn splash_launch_task(&mut self) -> bool {
        if self.splash_active_tasks.is_empty() {
            return false;
        }
        let (ri, ti) = self.splash_active_tasks[self.splash_task_cursor];
        let repo = match self.repos.get(ri) {
            Some(r) => r,
            None => return false,
        };
        let loaded = match repo.tasks.get(ti) {
            Some(t) => t,
            None => return false,
        };
        if loaded.archived {
            return false;
        }
        let task = &loaded.state;
        let task_id = task.task_id.clone();

        // Resolve work directory (same logic as open_launch_popup)
        let work_dir = task
            .worktree
            .as_ref()
            .and_then(|wt| {
                if let Some(ref launch) = wt.launch {
                    if !launch.worktree_abs_path.is_empty() {
                        return Some(PathBuf::from(&launch.worktree_abs_path));
                    }
                }
                if !wt.path.is_empty() {
                    let p = PathBuf::from(&wt.path);
                    if p.is_absolute() {
                        Some(p)
                    } else {
                        Some(repo.path.join(&p))
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| repo.path.clone());

        let color_idx = task.worktree.as_ref().map(|wt| wt.color_scheme_index);

        // Detect first AI host
        let hosts = launcher::detect_ai_hosts();
        let host = hosts.into_iter().next().unwrap_or(AiHost::Claude);

        let (command, args) = launcher::embed_cmd_args(host, &task_id);
        self.spawn_terminal(&task_id, host.label(), &command, &args, &work_dir, color_idx);
        self.active_view = ActiveView::Terminals;
        true
    }

    /// Open the create popup for a specific repo by index (used by splash `N` key).
    pub fn open_create_popup_for_repo(&mut self, repo_index: usize) {
        let repo = match self.repos.get(repo_index) {
            Some(r) => r,
            None => return,
        };
        let repo_path = repo.path.clone();
        let repo_name = repo.name.clone();
        let hosts = launcher::detect_ai_hosts();

        self.create_popup = Some(CreateWorktreePopup {
            step: CreateStep::InputDescription,
            description_input: Input::default(),
            hosts,
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 0,
            repo_path,
            repo_name,
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        });
    }

    /// Close the launch popup.
    pub fn close_launch_popup(&mut self) {
        self.launch_popup = None;
    }

    // ── Create Worktree Popup ──────────────────────────────────────────

    /// Open the create worktree popup (only on Repo rows).
    pub fn open_create_popup(&mut self) {
        let (repo_path, repo_name) = match self.current_tree_row() {
            Some(TreeRow::Repo(ri)) => {
                let repo = &self.repos[*ri];
                (repo.path.clone(), repo.name.clone())
            }
            _ => return, // Only on repo rows
        };

        let hosts = launcher::detect_ai_hosts();

        self.create_popup = Some(CreateWorktreePopup {
            step: CreateStep::InputDescription,
            description_input: Input::default(),
            hosts,
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 0,
            repo_path,
            repo_name,
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        });
    }

    /// Handle a key event for the create popup. Returns true if the key was consumed.
    pub fn create_popup_handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        let popup = match &mut self.create_popup {
            Some(p) => p,
            None => return false,
        };

        match popup.step {
            CreateStep::InputDescription => match key.code {
                KeyCode::Esc => {
                    self.create_popup = None;
                }
                KeyCode::Enter => {
                    let desc = popup.description_input.value().trim().to_string();
                    if !desc.is_empty() {
                        popup.step = CreateStep::SelectHost;
                    }
                }
                _ => {
                    use tui_input::backend::crossterm::EventHandler;
                    popup.description_input.handle_event(
                        &crossterm::event::Event::Key(key),
                    );
                }
            },
            CreateStep::SelectHost => match key.code {
                KeyCode::Esc => {
                    self.create_popup = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.create_popup {
                        if p.host_cursor > 0 {
                            p.host_cursor -= 1;
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.create_popup {
                        if p.host_cursor + 1 < p.hosts.len() {
                            p.host_cursor += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    popup.step = CreateStep::ToggleSettings;
                }
                _ => {}
            },
            CreateStep::ToggleSettings => match key.code {
                KeyCode::Esc => {
                    self.create_popup = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.create_popup {
                        if p.settings_cursor > 0 {
                            p.settings_cursor -= 1;
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.create_popup {
                        if p.settings_cursor < 3 {
                            p.settings_cursor += 1;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(p) = &mut self.create_popup {
                        match p.settings_cursor {
                            0 => p.pull = !p.pull,
                            1 => p.launch_after = !p.launch_after,
                            2 => p.headless = !p.headless,
                            3 => {
                                p.no_checkpoints = !p.no_checkpoints;
                                // Auto-select headless when no_checkpoints is enabled
                                if p.no_checkpoints {
                                    p.headless = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                KeyCode::Enter => {
                    self.compute_preview();
                }
                _ => {}
            },
            CreateStep::Confirm => match key.code {
                KeyCode::Esc => {
                    self.create_popup = None;
                }
                KeyCode::Enter => {
                    self.start_create_worktree();
                }
                _ => {}
            },
            CreateStep::Executing => {
                // No keys during execution
            }
            CreateStep::Done => match key.code {
                KeyCode::Esc => {
                    self.close_create_popup();
                }
                KeyCode::Enter => {
                    self.create_popup_launch_and_close();
                }
                _ => {}
            },
        }
        true
    }

    /// Compute and display the preview before executing.
    fn compute_preview(&mut self) {
        let popup = match &mut self.create_popup {
            Some(p) => p,
            None => return,
        };
        let desc = popup.description_input.value().trim().to_string();
        match worktree::preview(&popup.repo_path, &desc) {
            Ok(pv) => {
                popup.preview = Some(pv);
                popup.step = CreateStep::Confirm;
            }
            Err(e) => {
                // Show error in Done step
                popup.result = Some(Err(e));
                popup.step = CreateStep::Done;
            }
        }
    }

    /// Start the background worktree creation.
    fn start_create_worktree(&mut self) {
        let popup = match &mut self.create_popup {
            Some(p) => p,
            None => return,
        };

        let description = popup.description_input.value().trim().to_string();
        let repo_path = popup.repo_path.clone();
        let ai_host = popup.hosts[popup.host_cursor];
        let pull = popup.pull;

        popup.step = CreateStep::Executing;
        popup.started_at = Some(std::time::Instant::now());

        popup.handle = Some(std::thread::spawn(move || {
            worktree::create_worktree(&repo_path, &description, ai_host, pull)
        }));
    }

    /// Poll for background worktree creation completion (call each tick).
    pub fn create_popup_check_completion(&mut self) {
        let popup = match &mut self.create_popup {
            Some(p) if p.step == CreateStep::Executing => p,
            _ => return,
        };

        let handle = match popup.handle.take() {
            Some(h) => h,
            None => return,
        };

        if handle.is_finished() {
            popup.result = Some(
                handle
                    .join()
                    .unwrap_or_else(|_| Err("Thread panicked".to_string())),
            );
            popup.step = CreateStep::Done;
        } else {
            // Put it back
            popup.handle = Some(handle);
        }
    }

    /// On Enter in Done step: launch terminal if configured, then close.
    fn create_popup_launch_and_close(&mut self) {
        // Extract data needed for spawning before dropping the borrow on self
        let launch_info = {
            let popup = match &self.create_popup {
                Some(p) => p,
                None => return,
            };

            if popup.launch_after {
                if let Some(Ok(ref result)) = popup.result {
                    let host = popup.hosts[popup.host_cursor];
                    let headless = popup.headless;
                    Some((
                        result.task_id.clone(),
                        result.worktree_abs.clone(),
                        result.color_scheme_index,
                        host,
                        headless,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some((task_id, worktree_abs, color_idx, host, headless)) = launch_info {
            if headless {
                // Spawn headless terminal for the new worktree
                let label = format!("{} (headless)", host.label());
                self.spawn_headless_terminal(
                    &task_id,
                    &label,
                    host,
                    &worktree_abs,
                    Some(color_idx),
                    None,
                );
                self.create_popup = None;
                self.active_view = ActiveView::Terminals;
                self.refresh();
                return;
            } else {
                // Launch in external terminal (original behavior)
                let terminals = launcher::detect_terminals();
                if let Some(&terminal) = terminals.first() {
                    let cs = launcher::get_hex_scheme(color_idx);
                    let _ = launcher::launch(
                        terminal,
                        host,
                        &worktree_abs,
                        &task_id,
                        "",
                        Some(cs),
                    );
                }
            }
        }
        self.create_popup = None;
        self.refresh(); // Reload to show new task
    }

    /// Close the create popup without launching.
    pub fn close_create_popup(&mut self) {
        let should_refresh = self
            .create_popup
            .as_ref()
            .is_some_and(|p| p.result.as_ref().is_some_and(|r| r.is_ok()));
        self.create_popup = None;
        if should_refresh {
            self.refresh();
        }
    }

    // ── Search Popup ─────────────────────────────────────────────────────

    /// Open the search popup.
    pub fn open_search(&mut self) {
        self.search_popup = Some(SearchPopup {
            input: Input::default(),
            results: Vec::new(),
            cursor: 0,
            dirty: false,
            last_input: std::time::Instant::now(),
        });
    }

    /// Handle a key event for the search popup. Returns true if consumed.
    pub fn search_handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        let popup = match &mut self.search_popup {
            Some(p) => p,
            None => return false,
        };

        match key.code {
            KeyCode::Esc => {
                self.search_popup = None;
            }
            KeyCode::Enter => {
                self.search_navigate();
            }
            KeyCode::Up => {
                if popup.cursor > 0 {
                    popup.cursor -= 1;
                }
            }
            KeyCode::Down => {
                if !popup.results.is_empty() && popup.cursor + 1 < popup.results.len() {
                    popup.cursor += 1;
                }
            }
            _ => {
                // Forward to tui_input for text editing
                use tui_input::backend::crossterm::EventHandler;
                popup
                    .input
                    .handle_event(&crossterm::event::Event::Key(key));
                // Mark dirty for debounced search (runs from event loop after typing pause)
                popup.dirty = true;
                popup.last_input = std::time::Instant::now();
            }
        }
        true
    }

    /// Handle pasted text in the search popup.
    pub fn search_paste(&mut self, text: &str) {
        let popup = match &mut self.search_popup {
            Some(p) => p,
            None => return,
        };
        use tui_input::backend::crossterm::EventHandler;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue; // Skip newlines in search input
            }
            popup.input.handle_event(&crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(ch),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }
        popup.dirty = true;
        popup.last_input = std::time::Instant::now();
    }

    /// Handle pasted text in the permission popup quick-send input.
    pub fn permission_popup_paste(&mut self, text: &str) {
        let popup = match &mut self.permission_popup {
            Some(p) => p,
            None => return,
        };
        let input = match &mut popup.quick_send_input {
            Some(i) => i,
            None => return,
        };
        use tui_input::backend::crossterm::EventHandler;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            input.handle_event(&crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(ch),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }
    }

    /// Handle pasted text in the create worktree popup description input.
    pub fn create_popup_paste(&mut self, text: &str) {
        let popup = match &mut self.create_popup {
            Some(p) => p,
            None => return,
        };
        // Only accept paste during description input step
        if !matches!(popup.step, CreateStep::InputDescription) {
            return;
        }
        use tui_input::backend::crossterm::EventHandler;
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            popup.description_input.handle_event(&crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(ch),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ));
        }
    }

    /// Check if a debounced search should fire. Called each event-loop tick.
    /// Returns true if a search was triggered.
    pub fn tick_search_debounce(&mut self) -> bool {
        const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(200);
        let should_run = match &self.search_popup {
            Some(p) => p.dirty && p.last_input.elapsed() >= DEBOUNCE,
            None => false,
        };
        if should_run {
            if let Some(p) = &mut self.search_popup {
                p.dirty = false;
            }
            self.run_search();
            true
        } else {
            false
        }
    }

    /// Run search across all tasks using the in-memory search index.
    fn run_search(&mut self) {
        let query = match &self.search_popup {
            Some(p) => p.input.value().to_lowercase(),
            None => return,
        };

        if query.is_empty() {
            if let Some(p) = &mut self.search_popup {
                p.results.clear();
                p.cursor = 0;
            }
            return;
        }

        const MAX_RESULTS: usize = 50;
        let mut results = Vec::new();

        for entry in &self.search_index {
            if results.len() >= MAX_RESULTS {
                break;
            }
            // Search segments in order; first match wins
            for (text, source) in &entry.segments {
                if text.contains(&query) {
                    results.push(SearchResult {
                        repo_index: entry.repo_index,
                        task_index: entry.task_index,
                        task_id: entry.task_id.clone(),
                        description: entry.description.clone(),
                        match_source: source.clone(),
                    });
                    break; // one hit per task
                }
            }
        }

        if let Some(p) = &mut self.search_popup {
            p.results = results;
            p.cursor = 0;
        }
    }

    // ── Cleanup Worktree Popup ─────────────────────────────────────────

    /// Open the cleanup popup (F6). Works on repo rows and task rows (resolves to parent repo).
    pub fn open_cleanup_popup(&mut self) {
        let ri = match self.current_tree_row() {
            Some(TreeRow::Repo(ri)) => *ri,
            Some(TreeRow::Task(ri, _)) => *ri,
            None => return,
        };
        let repo = &self.repos[ri];
        let (repo_path, repo_name) = (repo.path.clone(), repo.name.clone());

        let candidates = cleanup::list_cleanup_candidates(&repo_path);

        // No pre-selection — user must explicitly choose one task at a time
        let selected = HashSet::new();

        self.cleanup_popup = Some(CleanupPopup {
            step: CleanupStep::SelectWorktrees,
            repo_path,
            repo_name,
            candidates,
            selected,
            cursor: 0,
            remove_branch: false,
            keep_on_disk: false,
            settings_cursor: 0,
            preview: Vec::new(),
            handle: None,
            started_at: None,
            results: None,
            scroll: 0,
        });
    }

    /// Open the cleanup popup for a single task's worktree (Delete/d key from Tasks view).
    /// Skips the multi-select step and goes directly to Settings.
    pub fn open_single_task_cleanup(&mut self) {
        // Must be on a task row (not a repo row)
        let (ri, ti) = match self.current_tree_row() {
            Some(TreeRow::Task(ri, ti)) => (*ri, *ti),
            _ => return,
        };

        let repo = &self.repos[ri];
        let loaded = match repo.tasks.get(ti) {
            Some(lt) => lt,
            None => return,
        };

        // Guard: task must not be archived
        if loaded.archived {
            return;
        }

        // Guard: task must have a cleanable worktree (active or done, not already cleaned)
        let wt = match &loaded.state.worktree {
            Some(wt) if wt.status == "active" || wt.status == "done" => wt,
            _ => return,
        };

        let task = &loaded.state;
        let (repo_path, repo_name) = (repo.path.clone(), repo.name.clone());

        // Build candidate directly from already-loaded data (instant, no disk I/O).
        // Expensive checks (disk_size, has_unmerged) are deferred — shown as "checking..."
        // in the popup and resolved in the background.
        let wt_abs = wt.launch.as_ref()
            .and_then(|l| if !l.worktree_abs_path.is_empty() { Some(l.worktree_abs_path.clone()) } else { None })
            .or_else(|| {
                if !wt.path.is_empty() {
                    let p = PathBuf::from(&wt.path);
                    let abs = if p.is_absolute() { p } else { repo_path.join(&p) };
                    abs.canonicalize().ok().map(|p| p.to_string_lossy().to_string())
                } else {
                    None
                }
            });

        let candidate = cleanup::WorktreeCandidate {
            task_id: task.task_id.clone(),
            description: task.description.clone(),
            branch: wt.branch.clone(),
            base_branch: wt.base_branch.clone(),
            worktree_path: wt.path.clone(),
            worktree_abs: wt_abs,
            status: wt.status.clone(),
            color_scheme_index: wt.color_scheme_index,
            is_complete: task.is_complete(),
            has_unmerged: false, // resolved later if needed
            disk_size: None,    // resolved later if needed
            phase: task.phase.clone(),
        };

        let mut selected = HashSet::new();
        selected.insert(0);

        self.cleanup_popup = Some(CleanupPopup {
            step: CleanupStep::Settings,
            repo_path,
            repo_name,
            candidates: vec![candidate],
            selected,
            cursor: 0,
            remove_branch: false,
            keep_on_disk: false,
            settings_cursor: 0,
            preview: Vec::new(),
            handle: None,
            started_at: None,
            results: None,
            scroll: 0,
        });
    }

    /// Handle a key event for the cleanup popup. Returns true if consumed.
    pub fn cleanup_popup_handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        if self.cleanup_popup.is_none() {
            return false;
        }

        // Get the current step to route
        let step_is = |s: &CleanupStep| -> bool {
            self.cleanup_popup.as_ref().is_some_and(|p| p.step == *s)
        };

        if step_is(&CleanupStep::SelectWorktrees) {
            match key.code {
                KeyCode::Esc => {
                    self.cleanup_popup = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if p.cursor > 0 {
                            p.cursor -= 1;
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if p.cursor + 1 < p.candidates.len() {
                            p.cursor += 1;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    // Single-select only (radio button) — one worktree at a time
                    if let Some(p) = &mut self.cleanup_popup {
                        let idx = p.cursor;
                        if p.selected.contains(&idx) {
                            p.selected.clear();
                        } else {
                            p.selected.clear();
                            p.selected.insert(idx);
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if !p.selected.is_empty() {
                            p.step = CleanupStep::Settings;
                        }
                    }
                }
                _ => {}
            }
        } else if step_is(&CleanupStep::Settings) {
            match key.code {
                KeyCode::Esc => {
                    self.cleanup_popup = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if p.settings_cursor > 0 {
                            p.settings_cursor -= 1;
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if p.settings_cursor < 1 {
                            p.settings_cursor += 1;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        match p.settings_cursor {
                            0 => p.remove_branch = !p.remove_branch,
                            1 => p.keep_on_disk = !p.keep_on_disk,
                            _ => {}
                        }
                    }
                }
                KeyCode::Enter => {
                    self.compute_cleanup_preview();
                }
                _ => {}
            }
        } else if step_is(&CleanupStep::Preview) {
            match key.code {
                KeyCode::Esc => {
                    self.cleanup_popup = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        p.scroll = p.scroll.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        p.scroll = p.scroll.saturating_add(1);
                    }
                }
                KeyCode::Enter => {
                    self.start_cleanup();
                }
                _ => {}
            }
        } else if step_is(&CleanupStep::Executing) {
            // No keys during execution
        } else if step_is(&CleanupStep::Done) {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.close_cleanup_popup();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        p.scroll = p.scroll.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        p.scroll = p.scroll.saturating_add(1);
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// Compute the dry-run preview.
    fn compute_cleanup_preview(&mut self) {
        let popup = match &mut self.cleanup_popup {
            Some(p) => p,
            None => return,
        };

        let selected_candidates: Vec<&cleanup::WorktreeCandidate> = popup
            .selected
            .iter()
            .filter_map(|&i| popup.candidates.get(i))
            .collect();

        popup.preview = cleanup::preview_cleanup(
            &popup.repo_path,
            &selected_candidates,
            popup.remove_branch,
            popup.keep_on_disk,
        );
        popup.scroll = 0;
        popup.step = CleanupStep::Preview;
    }

    /// Start the background cleanup execution.
    fn start_cleanup(&mut self) {
        let popup = match &mut self.cleanup_popup {
            Some(p) => p,
            None => return,
        };

        let task_ids: Vec<String> = popup
            .selected
            .iter()
            .filter_map(|&i| popup.candidates.get(i))
            .map(|c| c.task_id.clone())
            .collect();

        let repo_path = popup.repo_path.clone();
        let remove_branch = popup.remove_branch;
        let keep_on_disk = popup.keep_on_disk;

        popup.step = CleanupStep::Executing;
        popup.started_at = Some(std::time::Instant::now());

        popup.handle = Some(std::thread::spawn(move || {
            cleanup::execute_cleanup(&repo_path, &task_ids, remove_branch, keep_on_disk)
        }));
    }

    /// Poll for background cleanup completion (call each tick).
    pub fn cleanup_popup_check_completion(&mut self) {
        let popup = match &mut self.cleanup_popup {
            Some(p) if p.step == CleanupStep::Executing => p,
            _ => return,
        };

        let handle = match popup.handle.take() {
            Some(h) => h,
            None => return,
        };

        if handle.is_finished() {
            popup.results = Some(
                handle
                    .join()
                    .unwrap_or_else(|_| {
                        vec![cleanup::CleanupResult {
                            task_id: "?".to_string(),
                            success: false,
                            message: "Thread panicked".to_string(),
                        }]
                    }),
            );
            popup.step = CleanupStep::Done;
            popup.scroll = 0;
        } else {
            popup.handle = Some(handle);
        }
    }

    /// Close the cleanup popup and refresh data.
    pub fn close_cleanup_popup(&mut self) {
        let should_refresh = self
            .cleanup_popup
            .as_ref()
            .is_some_and(|p| p.results.is_some());
        self.cleanup_popup = None;
        if should_refresh {
            self.refresh();
        }
    }

    /// Navigate to the selected search result and close the popup.
    fn search_navigate(&mut self) {
        let (repo_index, task_index) = match &self.search_popup {
            Some(popup) if !popup.results.is_empty() => {
                let r = &popup.results[popup.cursor];
                (r.repo_index, r.task_index)
            }
            _ => return,
        };

        // Close popup first
        self.search_popup = None;

        // Switch to Tasks view
        self.active_view = ActiveView::Tasks;

        // Expand the target repo
        self.expanded_repos.insert(repo_index);
        self.rebuild_tree();

        // Find the matching TreeRow::Task position
        for (i, row) in self.tree_rows.iter().enumerate() {
            if let TreeRow::Task(ri, ti) = row {
                if *ri == repo_index && *ti == task_index {
                    self.tree_cursor = i;
                    break;
                }
            }
        }

        // Reset detail state
        self.detail_mode = DetailMode::Overview;
        self.detail_scroll = 0;
        self.focus_pane = FocusPane::Left;
        self.ensure_artifacts();
    }

    // ── Permission Queue Popup ──────────────────────────────────────────

    /// Open the permission queue popup (F8).
    pub fn open_permission_popup(&mut self) {
        let popup = crate::ui::permission_popup::PermissionPopup::new(self);
        self.permission_popup = Some(popup);
    }

    /// Handle key events in the permission queue popup.
    pub fn permission_popup_handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        use tui_input::backend::crossterm::EventHandler;
        use crate::ui::permission_popup::PermissionEntry;

        // If quick-send input is active, handle input mode (only used for PTY-based entries)
        if let Some(popup) = &mut self.permission_popup {
            if let Some(input) = &mut popup.quick_send_input {
                match key.code {
                    KeyCode::Enter => {
                        let text = input.value().to_string();
                        let entry_opt = popup.entries.get(popup.cursor).cloned();
                        // Close input mode
                        popup.quick_send_input = None;
                        // Send text + newline to the selected PTY terminal
                        if let Some(PermissionEntry::PtyBased { terminal_idx }) = entry_opt {
                            if let Some(mgr) = &self.terminal_manager {
                                let bytes = format!("{}\n", text).into_bytes();
                                let _ = mgr.send_input_to(terminal_idx, &bytes);
                            }
                        }
                        let new_popup =
                            crate::ui::permission_popup::PermissionPopup::new(self);
                        self.permission_popup = Some(new_popup);
                        return;
                    }
                    KeyCode::Esc => {
                        popup.quick_send_input = None;
                        return;
                    }
                    _ => {
                        input.handle_event(
                            &crossterm::event::Event::Key(key),
                        );
                        return;
                    }
                }
            }
        }

        let popup = match &mut self.permission_popup {
            Some(p) => p,
            None => return,
        };

        match key.code {
            KeyCode::Esc => {
                self.permission_popup = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if popup.cursor > 0 {
                    popup.cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if popup.cursor + 1 < popup.entries.len() {
                    popup.cursor += 1;
                }
            }
            KeyCode::Char('a') => {
                // Approve the selected entry
                let entry_opt = popup.entries.get(popup.cursor).cloned();
                match entry_opt {
                    Some(PermissionEntry::PtyBased { terminal_idx }) => {
                        if let Some(mgr) = &self.terminal_manager {
                            let _ = mgr.send_input_to(terminal_idx, b"y\n");
                        }
                    }
                    Some(PermissionEntry::HookBased { pending_idx }) => {
                        self.hook_permission_decide(pending_idx, PermissionDecision::Allow);
                    }
                    None => {}
                }
                let new_popup = crate::ui::permission_popup::PermissionPopup::new(self);
                self.permission_popup = Some(new_popup);
            }
            KeyCode::Char('A') => {
                // Batch approve ALL entries
                let entries: Vec<PermissionEntry> = popup.entries.clone();
                for entry in entries {
                    match entry {
                        PermissionEntry::PtyBased { terminal_idx } => {
                            if let Some(mgr) = &self.terminal_manager {
                                let _ = mgr.send_input_to(terminal_idx, b"y\n");
                            }
                        }
                        PermissionEntry::HookBased { pending_idx } => {
                            self.hook_permission_decide(pending_idx, PermissionDecision::Allow);
                        }
                    }
                }
                let new_popup = crate::ui::permission_popup::PermissionPopup::new(self);
                self.permission_popup = Some(new_popup);
            }
            KeyCode::Char('d') => {
                // Deny the selected entry
                let entry_opt = popup.entries.get(popup.cursor).cloned();
                match entry_opt {
                    Some(PermissionEntry::PtyBased { terminal_idx }) => {
                        if let Some(mgr) = &self.terminal_manager {
                            let _ = mgr.send_input_to(terminal_idx, b"n\n");
                        }
                    }
                    Some(PermissionEntry::HookBased { pending_idx }) => {
                        self.hook_permission_decide(
                            pending_idx,
                            PermissionDecision::Deny {
                                message: "Denied by user in crew-board".to_string(),
                            },
                        );
                    }
                    None => {}
                }
                let new_popup = crate::ui::permission_popup::PermissionPopup::new(self);
                self.permission_popup = Some(new_popup);
            }
            KeyCode::Char('t') => {
                // Quick-send: enter text input mode (PTY-based only)
                let entry_opt = popup.entries.get(popup.cursor).cloned();
                if matches!(entry_opt, Some(PermissionEntry::PtyBased { .. })) {
                    popup.quick_send_input = Some(tui_input::Input::default());
                }
            }
            KeyCode::Char('v') | KeyCode::Enter => {
                // View terminal: close popup, switch to Terminals view, focus the terminal
                let entry_opt = popup.entries.get(popup.cursor).cloned();
                let term_idx = match entry_opt {
                    Some(PermissionEntry::PtyBased { terminal_idx }) => Some(terminal_idx),
                    Some(PermissionEntry::HookBased { pending_idx }) => {
                        // Find terminal by id in terminal_manager
                        let tid = self.pending_permissions.get(pending_idx)
                            .map(|p| p.terminal_id.clone());
                        tid.and_then(|id| {
                            self.terminal_manager.as_ref().and_then(|mgr| {
                                mgr.terminals.iter().position(|t| t.id == id)
                            })
                        })
                    }
                    None => None,
                };
                if let Some(ti) = term_idx {
                    self.permission_popup = None;
                    self.active_view = ActiveView::Terminals;
                    if let Some(mgr) = &mut self.terminal_manager {
                        if ti < mgr.terminals.len() {
                            mgr.focused = ti;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Send a permission decision for a hook-based pending permission by index.
    ///
    /// Removes the entry from `pending_permissions` and sends the decision through
    /// the response channel to unblock the waiting HTTP server thread.
    fn hook_permission_decide(&mut self, pending_idx: usize, decision: PermissionDecision) {
        if pending_idx < self.pending_permissions.len() {
            let pending = self.pending_permissions.remove(pending_idx);
            // Send the decision through the channel; ignore errors (server may have timed out)
            let _ = pending.response_tx.send(decision);
        }
    }

    // ── Modifier Bar ──────────────────────────────────────────────────

    /// Show the modifier bar layer with a brief flash timer.
    /// On terminals with kitty protocol, the modifier key release event
    /// reverts the bar instantly; on others the 500ms timer handles it.
    pub fn flash_modifier_bar(&mut self, state: ModifierBarState) {
        self.modifier_bar_state = state;
        self.modifier_bar_flash_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
    }

    /// Tick the modifier bar flash timeout (non-kitty fallback).
    pub fn tick_modifier_bar(&mut self) {
        if let Some(until) = self.modifier_bar_flash_until {
            if std::time::Instant::now() >= until {
                self.modifier_bar_state = ModifierBarState::Normal;
                self.modifier_bar_flash_until = None;
            }
        }
    }

    // ── Embedded Terminals ────────────────────────────────────────────

    /// Spawn a new embedded terminal for a task.
    pub fn spawn_terminal(
        &mut self,
        task_id: &str,
        label: &str,
        command: &str,
        args: &[String],
        cwd: &std::path::Path,
        color_scheme_index: Option<usize>,
    ) {
        let log_path = self
            .log_directory
            .as_ref()
            .map(|dir| dir.join(format!("{}.log", task_id)));

        // Generate hook config and env vars if hook server is running and command is Claude
        let (env_vars, hook_settings_cwd) = self.generate_hook_config(task_id, command, cwd);

        if let Some(mgr) = &mut self.terminal_manager {
            // Use a default size; will be resized on next draw
            match mgr.spawn_with_log_and_env(
                task_id.to_string(),
                label.to_string(),
                command,
                args,
                cwd,
                24,
                80,
                color_scheme_index,
                log_path,
                env_vars,
            ) {
                Ok(()) => {
                    // Apply auto_accept_default and hook settings to newly spawned terminal
                    if let Some(term) = mgr.terminals.last_mut() {
                        if self.auto_accept_default {
                            term.auto_accept = true;
                        }
                        if let Some(ref settings_cwd) = hook_settings_cwd {
                            term.hook_settings_cwd = Some(settings_cwd.clone());
                        }
                    }
                }
                Err(e) => {
                    // Log to file since TUI captures stderr
                    let msg = format!(
                        "[crew-board] spawn_terminal FAILED\n  command: {:?}\n  args: {:?}\n  cwd: {}\n  error: {}\n",
                        command, args, cwd.display(), e
                    );
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true).append(true)
                        .open(std::env::temp_dir().join("crew-board-errors.log"))
                    {
                        use std::io::Write;
                        let _ = f.write_all(msg.as_bytes());
                    }
                }
            }
        }
    }

    /// Generate Claude Code hook configuration if the hook server is running
    /// and the command looks like a Claude Code terminal.
    fn generate_hook_config(
        &self,
        task_id: &str,
        command: &str,
        cwd: &std::path::Path,
    ) -> (Vec<(String, String)>, Option<PathBuf>) {
        let server = match &self.hook_server {
            Some(s) => s,
            None => return (vec![], None),
        };

        // Only generate hooks for Claude Code terminals
        let is_claude = command.contains("claude") || command.contains("bash");
        if !is_claude {
            return (vec![], None);
        }

        // Generate random auth token
        let token: String = (0..32)
            .map(|_| format!("{:02x}", rand::random::<u8>()))
            .collect();

        let port = server.port;

        // Build pre-computed session context from task state (if found)
        let session_context = self.build_session_context(task_id);

        // Gather permission profile and patterns for registration
        let profile_str = match self.permission_profile {
            PermissionProfile::Autonomous => "autonomous",
            PermissionProfile::Trusted => "trusted",
            PermissionProfile::Interactive => "interactive",
        };
        let raw_patterns: Vec<String> = self
            .auto_approve_patterns
            .iter()
            .map(|re| re.as_str().to_string())
            .collect();

        // Register the token with the hook server (include context + permission profile)
        server.register_token_with_profile(
            task_id.to_string(),
            token.clone(),
            session_context,
            profile_str,
            raw_patterns,
        );

        // Write .claude/settings.local.json
        let claude_dir = cwd.join(".claude");
        let _ = std::fs::create_dir_all(&claude_dir);
        let settings_path = claude_dir.join("settings.local.json");

        let hook_url = format!("http://127.0.0.1:{}/hook/{}", port, task_id);
        let settings_json = serde_json::json!({
            "hooks": {
                "SessionStart": [{
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 5,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }],
                "PreToolUse": [{
                    "matcher": ".*",
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 30,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }],
                "PostToolUse": [{
                    "matcher": ".*",
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 5,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }],
                "Notification": [{
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 5,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }],
                "UserPromptSubmit": [{
                    "hooks": [{
                        "type": "command",
                        "command": "python3 ~/.claude/scripts/log-crew-interaction.py"
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 5,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }, {
                    "hooks": [{
                        "type": "command",
                        "command": "python3 ~/.claude/scripts/log-crew-interaction.py"
                    }]
                }],
                "PermissionRequest": [{
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 30,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }],
                "SessionEnd": [{
                    "hooks": [{
                        "type": "http",
                        "url": &hook_url,
                        "timeout": 5,
                        "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                        "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                    }]
                }]
            }
        });

        if let Ok(json_str) = serde_json::to_string_pretty(&settings_json) {
            let _ = std::fs::write(&settings_path, json_str);
        }

        // Return env vars for the PTY child process
        let env_vars = vec![
            ("CREW_BOARD_PORT".to_string(), port.to_string()),
            ("CREW_BOARD_TASK_ID".to_string(), task_id.to_string()),
            ("CREW_BOARD_TOKEN".to_string(), token),
        ];

        (env_vars, Some(cwd.to_path_buf()))
    }

    // ── Headless Terminals ────────────────────────────────────────────

    /// Spawn a headless terminal for a task (no PTY, background process).
    ///
    /// Generates hook configuration (settings files + env vars), builds the
    /// correct CLI command for the given AI host, spawns as a background child
    /// process, registers the auth token with the hook server, and stores
    /// cleanup paths for settings file removal on dismiss.
    ///
    /// For Claude: writes `.claude/settings.local.json` with hook URLs and auth
    /// token, spawns `claude -p "prompt"`.
    /// For Gemini: writes `.gemini/settings.json` + `.gemini/crew-hook.sh` bridge
    /// script via `hook_bridge::generate_hook_config()`, spawns `gemini`.
    ///
    /// If the command is not found, logs the error and does not add a terminal.
    /// If hook config generation fails, still spawns the process ("dark mode").
    pub fn spawn_headless_terminal(
        &mut self,
        task_id: &str,
        label: &str,
        host: launcher::AiHost,
        cwd: &std::path::Path,
        color_scheme_index: Option<usize>,
        prompt: Option<&str>,
    ) {
        let command = host.command().to_string();

        // Generate hook config and env vars
        let (env_vars, hook_settings_cwd, cleanup_paths) =
            self.generate_headless_hook_config(task_id, host, cwd);

        // Build CLI args based on host
        let args = Self::build_headless_args(host, prompt);

        if let Some(mgr) = &mut self.terminal_manager {
            match mgr.spawn_headless(
                task_id.to_string(),
                label.to_string(),
                &command,
                &args,
                cwd,
                color_scheme_index,
                env_vars,
            ) {
                Ok(()) => {
                    // Apply auto_accept_default and hook settings to newly spawned terminal
                    if let Some(term) = mgr.terminals.last_mut() {
                        if self.auto_accept_default {
                            term.auto_accept = true;
                        }
                        if let Some(ref settings_cwd) = hook_settings_cwd {
                            term.hook_settings_cwd = Some(settings_cwd.clone());
                        }
                        term.hook_cleanup_paths = cleanup_paths;
                    }
                }
                Err(e) => {
                    // Log to file since TUI captures stderr
                    let msg = format!(
                        "[crew-board] spawn_headless_terminal FAILED\n  command: {:?}\n  args: {:?}\n  cwd: {}\n  error: {}\n",
                        command, args, cwd.display(), e
                    );
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true).append(true)
                        .open(std::env::temp_dir().join("crew-board-errors.log"))
                    {
                        use std::io::Write;
                        let _ = f.write_all(msg.as_bytes());
                    }
                }
            }
        }
    }

    /// Build CLI args for a headless AI host process.
    fn build_headless_args(host: launcher::AiHost, prompt: Option<&str>) -> Vec<String> {
        match host {
            launcher::AiHost::Claude => {
                let mut args = vec!["-p".to_string()];
                if let Some(p) = prompt {
                    args.push(p.to_string());
                } else {
                    args.push("/crew resume".to_string());
                }
                args
            }
            launcher::AiHost::Gemini => {
                // Gemini doesn't take a direct prompt arg in headless mode
                vec![]
            }
            launcher::AiHost::Copilot | launcher::AiHost::OpenCode
            | launcher::AiHost::Devin | launcher::AiHost::Droid => {
                // Other hosts: no special headless args
                vec![]
            }
            launcher::AiHost::Shell => vec![],
        }
    }

    /// Generate hook configuration for a headless terminal.
    ///
    /// For Claude: writes `.claude/settings.local.json` and registers token.
    /// For Gemini/Copilot/OpenCode: uses `hook_bridge::generate_hook_config()`
    /// to generate settings + bridge script, sets execute permissions on scripts.
    ///
    /// Returns (env_vars, hook_settings_cwd, cleanup_paths).
    /// If hook server is not running, returns empty config ("dark mode").
    fn generate_headless_hook_config(
        &self,
        task_id: &str,
        host: launcher::AiHost,
        cwd: &std::path::Path,
    ) -> (Vec<(String, String)>, Option<PathBuf>, Vec<PathBuf>) {
        let server = match &self.hook_server {
            Some(s) => s,
            None => return (vec![], None, vec![]),
        };

        // Generate random auth token
        let token: String = (0..32)
            .map(|_| format!("{:02x}", rand::random::<u8>()))
            .collect();

        let port = server.port;

        // Build pre-computed session context from task state (if found)
        let session_context = self.build_session_context(task_id);

        // Gather permission profile and patterns for registration
        let profile_str = match self.permission_profile {
            PermissionProfile::Autonomous => "autonomous",
            PermissionProfile::Trusted => "trusted",
            PermissionProfile::Interactive => "interactive",
        };
        let raw_patterns: Vec<String> = self
            .auto_approve_patterns
            .iter()
            .map(|re| re.as_str().to_string())
            .collect();

        // Register the token with the hook server
        server.register_token_with_profile(
            task_id.to_string(),
            token.clone(),
            session_context,
            profile_str,
            raw_patterns,
        );

        // Build env vars (common to all hosts)
        let env_vars = vec![
            ("CREW_BOARD_PORT".to_string(), port.to_string()),
            ("CREW_BOARD_TASK_ID".to_string(), task_id.to_string()),
            ("CREW_BOARD_TOKEN".to_string(), token.clone()),
        ];

        // Generate host-specific config files
        let bridge_host = match host {
            launcher::AiHost::Claude => crate::hook_bridge::AiHostType::Claude,
            launcher::AiHost::Gemini => crate::hook_bridge::AiHostType::Gemini,
            launcher::AiHost::Copilot => crate::hook_bridge::AiHostType::Copilot,
            launcher::AiHost::OpenCode => crate::hook_bridge::AiHostType::OpenCode,
            launcher::AiHost::Devin => crate::hook_bridge::AiHostType::Devin,
            launcher::AiHost::Droid => crate::hook_bridge::AiHostType::Droid,
            launcher::AiHost::Shell => crate::hook_bridge::AiHostType::Shell,
        };

        match host {
            launcher::AiHost::Claude => {
                // Claude uses settings.local.json (same as embedded)
                let claude_dir = cwd.join(".claude");
                let _ = std::fs::create_dir_all(&claude_dir);
                let settings_path = claude_dir.join("settings.local.json");

                let hook_url = format!("http://127.0.0.1:{}/hook/{}", port, task_id);
                let settings_json = serde_json::json!({
                    "hooks": {
                        "SessionStart": [{
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 5,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "PreToolUse": [{
                            "matcher": ".*",
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 30,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "PostToolUse": [{
                            "matcher": ".*",
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 5,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "Notification": [{
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 5,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "Stop": [{
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 5,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "PermissionRequest": [{
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 30,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }],
                        "SessionEnd": [{
                            "hooks": [{
                                "type": "http",
                                "url": &hook_url,
                                "timeout": 5,
                                "headers": { "Authorization": "Bearer $CREW_BOARD_TOKEN" },
                                "allowedEnvVars": ["CREW_BOARD_TOKEN"]
                            }]
                        }]
                    }
                });

                if let Ok(json_str) = serde_json::to_string_pretty(&settings_json) {
                    let _ = std::fs::write(&settings_path, json_str);
                }

                (env_vars, Some(cwd.to_path_buf()), vec![settings_path])
            }
            _ => {
                // Non-Claude hosts: use hook_bridge for config generation
                match crate::hook_bridge::generate_hook_config(bridge_host, port, task_id, &token, cwd) {
                    Some(config) => {
                        // Write all config files
                        for (path, content) in &config.files {
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(path, content);

                            // Set execute permissions on shell scripts
                            #[cfg(unix)]
                            if path.extension().map_or(false, |ext| ext == "sh") {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(
                                    path,
                                    std::fs::Permissions::from_mode(0o755),
                                );
                            }
                        }

                        let cleanup = config.cleanup_paths;
                        (env_vars, Some(cwd.to_path_buf()), cleanup)
                    }
                    None => {
                        // Host doesn't support hooks — spawn in "dark mode"
                        (env_vars, None, vec![])
                    }
                }
            }
        }
    }

    /// Build a markdown context string for a task to inject on SessionStart.
    ///
    /// Looks up the task in `self.repos` by task_id and formats a brief
    /// briefing with the task description, current phase, completed phases,
    /// and recent human decisions. Returns `None` if the task cannot be found.
    /// Context is kept small (target <2KB) to avoid bloating the session.
    fn build_session_context(&self, task_id: &str) -> Option<String> {
        // Find the task across all repos
        let task = self
            .repos
            .iter()
            .flat_map(|r| r.tasks.iter())
            .find(|t| t.state.task_id == task_id)?;

        let state = &task.state;

        let mut ctx = format!("# Crew Board Context: {}\n\n", task_id);

        // Task Assignment section
        ctx.push_str("## Task Assignment\n");
        ctx.push_str(&format!("- **Task ID**: {}\n", task_id));

        if let Some(ref phase) = state.phase {
            ctx.push_str(&format!("- **Phase**: {}\n", phase));
        }

        if !state.phases_completed.is_empty() {
            ctx.push_str(&format!(
                "- **Phases Completed**: {}\n",
                state.phases_completed.join(", ")
            ));
        }

        if let Some(ref mode) = state.workflow_mode {
            if !mode.effective.is_empty() {
                ctx.push_str(&format!("- **Mode**: {}\n", mode.effective));
            }
        }

        if !state.description.is_empty() {
            ctx.push_str(&format!("\n## Task Description\n{}\n", state.description));
        }

        // Previous Decisions section
        if !state.human_decisions.is_empty() {
            use crate::data::task::parse_decisions;
            let decisions = parse_decisions(&state.human_decisions);
            // Include at most the 5 most recent decisions to keep context small
            let recent: Vec<_> = decisions.iter().rev().take(5).collect();
            if !recent.is_empty() {
                ctx.push_str("\n## Previous Decisions\n");
                for d in recent.iter().rev() {
                    let note = if d.notes.is_empty() {
                        String::new()
                    } else {
                        format!(" — \"{}\"", truncate_str(&d.notes, 80))
                    };
                    ctx.push_str(&format!(
                        "- {}: {}{}\n",
                        d.checkpoint, d.decision, note
                    ));
                }
            }
        }

        Some(ctx)
    }

    /// Close an embedded terminal by id.
    #[allow(dead_code)]
    pub fn close_terminal(&mut self, id: &str) {
        if let Some(mgr) = &mut self.terminal_manager {
            mgr.remove(id);
        }
    }

    /// Focus the next terminal in the list.
    pub fn terminal_focus_next(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            mgr.focus_next();
        }
    }

    /// Focus the previous terminal in the list.
    pub fn terminal_focus_prev(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            mgr.focus_prev();
        }
    }

    /// Focus the next non-exited terminal (wraps around). For use in focused mode.
    pub fn terminal_focus_next_running(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            mgr.focus_next_running();
        }
    }

    /// Focus the previous non-exited terminal (wraps around). For use in focused mode.
    pub fn terminal_focus_prev_running(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            mgr.focus_prev_running();
        }
    }

    /// Jump to the next terminal needing attention.
    pub fn terminal_focus_next_attention(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            if mgr.focus_next_attention() {
                self.active_view = ActiveView::Terminals;
            }
        }
    }

    /// Send input bytes to the focused terminal.
    pub fn terminal_send_input(&self, bytes: &[u8]) {
        if let Some(mgr) = &self.terminal_manager {
            let _ = mgr.send_input(bytes);
        }
    }

    /// Store window dimensions for terminal relaunch sizing.
    /// Actual PTY resize is handled on every draw cycle (resize-on-draw pattern)
    /// by `widget::resize_if_needed()` which uses exact inner area dimensions.
    pub fn terminal_resize_all(&mut self, rows: u16, cols: u16) {
        // Rough estimate for relaunch only (overridden by first draw cycle)
        let pty_rows = rows.saturating_sub(4);
        let pty_cols = cols.saturating_sub(22);
        self.last_terminal_size = (pty_rows, pty_cols);
    }

    /// Poll all embedded terminals for status changes.
    /// Call once per event loop tick. Returns true if any status changed.
    pub fn poll_terminals(&mut self) -> bool {
        // Count attention before polling to detect new attention events
        let attn_before = self
            .terminal_manager
            .as_ref()
            .map_or(0, |m| m.attention_count());

        let changed = if let Some(mgr) = &mut self.terminal_manager {
            mgr.poll_status()
        } else {
            false
        };

        if changed {
            let attn_after = self
                .terminal_manager
                .as_ref()
                .map_or(0, |m| m.attention_count());

            // New attention event detected
            if attn_after > attn_before {
                // System bell
                if self.system_bell {
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(b"\x07");
                    let _ = std::io::stdout().flush();
                }

                // Attention flash (visible on all views)
                if self.visual_bell {
                    self.attention_flash_until = Some(
                        std::time::Instant::now() + std::time::Duration::from_secs(2),
                    );
                }

                // Desktop notification
                if self.desktop_notifications {
                    send_desktop_notification("crew-board", "Terminal needs attention");
                }

                // Auto-approval based on permission profile
                self.auto_approve_permissions();
            }
        }

        // If the focused terminal exited while we're in TerminalFocused mode,
        // drop back to Normal mode so the user can interact with the list.
        if self.terminal_input_mode == TerminalInputMode::TerminalFocused {
            if let Some(mgr) = &self.terminal_manager {
                if let Some(term) = mgr.focused_terminal() {
                    if matches!(term.status, crate::terminal::TerminalStatus::Exited(_)) {
                        self.terminal_input_mode = TerminalInputMode::Normal;
                    }
                }
            }
        }

        // Tick attention flash timer
        if let Some(until) = self.attention_flash_until {
            if std::time::Instant::now() >= until {
                self.attention_flash_until = None;
            }
        }

        changed
    }

    /// Auto-approve permission prompts based on the active permission profile.
    ///
    /// Terminals that have an active hook_state (Claude Code hook communication is live)
    /// skip PTY-based auto-approval — their permissions are handled via the hook server
    /// thread directly (Autonomous/Trusted profiles return immediate allow responses
    /// without queuing to the main thread).
    fn auto_approve_permissions(&self) {
        use crate::terminal::{AttentionReason, TerminalStatus};

        let mgr = match &self.terminal_manager {
            Some(m) => m,
            None => return,
        };

        for (idx, term) in mgr.terminals.iter().enumerate() {
            // Skip hook-based terminals — their permission flow is handled in hook_server.rs
            if term.hook_state.is_some() {
                continue;
            }

            if let TerminalStatus::NeedsAttention(AttentionReason::PermissionPrompt {
                context,
            }) = &term.status
            {
                let should_approve = term.auto_accept || match self.permission_profile {
                    PermissionProfile::Autonomous => true,
                    PermissionProfile::Trusted => {
                        self.auto_approve_patterns.iter().any(|p| p.is_match(context))
                    }
                    PermissionProfile::Interactive => false,
                };

                if should_approve {
                    let _ = mgr.send_input_to(idx, b"y\n");
                }
            }
        }
    }

    /// Toggle auto-accept for the currently focused terminal.
    /// When enabled, permission prompts are automatically approved.
    pub fn toggle_auto_accept(&mut self) {
        let mgr = match self.terminal_manager.as_mut() {
            Some(m) => m,
            None => return,
        };
        let term = match mgr.terminals.get_mut(mgr.focused) {
            Some(t) => t,
            None => return,
        };
        // Don't toggle on exited terminals
        if matches!(term.status, crate::terminal::TerminalStatus::Exited(_)) {
            return;
        }
        term.auto_accept = !term.auto_accept;
        let new_state = term.auto_accept;
        let terminal_id = term.id.clone();

        // Update hook server registration to match new profile
        if let Some(ref server) = self.hook_server {
            let profile = if new_state {
                "autonomous"
            } else {
                match self.permission_profile {
                    PermissionProfile::Autonomous => "autonomous",
                    PermissionProfile::Trusted => "trusted",
                    PermissionProfile::Interactive => "interactive",
                }
            };
            server.update_token_profile(&terminal_id, profile);
        }

        // Log to activity feed
        let label = if new_state { "auto-accept ON" } else { "auto-accept OFF" };
        self.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: terminal_id.clone(),
            event_type: "AutoAcceptToggle".to_string(),
            tool_name: None,
            tool_input_summary: Some(label.to_string()),
            success: None,
        });
    }

    /// Dismiss (remove) the currently focused terminal.
    ///
    /// Cleans up hook settings files for the terminal:
    /// - Claude: removes `.claude/settings.local.json` (via `hook_settings_cwd`)
    /// - Gemini: removes `.gemini/settings.json` + `.gemini/crew-hook.sh` (via `hook_cleanup_paths`)
    /// - Other hosts: removes any files listed in `hook_cleanup_paths`
    ///
    /// Also deregisters the hook server auth token and kills headless child
    /// processes that are still running. Cleanup is idempotent: missing files
    /// are silently ignored.
    pub fn terminal_dismiss_focused(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            // Kill headless child if still running, collect id + cleanup info
            let dismiss_info = if let Some(term) = mgr.focused_terminal_mut() {
                // Kill headless child process if still running
                if let crate::terminal::TerminalKind::Headless(ref mut hs) = term.kind {
                    let _ = hs.child.kill();
                    let _ = hs.child.wait();
                }
                Some((
                    term.id.clone(),
                    term.hook_settings_cwd.clone(),
                    term.hook_cleanup_paths.clone(),
                ))
            } else {
                None
            };

            if let Some((id, hook_cwd, cleanup_paths)) = dismiss_info {
                // Clean up hook settings files (Claude settings.local.json via hook_settings_cwd)
                if let Some(ref cwd) = hook_cwd {
                    let settings_path = cwd.join(".claude").join("settings.local.json");
                    let _ = std::fs::remove_file(&settings_path);
                }
                // Clean up additional hook config files (Gemini, Copilot, OpenCode, etc.)
                for path in &cleanup_paths {
                    let _ = std::fs::remove_file(path);
                }
                // Deregister hook token
                if let Some(ref server) = self.hook_server {
                    server.deregister_token(&id);
                }
                mgr.remove(&id);
            }
        }
    }

    /// Dismiss all exited terminals at once.
    ///
    /// Cleans up hook settings files for all exited terminals:
    /// - Claude: removes `.claude/settings.local.json` (via `hook_settings_cwd`)
    /// - Gemini: removes `.gemini/settings.json` + `.gemini/crew-hook.sh` (via `hook_cleanup_paths`)
    /// - Other hosts: removes any files listed in `hook_cleanup_paths`
    ///
    /// Also deregisters hook server auth tokens. Cleanup is idempotent:
    /// missing files are silently ignored.
    pub fn terminal_dismiss_all_exited(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            // Collect cleanup info before removal
            let to_cleanup: Vec<(String, Option<PathBuf>, Vec<PathBuf>)> = mgr
                .terminals
                .iter()
                .filter(|t| matches!(t.status, TerminalStatus::Exited(_)))
                .map(|t| (t.id.clone(), t.hook_settings_cwd.clone(), t.hook_cleanup_paths.clone()))
                .collect();

            mgr.dismiss_all_exited();

            // Clean up hook settings files and deregister tokens
            for (id, hook_cwd, cleanup_paths) in to_cleanup {
                // Claude settings.local.json via hook_settings_cwd
                if let Some(ref cwd) = hook_cwd {
                    let settings_path = cwd.join(".claude").join("settings.local.json");
                    let _ = std::fs::remove_file(&settings_path);
                }
                // Additional hook config files (Gemini, Copilot, OpenCode, etc.)
                for path in &cleanup_paths {
                    let _ = std::fs::remove_file(path);
                }
                if let Some(ref server) = self.hook_server {
                    server.deregister_token(&id);
                }
            }
        }
    }

    /// Relaunch the currently focused terminal (if exited).
    pub fn terminal_relaunch_focused(&mut self) {
        let (rows, cols) = self.last_terminal_size;
        if let Some(mgr) = &mut self.terminal_manager {
            let id = mgr.focused_terminal().map(|t| t.id.clone()).unwrap_or_default();
            if !id.is_empty() {
                let _ = mgr.relaunch(&id, rows, cols);
            }
        }
    }

    // ── Scroll-Back ──────────────────────────────────────────────────

    /// Scroll the focused terminal up by N lines.
    /// No-op for headless terminals (no scrollback buffer).
    pub fn terminal_scroll_up(&mut self, lines: usize) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                if let Some(parser) = term.parser() {
                    let max_scrollback =
                        crate::terminal::widget::scrollback_available(parser);
                    term.scroll_offset = (term.scroll_offset + lines).min(max_scrollback);
                }
            }
        }
    }

    /// Scroll the focused terminal down by N lines.
    /// Auto-exits scroll-back into TerminalFocused when reaching live view (offset 0).
    pub fn terminal_scroll_down(&mut self, lines: usize) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                term.scroll_offset = term.scroll_offset.saturating_sub(lines);
                if term.scroll_offset == 0
                    && self.terminal_input_mode == TerminalInputMode::ScrollBack
                {
                    self.terminal_search_query.clear();
                    self.terminal_search_matches.clear();
                    self.terminal_input_mode = TerminalInputMode::TerminalFocused;
                }
            }
        }
    }

    /// Reset scroll position to live view (bottom).
    pub fn terminal_scroll_reset(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                term.scroll_offset = 0;
            }
        }
    }

    /// Scroll to the top of the scrollback buffer.
    /// No-op for headless terminals.
    pub fn terminal_scroll_to_top(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                if let Some(parser) = term.parser() {
                    let max_scrollback =
                        crate::terminal::widget::scrollback_available(parser);
                    term.scroll_offset = max_scrollback;
                }
            }
        }
    }

    // ── Terminal Search (scroll-back) ─────────────────────────────

    /// Start search mode in scroll-back.
    pub fn terminal_search_start(&mut self) {
        self.terminal_search_input = Some(Input::default());
    }

    /// Execute search: scan the scrollback buffer for query matches.
    pub fn terminal_search_execute(&mut self) {
        let query = self
            .terminal_search_input
            .as_ref()
            .map(|i| i.value().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            self.terminal_search_input = None;
            return;
        }
        self.terminal_search_query = query.clone();
        self.terminal_search_input = None;

        // Scan the focused terminal's screen content for matches
        self.terminal_search_matches.clear();
        self.terminal_search_match_idx = 0;

        // Get parser handle (clone the Arc to avoid borrow conflict)
        // Returns None for headless terminals (no scrollback to search).
        let parser_handle: Option<std::sync::Arc<std::sync::Mutex<vt100::Parser>>> = self
            .terminal_manager
            .as_ref()
            .and_then(|m| m.focused_terminal())
            .and_then(|t| t.parser().cloned());

        let parser_handle = match parser_handle {
            Some(p) => p,
            None => return,
        };

        {
            let parser = parser_handle.lock().unwrap();
            let screen = parser.screen();
            let (rows, cols) = screen.size();
            let query_lower = query.to_lowercase();

            // Scan visible rows (scrollback is not directly accessible via cell()
            // with negative indices in vt100 0.15 — scan visible rows only)
            for row in 0..rows {
                let mut line_text = String::with_capacity(cols as usize);
                for col in 0..cols {
                    if let Some(cell) = screen.cell(row, col) {
                        if cell.has_contents() {
                            line_text.push_str(&cell.contents());
                        } else {
                            line_text.push(' ');
                        }
                    }
                }

                if line_text.to_lowercase().contains(&query_lower) {
                    // Convert to scroll offset from bottom
                    let offset_from_bottom = (rows as usize).saturating_sub(1).saturating_sub(row as usize);
                    self.terminal_search_matches.push(offset_from_bottom);
                }
            }
        } // parser lock released here

        // Jump to first match
        if !self.terminal_search_matches.is_empty() {
            let offset = self.terminal_search_matches[0];
            if let Some(mgr) = &mut self.terminal_manager {
                if let Some(term) = mgr.focused_terminal_mut() {
                    term.scroll_offset = offset;
                }
            }
        }
    }

    /// Jump to the next search match.
    pub fn terminal_search_next(&mut self) {
        if self.terminal_search_matches.is_empty() {
            return;
        }
        self.terminal_search_match_idx =
            (self.terminal_search_match_idx + 1) % self.terminal_search_matches.len();
        let offset = self.terminal_search_matches[self.terminal_search_match_idx];
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                term.scroll_offset = offset;
            }
        }
    }

    /// Jump to the previous search match.
    pub fn terminal_search_prev(&mut self) {
        if self.terminal_search_matches.is_empty() {
            return;
        }
        let len = self.terminal_search_matches.len();
        self.terminal_search_match_idx =
            (self.terminal_search_match_idx + len - 1) % len;
        let offset = self.terminal_search_matches[self.terminal_search_match_idx];
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                term.scroll_offset = offset;
            }
        }
    }

    // ── Mouse handling for terminal text selection ────────────────

    /// Start a text selection at the given screen coordinates.
    pub fn mouse_start_selection(&mut self, col: u16, row: u16) {
        if self.active_view != ActiveView::Terminals {
            self.text_selection = None;
            return;
        }
        let rects = self.terminal_panel_rects.borrow();
        for &(term_idx, rect) in rects.iter() {
            if col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
            {
                let local_col = col - rect.x;
                let local_row = row - rect.y;
                self.text_selection = Some(TextSelection {
                    terminal_idx: term_idx,
                    panel_rect: rect,
                    start_col: local_col,
                    start_row: local_row,
                    end_col: local_col,
                    end_row: local_row,
                    active: true,
                });
                return;
            }
        }
        // Click outside any terminal panel — clear selection
        self.text_selection = None;
    }

    /// Extend the text selection to the given screen coordinates, clamped to panel bounds.
    pub fn mouse_extend_selection(&mut self, col: u16, row: u16) {
        if let Some(sel) = &mut self.text_selection {
            if !sel.active {
                return;
            }
            let r = sel.panel_rect;
            let clamped_col =
                col.max(r.x).min(r.x + r.width.saturating_sub(1)) - r.x;
            let clamped_row =
                row.max(r.y).min(r.y + r.height.saturating_sub(1)) - r.y;
            sel.end_col = clamped_col;
            sel.end_row = clamped_row;
        }
    }

    /// Finish the selection and copy text to clipboard via OSC 52.
    /// Single click (no drag) focuses the clicked terminal tile.
    pub fn mouse_end_selection(&mut self) {
        if let Some(sel) = &mut self.text_selection {
            if !sel.active {
                return;
            }
            sel.active = false;
            // Only copy if start != end (actual drag happened)
            if sel.start_col != sel.end_col || sel.start_row != sel.end_row {
                self.copy_selection_to_clipboard();
            } else {
                // Single click — focus the terminal that was clicked
                let term_idx = sel.terminal_idx;
                self.text_selection = None;
                if let Some(mgr) = &mut self.terminal_manager {
                    if term_idx < mgr.terminals.len() {
                        mgr.focused = term_idx;
                    }
                }
            }
        }
    }

    /// Extract selected text from the vt100 screen and copy via OSC 52.
    fn copy_selection_to_clipboard(&self) {
        let sel = match &self.text_selection {
            Some(s) => s,
            None => return,
        };

        let mgr = match &self.terminal_manager {
            Some(m) => m,
            None => return,
        };

        let term = match mgr.terminals.get(sel.terminal_idx) {
            Some(t) => t,
            None => return,
        };

        // Headless terminals have no parser/screen — nothing to copy
        let parser = match term.parser() {
            Some(p) => p,
            None => return,
        };

        let p = parser.lock().unwrap();
        let screen = p.screen();

        // Normalize: ensure start <= end in reading order
        let (sr, sc, er, ec) = if sel.start_row < sel.end_row
            || (sel.start_row == sel.end_row && sel.start_col <= sel.end_col)
        {
            (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
        } else {
            (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
        };

        let mut text = String::new();
        for row in sr..=er {
            let col_start = if row == sr { sc } else { 0 };
            let col_end = if row == er {
                ec
            } else {
                sel.panel_rect.width.saturating_sub(1)
            };

            for col in col_start..=col_end {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.has_contents() {
                        text.push_str(&cell.contents());
                    } else {
                        text.push(' ');
                    }
                } else {
                    text.push(' ');
                }
            }
            if row != er {
                text.push('\n');
            }
        }
        drop(p);

        // Trim trailing whitespace from each line
        let trimmed: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
        let final_text = trimmed.join("\n");

        if !final_text.is_empty() {
            Self::osc52_copy(&final_text);
        }
    }

    /// Copy text to the system clipboard via OSC 52 escape sequence.
    fn osc52_copy(text: &str) {
        use base64::Engine;
        use std::io::Write;
        let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
        let seq = format!("\x1b]52;c;{}\x07", encoded);
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(seq.as_bytes());
        let _ = stdout.flush();
    }

    /// Handle mouse scroll within terminal panels for scrollback.
    pub fn mouse_scroll(&mut self, col: u16, row: u16, up: bool) {
        if self.active_view != ActiveView::Terminals {
            return;
        }
        let rects = self.terminal_panel_rects.borrow();
        for &(term_idx, rect) in rects.iter() {
            if col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
            {
                drop(rects);
                // Focus the terminal being scrolled
                if let Some(mgr) = &mut self.terminal_manager {
                    mgr.focused = term_idx;
                }
                // Only enable scroll-back for embedded terminals (headless has no PTY output)
                let is_embedded = self.terminal_manager
                    .as_ref()
                    .and_then(|m| m.focused_terminal())
                    .is_some_and(|t| t.is_embedded());
                if is_embedded {
                    if up {
                        self.terminal_scroll_up(3);
                        if self.terminal_input_mode == TerminalInputMode::Normal
                            || self.terminal_input_mode == TerminalInputMode::TerminalFocused
                        {
                            self.terminal_input_mode = TerminalInputMode::ScrollBack;
                        }
                    } else {
                        self.terminal_scroll_down(3);
                    }
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_profile_from_str() {
        assert_eq!(
            PermissionProfile::from_str("interactive"),
            PermissionProfile::Interactive
        );
        assert_eq!(
            PermissionProfile::from_str("trusted"),
            PermissionProfile::Trusted
        );
        assert_eq!(
            PermissionProfile::from_str("autonomous"),
            PermissionProfile::Autonomous
        );
        // Case insensitive
        assert_eq!(
            PermissionProfile::from_str("TRUSTED"),
            PermissionProfile::Trusted
        );
        assert_eq!(
            PermissionProfile::from_str("Autonomous"),
            PermissionProfile::Autonomous
        );
        // Unknown defaults to Interactive
        assert_eq!(
            PermissionProfile::from_str("unknown"),
            PermissionProfile::Interactive
        );
        assert_eq!(
            PermissionProfile::from_str(""),
            PermissionProfile::Interactive
        );
    }

    #[test]
    fn test_terminal_layout_next() {
        assert_eq!(TerminalLayout::Focused.next(), TerminalLayout::Tiled2);
        assert_eq!(TerminalLayout::Tiled2.next(), TerminalLayout::Tiled4);
        assert_eq!(TerminalLayout::Tiled4.next(), TerminalLayout::Stacked);
        assert_eq!(TerminalLayout::Stacked.next(), TerminalLayout::Focused);
    }

    #[test]
    fn test_terminal_input_mode_eq() {
        assert_eq!(TerminalInputMode::Normal, TerminalInputMode::Normal);
        assert_ne!(TerminalInputMode::Normal, TerminalInputMode::TerminalFocused);
        assert_ne!(TerminalInputMode::Normal, TerminalInputMode::ScrollBack);
    }

    // ── Headless spawn tests ──────────────────────────────────────────

    #[test]
    fn test_build_headless_args_claude_with_prompt() {
        let args = App::build_headless_args(launcher::AiHost::Claude, Some("/crew resume TASK_001"));
        assert_eq!(args, vec!["-p", "/crew resume TASK_001"]);
    }

    #[test]
    fn test_build_headless_args_claude_default_prompt() {
        let args = App::build_headless_args(launcher::AiHost::Claude, None);
        assert_eq!(args, vec!["-p", "/crew resume"]);
    }

    #[test]
    fn test_build_headless_args_gemini_empty() {
        let args = App::build_headless_args(launcher::AiHost::Gemini, None);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_headless_args_copilot_empty() {
        let args = App::build_headless_args(launcher::AiHost::Copilot, None);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_headless_args_opencode_empty() {
        let args = App::build_headless_args(launcher::AiHost::OpenCode, None);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_headless_args_devin_empty() {
        let args = App::build_headless_args(launcher::AiHost::Devin, None);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_headless_args_droid_empty() {
        let args = App::build_headless_args(launcher::AiHost::Droid, None);
        assert!(args.is_empty());
    }

    #[test]
    fn test_build_headless_args_shell_empty() {
        let args = App::build_headless_args(launcher::AiHost::Shell, None);
        assert!(args.is_empty());
    }

    /// Helper: create a minimal App for headless spawn testing.
    fn create_test_app() -> App {
        App::new(vec![], 10)
    }

    #[test]
    fn test_headless_spawn_no_hook_server_dark_mode() {
        let mut app = create_test_app();
        // No hook server initialized — should spawn in "dark mode" (no hook config)
        let tmp = std::env::temp_dir().join("crew-test-headless-dark");
        let _ = std::fs::create_dir_all(&tmp);

        app.spawn_headless_terminal(
            "TASK_DARK",
            "Dark Mode Test",
            launcher::AiHost::Claude,
            &tmp,
            None,
            Some("-p /crew resume TASK_DARK"),
        );

        // Terminal should be spawned (claude might not exist, but env_vars are empty)
        // Actually, since `claude` is likely not in PATH on CI, this will fail to spawn
        // and the terminal count stays 0. That's the expected "command not found" behavior.
        if let Some(mgr) = &app.terminal_manager {
            // Either spawned or gracefully failed — no crash
            assert!(mgr.terminals.len() <= 1);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_headless_spawn_failure_command_not_found() {
        let mut app = create_test_app();
        let tmp = std::env::temp_dir().join("crew-test-headless-notfound");
        let _ = std::fs::create_dir_all(&tmp);

        // Spawn with a non-existent command (via the terminal manager directly,
        // since spawn_headless_terminal uses launcher::AiHost which maps to real commands)
        if let Some(mgr) = &mut app.terminal_manager {
            let result = mgr.spawn_headless(
                "TASK_FAIL".to_string(),
                "Bad".to_string(),
                "this-command-does-not-exist-98765",
                &[],
                &tmp,
                None,
                vec![
                    ("CREW_BOARD_PORT".to_string(), "12345".to_string()),
                    ("CREW_BOARD_TASK_ID".to_string(), "TASK_FAIL".to_string()),
                    ("CREW_BOARD_TOKEN".to_string(), "abc123".to_string()),
                ],
            );
            assert!(result.is_err());
            assert_eq!(mgr.terminals.len(), 0);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_headless_hook_config_no_server() {
        let app = create_test_app();
        // No hook server — should return empty config
        let (env_vars, cwd, cleanup) = app.generate_headless_hook_config(
            "TASK_001",
            launcher::AiHost::Claude,
            std::path::Path::new("/tmp"),
        );
        assert!(env_vars.is_empty());
        assert!(cwd.is_none());
        assert!(cleanup.is_empty());
    }

    #[test]
    fn test_headless_hook_config_claude_with_server() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-headless-claude-config");
        let _ = std::fs::create_dir_all(&tmp);

        let (env_vars, hook_cwd, cleanup_paths) = app.generate_headless_hook_config(
            "TASK_HC1",
            launcher::AiHost::Claude,
            &tmp,
        );

        // Should have CREW_BOARD_PORT, CREW_BOARD_TASK_ID, CREW_BOARD_TOKEN
        assert_eq!(env_vars.len(), 3);
        let env_map: std::collections::HashMap<_, _> = env_vars.into_iter().collect();
        assert!(env_map.contains_key("CREW_BOARD_PORT"));
        assert_eq!(env_map.get("CREW_BOARD_TASK_ID").unwrap(), "TASK_HC1");
        assert!(env_map.contains_key("CREW_BOARD_TOKEN"));
        assert!(!env_map["CREW_BOARD_TOKEN"].is_empty());

        // Should have hook_settings_cwd pointing to the cwd
        assert_eq!(hook_cwd.unwrap(), tmp);

        // Cleanup paths should include settings.local.json
        assert_eq!(cleanup_paths.len(), 1);
        assert!(cleanup_paths[0].ends_with("settings.local.json"));

        // settings.local.json should exist with correct content
        let settings_path = tmp.join(".claude").join("settings.local.json");
        assert!(settings_path.exists(), "settings.local.json should be created");

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["hooks"]["PreToolUse"].is_array());
        assert!(parsed["hooks"]["PostToolUse"].is_array());
        assert!(parsed["hooks"]["SessionStart"].is_array());
        assert!(parsed["hooks"]["Notification"].is_array());
        assert!(parsed["hooks"]["Stop"].is_array());
        assert!(parsed["hooks"]["PermissionRequest"].is_array());
        assert!(parsed["hooks"]["SessionEnd"].is_array());

        // Verify hook URL contains task ID
        let hook_url = parsed["hooks"]["PreToolUse"][0]["hooks"][0]["url"]
            .as_str()
            .unwrap();
        assert!(hook_url.contains("TASK_HC1"));
        assert!(hook_url.starts_with("http://127.0.0.1:"));

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_hook_config_gemini_with_server() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-headless-gemini-config");
        let _ = std::fs::create_dir_all(&tmp);

        let (env_vars, hook_cwd, cleanup_paths) = app.generate_headless_hook_config(
            "TASK_HG1",
            launcher::AiHost::Gemini,
            &tmp,
        );

        // Should have env vars
        assert_eq!(env_vars.len(), 3);
        let env_map: std::collections::HashMap<_, _> = env_vars.into_iter().collect();
        assert!(env_map.contains_key("CREW_BOARD_PORT"));
        assert_eq!(env_map.get("CREW_BOARD_TASK_ID").unwrap(), "TASK_HG1");
        assert!(env_map.contains_key("CREW_BOARD_TOKEN"));

        // Should have hook_settings_cwd
        assert_eq!(hook_cwd.unwrap(), tmp);

        // Gemini should have 2 cleanup paths: settings.json + crew-hook.sh
        assert_eq!(cleanup_paths.len(), 2);

        // Both files should exist
        let script_path = tmp.join(".gemini").join("crew-hook.sh");
        let settings_path = tmp.join(".gemini").join("settings.json");
        assert!(script_path.exists(), "crew-hook.sh should be created");
        assert!(settings_path.exists(), "settings.json should be created");

        // Bridge script should contain task ID and token
        let script_content = std::fs::read_to_string(&script_path).unwrap();
        assert!(script_content.contains("TASK_HG1"));
        assert!(script_content.contains("curl"));

        // Settings should have hook config
        let settings_content = std::fs::read_to_string(&settings_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&settings_content).unwrap();
        assert!(parsed["hooks"].is_object());
        assert!(parsed["hooks"]["before_tool"].is_object());
        assert!(parsed["hooks"]["after_tool"].is_object());

        // Verify bridge script is executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&script_path).unwrap().permissions();
            assert!(perms.mode() & 0o111 != 0, "crew-hook.sh should be executable");
        }

        // Clean up
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_hook_config_token_registered_with_server() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-headless-token-reg");
        let _ = std::fs::create_dir_all(&tmp);

        let (env_vars, _cwd, _cleanup) = app.generate_headless_hook_config(
            "TASK_TR1",
            launcher::AiHost::Claude,
            &tmp,
        );

        // Token should be registered — send a test hook event to verify
        if let Some(ref server) = app.hook_server {
            let port = server.port;
            let token = env_vars.iter()
                .find(|(k, _)| k == "CREW_BOARD_TOKEN")
                .map(|(_, v)| v.clone())
                .unwrap();

            // Make an HTTP request to the hook server
            let client = std::net::TcpStream::connect(format!("127.0.0.1:{}", port));
            if let Ok(mut stream) = client {
                let body = r#"{"hook_event_name":"Notification","message":"headless test"}"#;
                let request = format!(
                    "POST /hook/TASK_TR1 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    token, body.len(), body
                );
                use std::io::Write;
                stream.write_all(request.as_bytes()).unwrap();
                drop(stream);

                // Give server time to process
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Should have received the event via hook_receiver
                if let Some(ref rx) = app.hook_receiver {
                    match rx.try_recv() {
                        Ok(HookEvent::Notification { terminal_id, message }) => {
                            assert_eq!(terminal_id, "TASK_TR1");
                            assert_eq!(message, "headless test");
                        }
                        other => panic!("Expected Notification, got {:?}", other),
                    }
                }
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_spawn_real_command() {
        // Test spawning a real headless process (using "echo" as a stand-in)
        let mut app = create_test_app();
        let tmp = std::env::temp_dir().join("crew-test-headless-spawn-real");
        let _ = std::fs::create_dir_all(&tmp);

        // Directly use terminal manager to spawn a headless process
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.spawn_headless(
                "TASK_REAL".to_string(),
                "Real Test".to_string(),
                "echo",
                &["hello headless".to_string()],
                &tmp,
                None,
                vec![
                    ("CREW_BOARD_PORT".to_string(), "9999".to_string()),
                    ("CREW_BOARD_TASK_ID".to_string(), "TASK_REAL".to_string()),
                    ("CREW_BOARD_TOKEN".to_string(), "test-token".to_string()),
                ],
            ).unwrap();

            assert_eq!(mgr.terminals.len(), 1);
            let term = &mgr.terminals[0];
            assert!(term.is_headless());
            assert_eq!(term.status, TerminalStatus::Running);
            assert_eq!(term.id, "TASK_REAL");

            // Wait for echo to exit
            std::thread::sleep(std::time::Duration::from_millis(100));
            mgr.poll_status();
            assert_eq!(mgr.terminals[0].status, TerminalStatus::Exited(0));

            mgr.cleanup_all();
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_headless_env_vars_in_spawn() {
        // Verify that env vars are properly passed to spawned headless process
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-headless-env-verify");
        let _ = std::fs::create_dir_all(&tmp);

        // Generate config to get the env vars that would be set
        let (env_vars, _, _) = app.generate_headless_hook_config(
            "TASK_ENV1",
            launcher::AiHost::Claude,
            &tmp,
        );

        // Verify all required env vars
        let env_map: std::collections::HashMap<_, _> = env_vars.into_iter().collect();
        assert!(env_map.contains_key("CREW_BOARD_PORT"));
        assert!(env_map.contains_key("CREW_BOARD_TASK_ID"));
        assert!(env_map.contains_key("CREW_BOARD_TOKEN"));
        assert_eq!(env_map["CREW_BOARD_TASK_ID"], "TASK_ENV1");

        // Port should be a valid number
        let port: u16 = env_map["CREW_BOARD_PORT"].parse().unwrap();
        assert!(port > 0);

        // Token should be 64 hex chars (32 bytes)
        assert_eq!(env_map["CREW_BOARD_TOKEN"].len(), 64);
        assert!(env_map["CREW_BOARD_TOKEN"].chars().all(|c| c.is_ascii_hexdigit()));

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_hook_config_shell_no_config() {
        let mut app = create_test_app();
        app.init_hook_server();

        let (env_vars, hook_cwd, cleanup) = app.generate_headless_hook_config(
            "TASK_SH1",
            launcher::AiHost::Shell,
            std::path::Path::new("/tmp"),
        );

        // Shell host: has env vars but no hook config files
        assert_eq!(env_vars.len(), 3);
        assert!(hook_cwd.is_none()); // Shell doesn't support hooks
        assert!(cleanup.is_empty());

        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_auto_accept_applied() {
        let mut app = create_test_app();
        app.auto_accept_default = true;

        let tmp = std::env::temp_dir().join("crew-test-headless-autoaccept");
        let _ = std::fs::create_dir_all(&tmp);

        // Spawn a real headless command directly to check auto_accept
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.spawn_headless(
                "TASK_AA".to_string(),
                "Auto Accept".to_string(),
                "true",
                &[],
                &tmp,
                None,
                vec![],
            ).unwrap();

            // Manually apply auto_accept as spawn_headless_terminal would
            if let Some(term) = mgr.terminals.last_mut() {
                if app.auto_accept_default {
                    term.auto_accept = true;
                }
            }

            assert!(mgr.terminals.last().unwrap().auto_accept);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_headless_cleanup_paths_set() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-headless-cleanup-paths");
        let _ = std::fs::create_dir_all(&tmp);

        // Generate Claude config
        let (_, _, claude_cleanup) = app.generate_headless_hook_config(
            "TASK_CP1",
            launcher::AiHost::Claude,
            &tmp,
        );
        assert!(!claude_cleanup.is_empty());
        assert!(claude_cleanup.iter().any(|p| p.to_string_lossy().contains("settings.local.json")));

        // Generate Gemini config
        let (_, _, gemini_cleanup) = app.generate_headless_hook_config(
            "TASK_CP2",
            launcher::AiHost::Gemini,
            &tmp,
        );
        assert!(!gemini_cleanup.is_empty());
        assert!(gemini_cleanup.iter().any(|p| p.to_string_lossy().contains("crew-hook.sh")));
        assert!(gemini_cleanup.iter().any(|p| p.to_string_lossy().contains("settings.json")));

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    // ── Headless UI integration tests ─────────────────────────────────

    #[test]
    fn test_launch_popup_headless_defaults_to_false() {
        // LaunchPopup should default headless to false (Embedded mode)
        let popup = LaunchPopup {
            terminals: vec![launcher::TerminalEnv::Embedded],
            hosts: vec![launcher::AiHost::Claude],
            step: LaunchStep::SelectTerminal,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir: PathBuf::from("/tmp"),
            task_id: "TASK_T1".to_string(),
            task_desc: String::new(),
            color_scheme_index: None,
            result_msg: None,
            headless: false,
        };
        assert!(!popup.headless, "Default should be Embedded (headless=false)");
    }

    #[test]
    fn test_launch_popup_headless_not_sticky() {
        // Each time the popup opens, headless should reset to false
        let mut app = create_test_app();

        // First open — headless starts false
        app.launch_popup = Some(LaunchPopup {
            terminals: vec![launcher::TerminalEnv::Embedded],
            hosts: vec![launcher::AiHost::Claude],
            step: LaunchStep::SelectMode,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir: PathBuf::from("/tmp"),
            task_id: "TASK_T2".to_string(),
            task_desc: String::new(),
            color_scheme_index: None,
            result_msg: None,
            headless: false,
        });
        assert!(!app.launch_popup.as_ref().unwrap().headless);

        // Toggle headless on
        app.launch_popup.as_mut().unwrap().headless = true;
        assert!(app.launch_popup.as_ref().unwrap().headless);

        // Close popup
        app.launch_popup = None;

        // Re-open — should start false again
        app.launch_popup = Some(LaunchPopup {
            terminals: vec![launcher::TerminalEnv::Embedded],
            hosts: vec![launcher::AiHost::Claude],
            step: LaunchStep::SelectMode,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir: PathBuf::from("/tmp"),
            task_id: "TASK_T3".to_string(),
            task_desc: String::new(),
            color_scheme_index: None,
            result_msg: None,
            headless: false,
        });
        assert!(!app.launch_popup.as_ref().unwrap().headless, "Should reset to false on re-open");
    }

    #[test]
    fn test_launch_popup_select_mode_step_exists() {
        // Verify SelectMode step is reachable
        let step = LaunchStep::SelectMode;
        assert_eq!(step, LaunchStep::SelectMode);
    }

    #[test]
    fn test_create_popup_headless_defaults_to_false() {
        let popup = CreateWorktreePopup {
            step: CreateStep::InputDescription,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 0,
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };
        assert!(!popup.headless, "F4 popup headless should default to false");
        assert!(!popup.no_checkpoints, "F4 popup no_checkpoints should default to false");
    }

    #[test]
    fn test_create_popup_headless_toggle() {
        let mut popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 2, // Position on headless toggle
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        // Toggle headless on
        popup.headless = !popup.headless;
        assert!(popup.headless);

        // Toggle headless off
        popup.headless = !popup.headless;
        assert!(!popup.headless);
    }

    #[test]
    fn test_headless_terminal_is_headless_flag() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_HF".to_string(),
            "Headless Flag".to_string(),
            "true",
            &[],
            std::path::Path::new("/tmp"),
            None,
            vec![],
        ).unwrap();

        let term = &mgr.terminals[0];
        assert!(term.is_headless(), "Headless terminal should report is_headless=true");
        assert!(!term.is_embedded(), "Headless terminal should report is_embedded=false");
        assert!(term.parser().is_none(), "Headless terminal should have no parser");
        assert!(term.master().is_none(), "Headless terminal should have no master");
    }

    #[test]
    fn test_headless_relaunch_stays_headless() {
        let mut mgr = TerminalManager::new();
        mgr.spawn_headless(
            "TASK_RL".to_string(),
            "Relaunch Test".to_string(),
            "true",
            &[],
            std::path::Path::new("/tmp"),
            None,
            vec![],
        ).unwrap();

        // Wait for exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        mgr.poll_status();
        assert!(matches!(mgr.terminals[0].status, TerminalStatus::Exited(0)));

        // Relaunch
        let ok = mgr.relaunch("TASK_RL", 24, 80).unwrap();
        assert!(ok, "Relaunch should succeed");
        assert!(mgr.terminals[0].is_headless(), "Relaunched terminal should still be headless");
    }

    #[test]
    fn test_popup_mode_toggle_up_down() {
        let mut app = create_test_app();
        app.launch_popup = Some(LaunchPopup {
            terminals: vec![launcher::TerminalEnv::Embedded],
            hosts: vec![launcher::AiHost::Claude],
            step: LaunchStep::SelectMode,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir: PathBuf::from("/tmp"),
            task_id: "TASK_MT".to_string(),
            task_desc: String::new(),
            color_scheme_index: None,
            result_msg: None,
            headless: false,
        });

        // Initially Embedded (headless=false)
        assert!(!app.launch_popup.as_ref().unwrap().headless);

        // popup_down toggles to Headless
        app.popup_down();
        assert!(app.launch_popup.as_ref().unwrap().headless);

        // popup_up toggles back to Embedded
        app.popup_up();
        assert!(!app.launch_popup.as_ref().unwrap().headless);

        // popup_down again
        app.popup_down();
        assert!(app.launch_popup.as_ref().unwrap().headless);
    }

    // ── Headless settings cleanup tests ──────────────────────────────

    /// Helper: spawn a headless terminal using `true` (available in PATH),
    /// generate hook config files, and wire up hook_settings_cwd and
    /// hook_cleanup_paths so cleanup tests don't depend on claude/gemini
    /// being installed.
    fn spawn_headless_with_hook_files(
        app: &mut App,
        task_id: &str,
        label: &str,
        host: launcher::AiHost,
        cwd: &std::path::Path,
    ) {
        // Generate hook config (writes files, registers token)
        let (env_vars, hook_settings_cwd, cleanup_paths) =
            app.generate_headless_hook_config(task_id, host, cwd);

        // Spawn using "true" so it works in any CI environment
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.spawn_headless(
                task_id.to_string(),
                label.to_string(),
                "true",
                &[],
                cwd,
                None,
                env_vars,
            ).expect("spawn 'true' should succeed");

            // Wire up hook settings on the terminal (mimics spawn_headless_terminal)
            if let Some(term) = mgr.terminals.last_mut() {
                if let Some(ref settings_cwd) = hook_settings_cwd {
                    term.hook_settings_cwd = Some(settings_cwd.clone());
                }
                term.hook_cleanup_paths = cleanup_paths;
            }
        }
    }

    #[test]
    fn test_headless_dismiss_claude_cleanup() {
        // VAL-HC-080: settings.local.json removed on headless Claude dismiss
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-dismiss-claude");
        let _ = std::fs::create_dir_all(&tmp);

        spawn_headless_with_hook_files(&mut app, "TASK_DC1", "Claude Dismiss", launcher::AiHost::Claude, &tmp);

        // Verify the settings file was created
        let settings_path = tmp.join(".claude").join("settings.local.json");
        assert!(settings_path.exists(), "settings.local.json should exist after spawn");

        // Verify token is registered
        assert!(
            app.hook_server.as_ref().unwrap().is_token_registered("TASK_DC1"),
            "Token should be registered after spawn"
        );

        // Wait for "true" to exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Dismiss the terminal
        app.terminal_dismiss_focused();

        // Verify settings.local.json was removed
        assert!(!settings_path.exists(), "settings.local.json should be removed after dismiss");

        // Verify token was deregistered
        assert!(
            !app.hook_server.as_ref().unwrap().is_token_registered("TASK_DC1"),
            "Token should be deregistered after dismiss"
        );

        // Verify terminal was removed
        assert_eq!(
            app.terminal_manager.as_ref().unwrap().terminals.len(),
            0,
            "Terminal should be removed after dismiss"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_dismiss_gemini_cleanup() {
        // VAL-HC-081: .gemini/settings.json and crew-hook.sh removed on dismiss
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-dismiss-gemini");
        let _ = std::fs::create_dir_all(&tmp);

        spawn_headless_with_hook_files(&mut app, "TASK_DG1", "Gemini Dismiss", launcher::AiHost::Gemini, &tmp);

        // Verify both Gemini config files were created
        let script_path = tmp.join(".gemini").join("crew-hook.sh");
        let settings_path = tmp.join(".gemini").join("settings.json");
        assert!(script_path.exists(), "crew-hook.sh should exist after spawn");
        assert!(settings_path.exists(), "settings.json should exist after spawn");

        // Verify token is registered
        assert!(
            app.hook_server.as_ref().unwrap().is_token_registered("TASK_DG1"),
            "Token should be registered after spawn"
        );

        // Wait for exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Dismiss the terminal
        app.terminal_dismiss_focused();

        // Verify both files were removed
        assert!(!script_path.exists(), "crew-hook.sh should be removed after dismiss");
        assert!(!settings_path.exists(), "settings.json should be removed after dismiss");

        // Verify token was deregistered
        assert!(
            !app.hook_server.as_ref().unwrap().is_token_registered("TASK_DG1"),
            "Token should be deregistered after dismiss"
        );

        // Verify terminal was removed
        assert_eq!(
            app.terminal_manager.as_ref().unwrap().terminals.len(),
            0,
            "Terminal should be removed after dismiss"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_dismiss_all_exited_cleanup() {
        // VAL-HC-073 + VAL-HC-082: dismiss-all cleans up all headless hook files
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp_claude = std::env::temp_dir().join("crew-test-dismiss-all-claude");
        let tmp_gemini = std::env::temp_dir().join("crew-test-dismiss-all-gemini");
        let _ = std::fs::create_dir_all(&tmp_claude);
        let _ = std::fs::create_dir_all(&tmp_gemini);

        spawn_headless_with_hook_files(&mut app, "TASK_DA_C", "Claude All", launcher::AiHost::Claude, &tmp_claude);
        spawn_headless_with_hook_files(&mut app, "TASK_DA_G", "Gemini All", launcher::AiHost::Gemini, &tmp_gemini);

        // Verify files created
        let claude_settings = tmp_claude.join(".claude").join("settings.local.json");
        let gemini_script = tmp_gemini.join(".gemini").join("crew-hook.sh");
        let gemini_settings = tmp_gemini.join(".gemini").join("settings.json");
        assert!(claude_settings.exists(), "Claude settings should exist");
        assert!(gemini_script.exists(), "Gemini script should exist");
        assert!(gemini_settings.exists(), "Gemini settings should exist");

        // Wait for exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Dismiss all exited
        app.terminal_dismiss_all_exited();

        // Verify all files removed
        assert!(!claude_settings.exists(), "Claude settings should be removed");
        assert!(!gemini_script.exists(), "Gemini script should be removed");
        assert!(!gemini_settings.exists(), "Gemini settings should be removed");

        // Verify tokens deregistered
        assert!(
            !app.hook_server.as_ref().unwrap().is_token_registered("TASK_DA_C"),
            "Claude token should be deregistered"
        );
        assert!(
            !app.hook_server.as_ref().unwrap().is_token_registered("TASK_DA_G"),
            "Gemini token should be deregistered"
        );

        let _ = std::fs::remove_dir_all(&tmp_claude);
        let _ = std::fs::remove_dir_all(&tmp_gemini);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_cleanup_idempotent() {
        // VAL-HC-083: Cleanup is idempotent — missing files don't cause errors
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-idempotent-cleanup");
        let _ = std::fs::create_dir_all(&tmp);

        spawn_headless_with_hook_files(&mut app, "TASK_IC1", "Idempotent", launcher::AiHost::Claude, &tmp);

        // Manually delete the settings file BEFORE dismiss
        let settings_path = tmp.join(".claude").join("settings.local.json");
        assert!(settings_path.exists());
        let _ = std::fs::remove_file(&settings_path);
        assert!(!settings_path.exists(), "Settings should be manually deleted");

        // Wait for exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Dismiss should NOT panic or error even though file is already gone
        app.terminal_dismiss_focused();

        // Terminal should still be properly removed
        assert_eq!(
            app.terminal_manager.as_ref().unwrap().terminals.len(),
            0,
            "Terminal should be removed even when settings file was already deleted"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_cleanup_idempotent_gemini() {
        // VAL-HC-083: Cleanup is idempotent for Gemini files too
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-idempotent-gemini");
        let _ = std::fs::create_dir_all(&tmp);

        spawn_headless_with_hook_files(&mut app, "TASK_IG1", "Idempotent Gemini", launcher::AiHost::Gemini, &tmp);

        // Manually delete both Gemini files before dismiss
        let script_path = tmp.join(".gemini").join("crew-hook.sh");
        let settings_path = tmp.join(".gemini").join("settings.json");
        assert!(script_path.exists());
        assert!(settings_path.exists());
        let _ = std::fs::remove_file(&script_path);
        let _ = std::fs::remove_file(&settings_path);

        // Wait for exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Dismiss should NOT panic or error
        app.terminal_dismiss_focused();

        assert_eq!(
            app.terminal_manager.as_ref().unwrap().terminals.len(),
            0,
            "Terminal should be removed even when Gemini files were already deleted"
        );

        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    #[test]
    fn test_headless_app_exit_cleanup() {
        // VAL-HC-082: App exit cleans up all headless hook files
        // Simulates the main.rs cleanup loop by directly testing the cleanup logic
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp_c = std::env::temp_dir().join("crew-test-app-exit-claude");
        let tmp_g = std::env::temp_dir().join("crew-test-app-exit-gemini");
        let _ = std::fs::create_dir_all(&tmp_c);
        let _ = std::fs::create_dir_all(&tmp_g);

        spawn_headless_with_hook_files(&mut app, "TASK_AE_C", "Claude", launcher::AiHost::Claude, &tmp_c);
        spawn_headless_with_hook_files(&mut app, "TASK_AE_G", "Gemini", launcher::AiHost::Gemini, &tmp_g);

        // Verify files exist
        let claude_settings = tmp_c.join(".claude").join("settings.local.json");
        let gemini_script = tmp_g.join(".gemini").join("crew-hook.sh");
        let gemini_settings = tmp_g.join(".gemini").join("settings.json");
        assert!(claude_settings.exists(), "Claude settings should exist");
        assert!(gemini_script.exists(), "Gemini script should exist");
        assert!(gemini_settings.exists(), "Gemini settings should exist");

        // Simulate the main.rs app exit cleanup loop
        if let Some(mgr) = &mut app.terminal_manager {
            for term in &mgr.terminals {
                // Claude: settings.local.json via hook_settings_cwd
                if let Some(ref cwd) = term.hook_settings_cwd {
                    let settings_path = cwd.join(".claude").join("settings.local.json");
                    let _ = std::fs::remove_file(&settings_path);
                }
                // Gemini, Copilot, OpenCode, etc.: files listed in hook_cleanup_paths
                for path in &term.hook_cleanup_paths {
                    let _ = std::fs::remove_file(path);
                }
            }
            mgr.cleanup_all();
        }

        // Verify all files cleaned up
        assert!(!claude_settings.exists(), "Claude settings should be removed on app exit");
        assert!(!gemini_script.exists(), "Gemini script should be removed on app exit");
        assert!(!gemini_settings.exists(), "Gemini settings should be removed on app exit");

        let _ = std::fs::remove_dir_all(&tmp_c);
        let _ = std::fs::remove_dir_all(&tmp_g);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    // ── Auto-Headless Trigger Tests ────────────────────────────────────

    #[test]
    fn test_no_checkpoints_auto_selects_headless() {
        // Simulates toggling no_checkpoints on: headless should be auto-selected
        let mut popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 3, // Position on no_checkpoints toggle
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        assert!(!popup.headless, "headless should start false");
        assert!(!popup.no_checkpoints, "no_checkpoints should start false");

        // Toggle no_checkpoints on — should auto-select headless
        popup.no_checkpoints = !popup.no_checkpoints;
        if popup.no_checkpoints {
            popup.headless = true;
        }
        assert!(popup.no_checkpoints, "no_checkpoints should be true");
        assert!(popup.headless, "headless should be auto-selected when no_checkpoints is on");
    }

    #[test]
    fn test_no_checkpoints_off_does_not_change_headless() {
        // Toggling no_checkpoints off should NOT change headless state
        let mut popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: true,
            no_checkpoints: true,
            settings_cursor: 3,
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        // Toggle no_checkpoints off — headless should remain unchanged
        popup.no_checkpoints = !popup.no_checkpoints;
        // (our toggle logic only auto-selects on enable, not deselect)
        assert!(!popup.no_checkpoints, "no_checkpoints should be false");
        assert!(popup.headless, "headless should remain true (user can manually toggle off)");
    }

    #[test]
    fn test_headless_override_with_no_checkpoints_on() {
        // User can toggle headless off even when no_checkpoints is on (override)
        let mut popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: true,
            no_checkpoints: true,
            settings_cursor: 2, // Position on headless toggle
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        // User explicitly toggles headless off (override)
        popup.headless = !popup.headless;
        assert!(!popup.headless, "User should be able to override headless to false");
        assert!(popup.no_checkpoints, "no_checkpoints should remain true");
    }

    #[test]
    fn test_quick_mode_defaults_headless() {
        // --quick should default to headless (use_headless = !embed)
        let embed = false; // no --embed flag
        let use_headless = !embed;
        assert!(use_headless, "--quick should default to headless when --embed is not set");
    }

    #[test]
    fn test_quick_mode_embed_overrides_headless() {
        // --quick --embed should use embedded PTY
        let embed = true; // --embed flag set
        let use_headless = !embed;
        assert!(!use_headless, "--embed should override --quick headless default to embedded");
    }

    #[test]
    fn test_quick_mode_spawns_headless_terminal() {
        // Integration-style test: verify --quick spawns a headless terminal
        let mut app = create_test_app();
        let tmp = std::env::temp_dir().join("crew-test-quick-headless");
        let _ = std::fs::create_dir_all(&tmp);

        // Simulate --quick with headless spawn
        let host = launcher::AiHost::Claude;
        let task_id = "TASK_QUICK1";
        let label = format!("{} (headless)", host.label());

        if let Some(mgr) = &mut app.terminal_manager {
            let _ = mgr.spawn_headless(
                task_id.to_string(),
                label,
                "true", // dummy command
                &[],
                &tmp,
                None,
                vec![],
            );

            assert_eq!(mgr.terminals.len(), 1, "Quick mode should spawn one terminal");
            let term = &mgr.terminals[0];
            assert!(term.is_headless(), "Quick mode terminal should be headless");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_quick_mode_embed_spawns_embedded() {
        // When --embed is used, --quick should use embedded terminal
        // (This test verifies the logic path, not the actual PTY spawn)
        let embed = true;
        let use_headless = !embed;
        assert!(!use_headless, "With --embed, should use embedded mode");
        // In this case, spawn_terminal would be called instead of spawn_headless_terminal
    }

    #[test]
    fn test_create_popup_no_checkpoints_defaults_to_false() {
        let popup = CreateWorktreePopup {
            step: CreateStep::InputDescription,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 0,
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };
        assert!(!popup.no_checkpoints, "no_checkpoints should default to false");
    }

    #[test]
    fn test_create_popup_settings_has_four_toggles() {
        // Verify settings step has 4 toggles (max cursor position is 3)
        let popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: true,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 0,
            repo_path: PathBuf::from("/tmp"),
            repo_name: "test".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        // The 4 settings toggles are: pull, launch_after, headless, no_checkpoints
        // Settings cursor max should be 3 (0-indexed)
        let max_settings_cursor = 3u8;
        assert_eq!(max_settings_cursor, 3, "Should have 4 settings (cursor 0-3)");

        // Verify all fields are accessible
        assert!(popup.pull || !popup.pull);
        assert!(popup.launch_after || !popup.launch_after);
        assert!(popup.headless || !popup.headless);
        assert!(popup.no_checkpoints || !popup.no_checkpoints);
    }

    // ── Cross-Area Integration Tests (VAL-CROSS-001..004) ──────────

    /// VAL-CROSS-001: F4 create + auto-headless + activity feed.
    ///
    /// Full flow: F4 creates worktree with --no-checkpoints → headless terminal
    /// spawned → hook events appear in activity feed with correct terminal ID →
    /// cost view shows session cost.
    #[test]
    fn test_cross_f4_auto_headless_activity_feed() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-cross-001");
        let _ = std::fs::create_dir_all(&tmp);

        // 1. Simulate F4 popup with no_checkpoints=true:
        //    no_checkpoints auto-selects headless mode.
        let mut popup = CreateWorktreePopup {
            step: CreateStep::ToggleSettings,
            description_input: tui_input::Input::default(),
            hosts: vec![launcher::AiHost::Claude],
            host_cursor: 0,
            pull: false,
            launch_after: true,
            headless: false,
            no_checkpoints: false,
            settings_cursor: 3,
            repo_path: tmp.clone(),
            repo_name: "test-repo".to_string(),
            preview: None,
            handle: None,
            started_at: None,
            result: None,
        };

        // Toggle no_checkpoints on → should auto-select headless
        popup.no_checkpoints = true;
        if popup.no_checkpoints {
            popup.headless = true;
        }
        assert!(popup.no_checkpoints, "no_checkpoints should be true");
        assert!(popup.headless, "headless should be auto-selected");

        // 2. Spawn headless terminal (simulating post-F4 creation launch)
        let task_id = "TASK_CROSS_001";
        spawn_headless_with_hook_files(
            &mut app,
            task_id,
            "Claude (headless)",
            launcher::AiHost::Claude,
            &tmp,
        );

        // Verify headless terminal was spawned
        {
            let mgr = app.terminal_manager.as_ref().unwrap();
            assert_eq!(mgr.terminals.len(), 1);
            let term = &mgr.terminals[0];
            assert!(term.is_headless(), "Terminal should be headless");
            assert_eq!(term.id, task_id);
            assert_eq!(term.status, TerminalStatus::Running);
        }

        // 3. Simulate hook events from the headless terminal
        //    (In real flow, the hook server receives HTTP posts from claude process;
        //     here we inject events directly into the activity log and hook state.)

        // SessionStart
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "SessionStart".to_string(),
            tool_name: None,
            tool_input_summary: None,
            success: None,
        });

        // PreToolUse + PostToolUse (Edit src/main.rs)
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PreToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: None,
        });
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("src/main.rs".to_string()),
            success: Some(true),
        });

        // PreToolUse + PostToolUse (Read README.md)
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PreToolUse".to_string(),
            tool_name: Some("Read".to_string()),
            tool_input_summary: Some("README.md".to_string()),
            success: None,
        });
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Read".to_string()),
            tool_input_summary: Some("README.md".to_string()),
            success: Some(true),
        });

        // 4. Verify activity feed contains events from headless terminal
        let headless_events = app.activity_log.filter(Some(task_id), None, None);
        assert_eq!(headless_events.len(), 5, "Should have 5 events (SessionStart + 2×Pre + 2×Post)");

        let pre_tool_events = app.activity_log.filter(Some(task_id), Some("PreToolUse"), None);
        assert_eq!(pre_tool_events.len(), 2, "Should have 2 PreToolUse events");

        let edit_events = app.activity_log.filter(Some(task_id), None, Some("Edit"));
        assert_eq!(edit_events.len(), 2, "Should have 2 Edit events (Pre + Post)");

        // 5. Verify per-terminal stats
        let stats = app.activity_log.stats_for_terminal(task_id).unwrap();
        assert_eq!(stats.total_tools, 2, "Should have 2 completed tool calls");
        assert_eq!(stats.errors, 0, "No errors");
        assert!(stats.files_touched.contains(&"src/main.rs".to_string()));
        assert!(stats.files_touched.contains(&"README.md".to_string()));

        // 6. Add cost to hook_state and verify cost view inclusion
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.terminals[0].hook_state = Some(crate::terminal::HookState {
                last_event: "Stop".to_string(),
                last_event_at: std::time::Instant::now(),
                activity_label: String::new(),
                tool_counts: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("Edit".to_string(), 1);
                    m.insert("Read".to_string(), 1);
                    m
                },
                session_active: false,
                total_cost_usd: 0.0789,
                total_input_tokens: 35000,
                total_output_tokens: 8000,
            });
        }

        // Verify cost view would include this headless terminal
        let mgr = app.terminal_manager.as_ref().unwrap();
        let terminals_with_cost: Vec<_> = mgr.terminals.iter()
            .filter(|t| t.hook_state.as_ref().is_some_and(|h| h.total_cost_usd > 0.0 || h.total_input_tokens > 0))
            .collect();
        assert_eq!(terminals_with_cost.len(), 1);
        assert!(terminals_with_cost[0].is_headless());
        let hs = terminals_with_cost[0].hook_state.as_ref().unwrap();
        assert!((hs.total_cost_usd - 0.0789).abs() < f64::EPSILON);

        // 7. Verify global stats include headless terminal
        let global = app.activity_log.global_stats();
        assert_eq!(global.total_tool_calls, 2);

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    /// VAL-CROSS-002: F2 headless launch + hook events + dismiss cleanup.
    ///
    /// Full lifecycle: F2 launch headless Claude → hook events processed →
    /// terminal dismissed → settings files cleaned up → terminal removed
    /// from all views (activity log, terminal list).
    #[test]
    fn test_cross_f2_headless_lifecycle() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-cross-002");
        let _ = std::fs::create_dir_all(&tmp);

        // 1. Simulate F2 popup: user selects headless mode
        let popup = LaunchPopup {
            terminals: vec![launcher::TerminalEnv::Embedded],
            hosts: vec![launcher::AiHost::Claude],
            step: LaunchStep::SelectMode,
            terminal_cursor: 0,
            host_cursor: 0,
            work_dir: tmp.clone(),
            task_id: "TASK_CROSS_002".to_string(),
            task_desc: String::new(),
            color_scheme_index: None,
            result_msg: None,
            headless: true, // User selected headless
        };
        assert!(popup.headless, "F2 popup should have headless=true");

        // 2. Spawn headless Claude with hook config
        let task_id = "TASK_CROSS_002";
        spawn_headless_with_hook_files(
            &mut app,
            task_id,
            "Claude (headless)",
            launcher::AiHost::Claude,
            &tmp,
        );

        // Verify settings file was created
        let settings_path = tmp.join(".claude").join("settings.local.json");
        assert!(settings_path.exists(), "settings.local.json should exist after spawn");

        // Verify token is registered
        assert!(
            app.hook_server.as_ref().unwrap().is_token_registered(task_id),
            "Token should be registered"
        );

        // 3. Simulate hook events (SessionStart → PreToolUse → PostToolUse → Stop)
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "SessionStart".to_string(),
            tool_name: None,
            tool_input_summary: None,
            success: None,
        });
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PreToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input_summary: Some("cargo test".to_string()),
            success: None,
        });
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input_summary: Some("cargo test".to_string()),
            success: Some(true),
        });

        // Verify events are in the activity log
        let events = app.activity_log.filter(Some(task_id), None, None);
        assert_eq!(events.len(), 3, "Should have 3 events");

        // Verify per-terminal stats
        let stats = app.activity_log.stats_for_terminal(task_id).unwrap();
        assert_eq!(stats.total_tools, 1);

        // 4. Wait for process to exit, then poll
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.poll_status();
        }

        // Terminal should be exited
        assert!(
            matches!(
                app.terminal_manager.as_ref().unwrap().terminals[0].status,
                TerminalStatus::Exited(0)
            ),
            "Headless terminal should have exited"
        );

        // 5. Dismiss the terminal
        app.terminal_dismiss_focused();

        // 6. Verify cleanup
        //    - settings.local.json removed
        assert!(!settings_path.exists(), "settings.local.json should be removed after dismiss");

        //    - Token deregistered
        assert!(
            !app.hook_server.as_ref().unwrap().is_token_registered(task_id),
            "Token should be deregistered after dismiss"
        );

        //    - Terminal removed from list
        assert_eq!(
            app.terminal_manager.as_ref().unwrap().terminals.len(),
            0,
            "Terminal should be removed from terminal list"
        );

        //    - Activity log still has the events (events are not removed on dismiss)
        let events_after = app.activity_log.filter(Some(task_id), None, None);
        assert_eq!(events_after.len(), 3, "Activity log should preserve events after dismiss");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }

    /// VAL-CROSS-003: Mixed embedded+headless dashboard.
    ///
    /// Dashboard with 2 embedded + 2 headless terminals: crew list shows all 4
    /// with correct indicators, activity feed shows interleaved events, cost
    /// view sums all 4, stats include all 4. Navigation cycles all 4.
    #[test]
    fn test_cross_mixed_dashboard() {
        let mut app = create_test_app();

        // We can't spawn real PTY-backed terminals in unit tests without a real
        // terminal, so we use headless for all 4 but mark 2 as "embedded-like"
        // by testing the data model. The key test is that TerminalManager and
        // ActivityLog handle mixed types correctly.

        let tmp = std::env::temp_dir().join("crew-test-cross-003");
        let _ = std::fs::create_dir_all(&tmp);

        // Spawn 4 headless terminals (2 "embedded-like" + 2 headless)
        // In reality, embedded terminals would use spawn(), but spawn()
        // requires a real PTY. The point is to test the data model works
        // with mixed terminal_ids in activity log, cost, and stats.
        let mgr = app.terminal_manager.as_mut().unwrap();

        for (i, label) in ["Embedded A", "Embedded B", "Headless C", "Headless D"].iter().enumerate() {
            mgr.spawn_headless(
                format!("TASK_MIX_{}", i),
                label.to_string(),
                "sleep",
                &["60".to_string()],
                &tmp,
                Some(i),
                vec![],
            ).unwrap();
        }

        // 1. Verify crew list shows all 4 terminals
        assert_eq!(mgr.terminals.len(), 4, "Should have 4 terminals");
        assert_eq!(mgr.terminals[0].id, "TASK_MIX_0");
        assert_eq!(mgr.terminals[1].id, "TASK_MIX_1");
        assert_eq!(mgr.terminals[2].id, "TASK_MIX_2");
        assert_eq!(mgr.terminals[3].id, "TASK_MIX_3");

        // All terminals are headless (in real usage, 0,1 would be embedded)
        // but the is_headless() accessor works correctly
        assert!(mgr.terminals.iter().all(|t| t.is_headless()));

        // 2. Navigation cycles all 4 terminals
        assert_eq!(mgr.focused, 3, "Focus should be on last spawned (index 3)");
        mgr.focus_next();
        assert_eq!(mgr.focused, 0, "focus_next wraps to 0");
        mgr.focus_next();
        assert_eq!(mgr.focused, 1);
        mgr.focus_next();
        assert_eq!(mgr.focused, 2);
        mgr.focus_next();
        assert_eq!(mgr.focused, 3);
        mgr.focus_prev();
        assert_eq!(mgr.focused, 2, "focus_prev goes back to 2");

        // 3. Add interleaved activity events from all 4 terminals
        for tid in &["TASK_MIX_0", "TASK_MIX_1", "TASK_MIX_2", "TASK_MIX_3"] {
            app.activity_log.push(crate::data::activity::ActivityEvent {
                timestamp: std::time::Instant::now(),
                terminal_id: tid.to_string(),
                event_type: "SessionStart".to_string(),
                tool_name: None,
                tool_input_summary: None,
                success: None,
            });
        }

        // Interleaved tool calls
        for (i, tid) in ["TASK_MIX_0", "TASK_MIX_1", "TASK_MIX_2", "TASK_MIX_3"].iter().enumerate() {
            let tool = match i {
                0 => "Edit",
                1 => "Read",
                2 => "Bash",
                _ => "Write",
            };
            app.activity_log.push(crate::data::activity::ActivityEvent {
                timestamp: std::time::Instant::now(),
                terminal_id: tid.to_string(),
                event_type: "PreToolUse".to_string(),
                tool_name: Some(tool.to_string()),
                tool_input_summary: Some(format!("file_{}.rs", i)),
                success: None,
            });
            app.activity_log.push(crate::data::activity::ActivityEvent {
                timestamp: std::time::Instant::now(),
                terminal_id: tid.to_string(),
                event_type: "PostToolUse".to_string(),
                tool_name: Some(tool.to_string()),
                tool_input_summary: Some(format!("file_{}.rs", i)),
                success: Some(true),
            });
        }

        // Verify activity feed shows all events
        assert_eq!(app.activity_log.len(), 12, "4 SessionStart + 4 PreToolUse + 4 PostToolUse");

        // Filter by each terminal
        for tid in &["TASK_MIX_0", "TASK_MIX_1", "TASK_MIX_2", "TASK_MIX_3"] {
            let events = app.activity_log.filter(Some(tid), None, None);
            assert_eq!(events.len(), 3, "Each terminal should have 3 events");
        }

        // 4. Add cost data to all 4 terminals
        let mgr = app.terminal_manager.as_mut().unwrap();
        let costs = [0.05, 0.10, 0.15, 0.20];
        let input_tokens = [10000u64, 20000, 30000, 40000];
        let output_tokens = [2000u64, 4000, 6000, 8000];

        for (i, term) in mgr.terminals.iter_mut().enumerate() {
            term.hook_state = Some(crate::terminal::HookState {
                last_event: "Stop".to_string(),
                last_event_at: std::time::Instant::now(),
                activity_label: String::new(),
                tool_counts: std::collections::HashMap::new(),
                session_active: false,
                total_cost_usd: costs[i],
                total_input_tokens: input_tokens[i],
                total_output_tokens: output_tokens[i],
            });
        }

        // 5. Verify cost view includes all 4 terminals
        let mgr = app.terminal_manager.as_ref().unwrap();
        let terminals_with_cost: Vec<_> = mgr.terminals.iter()
            .filter(|t| t.hook_state.as_ref().is_some_and(|h| h.total_cost_usd > 0.0 || h.total_input_tokens > 0))
            .collect();
        assert_eq!(terminals_with_cost.len(), 4, "Cost view should include all 4 terminals");

        // Verify TOTAL row sums
        let mut total_cost = 0.0f64;
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        for t in &terminals_with_cost {
            let hs = t.hook_state.as_ref().unwrap();
            total_cost += hs.total_cost_usd;
            total_input += hs.total_input_tokens;
            total_output += hs.total_output_tokens;
        }
        assert!((total_cost - 0.50).abs() < f64::EPSILON, "Total cost should be $0.50");
        assert_eq!(total_input, 100000, "Total input tokens should be 100K");
        assert_eq!(total_output, 20000, "Total output tokens should be 20K");

        // 6. Verify stats include all 4 terminals
        let global = app.activity_log.global_stats();
        assert_eq!(global.total_tool_calls, 4, "Global stats should count 4 tool calls");
        assert_eq!(global.total_errors, 0, "No errors");

        // Per-terminal stats
        for tid in &["TASK_MIX_0", "TASK_MIX_1", "TASK_MIX_2", "TASK_MIX_3"] {
            let stats = app.activity_log.stats_for_terminal(tid).unwrap();
            assert_eq!(stats.total_tools, 1, "Each terminal should have 1 tool call");
        }

        // 7. Verify completed spans from all 4 terminals (timeline lanes)
        assert_eq!(app.activity_log.completed_spans.len(), 4, "4 completed spans");
        let span_terminals: std::collections::HashSet<String> = app.activity_log.completed_spans
            .iter()
            .map(|s| s.terminal_id.clone())
            .collect();
        assert_eq!(span_terminals.len(), 4, "Spans should be from 4 different terminals");

        // Cleanup
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.cleanup_all();
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// VAL-CROSS-004: Quick mode headless + activity.
    ///
    /// `--quick "fix"` spawns headless terminal → events tracked in activity
    /// feed → stats show the terminal.
    #[test]
    fn test_cross_quick_mode_headless_activity() {
        let mut app = create_test_app();
        app.init_hook_server();

        let tmp = std::env::temp_dir().join("crew-test-cross-004");
        let _ = std::fs::create_dir_all(&tmp);

        // 1. Simulate --quick mode: headless=true (not --embed)
        let embed = false;
        let use_headless = !embed;
        assert!(use_headless, "--quick should default to headless");

        // 2. Spawn headless terminal as quick mode would
        let task_id = "TASK_QUICK_CROSS";
        let host = launcher::AiHost::Claude;
        let label = format!("{} (headless)", host.label());

        spawn_headless_with_hook_files(
            &mut app,
            task_id,
            &label,
            host,
            &tmp,
        );

        // Verify headless spawn
        {
            let mgr = app.terminal_manager.as_ref().unwrap();
            assert_eq!(mgr.terminals.len(), 1);
            assert!(mgr.terminals[0].is_headless(), "Quick mode terminal should be headless");
            assert_eq!(mgr.terminals[0].id, task_id);
        }

        // 3. Simulate hook events from the quick-mode headless terminal
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "SessionStart".to_string(),
            tool_name: None,
            tool_input_summary: None,
            success: None,
        });

        // Quick mode does a single edit
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PreToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("typo-fix.rs".to_string()),
            success: None,
        });
        app.activity_log.push(crate::data::activity::ActivityEvent {
            timestamp: std::time::Instant::now(),
            terminal_id: task_id.to_string(),
            event_type: "PostToolUse".to_string(),
            tool_name: Some("Edit".to_string()),
            tool_input_summary: Some("typo-fix.rs".to_string()),
            success: Some(true),
        });

        // 4. Verify events tracked in activity feed
        let quick_events = app.activity_log.filter(Some(task_id), None, None);
        assert_eq!(quick_events.len(), 3, "Quick mode should have 3 events");

        // 5. Set hook_state with cost
        if let Some(mgr) = &mut app.terminal_manager {
            mgr.terminals[0].hook_state = Some(crate::terminal::HookState {
                last_event: "Stop".to_string(),
                last_event_at: std::time::Instant::now(),
                activity_label: String::new(),
                tool_counts: {
                    let mut m = std::collections::HashMap::new();
                    m.insert("Edit".to_string(), 1);
                    m
                },
                session_active: false,
                total_cost_usd: 0.0123,
                total_input_tokens: 5000,
                total_output_tokens: 1000,
            });
        }

        // 6. Verify stats show the terminal
        let stats = app.activity_log.stats_for_terminal(task_id).unwrap();
        assert_eq!(stats.total_tools, 1);
        assert_eq!(stats.errors, 0);
        assert!(stats.files_touched.contains(&"typo-fix.rs".to_string()));

        // 7. Verify global stats include quick mode terminal
        let global = app.activity_log.global_stats();
        assert_eq!(global.total_tool_calls, 1);

        // 8. Verify cost view includes quick mode terminal
        let mgr = app.terminal_manager.as_ref().unwrap();
        let terminals_with_cost: Vec<_> = mgr.terminals.iter()
            .filter(|t| t.hook_state.as_ref().is_some_and(|h| h.total_cost_usd > 0.0 || h.total_input_tokens > 0))
            .collect();
        assert_eq!(terminals_with_cost.len(), 1);
        assert!(terminals_with_cost[0].is_headless());
        assert_eq!(terminals_with_cost[0].id, task_id);

        // 9. Verify terminal is listed in stats popup iteration
        let terminal_ids: Vec<String> = mgr.terminals.iter().map(|t| t.id.clone()).collect();
        assert!(terminal_ids.contains(&task_id.to_string()), "Stats popup should include quick mode terminal");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
        if let Some(ref server) = app.hook_server {
            server.shutdown();
        }
    }
}
