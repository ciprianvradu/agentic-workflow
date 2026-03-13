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

sys.path.insert(0, str(Path(__file__).parent))
from workflow_state import _resolve_tasks_dir, _detect_worktree_task_id


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


def _handle_stop(input_data: dict, task_dir: str) -> None:
    """Log an agent message entry for Stop events."""
    # Extract a useful summary from the stop event
    # The stop hook includes stop_hook_active and transcript_path fields
    transcript_path = input_data.get("transcript_path", "")
    stop_hook_active = input_data.get("stop_hook_active", False)

    # Don't log if this stop was triggered by another stop hook (avoid infinite loops)
    if stop_hook_active:
        return

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
    """Extract the last assistant response from the transcript file."""
    if not transcript_path:
        return ""

    try:
        transcript_file = Path(transcript_path)
        if not transcript_file.exists():
            return ""

        # Read transcript (JSONL format) and find the last assistant message
        last_assistant_content = ""
        with open(transcript_file, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    msg = json.loads(line)
                    role = msg.get("role", "")
                    if role == "assistant":
                        # Extract text content from assistant message
                        content = msg.get("content", "")
                        if isinstance(content, str) and content.strip():
                            last_assistant_content = content.strip()
                        elif isinstance(content, list):
                            # Content blocks — concatenate text blocks
                            text_parts = []
                            for block in content:
                                if isinstance(block, dict) and block.get("type") == "text":
                                    text = block.get("text", "").strip()
                                    if text:
                                        text_parts.append(text)
                            if text_parts:
                                last_assistant_content = " ".join(text_parts)
                except (json.JSONDecodeError, TypeError):
                    continue

        if not last_assistant_content:
            return ""

        # Truncate to a reasonable summary length
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
