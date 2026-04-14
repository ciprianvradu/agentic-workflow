"""
Configuration Tools for Agentic Workflow MCP Server

Handles YAML configuration cascade merge:
  1. Global defaults:  ~/.claude/ or ~/.copilot/ or ~/.gemini/ or ~/.config/opencode/workflow-config.yaml
  2. Project config:   <repo>/.claude/ or .copilot/ or .gemini/ or .opencode/workflow-config.yaml
  3. Task config:      <repo>/.tasks/TASK_XXX/config.yaml

workflow-config.yaml is the single configuration file at each cascade level.
Each level overrides the previous. Platform directories are checked
in order (.claude first, then .copilot, then .gemini, then .config/opencode),
using whichever exists.
"""

import os
import shutil
import time
from pathlib import Path
from typing import Any, Optional

# Module-level cache: cache_key → (result, timestamp)
# cache_key is a tuple of (file_path, mtime) pairs for all config files in the cascade.
# Per-process only — no cross-session leaking.
_config_cache: dict[tuple, tuple] = {}
_CACHE_TTL = 300  # seconds

try:
    import yaml
except ImportError:
    yaml = None


PERMISSION_PROFILES = {
    "strict": {
        "checkpoints": {
            "planning": {
                "after_architect": True,
                "after_developer": True,
                "after_reviewer": True,
                "after_skeptic": True,
            },
            "implementation": {
                "at_25_percent": True,
                "at_50_percent": True,
                "at_75_percent": True,
                "before_commit": True,
            },
            "documentation": {
                "after_technical_writer": True,
            },
            "quality_guard": {
                "on_deviation": True,
                "on_test_failure": True,
                "on_major_change": True,
            },
        },
        "auto_actions": {
            "run_tests": True,
            "create_files": False,
            "modify_files": False,
            "run_build": True,
            "git_add": False,
            "git_commit": False,
            "git_push": False,
        },
    },
    "standard": {
        "checkpoints": {
            "planning": {
                "after_architect": True,
                "after_developer": False,
                "after_reviewer": True,
                "after_skeptic": True,
            },
            "implementation": {
                "at_25_percent": False,
                "at_50_percent": True,
                "at_75_percent": False,
                "before_commit": True,
            },
            "documentation": {
                "after_technical_writer": True,
            },
            "quality_guard": {
                "on_deviation": True,
                "on_test_failure": True,
                "on_major_change": True,
            },
        },
        "auto_actions": {
            "run_tests": True,
            "create_files": True,
            "modify_files": True,
            "run_build": True,
            "git_add": False,
            "git_commit": False,
            "git_push": False,
        },
    },
    "autonomous": {
        "checkpoints": {
            "planning": {
                "after_architect": False,
                "after_developer": False,
                "after_reviewer": False,
                "after_skeptic": False,
            },
            "implementation": {
                "at_25_percent": False,
                "at_50_percent": False,
                "at_75_percent": False,
                "before_commit": True,
            },
            "documentation": {
                "after_technical_writer": False,
            },
            "quality_guard": {
                "on_deviation": False,
                "on_test_failure": False,
                "on_major_change": False,
            },
        },
        "auto_actions": {
            "run_tests": True,
            "create_files": True,
            "modify_files": True,
            "run_build": True,
            "git_add": True,
            "git_commit": True,
            "git_push": False,
        },
    },
}


