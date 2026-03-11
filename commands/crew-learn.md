# Learn from Recent Changes

Run the Technical Writer agent against recent git changes to update project documentation, without running a full workflow pipeline.

## Command: /crew-learn $ARGS

### Initialize

Run: `python3 {__scripts_dir__}/crew_orchestrator.py learn --args "$ARGS" --host {__platform__}`

The script returns JSON with routing details. Handle by result:

- **error** -> Show `result.errors` to user
- **learn** -> Continue to Gather Context below

### Gather Context

Based on the `result.diff_command`, gather the git diff context:

1. Run the diff command from `result.diff_command` (e.g., `git diff HEAD~1`, `git diff --since="3 days ago"`, or custom range)
2. Run `git log --oneline` with the same range to get commit summaries
3. If `result.task_dir` is set, read prior agent outputs from that task directory

### Spawn Technical Writer

1. Read agent prompt from `result.agent_prompt_path`
2. Compose prompt:
   - Agent prompt (Technical Writer)
   - Git diff output as "Branch Changes"
   - Git log summary as context
   - Focus description if provided: "Focus area: {result.focus}"
   - If task context available: include task.md and implementer.md
3. Spawn: `Task(subagent_type: "general-purpose", model: result.model, max_turns: result.max_turns, prompt: "<composed prompt>")`
4. Display the Technical Writer's output to the user

### Auto-Commit (if enabled)

If `result.auto_commit` is true:
1. Check if the Technical Writer made any file changes: `git diff --name-only`
2. If changes exist in documentation paths:
   - Stage doc files: `git add docs/ *.md`
   - Suggest commit message: "docs: update documentation from /crew learn"
   - Ask user to confirm commit

### Done

Display summary of documentation updates. No workflow state is created or modified.

Now, process the command arguments:

Arguments: $ARGS
