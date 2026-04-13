"""
Tests for crew_orchestrator.py CLI script.

Tests the orchestrator's subcommands which batch multiple MCP tool calls
into single instant decisions.

Run with: pytest tests/test_crew_orchestrator.py -v
"""

import json
import shutil
import subprocess
import sys
import pytest
from pathlib import Path

# Path to the orchestrator script
SCRIPT_DIR = Path(__file__).resolve().parent.parent.parent.parent / "scripts"
ORCHESTRATOR = SCRIPT_DIR / "crew_orchestrator.py"

# Also import MCP tools directly for setup
sys.path.insert(0, str(Path(__file__).parent.parent))

from agentic_workflow_server.state_tools import (
    get_tasks_dir,
    workflow_initialize,
    workflow_transition,
    workflow_complete_phase,
    workflow_set_mode,
    workflow_set_implementation_progress,
    workflow_complete_step,
    find_task_dir,
    _load_state,
)


def run_orchestrator(*args: str) -> dict:
    """Run the orchestrator script and return parsed JSON output."""
    result = subprocess.run(
        [sys.executable, str(ORCHESTRATOR)] + list(args),
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0 and not result.stdout:
        raise RuntimeError(f"Orchestrator failed: {result.stderr}")
    return json.loads(result.stdout)


@pytest.fixture
def clean_tasks_dir():
    """Clean up test task directories."""
    tasks_dir = get_tasks_dir()
    prefixes = ["TASK_CO_*", "TASK_ORCH_CO_*"]

    for pattern in prefixes:
        for d in tasks_dir.glob(pattern):
            if d.is_dir():
                shutil.rmtree(d)

    # Clean .active_task if leftover from previous test
    active_file = tasks_dir / ".active_task"
    if active_file.exists():
        active_file.unlink()

    # Snapshot existing TASK_XXX dirs so we can clean up auto-generated ones
    pre_existing = {d.name for d in tasks_dir.glob("TASK_*") if d.is_dir()}

    yield tasks_dir

    for pattern in prefixes:
        for d in tasks_dir.glob(pattern):
            if d.is_dir():
                shutil.rmtree(d)

    # Clean up any auto-generated TASK_XXX dirs created during the test
    for d in tasks_dir.glob("TASK_*"):
        if d.is_dir() and d.name not in pre_existing:
            shutil.rmtree(d)

    if active_file.exists():
        active_file.unlink()


# ============================================================================
# init subcommand
# ============================================================================

class TestInitCommand:
    def test_init_start(self, clean_tasks_dir):
        result = run_orchestrator("init", "--args", '"Fix typo in README" --mode quick')
        assert result["action"] == "start"
        assert result["task_id"]
        assert result["mode"] == "quick"
        assert "next" in result

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_status(self):
        result = run_orchestrator("init", "--args", "status")
        assert result["action"] == "status"

    def test_init_config(self):
        result = run_orchestrator("init", "--args", "config")
        assert result["action"] == "config"

    def test_init_empty_args(self):
        result = run_orchestrator("init", "--args", "")
        # With smart resume, empty args may find an incomplete task and resume it.
        # Either an error (no tasks) or a resume action is valid.
        assert result.get("error") is True or result.get("action") == "resume"

    def test_init_ask(self):
        result = run_orchestrator("init", "--args", 'ask architect "Should we use Redis?"')
        assert result["action"] == "ask"
        assert result["agent"] == "architect"

    def test_init_resume_missing_task(self):
        result = run_orchestrator("init", "--args", "resume TASK_NONEXISTENT")
        assert result.get("error") is True

    def test_init_with_beads(self, clean_tasks_dir):
        result = run_orchestrator("init", "--args", '--beads PROJ-99 "Add caching"')
        assert result["action"] == "start"
        assert result["beads_issue"] == "PROJ-99"

        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_no_description(self):
        result = run_orchestrator("init", "--args", "--mode standard")
        assert result.get("error") is True


# ============================================================================
# next subcommand
# ============================================================================

class TestNextCommand:
    def test_next_nonexistent(self):
        result = run_orchestrator("next", "--task-id", "TASK_NONEXISTENT")
        assert "error" in result

    def test_next_after_init(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_001", description="Test task")
        workflow_set_mode(mode="standard", task_id="TASK_CO_001")

        result = run_orchestrator("next", "--task-id", "TASK_CO_001")
        # Should suggest developer (first phase in minimal mode)
        assert result.get("action") in ("spawn_agent", "checkpoint", "process_output")


# ============================================================================
# agent-done subcommand
# ============================================================================

class TestAgentDoneCommand:
    def test_agent_done_basic(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_002", description="Test task")
        workflow_set_mode(mode="standard", task_id="TASK_CO_002")
        # Transition to planner (first phase in standard)
        workflow_transition(to_phase="planner", task_id="TASK_CO_002")

        # Create a dummy output file
        task_dir = clean_tasks_dir / "TASK_CO_002"
        output_file = task_dir / "planner.md"
        output_file.write_text("# Plan\nPlanning done.\n")

        result = run_orchestrator(
            "agent-done",
            "--task-id", "TASK_CO_002",
            "--agent", "planner",
            "--output-file", str(output_file),
        )
        assert result["action"] == "agent_done"
        assert result["phase_completed"] is True
        assert "next" in result

    def test_agent_done_with_cost(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_003", description="Test task")
        workflow_set_mode(mode="standard", task_id="TASK_CO_003")
        workflow_transition(to_phase="planner", task_id="TASK_CO_003")

        result = run_orchestrator(
            "agent-done",
            "--task-id", "TASK_CO_003",
            "--agent", "planner",
            "--input-tokens", "5000",
            "--output-tokens", "2000",
            "--model", "opus",
        )
        assert result["cost_recorded"] is True

    def test_agent_done_with_blocking_issues(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_004", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_004")
        workflow_transition(to_phase="planner", task_id="TASK_CO_004")
        workflow_complete_phase(task_id="TASK_CO_004")
        workflow_transition(to_phase="reviewer", task_id="TASK_CO_004")

        # Create output file with blocking issues
        task_dir = clean_tasks_dir / "TASK_CO_004"
        output_file = task_dir / "reviewer.md"
        output_file.write_text(
            '# Review\n<review_issues>[{"description": "Missing tests", "severity": "high"}]</review_issues>\n'
            '<recommendation>REVISE</recommendation>\n'
        )

        result = run_orchestrator(
            "agent-done",
            "--task-id", "TASK_CO_004",
            "--agent", "reviewer",
            "--output-file", str(output_file),
        )
        assert result.get("has_blocking_issues") is True


# ============================================================================
# checkpoint-done subcommand
# ============================================================================

class TestCheckpointDoneCommand:
    def test_approve(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_005", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_005")

        result = run_orchestrator(
            "checkpoint-done",
            "--task-id", "TASK_CO_005",
            "--decision", "approve",
        )
        assert result["decision"] == "approve"
        assert "next" in result

    def test_revise(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_006", description="Test task")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_006")
        workflow_transition(to_phase="planner", task_id="TASK_CO_006")
        workflow_complete_phase(task_id="TASK_CO_006")
        workflow_transition(to_phase="reviewer", task_id="TASK_CO_006")

        result = run_orchestrator(
            "checkpoint-done",
            "--task-id", "TASK_CO_006",
            "--decision", "revise",
            "--notes", "Need more error handling",
        )
        assert result["decision"] == "revise"


# ============================================================================
# impl-action subcommand
# ============================================================================

class TestImplActionCommand:
    def test_basic_implement(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_007", description="Test task")
        workflow_set_implementation_progress(total_steps=3, task_id="TASK_CO_007")

        result = run_orchestrator("impl-action", "--task-id", "TASK_CO_007")
        assert result["action"] == "implement_step"
        assert result["step_id"] == "step_1"

    def test_verification_passed(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_008", description="Test task")
        workflow_set_implementation_progress(total_steps=3, task_id="TASK_CO_008")

        result = run_orchestrator(
            "impl-action",
            "--task-id", "TASK_CO_008",
            "--verified", "true",
        )
        assert result["action"] == "next_step"

    def test_all_complete(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_009", description="Test task")
        workflow_set_implementation_progress(total_steps=2, task_id="TASK_CO_009")
        workflow_complete_step(step_id="step_1", task_id="TASK_CO_009")
        workflow_complete_step(step_id="step_2", task_id="TASK_CO_009")

        result = run_orchestrator("impl-action", "--task-id", "TASK_CO_009")
        assert result["action"] in ("complete", "checkpoint")

    def test_nonexistent(self):
        result = run_orchestrator("impl-action", "--task-id", "TASK_NONEXISTENT")
        assert "error" in result


# ============================================================================
# complete subcommand
# ============================================================================

class TestCompleteCommand:
    def test_basic_completion(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_010", description="Add caching")
        workflow_set_mode(mode="standard", task_id="TASK_CO_010")

        result = run_orchestrator(
            "complete",
            "--task-id", "TASK_CO_010",
            "--files", "src/cache.ts,src/api.ts",
        )
        assert result["task_id"] == "TASK_CO_010"
        assert "cost_summary" in result
        assert "commit_message" in result
        assert "jira_actions" in result

    def test_nonexistent(self):
        result = run_orchestrator("complete", "--task-id", "TASK_NONEXISTENT")
        assert "error" in result


# ============================================================================
# resume subcommand
# ============================================================================

class TestResumeCommand:
    def test_resume_basic(self, clean_tasks_dir):
        workflow_initialize(task_id="TASK_CO_011", description="Test resume")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_011")

        result = run_orchestrator("resume", "--task-id", "TASK_CO_011")
        assert result["action"] == "resume"
        assert "resume_state" in result
        assert "next" in result

    def test_resume_nonexistent(self):
        result = run_orchestrator("resume", "--task-id", "TASK_NONEXISTENT")
        assert result.get("error") is True


# ============================================================================
# Full workflow sequence
# ============================================================================

class TestFullSequence:
    def test_standard_workflow(self, clean_tasks_dir):
        """Test init → agent-done sequence for standard mode reaches complete."""
        # Init
        init_result = run_orchestrator("init", "--args", '"Add caching" --mode standard')
        assert init_result["action"] == "start"
        task_id = init_result["task_id"]
        task_dir = clean_tasks_dir / task_id

        # Standard mode: planner → implementer → technical_writer
        phases = ["planner", "implementer", "technical_writer"]
        output_files = {
            "planner": "planner.md",
            "implementer": "implementer.md",
            "technical_writer": "technical-writer.md",
        }

        for phase in phases:
            output_name = output_files[phase]
            (task_dir / output_name).write_text(f"# {phase}\nDone.\n")
            workflow_transition(to_phase=phase, task_id=task_id)
            agent_done = run_orchestrator(
                "agent-done", "--task-id", task_id,
                "--agent", phase,
                "--output-file", str(task_dir / output_name),
            )
            assert agent_done["phase_completed"] is True

        # After all phases, next should be complete
        final_next = agent_done["next"]
        assert final_next.get("action") in ("complete", "checkpoint")


# ============================================================================
# State update tests
# ============================================================================

class TestStateUpdates:
    """Tests for state.json updates that the orchestrator now owns."""

    def test_agent_done_transitions_to_next_phase(self, clean_tasks_dir):
        """After agent-done, state.json phase should reflect the next agent."""
        workflow_initialize(task_id="TASK_CO_SU_001", description="Test state update")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_SU_001")
        # Start at planner phase
        workflow_transition(to_phase="planner", task_id="TASK_CO_SU_001")

        # Create dummy output
        task_dir = clean_tasks_dir / "TASK_CO_SU_001"
        output_file = task_dir / "planner.md"
        output_file.write_text("# Plan\nPlanning done.\n")

        result = run_orchestrator(
            "agent-done",
            "--task-id", "TASK_CO_SU_001",
            "--agent", "planner",
            "--output-file", str(output_file),
        )
        assert result["phase_completed"] is True

        # If the next action is spawn_agent, state.json should have transitioned
        if result["next"].get("action") == "spawn_agent":
            expected_phase = result["next"]["agent"]
            assert result.get("transitioned_to") == expected_phase
            # Verify state.json on disk
            state = _load_state(task_dir)
            assert state["phase"] == expected_phase

    def test_init_turbo_sets_correct_first_phase(self, clean_tasks_dir):
        """Turbo mode (now aliased to standard) includes planner in its phases."""
        result = run_orchestrator("init", "--args", '"Build feature" --mode turbo')
        assert result["action"] == "start"
        task_id = result["task_id"]
        task_dir = clean_tasks_dir / task_id

        # Turbo now aliases to standard — verify the mode resolved correctly
        state = _load_state(task_dir)
        assert state["workflow_mode"]["effective"] == "standard"
        # The first agent may be a custom phase (product_manager, ba_designer) if
        # project-level custom_phases are configured. The key assertion is that
        # turbo aliased to standard and the workflow started successfully.
        assert result["next"].get("action") == "spawn_agent"

    def test_complete_marks_state_done(self, clean_tasks_dir):
        """After complete, state.json should have status='completed' and completed_at."""
        workflow_initialize(task_id="TASK_CO_SU_003", description="Test completion")
        workflow_set_mode(mode="standard", task_id="TASK_CO_SU_003")

        result = run_orchestrator(
            "complete",
            "--task-id", "TASK_CO_SU_003",
            "--files", "src/main.py,src/utils.py",
        )
        assert result["task_id"] == "TASK_CO_SU_003"

        # Verify state.json has completion markers
        task_dir = clean_tasks_dir / "TASK_CO_SU_003"
        state = _load_state(task_dir)
        assert state["status"] == "completed"
        assert "completed_at" in state
        assert state["files_changed"] == ["src/main.py", "src/utils.py"]

    def test_checkpoint_approve_completes_phase(self, clean_tasks_dir):
        """Approving at checkpoint should mark phase complete in state.json."""
        workflow_initialize(task_id="TASK_CO_SU_004", description="Test checkpoint")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_SU_004")
        workflow_transition(to_phase="planner", task_id="TASK_CO_SU_004")

        result = run_orchestrator(
            "checkpoint-done",
            "--task-id", "TASK_CO_SU_004",
            "--decision", "approve",
        )
        assert result["decision"] == "approve"

        # Verify phase is in phases_completed
        task_dir = clean_tasks_dir / "TASK_CO_SU_004"
        state = _load_state(task_dir)
        assert "planner" in state.get("phases_completed", [])


# ============================================================================
# .active_task session isolation
# ============================================================================

class TestActiveTaskLifecycle:
    """Tests for .tasks/.active_task file created on init/resume, removed on complete."""

    def test_init_creates_active_task(self, clean_tasks_dir):
        """cmd_init start writes .tasks/.active_task with the task ID."""
        active_file = clean_tasks_dir / ".active_task"
        if active_file.exists():
            active_file.unlink()

        result = run_orchestrator("init", "--args", '"Test active task" --mode quick')
        assert result["action"] == "start"
        task_id = result["task_id"]

        assert active_file.exists(), ".active_task should be created on init"
        assert active_file.read_text().strip() == task_id

        # Clean up
        task_dir = clean_tasks_dir / task_id
        if task_dir.exists():
            shutil.rmtree(task_dir)
        if active_file.exists():
            active_file.unlink()

    def test_complete_removes_active_task(self, clean_tasks_dir):
        """cmd_complete removes .tasks/.active_task when it matches the task ID."""
        active_file = clean_tasks_dir / ".active_task"

        workflow_initialize(task_id="TASK_CO_AT_001", description="Test removal")
        workflow_set_mode(mode="standard", task_id="TASK_CO_AT_001")

        # Simulate what init would do
        active_file.write_text("TASK_CO_AT_001\n")
        assert active_file.exists()

        run_orchestrator("complete", "--task-id", "TASK_CO_AT_001")

        assert not active_file.exists(), ".active_task should be removed on complete"

    def test_complete_preserves_other_active_task(self, clean_tasks_dir):
        """cmd_complete does NOT remove .active_task if it belongs to a different task."""
        active_file = clean_tasks_dir / ".active_task"

        workflow_initialize(task_id="TASK_CO_AT_002", description="Test preserve")
        workflow_set_mode(mode="standard", task_id="TASK_CO_AT_002")

        # .active_task points to a different task
        active_file.write_text("TASK_CO_AT_OTHER\n")

        run_orchestrator("complete", "--task-id", "TASK_CO_AT_002")

        assert active_file.exists(), ".active_task should be preserved for other task"
        assert active_file.read_text().strip() == "TASK_CO_AT_OTHER"

        # Clean up
        if active_file.exists():
            active_file.unlink()

    def test_resume_creates_active_task(self, clean_tasks_dir):
        """cmd_resume writes .tasks/.active_task with the task ID."""
        active_file = clean_tasks_dir / ".active_task"
        if active_file.exists():
            active_file.unlink()

        workflow_initialize(task_id="TASK_CO_AT_003", description="Test resume")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_AT_003")

        result = run_orchestrator("resume", "--task-id", "TASK_CO_AT_003")
        assert result["action"] == "resume"

        assert active_file.exists(), ".active_task should be created on resume"
        assert active_file.read_text().strip() == "TASK_CO_AT_003"

        # Clean up
        if active_file.exists():
            active_file.unlink()

    def test_init_resume_creates_active_task(self, clean_tasks_dir):
        """cmd_init with 'resume TASK_ID' also writes .active_task."""
        active_file = clean_tasks_dir / ".active_task"
        if active_file.exists():
            active_file.unlink()

        workflow_initialize(task_id="TASK_CO_AT_004", description="Test init resume")
        workflow_set_mode(mode="thorough", task_id="TASK_CO_AT_004")

        result = run_orchestrator("init", "--args", "resume TASK_CO_AT_004")
        assert result["action"] == "resume"

        assert active_file.exists(), ".active_task should be created on init resume"
        assert active_file.read_text().strip() == "TASK_CO_AT_004"

        # Clean up
        if active_file.exists():
            active_file.unlink()


class TestClassifyError:
    """Test that the catch-all error handler produces structured JSON, not raw tracebacks."""

    def test_nonexistent_task_returns_structured_error(self):
        """Running with a nonexistent task ID produces structured JSON, not a traceback."""
        result = subprocess.run(
            [sys.executable, str(ORCHESTRATOR), "next", "--task-id", "NONEXISTENT_TASK_XYZ"],
            capture_output=True, text=True, timeout=30,
        )
        output = json.loads(result.stdout)
        # Should be valid JSON with an "error" field, not a raw Python traceback
        assert "error" in output
        assert "Traceback" not in result.stdout
        assert "NoneType" not in result.stdout

    def test_classify_error_directly(self):
        """Import _classify_error and verify classification of known exception types."""
        import importlib.util
        spec = importlib.util.spec_from_file_location(
            "crew_orchestrator", str(ORCHESTRATOR))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)

        # FileNotFoundError with .tasks
        result = mod._classify_error(FileNotFoundError("[Errno 2] No such file or directory: '.tasks/FOO'"))
        assert result["error"] is True
        assert "Task directory not found" in result["errors"][0]
        assert "hint" in result

        # AttributeError with NoneType
        result = mod._classify_error(AttributeError("'NoneType' object has no attribute 'get'"))
        assert "Task state could not be loaded" in result["errors"][0]

        # KeyError
        result = mod._classify_error(KeyError("config"))
        assert "Missing expected field" in result["errors"][0]

        # json.JSONDecodeError
        result = mod._classify_error(json.JSONDecodeError("Expecting value", "", 0))
        assert "Corrupted state file" in result["errors"][0]

        # PermissionError
        result = mod._classify_error(PermissionError("Permission denied: '.tasks/foo'"))
        assert "Permission denied" in result["errors"][0]

        # Unknown error type
        result = mod._classify_error(RuntimeError("something weird"))
        assert "Unexpected error" in result["errors"][0]
        assert "RuntimeError" in result["errors"][0]


# ============================================================================
# crew_init full context return tests (AW-6q3)
# ============================================================================

class TestInitFullContextReturn:
    """Tests for cmd_init() returning full context: assembled_prompt, resume_command, worktree_info."""

    # --- VAL-CI-001: Init start returns assembled_prompt ---
    def test_init_start_returns_assembled_prompt(self, clean_tasks_dir):
        """Start path includes next.assembled_prompt with agent instructions and task context."""
        result = run_orchestrator(
            "init", "--args", '"Implement auth flow" --mode quick --no-resume',
            "--host", "claude",
        )
        assert result["action"] == "start"
        assert "next" in result
        next_action = result["next"]
        assert "assembled_prompt" in next_action, "next should contain assembled_prompt"
        prompt = next_action["assembled_prompt"]
        assert isinstance(prompt, str)
        assert len(prompt) > 0, "assembled_prompt should be non-empty"
        # Should contain the task description somewhere in the context
        assert "Implement auth flow" in prompt or "auth" in prompt.lower()

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    # --- VAL-CI-002: Init resume returns assembled_prompt with inlined content ---
    def test_init_resume_returns_assembled_prompt(self, clean_tasks_dir):
        """Resume path includes next.assembled_prompt with full inlined content."""
        # Create a task with completed planner phase, so resume advances to implementer
        workflow_initialize(task_id="TASK_CO_RESUME_PROMPT", description="Test resume prompt")
        workflow_set_mode(mode="standard", task_id="TASK_CO_RESUME_PROMPT")
        workflow_transition(to_phase="planner", task_id="TASK_CO_RESUME_PROMPT")

        # Write planner output and mark planner phase complete
        task_dir = clean_tasks_dir / "TASK_CO_RESUME_PROMPT"
        planner_output = task_dir / "planner.md"
        planner_output.write_text("# Plan\n## Implementation Plan\nStep 1: Do the thing\n")
        workflow_complete_phase(task_id="TASK_CO_RESUME_PROMPT")

        result = run_orchestrator(
            "init", "--args", "resume TASK_CO_RESUME_PROMPT",
            "--host", "claude",
        )
        assert result["action"] == "resume"
        assert "next" in result
        next_action = result["next"]
        # When the next action is spawn_agent, it should contain assembled_prompt
        if next_action.get("action") == "spawn_agent":
            assert "assembled_prompt" in next_action, "Resume next should contain assembled_prompt"
            prompt = next_action["assembled_prompt"]
            assert isinstance(prompt, str)
            assert len(prompt) > 0
        else:
            # Even for non-spawn actions (checkpoint, process_output), verify the action is valid
            assert next_action.get("action") in ("spawn_agent", "checkpoint", "process_output", "complete")

    # --- VAL-CI-003: Init returns worktree_info ---
    def test_init_start_returns_worktree_info(self, clean_tasks_dir):
        """Init result includes worktree_info with in_worktree, detected_task_id, worktree_path."""
        result = run_orchestrator(
            "init", "--args", '"Test worktree info" --mode quick --no-resume',
            "--host", "claude",
        )
        assert result["action"] == "start"
        assert "worktree_info" in result, "Start result should include worktree_info"
        wt_info = result["worktree_info"]
        assert "in_worktree" in wt_info
        assert "detected_task_id" in wt_info
        assert "worktree_path" in wt_info
        # When not in a worktree, in_worktree should be False
        assert wt_info["in_worktree"] is False

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    def test_init_resume_returns_worktree_info(self, clean_tasks_dir):
        """Resume result includes worktree_info."""
        workflow_initialize(task_id="TASK_CO_WT_INFO", description="Test worktree info resume")
        workflow_set_mode(mode="standard", task_id="TASK_CO_WT_INFO")

        result = run_orchestrator(
            "init", "--args", "resume TASK_CO_WT_INFO",
            "--host", "claude",
        )
        assert result["action"] == "resume"
        assert "worktree_info" in result, "Resume result should include worktree_info"
        wt_info = result["worktree_info"]
        assert "in_worktree" in wt_info

    # --- VAL-CI-004 through VAL-CI-009: Host-specific resume commands ---
    @pytest.mark.parametrize("host,expected_pattern", [
        ("claude", "/crew resume"),       # VAL-CI-004
        ("gemini", "@crew-resume"),       # VAL-CI-005
        ("copilot", "@crew-resume"),      # VAL-CI-006
        ("opencode", "/crew-resume"),     # VAL-CI-007
        ("devin", "/crew-resume"),        # VAL-CI-008
        ("droid", "/crew-resume"),        # VAL-CI-009
    ])
    def test_init_start_returns_host_specific_resume_command(
        self, clean_tasks_dir, host, expected_pattern
    ):
        """Init returns resume_command with correct host-specific syntax."""
        result = run_orchestrator(
            "init", "--args", f'"Test resume cmd for {host}" --mode quick --no-resume',
            "--host", host,
        )
        assert result["action"] == "start"
        assert "resume_command" in result, f"Start should include resume_command for {host}"
        assert expected_pattern in result["resume_command"], (
            f"resume_command for {host} should contain '{expected_pattern}', "
            f"got: {result['resume_command']}"
        )

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    @pytest.mark.parametrize("host,expected_pattern", [
        ("claude", "/crew resume"),
        ("gemini", "@crew-resume"),
        ("copilot", "@crew-resume"),
        ("opencode", "/crew-resume"),
        ("devin", "/crew-resume"),
        ("droid", "/crew-resume"),
    ])
    def test_init_resume_returns_host_specific_resume_command(
        self, clean_tasks_dir, host, expected_pattern
    ):
        """Resume returns resume_command with correct host-specific syntax."""
        workflow_initialize(task_id=f"TASK_CO_RC_{host.upper()}", description=f"Resume cmd {host}")
        workflow_set_mode(mode="standard", task_id=f"TASK_CO_RC_{host.upper()}")

        result = run_orchestrator(
            "init", "--args", f"resume TASK_CO_RC_{host.upper()}",
            "--host", host,
        )
        assert result["action"] == "resume"
        assert "resume_command" in result, f"Resume should include resume_command for {host}"
        assert expected_pattern in result["resume_command"]

    # --- VAL-CI-013: Fresh start with no prior task ---
    def test_fresh_start_no_prior_task(self, clean_tasks_dir):
        """New task: action=start, task_id present, mode set, assembled_prompt non-empty."""
        result = run_orchestrator(
            "init", "--args", '"Brand new task from scratch" --mode quick --no-resume',
            "--host", "claude",
        )
        assert result["action"] == "start"
        assert result["task_id"].startswith("TASK_")
        assert result["mode"] == "quick"
        assert "next" in result
        assert result["next"].get("assembled_prompt")
        assert "resume_command" in result
        assert "worktree_info" in result

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    # --- VAL-CI-014: Resume existing task ---
    def test_resume_existing_task_full_context(self, clean_tasks_dir):
        """Resume: action=resume, correct task_id, assembled_prompt present, resume_command present."""
        workflow_initialize(task_id="TASK_CO_RESUME_FULL", description="Full resume context")
        workflow_set_mode(mode="standard", task_id="TASK_CO_RESUME_FULL")
        workflow_transition(to_phase="planner", task_id="TASK_CO_RESUME_FULL")

        result = run_orchestrator(
            "init", "--args", "resume TASK_CO_RESUME_FULL",
            "--host", "gemini",
        )
        assert result["action"] == "resume"
        assert "resume_state" in result
        assert "next" in result
        assert "resume_command" in result
        assert "worktree_info" in result
        # Resume command should use gemini syntax
        assert "@crew-resume" in result["resume_command"]

    # --- VAL-CI-016: Invalid task_id returns structured error ---
    def test_invalid_task_id_returns_structured_error(self):
        """Resume INVALID_TASK returns error dict, no crash."""
        result = run_orchestrator(
            "init", "--args", "resume TASK_INVALID_DOES_NOT_EXIST",
            "--host", "claude",
        )
        assert result.get("error") is True
        assert "errors" in result
        assert any("not found" in e.lower() or "error" in e.lower() for e in result["errors"])

    # --- VAL-CI-017: assembled_prompt includes agent + task + human guidance ---
    def test_assembled_prompt_includes_all_sections(self, clean_tasks_dir):
        """Prompt contains agent instructions, task context, and human guidance trail."""
        workflow_initialize(task_id="TASK_CO_PROMPT_SECTIONS", description="Test all prompt sections")
        workflow_set_mode(mode="standard", task_id="TASK_CO_PROMPT_SECTIONS")

        # Write human guidance
        task_dir = clean_tasks_dir / "TASK_CO_PROMPT_SECTIONS"
        interactions_file = task_dir / "interactions.jsonl"
        import json as _json
        guidance_entry = _json.dumps({
            "role": "human",
            "type": "guidance",
            "content": "Make sure to handle edge cases carefully",
            "phase": "init",
        })
        interactions_file.write_text(guidance_entry + "\n")

        # Write task.md
        task_md = task_dir / "task.md"
        task_md.write_text("# Task\n\nTest all prompt sections\n")

        result = run_orchestrator(
            "init", "--args", "resume TASK_CO_PROMPT_SECTIONS",
            "--host", "claude",
        )
        assert result["action"] == "resume"
        next_action = result["next"]
        assert "assembled_prompt" in next_action
        prompt = next_action["assembled_prompt"]
        # Should contain human guidance
        assert "Human Guidance" in prompt or "edge cases" in prompt

    # --- VAL-CI-018: assembled_prompt includes platform context ---
    def test_assembled_prompt_includes_platform_context(self, clean_tasks_dir):
        """Prompt has Platform Context section with OS, shell, AI host."""
        result = run_orchestrator(
            "init", "--args", '"Test platform context" --mode quick --no-resume',
            "--host", "gemini",
        )
        assert result["action"] == "start"
        next_action = result["next"]
        assert "assembled_prompt" in next_action
        prompt = next_action["assembled_prompt"]
        assert "Platform Context" in prompt
        assert "gemini" in prompt.lower()

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)

    # --- VAL-CI-019: Resume state includes worktree status ---
    def test_resume_state_includes_worktree_status(self, clean_tasks_dir):
        """has_worktree reflects actual worktree state."""
        workflow_initialize(task_id="TASK_CO_WT_STATUS", description="Test worktree status")
        workflow_set_mode(mode="standard", task_id="TASK_CO_WT_STATUS")

        result = run_orchestrator(
            "init", "--args", "resume TASK_CO_WT_STATUS",
            "--host", "claude",
        )
        assert result["action"] == "resume"
        resume_state = result["resume_state"]
        # No worktree configured, so has_worktree should be False
        assert resume_state["has_worktree"] is False

    # --- VAL-CI-015: Worktree auto-detection triggers resume ---
    def test_worktree_autodetection_resume(self, clean_tasks_dir):
        """Worktree_info includes detection results from _detect_worktree_task_id."""
        # Can't easily test actual worktree detection in unit tests,
        # but we can verify the worktree_info structure includes the field
        result = run_orchestrator(
            "init", "--args", '"Test worktree autodetect" --mode quick --no-resume',
            "--host", "claude",
        )
        assert result["action"] == "start"
        wt_info = result["worktree_info"]
        # detected_task_id should be None when not in worktree
        assert wt_info["detected_task_id"] is None

        # Clean up
        task_dir = clean_tasks_dir / result["task_id"]
        if task_dir.exists():
            shutil.rmtree(task_dir)
