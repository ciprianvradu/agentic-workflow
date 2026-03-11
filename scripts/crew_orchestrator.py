#!/usr/bin/env python3
"""
Crew Orchestrator — CLI wrapper for deterministic workflow routing.

Batches multiple MCP tool calls into single instant decisions, replacing
LLM interpretation of procedural routing logic. Each subcommand returns
structured JSON that tells the crew.md orchestrator exactly what to do next.

Usage:
    python3 scripts/crew_orchestrator.py init --args "Fix typo in README --mode minimal"
    python3 scripts/crew_orchestrator.py next --task-id TASK_001
    python3 scripts/crew_orchestrator.py agent-done --task-id TASK_001 --agent architect --output-file .tasks/TASK_001/architect.md
    python3 scripts/crew_orchestrator.py checkpoint-done --task-id TASK_001 --decision approve
    python3 scripts/crew_orchestrator.py impl-action --task-id TASK_001
    python3 scripts/crew_orchestrator.py complete --task-id TASK_001
    python3 scripts/crew_orchestrator.py resume --task-id TASK_001
"""

import argparse
import json
import os
import sys
from datetime import datetime
from pathlib import Path

# Add MCP server package to path so we can import directly
_SCRIPT_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _SCRIPT_DIR.parent
_MCP_PKG = _REPO_ROOT / "mcp" / "agentic-workflow-server"
if str(_MCP_PKG) not in sys.path:
    sys.path.insert(0, str(_MCP_PKG))

try:
    from agentic_workflow_server.orchestration_tools import (
        crew_parse_args,
        crew_init_task,
        crew_get_next_phase,
        crew_parse_agent_output,
        crew_get_implementation_action,
        crew_format_completion,
        crew_get_resume_state,
    )
    from agentic_workflow_server.state_tools import (
        workflow_complete_phase,
        workflow_add_human_decision,
        workflow_record_cost,
        workflow_transition,
        workflow_log_interaction,
        find_task_dir,
        get_tasks_dir,
        _load_state,
        _save_state,
        _detect_worktree_task_id,
    )
    from agentic_workflow_server.config_tools import config_get_effective
except ImportError as e:
    print(
        f"Error: Could not import agentic-workflow-server package.\n"
        f"  Looked in: {_MCP_PKG}\n"
        f"  Import error: {e}\n"
        f"  Fix: Run 'pip install -e {_MCP_PKG}' or ensure the package is installed.",
        file=sys.stderr,
    )
    sys.exit(1)


ACTIVE_TASK_FILE = ".active_task"


def _write_active_task(task_id: str) -> None:
    """Write the active task ID to .tasks/.active_task for session isolation.

    Uses FileLock to prevent race conditions between concurrent sessions.
    Falls back to atomic write (temp file + os.replace) if filelock is not installed.
    """
    tasks_dir = get_tasks_dir()
    tasks_dir.mkdir(parents=True, exist_ok=True)
    active_file = tasks_dir / ACTIVE_TASK_FILE
    lock_file = tasks_dir / f"{ACTIVE_TASK_FILE}.lock"
    try:
        from filelock import FileLock
        with FileLock(str(lock_file)):
            active_file.write_text(task_id + "\n")
    except ImportError:
        import logging
        logging.warning(
            "filelock not installed — concurrent session safety disabled. "
            "Install with: pip install filelock"
        )
        import tempfile
        fd, tmp_path = tempfile.mkstemp(
            dir=str(tasks_dir), prefix=".active_task.tmp."
        )
        closed = False
        try:
            os.write(fd, (task_id + "\n").encode())
            os.close(fd)
            closed = True
            os.replace(tmp_path, str(active_file))
        except Exception:
            if not closed:
                os.close(fd)
            if os.path.exists(tmp_path):
                os.unlink(tmp_path)
            raise


def _remove_active_task(task_id: str) -> None:
    """Remove .tasks/.active_task if it matches the given task ID.

    Uses FileLock and atomic check-then-delete to prevent TOCTOU races.
    Falls back to best-effort unlocked operation if filelock is not installed.
    """
    tasks_dir = get_tasks_dir()
    active_file = tasks_dir / ACTIVE_TASK_FILE
    lock_file = tasks_dir / f"{ACTIVE_TASK_FILE}.lock"
    try:
        from filelock import FileLock
        with FileLock(str(lock_file)):
            if active_file.exists():
                current = active_file.read_text().strip()
                if current == task_id:
                    active_file.unlink()
    except ImportError:
        try:
            current = active_file.read_text().strip()
            if current == task_id:
                active_file.unlink()
        except (OSError, FileNotFoundError):
            pass
    except OSError:
        pass


