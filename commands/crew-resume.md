# Resume Workflow

## Command: /crew-resume $ARGS

Run: `python3 {__scripts_dir__}/crew_orchestrator.py resume --host {__platform__} $( [ -n "$ARGS" ] && echo "--task-id $ARGS" )`

The script auto-detects the task ID from the current directory if no args given.

Follow `result.instructions` exactly. Do NOT add steps, read files, call MCP tools, or explore before spawning.

Then enter the action loop from `/crew` — follow `result.next.instructions` (spawn → save → agent-done → continue).
