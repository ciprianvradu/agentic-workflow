# Resume Workflow

Resume an existing workflow from its saved state.

## Command: /crew-resume $ARGS

Arguments should be a task ID like `TASK_042` or a path like `.tasks/TASK_042`.

### Step 1: Let the Orchestrator Find Everything

Run: `python3 {__scripts_dir__}/crew_orchestrator.py init --host {__platform__} --args "$ARGS"`

The orchestrator handles all path resolution automatically:
- Detects worktrees and resolves `.tasks/` to the main repo
- Finds the active task from `.active_task` file or recent state
- Reads `state.json`, determines resume point
- Returns `action: "resume"` with full context

**Do NOT search for `.tasks/` directories or read `state.json` yourself.** The orchestrator does this and returns everything you need.

### Step 2: Display Resume Summary

From `result.resume_state.display_summary`, show the user where we left off:

```
┌─────────────────────────────────────────────────────────────┐
│ Resuming: TASK_042 - description                            │
├─────────────────────────────────────────────────────────────┤
│ Phase: [current phase]                                      │
│ Progress: [completed phases] / [total phases]               │
│ Last activity: [timestamp]                                  │
└─────────────────────────────────────────────────────────────┘
```

Check `result.resume_state.recovery_needed` and `result.resume_state.stale_phase_warning` — display warnings if set.

### Step 3: Enter Action Loop

Use `result.next` to enter the action loop from `/crew`:
- `next.assembled_prompt` contains the complete agent prompt (use directly)
- `next.output_file` is where to save agent output
- Follow the same action loop as `/crew` (spawn_agent → agent-done → loop)

Now, resume the specified task:

Task ID: $ARGS
