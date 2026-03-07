# crew-board — Agent Instructions

Rust TUI dashboard for cross-project agentic-workflow task monitoring. Built with ratatui + crossterm.

## Build & Test

```bash
cd crew-board
cargo build                  # Dev build
cargo build --release        # Optimized binary (strip + LTO)
cargo test                   # All ~110 unit tests
cargo clippy                 # Lint (fix all warnings before committing)
```

The release binary lands at `target/release/crew-board` (~1.6MB).

## Architecture

```
src/
├── main.rs          # CLI parsing (clap), terminal setup, event loop, bg refresh polling
├── app.rs           # Application state: tree nav, views, popups, search index, bg refresh
├── settings.rs      # Loads ~/.config/crew-board.toml (TOML)
├── hook_server.rs   # HTTP hook server (127.0.0.1:0): receives Claude Code hook events, auth, mpsc dispatch
├── discovery.rs     # Repo discovery: --repo paths + --scan directories
├── launcher.rs      # Terminal launch: detect env, spawn wt.exe/tmux/osascript, color schemes
├── worktree.rs      # Native worktree creation: task ID, git ops, state.json, symlink
├── cleanup.rs       # Worktree cleanup: candidate discovery, dry-run preview, execute via script
├── orchestration.rs # Auto-orchestration engine: modes, tasks, circuit breaker, guardrails
├── security.rs      # Security rules engine: regex rules, credential scanning, rate limiter, sensitive files
├── hook_bridge.rs   # Cross-platform hook support: Gemini/Copilot/OpenCode bridge scripts, event normalization
├── data/
│   ├── mod.rs       # RepoData: load + load_incremental, aggregates tasks + issues + config
│   ├── task.rs      # Parses .tasks/*/state.json, incremental mtime reload, *.md artifacts
│   ├── beads.rs     # Parses .beads/issues.jsonl (stream, skip malformed)
│   ├── config.rs    # Config cascade: global → project → task levels
│   ├── activity.rs  # Activity event log: ring buffer, per-terminal stats, tool spans for timeline
│   └── file_claims.rs # File claims registry: cross-terminal conflict detection, 10-min expiry
├── terminal/
│   ├── mod.rs       # TerminalManager, EmbeddedTerminal, TerminalStatus, AttentionReason, HookState, LaunchParams
│   ├── pty.rs       # spawn_pty(), PtyHandles, ExitEvent, AttentionEvent, AttentionKind, reader thread
│   └── widget.rs    # Terminal rendering (draw_terminal), keyboard encoding (key_to_bytes), SelectionRange
└── ui/
    ├── mod.rs        # Root layout: main content + status bar + popup overlay
    ├── task_list.rs  # Left pane: tree view with repo/task rows + vertical scrollbar
    ├── detail_pane.rs# Right pane: overview, doc list, doc reader, history + vertical scrollbar
    ├── beads_view.rs # View 2: issues list + detail
    ├── config_view.rs# View 3: config cascade display
    ├── cost_view.rs  # View 4: cost summary from workflow state
    ├── terminal_view.rs # View 5: embedded terminal multiplexer (list panel + PTY output + mouse hit rects)
    ├── status_bar.rs # Bottom: view tabs + contextual keybinding hints + aggregate stats
    ├── launch_popup.rs # F2 popup: terminal + AI host selection
    ├── permission_popup.rs # F8 popup: permission queue for pending approvals
    ├── create_popup.rs # F4 popup: multi-step worktree creation wizard
    ├── cleanup_popup.rs# F6 popup: multi-select worktree cleanup with dry-run preview
    ├── search_popup.rs # F3 popup: full-text search across tasks + artifacts
    ├── help_popup.rs  # F1 popup: scrollable help overlay with all key bindings
    ├── activity_view.rs # View 6: Activity Feed with event table + Gantt timeline
    ├── stats_popup.rs   # Ctrl+F6: Statistics popup with global/security/orchestration/per-terminal breakdown
    └── styles.rs     # 8 crew color schemes, phase styles, selection/border/hint helpers
```

## Key Patterns

### Data Layer
- All structs use `#[serde(default)]` — tolerant of missing/extra fields
- `load_tasks()` and `load_issues()` silently skip malformed entries
- `.tasks/` symlinks in worktrees are resolved via `canonicalize()`
- Artifacts (architect.md, developer.md, etc.) are discovered at runtime from the task directory
- **metadata.json fallback**: When a task dir has no `state.json`, `load_tasks()` tries `metadata.json` (written by external setup scripts with a different schema). `TaskMetadata` struct deserializes it, `TaskState::from_metadata()` maps fields to a minimal TaskState, and the task is marked `archived: true`. The `jira_key` field lives on `LoadedTask` (not `TaskState`) since it's external metadata.
- **Incremental refresh**: `load_tasks_incremental()` compares `state.json` mtime against the stored `state_mtime` on each `LoadedTask`. Only tasks whose mtime changed are re-read from disk. `RepoData::load_incremental()` wraps this for the full repo. First load in `App::new()` is always a full `load_tasks()`.
- **`state_mtime` field**: `LoadedTask` carries `state_mtime: Option<SystemTime>` — the mtime of `state.json` at last read. `None` for archived/fallback tasks. This field powers the incremental refresh comparison.

### Navigation Model
- Tree view: flattened `Vec<TreeRow>` where `TreeRow` is either `Repo(idx)` or `Task(repo_idx, task_idx)`
- `expanded_repos: HashSet<usize>` tracks which repos are open
- `rebuild_tree()` must be called after any expand/collapse or data refresh
- Tree cursor clamps at boundaries (does not wrap around)
- Navigation methods: `tree_down()`, `tree_up()`, `tree_page_down(page_size)`, `tree_page_up(page_size)`, `tree_expand()`, `tree_collapse()`
- Key bindings: Up/Down/j/k for single step, PgUp/PgDn for page, Right/l to expand repo, Left to collapse repo

### Detail Pane Modes
```
Overview ──d──> DocList ──Enter──> DocReader
    │                      │            │
    │<─────Esc────────────Esc──────Esc──┘
    │
    │──h──> History
    │          │
    │<───Esc───┘
```
- `cached_artifacts` and `cached_task_dir` prevent re-scanning on every draw
- `ensure_artifacts()` is called on tree cursor change and refresh

### Event Loop
Keys are routed in priority order:
1. Resize events → `terminal_resize_all()`
2. Mouse events → selection (Down/Drag/Up) + scroll (ScrollUp/ScrollDown) in terminal panels
3. Key release events (kitty protocol) → update `modifier_bar_state` based on remaining modifiers
4. Modifier-only key press events (kitty protocol) → update `modifier_bar_state`, continue
5. Help overlay open → any key closes it
6. Terminal view + `ScrollBack` mode → scroll navigation keys
7. Terminal view + `TerminalFocused` mode → `F12` exits, Shift+F-keys switch views, Shift+PgUp/PgDn cycle terminals, all other keys forwarded to PTY via `key_to_bytes()`
8. Search popup open → search-specific keys
9. Create worktree popup open → popup keys (text input, selection, toggles)
10. Cleanup worktree popup open → popup keys (multi-select, toggles, preview)
11. Permission queue popup open → popup keys (approve/deny/view)
12. Launch popup open → popup keys only
13. Right pane focused + non-Overview mode → doc/history navigation
14. Default → Shift+F-key layer → Ctrl+F-key layer → base F-keys, tree nav, view switching

