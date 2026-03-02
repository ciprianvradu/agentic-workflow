# Task: New API Endpoint

## Description

Add API endpoint: `[METHOD] /api/[path]` — [what it does].

## Requirements

- [ ] Route handler with request validation
- [ ] Business logic (service layer)
- [ ] Response format (JSON schema)
- [ ] Error handling (400, 401, 404, 500)
- [ ] Authentication/authorization if required
- [ ] Tests (unit + integration)

## API Contract

```
[METHOD] /api/[path]

Request:
  Headers: Authorization: Bearer <token>
  Body: { "field": "value" }

Response 200:
  { "data": { ... } }

Response 400:
  { "error": "Validation failed", "details": [...] }
```

## Success Criteria

- Endpoint responds correctly for valid requests
- Returns appropriate error codes for invalid input
- Auth is enforced if required
- Tests cover happy path, validation errors, and auth failures
- No existing endpoints broken

## Suggested Mode

```
/crew --mode reviewed "Add endpoint: [METHOD] /api/[path]"
```
