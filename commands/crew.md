# Agentic Development Workflow

You orchestrate the /crew workflow by running the orchestrator script for routing decisions and handling each action with full intelligence.

## Command: /crew $ARGS

### Initialize

Run: `python3 {__scripts_dir__}/crew_orchestrator.py init --host {__platform__} --args "$ARGS"`

The script returns JSON with `action` and routing details. Handle by action:

- **start** → Display task summary (ID, mode, optional agents), then run LLM Triage (if needed), Context Preparation, and Beads Integration (below), then enter Action Loop with `result.next`
- **resume** → Display `result.resume_state.display_summary`, enter Action Loop with `result.next`
- **status** → List `.tasks/` contents and show active workflows
- **config** → Call `config_get_effective()` and display configuration
- **ask** → Go to Single Agent Consultation with `result.agent` and `result.question`
- **error** → Show `result.errors` to user

**Resume detection:** When called with no args (or from a worktree), `init` auto-detects the active task and routes to **resume**. To skip resume detection and always start a fresh task, pass `--no-resume` to the orchestrator:

```
python3 {__scripts_dir__}/crew_orchestrator.py init --host {__platform__} --args "$ARGS" --no-resume
```

Or include `--no-resume` in the `/crew` arguments string:

```
/crew "Fix the authentication bug" --no-resume
```

**Resume health warnings:** When resuming, check `result.resume_state.recovery_needed` and `result.resume_state.stale_phase_warning`. If set, display the warning to the user before entering the action loop:
- `recovery_needed`: Missing output files for completed phases — suggest re-running affected phases
- `stale_phase_warning`: Current phase has been running >30 min with no output — may indicate a crash

### LLM Triage Fallback (if needed)

If `result.mode_confidence < config.llm_triage.confidence_threshold` (default 0.8) and `config.llm_triage.enabled` is true:

1. Spawn Haiku (1 turn) with model `config.llm_triage.model` (default: haiku):
   ```
   Agent(model: "haiku", prompt: "Classify this software task into exactly one mode. Respond with JSON only, no explanation.\n\nModes:\n- quick: trivial change — typo, rename, single-field addition, config tweak, one-file fix\n- standard: routine feature, refactor, multi-file change, non-trivial addition\n- thorough: security-sensitive, database migration, breaking API change, auth, crypto\n\nTask: <task_description>\nFiles: <files_affected or 'none'>\n\n{\"mode\": \"<quick|standard|thorough>\", \"reason\": \"<one sentence>\"}")
   ```
2. Parse the JSON response. If the LLM mode differs from `result.mode`:
   - Call `workflow_set_mode(mode=<llm_mode>, task_id=<task_id>)`
   - Re-call `crew_get_next_phase(task_id=<task_id>)` to update `result.next`
   - Log the override: `python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role system --content "LLM triage override: <old_mode> → <llm_mode> (reason: <reason>)" --type message --phase init`
3. If the LLM call fails or times out and `config.llm_triage.fallback_to_local` is true, keep the original detection result.

### Context Preparation (if configured)

If `config.gemini_research.enabled` is true:
1. Check prerequisites: `which repomix`, `which gemini`
2. Run file discovery, generate repomix config, run repomix
3. Run Gemini analysis, save outputs to task directory
4. If tools unavailable and `fallback_to_opus: true`, skip and continue

### Beads Integration (if configured)

If `result.beads_issue` is set:
1. `bd update <issue> --status=in_progress`
2. `bd comments add <issue> "Workflow <task_id> started"`

### Action Loop

Loop on the returned JSON action from the orchestrator:

#### action: "spawn_agent"

1. **Prompt resolution**: If `next.assembled_prompt` exists, use it directly as the agent prompt (skip Agent Prompt Composition below — the orchestrator already read all files, applied variable substitution, and included human guidance). Otherwise, fall back to reading `next.agent_prompt_path` and composing manually using Agent Prompt Composition (below).
2. If `next.beads_comment`, run: `bd comments add <issue> "<comment>"`
3. If `next.parallel_agents` is set (list of agent dicts with agent, model, max_turns, effort_level, agent_prompt_path, assembled_prompt):
   - Spawn ALL agents simultaneously: the primary agent in foreground + each parallel agent with `run_in_background: true`
   - Use `assembled_prompt` from each agent dict when available
   - Primary agent uses `model: next.model`; each parallel agent uses its own `model` from the list
   - Call `workflow_start_parallel_phase` with all phase names (primary + all parallel)
   - Wait for all parallel agents with TaskOutput
   - Call `workflow_complete_parallel_phase` for each, then `workflow_merge_parallel_results`
   - In `agent-done` calls, pass the actual model used for each agent
   Alternatively, if only `next.parallel_with` is set (single string, backwards compat):
   - Same as above but with just 2 agents (primary + one parallel agent using `next.parallel_agent_model`)
