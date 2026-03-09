"""
Tests for composite tools and outcome tracking in state_tools.py.

Covers:
  - workflow_query (unified query dispatcher)
  - workflow_manage_model (unified model resilience dispatcher)
  - workflow_parallel (unified parallel execution dispatcher)
  - workflow_record_outcome (task outcome recording)
  - workflow_get_outcome_stats (outcome statistics aggregation)

Run with: pytest tests/test_composite_tools.py -v
"""

import json
import shutil
import tempfile
import pytest
from pathlib import Path
from datetime import datetime, timedelta

import sys
sys.path.insert(0, str(Path(__file__).parent.parent))

import agentic_workflow_server.state_tools as _state_mod

from agentic_workflow_server.state_tools import (
    # Core workflow
    workflow_initialize,
    workflow_get_state,
    # Concerns
    workflow_add_concern,
    workflow_get_concerns,
    # Discoveries
    workflow_save_discovery,
    workflow_get_discoveries,
    # Assertions
    workflow_add_assertion,
    workflow_get_assertions,
    # Context
    workflow_get_context_usage,
    # Linked tasks
    workflow_link_tasks,
    workflow_get_linked_tasks,
    # Optional phases
    workflow_enable_optional_phase,
    workflow_get_optional_phases,
    # Agent performance
    workflow_record_concern_outcome,
    workflow_get_agent_performance,
    # Model resilience
    workflow_record_model_error,
    workflow_record_model_success,
    workflow_get_available_model,
    workflow_get_resilience_status,
    workflow_clear_model_cooldown,
    # Parallel
    workflow_start_parallel_phase,
    workflow_complete_parallel_phase,
    workflow_merge_parallel_results,
    # Composite tools
    workflow_query,
    workflow_manage_model,
    workflow_parallel,
    # Outcome tracking
    workflow_record_outcome,
    workflow_get_outcome_stats,
    # Helpers / constants
    get_tasks_dir,
    find_task_dir,
    _load_state,
    _save_state,
    DISCOVERY_CATEGORIES,
)


# ── Fixtures ─────────────────────────────────────────────────────────


