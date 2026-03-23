#!/usr/bin/env python3
"""
Display workflow statistics dashboard.

Aggregates metrics across all tasks: mode distribution, cost trends,
concern hit rates, phase timing, and error pattern counts.
Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-stats.py            # Show full dashboard
    python3 scripts/crew-stats.py --json     # Output raw JSON
    python3 scripts/crew-stats.py --recent   # Show recent task details
    python3 scripts/crew-stats.py --compare  # Compare tasks by type
    python3 scripts/crew-stats.py --repos ~/project-a ~/project-b  # Cross-repo
"""

import argparse
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


def _fmt_cost(cost: float) -> str:
    if cost < 0.01:
        return f"${cost:.4f}"
    return f"${cost:.2f}"


def _fmt_duration(seconds: float) -> str:
    if seconds < 60:
        return f"{seconds:.0f}s"
    minutes = seconds / 60
    if minutes < 60:
        return f"{minutes:.1f}m"
    hours = minutes / 60
    return f"{hours:.1f}h"


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Workflow statistics dashboard")
    parser.add_argument("--json", action="store_true", help="Output raw JSON")
    parser.add_argument("--recent", action="store_true", help="Show recent task details")
    parser.add_argument("--compare", action="store_true", help="Compare tasks by type")
    parser.add_argument("--repos", nargs="+", metavar="PATH",
                        help="Aggregate stats across multiple repo paths")
    return parser.parse_args()


# ── Data Loading ─────────────────────────────────────────────────────────────

def load_all_states_multi(tasks_dirs: list[tuple[str, Path]]) -> list[dict]:
    """Load states from multiple repos, tagging each with repo name."""
    states = []
    for repo_name, tasks_dir in tasks_dirs:
        if not tasks_dir.exists():
            continue
        for d in sorted(tasks_dir.iterdir()):
            if not d.is_dir() or d.name.startswith("."):
                continue
            sf = d / "state.json"
            if sf.exists():
                try:
                    s = json.loads(sf.read_text())
                    s["_repo"] = repo_name
                    states.append(s)
                except Exception:
                    continue
    return states


def load_all_states(tasks_dir: Path) -> list[dict]:
    return load_all_states_multi([("local", tasks_dir)])


def load_interactions(task_dir: Path) -> list[dict]:
    """Load interactions.jsonl for a task directory."""
    ifile = task_dir / "interactions.jsonl"
    if not ifile.exists():
        return []
    entries = []
    for line in ifile.read_text().splitlines():
        line = line.strip()
        if line:
            try:
                entries.append(json.loads(line))
            except Exception:
                continue
    return entries


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
    status = state.get("status", "")
    if status == "completed":
        return True
    mode = state.get("workflow_mode", {})
    required = mode.get("phases", [])
    completed = state.get("phases_completed", [])
    if not required:
        return False
    return all(p in completed for p in required)


# ── Statistics ───────────────────────────────────────────────────────────────

def compute_cost_stats(states: list[dict]) -> dict:
    """Aggregate cost data across all tasks."""
    total_cost = 0.0
    total_tokens = 0
    tasks_with_cost = 0
    by_agent: Counter = Counter()
    by_model: Counter = Counter()
    costs_per_task = []

    for s in states:
        ct = s.get("cost_tracking", {})
        totals = ct.get("totals", {})
        task_cost = totals.get("total_cost", 0)
        if task_cost > 0:
            tasks_with_cost += 1
            total_cost += task_cost
            total_tokens += totals.get("input_tokens", 0) + totals.get("output_tokens", 0)
            costs_per_task.append(task_cost)

        for agent, data in ct.get("by_agent", {}).items():
            by_agent[agent] += data.get("total_cost", 0)
        for model, data in ct.get("by_model", {}).items():
            by_model[model] += data.get("total_cost", 0)

    avg_cost = total_cost / tasks_with_cost if tasks_with_cost > 0 else 0

    return {
        "total_cost": round(total_cost, 4),
        "total_tokens": total_tokens,
        "tasks_with_cost": tasks_with_cost,
        "avg_cost_per_task": round(avg_cost, 4),
        "by_agent": {k: round(v, 4) for k, v in by_agent.most_common(10)},
        "by_model": {k: round(v, 4) for k, v in by_model.most_common(5)},
    }