def _read_active_task() -> str | None:
    """Read the active task ID from .tasks/.active_task.

    Returns None if the file doesn't exist, is empty, or points to a
    completed/nonexistent task (stale marker from a crashed session).
    """
    tasks_dir = get_tasks_dir()
    active_file = tasks_dir / ACTIVE_TASK_FILE
    if not active_file.exists():
        return None
    try:
        task_id = active_file.read_text().strip()
        if not task_id:
            return None
        # Validate the task still exists and isn't completed
        task_dir = tasks_dir / task_id
        state_file = task_dir / "state.json"
        if not state_file.exists():
            return None
        with open(state_file) as f:
            state = json.load(f)
        if state.get("status") == "completed":
            # Stale marker from a previous session — clean it up
            try:
                active_file.unlink()
            except OSError:
                pass
            return None
        return task_id
    except (OSError, json.JSONDecodeError):
        return None


def _output(data: dict) -> None:
    """Print JSON to stdout."""
    json.dump(data, sys.stdout, indent=2)
    sys.stdout.write("\n")


def cmd_init(args: argparse.Namespace) -> None:
    """Parse args, route action, init task, return first action.

    Batches: crew_parse_args → crew_init_task → crew_get_next_phase
    """
    parsed = crew_parse_args(args.args)

    if parsed.get("errors"):
        # No args provided — try to auto-resume from context
        # 1. Worktree detection: match cwd against task worktree metadata
        task_id = _detect_worktree_task_id()
        # 2. .crew-resume file: directory-specific (written when worktree created)
        if not task_id:
            resume_file = Path.cwd() / ".crew-resume"
            if resume_file.exists():
                try:
                    for line in resume_file.read_text().splitlines():
                        if line.startswith("task_id:"):
                            task_id = line.split(":", 1)[1].strip()
                            break
                except OSError:
                    pass
        # 3. .active_task file: global fallback (last session to write wins)
        if not task_id:
            task_id = _read_active_task()
        if task_id:
            # Auto-resume the found task
            parsed = {"action": "resume", "task_id": task_id, "options": {}, "errors": []}
        else:
            _output({"error": True, "errors": parsed["errors"], "action": parsed["action"]})
            return

    action = parsed["action"]

    # Non-start actions: return immediately with routing info
    if action in ("status", "config", "proceed"):
        _output({"action": action})
        return

    if action == "resume":
        task_id = parsed.get("task_id")
        if not task_id:
            _output({"error": True, "errors": ["No task ID provided for resume"]})
            return
        resume = crew_get_resume_state(task_id=task_id)
        if resume.get("error"):
            _output({"error": True, "errors": [resume["error"]]})
            return
        workflow_log_interaction(
            role="system",
            content="Session resumed",
            interaction_type="message",
            phase="init",
            task_id=task_id,
            metadata={"ai_host": args.host},
        )
        next_action = crew_get_next_phase(task_id=task_id)
        _write_active_task(task_id)
        _output({
            "action": "resume",
            "resume_state": resume,
            "next": next_action,
        })
        return

    if action == "ask":
        _output({
            "action": "ask",
            "agent": parsed.get("agent"),
            "question": parsed.get("task_description", ""),
            "options": parsed.get("options", {}),
        })
        return

    # Action is "start" — full initialization
    task_description = parsed["task_description"]
    options = parsed.get("options", {})
    options["ai_host"] = args.host

    # Resolve task description from file if specified
    if options.get("task_file"):
        raw_path = options["task_file"]
        # Strip leading @ (used as file indicator in CLI)
        if raw_path.startswith("@"):
            raw_path = raw_path[1:]
        task_file = Path(raw_path)
        if task_file.exists():
            task_description = task_file.read_text().strip()
        else:
            _output({"error": True, "errors": [f"Task file not found: {options['task_file']}"]})
            return

    if not task_description:
        _output({"error": True, "errors": ["No task description provided"]})
        return

    init_result = crew_init_task(
        task_description=task_description,
        options=options,
    )

    if not init_result.get("success"):
        _output({"error": True, "errors": [init_result.get("error", "Initialization failed")]})
        return

    task_id = init_result["task_id"]

    # Get first phase action
    next_action = crew_get_next_phase(task_id=task_id)

    # Pre-transition to first phase if it differs from default
    first_phase = next_action.get("agent") or next_action.get("phase")
    if first_phase and next_action.get("action") in ("spawn_agent", "run_skill", "run_script"):
        task_dir_path = find_task_dir(task_id)
        if task_dir_path:
            current_state = _load_state(task_dir_path)
            if current_state.get("phase") != first_phase:
                workflow_transition(to_phase=first_phase, task_id=task_id)

    _write_active_task(task_id)

    _output({
        "action": "start",
        "task_id": task_id,
        "task_dir": init_result["task_dir"],
        "mode": init_result["mode"],
        "optional_agents": init_result.get("optional_agents", []),
        "kb_inventory": init_result.get("kb_inventory", {}),
        "beads_issue": init_result.get("beads_issue"),
        "config": init_result.get("config", {}),
        "next": next_action,
    })


