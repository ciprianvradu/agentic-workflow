"""
Crew Definitions for Agentic Workflow MCP Server

A crew definition packages everything the orchestrator needs for a
domain-specific workflow: roles (agents), pipelines (modes), auto-detection
rules, specialized role triggers, and category settings.

The built-in "software-dev" crew provides backward-compatible defaults.
Projects can override or replace it entirely via ``crew:`` in
workflow-config.yaml.

Resolution order:
  1. ``crew:`` in effective config (project/task override)
  2. Synthesized from legacy config keys (workflow_modes, specialized_agents, etc.)
  3. Built-in SOFTWARE_DEV_CREW defaults
"""

from __future__ import annotations

import copy
from pathlib import Path
from typing import Any, Optional


# ============================================================================
# Built-in Software Development Crew (the default)
# ============================================================================

SOFTWARE_DEV_CREW: dict[str, Any] = {
    "name": "software-dev",
    "description": "Multi-agent software development workflow",

    # Every role the engine knows about out of the box
    "roles": {
        "planner": {
            "prompt_file": "planner.md",
            "category": "planning",
            "description": "Creates step-by-step implementation plans",
        },
        "architect": {
            "prompt_file": "architect.md",
            "category": "planning",
            "description": "Analyzes system-wide architectural implications",
        },
        "developer": {
            "prompt_file": "developer.md",
            "category": "planning",
            "description": "Creates detailed implementation plans (legacy)",
        },
        "reviewer": {
            "prompt_file": "reviewer.md",
            "category": "planning",
            "description": "Validates completeness and correctness",
        },
        "skeptic": {
            "prompt_file": "skeptic.md",
            "category": "planning",
            "description": "Stress-tests plans for edge cases and failure modes",
        },
        "implementer": {
            "prompt_file": "implementer.md",
            "category": "execution",
            "description": "Executes implementation plans step by step",
        },
        "quality_guard": {
            "prompt_file": "quality-guard.md",
            "category": "execution",
            "description": "Reviews code quality, reuse, efficiency",
        },
        "technical_writer": {
            "prompt_file": "technical-writer.md",
            "category": "documentation",
            "description": "Maintains AI-context documentation",
        },
        "security_auditor": {
            "prompt_file": "security-auditor.md",
            "category": "planning",
            "description": "Finds vulnerabilities (OWASP Top 10)",
        },
        "performance_analyst": {
            "prompt_file": "performance-analyst.md",
            "category": "planning",
            "description": "Identifies bottlenecks and scalability issues",
        },
        "api_guardian": {
            "prompt_file": "api-guardian.md",
            "category": "planning",
            "description": "Protects API contracts and backward compatibility",
        },
        "accessibility_reviewer": {
            "prompt_file": "accessibility-reviewer.md",
            "category": "planning",
            "description": "Ensures WCAG compliance",
        },
    },

    # Named pipelines (what the system used to call "modes")
    "pipelines": {
        "quick": {
            "description": "Implementer only — typos, one-line fixes, trivial changes",
            "phases": ["implementer"],
            "estimated_cost": "$0.03",
        },
        "standard": {
            "description": "Planner + Implementer + Technical Writer — routine to non-trivial features",
            "phases": ["planner", "implementer", "technical_writer"],
            "estimated_cost": "$0.10",
        },
        "thorough": {
            "description": "Full pipeline with review and security — security, migrations, breaking changes",
            "phases": [
                "planner", "reviewer", "implementer",
                "quality_guard", "security_auditor", "technical_writer",
            ],
            "estimated_cost": "$0.30+",
        },
    },

    # Per-pipeline, per-role effort levels
    "effort_levels": {
        "quick": {
            "implementer": "low",
        },
        "standard": {
            "planner": "high",
            "implementer": "high",
            "technical_writer": "medium",
        },
        "thorough": {
            "planner": "max",
            "reviewer": "high",
            "implementer": "high",
            "quality_guard": "high",
            "security_auditor": "high",
            "technical_writer": "medium",
        },
    },

    # Auto-detection rules for choosing a pipeline
    "auto_detection": {
        "quick": {
            "keywords": [
                "typo", "fix typo", "simple fix", "rename", "update comment",
                "fix import", "fix test", "fix lint", "fix build",
                "fix formatting", "fix whitespace", "fix spelling",
                "bump version", "update version", "add dependency",
                "remove dependency", "update dependency", "update config",
                "toggle flag", "change constant", "delete unused",
                "remove dead code", "one-line", "trivial",
            ],
            "patterns": [
                r"^fix (a |the )?broken test",
                r"^change .+ from .+ to",
                r"^set .+ to",
            ],
            "additive_patterns": [
                r"^add (a |an )?(\w+ )?(field|property|column|attribute|parameter|header|flag)s?\b",
                r"^add .+ to (the |a )?(\/\w+|`.+`|\w+\.\w+)",
            ],
            "exclude_keywords": [
                "security", "auth", "database", "migration", "api", "breaking",
                "authentication", "authorization", "password", "token", "critical",
                "add feature", "implement", "refactor", "create", "build",
            ],
        },
        "standard": {
            "keywords": [
                "typo", "fix typo", "simple fix", "rename", "update comment",
                "fix import", "add feature", "implement", "update", "refactor",
                "add", "create", "build", "utility",
            ],
            "exclude_keywords": [
                "security", "auth", "database", "migration", "api", "breaking",
                "authentication", "authorization", "password", "token", "critical",
            ],
        },
        "thorough": {
            "keywords": [
                "security", "authentication", "authorization", "database",
                "migration", "api", "breaking change", "critical", "auth",
                "password", "token",
            ],
        },
    },

    # Specialized (optional) roles — auto-triggered by task content
    "specialized_roles": {
        "security_auditor": {
            "triggers": {
                "keywords": [
                    "auth", "password", "token", "secret", "sql", "encryption",
                    "security", "csrf", "xss", "injection", "oauth", "jwt",
                    "credential", "permission", "rbac", "acl",
                ],
                "file_patterns": ["**/auth/**", "**/security/**", "**/.env*"],
            },
        },
        "performance_analyst": {
            "triggers": {
                "keywords": [
                    "performance", "cache", "optimize", "slow", "scale",
                    "latency", "throughput", "benchmark", "profil", "memory leak",
                    "n+1", "database index", "query optimization",
                ],
                "file_patterns": ["**/database/**", "**/cache/**"],
            },
        },
        "api_guardian": {
            "triggers": {
                "keywords": [
                    "api", "endpoint", "breaking", "deprecat", "schema",
                    "rest", "graphql", "grpc", "openapi", "swagger",
                    "backward compat", "versioning",
                ],
                "file_patterns": ["**/api/**", "**/routes/**", "**/openapi*"],
            },
        },
        "accessibility_reviewer": {
            "triggers": {
                "keywords": [
                    "ui", "component", "form", "a11y", "wcag", "aria",
                    "screen reader", "keyboard nav", "accessibility",
                    "contrast", "focus",
                ],
                "file_patterns": ["**/*.tsx", "**/*.jsx", "**/*.vue"],
            },
        },
    },

    # Role categories — for turn limits and cost grouping
    "categories": {
        "planning": {"max_turns": 30},
        "execution": {"max_turns": 50},
        "documentation": {"max_turns": 20},
        "consultation": {"max_turns": 15},
    },
}

