#!/usr/bin/env python3
"""
Display cost breakdown for a workflow task.

Reads .tasks/TASK_XXX/state.json and cost entries to produce a
formatted cost report. Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-cost-report.py                # Active task
    python3 scripts/crew-cost-report.py TASK_042       # Specific task
    python3 scripts/crew-cost-report.py --json         # JSON output
    python3 scripts/crew-cost-report.py --all          # All tasks summary
"""

import json
import sys
from pathlib import Path


# Model costs per million tokens (matches state_tools.py pricing)
MODEL_COSTS = {
    "opus": {"input": 5.00, "output": 25.00},
    "sonnet": {"input": 3.00, "output": 15.00},
    "haiku": {"input": 0.80, "output": 4.00},
}


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


def _find_task_dir(task_id: str = "") -> Path | None:
    tasks_dir = _find_tasks_dir()
    if not tasks_dir.exists():
        return None

    if task_id:
        # Exact match
        candidate = tasks_dir / task_id
        if candidate.exists():
            return candidate
        # Fuzzy: TASK_042 might be TASK_042_description
        for d in tasks_dir.iterdir():
            if d.is_dir() and d.name.startswith(task_id):
                return d
        return None

    # Find most recently updated active task
    best = None
    best_time = ""
    for d in tasks_dir.iterdir():
        if not d.is_dir() or d.name.startswith("."):
            continue
        state_file = d / "state.json"
        if state_file.exists():
            try:
                state = json.loads(state_file.read_text())
                updated = state.get("updated_at", "")
                if updated > best_time:
                    best_time = updated
                    best = d
            except Exception:
                continue
    return best


def _load_cost_entries(task_dir: Path) -> list[dict]:
    """Load cost entries from costs.jsonl; fall back to state.json cost_tracking."""
    cost_file = task_dir / "costs.jsonl"
    entries = []
    if cost_file.exists():
        for line in cost_file.read_text().splitlines():
            line = line.strip()
            if line:
                try:
                    entries.append(json.loads(line))
                except json.JSONDecodeError:
                    continue

    # If no entries in costs.jsonl, try state.json cost_tracking as fallback
    if not entries:
        state_file = task_dir / "state.json"
        if state_file.exists():
            try:
                state = json.loads(state_file.read_text())
                cost_tracking = state.get("cost_tracking", {})
                for entry in cost_tracking.get("entries", []):
                    entries.append({
                        "agent": entry.get("agent", "unknown"),
                        "model": entry.get("model", "unknown"),
                        "input_tokens": entry.get("input_tokens", 0),
                        "output_tokens": entry.get("output_tokens", 0),
                        "total_cost": entry.get("total_cost", 0.0),
                    })
            except Exception:
                pass

    return entries


def _format_cost(amount: float) -> str:
    if amount < 0.01:
        return f"${amount:.4f}"
    return f"${amount:.2f}"


def _format_tokens(count: int) -> str:
    if count >= 1_000_000:
        return f"{count / 1_000_000:.1f}M"
    if count >= 1_000:
        return f"{count / 1_000:.1f}K"
    return str(count)


# ── Report ───────────────────────────────────────────────────────────────────

def compute_report(task_dir: Path) -> dict:
    state = json.loads((task_dir / "state.json").read_text())
    entries = _load_cost_entries(task_dir)

    by_agent: dict[str, dict] = {}
    by_model: dict[str, dict] = {}
    total_input = 0
    total_output = 0
    total_cost = 0.0

    for e in entries:
        agent = e.get("agent", "unknown")
        model = e.get("model", "unknown")
        input_t = e.get("input_tokens", 0)
        output_t = e.get("output_tokens", 0)
        cost = e.get("total_cost", 0.0)

        if cost == 0 and model in MODEL_COSTS:
            mc = MODEL_COSTS[model]
            cost = (input_t / 1_000_000) * mc["input"] + (output_t / 1_000_000) * mc["output"]

        total_input += input_t
        total_output += output_t
        total_cost += cost

        if agent not in by_agent:
            by_agent[agent] = {"input": 0, "output": 0, "cost": 0.0}
        by_agent[agent]["input"] += input_t
        by_agent[agent]["output"] += output_t
        by_agent[agent]["cost"] += cost

        if model not in by_model:
            by_model[model] = {"tokens": 0, "cost": 0.0}
        by_model[model]["tokens"] += input_t + output_t
        by_model[model]["cost"] += cost

    return {
        "task_id": state.get("task_id", task_dir.name),
        "description": state.get("description", ""),
        "mode": state.get("workflow_mode", {}).get("effective", "?"),
        "entries_count": len(entries),
        "total_input_tokens": total_input,
        "total_output_tokens": total_output,
        "total_cost": total_cost,
        "by_agent": by_agent,
        "by_model": by_model,
    }


