#!/usr/bin/env python3
"""
Display current workflow configuration.

Reads workflow-config.yaml from global + project paths and prints
a formatted summary. Deterministic — no LLM needed.

Usage:
    python3 scripts/crew-config.py           # Show current config
    python3 scripts/crew-config.py --json    # Output raw JSON
"""

import json
import sys
from pathlib import Path

try:
    import yaml
except ImportError:
    yaml = None  # type: ignore

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


def _load_yaml(path: Path) -> dict:
    if not path.exists():
        return {}
    text = path.read_text()
    if yaml:
        return yaml.safe_load(text) or {}
    # Fallback: try json (some configs are json-compatible)
    try:
        return json.loads(text)
    except Exception:
        return {}


def _deep_merge(base: dict, override: dict) -> dict:
    result = dict(base)
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = _deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def _detect_platform_dir() -> str:
    for d in [".claude", ".copilot", ".gemini", ".opencode"]:
        if (Path.home() / d).exists():
            return d
    return ".claude"


def _bool_icon(val) -> str:
    if val is True:
        return "  [ON]"
    if val is False:
        return "  [OFF]"
    return f"  [{val}]"


# ── Main ─────────────────────────────────────────────────────────────────────

def load_effective_config() -> tuple[dict, list[str]]:
    """Load config with cascade and return (config, sources)."""
    sources = []
    config: dict = {}

    # Level 1: Global config
    platform_dir = _detect_platform_dir()
    global_path = Path.home() / platform_dir / "workflow-config.yaml"
    if global_path.exists():
        config = _deep_merge(config, _load_yaml(global_path))
        sources.append(f"~/{platform_dir}/workflow-config.yaml")

    # Level 2: Repo bundled config
    repo_root = _find_repo_root()
    bundled_path = repo_root / "config" / "workflow-config.yaml"
    if bundled_path.exists():
        config = _deep_merge(config, _load_yaml(bundled_path))
        sources.append(str(bundled_path.relative_to(repo_root)))

    # Level 3: Project config
    project_path = repo_root / platform_dir / "workflow-config.yaml"
    if project_path.exists():
        config = _deep_merge(config, _load_yaml(project_path))
        sources.append(str(project_path.relative_to(repo_root)))

    if not sources:
        sources.append("(defaults only — no config files found)")

    return config, sources


def print_config(config: dict, sources: list[str]) -> None:
    w = 64
    print("+" + "-" * w + "+")
    print(f"| {'Workflow Configuration':^{w}} |")
    print("+" + "-" * w + "+")
    print(f"| {'Sources:':^{w}} |")
    for s in sources:
        print(f"|   {s:<{w-2}} |")
    print("+" + "-" * w + "+")

    # Permission Profile
    profile = config.get("permission_profile", "standard")
    print(f"|  {'PERMISSION PROFILE':<{w-1}}|")
    print(f"|    {'Active profile:':<30}{profile:>{w-35}}|")
    print(f"|    {'(strict | standard | autonomous)':<{w-1}}|")
    print("|" + " " * w + "|")

    # Checkpoints
    cp = config.get("checkpoints", {})
    print(f"|  {'PLANNING CHECKPOINTS':<{w-1}}|")
    planning = cp.get("planning", {})
    for key in ["after_architect", "after_developer", "after_reviewer", "after_skeptic"]:
        val = planning.get(key, "-")
        label = key.replace("_", " ").title()
        print(f"|    {label:<30}{_bool_icon(val):>{w-35}}|")

    impl = cp.get("implementation", {})
    print(f"|  {'IMPLEMENTATION CHECKPOINTS':<{w-1}}|")
    for key in ["at_25_percent", "at_50_percent", "at_75_percent", "before_commit"]:
        val = impl.get(key, "-")
        label = key.replace("_", " ").title()
        print(f"|    {label:<30}{_bool_icon(val):>{w-35}}|")

    print("|" + " " * w + "|")

    # Models
    models = config.get("models", {})
    print(f"|  {'MODELS':<{w-1}}|")
    default_model = models.get("default", "opus")
    print(f"|    {'Default:':<30}{default_model:>{w-35}}|")
    orchestrator = models.get("orchestrator", default_model)
    print(f"|    {'Orchestrator:':<30}{orchestrator:>{w-35}}|")
    for mode in ["standard", "reviewed", "thorough"]:
        mode_models = models.get(mode, {})
        if mode_models:
            print(f"|    {mode.title() + ' mode:':<{w-1}}|")
            for agent, model in mode_models.items():
                print(f"|      {agent:<28}{model:>{w-37}}|")

    print("|" + " " * w + "|")

    # Workflow Modes
    wm = config.get("workflow_modes", {})
    print(f"|  {'WORKFLOW MODES':<{w-1}}|")
    default_mode = wm.get("default", "auto")
    print(f"|    {'Default mode:':<30}{default_mode:>{w-35}}|")
    modes_def = wm.get("modes", {})
    for name, mdef in modes_def.items():
        phases = mdef.get("phases", [])
        cost = mdef.get("estimated_cost", "?")
        desc = mdef.get("description", "")[:40]
        print(f"|    {name:<12} {cost:<8} {desc:>{w-27}}|")

    print("|" + " " * w + "|")

    # Subagent Limits
    sl = config.get("subagent_limits", {})
    if sl:
        print(f"|  {'SUBAGENT LIMITS':<{w-1}}|")
        turns = sl.get("max_turns", {})
        for key, val in turns.items():
            label = key.replace("_", " ").title()
            print(f"|    {label:<30}{str(val) + ' turns':>{w-35}}|")
        timeout = sl.get("agent_timeout", 300)
        print(f"|    {'Timeout:':<30}{str(timeout) + 's':>{w-35}}|")

    print("|" + " " * w + "|")

    # Worktree
    wt = config.get("worktree", {})
    if wt:
        print(f"|  {'WORKTREE':<{w-1}}|")
        print(f"|    {'Base path:':<30}{wt.get('base_path', '-'):>{w-35}}|")
        print(f"|    {'Branch prefix:':<30}{wt.get('branch_prefix', 'crew/'):>{w-35}}|")
        print(f"|    {'Cleanup:':<30}{wt.get('cleanup_on_complete', 'prompt'):>{w-35}}|")
        print(f"|    {'AI host:':<30}{wt.get('ai_host', 'auto'):>{w-35}}|")

    print("|" + " " * w + "|")

    # Cost Tracking
    ct = config.get("cost_tracking", {})
    print(f"|  {'COST TRACKING':<{w-1}}|")
    print(f"|    {'Enabled:':<30}{_bool_icon(ct.get('enabled', True)):>{w-35}}|")

    # Beads
    beads = config.get("beads", {})
    print(f"|  {'BEADS':<{w-1}}|")
    print(f"|    {'Enabled:':<30}{_bool_icon(beads.get('enabled', 'auto')):>{w-35}}|")

    print("+" + "-" * w + "+")

    # Paths
    print(f"\n  Knowledge base: {config.get('knowledge_base', 'docs/ai-context/')}")
    print(f"  Task directory: {config.get('task_directory', '.tasks/')}")


def main():
    config, sources = load_effective_config()

    if "--json" in sys.argv:
        print(json.dumps(config, indent=2))
        return

    print_config(config, sources)


if __name__ == "__main__":
    main()
