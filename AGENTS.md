# Agent Instructions

Agentic Workflow — persistent AI development workflows with quality gates when you need them.

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.
For project context and architecture, see [docs/ai-context/](./docs/ai-context/).

## Project Overview

A multi-agent orchestration framework for AI-augmented software development. Supports Claude Code, GitHub Copilot, Gemini CLI, and OpenCode from a single source of agent definitions.

**Key components:**
- `agents/` — Agent prompt sources (16 agents, compiled to 4 platforms via `scripts/build-agents.py`)
- `mcp/agentic-workflow-server/` — MCP server (state, config, orchestration tools)
- `config/workflow-config.yaml` — Single configuration file (4-level cascade: global → project → task → CLI); essential settings at top, advanced settings below separator
- `docs/orchestrator-spec.md` — Orchestrator agent specification (formerly `agents/orchestrator.md`)
- `.tasks/` — Per-task state persistence (survives session crashes and context compaction)

## Workflow Modes

| Mode | Agents | When to use | Default models |
|------|--------|-------------|----------------|
| **quick** | implementer | Typos, one-line fixes, trivial changes | Sonnet |
| **standard** | planner → implementer → technical-writer | Routine features, fixes, refactors | Opus (planning) + Sonnet (execution) |
| **thorough** | planner → reviewer → implementer → quality-guard + security-auditor (parallel) → technical-writer | Security, migrations, breaking changes | Opus (planning) + Sonnet (execution) |

Legacy aliases: `micro`/`minimal` → quick, `turbo`/`fast`/`reviewed` → standard, `full` → thorough.

## Development Setup

```bash
# Install MCP server dependencies
pip install -e mcp/agentic-workflow-server

# Build agents for Claude Code
python3 scripts/build-agents.py claude --output ~/.claude

# Run tests
python3 -m pytest mcp/agentic-workflow-server/tests/ -v
```

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --status in_progress  # Claim work
bd close <id>         # Complete work
bd sync               # Sync with git
```

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

