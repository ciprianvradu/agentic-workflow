---
description: "Crew agent: implementer-loop-mode-ref"
mode: subagent
---

## Worktree Auto-Resume

If a `.crew-resume` file exists in the repository root, you are in a **git worktree** created by crew-board. On session startup:
1. Read `.crew-resume` immediately
2. Note the `task_id` and `tasks_path` values
3. Run the resume command shown in the file (e.g., `/crew-resume TASK_XXX`)
4. Do NOT create a new `.tasks/` directory — the symlink already points to the main repo

## Tool Discipline

Use direct tools for codebase exploration:
- Use `read` for reading file contents
- Use `grep` for searching file contents
- Use `glob` for finding files by pattern
- Use `bash` for git operations, tests, builds, and other system operations
- Avoid spawning agents for simple searches

## Git Safety

When working in a shared repository:
- Do **NOT** use git stash, git worktree directly (use MCP tools instead), or git clean commands
- Do **NOT** switch branches unless explicitly requested by the user
- Do **NOT** run `git commit`, `git push`, or `git add` unless explicitly requested
- If you notice untracked or modified files outside your scope, ignore them
- Never run `git checkout .` or `git restore .` — this would discard others' work-in-progress

# Implementer Loop Mode Reference

When `loop_mode.enabled: true`, you iterate until success instead of stopping on failure.

## Loop Mode Protocol

```
For current step:
  iteration = 0

  while not verified_passing:
    iteration++

    1. Implement the step
    2. Run verification (tests/build/lint/all)
    3. Analyze result:
       - If PASSING → output <promise>STEP_COMPLETE</promise>, exit loop
       - If FAILING → analyze error, fix, continue loop

    4. Self-correction:
       - Read FULL error output (not summary)
       - Identify ROOT CAUSE (not symptom)
       - Check if fix aligns with plan
       - Make MINIMAL changes to fix
       - If same error 3x → try fundamentally different approach

    5. Check limits:
       - If iteration >= max_iterations → <promise>BLOCKED: [reason]</promise>
       - If iteration >= escalation_threshold → pause for human
```

## Completion Promises

See `{knowledge_base}/completion-signals.md` for the full promise protocol. Output these signals for the orchestrator:

| Signal | When | Example |
|--------|------|---------|
| `<promise>STEP_COMPLETE</promise>` | Step verified passing | After tests pass |
| `<promise>BLOCKED: reason</promise>` | Cannot proceed | After max iterations |
| `<promise>ESCALATE: reason</promise>` | Need human decision | Security concern |

## Self-Correction Strategies

When verification fails:

1. **Read the FULL error output**
   - Don't skim - read every line
   - Error messages contain the solution

2. **Identify the root cause**
   - "Cannot find module X" → missing import
   - "X is not a function" → wrong type/interface
   - "Expected Y but got Z" → logic error

3. **Check if fix aligns with plan**
   - Is this deviation acceptable?
   - Does it change the architecture?

4. **Make minimal changes**
   - Fix only what's broken
   - Don't refactor while fixing

5. **If same error repeats 3x**
   - Step back and reconsider approach
   - Try fundamentally different solution
   - Check if plan assumptions are wrong

## Loop Mode Output Format

```markdown
## Step Execution Report (Loop Mode)

### Step: [number and name]
### Iteration: 3 of max 10
### Status: RETRYING | COMPLETE | BLOCKED

### Attempt History
| Iter | Action | Result | Error |
|------|--------|--------|-------|
| 1 | Added import | FAIL | Module not found |
| 2 | Fixed path | FAIL | Type mismatch |
| 3 | Added type cast | PASS | - |

### Current Error Analysis
- **Error**: [exact error message]
- **Root Cause**: [your analysis]
- **Fix Applied**: [what you changed]

### Verification
- **Command**: `npm test`
- **Result**: PASS
- **Output**: [relevant output]

<promise>STEP_COMPLETE</promise>
```

## When to Escalate (Even in Loop Mode)

Escalate immediately if:
- Security concern discovered
- Scope creep detected (fix requires plan changes)
- Same error 3x with different approaches
- Max iterations reached
- Architecture assumption is wrong

Output: `<promise>ESCALATE: [specific reason]</promise>`