def print_report(report: dict) -> None:
    print(f"\nCost Report: {report['task_id']}")
    if report["description"]:
        desc = report["description"][:60]
        print(f"  {desc}")
    print(f"  Mode: {report['mode']}")
    print()

    if report["entries_count"] == 0:
        print("  No cost entries recorded yet.")
        print("  (Cost tracking records entries when agents report token usage)")
        return

    # By Agent
    fmt = "  {:<20} {:>12} {:>13} {:>10}"
    print(fmt.format("Agent", "Input Tokens", "Output Tokens", "Cost"))
    print(fmt.format("-" * 20, "-" * 12, "-" * 13, "-" * 10))

    for agent, data in sorted(report["by_agent"].items()):
        print(fmt.format(
            agent,
            _format_tokens(data["input"]),
            _format_tokens(data["output"]),
            _format_cost(data["cost"]),
        ))

    print(fmt.format("-" * 20, "-" * 12, "-" * 13, "-" * 10))
    print(fmt.format(
        "TOTAL",
        _format_tokens(report["total_input_tokens"]),
        _format_tokens(report["total_output_tokens"]),
        _format_cost(report["total_cost"]),
    ))

    # By Model
    if report["by_model"]:
        print()
        fmt2 = "  {:<20} {:>12} {:>10}"
        print(fmt2.format("Model", "Tokens", "Cost"))
        print(fmt2.format("-" * 20, "-" * 12, "-" * 10))
        for model, data in sorted(report["by_model"].items()):
            print(fmt2.format(model, _format_tokens(data["tokens"]), _format_cost(data["cost"])))


def print_all_summary() -> None:
    tasks_dir = _find_tasks_dir()
    if not tasks_dir.exists():
        print("No .tasks/ directory found.")
        return

    total_cost = 0.0
    by_mode: dict[str, list[float]] = {}
    reports = []

    for d in sorted(tasks_dir.iterdir()):
        if not d.is_dir() or d.name.startswith("."):
            continue
        if not (d / "state.json").exists():
            continue
        report = compute_report(d)
        reports.append(report)
        total_cost += report["total_cost"]
        mode = report["mode"]
        by_mode.setdefault(mode, []).append(report["total_cost"])

    if not reports:
        print("No tasks found.")
        return

    print(f"\nCost Summary — {len(reports)} tasks")
    print(f"  Total: {_format_cost(total_cost)}")
    if reports:
        print(f"  Average: {_format_cost(total_cost / len(reports))}/task")

    if by_mode:
        print(f"\n  By Mode:")
        for mode, costs in sorted(by_mode.items()):
            avg = sum(costs) / len(costs) if costs else 0
            print(f"    {mode:<12} {_format_cost(sum(costs)):>8}  (avg {_format_cost(avg)}/task, {len(costs)} tasks)")

    # Per-task summary
    print()
    fmt = "  {:<14} {:<8} {:>8} {:>6}"
    print(fmt.format("Task", "Mode", "Cost", "Entries"))
    print(fmt.format("-" * 14, "-" * 8, "-" * 8, "-" * 6))
    for r in reports:
        print(fmt.format(r["task_id"], r["mode"], _format_cost(r["total_cost"]), str(r["entries_count"])))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    flags = [a for a in sys.argv[1:] if a.startswith("-")]

    if "--all" in flags:
        if "--json" in flags:
            tasks_dir = _find_tasks_dir()
            reports = []
            if tasks_dir.exists():
                for d in sorted(tasks_dir.iterdir()):
                    if d.is_dir() and not d.name.startswith(".") and (d / "state.json").exists():
                        reports.append(compute_report(d))
            print(json.dumps(reports, indent=2))
        else:
            print_all_summary()
        return

    task_id = args[0] if args else ""
    task_dir = _find_task_dir(task_id)

    if not task_dir:
        if task_id:
            print(f"Task {task_id} not found in .tasks/", file=sys.stderr)
        else:
            print("No tasks found. Specify a task ID or use --all.", file=sys.stderr)
        sys.exit(1)

    report = compute_report(task_dir)

    if "--json" in flags:
        print(json.dumps(report, indent=2))
    else:
        print_report(report)


if __name__ == "__main__":
    main()
