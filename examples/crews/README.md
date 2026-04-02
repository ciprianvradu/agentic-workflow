# Crew Definitions — README
#
# These example YAML files show how to define custom crews for
# non-software-development workflows using the agentic-workflow system.
#
# ## What is a Crew Definition?
#
# A crew definition packages everything the orchestrator needs for a
# domain-specific workflow:
#
# - **Roles** — the cast of AI agents, each with a prompt file and category
# - **Pipelines** — named phase sequences (like "quick", "standard", "thorough")
# - **Auto-detection** — keywords that pick a pipeline from task descriptions
# - **Specialized roles** — optional agents triggered by task content
# - **Categories** — groups for turn limits and cost tracking
# - **Effort levels** — per-pipeline thinking depth for each role
#
# ## How to Use
#
# 1. Copy the `crew:` block from an example into your project's
#    `workflow-config.yaml` (or `<platform_dir>/workflow-config.yaml`)
#
# 2. Create prompt files for each role in your agents directory
#    (e.g., `~/.claude/agents/researcher.md`)
#
# 3. Run the workflow as usual — the orchestrator reads the crew definition
#    and sequences your custom roles through the configured pipelines
#
# ## Available Examples
#
# - `content-creation.yaml` — editorial workflows (research → write → edit)
# - `research-analysis.yaml` — investigation workflows (scout → analyze → synthesize)
#
# ## Backward Compatibility
#
# When no `crew:` section exists in config, the system uses the built-in
# "software-dev" crew. All existing config keys (`workflow_modes`,
# `specialized_agents`, `effort_levels`, etc.) continue to work and are
# automatically merged into the crew definition.
#
# ## Extending the Default Crew
#
# To add roles to the software-dev crew without replacing it, use `_extend`:
#
# ```yaml
# crew:
#   roles:
#     _extend: true
#     my_custom_linter:
#       prompt_file: custom-linter.md
#       category: planning
#       description: "Domain-specific linting"
# ```
