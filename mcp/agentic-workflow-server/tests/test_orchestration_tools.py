"""
Tests for Orchestration Tools

Run with: pytest tests/test_orchestration_tools.py -v
"""

import json
import shutil
import pytest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).parent.parent))

from agentic_workflow_server.orchestration_tools import (
    crew_parse_args,
    crew_init_task,
    crew_apply_config_overrides,
    crew_detect_optional_agents,
    crew_get_next_phase,
    crew_parse_agent_output,
    crew_get_implementation_action,
    crew_format_completion,
    crew_get_resume_state,
    _slugify,
    _generate_branch_name,
    _tokenize,
)
from agentic_workflow_server.state_tools import (
    get_tasks_dir,
    workflow_initialize,
    workflow_transition,
    workflow_complete_phase,
    workflow_set_mode,
    workflow_set_implementation_progress,
    workflow_complete_step,
    workflow_guard_acquire,
    workflow_guard_release,
    _slugify as state_slugify,
    _generate_branch_name as state_generate_branch_name,
)


@pytest.fixture
def clean_tasks_dir():
    """Clean up .tasks directory before and after tests."""
    tasks_dir = get_tasks_dir()

    for pattern in ["TASK_TEST_*", "TASK_ORCH_*"]:
        for d in tasks_dir.glob(pattern):
            if d.is_dir():
                shutil.rmtree(d)

    yield tasks_dir

    for pattern in ["TASK_TEST_*", "TASK_ORCH_*"]:
        for d in tasks_dir.glob(pattern):
            if d.is_dir():
                shutil.rmtree(d)


# ============================================================================
# crew_parse_args tests
# ============================================================================

class TestCrewParseArgs:
    def test_simple_task(self):
        result = crew_parse_args('"Fix typo in README"')
        assert result["action"] == "start"
        assert result["task_description"] == "Fix typo in README"
        assert result["errors"] == []

    def test_task_with_mode(self):
        result = crew_parse_args('--mode turbo "Add logout button"')
        assert result["action"] == "start"
        assert result["options"]["mode"] == "turbo"
        assert result["task_description"] == "Add logout button"

    def test_task_with_loop_mode(self):
        result = crew_parse_args('--loop-mode "Fix all failing tests"')
        assert result["options"]["loop_mode"] is True

    def test_task_with_no_loop(self):
        result = crew_parse_args('--no-loop "Simple task"')
        assert result["options"]["loop_mode"] is False

    def test_task_with_max_iterations(self):
        result = crew_parse_args('--max-iterations 50 "Big refactor"')
        assert result["options"]["max_iterations"] == 50

    def test_task_with_verify(self):
        result = crew_parse_args('--verify tests "Fix tests"')
        assert result["options"]["verify"] == "tests"

    def test_task_with_no_checkpoints(self):
        result = crew_parse_args('--no-checkpoints "Overnight task"')
        assert result["options"]["no_checkpoints"] is True

    def test_task_with_parallel(self):
        result = crew_parse_args('--parallel "Add caching"')
        assert result["options"]["parallel"] is True

    def test_task_with_beads(self):
        result = crew_parse_args('--beads PROJ-42 "Add caching"')
        assert result["options"]["beads"] == "PROJ-42"

    def test_task_with_task_file(self):
        result = crew_parse_args('--task ./tasks/implement-caching.md')
        assert result["options"]["task_file"] == "./tasks/implement-caching.md"

    def test_task_with_config_file(self):
        result = crew_parse_args('--config ./my-config.yaml "Simple task"')
        assert result["options"]["config_file"] == "./my-config.yaml"

    def test_multiple_options(self):
        result = crew_parse_args('--mode fast --loop-mode --no-checkpoints --beads API-42 "Add caching layer"')
        assert result["options"]["mode"] == "fast"
        assert result["options"]["loop_mode"] is True
        assert result["options"]["no_checkpoints"] is True
        assert result["options"]["beads"] == "API-42"
        assert result["task_description"] == "Add caching layer"

    def test_resume_action(self):
        result = crew_parse_args('resume TASK_042')
        assert result["action"] == "resume"
        assert result["task_id"] == "TASK_042"

    def test_status_action(self):
        result = crew_parse_args('status')
        assert result["action"] == "status"

    def test_proceed_action(self):
        result = crew_parse_args('proceed')
        assert result["action"] == "proceed"

    def test_config_action(self):
        result = crew_parse_args('config')
        assert result["action"] == "config"

    def test_ask_action(self):
        result = crew_parse_args('ask architect "Should we use Redis or Memcached?"')
        assert result["action"] == "ask"
        assert result["agent"] == "architect"
        assert result["task_description"] == "Should we use Redis or Memcached?"

    def test_ask_with_context(self):
        result = crew_parse_args('ask reviewer "Review this" --context src/auth/')
        assert result["action"] == "ask"
        assert result["agent"] == "reviewer"
        assert result["options"]["context"] == "src/auth/"

    def test_ask_with_diff(self):
        result = crew_parse_args('ask developer "How to structure?" --diff')
        assert result["options"]["diff"] is True

    def test_invalid_mode(self):
        result = crew_parse_args('--mode invalid "Task"')
        assert len(result["errors"]) > 0
        assert "Invalid mode" in result["errors"][0]

    def test_empty_args(self):
        result = crew_parse_args('')
        assert len(result["errors"]) > 0

    def test_start_prefix(self):
        result = crew_parse_args('start "Add feature"')
        assert result["action"] == "start"
        assert result["task_description"] == "Add feature"

    def test_unknown_option(self):
        result = crew_parse_args('--unknown-flag "Task"')
        assert len(result["errors"]) > 0
        assert "Unknown option" in result["errors"][0]

    def test_options_after_description(self):
        result = crew_parse_args('Add caching --mode fast')
        assert result["task_description"] == "Add caching"
        assert result["options"]["mode"] == "fast"


