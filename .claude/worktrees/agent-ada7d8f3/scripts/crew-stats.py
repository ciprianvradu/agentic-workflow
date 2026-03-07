#!/usr/bin/env python3
"""
Display workflow statistics dashboard.

Aggregates metrics across all tasks: mode distribution, cost trends,
iteration stats, and error pattern counts. Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-stats.py           # Show dashboard
    python3 scripts/crew-stats.py --json    # Output raw JSON
"""

import json
import sys
from collections import Counter
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
    return _find_repo_root() / ".tasks"


def _bar(count: int, total: int, width: int = 10) -> str:
    if total <= 0:
        return "." * width
    filled = max(1, int(count / total * width)) if count > 0 else 0
    return "=" * filled + "." * (width - filled)


# ── Data Loading ─────────────────────────────────────────────────────────────

def load_all_states(tasks_dir: Path) -> list[dict]:
    states = []
    if not tasks_dir.exists():
        return states
    for d in sorted(tasks_dir.iterdir()):
        if not d.is_dir() or d.name.startswith("."):
            continue
        sf = d / "state.json"
        if sf.exists():
            try:
                states.append(json.loads(sf.read_text()))
            except Exception:
                continue
    return states


def load_error_patterns(tasks_dir: Path) -> list[dict]:
    pf = tasks_dir / ".error_patterns.jsonl"
    if not pf.exists():
        return []
    patterns = []
    for line in pf.read_text().splitlines():
        line = line.strip()
        if line:
            try:
                patterns.append(json.loads(line))
            except Exception:
                continue
    return patterns


def is_complete(state: dict) -> bool:
    mode = state.get("workflow_mode", {})
    required = mode.get("phases", [])
    completed = state.get("phases_completed", [])
    if not required:
        return False
    return all(p in completed for p in required)


# ── Dashboard ────────────────────────────────────────────────────────────────

def compute_stats(states: list[dict], patterns: list[dict]) -> dict:
    total = len(states)
    active = [s for s in states if not is_complete(s)]
    completed = [s for s in states if is_complete(s)]

    # Mode distribution
    mode_counts: Counter[str] = Counter()
    for s in states:
        mode = s.get("workflow_mode", {}).get("effective", "unknown")
        mode_counts[mode] += 1

    # Iteration stats
    single_pass = 0
    one_revision = 0
    multi_revision = 0
    for s in states:
        iteration = s.get("iteration", 1)
        if iteration <= 1:
            single_pass += 1
        elif iteration == 2:
            one_revision += 1
        else:
            multi_revision += 1

    # Average phases
    total_phases = sum(len(s.get("phases_completed", [])) for s in states)
    avg_phases = total_phases / total if total > 0 else 0

    # Error patterns
    top_pattern = None
    if patterns:
        patterns_sorted = sorted(patterns, key=lambda p: p.get("times_seen", 0), reverse=True)
        top_pattern = {
            "signature": patterns_sorted[0].get("signature", "?"),
            "times_seen": patterns_sorted[0].get("times_seen", 0),
        }

    return {
        "total": total,
        "active": len(active),
        "active_ids": [s.get("task_id", "?") for s in active],
        "completed": len(completed),
        "avg_phases": round(avg_phases, 1),
        "mode_distribution": dict(mode_counts),
        "iterations": {
            "single_pass": single_pass,
            "one_revision": one_revision,
            "multi_revision": multi_revision,
        },
        "error_patterns": {
            "total": len(patterns),
            "top_pattern": top_pattern,
        },
    }


def print_dashboard(stats: dict) -> None:
    total = stats["total"]

    # Task Overview
    print(f"\nTasks:")
    active_ids = ", ".join(stats["active_ids"][:5])
    print(f"  Total:       {stats['total']}")
    print(f"  Active:      {stats['active']}  ({active_ids})" if stats["active"] else f"  Active:      0")
    print(f"  Completed:   {stats['completed']}")
    print(f"  Avg phases:  {stats['avg_phases']} per task")

    # Mode Distribution
    mode_dist = stats["mode_distribution"]
    if mode_dist:
        print(f"\nMode Distribution:")
        for mode in ["standard", "reviewed", "thorough"]:
            count = mode_dist.get(mode, 0)
            pct = int(count / total * 100) if total > 0 else 0
            bar = _bar(count, total)
            print(f"  {mode:<12} {bar}  {count} tasks ({pct}%)")
        # Show any other modes
        for mode, count in sorted(mode_dist.items()):
            if mode not in ("standard", "reviewed", "thorough"):
                pct = int(count / total * 100) if total > 0 else 0
                bar = _bar(count, total)
                print(f"  {mode:<12} {bar}  {count} tasks ({pct}%)")

    # Iteration Stats
    iters = stats["iterations"]
    if total > 0:
        print(f"\nReview Iterations:")
        sp_pct = int(iters["single_pass"] / total * 100)
        or_pct = int(iters["one_revision"] / total * 100)
        mr_pct = int(iters["multi_revision"] / total * 100)
        print(f"  Single pass:       {iters['single_pass']} tasks ({sp_pct}%)")
        print(f"  One revision:      {iters['one_revision']} tasks ({or_pct}%)")
        print(f"  Multiple revisions:{iters['multi_revision']} tasks ({mr_pct}%)")

    # Error Patterns
    ep = stats["error_patterns"]
    print(f"\nError Patterns:")
    print(f"  Total recorded:  {ep['total']}")
    if ep["top_pattern"]:
        sig = ep["top_pattern"]["signature"][:50]
        seen = ep["top_pattern"]["times_seen"]
        print(f"  Most common:     \"{sig}\" (seen {seen} times)")


def main():
    tasks_dir = _find_tasks_dir()
    states = load_all_states(tasks_dir)
    patterns = load_error_patterns(tasks_dir)
    stats = compute_stats(states, patterns)

    if "--json" in sys.argv:
        print(json.dumps(stats, indent=2))
        return

    if not states:
        print("No tasks found in .tasks/")
        return

    print_dashboard(stats)


if __name__ == "__main__":
    main()
