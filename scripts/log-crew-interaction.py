#!/usr/bin/env python3
"""
UserPromptSubmit + Stop Hook: Log All Crew Interactions to interactions.jsonl

Fires on every user prompt submission and every Claude stop event during a
crew workflow session. Captures the complete conversation trail (human input
AND agent responses) deterministically via hooks — no LLM logging needed.

- UserPromptSubmit: logs {role: "human", type: "guidance", content: <prompt>}
- Stop: logs {role: "agent", type: "message", content: <transcript summary>}

Only activates when a crew workflow is active in this session.
Always exits 0 — never blocks the user.

Usage in ~/.claude/settings.json:
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "python3 ~/.claude/scripts/log-crew-interaction.py"
      }]
    }],
    "Stop": [{
      "hooks": [{
        "type": "command",
        "command": "python3 ~/.claude/scripts/log-crew-interaction.py"
      }]
    }]
  }
}
"""

import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

# Fast-path: bail immediately if no .tasks/ directory exists (no crew workflow possible)
if not os.path.isdir(os.path.join(os.getcwd(), ".tasks")):
    sys.exit(0)

sys.path.insert(0, str(Path(__file__).parent))
from workflow_state import _resolve_tasks_dir, _detect_worktree_task_id

# Add MCP server to sys.path so we can import state_tools directly
try:
    _mcp_server_path = Path(__file__).parent.parent / "mcp" / "agentic-workflow-server"
    if _mcp_server_path.exists():
        sys.path.insert(0, str(_mcp_server_path))
except Exception:
    pass


# Prompts that are already logged by the orchestrator — skip to avoid duplication
_SKIP_PREFIXES = ("/crew", "/crew-resume")
# Commands that are part of internal workflow machinery — skip these too
_SKIP_PATTERNS = [
    "python3",          # orchestrator calls
    "crew_orchestrator",
    "checkpoint-done",
    "log-interaction",
]


def _find_session_task() -> str | None:
    """Find the active crew task directory for this session.

    Uses the same priority logic as check-workflow-complete.py:
    1. Worktree detection (most reliable — tied to cwd)
    2. .tasks/.active_task file
    3. Scan for recently-active incomplete tasks (last 2h)
    """
    tasks_dir = _resolve_tasks_dir()

    # 1. Worktree detection
    wt_task_id = _detect_worktree_task_id(tasks_dir)
    if wt_task_id:
        task_dir = tasks_dir / wt_task_id
        if task_dir.exists():
            return str(task_dir)

    # 2. .active_task file
    active_file = tasks_dir / ".active_task"
    if active_file.exists():
        try:
            task_id = active_file.read_text().strip()
            if task_id:
                task_dir = tasks_dir / task_id
                state_file = task_dir / "state.json"
                if state_file.exists():
                    try:
                        with open(state_file) as f:
                            state = json.load(f)
                        if state.get("status", "") not in ("completed", "complete"):
                            return str(task_dir)
                    except (json.JSONDecodeError, OSError):
                        pass
        except OSError:
            pass

    # 3. Scan for recently active incomplete tasks
    if tasks_dir.exists():
        import time
        candidates = []
        try:
            for entry in tasks_dir.iterdir():
                if not entry.is_dir() or entry.name.startswith("."):
                    continue
                state_file = entry / "state.json"
                if not state_file.exists():
                    continue
                try:
                    mtime = state_file.stat().st_mtime
                    if time.time() - mtime > 7200:
                        continue
                    with open(state_file) as f:
                        state = json.load(f)
                    status = state.get("status", "")
                    phase = state.get("phase", "")
                    if status in ("completed", "complete") or not phase:
                        continue
                    candidates.append((mtime, str(entry)))
                except (json.JSONDecodeError, OSError):
                    continue
        except OSError:
            pass
        if candidates:
            candidates.sort(reverse=True)
            return candidates[0][1]

    return None


def _get_phase(task_dir: str) -> str:
    """Read the current phase from state.json."""
    try:
        state_file = Path(task_dir) / "state.json"
        with open(state_file) as f:
            state = json.load(f)
        return state.get("phase") or ""
    except (OSError, json.JSONDecodeError):
        return ""


def _should_skip_prompt(prompt: str) -> bool:
    """Return True if this prompt should not be logged (already captured elsewhere)."""
    stripped = prompt.strip()
    for prefix in _SKIP_PREFIXES:
        if stripped.startswith(prefix):
            return True
    return False