**Esc behavior:** Esc never quits the application. It only closes popups, backs out of detail views, or switches focus from right pane to left pane. Use `q` or `F10` to quit.

**Poll timeout:** The `event::poll()` timeout is **50ms** when the Terminals view is active with at least one running terminal, or when a search debounce is pending. Otherwise it is **250ms**. This provides responsive PTY rendering without unnecessary CPU use.

### Refresh & Background Threading
All data refresh is **non-blocking**. `refresh()` spawns a background thread via `start_bg_refresh()`. Each event-loop tick calls `check_bg_refresh()` which polls `JoinHandle::is_finished()` and swaps in the new data on completion.

- **Incremental**: Background thread uses `RepoData::load_incremental()` — only re-reads `state.json` files whose mtime changed. Issues and config are still fully reloaded (single files, cheap).
- **Search index**: Built in the background thread alongside the refresh. The `Vec<SearchEntry>` is swapped in atomically with the new repos.
- **Timer reset**: `last_refresh` is reset on **completion** (not start), preventing rapid re-refresh if the background thread takes longer than `poll_interval_secs`.
- **Coalescing**: If a refresh is already in progress, `start_bg_refresh()` is a no-op. Multiple F5 presses or auto-refresh triggers are safely coalesced.
- **Callers affected**: `refresh()` is also called from `close_create_popup()` and `close_cleanup_popup()`. After the change these get non-blocking behavior — data appears after the next tick rather than synchronously.
- **Terminal polling**: `app.poll_terminals()` is called every tick (after `check_bg_refresh()`). It delegates to `TerminalManager::poll_status()`. This is separate from the background data-refresh thread — terminal status updates happen synchronously in the event loop tick, not in a background thread.

### Hook-Based Communication

When `hook_communication = true` (default), crew-board opens a lightweight HTTP server (`hook_server.rs`) on `127.0.0.1:0` at startup and injects hook configuration into each Claude Code terminal it spawns. This gives the dashboard structured real-time visibility into what each agent is doing.

**Startup and lifecycle:**

1. `App::init_hook_server()` is called from `main.rs` after settings are applied. It calls `HookServer::start()`, which binds `tiny_http` to `127.0.0.1:0` (OS-assigned port), starts a background thread polling the socket, and returns `(HookServer, port, Receiver<HookEvent>, Receiver<PendingPermission>)`. The port, server handle, and both receivers are stored on `App`.
2. Before spawning a Claude Code terminal, `App::generate_hook_config(task_id, command, cwd)` generates a per-terminal auth token, calls `HookServer::register_token_with_profile(terminal_id, token, context, profile, patterns)`, and writes `.claude/settings.local.json` in the worktree directory with HTTP hook entries for all 7 event types (including `PermissionRequest`) pointing to `http://127.0.0.1:{port}/hook/{task_id}`. `PreToolUse` and `PermissionRequest` hooks use 30s timeout (for blocking approval); others use 5s.
3. Three env vars are passed to the terminal process: `CREW_BOARD_PORT`, `CREW_BOARD_TASK_ID`, `CREW_BOARD_TOKEN`. The settings file uses `$CREW_BOARD_TOKEN` and `allowedEnvVars` so Claude Code expands the token at hook send time.
4. `App::drain_hook_events()` is called each event-loop tick. It drains both the `mpsc::Receiver<HookEvent>` (activity events) and `mpsc::Receiver<PendingPermission>` (queued permission requests), routes events to terminals via `process_hook_event()`, and appends pending permissions to `App.pending_permissions`.
5. On terminal dismiss (`d`/`Delete`) or app exit, `settings.local.json` is deleted from the worktree and `HookServer::deregister_token()` is called. `HookServer` also deregisters on `Drop`.

**`HookEvent` enum** (in `hook_server.rs`):

| Variant | Key Fields |
|---------|-----------|
| `SessionStart` | `session_id` |
| `PreToolUse` | `tool_name`, `tool_input_summary` |
| `PostToolUse` | `tool_name`, `tool_input_summary`, `success` |
| `PermissionRequest` | `tool_name`, `tool_input: serde_json::Value` |
| `Notification` | `message` |
| `Stop` | `preview` |
| `SessionEnd` | — |

**`HookState`** (per-terminal, in `terminal/mod.rs`):

| Field | Description |
|-------|-------------|
| `last_event` | Name of the most recent event received (e.g. `"PreToolUse"`). |
| `last_event_at` | `Instant` when the last event arrived. |
| `activity_label` | Human-readable tool activity (e.g. `"Edit src/main.rs"`). Set on `PreToolUse`, cleared on `PostToolUse`. |
| `tool_counts` | `HashMap<String, u32>` of cumulative tool invocations per tool name. |
| `session_active` | `true` between `SessionStart` and `SessionEnd`. |

`HookState` is `Option<HookState>` on `EmbeddedTerminal` — `None` until the first hook event arrives for that terminal.

**`AttentionReason::HookNotification`:** A `Notification` hook event triggers `NeedsAttention(HookNotification { message })`. The border turns bold yellow, border title shows `[NOTIFY]`, and the status bar attention count increments. Icon in the crew summary line: `◆` (same as other attention states).

**Permission approval via hooks (Phase 2):** For `PreToolUse` and `PermissionRequest` events, the hook server applies the permission profile before responding:
- **Autonomous**: Immediate `200` with `{"hookSpecificOutput": {"permissionDecision": "allow"}}` — server thread never blocks.
- **Trusted**: Checks `auto_approve_patterns` (regex match against tool name). Match → allow; no match → queue.
- **Interactive**: Always queues for user approval.

When queued, the server thread creates an `mpsc::channel`, sends a `PendingPermission` to the main thread, and blocks on `recv_timeout(25s)`. The main thread shows the pending permission in the F8 popup. When the user approves/denies, the decision is sent back through the channel, and the server responds with the appropriate `hookSpecificOutput` JSON. If the 25s timeout expires, a fallback `"ask"` response is sent (for PreToolUse) or `"allow"` (for PermissionRequest), letting Claude Code fall back to its built-in prompt.

**`PendingPermission`** (in `hook_server.rs`):

| Field | Description |
|-------|-------------|
| `terminal_id` | Which terminal this permission request came from. |
| `tool_name` | Name of the tool (e.g. `"Bash"`, `"Edit"`). |
| `tool_input_summary` | Short human-readable summary of the tool input. |
| `tool_input` | `Option<serde_json::Value>` — full tool input object for detailed display. |
| `event_type` | `"PreToolUse"` or `"PermissionRequest"` — determines response format. |
| `response_tx` | `mpsc::Sender<PermissionDecision>` — channel to send decision back to server thread. |

**`PermissionEntry`** (in `permission_popup.rs`): The F8 popup shows both PTY-scanned prompts (`PtyBased { terminal_idx }`) and hook-based pending permissions (`HookBased { pending_idx }`). PTY entries use `y\n`/`n\n` bytes; hook entries use the `response_tx` channel.

**Activity tracking (Phase 3):** Tool counts are tracked per-terminal in `HookState.tool_counts` (incremented on `PostToolUse`). The crew list shows a second line per terminal with the current activity label or aggregated tool counts. All hook events are appended to `{task_dir}/history.jsonl` as structured JSONL for audit trail.