# ============================================================================
# crew_apply_config_overrides tests
# ============================================================================

class TestCrewApplyConfigOverrides:
    def test_loop_mode_enable(self):
        result = crew_apply_config_overrides({"loop_mode": True})
        assert result["overrides"]["loop_mode"]["enabled"] is True
        assert len(result["applied"]) == 1

    def test_loop_mode_disable(self):
        result = crew_apply_config_overrides({"loop_mode": False})
        assert result["overrides"]["loop_mode"]["enabled"] is False

    def test_max_iterations(self):
        result = crew_apply_config_overrides({"max_iterations": 50})
        assert result["overrides"]["loop_mode"]["max_iterations"]["per_step"] == 50

    def test_verify_method(self):
        result = crew_apply_config_overrides({"verify": "all"})
        assert result["overrides"]["loop_mode"]["verification"]["method"] == "all"

    def test_no_checkpoints(self):
        result = crew_apply_config_overrides({"no_checkpoints": True})
        checkpoints = result["overrides"]["checkpoints"]
        assert checkpoints["planning"]["after_architect"] is False
        assert checkpoints["implementation"]["at_50_percent"] is False
        assert checkpoints["documentation"]["after_technical_writer"] is False

    def test_parallel(self):
        result = crew_apply_config_overrides({"parallel": True})
        assert result["overrides"]["parallelization"]["reviewer_skeptic"]["enabled"] is True

    def test_beads(self):
        result = crew_apply_config_overrides({"beads": "PROJ-42"})
        assert result["overrides"]["beads"]["enabled"] is True
        assert result["overrides"]["beads"]["linked_issue"] == "PROJ-42"

    def test_empty_options(self):
        result = crew_apply_config_overrides({})
        assert result["overrides"] == {}
        assert result["applied"] == []

    def test_multiple_overrides(self):
        result = crew_apply_config_overrides({
            "loop_mode": True,
            "max_iterations": 20,
            "no_checkpoints": True
        })
        assert len(result["applied"]) == 3


# ============================================================================
# crew_detect_optional_agents tests
# ============================================================================

class TestCrewDetectOptionalAgents:
    def test_security_keywords(self):
        result = crew_detect_optional_agents("Add JWT authentication with password hashing")
        assert "security_auditor" in result["enabled"]
        assert "security_auditor" in result["reasons"]

    def test_performance_keywords(self):
        result = crew_detect_optional_agents("Optimize database cache performance")
        assert "performance_analyst" in result["enabled"]

    def test_api_keywords(self):
        result = crew_detect_optional_agents("Add new REST API endpoint")
        assert "api_guardian" in result["enabled"]

    def test_accessibility_keywords(self):
        result = crew_detect_optional_agents("Build new UI component with form validation")
        assert "accessibility_reviewer" in result["enabled"]

    def test_no_matches(self):
        result = crew_detect_optional_agents("Fix typo in README")
        assert result["enabled"] == []
        assert len(result["skipped"]) == 4

    def test_multiple_matches(self):
        result = crew_detect_optional_agents("Add authentication API endpoint with UI form")
        assert "security_auditor" in result["enabled"]
        assert "api_guardian" in result["enabled"]
        assert "accessibility_reviewer" in result["enabled"]


# ============================================================================
# crew_parse_agent_output tests
# ============================================================================