DEFAULT_CONFIG = {
    "permission_profile": "standard",
    "checkpoints": {
        "planning": {
            "after_architect": True,
            "after_developer": False,
            "after_reviewer": True,
            "after_skeptic": True
        },
        "implementation": {
            "at_25_percent": False,
            "at_50_percent": True,
            "at_75_percent": False,
            "before_commit": True
        },
        "documentation": {
            "after_technical_writer": True
        },
        "quality_guard": {
            "on_deviation": True,
            "on_test_failure": True,
            "on_major_change": True
        },
        "concern_threshold": 0,
        "concern_severity_threshold": "low",
    },
    "knowledge_base": "docs/ai-context/",
    "task_directory": ".tasks/",
    "max_iterations": {
        "planning": 3,
        "implementation": 5,
        "quality_guard": 2,
        "phase_spawn_attempts": 3
    },
    "models": {
        "default": "opus",
        "orchestrator": "opus",
        "architect": "opus",
        "developer": "opus",
        "planner": "opus",
        "reviewer": "opus",
        "skeptic": "opus",
        "implementer": "opus",
        "quality_guard": "opus",
        "technical-writer": "opus",
        # Mode-specific overrides (user config always wins via deep-merge)
        "quick": {
            "implementer": "sonnet",
        },
        "standard": {
            "planner": "opus",
            "implementer": "sonnet",
            "technical_writer": "sonnet",
        },
        "thorough": {
            "planner": "opus",
            "reviewer": "sonnet",
            "implementer": "sonnet",
            "quality_guard": "sonnet",
            "security_auditor": "sonnet",
            "technical_writer": "sonnet",
        },
    },
    "workflow_modes": {
        "default": "auto",
        "modes": {},
        "auto_detection": {},
    },
    "effort_levels": {},
    "beads": {
        "enabled": False,
        "auto_create_issue": False,
        "auto_link": True,
        "sync_status": True,
        "add_comments": True,
    },
    "worktree": {
        "base_path": "../{repo_name}-worktrees",
        "branch_prefix": "crew/",
        "cleanup_on_complete": "prompt",
        "auto_launch": "prompt",
        "terminal_launch_mode": "auto",
        "ai_host": "auto",
        "copy_settings": True,
        "recycle": "prompt",
        "sync_before_create": "prompt",
        "wsl_native_path": "",
        "install_deps": "auto",
        "jira": {
            "auto_assign": "never",
            "transitions": {
                "on_create": {
                    "to": "",
                    "mode": "auto",
                    "only_from": [],
                },
                "on_complete": {
                    "to": "",
                    "mode": "auto",
                    "only_from": [],
                },
                "on_cleanup": {
                    "to": "",
                    "mode": "prompt",
                    "only_from": [],
                },
            },
        },
        "post_setup_commands": [],
    },
    "auto_actions": {
        "run_tests": True,
        "create_files": True,
        "modify_files": True,
        "run_build": True,
        "git_add": False,
        "git_commit": False,
        "git_push": False
    },
    "loop_mode": {
        "enabled": False,
        "phases": {
            "planning": False,
            "implementation": True,
            "documentation": False
        },
        "completion_promise": "COMPLETE",
        "blocked_promise": "BLOCKED",
        "max_iterations": {
            "per_step": 10,
            "per_phase": 30,
            "before_escalate": 5
        },
        "verification": {
            "method": "tests",
            "custom_command": "",
            "require_all_pass": True
        }
    },
    "documentation": {
        "async_mode": False,
        "async_mode_thorough": False,
        "auto_commit_docs": False,
        "notify_on_complete": True,
    },
    "llm_triage": {
        "enabled": True,
        "confidence_threshold": 0.8,
        "model": "haiku",
        "timeout_seconds": 10,
        "fallback_to_local": True,
    },
    "custom_phases": {},
    "parallelization": {
        "design_challenger_reviewer_skeptic": {
            "enabled": True,
            "timeout_seconds": 300,
            "merge_strategy": "deduplicate",
        },
        "quality_guard_security_auditor": {
            "enabled": True,
            "timeout_seconds": 300,
        },
        "optional_agents": {
            "enabled": True,
            "max_concurrent": 4,
            "timeout_seconds": 300,
        },
        "optional_with_next": {
            "enabled": True,
            "timeout_seconds": 300,
        },
    },
    "host_aware": {
        "enabled": True,
        "skip_exploration": {
            "claude": True,
            "copilot": False,
            "gemini": False,
            "opencode": True,
            "devin": True,
        },
        "planner_mode": "auto",
    },
}


def _get_valid_keys(defaults: dict, prefix: str = "") -> set[str]:
    """Recursively collect all valid keys from defaults."""
    keys = set()
    for key, value in defaults.items():
        full_key = f"{prefix}.{key}" if prefix else key
        keys.add(full_key)
        if isinstance(value, dict):
            keys.update(_get_valid_keys(value, full_key))
    return keys


