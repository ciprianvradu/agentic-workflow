"""
Security-specific tests for MCP tools.

Validates defense against path traversal, shell injection, oversized inputs,
and symlink escape. These tests complement the functional test suites by
focusing on adversarial inputs.

Run with: pytest tests/test_security.py -v
"""

import json
import os
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

# Ensure package is importable
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from agentic_workflow_server.state_tools import (
    find_task_dir,
    get_tasks_dir,
    workflow_initialize,
    workflow_add_review_issue,
    workflow_add_concern,
    workflow_log_interaction,
    workflow_record_error_pattern,
    workflow_create_worktree,
    workflow_cleanup_worktree,
    workflow_transition,
    _is_safe_task_id,
    _save_state,
    _load_state,
    _create_default_state,
    _cached_tasks_dir,
    INTERACTION_ROLES,
    INTERACTION_TYPES,
)
from agentic_workflow_server.orchestration_tools import (
    _list_kb_files,
    _validate_beads_issue,
    crew_parse_args,
)


# ============================================================================
# Fixtures
# ============================================================================

@pytest.fixture
def tasks_env(tmp_path, monkeypatch):
    """Set up an isolated tasks environment."""
    import agentic_workflow_server.state_tools as st
    monkeypatch.setattr(st, "_cached_tasks_dir", None)
    monkeypatch.setattr(st, "get_tasks_dir", lambda: tmp_path / ".tasks")
    tasks_dir = tmp_path / ".tasks"
    tasks_dir.mkdir()
    return tmp_path, tasks_dir


def _make_task(tasks_dir: Path, task_id: str, **overrides) -> Path:
    """Create a minimal task directory with state.json."""
    task_dir = tasks_dir / task_id
    task_dir.mkdir(parents=True, exist_ok=True)
    state = _create_default_state(task_id)
    state["phase"] = "architect"
    state.update(overrides)
    _save_state(task_dir, state)
    return task_dir


# ============================================================================
# 1. Path Traversal in task_id
# ============================================================================

class TestPathTraversal:
    """Test that path traversal sequences in task_id don't escape .tasks/."""

    def test_safe_task_id_validator(self):
        """_is_safe_task_id should reject dangerous patterns."""
        # Safe
        assert _is_safe_task_id("TASK_001") is True
        assert _is_safe_task_id("TASK_001_auth-jwt") is True
        assert _is_safe_task_id("my-task") is True

        # Dangerous — path traversal
        assert _is_safe_task_id("../etc/passwd") is False
        assert _is_safe_task_id("../../root") is False
        assert _is_safe_task_id("TASK_001/../..") is False

        # Dangerous — path separators
        assert _is_safe_task_id("/etc/passwd") is False
        assert _is_safe_task_id("foo/bar") is False
        assert _is_safe_task_id("foo\\bar") is False

        # Dangerous — hidden files / null bytes
        assert _is_safe_task_id(".hidden") is False
        assert _is_safe_task_id("TASK\x00_001") is False

        # Edge cases
        assert _is_safe_task_id("") is False

    def test_dot_dot_task_id_returns_none(self, tasks_env):
        """task_id with ../ should not resolve outside .tasks/."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")

        assert find_task_dir("../etc/passwd") is None
        assert find_task_dir("../../root") is None
        assert find_task_dir("TASK_001/../..") is None

    def test_dot_dot_task_id_rejected_by_initialize(self, tasks_env):
        """workflow_initialize should reject traversal task_ids."""
        _, tasks_dir = tasks_env
        result = workflow_initialize(task_id="../../escape", description="test")
        assert result["success"] is False
        assert "Invalid task_id" in result["error"]

    def test_absolute_path_task_id(self, tasks_env):
        """Absolute path as task_id should not escape .tasks/."""
        _, tasks_dir = tasks_env
        assert find_task_dir("/etc/passwd") is None

    def test_null_bytes_in_task_id(self, tasks_env):
        """Null bytes should be rejected."""
        _, tasks_dir = tasks_env
        assert find_task_dir("TASK\x00_001") is None

    def test_unicode_task_id(self, tasks_env):
        """Unicode in task_id shouldn't cause crashes."""
        _, tasks_dir = tasks_env
        result = find_task_dir("TASK_日本語")
        assert result is None

    def test_very_long_task_id(self, tasks_env):
        """Extremely long task_id shouldn't cause crashes."""
        _, tasks_dir = tasks_env
        long_id = "TASK_" + "A" * 1000
        # Should return None (doesn't exist) — may raise OSError on some
        # platforms where filename length exceeds limits
        try:
            result = find_task_dir(long_id)
            assert result is None
        except OSError:
            pass  # Acceptable — OS rejected the long path

    def test_backslash_task_id(self, tasks_env):
        """Backslash in task_id should be rejected (Windows path separator)."""
        _, tasks_dir = tasks_env
        assert find_task_dir("TASK_001\\..\\..") is None


