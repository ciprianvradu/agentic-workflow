#!/usr/bin/env python3
"""
Display workflow status for all tasks.

Reads .tasks/*/state.json and shows a summary table with worktree info.
Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-status.py           # Show status table
    python3 scripts/crew-status.py --json    # Output raw JSON
"""

import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path


# ── Helpers ──────────────────────────────────────────────────────────────────

def _find_repo_root() -> Path:
    current = Path.cwd().resolve()
    while True:
        if (current / ".git").is_dir() or (current / ".git").is_file():
            return current
        parent = current.parent
        if parent == current:
            return Path.cwd()
        current = parent


def _find_tasks_dir() -> Path:
    repo = _find_repo_root()
    tasks_dir = repo / ".tasks"
    if tasks_dir.exists():
        return tasks_dir
    # Check if .tasks is a symlink
    for candidate in [repo / ".tasks"]:
        if candidate.is_symlink():
            return candidate.resolve()
    return tasks_dir


def _relative_time(iso_str: str) -> str:
    try:
        dt = datetime.fromisoformat(iso_str)
        now = datetime.now()
        delta = now - dt
        secs = int(delta.total_seconds())
        if secs < 60:
            return "just now"
        if secs < 3600:
            mins = secs // 60
            return f"{mins}m ago"
        if secs < 86400:
            hours = secs // 3600
            return f"{hours}h ago"
        days = secs // 86400
        if days == 1:
            return "yesterday"
        return f"{days}d ago"
    except Exception:
        return iso_str[:10] if iso_str else "-"


def _progress_bar(current: int, total: int, width: int = 16) -> str:
    if total <= 0:
        return "-"
    pct = min(current / total, 1.0)
    filled = int(pct * width)
    bar = "=" * filled + "." * (width - filled)
    return f"{bar} {int(pct * 100)}% ({current}/{total})"


def _get_git_worktrees() -> dict[str, str]:
    """Get git worktree paths -> branch mapping."""
    try:
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            capture_output=True, text=True, timeout=5
        )
        worktrees: dict[str, str] = {}
        current_path = ""
        for line in result.stdout.splitlines():
            if line.startswith("worktree "):
                current_path = line[9:]
            elif line.startswith("branch "):
                branch = line[7:]
                if branch.startswith("refs/heads/"):
                    branch = branch[11:]
                worktrees[current_path] = branch
        return worktrees
    except Exception:
        return {}


# ── Load Tasks ───────────────────────────────────────────────────────────────

def load_tasks(tasks_dir: Path) -> list[dict]:
    tasks = []
    if not tasks_dir.exists():
        return tasks

    for d in sorted(tasks_dir.iterdir()):
        if not d.is_dir() or d.name.startswith("."):
            continue
        state_file = d / "state.json"
        if not state_file.exists():
            continue
        try:
            state = json.loads(state_file.read_text())
            tasks.append(state)
        except Exception:
            continue
    return tasks


def is_complete(state: dict) -> bool:
    mode = state.get("workflow_mode", {})
    required = mode.get("phases", [])
    completed = state.get("phases_completed", [])
    if not required:
        return False
    return all(p in completed for p in required)


def get_action(state: dict) -> str:
    if is_complete(state):
        wt = state.get("worktree")
        if wt:
            status = wt.get("status", "")
            if status == "recyclable":
                return "recyclable"
            if status == "cleaned":
                return "done"
            return "cleanup"
        return "done"
    return "resume"


# ── Display ──────────────────────────────────────────────────────────────────

def print_status(tasks: list[dict]) -> None:
    if not tasks:
        print("No tasks found in .tasks/")
        return

    # Summary counts
    active = [t for t in tasks if not is_complete(t)]
    completed = [t for t in tasks if is_complete(t)]
    print(f"\nTasks: {len(tasks)} total, {len(active)} active, {len(completed)} completed\n")

    # Table header
    fmt = "{:<12} {:<16} {:<20} {:<24} {:<12} {:<12}"
    print(fmt.format("Task", "Phase", "Progress", "Branch", "Action", "Updated"))
    print(fmt.format("-" * 12, "-" * 16, "-" * 20, "-" * 24, "-" * 12, "-" * 12))

    for state in tasks:
        task_id = state.get("task_id", "?")
        phase = state.get("phase") or "-"
        if is_complete(state):
            phase = "complete"

        # Progress
        impl = state.get("implementation_progress", {})
        total = impl.get("total_steps", 0)
        current = impl.get("current_step", 0)
        if total > 0:
            pct = f"{int(current / total * 100)}%"
        else:
            pct = "-"

        # Branch
        wt = state.get("worktree", {})
        branch = wt.get("branch", "-") if wt else "-"
        if len(branch) > 24:
            branch = branch[:21] + "..."

        action = get_action(state)
        updated = _relative_time(state.get("updated_at", ""))

        print(fmt.format(task_id, phase, pct, branch, action, updated))

    # Worktree overview
    wt_tasks = [t for t in tasks if t.get("worktree")]
    if wt_tasks:
        active_wt = [t for t in wt_tasks if t.get("worktree", {}).get("status") == "active"]
        recyclable_wt = [t for t in wt_tasks if t.get("worktree", {}).get("status") == "recyclable"]

        print(f"\nWorktrees:")
        print(f"  Active:      {len(active_wt)}")
        print(f"  Recyclable:  {len(recyclable_wt)}")

        # Cleanup candidates
        cleanup = [t for t in tasks if get_action(t) == "cleanup"]
        if cleanup:
            print(f"\n  Cleanup candidates (complete but worktree active):")
            for t in cleanup:
                tid = t["task_id"]
                wt = t.get("worktree", {})
                print(f"    {tid}  {wt.get('path', '?')}  {wt.get('branch', '?')}")

    # Resume commands
    if active:
        print(f"\nResume commands:")
        for t in active:
            tid = t["task_id"]
            print(f"  /crew resume {tid}")


def main():
    tasks_dir = _find_tasks_dir()
    tasks = load_tasks(tasks_dir)

    if "--json" in sys.argv:
        print(json.dumps(tasks, indent=2, default=str))
        return

    print_status(tasks)


if __name__ == "__main__":
    main()