def compute_concern_stats(states: list[dict]) -> dict:
    """Aggregate concern data across all tasks."""
    total_concerns = 0
    addressed = 0
    tasks_with_concerns = 0
    by_severity: Counter = Counter()
    by_source: Counter = Counter()

    for s in states:
        concerns = s.get("concerns", [])
        if concerns:
            tasks_with_concerns += 1
            total_concerns += len(concerns)
            for c in concerns:
                if c.get("addressed_by"):
                    addressed += 1
                by_severity[c.get("severity", "unknown")] += 1
                by_source[c.get("source", "unknown")] += 1

    hit_rate = tasks_with_concerns / len(states) * 100 if states else 0
    address_rate = addressed / total_concerns * 100 if total_concerns > 0 else 0

    return {
        "total_concerns": total_concerns,
        "addressed": addressed,
        "tasks_with_concerns": tasks_with_concerns,
        "concern_hit_rate_pct": round(hit_rate, 1),
        "address_rate_pct": round(address_rate, 1),
        "by_severity": dict(by_severity.most_common()),
        "by_source": dict(by_source.most_common(5)),
    }


def compute_phase_timing(tasks_dir: Path, states: list[dict]) -> dict:
    """Compute average time per phase from interactions.jsonl timestamps."""
    phase_durations: dict[str, list[float]] = {}

    for s in states:
        task_id = s.get("task_id", "")
        if not task_id:
            continue
        task_dir = tasks_dir / task_id
        interactions = load_interactions(task_dir)
        if not interactions:
            continue

        # Group interactions by phase and find min/max timestamps
        phase_times: dict[str, dict] = {}
        for entry in interactions:
            phase = entry.get("phase", "")
            ts = entry.get("timestamp", "")
            if not phase or not ts:
                continue
            if phase not in phase_times:
                phase_times[phase] = {"first": ts, "last": ts}
            else:
                if ts < phase_times[phase]["first"]:
                    phase_times[phase]["first"] = ts
                if ts > phase_times[phase]["last"]:
                    phase_times[phase]["last"] = ts

        # Calculate duration per phase
        for phase, times in phase_times.items():
            try:
                from datetime import datetime as dt
                first = dt.fromisoformat(times["first"])
                last = dt.fromisoformat(times["last"])
                duration = (last - first).total_seconds()
                if duration > 0:
                    if phase not in phase_durations:
                        phase_durations[phase] = []
                    phase_durations[phase].append(duration)
            except (ValueError, TypeError):
                continue

    # Compute averages
    avg_by_phase = {}
    for phase, durations in sorted(phase_durations.items()):
        avg_by_phase[phase] = {
            "avg_seconds": round(sum(durations) / len(durations), 1),
            "count": len(durations),
        }

    return {"by_phase": avg_by_phase}


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


def compute_version_stats(states: list[dict]) -> dict:
    """Aggregate tool version data across tasks."""
    version_counts: Counter = Counter()
    for s in states:
        version = s.get("tool_version", "unknown")
        version_counts[version] += 1
    return {"by_version": dict(version_counts.most_common())}


def compute_config_delta_stats(states: list[dict]) -> dict:
    """Aggregate config delta data — which settings are most commonly changed."""
    key_counts: Counter = Counter()
    for s in states:
        delta = s.get("config_delta", {})
        _count_delta_keys(delta, "", key_counts)
    return {"most_customized": dict(key_counts.most_common(10))}


def _count_delta_keys(delta: dict, prefix: str, counter: Counter) -> None:
    for key, value in delta.items():
        full_key = f"{prefix}{key}" if not prefix else f"{prefix}.{key}"
        if isinstance(value, dict):
            _count_delta_keys(value, full_key, counter)
        else:
            counter[full_key] += 1