# Backward-compatible mode aliases (not domain-specific)
MODE_ALIASES: dict[str, str] = {
    "micro": "quick",
    "minimal": "quick",
    "turbo": "standard",
    "fast": "standard",
    "reviewed": "standard",
    "full": "thorough",
}


# ============================================================================
# Crew Definition Resolver
# ============================================================================

def resolve_crew(config: dict) -> dict[str, Any]:
    """Resolve the effective crew definition from config.

    Resolution order:
      1. Explicit ``crew:`` key in config
      2. Synthesized from legacy config keys (workflow_modes, specialized_agents, etc.)
      3. Built-in SOFTWARE_DEV_CREW

    The result is always a complete crew definition — missing sections are
    filled from the built-in default.

    Args:
        config: The effective merged configuration dict.

    Returns:
        Complete crew definition dict.
    """
    crew = config.get("crew")

    if crew and isinstance(crew, dict):
        # Explicit crew definition — merge with defaults for completeness
        return _merge_crew_with_defaults(crew)

    # No explicit crew: section — synthesize from legacy config keys
    return _synthesize_from_legacy(config)


def _merge_crew_with_defaults(crew: dict) -> dict[str, Any]:
    """Merge a partial crew definition with SOFTWARE_DEV_CREW defaults.

    Top-level keys in the user's crew override the default entirely;
    missing keys fall back to the built-in default.

    For nested dicts (roles, pipelines, categories), user values are
    merged into defaults so users only need to specify what they change.
    """
    base = copy.deepcopy(SOFTWARE_DEV_CREW)
    result = {}

    for key in ("name", "description"):
        result[key] = crew.get(key, base[key])

    # For dict-of-dicts sections, deep-merge user into base
    for section in ("roles", "pipelines", "effort_levels",
                    "auto_detection", "specialized_roles", "categories"):
        base_section = base.get(section, {})
        user_section = crew.get(section)
        if user_section is None:
            result[section] = base_section
        elif isinstance(user_section, dict):
            # If the user defines this section, it completely replaces the default
            # UNLESS they set _extend: true
            if user_section.get("_extend", False):
                merged = copy.deepcopy(base_section)
                # Merge user keys into base, excluding the _extend directive
                _deep_merge_dict(merged, {k: v for k, v in user_section.items() if k != "_extend"})
                result[section] = merged
            else:
                result[section] = user_section
        else:
            result[section] = base_section

    return result


