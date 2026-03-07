# Changelog

All notable changes to crew-board are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.5.0] - 2026-03-07

### Added
- **Hook-based communication** — HTTP hook server (`hook_server.rs`) receives
  structured events from Claude Code hooks. Real-time visibility into tool usage,
  permission requests, and session lifecycle without screen-scraping.
- **Activity Feed (View 6)** — new view showing a real-time event stream from all
  terminals. Filter by terminal (`t`), event type (`e`), or tool name (`f`).
  Toggle auto-scroll with `a`. Toggle Gantt timeline view with `g`.
- **Gantt timeline** — visualize tool execution spans across terminals as
  color-coded horizontal bars. Per-terminal swim lanes, time scale, and tool
  color legend. Toggle with `g` in View 6.
- **Permission approval via hooks** — permission requests route through the hook
  server with configurable profiles: `interactive` (manual F8 popup), `trusted`
  (auto-approve matching patterns), `autonomous` (approve all).
- **Context injection** — `SessionStart` hook injects task context (description,
  phase, decisions) into Claude Code via `additionalContext` JSON field.
- **History logging** — all hook events appended to `{task_dir}/history.jsonl`
  as structured JSONL for audit trail.
- **Activity log** (`data/activity.rs`) — ring buffer of 500 events with
  per-terminal stats, tool span tracking, and global aggregation.
- **File claims registry** (`data/file_claims.rs`) — cross-terminal file
  conflict detection with 10-minute claim expiry.
- **Security rules engine** (`security.rs`) — configurable regex rules for tool
  governance (deny/ask/allow), credential scanning, sensitive file protection,
  and per-terminal rate limiting.
- **Auto-orchestration engine** (`orchestration.rs`) — task scheduling with
  dependency resolution, circuit breaker (3 failures → downgrade), cost ceiling,
  and concurrent terminal limits. Modes: Manual, SemiAuto, FullAuto.
- **Cross-platform hook bridge** (`hook_bridge.rs`) — bridge scripts and config
  generation for Gemini CLI, GitHub Copilot, and OpenCode. Event name
  normalization across hosts.
- **Statistics popup** (`Ctrl+F6`) — global stats, security metrics,
  orchestration status, and per-terminal breakdown.
- **F5/F6 terminal cycling** — in focused mode, `F5` goes to previous terminal,
  `F6` to next (skips exited, wraps around).
- **F7 attention bypass** — `F7` jumps to next attention terminal even while in
  focused mode, without needing to exit first.
- **Hook state in terminal borders** — active tool label and cumulative tool
  counts shown in terminal list and borders.
- **Crew summary line enrichment** — hook activity label and tool count badges
  in the crew list.

### Changed
- **Focused mode keybindings** — replaced Shift+PgUp/PgDn (intercepted by
  Windows Terminal, causing screen corruption) with F5/F6 for terminal cycling.
- Permission popup now shows both PTY-scanned prompts and hook-based pending
  permissions with unified approve/deny UX.
- Terminal view borders show hook-driven activity labels when available.
- Status bar shows attention badge `5:Terms[⚠N]` and hook event stats.
- Help popup updated with all new key bindings (F5/F6/F7 focused mode, View 6
  keys, Ctrl+F6 stats).

### Fixed
- **Shift+PgUp/PgDn screen corruption** — Windows Terminal intercepts these keys
  for its own scrollback, corrupting the TUI. Replaced with F5/F6.

## [0.4.0] - 2026-03-05

### Added
- **Embedded terminal multiplexer** (View 5) — full PTY terminals inside the
  TUI. Run Claude Code, Gemini CLI, GitHub Copilot, or OpenCode with ANSI color,
  cursor positioning, and alternate screen support.
- **Permission prompt detection** — reader thread scans terminal output for
  `Allow`/`Deny`, `(y/n)`, `do you want to proceed` patterns. Yellow attention
  badges in the crew list, F7 jumps to next blocked crew.
- **Permission Queue (F8)** — centralized popup for batch permission management.
  Approve (`a`), deny (`d`), approve all (`A`), type custom response (`t`),
  or view terminal (`v`/`Enter`).
- **Permission profiles** — `interactive` (manual), `trusted` (regex auto-
  approve), `autonomous` (approve everything). Configurable in settings.
- **Four terminal layouts** — Focused (one large), Tiled-2 (side-by-side),
  Tiled-4 (2x2 grid), Stacked (vertical). Cycle with `l`.
- **Terminal input modes** — Normal (navigate list), TerminalFocused (all keys
  to PTY, F12 exits), ScrollBack (browse history with search).
- **Terminal search** — `/` in scroll-back to search output, `n`/`N` to
  navigate matches.
- **Mouse support** — click+drag text selection in terminal panels (constrained
  to panel boundaries), scroll wheel for scrollback, auto-copy via OSC 52.
- **Crew summary line** — compact status line showing all crew members when in
  focused layout with multiple terminals.
- **Phase & progress in title** — terminal borders show workflow phase and
  implementation progress (e.g., `[implementer 60%]`).
- **Terminal output logging** — `log_directory` setting captures all PTY output
  to per-task log files.
- **Desktop notifications** — `desktop_notifications` setting triggers OS
  notifications on attention events.