def _validate_config(config: dict, defaults: dict, prefix: str = "") -> list[str]:
    """Validate config against defaults, returning warnings for unknown keys."""
    warnings = []
    for key, value in config.items():
        full_key = f"{prefix}.{key}" if prefix else key
        if key not in defaults:
            warnings.append(f"Unknown config key: '{full_key}'")
        elif key == "custom_phases":
            # Skip deep validation — subkeys are user-defined phase names
            continue
        elif isinstance(value, dict) and isinstance(defaults.get(key), dict):
            warnings.extend(_validate_config(value, defaults[key], full_key))
        elif value is not None:
            expected_type = type(defaults.get(key))
            if expected_type is not type(None) and not isinstance(value, expected_type):
                if not (expected_type == int and isinstance(value, bool)):
                    warnings.append(
                        f"Invalid type for '{full_key}': expected {expected_type.__name__}, got {type(value).__name__}"
                    )
    return warnings


def _deep_merge(base: dict, override: dict) -> dict:
    result = base.copy()
    for key, value in override.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = _deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def _compute_config_delta(effective: dict, defaults: dict) -> dict:
    """Return only the keys in effective that differ from defaults.

    Recursively walks both dicts. Keys present in effective but absent
    from defaults are included. Keys identical to defaults are omitted.
    Returns a nested dict mirroring the original structure, containing
    only the differences.
    """
    delta: dict = {}
    for key, value in effective.items():
        default_value = defaults.get(key)
        if isinstance(value, dict) and isinstance(default_value, dict):
            sub = _compute_config_delta(value, default_value)
            if sub:
                delta[key] = sub
        elif value != default_value:
            delta[key] = value
    return delta


def config_compute_delta(effective: dict) -> dict:
    """Compute config delta from DEFAULT_CONFIG."""
    return _compute_config_delta(effective, DEFAULT_CONFIG)


def _resolve_permission_profile(config: dict) -> dict:
    """Expand permission_profile into checkpoints and auto_actions.

    The profile acts as the base layer for checkpoints and auto_actions.
    Any keys present in config's checkpoints/auto_actions override the profile.

    Intended usage in config_get_effective():
      1. Build base = DEFAULT_CONFIG with profile_name set
      2. resolved_base = _resolve_permission_profile(base)  ← profile applied
      3. config = _deep_merge(resolved_base, user_overrides) ← user wins

    When called directly (e.g. tests with user-only overrides):
      config should contain only user-set keys (not DEFAULT_CONFIG), so
      profile values fill in the gaps and user keys override the profile.

    Args:
        config: Config dict containing at least "permission_profile"

    Returns:
        New config dict with profile values as base for checkpoints/auto_actions,
        overridden by any keys already present in config
    """
    profile_name = config.get("permission_profile", "standard")
    if profile_name not in PERMISSION_PROFILES:
        # Unknown profile — leave config unchanged
        return config

    profile_values = PERMISSION_PROFILES[profile_name]

    # Profile is base; config keys win on top.
    # We only replace checkpoints and auto_actions sections, leaving other
    # config keys untouched.
    result = config.copy()
    if "checkpoints" in profile_values:
        result["checkpoints"] = _deep_merge(
            profile_values["checkpoints"],
            config.get("checkpoints", {})
        )
    if "auto_actions" in profile_values:
        result["auto_actions"] = _deep_merge(
            profile_values["auto_actions"],
            config.get("auto_actions", {})
        )
    return result


def _load_yaml(path: Path) -> Optional[dict]:
    try:
        if not path.exists():
            return None
    except OSError:
        # On Windows, symlinks with \\?\ prefix paths can raise OSError
        return None

    if yaml is None:
        try:
            with open(path) as f:
                content = f.read()
            import re
            config = {}
            for line in content.split('\n'):
                line = line.strip()
                if line and not line.startswith('#'):
                    match = re.match(r'^(\w+):\s*(.+)$', line)
                    if match:
                        key, value = match.groups()
                        if value.lower() == 'true':
                            config[key] = True
                        elif value.lower() == 'false':
                            config[key] = False
                        elif value.isdigit():
                            config[key] = int(value)
                        else:
                            config[key] = value
            return config if config else None
        except OSError:
            return None

    try:
        with open(path) as f:
            return yaml.safe_load(f)
    except Exception:
        return None


PLATFORM_DIRS = [".claude", ".copilot", ".gemini", ".config/opencode", ".opencode", ".devin", ".config/devin", ".factory"]


