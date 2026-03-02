# Task: Bug Fix

## Description

Fix the reported bug: [describe the bug here].

## Approach

1. Reproduce the bug with a failing test
2. Identify root cause
3. Implement fix
4. Verify fix doesn't break existing tests
5. Add regression test if none exists

## Success Criteria

- Bug is no longer reproducible
- All existing tests pass
- At least one test covers the fixed scenario
- No new warnings introduced

## Constraints

- Minimal changes — fix the bug, don't refactor surrounding code
- If the fix requires a broader refactor, document it as a follow-up

## Suggested Mode

```
/crew --mode standard "Fix: [describe bug]"
```
