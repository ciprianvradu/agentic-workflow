# Custom Phases (Lifecycle Hooks)

Custom phases let you inject your own steps into the crew workflow — skills, scripts, or agents — at any point in the pipeline. They are configured in `workflow-config.yaml` and evaluated dynamically at runtime.

## Quick Start

Add to your project's `workflow-config.yaml` (or the global one):

```yaml
custom_phases:
  # Run a Jira triage skill before planning starts
  triage:
    after: init
    type: skill
    skill: "evaluate-jira"
    condition:
      task_has: "jira_key"
    writes_to_state: true

  # Run an encoding check before the workflow completes
  encoding_check:
    before: complete
    type: script
    command: "python3 scripts/check-encodings.py {task_id}"
    timeout: 60

  # Run a custom security scan after the reviewer
  deep_security:
    after: reviewer
    type: agent
    prompt_file: "agents/deep-security.md"
    condition:
      file_patterns: ["**/auth/**", "**/security/**"]
    writes_to_state: true
```

## Execution Types

### `skill` — Claude Code Skill

Invokes a Claude Code slash command/skill. The orchestrator calls it via `Skill(skill: "<name>")`.

```yaml
custom_phases:
  jira_triage:
    after: init
    type: skill
    skill: "evaluate-jira"        # Required: skill name
    condition:
      task_has: "jira_key"
    writes_to_state: true         # Save output to state.json
    blocking: true                # Failure blocks the workflow (default)
```

**Action returned:** `run_skill` with `skill`, `phase`, `output_file`.

### `script` — Shell Command

Runs a shell command via Bash. Exit code 0 = success, non-zero = failure.

```yaml
custom_phases:
  lint_check:
    before: complete
    type: script
    command: "npm run lint -- --quiet"   # Required: shell command
    timeout: 60                          # Max seconds (default: 120)
    blocking: true                       # Non-zero exit blocks workflow
```

**Variable substitution** in `command`:
- `{task_id}` — Current task ID (e.g., `TASK_042`)
- `{task_dir}` — Task directory path (e.g., `.tasks/TASK_042`)

**Action returned:** `run_script` with `command`, `phase`, `output_file`, `timeout`.

### `agent` — Subagent with Prompt File

Spawns a general-purpose subagent using a custom prompt file.

```yaml
custom_phases:
  compliance_review:
    after: developer
    type: agent
    prompt_file: "agents/compliance.md"  # Required: path to prompt markdown
    condition:
      mode_in: [thorough]
    writes_to_state: true
```

**Action returned:** `spawn_agent` with `agent_prompt_path`, `agent`, `phase`.

## Positioning

Every custom phase must specify exactly one of `after` or `before`:

| Position | Meaning | Example |
|----------|---------|---------|
| `after: init` | Before the first agent phase (post-initialization) | Pre-planning triage |
| `after: architect` | Immediately after architect completes | Post-architecture validation |
| `after: developer` | Immediately after developer completes | Plan enrichment |
| `after: reviewer` | Immediately after reviewer completes | Extra review step |
| `before: implementer` | Immediately before implementer starts | Pre-implementation setup |
| `before: complete` | After all agents, before workflow completion | Final checks |

**Notes:**
- `after: init` inserts at the start of the phase sequence (before the first mode agent).
- `before: complete` appends at the end of the phase sequence (after the last mode agent).
- If the anchor phase is not in the current mode's sequence, the custom phase is silently skipped.

## Conditions

Conditions control whether a custom phase runs. All specified conditions must be true (AND logic). Omit all conditions for an always-run phase.

### `always: true`

Unconditional — always runs regardless of task or mode.

```yaml
condition:
  always: true
```

### `task_has: "keyword"`

Runs when the task description contains the keyword (case-insensitive).

```yaml
condition:
  task_has: "jira_key"    # Matches "Fix jira_key issue SAD-123"
```

### `mode_in: [mode1, mode2]`

Runs only in specific workflow modes. Supports aliases (`full` = `thorough`, `turbo`/`minimal` = `standard`).

```yaml
condition:
  mode_in: [thorough, reviewed]   # Only for thorough/reviewed modes
```

### `file_patterns: ["glob1", "glob2"]`

Runs when any affected file matches a glob pattern. Requires `files_affected` to be populated (typically from task description or git diff).

```yaml
condition:
  file_patterns: ["**/auth/**", "**/*.security.*"]
```

### Combined Conditions

