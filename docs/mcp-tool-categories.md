# MCP Tool Categories

The agentic-workflow MCP server exposes 69 tools. They are split into two
categories to clarify which tools are essential for a basic workflow run and
which provide optional, advanced, or analytics capabilities.

## Category definitions

| Category | Purpose |
|----------|---------|
| **Core** | Essential for running a workflow from start to finish. Every mode (standard, reviewed, thorough) depends on these tools. |
| **Extra** | Optional enhancements -- analytics, parallelization, error-pattern matching, worktree management, cost tracking, etc. A workflow completes successfully without them. |

> **Note:** Currently all tools are always loaded regardless of category.
> The categorization exists for documentation, discoverability, and future
> filtering (e.g., loading only core tools in lightweight deployments).

---

## Tool inventory

### Core -- State Management (9 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 1 | `workflow_initialize` | state_tools | Initialize a new workflow task with initial state |
| 2 | `workflow_transition` | state_tools | Validate and execute a phase transition |
| 3 | `workflow_get_state` | state_tools | Read current workflow state for a task |
| 4 | `workflow_complete_phase` | state_tools | Mark the current phase as complete |
| 5 | `workflow_is_complete` | state_tools | Check if all required workflow phases have been completed |
| 6 | `workflow_can_transition` | state_tools | Check if a transition to a given phase is valid (dry-run) |
| 7 | `workflow_can_stop` | state_tools | Check if the workflow can be stopped |
| 8 | `workflow_add_review_issue` | state_tools | Add an issue found during review |
| 9 | `workflow_mark_docs_needed` | state_tools | Flag files that need documentation |

### Core -- Planning & Knowledge (4 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 10 | `workflow_set_kb_inventory` | state_tools | Store knowledge base path and file inventory in state |
| 11 | `workflow_save_discovery` | state_tools | Save a discovery to persistent memory |
| 12 | `workflow_get_discoveries` | state_tools | Retrieve saved discoveries from persistent memory |
| 13 | `workflow_flush_context` | state_tools | Return all discoveries grouped by category (for context compaction) |

### Core -- Implementation Progress (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 14 | `workflow_set_implementation_progress` | state_tools | Set total number of implementation steps and optionally current step |
| 15 | `workflow_complete_step` | state_tools | Mark an implementation step as completed |

### Core -- Concerns & Decisions (4 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 16 | `workflow_add_concern` | state_tools | Add a concern from an agent for cross-agent tracking |
| 17 | `workflow_address_concern` | state_tools | Mark a concern as addressed by a step or action |
| 18 | `workflow_get_concerns` | state_tools | Get all concerns, optionally filtering to unaddressed only |
| 19 | `workflow_add_human_decision` | state_tools | Record a human decision at a checkpoint |

### Core -- Configuration (3 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 20 | `config_get_effective` | config_tools | Get the fully merged effective configuration |
| 21 | `config_get_checkpoint` | config_tools | Check if a specific checkpoint is enabled |
| 22 | `config_get_beads` | config_tools | Get beads configuration with auto-detection |

### Core -- Orchestration (7 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 23 | `crew_parse_args` | orchestration_tools | Parse /crew command arguments into structured format |
| 24 | `crew_init_task` | orchestration_tools | Full task initialization in one call |
| 25 | `crew_get_next_phase` | orchestration_tools | Determine the next workflow action based on state and mode |
| 26 | `crew_parse_agent_output` | orchestration_tools | Extract structured data from agent output and update state |
| 27 | `crew_get_implementation_action` | orchestration_tools | Get the next implementation loop action |
| 28 | `crew_format_completion` | orchestration_tools | Generate workflow completion output |
| 29 | `crew_get_resume_state` | orchestration_tools | Load complete resume context for a task |

**Core total: 29 tools**

---

### Extra -- Mode & Effort (6 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 30 | `workflow_detect_mode` | state_tools | Auto-detect workflow mode from task description |
| 31 | `workflow_set_mode` | state_tools | Set the workflow mode for a task |
| 32 | `workflow_get_mode` | state_tools | Get the current workflow mode |
| 33 | `workflow_is_phase_in_mode` | state_tools | Check if a phase is included in the current mode |
| 34 | `workflow_get_effort_level` | state_tools | Get recommended thinking effort level for an agent |
| 35 | `workflow_get_agent_team_config` | state_tools | Get agent team configuration for a feature |