def _append_interaction(task_dir: str, entry: dict) -> None:
    """Append a JSONL entry to interactions.jsonl using a file lock for safety."""
    interactions_path = Path(task_dir) / "interactions.jsonl"

    # Try filelock if available; fall back to plain write (still safe for
    # single-session use, which is the common case here)
    try:
        from filelock import FileLock
        lock = FileLock(str(interactions_path) + ".lock", timeout=5)
        with lock:
            with open(interactions_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry) + "\n")
    except ImportError:
        # filelock not available — write directly (acceptable for hook context)
        with open(interactions_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(entry) + "\n")
    except Exception:
        # Any lock/write failure — silently ignore to never block the user
        pass


def _classify_input(prompt: str) -> str:
    """Classify user input into an interaction type based on content signals.

    Returns one of: guidance, correction, new_requirement, question.
    """
    lower = prompt.lower().strip()

    # Question: ends with ? or starts with interrogative words
    if lower.endswith("?"):
        return "question"
    question_starters = (
        "how ", "what ", "why ", "where ", "when ", "which ",
        "can you ", "could you ", "is there ", "are there ",
        "do we ", "does ", "did ", "should ",
    )
    if any(lower.startswith(s) for s in question_starters):
        return "question"

    # Correction: signals of "no, do this instead"
    correction_starters = (
        "no,", "no ", "don't ", "dont ", "do not ", "stop ",
        "actually,", "actually ", "instead,", "instead ",
        "not that", "wrong ", "that's wrong", "thats wrong",
        "fix ", "revert ", "undo ",
    )
    if any(lower.startswith(s) for s in correction_starters):
        return "correction"

    # New requirement: signals of adding something new
    new_req_signals = (
        "also ", "add ", "we also need", "new requirement",
        "additionally", "one more thing", "another thing",
        "please also", "i also want", "i also need",
        "make sure to also", "include ",
    )
    if any(s in lower for s in new_req_signals):
        return "new_requirement"

    return "guidance"


def _handle_user_prompt_submit(input_data: dict, task_dir: str) -> None:
    """Log a human guidance entry for UserPromptSubmit events."""
    prompt = input_data.get("prompt", "").strip()
    if not prompt:
        return
    if _should_skip_prompt(prompt):
        return

    phase = _get_phase(task_dir)
    interaction_type = _classify_input(prompt)
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "role": "human",
        "type": interaction_type,
        "content": prompt,
        "phase": phase,
        "agent": "user",
        "source": "hook",
    }
    _append_interaction(task_dir, entry)


def _extract_session_cost(input_data: dict) -> dict | None:
    """Parse cost information from a Stop hook payload.

    Tries multiple key names since Claude may use different formats.
    Returns None if no usable cost data is found.
    """
    cost_obj = (
        input_data.get("session_cost")
        or input_data.get("cost")
        or input_data.get("sessionCost")
    )
    if not cost_obj or not isinstance(cost_obj, dict):
        return None

    cost_usd = (
        cost_obj.get("costUsd")
        or cost_obj.get("cost_usd")
        or cost_obj.get("total_cost")
        or 0.0
    )
    input_tokens = cost_obj.get("input_tokens", 0) or cost_obj.get("inputTokens", 0) or 0
    output_tokens = cost_obj.get("output_tokens", 0) or cost_obj.get("outputTokens", 0) or 0
    model = cost_obj.get("model", "") or ""

    if not cost_usd and not input_tokens and not output_tokens:
        return None

    return {
        "cost_usd": float(cost_usd),
        "input_tokens": int(input_tokens),
        "output_tokens": int(output_tokens),
        "model": model,
    }


def _record_cost(task_dir: str, cost_data: dict, agent: str) -> None:
    """Record cost data to state.json (via workflow_record_cost) and costs.jsonl."""
    try:
        task_id = Path(task_dir).name
        model = cost_data.get("model", "opus")
        model_lower = model.lower()
        if "opus" in model_lower:
            model = "opus"
        elif "sonnet" in model_lower:
            model = "sonnet"
        elif "haiku" in model_lower:
            model = "haiku"

        input_tokens = cost_data["input_tokens"]
        output_tokens = cost_data["output_tokens"]

        # Record via state_tools (updates state.json cost_tracking)
        try:
            from agentic_workflow_server.state_tools import workflow_record_cost
            workflow_record_cost(
                agent=agent, model=model,
                input_tokens=input_tokens, output_tokens=output_tokens,
                task_id=task_id,
            )
        except ImportError:
            pass

        # Also append to costs.jsonl for crew-cost-report.py
        cost_entry = {
            "agent": agent, "model": model,
            "input_tokens": input_tokens, "output_tokens": output_tokens,
            "total_cost": cost_data.get("cost_usd", 0.0),
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "source": "hook",
        }
        costs_path = Path(task_dir) / "costs.jsonl"
        with open(costs_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(cost_entry) + "\n")
    except Exception:
        pass


