"""
Tests for Custom Phases (lifecycle hooks)

Run with: pytest tests/test_custom_phases.py -v
"""

import json
import shutil
import pytest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).parent.parent))

from agentic_workflow_server.orchestration_tools import (
    crew_get_next_phase,
    _evaluate_custom_phase_condition,
    _load_custom_phases,
    _insert_custom_phases_into_sequence,
)
from agentic_workflow_server.state_tools import (
    get_tasks_dir,
    workflow_initialize,
    workflow_transition,
    workflow_complete_phase,
    workflow_set_mode,
    _load_state,
    _save_state,
)


@pytest.fixture
def clean_tasks_dir():
    """Clean up .tasks directory before and after tests."""
    tasks_dir = get_tasks_dir()
    for d in tasks_dir.glob("TASK_CP_*"):
        if d.is_dir():
            shutil.rmtree(d)
    yield tasks_dir
    for d in tasks_dir.glob("TASK_CP_*"):
        if d.is_dir():
            shutil.rmtree(d)


# ============================================================================
# _evaluate_custom_phase_condition tests
# ============================================================================

class TestEvaluateCondition:
    def test_empty_condition_returns_true(self):
        assert _evaluate_custom_phase_condition({}, "any task", "reviewed") is True

    def test_none_condition_returns_true(self):
        assert _evaluate_custom_phase_condition(None, "any task", "reviewed") is True

    def test_always_true(self):
        assert _evaluate_custom_phase_condition(
            {"always": True}, "any task", "reviewed"
        ) is True

    def test_task_has_match(self):
        assert _evaluate_custom_phase_condition(
            {"task_has": "jira_key"}, "Fix SAD-123 jira_key issue", "reviewed"
        ) is True

    def test_task_has_no_match(self):
        assert _evaluate_custom_phase_condition(
            {"task_has": "jira_key"}, "Fix typo in README", "reviewed"
        ) is False

    def test_task_has_case_insensitive(self):
        assert _evaluate_custom_phase_condition(
            {"task_has": "JIRA_KEY"}, "has jira_key in it", "reviewed"
        ) is True

    def test_mode_in_match(self):
        assert _evaluate_custom_phase_condition(
            {"mode_in": ["thorough", "reviewed"]}, "task", "reviewed"
        ) is True

    def test_mode_in_no_match(self):
        assert _evaluate_custom_phase_condition(
            {"mode_in": ["thorough"]}, "task", "standard"
        ) is False

    def test_mode_in_alias_match(self):
        """full is an alias for thorough — should match."""
        assert _evaluate_custom_phase_condition(
            {"mode_in": ["full"]}, "task", "thorough"
        ) is True

    def test_file_patterns_match(self):
        assert _evaluate_custom_phase_condition(
            {"file_patterns": ["**/auth/**"]},
            "task",
            "reviewed",
            files_affected=["src/auth/login.ts"],
        ) is True

    def test_file_patterns_no_match(self):
        assert _evaluate_custom_phase_condition(
            {"file_patterns": ["**/auth/**"]},
            "task",
            "reviewed",
            files_affected=["src/utils/helper.ts"],
        ) is False

    def test_file_patterns_no_files(self):
        assert _evaluate_custom_phase_condition(
            {"file_patterns": ["**/auth/**"]},
            "task",
            "reviewed",
            files_affected=[],
        ) is False

    def test_combined_conditions_all_met(self):
        assert _evaluate_custom_phase_condition(
            {"task_has": "auth", "mode_in": ["thorough"]},
            "Fix auth token issue",
            "thorough",
        ) is True

    def test_combined_conditions_one_fails(self):
        assert _evaluate_custom_phase_condition(
            {"task_has": "auth", "mode_in": ["thorough"]},
            "Fix auth token issue",
            "standard",
        ) is False


# ============================================================================
# _load_custom_phases tests
# ============================================================================

class TestLoadCustomPhases:
    def test_empty_config(self):
        assert _load_custom_phases({}) == {}

    def test_no_custom_phases_key(self):
        assert _load_custom_phases({"checkpoints": {}}) == {}

    def test_valid_skill_phase(self):
        config = {
            "custom_phases": {
                "triage": {
                    "after": "init",
                    "type": "skill",
                    "skill": "evaluate-jira",
                    "condition": {"task_has": "jira"},
                    "writes_to_state": True,
                }
            }
        }
        result = _load_custom_phases(config)
        assert "triage" in result
        assert result["triage"]["type"] == "skill"
        assert result["triage"]["skill"] == "evaluate-jira"
        assert result["triage"]["writes_to_state"] is True

    def test_valid_script_phase(self):
        config = {
            "custom_phases": {
                "lint_check": {
                    "before": "complete",
                    "type": "script",
                    "command": "python3 scripts/lint.py {task_id}",
                }
            }
        }
        result = _load_custom_phases(config)
        assert "lint_check" in result
        assert result["lint_check"]["type"] == "script"
        assert result["lint_check"]["blocking"] is True  # default

    def test_valid_agent_phase(self):
        config = {
            "custom_phases": {
                "custom_review": {
                    "after": "developer",
                    "type": "agent",
                    "prompt_file": "agents/custom-review.md",
                }
            }
        }
        result = _load_custom_phases(config)
        assert "custom_review" in result
        assert result["custom_review"]["type"] == "agent"

    def test_invalid_type_skipped(self):
        config = {
            "custom_phases": {
                "bad": {
                    "after": "init",
                    "type": "invalid",
                }
            }
        }
        assert _load_custom_phases(config) == {}

    def test_missing_position_skipped(self):
        config = {
            "custom_phases": {
                "bad": {
                    "type": "skill",
                    "skill": "test",
                }
            }
        }
        assert _load_custom_phases(config) == {}

    def test_skill_without_skill_name_skipped(self):
        config = {
            "custom_phases": {
                "bad": {
                    "after": "init",
                    "type": "skill",
                }
            }
        }
        assert _load_custom_phases(config) == {}

    def test_script_without_command_skipped(self):
        config = {
            "custom_phases": {
                "bad": {
                    "after": "init",
                    "type": "script",
                }
            }
        }
        assert _load_custom_phases(config) == {}


