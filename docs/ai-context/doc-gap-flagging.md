# Documentation Gap Flagging

When any agent notices undocumented or outdated code during its work, it flags the gap for the Technical Writer to address.

## How to Flag

```
workflow_mark_docs_needed(task_id: "<task_id>", files: ["path/to/undocumented-or-outdated.md"])
```

## When to Flag

- Code contradicts existing documentation
- Important base classes, frameworks, or patterns lack documentation
- Conventions are followed but not written down
- Files referenced by multiple agents have no docs

## What Happens Next

The Technical Writer runs after every workflow and checks the `docs_needed` list in the task state. Flagged gaps are prioritized for documentation updates.

The Planner is the primary agent that flags documentation gaps during planning. All other agents (Design Challenger, Reviewer, Skeptic, Implementer, Quality Guard, Security Auditor) can and should also flag gaps they discover during their phases.
