# Task: Refactor

## Description

Refactor [component/module]: [what and why].

## Goals

- [ ] [Goal 1 — e.g., reduce duplication]
- [ ] [Goal 2 — e.g., improve testability]
- [ ] [Goal 3 — e.g., clarify naming]

## Success Criteria

- All existing tests pass without modification
- No behavior changes (refactor only)
- Code is measurably simpler (fewer lines, clearer structure, or better naming)

## Constraints

- Zero behavior changes — if a bug is found, fix it in a separate commit
- Keep the diff reviewable (split into logical commits if large)
- Do NOT change public APIs unless explicitly agreed

## Suggested Mode

```
/crew --mode standard "Refactor [component]"
```

For cross-module refactors:
```
/crew --mode reviewed "Refactor [component] across [modules]"
```