### Extra -- Model Resilience (5 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 36 | `workflow_record_model_error` | state_tools | Record a model API error for cooldown tracking |
| 37 | `workflow_record_model_success` | state_tools | Record a successful model call, resetting cooldown |
| 38 | `workflow_get_available_model` | state_tools | Get the next available model considering cooldowns |
| 39 | `workflow_get_resilience_status` | state_tools | Get current resilience status for all models |
| 40 | `workflow_clear_model_cooldown` | state_tools | Manually clear a model's cooldown state |

### Extra -- Parallelization (3 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 41 | `workflow_start_parallel_phase` | state_tools | Start parallel execution of multiple phases |
| 42 | `workflow_complete_parallel_phase` | state_tools | Mark a parallel phase as complete and store results |
| 43 | `workflow_merge_parallel_results` | state_tools | Merge results from parallel phase execution |

### Extra -- Assertions (3 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 44 | `workflow_add_assertion` | state_tools | Add an assertion to the workflow for verification |
| 45 | `workflow_verify_assertion` | state_tools | Record the verification result of an assertion |
| 46 | `workflow_get_assertions` | state_tools | Get assertions filtered by step or status |

### Extra -- Error Patterns (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 47 | `workflow_record_error_pattern` | state_tools | Record an error pattern and its solution |
| 48 | `workflow_match_error` | state_tools | Match an error output against known patterns |

### Extra -- Context Management (3 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 49 | `workflow_get_context_usage` | state_tools | Estimate context usage for the task |
| 50 | `workflow_prune_old_outputs` | state_tools | Prune old tool outputs to reduce context pressure |
| 51 | `workflow_search_memories` | state_tools | Search across task memories using keyword matching |

### Extra -- Task Linking (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 52 | `workflow_link_tasks` | state_tools | Link related tasks for context inheritance |
| 53 | `workflow_get_linked_tasks` | state_tools | Get all tasks linked to the specified task |

### Extra -- Concurrent Workflow Guard (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 54 | `workflow_guard_acquire` | state_tools | Acquire an exclusive workflow guard for a task |
| 55 | `workflow_guard_release` | state_tools | Release the workflow guard for a task |

### Extra -- Performance Tracking (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 56 | `workflow_record_concern_outcome` | state_tools | Record the outcome of a concern for agent performance tracking |
| 57 | `workflow_get_agent_performance` | state_tools | Get performance statistics for agents |

### Extra -- Cost Tracking (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 58 | `workflow_record_cost` | state_tools | Record token usage and cost for an agent run |
| 59 | `workflow_get_cost_summary` | state_tools | Get cost summary for a workflow task |

### Extra -- Optional Phases (2 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 60 | `workflow_enable_optional_phase` | state_tools | Enable an optional specialized phase |
| 61 | `workflow_get_optional_phases` | state_tools | Get enabled optional phases for a workflow |

### Extra -- Worktree Support (4 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 62 | `workflow_create_worktree` | state_tools | Record worktree metadata and return git commands |
| 63 | `workflow_get_worktree_info` | state_tools | Get worktree metadata for a task |
| 64 | `workflow_cleanup_worktree` | state_tools | Mark worktree as cleaned and return git commands |
| 65 | `workflow_get_launch_command` | state_tools | Generate platform-specific commands to launch a terminal in a worktree |

### Extra -- Interaction Logging (1 tool)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 66 | `workflow_log_interaction` | state_tools | Append an interaction entry to the task's interactions log |

### Extra -- Orchestration Extras (3 tools)

| # | Tool | Module | Description |
|---|------|--------|-------------|
| 67 | `crew_apply_config_overrides` | orchestration_tools | Merge CLI option flags into config overrides |
| 68 | `crew_detect_optional_agents` | orchestration_tools | Detect which optional specialized agents should be enabled |
| 69 | `crew_jira_transition` | orchestration_tools | Resolve a Jira lifecycle transition for a hook |

**Extra total: 40 tools**

---

## Summary

| Category | Count | Percentage |
|----------|------:|:----------:|
| Core     |    29 |    42%     |
| Extra    |    40 |    58%     |
| **Total**|**69** | **100%**   |

### By module

| Module | Core | Extra | Total |
|--------|-----:|------:|------:|
| state_tools | 19 | 37 | 56 |
| config_tools | 3 | 0 | 3 |
| orchestration_tools | 7 | 3 | 10 |
| **Total** | **29** | **40** | **69** |