**Context injection:** `SessionStart` hook responses include pre-computed task context (description, phase, decisions) via the `additionalContext` JSON field, so Claude Code automatically has task context at session start.

**Auth flow:** Each request must include `Authorization: Bearer <token>`. Unknown `terminal_id` values receive `200 OK` but the event is silently dropped (not a 401 — avoids leaking terminal existence). Invalid token receives `401`. The token registry is an `Arc<RwLock<HashMap<String, TerminalRegistration>>>` shared between the server thread and the main thread. `TerminalRegistration` holds the token, session context, permission profile, and auto-approve patterns.

### Embedded Terminal Multiplexer (View 5)

View 5 (`5` key) hosts PTY terminals that run agent processes inside the TUI. Key types live in `src/terminal/`:

**`pty.rs` — low-level PTY primitives**

| Type | Description |
|------|-------------|
| `PtyHandles` | Returned by `spawn_pty()`. Bundles `parser`, `writer`, `master`, `exit_signal`, `attention_signal`, `last_output`. |
| `ExitEvent` | `{ code: i32, timestamp: Instant }` — written by the reader thread on EOF. |
| `AttentionKind` | Enum: `PermissionPrompt { line }`, `Idle { seconds }`, `Error { line }`. |
| `AttentionEvent` | `{ kind: AttentionKind, timestamp: Instant }` — written by the reader thread. |
| `SharedExitSignal` | `Arc<Mutex<Option<ExitEvent>>>` shared between reader thread and `poll_status()`. |
| `SharedAttentionSignal` | `Arc<Mutex<Option<AttentionEvent>>>` shared between reader thread and `poll_status()`. |

**`mod.rs` — manager types**

| Type | Description |
|------|-------------|
| `EmbeddedTerminal` | Holds all `PtyHandles` fields plus `id`, `label`, `status`, `color_scheme_index`, `launch_params`, `scroll_offset`, `spawned_at`, `hook_state`, `hook_settings_cwd`. |
| `LaunchParams` | `{ command: String, args: Vec<String>, cwd: PathBuf }` — stored on each terminal for relaunch. |
| `TerminalStatus` | Enum: `Running`, `NeedsAttention(AttentionReason)`, `Exited(i32)`. |
| `AttentionReason` | Public mirror of `AttentionKind`: `PermissionPrompt { context: String }`, `Idle { seconds }`, `Error { context: String }`, `HookNotification { message: String }`. Context carries the actual prompt/error line for display in the F8 popup. `HookNotification` is triggered by Claude Code `Notification` hook events. |
| `HookState` | Per-terminal hook activity tracker: `last_event`, `last_event_at`, `activity_label`, `tool_counts`, `session_active`. `None` until first hook event arrives. |
| `TerminalManager` | `Vec<EmbeddedTerminal>` + `focused: usize`. Entry point for all terminal operations. Includes `focus_next_running()` / `focus_prev_running()` which skip exited terminals. |

#### Reader Thread Architecture

`spawn_pty()` starts a background reader thread that is **not** fire-and-forget — it actively writes to shared signals:

1. Reads PTY output in a 4096-byte loop, feeding bytes into the `vt100::Parser`.
2. Updates `last_output` (an `Arc<Mutex<Instant>>`) on every successful read.
3. Every 500ms (throttled) calls `scan_for_attention()` which scans the last 5 vt100 screen rows for permission prompts (`Allow`/`Deny`, `(y/n)`, `(yes/no)`, `do you want to proceed`, `press enter to continue`) and error prefixes (`error:`, `fatal:`, `panic at`). Writes result to `attention_signal`.
4. On EOF, calls `child.wait()` to get the exit code, then writes `ExitEvent` to `exit_signal`.

The reader thread exits only when the child process closes the slave PTY (EOF).

Scrollback is set to **1000 lines** (`vt100::Parser::new(rows, cols, 1000)`).

#### Status Polling

`TerminalManager::poll_status()` is called once per event-loop tick (via `App::poll_terminals()`):

- Skips terminals already in `Exited` state.
- Checks `exit_signal`; if set, transitions to `Exited(code)`.
- Checks whether `last_output` is older than **120 seconds**; if so, synthesizes an `Idle` attention event (only if no higher-priority attention is already set).
- Checks `attention_signal`; maps it to `NeedsAttention(reason)` or clears back to `Running` if the signal was cleared (e.g. by user input).
- Returns `true` if any terminal changed status (used to decide whether to redraw).

`App::poll_terminals()` additionally drops `TerminalInputMode` back to `Normal` when the focused terminal exits while in focused mode.

#### Process Exit Handling

When a terminal exits:

- Status becomes `Exited(code)`. Border turns gray (code 0) or red (non-zero).
- Border title shows `[exited: N - ok]` or `[exited: N - FAILED]`.
- A centered overlay appears: `"Process exited. Press Enter to relaunch, d to dismiss."`.
- `TerminalInputMode` automatically reverts to `Normal`.
- **`d` / Delete** — calls `terminal_dismiss_focused()`, which removes the terminal from the manager entirely.
- **Enter** — calls `terminal_relaunch_focused()`, which removes the old terminal and calls `TerminalManager::relaunch()`. Relaunch re-runs `spawn_pty()` with the stored `LaunchParams` and re-focuses the new terminal.

#### Attention Detection

When a terminal needs attention:

- Status becomes `NeedsAttention(reason)`. Border turns bold yellow.
- Border title appends `[PROMPT]`, `[idle Ns]`, or `[ERROR]` depending on the reason.
- The status bar "5:Terms" tab shows `5:Terms[⚠N]` (N = count of attention terminals).
- **F7** — calls `terminal_focus_next_attention()`, cycling through terminals that need attention.
- Sending any input to a terminal clears its attention signal (`send_input()` sets `attention_signal` to `None`).

#### Adaptive Refresh

The event-loop poll timeout in `main.rs` is reduced to **50ms** when the Terminals view is active and at least one terminal has `TerminalStatus::Running`. Otherwise the normal 250ms timeout applies. This keeps PTY output rendering responsive without burning CPU when no terminals are active.

#### Terminal Input Modes

`TerminalInputMode` has four states:

| Mode | Description |
|------|-------------|
| `Normal` | Keys navigate the terminal list. Standard crew-board navigation. |
| `TerminalFocused` | All keys go to the PTY. `F12` exits back to Normal. Shift+F-keys bypass for global view switching. Shift+PgUp/PgDn cycle terminals. |
| `PrefixPending` | Legacy (unused). Kept for enum compatibility; no code path enters this state. |
| `ScrollBack` | Navigate the vt100 scrollback buffer with Up/Down/PgUp/PgDn. |

#### Layout Modes

`TerminalLayout` controls how terminals are arranged in View 5:

| Layout | Description |
|--------|-------------|
| `Focused` | One large terminal + 20-col crew list (default). |
| `Tiled2` | Two terminals side by side + 18-col crew list. |
| `Tiled4` | Four terminals in 2x2 grid + 15-col crew list. |
| `Stacked` | Up to 5 terminals stacked vertically + 20-col crew list. |

Cycle with `l` / `Right` in Normal mode. Layout cannot be cycled while in TerminalFocused mode (all keys go to PTY). Persists from `terminal_layout` setting.

#### Terminal View Key Bindings (View 5)

**Normal mode:**