# ============================================================================
# _insert_custom_phases_into_sequence tests
# ============================================================================

class TestInsertCustomPhases:
    def _make_phase(self, **overrides):
        """Helper to create a normalized custom phase config."""
        defaults = {
            "type": "skill",
            "after": None,
            "before": None,
            "skill": "test-skill",
            "command": None,
            "prompt_file": None,
            "condition": {},
            "writes_to_state": False,
            "blocking": True,
            "timeout": 120,
        }
        defaults.update(overrides)
        return defaults

    def test_no_custom_phases(self):
        seq = ["architect", "developer", "implementer"]
        result = _insert_custom_phases_into_sequence(seq, {}, "task", "reviewed")
        assert result == seq

    def test_after_init_inserts_at_start(self):
        custom = {"triage": self._make_phase(after="init")}
        seq = ["architect", "developer"]
        result = _insert_custom_phases_into_sequence(seq, custom, "task", "reviewed")
        assert result[0] == "triage"
        assert result[1] == "architect"

    def test_before_complete_inserts_at_end(self):
        custom = {"final_check": self._make_phase(before="complete", type="script", command="check.sh")}
        seq = ["architect", "developer", "implementer"]
        result = _insert_custom_phases_into_sequence(seq, custom, "task", "reviewed")
        assert result[-1] == "final_check"
        assert result[-2] == "implementer"

    def test_after_specific_phase(self):
        custom = {"post_review": self._make_phase(after="reviewer", type="agent", prompt_file="x.md")}
        seq = ["architect", "developer", "reviewer", "implementer"]
        result = _insert_custom_phases_into_sequence(seq, custom, "task", "reviewed")
        assert result.index("post_review") == result.index("reviewer") + 1

    def test_before_specific_phase(self):
        custom = {"pre_impl": self._make_phase(before="implementer", type="script", command="prep.sh")}
        seq = ["architect", "developer", "implementer"]
        result = _insert_custom_phases_into_sequence(seq, custom, "task", "reviewed")
        assert result.index("pre_impl") == result.index("implementer") - 1

    def test_condition_filters_out_phase(self):
        custom = {
            "triage": self._make_phase(
                after="init",
                condition={"task_has": "jira_key"},
            )
        }
        seq = ["architect", "developer"]
        # "simple fix" does not contain "jira_key"
        result = _insert_custom_phases_into_sequence(seq, custom, "simple fix", "reviewed")
        assert "triage" not in result
        assert result == ["architect", "developer"]

    def test_condition_includes_phase(self):
        custom = {
            "triage": self._make_phase(
                after="init",
                condition={"task_has": "jira"},
            )
        }
        seq = ["architect", "developer"]
        result = _insert_custom_phases_into_sequence(seq, custom, "Fix JIRA issue SAD-123", "reviewed")
        assert result[0] == "triage"


# ============================================================================
# Integration: crew_get_next_phase with custom phases
# ============================================================================