def cmd_next(args: argparse.Namespace) -> None:
    """Get next phase/action.

    Wraps: crew_get_next_phase
    """
    result = crew_get_next_phase(task_id=args.task_id)
    _output(result)


def cmd_agent_done(args: argparse.Namespace) -> None:
    """Parse output, complete phase, record cost, get next action.

    Batches: crew_parse_agent_output → workflow_complete_phase →
             workflow_record_cost → crew_get_next_phase
    """
    task_id = args.task_id
    agent = args.agent

    # Read output from file if provided
    output_text = ""
    if args.output_file:
        output_path = Path(args.output_file)
        if output_path.exists():
            output_text = output_path.read_text()

    # 1. Parse agent output
    parse_result = crew_parse_agent_output(
        agent=agent,
        output_text=output_text,
        task_id=task_id,
    )

    # 2. Complete phase — ensure the agent that just finished is tracked
    #    workflow_complete_phase() only completes state["phase"], which may differ
    #    from the agent (e.g., custom/specialized agents like accessibility_reviewer
    #    may run while state["phase"] still points to a different phase).
    task_dir = find_task_dir(task_id)
    if task_dir:
        state = _load_state(task_dir)
        phases_completed = state.get("phases_completed", [])
        agent_normalized = agent.lower().replace("-", "_")
        if agent_normalized not in [p.lower().replace("-", "_") for p in phases_completed]:
            phases_completed.append(agent)
            state["phases_completed"] = phases_completed
            _save_state(task_dir, state)
    complete_result = workflow_complete_phase(task_id=task_id)

    # 3. Record cost if provided
    cost_recorded = False
    if args.input_tokens and args.output_tokens:
        workflow_record_cost(
            agent=agent,
            model=args.model or "opus",
            input_tokens=args.input_tokens,
            output_tokens=args.output_tokens,
            duration_seconds=args.duration or 0,
            task_id=task_id,
        )
        cost_recorded = True

    # 4. Handle REVISE loopback — if reviewer recommends REVISE, route back
    #    to planner (or developer for legacy tasks) instead of advancing
    recommendation = parse_result.get("extracted", {}).get("recommendation", "")
    if agent == "reviewer" and recommendation == "REVISE":
        # Remove reviewer from phases_completed so it runs again after planner
        if task_dir:
            state = _load_state(task_dir)
            phases_completed = state.get("phases_completed", [])
            phases_completed = [
                p for p in phases_completed
                if p.lower().replace("-", "_") != "reviewer"
            ]
            state["phases_completed"] = phases_completed
            _save_state(task_dir, state)
        # Route back to planner (or developer for in-flight legacy tasks)
        mode_phases = state.get("workflow_mode", {}).get("phases", []) if task_dir else []
        revise_target = "planner" if "planner" in mode_phases else "developer"
        workflow_transition(to_phase=revise_target, task_id=task_id)
        next_action = crew_get_next_phase(task_id=task_id)
    else:
        # 4b. Normal forward progression
        next_action = crew_get_next_phase(task_id=task_id)

    # 5. Pre-transition to next phase
    transition_result = None
    next_phase = next_action.get("agent") or next_action.get("phase")
    if next_phase and next_action.get("action") in ("spawn_agent", "run_skill", "run_script"):
        transition_result = workflow_transition(
            to_phase=next_phase,
            task_id=task_id,
        )

    _output({
        "action": "agent_done",
        "parse_result": parse_result,
        "phase_completed": complete_result.get("success", False),
        "cost_recorded": cost_recorded,
        "has_blocking_issues": parse_result.get("has_blocking_issues", False),
        "transitioned_to": transition_result.get("to_phase") if transition_result else None,
        "next": next_action,
    })


