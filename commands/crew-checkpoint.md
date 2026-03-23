# Workflow Checkpoint

Save a manual checkpoint of the current workflow state with a summary.

## Command: /crew-checkpoint $ARGS

### Step 1: Get Active Task State

Run: `python3 {__scripts_dir__}/crew_orchestrator.py next --task-id active`

This returns the current task state including phase, progress, and task_id. If no active task, it will return an error — tell the user.

**Do NOT search for `.tasks/` directories or read `state.json` yourself.**

### Step 2: Save Checkpoint

Call `workflow_save_discovery` with:
```
workflow_save_discovery(category="decision", content="Checkpoint: <phase> — <user's note or auto-summary>")
```

If the user provided arguments ($ARGS), use that as the checkpoint note.
If no arguments, generate a brief summary of what's been done so far.

### Step 3: Display Summary

```
Checkpoint saved for TASK_XXX:
  Phase:    <current phase>
  Progress: <progress if in implementation>
  Note:     <checkpoint note>

  Resume with: /crew-resume TASK_XXX
```

### Step 4: Git Status

Run `git status --short` and if there are changes, suggest:
```
Uncommitted changes detected. Consider committing before continuing.
```

Now, save a checkpoint:

Arguments: $ARGS