| Key | Action |
|-----|--------|
| `↑` / `k` | Focus previous terminal |
| `↓` / `j` | Focus next terminal |
| `F9` / `F12` | Enter `TerminalFocused` mode (running terminal required) |
| `Enter` (exited) | Relaunch the exited terminal |
| `d` / `Delete` (exited) | Dismiss (remove) the exited terminal |
| `D` | Dismiss ALL exited terminals at once |
| `l` / `Right` | Cycle layout mode (focused → tiled-2 → tiled-4 → stacked) |
| `[` | Enter scroll-back mode |
| `F7` | Jump focus to next terminal needing attention |
| `F8` | Open Permission Queue popup |

**TerminalFocused mode:**

| Key | Action |
|-----|--------|
| `F12` | Exit focus, return to Normal mode |
| `Shift+F1` | Exit focus + switch to Tasks view |
| `Shift+F2` | Exit focus + switch to Issues view |
| `Shift+F3` | Exit focus + switch to Config view |
| `Shift+F4` | Exit focus + switch to Cost view |
| `Shift+F5` | Exit focus (already in Terminals view) |
| `Shift+PgUp` | Focus previous running terminal (skips exited, wraps) |
| `Shift+PgDn` | Focus next running terminal (skips exited, wraps) |
| All other keys | Sent directly to the PTY via `key_to_bytes()` encoding |

**ScrollBack mode (entered via `[` in Normal mode or mouse scroll):**

| Key | Action |
|-----|--------|
| `↑` / `k` | Scroll up 1 line |
| `↓` / `j` | Scroll down 1 line |
| `PgUp` | Scroll up one page |
| `PgDn` | Scroll down one page |
| `Home` | Scroll to top of scrollback buffer |
| `End` | Scroll to live view (bottom) |
| `/` | Enter search mode (type query, Enter to search) |
| `n` | Jump to next search match |
| `N` | Jump to previous search match |
| `q` / `Esc` | Exit scroll-back, return to Normal |

Scrollback offset is stored per-terminal (`EmbeddedTerminal.scroll_offset`). The border turns magenta in scroll-back mode, with a `[N/total]` indicator at the bottom. Mouse scroll wheel also enters scroll-back mode (see Mouse Support below).

#### Permission Queue Popup (F8)

Shows all terminals with `NeedsAttention(PermissionPrompt { context })` or `NeedsAttention(Error { context })` status. Displays the actual context line from the terminal.

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate the list |
| `a` | Approve: send `y\n` to the selected terminal |
| `d` | Deny: send `n\n` to the selected terminal |
| `A` | Approve ALL: batch-approve all pending terminals |
| `t` | Type: enter quick-send input mode (type custom response) |
| `v` / `Enter` | View: close popup, switch to Terminals view, focus the terminal |
| `Esc` | Close popup |

#### Mouse Support (Terminal Panels)

Mouse capture is enabled globally (`EnableMouseCapture` / `DisableMouseCapture` in `main.rs`). Mouse events are handled in the event loop before keyboard events.

**Text Selection:**

| Event | Action |
|-------|--------|
| `MouseDown(Left)` | Start selection at clicked terminal panel (hit-tested via `terminal_panel_rects`) |
| `MouseDrag(Left)` | Extend selection, clamped to the originating panel bounds |
| `MouseUp(Left)` | Finish selection + auto-copy to clipboard via OSC 52 (if selection spans multiple cells) |

Selection state is tracked in `App.text_selection: Option<TextSelection>`. The `TextSelection` struct stores the terminal index, panel rect (for clamping), start/end coordinates (panel-relative), and an `active` flag. Panel rects are written into `App.terminal_panel_rects: RefCell<Vec<(usize, Rect)>>` during each draw cycle by `terminal_view.rs`.

Selected cells are highlighted with white text on deep blue background (`Color::Indexed(24)`) in `widget::draw_terminal()`.

**Scroll Wheel:**

| Event | Action |
|-------|--------|
| `ScrollUp` | Scroll back 3 lines in the terminal under the cursor; enters ScrollBack mode if in Normal or TerminalFocused |
| `ScrollDown` | Scroll forward 3 lines in the terminal under the cursor |

Mouse scroll also focuses the terminal being scrolled (changes `mgr.focused`).

**Clipboard:** `osc52_copy()` writes `\x1b]52;c;{base64}\x07` to stdout. This works in terminals that support the OSC 52 clipboard protocol (most modern terminals including WezTerm, iTerm2, Windows Terminal).

#### Keyboard Encoding (`widget::key_to_bytes()`)

`key_to_bytes(code, modifiers)` in `widget.rs` converts crossterm key events to the byte sequences expected by programs running inside the PTY. It uses `xterm_mod_param()` to compute the xterm modifier parameter (`1 + shift?1:0 + alt?2:0 + ctrl?4:0`; 0 when no modifiers).

| Key Category | Encoding |
|-------------|----------|
| Ctrl+letter | Control code 0x01-0x1a (Ctrl+A=0x01, etc.) |
| Alt+letter | ESC prefix (`\x1b` + char) |
| Enter | `\r` plain, `\x1b[13;{mod}u` with modifiers (CSI u) |
| Backspace | `\x7f` plain, `\x1b[127;{mod}u` with modifiers (CSI u) |
| Arrow keys | `\x1b[A` plain, `\x1b[1;{mod}A` with modifiers |
| Home/End | `\x1b[H`/`\x1b[F` plain, `\x1b[1;{mod}H`/`F` with modifiers |
| Insert/Delete/PgUp/PgDn | `\x1b[N~` plain, `\x1b[N;{mod}~` with modifiers (tilde encoding) |
| F1-F4 | `\x1bOP` (SS3) plain, `\x1b[1;{mod}P` (CSI) with modifiers |
| F5-F12 | `\x1b[N~` (tilde encoding), same modifier scheme |

The CSI u encoding for Enter/Backspace is important for programs like Claude Code that distinguish `Ctrl+Enter` from `Enter`.

Helper functions: `csi_final()`, `csi_tilde()`, `fkey_ss3()`.

#### PTY Cleanup

On app exit (`q`/`F10`), `TerminalManager::cleanup_all()` drops all terminals, which closes their writers and causes child processes to receive EOF.

### In-Memory Search Index
F3 search uses a pre-built `Vec<SearchEntry>` instead of scanning disk on every keystroke.

- **`SearchEntry`**: Holds `(repo_index, task_index)` plus pre-lowercased text segments (task_id, description, branch, phase, raw state.json, first 4KB of each .md artifact).
- **`build_search_index()` / `build_search_entry()`**: Module-level free functions (not `impl App` methods) so they can be called from both `App` methods and the background thread closure.
- **Index build**: Happens once on startup and again in each background refresh. Disk I/O (reading state.json + artifacts) only occurs during index build, never during search.
- **`run_search()`**: Iterates `self.search_index` in memory. First matching segment per task wins. Capped at 50 results.

### Worktree Creation (`F4` key)
Native Rust reimplementation of the core steps from `scripts/setup-worktree.py`. Press `F4` on a repo row to:
1. Enter task description (text input via `tui-input`)
2. Select AI host (Claude/Copilot/Gemini/OpenCode)
3. Toggle settings (pull latest, launch terminal)
4. Background thread runs git operations (~100ms vs ~2s for Python)
5. Shows result with task ID, branch, directory, color scheme
6. Optionally launches a color-themed terminal tab