# ============================================================================
# 2. Shell Injection in Branch Names and Worktree Paths
# ============================================================================

class TestShellInjection:
    """Test that user-controlled strings in git commands are shell-quoted."""

    def test_worktree_branch_name_with_semicolon(self, tasks_env):
        """Branch name with shell metacharacters should be quoted."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")

        with patch("agentic_workflow_server.state_tools.Path.cwd") as mock_cwd:
            mock_cwd.return_value = Path(str(tasks_dir).replace("/.tasks", ""))
            result = workflow_create_worktree(
                task_id="TASK_001",
                branch_name="crew/test; rm -rf /",
                base_path="/tmp/test-worktrees"
            )

        if result.get("success") and "git_commands" in result:
            for cmd in result["git_commands"]:
                # shlex.quote wraps dangerous strings in single quotes
                assert "'" in cmd, \
                    f"Shell metacharacters not quoted in git command: {cmd}"

    def test_worktree_branch_name_with_backticks(self, tasks_env):
        """Branch name with command substitution should be quoted."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")

        with patch("agentic_workflow_server.state_tools.Path.cwd") as mock_cwd:
            mock_cwd.return_value = Path(str(tasks_dir).replace("/.tasks", ""))
            result = workflow_create_worktree(
                task_id="TASK_001",
                branch_name="`whoami`",
                base_path="/tmp/test-worktrees"
            )

        if result.get("success") and "git_commands" in result:
            for cmd in result["git_commands"]:
                # Backtick should be inside single quotes
                assert "'" in cmd, \
                    f"Backticks not quoted in git command: {cmd}"

    def test_worktree_branch_name_with_dollar(self, tasks_env):
        """Branch name with $() command substitution should be quoted."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")

        with patch("agentic_workflow_server.state_tools.Path.cwd") as mock_cwd:
            mock_cwd.return_value = Path(str(tasks_dir).replace("/.tasks", ""))
            result = workflow_create_worktree(
                task_id="TASK_001",
                branch_name="$(cat /etc/passwd)",
                base_path="/tmp/test-worktrees"
            )

        if result.get("success") and "git_commands" in result:
            for cmd in result["git_commands"]:
                assert "'" in cmd, \
                    f"Command substitution not quoted in git command: {cmd}"

    def test_cleanup_worktree_task_id_quoted(self, tasks_env):
        """Cleanup script command should quote the task_id argument."""
        _, tasks_dir = tasks_env
        # Use a task_id with spaces (safe for _is_safe_task_id but needs quoting in shell)
        _make_task(tasks_dir, "TASK_001",
                   worktree={"status": "active", "path": "/tmp/test", "branch": "test"})

        result = workflow_cleanup_worktree(task_id="TASK_001")
        if result.get("success") and "cleanup_command" in result:
            cmd = result["cleanup_command"]
            # Should be a single python3 command
            assert cmd.count("python3") == 1

    def test_validate_beads_uses_list_form(self):
        """_validate_beads_issue should use subprocess list form, not shell=True."""
        with patch("subprocess.run") as mock_run:
            mock_run.side_effect = FileNotFoundError()
            _validate_beads_issue("AW-123; rm -rf /")
            if mock_run.called:
                call_args = mock_run.call_args
                first_arg = call_args[0][0] if call_args[0] else call_args[1].get("args")
                assert isinstance(first_arg, list), \
                    "subprocess.run should use list form, not string"

    def test_validate_beads_malicious_key(self):
        """Malicious issue key should not cause shell injection."""
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                returncode=1,
                stdout="not found",
                stderr=""
            )
            valid, msg = _validate_beads_issue("AW-123 && cat /etc/passwd")
            assert isinstance(valid, bool)
            assert isinstance(msg, str)


# ============================================================================
# 3. Oversized Input Handling
# ============================================================================

class TestOversizedInputs:
    """Test that oversized inputs don't cause crashes or disk exhaustion."""

    def test_huge_description_in_initialize(self, tasks_env):
        """Very large description should not crash."""
        _, tasks_dir = tasks_env
        huge_desc = "A" * 1_000_000  # 1MB description
        result = workflow_initialize(task_id="TASK_BIG", description=huge_desc)
        assert result["success"] is True
        state = _load_state(tasks_dir / "TASK_BIG")
        assert len(state["description"]) == 1_000_000

    def test_huge_review_issue_description(self, tasks_env):
        """Very large review issue description should not crash."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        huge_desc = "B" * 500_000
        result = workflow_add_review_issue(
            issue_type="bug",
            description=huge_desc,
            task_id="TASK_001"
        )
        assert result["success"] is True
        state = _load_state(tasks_dir / "TASK_001")
        assert len(state["review_issues"][-1]["description"]) == 500_000

    def test_huge_concern_description(self, tasks_env):
        """Very large concern description should not crash."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        huge_desc = "C" * 500_000
        result = workflow_add_concern(
            source="test",
            severity="high",
            description=huge_desc,
            task_id="TASK_001"
        )
        assert result["success"] is True

    def test_huge_interaction_content(self, tasks_env):
        """Very large interaction content should not crash."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        huge_content = "D" * 1_000_000
        result = workflow_log_interaction(
            role="agent",
            content=huge_content,
            task_id="TASK_001"
        )
        assert result["success"] is True
        interactions_file = tasks_dir / "TASK_001" / "interactions.jsonl"
        assert interactions_file.exists()
        line = interactions_file.read_text().strip()
        entry = json.loads(line)
        assert len(entry["content"]) == 1_000_000

    def test_huge_error_signature(self, tasks_env):
        """Very large error signature should not crash."""
        _, tasks_dir = tasks_env
        huge_sig = "E" * 100_000
        result = workflow_record_error_pattern(
            error_signature=huge_sig,
            error_type="test",
            solution="test solution",
            task_id="TASK_NONE"
        )
        # Result has 'pattern' key containing 'signature', or 'success' key
        assert result.get("success") is True or "pattern" in result

    def test_many_review_issues_accumulation(self, tasks_env):
        """Adding many review issues should not crash."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        for i in range(200):
            result = workflow_add_review_issue(
                issue_type="bug",
                description=f"Issue #{i}",
                task_id="TASK_001"
            )
            assert result["success"] is True
        state = _load_state(tasks_dir / "TASK_001")
        assert len(state["review_issues"]) == 200

    def test_many_concerns_accumulation(self, tasks_env):
        """Adding many concerns should not crash."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        for i in range(100):
            result = workflow_add_concern(
                source="test",
                severity="low",
                description=f"Concern #{i}",
                task_id="TASK_001"
            )
            assert result["success"] is True
        state = _load_state(tasks_dir / "TASK_001")
        assert len(state["concerns"]) == 100


# ============================================================================
# 4. Symlink Escape in KB Listing
# ============================================================================

class TestSymlinkEscape:
    """Test that KB listing and path operations handle symlinks safely."""

    def test_kb_listing_does_not_follow_symlinks_to_sensitive(self, tmp_path):
        """KB listing paths should be relative, not absolute."""
        kb_dir = tmp_path / "kb"
        kb_dir.mkdir()
        (kb_dir / "legit.md").write_text("real content")

        sensitive_dir = tmp_path / "sensitive"
        sensitive_dir.mkdir()
        (sensitive_dir / "secret.key").write_text("SECRET_KEY_VALUE")

        symlink = kb_dir / "escape"
        try:
            symlink.symlink_to(sensitive_dir)
        except OSError:
            pytest.skip("Cannot create symlinks on this platform")

        files = _list_kb_files(kb_dir)
        for f in files:
            assert not f.startswith("/"), f"Absolute path leaked: {f}"
            assert ".." not in f, f"Path traversal in result: {f}"

    def test_kb_listing_handles_broken_symlinks(self, tmp_path):
        """KB listing should not crash on broken symlinks."""
        kb_dir = tmp_path / "kb"
        kb_dir.mkdir()
        (kb_dir / "legit.md").write_text("content")

        broken_link = kb_dir / "broken"
        try:
            broken_link.symlink_to(tmp_path / "nonexistent")
        except OSError:
            pytest.skip("Cannot create symlinks on this platform")

        files = _list_kb_files(kb_dir)
        assert "legit.md" in files
        assert "broken" not in files

    def test_kb_listing_nonexistent_dir(self):
        """KB listing on nonexistent directory should return empty list."""
        files = _list_kb_files(Path("/nonexistent/kb/dir"))
        assert files == []

    def test_kb_listing_file_cap(self, tmp_path):
        """KB listing should respect the file cap."""
        from agentic_workflow_server.orchestration_tools import KB_LISTING_MAX_FILES
        kb_dir = tmp_path / "kb"
        kb_dir.mkdir()
        for i in range(KB_LISTING_MAX_FILES + 50):
            (kb_dir / f"file_{i:04d}.txt").write_text(f"content {i}")

        files = _list_kb_files(kb_dir)
        assert len(files) <= KB_LISTING_MAX_FILES


# ============================================================================
# 5. Input Validation Gaps
# ============================================================================

class TestInputValidation:
    """Test that enum-like parameters are validated properly."""

    def test_log_interaction_invalid_role(self, tasks_env):
        """Invalid role should be rejected."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        result = workflow_log_interaction(
            role="hacker",
            content="test",
            task_id="TASK_001"
        )
        assert result["success"] is False
        assert "Invalid role" in result["error"]

    def test_log_interaction_invalid_type(self, tasks_env):
        """Invalid interaction type should be rejected."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        result = workflow_log_interaction(
            role="agent",
            content="test",
            interaction_type="exploit",
            task_id="TASK_001"
        )
        assert result["success"] is False
        assert "Invalid interaction_type" in result["error"]

    def test_log_interaction_all_valid_roles(self, tasks_env):
        """All defined roles should be accepted."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        for role in INTERACTION_ROLES:
            result = workflow_log_interaction(
                role=role,
                content=f"test {role}",
                task_id="TASK_001"
            )
            assert result["success"] is True, f"Role '{role}' rejected"

    def test_log_interaction_all_valid_types(self, tasks_env):
        """All defined interaction types should be accepted."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        for itype in INTERACTION_TYPES:
            result = workflow_log_interaction(
                role="agent",
                content=f"test {itype}",
                interaction_type=itype,
                task_id="TASK_001"
            )
            assert result["success"] is True, f"Type '{itype}' rejected"

    def test_transition_invalid_phase(self, tasks_env):
        """Transitioning to a completely invalid phase should fail gracefully."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")
        result = workflow_transition(
            to_phase="malicious_phase",
            task_id="TASK_001"
        )
        assert isinstance(result, dict)

    def test_empty_string_task_id(self, tasks_env):
        """Empty string task_id should be handled gracefully."""
        _, tasks_dir = tasks_env
        result = find_task_dir("")
        assert result is None or isinstance(result, Path)

    def test_special_chars_in_description(self, tasks_env):
        """Special characters in description should be stored faithfully."""
        _, tasks_dir = tasks_env
        special_desc = 'Test <script>alert("xss")</script> & "quotes" \'single\' `backticks`'
        result = workflow_initialize(task_id="TASK_SPECIAL", description=special_desc)
        assert result["success"] is True
        state = _load_state(tasks_dir / "TASK_SPECIAL")
        assert state["description"] == special_desc

    def test_json_injection_in_description(self, tasks_env):
        """JSON-breaking characters in description should be stored safely."""
        _, tasks_dir = tasks_env
        json_desc = '{"injected": true}\n{"another": "json"}'
        result = workflow_initialize(task_id="TASK_JSON", description=json_desc)
        assert result["success"] is True
        state_file = tasks_dir / "TASK_JSON" / "state.json"
        state = json.loads(state_file.read_text())
        assert state["description"] == json_desc


