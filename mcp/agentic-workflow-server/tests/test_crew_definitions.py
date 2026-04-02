"""Tests for crew_definitions module."""

import copy
import pytest
from agentic_workflow_server.crew_definitions import (
    SOFTWARE_DEV_CREW,
    MODE_ALIASES,
    resolve_crew,
    get_pipelines,
    get_pipeline,
    get_all_pipeline_names,
    get_roles,
    get_role_prompt_file,
    get_role_category,
    get_category_max_turns,
    get_effort_level,
    get_specialized_roles,
    get_auto_detection_rules,
    get_phase_order,
    _merge_crew_with_defaults,
    _synthesize_from_legacy,
)


class TestSoftwareDevCrew:
    """The built-in default crew is well-formed."""

    def test_has_required_keys(self):
        for key in ("name", "roles", "pipelines", "effort_levels",
                     "auto_detection", "specialized_roles", "categories"):
            assert key in SOFTWARE_DEV_CREW, f"Missing key: {key}"

    def test_all_pipeline_phases_are_defined_roles(self):
        roles = SOFTWARE_DEV_CREW["roles"]
        for mode, pipeline in SOFTWARE_DEV_CREW["pipelines"].items():
            for phase in pipeline["phases"]:
                assert phase in roles, f"Pipeline '{mode}' references undefined role '{phase}'"

    def test_all_specialized_roles_are_defined(self):
        roles = SOFTWARE_DEV_CREW["roles"]
        for name in SOFTWARE_DEV_CREW["specialized_roles"]:
            assert name in roles, f"Specialized role '{name}' not in roles"

    def test_mode_aliases_map_to_existing_pipelines(self):
        pipelines = SOFTWARE_DEV_CREW["pipelines"]
        for alias, target in MODE_ALIASES.items():
            assert target in pipelines, f"Alias '{alias}' targets non-existent pipeline '{target}'"


class TestResolveCrew:
    """resolve_crew returns a complete crew from any config."""

    def test_empty_config_returns_default(self):
        crew = resolve_crew({})
        assert crew["name"] == "software-dev"
        assert "planner" in crew["roles"]
        assert "standard" in crew["pipelines"]

    def test_explicit_crew_replaces_defaults(self):
        config = {
            "crew": {
                "name": "my-custom",
                "description": "Custom workflow",
                "roles": {
                    "writer": {"prompt_file": "writer.md", "category": "creation"},
                },
                "pipelines": {
                    "quick": {"phases": ["writer"], "description": "Fast"},
                },
            }
        }
        crew = resolve_crew(config)
        assert crew["name"] == "my-custom"
        assert "writer" in crew["roles"]
        # Default roles should NOT be present (no _extend)
        assert "planner" not in crew["roles"]

    def test_extend_merges_with_defaults(self):
        config = {
            "crew": {
                "name": "extended-dev",
                "roles": {
                    "_extend": True,
                    "my_linter": {"prompt_file": "linter.md", "category": "planning"},
                },
            }
        }
        crew = resolve_crew(config)
        assert crew["name"] == "extended-dev"
        assert "planner" in crew["roles"]  # from default
        assert "my_linter" in crew["roles"]  # from extension

    def test_extend_is_idempotent(self):
        """Calling resolve_crew twice with the same config must produce identical results."""
        config = {
            "crew": {
                "name": "idem-test",
                "roles": {
                    "_extend": True,
                    "extra_role": {"prompt_file": "extra.md", "category": "planning"},
                },
            }
        }
        crew1 = resolve_crew(config)
        crew2 = resolve_crew(config)
        assert crew1["roles"] == crew2["roles"]
        # Config must not be mutated by resolve_crew
        assert "_extend" in config["crew"]["roles"]

    def test_missing_sections_fall_back_to_defaults(self):
        config = {
            "crew": {
                "name": "partial",
                "roles": {
                    "analyst": {"prompt_file": "analyst.md", "category": "analysis"},
                },
            }
        }
        crew = resolve_crew(config)
        # Pipelines not specified → fall back to software-dev defaults
        assert "standard" in crew["pipelines"]
        # Categories not specified → fall back
        assert "planning" in crew["categories"]