4. Otherwise spawn single agent:
   ```
   Task(subagent_type: "general-purpose", model: next.model, max_turns: next.max_turns, prompt: "<next.assembled_prompt OR composed prompt>")
   ```
5. Save agent output to `next.output_file` (if set) or `<next.variables.task_dir>/<agent>.md`
6. Run: `python3 {__scripts_dir__}/crew_orchestrator.py agent-done --task-id <id> --agent <agent> --output-file <path> [--input-tokens N --output-tokens N --model <next.model>]`
8. If `result.parse_result.unaddressed_concerns` is non-empty, display them to the user:
   ```
   **Unaddressed Concerns ({count}):**
   - [{severity}] ({source}): {description}
   ```
9. If `result.has_blocking_issues` and recommendation is REVISE → inform user, loop continues via `result.next`
10. Continue loop with `result.next`

#### action: "run_skill"

Custom phase that invokes a Claude Code skill:

1. Log: `python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role system --content "Running custom phase: <phase>" --type message --phase <phase>`
2. Run the skill: `Skill(skill: next.skill)`
3. Save skill output to `next.output_file`
4. Run: `python3 {__scripts_dir__}/crew_orchestrator.py custom-phase-done --task-id <id> --phase <phase> --output-file <output_file> [--writes-to-state] [--exit-code 0]`
5. If `result.action` is `"custom_phase_failed"` and `blocking` is true, inform the user and ask how to proceed (Retry / Skip / Abort)
6. Continue loop with `result.next`

#### action: "run_script"

Custom phase that runs a shell command:

1. Log: `python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role system --content "Running custom phase: <phase>" --type message --phase <phase>`
2. Run the command via Bash: `next.command` (with timeout: `next.timeout` seconds)
3. Capture stdout+stderr and exit code
4. Save output to `next.output_file`
5. Run: `python3 {__scripts_dir__}/crew_orchestrator.py custom-phase-done --task-id <id> --phase <phase> --output-file <output_file> --exit-code <code> [--writes-to-state] [--blocking]`
6. If `result.action` is `"custom_phase_failed"` and `blocking` is true, inform the user: "Custom phase '<phase>' failed (exit code <code>)." Ask: Retry / Skip / Abort.
7. Continue loop with `result.next`

#### action: "checkpoint"

Summarize the preceding agent's key findings. If `result.unaddressed_concerns` is non-empty, display each concern:
```
**Unaddressed Concerns ({concerns_count}):**
- [{severity}] ({source}): {description}
```
Then present to user:
```
AskUserQuestion: "Based on [Agent]'s analysis: [summary]. [N unaddressed concern(s) require attention.] How would you like to proceed?"
Options: Approve, Revise, Restart, Skip
```
After user responds, run: `python3 {__scripts_dir__}/crew_orchestrator.py checkpoint-done --task-id <id> --decision <decision> [--notes "..."] --question "<checkpoint summary that was presented>"`
The `--question` flag logs both the checkpoint question and response to `interactions.jsonl`.
Continue loop with `result.next`.

#### action: "implement_step" / "verify" / "retry" / "next_step" / "escalate"

Run: `python3 {__scripts_dir__}/crew_orchestrator.py impl-action --task-id <id> [--verified true/false] [--error "..."]`
- **implement_step**: Spawn implementer for `step_id`. If `loop_mode`, run verification after.
- **verify**: Run verification command, then call impl-action again with result.
- **retry**: Re-attempt with `should_try_different_approach` guidance. Use `known_solution` if available.
- **next_step**: Call `workflow_complete_step(step_id)`, then get next action.
- **checkpoint**: Present progress checkpoint to user.
- **escalate**: Pause and ask user for help. Show `reason`. Log the escalation and response:
  ```
  python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role agent --content "<reason>" --type escalation_question --agent implementer --phase implementer
  ```
  After user responds:
  ```
  python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role human --content "<user response>" --type escalation_response --phase implementer
  ```
- **complete**: Implementation done, continue to next phase. In standard mode: technical_writer → complete. In thorough mode: quality_guard + security_auditor (parallel) → technical_writer → complete.

**Note**: After the implementer returns "complete", the action loop continues with `result.next` which will be `spawn_agent` for the remaining phases (quality_guard + security_auditor in thorough, then technical_writer). The final `action: "complete"` only arrives after ALL phases including technical_writer are done. **Never commit before the technical-writer has run.**

#### action: "complete"

Run: `python3 {__scripts_dir__}/crew_orchestrator.py complete --task-id <id> [--files <comma-separated>]`
- Display cost summary as formatted table
- Suggest commit message to user
- Execute worktree commands based on `worktree_action` (prompt/auto/never)
- Execute beads commands to close/sync issues
- Handle Jira transitions from `result.jira_actions`: for each action with `"prompt"` → ask user, with `"execute"` → call `jira_issues_transition` (check `only_from` first via `jira_issues_get`)
- Ask human to approve commit. Record concern outcomes if any were raised.

