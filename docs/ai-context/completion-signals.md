# Completion Signals (Promise Protocol)

All agents use `<promise>` tags to signal completion status to the orchestrator. These signals are machine-parsed — do not alter the format.

## Signals

| Signal | When to Use |
|--------|-------------|
| `<promise>AGENT_COMPLETE</promise>` | Agent finished successfully. Replace `AGENT` with your role (e.g., `ARCHITECT_COMPLETE`, `DEVELOPER_COMPLETE`). |
| `<promise>BLOCKED: reason</promise>` | Cannot proceed without missing information or human input. Be specific about what's needed. |
| `<promise>ESCALATE: reason</promise>` | Critical issue (security, architecture, compliance) requiring immediate human decision. |

## Agent-Specific Signals

Some agents emit additional signals alongside their completion:

| Agent | Extra Signal | Purpose |
|-------|-------------|---------|
| Design Challenger | `<design_verdict>CONFIRMED\|ALTERNATIVE_PROPOSED\|SIMPLIFICATION_POSSIBLE</design_verdict>` | Design approach verdict |
| Technical Writer | `<promise>DOCS: NO_CHANGES\|MINOR_UPDATES\|NEW_DOCUMENTATION\|MAJOR_REVISION</promise>` | Documentation impact |
| Quality Guard | `<promise>FIXES_APPLIED: N fixes, M flagged</promise>` | Fix summary |
| Implementer | `<promise>STEP_COMPLETE</promise>` | Individual step verified (loop mode) |

## Orchestrator Handling of BLOCKED and ESCALATE

`BLOCKED` and `ESCALATE` are **machine-parsed** by the orchestrator via `crew_parse_agent_output`. When detected, the `agent-done` subcommand routes them before any REVISE logic and does **not** complete the current phase:

| Signal | Orchestrator Action | Result |
|--------|--------------------|-|
| `BLOCKED` | Returns `agent_blocked` action | Orchestrator presents guidance options (provide info / skip phase / abort) |
| `ESCALATE` | Returns `agent_escalated` action | Orchestrator **must** pause and ask the human via `AskUserQuestion` |

**Priority**: If both signals appear in the output, `ESCALATE` takes priority over `BLOCKED`.

## Rules

1. **Always emit exactly one** completion signal (`*_COMPLETE`, `BLOCKED`, or `ESCALATE`) at the end of your output.
2. **BLOCKED** and **ESCALATE** halt the workflow — use only when genuinely stuck.
3. **Be specific** in BLOCKED/ESCALATE reasons — the orchestrator relays them to the human.