def _synthesize_from_legacy(config: dict) -> dict[str, Any]:
    """Build a crew definition from legacy config keys.

    Maps existing workflow-config.yaml keys onto the crew definition
    structure so all existing configs keep working without changes.
    """
    crew = copy.deepcopy(SOFTWARE_DEV_CREW)

    # workflow_modes.modes → pipelines
    wm = config.get("workflow_modes", {})
    custom_modes = wm.get("modes", {})
    if custom_modes:
        for mode_name, mode_cfg in custom_modes.items():
            if isinstance(mode_cfg, dict) and "phases" in mode_cfg:
                crew["pipelines"][mode_name] = mode_cfg

    # specialized_agents → specialized_roles
    sa = config.get("specialized_agents", {})
    if sa:
        for agent_name, agent_cfg in sa.items():
            if isinstance(agent_cfg, dict):
                # Map the legacy structure to crew specialized_roles
                role_def: dict[str, Any] = {}
                if "triggers" in agent_cfg:
                    role_def["triggers"] = agent_cfg["triggers"]
                if "position" in agent_cfg:
                    role_def["position"] = agent_cfg["position"]
                if "checkpoint_after" in agent_cfg:
                    role_def["checkpoint_after"] = agent_cfg["checkpoint_after"]
                if "enabled" in agent_cfg:
                    role_def["enabled"] = agent_cfg["enabled"]
                crew["specialized_roles"][agent_name] = role_def

    # effort_levels (from config)
    el = config.get("effort_levels", {})
    if el:
        for mode_name, agents in el.items():
            if isinstance(agents, dict):
                crew["effort_levels"][mode_name] = agents

    # auto_detection (from config)
    ad = config.get("workflow_modes", {}).get("auto_detection", {})
    if not ad:
        ad = config.get("auto_detection", {})
    if ad:
        for mode_name, rules in ad.items():
            if isinstance(rules, dict):
                crew["auto_detection"][mode_name] = rules

    # subagent_limits → categories
    sl = config.get("subagent_limits", {}).get("max_turns", {})
    if sl:
        # Map old category names to new ones
        legacy_to_new = {
            "planning_agents": "planning",
            "implementation_agents": "execution",
            "documentation_agents": "documentation",
            "consultation_agents": "consultation",
        }
        for old_name, new_name in legacy_to_new.items():
            if old_name in sl:
                crew["categories"].setdefault(new_name, {})["max_turns"] = sl[old_name]

    # Discover any roles mentioned in pipelines but not in roles dict
    # (e.g., custom agents like axiom_miner, design_challenger)
    all_pipeline_roles = set()
    for pipeline in crew["pipelines"].values():
        if isinstance(pipeline, dict):
            for phase in pipeline.get("phases", []):
                all_pipeline_roles.add(phase)
    for role_name in all_pipeline_roles:
        if role_name not in crew["roles"]:
            crew["roles"][role_name] = {
                "prompt_file": f"{role_name.replace('_', '-')}.md",
                "category": "planning",
                "description": f"Custom role: {role_name}",
            }

    return crew