def _get_global_config_path() -> Path:
    """Return global config path, checking multiple platform directories."""
    for platform_dir in PLATFORM_DIRS:
        path = Path.home() / platform_dir / "workflow-config.yaml"
        if path.exists():
            return path
    return Path.home() / ".claude" / "workflow-config.yaml"


def _get_project_config_path(project_dir: Optional[str] = None) -> Path:
    """Return project config path, checking multiple platform directories."""
    base = Path(project_dir) if project_dir else Path.cwd()
    for platform_dir in PLATFORM_DIRS:
        path = base / platform_dir / "workflow-config.yaml"
        if path.exists():
            return path
    return base / ".claude" / "workflow-config.yaml"


def _get_task_config_path(task_id: str, project_dir: Optional[str] = None) -> Path:
    base = Path(project_dir) if project_dir else Path.cwd()
    return base / ".tasks" / task_id / "config.yaml"


def _get_file_mtime(path: Path) -> Optional[float]:
    """Return the mtime of a file, or None if it does not exist."""
    try:
        return os.path.getmtime(path)
    except FileNotFoundError:
        return None


def config_get_effective(
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    global _config_cache

    # Resolve candidate paths before checking the cache so we can build the key.
    global_path = _get_global_config_path()
    project_path = _get_project_config_path(project_dir)
    task_path = _get_task_config_path(task_id, project_dir) if task_id else None

    # Build a cache key from (path, mtime) for every config file in the cascade.
    # Files that do not exist contribute a mtime of None, which is still stable
    # unless the file is later created (mtime would then differ).
    cache_key = (
        (str(global_path), _get_file_mtime(global_path)),
        (str(project_path), _get_file_mtime(project_path)),
        (str(task_path), _get_file_mtime(task_path)) if task_path is not None else None,
    )

    now = time.monotonic()
    if cache_key in _config_cache:
        cached_result, cached_at = _config_cache[cache_key]
        if now - cached_at < _CACHE_TTL:
            return cached_result

    # Cache miss or expired — perform the full merge.
    warnings = []
    sources = []

    # Collect all user YAML overrides (without DEFAULT_CONFIG mixed in).
    # This lets _resolve_permission_profile treat profile as base, user values as override.
    user_overrides: dict = {}

    # --- Global level ---
    global_config = _load_yaml(global_path)
    if global_config:
        warnings.extend(_validate_config(global_config, DEFAULT_CONFIG))
        user_overrides = _deep_merge(user_overrides, global_config)
        sources.append(str(global_path))

    # --- Project level ---
    project_config = _load_yaml(project_path)
    if project_config:
        warnings.extend(_validate_config(project_config, DEFAULT_CONFIG))
        user_overrides = _deep_merge(user_overrides, project_config)
        sources.append(str(project_path))

    # --- Task level ---
    task_config = None
    if task_id and task_path is not None:
        task_config = _load_yaml(task_path)
        if task_config:
            warnings.extend(_validate_config(task_config, DEFAULT_CONFIG))
            user_overrides = _deep_merge(user_overrides, task_config)

    if task_path is not None and task_path.exists():
        sources.append(str(task_path))

    # Determine the permission_profile from user YAML (or DEFAULT_CONFIG fallback).
    profile_name = user_overrides.get("permission_profile", DEFAULT_CONFIG.get("permission_profile", "standard"))

    # Build base config: start from DEFAULT_CONFIG, then replace checkpoints and
    # auto_actions with the profile values. This gives us a base where the
    # profile drives permission-related settings rather than DEFAULT_CONFIG.
    if profile_name in PERMISSION_PROFILES:
        profile_values = PERMISSION_PROFILES[profile_name]
        base_config = dict(DEFAULT_CONFIG)
        base_config["permission_profile"] = profile_name
        if "checkpoints" in profile_values:
            base_config["checkpoints"] = _deep_merge(
                DEFAULT_CONFIG.get("checkpoints", {}),
                profile_values["checkpoints"]
            )
        if "auto_actions" in profile_values:
            base_config["auto_actions"] = _deep_merge(
                DEFAULT_CONFIG.get("auto_actions", {}),
                profile_values["auto_actions"]
            )
    else:
        base_config = dict(DEFAULT_CONFIG)

    # Now merge user YAML overrides on top — explicit user keys always win.
    config = _deep_merge(base_config, user_overrides)

    result = {
        "config": config,
        "sources": sources,
        "warnings": warnings,
        "has_global": global_config is not None,
        "has_project": project_config is not None,
        "has_task": task_path is not None and task_path.exists(),
    }

    _config_cache[cache_key] = (result, now)
    return result


def config_get_checkpoint(
    checkpoint: str,
    category: str,
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    effective = config_get_effective(task_id, project_dir)
    config = effective["config"]

    checkpoints = config.get("checkpoints", {})
    category_checkpoints = checkpoints.get(category, {})

    if checkpoint not in category_checkpoints:
        available = list(category_checkpoints.keys())
        return {
            "error": f"Unknown checkpoint '{checkpoint}' in category '{category}'",
            "available_checkpoints": available,
            "category": category
        }

    enabled = category_checkpoints[checkpoint]

    return {
        "checkpoint": checkpoint,
        "category": category,
        "enabled": enabled,
        "sources": effective["sources"]
    }


def config_get_model(
    agent: str,
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    effective = config_get_effective(task_id, project_dir)
    config = effective["config"]

    models = config.get("models", {})

    if agent not in models:
        return {
            "error": f"Unknown agent '{agent}'",
            "available_agents": list(models.keys())
        }

    return {
        "agent": agent,
        "model": models[agent],
        "sources": effective["sources"]
    }


def config_get_auto_action(
    action: str,
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    effective = config_get_effective(task_id, project_dir)
    config = effective["config"]

    auto_actions = config.get("auto_actions", {})

    if action not in auto_actions:
        return {
            "error": f"Unknown auto action '{action}'",
            "available_actions": list(auto_actions.keys())
        }

    return {
        "action": action,
        "allowed": auto_actions[action],
        "sources": effective["sources"]
    }


def config_get_loop_mode(
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    effective = config_get_effective(task_id, project_dir)
    config = effective["config"]

    loop_mode = config.get("loop_mode", {})

    return {
        "enabled": loop_mode.get("enabled", False),
        "phases": loop_mode.get("phases", {}),
        "max_iterations": loop_mode.get("max_iterations", {}),
        "verification": loop_mode.get("verification", {}),
        "sources": effective["sources"]
    }


def _is_beads_installed() -> bool:
    """Check if beads is installed and available."""
    # Check for beads CLI
    if shutil.which("beads") or shutil.which("bd"):
        return True

    # Check for beads-mcp in PATH
    if shutil.which("beads-mcp"):
        return True

    # Check for .beads directory in current project (beads is initialized)
    if (Path.cwd() / ".beads").exists():
        return True

    return False


def _is_beads_initialized() -> bool:
    """Check if beads is initialized in the current project."""
    return (Path.cwd() / ".beads").exists()


def config_get_beads(
    task_id: Optional[str] = None,
    project_dir: Optional[str] = None
) -> dict[str, Any]:
    """Get beads configuration with auto-detection support.

    If beads.enabled is set to "auto", will check if beads is installed
    and initialized in the current project.

    Returns:
        Beads configuration with resolved enabled status
    """
    effective = config_get_effective(task_id, project_dir)
    config = effective["config"]

    beads_config = config.get("beads", {})
    enabled_setting = beads_config.get("enabled", False)

    # Handle auto-detection
    if enabled_setting == "auto":
        beads_installed = _is_beads_installed()
        beads_initialized = _is_beads_initialized()
        enabled = beads_installed and beads_initialized
        detection_info = {
            "mode": "auto",
            "beads_installed": beads_installed,
            "beads_initialized": beads_initialized,
            "resolved_to": enabled
        }
    else:
        enabled = bool(enabled_setting)
        detection_info = {
            "mode": "manual",
            "configured_value": enabled_setting
        }

    return {
        "enabled": enabled,
        "auto_create_issue": beads_config.get("auto_create_issue", False),
        "auto_link": beads_config.get("auto_link", True),
        "sync_status": beads_config.get("sync_status", True),
        "add_comments": beads_config.get("add_comments", True),
        "detection": detection_info,
        "sources": effective["sources"]
    }