def cmd_custom_phase_done(args: argparse.Namespace) -> None:
    """Handle completion of a custom phase (skill or script).

    Batches: save output → optionally write to state → complete phase → get next
    """
    task_id = args.task_id
    phase_name = args.phase

    # Read output if provided
    output_text = ""
    if args.output_file:
        output_path = Path(args.output_file)
        if output_path.exists():
            output_text = output_path.read_text()

    # Save output to task dir
    task_dir = find_task_dir(task_id)
    if task_dir and output_text:
        output_file = task_dir / f"{phase_name}.md"
        output_file.write_text(output_text)

    # Write to state if configured
    if args.writes_to_state and task_dir:
        state = _load_state(task_dir)
        custom_results = state.get("custom_phase_results", {})
        custom_results[phase_name] = {
            "output": output_text[:5000],  # Truncate to avoid bloating state
            "exit_code": args.exit_code or 0,
            "completed_at": datetime.now().isoformat(),
        }
        state["custom_phase_results"] = custom_results
        _save_state(task_dir, state)

    # Handle failure for blocking phases
    if args.exit_code and args.exit_code != 0 and args.blocking:
        _output({
            "action": "custom_phase_failed",
            "phase": phase_name,
            "exit_code": args.exit_code,
            "blocking": True,
            "output": output_text[:2000],
            "task_id": task_id,
        })
        return

    # Complete phase — ensure we're on the right phase before completing
    task_dir_check = find_task_dir(task_id)
    if task_dir_check:
        current_state = _load_state(task_dir_check)
        if current_state.get("phase") != phase_name:
            workflow_transition(to_phase=phase_name, task_id=task_id)
    complete_result = workflow_complete_phase(task_id=task_id)

    # Get next action
    next_action = crew_get_next_phase(task_id=task_id)

    # Pre-transition to next phase
    transition_result = None
    next_phase = next_action.get("agent") or next_action.get("phase")
    if next_phase and next_action.get("action") in ("spawn_agent", "run_skill", "run_script"):
        transition_result = workflow_transition(
            to_phase=next_phase,
            task_id=task_id,
        )

    _output({
        "action": "custom_phase_done",
        "phase": phase_name,
        "phase_completed": complete_result.get("success", False),
        "transitioned_to": transition_result.get("to_phase") if transition_result else None,
        "next": next_action,
    })


def cmd_checkpoint_done(args: argparse.Namespace) -> None:
    """Record decision, log interactions, get next action.

    Batches: log interactions → workflow_add_human_decision → crew_get_next_phase
    """
    task_id = args.task_id
    decision = args.decision

    # Determine checkpoint name from current state
    task_dir = find_task_dir(task_id)
    checkpoint_name = "checkpoint"
    phase = "unknown"
    if task_dir:
        state = _load_state(task_dir)
        phase = state.get("phase", "unknown")
        checkpoint_name = f"after_{phase}"

    # 0. Log checkpoint question and response to interactions.jsonl
    if args.question:
        workflow_log_interaction(
            role="agent",
            content=args.question,
            interaction_type="checkpoint_question",
            agent="orchestrator",
            phase=phase,
            task_id=task_id,
        )

    response_content = decision
    if args.notes:
        response_content = f"{decision}: {args.notes}"
    workflow_log_interaction(
        role="human",
        content=response_content,
        interaction_type="checkpoint_response",
        agent="orchestrator",
        phase=phase,
        task_id=task_id,
    )

    # 1. Record decision
    workflow_add_human_decision(
        checkpoint=checkpoint_name,
        decision=decision,
        notes=args.notes or "",
        task_id=task_id,
    )

    # 2. Ensure phase is marked complete for approve/skip
    if decision in ("approve", "skip"):
        workflow_complete_phase(task_id=task_id)

    # 3. Handle revise — transition back to planner (or developer for legacy tasks)
    if decision == "revise":
        mode_phases = state.get("workflow_mode", {}).get("phases", []) if task_dir else []
        revise_target = "planner" if "planner" in mode_phases else "developer"
        workflow_transition(to_phase=revise_target, task_id=task_id)

    # 4. Handle restart — transition back to planner (or architect for legacy tasks)
    if decision == "restart":
        mode_phases = state.get("workflow_mode", {}).get("phases", []) if task_dir else []
        restart_target = "planner" if "planner" in mode_phases else "architect"
        workflow_transition(to_phase=restart_target, task_id=task_id)

    # 5. Get next action
    next_action = crew_get_next_phase(task_id=task_id)

    # 6. Pre-transition to next phase
    next_phase = next_action.get("agent") or next_action.get("phase")
    if next_phase and next_action.get("action") in ("spawn_agent", "run_skill", "run_script"):
        workflow_transition(to_phase=next_phase, task_id=task_id)

    _output({
        "action": "checkpoint_done",
        "decision": decision,
        "checkpoint": checkpoint_name,
        "next": next_action,
    })


