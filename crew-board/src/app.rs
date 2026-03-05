use crate::cleanup;
use crate::data::task::{self, Discovery, Interaction, TaskArtifact};
use crate::data::RepoData;
use crate::launcher::{self, AiHost, TerminalEnv};
use crate::terminal::TerminalManager;
use crate::worktree;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use ratatui::layout::Rect;
use tui_input::Input;

/// Which view/tab is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActiveView {
    Tasks,
    BeadsIssues,
    Config,
    CostSummary,
    Terminals,
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
}

#[derive(PartialEq)]
pub enum LaunchStep {
    SelectTerminal,
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

    // Issue navigation (for beads view)
    pub selected_issue: usize,

    pub active_view: ActiveView,
    pub focus_pane: FocusPane,

    // UI state
    pub should_quit: bool,
    pub show_help: bool,
    pub last_refresh: std::time::Instant,
    pub detail_scroll: u16,
    /// Max scroll offset computed during rendering (clamping target).
    pub detail_scroll_max: Cell<u16>,

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

    // Modifier F-key bar state
    /// Which modifier layer is currently shown on the F-key bar.
    pub modifier_bar_state: ModifierBarState,
    /// When set, the modifier bar reverts to Normal after this instant (flash fallback).
    pub modifier_bar_flash_until: Option<std::time::Instant>,
    /// Whether the kitty keyboard protocol is active (modifier-only detection).
    pub kitty_protocol_enabled: bool,