**What it does:** Validates git repo, fetches/pulls, generates task ID (scans `.tasks/`), creates `state.json`, runs `git worktree add`, symlinks `.tasks/`, assigns color scheme, writes `.crew-resume` context file (see below).

**What it skips (deferred to AI agent):** Settings patching, dependency install, WSL path fix, post-setup commands, Jira transitions -- all handled by the agent on first `/crew resume`.

### `.crew-resume` Context File
At worktree creation, both `worktree.rs` and `scripts/setup-worktree.py` write a `.crew-resume` file to the worktree root. This file contains task_id, description, main_repo path, tasks_path, base_branch, ai_host, and the resume command. It is `.gitignore`d and never committed.

**Purpose:** AI hosts that cannot accept CLI prompt arguments (notably Copilot via `gh cs`) read this file to discover what task to resume. The launcher (`launcher.rs`) omits the prompt argument for Copilot and relies on `.crew-resume` instead. Claude, Gemini, and OpenCode still receive the resume prompt as a CLI argument.

The worktree is created at `../{repo-name}-worktrees/TASK_XXX` with branch `crew/{slugified-description}`.

### Worktree Cleanup (`F6` key)
Press `F6` on a repo row to clean up worktrees:
1. Multi-select worktrees with Space (completed tasks pre-selected, `a` toggles all)
2. Toggle settings: delete branch, keep on disk (recyclable)
3. Dry-run preview showing exact git commands and warnings (unmerged branches, incomplete workflows)
4. Background thread runs `scripts/cleanup-worktree.py` for each selected task
5. Shows results with success/failure per worktree

**Safety:** Only removes git worktree directories and branches. The `.tasks/` directory and all task artifacts are NEVER deleted.

**Modes:** Remove (git worktree remove + state update) or Recycle (mark recyclable, keep on disk for reuse).