class TestSynthesizeFromLegacy:
    """Legacy config keys are correctly mapped to crew definition."""

    def test_workflow_modes_become_pipelines(self):
        config = {
            "workflow_modes": {
                "modes": {
                    "express": {
                        "phases": ["implementer"],
                        "description": "Express mode",
                    }
                }
            }
        }
        crew = _synthesize_from_legacy(config)
        assert "express" in crew["pipelines"]
        assert crew["pipelines"]["express"]["phases"] == ["implementer"]

    def test_specialized_agents_become_specialized_roles(self):
        config = {
            "specialized_agents": {
                "my_checker": {
                    "enabled": "auto",
                    "triggers": {"keywords": ["check", "verify"]},
                    "position": "after_reviewer",
                }
            }
        }
        crew = _synthesize_from_legacy(config)
        assert "my_checker" in crew["specialized_roles"]
        assert crew["specialized_roles"]["my_checker"]["triggers"]["keywords"] == ["check", "verify"]

    def test_effort_levels_from_config(self):
        config = {
            "effort_levels": {
                "standard": {"planner": "max"},
            }
        }
        crew = _synthesize_from_legacy(config)
        assert crew["effort_levels"]["standard"]["planner"] == "max"

    def test_subagent_limits_map_to_categories(self):
        config = {
            "subagent_limits": {
                "max_turns": {
                    "planning_agents": 99,
                    "implementation_agents": 88,
                }
            }
        }
        crew = _synthesize_from_legacy(config)
        assert crew["categories"]["planning"]["max_turns"] == 99
        assert crew["categories"]["execution"]["max_turns"] == 88

    def test_unknown_pipeline_roles_auto_created(self):
        config = {
            "workflow_modes": {
                "modes": {
                    "custom": {
                        "phases": ["axiom_miner", "planner", "implementer"],
                    }
                }
            }
        }
        crew = _synthesize_from_legacy(config)
        assert "axiom_miner" in crew["roles"]
        assert crew["roles"]["axiom_miner"]["prompt_file"] == "axiom-miner.md"


class TestAccessorFunctions:
    """Accessor functions correctly read from crew definitions."""

    def test_get_pipeline_resolves_aliases(self):
        pipeline = get_pipeline(SOFTWARE_DEV_CREW, "full")
        assert pipeline is not None
        assert pipeline == SOFTWARE_DEV_CREW["pipelines"]["thorough"]

    def test_get_pipeline_returns_none_for_unknown(self):
        assert get_pipeline(SOFTWARE_DEV_CREW, "nonexistent") is None

    def test_get_role_prompt_file_known_role(self):
        assert get_role_prompt_file(SOFTWARE_DEV_CREW, "planner") == "planner.md"

    def test_get_role_prompt_file_unknown_role_derives_name(self):
        assert get_role_prompt_file(SOFTWARE_DEV_CREW, "my_custom_agent") == "my-custom-agent.md"

    def test_get_role_category(self):
        assert get_role_category(SOFTWARE_DEV_CREW, "planner") == "planning"
        assert get_role_category(SOFTWARE_DEV_CREW, "implementer") == "execution"
        assert get_role_category(SOFTWARE_DEV_CREW, "technical_writer") == "documentation"

    def test_get_category_max_turns(self):
        assert get_category_max_turns(SOFTWARE_DEV_CREW, "planning") == 30
        assert get_category_max_turns(SOFTWARE_DEV_CREW, "execution") == 50
        assert get_category_max_turns(SOFTWARE_DEV_CREW, "documentation") == 20

    def test_get_effort_level(self):
        assert get_effort_level(SOFTWARE_DEV_CREW, "quick", "implementer") == "low"
        assert get_effort_level(SOFTWARE_DEV_CREW, "thorough", "planner") == "max"
        # Unknown role defaults to "high"
        assert get_effort_level(SOFTWARE_DEV_CREW, "standard", "unknown_agent") == "high"

    def test_get_phase_order_includes_all_roles(self):
        order = get_phase_order(SOFTWARE_DEV_CREW)
        roles = set(SOFTWARE_DEV_CREW["roles"].keys())
        assert roles == set(order)

    def test_get_phase_order_longest_pipeline_first(self):
        order = get_phase_order(SOFTWARE_DEV_CREW)
        thorough_phases = SOFTWARE_DEV_CREW["pipelines"]["thorough"]["phases"]
        # All thorough phases should appear in order at the start
        for i, phase in enumerate(thorough_phases):
            assert order.index(phase) == i


class TestCustomCrewAccessors:
    """Accessors work with a non-software-dev crew."""

    @pytest.fixture
    def content_crew(self):
        return {
            "name": "content-creation",
            "roles": {
                "researcher": {"prompt_file": "researcher.md", "category": "research"},
                "writer": {"prompt_file": "writer.md", "category": "creation"},
                "editor": {"prompt_file": "editor.md", "category": "review"},
            },
            "pipelines": {
                "quick": {"phases": ["writer"], "description": "Fast"},
                "standard": {"phases": ["researcher", "writer", "editor"]},
            },
            "categories": {
                "research": {"max_turns": 25},
                "creation": {"max_turns": 40},
                "review": {"max_turns": 20},
            },
            "effort_levels": {
                "quick": {"writer": "low"},
                "standard": {"researcher": "high", "writer": "high", "editor": "medium"},
            },
            "specialized_roles": {},
            "auto_detection": {},
        }

    def test_get_pipeline_custom(self, content_crew):
        p = get_pipeline(content_crew, "standard")
        assert p["phases"] == ["researcher", "writer", "editor"]

    def test_get_role_category_custom(self, content_crew):
        assert get_role_category(content_crew, "writer") == "creation"

    def test_get_category_max_turns_custom(self, content_crew):
        assert get_category_max_turns(content_crew, "creation") == 40

    def test_get_effort_level_custom(self, content_crew):
        assert get_effort_level(content_crew, "standard", "editor") == "medium"

    def test_phase_order_custom(self, content_crew):
        order = get_phase_order(content_crew)
        assert order[:3] == ["researcher", "writer", "editor"]