class TestCrewParseAgentOutput:
    def test_parse_docs_needed(self):
        output = '''
        Analysis complete.
        <docs_needed>["docs/api.md", "docs/auth.md"]</docs_needed>
        '''
        result = crew_parse_agent_output("architect", output)
        assert result["extracted"]["docs_needed"] == ["docs/api.md", "docs/auth.md"]
        assert result["has_blocking_issues"] is False

    def test_parse_review_issues(self):
        output = '''
        <review_issues>[{"description": "Missing error handling", "severity": "high"}]</review_issues>
        <recommendation>REVISE</recommendation>
        '''
        result = crew_parse_agent_output("reviewer", output)
        assert len(result["extracted"]["review_issues"]) == 1
        assert result["extracted"]["recommendation"] == "REVISE"
        assert result["has_blocking_issues"] is True

    def test_parse_approve_recommendation(self):
        output = '''
        <recommendation>APPROVE</recommendation>
        '''
        result = crew_parse_agent_output("reviewer", output)
        assert result["extracted"]["recommendation"] == "APPROVE"
        assert result["has_blocking_issues"] is False

    def test_parse_concerns(self):
        output = '''
        <concerns>[{"description": "Race condition possible", "severity": "high"}]</concerns>
        '''
        result = crew_parse_agent_output("skeptic", output)
        assert len(result["extracted"]["concerns"]) == 1
        assert result["has_blocking_issues"] is False

    def test_no_structured_output(self):
        output = "Just some plain text analysis."
        result = crew_parse_agent_output("architect", output)
        assert result["extracted"] == {}
        assert result["has_blocking_issues"] is False

    def test_invalid_json_in_tags(self):
        output = '<docs_needed>[invalid json]</docs_needed>'
        result = crew_parse_agent_output("architect", output)
        assert "docs_needed_parse_error" in result["extracted"]

    def test_string_concerns(self):
        output = '<concerns>["Race condition", "Memory leak"]</concerns>'
        result = crew_parse_agent_output("skeptic", output)
        assert len(result["extracted"]["concerns"]) == 2


# ============================================================================
# Slugify and branch naming tests
# ============================================================================

class TestSlugify:
    def test_basic(self):
        assert _slugify("Hello World") == "hello-world"

    def test_special_chars(self):
        assert _slugify("Add JWT auth!") == "add-jwt-auth"

    def test_underscores(self):
        assert _slugify("my_function_name") == "my-function-name"

    def test_multiple_spaces(self):
        assert _slugify("too   many   spaces") == "too-many-spaces"

    def test_leading_trailing(self):
        assert _slugify("  -hello- ") == "hello"

    def test_empty(self):
        assert _slugify("") == ""


class TestGenerateBranchName:
    def test_linked_issue(self):
        state = {"linked_issue": "PROJ-42"}
        assert _generate_branch_name("TASK_001", state) == "crew/proj-42"

    def test_beads_issue(self):
        state = {"beads_issue": "CACHE-12"}
        assert _generate_branch_name("TASK_001", state) == "crew/cache-12"

    def test_description(self):
        state = {"description": "Add JWT authentication"}
        result = _generate_branch_name("TASK_001", state)
        assert result == "crew/add-jwt-authentication"

    def test_long_description_truncated(self):
        state = {"description": "A" * 100}
        result = _generate_branch_name("TASK_001", state)
        branch_part = result.replace("crew/", "")
        assert len(branch_part) <= 50

    def test_fallback_task_id(self):
        state = {}
        result = _generate_branch_name("TASK_001", state)
        assert result == "crew/task-001"

    def test_linked_issue_priority_over_description(self):
        state = {"linked_issue": "PROJ-42", "description": "Some task"}
        assert _generate_branch_name("TASK_001", state) == "crew/proj-42"


class TestStatToolsSlugifyAndBranch:
    """Test the _slugify and _generate_branch_name added to state_tools.py."""

    def test_slugify(self):
        assert state_slugify("Hello World!") == "hello-world"

    def test_generate_branch_from_linked_issue(self):
        state = {"linked_issue": "API-42"}
        assert state_generate_branch_name("TASK_001", state) == "crew/api-42"

    def test_generate_branch_from_description(self):
        state = {"description": "Add caching layer"}
        assert state_generate_branch_name("TASK_001", state) == "crew/add-caching-layer"

    def test_generate_branch_fallback(self):
        state = {}
        assert state_generate_branch_name("TASK_001", state) == "crew/task-001"


# ============================================================================
# Tokenizer tests
# ============================================================================