- **Context-adaptive F-key bar** — status bar changes based on terminal input
  mode (Normal/Focused/ScrollBack), with modifier layers (Shift/Ctrl).
- **Kitty keyboard protocol** — runtime detection and opt-in for modifier-only
  key press events. Modifier bar updates in real-time on supporting terminals.
- **Full modifier encoding** — Ctrl+Arrow, Ctrl+Enter, Shift+Up/Down, and all
  modifier+key combinations work correctly inside embedded terminals.
- **Quit confirmation** — dialog summarizes running/attention terminals before
  quitting.
- `F9`/`F12` as focus toggle, `Shift+F1-F5` for view switching while focused.

### Changed
- View count increased from 4 to 5 (Terminals view added as View 5).
- Status bar "5:Terms" tab shows attention `[⚠N]` and exited `[✗N]` badges.
- Idle detection at 120s (configurable) with attention badge.

## [0.3.2] - 2026-02-27

### Fixed
- **Search debounce** — F3 search now waits 200ms after the last keystroke before
  running the search, instead of searching on every single keypress. Reduces
  unnecessary disk I/O when typing quickly.

## [0.3.1] - 2026-02-26

### Fixed
- **Tree navigation no longer wraps** — pressing Down at the last item or Up at
  the first item now stops (clamps) instead of wrapping around to the other end

### Added
- **Arrow key expand/collapse** — Right arrow (or `l`) expands a repo node,
  Left arrow collapses it
- **PgUp/PgDn tree paging** — page through the task list a screenful at a time,
  clamped to top/bottom like the arrow keys
- **Vertical scrollbars** — both left pane (task list) and right pane (detail,
  documents, history, repo summary) now show a scrollbar when content overflows
  the visible area

## [0.3.0] - 2026-02-23

### Added
- **Archived task support** — deleted tasks now appear in the tree view with a
  `✗` marker and `[deleted]` label instead of silently disappearing
- **Gap detection** — task IDs between 1 and the highest known ID that are
  missing from disk are filled in as archived placeholders
- **`.registry.jsonl`** — append-only registry in `.tasks/` records task
  metadata at creation time; used to enrich archived task display with
  description and branch info
- **`metadata.json` fallback** — task directories with no `state.json` but a
  `metadata.json` (written by external setup scripts) are loaded as archived
  tasks with description, branch, and Jira key
- **Jira key display** — archived tasks from `metadata.json` show their Jira
  key in the task list row and detail pane (yellow text)
- **`LoadedTask` wrapper** — replaces bare `(PathBuf, TaskState)` tuples with a
  struct carrying `dir`, `state`, `archived` flag, and optional `jira_key`
- Repo summary now shows archived task count alongside active/total

### Changed
- Data layer refactored from `Vec<(PathBuf, TaskState)>` to `Vec<LoadedTask>`
  across all modules (app, cleanup, cost_view, search, detail_pane, task_list)
- Archived tasks are excluded from cleanup candidates, search file scanning,
  document/history views, and terminal launch
- `active_task_count()` excludes archived tasks; new `archived_task_count()`
  method added to `RepoData`

## [0.2.0] - 2026-02-19

### Added
- **F1 Help overlay** listing all keybindings (press any key to dismiss)
- **Norton Commander-style F-key bar** at the bottom of the screen
  (`F1Help  F2Launch  F3Search  F4New  F5Rfrsh  ...  F10Quit`)
- **Enhanced History view** ("State Inspector") showing full state.json details:
  workflow mode (effective, requested, detection reason, confidence, cost estimate),
  implementation progress bar with step breakdown, worktree details (branch, base,
  path, color, AI host, terminal), documentation gaps, and cost summary
- `F10` as an additional quit binding

### Changed
- **Esc never quits** — it only closes popups, backs out of detail views, or
  switches focus from right pane to left pane. Use `q` or `F10` to quit.
- New worktree shortcut moved from `n` to `F4` for F-key consistency
- Status bar redesigned: line 1 shows view tabs + navigation hints + stats,
  line 2 shows the NC-style F-key bar (or popup hints when a popup is open)
- Context hints simplified (F-key actions removed since they're in the bar)

## [0.1.0] - 2026-02-18

### Added
- Initial release — Norton Commander-style TUI dashboard
- Dual-pane layout: tree view (left) + detail pane (right)
- Four views: Tasks, Issues (beads), Config cascade, Cost summary
- Task tree with repo grouping, expand/collapse, phase indicators
- Detail pane modes: Overview, Document list, Document reader, History
- Markdown rendering with basic syntax highlighting in document reader
- Beads issue tracker integration (`.beads/issues.jsonl`)
- Config cascade display (global/project/task YAML)
- `F2` Launch popup: terminal detection + AI host selection
- `F3` Full-text search across tasks, state.json, and artifact files
- `n` New worktree wizard: description input, host selection, settings toggles,
  background git operations, color-themed terminal launch
- 8 color schemes (from Python `CREW_COLOR_SCHEMES`), assigned per task
- Auto-refresh on configurable poll interval
- `~/.config/crew-board.toml` for persistent scan paths and settings
- CI/CD pipeline: 6-target build matrix (Linux/Windows/macOS, amd64/arm64),
  clippy + test gates, GitHub Release with platform binaries