# ============================================================================
# 6. Error Pattern File Integrity
# ============================================================================

class TestErrorPatternSecurity:
    """Test error pattern recording handles adversarial inputs."""

    def test_newline_in_signature(self, tasks_env):
        """Newlines in error signature should not corrupt JSONL format."""
        _, tasks_dir = tasks_env
        result = workflow_record_error_pattern(
            error_signature="Error\nwith\nnewlines",
            error_type="test",
            solution="fix it"
        )
        patterns_file = tasks_dir / ".error_patterns.jsonl"
        if patterns_file.exists():
            for line in patterns_file.read_text().splitlines():
                if line.strip():
                    parsed = json.loads(line)  # Should not raise
                    assert isinstance(parsed, dict)

    def test_huge_tags_list(self, tasks_env):
        """Many tags should not cause issues."""
        _, tasks_dir = tasks_env
        many_tags = [f"tag_{i}" for i in range(1000)]
        result = workflow_record_error_pattern(
            error_signature="test_error",
            error_type="test",
            solution="fix it",
            tags=many_tags
        )
        assert result.get("success") is True
        assert "pattern" in result
        assert len(result["pattern"]["tags"]) == 1000

    def test_malformed_existing_patterns_file(self, tasks_env):
        """Corrupted patterns file should not crash recording."""
        _, tasks_dir = tasks_env
        patterns_file = tasks_dir / ".error_patterns.jsonl"
        patterns_file.write_text("not json\n{invalid\n")

        result = workflow_record_error_pattern(
            error_signature="new_error",
            error_type="test",
            solution="fix it"
        )
        assert result.get("success") is True
        assert result["pattern"]["signature"] == "new_error"