class TestTokenize:
    def test_basic(self):
        assert _tokenize("hello world") == ["hello", "world"]

    def test_quoted(self):
        assert _tokenize('"hello world" foo') == ["hello world", "foo"]

    def test_single_quoted(self):
        assert _tokenize("'hello world' foo") == ["hello world", "foo"]

    def test_mixed(self):
        assert _tokenize('--mode fast "Add feature"') == ["--mode", "fast", "Add feature"]

    def test_empty(self):
        assert _tokenize("") == []


# ============================================================================
# crew_init_task tests (requires filesystem)
# ============================================================================

class TestCrewInitTask:
    def test_basic_init(self, clean_tasks_dir):
        result = crew_init_task(
            task_description="Fix typo in README",
            options={"mode": "quick"}
        )
        assert result["success"] is True
        assert result["task_id"]
        assert result["mode"] == "quick"

        # Clean up
        workflow_guard_release(task_id=result["task_id"])
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_with_beads(self, clean_tasks_dir):
        result = crew_init_task(
            task_description="Add caching",
            options={"beads": "CACHE-42", "mode": "fast"}
        )
        assert result["success"] is True
        assert result["beads_issue"] == "CACHE-42"

        # Clean up
        workflow_guard_release(task_id=result["task_id"])
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_saves_task_md(self, clean_tasks_dir):
        result = crew_init_task(
            task_description="Add feature X",
            options={"mode": "turbo"}
        )
        task_dir = clean_tasks_dir / result["task_id"]
        assert (task_dir / "task.md").exists()

        # Clean up
        workflow_guard_release(task_id=result["task_id"])
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_acquires_guard(self, clean_tasks_dir):
        """crew_init_task should acquire workflow guard."""
        result = crew_init_task(
            task_description="Test guard acquisition",
            options={"mode": "standard"}
        )
        assert result["success"] is True
        task_id = result["task_id"]

        # Guard should be held — second acquire should fail
        guard_result = workflow_guard_acquire(task_id=task_id)
        assert guard_result["success"] is False
        assert "already active" in guard_result["error"]

        # Clean up
        workflow_guard_release(task_id=task_id)
        task_dir = clean_tasks_dir / task_id
        if task_dir.exists():
            shutil.rmtree(task_dir)


# ============================================================================
# crew_get_next_phase tests (requires filesystem)
# ============================================================================

