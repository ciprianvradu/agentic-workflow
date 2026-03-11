#!/usr/bin/env python3
"""
Display knowledge base health metrics.

Shows coverage, freshness, gap count, and trends for the documentation
in the knowledge base. Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-docs-report.py           # Show report
    python3 scripts/crew-docs-report.py --json     # Output raw JSON
"""

import json
import os
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
    return _find_repo_root() / ".tasks"


def _find_kb_dirs(repo_root: Path) -> list[Path]:
    """Find all ai-context directories in the repo."""
    dirs = []
    primary = repo_root / "docs" / "ai-context"
    if primary.is_dir():
        dirs.append(primary)
    # Also check for distributed ai-context dirs (skip worktrees/node_modules)
    skip = {".git", "node_modules", ".tasks", "__pycache__", ".claude"}
    for root, subdirs, _files in os.walk(repo_root):
        subdirs[:] = [d for d in subdirs if d not in skip]
        p = Path(root)
        if p.name == "ai-context" and p != primary and "worktree" not in str(p):
            dirs.append(p)
    return dirs


def _days_since_modified(path: Path) -> int:
    """Days since file was last modified."""
    try:
        mtime = datetime.fromtimestamp(path.stat().st_mtime)
        return (datetime.now() - mtime).days
    except OSError:
        return -1


# ── Metrics ──────────────────────────────────────────────────────────────────

def compute_doc_metrics(repo_root: Path, tasks_dir: Path) -> dict:
    """Compute documentation health metrics."""
    kb_dirs = _find_kb_dirs(repo_root)

    # Inventory all doc files
    doc_files = []
    for kb_dir in kb_dirs:
        for f in sorted(kb_dir.rglob("*.md")):
            rel = str(f.relative_to(repo_root))
            days = _days_since_modified(f)
            size = f.stat().st_size if f.exists() else 0
            doc_files.append({
                "path": rel,
                "days_since_update": days,
                "size_bytes": size,
            })

    # Collect all source files from recent tasks
    source_modules = set()
    unfilled_gaps = set()
    if tasks_dir.exists():
        for d in sorted(tasks_dir.iterdir()):
            if not d.is_dir() or d.name.startswith("."):
                continue
            sf = d / "state.json"
            if sf.exists():
                try:
                    state = json.loads(sf.read_text())
                    for f in state.get("docs_needed", []):
                        unfilled_gaps.add(f)
                    # Track implementation progress files
                    impl = state.get("implementation_progress", {})
                    for step in impl.get("steps", []):
                        if step.get("file"):
                            source_modules.add(step["file"])
                except Exception:
                    continue

    # Coverage: what fraction of source modules have docs?
    documented_modules = set()
    for doc in doc_files:
        # A doc "covers" source if its name matches a module concept
        doc_stem = Path(doc["path"]).stem.lower()
        documented_modules.add(doc_stem)

    # Freshness stats
    if doc_files:
        ages = [d["days_since_update"] for d in doc_files if d["days_since_update"] >= 0]
        avg_age = round(sum(ages) / len(ages), 1) if ages else 0
        max_age = max(ages) if ages else 0
        stale_count = sum(1 for a in ages if a > 30)
    else:
        avg_age = 0
        max_age = 0
        stale_count = 0

    # Gap analysis — docs_needed files not yet addressed
    resolved_gaps = set()
    for doc in doc_files:
        doc_path = doc["path"]
        if doc_path in unfilled_gaps:
            resolved_gaps.add(doc_path)
    remaining_gaps = unfilled_gaps - resolved_gaps

    return {
        "knowledge_base_dirs": [str(d.relative_to(repo_root)) for d in kb_dirs],
        "total_docs": len(doc_files),
        "docs": doc_files,
        "freshness": {
            "avg_days": avg_age,
            "max_days": max_age,
            "stale_count": stale_count,
            "stale_threshold_days": 30,
        },
        "gaps": {
            "total_flagged": len(unfilled_gaps),
            "remaining": len(remaining_gaps),
            "remaining_files": sorted(remaining_gaps),
        },
    }


def load_metrics_history(tasks_dir: Path) -> list[dict]:
    """Load historical metrics snapshots."""
    mf = tasks_dir / "_doc_metrics.jsonl"
    if not mf.exists():
        return []
    entries = []
    for line in mf.read_text().splitlines():
        line = line.strip()
        if line:
            try:
                entries.append(json.loads(line))
            except Exception:
                continue
    return entries


# ── Display ──────────────────────────────────────────────────────────────────

def print_report(metrics: dict, history: list[dict]) -> None:
    kb_dirs = metrics["knowledge_base_dirs"]
    print(f"\nKnowledge Base: {', '.join(kb_dirs) if kb_dirs else '(none found)'}")
    print(f"  Total docs: {metrics['total_docs']}")

    # Freshness
    fresh = metrics["freshness"]
    print(f"\nFreshness:")
    print(f"  Avg age:     {fresh['avg_days']} days")
    print(f"  Oldest:      {fresh['max_days']} days")
    print(f"  Stale (>30d): {fresh['stale_count']}")

    # Doc inventory
    if metrics["docs"]:
        print(f"\nDocuments:")
        for doc in metrics["docs"]:
            age = doc["days_since_update"]
            size_kb = doc["size_bytes"] / 1024
            marker = " (!)" if age > 30 else ""
            print(f"  {doc['path']:<50} {age:>3}d  {size_kb:.1f}kb{marker}")

    # Gaps
    gaps = metrics["gaps"]
    print(f"\nDoc Gaps:")
    print(f"  Total flagged:  {gaps['total_flagged']}")
    print(f"  Remaining:      {gaps['remaining']}")
    if gaps["remaining_files"]:
        for f in gaps["remaining_files"][:10]:
            print(f"    - {f}")

    # Trends
    if len(history) >= 2:
        latest = history[-1]
        prev = history[-2]
        doc_delta = latest.get("total_docs", 0) - prev.get("total_docs", 0)
        gap_delta = latest.get("remaining_gaps", 0) - prev.get("remaining_gaps", 0)
        print(f"\nTrends (vs previous snapshot):")
        print(f"  Docs:  {'+' if doc_delta >= 0 else ''}{doc_delta}")
        print(f"  Gaps:  {'+' if gap_delta >= 0 else ''}{gap_delta}")


def main():
    repo_root = _find_repo_root()
    tasks_dir = _find_tasks_dir()
    metrics = compute_doc_metrics(repo_root, tasks_dir)
    history = load_metrics_history(tasks_dir)

    if "--json" in sys.argv:
        print(json.dumps(metrics, indent=2))
        return

    if not metrics["docs"]:
        print("No documentation found in knowledge base.")
        return

    print_report(metrics, history)


if __name__ == "__main__":
    main()
