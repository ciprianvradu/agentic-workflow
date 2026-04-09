# crew-board

A terminal cockpit for supervising parallel AI agent crews. Built with Rust, [ratatui](https://ratatui.rs), and [crossterm](https://github.com/crossterm-rs/crossterm).

Monitor tasks, embed live terminals, detect permission prompts, and manage 10+ parallel AI crews — all from a single TUI window.

```
┌ Crew List ───────┐┌ #042 (Claude Code) 3m ◄ ────────────────────────────────────────────────┐
│● #042  impl  3m  ││                                                                          │
│◉ #043  impl  5m !││ ⠿ Implementing step 3: Add auth middleware                               │
│● #044  arch  1m  ││                                                                          │
│○ #048  ---   --  ││ Reading src/middleware/auth.rs...                                         │
│✓ #046  done  12m ││ Creating src/middleware/jwt.rs...                                         │
│✗ #050  ERR   8m  ││                                                                          │
│                  ││ Allow tool: Edit src/middleware/auth.rs?                                   │
│                  ││   + import { JwtService } from './jwt';                                   │
│                  ││   + const auth = new JwtService(config);                                  │
│                  ││                                                                          │
│                  ││ [Y/n]                                                                     │
│                  ││                                                                          │
└──────────────────┘└──────────────────────────────────────────────────────────────────────────┘
Tasks  Issues  Config  Cost  [Terms]  Activity    ↑↓ crew  ! 1 attn  5 repos 12 tasks (3s)
 F1Help F2Launch F3Search F4Layot F5Rfrsh F6Dsmis F7Attn  F8Perms F9Focus           F10Quit
```

## Features

**Embedded Terminal Multiplexer** — Run AI agents (Claude Code, Gemini CLI, GitHub Copilot, OpenCode, Devin, Droid) inside crew-board. Each crew gets a full PTY terminal with ANSI color, cursor positioning, and alternate screen support. Windows ConPTY is supported for native Windows builds.

**Permission Prompt Detection** — Automatically detects when an AI agent is waiting for approval. Yellow attention badges appear in the crew list, and F7 jumps straight to the next blocked crew.

**Permission Queue (F8)** — Centralized popup showing all pending permissions across all crews. Context preview shows the actual prompt text. Approve, deny, batch-approve (A), or type a custom response (t).

**Permission Profiles** — Configure `permission_profile` in settings: `interactive` (manual approval), `trusted` (auto-approve matching regex patterns), or `autonomous` (auto-approve everything).

**Terminal Output Logging** — Set `log_directory` in settings to capture all terminal output to per-task log files for post-mortem analysis.

**Desktop Notifications** — Enable `desktop_notifications` to receive OS notifications when any terminal needs attention (via `notify-send` on Linux, `osascript` on macOS).

**Terminal Search** — Press `/` in scroll-back mode to search terminal output. Navigate matches with `n`/`N`.

**Crew Summary Line** — In focused layout with multiple terminals, a compact status line shows all crew members' status at a glance.

**Phase & Progress in Title** — Terminal borders show the current workflow phase and implementation progress (e.g., `[implementer 60%]`).

**Mouse Support** — Click and drag to select text within terminal panels (constrained to panel boundaries). Scroll wheel for scrollback. Auto-copy to clipboard via OSC 52.

**Four Layout Modes** — Focused (one large terminal), Tiled-2 (side-by-side), Tiled-4 (2x2 grid), Stacked (vertical). Cycle with `F4` in Terminals view.

**Bracketed Paste** — Pasting multi-line text into embedded terminals works correctly. Paste content is wrapped in bracketed paste sequences so receiving applications (Claude Code, bash, etc.) treat it as a single paste rather than executing each line.

**Full Modifier Encoding** — Ctrl+Arrow (word jump), Ctrl+Enter (newline), Shift+Up/Down, and all other modifier+key combinations work correctly inside embedded terminals.

**Six Dashboard Views** — Tasks, Issues, Config, Cost, Terminals, and Activity Feed. Switch instantly with Shift+F1-F6 (even while focused in a terminal or in the Statistics popup).

**Configurable Pane Widths** — Resize panes on the fly with Ctrl+Left/Right. Persist defaults with `pane_width_tasks`, `pane_width_issues`, and `pane_width_terminals` in settings.

**Hook-Based Communication** — HTTP hook server receives structured events from Claude Code hooks. Real-time visibility into tool usage, permission requests, and session lifecycle. Session cost is automatically captured from `Stop` hook events and displayed in the Cost view.

**Activity Feed (View 6)** — Real-time event stream from all terminals with filtering by terminal, event type, or tool name. Includes a Gantt timeline view showing tool execution spans across terminals.

**Statistics Popup (Ctrl+F6)** — Global stats, security metrics, orchestration status, and per-terminal breakdown.

**Security Rules Engine** — Configurable regex rules for tool governance (deny/ask/allow), credential scanning, sensitive file protection, and rate limiting.

**Auto-Orchestration** — Task scheduling with dependency resolution, circuit breaker, cost ceiling, and concurrent terminal limits. Modes: Manual, SemiAuto, FullAuto.

**Cross-Platform Hook Bridge** — Bridge scripts for Gemini CLI, GitHub Copilot, OpenCode, Devin, and Droid with event name normalization.

**Per-Terminal Auto-Accept (Ctrl+F7)** — Toggle auto-accept on individual terminals, overriding the global permission profile. Shows a lightning bolt icon in the crew list.

**Splash Screen** — Displays a branded splash screen on startup while repos are loading.

**Task Filtering** — Filter the task list by status, phase, or other criteria.

**Worktree Management** — Create (F4) and cleanup (F6) git worktrees with color-themed terminal tabs. Each crew gets its own isolated workspace.

**Full-Text Search (F3)** — Search across all tasks, documents, and artifacts with a pre-built in-memory index.

## Install

```bash
cd crew-board
cargo build --release
cp target/release/crew-board ~/.local/bin/   # or anywhere on PATH
```

Requires Rust 1.70+.

## Quick Start

```bash
# Scan a parent directory for repos with .tasks/ or .beads/
crew-board --scan /path/to/projects

# Monitor specific repos
crew-board --repo /path/to/repo1 --repo /path/to/repo2

# Both (CLI args override config file)
crew-board --scan /path/to/projects --repo /extra/repo
```

On first run with `--scan`, the path is saved to `~/.config/crew-board.toml` so you can just run `crew-board` next time.

## Views

Switch views with Shift+F1-F6:

| Key | View | Shows |
|-----|------|-------|
| `Shift+F1` | Tasks | Tree of repos and tasks with detail pane |
| `Shift+F2` | Issues | Beads issue tracker (`.beads/issues.jsonl`) |
| `Shift+F3` | Config | Configuration cascade (global/project/task) |
| `Shift+F4` | Cost | Cost estimates and actuals from workflow state |
| `Shift+F5` | Terminals | Embedded terminal multiplexer with crew list |
| `Shift+F6` | Activity | Real-time hook event feed with Gantt timeline |

## Key Bindings

### F-Key Bar

The bottom bar shows context-sensitive F-key actions. Holding Shift reveals a second layer for view switching:

**Global keys (same in every view):**

| Key | Action |
|-----|--------|
| `F1` | Scrollable help overlay (all keybindings). Navigate with Up/Down/PgUp/PgDn, close with Esc. |
| `F2` | Launch terminal with AI host |
| `F3` | Search across tasks & documents |
| `F5` | Force refresh |
| `F10` | Quit |

**Per-view keys (change with active view):**

| View | F4 | F6 | F7 | F8 | F9 |
|------|----|----|----|----|-----|
| Tasks | New worktree | Documents | History | Permissions | — |
| Tasks (DocList) | — | Open | Back | Permissions | — |
| Terminals | Layout cycle | Dismiss | Attention | Permissions | Focus |
| Others | — | — | — | Permissions | — |

**Shift+F layer (always view switching):**

| Key | Action |
|-----|--------|
| `Shift+F1` | Switch to Tasks view |
| `Shift+F2` | Switch to Issues view |
| `Shift+F3` | Switch to Config view |
| `Shift+F4` | Switch to Cost view |
| `Shift+F5` | Switch to Terminals view |
| `Shift+F6` | Switch to Activity Feed |

**Ctrl+F layer (view-specific extras):**

| View | Ctrl+F4 | Ctrl+F5 | Ctrl+F6 | Ctrl+F7 | Ctrl+F8 |
|------|---------|---------|---------|---------|---------|
| Terminals | Dismiss All | Live view | Statistics | Auto Accept | ScrollBack |
| Activity | Crew filter | Event filter | Tool filter | Auto-scroll | Gantt |

**Adaptive labels:** On wide terminals (>=130 cols), the F-key bar shows descriptive labels (e.g. "F6 Documents"). On narrow terminals, compact labels are used (e.g. "F6Docs").

### Navigation

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Move up/down |
| `Enter`/`Space` | Expand/collapse repo |
| `Tab` | Switch pane focus (most views) / Enter terminal focused mode (Terminals view) |
| `PgUp`/`PgDn` | Scroll detail pane |
| `Home`/`End` | Jump to top/bottom of detail pane |
| `Ctrl+Left` | Narrow left pane (Tasks/Issues: −5%, Terminals: −2 chars) |
| `Ctrl+Right` | Widen left pane (Tasks/Issues: +5%, Terminals: +2 chars) |
| `Shift+F1`-`F6` | Switch views |
| `` ` `` | Cycle views |
| `q` / `Ctrl+C` | Quit |

**Note:** `Esc` never quits — it only closes popups, backs out of detail views, or switches pane focus.

### Task Detail

| Key | Action |
|-----|--------|
| `F6` | Browse task documents (architect.md, developer.md, etc.) |
| `F7` | View task history (decisions, phases, concerns) |
| `Esc` | Back (reader → list → overview) |

### Embedded Terminals (View 5)

View 5 is a full terminal multiplexer with three input modes:

**Normal Mode** — Navigate the crew list:

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Focus previous/next terminal |
| `Tab` / `F9` / `F12` | Enter focused mode (all keys go to PTY) |
| `Enter` | Relaunch exited terminal |
| `F6` / `Delete` | Dismiss exited terminal |
| `Ctrl+F4` | Dismiss ALL exited terminals |
| `F4` | Cycle layout (focused → tiled-2 → tiled-4 → stacked) |
| `Ctrl+Left` / `Ctrl+Right` | Narrow / widen the crew list |
| `Ctrl+F8` | Enter scroll-back mode |

**Focused Mode** — All keystrokes go to the active terminal:

| Key | Action |
|-----|--------|
| `F5` | Focus previous running terminal (skips exited) |
| `F6` | Focus next running terminal (skips exited) |
| `F7` | Jump to next terminal needing attention |
| `F12` | Exit focus (back to Normal) |
| `Shift+F1`-`F6` | Switch view (works even while focused) |
| All other keys | Sent to PTY with full modifier encoding |

**Scroll-Back Mode** — Browse terminal history:

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Scroll line by line |
| `PgUp`/`PgDn` | Scroll by page |
| `Home` | Jump to top of scroll-back buffer |
| `End` | Jump to bottom (stay in scroll-back mode) |
| `/` | Search terminal output |
| `n` / `N` | Next / previous search match |
| `Shift+F1`-`F6` | Exit scroll-back and switch to view |
| `q` / `Esc` | Exit scroll-back |

### Mouse (Terminal Panels)

| Action | Effect |
|--------|--------|
| Click + drag | Select text (constrained to panel boundaries) |
| Release | Auto-copy selection to clipboard (OSC 52) |
| Scroll wheel | Scroll-back (3 lines per tick) |

### Permission Queue (F8)

| Key | Action |
|-----|--------|
| `↑`/`↓` | Navigate pending permissions |
| `a` | Approve selected (sends `y` to the terminal) |
| `d` | Deny selected (sends `n` to the terminal) |
| `A` | Approve ALL pending terminals at once |
| `t` | Type custom response (opens input field) |
| `v` / `Enter` | View — jump to the terminal for full context |
| `Esc` | Close |

### Activity Feed (View 6)

| Key | Action |
|-----|--------|
| `Ctrl+F4` | Cycle terminal filter |
| `Ctrl+F5` | Cycle event type filter |
| `Ctrl+F6` | Cycle tool name filter |
| `Ctrl+F7` | Toggle auto-scroll |
| `Ctrl+F8` | Toggle Gantt timeline view |
| `↑`/`↓` | Manual scroll (when auto-scroll off) |

### Statistics Popup (Ctrl+F6)

| Key | Action |
|-----|--------|
| `↑`/`↓` | Scroll by line |
| `PgUp`/`PgDn` | Scroll by page |
| `Shift+F1`-`F6` | Close popup and switch to the corresponding view |
| `Esc` | Close |

### Worktree Creation (F4)

Opens a multi-step wizard (on repo rows only):
1. **Task description** — free text input
2. **AI host** — Claude Code, GitHub Copilot, Gemini CLI, OpenCode, Devin, or Droid
3. **Settings** — toggle pull latest / launch terminal after creation
4. **Execution** — background thread runs git operations with spinner
5. **Result** — shows task ID, branch, directory, color scheme

Creates a worktree at `../{repo}-worktrees/TASK_XXX` with branch `crew/{slugified-description}`. Optionally launches into an embedded terminal.

### Worktree Cleanup (F6)

Opens a multi-select cleanup dialog (on repo rows only):
1. Multi-select worktrees with Space (completed tasks pre-selected, `a` toggles all)
2. Toggle settings: delete branch, keep on disk (recyclable)
3. Dry-run preview showing exact git commands and warnings
4. Background execution with per-worktree success/failure results

## Configuration

`~/.config/crew-board.toml`:

```toml
# Directories to scan for repos containing .tasks/ or .beads/
scan = ["/path/to/projects"]

# Explicit repo paths (always included)
repos = ["/path/to/specific/repo"]

# Auto-refresh interval in seconds (default: 3)
poll_interval = 5

# ── Embedded Terminals ──────────────────────────

# Master toggle (default: true)
embed_terminals = true

# Default layout: "focused", "tiled-2", "tiled-4", "stacked"
terminal_layout = "focused"

# Scrollback buffer size per terminal (default: 10000)
scrollback_lines = 10000

# Left pane width % for Tasks view, 10-90 (default: 40). Runtime: Ctrl+Left/Right
# pane_width_tasks = 40

# Left pane width % for Issues view, 10-90 (default: 40). Runtime: Ctrl+Left/Right
# pane_width_issues = 40

# Crew list width in chars for Terminals view, 10-50 (default: 20). Runtime: Ctrl+Left/Right
# pane_width_terminals = 20

# Auto-launch embedded terminal on F4 worktree creation (default: true)
auto_embed_on_create = true

# ── Attention & Notifications ───────────────────

# Idle detection: mark terminal as idle after N seconds of no output (default: 120)
idle_timeout_secs = 120

# Flash status indicator when a crew needs attention (default: true)
visual_bell = true

# Trigger terminal bell on attention (default: false)
system_bell = false

# Log terminal output to files (default: disabled)
# log_directory = "/tmp/crew-board-logs"

# ── Permission Profiles ─────────────────────────

# Permission profile: "interactive" (default), "trusted", or "autonomous"
permission_profile = "interactive"

# Regex patterns for auto-approval in trusted profile
# auto_approve_patterns = ["(?i)read file", "(?i)list directory"]

# Default auto-accept state for new terminals (default: false)
# auto_accept_default = false

# Send desktop notification on attention events (default: false)
desktop_notifications = false

# ── Hook Communication ─────────────────────────

# Enable HTTP hook server for structured AI activity tracking (default: true)
hook_communication = true

# ── Security ───────────────────────────────────

# Enable security rules enforcement (default: false)
security_enabled = false

# Rate limit per terminal (0 = unlimited, default: 0)
# rate_limit_per_minute = 60

# ── Orchestration ──────────────────────────────

# Orchestration mode: "manual" (default), "semi-auto", "full-auto"
# orchestration_mode = "manual"

# Max concurrent terminals (default: 5)
# max_concurrent = 5

# Cost ceiling in dollars (default: 50.0)
# cost_limit = 50.0
```

CLI flags override config values when both are present.

## Terminal Status Indicators

Each crew in the terminal list shows its current state:

| Icon | Color | Meaning |
|------|-------|---------|
| `●` | Green | Running normally |
| `●` | Yellow | Needs attention (permission prompt, idle, error) |
| `●` | Gray | Exited (code 0) |
| `●` | Red | Exited (non-zero / failed) |

The `5:Terms` tab in the status bar shows attention badges:
- `5:Terms[!N]` — N terminals need attention (priority)
- `5:Terms[xN]` — N terminals have exited

## What It Monitors

### Task State (`.tasks/*/state.json`)
- Current phase and completed phases
- Workflow mode (full/fast/turbo/minimal)
- Implementation progress with step tracking
- Worktree info (branch, color scheme, status)
- Review issues, concerns, human decisions
- Cost estimates

### Task Documents (`.tasks/*/*.md`)
- Architect analysis, developer plan, reviewer feedback
- Browsable with preview and full reading mode

### Beads Issues (`.beads/issues.jsonl`)
- Open/in-progress/closed issue counts with detail drilldown

### Embedded Terminals
- Live PTY output from AI agent processes
- Permission prompt detection (Claude Code, Gemini CLI, etc.)
- Idle detection (configurable timeout)
- Process exit with relaunch capability

## Architecture

```
crew-board/src/
├── main.rs            — CLI (clap), terminal setup, event loop, mouse handling
├── app.rs             — App state, navigation, detail modes, popups, text selection
├── settings.rs        — ~/.config/crew-board.toml loader
├── discovery.rs       — Repo scanning (finds .tasks/ and .beads/ dirs)
├── launcher.rs        — Terminal detection, AI host launch, color schemes
├── worktree.rs        — Native worktree creation (git ops, state.json, symlinks)
├── cleanup.rs         — Worktree cleanup (candidate discovery, dry-run, execution)
├── hook_server.rs     — HTTP hook server for Claude Code event streaming
├── hook_bridge.rs     — Cross-platform hook bridge (Gemini/Copilot/OpenCode)
├── orchestration.rs   — Auto-orchestration engine (tasks, circuit breaker, guardrails)
├── security.rs        — Security rules engine (regex rules, credentials, rate limiting)
├── data/              — Data layer (all parsers use serde with #[serde(default)])
│   ├── task.rs        — .tasks/*/state.json + artifact discovery
│   ├── beads.rs       — .beads/issues.jsonl stream parser
│   ├── config.rs      — Config cascade loader (YAML)
│   ├── activity.rs    — Activity event ring buffer + tool span tracking
│   └── file_claims.rs — Cross-terminal file conflict detection
├── terminal/          — Embedded terminal multiplexer
│   ├── mod.rs         — TerminalManager, EmbeddedTerminal, HookState, status polling
│   ├── pty.rs         — PTY spawning, reader thread, attention detection
│   └── widget.rs      — Terminal rendering (vt100→ratatui), keyboard encoding
└── ui/                — ratatui rendering
    ├── task_list.rs       — Tree view (left pane)
    ├── detail_pane.rs     — Overview/docs/history (right pane)
    ├── terminal_view.rs   — View 5: crew list + terminal panels + mouse rects
    ├── activity_view.rs   — View 6: activity feed + Gantt timeline
    ├── status_bar.rs      — View tabs, F-key bar, modifier layers
    ├── help_popup.rs      — F1 scrollable help overlay
    ├── launch_popup.rs    — F2 terminal launch dialog
    ├── create_popup.rs    — F4 worktree creation wizard
    ├── cleanup_popup.rs   — F6 worktree cleanup dialog
    ├── search_popup.rs    — F3 full-text search
    ├── permission_popup.rs — F8 permission queue
    ├── stats_popup.rs     — Ctrl+F6 statistics overlay
    └── styles.rs          — Color schemes, selection/border/hint helpers
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Terminal I/O, keyboard, mouse events |
| `portable-pty` | Cross-platform PTY abstraction |
| `vt100` | Terminal emulation / ANSI parser |
| `base64` | OSC 52 clipboard encoding |
| `clap` | CLI argument parsing |
| `serde` / `toml` / `serde_yaml` / `serde_json` | Configuration and data parsing |
| `tui-input` | Text input widget for popups |
| `tiny_http` | HTTP hook server |
| `rand` | Auth token generation |
| `regex` | Security rules and pattern matching |
| `anyhow` | Error handling |

## License

MIT
