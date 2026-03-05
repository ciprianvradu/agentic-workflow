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
1:Tasks 2:Issues 3:Config 4:Cost [5:Terms]  ↑↓ crew  ! 1 attn  5 repos 12 tasks (3s)
 F1Help F2Launch F3Search F4New F5Rfrsh F6Clean F7Attn F8Perms F9Focus           F10Quit
```

## Features

**Embedded Terminal Multiplexer** — Run AI agents (Claude Code, Gemini CLI, GitHub Copilot, OpenCode) inside crew-board. Each crew gets a full PTY terminal with ANSI color, cursor positioning, and alternate screen support.

**Permission Prompt Detection** — Automatically detects when an AI agent is waiting for approval. Yellow attention badges appear in the crew list, and F7 jumps straight to the next blocked crew.

**Permission Queue (F8)** — Centralized popup showing all pending permissions across all crews. Approve or deny without switching terminals.

**Mouse Support** — Click and drag to select text within terminal panels (constrained to panel boundaries). Scroll wheel for scrollback. Auto-copy to clipboard via OSC 52.

**Four Layout Modes** — Focused (one large terminal), Tiled-2 (side-by-side), Tiled-4 (2x2 grid), Stacked (vertical). Cycle with `l`.

**Full Modifier Encoding** — Ctrl+Arrow (word jump), Ctrl+Enter (newline), Shift+Up/Down, and all other modifier+key combinations work correctly inside embedded terminals.

**Five Dashboard Views** — Tasks, Issues, Config, Cost, and Terminals. Switch instantly with number keys or Shift+F-keys (even while focused in a terminal).

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

Switch views with number keys, backtick to cycle, or Shift+F1-F5:

| Key | View | Shows |
|-----|------|-------|
| `1` | Tasks | Tree of repos and tasks with detail pane |
| `2` | Issues | Beads issue tracker (`.beads/issues.jsonl`) |
| `3` | Config | Configuration cascade (global/project/task) |
| `4` | Cost | Cost estimates and actuals from workflow state |
| `5` | Terminals | Embedded terminal multiplexer with crew list |

## Key Bindings

### F-Key Bar

The bottom bar shows context-sensitive F-key actions. Holding Shift reveals a second layer for view switching:

**Base layer:**

| Key | Action |
|-----|--------|
| `F1` | Help overlay (all keybindings) |
| `F2` | Launch terminal with AI host |
| `F3` | Search across tasks & documents |
| `F4` | Create new worktree (repo rows only) |
| `F5` | Force refresh |
| `F6` | Cleanup worktrees (repo rows only) |
| `F7` | Jump to next terminal needing attention |
| `F8` | Permission queue popup |
| `F9`/`F12` | Focus terminal (Terminals view) |
| `F10` | Quit |

**Shift+F layer:**

| Key | Action |
|-----|--------|
| `Shift+F1` | Switch to Tasks view |
| `Shift+F2` | Switch to Issues view |
| `Shift+F3` | Switch to Config view |
| `Shift+F4` | Switch to Cost view |
| `Shift+F5` | Switch to Terminals view |
| `Shift+F6` | Browse task documents |
| `Shift+F7` | View task history |

### Navigation

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Move up/down |
| `Enter`/`Space` | Expand/collapse repo |
| `Tab` | Switch focus between panes |
| `PgUp`/`PgDn` | Scroll detail pane |
| `1`-`5` | Switch views |
| `` ` `` | Cycle views |
| `q` / `Ctrl+C` | Quit |

**Note:** `Esc` never quits — it only closes popups, backs out of detail views, or switches pane focus.

### Task Detail

| Key | Action |
|-----|--------|
| `d` | Browse task documents (architect.md, developer.md, etc.) |
| `h` | View task history (decisions, phases, concerns) |
| `Esc` | Back (reader → list → overview) |

### Embedded Terminals (View 5)

View 5 is a full terminal multiplexer with three input modes:

**Normal Mode** — Navigate the crew list:

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Focus previous/next terminal |
| `F9` / `F12` | Enter focused mode (all keys go to PTY) |
| `Enter` | Relaunch exited terminal |
| `d` / `Delete` | Dismiss exited terminal |
| `l` / `Right` | Cycle layout (focused → tiled-2 → tiled-4 → stacked) |
| `[` | Enter scroll-back mode |

**Focused Mode** — All keystrokes go to the active terminal:

| Key | Action |
|-----|--------|
| `F12` | Exit focus (back to Normal) |
| `Shift+F1`-`F5` | Switch view (works even while focused) |
| `Shift+PgUp` | Focus previous running terminal (skips exited) |
| `Shift+PgDn` | Focus next running terminal (skips exited) |
| All other keys | Sent to PTY with full modifier encoding |

**Scroll-Back Mode** — Browse terminal history:

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Scroll line by line |
| `PgUp`/`PgDn` | Scroll by page |
| `Home`/`End` | Jump to top / live view |
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
| `a` | Approve (sends `y` to the terminal) |
| `d` | Deny (sends `n` to the terminal) |
| `v` / `Enter` | View — jump to the terminal for full context |
| `Esc` | Close |

### Worktree Creation (F4)

Opens a multi-step wizard (on repo rows only):
1. **Task description** — free text input
2. **AI host** — Claude Code, GitHub Copilot, Gemini CLI, or OpenCode
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

# Auto-launch embedded terminal on F4 worktree creation (default: true)
auto_embed_on_create = true

# ── Attention & Notifications ───────────────────

# Idle detection: mark terminal as idle after N seconds of no output (default: 120)
idle_timeout_secs = 120

# Flash status indicator when a crew needs attention (default: true)
visual_bell = true

# Trigger terminal bell on attention (default: false)
system_bell = false
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
├── main.rs          — CLI (clap), terminal setup, event loop, mouse handling
├── app.rs           — App state, navigation, detail modes, popups, text selection
├── settings.rs      — ~/.config/crew-board.toml loader
├── discovery.rs     — Repo scanning (finds .tasks/ and .beads/ dirs)
├── launcher.rs      — Terminal detection, AI host launch, color schemes
├── worktree.rs      — Native worktree creation (git ops, state.json, symlinks)
├── cleanup.rs       — Worktree cleanup (candidate discovery, dry-run, execution)
├── data/            — Data layer (all parsers use serde with #[serde(default)])
│   ├── task.rs      — .tasks/*/state.json + artifact discovery
│   ├── beads.rs     — .beads/issues.jsonl stream parser
│   └── config.rs    — Config cascade loader (YAML)
├── terminal/        — Embedded terminal multiplexer
│   ├── mod.rs       — TerminalManager, EmbeddedTerminal, status polling
│   ├── pty.rs       — PTY spawning, reader thread, attention detection
│   └── widget.rs    — Terminal rendering (vt100→ratatui), keyboard encoding
└── ui/              — ratatui rendering
    ├── task_list.rs      — Tree view (left pane)
    ├── detail_pane.rs    — Overview/docs/history (right pane)
    ├── terminal_view.rs  — View 5: crew list + terminal panels + mouse rects
    ├── status_bar.rs     — View tabs, F-key bar, modifier layers
    ├── help_popup.rs     — F1 scrollable help overlay
    ├── launch_popup.rs   — F2 terminal launch dialog
    ├── create_popup.rs   — F4 worktree creation wizard
    ├── cleanup_popup.rs  — F6 worktree cleanup dialog
    ├── search_popup.rs   — F3 full-text search
    ├── permission_popup.rs — F8 permission queue
    └── styles.rs         — Color schemes, selection/border/hint helpers
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
| `anyhow` | Error handling |

## License

MIT
