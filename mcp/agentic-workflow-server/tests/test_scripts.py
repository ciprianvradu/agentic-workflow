"""
Tests for deterministic CLI scripts (crew-config, crew-status, crew-cost-report, crew-stats).

These scripts replaced LLM-driven commands with pure Python. Tests verify they
produce correct output from known input data.

Run with: pytest tests/test_scripts.py -v
"""

import json
import subprocess
import sys
import tempfile
import pytest
from pathlib import Path
from datetime import datetime, timedelta

# Resolve scripts directory
SCRIPTS_DIR = Path(__file__).resolve().parent.parent.parent.parent / "scripts"


# ============================================================================
# Helpers
# ============================================================================

def _run_script(name: str, *args: str, cwd: str | None = None, env_override: dict | None = None) -> subprocess.CompletedProcess:
    """Run a script and return the result."""
    import os
    env = dict(os.environ)
    if env_override:
        env.update(env_override)
    return subprocess.run(
        [sys.executable, str(SCRIPTS_DIR / name), *args],
        capture_output=True, text=True, timeout=15,
        cwd=cwd, env=env,
    )


def _create_task(tasks_dir: Path, task_id: str, **state_overrides) -> Path:
    """Create a minimal task directory with state.json."""
    task_dir = tasks_dir / task_id
    task_dir.mkdir(parents=True, exist_ok=True)
    state = {
        "task_id": task_id,
        "phase": "architect",
        "phases_completed": [],
        "review_issues": [],
        "iteration": 1,
        "implementation_progress": {"total_steps": 0, "current_step": 0, "steps_completed": []},
        "created_at": datetime.now().isoformat(),
        "updated_at": datetime.now().isoformat(),
    }
    state.update(state_overrides)
    (task_dir / "state.json").write_text(json.dumps(state, indent=2))
    return task_dir


# ============================================================================
# crew-status.py
# ============================================================================