#### action: "complete_with_async_docs"

The main workflow is complete, but the Technical Writer should run asynchronously in the background.

1. Run: `python3 {__scripts_dir__}/crew_orchestrator.py complete --task-id <id> [--files <comma-separated>]`
2. Display cost summary and suggest commit message (same as normal complete)
3. Handle worktree/beads/Jira actions (same as normal complete)
4. Ask human to approve commit. Record concern outcomes if any were raised.
5. After commit is done, spawn Technical Writer in background:
   - Read agent prompt from `result.async_docs.agent_prompt_path`
   - Compose prompt using Agent Prompt Composition (same as spawn_agent)
   - Include git diff context from `result.async_docs.git_diff_command`
   - Spawn: `Task(subagent_type: "general-purpose", model: result.async_docs.model, max_turns: result.async_docs.max_turns, prompt: "<composed prompt>", run_in_background: true)`
6. If `result.async_docs.notify_on_complete` is true, inform user: "Technical Writer is running in the background. Documentation updates will appear shortly."
7. If `result.async_docs.auto_commit_docs` is true, after TW completes: stage and commit doc changes with message "docs: update documentation for <task_id>"

### Single Agent Consultation

1. Load agent prompt from `~/{__platform_dir__}/agents/<agent>.md`
2. Gather context: `options.context` (files), `options.diff` (git diff), `options.plan` (plan file), `options.file` (question from file)
3. Spawn: `Task(subagent_type: "general-purpose", model: options.model or "opus", max_turns: 15, prompt: "<agent prompt + question + context>")`
4. Return response directly to user (no state saved)

## Agent Prompt Composition

When building prompts for agents, include:
1. **Agent prompt** from `~/{__platform_dir__}/agents/<agent>.md`
2. **Task description and context files**: Read ALL paths from `next.context_files` (these are absolute paths resolved by the MCP server — they work correctly in worktrees). Do NOT construct `.tasks/` paths yourself; always use the paths from the orchestrator response.
3. **Gemini analysis** (if available, extract relevant section)
4. **Knowledge base inventory** (list files, substitute `{knowledge_base}` path)
5. **Variable substitution**: Replace `{knowledge_base}`, `{task_directory}`, and `{task_dir}` with values from `next.variables`. Note: `{task_directory}` is an absolute path to the tasks directory (resolved through git for worktree support).
6. **Convention injection** (implementer + quality_guard): If `next.convention_files` exists, read each file and include under a `## Mandatory Conventions (from ai-context)` header in the prompt. These are actual convention files referenced by the Planner for the implementer to follow exactly.
7. **Human guidance trail**: Use `next.variables.task_dir` (absolute path) to read `interactions.jsonl` from the task directory. Include any entries with `role: "human"` and `type` in `["guidance", "correction", "new_requirement", "question"]` under a `## Human Guidance` header. This ensures agents have full context of user corrections and requirements given during the workflow. Skip if the file is empty or missing.

## Error Handling

If an agent fails or produces invalid output:
1. Retry once with clarified instructions
2. If still failing, escalate to human
3. Never silently continue past errors

## Interaction Logging

**All user input during active crew sessions is captured automatically** via Claude Code
hooks (`log-crew-interaction.py` fires on `UserPromptSubmit` and `Stop`). This provides:

- Complete conversation trail without relying on LLM logging
- Automatic classification of input type (guidance, correction, new_requirement, question)
- Context preserved across session recovery and compaction
- Visibility in crew-board's History view and agent prompt composition

The hook auto-classifies user input based on content signals:
- **Question**: Ends with `?` or starts with interrogative words (how, what, why...)
- **Correction**: Starts with negation/redirection (no, don't, actually, instead, fix...)
- **New requirement**: Contains additive signals (also, add, additionally, include...)
- **Guidance**: Default — general direction or instruction

Classified interactions are included in agent prompts via the Human Guidance Trail
(see Agent Prompt Composition), ensuring all agents see user corrections and requirements.

As a safety net, also log explicitly when the hook may not have captured context:

When the user provides ad-hoc guidance mid-workflow (outside of checkpoints):
```
python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role human --content "<user input>" --type guidance --phase <current_phase>
```

When the user provides a correction to agent output:
```
python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role human --content "<correction>" --type correction --phase <current_phase>
```

When the user adds a new requirement mid-workflow:
```
python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role human --content "<new requirement>" --type new_requirement --phase <current_phase>
```

When the user asks a clarifying question:
```
python3 {__scripts_dir__}/crew_orchestrator.py log-interaction --task-id <id> --role human --content "<question>" --type question --phase <current_phase>
```

## Output to User

Keep the user informed throughout:
- Show which agent is running and its purpose
- Summarize agent outputs concisely
- Clearly indicate checkpoints with options
- Show progress percentage during implementation
- Explain what happens next

Now, process the command arguments and begin the workflow:

Arguments: $ARGS
