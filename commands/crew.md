# Agentic Development Workflow

## Command: /crew $ARGS

### Step 1: Initialize

Run: `python3 {__scripts_dir__}/crew_orchestrator.py init --host {__platform__} --args "$ARGS"`

Handle `result.action`:

- **start** → Show task ID, mode, phases. If `result.mode_confidence < 0.8` and LLM triage is enabled, run LLM Triage (below). If `result.beads_issue`, run: `bd update <issue> --status=in_progress`. Enter Action Loop with `result.next`.
- **resume** → Follow `result.instructions` exactly (it contains the display text and spawn steps). **Do NOT add any steps, read files, search, call MCP tools, or explore.**
- **status** → List `.tasks/` contents.
- **config** → Call `config_get_effective()` and display.
- **ask** → Load `~/{__platform_dir__}/agents/<agent>.md`, spawn agent with question + context.
- **error** → Show `result.errors`.

To force a new task: `/crew "description" --no-resume`

### Step 2: Action Loop

Loop on `result.next` from the orchestrator. **If `next.instructions` exists, follow it literally — it contains the exact steps to execute. Do not add, skip, or modify any steps.**

#### spawn_agent

**If `next.instructions` exists**: follow it exactly (spawn → save → agent-done → continue).

**Otherwise**: Spawn agent using `next.assembled_prompt`:
   ```
   Task(subagent_type: "general-purpose", model: next.model, max_turns: next.max_turns, prompt: next.assembled_prompt)
   ```
   If `assembled_prompt` is also absent (legacy), fall back: read `next.agent_prompt_path`, `next.context_files`, apply `next.variables`.
2. **Parallel agents**: If `next.parallel_agents` is set, spawn all simultaneously (primary in foreground, others with `run_in_background: true`). Use each agent's own `assembled_prompt`, `model`, `max_turns`. Call `workflow_start_parallel_phase`, wait with TaskOutput, call `workflow_complete_parallel_phase` for each, then `workflow_merge_parallel_results`.
3. Save output to `next.output_file`
4. Run: `python3 {__scripts_dir__}/crew_orchestrator.py agent-done --task-id <id> --agent <agent> --output-file <path> [--model <next.model>]`
5. If `result.parse_result.unaddressed_concerns` is non-empty, show them.
6. Continue loop with `result.next`

#### run_skill

1. Run: `Skill(skill: next.skill)`
2. Save output to `next.output_file`
3. Run: `python3 {__scripts_dir__}/crew_orchestrator.py custom-phase-done --task-id <id> --phase <phase> --output-file <path> [--exit-code 0]`
4. If failed and blocking → ask user: Retry / Skip / Abort
5. Continue with `result.next`

#### run_script

1. Run: `Bash(next.command)` with timeout `next.timeout`
2. Save output to `next.output_file`
3. Run: `python3 {__scripts_dir__}/crew_orchestrator.py custom-phase-done --task-id <id> --phase <phase> --output-file <path> --exit-code <code>`
4. If failed and blocking → ask user: Retry / Skip / Abort
5. Continue with `result.next`

#### checkpoint

1. Show `result.question.text` and any unaddressed concerns
2. Ask user: Approve / Revise / Skip
3. Run: `python3 {__scripts_dir__}/crew_orchestrator.py checkpoint-done --task-id <id> --decision <decision> --question "<question>"`
4. Continue with `result.next`

#### implement_step / verify / retry / next_step / escalate

Run: `python3 {__scripts_dir__}/crew_orchestrator.py impl-action --task-id <id> [--verified true/false] [--error "..."]`
- **implement_step**: Spawn implementer for `step_id`
- **verify**: Run verification, call impl-action with result
- **retry**: Re-attempt with `should_try_different_approach`
- **escalate**: Show `reason`, ask user for help, log response
- **complete**: Continue with `result.next` (remaining phases follow)

#### complete / complete_with_async_docs

1. Run: `python3 {__scripts_dir__}/crew_orchestrator.py complete --task-id <id> [--files <files>]`
2. Show cost summary, suggest commit message
3. Handle worktree/beads/Jira actions from result
4. If `complete_with_async_docs`: spawn Technical Writer in background with `result.async_docs.assembled_prompt`

### LLM Triage (only when needed)

If `result.mode_confidence < config.llm_triage.confidence_threshold`:
1. Spawn Haiku: `Agent(model: "haiku", prompt: "Classify task into quick/standard/thorough. JSON only.\nTask: <description>\n{\"mode\": \"...\", \"reason\": \"...\"}")`
2. If mode differs: `workflow_set_mode(mode)`, re-call `crew_get_next_phase`
3. On failure: keep original detection

### Error Handling

1. Retry once with clarified instructions
2. If still failing, escalate to human

Now, process the command arguments and begin:

Arguments: $ARGS