def compute_repo_breakdown(states: list[dict]) -> dict:
    """Group states by repo and compute per-repo summary."""
    by_repo: dict[str, list[dict]] = {}
    for s in states:
        repo = s.get("_repo", "local")
        by_repo.setdefault(repo, []).append(s)

    breakdown = {}
    for repo_name, repo_states in sorted(by_repo.items()):
        completed = [s for s in repo_states if is_complete(s)]
        total_cost = sum(
            s.get("cost_tracking", {}).get("totals", {}).get("total_cost", 0)
            for s in repo_states
        )
        versions = Counter(s.get("tool_version", "unknown") for s in repo_states)
        breakdown[repo_name] = {
            "total_tasks": len(repo_states),
            "completed": len(completed),
            "total_cost": round(total_cost, 4),
            "versions": dict(versions.most_common(3)),
        }
    return breakdown


# ── Display ──────────────────────────────────────────────────────────────────

def print_dashboard(stats: dict, cost_stats: dict, concern_stats: dict,
                    phase_timing: dict, version_stats: dict | None = None,
                    config_delta_stats: dict | None = None) -> None:
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
        for mode in ["quick", "standard", "thorough"]:
            count = mode_dist.get(mode, 0)
            if count == 0 and mode not in mode_dist:
                continue
            pct = int(count / total * 100) if total > 0 else 0
            bar = _bar(count, total)
            print(f"  {mode:<12} {bar}  {count} tasks ({pct}%)")
        for mode, count in sorted(mode_dist.items()):
            if mode not in ("quick", "standard", "thorough"):
                pct = int(count / total * 100) if total > 0 else 0
                bar = _bar(count, total)
                print(f"  {mode:<12} {bar}  {count} tasks ({pct}%)")

    # Cost Summary
    if cost_stats["tasks_with_cost"] > 0:
        print(f"\nCost Summary:")
        print(f"  Total cost:    {_fmt_cost(cost_stats['total_cost'])}  ({cost_stats['tasks_with_cost']} tasks)")
        print(f"  Avg per task:  {_fmt_cost(cost_stats['avg_cost_per_task'])}")
        print(f"  Total tokens:  {cost_stats['total_tokens']:,}")
        if cost_stats["by_agent"]:
            print(f"  By agent:")
            for agent, cost in cost_stats["by_agent"].items():
                print(f"    {agent:<20} {_fmt_cost(cost)}")
        if cost_stats["by_model"]:
            print(f"  By model:")
            for model, cost in cost_stats["by_model"].items():
                print(f"    {model:<20} {_fmt_cost(cost)}")

    # Concern Hit Rates
    if concern_stats["total_concerns"] > 0:
        print(f"\nConcerns:")
        print(f"  Total raised:  {concern_stats['total_concerns']}")
        print(f"  Addressed:     {concern_stats['addressed']} ({concern_stats['address_rate_pct']}%)")
        print(f"  Hit rate:      {concern_stats['concern_hit_rate_pct']}% of tasks had concerns")
        if concern_stats["by_severity"]:
            print(f"  By severity:")
            for sev in ["critical", "high", "medium", "low"]:
                count = concern_stats["by_severity"].get(sev, 0)
                if count > 0:
                    print(f"    {sev:<12} {count}")
        if concern_stats["by_source"]:
            print(f"  Top sources:")
            for source, count in list(concern_stats["by_source"].items())[:3]:
                print(f"    {source:<20} {count}")

    # Phase Timing
    phase_data = phase_timing.get("by_phase", {})
    if phase_data:
        print(f"\nPhase Timing (avg):")
        for phase, data in phase_data.items():
            avg = data["avg_seconds"]
            count = data["count"]
            print(f"  {phase:<20} {_fmt_duration(avg):>8}  ({count} samples)")

    # Iteration Stats
    iters = stats["iterations"]
    if total > 0:
        print(f"\nReview Iterations:")
        sp_pct = int(iters["single_pass"] / total * 100)
        or_pct = int(iters["one_revision"] / total * 100)
        mr_pct = int(iters["multi_revision"] / total * 100)
        print(f"  Single pass:        {iters['single_pass']} tasks ({sp_pct}%)")
        print(f"  One revision:       {iters['one_revision']} tasks ({or_pct}%)")
        print(f"  Multiple revisions: {iters['multi_revision']} tasks ({mr_pct}%)")

    # Error Patterns
    ep = stats["error_patterns"]
    print(f"\nError Patterns:")
    print(f"  Total recorded:  {ep['total']}")
    if ep["top_pattern"]:
        sig = ep["top_pattern"]["signature"][:50]
        seen = ep["top_pattern"]["times_seen"]
        print(f"  Most common:     \"{sig}\" (seen {seen} times)")

    # Tool Versions
    if version_stats:
        version_data = version_stats.get("by_version", {})
        if version_data:
            print(f"\nTool Versions:")
            for version, count in version_data.items():
                print(f"  {version:<12} {count} tasks")

    # Config Customizations
    if config_delta_stats:
        delta_data = config_delta_stats.get("most_customized", {})
        if delta_data:
            print(f"\nMost Customized Settings:")
            for key, count in list(delta_data.items())[:5]:
                print(f"  {key:<40} {count} tasks")