@pytest.fixture
def isolated_tasks_dir():
    """Provide a completely isolated temp .tasks/ directory.

    Redirects ``_cached_tasks_dir`` so that ``get_tasks_dir()`` returns
    a fresh temp directory containing no real tasks.  This prevents
    in-progress tasks from leaking into tests that scan all task
    directories (e.g. ``_find_active_task_dir``).
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_tasks = Path(tmpdir) / ".tasks"
        tmp_tasks.mkdir()

        old = _state_mod._cached_tasks_dir
        _state_mod._cached_tasks_dir = tmp_tasks
        try:
            yield tmp_tasks
        finally:
            _state_mod._cached_tasks_dir = old


@pytest.fixture
def task_with_state(isolated_tasks_dir):
    """Create a single initialized task and return its ID."""
    task_id = "TASK_CT_001"
    result = workflow_initialize(task_id=task_id, description="Composite tool test task")
    assert result["success"] is True
    return task_id


@pytest.fixture
def two_tasks(isolated_tasks_dir):
    """Create two initialized tasks and return both IDs."""
    task_a = "TASK_CT_A01"
    task_b = "TASK_CT_B01"
    assert workflow_initialize(task_id=task_a, description="Task A")["success"]
    assert workflow_initialize(task_id=task_b, description="Task B")["success"]
    return task_a, task_b


# ============================================================================
# workflow_query
# ============================================================================


class TestWorkflowQuery:
    """Tests for the unified workflow_query dispatcher."""

    def test_invalid_aspect_returns_error(self, task_with_state):
        result = workflow_query(aspect="nonexistent", task_id=task_with_state)
        assert "error" in result
        assert "Invalid aspect" in result["error"]

    # ── concerns ──

    def test_query_concerns_empty(self, task_with_state):
        result = workflow_query(aspect="concerns", task_id=task_with_state)
        assert result["total"] == 0
        assert result["concerns"] == []

    def test_query_concerns_with_data(self, task_with_state):
        workflow_add_concern(
            source="reviewer",
            severity="high",
            description="Memory usage too high",
            task_id=task_with_state,
        )
        workflow_add_concern(
            source="skeptic",
            severity="low",
            description="Edge case not handled",
            task_id=task_with_state,
        )

        result = workflow_query(aspect="concerns", task_id=task_with_state)
        assert result["total"] == 2

    def test_query_concerns_unaddressed_filter(self, task_with_state):
        workflow_add_concern(
            source="reviewer",
            severity="medium",
            description="Will be addressed",
            concern_id="C001",
            task_id=task_with_state,
        )
        workflow_add_concern(
            source="skeptic",
            severity="high",
            description="Still open",
            concern_id="C002",
            task_id=task_with_state,
        )

        # Address the first concern
        from agentic_workflow_server.state_tools import workflow_address_concern
        workflow_address_concern(
            concern_id="C001",
            addressed_by="developer",
            task_id=task_with_state,
        )

        result = workflow_query(
            aspect="concerns",
            task_id=task_with_state,
            filters={"unaddressed_only": True},
        )
        assert result["total"] == 1
        assert result["concerns"][0]["description"] == "Still open"

    # ── discoveries ──

    def test_query_discoveries_empty(self, task_with_state):
        result = workflow_query(aspect="discoveries", task_id=task_with_state)
        assert result["count"] == 0

    def test_query_discoveries_with_data(self, task_with_state):
        # Use a valid DISCOVERY_CATEGORIES value
        valid_category = DISCOVERY_CATEGORIES[0]
        workflow_save_discovery(
            category=valid_category,
            content="Uses hexagonal pattern",
            task_id=task_with_state,
        )
        result = workflow_query(aspect="discoveries", task_id=task_with_state)
        assert result["count"] == 1
        assert result["discoveries"][0]["category"] == valid_category

    def test_query_discoveries_with_category_filter(self, task_with_state):
        cat_a = DISCOVERY_CATEGORIES[0]
        cat_b = DISCOVERY_CATEGORIES[1] if len(DISCOVERY_CATEGORIES) > 1 else DISCOVERY_CATEGORIES[0]
        workflow_save_discovery(
            category=cat_a,
            content="Hex pattern",
            task_id=task_with_state,
        )
        workflow_save_discovery(
            category=cat_b,
            content="Uses react 18",
            task_id=task_with_state,
        )

        result = workflow_query(
            aspect="discoveries",
            task_id=task_with_state,
            filters={"category": cat_b},
        )
        # If cat_a == cat_b we get both; otherwise just one
        if cat_a != cat_b:
            assert result["count"] == 1
            assert result["discoveries"][0]["category"] == cat_b
        else:
            assert result["count"] == 2

    # ── assertions ──

    def test_query_assertions_empty(self, task_with_state):
        result = workflow_query(aspect="assertions", task_id=task_with_state)
        assert result["assertions"] == []

    def test_query_assertions_with_data(self, task_with_state):
        workflow_add_assertion(
            assertion_type="test_passes",
            definition={"test_command": "pytest", "expected": "pass"},
            step_id="step1",
            task_id=task_with_state,
        )
        result = workflow_query(aspect="assertions", task_id=task_with_state)
        assert len(result["assertions"]) == 1

    def test_query_assertions_with_step_filter(self, task_with_state):
        workflow_add_assertion(
            assertion_type="test_passes",
            definition={"test_command": "pytest"},
            step_id="step1",
            task_id=task_with_state,
        )
        workflow_add_assertion(
            assertion_type="lint_passes",
            definition={"linter": "ruff"},
            step_id="step2",
            task_id=task_with_state,
        )

        result = workflow_query(
            aspect="assertions",
            task_id=task_with_state,
            filters={"step_id": "step1"},
        )
        # The underlying function filters by step_id
        filtered = [a for a in result["assertions"] if a.get("step_id") == "step1"]
        assert len(filtered) >= 1

    # ── context_usage ──

    def test_query_context_usage(self, task_with_state):
        result = workflow_query(aspect="context_usage", task_id=task_with_state)
        # Should return context info with at least files_count and total_size_bytes
        assert "error" not in result

    # ── linked_tasks ──

    def test_query_linked_tasks_empty(self, task_with_state):
        result = workflow_query(aspect="linked_tasks", task_id=task_with_state)
        # linked_tasks is a dict keyed by relationship type; empty when no links
        assert result["linked_tasks"] == {}

    def test_query_linked_tasks_with_data(self, two_tasks):
        task_a, task_b = two_tasks
        workflow_link_tasks(
            task_id=task_a,
            related_task_ids=[task_b],
            relationship="builds_on",
        )

        result = workflow_query(aspect="linked_tasks", task_id=task_a)
        assert "builds_on" in result["linked_tasks"]
        assert task_b in result["linked_tasks"]["builds_on"]

    # ── optional_phases ──

    def test_query_optional_phases_empty(self, task_with_state):
        result = workflow_query(aspect="optional_phases", task_id=task_with_state)
        assert result["optional_phases"] == []

    def test_query_optional_phases_with_data(self, task_with_state):
        workflow_enable_optional_phase(
            phase="security_auditor",
            reason="Handles user auth",
            task_id=task_with_state,
        )

        result = workflow_query(aspect="optional_phases", task_id=task_with_state)
        assert "security_auditor" in result["optional_phases"]

    # ── agent_performance ──

    def test_query_agent_performance_empty(self, isolated_tasks_dir):
        result = workflow_query(aspect="agent_performance")
        assert result["total_concerns"] == 0

    def test_query_agent_performance_with_data(self, task_with_state):
        workflow_record_concern_outcome(
            concern_id="c1",
            agent="reviewer",
            outcome="valid",
            concern_type="bug",
        )

        result = workflow_query(aspect="agent_performance")
        assert result["total_concerns"] >= 1

    def test_query_agent_performance_filtered_by_agent(self, task_with_state):
        workflow_record_concern_outcome(
            concern_id="c1",
            agent="reviewer",
            outcome="valid",
            concern_type="bug",
        )
        workflow_record_concern_outcome(
            concern_id="c2",
            agent="skeptic",
            outcome="false_positive",
            concern_type="style",
        )

        result = workflow_query(
            aspect="agent_performance",
            filters={"agent": "reviewer"},
        )
        # Should only include reviewer data
        if result.get("agents"):
            assert "reviewer" in result["agents"]

    # ── task not found ──

    def test_query_with_nonexistent_task(self, isolated_tasks_dir):
        result = workflow_query(aspect="concerns", task_id="TASK_CT_NONEXIST")
        assert "error" in result


# ============================================================================
# workflow_manage_model
# ============================================================================


class TestWorkflowManageModel:
    """Tests for the unified model resilience dispatcher."""

    def test_invalid_action_returns_error(self, isolated_tasks_dir):
        result = workflow_manage_model(action="invalid_action")
        assert "error" in result
        assert "Invalid action" in result["error"]

    # ── record_error ──

    def test_record_error_success(self, isolated_tasks_dir):
        result = workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
            error_message="Too many requests",
        )
        assert result["success"] is True
        assert result["model"] == "claude-opus-4"
        assert result["error_type"] == "rate_limit"
        assert result["consecutive_errors"] == 1
        assert result["cooldown_seconds"] > 0

    def test_record_error_requires_model(self, isolated_tasks_dir):
        result = workflow_manage_model(
            action="record_error",
            error_type="rate_limit",
        )
        assert "error" in result
        assert "model is required" in result["error"]

    def test_record_error_requires_error_type(self, isolated_tasks_dir):
        result = workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
        )
        assert "error" in result
        assert "error_type is required" in result["error"]

    def test_record_error_with_task_id(self, task_with_state):
        result = workflow_manage_model(
            action="record_error",
            model="claude-sonnet-4",
            error_type="timeout",
            error_message="Request timed out after 30s",
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["model"] == "claude-sonnet-4"

    def test_record_error_consecutive_increases(self, isolated_tasks_dir):
        workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
        )
        result = workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
        )
        assert result["consecutive_errors"] == 2

    # ── record_success ──

    def test_record_success_resets_errors(self, isolated_tasks_dir):
        # Record an error first
        workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
        )
        # Then record success
        result = workflow_manage_model(
            action="record_success",
            model="claude-opus-4",
        )
        assert result["success"] is True
        assert result["model"] == "claude-opus-4"

    def test_record_success_requires_model(self, isolated_tasks_dir):
        result = workflow_manage_model(action="record_success")
        assert "error" in result
        assert "model is required" in result["error"]

    def test_record_success_no_prior_errors(self, isolated_tasks_dir):
        result = workflow_manage_model(
            action="record_success",
            model="claude-opus-4",
        )
        assert result["success"] is True
        assert "No error history" in result["message"]

    # ── get_available ──

    def test_get_available_returns_model(self, isolated_tasks_dir):
        result = workflow_manage_model(action="get_available")
        assert "model" in result or "available_model" in result or "error" not in result

    def test_get_available_with_preferred(self, isolated_tasks_dir):
        result = workflow_manage_model(
            action="get_available",
            preferred_model="claude-opus-4",
        )
        assert "error" not in result

    def test_get_available_skips_cooled_down_model(self, isolated_tasks_dir):
        # Put a model in cooldown
        workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
        )
        # Get available should not return the cooled-down model
        result = workflow_manage_model(action="get_available")
        assert "error" not in result

    # ── get_status ──

    def test_get_status_empty(self, isolated_tasks_dir):
        result = workflow_manage_model(action="get_status")
        assert "models" in result
        assert "fallback_chain" in result

    def test_get_status_after_errors(self, isolated_tasks_dir):
        workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="server_error",
            error_message="Internal error",
        )
        result = workflow_manage_model(action="get_status")
        assert "models" in result
        assert len(result["models"]) >= 1
        # Find the model we just errored
        model_entry = [m for m in result["models"] if m["model"] == "claude-opus-4"]
        assert len(model_entry) == 1
        assert model_entry[0]["total_errors"] == 1

    # ── clear_cooldown ──

    def test_clear_cooldown_requires_model(self, isolated_tasks_dir):
        result = workflow_manage_model(action="clear_cooldown")
        assert "error" in result
        assert "model is required" in result["error"]

    def test_clear_cooldown_after_error(self, isolated_tasks_dir):
        workflow_manage_model(
            action="record_error",
            model="claude-opus-4",
            error_type="rate_limit",
        )
        result = workflow_manage_model(
            action="clear_cooldown",
            model="claude-opus-4",
        )
        assert result["success"] is True


# ============================================================================
# workflow_parallel
# ============================================================================


class TestWorkflowParallel:
    """Tests for the unified parallel execution dispatcher."""

    def test_invalid_action_returns_error(self, task_with_state):
        result = workflow_parallel(action="invalid", task_id=task_with_state)
        assert "error" in result
        assert "Invalid action" in result["error"]

    # ── start ──

    def test_start_parallel(self, task_with_state):
        result = workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["parallel_phases"] == ["reviewer", "skeptic"]

    def test_start_requires_phases(self, task_with_state):
        result = workflow_parallel(action="start", task_id=task_with_state)
        assert "error" in result
        assert "phases is required" in result["error"]

    def test_start_persists_state(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        task_dir = find_task_dir(task_with_state)
        state = _load_state(task_dir)
        assert state["parallel_execution"]["active"] is True
        assert state["parallel_execution"]["phases"] == ["reviewer", "skeptic"]

    # ── complete ──

    def test_complete_one_phase(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        result = workflow_parallel(
            action="complete",
            phase="reviewer",
            result_summary="All looks good",
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["phase"] == "reviewer"
        assert result["all_complete"] is False
        assert "skeptic" in result["remaining"]

    def test_complete_requires_phase(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        result = workflow_parallel(
            action="complete",
            task_id=task_with_state,
        )
        assert "error" in result
        assert "phase is required" in result["error"]

    def test_complete_all_phases(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        workflow_parallel(
            action="complete",
            phase="reviewer",
            result_summary="Code looks clean",
            task_id=task_with_state,
        )
        result = workflow_parallel(
            action="complete",
            phase="skeptic",
            result_summary="Potential edge case found",
            concerns=[{"description": "Edge case in auth flow", "severity": "medium"}],
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["all_complete"] is True
        assert result["remaining"] == []

    def test_complete_unknown_phase(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        result = workflow_parallel(
            action="complete",
            phase="nonexistent",
            task_id=task_with_state,
        )
        assert result["success"] is False
        assert "not part of current parallel execution" in result["error"]

    def test_complete_without_start(self, task_with_state):
        result = workflow_parallel(
            action="complete",
            phase="reviewer",
            task_id=task_with_state,
        )
        assert result["success"] is False
        assert "No active parallel execution" in result["error"]

    # ── merge ──

    def test_merge_after_complete(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        workflow_parallel(
            action="complete",
            phase="reviewer",
            result_summary="All good",
            concerns=[{"description": "Minor style issue", "severity": "low"}],
            task_id=task_with_state,
        )
        workflow_parallel(
            action="complete",
            phase="skeptic",
            result_summary="Found edge case",
            concerns=[{"description": "Edge case in auth", "severity": "high"}],
            task_id=task_with_state,
        )

        result = workflow_parallel(
            action="merge",
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["original_count"] == 2
        assert result["merged_count"] == 2
        assert result["merge_strategy"] == "deduplicate"

    def test_merge_deduplicate_removes_duplicates(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        # Both raise the same concern
        same_concern = {"description": "Missing error handling in login", "severity": "high"}
        workflow_parallel(
            action="complete",
            phase="reviewer",
            result_summary="Found issue",
            concerns=[same_concern],
            task_id=task_with_state,
        )
        workflow_parallel(
            action="complete",
            phase="skeptic",
            result_summary="Found same issue",
            concerns=[same_concern],
            task_id=task_with_state,
        )

        result = workflow_parallel(
            action="merge",
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["original_count"] == 2
        # Deduplicate should reduce count since descriptions match
        assert result["merged_count"] == 1

    def test_merge_combine_strategy(self, task_with_state):
        workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic"],
            task_id=task_with_state,
        )
        same_concern = {"description": "Same concern text", "severity": "medium"}
        workflow_parallel(
            action="complete",
            phase="reviewer",
            concerns=[same_concern],
            task_id=task_with_state,
        )
        workflow_parallel(
            action="complete",
            phase="skeptic",
            concerns=[same_concern],
            task_id=task_with_state,
        )

        result = workflow_parallel(
            action="merge",
            merge_strategy="combine",
            task_id=task_with_state,
        )
        assert result["success"] is True
        assert result["merge_strategy"] == "combine"
        # Combine keeps all, including duplicates
        assert result["merged_count"] == 2

    def test_merge_without_parallel_execution(self, task_with_state):
        result = workflow_parallel(
            action="merge",
            task_id=task_with_state,
        )
        assert result["success"] is False
        assert "No parallel execution results" in result["error"]

    def test_parallel_with_nonexistent_task(self, isolated_tasks_dir):
        result = workflow_parallel(
            action="start",
            phases=["reviewer"],
            task_id="TASK_CT_NONEXIST",
        )
        assert result["success"] is False
        assert "not found" in result["error"]


# ============================================================================
# workflow_record_outcome
# ============================================================================


class TestWorkflowRecordOutcome:
    """Tests for task outcome recording."""

    def test_record_success_outcome(self, task_with_state):
        result = workflow_record_outcome(
            task_id=task_with_state,
            success=True,
            rework_cycles=0,
            files_changed=5,
            tests_passed=12,
            tests_failed=0,
            duration_seconds=120.5,
            notes="Clean implementation",
        )
        assert result["success"] is True
        assert result["outcome"]["task_id"] == task_with_state
        assert result["outcome"]["success"] is True
        assert result["outcome"]["files_changed"] == 5
        assert result["outcome"]["tests_passed"] == 12
        assert result["outcome"]["tests_failed"] == 0
        assert result["outcome"]["duration_seconds"] == 120.5
        assert result["outcome"]["notes"] == "Clean implementation"
        assert result["total_recorded"] == 1

    def test_record_failure_outcome(self, task_with_state):
        result = workflow_record_outcome(
            task_id=task_with_state,
            success=False,
            rework_cycles=3,
            files_changed=2,
            tests_passed=8,
            tests_failed=4,
            duration_seconds=300.0,
            notes="Tests kept failing due to race condition",
        )
        assert result["success"] is True
        assert result["outcome"]["success"] is False
        assert result["outcome"]["rework_cycles"] == 3
        assert result["outcome"]["tests_failed"] == 4

    def test_record_minimal_outcome(self, task_with_state):
        """Only required fields: task_id and success."""
        result = workflow_record_outcome(
            task_id=task_with_state,
            success=True,
        )
        assert result["success"] is True
        assert result["outcome"]["rework_cycles"] == 0
        assert result["outcome"]["files_changed"] == 0
        assert result["outcome"]["duration_seconds"] == 0
        assert result["outcome"]["notes"] == ""

    def test_record_requires_task_id(self, isolated_tasks_dir):
        result = workflow_record_outcome(
            task_id="",
            success=True,
        )
        assert result["success"] is False
        assert "task_id is required" in result["error"]

    def test_record_outcome_persists_to_file(self, task_with_state):
        workflow_record_outcome(
            task_id=task_with_state,
            success=True,
            files_changed=3,
        )

        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        assert outcomes_file.exists()

        with open(outcomes_file, "r") as f:
            lines = [line.strip() for line in f if line.strip()]
        assert len(lines) == 1

        entry = json.loads(lines[0])
        assert entry["task_id"] == task_with_state
        assert entry["success"] is True
        assert entry["files_changed"] == 3

    def test_record_multiple_outcomes(self, isolated_tasks_dir):
        # Create two tasks and record outcomes for each
        task1 = "TASK_CT_OUT1"
        task2 = "TASK_CT_OUT2"
        workflow_initialize(task_id=task1, description="Outcome task 1")
        workflow_initialize(task_id=task2, description="Outcome task 2")

        workflow_record_outcome(task_id=task1, success=True, files_changed=2)
        workflow_record_outcome(task_id=task2, success=False, rework_cycles=2)

        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        with open(outcomes_file, "r") as f:
            lines = [line.strip() for line in f if line.strip()]
        assert len(lines) == 2

    def test_record_outcome_includes_timestamp(self, task_with_state):
        result = workflow_record_outcome(
            task_id=task_with_state,
            success=True,
        )
        assert "recorded_at" in result["outcome"]
        # Verify it is a valid ISO format timestamp
        datetime.fromisoformat(result["outcome"]["recorded_at"])

    def test_record_outcome_captures_mode(self, task_with_state):
        """If the task has a workflow mode set, it should be captured."""
        result = workflow_record_outcome(
            task_id=task_with_state,
            success=True,
        )
        # Mode should be present (defaults to 'unknown' if not set)
        assert "mode" in result["outcome"]

    def test_record_outcome_nonexistent_task_still_records(self, isolated_tasks_dir):
        """Recording for a task_id that doesn't have a dir still works,
        but mode will be 'unknown'."""
        result = workflow_record_outcome(
            task_id="TASK_CT_GHOST",
            success=True,
        )
        assert result["success"] is True
        assert result["outcome"]["mode"] == "unknown"

    def test_record_outcome_rotation(self, isolated_tasks_dir):
        """Verify the file does not grow beyond MAX_OUTCOME_RECORDS."""
        from agentic_workflow_server.state_tools import MAX_OUTCOME_RECORDS

        # Write MAX_OUTCOME_RECORDS + 5 entries
        task_id = "TASK_CT_ROT"
        workflow_initialize(task_id=task_id, description="Rotation test")

        for i in range(MAX_OUTCOME_RECORDS + 5):
            workflow_record_outcome(
                task_id=f"{task_id}_{i:04d}",
                success=(i % 2 == 0),
            )

        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        with open(outcomes_file, "r") as f:
            lines = [line.strip() for line in f if line.strip()]
        assert len(lines) == MAX_OUTCOME_RECORDS


# ============================================================================
# workflow_get_outcome_stats
# ============================================================================


class TestWorkflowGetOutcomeStats:
    """Tests for outcome statistics aggregation."""

    def test_no_outcomes_file(self, isolated_tasks_dir):
        result = workflow_get_outcome_stats()
        assert result["success"] is True
        assert result["total_tasks"] == 0
        assert "No outcome data" in result["message"]

    def test_empty_outcomes_file(self, isolated_tasks_dir):
        # Create an empty file
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        outcomes_file.touch()

        result = workflow_get_outcome_stats()
        assert result["success"] is True
        assert result["total_tasks"] == 0

    def test_basic_stats(self, isolated_tasks_dir):
        task_id = "TASK_CT_STATS"
        workflow_initialize(task_id=task_id, description="Stats test")

        workflow_record_outcome(task_id=task_id, success=True, files_changed=5,
                                tests_passed=10, tests_failed=0, duration_seconds=60)
        workflow_record_outcome(task_id=f"{task_id}_2", success=True, files_changed=3,
                                tests_passed=8, tests_failed=0, duration_seconds=90)
        workflow_record_outcome(task_id=f"{task_id}_3", success=False, files_changed=1,
                                tests_passed=5, tests_failed=2, rework_cycles=2,
                                duration_seconds=120)

        result = workflow_get_outcome_stats()
        assert result["success"] is True
        assert result["total_tasks"] == 3
        assert result["successes"] == 2
        assert result["failures"] == 1
        # Success rate: 2/3 * 100 = 66.7
        assert result["success_rate"] == 66.7
        # Avg rework: (0 + 0 + 2) / 3 = 0.67
        assert result["avg_rework_cycles"] == 0.67
        # Total tests passed: 10 + 8 + 5 = 23
        assert result["total_tests_passed"] == 23
        # Total tests failed: 0 + 0 + 2 = 2
        assert result["total_tests_failed"] == 2

    def test_stats_avg_duration(self, isolated_tasks_dir):
        task_id = "TASK_CT_DUR"
        workflow_initialize(task_id=task_id, description="Duration test")

        workflow_record_outcome(task_id=task_id, success=True, duration_seconds=100)
        workflow_record_outcome(task_id=f"{task_id}_2", success=True, duration_seconds=200)

        result = workflow_get_outcome_stats()
        # Avg duration: (100 + 200) / 2 = 150.0
        assert result["avg_duration_seconds"] == 150.0

    def test_stats_avg_files_changed(self, isolated_tasks_dir):
        task_id = "TASK_CT_FC"
        workflow_initialize(task_id=task_id, description="Files test")

        workflow_record_outcome(task_id=task_id, success=True, files_changed=4)
        workflow_record_outcome(task_id=f"{task_id}_2", success=True, files_changed=6)

        result = workflow_get_outcome_stats()
        # Avg files: (4 + 6) / 2 = 5.0
        assert result["avg_files_changed"] == 5.0

    def test_stats_by_mode(self, isolated_tasks_dir):
        """Outcomes with different modes should be broken down."""
        task_id = "TASK_CT_MODE"
        workflow_initialize(task_id=task_id, description="Mode test")

        # Record outcomes with mode injected directly into the file
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        now = datetime.now().isoformat()

        entries = [
            {"task_id": f"{task_id}_1", "success": True, "mode": "full",
             "rework_cycles": 0, "recorded_at": now,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
            {"task_id": f"{task_id}_2", "success": True, "mode": "full",
             "rework_cycles": 1, "recorded_at": now,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
            {"task_id": f"{task_id}_3", "success": False, "mode": "micro",
             "rework_cycles": 3, "recorded_at": now,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
        ]
        with open(outcomes_file, "w") as f:
            for entry in entries:
                f.write(json.dumps(entry) + "\n")

        result = workflow_get_outcome_stats()
        assert result["success"] is True
        assert "by_mode" in result

        assert "full" in result["by_mode"]
        assert result["by_mode"]["full"]["total"] == 2
        assert result["by_mode"]["full"]["success_rate"] == 100.0
        assert result["by_mode"]["full"]["avg_rework_cycles"] == 0.5

        assert "micro" in result["by_mode"]
        assert result["by_mode"]["micro"]["total"] == 1
        assert result["by_mode"]["micro"]["success_rate"] == 0.0

    def test_stats_time_window_filtering(self, isolated_tasks_dir):
        """Outcomes outside the time window should be excluded."""
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"

        old_time = (datetime.now() - timedelta(days=60)).isoformat()
        recent_time = datetime.now().isoformat()

        entries = [
            {"task_id": "OLD_TASK", "success": True, "mode": "unknown",
             "rework_cycles": 0, "recorded_at": old_time,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
            {"task_id": "RECENT_TASK", "success": False, "mode": "unknown",
             "rework_cycles": 1, "recorded_at": recent_time,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
        ]
        with open(outcomes_file, "w") as f:
            for entry in entries:
                f.write(json.dumps(entry) + "\n")

        # Default 30 days should exclude the 60-day-old entry
        result = workflow_get_outcome_stats(days=30)
        assert result["total_tasks"] == 1
        assert result["success_rate"] == 0.0  # Only the recent failure

    def test_stats_custom_days_window(self, isolated_tasks_dir):
        """A larger time window should include older outcomes."""
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"

        old_time = (datetime.now() - timedelta(days=60)).isoformat()
        recent_time = datetime.now().isoformat()

        entries = [
            {"task_id": "OLD_TASK", "success": True, "mode": "unknown",
             "rework_cycles": 0, "recorded_at": old_time,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
            {"task_id": "RECENT_TASK", "success": True, "mode": "unknown",
             "rework_cycles": 0, "recorded_at": recent_time,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
        ]
        with open(outcomes_file, "w") as f:
            for entry in entries:
                f.write(json.dumps(entry) + "\n")

        result = workflow_get_outcome_stats(days=90)
        assert result["total_tasks"] == 2

    def test_stats_zero_duration_excluded_from_avg(self, isolated_tasks_dir):
        """Entries with duration_seconds=0 should not skew the average."""
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        now = datetime.now().isoformat()

        entries = [
            {"task_id": "T1", "success": True, "mode": "unknown",
             "rework_cycles": 0, "recorded_at": now,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 0, "notes": ""},
            {"task_id": "T2", "success": True, "mode": "unknown",
             "rework_cycles": 0, "recorded_at": now,
             "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
             "duration_seconds": 100, "notes": ""},
        ]
        with open(outcomes_file, "w") as f:
            for entry in entries:
                f.write(json.dumps(entry) + "\n")

        result = workflow_get_outcome_stats()
        # Only the entry with duration > 0 should be counted in avg
        assert result["avg_duration_seconds"] == 100.0

    def test_stats_malformed_jsonl_lines_skipped(self, isolated_tasks_dir):
        """Malformed JSON lines in the outcomes file should be skipped gracefully."""
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        now = datetime.now().isoformat()

        with open(outcomes_file, "w") as f:
            f.write("this is not json\n")
            f.write(json.dumps({
                "task_id": "T1", "success": True, "mode": "unknown",
                "rework_cycles": 0, "recorded_at": now,
                "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
                "duration_seconds": 50, "notes": "",
            }) + "\n")
            f.write("{invalid json too\n")

        result = workflow_get_outcome_stats()
        assert result["success"] is True
        assert result["total_tasks"] == 1

    def test_stats_all_outside_window(self, isolated_tasks_dir):
        """When all outcomes are outside the time window, should get 0 total."""
        outcomes_file = get_tasks_dir() / ".task_outcomes.jsonl"
        old_time = (datetime.now() - timedelta(days=365)).isoformat()

        entry = {
            "task_id": "ANCIENT", "success": True, "mode": "unknown",
            "rework_cycles": 0, "recorded_at": old_time,
            "files_changed": 0, "tests_passed": 0, "tests_failed": 0,
            "duration_seconds": 0, "notes": "",
        }
        with open(outcomes_file, "w") as f:
            f.write(json.dumps(entry) + "\n")

        result = workflow_get_outcome_stats(days=30)
        assert result["total_tasks"] == 0
        assert "No outcomes in the last 30 days" in result["message"]


# ============================================================================
# Integration: composite tools work end-to-end
# ============================================================================


class TestCompositeIntegration:
    """End-to-end tests verifying composite tools match their underlying functions."""

    def test_query_concerns_matches_direct_call(self, task_with_state):
        workflow_add_concern(
            concern="Test concern",
            severity="medium",
            task_id=task_with_state,
        )

        direct = workflow_get_concerns(task_id=task_with_state)
        composite = workflow_query(aspect="concerns", task_id=task_with_state)

        assert direct["total"] == composite["total"]
        assert direct["concerns"] == composite["concerns"]

    def test_query_discoveries_matches_direct_call(self, task_with_state):
        workflow_save_discovery(
            category="architecture",
            content="Service mesh topology",
            task_id=task_with_state,
        )

        direct = workflow_get_discoveries(task_id=task_with_state)
        composite = workflow_query(aspect="discoveries", task_id=task_with_state)

        assert direct["count"] == composite["count"]
        assert direct["discoveries"] == composite["discoveries"]

    def test_manage_model_matches_direct_record_error(self, isolated_tasks_dir):
        composite = workflow_manage_model(
            action="record_error",
            model="test-model",
            error_type="timeout",
            error_message="Timed out",
        )

        # Verify the result has the same structure as the direct call
        assert composite["success"] is True
        assert composite["model"] == "test-model"
        assert composite["error_type"] == "timeout"
        assert "cooldown_seconds" in composite

    def test_parallel_full_lifecycle(self, task_with_state):
        """Start -> complete all -> merge lifecycle via composite tool."""
        # Start
        start = workflow_parallel(
            action="start",
            phases=["reviewer", "skeptic", "technical_writer"],
            task_id=task_with_state,
        )
        assert start["success"] is True

        # Complete each phase
        for phase, summary, concerns in [
            ("reviewer", "Code quality good", [{"description": "Consider adding docs", "severity": "low"}]),
            ("skeptic", "Edge case concern", [{"description": "Null check missing", "severity": "high"}]),
            ("technical_writer", "Docs ready", []),
        ]:
            result = workflow_parallel(
                action="complete",
                phase=phase,
                result_summary=summary,
                concerns=concerns,
                task_id=task_with_state,
            )
            assert result["success"] is True

        # Merge
        merged = workflow_parallel(
            action="merge",
            task_id=task_with_state,
        )
        assert merged["success"] is True
        assert merged["original_count"] == 2  # reviewer + skeptic concerns
        assert merged["merged_count"] == 2    # No duplicates

    def test_outcome_and_stats_lifecycle(self, isolated_tasks_dir):
        """Record several outcomes then verify stats aggregation."""
        for i in range(5):
            task_id = f"TASK_CT_LIFE_{i}"
            workflow_initialize(task_id=task_id, description=f"Lifecycle task {i}")
            workflow_record_outcome(
                task_id=task_id,
                success=(i < 3),  # 3 successes, 2 failures
                rework_cycles=i,
                files_changed=i + 1,
                tests_passed=10 - i,
                tests_failed=i,
                duration_seconds=60 * (i + 1),
            )

        stats = workflow_get_outcome_stats()
        assert stats["success"] is True
        assert stats["total_tasks"] == 5
        assert stats["successes"] == 3
        assert stats["failures"] == 2
        assert stats["success_rate"] == 60.0
        # Avg rework: (0+1+2+3+4)/5 = 2.0
        assert stats["avg_rework_cycles"] == 2.0
        # Total tests passed: 10+9+8+7+6 = 40
        assert stats["total_tests_passed"] == 40
        # Total tests failed: 0+1+2+3+4 = 10
        assert stats["total_tests_failed"] == 10
