# Architecture & Patterns

Detailed architecture reference for AI agents working on the agentic-workflow codebase.

## System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  AI Host CLI (Claude Code / Copilot / Gemini / OpenCode)     │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  /crew command (commands/crew.md)                     │  │
│  │  Orchestrates the agent loop, spawns subagents        │  │
│  └───────────────┬───────────────────────────────────────┘  │
│                  │ MCP tool calls                            │
│  ┌───────────────▼───────────────────────────────────────┐  │
│  │  MCP Server (agentic-workflow-server)                  │  │
│  │  ┌─────────────┐ ┌──────────────┐ ┌────────────────┐ │  │
│  │  │ state_tools  │ │ config_tools │ │ orchestration  │ │  │
│  │  │             │ │              │ │ _tools         │ │  │
│  │  └──────┬──────┘ └──────┬───────┘ └───────┬────────┘ │  │
│  │         │               │                  │          │  │
│  │         ▼               ▼                  ▼          │  │
│  │    .tasks/TASK_XXX/   config cascade    crew_* helpers│  │
│  │    state.json         (4 levels)        (arg parsing, │  │
│  │    *.md outputs                          phase loop)  │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## MCP Server Internals

### state_tools.py — The Core (~4200 lines)

This is the largest and most important module. It manages all persistent workflow state.

**Key functions and their groupings:**

#### Task Lifecycle
- `workflow_initialize(task_id, description)` — Creates `.tasks/TASK_XXX/state.json`
- `workflow_transition(to_phase)` — Validates and records phase transitions
- `workflow_get_state(task_id)` — Returns current state
- `workflow_complete_phase()` — Marks current phase done, advances to next
- `workflow_is_complete()` — Checks if all phases are done

#### Implementation Progress
- `workflow_set_implementation_progress(total_steps, current_step)` — Track build progress
- `workflow_complete_step(step_id)` — Mark a plan step as done

#### Worktree Management
- `workflow_create_worktree(task_id, base_branch, ai_host)` — Records worktree metadata, returns git commands
- `workflow_get_launch_command(task_id, terminal_env, ai_host, main_repo_path, launch_mode)` — Generates platform-specific terminal launch commands
- `workflow_get_worktree_info(task_id)` — Check worktree status
- `workflow_cleanup_worktree(task_id)` — Mark cleaned, return cleanup commands

#### Discovery & Memory
- `workflow_save_discovery(category, content)` — Persist a learning to JSONL
- `workflow_get_discoveries(category?)` — Retrieve learnings
- `workflow_flush_context()` — Get all learnings grouped by category
- `workflow_search_memories(query, category?, max_results?)` — Cross-task search

#### Concerns & Review
- `workflow_add_concern(agent, severity, description)` — Record a concern
- `workflow_address_concern(concern_id, resolution)` — Mark concern addressed
- `workflow_add_review_issue(agent, severity, description)` — Add blocking issue

#### Human Decisions
- `workflow_add_human_decision(decision, context)` — Record checkpoint outcome

#### Parallel Execution
- `workflow_start_parallel_phase(phase_name)` — Begin parallel agent execution
- `workflow_complete_parallel_phase(phase_name, output)` — Record parallel output
- `workflow_merge_parallel_results(phase_names)` — Deduplicate and merge

#### Concurrent Access
- `workflow_guard_acquire(task_id)` — Acquire task-level FileLock (`.workflow.lock`), prevents two orchestrators on same task
- `workflow_guard_release(task_id)` — Release the task guard

#### Quality & Error Patterns
- `workflow_add_assertion(name, check)` / `workflow_verify_assertion(name)` — Quality gates
- `workflow_record_error_pattern(error, solution)` / `workflow_match_error(error)` — Error learning (capped at 500 entries with oldest-first rotation)