def cmd_impl_action(args: argparse.Namespace) -> None:
    """Implementation loop step.

    Wraps: crew_get_implementation_action
    """
    verified = None
    if args.verified is not None:
        verified = args.verified.lower() in ("true", "1", "yes")

    result = crew_get_implementation_action(
        task_id=args.task_id,
        last_verification_passed=verified,
        last_error_output=args.error or None,
    )
    _output(result)


def cmd_complete(args: argparse.Namespace) -> None:
    """Format completion + Jira + beads.

    Batches: crew_format_completion + crew_jira_transition
    """
    task_id = args.task_id

    # Get files changed from git if not provided
    files_changed = []
    if args.files:
        files_changed = args.files.split(",")

    completion = crew_format_completion(
        task_id=task_id,
        files_changed=files_changed,
    )

    # Resolve Jira transitions if applicable
    jira_actions = []
    task_dir = find_task_dir(task_id)
    if task_dir:
        state = _load_state(task_dir)
        linked_issue = state.get("linked_issue") or state.get("jira_issue")
        if linked_issue:
            try:
                from agentic_workflow_server.orchestration_tools import crew_jira_transition
                for hook in ("on_complete", "on_cleanup"):
                    jira_result = crew_jira_transition(
                        task_id=task_id,
                        hook_name=hook,
                        issue_key=linked_issue,
                    )
                    if jira_result.get("action") != "skip":
                        jira_actions.append(jira_result)
            except (ImportError, Exception):
                pass  # Jira integration is non-blocking

    # Mark workflow as complete in state.json
    if task_dir:
        state = _load_state(task_dir)
        state["status"] = "completed"
        state["completed_at"] = datetime.now().isoformat()
        if files_changed:
            state["files_changed"] = files_changed
        _save_state(task_dir, state)

    # Remove session-local active task marker
    _remove_active_task(task_id)

    completion["jira_actions"] = jira_actions

    # Check if async docs were signaled
    if task_dir:
        state_reloaded = _load_state(task_dir)
        if state_reloaded.get("async_docs_pending"):
            completion["async_docs_pending"] = True

    _output(completion)


def cmd_log_interaction(args: argparse.Namespace) -> None:
    """Log an interaction entry.

    Wraps: workflow_log_interaction
    """
    metadata = None
    if args.metadata:
        try:
            metadata = json.loads(args.metadata)
        except json.JSONDecodeError:
            pass
    result = workflow_log_interaction(
        role=args.role,
        content=args.content,
        interaction_type=args.type,
        agent=args.agent or "",
        phase=args.phase or "",
        task_id=args.task_id,
        metadata=metadata,
    )
    _output(result)


def cmd_resume(args: argparse.Namespace) -> None:
    """Load resume context.

    Batches: crew_get_resume_state → crew_get_next_phase
    """
    task_id = args.task_id

    resume = crew_get_resume_state(task_id=task_id)
    if resume.get("error"):
        _output({"error": True, "errors": [resume["error"]]})
        return

    workflow_log_interaction(
        role="system",
        content="Session resumed",
        interaction_type="message",
        phase="init",
        task_id=task_id,
        metadata={"ai_host": args.host},
    )

    next_action = crew_get_next_phase(task_id=task_id)
    _write_active_task(task_id)

    _output({
        "action": "resume",
        "resume_state": resume,
        "next": next_action,
    })