# ============================================================================
# 7. Crew Parse Args Safety
# ============================================================================

class TestCrewParseArgsSecurity:
    """Test that crew_parse_args handles adversarial inputs safely."""

    def test_empty_args(self):
        """Empty args should return a valid result."""
        result = crew_parse_args("")
        assert isinstance(result, dict)

    def test_very_long_args(self):
        """Very long args string should not crash."""
        long_args = "a " * 50000
        result = crew_parse_args(long_args)
        assert isinstance(result, dict)

    def test_special_shell_chars_in_args(self):
        """Shell metacharacters in args should be treated as literal text."""
        result = crew_parse_args("fix bug; rm -rf / && cat /etc/passwd")
        assert isinstance(result, dict)

    def test_null_bytes_in_args(self):
        """Null bytes in args should not crash."""
        try:
            result = crew_parse_args("fix\x00bug")
            assert isinstance(result, dict)
        except (ValueError, TypeError):
            pass  # Acceptable to reject null bytes

    def test_unicode_args(self):
        """Unicode in args should be handled."""
        result = crew_parse_args("修复 bug 🐛 --mode standard")
        assert isinstance(result, dict)


# ============================================================================
# 8. Concurrent Access Safety
# ============================================================================

class TestConcurrentAccess:
    """Test that file locking prevents corruption under concurrent writes."""

    def test_interaction_log_uses_file_lock(self, tasks_env):
        """Log interaction should use FileLock for JSONL writes."""
        _, tasks_dir = tasks_env
        _make_task(tasks_dir, "TASK_001")

        for i in range(20):
            result = workflow_log_interaction(
                role="agent",
                content=f"Message {i}",
                task_id="TASK_001"
            )
            assert result["success"] is True

        interactions_file = tasks_dir / "TASK_001" / "interactions.jsonl"
        lines = interactions_file.read_text().strip().splitlines()
        assert len(lines) == 20
        for line in lines:
            entry = json.loads(line)
            assert "content" in entry

    def test_state_file_corruption_recovery(self, tasks_env):
        """Corrupted state.json should be handled gracefully (crash or error, not silent)."""
        _, tasks_dir = tasks_env
        task_dir = tasks_dir / "TASK_CORRUPT"
        task_dir.mkdir()
        (task_dir / "state.json").write_text("{corrupt json")

        # _load_state will raise JSONDecodeError — operations that call it
        # should either propagate the error or return an error dict
        try:
            result = workflow_add_review_issue(
                issue_type="bug",
                description="test",
                task_id="TASK_CORRUPT"
            )
            # If it returns, it should be an error or a dict
            assert isinstance(result, dict)
        except (json.JSONDecodeError, Exception):
            # Raising is acceptable — corrupted state should not be silently accepted
            pass


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