    // Mouse text selection
    /// Current text selection state (if any).
    pub text_selection: Option<TextSelection>,
    /// Panel rects for terminal panels, set during draw for mouse hit-testing.
    /// Vec of (terminal_index, inner_rect).
    pub terminal_panel_rects: RefCell<Vec<(usize, Rect)>>,
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
            selected_issue: 0,
            active_view: ActiveView::Tasks,
            focus_pane: FocusPane::Left,
            should_quit: false,
            show_help: false,
            last_refresh: std::time::Instant::now(),
            detail_scroll: 0,
            detail_scroll_max: Cell::new(0),
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
            modifier_bar_state: ModifierBarState::Normal,
            modifier_bar_flash_until: None,
            kitty_protocol_enabled: false,
            text_selection: None,
            terminal_panel_rects: RefCell::new(Vec::new()),
        };
        app.rebuild_tree();
        app.ensure_artifacts();
        app
    }

    /// Rebuild the flattened tree from repos + expanded state.
    pub fn rebuild_tree(&mut self) {
        self.tree_rows.clear();
        for (ri, repo) in self.repos.iter().enumerate() {
            self.tree_rows.push(TreeRow::Repo(ri));
            if self.expanded_repos.contains(&ri) {
                for ti in 0..repo.tasks.len() {
                    self.tree_rows.push(TreeRow::Task(ri, ti));
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
            ActiveView::Terminals => ActiveView::Tasks,
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
                shell_cmd: String,
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
                    popup.step = LaunchStep::SelectHost;
                    Outcome::None
                }
                LaunchStep::SelectHost => {
                    let terminal = popup.terminals[popup.terminal_cursor];
                    let host = popup.hosts[popup.host_cursor];

                    if terminal == launcher::TerminalEnv::Embedded {
                        let task_id = popup.task_id.clone();
                        let work_dir = popup.work_dir.clone();
                        let color_idx = popup.color_scheme_index;
                        let host_label = host.label().to_string();
                        let shell_cmd = match host {
                            launcher::AiHost::Shell => String::new(),
                            launcher::AiHost::Copilot | launcher::AiHost::OpenCode => {
                                host.command().to_string()
                            }
                            _ => format!(
                                "{} \"/crew resume {}\"",
                                host.command(),
                                task_id
                            ),
                        };
                        Outcome::SpawnEmbedded {
                            task_id,
                            host_label,
                            shell_cmd,
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
                shell_cmd,
                work_dir,
                color_idx,
            } => {
                if shell_cmd.is_empty() {
                    self.spawn_terminal(
                        &task_id, &host_label, "bash", &[], &work_dir, color_idx,
                    );
                } else {
                    self.spawn_terminal(
                        &task_id,
                        &host_label,
                        "bash",
                        &["-lc".to_string(), shell_cmd],
                        &work_dir,
                        color_idx,
                    );
                }
                self.launch_popup = None;
                self.active_view = ActiveView::Terminals;
            }
            Outcome::None => {}
        }
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
                        if p.settings_cursor < 1 {
                            p.settings_cursor += 1;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(p) = &mut self.create_popup {
                        match p.settings_cursor {
                            0 => p.pull = !p.pull,
                            1 => p.launch_after = !p.launch_after,
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
        let popup = match &self.create_popup {
            Some(p) => p,
            None => return,
        };

        if popup.launch_after {
            if let Some(Ok(ref result)) = popup.result {
                let terminals = launcher::detect_terminals();
                if let Some(&terminal) = terminals.first() {
                    let host = popup.hosts[popup.host_cursor];
                    let cs = launcher::get_hex_scheme(result.color_scheme_index);
                    let _ = launcher::launch(
                        terminal,
                        host,
                        &result.worktree_abs,
                        &result.task_id,
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

        // Pre-select completed tasks by default
        let mut selected = HashSet::new();
        for (i, c) in candidates.iter().enumerate() {
            if c.is_complete {
                selected.insert(i);
            }
        }

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
                    if let Some(p) = &mut self.cleanup_popup {
                        let idx = p.cursor;
                        if p.selected.contains(&idx) {
                            p.selected.remove(&idx);
                        } else {
                            p.selected.insert(idx);
                        }
                    }
                }
                KeyCode::Char('a') => {
                    if let Some(p) = &mut self.cleanup_popup {
                        if p.selected.len() == p.candidates.len() {
                            p.selected.clear();
                        } else {
                            p.selected = (0..p.candidates.len()).collect();
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
                // Approve: send 'y\n' to the selected terminal
                if let Some(&term_idx) = popup.entries.get(popup.cursor) {
                    if let Some(mgr) = &self.terminal_manager {
                        if let Some(term) = mgr.terminals.get(term_idx) {
                            let _ = term.writer.lock().unwrap().write_all(b"y\n");
                            let _ = term.writer.lock().unwrap().flush();
                            *term.attention_signal.lock().unwrap() = None;
                        }
                    }
                    // Refresh entries
                    let new_popup = crate::ui::permission_popup::PermissionPopup::new(self);
                    self.permission_popup = Some(new_popup);
                }
            }
            KeyCode::Char('d') => {
                // Deny: send 'n\n' to the selected terminal
                if let Some(&term_idx) = popup.entries.get(popup.cursor) {
                    if let Some(mgr) = &self.terminal_manager {
                        if let Some(term) = mgr.terminals.get(term_idx) {
                            let _ = term.writer.lock().unwrap().write_all(b"n\n");
                            let _ = term.writer.lock().unwrap().flush();
                            *term.attention_signal.lock().unwrap() = None;
                        }
                    }
                    let new_popup = crate::ui::permission_popup::PermissionPopup::new(self);
                    self.permission_popup = Some(new_popup);
                }
            }
            KeyCode::Char('v') | KeyCode::Enter => {
                // View terminal: close popup, switch to Terminals view, focus the terminal
                if let Some(&term_idx) = popup.entries.get(popup.cursor) {
                    self.permission_popup = None;
                    self.active_view = ActiveView::Terminals;
                    if let Some(mgr) = &mut self.terminal_manager {
                        if term_idx < mgr.terminals.len() {
                            mgr.focused = term_idx;
                        }
                    }
                }
            }
            _ => {}
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
        if let Some(mgr) = &mut self.terminal_manager {
            // Use a default size; will be resized on next draw
            let _ = mgr.spawn(
                task_id.to_string(),
                label.to_string(),
                command,
                args,
                cwd,
                24,
                80,
                color_scheme_index,
            );
        }
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
        let changed = if let Some(mgr) = &mut self.terminal_manager {
            mgr.poll_status()
        } else {
            false
        };

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

        changed
    }

    /// Dismiss (remove) the currently focused terminal.
    pub fn terminal_dismiss_focused(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal() {
                let id = term.id.clone();
                mgr.remove(&id);
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
    pub fn terminal_scroll_up(&mut self, lines: usize) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                let max_scrollback =
                    crate::terminal::widget::scrollback_available(&term.parser);
                term.scroll_offset = (term.scroll_offset + lines).min(max_scrollback);
            }
        }
    }

    /// Scroll the focused terminal down by N lines.
    pub fn terminal_scroll_down(&mut self, lines: usize) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                term.scroll_offset = term.scroll_offset.saturating_sub(lines);
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
    pub fn terminal_scroll_to_top(&mut self) {
        if let Some(mgr) = &mut self.terminal_manager {
            if let Some(term) = mgr.focused_terminal_mut() {
                let max_scrollback =
                    crate::terminal::widget::scrollback_available(&term.parser);
                term.scroll_offset = max_scrollback;
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
                // Single click — clear selection
                self.text_selection = None;
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

        let p = term.parser.lock().unwrap();
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
                return;
            }
        }
    }
}