def cmd_learn(args: argparse.Namespace) -> None:
    """Run Technical Writer standalone against recent git changes.

    No full pipeline state is created. Parses args, resolves diff range,
    and returns agent spawn instructions.
    """
    parsed = crew_parse_args(f"learn {args.args}")

    if parsed.get("errors"):
        _output({"error": True, "errors": parsed["errors"]})
        return

    options = parsed.get("options", {})

    # Resolve the git diff command based on options
    if options.get("diff"):
        diff_command = f"git diff {options['diff']}"
        log_command = f"git log --oneline {options['diff']}"
    elif options.get("since"):
        since = options["since"]
        diff_command = f"git diff HEAD@{{'{since} ago'}}"
        log_command = f"git log --oneline --since='{since}'"
    elif options.get("task"):
        # Use task's worktree base branch for diff
        task_id = options["task"]
        task_dir = find_task_dir(task_id)
        if task_dir:
            state = _load_state(task_dir)
            wt = state.get("worktree", {})
            base_branch = wt.get("base_branch", "main")
            diff_command = f"git diff {base_branch}...HEAD"
            log_command = f"git log --oneline {base_branch}...HEAD"
        else:
            diff_command = "git diff HEAD~1"
            log_command = "git log --oneline -1"
    else:
        # Default: last commit
        diff_command = "git diff HEAD~1"
        log_command = "git log --oneline -1"

    # Resolve model
    effective = config_get_effective()
    config = effective.get("config", {})
    models_config = config.get("models", {})
    default_model = models_config.get("default", "opus")
    model = options.get("model") or models_config.get("standard", {}).get("technical_writer") or models_config.get("technical_writer") or default_model

    # Get max turns
    subagent_limits = config.get("subagent_limits", {}).get("max_turns", {})
    max_turns = subagent_limits.get("documentation_agents", 20)

    # Agent prompt path
    agents_dir = Path.home() / ".claude" / "agents"
    agent_prompt_path = str(agents_dir / "technical-writer.md")

    # Resolve task context if --task provided
    task_dir_str = None
    context_files = []
    if options.get("task"):
        td = find_task_dir(options["task"])
        if td:
            task_dir_str = str(td)
            for name in ["task.md", "implementer.md", "planner.md"]:
                f = td / name
                if f.exists():
                    context_files.append(str(f))

    _output({
        "action": "learn",
        "diff_command": diff_command,
        "log_command": log_command,
        "agent_prompt_path": agent_prompt_path,
        "model": model,
        "max_turns": max_turns,
        "auto_commit": options.get("auto_commit", False),
        "focus": parsed.get("task_description", ""),
        "task_dir": task_dir_str,
        "context_files": context_files,
    })


def _classify_error(e: Exception) -> dict:
    """Map exceptions to structured, actionable error messages."""
    msg = str(e)
    etype = type(e).__name__

    if isinstance(e, FileNotFoundError):
        if ".tasks" in msg or "tasks" in msg:
            return {"error": True, "errors": ["Task directory not found"],
                    "hint": "Run from repo root, or run '/crew <task>' to create one"}
        return {"error": True, "errors": [f"File not found: {msg}"],
                "hint": "Check that the file path exists and is accessible"}

    if isinstance(e, AttributeError) and "NoneType" in msg:
        return {"error": True, "errors": ["Task state could not be loaded"],
                "hint": "Verify task ID with: ls .tasks/"}

    if isinstance(e, json.JSONDecodeError):
        return {"error": True, "errors": ["Corrupted state file"],
                "hint": "Check state.json for syntax errors"}

    if isinstance(e, KeyError):
        return {"error": True, "errors": [f"Missing expected field: {e}"],
                "hint": "May indicate version mismatch. Reinstall with: bash install.sh"}

    if isinstance(e, PermissionError):
        return {"error": True, "errors": [f"Permission denied: {msg}"],
                "hint": "Check file permissions in .tasks/"}

    return {"error": True, "errors": [f"Unexpected error: {etype}: {msg}"],
            "hint": "Report at github.com/anthropics/agentic-workflow/issues"}