class TestCustomPhaseIntegration:
    def test_custom_phase_in_sequence(self, clean_tasks_dir):
        """A custom phase after init should appear before the first mode phase."""
        task_id = "TASK_CP_001"
        workflow_initialize(task_id=task_id, description="Fix jira_key issue SAD-123")
        workflow_set_mode(mode="standard", task_id=task_id)

        # Inject custom_phases into the task's config
        task_dir = clean_tasks_dir / task_id
        config_path = task_dir / "config.yaml"
        import yaml
        config_data = {
            "custom_phases": {
                "triage": {
                    "after": "init",
                    "type": "skill",
                    "skill": "evaluate-jira",
                    "condition": {"task_has": "jira_key"},
                    "writes_to_state": True,
                    "blocking": True,
                }
            }
        }
        existing = {}
        if config_path.exists():
            with open(config_path) as f:
                existing = yaml.safe_load(f) or {}
        existing.update(config_data)
        with open(config_path, "w") as f:
            yaml.dump(existing, f)

        result = crew_get_next_phase(task_id=task_id)
        # The first phase should be "triage" (custom, after init)
        assert result.get("action") == "run_skill"
        assert result.get("phase") == "triage"
        assert result.get("skill") == "evaluate-jira"

    def test_script_phase_before_complete(self, clean_tasks_dir):
        """A script phase before complete should run after all standard phases."""
        task_id = "TASK_CP_002"
        workflow_initialize(task_id=task_id, description="Add caching layer")
        workflow_set_mode(mode="standard", task_id=task_id)

        task_dir = clean_tasks_dir / task_id
        config_path = task_dir / "config.yaml"
        import yaml
        config_data = {
            "custom_phases": {
                "encoding_check": {
                    "before": "complete",
                    "type": "script",
                    "command": "echo checking {task_id}",
                }
            }
        }
        existing = {}
        if config_path.exists():
            with open(config_path) as f:
                existing = yaml.safe_load(f) or {}
        existing.update(config_data)
        with open(config_path, "w") as f:
            yaml.dump(existing, f)

        # Complete all standard mode phases
        for phase in ["architect", "developer", "implementer", "quality_guard"]:
            workflow_transition(to_phase=phase, task_id=task_id)
            workflow_complete_phase(task_id=task_id)

        result = crew_get_next_phase(task_id=task_id)
        assert result.get("action") == "run_script"
        assert result.get("phase") == "encoding_check"
        assert "TASK_CP_002" in result.get("command", "")

    def test_condition_not_met_skips_phase(self, clean_tasks_dir):
        """When condition is not met, custom phase is not in sequence."""
        task_id = "TASK_CP_004"
        workflow_initialize(task_id=task_id, description="Fix typo in README")
        workflow_set_mode(mode="standard", task_id=task_id)

        task_dir = clean_tasks_dir / task_id
        config_path = task_dir / "config.yaml"
        import yaml
        config_data = {
            "custom_phases": {
                "triage": {
                    "after": "init",
                    "type": "skill",
                    "skill": "evaluate-jira",
                    "condition": {"task_has": "jira_key"},
                }
            }
        }
        existing = {}
        if config_path.exists():
            with open(config_path) as f:
                existing = yaml.safe_load(f) or {}
        existing.update(config_data)
        with open(config_path, "w") as f:
            yaml.dump(existing, f)

        result = crew_get_next_phase(task_id=task_id)
        # Should skip triage (condition not met) and go straight to architect
        assert result.get("action") == "spawn_agent"
        assert result.get("agent") == "architect"

    def test_custom_phase_after_init_with_pretransition(self, clean_tasks_dir):
        """Custom phase after init works even when crew_init_task pre-transitions to first mode phase."""
        task_id = "TASK_CP_005"
        workflow_initialize(task_id=task_id, description="Fix jira_key issue SAD-123")
        workflow_set_mode(mode="standard", task_id=task_id)

        # Simulate what crew_init_task does: pre-transition to first mode phase
        workflow_transition(to_phase="architect", task_id=task_id)

        task_dir = clean_tasks_dir / task_id
        config_path = task_dir / "config.yaml"
        import yaml
        config_data = {
            "custom_phases": {
                "triage": {
                    "after": "init",
                    "type": "skill",
                    "skill": "evaluate-jira",
                    "condition": {"task_has": "jira_key"},
                    "writes_to_state": True,
                    "blocking": True,
                }
            }
        }
        existing = {}
        if config_path.exists():
            with open(config_path) as f:
                existing = yaml.safe_load(f) or {}
        existing.update(config_data)
        with open(config_path, "w") as f:
            yaml.dump(existing, f)

        result = crew_get_next_phase(task_id=task_id)
        # Should return triage (custom phase after init) even though
        # current_phase is already "architect" — fresh start detection
        assert result.get("action") == "run_skill"
        assert result.get("phase") == "triage"


class TestPhaseNameSanitization:
    def test_slash_in_name_rejected(self):
        config = {"custom_phases": {"my/phase": {"after": "init", "type": "skill", "skill": "test"}}}
        assert _load_custom_phases(config) == {}

    def test_dotdot_in_name_rejected(self):
        config = {"custom_phases": {"..phase": {"after": "init", "type": "skill", "skill": "test"}}}
        assert _load_custom_phases(config) == {}

    def test_space_in_name_rejected(self):
        config = {"custom_phases": {"my phase": {"after": "init", "type": "skill", "skill": "test"}}}
        assert _load_custom_phases(config) == {}

    def test_valid_name_with_underscores_accepted(self):
        config = {"custom_phases": {"my_phase_123": {"after": "init", "type": "skill", "skill": "test"}}}
        result = _load_custom_phases(config)
        assert "my_phase_123" in result

    def test_valid_name_with_hyphens_accepted(self):
        config = {"custom_phases": {"my-phase": {"after": "init", "type": "skill", "skill": "test"}}}
        result = _load_custom_phases(config)
        assert "my-phase" in result