def compute_task_comparison(tasks_dir: Path, states: list[dict]) -> dict:
    """Group tasks by type keywords and compare metrics."""
    # Keywords to group by
    type_keywords = {
        "refactor": ["refactor", "rename", "reorganize", "restructure"],
        "feature": ["add", "implement", "create", "new", "feature", "build"],
        "fix": ["fix", "bug", "patch", "resolve", "hotfix"],
        "docs": ["doc", "documentation", "readme", "write-up"],
        "test": ["test", "spec", "coverage"],
    }

    groups: dict[str, list[dict]] = {k: [] for k in type_keywords}
    groups["other"] = []

    for s in states:
        task_id = s.get("task_id", "")
        desc = s.get("description", "")
        # Also try loading from task.md
        if not desc and task_id:
            task_md = tasks_dir / task_id / "task.md"
            if task_md.exists():
                try:
                    desc = task_md.read_text()[:200]
                except Exception:
                    pass
        desc_lower = desc.lower()

        matched = False
        for group_name, keywords in type_keywords.items():
            if any(kw in desc_lower for kw in keywords):
                groups[group_name].append(s)
                matched = True
                break
        if not matched:
            groups["other"].append(s)

    # Compute per-group metrics
    result = {}
    for group_name, group_states in groups.items():
        if not group_states:
            continue
        count = len(group_states)
        costs = [s.get("cost_tracking", {}).get("totals", {}).get("total_cost", 0) for s in group_states]
        avg_cost = sum(costs) / count if count > 0 else 0
        phases_counts = [len(s.get("phases_completed", [])) for s in group_states]
        avg_phases = sum(phases_counts) / count if count > 0 else 0
        concern_counts = [len(s.get("concerns", [])) for s in group_states]
        avg_concerns = sum(concern_counts) / count if count > 0 else 0

        result[group_name] = {
            "count": count,
            "avg_cost": round(avg_cost, 4),
            "avg_phases": round(avg_phases, 1),
            "avg_concerns": round(avg_concerns, 1),
        }

    return result


def print_comparison(comparison: dict) -> None:
    """Display task comparison by type."""
    if not comparison:
        return
    print(f"\nTask Comparison by Type:")
    print(f"  {'Type':<12} {'Count':>6} {'Avg Cost':>10} {'Avg Phases':>11} {'Avg Concerns':>13}")
    print(f"  {'─' * 12} {'─' * 6} {'─' * 10} {'─' * 11} {'─' * 13}")
    for group_name in ["feature", "refactor", "fix", "docs", "test", "other"]:
        if group_name not in comparison:
            continue
        g = comparison[group_name]
        cost_str = _fmt_cost(g["avg_cost"]) if g["avg_cost"] > 0 else "-"
        print(f"  {group_name:<12} {g['count']:>6} {cost_str:>10} {g['avg_phases']:>11.1f} {g['avg_concerns']:>13.1f}")