Multiple conditions are ANDed together:

```yaml
condition:
  task_has: "auth"
  mode_in: [thorough]
  file_patterns: ["**/auth/**"]
# Runs only when: task mentions "auth" AND mode is thorough AND auth files affected
```

## Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `writes_to_state` | bool | `false` | Store phase output in `state.json` under `custom_phase_results.<phase_name>` |
| `blocking` | bool | `true` | When `true`, a failure (non-zero exit, skill error) blocks the workflow and prompts the user |
| `timeout` | int | `120` | Max seconds for script execution (script type only) |

## Phase Name Rules

Phase names must be safe filesystem identifiers:
- Allowed: letters, digits, underscores, hyphens (`my_phase`, `lint-check`, `step2`)
- Rejected: slashes, backslashes, `..`, spaces (`my/phase`, `..phase`, `my phase`)

## How It Works Internally

1. **Loading**: `_load_custom_phases()` reads `custom_phases` from effective config, validates each entry, and normalizes defaults.
2. **Insertion**: `_insert_custom_phases_into_sequence()` takes the mode's standard phase list and inserts custom phases at their `after`/`before` positions, filtering by condition.
3. **Action building**: `_build_custom_phase_action()` returns the appropriate action dict (`run_skill`, `run_script`, or `spawn_agent`) with variable substitution.
4. **Completion**: `crew_orchestrator.py custom-phase-done` handles output saving, state updates, and phase transitions.

The existing concern system, checkpoint logic, and phase completion tracking all work with custom phases — they are first-class phases in the sequence.

## Examples

### Example 1: Jira Triage Before Planning

Evaluate a Jira issue before the planning agents start. Stores triage results in state for downstream agents to read.

```yaml
custom_phases:
  jira_triage:
    after: init
    type: skill
    skill: "evaluate-jira"
    condition:
      task_has: "jira_key"
    writes_to_state: true
    blocking: true
```

### Example 2: License Check Before Completion

Run a license compliance script after all agents are done, before the workflow completes.

```yaml
custom_phases:
  license_check:
    before: complete
    type: script
    command: "python3 scripts/check-licenses.py {task_id}"
    timeout: 30
    blocking: false      # Warn but don't block
```

### Example 3: Domain-Specific Review for Auth Changes

Spawn a custom security-focused agent only when authentication files are affected.

```yaml
custom_phases:
  auth_deep_review:
    after: reviewer
    type: agent
    prompt_file: "agents/auth-review.md"
    condition:
      file_patterns: ["**/auth/**", "**/login/**", "**/session/**"]
    writes_to_state: true
```

### Example 4: Thorough-Only Architecture Validation

Run an extra architecture validation step, but only in thorough mode.

```yaml
custom_phases:
  arch_validation:
    after: architect
    type: script
    command: "python3 scripts/validate-architecture.py {task_dir}"
    condition:
      mode_in: [thorough]
    timeout: 60
```

### Example 5: Multiple Custom Phases

You can define multiple custom phases — they are inserted independently based on their positioning.

```yaml
custom_phases:
  # Pre-planning: gather context from external systems
  gather_context:
    after: init
    type: script
    command: "python3 scripts/gather-context.py {task_id}"
    condition:
      always: true
    writes_to_state: true

  # Post-review: run static analysis
  static_analysis:
    after: reviewer
    type: script
    command: "npx eslint --format json src/"
    timeout: 120
    blocking: false

  # Pre-completion: generate changelog entry
  changelog:
    before: complete
    type: skill
    skill: "generate-changelog"
    condition:
      mode_in: [thorough, reviewed]
```

## Configuration Cascade

Custom phases follow the same cascade as all other config:

1. **Global** (`~/.claude/workflow-config.yaml`) — Applies to all projects
2. **Project** (`<repo>/.claude/workflow-config.yaml`) — Project-specific phases
3. **Task** (`.tasks/TASK_XXX/config.yaml`) — Task-specific overrides

Later levels override earlier ones. A project can define phases that only apply to that codebase, while the global config provides universal checks.

## Failure Handling

When a custom phase fails:

- **`blocking: true` (default)**: The orchestrator returns `custom_phase_failed`. The crew.md handler prompts the user with options: Retry, Skip, or Abort.
- **`blocking: false`**: The failure is logged but the workflow continues to the next phase.

Script failures are determined by exit code (non-zero = failure). Skill and agent failures are determined by the orchestrator's error handling.