### UI Style System
Central style helpers in `styles.rs` ensure visual consistency:
- `selected_style()` — blue background (#2A4A6B) for selected rows everywhere
- `focused_border_style()` / `unfocused_border_style()` — bold cyan vs dim gray borders
- `popup_selected_style()` — delegates to `selected_style()` (single source of truth)
- `hint_style()` — dim gray for keybinding hints

Pane focus is indicated by bold border + `◄` marker in the title. Detail pane titles show breadcrumb trails (e.g. `TASK_003 > Documents > Architect Analysis`).

Vertical scrollbars appear in both panes when content overflows the visible area. Uses ratatui's `Scrollbar` widget with `ScrollbarOrientation::VerticalRight`. The detail pane uses a shared `render_detail_scrollbar()` helper called from each rendering function.

### Status Bar (Norton Commander-style)
Two-line status bar:
- **Line 1**: View tabs + contextual navigation hints + aggregate stats
- **Line 2**: Context-adaptive F-key bar with modifier layers

#### F-Key Bar Layers

The bar supports three modifier layers, triggered by holding Shift or Ctrl:

**Base layer (no modifier):**
`F1Help  F2Launch  F3Search  F4New  F5Rfrsh  F6Clean  F7Attn  F8Perms  F9Focus  F10Quit`
(F9 Focus label only shows in Terminals view Normal mode; otherwise dimmed placeholder. F12 also enters focus.)

**Shift layer (view switching + detail):**
`SHIFT▶ S+F1Tasks  S+F2Issues  S+F3Config  S+F4Cost  S+F5Terms  S+F6Docs  S+F7Hist`

**Ctrl layer (reserved for expansion):**
`CTRL▶` (all F-key slots empty/reserved)

**Modifier detection:** At startup, crew-board enables the kitty keyboard protocol via `PushKeyboardEnhancementFlags` if supported. With kitty protocol, modifier-only key presses (Shift alone, Ctrl alone) are detected and the bar updates in real-time. On terminals without kitty support (e.g., Windows Terminal), pressing a Shift+F-key or Ctrl+F-key flashes the corresponding layer for 2 seconds, showing what else is available.

#### Context-Adaptive Terminal Bars

In Terminals view, the bar changes based on the input mode:

- **TerminalFocused**: `━━ INPUT → TERMINAL ━━  F12 exit  S+F1 Tasks  F2 Issues  F3 Cfg  F4 Cost  F5 Terms  S+PgUp/Dn crew`
- **ScrollBack**: `SCROLL▶  ↑↓/jk:Line  PgUp/Dn:Page  Home:Top  End:Live  q/Esc:Exit  offset:N`

The PrefixPending bar is no longer shown (the prefix system is legacy; F12 is now the sole focus toggle).

#### F-Key Empty Slots

Unbound F-key slots show a dimmed F-number with no label (e.g., F9 appears in dark gray when not contextually active). This preserves positional reference without noise.

#### Popup Override

When a popup is open, line 2 shows popup-specific hints instead of the F-key bar (unchanged from before).

The "5:Terms" tab in line 1 carries a status badge when terminals need attention:
- `5:Terms[⚠N]` — N terminals have `NeedsAttention` status (attention count > 0, takes priority)
- `5:Terms[✗N]` — N terminals have exited (exited count > 0, shown when no attention)
- `5:Terms` — all terminals running or none present

### Color Schemes
8 schemes from Python `CREW_COLOR_SCHEMES` (state_tools.py), indexed by `color_scheme_index` from worktree state. Used for tree row accents, detail pane colors, and terminal tab colors.

`launcher.rs` provides `ColorSchemeHex` with hex strings for terminal commands:
- Windows Terminal: `--tabColor` and `--colorScheme` args to `wt.exe`
- tmux: `set-option window-style bg=...,fg=...`

### Activity Feed (View 6)

View 6 (`6` key) shows a real-time stream of hook events from all embedded terminals. The view has two areas: a filter bar at the top and an event table below.

**Filter bar** shows active filters, auto-scroll state, and global stats (total tool calls, errors, active terminals). Key hints appear on the right.

**Event table** shows one row per hook event: relative timestamp, terminal ID (short form), event type with color coding, tool name, and detail/input summary. Color coding: cyan for `PreToolUse`, green for `PostToolUse`, blue for `SessionStart`, dark gray for `SessionEnd`, yellow/bold for `Notification`, red for `PermissionRequest`, magenta for `Stop`. Success/failure markers appear on `PostToolUse` rows.

**Key bindings (View 6):**

| Key | Action |
|-----|--------|
| `t` | Cycle terminal filter (All -> T1 -> T2 -> ... -> All) |
| `e` | Cycle event type filter (All -> PreToolUse -> PostToolUse -> ... -> All) |
| `f` | Cycle tool name filter (All -> Edit -> Bash -> ... -> All) |
| `a` | Toggle auto-scroll (on/off) |
| `g` | Toggle Gantt timeline view |
| `Up` / `Down` | Manual scroll (when auto-scroll is off) |

When auto-scroll is enabled (default), the table always shows the most recent events. When disabled, Up/Down keys scroll through the event history. A scrollbar appears when events exceed the visible area.

### Statistics Popup (Ctrl+F6)

Press `Ctrl+F6` to open a scrollable statistics overlay. Shows four sections:

**Global Statistics:** Total tool calls, total errors, active terminals, and activity log event count.

**Security:** Denied requests, warnings, auto-approved and human-approved decisions, credential exposures. Values are highlighted in red/yellow when non-zero.

**Orchestration** (shown only when orchestration is active): Current mode (Manual/Semi-Auto/Full-Auto), pending/running/completed/failed task counts, total cost, and circuit breaker status (OK or TRIPPED in red).

**Per-Terminal Breakdown:** For each terminal: tools used, errors, files touched, currently active tool, and the 4 most recently touched files.

| Key | Action |
|-----|--------|
| `Up` / `Down` | Scroll by line |
| `PgUp` / `PgDn` | Scroll by page |
| `Esc` | Close popup |

### Security Rules Engine

The security rules engine (`security.rs`) evaluates tool requests against configurable rules before the permission profile check. Enable via `security_enabled = true` in settings.

**`SecurityRuleConfig`** defines a rule with:
- `name` — human-readable rule identifier
- `tool_pattern` — regex matched against tool name (e.g. `"Bash"`)
- `input_pattern` — regex matched against tool input summary (e.g. `"(?i)push.*--force"`)
- `file_pattern` — regex matched against tool input summary for file paths
- `action` — `deny` (block immediately), `ask` (force human review), or `allow` (permit without prompting)
- `reason` — explanation shown when rule triggers

**Rule evaluation priority:** Deny > Ask > Allow. When multiple rules match, the highest-priority action wins. All specified patterns within a rule must match (AND logic). Rules with no patterns are skipped.

**Credential scanning:** `credential_patterns` is a list of regex patterns matched against tool output to detect leaked secrets (API keys, passwords). Detections increment `SecurityStats.credential_exposures`.

**Sensitive file protection:**
- `sensitive_files.never_access` — file patterns that are automatically denied (e.g. `.env`, `*.pem`)
- `sensitive_files.warn_on_access` — file patterns that trigger a warning (e.g. `Cargo.lock`)

**Rate limiting:** `rate_limit_per_minute` sets a per-terminal sliding-window rate limit. When exceeded, tool requests are blocked until the window clears. Set to 0 (default) for unlimited.

**Status bar badge:** When security is enabled and rules have triggered, the status bar shows `D{n} W{n}` indicating denied and warned counts.

### Auto-Orchestration

The orchestration engine (`orchestration.rs`) manages automated task scheduling and execution. It is opt-in and defaults to Manual mode.

**`OrchestrationMode`:**
- `Manual` — user controls everything; the engine only suggests actions
- `SemiAuto` — suggests actions and requires confirmation before execution
- `FullAuto` — executes actions automatically within guardrails

**`OrchestratedTask` lifecycle:** `Pending` (waiting for dependencies) -> `Running` (terminal launched) -> `Completed` or `Failed { error }`. Each task tracks its `depends_on` list, terminal_id, retry count, and timing.

**`CircuitBreaker`:** Tracks recent failures in a sliding window. Default: 3 failures within 10 minutes triggers the breaker, which automatically downgrades `FullAuto` to `SemiAuto`. Call `reset()` to re-enable after investigation.

**Guardrails:**
- `cost_ceiling` — maximum total cost before pausing all orchestration (default: $50)
- `max_concurrent` — maximum terminals running simultaneously (default: 5)
- `max_retries` — maximum retry attempts per failed task (default: 5)

**Tick cycle:** `OrchestrationState::tick()` runs each event-loop iteration. It checks cost ceiling, finds tasks with all dependencies completed, finds failed tasks eligible for retry, respects concurrent limits, and populates the action queue. Actions are `LaunchTask`, `RetryTask`, `DowngradeMode`, or `CostLimitReached`.

**Configuration:** Set `orchestration_mode` in settings to `"manual"`, `"semi-auto"`, or `"full-auto"`. Guardrail values are configured via `max_concurrent`, `cost_limit`, and `max_retries`.

### Cross-Platform Hook Bridge

The hook bridge (`hook_bridge.rs`) enables non-Claude AI hosts to communicate with the crew-board hook server.

**`AiHostType` enum:** `Claude`, `Gemini`, `Copilot`, `OpenCode`, `Shell`. Each host has its own event naming conventions.

**Event normalization:** `AiHostType::normalize_event_name()` maps host-specific event names to internal names:
- Gemini: `BeforeTool`/`before_tool` -> `PreToolUse`, `AfterTool`/`after_tool` -> `PostToolUse`
- Copilot: `tool.execute.before` -> `PreToolUse`, `tool.execute.after` -> `PostToolUse`
- OpenCode: `pre_tool`/`PreTool` -> `PreToolUse`, `post_tool`/`PostTool` -> `PostToolUse`

**Bridge script generation:** `generate_bridge_script()` creates a bash script that reads JSON from stdin, POSTs it to the crew-board HTTP server with auth, and echoes the response. Used by non-Claude hosts that support shell-based hooks.

**Hook config generation:** `generate_hook_config()` produces host-specific configuration files:
- Gemini: `.gemini/crew-hook.sh` + `.gemini/settings.json`
- Copilot: `.github/hooks/crew-hook.sh` + `.github/hooks/hooks.json`
- OpenCode: `.opencode/plugins/crew-hook.sh` + `.opencode/plugins/crew-board.ts`
- Claude: returns `None` (handled via `settings.local.json` elsewhere)
- Shell: returns `None` (no hook system)

**Response formatting:** `format_response()` produces host-appropriate JSON for allow/deny decisions. Claude uses `hookSpecificOutput.permissionDecision`; others use `{"action": "allow/deny"}`.

## Settings Keys (`~/.config/crew-board.toml`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `repos` | `string[]` | `[]` | Explicit repo paths |
| `scan` | `string[]` | `[]` | Directories to scan for repos |
| `poll_interval` | `u64` | `3` | Auto-refresh interval (seconds) |
| `embed_terminals` | `bool` | `true` | Enable embedded terminal feature |
| `prefix_key` | `string` | `"C-b"` | Prefix key for terminal commands (tmux syntax) |
| `terminal_layout` | `string` | `"focused"` | Default layout: focused/tiled-2/tiled-4/stacked |
| `scrollback_lines` | `u32` | `10000` | Scrollback buffer per terminal |
| `auto_embed_on_create` | `bool` | `true` | Embed terminal on F4 worktree creation |
| `idle_timeout_secs` | `u64` | `120` | Idle detection threshold |
| `visual_bell` | `bool` | `true` | Flash status indicator on attention |
| `system_bell` | `bool` | `false` | Terminal bell on attention |
| `log_directory` | `string?` | `null` | Terminal output log dir (each terminal logs to `<dir>/<id>.log`) |
| `permission_profile` | `string` | `"interactive"` | Permission profile: interactive/trusted/autonomous |
| `auto_approve_patterns` | `string[]` | `[]` | Regex patterns for auto-approval in trusted profile |
| `desktop_notifications` | `bool` | `false` | Send desktop notification on attention events |
| `hook_communication` | `bool` | `true` | Enable HTTP hook server for structured Claude Code activity tracking |
| `security_enabled` | `bool` | `false` | Enable security rules enforcement |
| `security_rules` | `SecurityRuleConfig[]` | `[]` | Security rules for tool governance |
| `credential_patterns` | `string[]` | `[]` | Regex patterns for credential detection |
| `rate_limit_per_minute` | `u32` | `0` | Rate limit per terminal (0=unlimited) |
| `sensitive_files` | `SensitiveFiles` | `{}` | Files to protect (never_access, warn_on_access) |
| `capture_tool_events` | `bool` | `true` | Capture tool events in history.jsonl |
| `capture_prompts` | `bool` | `false` | Capture user prompts in history.jsonl |
| `capture_permissions` | `bool` | `true` | Capture permission decisions in history.jsonl |
| `orchestration_mode` | `string` | `""` | Orchestration: manual/semi-auto/full-auto |
| `max_concurrent` | `u32` | `5` | Max concurrent orchestrated terminals |
| `cost_limit` | `f64` | `50.0` | Cost ceiling for orchestration |
| `max_retries` | `u32` | `5` | Max retries per orchestrated task |

All new keys use `#[serde(default)]` with custom default functions, so existing configs are fully backward-compatible.

## Data Sources

| Source | Path | Format |
|--------|------|--------|
| Task state | `.tasks/TASK_XXX/state.json` | JSON |
| Task metadata | `.tasks/TASK_XXX/metadata.json` | JSON (fallback when state.json missing) |
| Task artifacts | `.tasks/TASK_XXX/*.md` | Markdown |
| Beads issues | `.beads/issues.jsonl` | JSONL (one JSON per line) |
| Config (global) | `~/.claude/workflow-config.yaml` | YAML |
| Config (project) | `config/workflow-config.yaml` | YAML |
| User settings | `~/.config/crew-board.toml` | TOML |
| Activity log | (in-memory) | Ring buffer of 500 ActivityEvents |
| File claims | (in-memory) | HashMap with 10-min expiry per file |
| Tool spans | (in-memory) | Completed PreToolUse->PostToolUse spans |

## Adding a New Popup

Follow the pattern established by `launch_popup.rs` and `create_popup.rs`:
1. Create `src/ui/new_popup.rs` with `pub fn draw(frame, app)` using `centered_rect()` for positioning
2. Add popup state struct + step enum to `app.rs`
3. Add `Option<NewPopup>` field to `App` struct
4. Add lifecycle methods: `open_*`, `*_handle_key`, `close_*`
5. Register in `src/ui/mod.rs` — add `pub mod` and draw overlay call
6. Add key routing priority in `main.rs` event loop (popups go first)
7. Add keybinding hint in `status_bar.rs`

## Adding a New View

1. Create `src/ui/new_view.rs` with `pub fn draw(frame, app, area)`
2. Register in `src/ui/mod.rs` — add `pub mod` and dispatch in `draw()`
3. Add variant to `ActiveView` enum in `app.rs`
4. Add number key binding in `main.rs`
5. Add tab label in `status_bar.rs`

## Adding a New Data Source

1. Create parser in `src/data/new_source.rs`
2. Add field to `RepoData` in `data/mod.rs`
3. Load in `RepoData::load()` **and** `RepoData::load_incremental()`
4. Use `#[serde(default)]` on all fields for resilience

## Common Gotchas

- **Worktree paths**: The `worktree.path` field in state.json is relative. Use `launch.worktree_abs_path` for absolute paths.
- **WSL paths**: `wt.exe` runs on Windows side but receives Linux paths via `wsl.exe --cd`. Always include explicit `cd` in bash commands.
- **Login shells**: `bash -lic` sources profile which may reset cwd. Always prefix commands with `cd <dir> &&`.
- **Tree rebuild**: Forgetting `rebuild_tree()` after changing `expanded_repos` causes stale cursor state.
- **Detail mode reset**: Must reset `detail_mode` to `Overview` when tree cursor changes.
- **Popup priority**: Help overlay > search popup > create popup > cleanup popup > launch popup > detail nav > default. All must be checked before default key handling.
- **Background threads**: `create_worktree()`, `cleanup_worktree()`, and `refresh()` all run on `std::thread::spawn`. Poll `JoinHandle::is_finished()` each tick (250ms). No async runtime needed.
- **`F4`/`F6` key scope**: Only works on Repo rows — `open_create_popup()` and `open_cleanup_popup()` return early on Task rows.
- **Search index free functions**: `build_search_index()` and `build_search_entry()` are module-level free functions, not `impl App` methods. This is required because they are called from the background thread closure (which cannot capture `&self`).
- **`LoadedTask` construction sites**: When adding fields to `LoadedTask`, update all construction sites: 3 in `load_tasks()` (metadata fallback, normal state.json, gap-fill), plus the corresponding sites in `load_tasks_incremental()`. The `state_mtime` field must be `Some(mtime)` for normal tasks and `None` for archived/fallback tasks.
- **Terminal `d` key conflict**: The `d` key normally opens the doc list (`enter_doc_list()`). In the Terminals view it dismisses an exited terminal instead. The event loop checks `app.active_view == ActiveView::Terminals` before deciding which action to take.
- **Terminal `Enter` key conflict**: In the Terminals view `Enter` only relaunches exited terminals (not running ones). `F9`/`F12` is the focus toggle. Outside the Terminals view, `Enter` expands/collapses tree rows.
- **`poll_terminals()` auto-exits focus mode**: If the user is in `TerminalFocused` mode and the terminal exits, `poll_terminals()` automatically resets `terminal_input_mode` to `Normal`. No manual key press is needed.
- **Attention cleared on input**: `TerminalManager::send_input()` always sets `attention_signal` to `None`. This means typing in a terminal immediately clears any attention badge, even if the screen still shows the prompt.
- **Idle threshold in `poll_status()`**: Idle detection (120s) is handled in `poll_status()`, not in the reader thread. The reader thread only sets `attention_signal` for screen-content patterns (prompts/errors). Idle is a time-based check against `last_output` done each tick.
- **Reader thread lifetime**: The reader thread for each PTY runs until the child process exits (EOF on the master reader). It is not killed explicitly — it exits naturally. After EOF it waits for the child exit code via `child.wait()` before writing the exit signal.
- **Scroll-back via `set_scrollback()`**: The vt100 crate's `Parser::set_scrollback(n)` controls how many scrollback lines appear in the visible view. `draw_terminal()` temporarily sets it during rendering and restores it after. The parser lock is held for the entire render — the reader thread will block during this brief period.
- **`PrefixPending` timeout (legacy)**: The prefix-pending state is no longer reachable in the current code. This gotcha is retained for reference only.
- **Layout mode in tiled views**: `terminal_indices_for_layout()` selects which terminals to show. It always includes the focused terminal and fills remaining slots with neighbors. The focused terminal's border uses `focused_border_style()` to distinguish it from inactive tiles.
- **Permission popup refreshes entries**: After approve/deny, the popup rebuilds its entries list from scratch. The cursor clamps if the list shrinks.
- **`last_terminal_size` for relaunch**: `terminal_resize_all()` stores the computed PTY dimensions in `App.last_terminal_size`. `terminal_relaunch_focused()` uses these instead of hardcoded 24x80.
- **Kitty protocol runtime detection**: `crossterm::terminal::supports_keyboard_enhancement()` queries the terminal at startup. If supported, `PushKeyboardEnhancementFlags` is executed with `DISAMBIGUATE_ESCAPE_CODES | REPORT_EVENT_TYPES | REPORT_ALL_KEYS_AS_ESCAPE_CODES`. Must be popped with `PopKeyboardEnhancementFlags` on exit.
- **Modifier bar flash fallback**: On terminals without kitty protocol (e.g., Windows Terminal), pressing Shift+F1 both executes the action AND sets `modifier_bar_flash_until` to `Instant::now() + 2s`. The flash timeout is ticked each loop iteration via `tick_modifier_bar()`. The poll timeout drops to 50ms during flash for responsive decay.
- **Modifier+F-key priority**: Shift+F-key and Ctrl+F-key bindings are checked BEFORE the base `match` in Priority 12. This prevents `Shift+F(1)` from falling through to the plain `F(1)` handler.
- **Shift+F-keys DO bypass TerminalFocused mode**: Unlike regular keys, Shift+F1-F5 in TerminalFocused mode exit focus and switch views. Shift+PgUp/PgDn cycle terminals while staying focused. Other Shift+key combinations are forwarded to the PTY. `F12` is the sole single-key exit from focused mode.
- **F9/F12 Focus is contextual**: F9 and F12 both enter TerminalFocused mode, but only when in Terminals view with at least one terminal. F9 shows a label ("Focus") in the base F-key bar only in Terminals view Normal mode; otherwise it renders as a dimmed empty placeholder.
- **Mouse capture scope**: Mouse capture is always enabled (`EnableMouseCapture`). Click/drag/release are handled only when `active_view == Terminals`. Scroll events also only apply in Terminals view. Clicking outside any terminal panel clears the selection.
- **`terminal_panel_rects` is RefCell**: Because panel rects are written during `draw()` (immutable `&App`) and read during mouse handling (mutable `&mut App`), the rects use `RefCell<Vec<(usize, Rect)>>`. This is the only `RefCell` in `App`.
- **OSC 52 clipboard**: Text is copied via OSC 52 (`\x1b]52;c;{base64}\x07`). This requires terminal support (WezTerm, iTerm2, modern Windows Terminal). The `base64` crate is a dependency for this feature.
- **`key_to_bytes()` CSI u encoding**: Modified Enter and Backspace use CSI u encoding (`\x1b[13;{mod}u`, `\x1b[127;{mod}u`) so that programs like Claude Code can distinguish e.g. Ctrl+Enter from plain Enter.
- **PrefixPending is legacy**: The `PrefixPending` variant of `TerminalInputMode` is kept for enum compatibility but no code path enters this state. The `Ctrl+B` prefix system was replaced by `F12` toggle + `Shift+F-key` view switching.
- **Permission profile auto-approval**: In `Autonomous` profile, `auto_approve_permissions()` sends `y\n` to every terminal with `PermissionPrompt` attention on each poll tick. In `Trusted` profile, only prompts matching `auto_approve_patterns` are approved. In `Interactive` (default), no auto-approval occurs.
- **Terminal output logging**: When `log_directory` is set, `spawn_pty_with_log()` opens an append-mode log file. The reader thread tees all raw PTY output bytes to the file. Log files include ANSI escape codes (use `less -R` or `cat` to view).
- **Desktop notifications**: `send_desktop_notification()` spawns a detached thread to run `notify-send` (Linux) or `osascript` (macOS). Failures are silently ignored.
- **Terminal search in scroll-back**: `/` enters search mode, query is matched case-insensitively against visible screen rows. Matches are stored as scroll offsets; `n`/`N` navigate between them. Search state (`terminal_search_query`, `terminal_search_matches`) persists until scroll-back is exited.
- **Crew summary line**: In Focused layout with 2+ terminals, a one-line summary appears above the terminal panel showing all terminals' status icons and short IDs. Uses `●` (running), `◆` (attention), `✓`/`✗` (exited ok/failed).
- **Phase+progress in terminal title**: `lookup_task_phase()` scans `app.repos` for a task matching the terminal's ID and appends `[phase]` or `[phase N%]` to the border title of running terminals.
- **`D` key vs `d` key**: `D` (shift+d) dismisses ALL exited terminals at once via `dismiss_all_exited()`. Lowercase `d` dismisses only the focused exited terminal.
- **Hook server port is dynamic**: `HookServer::start()` binds to `127.0.0.1:0`, so the OS assigns the port. The port is stored in `App.hook_server.port` and written into each terminal's `settings.local.json` and env vars at spawn time. There is no fixed port to configure.
- **`settings.local.json` cleanup is best-effort**: On terminal dismiss or app exit, crew-board removes the file at `{worktree}/.claude/settings.local.json`. If crew-board crashes, the file is left behind and must be removed manually. It does not persist any user settings — it is solely crew-board's hook injection file.
- **Hook events only flow for Claude Code terminals**: `generate_hook_config()` checks the `command` value and only writes hook config when the command is `claude`. Non-Claude terminals (Copilot, Gemini, OpenCode) get no hook injection and `hook_state` remains `None`.
- **`HookState` is initialized lazily**: The `hook_state: Option<HookState>` field on `EmbeddedTerminal` starts as `None` and is created on first `process_hook_event()` call. Do not assume it is `Some` until at least one event has arrived.
- **`HookNotification` vs PTY-scanned attention**: `AttentionReason::HookNotification` is set directly by `process_hook_event()` on the `EmbeddedTerminal.status` field, bypassing `poll_status()` and the reader-thread attention signal. This means it is not cleared by sending input to the terminal — only a subsequent event or explicit status reset clears it.
- **Token registry is `Arc<RwLock<>>`**: `HookServer.tokens` is shared between the server background thread (reads per-request) and the main thread (writes on register/deregister). Take the write lock only briefly. Do not call `register_token` or `deregister_token` while already holding any other lock that the server thread might need.
- **Hook permission blocking**: For `PreToolUse` and `PermissionRequest` hooks, the server thread blocks on `recv_timeout(25s)` waiting for user decision from the F8 popup. If the main thread is unresponsive, the 25s timeout returns a safe fallback (`"ask"` for PreToolUse, `"allow"` for PermissionRequest). Never hold locks across the blocking receive.
- **PendingPermission lifetime**: `PendingPermission` entries in `App.pending_permissions` hold a `response_tx` channel sender. When the popup sends a decision, the entry must be removed (via `Vec::remove()`) — the server thread will unblock. If the app exits without responding, channels drop and the server thread's `recv_timeout` returns `Err`, triggering the fallback.
- **F8 popup dual entries**: The permission popup shows both `PtyBased` (screen-scanned prompts) and `HookBased` (hook pending permissions). Quick-send (`t`) is only available for PTY entries since hook entries require structured `allow`/`deny` decisions.
- **Auto-approve skips hook terminals**: `auto_approve_permissions()` (PTY-based `y\n` sending) skips terminals with `hook_state.is_some()`. For hook-based terminals, auto-approval happens in the server thread itself via `check_permission_profile()`, so no PTY bytes are needed.
- **`RulesEngine` is on main thread**: `rules_engine` is a field on `App`, not shared with the server thread. Security evaluation in the hook server path must be done via the permission profile, not direct rule evaluation.
- **`ActivityLog` ring buffer**: Max 500 events. Oldest events are evicted. `completed_spans` is separately capped at 1000.
- **`FileClaimsRegistry` 10-min expiry**: Claims expire after 10 minutes. Call `gc()` periodically (not yet automated).
- **Stats popup priority**: The stats popup (Ctrl+F6) has priority 0.5 in the key routing -- between help overlay (0) and scroll-back mode (0.6).
- **Orchestration is opt-in**: Default mode is Manual. No automatic actions without explicit `orchestration_mode` config.