def main():
    parser = argparse.ArgumentParser(
        description="Crew Orchestrator — instant routing decisions for /crew workflow"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    # init
    p_init = subparsers.add_parser("init", help="Parse args, init task, get first action")
    p_init.add_argument("--args", required=True, help="Raw /crew arguments string")
    p_init.add_argument("--host", default="unknown", help="AI host platform identifier")

    # next
    p_next = subparsers.add_parser("next", help="Get next phase/action")
    p_next.add_argument("--task-id", required=True, help="Task identifier")

    # agent-done
    p_agent = subparsers.add_parser("agent-done", help="Parse output, complete phase, get next")
    p_agent.add_argument("--task-id", required=True, help="Task identifier")
    p_agent.add_argument("--agent", required=True, help="Agent name")
    p_agent.add_argument("--output-file", help="Path to agent output file")
    p_agent.add_argument("--input-tokens", type=int, help="Input tokens used")
    p_agent.add_argument("--output-tokens", type=int, help="Output tokens used")
    p_agent.add_argument("--model", default="opus", help="Model used")
    p_agent.add_argument("--duration", type=float, help="Duration in seconds")

    # custom-phase-done
    p_custom = subparsers.add_parser("custom-phase-done", help="Complete custom phase, get next")
    p_custom.add_argument("--task-id", required=True, help="Task identifier")
    p_custom.add_argument("--phase", required=True, help="Custom phase name")
    p_custom.add_argument("--output-file", help="Path to output file")
    p_custom.add_argument("--exit-code", type=int, default=0, help="Exit code (scripts)")
    p_custom.add_argument("--writes-to-state", action="store_true", help="Store output in state")
    p_custom.add_argument("--blocking", action=argparse.BooleanOptionalAction, default=True, help="Phase blocks on failure")

    # checkpoint-done
    p_ckpt = subparsers.add_parser("checkpoint-done", help="Record decision, get next")
    p_ckpt.add_argument("--task-id", required=True, help="Task identifier")
    p_ckpt.add_argument("--decision", required=True, choices=["approve", "revise", "restart", "skip"],
                        type=str.lower)
    p_ckpt.add_argument("--notes", help="Optional decision notes")
    p_ckpt.add_argument("--question", help="Checkpoint question that was presented to user")

    # impl-action
    p_impl = subparsers.add_parser("impl-action", help="Implementation loop step")
    p_impl.add_argument("--task-id", required=True, help="Task identifier")
    p_impl.add_argument("--verified", help="Last verification result (true/false)")
    p_impl.add_argument("--error", help="Last error output")

    # complete
    p_complete = subparsers.add_parser("complete", help="Format completion + Jira + beads")
    p_complete.add_argument("--task-id", required=True, help="Task identifier")
    p_complete.add_argument("--files", help="Comma-separated list of changed files")

    # log-interaction
    p_log = subparsers.add_parser("log-interaction", help="Log an interaction entry")
    p_log.add_argument("--task-id", required=True, help="Task identifier")
    p_log.add_argument("--role", required=True, choices=["human", "agent", "system"])
    p_log.add_argument("--content", required=True, help="Message content")
    p_log.add_argument("--type", default="message",
                       choices=["message", "checkpoint_question", "checkpoint_response",
                                "guidance", "escalation_question", "escalation_response"])
    p_log.add_argument("--agent", help="Agent context (e.g., orchestrator, architect)")
    p_log.add_argument("--phase", help="Current workflow phase")
    p_log.add_argument("--metadata", help="JSON metadata string")

    # resume
    p_resume = subparsers.add_parser("resume", help="Load resume context")
    p_resume.add_argument("--task-id", required=True, help="Task identifier")
    p_resume.add_argument("--host", default="unknown", help="AI host platform identifier")

    # learn
    p_learn = subparsers.add_parser("learn", help="Run Technical Writer standalone")
    p_learn.add_argument("--args", required=True, help="Raw /crew learn arguments string")
    p_learn.add_argument("--host", default="unknown", help="AI host platform identifier")

    args = parser.parse_args()

    commands = {
        "init": cmd_init,
        "next": cmd_next,
        "agent-done": cmd_agent_done,
        "custom-phase-done": cmd_custom_phase_done,
        "checkpoint-done": cmd_checkpoint_done,
        "impl-action": cmd_impl_action,
        "complete": cmd_complete,
        "log-interaction": cmd_log_interaction,
        "resume": cmd_resume,
        "learn": cmd_learn,
    }

    try:
        commands[args.command](args)
    except Exception as e:
        _output(_classify_error(e))
        sys.exit(1)


if __name__ == "__main__":
    main()