# ============================================================================
# Accessor Functions (used by state_tools / orchestration_tools)
# ============================================================================

def get_pipelines(crew: dict) -> dict[str, dict]:
    """Get all pipeline definitions (replaces WORKFLOW_MODES)."""
    return crew.get("pipelines", SOFTWARE_DEV_CREW["pipelines"])


def get_pipeline(crew: dict, name: str) -> Optional[dict]:
    """Get a single pipeline by name, resolving aliases."""
    resolved = MODE_ALIASES.get(name, name)
    pipelines = get_pipelines(crew)
    return pipelines.get(resolved) or pipelines.get(name)


def get_all_pipeline_names(crew: dict) -> list[str]:
    """Get all available pipeline names (canonical + aliases)."""
    names = list(get_pipelines(crew).keys())
    names.extend(MODE_ALIASES.keys())
    return names


def get_roles(crew: dict) -> dict[str, dict]:
    """Get all role definitions (replaces AGENT_PROMPT_FILES)."""
    return crew.get("roles", SOFTWARE_DEV_CREW["roles"])


def get_role_prompt_file(crew: dict, role_name: str) -> str:
    """Get the prompt filename for a role (replaces AGENT_PROMPT_FILES lookup)."""
    roles = get_roles(crew)
    role = roles.get(role_name)
    if role:
        return role.get("prompt_file", f"{role_name.replace('_', '-')}.md")
    # Fallback: derive from name
    return f"{role_name.replace('_', '-')}.md"


def get_role_category(crew: dict, role_name: str) -> str:
    """Get the category for a role (replaces AGENT_LIMIT_CATEGORY lookup)."""
    roles = get_roles(crew)
    role = roles.get(role_name)
    if role:
        return role.get("category", "planning")
    return "planning"


def get_category_max_turns(crew: dict, category: str) -> int:
    """Get max turns for a category (replaces SUBAGENT_LIMITS lookup)."""
    categories = crew.get("categories", SOFTWARE_DEV_CREW["categories"])
    cat_config = categories.get(category, {})
    return cat_config.get("max_turns", 30)


def get_effort_level(crew: dict, pipeline_name: str, role_name: str) -> str:
    """Get effort level for a role in a pipeline (replaces EFFORT_LEVELS lookup)."""
    resolved = MODE_ALIASES.get(pipeline_name, pipeline_name)
    effort_levels = crew.get("effort_levels", SOFTWARE_DEV_CREW["effort_levels"])
    pipeline_efforts = effort_levels.get(resolved, {})
    return pipeline_efforts.get(role_name, "high")


def get_specialized_roles(crew: dict) -> dict[str, dict]:
    """Get specialized (optional) role definitions (replaces OPTIONAL_AGENT_TRIGGERS)."""
    return crew.get("specialized_roles", SOFTWARE_DEV_CREW["specialized_roles"])


def get_auto_detection_rules(crew: dict) -> dict[str, dict]:
    """Get auto-detection rules (replaces AUTO_DETECT_RULES)."""
    return crew.get("auto_detection", SOFTWARE_DEV_CREW["auto_detection"])


def get_phase_order(crew: dict) -> list[str]:
    """Derive a phase order from the crew's roles.

    Returns all known role names in a reasonable order: roles from the
    longest pipeline first, then any remaining roles.
    This replaces the hardcoded PHASE_ORDER.
    """
    # Find the longest pipeline to establish base ordering
    pipelines = get_pipelines(crew)
    longest: list[str] = []
    for p in pipelines.values():
        phases = p.get("phases", [])
        if len(phases) > len(longest):
            longest = list(phases)

    # Add any roles not in the longest pipeline
    all_roles = list(get_roles(crew).keys())
    order = list(longest)
    for role in all_roles:
        if role not in order:
            order.append(role)

    return order


# ============================================================================
# Helpers
# ============================================================================

def _deep_merge_dict(base: dict, override: dict) -> None:
    """Recursively merge override into base, mutating base in place."""
    for key, value in override.items():
        if (
            key in base
            and isinstance(base[key], dict)
            and isinstance(value, dict)
        ):
            _deep_merge_dict(base[key], value)
        else:
            base[key] = value
