# Task: Database Migration

## Description

Create migration for: [describe schema change].

## Requirements

- [ ] Migration script (up and down)
- [ ] Data migration if existing rows need updating
- [ ] Update affected models/types
- [ ] Update affected queries
- [ ] Update tests for new schema

## Success Criteria

- Migration runs cleanly on empty database
- Migration runs cleanly on database with existing data
- Rollback (down) migration works
- All existing tests pass with new schema
- New tests cover migration edge cases

## Risks

- Data loss on rollback — document any irreversible changes
- Performance impact of migration on large tables
- Foreign key constraints that may block migration

## Constraints

- Migration must be backward-compatible if possible
- Lock time on production tables should be minimized
- Test with realistic data volumes

## Suggested Mode

```
/crew --mode thorough "Database migration: [describe change]"
```