def _handle_stop(input_data: dict, task_dir: str) -> None:
    """Log an agent message entry for Stop events and record cost if available."""
    # Extract a useful summary from the stop event
    # The stop hook includes stop_hook_active and transcript_path fields
    transcript_path = input_data.get("transcript_path", "")
    stop_hook_active = input_data.get("stop_hook_active", False)

    # Don't log if this stop was triggered by another stop hook (avoid infinite loops)
    if stop_hook_active:
        return

    # Record cost if the Stop payload carries session cost data
    cost_data = _extract_session_cost(input_data)
    if cost_data:
        _record_cost(task_dir, cost_data, agent="orchestrator")

    # Try to get a summary from the transcript
    summary = _extract_response_summary(transcript_path)
    if not summary:
        return

    phase = _get_phase(task_dir)
    entry = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "role": "agent",
        "type": "message",
        "content": summary,
        "phase": phase,
        "agent": "orchestrator",
        "source": "hook",
    }
    _append_interaction(task_dir, entry)


def _extract_response_summary(transcript_path: str) -> str:
    """Extract the last assistant response from the transcript file.

    Reads only the tail of the file (last 64 KB) to avoid scanning
    multi-MB transcripts on every Stop event.
    """
    if not transcript_path:
        return ""

    try:
        transcript_file = Path(transcript_path)
        if not transcript_file.exists():
            return ""

        TAIL_BYTES = 65536  # 64 KB — enough for several assistant messages

        with open(transcript_file, "rb") as f:
            f.seek(0, 2)
            size = f.tell()
            start = max(0, size - TAIL_BYTES)
            f.seek(start)
            raw = f.read()

        # Decode and split into lines; first line may be partial, skip it
        lines = raw.decode("utf-8", errors="replace").splitlines()
        if start > 0:
            lines = lines[1:]  # drop partial first line

        # Walk lines to find the last assistant message in this chunk
        last_assistant_content = ""
        for line in lines:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
                if msg.get("role") != "assistant":
                    continue
                content = msg.get("content", "")
                if isinstance(content, str) and content.strip():
                    last_assistant_content = content.strip()
                elif isinstance(content, list):
                    text_parts = [
                        block.get("text", "").strip()
                        for block in content
                        if isinstance(block, dict) and block.get("type") == "text"
                    ]
                    joined = " ".join(t for t in text_parts if t)
                    if joined:
                        last_assistant_content = joined
            except (json.JSONDecodeError, TypeError):
                continue

        if not last_assistant_content:
            return ""

        max_len = 500
        if len(last_assistant_content) > max_len:
            return last_assistant_content[:max_len] + "..."
        return last_assistant_content

    except (OSError, Exception):
        return ""


def _update_resume_md(task_dir: str) -> None:
    """Regenerate RESUME.md if stale (>10s since last write).

    Imports _generate_resume_md from crew_orchestrator — pure Python,
    no LLM involvement. Reads state.json + interactions.jsonl, writes
    a crash-recoverable snapshot to RESUME.md.
    """
    import time

    resume_path = Path(task_dir) / "RESUME.md"
    if resume_path.exists():
        try:
            age = time.time() - resume_path.stat().st_mtime
            if age < 10:
                return  # Written recently — skip
        except OSError:
            pass

    try:
        from crew_orchestrator import _generate_resume_md
        task_id = Path(task_dir).name
        _generate_resume_md(task_id)
    except Exception:
        pass  # Never block user


def main():
    try:
        input_data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        sys.exit(0)

    event_name = input_data.get("hook_event_name", "")

    # Only handle events we care about
    if event_name not in ("UserPromptSubmit", "Stop"):
        sys.exit(0)

    # Find active crew task — if none, exit silently
    task_dir = _find_session_task()
    if not task_dir:
        sys.exit(0)

    try:
        if event_name == "UserPromptSubmit":
            _handle_user_prompt_submit(input_data, task_dir)
        elif event_name == "Stop":
            _handle_stop(input_data, task_dir)
    except Exception:
        # Never fail — hook errors must not block the user
        pass

    # Update RESUME.md after logging interaction (crash recovery snapshot)
    _update_resume_md(task_dir)

    sys.exit(0)


if __name__ == "__main__":
    main()
