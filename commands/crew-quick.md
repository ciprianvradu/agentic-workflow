# Quick Workflow Start (from within a running AI session)

Create a new worktree and start working immediately — all in one command.
Use this when you're already in a Claude Code session and want to spin up a second task.

**For cold-start (shell prompt → AI ready), use crew-board CLI instead:**
```
crew-board --quick "description"      # Create worktree + launch host
crew-board --resume-latest            # Resume most recent active task
```

## Command: /crew-quick $ARGS

**Purpose:** Combines task creation, worktree setup, and host launch into a single step.
Equivalent to running `/crew-worktree "description"` then waiting, then `/crew resume TASK_XXX`,
but faster and non-interactive.

**Usage:**
```
/crew-quick "Add caching layer to the API"
/crew-quick "Fix login bug" --host copilot
/crew-quick "Refactor auth module" --host claude --mode thorough
/crew-quick "Quick typo fix" --no-launch
```

**Flags:**
- `--host claude|copilot|gemini|opencode` — AI host to launch (default: claude)
- `--mode quick|standard|thorough` — workflow mode (default: standard)
- `--no-launch` — create task and worktree but skip launching the host

---

### Step 1: Parse Arguments

Extract description and flags from `$ARGS`:

```
description = everything before any -- flags
host = --host value (default "claude")
mode = --mode value (default "standard")
no_launch = --no-launch present
```

### Step 2: Run Quick Orchestration

Call the orchestrator with a single command:

```bash
python3 scripts/crew_orchestrator.py quick \
  --description "<description>" \
  --host <host>
```

This single call does:
1. Initializes the task (`crew_init_task`)
2. Creates the worktree (`workflow_create_worktree`)
3. Generates the launch command (`workflow_get_launch_command`)
4. Returns all results in one JSON response

### Step 3: Display Result

Show the user what was created:

```
Created TASK_084: <description>
Worktree: ../<repo>-worktrees/TASK_084/
Branch:   crew/<slugified-description>
Mode:     standard

To start working:
  <launch command shown here>

To resume later:
  /crew resume TASK_084
```

### Step 4: Launch (unless --no-launch)

If the orchestrator returns a `launch_command`, execute it to start the AI host
in the new worktree. The host will automatically receive `/crew resume TASK_084`
as its startup prompt.

If `--no-launch` was specified, skip this step and just show the launch command
for the user to run manually.

---

### Error Handling

If task creation fails:
```
Error: <error message from orchestrator>
```

If worktree creation fails (e.g., repo not found, git error):
```
Task TASK_084 created but worktree creation failed: <error>
To retry: /crew-worktree resume TASK_084
```

If host launch fails:
```
Worktree ready at ../<repo>-worktrees/TASK_084/
Launch failed: <error>
Start manually: <launch_command>
```

---

### Notes

- This command is designed for the "I know what I want to do, just start" case.
- For the full wizard (host selection, pull toggle, etc.), use F4 in crew-board.
- The worktree is created at `../<repo-name>-worktrees/TASK_XXX/` following the standard layout.
- Use `/crew resume TASK_XXX` any time to return to this task.
- To skip resume detection when calling `/crew`, use: `/crew "description" --no-resume`