#### Security
- `_is_safe_task_id(task_id)` — Validates task IDs (rejects `../`, `/`, `\`, null bytes, hidden files)

#### Cost Tracking
- `workflow_record_cost(agent, model, input_tokens, output_tokens)` — Per-agent cost, writes to `state.json` under `cost_tracking.entries[]`
- `workflow_get_cost_summary()` — Breakdown by agent and model

**Primary path — automatic via Stop hook**: `log-crew-interaction.py` fires on every Claude Code Stop event. It calls `_extract_session_cost()` to parse the `session_cost` object from the hook payload (also tries `cost` and `sessionCost` key names for forward compatibility), then calls `_record_cost()` which writes to both:
1. `state.json` via `workflow_record_cost()` — structured `cost_tracking.entries[]`
2. `.tasks/TASK_XXX/costs.jsonl` — append-only JSONL record with `source: "hook"`

Because the hook fires on every session stop (including ad-hoc user interactions during an active task), all cost is attributed to the active task — not just orchestrated agent turns.

**Secondary path — explicit agent reporting**: The `agent-done` subcommand in `crew_orchestrator.py` accepts `--input-tokens`/`--output-tokens` flags and calls `workflow_record_cost()` directly. This path is a backup for cases where the Stop hook payload does not carry cost data.

**Reporting**: `scripts/crew-cost-report.py` reads `costs.jsonl` first; falls back to `state.json cost_tracking.entries[]` if the file is absent. Pricing is unified with `state_tools.py`: opus $5/$25, sonnet $3/$15, haiku $0.80/$4 per million tokens (input/output).

#### Mode & Effort
- `workflow_detect_mode(description)` — Auto-detect workflow mode from task text
- `workflow_set_mode(mode)` / `workflow_get_mode()` — Manual mode control
- `workflow_get_effort_level(agent)` — Recommended thinking depth per mode

**Workflow Modes:**
| Mode | Agents | Use case | Est. cost |
|------|--------|----------|-----------|
| **quick** | implementer | Typos, one-line fixes | ~$0.03 |
| **standard** | planner → implementer → technical_writer | Routine features, refactors | ~$0.10 |
| **thorough** | planner → reviewer → implementer → quality_guard + security_auditor (parallel) → technical_writer | Security, migrations, breaking changes | ~$0.30+ |

Legacy aliases: `micro`/`minimal` → quick, `turbo`/`fast`/`reviewed` → standard, `full` → thorough.

The **technical_writer** phase is a required phase in all modes that include it (standard and thorough). REQUIRED_PHASES are: planner, implementer, technical_writer.

**Async Documentation Mode (standard mode only):**

When `documentation.async_mode` is true, the standard mode workflow completes after the implementer phase and runs the Technical Writer in the background. This allows the user to commit and continue work while docs are updated asynchronously. The `complete_with_async_docs` action signals this to the orchestrator. This setting is ignored in thorough mode, where TW always runs synchronously. Config options: `documentation.auto_commit_docs` (auto-commit doc changes) and `documentation.notify_on_complete` (show notification when done).

**Auto-Detection (default `--mode auto`):**

Mode is selected by two signals — highest wins (thorough > standard > quick):

1. **Keyword matching** against task description (case-insensitive):
   - **quick**: "typo", "spelling", "whitespace", "one-line", "trivial", "rename variable", "update comment", "fix import" — but excluded if description also contains "implement", "refactor", "create", "build", "security", etc.
   - **standard**: "add", "implement", "update", "refactor", "create", "build", "utility" — but excluded if "security", "auth", "database", "migration", "api", "breaking" present.
   - **thorough**: "security", "authentication", "database", "migration", "api", "breaking change", "critical", "auth", "password", "token".
   - If no keywords match → defaults to **standard**.

2. **File scope analysis** (when `files_affected` is provided):
   - Sensitive paths (auth/, security/, migration/, db/) → escalate to thorough
   - Config paths (.env, Dockerfile, CI workflows) → escalate to standard
   - Many files (>10) or many directories (>3) → escalate to standard
   - Cross-module changes (>5 distinct modules) → escalate to thorough

The detection logic is in `workflow_detect_mode()` — pure Python, no LLM involved. Override with `--mode <name>` to skip auto-detection.

**Model Routing:** `_build_phase_action()` returns a `model` field resolved from config:
- Fallback chain: `models.<mode>.<agent>` → `models.<agent>` → `models.default`
- Quick mode defaults to Sonnet, standard/thorough use Opus for planning agents
- Override with `models.default: opus` in project config to use Opus everywhere

**Host-aware Planner Mode:** `_build_phase_action()` also injects a `planner_mode` variable when spawning the planner agent. The value is derived from the `host_aware` config section:
- `planner_mode: auto` (default) — resolves to `plan_only` for hosts that explore before invoking `/crew` (claude, opencode), and `full` for hosts that do not (copilot, gemini)
- `planner_mode: plan_only` — forces the planner to skip Phase 2 (code investigation) regardless of host
- `planner_mode: full` — forces full two-phase planning regardless of host
- When `host_aware.enabled: false`, `planner_mode` is always `full`

**Internal helpers (prefixed with `_`):**
- `_load_state(task_dir)` / `_save_state(task_dir, state)` — JSON I/O with file locking
- `_build_resume_prompt(task_id, path, ai_host)` — Platform-specific resume prompt
- `find_task_dir(task_id)` — Locate `.tasks/TASK_XXX/` directory

### config_tools.py — Configuration (~900 lines)

**`DEFAULT_CONFIG` dict** (line ~24) — All settings with defaults. This is the source of truth for what settings exist.

**Config caching**: `config_get_effective()` caches merged results with a 5-minute TTL. The cache key is based on file paths and their `mtime` values, so editing any config file invalidates the cache on the next call. The cache is per-process only — restarting the MCP server always starts fresh.

Key config sections:
- `permission_profile` — Preset that controls both `checkpoints` and `auto_actions` as a single setting. Values: `strict` (all checkpoints, minimal auto), `standard` (current default), `autonomous` (minimal checkpoints, git auto-enabled). Can be overridden by the `--profile` CLI flag passed to `crew_parse_args()`.
- `checkpoints` — Which human approval points are active per phase (overrides `permission_profile` when set explicitly)
- `knowledge_base` — Path to AI context docs (default: `docs/ai-context/`)
- `models` — Which AI model each agent uses
- `worktree` — Worktree settings (base_path, auto_launch, terminal_launch_mode, ai_host, jira, etc.)
- `auto_actions` — What agents can do without asking (run_tests, git_add, etc.) (overrides `permission_profile` when set explicitly)
- `loop_mode` — Autonomous execution settings
- `max_iterations` — Retry limits per phase type
- `documentation` — Async documentation mode settings (async_mode, auto_commit_docs, notify_on_complete)
- `host_aware` — Host-aware Planner optimization (enabled, skip_exploration per-host, planner_mode)

**`config_get_effective(task_id?)`** — Returns merged config from all 4 cascade levels.

**Multi-platform config paths** — The server searches for config files in Claude, Copilot, Gemini, and OpenCode config directories (in that preference order).

### orchestration_tools.py — Crew Helpers (~1500 lines)

High-level functions called by the `/crew` command and `scripts/crew_orchestrator.py`:

- `crew_parse_args(raw_args)` — Parse command arguments (action, task description, options). Supports `--no-resume` flag to skip auto-detection of active tasks and always start fresh.
- `crew_init_task(description, options)` — Full task initialization (config, state, mode, KB inventory)
- `crew_get_next_phase(task_id)` — Returns next action: spawn_agent, checkpoint (with structured question/options), complete
- `crew_parse_agent_output(agent, output_text)` — Extract issues and recommendations
- `crew_get_implementation_action(task_id, verification_passed?, error_output?)` — Implementation loop logic
- `crew_format_completion(task_id, files_changed)` — Final summary, commit message, cleanup
- `crew_jira_transition(task_id, hook_name, issue_key)` — Resolve Jira lifecycle transition (skip/prompt/execute)
- `crew_get_resume_state(task_id)` — Load resume context for a paused task. Now also returns `recovery_needed` (list of missing outputs) and `stale_phase_warning` fields populated by `_check_task_health()`.

**Crash recovery helper:**

- `_check_task_health(task_dir, state)` — Internal helper that mirrors the Rust `TaskHealth` logic. Checks for missing `{phase}.md` output files for completed phases, and detects stale current phases (no output file + `updated_at` > 30 minutes ago). Returns `{status, stale_phase, missing_outputs, recovery_suggestions}`. Called by `crew_get_resume_state()` and the `health-check` orchestrator subcommand.

### scripts/crew_orchestrator.py — CLI Routing (~430 lines)

CLI script that batches multiple MCP tool calls into single instant JSON decisions, replacing LLM interpretation of procedural routing logic. The orchestrator owns all `state.json` phase transitions -- it calls `workflow_transition()` after determining the next phase, so the LLM never needs to call `workflow_transition` directly. Subcommands:

- `init --args "..." [--no-resume]` — Parse args → init task → get first phase → transition state to first phase (replaces 3 LLM turns). `--no-resume` skips auto-detection of existing active tasks.
- `next --task-id X` — Get next phase/action
- `agent-done --task-id X --agent A` — Parse output → complete phase → record cost → get next → transition to next phase (replaces 4 LLM turns)
- `checkpoint-done --task-id X --decision D` — Record decision → complete phase (approve/skip) → get next → transition to next phase
- `impl-action --task-id X` — Implementation loop step
- `complete --task-id X` — Format completion + resolve Jira transitions + mark state as completed
- `resume --task-id X` — Load resume context + get next phase
- `health-check --task-id X` — Run `_check_task_health()` and return structured JSON report for crash recovery detection
- `quick --description "..." --host X [--mode M] [--no-launch]` — One-shot command: init task + create worktree + generate launch command, returning all results in a single JSON response

### server.py — MCP Registration (~1500 lines)

Registers all tools with the MCP protocol. Each tool has:
- A `Tool()` object with name, description, and JSON Schema for parameters
- A dispatch entry mapping tool name to function

**Pattern for adding new tools:**
1. Import the function from state_tools/config_tools
2. Add a `Tool()` entry in the tools list (~line 200+) with the input schema
3. Add dispatch entry in the `_TOOL_DISPATCH` dict (~line 1500+)

### resources.py — MCP Resources (~200 lines)

Exposes project files as MCP resources that agents can read:
- Agent prompt files from `agents/`
- Configuration files from `config/`
- Documentation from `docs/ai-context/`

## Crew-Board TUI (Rust)

crew-board is a Norton Commander-style terminal dashboard (`crew-board/src/`) written in Rust with ratatui + crossterm. Its full architecture is documented in `crew-board/CLAUDE.md` (agent instructions). This section covers modules and features relevant to MCP/Python developers.

### crew-board UI Modules

```
crew-board/src/ui/
├── mod.rs             # Root layout dispatcher + popup overlay stacking
├── task_list.rs       # Left pane: repo/task tree with filter label
├── detail_pane.rs     # Right pane: task overview, docs, history. Shows Quick Actions (F2/F4/F6/F7) and health warning banners
├── status_bar.rs      # Two-line bottom bar: active work badge, filter indicator, tile navigation hints
├── splash_popup.rs    # Welcome splash overlay (NEW in TASK_083): active tasks, health warnings, quick-start hints
├── help_popup.rs      # F1 scrollable help overlay
├── keybindings.rs     # F-key registry (single source of truth for all views)
└── terminal_view.rs   # Embedded terminal multiplexer (View 5)
```

### Welcome Splash Screen (`ui/splash_popup.rs`)

Shown on startup (configurable with `show_splash_on_start` setting, default `true`). Displays:
- Health warnings for any tasks with crash recovery issues
- Up to 5 active tasks (task_id, phase, description) across all repos
- Quick-start keybinding hints (F4 new worktree, F2 launch host, etc.)
- Stats summary line

Scroll: Up/Down/PgUp/PgDn. Dismissed by any other key press. F-keys pass through (e.g., pressing F4 on the splash directly opens the create-worktree popup).

App state: `app.show_splash: bool`, `app.splash_scroll: u16`.

### Status Bar Active Work Badge

`draw_info_line()` in `status_bar.rs` shows a prominent badge immediately after the view label:
- `[N active]` — bold green background + black text when N > 0
- `[no active work]` — dim gray when no active tasks
- `filter:Active` or `filter:Active+Recent` — dim badge shown next to the active badge when a filter is active

The detailed stats line no longer includes the active count (it moved to the badge).

### Task List Filtering (`f` key)

`TaskFilter` enum in `app.rs` controls which tasks appear in the tree view:

| Filter | Shows |
|--------|-------|
| `All` (default) | All tasks regardless of status |
| `Active` | Tasks not archived and not complete |
| `ActiveAndRecentDone` | Active tasks + completed tasks updated within `recent_done_days` |

Press `f` in Tasks view with left pane focused to cycle: All → Active → Active+Recent → All. Repos with zero matching tasks are still shown (rows remain visible). F3 search always searches all tasks regardless of the active filter.

Settings:
- `recent_done_days` (default 7): days to include completed tasks in the `ActiveAndRecentDone` filter
- `default_task_filter` (default `"all"`): startup filter mode; accepts `"all"`, `"active"`, `"active-recent"`

### Quick Actions in Detail Pane (`ui/detail_pane.rs`)

`draw_overview()` shows a "Quick Actions" section at the bottom of the task detail:
- **F2** (active, when worktree exists): Open AI host in the worktree
- **F4** (active, when no worktree): Create a worktree for this task
- **F2** (dim, when no worktree): Shown as disabled with DIM style
- **F4** (dim, when worktree exists): Shown as disabled with DIM style
- **F6/F7**: Always active (documents / history)

When the task is in-progress, the current phase is shown prominently above the Quick Actions section.

Health warning banners appear after the task description if `health_check()` returns a non-Healthy result (bold red "WARNING: " prefix + yellow message text).

### Crash Recovery in crew-board (`data/task.rs`)

`LoadedTask::health_check()` returns a `TaskHealth` enum:

| Variant | Condition |
|---------|-----------|
| `Healthy` | All completed phases have output files; current phase is not stale |
| `MissingOutputs(Vec<String>)` | One or more `{phase}.md` files are absent for phases listed in `phases_completed` |
| `StalePhase(String)` | Current phase set but not in `phases_completed`, no output file, and `updated_at` is older than 30 minutes |

The welcome splash calls `collect_health_warnings()` to aggregate warnings across all repos at startup.

### F5/F6 Terminal Navigation (Normal Mode)

In Terminals view Normal mode (previously F5=Refresh, F6=Dismiss):
- **F5**: `terminal_focus_prev_running()` — focus previous running terminal, skipping exited
- **F6**: `terminal_focus_next_running()` — focus next running terminal, skipping exited
- F5=Refresh behavior is preserved in all other views (Tasks, Issues, Config, Cost, Activity)
- Exited terminal dismiss: `Delete` key; dismiss all exited: `Ctrl+F4`

### Split Pane Navigation (Alt+Arrow)

Spatial navigation between visible terminal tiles in tiled/stacked layouts:

| Keys | Action |
|------|--------|
| `Alt+Left` / `Alt+Right` | Move focus between horizontal tiles (Tiled2, Tiled4) |
| `Alt+Up` / `Alt+Down` | Move focus between vertical tiles/rows (Tiled4, Stacked) |
| Click on tile | Focus the clicked tile (no drag = focus, drag = text selection) |

Works in both Normal mode and TerminalFocused mode. In TerminalFocused mode, `Alt+Arrow` is intercepted before PTY forwarding; other `Alt+key` combinations (e.g., `Alt+B` for word-back in bash) still reach the PTY. Status bar shows `Alt+←↑→↓:tiles` hint in tiled/stacked layouts.

Implemented via four spatial methods on `App`: `terminal_tile_focus_right/left/down/up()`. These use `terminal_indices_for_layout()` (made `pub(crate)` in `terminal_view.rs`) to map the current focused terminal to its grid position.

### crew-board Settings (`settings.rs` / `~/.config/crew-board.toml`)

New settings added in TASK_083:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `show_splash_on_start` | `bool` | `true` | Show welcome splash overlay at startup |
| `recent_done_days` | `u32` | `7` | Days to include completed tasks in `active-recent` filter |
| `default_task_filter` | `string` | `"all"` | Startup filter mode: `"all"`, `"active"`, `"active-recent"` |

## State Management Pattern

### state.json Structure

```json
{
  "task_id": "TASK_002",
  "phase": "implementer",
  "phases_completed": ["planner", "reviewer"],
  "review_issues": [],
  "iteration": 1,
  "docs_needed": [],
  "implementation_progress": {
    "total_steps": 20,
    "current_step": 13,
    "steps_completed": ["1.1", "1.2", "2.1"]
  },
  "human_decisions": [],
  "concerns": [],
  "worktree": {
    "status": "active",
    "path": "../repo-worktrees/TASK_002",
    "branch": "crew/feature-name",
    "base_branch": "main",
    "color_scheme_index": 2,
    "launch": {
      "terminal_env": "windows_terminal",
      "ai_host": "claude",
      "launch_mode": "tab",
      "launched_at": "2026-02-18T07:20:10",
      "worktree_abs_path": "/path/to/worktree",
      "color_scheme": "Crew Sunset"
    }
  },
  "description": "task description text",
  "created_at": "...",
  "updated_at": "..."
}
```

**File locking**: `_save_state()` uses `filelock` to prevent concurrent writes. Lock files are `state.json.lock`. See also the Filelock Fallback pattern under Cross-Platform Fallback Patterns below.

### metadata.json Fallback (External Task Setup)

Some repositories use external task setup scripts (not the Python MCP server) that create task directories with a `metadata.json` file instead of `state.json`. This has a different, simpler schema:

```json
{
  "jira_key": "SAD-739",
  "description": "Prisomräkning Leverantörspriser counts wrong",
  "branch_name": "task/TASK_005",
  "task_id": "TASK_005",
  "created_at": "2026-02-12T12:24:36.2024350+01:00",
  "worktree_path": "..\\visma.administration-worktrees\\TASK_005",
  "base_branch": "tremendous"
}
```

**crew-board handling**: When `load_tasks()` finds a task directory with no `state.json`, it tries `metadata.json` as a fallback. The `TaskMetadata` struct deserializes this schema, and `TaskState::from_metadata()` maps the fields to a minimal `TaskState`:
- `branch_name` maps to `worktree.branch`
- `worktree_path` maps to `worktree.path`
- `base_branch` maps to `worktree.base_branch`
- `jira_key` is carried on `LoadedTask` (not `TaskState`) as external metadata

Tasks loaded this way are marked `archived: true` since they lack full workflow state. The fallback is silent -- no errors if `metadata.json` is also missing or malformed. See `crew-board/src/data/task.rs` for the implementation.

### Agent Output Files

Each agent writes its output to `.tasks/TASK_XXX/<agent>.md`. These accumulate and are passed as context to subsequent agents.

## Configuration Pattern

### Config File Structure

All configuration lives in a single file:
- `config/workflow-config.yaml` — Essential settings at the top (checkpoints, models, modes, worktree); advanced/power-user settings below a separator line (effort levels, compaction, agent teams, subagent limits, Gemini integration, cost tracking). This is the file users copy and customize.

### Adding a New Setting

Follow this checklist:

1. **Default**: Add to `DEFAULT_CONFIG` in `config_tools.py`
2. **Reference**: Add to `config/workflow-config.yaml` with inline comment (essential settings above the separator, advanced settings below)
3. **Usage**: Read via `config_get_effective()` in the consuming code
4. **Schema**: If exposed as MCP tool parameter, add to `server.py` Tool schema
5. **Tests**: Add to `tests/test_config_tools.py`
6. **Docs**: Update agent docs if it affects agent behavior

### Config Validation

`config_get_effective()` warns about unknown keys but doesn't reject them. The `_get_valid_keys()` helper recursively collects valid keys from `DEFAULT_CONFIG`.

## Agent System Pattern

### Agent Roster

| Agent | Mode(s) | Role |
|-------|---------|------|
| **planner** | standard, thorough | System analysis + implementation plan (read-only) |
| **reviewer** | thorough | Plan review + adversarial analysis (read-only) |
| **implementer** | all | Executes the plan (read-write) |
| **quality_guard** | thorough | Code quality, conventions, plan adherence — runs in parallel with security_auditor (read-write) |
| **security_auditor** | thorough | Security vulnerability review — runs in parallel with quality_guard (read-only) |
| **technical_writer** | standard, thorough | Documentation updates (docs-only write) |

The orchestrator spec lives in `docs/orchestrator-spec.md` (moved from `agents/orchestrator.md`).

Severity levels used by reviewer, quality_guard, and security_auditor are defined in [docs/ai-context/severity-scale.md](severity-scale.md).

### Agent Definition Structure

Each agent is a markdown file in `agents/` with:
- Role description and personality
- Input format (what context they receive)
- Analysis steps / checklist
- Output format (structured markdown)
- Permissions (read-only vs read-write)
- Completion signals (promise tags)

### Platform Mirroring and build-agents.py

`scripts/build-agents.py` generates platform-specific copies from the shared `agents/` source:

| Platform | Output Location (Global / Project) | Format |
|----------|--------------------------------------|--------|
| Claude   | `~/.claude/agents/` + `~/.claude/commands/` | Plain markdown; commands use `$ARGS` |
| Copilot  | `~/.copilot/agents/` (global) or `.github/agents/` (project) `crew-*.agent.md` | YAML frontmatter with name, description, tools |
| Gemini   | `~/.gemini/agents/crew-*.md` | YAML frontmatter with tools, `max_turns`, `timeout_mins` |
| OpenCode | `~/.config/opencode/` (global) or `.opencode/` (project): `agents/crew-*.md` + `commands/` | YAML frontmatter with `mode: subagent`, tool restrictions; commands use `$ARGUMENTS` |

**Do not edit mirror files directly.** Edit `agents/` source and run the build script.

#### Global vs Project Routing

Two platforms require path routing based on whether the build is global (`--output $HOME`) or project-level:

- **`_copilot_agents_dir(output_dir)`**: Routes to `~/.copilot/agents/` for global installs, `.github/agents/` for project installs
- **`_opencode_base(output_dir)`**: Routes to `~/.config/opencode/` for global installs, `.opencode/` for project installs

Claude and Gemini use fixed output paths regardless of install scope.

#### Template Placeholder Substitution

Agent source files use three template placeholders that `build-agents.py` replaces at build time:

| Placeholder | Purpose | Example Substitution |
|---|---|---|
| `{__platform__}` | Platform name | `claude`, `copilot`, `gemini`, `opencode` |
| `{__platform_dir__}` | Platform config directory | `.claude`, `.copilot`, `.gemini`, `.opencode` |
| `{__scripts_dir__}` | Absolute path to helper scripts | `~/.claude/scripts`, `/home/user/agentic-workflow/scripts` |

The mappings are defined in `PLATFORM_DIRS` and `SCRIPTS_DIRS` in `build-agents.py`:

```python
PLATFORM_DIRS = {
    "claude": ".claude",
    "copilot": ".copilot",
    "gemini": ".gemini",
    "opencode": ".opencode",
}

SCRIPTS_DIRS = {
    "claude": "~/.claude/scripts",
    "copilot": str(REPO_ROOT / "scripts"),   # absolute repo path
    "gemini": str(REPO_ROOT / "scripts"),    # absolute repo path
    "opencode": str(REPO_ROOT / "scripts"),  # absolute repo path
}
```

The `{__scripts_dir__}` placeholder ensures that agent/command files can reference helper scripts (e.g., `crew_orchestrator.py`) with paths that work regardless of the user's CWD. For Claude, scripts are installed globally to `~/.claude/scripts/`. For Copilot, scripts are bundled alongside agents (`.github/scripts/`) so they work when deployed to any repo. For Gemini/OpenCode, they resolve to the agentic-workflow repo's absolute path at build time.

#### Build Assertion

After building, `_assert_no_raw_placeholders()` scans all generated `.md` files for any un-substituted `{__platform__}`, `{__platform_dir__}`, or `{__scripts_dir__}` strings. If any remain, the build fails with exit code 1. This catches missing substitutions before they reach agents at runtime.

#### Per-Agent Tool Restrictions

Each platform limits what tools sub-agents can use:

- **Copilot**: No per-tool restrictions; orchestrator gets `tools: ["*"]`
- **Gemini**: Explicit tool allowlists per agent (`GEMINI_AGENT_TOOLS` dict), plus `max_turns` limits (`GEMINI_MAX_TURNS`)
- **OpenCode**: Tool deny-maps per agent (`OPENCODE_AGENT_TOOLS` dict), e.g., `{"write": false, "edit": false}` to restrict read-only agents. Additionally, `OPENCODE_AGENT_PERMISSIONS` provides granular bash command permissions via glob patterns (see [cross-platform.md](./cross-platform.md#opencode-granular-permissions) for details)

#### Per-Agent Model Selection

Two platforms support per-agent model selection via frontmatter emitted by `build-agents.py`:

- **Gemini**: `GEMINI_AGENT_MODELS` dict maps agents to `gemini-2.5-pro` (complex reasoning) or `gemini-2.0-flash` (utility). Emitted as `model:` in frontmatter.
- **OpenCode**: `OPENCODE_AGENT_MODELS` dict (all empty strings by default = inherit from global config). Emitted as `model:` in frontmatter when non-empty.

#### OpenCode-Specific Patterns

- Commands use `$ARGUMENTS` instead of `$ARGS` (auto-substituted by the builder)
- The orchestrator is `mode: primary`; sub-agents are `mode: subagent`
- Command agents get `subtask: true` and an `agent: build` or `agent: read` profile
- Granular `permission:` blocks control bash command access with glob patterns (`_READ_ONLY_BASH`, `_FEEDBACK_BASH`, `_IMPLEMENTER_BASH` profiles)

### Agent Preambles

Each platform prepends a preamble (`config/platform-preambles/`) that adapts tool names, permissions syntax, and conventions to the specific platform.

### Hooks System (Claude Code)

Claude Code supports lifecycle hooks that run scripts at specific trigger points. The hook configuration lives in `config/hooks-settings.json` and is merged into `~/.claude/settings.json` by the installer.

**Hook scripts** (installed to `~/.claude/scripts/`):

| Script | Hook Type | Matcher | Purpose |
|---|---|---|---|
| `validate-transition.py` | PreToolUse | `Task` | Validates workflow phase transitions follow the correct sequence |
| `check-bash-safety.py` | PreToolUse | `Bash` | Warns about destructive git commands, git push during workflows, git commit during planning |
| `check-workflow-complete.py` | Stop | (none) | Blocks exit if workflow incomplete; emits session-close reminders when complete |
| `log-crew-interaction.py` | UserPromptSubmit + Stop | (none) | Captures all human input and agent responses to `interactions.jsonl` |

**Session isolation**: All hooks use `_find_session_task()` which checks `.active_task` file and worktree detection. Non-crew sessions are never affected.

### Interaction Capture System

`log-crew-interaction.py` provides deterministic, automatic capture of the full conversation trail during active crew sessions. It fires on two events:

- **UserPromptSubmit**: Logs every user prompt as `{role: "human", type: "guidance"}`. Skips `/crew` and `/crew-resume` prefixes (already logged by the orchestrator) and internal orchestrator calls (`python3`, `crew_orchestrator`, etc.).
- **Stop**: (1) Extracts cost from the payload (`session_cost` object) and records it to `state.json` + `costs.jsonl` via `_record_cost()`. (2) Reads the transcript file and extracts the last assistant response (truncated to 500 chars), logged as `{role: "agent", type: "message"}`. Cost recording happens even when no transcript summary is produced.

**Interaction entry format:**
```json
{
  "timestamp": "2026-03-12T10:00:00+00:00",
  "role": "human",
  "type": "guidance",
  "content": "...",
  "phase": "implementer",
  "agent": "user",
  "source": "hook"
}
```

The `source: "hook"` field distinguishes entries written by the hook script from entries written explicitly by the orchestrator via `log-interaction`.

**Interaction types** (`type` field):
- `guidance` — ad-hoc user input mid-workflow (hook-captured or explicit log-interaction)
- `correction` — user corrects agent output (explicit log-interaction only)
- `new_requirement` — user adds a requirement mid-workflow (explicit log-interaction only)
- `question` — user asks a clarifying question (explicit log-interaction only)
- `message` — agent response (hook-captured on Stop)
- `escalation_question` / `escalation_response` — implementer escalation exchanges

**Human Guidance Trail**: When the orchestrator spawns an agent (step 8 of Agent Prompt Composition in `crew.md`), it reads `interactions.jsonl` and injects any entries with `role: "human"` and `type` in `["guidance", "correction", "new_requirement", "question"]` under a `## Human Guidance` header. This ensures agents see all user corrections and requirements accumulated during the workflow.

**Compaction safety**: `interactions.jsonl` is listed in `preserve_patterns` in the compaction config so it survives context compaction. The file uses append-only JSONL with `filelock` for safe concurrent access.

**Hook response format**:
- `{"decision": "approve", "reason": "..."}` — Allow the action, display reason as informational message
- `{"decision": "block", "reason": "..."}` — Prevent the action (used by Stop hook for incomplete workflows)
- Exit code 0 with no output — Allow without comment

**Shared state module**: `scripts/workflow_state.py` provides `WorkflowState`, `_resolve_tasks_dir()`, and `_detect_worktree_task_id()` used by all hook scripts. This is a lightweight state reader that does not depend on the MCP server package.

## Worktree Pattern

### Launch Flow

1. Agent detects terminal environment (see Terminal Detection Order below)
2. `workflow_create_worktree()` records metadata, returns git commands
3. Agent executes git commands (worktree add, branch create)
4. Agent runs setup (symlink/junction .tasks, copy settings, pre-authorize permissions, fix paths, install deps)
5. A `.crew-resume` context file is written to the worktree root (see below)
6. `workflow_get_launch_command()` generates platform-specific launch command
7. Agent executes the launch command

### `.crew-resume` Context File

Both `crew-board/src/worktree.rs` and `scripts/setup-worktree.py` write a `.crew-resume` file to the worktree root at creation time. The file is `.gitignore`d and never committed.

**Contents:** task_id, description, main_repo path, tasks_path, base_branch, ai_host, created_at timestamp, and the platform-specific resume command.

**Purpose:** AI hosts that cannot accept CLI prompt arguments (notably Copilot via `gh cs`) read this file to discover what task to resume. The launcher omits the prompt argument for Copilot and relies on `.crew-resume` instead. Claude, Gemini, and OpenCode still receive the resume prompt as a CLI argument, but `.crew-resume` serves as a fallback for any host.

### Terminal Detection Order (`detect_terminal_env()`)

The detection runs in strict priority order. The first match wins:

1. **tmux** — `TMUX` environment variable is set
2. **windows_native** — `platform.system() == "Windows"` (native Windows, NOT WSL). Checked before Windows Terminal to avoid WSL-only `wsl.exe` commands on native Windows.
3. **windows_terminal** — `wt.exe` or `wt` is on PATH (indicates WSL under Windows Terminal)
4. **macos** — `platform.system() == "Darwin"`
5. **linux_generic** — Fallback for all other Linux environments

Each terminal type produces different launch commands:
- **tmux**: `tmux new-window` with window-style color settings
- **windows_terminal**: `wt.exe new-tab` with `--tabColor`, `--colorScheme`, and `wsl.exe --cd` for the AI CLI
- **macos**: `osascript` to open a new Terminal.app window
- **windows_native**: `start powershell -NoExit -EncodedCommand <base64>` (see PowerShell Encoding below)
- **linux_generic**: Manual instructions only (no reliable cross-distro new-terminal command)

### Symlink and Junction Handling

Worktrees need a `.tasks/` link pointing back to the main repo. On Unix this is a simple symlink. On native Windows, symlinks require admin/developer-mode privileges, so the system falls back to NTFS junctions.

#### `_symlink_or_junction(target, link)`

```
Non-Windows: os.symlink(target, link)
Windows:     try os.symlink(target, link)
             except OSError → validate paths → cmd /c mklink /J link target
```

- Before running `mklink /J`, `_validate_path_for_cmd(path)` checks that the path contains no cmd.exe metacharacters (`& | < > ^ % "`). This prevents command injection via crafted paths.
- The function is in `scripts/setup-worktree.py`.

#### `_remove_symlink_or_junction(path)`

```
If os.path.islink(path): os.remove(path)
Elif Windows and os.path.isdir(path): os.rmdir(path)    # Junction!
Else: os.remove(path)
```

**Critical**: NTFS junctions look like directories to Python. Using `os.unlink()` or `shutil.rmtree()` on a junction would follow it and delete the *target's* contents. `os.rmdir()` removes only the junction reparse point without touching the target. This is the correct way to remove a junction.

### Permission Pre-Authorization

Worktree sessions need access to MCP workflow tools and common bash commands. Without pre-authorization, Claude Code prompts the user to approve each tool call individually, which is disruptive in autonomous crew workflows. The permission system solves this at two levels:

#### At Install Time (`install.sh`)

The installer performs global setup that applies to all future worktrees:

- **Claude**: Adds the worktree base directory (`../{repo}-worktrees`) to `~/.claude/settings.local.json` `additionalDirectories`, giving Claude Code file access to all worktree directories
- **Gemini**: Adds the worktree base directory to `~/.gemini/trustedFolders.json` (only if `~/.gemini/` exists)
- **Copilot**: No equivalent permission model; no action taken
- **Template**: Copies `config/worktree-permissions.json` to `~/.claude/config/` for use by per-worktree setup

#### Per-Worktree Creation (setup-worktree.py / state_tools.py)

When a worktree is created (either via `setup-worktree.py` or `workflow_create_worktree()` MCP tool), the settings copy step does:

1. Locates the permissions template — checks `config/worktree-permissions.json` in the repo first, then falls back to `~/.claude/config/worktree-permissions.json`
2. Copies the main repo's `.claude/settings.local.json` into the worktree
3. Patches the copy: injects the main repo's `.tasks/` path into `additionalDirectories`
4. If a permissions template was found, merges its `permissions.allow` entries into the worktree's settings (deduplicating)
5. For Gemini: adds the worktree's absolute path to `~/.gemini/trustedFolders.json`

This is controlled by the `copy_settings` flag in worktree config. When `copy_settings: false`, steps 1-5 are skipped entirely — only the `.tasks/` symlink is created.

#### The Permissions Template (`config/worktree-permissions.json`)

A curated baseline of pre-authorized permissions:

```json
{
  "permissions": {
    "allow": [
      "mcp__agentic-workflow__workflow_initialize",
      "mcp__agentic-workflow__workflow_detect_mode",
      "mcp__agentic-workflow__workflow_transition",
      "mcp__agentic-workflow__workflow_complete_phase",
      ...21 MCP workflow tools total...
      "Bash(python3:*)",
      "Bash(git status:*)",
      "Bash(git diff:*)",
      "Bash(git log:*)",
      "Bash(git add:*)",
      "Bash(bd sync:*)",
      "Bash(bd show:*)",
      ...11 bash command patterns total...
    ]
  }
}
```

The template includes only tools essential for crew workflow operation. It does not pre-authorize destructive operations like `git push`, `git commit`, or file writes — those still require explicit user approval.

#### Per-Platform Support

| Capability | Claude | Gemini | Copilot | OpenCode |
|---|---|---|---|---|
| **File access** (additionalDirectories) | Global (install) + per-worktree | N/A | N/A | N/A |
| **MCP tool permissions** (permissions.allow) | Per-worktree via template merge | No equivalent system | No equivalent system | Per-agent via frontmatter |
| **Folder trust** (trustedFolders.json) | N/A | Global (install) + per-worktree | N/A | N/A |
| **Bash command patterns** | Per-worktree via template merge | No equivalent system | No equivalent system | Per-agent via frontmatter |
| **.tasks/ symlink** | Yes | Yes | Yes | Yes |

#### How to Customize or Opt Out

- **Remove specific permissions**: Edit `config/worktree-permissions.json` to remove entries you do not want pre-authorized. The template is the single source — changes apply to all future worktrees.
- **Skip all settings copying**: Set `copy_settings: false` in `workflow-config.yaml` under `worktree:`. This disables the entire settings copy + permission merge pipeline. Only the `.tasks/` symlink is created.
- **Revoke Claude file access**: Manually edit `~/.claude/settings.local.json` to remove the worktree base directory from `additionalDirectories`.
- **Revoke Gemini trust**: Remove entries from `~/.gemini/trustedFolders.json`.
- **Per-worktree override**: After a worktree is created, edit its `.claude/settings.local.json` directly to add or remove permissions for that specific session.

### Color Schemes

8 schemes cycle by task number (defined in `state_tools.py:CREW_COLOR_SCHEMES`):
- Crew Ocean, Forest, Sunset, Amethyst, Steel, Ember, Frost, Earth
- Applied as tab colors in Windows Terminal, window-style in tmux

### Terminal Launch Modes

`terminal_launch_mode` setting (in `worktree` config):
- `auto` — Platform default (tmux→window, WT→tab, macOS→window)
- `window` — Force `wt.exe new-window` on Windows Terminal
- `tab` — Force `wt.exe new-tab` on Windows Terminal

tmux and macOS always use windows regardless of this setting.

## Testing Patterns

### Test Organization

```
tests/
├── test_state_tools.py          # Core state, worktree, launch, resume tests
├── test_state_tools_extended.py # Additional state tests (assertions, errors, etc.)
├── test_config_tools.py         # Config loading, cascade, platform paths
├── test_config_tools_extended.py # Additional config edge cases
├── test_orchestration_tools.py  # Crew helpers (arg parsing, phase routing, checkpoints, Jira transition)
├── test_crew_orchestrator.py    # CLI orchestrator script (subprocess tests)
├── test_resources.py            # MCP resource tests
├── test_scripts.py              # Deterministic CLI scripts (crew-config, crew-status, crew-cost-report, crew-stats)
├── test_security.py             # Security tests (path traversal, shell injection, oversized inputs, symlinks)
└── conftest.py                  # Shared fixtures
```

### Key Fixtures

```python
@pytest.fixture
def clean_tasks_dir(tmp_path, monkeypatch):
    """Creates a temp .tasks/ dir and patches find_task_dir to use it."""
```

All tests use this to avoid polluting real state.

```python
@pytest.fixture
def isolated_tasks_dir():
    """Provide a completely isolated temp .tasks/ directory.
    Redirects _cached_tasks_dir so that get_tasks_dir() returns
    a fresh temp directory containing no real tasks."""
```

The `isolated_tasks_dir` fixture redirects `_cached_tasks_dir` to a temp directory, preventing real in-progress tasks from leaking into tests that scan all task directories (e.g., `_find_active_task_dir`). Use this fixture for tests that need a completely clean task namespace.

### Running Tests

```bash
cd mcp/agentic-workflow-server
python3 -m pytest tests/ -v                    # All (463+ tests)
python3 -m pytest tests/test_state_tools.py -v # One file
python3 -m pytest tests/test_state_tools.py::TestLaunchMode -v  # One class
python3 -m pytest tests/ -k "test_tmux" -v     # By name pattern
```

## Common Gotchas

1. **LF line endings**: The `.git` pointer file in worktrees MUST be LF, not CRLF. Always use `printf` to write it, never echo or file-write tools.

2. **Agent mirror files**: Files in `.github/agents/`, `~/.copilot/agents/`, `~/.gemini/agents/`, `~/.config/opencode/`, and `.opencode/` are auto-generated by `build-agents.py`. Edit the source in `agents/` instead.

3. **State file locking**: `state.json` uses filelock. If a lock file is stale, delete `state.json.lock`.

4. **Config cascade order**: Claude dirs are searched first, then Copilot, then Gemini, then OpenCode. The first found wins at each level. The search order is defined by `PLATFORM_DIRS = [".claude", ".copilot", ".gemini", ".config/opencode", ".opencode"]`. Both `.config/opencode` (XDG standard) and `.opencode` (legacy) are searched for OpenCode.

5. **Worktree .tasks/ symlink or junction**: In worktrees, `.tasks/` is a symlink (Unix) or NTFS junction (Windows) to the main repo. MCP tools resolve this automatically via `find_task_dir()`, but direct file reads should use the absolute main repo path. **Never use `os.unlink()` or `shutil.rmtree()` on a junction** — use `os.rmdir()` instead to avoid deleting the target's contents.

6. **`_shell_quote()` not `shlex.quote()`**: For cross-platform safety, use `_shell_quote(s, use_powershell)` from `setup-worktree.py`. On Unix it delegates to `shlex.quote()`. On PowerShell it wraps in single quotes with doubled internal quotes. This distinction matters because PowerShell treats backticks, dollar signs, and double quotes differently from bash.

7. **`{__platform__}`, `{__platform_dir__}`, and `{__scripts_dir__}` placeholders**: Agent source files in `agents/` and `commands/` use these placeholders. They are substituted by `build-agents.py` at build time. `{__scripts_dir__}` resolves to an absolute scripts path per platform (e.g., `~/.claude/scripts` for Claude, absolute repo path for others). If you see any of these in built output, the build has a bug — the `_assert_no_raw_placeholders()` check should catch this.

8. **WSL path performance**: When a worktree lives on `/mnt/` (NTFS via 9P bridge), git and npm operations are extremely slow. The system detects this (`is_wsl()` + path starts with `/mnt/`) and routes commands through PowerShell to use native Windows git. The `fix-worktree-paths.py` script fixes `.git` pointer files for this scenario.

9. **OpenCode resume syntax**: OpenCode uses `/crew-resume TASK_XXX` (hyphenated slash command), not `/crew resume TASK_XXX` (Claude) or `@crew-resume TASK_XXX` (Copilot/Gemini). This is handled by `_build_resume_prompt()` in both `state_tools.py` and `setup-worktree.py`.

10. **Inlined vs shared utilities**: `setup-worktree.py` inlines copies of `is_wsl()` and `find_repo_root()` to remain standalone (no imports from `scripts/`). The canonical versions live in `scripts/shared_utils.py`. Additionally, `install-wt-colorschemes.py` inlines its own copy of `is_wsl()`. If you change the logic, update all three locations.