class TestCrewGetNextPhase:
    def _clear_custom_phases(self, clean_tasks_dir, task_id):
        """Write task-level config to nullify any project-level custom_phases."""
        import yaml
        task_dir = clean_tasks_dir / task_id
        config_path = task_dir / "config.yaml"
        config_data = {}
        if config_path.exists():
            with open(config_path) as f:
                config_data = yaml.safe_load(f) or {}
        # Set project-level custom phases to None so they're skipped by
        # _load_custom_phases (which filters non-dict entries)
        config_data["custom_phases"] = {
            "product_manager": None,
            "ba_designer": None,
        }
        with open(config_path, "w") as f:
            yaml.dump(config_data, f)

    def test_first_phase_full_mode(self, clean_tasks_dir):
        init = workflow_initialize(task_id="TASK_ORCH_001", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_001")
        self._clear_custom_phases(clean_tasks_dir, "TASK_ORCH_001")

        result = crew_get_next_phase(task_id="TASK_ORCH_001")
        # Phase is None after initialize — crew_get_next_phase returns spawn_agent for first phase
        assert result.get("action") == "spawn_agent"
        assert result.get("agent") == "planner"

    def test_next_after_architect(self, clean_tasks_dir):
        init = workflow_initialize(task_id="TASK_ORCH_001B", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_001B")
        workflow_transition(to_phase="architect", task_id="TASK_ORCH_001B")
        workflow_complete_phase(task_id="TASK_ORCH_001B")
        workflow_transition(to_phase="developer", task_id="TASK_ORCH_001B")
        workflow_complete_phase(task_id="TASK_ORCH_001B")

        result = crew_get_next_phase(task_id="TASK_ORCH_001B")
        # Should suggest reviewer next in thorough mode
        assert result.get("action") in ("spawn_agent", "process_output", "checkpoint")

    def test_complete_when_all_done(self, clean_tasks_dir):
        init = workflow_initialize(task_id="TASK_ORCH_002", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_002")

        # Complete all thorough mode phases sequentially
        for phase in ["planner", "reviewer", "implementer", "quality_guard", "security_auditor", "technical_writer"]:
            workflow_transition(to_phase=phase, task_id="TASK_ORCH_002")
            workflow_complete_phase(task_id="TASK_ORCH_002")

        result = crew_get_next_phase(task_id="TASK_ORCH_002")
        assert result["action"] == "complete"

    def test_nonexistent_task(self):
        result = crew_get_next_phase(task_id="TASK_NONEXISTENT")
        assert "error" in result

    def test_quality_guard_parallel_with_security_auditor(self, clean_tasks_dir):
        """quality_guard should carry parallel_with=security_auditor when config enables it."""
        from unittest.mock import patch as mock_patch
        import agentic_workflow_server.orchestration_tools as _orch

        workflow_initialize(task_id="TASK_ORCH_009", description="Test parallel QG+SA")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_009")

        # Complete all phases up to quality_guard
        for phase in ["planner", "reviewer", "implementer"]:
            workflow_transition(to_phase=phase, task_id="TASK_ORCH_009")
            workflow_complete_phase(task_id="TASK_ORCH_009")

        mock_config = {
            "parallelization": {
                "quality_guard_security_auditor": {"enabled": True},
            },
            "models": {"default": "opus", "thorough": {}},
            "subagent_limits": {"max_turns": {}},
            "knowledge_base": "docs/ai-context/",
            "task_directory": ".tasks/",
            "beads": {"enabled": False},
            "checkpoints": {"quality_guard": {}},
        }
        with mock_patch.object(_orch, "config_get_effective", return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_009")

        assert result.get("action") == "spawn_agent"
        assert result.get("agent") == "quality_guard"
        assert result.get("parallel_with") == "security_auditor"
        assert "parallel_agent_model" in result
        assert "parallel_effort_level" in result

    def test_parallel_agent_model_from_config(self, clean_tasks_dir):
        """parallel_agent_model should use mode-specific model config for the parallel agent."""
        from unittest.mock import patch as mock_patch
        import agentic_workflow_server.orchestration_tools as _orch

        workflow_initialize(task_id="TASK_ORCH_011", description="Test parallel model")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_011")

        for phase in ["planner", "reviewer", "implementer"]:
            workflow_transition(to_phase=phase, task_id="TASK_ORCH_011")
            workflow_complete_phase(task_id="TASK_ORCH_011")

        mock_config = {
            "parallelization": {
                "quality_guard_security_auditor": {"enabled": True},
            },
            "models": {
                "default": "opus",
                "thorough": {
                    "quality_guard": "sonnet",
                    "security_auditor": "sonnet",
                },
            },
            "subagent_limits": {"max_turns": {}},
            "knowledge_base": "docs/ai-context/",
            "task_directory": ".tasks/",
            "beads": {"enabled": False},
            "checkpoints": {"quality_guard": {}},
        }
        with mock_patch.object(_orch, "config_get_effective", return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_011")

        assert result.get("model") == "sonnet"
        assert result.get("parallel_agent_model") == "sonnet"
        assert result.get("parallel_effort_level") in ("high", "medium", "low")

    def test_quality_guard_no_parallel_when_disabled(self, clean_tasks_dir):
        """When quality_guard_security_auditor disabled, no parallel_with is set."""
        from unittest.mock import patch as mock_patch
        import agentic_workflow_server.orchestration_tools as _orch

        workflow_initialize(task_id="TASK_ORCH_010", description="Test QG no parallel")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_010")

        for phase in ["planner", "reviewer", "implementer"]:
            workflow_transition(to_phase=phase, task_id="TASK_ORCH_010")
            workflow_complete_phase(task_id="TASK_ORCH_010")

        mock_config = {
            "parallelization": {
                "quality_guard_security_auditor": {"enabled": False},
            },
            "models": {"default": "opus", "thorough": {}},
            "subagent_limits": {"max_turns": {}},
            "knowledge_base": "docs/ai-context/",
            "task_directory": ".tasks/",
            "beads": {"enabled": False},
            "checkpoints": {"quality_guard": {}},
        }
        with mock_patch.object(_orch, "config_get_effective", return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_010")

        assert result.get("agent") == "quality_guard"
        assert result.get("parallel_with") is None


# ============================================================================
# crew_get_implementation_action tests (requires filesystem)
# ============================================================================

class TestCrewGetImplementationAction:
    def test_basic_implement_step(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_003", description="Test")
        workflow_set_implementation_progress(total_steps=3, task_id="TASK_ORCH_003")

        result = crew_get_implementation_action(task_id="TASK_ORCH_003")
        assert result["action"] == "implement_step"
        assert result["step_id"] == "step_1"
        assert result["progress_percent"] == 0

    def test_all_steps_complete(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_004", description="Test")
        workflow_set_implementation_progress(total_steps=2, task_id="TASK_ORCH_004")
        workflow_complete_step(step_id="step_1", task_id="TASK_ORCH_004")
        workflow_complete_step(step_id="step_2", task_id="TASK_ORCH_004")

        result = crew_get_implementation_action(task_id="TASK_ORCH_004")
        assert result["action"] in ("complete", "checkpoint")

    def test_verification_passed(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_005", description="Test")
        workflow_set_implementation_progress(total_steps=3, task_id="TASK_ORCH_005")

        result = crew_get_implementation_action(
            task_id="TASK_ORCH_005",
            last_verification_passed=True
        )
        assert result["action"] == "next_step"

    def test_nonexistent_task(self):
        result = crew_get_implementation_action(task_id="TASK_NONEXISTENT")
        assert "error" in result


# ============================================================================
# crew_format_completion tests (requires filesystem)
# ============================================================================

class TestCrewFormatCompletion:
    def test_basic_completion(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_006", description="Add caching layer")
        workflow_set_mode(mode="standard", task_id="TASK_ORCH_006")

        result = crew_format_completion(
            task_id="TASK_ORCH_006",
            files_changed=["src/cache.ts", "src/api.ts"]
        )
        assert result["task_id"] == "TASK_ORCH_006"
        assert "cost_summary" in result
        assert "commit_message" in result
        assert "caching" in result["commit_message"].lower() or "Add caching layer" in result["commit_message"]
        assert result["mode"] == "standard"

    def test_nonexistent_task(self):
        result = crew_format_completion(task_id="TASK_NONEXISTENT")
        assert "error" in result


# ============================================================================
# crew_get_resume_state tests (requires filesystem)
# ============================================================================

class TestCrewGetResumeState:
    def test_basic_resume(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_007", description="Test task for resume")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_007")

        result = crew_get_resume_state(task_id="TASK_ORCH_007")
        assert result["task_id"] == "TASK_ORCH_007"
        assert "display_summary" in result
        assert "Test task for resume" in result["display_summary"]
        assert result["mode"] == "thorough"

    def test_resume_with_progress(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_ORCH_008", description="Implementation task")
        workflow_set_mode(mode="thorough", task_id="TASK_ORCH_008")
        # Transition through all planning phases to implementer
        for phase in ["planner", "reviewer"]:
            workflow_transition(to_phase=phase, task_id="TASK_ORCH_008")
            workflow_complete_phase(task_id="TASK_ORCH_008")
        workflow_transition(to_phase="implementer", task_id="TASK_ORCH_008")
        workflow_set_implementation_progress(total_steps=5, current_step=2, task_id="TASK_ORCH_008")

        result = crew_get_resume_state(task_id="TASK_ORCH_008")
        assert "implementation step" in result["resume_point"]
        assert result["progress_summary"]["total_steps"] == 5

    def test_nonexistent_task(self):
        result = crew_get_resume_state(task_id="TASK_NONEXISTENT")
        assert "error" in result


# ============================================================================
# crew_jira_transition tests
# ============================================================================

from agentic_workflow_server.orchestration_tools import crew_jira_transition


class TestCrewJiraTransition:
    def test_no_issue_key(self):
        result = crew_jira_transition(hook_name="on_complete", issue_key=None)
        assert result["action"] == "skip"
        assert "No Jira issue key" in result["reason"]

    def test_empty_issue_key(self):
        result = crew_jira_transition(hook_name="on_complete", issue_key="")
        assert result["action"] == "skip"

    def test_no_target_status_configured(self):
        """Default config has empty 'to' for all hooks."""
        result = crew_jira_transition(hook_name="on_complete", issue_key="PROJ-42")
        assert result["action"] == "skip"
        assert "No target status" in result["reason"]

    def test_on_create_default_config(self):
        """on_create has mode=auto but empty to, so should skip."""
        result = crew_jira_transition(hook_name="on_create", issue_key="PROJ-42")
        assert result["action"] == "skip"

    def test_on_cleanup_default_config(self):
        """on_cleanup has mode=prompt but empty to, so should skip."""
        result = crew_jira_transition(hook_name="on_cleanup", issue_key="PROJ-42")
        assert result["action"] == "skip"

    def test_unknown_hook_name(self):
        """Unknown hook name returns skip (no config found)."""
        result = crew_jira_transition(hook_name="on_unknown", issue_key="PROJ-42")
        assert result["action"] == "skip"

    def test_skip_preserves_hook_name(self):
        result = crew_jira_transition(hook_name="on_complete", issue_key="PROJ-42")
        assert result.get("hook_name") == "on_complete"


# ============================================================================
# AW-gwq: KB listing timeout tests
# ============================================================================

from agentic_workflow_server.orchestration_tools import (
    _list_kb_files,
    KB_LISTING_TIMEOUT,
    KB_LISTING_MAX_FILES,
    _validate_beads_issue,
)
import tempfile


class TestKBListingTimeout:
    def test_normal_listing(self):
        """Should list files in a small directory without issues."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmppath = Path(tmpdir)
            (tmppath / "file1.md").write_text("content1")
            (tmppath / "file2.md").write_text("content2")
            (tmppath / "sub").mkdir()
            (tmppath / "sub" / "file3.md").write_text("content3")

            files = _list_kb_files(tmppath)
            assert len(files) == 3
            assert "file1.md" in files
            assert "file2.md" in files

    def test_empty_directory(self):
        """Empty directory returns empty list."""
        with tempfile.TemporaryDirectory() as tmpdir:
            files = _list_kb_files(Path(tmpdir))
            assert files == []

    def test_max_files_cap(self):
        """Should stop at KB_LISTING_MAX_FILES."""
        with tempfile.TemporaryDirectory() as tmpdir:
            tmppath = Path(tmpdir)
            # Create more than max files
            for i in range(KB_LISTING_MAX_FILES + 10):
                (tmppath / f"file_{i:04d}.txt").write_text(f"content {i}")

            files = _list_kb_files(tmppath)
            assert len(files) == KB_LISTING_MAX_FILES

    def test_nonexistent_directory_doesnt_crash(self):
        """Non-existent path should not raise."""
        files = _list_kb_files(Path("/nonexistent/path/that/doesnt/exist"))
        assert files == []

    def test_timeout_constant(self):
        assert KB_LISTING_TIMEOUT == 10

    def test_max_files_constant(self):
        assert KB_LISTING_MAX_FILES == 500


# ============================================================================
# AW-0l9: Beads validation tests
# ============================================================================

class TestValidateBeadsIssue:
    def test_nonexistent_issue(self):
        """Validation of a definitely-nonexistent issue should handle gracefully."""
        valid, warning = _validate_beads_issue("NONEXISTENT-999999")
        # Either bd is installed and returns not-found, or bd isn't installed and we pass through
        assert isinstance(valid, bool)
        assert isinstance(warning, str)

    def test_bd_not_installed_passes_through(self):
        """If bd is not installed, should return (True, '')."""
        from unittest.mock import patch
        with patch("subprocess.run", side_effect=FileNotFoundError):
            valid, warning = _validate_beads_issue("AW-123")
            assert valid is True
            assert warning == ""

    def test_bd_timeout_passes_through(self):
        """If bd times out, should return (True, warning)."""
        import subprocess
        from unittest.mock import patch
        with patch("subprocess.run", side_effect=subprocess.TimeoutExpired("bd", 5)):
            valid, warning = _validate_beads_issue("AW-123")
            assert valid is True
            assert "Timed out" in warning

    def test_crew_format_completion_with_invalid_issue(self, clean_tasks_dir):
        """crew_format_completion with an invalid linked issue should include warnings."""
        workflow_initialize(task_id="TASK_ORCH_BEADS_001", description="Test beads validation")
        workflow_set_mode(mode="standard", task_id="TASK_ORCH_BEADS_001")

        # Set a linked issue that definitely doesn't exist
        from agentic_workflow_server.state_tools import _load_state, _save_state, find_task_dir as _find
        task_dir = _find("TASK_ORCH_BEADS_001")
        state = _load_state(task_dir)
        state["linked_issue"] = "NONEXISTENT-999"
        _save_state(task_dir, state)

        from agentic_workflow_server.orchestration_tools import crew_format_completion
        from unittest.mock import patch
        # Mock config to enable beads
        mock_config = {"beads": {"enabled": True, "add_comments": False}}
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_format_completion(task_id="TASK_ORCH_BEADS_001")
            # Should have either beads_commands with a skip comment or beads_warnings
            assert "beads_commands" in result


# ============================================================================
# Deterministic Checkpoint Tests
# ============================================================================

class TestDeterministicCheckpoints:
    """Test that checkpoint invocation is deterministic and config-driven."""

    def test_checkpoint_always_fires_at_threshold_zero(self, clean_tasks_dir):
        """With concern_threshold=0 (default), checkpoint fires even with no concerns."""
        from unittest.mock import patch
        from agentic_workflow_server.state_tools import _load_state, _save_state, find_task_dir as _find

        workflow_initialize(task_id="TASK_ORCH_CP_001", description="Checkpoint test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_001")
        workflow_transition("planner", task_id="TASK_ORCH_CP_001")

        # Config with after_planner: true and threshold=0
        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 0,
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_001")

        assert result["action"] == "checkpoint"
        assert result["concerns_count"] == 0
        assert "question" in result
        assert result["question"]["header"] == "Checkpoint"

    def test_checkpoint_skipped_below_threshold(self, clean_tasks_dir):
        """With concern_threshold > 0 and no concerns, checkpoint is skipped."""
        from unittest.mock import patch

        workflow_initialize(task_id="TASK_ORCH_CP_002", description="Threshold test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_002")
        workflow_transition("planner", task_id="TASK_ORCH_CP_002")

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 1,
                "concern_severity_threshold": "low",
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_002")

        # Should skip checkpoint and return process_output
        assert result["action"] == "process_output"

    def test_checkpoint_fires_when_concerns_meet_threshold(self, clean_tasks_dir):
        """With concerns meeting threshold, checkpoint fires."""
        from unittest.mock import patch
        from agentic_workflow_server.state_tools import workflow_add_concern

        workflow_initialize(task_id="TASK_ORCH_CP_003", description="Concern threshold test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_003")
        workflow_transition("planner", task_id="TASK_ORCH_CP_003")

        # Add a concern
        workflow_add_concern(
            source="planner",
            severity="high",
            description="Security risk in auth flow",
            task_id="TASK_ORCH_CP_003"
        )

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 1,
                "concern_severity_threshold": "medium",
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_003")

        assert result["action"] == "checkpoint"
        assert result["concerns_count"] == 1
        assert "Security risk" in result["question"]["text"]

    def test_checkpoint_severity_filter(self, clean_tasks_dir):
        """Concerns below severity threshold are not counted."""
        from unittest.mock import patch
        from agentic_workflow_server.state_tools import workflow_add_concern

        workflow_initialize(task_id="TASK_ORCH_CP_004", description="Severity filter test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_004")
        workflow_transition("planner", task_id="TASK_ORCH_CP_004")

        # Add a low-severity concern
        workflow_add_concern(
            source="planner",
            severity="low",
            description="Minor style issue",
            task_id="TASK_ORCH_CP_004"
        )

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 1,
                "concern_severity_threshold": "high",  # Only high+ counts
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_004")

        # Low concern doesn't meet "high" threshold — skipped
        assert result["action"] == "process_output"

    def test_checkpoint_no_config_means_no_checkpoint(self, clean_tasks_dir):
        """Phase without checkpoint config should never trigger checkpoint."""
        from unittest.mock import patch

        workflow_initialize(task_id="TASK_ORCH_CP_005", description="No checkpoint test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_005")
        # Transition to planner (first phase) — phase is active but not completed
        workflow_transition("planner", task_id="TASK_ORCH_CP_005")

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": False},  # Explicitly off
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_005")

        # Checkpoint is off — should get process_output, not checkpoint
        assert result["action"] == "process_output"

    def test_checkpoint_result_has_structured_question(self, clean_tasks_dir):
        """Checkpoint result should contain pre-built question with options."""
        from unittest.mock import patch

        workflow_initialize(task_id="TASK_ORCH_CP_006", description="Structured question test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_006")
        workflow_transition("planner", task_id="TASK_ORCH_CP_006")

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 0,
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_006")

        assert result["action"] == "checkpoint"
        q = result["question"]
        assert "text" in q
        assert "header" in q
        assert "options" in q
        assert len(q["options"]) == 3
        labels = [o["label"] for o in q["options"]]
        assert "Approve" in labels
        assert "Revise" in labels
        assert "Skip" in labels

    def test_checkpoint_concern_summary_in_question(self, clean_tasks_dir):
        """Checkpoint question should include concern summary when concerns exist."""
        from unittest.mock import patch
        from agentic_workflow_server.state_tools import workflow_add_concern

        workflow_initialize(task_id="TASK_ORCH_CP_007", description="Summary test")
        workflow_set_mode("standard", task_id="TASK_ORCH_CP_007")
        workflow_transition("planner", task_id="TASK_ORCH_CP_007")

        workflow_add_concern(
            source="planner",
            severity="critical",
            description="SQL injection vulnerability in user input handler",
            task_id="TASK_ORCH_CP_007"
        )
        workflow_add_concern(
            source="planner",
            severity="medium",
            description="Missing rate limiting",
            task_id="TASK_ORCH_CP_007"
        )

        mock_config = {
            "checkpoints": {
                "planning": {"after_planner": True},
                "concern_threshold": 0,
            },
            "models": {"default": "opus"},
        }
        with patch("agentic_workflow_server.orchestration_tools.config_get_effective",
                   return_value={"config": mock_config}):
            result = crew_get_next_phase(task_id="TASK_ORCH_CP_007")

        assert result["action"] == "checkpoint"
        assert result["concerns_count"] == 2
        q_text = result["question"]["text"]
        assert "2 unaddressed concern" in q_text
        assert "critical" in q_text
        assert "SQL injection" in q_text