def print_repo_breakdown(breakdown: dict) -> None:
    """Display per-repo summary when using --repos."""
    if not breakdown or len(breakdown) <= 1:
        return
    print(f"\nPer-Repo Breakdown:")
    print(f"  {'Repo':<30} {'Tasks':>6} {'Done':>6} {'Cost':>10} {'Version'}")
    print(f"  {'─' * 30} {'─' * 6} {'─' * 6} {'─' * 10} {'─' * 15}")
    for repo_name, data in breakdown.items():
        cost_str = _fmt_cost(data["total_cost"]) if data["total_cost"] > 0 else "-"
        top_version = next(iter(data["versions"]), "?")
        print(f"  {repo_name:<30} {data['total_tasks']:>6} {data['completed']:>6} {cost_str:>10} {top_version}")


def print_recent(states: list[dict], n: int = 10) -> None:
    """Show details for the N most recent tasks."""
    recent = states[-n:]
    if not recent:
        print("No tasks found.")
        return

    print(f"\nRecent Tasks ({len(recent)}):")
    print(f"  {'Task':<14} {'Mode':<10} {'Phases':<8} {'Cost':>8}  {'Status'}")
    print(f"  {'─' * 14} {'─' * 10} {'─' * 8} {'─' * 8}  {'─' * 10}")
    for s in reversed(recent):
        task_id = s.get("task_id", "?")
        mode = s.get("workflow_mode", {}).get("effective", "?")
        phases = len(s.get("phases_completed", []))
        cost = s.get("cost_tracking", {}).get("totals", {}).get("total_cost", 0)
        status = "done" if is_complete(s) else s.get("phase", "?")
        cost_str = _fmt_cost(cost) if cost > 0 else "-"
        print(f"  {task_id:<14} {mode:<10} {phases:<8} {cost_str:>8}  {status}")


def main():
    args = _parse_args()

    # Determine which tasks dirs to load from
    if args.repos:
        tasks_dirs = []
        for repo_path in args.repos:
            p = Path(repo_path).resolve()
            repo_name = p.name
            tasks_dir = p / ".tasks"
            tasks_dirs.append((repo_name, tasks_dir))
    else:
        repo_root = _find_repo_root()
        tasks_dirs = [(repo_root.name, repo_root / ".tasks")]

    # Load states (with repo tagging for multi-repo)
    states = load_all_states_multi(tasks_dirs)

    # Error patterns only for single-repo mode
    patterns = load_error_patterns(tasks_dirs[0][1]) if len(tasks_dirs) == 1 else []

    stats = compute_stats(states, patterns)
    cost_stats = compute_cost_stats(states)
    concern_stats = compute_concern_stats(states)
    version_stats = compute_version_stats(states)
    config_delta_stats = compute_config_delta_stats(states)

    # Phase timing only for single-repo (needs interactions.jsonl access)
    phase_timing = compute_phase_timing(tasks_dirs[0][1], states) if len(tasks_dirs) == 1 else {}

    # Task comparison uses first repo's tasks_dir for task.md fallback
    comparison = compute_task_comparison(tasks_dirs[0][1], states)

    if args.json:
        all_stats = {
            **stats,
            "cost": cost_stats,
            "concerns": concern_stats,
            "phase_timing": phase_timing,
            "comparison": comparison,
            "versions": version_stats,
            "config_deltas": config_delta_stats,
        }
        if args.repos:
            all_stats["repo_breakdown"] = compute_repo_breakdown(states)
        print(json.dumps(all_stats, indent=2))
        return

    if not states:
        print("No tasks found in .tasks/")
        return

    if args.compare:
        print_comparison(comparison)
        return

    if args.recent:
        print_recent(states)
        return

    print_dashboard(stats, cost_stats, concern_stats, phase_timing,
                    version_stats, config_delta_stats)
    if comparison:
        print_comparison(comparison)
    if args.repos:
        print_repo_breakdown(compute_repo_breakdown(states))
    print_recent(states)


if __name__ == "__main__":
    main()