class TestCrewStatusScript:
    def test_no_tasks_dir(self, tmp_path):
        """Should handle missing .tasks/ gracefully."""
        result = _run_script("crew-status.py", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "No tasks found" in result.stdout

    def test_single_task(self, tmp_path):
        """Should display a single task."""
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001", description="Test task")

        # Need a .git dir so the script finds the repo root
        (tmp_path / ".git").mkdir()
        result = _run_script("crew-status.py", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "TASK_001" in result.stdout
        assert "1 total" in result.stdout

    def test_json_output(self, tmp_path):
        """--json should output valid JSON."""
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001")
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-status.py", "--json", cwd=str(tmp_path))
        assert result.returncode == 0
        data = json.loads(result.stdout)
        assert isinstance(data, list)
        assert len(data) == 1
        assert data[0]["task_id"] == "TASK_001"

    def test_complete_task_shows_done(self, tmp_path):
        """Completed task should show 'done' action."""
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001",
                     phase="technical_writer",
                     phases_completed=["developer", "implementer", "technical_writer"],
                     workflow_mode={"effective": "standard",
                                    "phases": ["developer", "implementer", "technical_writer"]})
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-status.py", cwd=str(tmp_path))
        assert "complete" in result.stdout
        assert "done" in result.stdout

    def test_multiple_tasks(self, tmp_path):
        """Should display multiple tasks."""
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001")
        _create_task(tasks_dir, "TASK_002")
        _create_task(tasks_dir, "TASK_003")
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-status.py", cwd=str(tmp_path))
        assert "3 total" in result.stdout


# ============================================================================
# crew-stats.py
# ============================================================================

class TestCrewStatsScript:
    def test_no_tasks(self, tmp_path):
        (tmp_path / ".git").mkdir()
        result = _run_script("crew-stats.py", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "No tasks found" in result.stdout

    def test_mode_distribution(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001",
                     workflow_mode={"effective": "standard", "phases": ["developer", "implementer", "technical_writer"]})
        _create_task(tasks_dir, "TASK_002",
                     workflow_mode={"effective": "thorough", "phases": ["architect", "developer", "reviewer", "skeptic", "implementer", "feedback", "technical_writer"]})
        _create_task(tasks_dir, "TASK_003",
                     workflow_mode={"effective": "standard", "phases": ["developer", "implementer", "technical_writer"]})
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-stats.py", cwd=str(tmp_path))
        assert "standard" in result.stdout
        assert "thorough" in result.stdout
        assert "3 total" in result.stdout or "Total:       3" in result.stdout

    def test_json_output(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001",
                     workflow_mode={"effective": "reviewed", "phases": ["architect", "developer", "reviewer", "implementer", "technical_writer"]})
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-stats.py", "--json", cwd=str(tmp_path))
        data = json.loads(result.stdout)
        assert data["total"] == 1
        assert "reviewed" in data["mode_distribution"]

    def test_iteration_stats(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001", iteration=1)
        _create_task(tasks_dir, "TASK_002", iteration=2)
        _create_task(tasks_dir, "TASK_003", iteration=3)
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-stats.py", "--json", cwd=str(tmp_path))
        data = json.loads(result.stdout)
        assert data["iterations"]["single_pass"] == 1
        assert data["iterations"]["one_revision"] == 1
        assert data["iterations"]["multi_revision"] == 1

    def test_error_patterns_counted(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        tasks_dir.mkdir(parents=True, exist_ok=True)
        _create_task(tasks_dir, "TASK_001")
        (tmp_path / ".git").mkdir()

        # Write some error patterns
        patterns = [
            {"signature": "ModuleNotFound", "times_seen": 5, "type": "compile", "solution": "fix"},
            {"signature": "TypeError", "times_seen": 2, "type": "runtime", "solution": "cast"},
        ]
        with open(tasks_dir / ".error_patterns.jsonl", "w") as f:
            for p in patterns:
                f.write(json.dumps(p) + "\n")

        result = _run_script("crew-stats.py", "--json", cwd=str(tmp_path))
        data = json.loads(result.stdout)
        assert data["error_patterns"]["total"] == 2
        assert data["error_patterns"]["top_pattern"]["signature"] == "ModuleNotFound"


# ============================================================================
# crew-cost-report.py
# ============================================================================

class TestCrewCostReportScript:
    def test_no_tasks(self, tmp_path):
        (tmp_path / ".git").mkdir()
        result = _run_script("crew-cost-report.py", cwd=str(tmp_path))
        assert result.returncode != 0 or "No tasks found" in (result.stdout + result.stderr)

    def test_no_cost_entries(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001", description="Test task")
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-cost-report.py", "TASK_001", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "No cost entries" in result.stdout

    def test_with_cost_entries(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        task_dir = _create_task(tasks_dir, "TASK_001", description="Test",
                                workflow_mode={"effective": "reviewed"})
        (tmp_path / ".git").mkdir()

        # Write cost entries
        entries = [
            {"agent": "architect", "model": "opus", "input_tokens": 10000, "output_tokens": 3000, "total_cost": 0.0},
            {"agent": "developer", "model": "opus", "input_tokens": 15000, "output_tokens": 5000, "total_cost": 0.0},
        ]
        with open(task_dir / "costs.jsonl", "w") as f:
            for e in entries:
                f.write(json.dumps(e) + "\n")

        result = _run_script("crew-cost-report.py", "TASK_001", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "architect" in result.stdout
        assert "developer" in result.stdout
        assert "TOTAL" in result.stdout

    def test_json_output(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001", description="Test",
                     workflow_mode={"effective": "standard"})
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-cost-report.py", "TASK_001", "--json", cwd=str(tmp_path))
        data = json.loads(result.stdout)
        assert data["task_id"] == "TASK_001"
        assert data["total_cost"] == 0.0

    def test_all_summary(self, tmp_path):
        tasks_dir = tmp_path / ".tasks"
        _create_task(tasks_dir, "TASK_001", workflow_mode={"effective": "standard"})
        _create_task(tasks_dir, "TASK_002", workflow_mode={"effective": "thorough"})
        (tmp_path / ".git").mkdir()

        result = _run_script("crew-cost-report.py", "--all", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "2 tasks" in result.stdout


# ============================================================================
# crew-config.py
# ============================================================================

class TestCrewConfigScript:
    def test_basic_output(self, tmp_path):
        """Should produce formatted config output."""
        (tmp_path / ".git").mkdir()
        # Create a minimal config
        config_dir = tmp_path / "config"
        config_dir.mkdir()
        config = {
            "checkpoints": {
                "planning": {"after_architect": True, "after_developer": False},
                "implementation": {"before_commit": True},
            },
            "models": {"default": "opus"},
            "knowledge_base": "docs/",
        }
        try:
            import yaml
            (config_dir / "workflow-config.yaml").write_text(yaml.dump(config))
        except ImportError:
            (config_dir / "workflow-config.yaml").write_text(json.dumps(config))

        result = _run_script("crew-config.py", cwd=str(tmp_path))
        assert result.returncode == 0
        assert "Workflow Configuration" in result.stdout

    def test_json_output(self, tmp_path):
        (tmp_path / ".git").mkdir()
        result = _run_script("crew-config.py", "--json", cwd=str(tmp_path))
        assert result.returncode == 0
        # Should be valid JSON (might be empty dict if no config found)
        data = json.loads(result.stdout)
        assert isinstance(data, dict)


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
